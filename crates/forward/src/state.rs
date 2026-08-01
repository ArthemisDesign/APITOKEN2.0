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
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

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
    /// Bounded heavy-processing envelope: request admission itself is unbounded and waits
    /// asynchronously when every permit is busy. The permit lives through EOF/error/Drop,
    /// including a long SSE stream, so accepted waiters do not multiply large response buffers.
    /// Connection-level slowloris/idle-таймауты —
    /// это reverse-proxy (Caddy/nginx/CF, Фаза 3 вместе с TLS), здесь бьём по стоимости обработки.
    pub concurrency: Arc<tokio::sync::Semaphore>,
    /// Разбудить liveness-поллер вне расписания (forward зовёт после `pool.request_probe`, когда
    /// подписка отдала 401/403 → надо СРАЗУ рассудить чистым probe, мёртв ли токен). `None` → поллер
    /// выключен (`CLAUDE_API_POLL=0`), тогда probe-по-требованию просто не нужен.
    pub probe_poke: Option<Arc<tokio::sync::Notify>>,
}

/// Enter the bounded expensive-processing envelope without rejecting a concurrent request. The
/// semaphore wait owns no customer reservation, reads no request body and is canceled automatically
/// when Axum drops a disconnected request future.
pub(crate) async fn acquire_processing_permit(
    app: &AppState,
) -> Result<tokio::sync::OwnedSemaphorePermit, ()> {
    acquire_processing_permit_from(app.concurrency.clone(), &app.metrics).await
}

async fn acquire_processing_permit_from(
    semaphore: Arc<tokio::sync::Semaphore>,
    metrics: &Metrics,
) -> Result<tokio::sync::OwnedSemaphorePermit, ()> {
    match semaphore.clone().try_acquire_owned() {
        Ok(permit) => Ok(permit),
        Err(tokio::sync::TryAcquireError::NoPermits) => {
            let wait = metrics.admission_wait();
            let permit = semaphore.acquire_owned().await.map_err(|_| ())?;
            wait.complete();
            Ok(permit)
        }
        Err(tokio::sync::TryAcquireError::Closed) => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn processing_envelope_queues_and_resumes_instead_of_rejecting() {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
        let occupied = semaphore.clone().acquire_owned().await.unwrap();
        let metrics = Arc::new(Metrics::new());
        let waiter_semaphore = semaphore.clone();
        let waiter_metrics = metrics.clone();
        let waiter = tokio::spawn(async move {
            acquire_processing_permit_from(waiter_semaphore, &waiter_metrics).await
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiter.is_finished());
        assert_eq!(Metrics::get(&metrics.admission_waiters), 1);
        drop(occupied);
        let permit = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("released envelope wakes one waiter")
            .expect("wait task")
            .expect("processing permit");
        assert_eq!(Metrics::get(&metrics.admission_waiters), 0);
        assert_eq!(Metrics::get(&metrics.admission_waits), 1);
        drop(permit);
    }

    #[tokio::test]
    async fn processing_wait_cancellation_does_not_leak_a_waiter_or_permit() {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
        let occupied = semaphore.clone().acquire_owned().await.unwrap();
        let metrics = Arc::new(Metrics::new());
        let waiter_semaphore = semaphore.clone();
        let waiter_metrics = metrics.clone();
        let waiter = tokio::spawn(async move {
            acquire_processing_permit_from(waiter_semaphore, &waiter_metrics).await
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        assert_eq!(Metrics::get(&metrics.admission_waiters), 0);
        assert_eq!(Metrics::get(&metrics.admission_wait_canceled), 1);
        drop(occupied);
        assert!(semaphore.try_acquire().is_ok());
    }
}
