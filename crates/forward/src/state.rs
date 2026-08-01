//! Общее состояние форвардинга, клонируется в каждый axum-хендлер.

use crate::affinity::AffinityStore;
use crate::billing::AsyncBilling;
use crate::breaker::Breaker;
use crate::codex::CodexGateway;
use crate::config::ProxyConfig;
use crate::gemini::GeminiGateway;
use crate::metrics::Metrics;
use crate::pricing::PricingShadowRuntime;
use crate::upstream::Clients;
use pool::Pool;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

/// Provider surface selected once at process startup. `Combined` is retained only as the rollout
/// bridge for installations whose systemd unit predates provider-specific services.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderMode {
    Combined,
    Anthropic,
    OpenAi,
    Gemini,
}

impl ProviderMode {
    pub fn serves_anthropic(self) -> bool {
        matches!(self, Self::Combined | Self::Anthropic)
    }

    pub fn serves_openai(self) -> bool {
        matches!(self, Self::Combined | Self::OpenAi)
    }

    pub fn serves_gemini(self) -> bool {
        matches!(self, Self::Gemini)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Combined => "combined",
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub provider: ProviderMode,
    pub cfg: Arc<ProxyConfig>,
    /// Authority selector for read-only admin overviews. Secrets are never rendered in logs/responses.
    pub authority: Arc<registry::authority::AuthorityConfig>,
    /// Local data directory path remains the home of non-authoritative metrics.db.
    pub data_db_path: Arc<String>,
    pub pool: Arc<Pool>,
    /// Ephemeral cache-lineage affinity. Local L1 is always available; Redis, when configured,
    /// shares bindings across engine slots. It never authorizes money or capacity.
    pub affinity: Arc<AffinityStore>,
    pub clients: Arc<Clients>,
    /// Optional OpenAI-compatible text provider backed by the native Codex profile pool.
    /// It owns no OAuth material; the child reads the dedicated authenticated `CODEX_HOME`.
    pub codex: Option<Arc<CodexGateway>>,
    /// Optional native Gemini surface backed by encrypted paid Code Assist OAuth profiles.
    pub gemini: Option<Arc<GeminiGateway>>,
    /// Биллинг клиентов (async DB-актор — синхронный SQLite не блокирует воркеры).
    /// `None` → биллинг выключен (только env-ключи/localhost).
    pub billing: Option<Arc<AsyncBilling>>,
    /// Default-off evaluation-time pricing shadow. Its producer uses one non-blocking enqueue;
    /// policy reads and persistence remain on its bounded background worker.
    pub pricing_shadow: Option<Arc<PricingShadowRuntime>>,
    /// Compile-fixed canonical capability manifest used by strict admission and stamped into the
    /// PostgreSQL owner lease. It is runtime-owned and never derived from request data.
    pub pricing_manifest: Arc<registry::pricing::PricingRuntimeManifestEvidence>,
    /// Live dependency health. PostgreSQL heartbeat toggles this and request admission fails closed.
    pub authority_ready: Arc<AtomicBool>,
    /// Глобальный circuit breaker апстрима (анти-амплификация при брауноуте api.anthropic.com).
    pub breaker: Arc<Breaker>,
    /// Счётчики форвардинга для `/metrics`.
    pub metrics: Arc<Metrics>,
    /// Разбудить backend quota-поллер вне расписания. Forward зовёт после `pool.request_probe` для
    /// clean auth-вердикта 401/403 и post-turn calibration pairing. `None` → поллер выключен
    /// (`CLAUDE_API_POLL=0`), тогда probe-по-требованию недоступен.
    pub probe_poke: Option<Arc<tokio::sync::Notify>>,
}

/// Unlimited request-task registration with an exact graceful-shutdown barrier.
///
/// Unlike a semaphore, this tracker never waits and never caps live work. Closing it atomically
/// rejects only tasks racing process retirement, while already registered tasks remain counted
/// until their RAII guards are dropped after delivery, persistence and settlement.
#[derive(Default)]
pub(crate) struct ActiveTaskTracker {
    state: AtomicUsize,
    idle: Notify,
}

const TASK_TRACKER_CLOSED: usize = 1usize << (usize::BITS - 1);
const TASK_TRACKER_COUNT: usize = TASK_TRACKER_CLOSED - 1;

pub(crate) struct ActiveTaskGuard {
    tracker: Arc<ActiveTaskTracker>,
}

impl ActiveTaskTracker {
    pub(crate) fn track(self: &Arc<Self>) -> Option<ActiveTaskGuard> {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            if state & TASK_TRACKER_CLOSED != 0 {
                return None;
            }
            assert_ne!(
                state & TASK_TRACKER_COUNT,
                TASK_TRACKER_COUNT,
                "active task tracker counter overflow"
            );
            match self.state.compare_exchange_weak(
                state,
                state + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(observed) => state = observed,
            }
        }
        Some(ActiveTaskGuard {
            tracker: self.clone(),
        })
    }

    pub(crate) fn close(&self) {
        self.state.fetch_or(TASK_TRACKER_CLOSED, Ordering::AcqRel);
        self.idle.notify_waiters();
    }

    pub(crate) async fn wait_idle(&self) {
        loop {
            let notified = self.idle.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.state.load(Ordering::Acquire) & TASK_TRACKER_COUNT == 0 {
                return;
            }
            notified.await;
        }
    }

    #[cfg(test)]
    fn active(&self) -> usize {
        self.state.load(Ordering::Acquire) & TASK_TRACKER_COUNT
    }

    fn release(&self) {
        let previous = self.state.fetch_sub(1, Ordering::AcqRel);
        debug_assert_ne!(previous & TASK_TRACKER_COUNT, 0);
        if previous & TASK_TRACKER_COUNT == 1 {
            self.idle.notify_waiters();
        }
    }
}

impl Drop for ActiveTaskGuard {
    fn drop(&mut self) {
        self.tracker.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn task_registration_never_waits_or_caps_live_work() {
        let tracker = Arc::new(ActiveTaskTracker::default());
        let guards = (0..10_000)
            .map(|_| tracker.track().expect("open tracker admits immediately"))
            .collect::<Vec<_>>();
        assert_eq!(tracker.active(), guards.len());
        drop(guards);
        tokio::time::timeout(Duration::from_millis(50), tracker.wait_idle())
            .await
            .expect("all guards released without a queued admission");
    }

    #[tokio::test]
    async fn closing_rejects_only_new_tasks_and_drains_existing_guards() {
        let tracker = Arc::new(ActiveTaskTracker::default());
        let guard = tracker.track().expect("open tracker");
        tracker.close();
        assert!(tracker.track().is_none());

        let waiting = tokio::time::timeout(Duration::from_millis(20), tracker.wait_idle()).await;
        assert!(waiting.is_err(), "shutdown waits for already admitted work");
        drop(guard);
        tokio::time::timeout(Duration::from_millis(50), tracker.wait_idle())
            .await
            .expect("dropping the final guard wakes shutdown");
    }
}
