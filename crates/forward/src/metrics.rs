//! Счётчики форвардинга для наблюдаемости (`/metrics`). Дёшевые атомики, монотонные с запуска.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::pricing::{
};
use registry::pricing::{
};














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
    pub upstream_auth: AtomicU64, // 401/403 (мёртвый токен → карантин)
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
    /// Successful Gemini generations that ended without authoritative usage. Metered non-stream
    /// delivery is withheld; a stream already delivered settles its conservative hold.
    pub gemini_usage_missing: AtomicU64,
    /// Gemini-only low-cardinality failure classes. They intentionally carry no profile, request,
    /// proxy, project or upstream-error labels.
    pub gemini_transport_failures: AtomicU64,
    pub gemini_backend_failures: AtomicU64,
    pub gemini_malformed_responses: AtomicU64,
    pub gemini_stream_start_failures: AtomicU64,
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









































}

fn execution_plane_index(plane: crate::ProviderMode) -> Option<usize> {
    match plane {
        crate::ProviderMode::Anthropic => Some(0),
        crate::ProviderMode::OpenAi => Some(1),
        crate::ProviderMode::Gemini => Some(2),
        // The backend-only KIMI plane has no public hostname feeding the Caddy no-upstream marker,
        // so its synthetic refusals stay out of the three public plane series, exactly like the
        // `Combined` bridge.
        crate::ProviderMode::Combined | crate::ProviderMode::Kimi => None,
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
    fn strict_resolution_rejections_keep_operational_categories_distinct() {
        use crate::pricing::PricingResolutionLineage;

        let cases = [
            (
                PricingResolutionRejection::NoPolicyBinding,
                StrictPricingRejectionReason::MissingPolicy,
            ),
            (
                PricingResolutionRejection::MissingRule,
                StrictPricingRejectionReason::MissingRule,
            ),
            (
                PricingResolutionRejection::ModelDisabled {
                    lineage: PricingResolutionLineage::Admission,
                },
                StrictPricingRejectionReason::ModelUnavailable,
            ),
            (
                PricingResolutionRejection::MissingScopedSwitch {
                    lineage: PricingResolutionLineage::Policy,
                },
                StrictPricingRejectionReason::SwitchUnavailable,
            ),
            (
                PricingResolutionRejection::CapabilityNotInManifest {
                    lineage: PricingResolutionLineage::Admission,
                    dependency: PricingDependencyKind::Catalog,
                },
                StrictPricingRejectionReason::UnsupportedCapability,
            ),
            (
                PricingResolutionRejection::InvalidPolicyContract,
                StrictPricingRejectionReason::InvalidContract,
            ),
        ];
        for (rejection, expected) in cases {
            assert_eq!(
                StrictPricingRejectionReason::from_resolution(rejection),
                expected
            );
        }
    }
}
