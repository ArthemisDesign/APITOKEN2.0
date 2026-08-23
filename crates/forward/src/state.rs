//! Общее состояние форвардинга, клонируется в каждый axum-хендлер.

use crate::affinity::AffinityStore;
use crate::billing::AsyncBilling;
use crate::breaker::Breaker;
use crate::codex::CodexGateway;
use crate::config::ProxyConfig;
use crate::gemini::GeminiGateway;
use crate::glm::GlmGateway;
use crate::kimi::KimiGateway;
use crate::metrics::Metrics;
use crate::upstream::Clients;
use pool::Pool;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

pub struct BodyStorage {
    pub limits: api_limits::BodyLimits,
    pub storage: bounded_body::Budget,
    pub memory: bounded_body::Budget,
    pub spool: bounded_body::PrivateSpoolFactory,
}

impl BodyStorage {
    pub fn new(
        limits: api_limits::BodyLimits,
        spool_root: impl AsRef<std::path::Path>,
    ) -> Result<Self, bounded_body::StorageError> {
        let storage = bounded_body::Budget::new(
            limits.spool_budget,
            api_limits::ByteLimit::from_bytes(api_limits::MIB),
        )
        .map_err(|_| bounded_body::StorageError::InvalidConfig)?;
        let memory = bounded_body::Budget::new(
            limits.memory_budget,
            api_limits::ByteLimit::from_bytes(api_limits::MIB),
        )
        .map_err(|_| bounded_body::StorageError::InvalidConfig)?;
        let spool = bounded_body::PrivateSpoolFactory::new(spool_root)?;
        Ok(Self {
            limits,
            storage,
            memory,
            spool,
        })
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct AdminChange {
    pub source: &'static str,
    pub resources: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub resync: bool,
}

impl AdminChange {
    pub fn engine(resources: &[&'static str], reason: &'static str) -> Self {
        Self {
            source: "engine",
            resources: resources.to_vec(),
            reason: Some(reason),
            resync: false,
        }
    }

    pub fn engine_resync() -> Self {
        Self {
            source: "engine",
            resources: vec![
                "/overview",
                "/capacity",
                "/subs",
                "/spend-stats",
                "/fleet-history",
                "/settlement-health",
                "/codex-subs",
                "/gemini-subs",
                "/kimi-subs",
                "/glm-subs",
            ],
            reason: None,
            resync: true,
        }
    }
}

/// Provider surface selected once at process startup. `Combined` is retained only as the rollout
/// bridge for installations whose systemd unit predates provider-specific services.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderMode {
    Combined,
    Anthropic,
    OpenAi,
    Gemini,
    Kimi,
    /// The dedicated backend-only Tripo3D task-media plane (default-off; composes only under
    /// `CLAUDE_API_TRIPO3D_ENABLED=1`). It never shares the Anthropic Messages surface.
    Tripo3d,
    /// The dedicated backend-only Suno subscription session-pool plane (default-off; composes
    /// only under `CLAUDE_API_SUNO_ENABLED=1`). It never shares the Anthropic Messages surface.
    Suno,
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

    /// This process composes the KIMI gateway. The Anthropic-serving modes keep the embedded
    /// backend for dev/tests; `Kimi` is the dedicated default-off plane that serves exact KIMI
    /// aliases only and never mounts the Claude pool.
    pub fn serves_kimi(self) -> bool {
        matches!(self, Self::Combined | Self::Anthropic | Self::Kimi)
    }

    /// The dedicated Tripo3D plane. Unlike KIMI/GLM there is no embedded form: a task-based
    /// media API cannot ride the Anthropic Messages surface, so only `Tripo3d` composes it.
    pub fn serves_tripo3d(self) -> bool {
        matches!(self, Self::Tripo3d)
    }

    /// The dedicated Suno plane. Same shape as Tripo3D: a task-based media API cannot ride the
    /// Anthropic Messages surface, so only `Suno` composes it.
    pub fn serves_suno(self) -> bool {
        matches!(self, Self::Suno)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Combined => "combined",
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
            Self::Kimi => "kimi",
            Self::Tripo3d => "tripo3d",
            Self::Suno => "suno",
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
    /// Process-local bounded request-body authorities. Native Anthropic is the first consumer;
    /// other provider paths retain their current readers until their route-specific integration.
    pub body_storage: Option<Arc<BodyStorage>>,
    /// Optional OpenAI-compatible text provider backed by the native Codex profile pool.
    /// It owns no OAuth material; the child reads the dedicated authenticated `CODEX_HOME`.
    pub codex: Option<Arc<CodexGateway>>,
    /// Optional native Gemini surface backed by encrypted paid Code Assist OAuth profiles.
    pub gemini: Option<Arc<GeminiGateway>>,
    pub gemini_batch: Option<Arc<crate::gemini::GeminiBatchPublicFacade>>,
    /// Optional scheduler runtime behind the public facade. Server-owned observability reads its
    /// fleet aggregate only; customer/job/profile identities never enter metrics or admin JSON.
    pub gemini_batch_runtime: Option<Arc<crate::gemini::GeminiBatchRuntime>>,
    /// Optional backend-only KIMI subscription pool. Anthropic-serving modes embed it to dispatch
    /// exact KIMI aliases inside the Messages plane; the dedicated `Kimi` mode serves only those
    /// aliases on `/v1/messages` plus the Anthropic-plane Chat/Responses adapters. It has no public
    /// hostname; discovery is the internal catalog producer, not a public `/v1/models`.
    pub kimi: Option<Arc<KimiGateway>>,
    /// Optional backend-only GLM (Z.ai Coding Plan) subscription pool. Same shape as KIMI:
    /// exact reviewed GLM aliases dispatch inside the Anthropic Messages plane; the credential
    /// is a static API key with a dual-ledger (API nanoUSD + native microcredits) calibration.
    pub glm: Option<Arc<GlmGateway>>,
    /// Optional backend-only Tripo3D (VAST) prepaid API pool on the dedicated
    /// `ProviderMode::Tripo3d` plane: a task-based media surface (create → poll → artifact
    /// download → exact settle from `consumed_credit`), never the Anthropic Messages wire.
    pub tripo3d: Option<Arc<crate::tripo3d::Tripo3dGateway>>,
    /// Optional backend-only Suno subscription session-pool on the dedicated
    /// `ProviderMode::Suno` plane: a task-based media surface (create → poll → artifact
    /// download → settle from the attributed credit delta, else the documented conservative
    /// reserve), never the Anthropic Messages wire.
    pub suno: Option<Arc<crate::suno::SunoGateway>>,
    /// Биллинг клиентов (async DB-актор — синхронный SQLite не блокирует воркеры).
    /// `None` → биллинг выключен (только env-ключи/localhost).
    pub billing: Option<Arc<AsyncBilling>>,
    /// Default-off evaluation-time pricing shadow. Its producer uses one non-blocking enqueue;
    /// policy reads and persistence remain on its bounded background worker.
    /// Compile-fixed canonical capability manifest used by strict admission and stamped into the
    /// PostgreSQL owner lease. It is runtime-owned and never derived from request data.
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
    /// Process-local invalidations for the admin console. It carries no data or secrets; browser
    /// consumers refetch only mounted resources after a real state transition.
    pub admin_changes: tokio::sync::broadcast::Sender<AdminChange>,
}

impl AppState {
    pub fn install_body_storage(&mut self, storage: Arc<BodyStorage>) {
        self.body_storage = Some(storage);
    }

    pub(crate) fn body_storage(&self) -> Result<&Arc<BodyStorage>, bounded_body::StorageError> {
        self.body_storage
            .as_ref()
            .ok_or(bounded_body::StorageError::InvalidConfig)
    }
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
    pub(crate) fn active(&self) -> usize {
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
