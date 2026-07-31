//! Счётчики форвардинга для наблюдаемости (`/metrics`). Дёшевые атомики, монотонные с запуска.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::pricing::PricingBridgeFallbackReason;
use registry::pricing::SnapshotProvider;

const PRICING_BRIDGE_PROVIDER_COUNT: usize = 2;
const PRICING_BRIDGE_FALLBACK_REASON_COUNT: usize = 6;
pub const PRICING_BRIDGE_LATENCY_BUCKETS_MS: [u64; 10] =
    [1, 2, 5, 10, 25, 50, 100, 250, 500, 1_000];

fn pricing_bridge_provider_index(provider: SnapshotProvider) -> usize {
    match provider {
        SnapshotProvider::Anthropic => 0,
        SnapshotProvider::OpenAi => 1,
    }
}

fn pricing_bridge_fallback_index(reason: PricingBridgeFallbackReason) -> usize {
    match reason {
        PricingBridgeFallbackReason::BridgeDisabled => 0,
        PricingBridgeFallbackReason::NotSampled => 1,
        PricingBridgeFallbackReason::UnsupportedModelIdentity => 2,
        PricingBridgeFallbackReason::UnsupportedModifier => 3,
        PricingBridgeFallbackReason::SnapshotIdentityOversized => 4,
        PricingBridgeFallbackReason::OfficialHoldOutOfRange => 5,
    }
}

struct PricingBridgeMetrics {
    selected: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT],
    inserted: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT],
    unchanged: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT],
    not_reserved: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT],
    failures: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT],
    conflicts: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT],
    latency_count: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT],
    latency_sum_micros: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT],
    latency_buckets:
        [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT * PRICING_BRIDGE_LATENCY_BUCKETS_MS.len()],
    fallbacks: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT * PRICING_BRIDGE_FALLBACK_REASON_COUNT],
}

impl Default for PricingBridgeMetrics {
    fn default() -> Self {
        Self {
            selected: [const { AtomicU64::new(0) }; PRICING_BRIDGE_PROVIDER_COUNT],
            inserted: [const { AtomicU64::new(0) }; PRICING_BRIDGE_PROVIDER_COUNT],
            unchanged: [const { AtomicU64::new(0) }; PRICING_BRIDGE_PROVIDER_COUNT],
            not_reserved: [const { AtomicU64::new(0) }; PRICING_BRIDGE_PROVIDER_COUNT],
            failures: [const { AtomicU64::new(0) }; PRICING_BRIDGE_PROVIDER_COUNT],
            conflicts: [const { AtomicU64::new(0) }; PRICING_BRIDGE_PROVIDER_COUNT],
            latency_count: [const { AtomicU64::new(0) }; PRICING_BRIDGE_PROVIDER_COUNT],
            latency_sum_micros: [const { AtomicU64::new(0) }; PRICING_BRIDGE_PROVIDER_COUNT],
            latency_buckets: [const { AtomicU64::new(0) };
                PRICING_BRIDGE_PROVIDER_COUNT * PRICING_BRIDGE_LATENCY_BUCKETS_MS.len()],
            fallbacks: [const { AtomicU64::new(0) };
                PRICING_BRIDGE_PROVIDER_COUNT * PRICING_BRIDGE_FALLBACK_REASON_COUNT],
        }
    }
}

#[derive(Default)]
pub struct Metrics {
    pub requests: AtomicU64,     // всего обслуженных запросов (после авторизации)
    pub upstream_429: AtomicU64, // ответов апстрима 429 (квота подписки)
    pub upstream_auth: AtomicU64, // 401/403 (мёртвый токен → карантин)
    pub upstream_5xx: AtomicU64, // backend-fault (5xx/408/409/425)
    pub breaker_rejects: AtomicU64, // отбито разомкнутым circuit breaker
    pub exhausted: AtomicU64,    // исчерпание пула (все за лимитом) → 429+Retry-After
    pub key_throttled: AtomicU64, // отбито fair-share (кит превысил потолок одновременных)
    pub auth_failures: AtomicU64, // неудачных авторизаций (спайк = брутфорс/скан управляющих ключей)
    /// Successful Gemini generations that ended without authoritative usage. Metered non-stream
    /// delivery is withheld; a stream already delivered settles its conservative hold.
    pub gemini_usage_missing: AtomicU64,
    /// Gemini-only low-cardinality failure classes. They intentionally carry no profile, request,
    /// proxy, project or upstream-error labels.
    pub gemini_transport_failures: AtomicU64,
    pub gemini_backend_failures: AtomicU64,
    pub gemini_malformed_responses: AtomicU64,
    pub gemini_stream_start_failures: AtomicU64,
    pricing_bridge: PricingBridgeMetrics,
}

pub struct PricingBridgeLatencyGuard<'a> {
    metrics: &'a Metrics,
    provider: SnapshotProvider,
    started: Instant,
}

impl Drop for PricingBridgeLatencyGuard<'_> {
    fn drop(&mut self) {
        self.metrics
            .observe_pricing_bridge_latency(self.provider, self.started.elapsed());
    }
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

    pub fn pricing_bridge_selected(&self, provider: SnapshotProvider) {
        Self::inc(&self.pricing_bridge.selected[pricing_bridge_provider_index(provider)]);
    }

    pub fn pricing_bridge_inserted(&self, provider: SnapshotProvider) {
        Self::inc(&self.pricing_bridge.inserted[pricing_bridge_provider_index(provider)]);
    }

    pub fn pricing_bridge_unchanged(&self, provider: SnapshotProvider) {
        Self::inc(&self.pricing_bridge.unchanged[pricing_bridge_provider_index(provider)]);
    }

    pub fn pricing_bridge_not_reserved(&self, provider: SnapshotProvider) {
        Self::inc(&self.pricing_bridge.not_reserved[pricing_bridge_provider_index(provider)]);
    }

    pub fn pricing_bridge_failure(&self, provider: SnapshotProvider) {
        Self::inc(&self.pricing_bridge.failures[pricing_bridge_provider_index(provider)]);
    }

    pub fn pricing_bridge_conflict(&self, provider: SnapshotProvider) {
        Self::inc(&self.pricing_bridge.conflicts[pricing_bridge_provider_index(provider)]);
    }

    pub fn pricing_bridge_latency_timer(
        &self,
        provider: SnapshotProvider,
    ) -> PricingBridgeLatencyGuard<'_> {
        PricingBridgeLatencyGuard {
            metrics: self,
            provider,
            started: Instant::now(),
        }
    }

    fn observe_pricing_bridge_latency(
        &self,
        provider: SnapshotProvider,
        elapsed: std::time::Duration,
    ) {
        let provider_index = pricing_bridge_provider_index(provider);
        Self::inc(&self.pricing_bridge.latency_count[provider_index]);
        let micros = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        let sum = &self.pricing_bridge.latency_sum_micros[provider_index];
        let _ = sum.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(current.saturating_add(micros))
        });
        for (bucket_index, upper_ms) in PRICING_BRIDGE_LATENCY_BUCKETS_MS.iter().enumerate() {
            if elapsed <= std::time::Duration::from_millis(*upper_ms) {
                let index = provider_index * PRICING_BRIDGE_LATENCY_BUCKETS_MS.len() + bucket_index;
                Self::inc(&self.pricing_bridge.latency_buckets[index]);
            }
        }
    }

    pub fn pricing_bridge_fallback(
        &self,
        provider: SnapshotProvider,
        reason: PricingBridgeFallbackReason,
    ) {
        let index = pricing_bridge_provider_index(provider) * PRICING_BRIDGE_FALLBACK_REASON_COUNT
            + pricing_bridge_fallback_index(reason);
        Self::inc(&self.pricing_bridge.fallbacks[index]);
    }

    pub fn pricing_bridge_selected_count(&self, provider: SnapshotProvider) -> u64 {
        Self::get(&self.pricing_bridge.selected[pricing_bridge_provider_index(provider)])
    }

    pub fn pricing_bridge_inserted_count(&self, provider: SnapshotProvider) -> u64 {
        Self::get(&self.pricing_bridge.inserted[pricing_bridge_provider_index(provider)])
    }

    pub fn pricing_bridge_unchanged_count(&self, provider: SnapshotProvider) -> u64 {
        Self::get(&self.pricing_bridge.unchanged[pricing_bridge_provider_index(provider)])
    }

    pub fn pricing_bridge_not_reserved_count(&self, provider: SnapshotProvider) -> u64 {
        Self::get(&self.pricing_bridge.not_reserved[pricing_bridge_provider_index(provider)])
    }

    pub fn pricing_bridge_failure_count(&self, provider: SnapshotProvider) -> u64 {
        Self::get(&self.pricing_bridge.failures[pricing_bridge_provider_index(provider)])
    }

    pub fn pricing_bridge_conflict_count(&self, provider: SnapshotProvider) -> u64 {
        Self::get(&self.pricing_bridge.conflicts[pricing_bridge_provider_index(provider)])
    }

    pub fn pricing_bridge_latency_count(&self, provider: SnapshotProvider) -> u64 {
        Self::get(&self.pricing_bridge.latency_count[pricing_bridge_provider_index(provider)])
    }

    pub fn pricing_bridge_latency_sum_seconds(&self, provider: SnapshotProvider) -> f64 {
        Self::get(&self.pricing_bridge.latency_sum_micros[pricing_bridge_provider_index(provider)])
            as f64
            / 1_000_000.0
    }

    pub fn pricing_bridge_latency_bucket_count(
        &self,
        provider: SnapshotProvider,
        bucket_index: usize,
    ) -> u64 {
        let index = pricing_bridge_provider_index(provider)
            * PRICING_BRIDGE_LATENCY_BUCKETS_MS.len()
            + bucket_index;
        Self::get(&self.pricing_bridge.latency_buckets[index])
    }

    pub fn pricing_bridge_fallback_count(
        &self,
        provider: SnapshotProvider,
        reason: PricingBridgeFallbackReason,
    ) -> u64 {
        let index = pricing_bridge_provider_index(provider) * PRICING_BRIDGE_FALLBACK_REASON_COUNT
            + pricing_bridge_fallback_index(reason);
        Self::get(&self.pricing_bridge.fallbacks[index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_bridge_counters_are_provider_and_reason_bounded() {
        let metrics = Metrics::new();
        metrics.pricing_bridge_selected(SnapshotProvider::Anthropic);
        metrics.pricing_bridge_selected(SnapshotProvider::OpenAi);
        metrics.pricing_bridge_inserted(SnapshotProvider::Anthropic);
        metrics.pricing_bridge_unchanged(SnapshotProvider::OpenAi);
        metrics.pricing_bridge_not_reserved(SnapshotProvider::Anthropic);
        metrics.pricing_bridge_failure(SnapshotProvider::OpenAi);
        metrics.pricing_bridge_conflict(SnapshotProvider::Anthropic);
        {
            let _timer = metrics.pricing_bridge_latency_timer(SnapshotProvider::OpenAi);
        }
        metrics.pricing_bridge_fallback(
            SnapshotProvider::Anthropic,
            PricingBridgeFallbackReason::NotSampled,
        );

        assert_eq!(
            metrics.pricing_bridge_selected_count(SnapshotProvider::Anthropic),
            1
        );
        assert_eq!(
            metrics.pricing_bridge_selected_count(SnapshotProvider::OpenAi),
            1
        );
        assert_eq!(
            metrics.pricing_bridge_inserted_count(SnapshotProvider::Anthropic),
            1
        );
        assert_eq!(
            metrics.pricing_bridge_unchanged_count(SnapshotProvider::OpenAi),
            1
        );
        assert_eq!(
            metrics.pricing_bridge_not_reserved_count(SnapshotProvider::Anthropic),
            1
        );
        assert_eq!(
            metrics.pricing_bridge_failure_count(SnapshotProvider::OpenAi),
            1
        );
        assert_eq!(
            metrics.pricing_bridge_conflict_count(SnapshotProvider::Anthropic),
            1
        );
        assert_eq!(
            metrics.pricing_bridge_latency_count(SnapshotProvider::OpenAi),
            1
        );
        assert_eq!(
            metrics.pricing_bridge_latency_bucket_count(
                SnapshotProvider::OpenAi,
                PRICING_BRIDGE_LATENCY_BUCKETS_MS.len() - 1,
            ),
            1
        );
        assert_eq!(
            metrics.pricing_bridge_fallback_count(
                SnapshotProvider::Anthropic,
                PricingBridgeFallbackReason::NotSampled,
            ),
            1
        );
        assert_eq!(
            metrics.pricing_bridge_fallback_count(
                SnapshotProvider::OpenAi,
                PricingBridgeFallbackReason::NotSampled,
            ),
            0
        );
    }
}
