//! Счётчики форвардинга для наблюдаемости (`/metrics`). Дёшевые атомики, монотонные с запуска.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::pricing::{
    PricingBridgeFallbackReason, PricingDependencyKind, PricingResolutionRejection,
    PricingShadowEnqueueResult, PricingShadowProcessingResult,
};
use registry::pricing::{
    PolicyRuleScope, PricingMode, PricingShadowComparison, PricingShadowEvaluationOutcome,
    PricingShadowReadErrorCode, PricingShadowRejectionCode, SnapshotProvider,
};

const PRICING_BRIDGE_PROVIDER_COUNT: usize = 2;
const PRICING_BRIDGE_FALLBACK_REASON_COUNT: usize = 6;
pub const PRICING_BRIDGE_LATENCY_BUCKETS_MS: [u64; 10] =
    [1, 2, 5, 10, 25, 50, 100, 250, 500, 1_000];
pub const PRICING_SHADOW_QUEUE_AGE_BUCKETS_SECS: [u64; 9] =
    [0, 1, 5, 15, 60, 300, 900, 3_600, 21_600];
const PRICING_SHADOW_ENQUEUE_RESULT_COUNT: usize = PricingShadowEnqueueResult::ALL.len();
const PRICING_SHADOW_PROCESSING_RESULT_COUNT: usize = PricingShadowProcessingResult::ALL.len();
const PRICING_SHADOW_REJECTION_COUNT: usize = PricingShadowRejectionCode::ALL.len();
const PRICING_SHADOW_READ_ERROR_COUNT: usize = PricingShadowReadErrorCode::ALL.len();
const PRICING_SHADOW_RESOLVED_DIMENSION_COUNT: usize = 8;
const STRICT_PRICING_ADMITTED_DIMENSION_COUNT: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum StrictPricingProvider {
    Anthropic,
    OpenAi,
    Gemini,
}

impl StrictPricingProvider {
    pub const ALL: &'static [Self] = &[Self::Anthropic, Self::OpenAi, Self::Gemini];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
        }
    }
}

impl From<SnapshotProvider> for StrictPricingProvider {
    fn from(provider: SnapshotProvider) -> Self {
        match provider {
            SnapshotProvider::Anthropic => Self::Anthropic,
            SnapshotProvider::OpenAi => Self::OpenAi,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum StrictPricingRejectionReason {
    UnsupportedModel,
    MissingPolicy,
    MissingRule,
    ModelUnavailable,
    SwitchUnavailable,
    UnsupportedCapability,
    InvalidContract,
    ReadUnavailable,
    LowBalance,
    QuoteInvariant,
    SnapshotInvariant,
    ReserveConflict,
    HandoffAborted,
    ReserveUnavailable,
    GeminiUnsupported,
}

impl StrictPricingRejectionReason {
    pub const ALL: &'static [Self] = &[
        Self::UnsupportedModel,
        Self::MissingPolicy,
        Self::MissingRule,
        Self::ModelUnavailable,
        Self::SwitchUnavailable,
        Self::UnsupportedCapability,
        Self::InvalidContract,
        Self::ReadUnavailable,
        Self::LowBalance,
        Self::QuoteInvariant,
        Self::SnapshotInvariant,
        Self::ReserveConflict,
        Self::HandoffAborted,
        Self::ReserveUnavailable,
        Self::GeminiUnsupported,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedModel => "unsupported_model",
            Self::MissingPolicy => "missing_policy",
            Self::MissingRule => "missing_rule",
            Self::ModelUnavailable => "model_unavailable",
            Self::SwitchUnavailable => "switch_unavailable",
            Self::UnsupportedCapability => "unsupported_capability",
            Self::InvalidContract => "invalid_contract",
            Self::ReadUnavailable => "read_unavailable",
            Self::LowBalance => "low_balance",
            Self::QuoteInvariant => "quote_invariant",
            Self::SnapshotInvariant => "snapshot_invariant",
            Self::ReserveConflict => "reserve_conflict",
            Self::HandoffAborted => "handoff_aborted",
            Self::ReserveUnavailable => "reserve_unavailable",
            Self::GeminiUnsupported => "gemini_unsupported",
        }
    }

    pub const fn from_resolution(reason: PricingResolutionRejection) -> Self {
        match reason {
            PricingResolutionRejection::NoPolicyBinding
            | PricingResolutionRejection::InactivePolicy => Self::MissingPolicy,
            PricingResolutionRejection::MissingRule => Self::MissingRule,
            PricingResolutionRejection::MissingDependency {
                dependency: PricingDependencyKind::Catalog,
                ..
            }
            | PricingResolutionRejection::ModelNotInCatalog { .. }
            | PricingResolutionRejection::ModelDisabled { .. } => Self::ModelUnavailable,
            PricingResolutionRejection::MissingDependency {
                dependency: PricingDependencyKind::Switches,
                ..
            }
            | PricingResolutionRejection::MissingMasterSwitch { .. }
            | PricingResolutionRejection::MasterSwitchDisabled { .. }
            | PricingResolutionRejection::MissingScopedSwitch { .. }
            | PricingResolutionRejection::PolicyScopedSwitchTargetMismatch
            | PricingResolutionRejection::AdmissionScopedSwitchTargetMismatch
            | PricingResolutionRejection::ScopedSwitchDisabled { .. } => Self::SwitchUnavailable,
            PricingResolutionRejection::InvalidRuntimeManifest
            | PricingResolutionRejection::CapabilityNotInManifest { .. } => {
                Self::UnsupportedCapability
            }
            PricingResolutionRejection::InvalidRequest
            | PricingResolutionRejection::AccountMismatch
            | PricingResolutionRejection::PolicySchemaMismatch
            | PricingResolutionRejection::SchemaMismatch { .. }
            | PricingResolutionRejection::CatalogTargetMismatch { .. }
            | PricingResolutionRejection::PolicySwitchTargetMismatch
            | PricingResolutionRejection::InvalidDependency { .. }
            | PricingResolutionRejection::InvalidPolicyContract => Self::InvalidContract,
        }
    }
}

const STRICT_PRICING_PROVIDER_COUNT: usize = StrictPricingProvider::ALL.len();

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

const fn pricing_shadow_resolved_index(
    mode: PricingMode,
    model_scope: bool,
    comparison: PricingShadowComparison,
) -> usize {
    let mode = match mode {
        PricingMode::Track => 0,
        PricingMode::Discount => 1,
    };
    let scope = if model_scope { 1 } else { 0 };
    let comparison = match comparison {
        PricingShadowComparison::Equal => 0,
        PricingShadowComparison::Different => 1,
    };
    mode * 4 + scope * 2 + comparison
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

struct PricingShadowMetrics {
    enqueue: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT * PRICING_SHADOW_ENQUEUE_RESULT_COUNT],
    processing: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT * PRICING_SHADOW_PROCESSING_RESULT_COUNT],
    resolved: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT * PRICING_SHADOW_RESOLVED_DIMENSION_COUNT],
    rejected: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT * PRICING_SHADOW_REJECTION_COUNT],
    read_error: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT * PRICING_SHADOW_READ_ERROR_COUNT],
    queue_depth: AtomicU64,
    queue_high_water: AtomicU64,
    queue_age_count: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT],
    queue_age_sum_secs: [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT],
    queue_age_buckets:
        [AtomicU64; PRICING_BRIDGE_PROVIDER_COUNT * PRICING_SHADOW_QUEUE_AGE_BUCKETS_SECS.len()],
}

struct StrictPricingMetrics {
    admitted: [AtomicU64; STRICT_PRICING_PROVIDER_COUNT * STRICT_PRICING_ADMITTED_DIMENSION_COUNT],
    rejected: [AtomicU64; STRICT_PRICING_PROVIDER_COUNT * StrictPricingRejectionReason::ALL.len()],
}

impl Default for StrictPricingMetrics {
    fn default() -> Self {
        Self {
            admitted: [const { AtomicU64::new(0) };
                STRICT_PRICING_PROVIDER_COUNT * STRICT_PRICING_ADMITTED_DIMENSION_COUNT],
            rejected: [const { AtomicU64::new(0) };
                STRICT_PRICING_PROVIDER_COUNT * StrictPricingRejectionReason::ALL.len()],
        }
    }
}

impl Default for PricingShadowMetrics {
    fn default() -> Self {
        Self {
            enqueue: [const { AtomicU64::new(0) };
                PRICING_BRIDGE_PROVIDER_COUNT * PRICING_SHADOW_ENQUEUE_RESULT_COUNT],
            processing: [const { AtomicU64::new(0) };
                PRICING_BRIDGE_PROVIDER_COUNT * PRICING_SHADOW_PROCESSING_RESULT_COUNT],
            resolved: [const { AtomicU64::new(0) };
                PRICING_BRIDGE_PROVIDER_COUNT * PRICING_SHADOW_RESOLVED_DIMENSION_COUNT],
            rejected: [const { AtomicU64::new(0) };
                PRICING_BRIDGE_PROVIDER_COUNT * PRICING_SHADOW_REJECTION_COUNT],
            read_error: [const { AtomicU64::new(0) };
                PRICING_BRIDGE_PROVIDER_COUNT * PRICING_SHADOW_READ_ERROR_COUNT],
            queue_depth: AtomicU64::new(0),
            queue_high_water: AtomicU64::new(0),
            queue_age_count: [const { AtomicU64::new(0) }; PRICING_BRIDGE_PROVIDER_COUNT],
            queue_age_sum_secs: [const { AtomicU64::new(0) }; PRICING_BRIDGE_PROVIDER_COUNT],
            queue_age_buckets: [const { AtomicU64::new(0) };
                PRICING_BRIDGE_PROVIDER_COUNT * PRICING_SHADOW_QUEUE_AGE_BUCKETS_SECS.len()],
        }
    }
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
    /// Exact `not_started` proofs actually returned by each fixed provider plane. The array is
    /// compile-bounded to Anthropic/OpenAI/Gemini and never carries request or credential labels.
    execution_not_started: [AtomicU64; 3],
    pricing_bridge: PricingBridgeMetrics,
    pricing_shadow: PricingShadowMetrics,
    strict_pricing: StrictPricingMetrics,
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

    pub fn pricing_bridge_selected(&self, provider: SnapshotProvider) {
        Self::inc(&self.pricing_bridge.selected[pricing_bridge_provider_index(provider)]);
    }

    pub fn strict_pricing_admitted(
        &self,
        provider: StrictPricingProvider,
        mode: PricingMode,
        model_scope: bool,
    ) {
        let mode = match mode {
            PricingMode::Track => 0,
            PricingMode::Discount => 1,
        };
        let scope = usize::from(model_scope);
        let index = provider as usize * STRICT_PRICING_ADMITTED_DIMENSION_COUNT + mode * 2 + scope;
        Self::inc(&self.strict_pricing.admitted[index]);
    }

    pub fn strict_pricing_rejected(
        &self,
        provider: StrictPricingProvider,
        reason: StrictPricingRejectionReason,
    ) {
        let index = provider as usize * StrictPricingRejectionReason::ALL.len() + reason as usize;
        Self::inc(&self.strict_pricing.rejected[index]);
    }

    pub fn strict_pricing_admitted_count(
        &self,
        provider: StrictPricingProvider,
        mode: PricingMode,
        model_scope: bool,
    ) -> u64 {
        let mode = match mode {
            PricingMode::Track => 0,
            PricingMode::Discount => 1,
        };
        let index = provider as usize * STRICT_PRICING_ADMITTED_DIMENSION_COUNT
            + mode * 2
            + usize::from(model_scope);
        Self::get(&self.strict_pricing.admitted[index])
    }

    pub fn strict_pricing_rejected_count(
        &self,
        provider: StrictPricingProvider,
        reason: StrictPricingRejectionReason,
    ) -> u64 {
        let index = provider as usize * StrictPricingRejectionReason::ALL.len() + reason as usize;
        Self::get(&self.strict_pricing.rejected[index])
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

    pub fn pricing_shadow_enqueue(
        &self,
        provider: SnapshotProvider,
        result: PricingShadowEnqueueResult,
    ) {
        let index = pricing_bridge_provider_index(provider) * PRICING_SHADOW_ENQUEUE_RESULT_COUNT
            + result as usize;
        Self::inc(&self.pricing_shadow.enqueue[index]);
    }

    pub fn pricing_shadow_processing(
        &self,
        provider: SnapshotProvider,
        result: PricingShadowProcessingResult,
    ) {
        let index = pricing_bridge_provider_index(provider)
            * PRICING_SHADOW_PROCESSING_RESULT_COUNT
            + result as usize;
        Self::inc(&self.pricing_shadow.processing[index]);
    }

    pub fn pricing_shadow_outcome(
        &self,
        provider: SnapshotProvider,
        outcome: &PricingShadowEvaluationOutcome,
    ) {
        let provider = pricing_bridge_provider_index(provider);
        match outcome {
            PricingShadowEvaluationOutcome::Resolved(resolved) => {
                let model_scope = matches!(&resolved.rule.scope, PolicyRuleScope::Model { .. });
                let index = provider * PRICING_SHADOW_RESOLVED_DIMENSION_COUNT
                    + pricing_shadow_resolved_index(
                        resolved.rule.pricing_mode,
                        model_scope,
                        resolved.comparison(),
                    );
                Self::inc(&self.pricing_shadow.resolved[index]);
            }
            PricingShadowEvaluationOutcome::Rejected { reason, .. } => {
                let index = provider * PRICING_SHADOW_REJECTION_COUNT + reason.metric_index();
                Self::inc(&self.pricing_shadow.rejected[index]);
            }
            PricingShadowEvaluationOutcome::ReadError { reason } => {
                let index = provider * PRICING_SHADOW_READ_ERROR_COUNT + reason.metric_index();
                Self::inc(&self.pricing_shadow.read_error[index]);
            }
        }
    }

    /// Reserve one process-local queue-depth slot immediately before the sole `try_send`.
    /// The sender mutex serializes producers, while the worker cannot receive the item before this
    /// increment, so a fast consumer cannot race the depth permanently above zero.
    pub fn pricing_shadow_queue_try_begin(&self) -> u64 {
        self.pricing_shadow
            .queue_depth
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1)
    }

    pub fn pricing_shadow_queue_accepted(&self, accepted_depth: u64) {
        let _ = self.pricing_shadow.queue_high_water.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |current| Some(current.max(accepted_depth)),
        );
    }

    pub fn pricing_shadow_queue_rejected(&self) {
        self.pricing_shadow_queue_started();
    }

    pub fn pricing_shadow_queue_started(&self) {
        let _ = self.pricing_shadow.queue_depth.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |current| Some(current.saturating_sub(1)),
        );
    }

    pub fn observe_pricing_shadow_queue_age(&self, provider: SnapshotProvider, age_secs: u64) {
        let provider = pricing_bridge_provider_index(provider);
        Self::inc(&self.pricing_shadow.queue_age_count[provider]);
        let _ = self.pricing_shadow.queue_age_sum_secs[provider].fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |current| Some(current.saturating_add(age_secs)),
        );
        for (bucket, upper) in PRICING_SHADOW_QUEUE_AGE_BUCKETS_SECS.iter().enumerate() {
            if age_secs <= *upper {
                let index = provider * PRICING_SHADOW_QUEUE_AGE_BUCKETS_SECS.len() + bucket;
                Self::inc(&self.pricing_shadow.queue_age_buckets[index]);
            }
        }
    }

    pub fn pricing_shadow_enqueue_count(
        &self,
        provider: SnapshotProvider,
        result: PricingShadowEnqueueResult,
    ) -> u64 {
        let index = pricing_bridge_provider_index(provider) * PRICING_SHADOW_ENQUEUE_RESULT_COUNT
            + result as usize;
        Self::get(&self.pricing_shadow.enqueue[index])
    }

    pub fn pricing_shadow_processing_count(
        &self,
        provider: SnapshotProvider,
        result: PricingShadowProcessingResult,
    ) -> u64 {
        let index = pricing_bridge_provider_index(provider)
            * PRICING_SHADOW_PROCESSING_RESULT_COUNT
            + result as usize;
        Self::get(&self.pricing_shadow.processing[index])
    }

    pub fn pricing_shadow_rejection_count(
        &self,
        provider: SnapshotProvider,
        reason: PricingShadowRejectionCode,
    ) -> u64 {
        let index = pricing_bridge_provider_index(provider) * PRICING_SHADOW_REJECTION_COUNT
            + reason.metric_index();
        Self::get(&self.pricing_shadow.rejected[index])
    }

    pub fn pricing_shadow_read_error_count(
        &self,
        provider: SnapshotProvider,
        reason: PricingShadowReadErrorCode,
    ) -> u64 {
        let index = pricing_bridge_provider_index(provider) * PRICING_SHADOW_READ_ERROR_COUNT
            + reason.metric_index();
        Self::get(&self.pricing_shadow.read_error[index])
    }

    pub fn pricing_shadow_resolved_count(
        &self,
        provider: SnapshotProvider,
        mode: PricingMode,
        model_scope: bool,
        comparison: PricingShadowComparison,
    ) -> u64 {
        let index = pricing_bridge_provider_index(provider)
            * PRICING_SHADOW_RESOLVED_DIMENSION_COUNT
            + pricing_shadow_resolved_index(mode, model_scope, comparison);
        Self::get(&self.pricing_shadow.resolved[index])
    }

    pub fn pricing_shadow_queue_depth(&self) -> u64 {
        Self::get(&self.pricing_shadow.queue_depth)
    }

    pub fn pricing_shadow_queue_high_water(&self) -> u64 {
        Self::get(&self.pricing_shadow.queue_high_water)
    }

    pub fn pricing_shadow_queue_age_count(&self, provider: SnapshotProvider) -> u64 {
        Self::get(&self.pricing_shadow.queue_age_count[pricing_bridge_provider_index(provider)])
    }

    pub fn pricing_shadow_queue_age_sum_seconds(&self, provider: SnapshotProvider) -> u64 {
        Self::get(&self.pricing_shadow.queue_age_sum_secs[pricing_bridge_provider_index(provider)])
    }

    pub fn pricing_shadow_queue_age_bucket_count(
        &self,
        provider: SnapshotProvider,
        bucket: usize,
    ) -> u64 {
        let index = pricing_bridge_provider_index(provider)
            * PRICING_SHADOW_QUEUE_AGE_BUCKETS_SECS.len()
            + bucket;
        Self::get(&self.pricing_shadow.queue_age_buckets[index])
    }
}

fn execution_plane_index(plane: crate::ProviderMode) -> Option<usize> {
    match plane {
        crate::ProviderMode::Anthropic => Some(0),
        crate::ProviderMode::OpenAi => Some(1),
        crate::ProviderMode::Gemini => Some(2),
        crate::ProviderMode::Combined => None,
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
    fn strict_pricing_counters_have_fixed_provider_reason_and_admission_dimensions() {
        let metrics = Metrics::new();
        for provider in StrictPricingProvider::ALL {
            for reason in StrictPricingRejectionReason::ALL {
                metrics.strict_pricing_rejected(*provider, *reason);
            }
            for mode in [PricingMode::Track, PricingMode::Discount] {
                for model_scope in [false, true] {
                    metrics.strict_pricing_admitted(*provider, mode, model_scope);
                }
            }
        }

        for provider in StrictPricingProvider::ALL {
            for reason in StrictPricingRejectionReason::ALL {
                assert_eq!(metrics.strict_pricing_rejected_count(*provider, *reason), 1);
            }
            for mode in [PricingMode::Track, PricingMode::Discount] {
                for model_scope in [false, true] {
                    assert_eq!(
                        metrics.strict_pricing_admitted_count(*provider, mode, model_scope),
                        1
                    );
                }
            }
        }
        assert_eq!(
            metrics.strict_pricing.rejected.len(),
            StrictPricingProvider::ALL.len() * StrictPricingRejectionReason::ALL.len()
        );
        assert_eq!(
            metrics.strict_pricing.admitted.len(),
            StrictPricingProvider::ALL.len() * STRICT_PRICING_ADMITTED_DIMENSION_COUNT
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
