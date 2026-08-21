//! Счётчики форвардинга для наблюдаемости (`/metrics`). Дёшевые атомики, монотонные с запуска.

use std::sync::atomic::{AtomicU64, Ordering};














#[derive(Default)]
pub struct Metrics {
    pub requests: AtomicU64,     // всего обслуженных запросов (после авторизации)
    /// Customer requests this process is serving right now, counted from the moment the request
    /// enters the router until the last byte of its response body is gone.
    ///
    /// This is the only number that answers "may this slot be stopped safely". Provider-side
    /// in-flight gauges free their lease as soon as the upstream is done, while the request still
    /// owns money: the settlement is written after the body ends. A deploy that stopped a slot on
    /// the provider gauge would cut exactly that tail — the window in which reservations were
    /// abandoned in `delivering` and later charged the full preflight hold.
    pub active_requests: AtomicU64,
    pub upstream_429: AtomicU64, // ответов апстрима 429 (квота подписки)
    pub upstream_auth: AtomicU64, // request-path upstream 401/403; credential death requires clean probes
    pub upstream_5xx: AtomicU64, // backend-fault (5xx/408/409/425)
    pub breaker_rejects: AtomicU64, // отбито разомкнутым circuit breaker
    pub exhausted: AtomicU64,    // исчерпание пула (все за лимитом) → 429+Retry-After
    /// Streams that died after the first public byte and were terminated with the protocol's own
    /// `event: error` frame.
    ///
    /// This is the one customer-visible failure the terminal-error audit cannot see: the response
    /// already carried `200`, so nothing downstream counts it. A customer reports "the connection
    /// keeps dropping" and the operator finds a clean error log. Split by cause, because the client
    /// sees the same frame either way and only these counters can tell a stalled upstream from a
    /// proxy that cut the tunnel mid-answer.
    pub stream_cut_timeout: AtomicU64,
    pub stream_cut_transport: AtomicU64,
    /// Selections rotated to the next candidate by the advisory cross-slot cooling hint. The
    /// counter proves the hint actually saves 429s; a permanently zero value alongside a
    /// configured Redis means publishes never arrive.
    pub cooling_hint_skips: AtomicU64,
    pub auth_failures: AtomicU64, // неудачных авторизаций (спайк = брутфорс/скан управляющих ключей)
    /// Customer-visible 402 responses returned even though the authoritative account balance was
    /// still positive when the terminal response was audited. This is process-wide and inherits
    /// the fixed provider label from the scrape target; it never carries account or key identity.
    pub positive_balance_402: AtomicU64,
    /// Last-resort ClaudeStore transport. These counters are intentionally provider-wide and carry
    /// no model, account, key, request, upstream-error, or customer labels. The Prometheus scrape
    /// target's provider label separates Anthropic Messages from OpenAI Responses observations.
    pub claudestore_fallback_attempts: AtomicU64,
    pub claudestore_fallback_successes: AtomicU64,
    pub claudestore_fallback_failures: AtomicU64,
    /// KIMI request outcomes. The plane had no request counter at all, so the error share — the
    /// first number anyone asks for during an incident — was uncomputable; the Gemini investigation
    /// of 2026-08-06 lost a day to exactly that blindness. Deliberately three fixed counters with
    /// no model, account, profile or upstream-error labels.
    pub kimi_requests: AtomicU64,
    /// Non-2xx responses the plane actually returned to a caller.
    pub kimi_failures: AtomicU64,
    /// The subset of failures that are our own capacity refusal rather than a provider verdict.
    pub kimi_capacity_exhausted: AtomicU64,
    /// Fleet-wide sum of KIMI quota fraction units that moved without any recorded durable spend
    /// (the calibration estimator's `unattributed_fraction_units`, refreshed from the durable
    /// report each quota cycle). Gauge, not a counter: it is re-read from authority, so a rebuild
    /// or an estimator version change can lower it. Fixed cardinality, no profile/plan labels.
    pub kimi_calibration_unattributed_units: AtomicU64,
    /// Tripo3D plane request outcomes — the same three fixed counters as KIMI, with no task,
    /// account, profile or upstream-error labels.
    pub tripo3d_requests: AtomicU64,
    /// Non-2xx responses the plane actually returned to a caller.
    pub tripo3d_failures: AtomicU64,
    /// The subset of failures that are our own capacity refusal rather than a provider verdict.
    pub tripo3d_capacity_exhausted: AtomicU64,
    /// Suno plane request outcomes — the same three fixed counters as Tripo3D, with no
    /// operation, account, profile or upstream-error labels.
    pub suno_requests: AtomicU64,
    /// Non-2xx responses the plane actually returned to a caller.
    pub suno_failures: AtomicU64,
    /// The subset of failures that are our own capacity refusal rather than a provider verdict.
    pub suno_capacity_exhausted: AtomicU64,
    /// Successful Gemini generations that ended without authoritative usage. Metered non-stream
    /// delivery is withheld; a stream already delivered settles its conservative hold.
    pub gemini_usage_missing: AtomicU64,
    /// Gemini-only low-cardinality failure classes. They intentionally carry no profile, request,
    /// proxy, project or upstream-error labels.
    pub gemini_transport_failures: AtomicU64,
    pub gemini_backend_failures: AtomicU64,
    pub gemini_malformed_responses: AtomicU64,
    pub gemini_stream_start_failures: AtomicU64,
    /// Materialized text-body admission outcomes. Reasons are compile-fixed; identity never
    /// becomes a label. The Prometheus scrape target already supplies `provider`.
    pub body_admission_oversized: AtomicU64,
    pub body_admission_overload: AtomicU64,
    pub body_admission_content_encoding: AtomicU64,
    /// Exact `not_started` proofs actually returned by each fixed provider plane. The array is
    /// compile-bounded to Anthropic/OpenAI/Gemini and never carries request or credential labels.
    execution_not_started: [AtomicU64; 3],
}



impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }
    #[inline]
    pub fn inc(c: &AtomicU64) {
        c.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get(c: &AtomicU64) -> u64 {
        c.load(Ordering::Relaxed)
    }

    /// Store a gauge-shaped value re-read from an authority (unlike `inc` counters, such series
    /// legitimately move both ways between scrapes).
    #[inline]
    pub fn set(c: &AtomicU64, value: u64) {
        c.store(value, Ordering::Relaxed);
    }

    /// Saturating decrement for the gauge-shaped counters. A gauge that underflows to `u64::MAX`
    /// would read as "this slot is busy forever" and block every later deploy, so the floor is
    /// clamped rather than wrapped.
    #[inline]
    pub fn dec(c: &AtomicU64) {
        let _ = c.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(value.saturating_sub(1))
        });
    }

    pub fn execution_not_started(&self, plane: crate::ProviderMode) {
        if let Some(index) = execution_plane_index(plane) {
            Self::inc(&self.execution_not_started[index]);
        }
    }

    pub fn execution_not_started_count(&self, plane: crate::ProviderMode) -> u64 {
        execution_plane_index(plane)
            .map(|index| Self::get(&self.execution_not_started[index]))
            .unwrap_or(0)
    }

    pub fn render_body_admission(&self) -> String {
        format!(
            "# HELP claude_api_body_admission_rejections_total Materialized text request bodies rejected before provider dispatch.\n\
             # TYPE claude_api_body_admission_rejections_total counter\n\
             claude_api_body_admission_rejections_total{{reason=\"oversized\"}} {}\n\
             claude_api_body_admission_rejections_total{{reason=\"admission_overload\"}} {}\n\
             claude_api_body_admission_rejections_total{{reason=\"content_encoding\"}} {}\n",
            Self::get(&self.body_admission_oversized),
            Self::get(&self.body_admission_overload),
            Self::get(&self.body_admission_content_encoding),
        )
    }
}

/// Process-wide Gemini binary IPC telemetry. The helper lives in this process, and scrape already
/// labels the target `provider="gemini"`; other planes export zeros.
pub struct GeminiIpc {
    pub request_control_bytes: AtomicU64,
    pub request_data_bytes: AtomicU64,
    pub response_control_bytes: AtomicU64,
    pub response_data_bytes: AtomicU64,
    pub active_requests: AtomicU64,
    pub protocol_failures: [AtomicU64; 4],
}

impl GeminiIpc {
    pub const REASON_PROTOCOL: usize = 0;
    pub const REASON_SPAWN: usize = 1;
    pub const REASON_CLOSED: usize = 2;
    pub const REASON_BODY_TOO_LARGE: usize = 3;

    pub fn global() -> &'static Self {
        static IPC: GeminiIpc = GeminiIpc {
            request_control_bytes: AtomicU64::new(0),
            request_data_bytes: AtomicU64::new(0),
            response_control_bytes: AtomicU64::new(0),
            response_data_bytes: AtomicU64::new(0),
            active_requests: AtomicU64::new(0),
            protocol_failures: [const { AtomicU64::new(0) }; 4],
        };
        &IPC
    }

    pub fn record_failure(&self, reason: usize) {
        if let Some(counter) = self.protocol_failures.get(reason) {
            Metrics::inc(counter);
        }
    }

    pub fn record_pipe_bytes(&self, request: bool, control: bool, n: u64) {
        let field = match (request, control) {
            (true, true) => &self.request_control_bytes,
            (true, false) => &self.request_data_bytes,
            (false, true) => &self.response_control_bytes,
            (false, false) => &self.response_data_bytes,
        };
        field.fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc_active(&self) {
        Metrics::inc(&self.active_requests);
    }

    pub fn dec_active(&self) {
        Metrics::dec(&self.active_requests);
    }

    pub fn dec_active_by(&self, n: u64) {
        if n == 0 {
            return;
        }
        let _ = self.active_requests.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |value| Some(value.saturating_sub(n)),
        );
    }

    pub fn render(&self) -> String {
        format!(
            "# HELP claude_api_gemini_ipc_bytes_total Bytes written or read on the Gemini binary IPC pipe.\n\
             # TYPE claude_api_gemini_ipc_bytes_total counter\n\
             claude_api_gemini_ipc_bytes_total{{direction=\"request\",kind=\"control\"}} {}\n\
             claude_api_gemini_ipc_bytes_total{{direction=\"request\",kind=\"data\"}} {}\n\
             claude_api_gemini_ipc_bytes_total{{direction=\"response\",kind=\"control\"}} {}\n\
             claude_api_gemini_ipc_bytes_total{{direction=\"response\",kind=\"data\"}} {}\n\
             # HELP claude_api_gemini_ipc_active_requests In-flight Gemini helper requests.\n\
             # TYPE claude_api_gemini_ipc_active_requests gauge\n\
             claude_api_gemini_ipc_active_requests {}\n\
             # HELP claude_api_gemini_ipc_protocol_failures_total Gemini helper framing or lifecycle failures.\n\
             # TYPE claude_api_gemini_ipc_protocol_failures_total counter\n\
             claude_api_gemini_ipc_protocol_failures_total{{reason=\"protocol\"}} {}\n\
             claude_api_gemini_ipc_protocol_failures_total{{reason=\"spawn\"}} {}\n\
             claude_api_gemini_ipc_protocol_failures_total{{reason=\"closed\"}} {}\n\
             claude_api_gemini_ipc_protocol_failures_total{{reason=\"body_too_large\"}} {}\n",
            Metrics::get(&self.request_control_bytes),
            Metrics::get(&self.request_data_bytes),
            Metrics::get(&self.response_control_bytes),
            Metrics::get(&self.response_data_bytes),
            Metrics::get(&self.active_requests),
            Metrics::get(&self.protocol_failures[Self::REASON_PROTOCOL]),
            Metrics::get(&self.protocol_failures[Self::REASON_SPAWN]),
            Metrics::get(&self.protocol_failures[Self::REASON_CLOSED]),
            Metrics::get(&self.protocol_failures[Self::REASON_BODY_TOO_LARGE]),
        )
    }
}

fn execution_plane_index(plane: crate::ProviderMode) -> Option<usize> {
    match plane {
        crate::ProviderMode::Anthropic => Some(0),
        crate::ProviderMode::OpenAi => Some(1),
        crate::ProviderMode::Gemini => Some(2),
        // The backend-only KIMI plane has no public hostname feeding the Caddy no-upstream marker,
        // so its synthetic refusals stay out of the three public plane series, exactly like the
        // `Combined` bridge. The dedicated Tripo3D/Suno planes are not an Anthropic/OpenAI/Gemini
        // wire at all, so they likewise own no execution-not-started series here.
        crate::ProviderMode::Combined
        | crate::ProviderMode::Kimi
        | crate::ProviderMode::Tripo3d
        | crate::ProviderMode::Suno => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn execution_not_started_counters_are_fixed_to_provider_planes() {
        let metrics = Metrics::new();
        metrics.execution_not_started(crate::ProviderMode::Anthropic);
        metrics.execution_not_started(crate::ProviderMode::OpenAi);
        metrics.execution_not_started(crate::ProviderMode::OpenAi);
        metrics.execution_not_started(crate::ProviderMode::Gemini);
        metrics.execution_not_started(crate::ProviderMode::Combined);

        assert_eq!(
            metrics.execution_not_started_count(crate::ProviderMode::Anthropic),
            1
        );
        assert_eq!(
            metrics.execution_not_started_count(crate::ProviderMode::OpenAi),
            2
        );
        assert_eq!(
            metrics.execution_not_started_count(crate::ProviderMode::Gemini),
            1
        );
        assert_eq!(
            metrics.execution_not_started_count(crate::ProviderMode::Combined),
            0
        );
    }

    #[test]
    fn body_admission_render_uses_fixed_reasons() {
        let metrics = Metrics::new();
        Metrics::inc(&metrics.body_admission_oversized);
        Metrics::inc(&metrics.body_admission_content_encoding);
        let body = metrics.render_body_admission();
        assert!(body.contains("reason=\"oversized\"} 1"));
        assert!(body.contains("reason=\"admission_overload\"} 0"));
        assert!(body.contains("reason=\"content_encoding\"} 1"));
        assert!(!body.contains("provider="));
    }

    #[test]
    fn gemini_ipc_render_is_fixed_cardinality() {
        let body = GeminiIpc::global().render();
        assert!(body.contains("direction=\"request\",kind=\"control\""));
        assert!(body.contains("direction=\"request\",kind=\"data\""));
        assert!(body.contains("direction=\"response\",kind=\"control\""));
        assert!(body.contains("direction=\"response\",kind=\"data\""));
        assert!(body.contains("claude_api_gemini_ipc_active_requests"));
        assert!(body.contains("reason=\"protocol\""));
        assert!(body.contains("reason=\"spawn\""));
        assert!(body.contains("reason=\"closed\""));
        assert!(body.contains("reason=\"body_too_large\""));
        assert!(!body.contains("provider="));
    }
}
