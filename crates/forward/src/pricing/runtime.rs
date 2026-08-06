//! Bounded, default-off PostgreSQL pricing-shadow producer and worker.
//!
//! Request handlers perform one non-blocking `try_send` only after an atomic actual snapshot has
//! committed. Policy reads, resolution and immutable evaluation persistence stay on background
//! tasks and can never change customer admission, response or money.

use super::{
    build_pricing_shadow_evaluation, EnginePricingRequestId, PricingShadowEvaluationSource,
    PricingShadowReadFailure, PricingShadowWorkItem, PricingShadowWorkItemError,
};
use crate::billing::AsyncBilling;
use crate::metrics::Metrics;
use anyhow::{bail, Context, Result};
use registry::pricing::{
    LegacyScalarAdmissionSnapshot, PricingRuntimeManifestEvidence, PricingShadowEvaluationWrite,
    ShadowActualSnapshotRef, ShadowDiagnosticContext, ShadowEligibilityError, SnapshotProvider,
    LEGACY_SCALAR_REPLAY_MAX_AGE_SECS,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const SHADOW_SAMPLER_DOMAIN_V1: &[u8] = b"claude-api/pricing/evaluation-shadow-sampler/v1\0";
const SAMPLER_BUCKETS: u16 = 10_000;
const TOKEN_UNIT: u128 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PricingShadowConfigValues {
    pub enabled: bool,
    pub sample_bp: i64,
    pub queue_capacity: usize,
    pub worker_concurrency: usize,
    pub timeout_ms: u64,
    pub max_queue_age_secs: i64,
    pub max_field_bytes: usize,
    pub max_item_bytes: usize,
    pub rate_per_sec: u64,
    pub rate_burst: u64,
    pub db_read_connections: usize,
}

impl Default for PricingShadowConfigValues {
    fn default() -> Self {
        Self {
            enabled: false,
            sample_bp: 0,
            queue_capacity: 256,
            worker_concurrency: 2,
            timeout_ms: 750,
            max_queue_age_secs: 300,
            max_field_bytes: 512,
            max_item_bytes: 16 * 1024,
            rate_per_sec: 20,
            rate_burst: 40,
            db_read_connections: 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingShadowConfigError {
    SampleOutOfRange,
    DisabledWithSample,
    EnabledWithoutSample,
    QueueCapacityOutOfRange,
    WorkerConcurrencyOutOfRange,
    TimeoutOutOfRange,
    QueueAgeOutOfRange,
    FieldBytesOutOfRange,
    ItemBytesOutOfRange,
    RateOutOfRange,
    BurstOutOfRange,
    DatabaseConnectionsOutOfRange,
}

impl PricingShadowConfigError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::SampleOutOfRange => "sample_out_of_range",
            Self::DisabledWithSample => "disabled_with_sample",
            Self::EnabledWithoutSample => "enabled_without_sample",
            Self::QueueCapacityOutOfRange => "queue_capacity_out_of_range",
            Self::WorkerConcurrencyOutOfRange => "worker_concurrency_out_of_range",
            Self::TimeoutOutOfRange => "timeout_out_of_range",
            Self::QueueAgeOutOfRange => "queue_age_out_of_range",
            Self::FieldBytesOutOfRange => "field_bytes_out_of_range",
            Self::ItemBytesOutOfRange => "item_bytes_out_of_range",
            Self::RateOutOfRange => "rate_out_of_range",
            Self::BurstOutOfRange => "burst_out_of_range",
            Self::DatabaseConnectionsOutOfRange => "database_connections_out_of_range",
        }
    }
}

/// Validated rollout configuration. Private fields prevent an enabled-at-zero or unbounded state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PricingShadowConfig {
    values: PricingShadowConfigValues,
}

impl PricingShadowConfig {
    pub fn from_values(
        values: PricingShadowConfigValues,
    ) -> std::result::Result<Self, PricingShadowConfigError> {
        if !(0..=i64::from(SAMPLER_BUCKETS)).contains(&values.sample_bp) {
            return Err(PricingShadowConfigError::SampleOutOfRange);
        }
        match (values.enabled, values.sample_bp) {
            (false, 0) => {}
            (false, _) => return Err(PricingShadowConfigError::DisabledWithSample),
            (true, 0) => return Err(PricingShadowConfigError::EnabledWithoutSample),
            (true, _) => {}
        }
        if !(1..=4_096).contains(&values.queue_capacity) {
            return Err(PricingShadowConfigError::QueueCapacityOutOfRange);
        }
        if !(1..=32).contains(&values.worker_concurrency)
            || values.worker_concurrency > values.queue_capacity
        {
            return Err(PricingShadowConfigError::WorkerConcurrencyOutOfRange);
        }
        if !(10..=15_000).contains(&values.timeout_ms) {
            return Err(PricingShadowConfigError::TimeoutOutOfRange);
        }
        if !(1..LEGACY_SCALAR_REPLAY_MAX_AGE_SECS).contains(&values.max_queue_age_secs) {
            return Err(PricingShadowConfigError::QueueAgeOutOfRange);
        }
        if !(64..=4_096).contains(&values.max_field_bytes) {
            return Err(PricingShadowConfigError::FieldBytesOutOfRange);
        }
        if !(1_024..=128 * 1_024).contains(&values.max_item_bytes)
            || values.max_item_bytes < values.max_field_bytes
        {
            return Err(PricingShadowConfigError::ItemBytesOutOfRange);
        }
        if !(1..=10_000).contains(&values.rate_per_sec) {
            return Err(PricingShadowConfigError::RateOutOfRange);
        }
        if values.rate_burst == 0 || values.rate_burst > values.rate_per_sec.saturating_mul(60) {
            return Err(PricingShadowConfigError::BurstOutOfRange);
        }
        if !(1..=8).contains(&values.db_read_connections) {
            return Err(PricingShadowConfigError::DatabaseConnectionsOutOfRange);
        }
        Ok(Self { values })
    }

    pub fn disabled() -> Self {
        Self::default()
    }

    pub const fn enabled(self) -> bool {
        self.values.enabled
    }

    pub const fn sample_bp(self) -> u16 {
        self.values.sample_bp as u16
    }

    pub const fn queue_capacity(self) -> usize {
        self.values.queue_capacity
    }

    pub const fn worker_concurrency(self) -> usize {
        self.values.worker_concurrency
    }

    pub const fn timeout_ms(self) -> u64 {
        self.values.timeout_ms
    }

    pub const fn max_queue_age_secs(self) -> i64 {
        self.values.max_queue_age_secs
    }

    pub const fn max_field_bytes(self) -> usize {
        self.values.max_field_bytes
    }

    pub const fn max_item_bytes(self) -> usize {
        self.values.max_item_bytes
    }

    pub const fn rate_per_sec(self) -> u64 {
        self.values.rate_per_sec
    }

    pub const fn rate_burst(self) -> u64 {
        self.values.rate_burst
    }

    pub const fn db_read_connections(self) -> usize {
        self.values.db_read_connections
    }
}

impl Default for PricingShadowConfig {
    fn default() -> Self {
        Self::from_values(PricingShadowConfigValues::default())
            .expect("built-in pricing shadow defaults are valid")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum PricingShadowEnqueueResult {
    Accepted,
    Disabled,
    NotSampled,
    RateLimited,
    OversizedField,
    OversizedItem,
    InvalidActual,
    InvalidTimestamp,
    /// Compatibility-only metric value for older producer binaries; current funding caps enqueue.
    BalanceCapped,
    QueueFull,
    QueueClosed,
}

impl PricingShadowEnqueueResult {
    pub const ALL: [Self; 11] = [
        Self::Accepted,
        Self::Disabled,
        Self::NotSampled,
        Self::RateLimited,
        Self::OversizedField,
        Self::OversizedItem,
        Self::InvalidActual,
        Self::InvalidTimestamp,
        Self::BalanceCapped,
        Self::QueueFull,
        Self::QueueClosed,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Disabled => "disabled",
            Self::NotSampled => "not_sampled",
            Self::RateLimited => "rate_limited",
            Self::OversizedField => "oversized_field",
            Self::OversizedItem => "oversized_item",
            Self::InvalidActual => "invalid_actual",
            Self::InvalidTimestamp => "invalid_timestamp",
            Self::BalanceCapped => "balance_capped",
            Self::QueueFull => "queue_full",
            Self::QueueClosed => "queue_closed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum PricingShadowProcessingResult {
    Expired,
    ClockError,
    BuildError,
    ReadError,
    ReadTimeout,
    WriteError,
    WriteTimeout,
    Inserted,
    Replayed,
    Conflict,
    Cancelled,
}

impl PricingShadowProcessingResult {
    pub const ALL: [Self; 11] = [
        Self::Expired,
        Self::ClockError,
        Self::BuildError,
        Self::ReadError,
        Self::ReadTimeout,
        Self::WriteError,
        Self::WriteTimeout,
        Self::Inserted,
        Self::Replayed,
        Self::Conflict,
        Self::Cancelled,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::Expired => "expired",
            Self::ClockError => "clock_error",
            Self::BuildError => "build_error",
            Self::ReadError => "read_error",
            Self::ReadTimeout => "read_timeout",
            Self::WriteError => "write_error",
            Self::WriteTimeout => "write_timeout",
            Self::Inserted => "inserted",
            Self::Replayed => "replayed",
            Self::Conflict => "conflict",
            Self::Cancelled => "cancelled",
        }
    }
}

struct TokenBucket {
    tokens: u128,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(burst: u64) -> Self {
        Self {
            tokens: u128::from(burst).saturating_mul(TOKEN_UNIT),
            last_refill: Instant::now(),
        }
    }

    fn take(&mut self, rate_per_sec: u64, burst: u64) -> bool {
        let now = Instant::now();
        let refill = now
            .duration_since(self.last_refill)
            .as_nanos()
            .saturating_mul(u128::from(rate_per_sec));
        let capacity = u128::from(burst).saturating_mul(TOKEN_UNIT);
        self.tokens = self.tokens.saturating_add(refill).min(capacity);
        self.last_refill = now;
        if self.tokens < TOKEN_UNIT {
            return false;
        }
        self.tokens -= TOKEN_UNIT;
        true
    }
}

/// One process-local producer plus its bounded worker supervisor.
pub struct PricingShadowRuntime {
    config: PricingShadowConfig,
    manifest: PricingRuntimeManifestEvidence,
    sender: Mutex<Option<mpsc::Sender<PricingShadowWorkItem>>>,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    producer_open: AtomicBool,
    bucket: Mutex<TokenBucket>,
    metrics: Arc<Metrics>,
    conflict_alerted: Arc<AtomicBool>,
}

impl PricingShadowRuntime {
    pub fn start(
        config: PricingShadowConfig,
        manifest: PricingRuntimeManifestEvidence,
        billing: Option<Arc<AsyncBilling>>,
        metrics: Arc<Metrics>,
    ) -> Result<Arc<Self>> {
        let runtime = Arc::new(Self {
            config,
            manifest,
            sender: Mutex::new(None),
            worker: Mutex::new(None),
            producer_open: AtomicBool::new(false),
            bucket: Mutex::new(TokenBucket::new(config.rate_burst())),
            metrics,
            conflict_alerted: Arc::new(AtomicBool::new(false)),
        });
        if !config.enabled() {
            return Ok(runtime);
        }

        let billing = billing.context("enabled pricing shadow requires billing authority")?;
        if !billing.pricing_shadow_readers_enabled() {
            bail!("enabled pricing shadow requires dedicated PostgreSQL read actors");
        }
        tokio::runtime::Handle::try_current()
            .context("enabled pricing shadow requires an active Tokio runtime")?;
        let (sender, receiver) = mpsc::channel(config.queue_capacity());
        *runtime
            .sender
            .lock()
            .map_err(|_| anyhow::anyhow!("pricing shadow sender lock poisoned"))? = Some(sender);
        runtime.producer_open.store(true, Ordering::Release);

        let metrics_for_worker = Arc::clone(&runtime.metrics);
        let conflict_alerted = Arc::clone(&runtime.conflict_alerted);
        let worker = tokio::spawn(run_workers(
            receiver,
            billing,
            config,
            metrics_for_worker,
            conflict_alerted,
        ));
        *runtime
            .worker
            .lock()
            .map_err(|_| anyhow::anyhow!("pricing shadow worker lock poisoned"))? = Some(worker);
        Ok(runtime)
    }

    /// Request-path producer: deterministic sample, bounded validation and exactly one try_send.
    pub fn try_enqueue(
        &self,
        snapshot: &LegacyScalarAdmissionSnapshot,
    ) -> PricingShadowEnqueueResult {
        let provider = snapshot.provider();
        if !self.config.enabled() || !self.producer_open.load(Ordering::Acquire) {
            return self.record_enqueue(provider, PricingShadowEnqueueResult::Disabled);
        }
        let Some(request_id) = EnginePricingRequestId::from_engine_uuid_v4(snapshot.request_id())
        else {
            return self.record_enqueue(provider, PricingShadowEnqueueResult::InvalidActual);
        };
        if shadow_sampler_bucket_v1(provider, &request_id) >= self.config.sample_bp() {
            return self.record_enqueue(provider, PricingShadowEnqueueResult::NotSampled);
        }
        match work_item_size(snapshot, &self.manifest, self.config.max_field_bytes()) {
            WorkItemSize::OversizedField => {
                return self.record_enqueue(provider, PricingShadowEnqueueResult::OversizedField)
            }
            WorkItemSize::Bytes(bytes) if bytes > self.config.max_item_bytes() => {
                return self.record_enqueue(provider, PricingShadowEnqueueResult::OversizedItem)
            }
            WorkItemSize::Bytes(_) => {}
        }
        let enqueued_ts = pool::now();
        if let Err(error) =
            ShadowActualSnapshotRef::validate_snapshot_shadow_eligibility(snapshot, enqueued_ts)
        {
            let result = match error {
                ShadowEligibilityError::BalanceCappedActual => {
                    PricingShadowEnqueueResult::BalanceCapped
                }
                ShadowEligibilityError::InvalidEnqueueTimestamp
                | ShadowEligibilityError::EnqueuedBeforeAdmission => {
                    PricingShadowEnqueueResult::InvalidTimestamp
                }
                ShadowEligibilityError::InvalidActualSnapshot
                | ShadowEligibilityError::InvalidActualAmount => {
                    PricingShadowEnqueueResult::InvalidActual
                }
            };
            return self.record_enqueue(provider, result);
        }
        let rate_allowed = self.bucket.lock().is_ok_and(|mut bucket| {
            bucket.take(self.config.rate_per_sec(), self.config.rate_burst())
        });
        if !rate_allowed {
            return self.record_enqueue(provider, PricingShadowEnqueueResult::RateLimited);
        }

        let work = match PricingShadowWorkItem::new(snapshot, self.manifest.clone(), enqueued_ts) {
            Ok(work) => work,
            Err(PricingShadowWorkItemError::BalanceCappedActual) => {
                return self.record_enqueue(provider, PricingShadowEnqueueResult::BalanceCapped)
            }
            Err(
                PricingShadowWorkItemError::InvalidEnqueueTimestamp
                | PricingShadowWorkItemError::EnqueuedBeforeAdmission,
            ) => {
                return self.record_enqueue(provider, PricingShadowEnqueueResult::InvalidTimestamp)
            }
            Err(
                PricingShadowWorkItemError::InvalidActualSnapshot
                | PricingShadowWorkItemError::InvalidActualAmount,
            ) => return self.record_enqueue(provider, PricingShadowEnqueueResult::InvalidActual),
        };

        let result = match self.sender.lock() {
            Ok(sender) => match sender.as_ref() {
                Some(sender) => {
                    let accepted_depth = self.metrics.pricing_shadow_queue_try_begin();
                    match sender.try_send(work) {
                        Ok(()) => {
                            self.metrics.pricing_shadow_queue_accepted(accepted_depth);
                            PricingShadowEnqueueResult::Accepted
                        }
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            self.metrics.pricing_shadow_queue_rejected();
                            PricingShadowEnqueueResult::QueueFull
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            self.metrics.pricing_shadow_queue_rejected();
                            PricingShadowEnqueueResult::QueueClosed
                        }
                    }
                }
                None => PricingShadowEnqueueResult::QueueClosed,
            },
            Err(_) => PricingShadowEnqueueResult::QueueClosed,
        };
        self.record_enqueue(provider, result)
    }

    fn record_enqueue(
        &self,
        provider: SnapshotProvider,
        result: PricingShadowEnqueueResult,
    ) -> PricingShadowEnqueueResult {
        self.metrics.pricing_shadow_enqueue(provider, result);
        result
    }

    pub const fn config(&self) -> PricingShadowConfig {
        self.config
    }

    pub fn manifest(&self) -> &PricingRuntimeManifestEvidence {
        &self.manifest
    }

    /// Close admission, drain queued work, and abort only if the shared shutdown deadline expires.
    pub async fn shutdown_until(&self, deadline: Option<tokio::time::Instant>) {
        self.producer_open.store(false, Ordering::Release);
        if let Ok(mut sender) = self.sender.lock() {
            sender.take();
        }
        let mut worker = match self.worker.lock() {
            Ok(mut slot) => slot.take(),
            Err(_) => None,
        };
        let Some(mut worker) = worker.take() else {
            return;
        };
        let completed = match deadline {
            Some(deadline) => tokio::time::timeout_at(deadline, &mut worker).await.is_ok(),
            None => {
                let _ = (&mut worker).await;
                true
            }
        };
        if !completed {
            worker.abort();
            let _ = worker.await;
        }
    }
}

enum WorkItemSize {
    Bytes(usize),
    OversizedField,
}

fn work_item_size(
    snapshot: &LegacyScalarAdmissionSnapshot,
    manifest: &PricingRuntimeManifestEvidence,
    max_field_bytes: usize,
) -> WorkItemSize {
    let fields = [
        snapshot.request_id(),
        snapshot.account_id(),
        snapshot.requested_model_id(),
        snapshot.canonical_model_id(),
        snapshot.snapshot_digest().as_str(),
        manifest.manifest_digest(),
    ];
    if fields.iter().any(|field| field.len() > max_field_bytes)
        || manifest
            .capabilities()
            .iter()
            .any(|capability| capability.capability_digest().len() > max_field_bytes)
    {
        return WorkItemSize::OversizedField;
    }
    let mut bytes = 12usize.saturating_mul(std::mem::size_of::<i64>());
    for field in fields {
        bytes = bytes.saturating_add(field.len());
    }
    for capability in manifest.capabilities() {
        bytes = bytes
            .saturating_add(2 * std::mem::size_of::<i64>())
            .saturating_add(capability.capability_digest().len());
    }
    WorkItemSize::Bytes(bytes)
}

fn shadow_sampler_bucket_v1(
    provider: SnapshotProvider,
    request_id: &EnginePricingRequestId,
) -> u16 {
    fn field(hasher: &mut Sha256, value: &[u8]) {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value);
    }
    let mut hasher = Sha256::new();
    hasher.update(SHADOW_SAMPLER_DOMAIN_V1);
    field(&mut hasher, provider.as_str().as_bytes());
    field(&mut hasher, request_id.as_str().as_bytes());
    let digest = hasher.finalize();
    let value = u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("SHA-256 prefix is eight bytes"),
    );
    (value % u64::from(SAMPLER_BUCKETS)) as u16
}

async fn run_workers(
    receiver: mpsc::Receiver<PricingShadowWorkItem>,
    billing: Arc<AsyncBilling>,
    config: PricingShadowConfig,
    metrics: Arc<Metrics>,
    conflict_alerted: Arc<AtomicBool>,
) {
    let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
    let mut workers = tokio::task::JoinSet::new();
    for _ in 0..config.worker_concurrency() {
        let receiver = Arc::clone(&receiver);
        let billing = Arc::clone(&billing);
        let metrics = Arc::clone(&metrics);
        let conflict_alerted = Arc::clone(&conflict_alerted);
        workers.spawn(async move {
            loop {
                let work = {
                    let mut receiver = receiver.lock().await;
                    receiver.recv().await
                };
                let Some(work) = work else {
                    break;
                };
                metrics.pricing_shadow_queue_started();
                let provider = work.provider();
                let mut cancellation = ProcessingCancellationGuard::new(&metrics, provider);
                evaluate_work(work, &billing, config, &metrics, &conflict_alerted).await;
                cancellation.complete();
            }
        });
    }
    while workers.join_next().await.is_some() {}
}

/// Accounts for a worker future that is aborted by the shared shutdown deadline. The guard owns
/// only a fixed provider enum, so cancellation never invents or exposes a customer identity.
struct ProcessingCancellationGuard<'a> {
    metrics: &'a Metrics,
    provider: SnapshotProvider,
    completed: bool,
}

impl<'a> ProcessingCancellationGuard<'a> {
    fn new(metrics: &'a Metrics, provider: SnapshotProvider) -> Self {
        Self {
            metrics,
            provider,
            completed: false,
        }
    }

    fn complete(&mut self) {
        self.completed = true;
    }
}

impl Drop for ProcessingCancellationGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.metrics
                .pricing_shadow_processing(self.provider, PricingShadowProcessingResult::Cancelled);
        }
    }
}

async fn evaluate_work(
    work: PricingShadowWorkItem,
    billing: &AsyncBilling,
    config: PricingShadowConfig,
    metrics: &Metrics,
    conflict_alerted: &AtomicBool,
) {
    let provider = work.provider();
    let now = pool::now();
    let Some(queue_age) = now.checked_sub(work.enqueued_ts()) else {
        metrics.pricing_shadow_processing(provider, PricingShadowProcessingResult::ClockError);
        return;
    };
    if queue_age < 0 {
        metrics.pricing_shadow_processing(provider, PricingShadowProcessingResult::ClockError);
        return;
    }
    metrics.observe_pricing_shadow_queue_age(provider, queue_age as u64);
    if queue_age >= config.max_queue_age_secs() {
        metrics.pricing_shadow_processing(provider, PricingShadowProcessingResult::Expired);
        return;
    }

    let timeout = Duration::from_millis(config.timeout_ms());
    let read = bounded_operation(
        timeout,
        billing.pricing_shadow_read_bundle(work.account_id(), config.timeout_ms()),
    )
    .await;
    let evaluated_ts = pool::now().max(work.enqueued_ts());
    let diagnostic = match ShadowDiagnosticContext::new(json!({})) {
        Ok(diagnostic) => diagnostic,
        Err(_) => {
            metrics.pricing_shadow_processing(provider, PricingShadowProcessingResult::BuildError);
            return;
        }
    };
    let input = match read {
        BoundedOperation::Completed(Ok(bundle)) => build_pricing_shadow_evaluation(
            work,
            PricingShadowEvaluationSource::Bundle(&bundle),
            evaluated_ts,
            diagnostic,
        ),
        BoundedOperation::Completed(Err(error)) => {
            let (processing, failure) = if registry::pg::is_statement_or_lock_timeout(&error) {
                (
                    PricingShadowProcessingResult::ReadTimeout,
                    PricingShadowReadFailure::EvaluationTimeout,
                )
            } else {
                (
                    PricingShadowProcessingResult::ReadError,
                    PricingShadowReadFailure::PricingReadFailed,
                )
            };
            metrics.pricing_shadow_processing(provider, processing);
            build_pricing_shadow_evaluation(
                work,
                PricingShadowEvaluationSource::ReadFailure(failure),
                evaluated_ts,
                diagnostic,
            )
        }
        BoundedOperation::TimedOut => {
            metrics.pricing_shadow_processing(provider, PricingShadowProcessingResult::ReadTimeout);
            build_pricing_shadow_evaluation(
                work,
                PricingShadowEvaluationSource::ReadFailure(
                    PricingShadowReadFailure::EvaluationTimeout,
                ),
                evaluated_ts,
                diagnostic,
            )
        }
    };
    let input = match input {
        Ok(input) => input,
        Err(_) => {
            metrics.pricing_shadow_processing(provider, PricingShadowProcessingResult::BuildError);
            return;
        }
    };
    metrics.pricing_shadow_outcome(provider, input.outcome());

    match bounded_operation(
        timeout,
        billing.insert_pricing_shadow_evaluation(input, config.timeout_ms()),
    )
    .await
    {
        BoundedOperation::Completed(Ok(PricingShadowEvaluationWrite::Inserted(_))) => {
            metrics.pricing_shadow_processing(provider, PricingShadowProcessingResult::Inserted)
        }
        BoundedOperation::Completed(Ok(PricingShadowEvaluationWrite::Unchanged(_))) => {
            metrics.pricing_shadow_processing(provider, PricingShadowProcessingResult::Replayed)
        }
        BoundedOperation::Completed(Ok(PricingShadowEvaluationWrite::Conflict(_))) => {
            metrics.pricing_shadow_processing(provider, PricingShadowProcessingResult::Conflict);
            if !conflict_alerted.swap(true, Ordering::AcqRel) {
                elog::error("pricing", "pricing shadow invariant alert: semantic idempotency conflict");
            }
        }
        BoundedOperation::Completed(Err(error)) => {
            let result = if registry::pg::is_statement_or_lock_timeout(&error) {
                PricingShadowProcessingResult::WriteTimeout
            } else {
                PricingShadowProcessingResult::WriteError
            };
            metrics.pricing_shadow_processing(provider, result)
        }
        BoundedOperation::TimedOut => {
            metrics.pricing_shadow_processing(provider, PricingShadowProcessingResult::WriteTimeout)
        }
    }
}

enum BoundedOperation<T> {
    Completed(T),
    TimedOut,
}

async fn bounded_operation<F>(timeout: Duration, future: F) -> BoundedOperation<F::Output>
where
    F: Future,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(value) => BoundedOperation::Completed(value),
        Err(_) => BoundedOperation::TimedOut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use registry::pricing::{
        LegacyPremiumModifiers, LegacyScalarAdmissionSnapshotInput, PricingPolicySnapshot,
        PricingReadBundle, PricingRuntimeCapabilityEvidence, PricingShadowEvaluationOutcome,
        PricingShadowReadErrorCode, SnapshotAnthropicInferenceGeo, SnapshotAnthropicSpeed,
        SnapshotGeminiContextRate, SnapshotGeminiSearchBilling,
    };
    use std::sync::atomic::AtomicU64;

    static NEXT_DB: AtomicU64 = AtomicU64::new(0);

    fn manifest() -> PricingRuntimeManifestEvidence {
        PricingRuntimeManifestEvidence::new(
            1,
            vec![PricingRuntimeCapabilityEvidence::new(1, 1, "capability-v1").unwrap()],
        )
        .unwrap()
    }

    fn snapshot(
        request_id: &str,
        account_id: &str,
        requested_model_id: &str,
        canonical_model_id: &str,
        charged_hold_nano: i64,
    ) -> LegacyScalarAdmissionSnapshot {
        let now = pool::now();
        snapshot_at(
            request_id,
            account_id,
            requested_model_id,
            canonical_model_id,
            charged_hold_nano,
            now,
        )
    }

    fn snapshot_at(
        request_id: &str,
        account_id: &str,
        requested_model_id: &str,
        canonical_model_id: &str,
        charged_hold_nano: i64,
        admission_ts: i64,
    ) -> LegacyScalarAdmissionSnapshot {
        LegacyScalarAdmissionSnapshot::new(LegacyScalarAdmissionSnapshotInput {
            request_id: request_id.to_owned(),
            account_id: account_id.to_owned(),
            provider: SnapshotProvider::Anthropic,
            requested_model_id: requested_model_id.to_owned(),
            canonical_model_id: canonical_model_id.to_owned(),
            alias_generation: 1,
            tariff_schedule_id: "anthropic-v1".to_owned(),
            tariff_priced_ts: admission_ts,
            admission_ts,
            payable_multiplier_bp: 8_000,
            official_hold_nano: 1_000,
            charged_hold_nano,
            premium_modifiers: LegacyPremiumModifiers::AnthropicV1 {
                speed: SnapshotAnthropicSpeed::Standard,
                inference_geo: SnapshotAnthropicInferenceGeo::Global,
                inference_geo_basis_points: 10_000,
            },
        })
        .unwrap()
    }

    fn google_snapshot(request_id: &str) -> LegacyScalarAdmissionSnapshot {
        let now = pool::now();
        LegacyScalarAdmissionSnapshot::new(LegacyScalarAdmissionSnapshotInput {
            request_id: request_id.to_owned(),
            account_id: "google-shadow-account".to_owned(),
            provider: SnapshotProvider::Google,
            requested_model_id: "gemini-3.6-flash".to_owned(),
            canonical_model_id: "gemini-3.6-flash".to_owned(),
            alias_generation: 1,
            tariff_schedule_id: metering::gemini::TARIFF_SCHEDULE_ID.to_owned(),
            tariff_priced_ts: now,
            admission_ts: now,
            payable_multiplier_bp: 8_000,
            official_hold_nano: 1_000,
            charged_hold_nano: 800,
            premium_modifiers: LegacyPremiumModifiers::GeminiV1 {
                context_rate: SnapshotGeminiContextRate::ConservativeMaximum,
                search_billing: SnapshotGeminiSearchBilling::PerQuery,
                grounding_enabled: true,
                search_reserve_units: 32,
            },
        })
        .unwrap()
    }

    fn sqlite_path(label: &str) -> String {
        let id = NEXT_DB.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!(
                "claude-api-pricing-shadow-runtime-{label}-{}-{id}.db",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned()
    }

    fn enabled_values() -> PricingShadowConfigValues {
        PricingShadowConfigValues {
            enabled: true,
            sample_bp: 10_000,
            ..PricingShadowConfigValues::default()
        }
    }

    fn producer_runtime(
        config: PricingShadowConfig,
        sender: Option<mpsc::Sender<PricingShadowWorkItem>>,
        metrics: Arc<Metrics>,
    ) -> PricingShadowRuntime {
        PricingShadowRuntime {
            config,
            manifest: manifest(),
            sender: Mutex::new(sender),
            worker: Mutex::new(None),
            producer_open: AtomicBool::new(config.enabled()),
            bucket: Mutex::new(TokenBucket::new(config.rate_burst())),
            metrics,
            conflict_alerted: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn config_is_default_off_and_rejects_unbounded_or_incoherent_values() {
        let config = PricingShadowConfig::default();
        assert!(!config.enabled());
        assert_eq!(config.sample_bp(), 0);
        assert!(config.max_queue_age_secs() < LEGACY_SCALAR_REPLAY_MAX_AGE_SECS);

        let mut values = PricingShadowConfigValues {
            enabled: true,
            ..PricingShadowConfigValues::default()
        };
        assert_eq!(
            PricingShadowConfig::from_values(values),
            Err(PricingShadowConfigError::EnabledWithoutSample)
        );
        values.sample_bp = 1;
        assert!(PricingShadowConfig::from_values(values).is_ok());
        values.max_queue_age_secs = LEGACY_SCALAR_REPLAY_MAX_AGE_SECS;
        assert_eq!(
            PricingShadowConfig::from_values(values),
            Err(PricingShadowConfigError::QueueAgeOutOfRange)
        );
        values = PricingShadowConfigValues::default();
        values.queue_capacity = 1;
        values.worker_concurrency = 2;
        assert_eq!(
            PricingShadowConfig::from_values(values),
            Err(PricingShadowConfigError::WorkerConcurrencyOutOfRange)
        );
    }

    #[test]
    fn token_bucket_is_bounded_and_refills_without_float_arithmetic() {
        let mut bucket = TokenBucket::new(2);
        assert!(bucket.take(1, 2));
        assert!(bucket.take(1, 2));
        assert!(!bucket.take(1, 2));
        bucket.last_refill = Instant::now() - Duration::from_secs(1);
        assert!(bucket.take(1, 2));
    }

    #[test]
    fn default_off_producer_never_enqueues() {
        let (sender, mut receiver) = mpsc::channel(1);
        let metrics = Arc::new(Metrics::new());
        let runtime = producer_runtime(
            PricingShadowConfig::default(),
            Some(sender),
            metrics.clone(),
        );
        let snapshot = snapshot(
            "123e4567-e89b-42d3-a456-426614174000",
            "account",
            "claude-sonnet-4-6",
            "claude-sonnet-4-6",
            800,
        );
        assert_eq!(
            runtime.try_enqueue(&snapshot),
            PricingShadowEnqueueResult::Disabled
        );
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert_eq!(
            metrics.pricing_shadow_enqueue_count(
                SnapshotProvider::Anthropic,
                PricingShadowEnqueueResult::Disabled,
            ),
            1
        );
    }

    #[test]
    fn google_snapshot_uses_the_same_bounded_enqueue_path() {
        let config = PricingShadowConfig::from_values(enabled_values()).unwrap();
        let metrics = Arc::new(Metrics::new());
        let (sender, mut receiver) = mpsc::channel(1);
        let runtime = producer_runtime(config, Some(sender), Arc::clone(&metrics));
        let snapshot = google_snapshot("123e4567-e89b-42d3-a456-426614174000");

        assert_eq!(
            runtime.try_enqueue(&snapshot),
            PricingShadowEnqueueResult::Accepted
        );
        let work = receiver.try_recv().unwrap();
        assert_eq!(work.provider(), SnapshotProvider::Google);
        assert_eq!(work.account_id(), "google-shadow-account");
        assert_eq!(
            metrics.pricing_shadow_enqueue_count(
                SnapshotProvider::Google,
                PricingShadowEnqueueResult::Accepted,
            ),
            1
        );
    }

    #[test]
    fn producer_uses_one_try_send_and_classifies_full_and_closed_queues() {
        let mut values = enabled_values();
        values.queue_capacity = 1;
        values.worker_concurrency = 1;
        let config = PricingShadowConfig::from_values(values).unwrap();
        let snapshot = snapshot(
            "123e4567-e89b-42d3-a456-426614174000",
            "account",
            "claude-sonnet-4-6",
            "claude-sonnet-4-6",
            800,
        );
        let (sender, receiver) = mpsc::channel(1);
        let metrics = Arc::new(Metrics::new());
        let runtime = producer_runtime(config, Some(sender), metrics.clone());
        assert_eq!(
            runtime.try_enqueue(&snapshot),
            PricingShadowEnqueueResult::Accepted
        );
        assert_eq!(
            runtime.try_enqueue(&snapshot),
            PricingShadowEnqueueResult::QueueFull
        );
        drop(receiver);
        assert_eq!(
            runtime.try_enqueue(&snapshot),
            PricingShadowEnqueueResult::QueueClosed
        );
        assert_eq!(metrics.pricing_shadow_queue_depth(), 1);
    }

    #[test]
    fn producer_applies_sample_rate_size_and_accepts_funding_cap_before_enqueue() {
        let base_snapshot = snapshot(
            "123e4567-e89b-42d3-a456-426614174000",
            "account",
            "claude-sonnet-4-6",
            "claude-sonnet-4-6",
            800,
        );
        let request =
            EnginePricingRequestId::from_engine_uuid_v4(base_snapshot.request_id()).unwrap();
        let bucket = shadow_sampler_bucket_v1(SnapshotProvider::Anthropic, &request);
        assert!(bucket > 0, "fixture must have a non-zero bucket");
        let mut values = enabled_values();
        values.sample_bp = i64::from(bucket);
        let (sender, _receiver) = mpsc::channel(values.queue_capacity);
        let runtime = producer_runtime(
            PricingShadowConfig::from_values(values).unwrap(),
            Some(sender),
            Arc::new(Metrics::new()),
        );
        assert_eq!(
            runtime.try_enqueue(&base_snapshot),
            PricingShadowEnqueueResult::NotSampled
        );

        values = enabled_values();
        values.max_field_bytes = 64;
        let oversized_field = snapshot(
            "123e4567-e89b-42d3-a456-426614174001",
            &"a".repeat(65),
            "claude-sonnet-4-6",
            "claude-sonnet-4-6",
            800,
        );
        let (sender, _receiver) = mpsc::channel(values.queue_capacity);
        let runtime = producer_runtime(
            PricingShadowConfig::from_values(values).unwrap(),
            Some(sender),
            Arc::new(Metrics::new()),
        );
        assert_eq!(
            runtime.try_enqueue(&oversized_field),
            PricingShadowEnqueueResult::OversizedField
        );

        values = enabled_values();
        values.max_item_bytes = 1_024;
        let oversized_item = snapshot(
            "123e4567-e89b-42d3-a456-426614174002",
            &"a".repeat(400),
            &"b".repeat(400),
            &"c".repeat(400),
            800,
        );
        let (sender, _receiver) = mpsc::channel(values.queue_capacity);
        let runtime = producer_runtime(
            PricingShadowConfig::from_values(values).unwrap(),
            Some(sender),
            Arc::new(Metrics::new()),
        );
        assert_eq!(
            runtime.try_enqueue(&oversized_item),
            PricingShadowEnqueueResult::OversizedItem
        );

        let capped = snapshot(
            "123e4567-e89b-42d3-a456-426614174003",
            "account",
            "claude-sonnet-4-6",
            "claude-sonnet-4-6",
            799,
        );
        let (sender, _receiver) = mpsc::channel(256);
        let runtime = producer_runtime(
            PricingShadowConfig::from_values(enabled_values()).unwrap(),
            Some(sender),
            Arc::new(Metrics::new()),
        );
        assert_eq!(
            runtime.try_enqueue(&capped),
            PricingShadowEnqueueResult::Accepted
        );
    }

    #[test]
    fn producer_token_bucket_drops_without_queue_backpressure() {
        let mut values = enabled_values();
        values.rate_per_sec = 1;
        values.rate_burst = 1;
        let (sender, _receiver) = mpsc::channel(values.queue_capacity);
        let runtime = producer_runtime(
            PricingShadowConfig::from_values(values).unwrap(),
            Some(sender),
            Arc::new(Metrics::new()),
        );
        let valid_snapshot = snapshot(
            "123e4567-e89b-42d3-a456-426614174000",
            "account",
            "claude-sonnet-4-6",
            "claude-sonnet-4-6",
            800,
        );
        assert_eq!(
            runtime.try_enqueue(&valid_snapshot),
            PricingShadowEnqueueResult::Accepted
        );
        assert_eq!(
            runtime.try_enqueue(&valid_snapshot),
            PricingShadowEnqueueResult::RateLimited
        );
        let capped = snapshot(
            "123e4567-e89b-42d3-a456-426614174004",
            "account",
            "claude-sonnet-4-6",
            "claude-sonnet-4-6",
            799,
        );
        assert_eq!(
            runtime.try_enqueue(&capped),
            PricingShadowEnqueueResult::RateLimited,
            "funding-capped snapshots remain eligible and consume the ordinary rate budget"
        );
    }

    #[tokio::test]
    async fn expired_work_is_dropped_before_any_authority_operation() {
        let path = sqlite_path("expired");
        let billing = AsyncBilling::start(path, 1).unwrap();
        let config = PricingShadowConfig::from_values(enabled_values()).unwrap();
        let enqueued_ts = pool::now() - config.max_queue_age_secs();
        let snapshot = snapshot_at(
            "123e4567-e89b-42d3-a456-426614174010",
            "account",
            "claude-sonnet-4-6",
            "claude-sonnet-4-6",
            800,
            enqueued_ts,
        );
        let work = PricingShadowWorkItem::new(&snapshot, manifest(), enqueued_ts).unwrap();
        let metrics = Metrics::new();
        evaluate_work(work, &billing, config, &metrics, &AtomicBool::new(false)).await;
        assert_eq!(
            metrics.pricing_shadow_processing_count(
                SnapshotProvider::Anthropic,
                PricingShadowProcessingResult::Expired,
            ),
            1
        );
        assert_eq!(
            metrics.pricing_shadow_processing_count(
                SnapshotProvider::Anthropic,
                PricingShadowProcessingResult::ReadError,
            ),
            0
        );
    }

    #[tokio::test]
    async fn operation_timeout_cancels_the_async_wait_budget() {
        let result =
            bounded_operation(Duration::from_millis(5), std::future::pending::<()>()).await;
        assert!(matches!(result, BoundedOperation::TimedOut));
        let completed = bounded_operation(Duration::from_secs(1), async { 7 }).await;
        assert!(matches!(completed, BoundedOperation::Completed(7)));
    }

    #[test]
    fn aborted_worker_guard_records_only_the_fixed_provider_cancellation() {
        let metrics = Metrics::new();
        {
            let _guard = ProcessingCancellationGuard::new(&metrics, SnapshotProvider::Anthropic);
        }
        assert_eq!(
            metrics.pricing_shadow_processing_count(
                SnapshotProvider::Anthropic,
                PricingShadowProcessingResult::Cancelled,
            ),
            1
        );
        assert_eq!(
            metrics.pricing_shadow_processing_count(
                SnapshotProvider::OpenAi,
                PricingShadowProcessingResult::Cancelled,
            ),
            0
        );

        let mut completed = ProcessingCancellationGuard::new(&metrics, SnapshotProvider::OpenAi);
        completed.complete();
        drop(completed);
        assert_eq!(
            metrics.pricing_shadow_processing_count(
                SnapshotProvider::OpenAi,
                PricingShadowProcessingResult::Cancelled,
            ),
            0
        );
    }

    #[tokio::test]
    async fn read_failure_is_durable_replayed_and_never_changes_reserved_money() {
        let path = sqlite_path("read-error-replay");
        let account_id = "shadow-runtime-account";
        let key = "shadow-runtime-key";
        {
            let conn = registry::open(&path).unwrap();
            registry::account_create(&conn, account_id, None, 8_000).unwrap();
            registry::key_issue(&conn, key, account_id, None).unwrap();
            registry::account_topup(&conn, account_id, 10_000, Some("seed")).unwrap();
        }
        let billing = AsyncBilling::start(path.clone(), 1).unwrap();
        let snapshot = snapshot(
            "123e4567-e89b-42d3-a456-426614174011",
            account_id,
            "claude-sonnet-4-6",
            "claude-sonnet-4-6",
            800,
        );
        assert!(matches!(
            billing
                .reserve_request_with_legacy_snapshot(key, snapshot.clone())
                .await
                .unwrap(),
            registry::pricing::LegacyScalarReserveOutcome::Inserted(_)
        ));
        let balance_after_reserve = billing
            .account(account_id)
            .await
            .unwrap()
            .unwrap()
            .balance_nano;
        let config = PricingShadowConfig::from_values(enabled_values()).unwrap();
        let metrics = Metrics::new();
        for _ in 0..2 {
            let work = PricingShadowWorkItem::new(&snapshot, manifest(), pool::now()).unwrap();
            evaluate_work(work, &billing, config, &metrics, &AtomicBool::new(false)).await;
        }
        billing.flush().await.unwrap();
        assert_eq!(
            billing
                .account(account_id)
                .await
                .unwrap()
                .unwrap()
                .balance_nano,
            balance_after_reserve
        );
        assert_eq!(
            metrics.pricing_shadow_processing_count(
                SnapshotProvider::Anthropic,
                PricingShadowProcessingResult::Inserted,
            ),
            1
        );
        assert_eq!(
            metrics.pricing_shadow_processing_count(
                SnapshotProvider::Anthropic,
                PricingShadowProcessingResult::Replayed,
            ),
            1
        );
        assert_eq!(
            metrics.pricing_shadow_read_error_count(
                SnapshotProvider::Anthropic,
                PricingShadowReadErrorCode::PricingReadFailed,
            ),
            2
        );
        let conn = registry::open(&path).unwrap();
        let evaluation = registry::pricing::sqlite_pricing_shadow_admission_evaluation(
            &conn,
            snapshot.request_id(),
        )
        .unwrap()
        .unwrap();
        assert!(matches!(
            evaluation.outcome(),
            PricingShadowEvaluationOutcome::ReadError {
                reason: PricingShadowReadErrorCode::PricingReadFailed
            }
        ));
    }

    #[tokio::test]
    async fn closed_producer_queue_drains_before_worker_supervisor_returns() {
        let path = sqlite_path("drain");
        let account_id = "shadow-drain-account";
        let key = "shadow-drain-key";
        {
            let conn = registry::open(&path).unwrap();
            registry::account_create(&conn, account_id, None, 8_000).unwrap();
            registry::key_issue(&conn, key, account_id, None).unwrap();
            registry::account_topup(&conn, account_id, 10_000, Some("seed")).unwrap();
        }
        let billing = Arc::new(AsyncBilling::start(path.clone(), 1).unwrap());
        let snapshot = snapshot(
            "123e4567-e89b-42d3-a456-426614174014",
            account_id,
            "claude-sonnet-4-6",
            "claude-sonnet-4-6",
            800,
        );
        billing
            .reserve_request_with_legacy_snapshot(key, snapshot.clone())
            .await
            .unwrap();
        let config = PricingShadowConfig::from_values(enabled_values()).unwrap();
        let work = PricingShadowWorkItem::new(&snapshot, manifest(), pool::now()).unwrap();
        let (sender, receiver) = mpsc::channel(1);
        sender.send(work).await.unwrap();
        drop(sender);
        let metrics = Arc::new(Metrics::new());
        run_workers(
            receiver,
            billing,
            config,
            metrics.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .await;
        assert_eq!(
            metrics.pricing_shadow_processing_count(
                SnapshotProvider::Anthropic,
                PricingShadowProcessingResult::Inserted,
            ),
            1
        );
        let conn = registry::open(&path).unwrap();
        assert!(
            registry::pricing::sqlite_pricing_shadow_admission_evaluation(
                &conn,
                snapshot.request_id(),
            )
            .unwrap()
            .is_some()
        );
    }

    #[tokio::test]
    async fn missing_actual_is_a_bounded_write_error_without_money_side_effects() {
        let path = sqlite_path("write-error");
        let billing = AsyncBilling::start(path, 1).unwrap();
        let config = PricingShadowConfig::from_values(enabled_values()).unwrap();
        let snapshot = snapshot(
            "123e4567-e89b-42d3-a456-426614174012",
            "missing-account",
            "claude-sonnet-4-6",
            "claude-sonnet-4-6",
            800,
        );
        let work = PricingShadowWorkItem::new(&snapshot, manifest(), pool::now()).unwrap();
        let metrics = Metrics::new();
        evaluate_work(work, &billing, config, &metrics, &AtomicBool::new(false)).await;
        assert_eq!(
            metrics.pricing_shadow_processing_count(
                SnapshotProvider::Anthropic,
                PricingShadowProcessingResult::WriteError,
            ),
            1
        );
        assert!(billing.account("missing-account").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn different_semantic_result_is_conflict_not_update() {
        let path = sqlite_path("conflict");
        let account_id = "shadow-conflict-account";
        let key = "shadow-conflict-key";
        {
            let conn = registry::open(&path).unwrap();
            registry::account_create(&conn, account_id, None, 8_000).unwrap();
            registry::key_issue(&conn, key, account_id, None).unwrap();
            registry::account_topup(&conn, account_id, 10_000, Some("seed")).unwrap();
        }
        let billing = AsyncBilling::start(path.clone(), 1).unwrap();
        let snapshot = snapshot(
            "123e4567-e89b-42d3-a456-426614174013",
            account_id,
            "claude-sonnet-4-6",
            "claude-sonnet-4-6",
            800,
        );
        billing
            .reserve_request_with_legacy_snapshot(key, snapshot.clone())
            .await
            .unwrap();
        let manifest = manifest();
        let enqueued_ts = pool::now();
        let work = PricingShadowWorkItem::new(&snapshot, manifest.clone(), enqueued_ts).unwrap();
        let bundle = PricingReadBundle {
            account_id: account_id.to_owned(),
            account_multiplier_bp: 8_000,
            policy: PricingPolicySnapshot::Unbound,
            policy_catalog: None,
            policy_switches: None,
            admission_catalog: None,
            admission_switches: None,
        };
        let input = build_pricing_shadow_evaluation(
            work,
            PricingShadowEvaluationSource::Bundle(&bundle),
            enqueued_ts,
            ShadowDiagnosticContext::new(json!({})).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            billing
                .insert_pricing_shadow_evaluation(input, 750)
                .await
                .unwrap(),
            PricingShadowEvaluationWrite::Inserted(_)
        ));

        let metrics = Metrics::new();
        let work = PricingShadowWorkItem::new(&snapshot, manifest, pool::now()).unwrap();
        evaluate_work(
            work,
            &billing,
            PricingShadowConfig::from_values(enabled_values()).unwrap(),
            &metrics,
            &AtomicBool::new(false),
        )
        .await;
        assert_eq!(
            metrics.pricing_shadow_processing_count(
                SnapshotProvider::Anthropic,
                PricingShadowProcessingResult::Conflict,
            ),
            1
        );
        let conn = registry::open(&path).unwrap();
        let evaluation = registry::pricing::sqlite_pricing_shadow_admission_evaluation(
            &conn,
            snapshot.request_id(),
        )
        .unwrap()
        .unwrap();
        assert!(matches!(
            evaluation.outcome(),
            PricingShadowEvaluationOutcome::Rejected { .. }
        ));
    }

    #[test]
    fn sampler_is_stable_and_provider_separated() {
        let request =
            EnginePricingRequestId::from_engine_uuid_v4("123e4567-e89b-42d3-a456-426614174000")
                .unwrap();
        let anthropic = shadow_sampler_bucket_v1(SnapshotProvider::Anthropic, &request);
        let openai = shadow_sampler_bucket_v1(SnapshotProvider::OpenAi, &request);
        let google = shadow_sampler_bucket_v1(SnapshotProvider::Google, &request);
        assert_eq!(
            anthropic,
            shadow_sampler_bucket_v1(SnapshotProvider::Anthropic, &request)
        );
        assert_ne!(anthropic, openai);
        assert_ne!(anthropic, google);
        assert_ne!(openai, google);
        assert!(anthropic < 10_000);
        assert!(openai < 10_000);
        assert!(google < 10_000);
    }

    #[test]
    fn enqueue_and_processing_reason_sets_are_fixed_and_unique() {
        let mut enqueue = std::collections::BTreeSet::new();
        for reason in PricingShadowEnqueueResult::ALL {
            assert!(enqueue.insert(reason.code()));
        }
        let mut processing = std::collections::BTreeSet::new();
        for reason in PricingShadowProcessingResult::ALL {
            assert!(processing.insert(reason.code()));
        }
    }
}
