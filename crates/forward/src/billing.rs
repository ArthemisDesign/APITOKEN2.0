//! Асинхронный биллинг поверх синхронного `registry` — БЕЗ блокировки async-воркеров.
//!
//! Проблема: rusqlite синхронна. Вызвать её в axum-хендлере = заблокировать tokio-воркер на время
//! запроса к БД; под нагрузкой воркеры встают → рантайм застревает.
//!
//! Решение (акторы + пул): ВЫДЕЛЕННЫЕ OS-потоки владеют соединениями и крутят блокирующий цикл
//! `blocking_recv`. Async-код шлёт команду в `mpsc` и `.await`-ит `oneshot`-ответ — воркеры не
//! блокируются ни на миг. Разделение под природу SQLite (single-writer, multi-reader на WAL):
//!   • ОДИН writer-поток — reserve/settle/topup. Записи сериализуются им идеально, без SQLITE_BUSY.
//!   • N reader-потоков (каждый со СВОИМ read-соединением) — key_auth/account/get/totals. WAL пускает
//!     параллельные чтения → key_auth (на КАЖДОМ запросе) масштабируется линейно по числу читателей.
//! Раздача чтений — round-robin по N каналам (без общего мьютекса на приём).
//!
//! RAII-возвраты (`HoldGuard::drop`, `TeeMeter::finalize`) СИНХРОННЫ (Drop не умеет await). Для них
//! `settle_detached` шлёт команду writer'у без ожидания (`mpsc::send` не блокирует и не требует
//! рантайма). Гарантия денег: durable reservation/outbox переживает краш, а периодический recovery
//! закрывает осиротевшую операцию после истечения lease. Ничего не застревает только в памяти.

use registry::pricing::{
    AccountPolicyActivationSpec, AccountPolicySpec, ActiveAccountPolicy, ActiveExpectation,
    LegacyScalarAdmissionSnapshot, LegacyScalarReserveOutcome, LockedOpenKeysPolicyTransitionSpec,
    PolicyActiveExpectation, PolicyAdmissionSnapshot, PolicyReserveOutcome, PricingCatalogSpec,
    PricingMutation, PricingReadBundle, PricingReleaseActivationOutcomeV2,
    PricingReleaseActivationRequestV2, PricingReleaseAssignmentExtensionV2, PricingReleaseHeadV2,
    PricingReleaseInventoryPageV2, PricingReleasePolicyV2, PricingReleaseProvisioningContextV2,
    PricingReleaseQuoteV2, PricingReleaseRecoveryLinkV2, PricingReleaseReserveOutcomeV2,
    PricingReleaseResolutionV2, PricingReleaseV2, PricingRuntimeManifestEvidence,
    PricingShadowAdmissionEvaluationInput, PricingShadowEvaluationWrite, ProviderSwitchSpec,
    VersionTarget,
};
use registry::{
    AccountFundingSnapshot, AccountRow, AnthropicCalibrationRow, AnthropicWindowObservation,
    BillingTotals, CodexCalibrationRow, CodexHomeCalibrationSpend, CodexTurnCalibrationAggregate,
    CodexTurnCalibrationEvent, CodexWindowObservation, FundingNormalizationApplyRequestV2,
    FundingNormalizationApplyResultV2, FundingNormalizationPlanV2, GeminiExactCalibrationRow,
    GeminiExactWindowObservation, GlmCalibrationRow, GlmSubjectSpend, GlmTurnCalibrationEvent,
    GlmWindowObservation, KeyActivationPolicyAck, KeyAuth, KeyPolicyUpdate, KeyRow,
    KimiCalibrationRow, KimiTurnCalibrationEvent, KimiWindowObservation,
    ProviderCalibrationSubjectSpend, ProviderTurnCalibrationAggregate,
    ProviderTurnCalibrationEvent,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, Notify};

/// Billing queues are deliberately bounded. The request path applies async backpressure instead of
/// retaining an arbitrary number of commands while PostgreSQL/SQLite is unavailable.
const WRITE_QUEUE_CAPACITY: usize = 4_096;
const READ_QUEUE_CAPACITY: usize = 1_024;
const PRICING_SHADOW_READ_QUEUE_CAPACITY: usize = 256;
const PG_OPERATION_RETRY_DEADLINE: Duration = Duration::from_secs(5);
const MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS: usize = 4_096;
const MAX_PENDING_GEMINI_CALIBRATION_EVENTS: usize = 4_096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AnthropicQuotaSnapshot {
    pub window_kind: String,
    pub window_duration_mins: i64,
    pub resets_at: i64,
    pub used_fraction_units: i64,
    pub measurement_resolution_fraction_units: i64,
    pub observed_at: i64,
}

type AnthropicTurnPersistenceResult = (
    ProviderCalibrationSubjectSpend,
    Vec<AnthropicCalibrationRow>,
);

#[derive(Clone)]
struct PendingAnthropicCalibrationTurn {
    delivery_id: u64,
    event: ProviderTurnCalibrationEvent,
    plan: String,
    snapshots: Vec<AnthropicQuotaSnapshot>,
}

#[derive(Default)]
struct AnthropicCalibrationDeliveryQueue {
    pending: VecDeque<PendingAnthropicCalibrationTurn>,
    next_delivery_id: u64,
}

struct AnthropicCalibrationDeliveryState {
    queue: std::sync::Mutex<AnthropicCalibrationDeliveryQueue>,
    dropped_events: std::sync::atomic::AtomicU64,
    persistence_ok: AtomicBool,
}

impl Default for AnthropicCalibrationDeliveryState {
    fn default() -> Self {
        Self {
            queue: std::sync::Mutex::new(AnthropicCalibrationDeliveryQueue::default()),
            dropped_events: std::sync::atomic::AtomicU64::new(0),
            persistence_ok: AtomicBool::new(true),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnthropicCalibrationDeliveryStatus {
    pub pending_events: usize,
    pub dropped_events: u64,
    pub persistence_ok: bool,
    pub queue_limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GeminiQuotaSnapshot {
    pub bucket_id: String,
    pub window_kind: String,
    pub window_duration_mins: i64,
    pub resets_at: i64,
    pub used_fraction_units: i64,
    pub measurement_resolution_fraction_units: i64,
    pub observed_at: i64,
}

/// One parsed `/usages` window before the billing writer pairs it with the subject's exact
/// durable cumulative spend. The gateway never supplies that spend: reading it and storing the
/// immutable observation belong to the same serial PostgreSQL writer hop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KimiQuotaSnapshot {
    pub window_duration_secs: i64,
    pub window_name: Option<String>,
    pub resets_at: i64,
    pub observed_at: i64,
    pub native_used_units: i64,
    pub native_limit_units: i64,
    pub used_fraction_units: i64,
    pub measurement_resolution_fraction_units: i64,
}

/// One parsed GLM quota-endpoint window before the writer pairs it with the subject's durable
/// cumulative dual-ledger spend (same single-writer hop as KIMI). Every provider-side value
/// stays raw and optional: the endpoint's counter units are unproven
/// (`docs/engine/GLM_PROVIDER.md` §6.3), so unknown is `None`, never `0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GlmQuotaSnapshot {
    pub window_duration_secs: i64,
    /// `nextResetTime` when the provider supplied it; a rolling window may not name one.
    pub resets_at: Option<i64>,
    pub observed_at: i64,
    pub native_used_units: Option<i64>,
    pub native_limit_units: Option<i64>,
    pub native_remaining_units: Option<i64>,
    /// The provider's own percentage display value at whole-percent granularity. Raw evidence
    /// only — the estimator fraction derives from the used/limit counters, never from this.
    pub percentage_raw: Option<i64>,
    /// Derived fraction pair, present only for the documented credits form.
    pub used_fraction_units: Option<i64>,
    pub measurement_resolution_fraction_units: Option<i64>,
}

type GeminiTurnPersistenceResult = (
    ProviderCalibrationSubjectSpend,
    Vec<GeminiExactCalibrationRow>,
);

#[derive(Clone)]
struct PendingGeminiCalibrationTurn {
    delivery_id: u64,
    event: ProviderTurnCalibrationEvent,
    plan: String,
    snapshots: Vec<GeminiQuotaSnapshot>,
}

#[derive(Default)]
struct GeminiCalibrationDeliveryQueue {
    pending: VecDeque<PendingGeminiCalibrationTurn>,
    next_delivery_id: u64,
}

struct GeminiCalibrationDeliveryState {
    queue: std::sync::Mutex<GeminiCalibrationDeliveryQueue>,
    dropped_events: std::sync::atomic::AtomicU64,
    persistence_ok: AtomicBool,
}

impl Default for GeminiCalibrationDeliveryState {
    fn default() -> Self {
        Self {
            queue: std::sync::Mutex::new(GeminiCalibrationDeliveryQueue::default()),
            dropped_events: std::sync::atomic::AtomicU64::new(0),
            persistence_ok: AtomicBool::new(true),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeminiCalibrationDeliveryStatus {
    pub pending_events: usize,
    pub dropped_events: u64,
    pub persistence_ok: bool,
    pub queue_limit: usize,
}

fn enqueue_gemini_calibration_turn(
    state: &GeminiCalibrationDeliveryState,
    event: ProviderTurnCalibrationEvent,
    plan: String,
    snapshots: Vec<GeminiQuotaSnapshot>,
) -> anyhow::Result<u64> {
    let mut queue = state
        .queue
        .lock()
        .expect("Gemini calibration delivery queue lock");
    if queue.pending.len() >= MAX_PENDING_GEMINI_CALIBRATION_EVENTS {
        state.dropped_events.fetch_add(1, Ordering::Relaxed);
        state.persistence_ok.store(false, Ordering::Relaxed);
        anyhow::bail!(
            "Gemini calibration event queue is full (limit {})",
            MAX_PENDING_GEMINI_CALIBRATION_EVENTS
        );
    }
    queue.next_delivery_id = queue.next_delivery_id.wrapping_add(1).max(1);
    let delivery_id = queue.next_delivery_id;
    queue.pending.push_back(PendingGeminiCalibrationTurn {
        delivery_id,
        event,
        plan,
        snapshots,
    });
    Ok(delivery_id)
}

/// Persist Gemini evidence in strict FIFO order. Ambiguous replies are safe because request ids are
/// immutable idempotency keys; semantic replay conflicts quarantine exactly one corrupt event.
fn flush_pending_gemini_calibration_turns(
    state: &GeminiCalibrationDeliveryState,
    target_delivery_id: Option<u64>,
    mut persist: impl FnMut(
        &PendingGeminiCalibrationTurn,
    ) -> anyhow::Result<GeminiTurnPersistenceResult>,
) -> anyhow::Result<Option<GeminiTurnPersistenceResult>> {
    let mut target_result = None;
    let mut target_conflict = None;
    loop {
        let front = state
            .queue
            .lock()
            .expect("Gemini calibration delivery queue lock")
            .pending
            .front()
            .cloned();
        let Some(front) = front else {
            break;
        };
        let delivery_id = front.delivery_id;
        match persist(&front) {
            Ok(value) => {
                let mut queue = state
                    .queue
                    .lock()
                    .expect("Gemini calibration delivery queue lock");
                if queue
                    .pending
                    .front()
                    .is_some_and(|pending| pending.delivery_id == delivery_id)
                {
                    queue.pending.pop_front();
                }
                if target_delivery_id == Some(delivery_id) {
                    target_result = Some(value);
                }
            }
            Err(error) if registry::is_provider_turn_calibration_replay_conflict(&error) => {
                let mut queue = state
                    .queue
                    .lock()
                    .expect("Gemini calibration delivery queue lock");
                if queue
                    .pending
                    .front()
                    .is_some_and(|pending| pending.delivery_id == delivery_id)
                {
                    queue.pending.pop_front();
                }
                state.dropped_events.fetch_add(1, Ordering::Relaxed);
                state.persistence_ok.store(false, Ordering::Relaxed);
                eprintln!("Gemini calibration event quarantined after immutable replay conflict");
                if target_delivery_id == Some(delivery_id) {
                    target_conflict = Some(error);
                }
            }
            Err(error) => {
                state.persistence_ok.store(false, Ordering::Relaxed);
                return Err(
                    if target_delivery_id.is_some_and(|target| target != delivery_id) {
                        error.context(
                            "Gemini calibration predecessor remains pending; later evidence held",
                        )
                    } else {
                        error
                    },
                );
            }
        }
    }
    if state.dropped_events.load(Ordering::Relaxed) == 0 {
        state.persistence_ok.store(true, Ordering::Relaxed);
    }
    if let Some(error) = target_conflict {
        return Err(error);
    }
    Ok(target_result)
}

fn enqueue_anthropic_calibration_turn(
    state: &AnthropicCalibrationDeliveryState,
    event: ProviderTurnCalibrationEvent,
    plan: String,
    snapshots: Vec<AnthropicQuotaSnapshot>,
) -> anyhow::Result<u64> {
    let mut queue = state
        .queue
        .lock()
        .expect("Anthropic calibration delivery queue lock");
    if queue.pending.len() >= MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS {
        state.dropped_events.fetch_add(1, Ordering::Relaxed);
        state.persistence_ok.store(false, Ordering::Relaxed);
        anyhow::bail!(
            "Anthropic calibration event queue is full (limit {})",
            MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS
        );
    }
    queue.next_delivery_id = queue.next_delivery_id.wrapping_add(1).max(1);
    let delivery_id = queue.next_delivery_id;
    queue.pending.push_back(PendingAnthropicCalibrationTurn {
        delivery_id,
        event,
        plan,
        snapshots,
    });
    Ok(delivery_id)
}

/// Replay exact Claude turn evidence in FIFO order. A transient failure leaves the head in memory,
/// so neither a later turn nor a free poll snapshot can observe quota against stale cumulative
/// spend. Immutable request-id replay makes an ambiguous lost database reply safe. A semantic
/// replay conflict is quarantined instead of blocking every later valid event forever.
fn flush_pending_anthropic_calibration_turns(
    state: &AnthropicCalibrationDeliveryState,
    target_delivery_id: Option<u64>,
    mut persist: impl FnMut(
        &PendingAnthropicCalibrationTurn,
    ) -> anyhow::Result<AnthropicTurnPersistenceResult>,
) -> anyhow::Result<Option<AnthropicTurnPersistenceResult>> {
    let mut target_result = None;
    let mut target_conflict = None;
    loop {
        let front = state
            .queue
            .lock()
            .expect("Anthropic calibration delivery queue lock")
            .pending
            .front()
            .cloned();
        let Some(front) = front else {
            break;
        };
        let delivery_id = front.delivery_id;
        let result = persist(&front);
        match result {
            Ok(value) => {
                let mut queue = state
                    .queue
                    .lock()
                    .expect("Anthropic calibration delivery queue lock");
                if queue
                    .pending
                    .front()
                    .is_some_and(|pending| pending.delivery_id == delivery_id)
                {
                    queue.pending.pop_front();
                }
                if target_delivery_id == Some(delivery_id) {
                    target_result = Some(value);
                }
            }
            Err(error) if registry::is_provider_turn_calibration_replay_conflict(&error) => {
                let mut queue = state
                    .queue
                    .lock()
                    .expect("Anthropic calibration delivery queue lock");
                if queue
                    .pending
                    .front()
                    .is_some_and(|pending| pending.delivery_id == delivery_id)
                {
                    queue.pending.pop_front();
                }
                state.dropped_events.fetch_add(1, Ordering::Relaxed);
                state.persistence_ok.store(false, Ordering::Relaxed);
                eprintln!(
                    "Anthropic calibration event quarantined after immutable replay conflict"
                );
                if target_delivery_id == Some(delivery_id) {
                    target_conflict = Some(error);
                }
            }
            Err(error) => {
                state.persistence_ok.store(false, Ordering::Relaxed);
                return Err(
                    if target_delivery_id.is_some_and(|target| target != delivery_id) {
                        error.context(
                        "Anthropic calibration predecessor remains pending; later evidence held",
                    )
                    } else {
                        error
                    },
                );
            }
        }
    }
    if state.dropped_events.load(Ordering::Relaxed) == 0 {
        state.persistence_ok.store(true, Ordering::Relaxed);
    }
    if let Some(error) = target_conflict {
        return Err(error);
    }
    Ok(target_result)
}

const RESERVE_HANDOFF_PENDING: u8 = 0;
const RESERVE_HANDOFF_COMMITTED: u8 = 1;
const RESERVE_HANDOFF_CLAIMED: u8 = 2;
const RESERVE_HANDOFF_CANCELED: u8 = 3;
const RESERVE_HANDOFF_REFUNDING: u8 = 4;
const RESERVE_HANDOFF_REFUNDED: u8 = 5;
const RESERVE_HANDOFF_FAILED: u8 = 6;

// Snapshot reserve is intentionally a separate protocol from the live legacy reserve above. Once
// its commit decision wins the race with caller cancellation, the durable reservation must remain
// active for exact replay or lease recovery; compensating a lost reply would destroy idempotency.
const SNAPSHOT_RESERVE_HANDOFF_PENDING: u8 = 0;
const SNAPSHOT_RESERVE_HANDOFF_COMMIT_DECIDED: u8 = 1;
const SNAPSHOT_RESERVE_HANDOFF_COMMITTED: u8 = 2;
const SNAPSHOT_RESERVE_HANDOFF_CLAIMED: u8 = 3;
const SNAPSHOT_RESERVE_HANDOFF_CANCELED: u8 = 4;
const SNAPSHOT_RESERVE_HANDOFF_FAILED: u8 = 5;
const SNAPSHOT_RESERVE_HANDOFF_COMMIT_UNKNOWN: u8 = 6;

// Закрывает окно отмены, пока `reserve().await` ещё не передал владение резервом вызывающему коду.
// Компенсация адресует durable request_id, поэтому повторный cancel/settle идемпотентен и не может
// вернуть резерв другого параллельного запроса того же аккаунта.
struct ReserveHandoffGuard<'a> {
    writer: &'a mpsc::Sender<WriteCmd>,
    detached: Arc<DetachedDispatchTracker>,
    request_id: String,
    account_id: String,
    key: String,
    hold: i64,
    handoff: Arc<AtomicU8>,
}

impl ReserveHandoffGuard<'_> {
    fn claim(&self) -> bool {
        self.handoff
            .compare_exchange(
                RESERVE_HANDOFF_COMMITTED,
                RESERVE_HANDOFF_CLAIMED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }
}

impl Drop for ReserveHandoffGuard<'_> {
    fn drop(&mut self) {
        loop {
            match self.handoff.load(Ordering::Acquire) {
                RESERVE_HANDOFF_PENDING => {
                    if self
                        .handoff
                        .compare_exchange(
                            RESERVE_HANDOFF_PENDING,
                            RESERVE_HANDOFF_CANCELED,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        return;
                    }
                }
                RESERVE_HANDOFF_COMMITTED => {
                    if self
                        .handoff
                        .compare_exchange(
                            RESERVE_HANDOFF_COMMITTED,
                            RESERVE_HANDOFF_CANCELED,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        dispatch_detached(
                            self.writer,
                            &self.detached,
                            WriteCmd::CancelReserve {
                                request_id: self.request_id.clone(),
                                account_id: self.account_id.clone(),
                                key: self.key.clone(),
                                hold: self.hold,
                                handoff: Arc::clone(&self.handoff),
                            },
                        );
                        return;
                    }
                }
                RESERVE_HANDOFF_CLAIMED
                | RESERVE_HANDOFF_CANCELED
                | RESERVE_HANDOFF_REFUNDING
                | RESERVE_HANDOFF_REFUNDED
                | RESERVE_HANDOFF_FAILED => return,
                _ => return,
            }
        }
    }
}

struct SnapshotReserveHandoffGuard {
    handoff: Arc<AtomicU8>,
}

impl SnapshotReserveHandoffGuard {
    fn claim(&self) -> bool {
        self.handoff
            .compare_exchange(
                SNAPSHOT_RESERVE_HANDOFF_COMMITTED,
                SNAPSHOT_RESERVE_HANDOFF_CLAIMED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }
}

impl Drop for SnapshotReserveHandoffGuard {
    fn drop(&mut self) {
        // Cancellation has exactly one safe linearization point: before the writer authorizes the
        // database commit. COMMIT_DECIDED and later states deliberately remain untouched.
        let _ = self.handoff.compare_exchange(
            SNAPSHOT_RESERVE_HANDOFF_PENDING,
            SNAPSHOT_RESERVE_HANDOFF_CANCELED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

fn authorize_snapshot_reserve_commit(handoff: &AtomicU8) -> bool {
    loop {
        match handoff.load(Ordering::Acquire) {
            SNAPSHOT_RESERVE_HANDOFF_PENDING => {
                if handoff
                    .compare_exchange(
                        SNAPSHOT_RESERVE_HANDOFF_PENDING,
                        SNAPSHOT_RESERVE_HANDOFF_COMMIT_DECIDED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    return true;
                }
            }
            // The callback is idempotent inside one already-authorized database attempt. A commit
            // error moves the handoff to COMMIT_UNKNOWN before any later exact replay is allowed.
            SNAPSHOT_RESERVE_HANDOFF_COMMIT_DECIDED => return true,
            SNAPSHOT_RESERVE_HANDOFF_CANCELED => return false,
            state => {
                eprintln!("billing snapshot reserve commit gate entered unexpected state {state}");
                return false;
            }
        }
    }
}

fn mark_snapshot_reserve_failed(handoff: &AtomicU8) {
    loop {
        match handoff.load(Ordering::Acquire) {
            SNAPSHOT_RESERVE_HANDOFF_PENDING => {
                if handoff
                    .compare_exchange(
                        SNAPSHOT_RESERVE_HANDOFF_PENDING,
                        SNAPSHOT_RESERVE_HANDOFF_FAILED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    return;
                }
            }
            // The physical commit result is ambiguous. Preserve it as a non-cancelable state so a
            // later exact replay or lease recovery can resolve the durable truth.
            SNAPSHOT_RESERVE_HANDOFF_COMMIT_DECIDED => {
                let _ = handoff.compare_exchange(
                    SNAPSHOT_RESERVE_HANDOFF_COMMIT_DECIDED,
                    SNAPSHOT_RESERVE_HANDOFF_COMMIT_UNKNOWN,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                return;
            }
            _ => return,
        }
    }
}

fn finish_snapshot_reserve(
    handoff: &AtomicU8,
    reply: oneshot::Sender<anyhow::Result<LegacyScalarReserveOutcome>>,
    result: anyhow::Result<LegacyScalarReserveOutcome>,
) {
    match result {
        Ok(outcome @ LegacyScalarReserveOutcome::Inserted(_))
        | Ok(outcome @ LegacyScalarReserveOutcome::Unchanged(_)) => {
            if let Err(state) = handoff.compare_exchange(
                SNAPSHOT_RESERVE_HANDOFF_COMMIT_DECIDED,
                SNAPSHOT_RESERVE_HANDOFF_COMMITTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                let _ = reply.send(Err(anyhow::anyhow!(
                    "snapshot reservation committed with unexpected handoff state {state}"
                )));
                return;
            }
            // A dropped receiver intentionally leaves COMMITTED untouched. Exact replay or the
            // durable reservation lease resolves ownership; no synthetic zero-cost settlement.
            let _ = reply.send(Ok(outcome));
        }
        Ok(outcome) => {
            if !matches!(&outcome, LegacyScalarReserveOutcome::AbortedBeforeCommit) {
                mark_snapshot_reserve_failed(handoff);
            }
            let _ = reply.send(Ok(outcome));
        }
        Err(error) => {
            mark_snapshot_reserve_failed(handoff);
            let _ = reply.send(Err(error));
        }
    }
}

fn finish_policy_snapshot_reserve(
    handoff: &AtomicU8,
    reply: oneshot::Sender<anyhow::Result<PolicyReserveOutcome>>,
    result: anyhow::Result<PolicyReserveOutcome>,
) {
    match result {
        Ok(outcome @ PolicyReserveOutcome::Inserted(_))
        | Ok(outcome @ PolicyReserveOutcome::Unchanged(_)) => {
            if let Err(state) = handoff.compare_exchange(
                SNAPSHOT_RESERVE_HANDOFF_COMMIT_DECIDED,
                SNAPSHOT_RESERVE_HANDOFF_COMMITTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                let _ = reply.send(Err(anyhow::anyhow!(
                    "policy snapshot reservation committed with unexpected handoff state {state}"
                )));
                return;
            }
            let _ = reply.send(Ok(outcome));
        }
        Ok(outcome) => {
            if !matches!(&outcome, PolicyReserveOutcome::AbortedBeforeCommit) {
                mark_snapshot_reserve_failed(handoff);
            }
            let _ = reply.send(Ok(outcome));
        }
        Err(error) => {
            mark_snapshot_reserve_failed(handoff);
            let _ = reply.send(Err(error));
        }
    }
}

fn finish_pricing_release_reserve(
    handoff: &AtomicU8,
    reply: oneshot::Sender<anyhow::Result<PricingReleaseReserveOutcomeV2>>,
    result: anyhow::Result<PricingReleaseReserveOutcomeV2>,
) {
    match result {
        Ok(outcome @ PricingReleaseReserveOutcomeV2::Inserted(_))
        | Ok(outcome @ PricingReleaseReserveOutcomeV2::Unchanged(_)) => {
            if let Err(state) = handoff.compare_exchange(
                SNAPSHOT_RESERVE_HANDOFF_COMMIT_DECIDED,
                SNAPSHOT_RESERVE_HANDOFF_COMMITTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                let _ = reply.send(Err(anyhow::anyhow!(
                    "pricing release reservation committed with unexpected handoff state {state}"
                )));
                return;
            }
            let _ = reply.send(Ok(outcome));
        }
        Ok(outcome) => {
            if !matches!(
                &outcome,
                PricingReleaseReserveOutcomeV2::AbortedBeforeCommit
            ) {
                mark_snapshot_reserve_failed(handoff);
            }
            let _ = reply.send(Ok(outcome));
        }
        Err(error) => {
            mark_snapshot_reserve_failed(handoff);
            let _ = reply.send(Err(error));
        }
    }
}

#[derive(Default)]
struct DetachedDispatchTracker {
    pending: AtomicUsize,
    idle: Notify,
}

impl DetachedDispatchTracker {
    fn begin(self: &Arc<Self>) -> DetachedDispatchGuard {
        self.pending.fetch_add(1, Ordering::AcqRel);
        DetachedDispatchGuard {
            tracker: self.clone(),
        }
    }

    async fn wait_idle(&self) {
        loop {
            let notified = self.idle.notified();
            if self.pending.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }
}

pub(crate) struct DetachedDispatchGuard {
    tracker: Arc<DetachedDispatchTracker>,
}

impl Drop for DetachedDispatchGuard {
    fn drop(&mut self) {
        if self.tracker.pending.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.tracker.idle.notify_waiters();
        }
    }
}

/// Queue a command from synchronous RAII/finalization code without blocking a Tokio worker. A full
/// bounded queue is drained by a tiny async waiter; the number of such waiters is itself bounded by
/// the global active-stream/request admission limits.
fn dispatch_detached(
    writer: &mpsc::Sender<WriteCmd>,
    tracker: &Arc<DetachedDispatchTracker>,
    cmd: WriteCmd,
) {
    let dispatch = tracker.begin();
    match writer.try_send(cmd) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(cmd)) => {
            let writer = writer.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _dispatch = dispatch;
                    if writer.send(cmd).await.is_err() {
                        eprintln!("billing writer stopped before a detached command was queued");
                    }
                });
            } else {
                let _ = std::thread::Builder::new()
                    .name("billing-backpressure".into())
                    .spawn(move || {
                        let _dispatch = dispatch;
                        if writer.blocking_send(cmd).is_err() {
                            eprintln!(
                                "billing writer stopped before a detached command was queued"
                            );
                        }
                    });
            }
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            eprintln!("billing writer stopped before a detached command was queued");
        }
    }
}

fn run_pg_with_retry<T>(
    pg: &mut registry::pg::PgStore,
    url: &str,
    owner: &registry::pg::Owner,
    operation: &str,
    mut call: impl FnMut(&mut registry::pg::PgStore) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let deadline = Instant::now() + PG_OPERATION_RETRY_DEADLINE;
    loop {
        match call(pg) {
            Ok(value) => return Ok(value),
            Err(error) => {
                let class = registry::pg::classify_failure(&error);
                if class != registry::pg::FailureClass::Transient || Instant::now() >= deadline {
                    return Err(error);
                }
                eprintln!("billing PostgreSQL {operation} transient failure: {error:#}");
            }
        }

        std::thread::sleep(Duration::from_millis(100));
        match registry::pg::PgStore::connect(url) {
            Ok(mut next) => match next.heartbeat_instance(owner, 30) {
                Ok(true) => *pg = next,
                Ok(false) => {
                    return Err(anyhow::anyhow!(
                        "engine owner was fenced during {operation}"
                    ))
                }
                Err(error) => {
                    if Instant::now() >= deadline {
                        return Err(
                            error.context(format!("heartbeat failed while retrying {operation}"))
                        );
                    }
                }
            },
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(
                        error.context(format!("reconnect deadline exceeded for {operation}"))
                    );
                }
            }
        }
    }
}

fn run_pg_snapshot_reserve_with_retry(
    pg: &mut registry::pg::PgStore,
    url: &str,
    owner: &registry::pg::Owner,
    key: &str,
    snapshot: &LegacyScalarAdmissionSnapshot,
    execution: &registry::ExecutionAttempt,
    handoff: &AtomicU8,
    lease_secs: i64,
) -> anyhow::Result<LegacyScalarReserveOutcome> {
    let deadline = Instant::now() + PG_OPERATION_RETRY_DEADLINE;
    loop {
        match pg.reserve_request_with_legacy_snapshot_guarded_for_execution(
            owner,
            key,
            lease_secs,
            snapshot,
            execution,
            || authorize_snapshot_reserve_commit(handoff),
        ) {
            Ok(value) => return Ok(value),
            Err(error) => {
                let class = registry::pg::classify_failure(&error);
                // Once the commit gate wins, a commit error is physically ambiguous. Leave the
                // active-or-absent result for a later exact replay; never blindly issue another
                // money operation from this actor turn.
                if handoff.load(Ordering::Acquire) != SNAPSHOT_RESERVE_HANDOFF_PENDING
                    || class != registry::pg::FailureClass::Transient
                    || Instant::now() >= deadline
                {
                    return Err(error);
                }
                eprintln!(
                    "billing PostgreSQL legacy snapshot reserve transient failure: {error:#}"
                );
            }
        }

        std::thread::sleep(Duration::from_millis(100));
        match registry::pg::PgStore::connect(url) {
            Ok(mut next) => match next.heartbeat_instance(owner, 30) {
                Ok(true) => *pg = next,
                Ok(false) => {
                    return Err(anyhow::anyhow!(
                        "engine owner was fenced during legacy snapshot reserve"
                    ))
                }
                Err(error) => {
                    if Instant::now() >= deadline {
                        return Err(error
                            .context("heartbeat failed while retrying legacy snapshot reserve"));
                    }
                }
            },
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(
                        error.context("reconnect deadline exceeded for legacy snapshot reserve")
                    );
                }
            }
        }
    }
}

fn run_pg_policy_snapshot_reserve_with_retry(
    pg: &mut registry::pg::PgStore,
    url: &str,
    owner: &registry::pg::Owner,
    key: &str,
    snapshot: &PolicyAdmissionSnapshot,
    execution: &registry::ExecutionAttempt,
    handoff: &AtomicU8,
    lease_secs: i64,
) -> anyhow::Result<PolicyReserveOutcome> {
    let deadline = Instant::now() + PG_OPERATION_RETRY_DEADLINE;
    loop {
        match pg.reserve_request_with_policy_snapshot_guarded_for_execution(
            owner,
            key,
            lease_secs,
            snapshot,
            execution,
            || authorize_snapshot_reserve_commit(handoff),
        ) {
            Ok(value) => return Ok(value),
            Err(error) => {
                let class = registry::pg::classify_failure(&error);
                if handoff.load(Ordering::Acquire) != SNAPSHOT_RESERVE_HANDOFF_PENDING
                    || class != registry::pg::FailureClass::Transient
                    || Instant::now() >= deadline
                {
                    return Err(error);
                }
                eprintln!(
                    "billing PostgreSQL policy snapshot reserve transient failure: {error:#}"
                );
            }
        }

        std::thread::sleep(Duration::from_millis(100));
        match registry::pg::PgStore::connect(url) {
            Ok(mut next) => match next.heartbeat_instance(owner, 30) {
                Ok(true) => *pg = next,
                Ok(false) => {
                    return Err(anyhow::anyhow!(
                        "engine owner was fenced during policy snapshot reserve"
                    ))
                }
                Err(error) => {
                    if Instant::now() >= deadline {
                        return Err(error
                            .context("heartbeat failed while retrying policy snapshot reserve"));
                    }
                }
            },
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(
                        error.context("reconnect deadline exceeded for policy snapshot reserve")
                    );
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_pg_pricing_release_reserve_with_retry(
    pg: &mut registry::pg::PgStore,
    url: &str,
    owner: &registry::pg::Owner,
    key: &str,
    resolution: &PricingReleaseResolutionV2,
    quote: &PricingReleaseQuoteV2,
    execution: &registry::ExecutionAttempt,
    handoff: &AtomicU8,
    lease_secs: i64,
) -> anyhow::Result<PricingReleaseReserveOutcomeV2> {
    let deadline = Instant::now() + PG_OPERATION_RETRY_DEADLINE;
    loop {
        match pg.reserve_request_with_pricing_release_v2_guarded_for_execution(
            owner,
            key,
            lease_secs,
            resolution,
            quote,
            execution,
            || authorize_snapshot_reserve_commit(handoff),
        ) {
            Ok(value) => return Ok(value),
            Err(error) => {
                let class = registry::pg::classify_failure(&error);
                if handoff.load(Ordering::Acquire) != SNAPSHOT_RESERVE_HANDOFF_PENDING
                    || class != registry::pg::FailureClass::Transient
                    || Instant::now() >= deadline
                {
                    return Err(error);
                }
                eprintln!(
                    "billing PostgreSQL pricing release reserve transient failure: {error:#}"
                );
            }
        }
        std::thread::sleep(Duration::from_millis(100));
        match registry::pg::PgStore::connect(url) {
            Ok(mut next) => match next.heartbeat_instance(owner, 30) {
                Ok(true) => *pg = next,
                Ok(false) => {
                    return Err(anyhow::anyhow!(
                        "engine owner was fenced during pricing release reserve"
                    ))
                }
                Err(error) if Instant::now() >= deadline => {
                    return Err(
                        error.context("heartbeat failed while retrying pricing release reserve")
                    );
                }
                Err(_) => {}
            },
            Err(error) if Instant::now() >= deadline => {
                return Err(
                    error.context("reconnect deadline exceeded for pricing release reserve")
                );
            }
            Err(_) => {}
        }
    }
}

fn observe_anthropic_postgres(
    pg: &mut registry::pg::PgStore,
    observation: &AnthropicWindowObservation,
) -> anyhow::Result<AnthropicCalibrationRow> {
    loop {
        let existing = pg.load_anthropic_calibration(
            &observation.subject_id,
            &observation.plan,
            &observation.window_kind,
        )?;
        if let Some(existing) = existing.as_ref().filter(|row| {
            row.estimator_version == crate::anthropic_calibration::ESTIMATOR_VERSION
                && anthropic_observation_is_stale_or_duplicate(row, observation)
        }) {
            return Ok(existing.clone());
        }
        let history = if existing.as_ref().is_some_and(|row| {
            row.estimator_version != crate::anthropic_calibration::ESTIMATOR_VERSION
        }) {
            pg.load_anthropic_window_observations(
                &observation.subject_id,
                &observation.plan,
                &observation.window_kind,
            )?
        } else {
            Vec::new()
        };
        let mut state = crate::anthropic_calibration::apply_observation_with_history(
            existing,
            &history,
            observation,
        )?;
        if let Some(version) = pg.save_anthropic_calibration(&state, observation)? {
            state.version = version;
            return Ok(state);
        }
    }
}

fn anthropic_observation_is_stale_or_duplicate(
    row: &AnthropicCalibrationRow,
    observation: &AnthropicWindowObservation,
) -> bool {
    observation.observed_at < row.observed_at
        || (observation.observed_at == row.observed_at
            && observation.resets_at == row.resets_at
            && observation.used_fraction_units == row.used_fraction_units
            && observation.measurement_resolution_fraction_units
                == row.measurement_resolution_fraction_units)
}

fn persist_anthropic_turn_postgres(
    pg: &mut registry::pg::PgStore,
    url: &str,
    owner: &registry::pg::Owner,
    turn: &PendingAnthropicCalibrationTurn,
) -> anyhow::Result<AnthropicTurnPersistenceResult> {
    run_pg_with_retry(pg, url, owner, "Anthropic turn calibration event", |pg| {
        if turn.event.provider != registry::PROVIDER_ANTHROPIC || turn.plan.is_empty() {
            anyhow::bail!("invalid Anthropic turn calibration command");
        }
        let spend = pg.record_provider_turn_calibration_event(&turn.event)?;
        let mut states = Vec::with_capacity(turn.snapshots.len());
        for snapshot in &turn.snapshots {
            let observation = anthropic_observation(
                &turn.event.subject_id,
                &turn.plan,
                snapshot,
                spend.spent_nano,
                "response",
                Some(&turn.event.request_id),
            );
            states.push(observe_anthropic_postgres(pg, &observation)?);
        }
        Ok((spend, states))
    })
}

fn anthropic_observation(
    subject_id: &str,
    plan: &str,
    snapshot: &AnthropicQuotaSnapshot,
    gateway_spend_nano: i64,
    source: &str,
    source_request_id: Option<&str>,
) -> AnthropicWindowObservation {
    AnthropicWindowObservation {
        subject_id: subject_id.to_owned(),
        plan: plan.to_owned(),
        window_kind: snapshot.window_kind.clone(),
        window_duration_mins: snapshot.window_duration_mins,
        resets_at: snapshot.resets_at,
        observed_at: snapshot.observed_at,
        used_fraction_units: snapshot.used_fraction_units,
        measurement_resolution_fraction_units: snapshot.measurement_resolution_fraction_units,
        gateway_spend_nano,
        observation_source: source.to_owned(),
        source_request_id: source_request_id.map(str::to_owned),
    }
}

fn gemini_observation_is_stale_or_duplicate(
    row: &GeminiExactCalibrationRow,
    observation: &GeminiExactWindowObservation,
) -> bool {
    observation.observed_at < row.observed_at
        || (observation.observed_at == row.observed_at
            && observation.resets_at == row.resets_at
            && observation.used_fraction_units == row.used_fraction_units
            && observation.measurement_resolution_fraction_units
                == row.measurement_resolution_fraction_units
            // A response quota point can be persisted before its turn spend is visible. If that
            // point is still above the retained anchor, an equal-second poll with a higher durable
            // cumulative spend is settlement catch-up, not a duplicate.
            && !(row.used_fraction_units > row.anchor_used_fraction_units
                && observation.gateway_spend_nano > row.anchor_spend_nano))
}

fn gemini_observation(
    profile_id: &str,
    plan: &str,
    snapshot: &GeminiQuotaSnapshot,
    gateway_spend_nano: i64,
    source: &str,
    source_request_id: Option<&str>,
) -> GeminiExactWindowObservation {
    GeminiExactWindowObservation {
        profile_id: profile_id.to_owned(),
        plan: plan.to_owned(),
        bucket_id: snapshot.bucket_id.clone(),
        window_kind: snapshot.window_kind.clone(),
        window_duration_mins: snapshot.window_duration_mins,
        resets_at: snapshot.resets_at,
        observed_at: snapshot.observed_at,
        used_fraction_units: snapshot.used_fraction_units,
        measurement_resolution_fraction_units: snapshot.measurement_resolution_fraction_units,
        gateway_spend_nano,
        observation_source: source.to_owned(),
        source_request_id: source_request_id.map(str::to_owned),
    }
}

fn observe_gemini_postgres(
    pg: &mut registry::pg::PgStore,
    observation: &GeminiExactWindowObservation,
) -> anyhow::Result<GeminiExactCalibrationRow> {
    loop {
        let existing = pg.load_gemini_exact_calibration(
            &observation.profile_id,
            &observation.plan,
            &observation.bucket_id,
        )?;
        if let Some(existing) = existing.as_ref().filter(|row| {
            row.estimator_version == crate::gemini::ESTIMATOR_VERSION
                && gemini_observation_is_stale_or_duplicate(row, observation)
        }) {
            return Ok(existing.clone());
        }
        let history = if existing
            .as_ref()
            .is_some_and(|row| row.estimator_version != crate::gemini::ESTIMATOR_VERSION)
        {
            pg.load_gemini_exact_window_observations(
                &observation.profile_id,
                &observation.plan,
                &observation.bucket_id,
            )?
        } else {
            Vec::new()
        };
        let mut state =
            crate::gemini::apply_observation_with_history(existing, &history, observation)?;
        if let Some(version) = pg.save_gemini_exact_calibration(&state, observation)? {
            state.version = version;
            return Ok(state);
        }
    }
}

fn kimi_observation(
    subject_id: &str,
    plan: &str,
    snapshot: &KimiQuotaSnapshot,
    cumulative_api_spend_nano: i64,
) -> KimiWindowObservation {
    KimiWindowObservation {
        subject_id: subject_id.to_owned(),
        plan: plan.to_owned(),
        window_duration_secs: snapshot.window_duration_secs,
        window_name: snapshot.window_name.clone(),
        resets_at: snapshot.resets_at,
        observed_at: snapshot.observed_at,
        native_used_units: snapshot.native_used_units,
        native_limit_units: snapshot.native_limit_units,
        used_fraction_units: snapshot.used_fraction_units,
        measurement_resolution_fraction_units: snapshot.measurement_resolution_fraction_units,
        cumulative_api_spend_nano,
    }
}

fn kimi_observation_is_stale_or_duplicate(
    row: &KimiCalibrationRow,
    observation: &KimiWindowObservation,
) -> bool {
    observation.observed_at < row.observed_at
        || (observation.observed_at == row.observed_at
            && observation.resets_at == row.resets_at
            && observation.native_used_units == row.native_used_units
            && observation.native_limit_units == row.native_limit_units
            && observation.used_fraction_units == row.used_fraction_units
            && observation.measurement_resolution_fraction_units
                == row.measurement_resolution_fraction_units
            // A quota point can be observed before the matching turn reaches durable spend.
            // An equal-second retry with higher spend is settlement catch-up, not a duplicate.
            && !(row.used_fraction_units > row.anchor_used_fraction_units
                && observation.cumulative_api_spend_nano > row.anchor_spend_nano))
}

fn observe_kimi_postgres(
    pg: &mut registry::pg::PgStore,
    observation: &KimiWindowObservation,
) -> anyhow::Result<KimiCalibrationRow> {
    loop {
        let existing = pg.load_kimi_calibration(
            &observation.subject_id,
            &observation.plan,
            observation.window_duration_secs,
        )?;
        if let Some(existing) = existing.as_ref().filter(|row| {
            row.estimator_version == crate::kimi_calibration::ESTIMATOR_VERSION
                && kimi_observation_is_stale_or_duplicate(row, observation)
        }) {
            return Ok(existing.clone());
        }
        let history = if existing
            .as_ref()
            .is_some_and(|row| row.estimator_version != crate::kimi_calibration::ESTIMATOR_VERSION)
        {
            pg.load_kimi_window_observations(
                &observation.subject_id,
                &observation.plan,
                observation.window_duration_secs,
            )?
        } else {
            Vec::new()
        };
        let mut state = crate::kimi_calibration::apply_observation_with_history(
            existing,
            &history,
            observation,
        )?;
        if let Some(version) = pg.save_kimi_calibration(&state, observation)? {
            state.version = version;
            return Ok(state);
        }
    }
}

/// Build the immutable GLM observation for one window: provider-side raw counters from the
/// snapshot plus the subject's exact durable cumulative dual ledgers, which only this serial
/// writer may read. API nanoUSD and native microcredits stay two independent totals — one is
/// never derived from the other (`docs/engine/GLM_PROVIDER.md` §5.3).
fn glm_observation(
    subject_id: &str,
    plan: &str,
    snapshot: &GlmQuotaSnapshot,
    spend: GlmSubjectSpend,
    source: &str,
    source_request_id: Option<&str>,
) -> GlmWindowObservation {
    GlmWindowObservation {
        subject_id: subject_id.to_owned(),
        plan: plan.to_owned(),
        window_duration_secs: snapshot.window_duration_secs,
        reset_at: snapshot.resets_at,
        observed_at: snapshot.observed_at,
        native_used_units: snapshot.native_used_units,
        native_limit_units: snapshot.native_limit_units,
        native_remaining_units: snapshot.native_remaining_units,
        percentage_raw: snapshot.percentage_raw,
        used_fraction_units: snapshot.used_fraction_units,
        measurement_resolution_fraction_units: snapshot.measurement_resolution_fraction_units,
        cumulative_api_nanousd: spend.spent_api_nanousd,
        cumulative_native_microcredits: spend.spent_native_microcredits,
        observation_source: source.to_owned(),
        source_request_id: source_request_id.map(str::to_owned),
    }
}

fn glm_observation_is_stale_or_duplicate(
    row: &GlmCalibrationRow,
    observation: &GlmWindowObservation,
) -> bool {
    observation.observed_at < row.observed_at
        || (observation.observed_at == row.observed_at
            && observation.reset_at == row.reset_at
            && observation.used_fraction_units == row.used_fraction_units
            && observation.measurement_resolution_fraction_units
                == row.measurement_resolution_fraction_units
            // A quota point can be observed before the matching turn reaches durable spend.
            // An equal-second retry with higher spend on EITHER ledger is settlement
            // catch-up, not a duplicate — the two ledgers advance together per turn.
            // (Option-ordered `>` would call `Some(x) > None` true, so the fraction
            // comparison is explicit: it only counts when both halves exist.)
            && !(matches!(
                (row.used_fraction_units, row.anchor_used_fraction_units),
                (Some(used), Some(anchor)) if used > anchor
            ) && (observation.cumulative_api_nanousd > row.anchor_spend_api_nanousd
                || observation.cumulative_native_microcredits
                    > row.anchor_spend_native_microcredits)))
}

fn observe_glm_postgres(
    pg: &mut registry::pg::PgStore,
    observation: &GlmWindowObservation,
) -> anyhow::Result<GlmCalibrationRow> {
    loop {
        let existing = pg.load_glm_calibration(
            &observation.subject_id,
            &observation.plan,
            observation.window_duration_secs,
        )?;
        if let Some(existing) = existing.as_ref().filter(|row| {
            row.estimator_version == crate::glm_calibration::ESTIMATOR_VERSION
                && glm_observation_is_stale_or_duplicate(row, observation)
        }) {
            return Ok(existing.clone());
        }
        let history = if existing
            .as_ref()
            .is_some_and(|row| row.estimator_version != crate::glm_calibration::ESTIMATOR_VERSION)
        {
            pg.load_glm_window_observations(
                &observation.subject_id,
                &observation.plan,
                observation.window_duration_secs,
            )?
        } else {
            Vec::new()
        };
        let mut state = crate::glm_calibration::apply_observation_with_history(
            existing,
            &history,
            observation,
        )?;
        // The save validates the state/observation pair and applies the estimator CAS.
        if let Some(version) = pg.save_glm_calibration(&state, observation)? {
            state.version = version;
            return Ok(state);
        }
    }
}

fn persist_gemini_turn_postgres(
    pg: &mut registry::pg::PgStore,
    url: &str,
    owner: &registry::pg::Owner,
    turn: &PendingGeminiCalibrationTurn,
) -> anyhow::Result<GeminiTurnPersistenceResult> {
    run_pg_with_retry(pg, url, owner, "Gemini turn calibration event", |pg| {
        if turn.event.provider != registry::PROVIDER_GOOGLE || turn.plan.is_empty() {
            anyhow::bail!("invalid Gemini turn calibration command");
        }
        let spend = pg.record_provider_turn_calibration_event(&turn.event)?;
        let mut states = Vec::with_capacity(turn.snapshots.len());
        for snapshot in &turn.snapshots {
            let observation = gemini_observation(
                &turn.event.subject_id,
                &turn.plan,
                snapshot,
                spend.spent_nano,
                "response",
                Some(&turn.event.request_id),
            );
            states.push(observe_gemini_postgres(pg, &observation)?);
        }
        Ok((spend, states))
    })
}

enum WriteCmd {
    AnthropicRecordTurn {
        delivery_id: u64,
        reply: Option<oneshot::Sender<anyhow::Result<AnthropicTurnPersistenceResult>>>,
    },
    AnthropicObserveWindow {
        subject_id: String,
        plan: String,
        snapshot: AnthropicQuotaSnapshot,
        reply: oneshot::Sender<anyhow::Result<(i64, AnthropicCalibrationRow)>>,
    },
    GeminiRecordTurn {
        delivery_id: u64,
        reply: Option<oneshot::Sender<anyhow::Result<GeminiTurnPersistenceResult>>>,
    },
    GeminiObserveWindow {
        profile_id: String,
        plan: String,
        snapshot: GeminiQuotaSnapshot,
        reply: oneshot::Sender<anyhow::Result<(i64, GeminiExactCalibrationRow)>>,
    },
    KimiRecordTurn {
        event: KimiTurnCalibrationEvent,
        reply: oneshot::Sender<anyhow::Result<bool>>,
    },
    KimiObserveWindows {
        subject_id: String,
        plan: String,
        snapshots: Vec<KimiQuotaSnapshot>,
        reply: oneshot::Sender<anyhow::Result<Vec<KimiCalibrationRow>>>,
    },
    GlmRecordTurn {
        event: GlmTurnCalibrationEvent,
        reply: oneshot::Sender<anyhow::Result<bool>>,
    },
    GlmObserveWindows {
        subject_id: String,
        plan: String,
        snapshots: Vec<GlmQuotaSnapshot>,
        reply: oneshot::Sender<anyhow::Result<Vec<GlmCalibrationRow>>>,
    },
    CodexRecordTurn {
        event: CodexTurnCalibrationEvent,
        reply: oneshot::Sender<anyhow::Result<CodexHomeCalibrationSpend>>,
    },
    CodexLoadHealth {
        home_id: String,
        reply: oneshot::Sender<anyhow::Result<registry::CodexHomeHealthRow>>,
    },
    CodexSaveHealth {
        home_id: String,
        row: registry::CodexHomeHealthRow,
        updated_ts: i64,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    CodexObserveWindow {
        home_id: String,
        window_duration_mins: i64,
        resets_at: i64,
        used_percent: i64,
        used_fraction_units: i64,
        observed_at: i64,
        reply: oneshot::Sender<anyhow::Result<(CodexHomeCalibrationSpend, CodexCalibrationRow)>>,
    },
    Reserve {
        request_id: String,
        account_id: String,
        key: String,
        hold: i64,
        execution: registry::ExecutionAttempt,
        handoff: Arc<AtomicU8>,
        reply: oneshot::Sender<anyhow::Result<Option<i64>>>,
    },
    ReserveWithLegacySnapshot {
        key: String,
        snapshot: LegacyScalarAdmissionSnapshot,
        execution: registry::ExecutionAttempt,
        handoff: Arc<AtomicU8>,
        reply: oneshot::Sender<anyhow::Result<LegacyScalarReserveOutcome>>,
    },
    ReserveWithPolicySnapshot {
        key: String,
        snapshot: PolicyAdmissionSnapshot,
        execution: registry::ExecutionAttempt,
        handoff: Arc<AtomicU8>,
        reply: oneshot::Sender<anyhow::Result<PolicyReserveOutcome>>,
    },
    ReserveWithPricingReleaseV2 {
        key: String,
        resolution: PricingReleaseResolutionV2,
        quote: PricingReleaseQuoteV2,
        execution: registry::ExecutionAttempt,
        handoff: Arc<AtomicU8>,
        reply: oneshot::Sender<anyhow::Result<PricingReleaseReserveOutcomeV2>>,
    },
    InsertPricingShadowEvaluation {
        input: PricingShadowAdmissionEvaluationInput,
        timeout_ms: u64,
        reply: oneshot::Sender<anyhow::Result<PricingShadowEvaluationWrite>>,
    },
    PreparePricingCatalog {
        spec: PricingCatalogSpec,
        reply: oneshot::Sender<anyhow::Result<PricingMutation>>,
    },
    ActivatePricingCatalog {
        product_id: String,
        target: VersionTarget,
        expectation: ActiveExpectation,
        reply: oneshot::Sender<anyhow::Result<PricingMutation>>,
    },
    PrepareProviderSwitches {
        spec: ProviderSwitchSpec,
        reply: oneshot::Sender<anyhow::Result<PricingMutation>>,
    },
    ActivateProviderSwitches {
        target: VersionTarget,
        expectation: ActiveExpectation,
        reply: oneshot::Sender<anyhow::Result<PricingMutation>>,
    },
    PrepareAccountPolicy {
        spec: AccountPolicySpec,
        reply: oneshot::Sender<anyhow::Result<PricingMutation>>,
    },
    LockedOpenKeysPolicyTransition {
        transition: LockedOpenKeysPolicyTransitionSpec,
        reply: oneshot::Sender<anyhow::Result<PricingMutation>>,
    },
    ActivateAccountPolicy {
        activation: AccountPolicyActivationSpec,
        expectation: PolicyActiveExpectation,
        reply: oneshot::Sender<anyhow::Result<PricingMutation>>,
    },
    PreparePricingReleasePolicyV2 {
        policy: PricingReleasePolicyV2,
        reply: oneshot::Sender<anyhow::Result<PricingMutation>>,
    },
    PreparePricingReleaseV2 {
        release: PricingReleaseV2,
        reply: oneshot::Sender<anyhow::Result<PricingMutation>>,
    },
    PreparePricingReleaseRecoveryLinkV2 {
        link: PricingReleaseRecoveryLinkV2,
        reply: oneshot::Sender<anyhow::Result<PricingMutation>>,
    },
    PreparePricingReleaseAssignmentExtensionV2 {
        extension: PricingReleaseAssignmentExtensionV2,
        reply: oneshot::Sender<anyhow::Result<PricingMutation>>,
    },
    ActivatePricingReleaseV2 {
        request: PricingReleaseActivationRequestV2,
        runtime_manifest: PricingRuntimeManifestEvidence,
        reply: oneshot::Sender<anyhow::Result<PricingReleaseActivationOutcomeV2>>,
    },
    ApplyFundingNormalizationV2 {
        account_id: String,
        request: FundingNormalizationApplyRequestV2,
        reply: oneshot::Sender<anyhow::Result<Option<FundingNormalizationApplyResultV2>>>,
    },
    CancelReserve {
        request_id: String,
        account_id: String,
        key: String,
        hold: i64,
        handoff: Arc<AtomicU8>,
    },
    Settle {
        request_id: String,
        account_id: String,
        key: String,
        hold: i64,
        actual: i64,
        reference: Option<String>,
        usage: Option<registry::UsageEventInput>, // разбивка токенов/модели (аналитика), если есть
        reply: Option<oneshot::Sender<anyhow::Result<Option<i64>>>>, // None → fire-and-forget (RAII из Drop)
    },
    Topup {
        account_id: String,
        amount: i64,
        reference: Option<String>,
        reply: oneshot::Sender<anyhow::Result<Option<i64>>>,
    },
    /// Control-плоскость (редкие управляющие записи из `/admin/*`) — через ТОТ ЖЕ writer, чтобы
    /// сохранить дисциплину единственного писателя (никаких гонок/BUSY с reserve/settle).
    CreateAccount {
        id: String,
        handle: Option<String>,
        mult_bp: i64,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    IssueKey {
        key: String,
        account_id: String,
        label: Option<String>,
        spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>,
        activation_policy_ack: Option<KeyActivationPolicyAck>,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    AccountStatus {
        id: String,
        status: String,
        reply: oneshot::Sender<anyhow::Result<usize>>,
    },
    AccountMultiplier {
        id: String,
        mult_bp: i64,
        reply: oneshot::Sender<anyhow::Result<usize>>,
    },
    KeyStatus {
        key: String,
        status: String,
        activation_policy_ack: Option<KeyActivationPolicyAck>,
        reply: oneshot::Sender<anyhow::Result<usize>>,
    },
    KeyStatusById {
        key_id: String,
        status: String,
        activation_policy_ack: Option<KeyActivationPolicyAck>,
        reply: oneshot::Sender<anyhow::Result<usize>>,
    },
    KeyLabelById {
        key_id: String,
        label: String,
        reply: oneshot::Sender<anyhow::Result<usize>>,
    },
    KeyPolicyById {
        account_id: String,
        key_id: String,
        spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>,
        reply: oneshot::Sender<anyhow::Result<KeyPolicyUpdate>>,
    },
    LedgerAck {
        consumer: String,
        account_id: String,
        last_id: i64,
        reply: oneshot::Sender<anyhow::Result<usize>>,
    },
    MarkDelivering {
        request_id: String,
        lease_secs: i64,
        reply: oneshot::Sender<anyhow::Result<bool>>,
    },
    RenewStreamLeases {
        request_id: Option<String>,
        capacity_lease_id: Option<String>,
        lease_secs: i64,
        reply: oneshot::Sender<anyhow::Result<bool>>,
    },
    AcquireCapacity {
        lease_id: String,
        request_id: String,
        email: String,
        lease_secs: i64,
        util_cap: f64,
        reply: oneshot::Sender<anyhow::Result<Option<registry::pg::CapacityLease>>>,
    },
    ReleaseCapacity {
        lease_id: String,
    },
    /// Барьер: writer FIFO → когда Flush обработан, ВСЕ прежние команды (settle) применены.
    /// Для дренажа очереди на graceful shutdown (иначе последние списания потерялись бы).
    Flush(oneshot::Sender<anyhow::Result<()>>),
}

enum ReadCmd {
    AnthropicCalibrationReport(
        oneshot::Sender<
            anyhow::Result<(
                Vec<AnthropicCalibrationRow>,
                Vec<ProviderTurnCalibrationAggregate>,
                Vec<ProviderTurnCalibrationEvent>,
            )>,
        >,
    ),
    GeminiCalibrationReport(
        oneshot::Sender<
            anyhow::Result<(
                Vec<GeminiExactCalibrationRow>,
                Vec<ProviderTurnCalibrationAggregate>,
                Vec<ProviderTurnCalibrationEvent>,
            )>,
        >,
    ),
    CodexCalibrationReport(oneshot::Sender<anyhow::Result<Vec<CodexTurnCalibrationAggregate>>>),
    KimiCalibrationReport(oneshot::Sender<anyhow::Result<Vec<KimiCalibrationRow>>>),
    KimiRecentTurns(
        i64,
        oneshot::Sender<anyhow::Result<Vec<KimiTurnCalibrationEvent>>>,
    ),
    KeyAuth(String, oneshot::Sender<anyhow::Result<Option<KeyAuth>>>),
    KeyGet(String, oneshot::Sender<anyhow::Result<Option<KeyRow>>>),
    Account(String, oneshot::Sender<anyhow::Result<Option<AccountRow>>>),
    AccountFunding(
        String,
        oneshot::Sender<anyhow::Result<Option<AccountFundingSnapshot>>>,
    ),
    AccountByHandle(String, oneshot::Sender<anyhow::Result<Option<AccountRow>>>),
    Totals(oneshot::Sender<anyhow::Result<BillingTotals>>),
    AccountsList(oneshot::Sender<anyhow::Result<Vec<AccountRow>>>),
    KeysByAccount(String, oneshot::Sender<anyhow::Result<Vec<KeyRow>>>),
    Ledger(
        String,
        i64,
        oneshot::Sender<anyhow::Result<Vec<registry::LedgerRow>>>,
    ),
    LedgerAfter(
        String,
        i64,
        i64,
        oneshot::Sender<anyhow::Result<Vec<registry::LedgerRow>>>,
    ),
    UsageByModel(
        String,
        i64,
        oneshot::Sender<anyhow::Result<Vec<registry::UsageModelAgg>>>,
    ),
    UsageReport(
        String,
        i64,
        i64,
        oneshot::Sender<anyhow::Result<registry::UsageReport>>,
    ),
    SpendByAccount {
        since_ts: i64,
        until_ts: i64,
        limit: i64,
        reply: oneshot::Sender<anyhow::Result<Vec<registry::SpendAccountAgg>>>,
    },
    SpendByProvider {
        since_ts: i64,
        until_ts: i64,
        reply: oneshot::Sender<anyhow::Result<Vec<registry::SpendProviderAgg>>>,
    },
    PricingCatalogByGeneration {
        product_id: String,
        generation: i64,
        reply: oneshot::Sender<anyhow::Result<Option<PricingCatalogSpec>>>,
    },
    ActivePricingCatalog {
        product_id: String,
        reply: oneshot::Sender<anyhow::Result<Option<PricingCatalogSpec>>>,
    },
    ProviderSwitchesByGeneration {
        generation: i64,
        reply: oneshot::Sender<anyhow::Result<Option<ProviderSwitchSpec>>>,
    },
    ActiveProviderSwitches {
        reply: oneshot::Sender<anyhow::Result<Option<ProviderSwitchSpec>>>,
    },
    AccountPolicyByVersion {
        account_id: String,
        effective_version: i64,
        reply: oneshot::Sender<anyhow::Result<Option<AccountPolicySpec>>>,
    },
    ActiveAccountPolicy {
        account_id: String,
        reply: oneshot::Sender<anyhow::Result<Option<ActiveAccountPolicy>>>,
    },
    PricingReadBundle {
        account_id: String,
        reply: oneshot::Sender<anyhow::Result<PricingReadBundle>>,
    },
    PricingReleasePolicyV2 {
        policy_id: String,
        policy_version: i64,
        reply: oneshot::Sender<anyhow::Result<Option<PricingReleasePolicyV2>>>,
    },
    PricingReleaseV2 {
        generation: i64,
        reply: oneshot::Sender<anyhow::Result<Option<PricingReleaseV2>>>,
    },
    PricingReleaseRecoveryLinkV2 {
        target_generation: i64,
        recovery_generation: i64,
        reply: oneshot::Sender<anyhow::Result<Option<PricingReleaseRecoveryLinkV2>>>,
    },
    PricingReleaseAssignmentExtensionV2 {
        provisioning_head_version: i64,
        account_id: String,
        reply: oneshot::Sender<anyhow::Result<Option<PricingReleaseAssignmentExtensionV2>>>,
    },
    PricingReleaseHeadV2 {
        reply: oneshot::Sender<anyhow::Result<Option<PricingReleaseHeadV2>>>,
    },
    PricingReleaseProvisioningContextV2 {
        reply: oneshot::Sender<anyhow::Result<Option<PricingReleaseProvisioningContextV2>>>,
    },
    PricingReleaseResolutionV2 {
        account_id: String,
        provider_id: String,
        canonical_model_id: String,
        reply: oneshot::Sender<anyhow::Result<Option<PricingReleaseResolutionV2>>>,
    },
    PricingReleaseInventoryV2 {
        after_account_id: Option<String>,
        limit: i64,
        reply: oneshot::Sender<anyhow::Result<PricingReleaseInventoryPageV2>>,
    },
    FundingNormalizationPlanV2 {
        account_id: String,
        reply: oneshot::Sender<anyhow::Result<Option<FundingNormalizationPlanV2>>>,
    },
    Stage8EngineEvidence {
        request: registry::stage8::Stage8EngineEvidenceRequest,
        reply: oneshot::Sender<anyhow::Result<registry::stage8::Stage8EngineEvidenceReport>>,
    },
    SpendByModel {
        since_ts: i64,
        until_ts: i64,
        limit: i64,
        reply: oneshot::Sender<anyhow::Result<Vec<registry::SpendModelAgg>>>,
    },
    SettlementHealth(
        i64,
        String,
        oneshot::Sender<anyhow::Result<registry::SettlementHealth>>,
    ),
}

enum PricingShadowReadCmd {
    Bundle {
        account_id: String,
        timeout_ms: u64,
        reply: oneshot::Sender<anyhow::Result<PricingReadBundle>>,
    },
}

/// Latency of the single-writer PostgreSQL money commands, measured around `run_pg_with_retry`
/// so the observation covers reconnect and retry — the budget the request path actually pays.
/// Owned here rather than in `forward::Metrics` because the billing writer starts before that
/// struct exists; the `/metrics` handler reads a snapshot through `pg_command_stats`. Bucket
/// boundaries match the pricing-bridge histogram so operator thresholds stay comparable, and the
/// array sizes stay within what `#[derive(Default)]` can initialize.
pub const PG_COMMAND_LATENCY_BUCKETS_MS: [u64; 10] =
    [1, 2, 5, 10, 25, 50, 100, 250, 500, 1_000];

/// The PostgreSQL write commands a request pays for synchronously. Compile-bounded so the metric
/// label set can never grow at runtime.
#[derive(Clone, Copy)]
#[repr(usize)]
pub enum PgCommandOp {
    Reserve = 0,
    Settle = 1,
    AcquireCapacity = 2,
}

pub const PG_COMMAND_OP_COUNT: usize = 3;

impl PgCommandOp {
    pub const ALL: [PgCommandOp; PG_COMMAND_OP_COUNT] = [
        PgCommandOp::Reserve,
        PgCommandOp::Settle,
        PgCommandOp::AcquireCapacity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PgCommandOp::Reserve => "reserve",
            PgCommandOp::Settle => "settle",
            PgCommandOp::AcquireCapacity => "acquire_capacity",
        }
    }
}

#[derive(Default)]
struct PgCommandMetrics {
    count: [AtomicU64; PG_COMMAND_OP_COUNT],
    sum_micros: [AtomicU64; PG_COMMAND_OP_COUNT],
    buckets: [AtomicU64; PG_COMMAND_OP_COUNT * PG_COMMAND_LATENCY_BUCKETS_MS.len()],
}

/// Point-in-time copy rendered by the `/metrics` handler.
pub struct PgCommandLatencyStats {
    pub count: [u64; PG_COMMAND_OP_COUNT],
    pub sum_micros: [u64; PG_COMMAND_OP_COUNT],
    pub buckets: [u64; PG_COMMAND_OP_COUNT * PG_COMMAND_LATENCY_BUCKETS_MS.len()],
}

struct PgCommandLatencyGuard<'a> {
    metrics: &'a PgCommandMetrics,
    op: PgCommandOp,
    started: Instant,
}

impl Drop for PgCommandLatencyGuard<'_> {
    fn drop(&mut self) {
        self.metrics.observe(self.op, self.started.elapsed());
    }
}

impl PgCommandMetrics {
    fn timer(&self, op: PgCommandOp) -> PgCommandLatencyGuard<'_> {
        PgCommandLatencyGuard {
            metrics: self,
            op,
            started: Instant::now(),
        }
    }

    fn observe(&self, op: PgCommandOp, elapsed: Duration) {
        let op_index = op as usize;
        self.count[op_index].fetch_add(1, Ordering::Relaxed);
        let micros = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        let _ = self.sum_micros[op_index].fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |current| Some(current.saturating_add(micros)),
        );
        for (bucket_index, upper_ms) in PG_COMMAND_LATENCY_BUCKETS_MS.iter().enumerate() {
            if elapsed <= Duration::from_millis(*upper_ms) {
                let index = op_index * PG_COMMAND_LATENCY_BUCKETS_MS.len() + bucket_index;
                self.buckets[index].fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn snapshot(&self) -> PgCommandLatencyStats {
        let load = |c: &AtomicU64| c.load(Ordering::Relaxed);
        PgCommandLatencyStats {
            count: self.count.each_ref().map(load),
            sum_micros: self.sum_micros.each_ref().map(load),
            buckets: self.buckets.each_ref().map(load),
        }
    }
}

/// Occupied slots of a bounded tokio mpsc channel: what a queue-depth gauge needs and nothing
/// the sender cannot already derive.
fn channel_queue_depth<T>(sender: &mpsc::Sender<T>) -> usize {
    sender.max_capacity().saturating_sub(sender.capacity())
}

/// Async-фасад биллинга: writer-канал + пул reader-каналов. Клонируется (в `Arc`) во все хендлеры.
pub struct AsyncBilling {
    writer: mpsc::Sender<WriteCmd>,
    detached: Arc<DetachedDispatchTracker>,
    anthropic_calibration_delivery: Arc<AnthropicCalibrationDeliveryState>,
    gemini_calibration_delivery: Arc<GeminiCalibrationDeliveryState>,
    readers: Vec<mpsc::Sender<ReadCmd>>,
    rr: AtomicUsize, // round-robin по читателям
    /// PostgreSQL-only connections reserved for evaluation-time shadow reads. They never share
    /// the customer authorization reader budget and are absent from live SQLite composition.
    pricing_shadow_readers: Vec<mpsc::Sender<PricingShadowReadCmd>>,
    pricing_shadow_rr: AtomicUsize,
    /// Present only for the PostgreSQL authority; the SQLite fallback keeps no latency stats
    /// because it is never the production hot path.
    pg_command: Option<Arc<PgCommandMetrics>>,
}

impl AsyncBilling {
    pub(crate) fn track_detached_work(&self) -> DetachedDispatchGuard {
        self.detached.begin()
    }

    /// Occupied slots of the single-writer command channel. A growing depth means the writer
    /// thread drains slower than requests enqueue — the earliest saturation signal of the money
    /// hot path, visible before any latency percentile moves.
    pub fn write_queue_depth(&self) -> usize {
        channel_queue_depth(&self.writer)
    }

    /// Latency histogram of the PostgreSQL reserve/settle/acquire_capacity commands, or `None`
    /// when this facade runs on the SQLite fallback.
    pub fn pg_command_stats(&self) -> Option<PgCommandLatencyStats> {
        self.pg_command
            .as_deref()
            .map(PgCommandMetrics::snapshot)
    }

    pub fn anthropic_calibration_delivery_status(&self) -> AnthropicCalibrationDeliveryStatus {
        AnthropicCalibrationDeliveryStatus {
            pending_events: self
                .anthropic_calibration_delivery
                .queue
                .lock()
                .expect("Anthropic calibration delivery queue lock")
                .pending
                .len(),
            dropped_events: self
                .anthropic_calibration_delivery
                .dropped_events
                .load(Ordering::Relaxed),
            persistence_ok: self
                .anthropic_calibration_delivery
                .persistence_ok
                .load(Ordering::Relaxed),
            queue_limit: MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS,
        }
    }

    pub fn gemini_calibration_delivery_status(&self) -> GeminiCalibrationDeliveryStatus {
        GeminiCalibrationDeliveryStatus {
            pending_events: self
                .gemini_calibration_delivery
                .queue
                .lock()
                .expect("Gemini calibration delivery queue lock")
                .pending
                .len(),
            dropped_events: self
                .gemini_calibration_delivery
                .dropped_events
                .load(Ordering::Relaxed),
            persistence_ok: self
                .gemini_calibration_delivery
                .persistence_ok
                .load(Ordering::Relaxed),
            queue_limit: MAX_PENDING_GEMINI_CALIBRATION_EVENTS,
        }
    }

    /// Persist one successful Claude turn, advance exact cumulative subject spend, and only then
    /// pair any response quota snapshots with that new total.
    #[cfg(test)]
    pub(crate) async fn record_anthropic_turn(
        &self,
        event: ProviderTurnCalibrationEvent,
        plan: &str,
        snapshots: Vec<AnthropicQuotaSnapshot>,
    ) -> anyhow::Result<(
        ProviderCalibrationSubjectSpend,
        Vec<AnthropicCalibrationRow>,
    )> {
        let delivery_id = enqueue_anthropic_calibration_turn(
            &self.anthropic_calibration_delivery,
            event,
            plan.to_owned(),
            snapshots,
        )?;
        let (reply, result) = oneshot::channel();
        if self
            .writer
            .send(WriteCmd::AnthropicRecordTurn {
                delivery_id,
                reply: Some(reply),
            })
            .await
            .is_err()
        {
            anyhow::bail!("billing writer unavailable; Anthropic evidence remains pending");
        }
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Finalization runs inside `Stream::poll`/`Drop`, so the same ordered operation also has a
    /// detached form. The billing FIFO and graceful-shutdown barrier still cover it.
    pub(crate) fn record_anthropic_turn_detached(
        &self,
        event: ProviderTurnCalibrationEvent,
        plan: &str,
        snapshots: Vec<AnthropicQuotaSnapshot>,
    ) -> bool {
        let delivery_id = match enqueue_anthropic_calibration_turn(
            &self.anthropic_calibration_delivery,
            event,
            plan.to_owned(),
            snapshots,
        ) {
            Ok(delivery_id) => delivery_id,
            Err(error) => {
                eprintln!("Anthropic calibration evidence dropped: {error:#}");
                return false;
            }
        };
        dispatch_detached(
            &self.writer,
            &self.detached,
            WriteCmd::AnthropicRecordTurn {
                delivery_id,
                reply: None,
            },
        );
        true
    }

    /// Pair a free liveness-poll snapshot with the current durable Claude spend without inventing
    /// a turn event. Successful-response snapshots use `record_anthropic_turn` instead.
    #[allow(clippy::too_many_arguments)]
    pub async fn observe_anthropic_window(
        &self,
        subject_id: &str,
        plan: &str,
        window_kind: &str,
        window_duration_mins: i64,
        resets_at: i64,
        used_fraction_units: i64,
        measurement_resolution_fraction_units: i64,
        observed_at: i64,
    ) -> anyhow::Result<(i64, AnthropicCalibrationRow)> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::AnthropicObserveWindow {
                subject_id: subject_id.to_owned(),
                plan: plan.to_owned(),
                snapshot: AnthropicQuotaSnapshot {
                    window_kind: window_kind.to_owned(),
                    window_duration_mins,
                    resets_at,
                    used_fraction_units,
                    measurement_resolution_fraction_units,
                    observed_at,
                },
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn anthropic_calibration_report(
        &self,
    ) -> anyhow::Result<(
        Vec<AnthropicCalibrationRow>,
        Vec<ProviderTurnCalibrationAggregate>,
        Vec<ProviderTurnCalibrationEvent>,
    )> {
        let (reply, result) = oneshot::channel();
        let reader = &self.readers[self.rr.fetch_add(1, Ordering::Relaxed) % self.readers.len()];
        reader
            .send(ReadCmd::AnthropicCalibrationReport(reply))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    /// Enqueue one immutable successful Gemini turn. Finalization can run from a stream drop, so
    /// delivery is detached while the bounded FIFO and shutdown barrier retain ordering.
    pub(crate) fn record_gemini_turn_detached(
        &self,
        event: ProviderTurnCalibrationEvent,
        plan: &str,
        snapshots: Vec<GeminiQuotaSnapshot>,
    ) -> bool {
        let delivery_id = match enqueue_gemini_calibration_turn(
            &self.gemini_calibration_delivery,
            event,
            plan.to_owned(),
            snapshots,
        ) {
            Ok(delivery_id) => delivery_id,
            Err(error) => {
                eprintln!("Gemini calibration evidence dropped: {error:#}");
                return false;
            }
        };
        dispatch_detached(
            &self.writer,
            &self.detached,
            WriteCmd::GeminiRecordTurn {
                delivery_id,
                reply: None,
            },
        );
        true
    }

    #[cfg(test)]
    pub(crate) async fn record_gemini_turn(
        &self,
        event: ProviderTurnCalibrationEvent,
        plan: &str,
        snapshots: Vec<GeminiQuotaSnapshot>,
    ) -> anyhow::Result<GeminiTurnPersistenceResult> {
        let delivery_id = enqueue_gemini_calibration_turn(
            &self.gemini_calibration_delivery,
            event,
            plan.to_owned(),
            snapshots,
        )?;
        let (reply, result) = oneshot::channel();
        if self
            .writer
            .send(WriteCmd::GeminiRecordTurn {
                delivery_id,
                reply: Some(reply),
            })
            .await
            .is_err()
        {
            anyhow::bail!("billing writer unavailable; Gemini evidence remains pending");
        }
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Pair one exact provider quota-summary snapshot with durable cumulative profile spend.
    #[allow(clippy::too_many_arguments)]
    pub async fn observe_gemini_window(
        &self,
        profile_id: &str,
        plan: &str,
        bucket_id: &str,
        window_kind: &str,
        window_duration_mins: i64,
        resets_at: i64,
        used_fraction_units: i64,
        measurement_resolution_fraction_units: i64,
        observed_at: i64,
    ) -> anyhow::Result<(i64, GeminiExactCalibrationRow)> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::GeminiObserveWindow {
                profile_id: profile_id.into(),
                plan: plan.into(),
                snapshot: GeminiQuotaSnapshot {
                    bucket_id: bucket_id.into(),
                    window_kind: window_kind.into(),
                    window_duration_mins,
                    resets_at,
                    used_fraction_units,
                    measurement_resolution_fraction_units,
                    observed_at,
                },
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn gemini_calibration_report(
        &self,
    ) -> anyhow::Result<(
        Vec<GeminiExactCalibrationRow>,
        Vec<ProviderTurnCalibrationAggregate>,
        Vec<ProviderTurnCalibrationEvent>,
    )> {
        let (reply, result) = oneshot::channel();
        let reader = &self.readers[self.rr.fetch_add(1, Ordering::Relaxed) % self.readers.len()];
        reader
            .send(ReadCmd::GeminiCalibrationReport(reply))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    /// Persist one immutable KIMI turn and advance the subject's exact API replacement spend.
    ///
    /// The gateway owns the bounded FIFO because it must gate `/usages` polling on a fully drained
    /// queue. This command is the single PostgreSQL writer hop for one FIFO head; exact request-id
    /// replay is a no-op and a different payload is a permanent typed conflict.
    pub(crate) async fn record_kimi_turn(
        &self,
        event: KimiTurnCalibrationEvent,
    ) -> anyhow::Result<bool> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::KimiRecordTurn { event, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Pair one whole `/usages` snapshot with exact durable subject spend and advance every
    /// independent window through immutable observation + estimator CAS.
    ///
    /// The caller has already drained the gateway's bounded turn FIFO and keeps that drain
    /// barrier held until this command returns. The writer still owns the spend read so no async
    /// caller can accidentally construct an observation from a stale total.
    pub(crate) async fn observe_kimi_windows(
        &self,
        subject_id: &str,
        plan: &str,
        snapshots: Vec<KimiQuotaSnapshot>,
    ) -> anyhow::Result<Vec<KimiCalibrationRow>> {
        if subject_id.is_empty() || plan.is_empty() || snapshots.is_empty() {
            anyhow::bail!("invalid KIMI quota observation command");
        }
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::KimiObserveWindows {
                subject_id: subject_id.to_owned(),
                plan: plan.to_owned(),
                snapshots,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Read every durable KIMI window calibration row for the admin operational projection.
    ///
    /// Rows are keyed by provider subject; joining them to opaque roster ids is the caller's
    /// concern (the gateway owns that mapping). KIMI calibration is PostgreSQL-only, so a SQLite
    /// authority reports an empty fleet rather than an error.
    pub async fn kimi_calibration_report(
        &self,
    ) -> anyhow::Result<Vec<KimiCalibrationRow>> {
        let (reply, result) = oneshot::channel();
        let reader = &self.readers[self.rr.fetch_add(1, Ordering::Relaxed) % self.readers.len()];
        reader
            .send(ReadCmd::KimiCalibrationReport(reply))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    /// Read the most recent immutable KIMI turn events, newest first, for exact request-id
    /// attribution in the admin-only calibration runner. PostgreSQL-only like every KIMI
    /// calibration read: a SQLite authority reports an empty list rather than an error.
    pub async fn kimi_recent_turns(
        &self,
        limit: i64,
    ) -> anyhow::Result<Vec<KimiTurnCalibrationEvent>> {
        let (reply, result) = oneshot::channel();
        let reader = &self.readers[self.rr.fetch_add(1, Ordering::Relaxed) % self.readers.len()];
        reader
            .send(ReadCmd::KimiRecentTurns(limit, reply))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    /// Persist one immutable GLM turn and advance the subject's exact dual ledgers (official
    /// API nanoUSD AND native microcredits) in the same transaction.
    ///
    /// The gateway owns the bounded FIFO because it must gate quota polling on a fully drained
    /// queue. This command is the single PostgreSQL writer hop for one FIFO head; exact
    /// request-id replay is a no-op and a different payload is a permanent typed conflict.
    pub(crate) async fn record_glm_turn(
        &self,
        event: GlmTurnCalibrationEvent,
    ) -> anyhow::Result<bool> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::GlmRecordTurn { event, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Pair one whole GLM quota snapshot with exact durable subject dual-ledger spend and
    /// advance every independent window (5h and weekly) through immutable observation +
    /// estimator CAS.
    ///
    /// The caller has already drained the gateway's bounded turn FIFO and keeps that drain
    /// barrier held until this command returns. The writer still owns the spend read so no
    /// async caller can accidentally construct an observation from a stale total.
    pub(crate) async fn observe_glm_windows(
        &self,
        subject_id: &str,
        plan: &str,
        snapshots: Vec<GlmQuotaSnapshot>,
    ) -> anyhow::Result<Vec<GlmCalibrationRow>> {
        if subject_id.is_empty() || plan.is_empty() || snapshots.is_empty() {
            anyhow::bail!("invalid GLM quota observation command");
        }
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::GlmObserveWindows {
                subject_id: subject_id.to_owned(),
                plan: plan.to_owned(),
                snapshots,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Persist exact official-price spend for one Codex home and return its durable cumulative
    /// total. This is provider-capacity evidence, independent of whether the customer turn was
    /// billable (admin turns consume the same subscription window).
    /// Idempotently persist exact API and ChatGPT-credit evidence for a successful Codex turn.
    pub async fn record_codex_turn(
        &self,
        event: CodexTurnCalibrationEvent,
    ) -> anyhow::Result<CodexHomeCalibrationSpend> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::CodexRecordTurn { event, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Read the durable account-level verdict for one home.
    ///
    /// Only the account axis is durable: transport health belongs to one transport generation and
    /// a restarted gateway holds a new bridge, so it deserves a fresh verdict rather than an
    /// inherited one.
    pub async fn load_codex_health(
        &self,
        home_id: &str,
    ) -> anyhow::Result<registry::CodexHomeHealthRow> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::CodexLoadHealth {
                home_id: home_id.into(),
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Persist the account-level verdict so it survives restart and blue-green handoff.
    pub async fn save_codex_health(
        &self,
        home_id: &str,
        row: registry::CodexHomeHealthRow,
        updated_ts: i64,
    ) -> anyhow::Result<()> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::CodexSaveHealth {
                home_id: home_id.into(),
                row,
                updated_ts,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Pair a provider snapshot with durable cumulative spend and CAS-persist the pure estimator.
    /// Duration/reset are required so an unknown window is never guessed as a weekly window.
    pub async fn observe_codex_window(
        &self,
        home_id: &str,
        window_duration_mins: i64,
        resets_at: i64,
        used_percent: i64,
        used_fraction_units: i64,
        observed_at: i64,
    ) -> anyhow::Result<(CodexHomeCalibrationSpend, CodexCalibrationRow)> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::CodexObserveWindow {
                home_id: home_id.into(),
                window_duration_mins,
                resets_at,
                used_percent,
                used_fraction_units,
                observed_at,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn codex_calibration_report(
        &self,
    ) -> anyhow::Result<Vec<CodexTurnCalibrationAggregate>> {
        let (reply, result) = oneshot::channel();
        let reader = &self.readers[self.rr.fetch_add(1, Ordering::Relaxed) % self.readers.len()];
        reader
            .send(ReadCmd::CodexCalibrationReport(reply))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub fn start_authority(
        config: registry::authority::AuthorityConfig,
        owner: Option<registry::pg::Owner>,
        readers: usize,
        _auth_ttl_ms: u64,
    ) -> anyhow::Result<Self> {
        Self::start_authority_with_pricing_shadow(config, owner, readers, 0, _auth_ttl_ms)
    }

    /// Start the billing actors plus a separate PostgreSQL-only pricing-shadow read budget.
    /// Shadow evaluation inserts still use the one existing billing writer.
    pub fn start_authority_with_pricing_shadow(
        config: registry::authority::AuthorityConfig,
        owner: Option<registry::pg::Owner>,
        readers: usize,
        pricing_shadow_readers: usize,
        _auth_ttl_ms: u64,
    ) -> anyhow::Result<Self> {
        match config {
            registry::authority::AuthorityConfig::Sqlite { path } => {
                if pricing_shadow_readers != 0 {
                    anyhow::bail!("live pricing shadow readers require PostgreSQL authority");
                }
                Self::start_with(path, readers, 0)
            }
            registry::authority::AuthorityConfig::Postgres { url } => {
                let owner = owner
                    .ok_or_else(|| anyhow::anyhow!("PostgreSQL billing requires owner epoch"))?;
                Self::start_postgres(url, owner, readers, pricing_shadow_readers, 0)
            }
        }
    }

    /// Поднять writer-поток + `readers` reader-потоков. `open` (миграции + PRAGMA WAL) — на этих
    /// потоках; синхронный SQLite не касается async-рантайма никогда.
    pub fn start(db_path: String, readers: usize) -> anyhow::Result<Self> {
        Self::start_with(db_path, readers, 0)
    }

    /// `auth_ttl_ms` — TTL кэша key_auth в мс (0 = кэш выключен).
    pub fn start_with(db_path: String, readers: usize, _auth_ttl_ms: u64) -> anyhow::Result<Self> {
        let readers = readers.max(1);
        // writer
        let (wtx, mut wrx) = mpsc::channel::<WriteCmd>(WRITE_QUEUE_CAPACITY);
        let anthropic_calibration_delivery = Arc::new(AnthropicCalibrationDeliveryState::default());
        let gemini_calibration_delivery = Arc::new(GeminiCalibrationDeliveryState::default());
        {
            let conn = registry::open(&db_path)?;
            let writer_anthropic_delivery = Arc::clone(&anthropic_calibration_delivery);
            let writer_gemini_delivery = Arc::clone(&gemini_calibration_delivery);
            std::thread::Builder::new().name("billing-writer".into()).spawn(move || {
                const RESERVATION_LEASE_SECS: i64 = 3600;
                let refund_canceled_reserve = |request_id: &str, account_id: &str, key: &str,
                                               hold: i64, handoff: &AtomicU8| {
                    if handoff.compare_exchange(
                        RESERVE_HANDOFF_CANCELED,
                        RESERVE_HANDOFF_REFUNDING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ).is_err() {
                        return;
                    }
                    match registry::sqlite_cancel_request(
                        &conn, request_id, account_id, key, hold,
                    ) {
                        Ok(Some(_)) => handoff.store(RESERVE_HANDOFF_REFUNDED, Ordering::Release),
                        Ok(None) => {
                            eprintln!("billing reserve cancellation did not produce a balance");
                            handoff.store(RESERVE_HANDOFF_CANCELED, Ordering::Release);
                        }
                        Err(err) => {
                            eprintln!("billing reserve cancellation refund failed: {err:#}");
                            handoff.store(RESERVE_HANDOFF_CANCELED, Ordering::Release);
                        }
                    }
                };
                let finish_reserve = |request_id: String, account_id: String, key: String, hold: i64,
                                      handoff: Arc<AtomicU8>,
                                      reply: oneshot::Sender<anyhow::Result<Option<i64>>>,
                                      res: anyhow::Result<Option<i64>>| {
                    let res = match res {
                        Ok(result) => result,
                        Err(error) => {
                            handoff.store(RESERVE_HANDOFF_FAILED, Ordering::Release);
                            let _ = reply.send(Err(error));
                            return;
                        }
                    };
                    if res.is_none() {
                        handoff.store(RESERVE_HANDOFF_FAILED, Ordering::Release);
                        let _ = reply.send(Ok(None));
                        return;
                    }
                    match handoff.compare_exchange(
                        RESERVE_HANDOFF_PENDING,
                        RESERVE_HANDOFF_COMMITTED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => {
                            if reply.send(Ok(res)).is_err() {
                                let _ = handoff.compare_exchange(
                                    RESERVE_HANDOFF_COMMITTED,
                                    RESERVE_HANDOFF_CANCELED,
                                    Ordering::AcqRel,
                                    Ordering::Acquire,
                                );
                                refund_canceled_reserve(&request_id, &account_id, &key, hold, &handoff);
                            }
                        }
                        Err(RESERVE_HANDOFF_CANCELED) => {
                            refund_canceled_reserve(&request_id, &account_id, &key, hold, &handoff);
                        }
                        Err(state) => {
                            eprintln!("billing reserve handoff entered unexpected state {state}");
                        }
                    }
                };
                let observe_anthropic = |observation: &AnthropicWindowObservation| {
                    loop {
                        let existing = registry::load_anthropic_calibration(
                            &conn,
                            &observation.subject_id,
                            &observation.plan,
                            &observation.window_kind,
                        )?;
                        if let Some(existing) = existing.as_ref().filter(|row| {
                            row.estimator_version
                                == crate::anthropic_calibration::ESTIMATOR_VERSION
                                && anthropic_observation_is_stale_or_duplicate(row, observation)
                        }) {
                            return Ok::<_, anyhow::Error>(existing.clone());
                        }
                        let history = if existing.as_ref().is_some_and(|row| {
                            row.estimator_version
                                != crate::anthropic_calibration::ESTIMATOR_VERSION
                        }) {
                            registry::load_anthropic_window_observations(
                                &conn,
                                &observation.subject_id,
                                &observation.plan,
                                &observation.window_kind,
                            )?
                        } else {
                            Vec::new()
                        };
                        let mut state =
                            crate::anthropic_calibration::apply_observation_with_history(
                                existing,
                                &history,
                                observation,
                            )?;
                        if let Some(version) = registry::save_anthropic_calibration(
                            &conn,
                            &state,
                            observation,
                        )? {
                            state.version = version;
                            return Ok(state);
                        }
                    }
                };
                let persist_anthropic_turn =
                    |turn: &PendingAnthropicCalibrationTurn| {
                        if turn.event.provider != registry::PROVIDER_ANTHROPIC
                            || turn.plan.is_empty()
                        {
                            anyhow::bail!("invalid Anthropic turn calibration command");
                        }
                        let spend = registry::record_provider_turn_calibration_event(
                            &conn,
                            &turn.event,
                        )?;
                        let mut states = Vec::with_capacity(turn.snapshots.len());
                        for snapshot in &turn.snapshots {
                            let observation = anthropic_observation(
                                &turn.event.subject_id,
                                &turn.plan,
                                snapshot,
                                spend.spent_nano,
                                "response",
                                Some(&turn.event.request_id),
                            );
                            states.push(observe_anthropic(&observation)?);
                        }
                        Ok((spend, states))
                    };
                let observe_gemini = |observation: &GeminiExactWindowObservation| {
                    loop {
                        let existing = registry::load_gemini_exact_calibration(
                            &conn,
                            &observation.profile_id,
                            &observation.plan,
                            &observation.bucket_id,
                        )?;
                        if let Some(existing) = existing.as_ref().filter(|row| {
                            row.estimator_version == crate::gemini::ESTIMATOR_VERSION
                                && gemini_observation_is_stale_or_duplicate(row, observation)
                        }) {
                            return Ok::<_, anyhow::Error>(existing.clone());
                        }
                        let history = if existing.as_ref().is_some_and(|row| {
                            row.estimator_version != crate::gemini::ESTIMATOR_VERSION
                        }) {
                            registry::load_gemini_exact_window_observations(
                                &conn,
                                &observation.profile_id,
                                &observation.plan,
                                &observation.bucket_id,
                            )?
                        } else {
                            Vec::new()
                        };
                        let mut state = crate::gemini::apply_observation_with_history(
                            existing,
                            &history,
                            observation,
                        )?;
                        if let Some(version) = registry::save_gemini_exact_calibration(
                            &conn,
                            &state,
                            observation,
                        )? {
                            state.version = version;
                            return Ok(state);
                        }
                    }
                };
                let persist_gemini_turn = |turn: &PendingGeminiCalibrationTurn| {
                    if turn.event.provider != registry::PROVIDER_GOOGLE || turn.plan.is_empty() {
                        anyhow::bail!("invalid Gemini turn calibration command");
                    }
                    let spend = registry::record_provider_turn_calibration_event(
                        &conn,
                        &turn.event,
                    )?;
                    let mut states = Vec::with_capacity(turn.snapshots.len());
                    for snapshot in &turn.snapshots {
                        let observation = gemini_observation(
                            &turn.event.subject_id,
                            &turn.plan,
                            snapshot,
                            spend.spent_nano,
                            "response",
                            Some(&turn.event.request_id),
                        );
                        states.push(observe_gemini(&observation)?);
                    }
                    Ok((spend, states))
                };
                while let Some(cmd) = wrx.blocking_recv() {
                    match cmd {
                    WriteCmd::AnthropicRecordTurn { delivery_id, reply } => {
                        if let Some(reply) = reply {
                            let result = flush_pending_anthropic_calibration_turns(
                                &writer_anthropic_delivery,
                                Some(delivery_id),
                                &persist_anthropic_turn,
                            )
                            .and_then(|result| {
                                result.ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "Anthropic calibration target was not replayed"
                                    )
                                })
                            });
                            let _ = reply.send(result);
                        } else {
                            let result = flush_pending_anthropic_calibration_turns(
                                &writer_anthropic_delivery,
                                None,
                                &persist_anthropic_turn,
                            );
                            if let Err(error) = result {
                                eprintln!(
                                    "Anthropic calibration persistence deferred with FIFO head retained: {error:#}"
                                );
                            }
                        }
                    }
                    WriteCmd::AnthropicObserveWindow {
                        subject_id, plan, snapshot, reply,
                    } => {
                        let result = (|| {
                            flush_pending_anthropic_calibration_turns(
                                &writer_anthropic_delivery,
                                None,
                                &persist_anthropic_turn,
                            )?;
                            let spend = registry::provider_calibration_subject_spend(
                                &conn,
                                registry::PROVIDER_ANTHROPIC,
                                &subject_id,
                            )?;
                            let observation = anthropic_observation(
                                &subject_id,
                                &plan,
                                &snapshot,
                                spend.spent_nano,
                                "poll",
                                None,
                            );
                            let state = observe_anthropic(&observation)?;
                            Ok((spend.spent_nano, state))
                        })();
                        let _ = reply.send(result);
                    }
                    WriteCmd::GeminiRecordTurn { delivery_id, reply } => {
                        if let Some(reply) = reply {
                            let result = flush_pending_gemini_calibration_turns(
                                &writer_gemini_delivery,
                                Some(delivery_id),
                                &persist_gemini_turn,
                            )
                            .and_then(|result| {
                                result.ok_or_else(|| {
                                    anyhow::anyhow!("Gemini calibration target was not replayed")
                                })
                            });
                            let _ = reply.send(result);
                        } else if let Err(error) = flush_pending_gemini_calibration_turns(
                            &writer_gemini_delivery,
                            None,
                            &persist_gemini_turn,
                        ) {
                            eprintln!(
                                "Gemini calibration persistence deferred with FIFO head retained: {error:#}"
                            );
                        }
                    }
                    WriteCmd::GeminiObserveWindow {
                        profile_id, plan, snapshot, reply,
                    } => {
                        let result = (|| {
                            flush_pending_gemini_calibration_turns(
                                &writer_gemini_delivery,
                                None,
                                &persist_gemini_turn,
                            )?;
                            let spend = registry::provider_calibration_subject_spend(
                                &conn,
                                registry::PROVIDER_GOOGLE,
                                &profile_id,
                            )?;
                            let observation = gemini_observation(
                                &profile_id,
                                &plan,
                                &snapshot,
                                spend.spent_nano,
                                "poll",
                                None,
                            );
                            let state = observe_gemini(&observation)?;
                            Ok((spend.spent_nano, state))
                        })();
                        let _ = reply.send(result);
                    }
                    WriteCmd::KimiRecordTurn { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "KIMI calibration authority requires PostgreSQL"
                        )));
                    }
                    WriteCmd::KimiObserveWindows { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "KIMI calibration authority requires PostgreSQL"
                        )));
                    }
                    WriteCmd::GlmRecordTurn { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "GLM calibration authority requires PostgreSQL"
                        )));
                    }
                    WriteCmd::GlmObserveWindows { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "GLM calibration authority requires PostgreSQL"
                        )));
                    }
                    WriteCmd::CodexRecordTurn { event, reply } => {
                        let _ = reply.send(registry::record_codex_turn_calibration_event(
                            &conn, &event,
                        ));
                    }
                    WriteCmd::CodexLoadHealth { home_id, reply } => {
                        let _ = reply.send(registry::load_codex_home_health(&conn, &home_id));
                    }
                    WriteCmd::CodexSaveHealth { home_id, row, updated_ts, reply } => {
                        let _ = reply.send(registry::save_codex_home_health(
                            &conn, &home_id, &row, updated_ts,
                        ));
                    }
                    WriteCmd::CodexObserveWindow {
                        home_id,
                        window_duration_mins,
                        resets_at,
                        used_percent,
                        used_fraction_units,
                        observed_at,
                        reply,
                    } => {
                        let result = (|| {
                            let spend = registry::codex_home_calibration_spend(&conn, &home_id)?;
                            let observation = CodexWindowObservation {
                                home_id: home_id.clone(),
                                window_duration_mins,
                                resets_at,
                                observed_at,
                                used_percent,
                                used_fraction_units,
                                gateway_spend_nano: spend.spent_nano,
                                gateway_spend_nanocredits: spend.spent_nanocredits,
                            };
                            loop {
                                let existing = registry::load_codex_calibration(
                                    &conn, &home_id, window_duration_mins,
                                )?;
                                if let Some(existing) = existing
                                    .as_ref()
                                    .filter(|row| {
                                        row.estimator_version
                                            == crate::codex::ESTIMATOR_VERSION
                                            && observed_at <= row.observed_at
                                    })
                                {
                                    return Ok((spend.clone(), existing.clone()));
                                }
                                let history = if existing.as_ref().is_some_and(|row| {
                                    row.estimator_version
                                        != crate::codex::ESTIMATOR_VERSION
                                }) {
                                    registry::load_codex_window_observations(
                                        &conn,
                                        &home_id,
                                        window_duration_mins,
                                    )?
                                } else {
                                    Vec::new()
                                };
                                let mut state = crate::codex::apply_observation_with_history(
                                    existing,
                                    &history,
                                    &observation,
                                )?;
                                if let Some(version) = registry::save_codex_calibration(
                                    &conn, &state, &observation,
                                )? {
                                    state.version = version;
                                    return Ok((spend.clone(), state));
                                }
                            }
                        })();
                        let _ = reply.send(result);
                    }
                    WriteCmd::Reserve { request_id, account_id, key, hold, execution, handoff, reply } => {
                        let result = registry::sqlite_reserve_request_for_execution(
                            &conn, &request_id, &account_id, &key, hold, RESERVATION_LEASE_SECS,
                            &execution,
                        );
                        finish_reserve(request_id, account_id, key, hold, handoff, reply, result);
                    }
                    WriteCmd::ReserveWithLegacySnapshot { key, snapshot, execution, handoff, reply } => {
                        if handoff.load(Ordering::Acquire) == SNAPSHOT_RESERVE_HANDOFF_CANCELED {
                            let _ = reply.send(Ok(
                                LegacyScalarReserveOutcome::AbortedBeforeCommit,
                            ));
                            continue;
                        }
                        let result = registry::sqlite_reserve_request_with_legacy_snapshot_guarded_for_execution(
                            &conn,
                            &key,
                            RESERVATION_LEASE_SECS,
                            &snapshot,
                            &execution,
                            || authorize_snapshot_reserve_commit(&handoff),
                        );
                        finish_snapshot_reserve(&handoff, reply, result);
                    }
                    WriteCmd::ReserveWithPolicySnapshot { key, snapshot, execution, handoff, reply } => {
                        if handoff.load(Ordering::Acquire) == SNAPSHOT_RESERVE_HANDOFF_CANCELED {
                            let _ = reply.send(Ok(PolicyReserveOutcome::AbortedBeforeCommit));
                            continue;
                        }
                        let result = registry::sqlite_reserve_request_with_policy_snapshot_guarded_for_execution(
                            &conn,
                            &key,
                            RESERVATION_LEASE_SECS,
                            &snapshot,
                            &execution,
                            || authorize_snapshot_reserve_commit(&handoff),
                        );
                        finish_policy_snapshot_reserve(&handoff, reply, result);
                    }
                    WriteCmd::ReserveWithPricingReleaseV2 { handoff, reply, .. } => {
                        finish_pricing_release_reserve(
                            &handoff,
                            reply,
                            Ok(PricingReleaseReserveOutcomeV2::NoActiveRelease),
                        );
                    }
                    WriteCmd::InsertPricingShadowEvaluation { input, timeout_ms: _, reply } => {
                        let _ = reply.send(
                            registry::pricing::sqlite_insert_pricing_shadow_admission_evaluation(
                                &conn, &input,
                            ),
                        );
                    }
                    WriteCmd::PreparePricingCatalog { spec, reply } => {
                        let _ = reply.send(registry::pricing::sqlite_prepare_pricing_catalog(
                            &conn, &spec,
                        ));
                    }
                    WriteCmd::ActivatePricingCatalog {
                        product_id, target, expectation, reply,
                    } => {
                        let _ = reply.send(registry::pricing::sqlite_activate_pricing_catalog(
                            &conn, &product_id, &target, &expectation,
                        ));
                    }
                    WriteCmd::PrepareProviderSwitches { spec, reply } => {
                        let _ = reply.send(registry::pricing::sqlite_prepare_provider_switches(
                            &conn, &spec,
                        ));
                    }
                    WriteCmd::ActivateProviderSwitches { target, expectation, reply } => {
                        let _ = reply.send(registry::pricing::sqlite_activate_provider_switches(
                            &conn, &target, &expectation,
                        ));
                    }
                    WriteCmd::PrepareAccountPolicy { spec, reply } => {
                        let _ = reply.send(registry::pricing::sqlite_prepare_account_policy(
                            &conn, &spec,
                        ));
                    }
                    WriteCmd::LockedOpenKeysPolicyTransition { transition, reply } => {
                        let _ = reply.send(
                            registry::pricing::sqlite_locked_openkeys_policy_transition(
                                &conn,
                                &transition,
                            ),
                        );
                    }
                    WriteCmd::ActivateAccountPolicy { activation, expectation, reply } => {
                        let _ = reply.send(registry::pricing::sqlite_activate_account_policy(
                            &conn, &activation, &expectation,
                        ));
                    }
                    WriteCmd::PreparePricingReleasePolicyV2 { reply, .. }
                    | WriteCmd::PreparePricingReleaseV2 { reply, .. }
                    | WriteCmd::PreparePricingReleaseRecoveryLinkV2 { reply, .. }
                    | WriteCmd::PreparePricingReleaseAssignmentExtensionV2 { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "pricing release v2 authority requires PostgreSQL"
                        )));
                    }
                    WriteCmd::ActivatePricingReleaseV2 { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "pricing release v2 authority requires PostgreSQL"
                        )));
                    }
                    WriteCmd::ApplyFundingNormalizationV2 { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "funding normalization v2 authority requires PostgreSQL"
                        )));
                    }
                    WriteCmd::CancelReserve { request_id, account_id, key, hold, handoff } => {
                        refund_canceled_reserve(&request_id, &account_id, &key, hold, &handoff);
                    }
                    WriteCmd::Settle {
                        request_id, account_id, key, hold, actual, reference, usage, reply,
                    } => {
                        let result = if actual == 0 && usage.is_none() {
                            registry::sqlite_cancel_request(
                                &conn,
                                &request_id,
                                &account_id,
                                &key,
                                hold,
                            )
                        } else {
                            registry::sqlite_settle_request(
                                &conn,
                                &request_id,
                                &account_id,
                                &key,
                                hold,
                                actual,
                                reference.as_deref(),
                                usage.as_ref(),
                            )
                        };
                        if let Err(error) = &result {
                            eprintln!("billing SQLite settlement persisted/retryable failure: {error:#}");
                        }
                        if let Some(reply) = reply { let _ = reply.send(result); }
                    }
                    WriteCmd::Topup { account_id, amount, reference, reply } => {
                        let _ = reply.send(registry::account_topup(&conn, &account_id, amount, reference.as_deref()));
                    }
                    WriteCmd::CreateAccount { id, handle, mult_bp, reply } => { let _ = reply.send(registry::account_create(&conn, &id, handle.as_deref(), mult_bp)); }
                    WriteCmd::IssueKey { key, account_id, label, spend_limit_nano, expires_ts, activation_policy_ack, reply } => {
                        let _ = reply.send(registry::key_issue_with_policy_ack(
                            &conn,&key,&account_id,label.as_deref(),spend_limit_nano,expires_ts,
                            activation_policy_ack.as_ref(),
                        ));
                    }
                    WriteCmd::AccountStatus { id, status, reply } => { let _ = reply.send(registry::account_set_status(&conn, &id, &status)); }
                    WriteCmd::AccountMultiplier { id, mult_bp, reply } => { let _ = reply.send(registry::account_set_mult_bp(&conn, &id, mult_bp)); }
                    WriteCmd::KeyStatus { key, status, activation_policy_ack, reply } => { let _ = reply.send(registry::key_set_status_with_policy_ack(&conn, &key, &status, activation_policy_ack.as_ref())); }
                    WriteCmd::KeyStatusById { key_id, status, activation_policy_ack, reply } => { let _ = reply.send(registry::key_set_status_by_id_with_policy_ack(&conn, &key_id, &status, activation_policy_ack.as_ref())); }
                    WriteCmd::KeyLabelById { key_id, label, reply } => { let _ = reply.send(registry::key_set_label_by_id(&conn, &key_id, &label)); }
                    WriteCmd::KeyPolicyById { account_id, key_id, spend_limit_nano, expires_ts, reply } => {
                        let _ = reply.send(registry::key_set_policy_by_id(
                            &conn,&account_id,&key_id,spend_limit_nano,expires_ts,
                        ));
                    }
                    WriteCmd::LedgerAck { consumer, account_id, last_id, reply } => {
                        let _ = reply.send(registry::ledger_ack(&conn, &consumer, &account_id, last_id));
                    }
                    WriteCmd::MarkDelivering { request_id, lease_secs, reply } => {
                        let _ = reply.send(registry::sqlite_mark_delivering(
                            &conn, &request_id, lease_secs,
                        ));
                    }
                    WriteCmd::RenewStreamLeases { request_id, lease_secs, reply, .. } => {
                        let result = match request_id {
                            Some(request_id) => registry::sqlite_renew_reservation_lease(
                                &conn, &request_id, lease_secs,
                            ),
                            None => Ok(true),
                        };
                        let _ = reply.send(result);
                    }
                    WriteCmd::AcquireCapacity { lease_id, request_id, email, lease_secs, reply, .. } => {
                        let _ = reply.send(Ok(Some(registry::pg::CapacityLease {
                            lease_id, request_id, subscription_email: email,
                            lease_until: pool::now().saturating_add(lease_secs.max(1)),
                        })));
                    }
                    WriteCmd::ReleaseCapacity { .. } => {}
                    WriteCmd::Flush(reply) => {
                        let result = flush_pending_anthropic_calibration_turns(
                            &writer_anthropic_delivery,
                            None,
                            &persist_anthropic_turn,
                        )
                        .and_then(|_| {
                            flush_pending_gemini_calibration_turns(
                                &writer_gemini_delivery,
                                None,
                                &persist_gemini_turn,
                            )
                        })
                        .and_then(|_| {
                            registry::sqlite_reconcile_expired(&conn, 10_000).map(|_| ())
                        });
                        let _ = reply.send(result);
                    }
                    }
                }
                eprintln!("⚠ billing-writer поток завершён (все sender'ы дропнуты)"); // супервизия
            })?;
        }
        // reader-пул
        let mut rtxs = Vec::with_capacity(readers);
        for i in 0..readers {
            let (rtx, mut rrx) = mpsc::channel::<ReadCmd>(READ_QUEUE_CAPACITY);
            let conn = registry::open(&db_path)?; // своё read-соединение (WAL параллелит чтения)
            std::thread::Builder::new()
                .name(format!("billing-reader-{i}"))
                .spawn(move || {
                    while let Some(cmd) = rrx.blocking_recv() {
                        match cmd {
                            ReadCmd::AnthropicCalibrationReport(reply) => {
                                let result = (|| {
                                    Ok((
                                        registry::list_anthropic_calibrations(&conn)?,
                                        registry::provider_turn_calibration_report(
                                            &conn,
                                            registry::PROVIDER_ANTHROPIC,
                                        )?,
                                        registry::recent_provider_turn_calibration_events(
                                            &conn,
                                            registry::PROVIDER_ANTHROPIC,
                                            registry::MAX_RECENT_PROVIDER_TURN_CALIBRATION_EVENTS,
                                        )?,
                                    ))
                                })();
                                let _ = reply.send(result);
                            }
                            ReadCmd::GeminiCalibrationReport(reply) => {
                                let result = (|| {
                                    Ok((
                                        registry::list_gemini_exact_calibrations(&conn)?,
                                        registry::provider_turn_calibration_report(
                                            &conn,
                                            registry::PROVIDER_GOOGLE,
                                        )?,
                                        registry::recent_provider_turn_calibration_events(
                                            &conn,
                                            registry::PROVIDER_GOOGLE,
                                            registry::MAX_RECENT_PROVIDER_TURN_CALIBRATION_EVENTS,
                                        )?,
                                    ))
                                })();
                                let _ = reply.send(result);
                            }
                            ReadCmd::CodexCalibrationReport(reply) => {
                                let _ = reply.send(registry::codex_turn_calibration_report(&conn));
                            }
                            ReadCmd::KimiCalibrationReport(reply) => {
                                // KIMI calibration authority is PostgreSQL-only; a SQLite
                                // authority simply has no rows to report.
                                let _ = reply.send(Ok(Vec::new()));
                            }
                            ReadCmd::KimiRecentTurns(_limit, reply) => {
                                // Same PostgreSQL-only contract as the calibration report.
                                let _ = reply.send(Ok(Vec::new()));
                            }
                            ReadCmd::KeyAuth(k, r) => {
                                let _ = r.send(registry::key_account(&conn, &k));
                            }
                            ReadCmd::KeyGet(k, r) => {
                                let _ = r.send(registry::key_get(&conn, &k));
                            }
                            ReadCmd::Account(id, r) => {
                                let _ = r.send(registry::account_get(&conn, &id));
                            }
                            ReadCmd::AccountFunding(id, r) => {
                                let _ = r.send(registry::account_funding_snapshot(&conn, &id));
                            }
                            ReadCmd::AccountByHandle(handle, r) => {
                                let _ = r.send(registry::account_by_handle(&conn, &handle));
                            }
                            ReadCmd::Totals(r) => {
                                let _ = r.send(registry::billing_totals(&conn));
                            }
                            ReadCmd::AccountsList(r) => {
                                let _ = r.send(registry::account_list(&conn));
                            }
                            ReadCmd::KeysByAccount(id, r) => {
                                let _ = r.send(registry::keys_by_account(&conn, &id));
                            }
                            ReadCmd::Ledger(id, lim, r) => {
                                let _ = r.send(registry::ledger_recent(&conn, &id, lim));
                            }
                            ReadCmd::LedgerAfter(id, after, lim, r) => {
                                let _ = r.send(registry::ledger_after(&conn, &id, after, lim));
                            }
                            ReadCmd::UsageByModel(id, since, r) => {
                                let _ = r.send(registry::usage_by_model(&conn, &id, since));
                            }
                            ReadCmd::UsageReport(id, since, until, r) => {
                                let _ = r.send(registry::usage_report(&conn, &id, since, until));
                            }
                            ReadCmd::SpendByAccount {
                                since_ts,
                                until_ts,
                                limit,
                                reply,
                            } => {
                                let _ = reply.send(registry::spend_by_account_range(
                                    &conn, since_ts, until_ts, limit,
                                ));
                            }
                            ReadCmd::SpendByProvider {
                                since_ts,
                                until_ts,
                                reply,
                            } => {
                                let _ = reply.send(registry::spend_by_provider_range(
                                    &conn, since_ts, until_ts,
                                ));
                            }
                            ReadCmd::PricingCatalogByGeneration {
                                product_id,
                                generation,
                                reply,
                            } => {
                                let _ = reply.send(
                                    registry::pricing::sqlite_pricing_catalog_by_generation(
                                        &conn,
                                        &product_id,
                                        generation,
                                    ),
                                );
                            }
                            ReadCmd::ActivePricingCatalog { product_id, reply } => {
                                let _ =
                                    reply.send(registry::pricing::sqlite_active_pricing_catalog(
                                        &conn,
                                        &product_id,
                                    ));
                            }
                            ReadCmd::ProviderSwitchesByGeneration { generation, reply } => {
                                let _ = reply.send(
                                    registry::pricing::sqlite_provider_switches_by_generation(
                                        &conn, generation,
                                    ),
                                );
                            }
                            ReadCmd::ActiveProviderSwitches { reply } => {
                                let _ = reply.send(
                                    registry::pricing::sqlite_active_provider_switches(&conn),
                                );
                            }
                            ReadCmd::AccountPolicyByVersion {
                                account_id,
                                effective_version,
                                reply,
                            } => {
                                let _ = reply.send(
                                    registry::pricing::sqlite_account_policy_by_version(
                                        &conn,
                                        &account_id,
                                        effective_version,
                                    ),
                                );
                            }
                            ReadCmd::ActiveAccountPolicy { account_id, reply } => {
                                let _ =
                                    reply.send(registry::pricing::sqlite_active_account_policy(
                                        &conn,
                                        &account_id,
                                    ));
                            }
                            ReadCmd::PricingReadBundle { account_id, reply } => {
                                let _ = reply.send(registry::pricing::sqlite_pricing_read_bundle(
                                    &conn,
                                    &account_id,
                                ));
                            }
                            ReadCmd::PricingReleasePolicyV2 { reply, .. } => {
                                let _ = reply.send(Err(anyhow::anyhow!(
                                    "pricing release v2 authority requires PostgreSQL"
                                )));
                            }
                            ReadCmd::PricingReleaseV2 { reply, .. } => {
                                let _ = reply.send(Err(anyhow::anyhow!(
                                    "pricing release v2 authority requires PostgreSQL"
                                )));
                            }
                            ReadCmd::PricingReleaseRecoveryLinkV2 { reply, .. } => {
                                let _ = reply.send(Err(anyhow::anyhow!(
                                    "pricing release v2 authority requires PostgreSQL"
                                )));
                            }
                            ReadCmd::PricingReleaseAssignmentExtensionV2 { reply, .. } => {
                                let _ = reply.send(Err(anyhow::anyhow!(
                                    "pricing release v2 authority requires PostgreSQL"
                                )));
                            }
                            ReadCmd::PricingReleaseHeadV2 { reply } => {
                                let _ = reply.send(Err(anyhow::anyhow!(
                                    "pricing release v2 authority requires PostgreSQL"
                                )));
                            }
                            ReadCmd::PricingReleaseProvisioningContextV2 { reply } => {
                                let _ = reply.send(Err(anyhow::anyhow!(
                                    "pricing release v2 authority requires PostgreSQL"
                                )));
                            }
                            ReadCmd::PricingReleaseResolutionV2 { reply, .. } => {
                                // SQLite cannot own a release head. Runtime resolution therefore
                                // preserves the legacy path while producer/head reads stay closed.
                                let _ = reply.send(Ok(None));
                            }
                            ReadCmd::PricingReleaseInventoryV2 { reply, .. } => {
                                let _ = reply.send(Err(anyhow::anyhow!(
                                    "pricing release v2 authority requires PostgreSQL"
                                )));
                            }
                            ReadCmd::FundingNormalizationPlanV2 { reply, .. } => {
                                let _ = reply.send(Err(anyhow::anyhow!(
                                    "funding normalization v2 authority requires PostgreSQL"
                                )));
                            }
                            ReadCmd::Stage8EngineEvidence { reply, .. } => {
                                let _ = reply.send(Err(anyhow::anyhow!(
                                    "Stage 8 engine evidence requires PostgreSQL authority"
                                )));
                            }
                            ReadCmd::SpendByModel {
                                since_ts,
                                until_ts,
                                limit,
                                reply,
                            } => {
                                let _ = reply.send(registry::spend_by_model_range(
                                    &conn, since_ts, until_ts, limit,
                                ));
                            }
                            ReadCmd::SettlementHealth(backlog, consumer, r) => {
                                let _ =
                                    r.send(registry::settlement_health(&conn, backlog, &consumer));
                            }
                        }
                    }
                    eprintln!("⚠ billing-reader-{i} поток завершён");
                })?;
            rtxs.push(rtx);
        }
        Ok(AsyncBilling {
            writer: wtx,
            detached: Arc::new(DetachedDispatchTracker::default()),
            anthropic_calibration_delivery,
            gemini_calibration_delivery,
            readers: rtxs,
            rr: AtomicUsize::new(0),
            pricing_shadow_readers: Vec::new(),
            pricing_shadow_rr: AtomicUsize::new(0),
            pg_command: None,
        })
    }

    fn start_postgres(
        url: String,
        owner: registry::pg::Owner,
        readers: usize,
        pricing_shadow_readers: usize,
        _auth_ttl_ms: u64,
    ) -> anyhow::Result<Self> {
        const RESERVATION_LEASE_SECS: i64 = 3600;
        let readers = readers.max(1);
        let (wtx, mut wrx) = mpsc::channel::<WriteCmd>(WRITE_QUEUE_CAPACITY);
        let anthropic_calibration_delivery = Arc::new(AnthropicCalibrationDeliveryState::default());
        let gemini_calibration_delivery = Arc::new(GeminiCalibrationDeliveryState::default());
        let pg_command = Arc::new(PgCommandMetrics::default());
        {
            let mut pg = registry::pg::PgStore::connect(&url)?;
            let writer_url = url.clone();
            let writer_owner = owner.clone();
            let writer_anthropic_delivery = Arc::clone(&anthropic_calibration_delivery);
            let writer_gemini_delivery = Arc::clone(&gemini_calibration_delivery);
            let writer_pg_command = Arc::clone(&pg_command);
            std::thread::Builder::new().name("billing-pg-writer".into()).spawn(move || {
                while let Some(cmd) = wrx.blocking_recv() {
                    match cmd {
                        WriteCmd::AnthropicRecordTurn {
                            delivery_id,
                            reply,
                        } => {
                            if let Some(reply) = reply {
                                let result = flush_pending_anthropic_calibration_turns(
                                    &writer_anthropic_delivery,
                                    Some(delivery_id),
                                    |turn| {
                                        persist_anthropic_turn_postgres(
                                            &mut pg,
                                            &writer_url,
                                            &writer_owner,
                                            turn,
                                        )
                                    },
                                )
                                .and_then(|result| {
                                    result.ok_or_else(|| {
                                        anyhow::anyhow!(
                                            "Anthropic calibration target was not replayed"
                                        )
                                    })
                                });
                                let _ = reply.send(result);
                            } else {
                                let result = flush_pending_anthropic_calibration_turns(
                                    &writer_anthropic_delivery,
                                    None,
                                    |turn| {
                                        persist_anthropic_turn_postgres(
                                            &mut pg,
                                            &writer_url,
                                            &writer_owner,
                                            turn,
                                        )
                                    },
                                );
                                if let Err(error) = result {
                                    eprintln!(
                                        "Anthropic calibration persistence deferred with FIFO head retained: {error:#}"
                                    );
                                }
                            }
                        }
                        WriteCmd::AnthropicObserveWindow {
                            subject_id,
                            plan,
                            snapshot,
                            reply,
                        } => {
                            let result = flush_pending_anthropic_calibration_turns(
                                &writer_anthropic_delivery,
                                None,
                                |turn| {
                                    persist_anthropic_turn_postgres(
                                        &mut pg,
                                        &writer_url,
                                        &writer_owner,
                                        turn,
                                    )
                                },
                            )
                            .and_then(|_| {
                                run_pg_with_retry(
                                    &mut pg,
                                    &writer_url,
                                    &writer_owner,
                                    "Anthropic window observation",
                                    |pg| {
                                        let spend = pg.provider_calibration_subject_spend(
                                            registry::PROVIDER_ANTHROPIC,
                                            &subject_id,
                                        )?;
                                        let observation = anthropic_observation(
                                            &subject_id,
                                            &plan,
                                            &snapshot,
                                            spend.spent_nano,
                                            "poll",
                                            None,
                                        );
                                        let state =
                                            observe_anthropic_postgres(pg, &observation)?;
                                        Ok((spend.spent_nano, state))
                                    },
                                )
                            });
                            let _ = reply.send(result);
                        }
                        WriteCmd::GeminiRecordTurn { delivery_id, reply } => {
                            if let Some(reply) = reply {
                                let result = flush_pending_gemini_calibration_turns(
                                    &writer_gemini_delivery,
                                    Some(delivery_id),
                                    |turn| {
                                        persist_gemini_turn_postgres(
                                            &mut pg,
                                            &writer_url,
                                            &writer_owner,
                                            turn,
                                        )
                                    },
                                )
                                .and_then(|result| {
                                    result.ok_or_else(|| {
                                        anyhow::anyhow!(
                                            "Gemini calibration target was not replayed"
                                        )
                                    })
                                });
                                let _ = reply.send(result);
                            } else {
                                let result = flush_pending_gemini_calibration_turns(
                                    &writer_gemini_delivery,
                                    None,
                                    |turn| {
                                        persist_gemini_turn_postgres(
                                            &mut pg,
                                            &writer_url,
                                            &writer_owner,
                                            turn,
                                        )
                                    },
                                );
                                if let Err(error) = result {
                                    eprintln!(
                                        "Gemini calibration persistence deferred with FIFO head retained: {error:#}"
                                    );
                                }
                            }
                        }
                        WriteCmd::GeminiObserveWindow {
                            profile_id, plan, snapshot, reply,
                        } => {
                            let result = flush_pending_gemini_calibration_turns(
                                &writer_gemini_delivery,
                                None,
                                |turn| {
                                    persist_gemini_turn_postgres(
                                        &mut pg,
                                        &writer_url,
                                        &writer_owner,
                                        turn,
                                    )
                                },
                            )
                            .and_then(|_| {
                                run_pg_with_retry(
                                    &mut pg,
                                    &writer_url,
                                    &writer_owner,
                                    "Gemini window observation",
                                    |pg| {
                                        let spend = pg.provider_calibration_subject_spend(
                                            registry::PROVIDER_GOOGLE,
                                            &profile_id,
                                        )?;
                                        let observation = gemini_observation(
                                            &profile_id,
                                            &plan,
                                            &snapshot,
                                            spend.spent_nano,
                                            "poll",
                                            None,
                                        );
                                        let state = observe_gemini_postgres(pg, &observation)?;
                                        Ok((spend.spent_nano, state))
                                    },
                                )
                            });
                            let _ = reply.send(result);
                        }
                        WriteCmd::KimiRecordTurn { event, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "KIMI turn calibration event",
                                |pg| pg.record_kimi_turn(&event),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::KimiObserveWindows {
                            subject_id,
                            plan,
                            snapshots,
                            reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "KIMI window observations",
                                |pg| {
                                    let spend = pg.kimi_subject_spend(&subject_id)?;
                                    let mut states = Vec::with_capacity(snapshots.len());
                                    for snapshot in &snapshots {
                                        let observation = kimi_observation(
                                            &subject_id,
                                            &plan,
                                            snapshot,
                                            spend,
                                        );
                                        states.push(observe_kimi_postgres(pg, &observation)?);
                                    }
                                    Ok(states)
                                },
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::GlmRecordTurn { event, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "GLM turn calibration event",
                                |pg| pg.record_glm_turn(&event),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::GlmObserveWindows {
                            subject_id,
                            plan,
                            snapshots,
                            reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "GLM window observations",
                                |pg| {
                                    // Durable dual-ledger spend is read FIRST; only then may a
                                    // window observation/CAS pair itself with it. Reversing the
                                    // order would let an observation see a window total that an
                                    // earlier turn has not yet reached.
                                    let spend = pg.glm_subject_spend(&subject_id)?;
                                    let mut states = Vec::with_capacity(snapshots.len());
                                    for snapshot in &snapshots {
                                        let observation = glm_observation(
                                            &subject_id,
                                            &plan,
                                            snapshot,
                                            spend,
                                            "poll",
                                            None,
                                        );
                                        states.push(observe_glm_postgres(pg, &observation)?);
                                    }
                                    Ok(states)
                                },
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::CodexRecordTurn { event, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "Codex turn calibration event",
                                |pg| pg.record_codex_turn_calibration_event(&event),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::CodexLoadHealth { home_id, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "Codex health read",
                                |pg| pg.load_codex_home_health(&home_id),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::CodexSaveHealth { home_id, row, updated_ts, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "Codex health write",
                                |pg| pg.save_codex_home_health(&home_id, &row, updated_ts),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::CodexObserveWindow {
                            home_id,
                            window_duration_mins,
                            resets_at,
                            used_percent,
                            used_fraction_units,
                            observed_at,
                            reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "Codex window observation",
                                |pg| {
                                    let spend = pg.codex_home_calibration_spend(&home_id)?;
                                    let observation = CodexWindowObservation {
                                        home_id: home_id.clone(),
                                        window_duration_mins,
                                        resets_at,
                                        observed_at,
                                        used_percent,
                                        used_fraction_units,
                                        gateway_spend_nano: spend.spent_nano,
                                        gateway_spend_nanocredits: spend.spent_nanocredits,
                                    };
                                    loop {
                                        let existing = pg.load_codex_calibration(
                                            &home_id, window_duration_mins,
                                        )?;
                                        if let Some(existing) = existing
                                            .as_ref()
                                            .filter(|row| {
                                                row.estimator_version
                                                    == crate::codex::ESTIMATOR_VERSION
                                                    && observed_at <= row.observed_at
                                            })
                                        {
                                            return Ok((spend.clone(), existing.clone()));
                                        }
                                        let history = if existing.as_ref().is_some_and(|row| {
                                            row.estimator_version
                                                != crate::codex::ESTIMATOR_VERSION
                                        }) {
                                            pg.load_codex_window_observations(
                                                &home_id,
                                                window_duration_mins,
                                            )?
                                        } else {
                                            Vec::new()
                                        };
                                        let mut state =
                                            crate::codex::apply_observation_with_history(
                                                existing,
                                                &history,
                                                &observation,
                                            )?;
                                        if let Some(version) =
                                            pg.save_codex_calibration(&state, &observation)?
                                        {
                                            state.version = version;
                                            return Ok((spend.clone(), state));
                                        }
                                    }
                                },
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::Reserve { request_id, account_id, key, hold, execution, handoff, reply } => {
                            let result = {
                                let _timer = writer_pg_command.timer(PgCommandOp::Reserve);
                                run_pg_with_retry(
                                    &mut pg,
                                    &writer_url,
                                    &writer_owner,
                                    "reserve",
                                    |pg| pg.reserve_request_for_execution(
                                        &writer_owner, &request_id, &account_id, &key, hold,
                                        RESERVATION_LEASE_SECS, &execution,
                                    ),
                                )
                            };
                            let result = match result {
                                Ok(result) => result,
                                Err(error) => {
                                    eprintln!("billing PostgreSQL reserve failed: {error:#}");
                                    handoff.store(RESERVE_HANDOFF_FAILED, Ordering::Release);
                                    let _ = reply.send(Err(error));
                                    continue;
                                }
                            };
                            if result.is_none() {
                                handoff.store(RESERVE_HANDOFF_FAILED, Ordering::Release);
                                let _ = reply.send(Ok(None));
                                continue;
                            }
                            match handoff.compare_exchange(
                                RESERVE_HANDOFF_PENDING, RESERVE_HANDOFF_COMMITTED,
                                Ordering::AcqRel, Ordering::Acquire,
                            ) {
                                Ok(_) => {
                                    if reply.send(Ok(result)).is_err() {
                                        let _ = handoff.compare_exchange(
                                            RESERVE_HANDOFF_COMMITTED, RESERVE_HANDOFF_CANCELED,
                                            Ordering::AcqRel, Ordering::Acquire,
                                        );
                                        match run_pg_with_retry(
                                            &mut pg, &writer_url, &writer_owner, "canceled reserve",
                                            |pg| pg.cancel_request(&request_id),
                                        ) {
                                            Ok(_) => handoff.store(RESERVE_HANDOFF_REFUNDED, Ordering::Release),
                                            Err(error) => eprintln!("billing PostgreSQL canceled reserve failed: {error:#}"),
                                        }
                                    }
                                }
                                Err(RESERVE_HANDOFF_CANCELED) => {
                                    match run_pg_with_retry(
                                        &mut pg, &writer_url, &writer_owner, "reserve handoff cancel",
                                        |pg| pg.cancel_request(&request_id),
                                    ) {
                                        Ok(_) => handoff.store(RESERVE_HANDOFF_REFUNDED, Ordering::Release),
                                        Err(error) => eprintln!("billing PostgreSQL reserve handoff cancel failed: {error:#}"),
                                    }
                                }
                                Err(state) => eprintln!("billing PostgreSQL reserve handoff unexpected state {state}"),
                            }
                        }
                        WriteCmd::ReserveWithLegacySnapshot { key, snapshot, execution, handoff, reply } => {
                            if handoff.load(Ordering::Acquire)
                                == SNAPSHOT_RESERVE_HANDOFF_CANCELED
                            {
                                let _ = reply.send(Ok(
                                    LegacyScalarReserveOutcome::AbortedBeforeCommit,
                                ));
                                continue;
                            }
                            let result = run_pg_snapshot_reserve_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                &key,
                                &snapshot,
                                &execution,
                                &handoff,
                                RESERVATION_LEASE_SECS,
                            );
                            if let Err(error) = &result {
                                eprintln!(
                                    "billing PostgreSQL legacy snapshot reserve failed: {error:#}"
                                );
                            }
                            finish_snapshot_reserve(&handoff, reply, result);
                        }
                        WriteCmd::ReserveWithPolicySnapshot { key, snapshot, execution, handoff, reply } => {
                            if handoff.load(Ordering::Acquire)
                                == SNAPSHOT_RESERVE_HANDOFF_CANCELED
                            {
                                let _ = reply.send(Ok(
                                    PolicyReserveOutcome::AbortedBeforeCommit,
                                ));
                                continue;
                            }
                            let result = run_pg_policy_snapshot_reserve_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                &key,
                                &snapshot,
                                &execution,
                                &handoff,
                                RESERVATION_LEASE_SECS,
                            );
                            if let Err(error) = &result {
                                eprintln!(
                                    "billing PostgreSQL policy snapshot reserve failed: {error:#}"
                                );
                            }
                            finish_policy_snapshot_reserve(&handoff, reply, result);
                        }
                        WriteCmd::ReserveWithPricingReleaseV2 {
                            key, resolution, quote, execution, handoff, reply,
                        } => {
                            if handoff.load(Ordering::Acquire)
                                == SNAPSHOT_RESERVE_HANDOFF_CANCELED
                            {
                                let _ = reply.send(Ok(
                                    PricingReleaseReserveOutcomeV2::AbortedBeforeCommit,
                                ));
                                continue;
                            }
                            let result = run_pg_pricing_release_reserve_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                &key,
                                &resolution,
                                &quote,
                                &execution,
                                &handoff,
                                RESERVATION_LEASE_SECS,
                            );
                            if let Err(error) = &result {
                                eprintln!(
                                    "billing PostgreSQL pricing release reserve failed: {error:#}"
                                );
                            }
                            finish_pricing_release_reserve(&handoff, reply, result);
                        }
                        WriteCmd::InsertPricingShadowEvaluation { input, timeout_ms, reply } => {
                            // Shadow is deliberately best-effort: one bounded database attempt,
                            // with no five-second retry loop that could head-of-line block money.
                            let result = pg.insert_pricing_shadow_admission_evaluation_with_timeout(
                                &input,
                                timeout_ms,
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::PreparePricingCatalog { spec, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "pricing catalog prepare",
                                |pg| pg.prepare_pricing_catalog(&spec),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::ActivatePricingCatalog {
                            product_id, target, expectation, reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "pricing catalog activation",
                                |pg| pg.activate_pricing_catalog(
                                    &product_id,
                                    &target,
                                    &expectation,
                                ),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::PrepareProviderSwitches { spec, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "provider switches prepare",
                                |pg| pg.prepare_provider_switches(&spec),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::ActivateProviderSwitches { target, expectation, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "provider switches activation",
                                |pg| pg.activate_provider_switches(&target, &expectation),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::PrepareAccountPolicy { spec, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "account pricing policy prepare",
                                |pg| pg.prepare_account_policy(&spec),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::LockedOpenKeysPolicyTransition { transition, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "locked OpenKeys policy transition",
                                |pg| pg.locked_openkeys_policy_transition(&transition),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::ActivateAccountPolicy { activation, expectation, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "account pricing policy activation",
                                |pg| pg.activate_account_policy(&activation, &expectation),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::PreparePricingReleasePolicyV2 { policy, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "pricing release policy v2 prepare",
                                |pg| pg.prepare_pricing_release_policy_v2(&policy),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::PreparePricingReleaseV2 { release, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "pricing release v2 prepare",
                                |pg| pg.prepare_pricing_release_v2(&release),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::PreparePricingReleaseRecoveryLinkV2 { link, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "pricing release recovery link v2 prepare",
                                |pg| pg.prepare_pricing_release_recovery_link_v2(&link),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::PreparePricingReleaseAssignmentExtensionV2 {
                            extension,
                            reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "pricing assignment extension v2 prepare",
                                |pg| pg.prepare_pricing_release_assignment_extension_v2(&extension),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::ActivatePricingReleaseV2 {
                            request,
                            runtime_manifest,
                            reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "pricing release v2 activation",
                                |pg| {
                                    pg.activate_pricing_release_v2(&request, &runtime_manifest)
                                },
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::ApplyFundingNormalizationV2 {
                            account_id,
                            request,
                            reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "account-local funding normalization v2",
                                |pg| pg.apply_funding_normalization_v2(&account_id, &request),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::CancelReserve { request_id, handoff, .. } => {
                            if handoff.compare_exchange(
                                RESERVE_HANDOFF_CANCELED, RESERVE_HANDOFF_REFUNDING,
                                Ordering::AcqRel, Ordering::Acquire,
                            ).is_err() { continue; }
                            match run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "cancellation",
                                |pg| pg.cancel_request(&request_id),
                            ) {
                                Ok(_) => handoff.store(RESERVE_HANDOFF_REFUNDED, Ordering::Release),
                                Err(error) => {
                                    eprintln!("billing PostgreSQL cancellation failed: {error:#}");
                                    handoff.store(RESERVE_HANDOFF_CANCELED, Ordering::Release);
                                }
                            }
                        }
                        WriteCmd::Settle { request_id, actual, reference, usage, reply, .. } => {
                            let result = {
                                let _timer = writer_pg_command.timer(PgCommandOp::Settle);
                                run_pg_with_retry(
                                    &mut pg, &writer_url, &writer_owner, "settlement",
                                    |pg| {
                                        if actual == 0 && usage.is_none() {
                                            pg.cancel_request(&request_id)
                                        } else {
                                            pg.settle_request(
                                                &request_id,
                                                actual,
                                                reference.as_deref(),
                                                usage.as_ref(),
                                            )
                                        }
                                    },
                                )
                            };
                            if let Err(error) = &result {
                                eprintln!("billing PostgreSQL settlement failed: {error:#}");
                            }
                            if let Some(reply) = reply { let _ = reply.send(result); }
                        }
                        WriteCmd::MarkDelivering { request_id, lease_secs, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "delivery marker",
                                |pg| pg.mark_delivering(&writer_owner, &request_id, lease_secs),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::RenewStreamLeases {
                            request_id, capacity_lease_id, lease_secs, reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "stream lease renewal",
                                |pg| pg.renew_stream_leases(
                                    &writer_owner,
                                    request_id.as_deref(),
                                    capacity_lease_id.as_deref(),
                                    lease_secs,
                                ),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::AcquireCapacity { lease_id, request_id, email, lease_secs,
                                                    util_cap, reply } => {
                            let result = {
                                let _timer = writer_pg_command.timer(PgCommandOp::AcquireCapacity);
                                run_pg_with_retry(
                                    &mut pg, &writer_url, &writer_owner, "capacity acquisition",
                                    |pg| pg.acquire_capacity(
                                        &writer_owner,&lease_id,&request_id,&email,lease_secs,util_cap,
                                    ),
                                )
                            };
                            let _ = reply.send(result);
                        }
                        WriteCmd::ReleaseCapacity { lease_id } => {
                            if let Err(error) = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "capacity release",
                                |pg| pg.release_capacity(&writer_owner, &lease_id),
                            ) {
                                eprintln!("capacity lease release failed: {error:#}");
                            }
                        }
                        WriteCmd::Topup { account_id, amount, reference, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "account top-up",
                                |pg| pg.account_topup(&account_id, amount, reference.as_deref()),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::CreateAccount { id, handle, mult_bp, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "account creation",
                                |pg| pg.account_create(&id,handle.as_deref(),mult_bp),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::IssueKey { key, account_id, label, spend_limit_nano, expires_ts, activation_policy_ack, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "key issuance",
                                |pg| pg.key_issue_with_policy_ack(
                                    &key,&account_id,label.as_deref(),spend_limit_nano,expires_ts,
                                    activation_policy_ack.as_ref(),
                                ),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::AccountStatus { id, status, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "account status update",
                                |pg| pg.account_set_status(&id,&status),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::AccountMultiplier { id, mult_bp, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "account pricing update",
                                |pg| pg.account_set_mult_bp(&id,mult_bp),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::KeyStatus { key, status, activation_policy_ack, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "key status update",
                                |pg| pg.key_set_status_with_policy_ack(
                                    &key,&status,activation_policy_ack.as_ref(),
                                ),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::KeyStatusById { key_id, status, activation_policy_ack, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "key status update",
                                |pg| pg.key_set_status_by_id_with_policy_ack(
                                    &key_id,&status,activation_policy_ack.as_ref(),
                                ),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::KeyLabelById { key_id, label, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "key label update",
                                |pg| pg.key_set_label_by_id(&key_id,&label),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::KeyPolicyById { account_id, key_id, spend_limit_nano, expires_ts, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "key policy update",
                                |pg| pg.key_set_policy_by_id(
                                    &account_id,&key_id,spend_limit_nano,expires_ts,
                                ),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::LedgerAck { consumer, account_id, last_id, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "ledger checkpoint",
                                |pg| pg.ledger_ack(&consumer, &account_id, last_id),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::Flush(reply) => {
                            let result = flush_pending_anthropic_calibration_turns(
                                &writer_anthropic_delivery,
                                None,
                                |turn| {
                                    persist_anthropic_turn_postgres(
                                        &mut pg,
                                        &writer_url,
                                        &writer_owner,
                                        turn,
                                    )
                                },
                            )
                            .and_then(|_| {
                                flush_pending_gemini_calibration_turns(
                                    &writer_gemini_delivery,
                                    None,
                                    |turn| {
                                        persist_gemini_turn_postgres(
                                            &mut pg,
                                            &writer_url,
                                            &writer_owner,
                                            turn,
                                        )
                                    },
                                )
                            })
                            .and_then(|_| {
                                loop {
                                    match run_pg_with_retry(
                                        &mut pg, &writer_url, &writer_owner, "outbox drain",
                                        |pg| pg.drain_outbox(10_000),
                                    ) {
                                        Ok(0) => break Ok(()),
                                        Ok(_) => continue,
                                        Err(error) => {
                                            eprintln!("billing PostgreSQL outbox drain failed: {error:#}");
                                            break Err(error);
                                        }
                                    }
                                }
                            });
                            let _ = reply.send(result);
                        }
                    }
                }
                eprintln!("billing-pg-writer thread stopped");
            })?;
        }

        let mut rtxs = Vec::with_capacity(readers);
        for i in 0..readers {
            let (rtx, mut rrx) = mpsc::channel::<ReadCmd>(READ_QUEUE_CAPACITY);
            let mut pg = registry::pg::PgStore::connect(&url)?;
            let reader_url = url.clone();
            std::thread::Builder::new()
                .name(format!("billing-pg-reader-{i}"))
                .spawn(move || {
                    while let Some(cmd) = rrx.blocking_recv() {
                        macro_rules! answer {
                            ($reply:expr, $call:expr) => {{
                                match $call {
                                    Ok(value) => {
                                        let _ = $reply.send(Ok(value));
                                    }
                                    Err(err) => {
                                        eprintln!("billing PostgreSQL read failed closed: {err:#}");
                                        if let Ok(next) =
                                            registry::pg::PgStore::connect(&reader_url)
                                        {
                                            pg = next;
                                        }
                                        let _ = $reply.send(Err(err));
                                    }
                                }
                            }};
                        }
                        match cmd {
                            ReadCmd::AnthropicCalibrationReport(reply) => {
                                let result = (|| {
                                    Ok((
                                        pg.list_anthropic_calibrations()?,
                                        pg.provider_turn_calibration_report(
                                            registry::PROVIDER_ANTHROPIC,
                                        )?,
                                        pg.recent_provider_turn_calibration_events(
                                            registry::PROVIDER_ANTHROPIC,
                                            registry::MAX_RECENT_PROVIDER_TURN_CALIBRATION_EVENTS,
                                        )?,
                                    ))
                                })();
                                answer!(reply, result)
                            }
                            ReadCmd::GeminiCalibrationReport(reply) => {
                                let result = (|| {
                                    Ok((
                                        pg.list_gemini_exact_calibrations()?,
                                        pg.provider_turn_calibration_report(
                                            registry::PROVIDER_GOOGLE,
                                        )?,
                                        pg.recent_provider_turn_calibration_events(
                                            registry::PROVIDER_GOOGLE,
                                            registry::MAX_RECENT_PROVIDER_TURN_CALIBRATION_EVENTS,
                                        )?,
                                    ))
                                })();
                                answer!(reply, result)
                            }
                            ReadCmd::CodexCalibrationReport(reply) => {
                                answer!(reply, pg.codex_turn_calibration_report())
                            }
                            ReadCmd::KimiCalibrationReport(reply) => {
                                answer!(reply, pg.list_kimi_calibrations())
                            }
                            ReadCmd::KimiRecentTurns(limit, reply) => {
                                answer!(reply, pg.list_kimi_recent_turns(limit))
                            }
                            ReadCmd::KeyAuth(k, r) => answer!(r, pg.key_account(&k)),
                            ReadCmd::KeyGet(k, r) => answer!(r, pg.key_get(&k)),
                            ReadCmd::Account(id, r) => answer!(r, pg.account_get(&id)),
                            ReadCmd::AccountFunding(id, r) => {
                                answer!(r, pg.account_funding_snapshot(&id))
                            }
                            ReadCmd::AccountByHandle(handle, r) => {
                                answer!(r, pg.account_by_handle(&handle))
                            }
                            ReadCmd::Totals(r) => answer!(r, pg.billing_totals()),
                            ReadCmd::AccountsList(r) => answer!(r, pg.account_list()),
                            ReadCmd::KeysByAccount(id, r) => answer!(r, pg.keys_by_account(&id)),
                            ReadCmd::Ledger(id, lim, r) => answer!(r, pg.ledger_recent(&id, lim)),
                            ReadCmd::LedgerAfter(id, after, lim, r) => {
                                answer!(r, pg.ledger_after(&id, after, lim))
                            }
                            ReadCmd::UsageByModel(id, since, r) => {
                                answer!(r, pg.usage_by_model(&id, since))
                            }
                            ReadCmd::UsageReport(id, since, until, r) => {
                                answer!(r, pg.usage_report(&id, since, until))
                            }
                            ReadCmd::SpendByAccount {
                                since_ts,
                                until_ts,
                                limit,
                                reply,
                            } => {
                                answer!(reply, pg.spend_by_account_range(since_ts, until_ts, limit))
                            }
                            ReadCmd::SpendByProvider {
                                since_ts,
                                until_ts,
                                reply,
                            } => {
                                answer!(reply, pg.spend_by_provider_range(since_ts, until_ts))
                            }
                            ReadCmd::PricingCatalogByGeneration {
                                product_id,
                                generation,
                                reply,
                            } => answer!(
                                reply,
                                pg.pricing_catalog_by_generation(&product_id, generation)
                            ),
                            ReadCmd::ActivePricingCatalog { product_id, reply } => {
                                answer!(reply, pg.active_pricing_catalog(&product_id))
                            }
                            ReadCmd::ProviderSwitchesByGeneration { generation, reply } => {
                                answer!(reply, pg.provider_switches_by_generation(generation))
                            }
                            ReadCmd::ActiveProviderSwitches { reply } => {
                                answer!(reply, pg.active_provider_switches())
                            }
                            ReadCmd::AccountPolicyByVersion {
                                account_id,
                                effective_version,
                                reply,
                            } => answer!(
                                reply,
                                pg.account_policy_by_version(&account_id, effective_version)
                            ),
                            ReadCmd::ActiveAccountPolicy { account_id, reply } => {
                                answer!(reply, pg.active_account_policy(&account_id))
                            }
                            ReadCmd::PricingReadBundle { account_id, reply } => {
                                answer!(reply, pg.pricing_read_bundle(&account_id))
                            }
                            ReadCmd::PricingReleasePolicyV2 {
                                policy_id,
                                policy_version,
                                reply,
                            } => answer!(
                                reply,
                                pg.pricing_release_policy_v2(&policy_id, policy_version)
                            ),
                            ReadCmd::PricingReleaseV2 { generation, reply } => {
                                answer!(reply, pg.pricing_release_v2(generation))
                            }
                            ReadCmd::PricingReleaseRecoveryLinkV2 {
                                target_generation,
                                recovery_generation,
                                reply,
                            } => answer!(
                                reply,
                                pg.pricing_release_recovery_link_v2(
                                    target_generation,
                                    recovery_generation,
                                )
                            ),
                            ReadCmd::PricingReleaseAssignmentExtensionV2 {
                                provisioning_head_version,
                                account_id,
                                reply,
                            } => answer!(
                                reply,
                                pg.pricing_release_assignment_extension_v2(
                                    provisioning_head_version,
                                    &account_id,
                                )
                            ),
                            ReadCmd::PricingReleaseHeadV2 { reply } => {
                                answer!(reply, pg.pricing_release_head_v2())
                            }
                            ReadCmd::PricingReleaseProvisioningContextV2 { reply } => {
                                answer!(reply, pg.pricing_release_provisioning_context_v2())
                            }
                            ReadCmd::PricingReleaseResolutionV2 {
                                account_id,
                                provider_id,
                                canonical_model_id,
                                reply,
                            } => answer!(
                                reply,
                                pg.pricing_release_resolution_v2(
                                    &account_id,
                                    &provider_id,
                                    &canonical_model_id,
                                )
                            ),
                            ReadCmd::PricingReleaseInventoryV2 {
                                after_account_id,
                                limit,
                                reply,
                            } => answer!(
                                reply,
                                pg.pricing_release_inventory_v2(
                                    after_account_id.as_deref(),
                                    limit,
                                )
                            ),
                            ReadCmd::FundingNormalizationPlanV2 { account_id, reply } => {
                                answer!(reply, pg.funding_normalization_plan_v2(&account_id))
                            }
                            ReadCmd::Stage8EngineEvidence { request, reply } => {
                                answer!(reply, pg.stage8_engine_evidence(&request))
                            }
                            ReadCmd::SpendByModel {
                                since_ts,
                                until_ts,
                                limit,
                                reply,
                            } => {
                                answer!(reply, pg.spend_by_model_range(since_ts, until_ts, limit))
                            }
                            ReadCmd::SettlementHealth(backlog, consumer, r) => {
                                answer!(r, pg.settlement_health(backlog, &consumer))
                            }
                        }
                    }
                })?;
            rtxs.push(rtx);
        }
        let mut pricing_shadow_rtxs = Vec::with_capacity(pricing_shadow_readers);
        for i in 0..pricing_shadow_readers {
            let (rtx, mut rrx) =
                mpsc::channel::<PricingShadowReadCmd>(PRICING_SHADOW_READ_QUEUE_CAPACITY);
            let mut pg = registry::pg::PgStore::connect(&url)?;
            let reader_url = url.clone();
            std::thread::Builder::new()
                .name(format!("billing-pg-pricing-shadow-reader-{i}"))
                .spawn(move || {
                    while let Some(cmd) = rrx.blocking_recv() {
                        match cmd {
                            PricingShadowReadCmd::Bundle {
                                account_id,
                                timeout_ms,
                                reply,
                            } => {
                                let result =
                                    pg.pricing_read_bundle_with_timeout(&account_id, timeout_ms);
                                if result.is_err() {
                                    // No per-request log: bounded counters in the worker own
                                    // observability and prevent account/error-storm disclosure.
                                    if let Ok(next) = registry::pg::PgStore::connect(&reader_url) {
                                        pg = next;
                                    }
                                }
                                let _ = reply.send(result);
                            }
                        }
                    }
                })?;
            pricing_shadow_rtxs.push(rtx);
        }

        Ok(AsyncBilling {
            writer: wtx,
            detached: Arc::new(DetachedDispatchTracker::default()),
            anthropic_calibration_delivery,
            gemini_calibration_delivery,
            readers: rtxs,
            rr: AtomicUsize::new(0),
            pricing_shadow_readers: pricing_shadow_rtxs,
            pricing_shadow_rr: AtomicUsize::new(0),
            pg_command: Some(pg_command),
        })
    }

    fn reader(&self) -> &mpsc::Sender<ReadCmd> {
        let i = self.rr.fetch_add(1, Ordering::Relaxed) % self.readers.len();
        &self.readers[i]
    }

    fn pricing_shadow_reader(&self) -> anyhow::Result<&mpsc::Sender<PricingShadowReadCmd>> {
        if self.pricing_shadow_readers.is_empty() {
            anyhow::bail!("pricing shadow reader budget is disabled");
        }
        let i = self.pricing_shadow_rr.fetch_add(1, Ordering::Relaxed)
            % self.pricing_shadow_readers.len();
        Ok(&self.pricing_shadow_readers[i])
    }

    pub(crate) fn pricing_shadow_readers_enabled(&self) -> bool {
        !self.pricing_shadow_readers.is_empty()
    }

    pub async fn pricing_shadow_read_bundle(
        &self,
        account_id: &str,
        timeout_ms: u64,
    ) -> anyhow::Result<PricingReadBundle> {
        let (reply, result) = oneshot::channel();
        self.pricing_shadow_reader()?
            .send(PricingShadowReadCmd::Bundle {
                account_id: account_id.to_owned(),
                timeout_ms,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("pricing shadow reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("pricing shadow reader stopped"))?
    }

    pub async fn insert_pricing_shadow_evaluation(
        &self,
        input: PricingShadowAdmissionEvaluationInput,
        timeout_ms: u64,
    ) -> anyhow::Result<PricingShadowEvaluationWrite> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::InsertPricingShadowEvaluation {
                input,
                timeout_ms,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn prepare_pricing_catalog(
        &self,
        spec: PricingCatalogSpec,
    ) -> anyhow::Result<PricingMutation> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::PreparePricingCatalog { spec, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn activate_pricing_catalog(
        &self,
        product_id: &str,
        target: VersionTarget,
        expectation: ActiveExpectation,
    ) -> anyhow::Result<PricingMutation> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::ActivatePricingCatalog {
                product_id: product_id.to_owned(),
                target,
                expectation,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn prepare_provider_switches(
        &self,
        spec: ProviderSwitchSpec,
    ) -> anyhow::Result<PricingMutation> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::PrepareProviderSwitches { spec, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn activate_provider_switches(
        &self,
        target: VersionTarget,
        expectation: ActiveExpectation,
    ) -> anyhow::Result<PricingMutation> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::ActivateProviderSwitches {
                target,
                expectation,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn prepare_account_policy(
        &self,
        spec: AccountPolicySpec,
    ) -> anyhow::Result<PricingMutation> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::PrepareAccountPolicy { spec, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn locked_openkeys_policy_transition(
        &self,
        transition: LockedOpenKeysPolicyTransitionSpec,
    ) -> anyhow::Result<PricingMutation> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::LockedOpenKeysPolicyTransition { transition, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn activate_account_policy(
        &self,
        activation: AccountPolicyActivationSpec,
        expectation: PolicyActiveExpectation,
    ) -> anyhow::Result<PricingMutation> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::ActivateAccountPolicy {
                activation,
                expectation,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn prepare_pricing_release_policy_v2(
        &self,
        policy: PricingReleasePolicyV2,
    ) -> anyhow::Result<PricingMutation> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::PreparePricingReleasePolicyV2 { policy, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn prepare_pricing_release_v2(
        &self,
        release: PricingReleaseV2,
    ) -> anyhow::Result<PricingMutation> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::PreparePricingReleaseV2 { release, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn prepare_pricing_release_recovery_link_v2(
        &self,
        link: PricingReleaseRecoveryLinkV2,
    ) -> anyhow::Result<PricingMutation> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::PreparePricingReleaseRecoveryLinkV2 { link, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn prepare_pricing_release_assignment_extension_v2(
        &self,
        extension: PricingReleaseAssignmentExtensionV2,
    ) -> anyhow::Result<PricingMutation> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::PreparePricingReleaseAssignmentExtensionV2 { extension, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn activate_pricing_release_v2(
        &self,
        request: PricingReleaseActivationRequestV2,
        runtime_manifest: PricingRuntimeManifestEvidence,
    ) -> anyhow::Result<PricingReleaseActivationOutcomeV2> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::ActivatePricingReleaseV2 {
                request,
                runtime_manifest,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn apply_funding_normalization_v2(
        &self,
        account_id: &str,
        request: FundingNormalizationApplyRequestV2,
    ) -> anyhow::Result<Option<FundingNormalizationApplyResultV2>> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::ApplyFundingNormalizationV2 {
                account_id: account_id.to_owned(),
                request,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    pub async fn pricing_catalog_by_generation(
        &self,
        product_id: &str,
        generation: i64,
    ) -> anyhow::Result<Option<PricingCatalogSpec>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::PricingCatalogByGeneration {
                product_id: product_id.to_owned(),
                generation,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn active_pricing_catalog(
        &self,
        product_id: &str,
    ) -> anyhow::Result<Option<PricingCatalogSpec>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::ActivePricingCatalog {
                product_id: product_id.to_owned(),
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn provider_switches_by_generation(
        &self,
        generation: i64,
    ) -> anyhow::Result<Option<ProviderSwitchSpec>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::ProviderSwitchesByGeneration { generation, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn active_provider_switches(&self) -> anyhow::Result<Option<ProviderSwitchSpec>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::ActiveProviderSwitches { reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn account_policy_by_version(
        &self,
        account_id: &str,
        effective_version: i64,
    ) -> anyhow::Result<Option<AccountPolicySpec>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::AccountPolicyByVersion {
                account_id: account_id.to_owned(),
                effective_version,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn active_account_policy(
        &self,
        account_id: &str,
    ) -> anyhow::Result<Option<ActiveAccountPolicy>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::ActiveAccountPolicy {
                account_id: account_id.to_owned(),
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn pricing_read_bundle(&self, account_id: &str) -> anyhow::Result<PricingReadBundle> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::PricingReadBundle {
                account_id: account_id.to_owned(),
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn pricing_release_policy_v2(
        &self,
        policy_id: &str,
        policy_version: i64,
    ) -> anyhow::Result<Option<PricingReleasePolicyV2>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::PricingReleasePolicyV2 {
                policy_id: policy_id.to_owned(),
                policy_version,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn pricing_release_v2(
        &self,
        generation: i64,
    ) -> anyhow::Result<Option<PricingReleaseV2>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::PricingReleaseV2 { generation, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn pricing_release_recovery_link_v2(
        &self,
        target_generation: i64,
        recovery_generation: i64,
    ) -> anyhow::Result<Option<PricingReleaseRecoveryLinkV2>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::PricingReleaseRecoveryLinkV2 {
                target_generation,
                recovery_generation,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn pricing_release_assignment_extension_v2(
        &self,
        provisioning_head_version: i64,
        account_id: &str,
    ) -> anyhow::Result<Option<PricingReleaseAssignmentExtensionV2>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::PricingReleaseAssignmentExtensionV2 {
                provisioning_head_version,
                account_id: account_id.to_owned(),
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn pricing_release_head_v2(&self) -> anyhow::Result<Option<PricingReleaseHeadV2>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::PricingReleaseHeadV2 { reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn pricing_release_provisioning_context_v2(
        &self,
    ) -> anyhow::Result<Option<PricingReleaseProvisioningContextV2>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::PricingReleaseProvisioningContextV2 { reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn pricing_release_resolution_v2(
        &self,
        account_id: &str,
        provider_id: &str,
        canonical_model_id: &str,
    ) -> anyhow::Result<Option<PricingReleaseResolutionV2>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::PricingReleaseResolutionV2 {
                account_id: account_id.to_owned(),
                provider_id: provider_id.to_owned(),
                canonical_model_id: canonical_model_id.to_owned(),
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn pricing_release_inventory_v2(
        &self,
        after_account_id: Option<&str>,
        limit: i64,
    ) -> anyhow::Result<PricingReleaseInventoryPageV2> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::PricingReleaseInventoryV2 {
                after_account_id: after_account_id.map(str::to_owned),
                limit,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn funding_normalization_plan_v2(
        &self,
        account_id: &str,
    ) -> anyhow::Result<Option<FundingNormalizationPlanV2>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::FundingNormalizationPlanV2 {
                account_id: account_id.to_owned(),
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    /// Capture one read-only Stage 8 report on a bounded PostgreSQL reader. The registry performs
    /// the full evidence scan in a repeatable-read, read-only transaction; this actor never sends
    /// the request through the billing writer or changes release/money state.
    pub async fn stage8_engine_evidence(
        &self,
        request: registry::stage8::Stage8EngineEvidenceRequest,
    ) -> anyhow::Result<registry::stage8::Stage8EngineEvidenceReport> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::Stage8EngineEvidence { request, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    pub async fn key_auth(&self, key: &str) -> anyhow::Result<Option<KeyAuth>> {
        // Policies are mutable. Even an unrestricted cached key can gain a limit or expiry on a
        // different engine instance, so authorization must always read the shared authority.
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::KeyAuth(key.into(), r))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    pub async fn get(&self, key: &str) -> anyhow::Result<Option<KeyRow>> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::KeyGet(key.into(), r))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    pub async fn account(&self, id: &str) -> anyhow::Result<Option<AccountRow>> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::Account(id.into(), r))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    pub async fn account_funding(
        &self,
        id: &str,
    ) -> anyhow::Result<Option<AccountFundingSnapshot>> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::AccountFunding(id.into(), r))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    pub async fn account_by_handle(&self, handle: &str) -> anyhow::Result<Option<AccountRow>> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::AccountByHandle(handle.into(), r))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    pub async fn totals(&self) -> anyhow::Result<BillingTotals> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::Totals(r))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    pub async fn accounts(&self) -> anyhow::Result<Vec<AccountRow>> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::AccountsList(r))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    pub async fn keys_by_account(&self, account_id: &str) -> anyhow::Result<Vec<KeyRow>> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::KeysByAccount(account_id.into(), r))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    pub async fn ledger(
        &self,
        account_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<registry::LedgerRow>> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::Ledger(account_id.into(), limit, r))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    pub async fn ledger_after(
        &self,
        account_id: &str,
        after_id: i64,
        limit: i64,
    ) -> anyhow::Result<Vec<registry::LedgerRow>> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::LedgerAfter(account_id.into(), after_id, limit, r))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    pub async fn reserve_request(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
    ) -> anyhow::Result<Option<i64>> {
        self.reserve_request_for_execution(
            request_id,
            account_id,
            key,
            hold,
            registry::ExecutionAttempt::direct(),
        )
        .await
    }

    pub async fn reserve_request_for_execution(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
        execution: registry::ExecutionAttempt,
    ) -> anyhow::Result<Option<i64>> {
        let (r, rx) = oneshot::channel();
        let handoff = Arc::new(AtomicU8::new(RESERVE_HANDOFF_PENDING));
        let guard = ReserveHandoffGuard {
            writer: &self.writer,
            detached: self.detached.clone(),
            request_id: request_id.into(),
            account_id: account_id.into(),
            key: key.into(),
            hold,
            handoff: Arc::clone(&handoff),
        };
        self.writer
            .send(WriteCmd::Reserve {
                request_id: request_id.into(),
                account_id: account_id.into(),
                key: key.into(),
                hold,
                execution,
                handoff,
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        match rx
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))??
        {
            Some(balance) if guard.claim() => Ok(Some(balance)),
            Some(_) => Err(anyhow::anyhow!("reservation handoff was canceled")),
            None => Ok(None),
        }
    }
    /// Atomically persists a legacy scalar reservation together with its immutable pricing
    /// snapshot. Default-off sampled Anthropic/OpenAI callers use this path; scalar fallbacks keep
    /// using `reserve_request` and never retry through this method after an atomic-path failure.
    pub async fn reserve_request_with_legacy_snapshot(
        &self,
        key: &str,
        snapshot: LegacyScalarAdmissionSnapshot,
    ) -> anyhow::Result<LegacyScalarReserveOutcome> {
        self.reserve_request_with_legacy_snapshot_for_execution(
            key,
            snapshot,
            registry::ExecutionAttempt::direct(),
        )
        .await
    }

    pub async fn reserve_request_with_legacy_snapshot_for_execution(
        &self,
        key: &str,
        snapshot: LegacyScalarAdmissionSnapshot,
        execution: registry::ExecutionAttempt,
    ) -> anyhow::Result<LegacyScalarReserveOutcome> {
        let (reply, result) = oneshot::channel();
        let handoff = Arc::new(AtomicU8::new(SNAPSHOT_RESERVE_HANDOFF_PENDING));
        let guard = SnapshotReserveHandoffGuard {
            handoff: Arc::clone(&handoff),
        };
        self.writer
            .send(WriteCmd::ReserveWithLegacySnapshot {
                key: key.into(),
                snapshot,
                execution,
                handoff,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        let outcome = result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))??;
        if matches!(
            &outcome,
            LegacyScalarReserveOutcome::Inserted(_) | LegacyScalarReserveOutcome::Unchanged(_)
        ) && !guard.claim()
        {
            return Err(anyhow::anyhow!(
                "snapshot reservation handoff was not claimable"
            ));
        }
        Ok(outcome)
    }
    /// Atomically revalidates and persists a strict policy admission together with its ordered
    /// funding allocations. As with the legacy snapshot bridge, caller cancellation can win only
    /// before the writer's final commit gate; a decided commit remains replayable and recoverable.
    pub async fn reserve_request_with_policy_snapshot(
        &self,
        key: &str,
        snapshot: PolicyAdmissionSnapshot,
    ) -> anyhow::Result<PolicyReserveOutcome> {
        self.reserve_request_with_policy_snapshot_for_execution(
            key,
            snapshot,
            registry::ExecutionAttempt::direct(),
        )
        .await
    }

    pub async fn reserve_request_with_policy_snapshot_for_execution(
        &self,
        key: &str,
        snapshot: PolicyAdmissionSnapshot,
        execution: registry::ExecutionAttempt,
    ) -> anyhow::Result<PolicyReserveOutcome> {
        let (reply, result) = oneshot::channel();
        let handoff = Arc::new(AtomicU8::new(SNAPSHOT_RESERVE_HANDOFF_PENDING));
        let guard = SnapshotReserveHandoffGuard {
            handoff: Arc::clone(&handoff),
        };
        self.writer
            .send(WriteCmd::ReserveWithPolicySnapshot {
                key: key.into(),
                snapshot,
                execution,
                handoff,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        let outcome = result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))??;
        if matches!(
            &outcome,
            PolicyReserveOutcome::Inserted(_) | PolicyReserveOutcome::Unchanged(_)
        ) && !guard.claim()
        {
            return Err(anyhow::anyhow!(
                "policy snapshot reservation handoff was not claimable"
            ));
        }
        Ok(outcome)
    }

    pub async fn reserve_request_with_pricing_release_v2_for_execution(
        &self,
        key: &str,
        resolution: PricingReleaseResolutionV2,
        quote: PricingReleaseQuoteV2,
        execution: registry::ExecutionAttempt,
    ) -> anyhow::Result<PricingReleaseReserveOutcomeV2> {
        let (reply, result) = oneshot::channel();
        let handoff = Arc::new(AtomicU8::new(SNAPSHOT_RESERVE_HANDOFF_PENDING));
        let guard = SnapshotReserveHandoffGuard {
            handoff: Arc::clone(&handoff),
        };
        self.writer
            .send(WriteCmd::ReserveWithPricingReleaseV2 {
                key: key.to_owned(),
                resolution,
                quote,
                execution,
                handoff,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        let outcome = result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))??;
        if matches!(
            &outcome,
            PricingReleaseReserveOutcomeV2::Inserted(_)
                | PricingReleaseReserveOutcomeV2::Unchanged(_)
        ) && !guard.claim()
        {
            return Err(anyhow::anyhow!(
                "pricing release reservation handoff was not claimable"
            ));
        }
        Ok(outcome)
    }
    pub async fn settle_request(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
        actual: i64,
        reference: Option<&str>,
    ) -> anyhow::Result<Option<i64>> {
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::Settle {
                request_id: request_id.into(),
                account_id: account_id.into(),
                key: key.into(),
                hold,
                actual,
                reference: reference.map(|s| s.into()),
                usage: None,
                reply: Some(r),
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Await one terminal settlement together with its exact provider-usage attribution.
    /// Streaming finalizers use this when provider evidence must be ordered before a later quota
    /// observation; unlike `settle_detached`, completion means the single writer applied both.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn settle_request_with_usage(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
        actual: i64,
        reference: Option<&str>,
        usage: Option<registry::UsageEventInput>,
    ) -> anyhow::Result<Option<i64>> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::Settle {
                request_id: request_id.into(),
                account_id: account_id.into(),
                key: key.into(),
                hold,
                actual,
                reference: reference.map(str::to_string),
                usage,
                reply: Some(reply),
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    /// Списание/возврат БЕЗ ожидания — для RAII в синхронном контексте (Drop/finalize). `mpsc::send`
    /// не блокирует и не требует рантайма; writer применит. Осиротевшее при краше вернёт `reconcile`.
    /// `usage` — разбивка токенов/модели (аналитика), пишется рядом с charge, если передана.
    #[allow(clippy::too_many_arguments)]
    pub fn settle_detached(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
        actual: i64,
        reference: Option<&str>,
        usage: Option<registry::UsageEventInput>,
    ) {
        dispatch_detached(
            &self.writer,
            &self.detached,
            WriteCmd::Settle {
                request_id: request_id.into(),
                account_id: account_id.into(),
                key: key.into(),
                hold,
                actual,
                reference: reference.map(|s| s.into()),
                usage,
                reply: None,
            },
        );
    }
    pub async fn mark_delivering(&self, request_id: &str, lease_secs: i64) -> anyhow::Result<bool> {
        let (reply, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::MarkDelivering {
                request_id: request_id.into(),
                lease_secs,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    pub async fn renew_stream_leases(
        &self,
        request_id: Option<&str>,
        capacity_lease_id: Option<&str>,
        lease_secs: i64,
    ) -> anyhow::Result<bool> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::RenewStreamLeases {
                request_id: request_id.map(str::to_string),
                capacity_lease_id: capacity_lease_id.map(str::to_string),
                lease_secs,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    pub async fn acquire_capacity(
        &self,
        lease_id: &str,
        request_id: &str,
        email: &str,
        lease_secs: i64,
        util_cap: f64,
    ) -> anyhow::Result<Option<registry::pg::CapacityLease>> {
        let (reply, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::AcquireCapacity {
                lease_id: lease_id.into(),
                request_id: request_id.into(),
                email: email.into(),
                lease_secs,
                util_cap,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    pub fn release_capacity(&self, lease_id: &str) {
        dispatch_detached(
            &self.writer,
            &self.detached,
            WriteCmd::ReleaseCapacity {
                lease_id: lease_id.into(),
            },
        );
    }
    /// Агрегат usage по модели за окно (ts ≥ since_ts) — для клиентского дашборда `/account/usage`.
    pub async fn usage_by_model(
        &self,
        account_id: &str,
        since_ts: i64,
    ) -> anyhow::Result<Vec<registry::UsageModelAgg>> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::UsageByModel(account_id.into(), since_ts, r))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    /// Exact model/day/key aggregates over one fixed half-open interval.
    pub async fn usage_report(
        &self,
        account_id: &str,
        since_ts: i64,
        until_ts: i64,
    ) -> anyhow::Result<registry::UsageReport> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::UsageReport(
                account_id.into(),
                since_ts,
                until_ts,
                r,
            ))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    /// Агрегат расхода по аккаунтам за окно (ts ≥ since_ts) — для админ-панели «кто тратит».
    pub async fn spend_by_account(
        &self,
        since_ts: i64,
        limit: i64,
    ) -> anyhow::Result<Vec<registry::SpendAccountAgg>> {
        self.spend_by_account_range(since_ts, i64::MAX, limit).await
    }
    /// То же с явной верхней границей: полуоткрытое окно [since_ts, until_ts) — произвольный
    /// диапазон панели (/spend-stats?from&to). Одна SQL-агрегация, без вычитания кумулятивов.
    pub async fn spend_by_account_range(
        &self,
        since_ts: i64,
        until_ts: i64,
        limit: i64,
    ) -> anyhow::Result<Vec<registry::SpendAccountAgg>> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::SpendByAccount {
                since_ts,
                until_ts,
                limit,
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    /// Расход по провайдеру за окно — панель показывает вклад Claude-флота и Codex-пула отдельно.
    pub async fn spend_by_provider(
        &self,
        since_ts: i64,
    ) -> anyhow::Result<Vec<registry::SpendProviderAgg>> {
        self.spend_by_provider_range(since_ts, i64::MAX).await
    }
    /// То же с явной верхней границей окна — см. spend_by_account_range.
    pub async fn spend_by_provider_range(
        &self,
        since_ts: i64,
        until_ts: i64,
    ) -> anyhow::Result<Vec<registry::SpendProviderAgg>> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::SpendByProvider {
                since_ts,
                until_ts,
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    /// Top-`limit` моделей по charge за окно — разбивка «что реально тарифицировано» для панели.
    pub async fn spend_by_model(
        &self,
        since_ts: i64,
        limit: i64,
    ) -> anyhow::Result<Vec<registry::SpendModelAgg>> {
        self.spend_by_model_range(since_ts, i64::MAX, limit).await
    }
    /// То же с явной верхней границей окна — см. spend_by_account_range.
    pub async fn spend_by_model_range(
        &self,
        since_ts: i64,
        until_ts: i64,
        limit: i64,
    ) -> anyhow::Result<Vec<registry::SpendModelAgg>> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::SpendByModel {
                since_ts,
                until_ts,
                limit,
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    /// Сводка settlement pipeline (outbox + лаг ledger-консьюмера) — денежная диагностика панели.
    pub async fn settlement_health(
        &self,
        backlog_secs: i64,
        consumer: &str,
    ) -> anyhow::Result<registry::SettlementHealth> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::SettlementHealth(backlog_secs, consumer.into(), r))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }
    pub async fn topup(
        &self,
        account_id: &str,
        amount: i64,
        reference: Option<&str>,
    ) -> anyhow::Result<Option<i64>> {
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::Topup {
                account_id: account_id.into(),
                amount,
                reference: reference.map(|s| s.into()),
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    // --- Control-плоскость (`/admin/*`) — редкие управляющие операции через writer ---
    pub async fn create_account(
        &self,
        id: &str,
        handle: Option<&str>,
        mult_bp: i64,
    ) -> anyhow::Result<()> {
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::CreateAccount {
                id: id.into(),
                handle: handle.map(|s| s.into()),
                mult_bp,
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    pub async fn issue_key(
        &self,
        key: &str,
        account_id: &str,
        label: Option<&str>,
        spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>,
    ) -> anyhow::Result<()> {
        self.issue_key_with_policy_ack(key, account_id, label, spend_limit_nano, expires_ts, None)
            .await
    }
    pub async fn issue_key_with_policy_ack(
        &self,
        key: &str,
        account_id: &str,
        label: Option<&str>,
        spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>,
        activation_policy_ack: Option<&KeyActivationPolicyAck>,
    ) -> anyhow::Result<()> {
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::IssueKey {
                key: key.into(),
                account_id: account_id.into(),
                label: label.map(|s| s.into()),
                spend_limit_nano,
                expires_ts,
                activation_policy_ack: activation_policy_ack.cloned(),
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    pub async fn account_status(&self, id: &str, status: &str) -> anyhow::Result<usize> {
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::AccountStatus {
                id: id.into(),
                status: status.into(),
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    pub async fn account_multiplier(&self, id: &str, mult_bp: i64) -> anyhow::Result<usize> {
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::AccountMultiplier {
                id: id.into(),
                mult_bp,
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    pub async fn key_status(&self, key: &str, status: &str) -> anyhow::Result<usize> {
        self.key_status_with_policy_ack(key, status, None).await
    }
    pub async fn key_status_with_policy_ack(
        &self,
        key: &str,
        status: &str,
        activation_policy_ack: Option<&KeyActivationPolicyAck>,
    ) -> anyhow::Result<usize> {
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::KeyStatus {
                key: key.into(),
                status: status.into(),
                activation_policy_ack: activation_policy_ack.cloned(),
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    pub async fn key_status_by_id(&self, key_id: &str, status: &str) -> anyhow::Result<usize> {
        self.key_status_by_id_with_policy_ack(key_id, status, None)
            .await
    }
    pub async fn key_status_by_id_with_policy_ack(
        &self,
        key_id: &str,
        status: &str,
        activation_policy_ack: Option<&KeyActivationPolicyAck>,
    ) -> anyhow::Result<usize> {
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::KeyStatusById {
                key_id: key_id.into(),
                status: status.into(),
                activation_policy_ack: activation_policy_ack.cloned(),
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    pub async fn key_label_by_id(&self, key_id: &str, label: &str) -> anyhow::Result<usize> {
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::KeyLabelById {
                key_id: key_id.into(),
                label: label.into(),
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    pub async fn key_policy_by_id(
        &self,
        account_id: &str,
        key_id: &str,
        spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>,
    ) -> anyhow::Result<KeyPolicyUpdate> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::KeyPolicyById {
                account_id: account_id.into(),
                key_id: key_id.into(),
                spend_limit_nano,
                expires_ts,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    pub async fn ledger_ack(
        &self,
        consumer: &str,
        account_id: &str,
        last_id: i64,
    ) -> anyhow::Result<usize> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::LedgerAck {
                consumer: consumer.into(),
                account_id: account_id.into(),
                last_id,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    async fn flush_writer_once(&self) -> anyhow::Result<()> {
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::Flush(r))
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Дренаж очереди writer'а (барьер): сначала ждёт, пока backpressure-waiters поставят все
    /// detached-команды в очередь, затем ждёт их применения. Вызывать на graceful shutdown ПОСЛЕ
    /// дренажа стримов — тогда их финальные списания не потеряются при выходе процесса. A retained
    /// Claude calibration head is retried until authority recovery; returning success while it is
    /// still only in process memory would make graceful shutdown silently destructive.
    pub async fn flush(&self) -> anyhow::Result<()> {
        self.detached.wait_idle().await;
        loop {
            match self.flush_writer_once().await {
                Ok(()) => return Ok(()),
                Err(error)
                    if self.anthropic_calibration_delivery_status().pending_events > 0
                        || self.gemini_calibration_delivery_status().pending_events > 0 =>
                {
                    eprintln!(
                        "Provider calibration shutdown drain waiting for authority recovery: {error:#}"
                    );
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    #[cfg(test)]
    async fn flush_once(&self) -> anyhow::Result<()> {
        self.detached.wait_idle().await;
        self.flush_writer_once().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn pg_command_metrics_buckets_are_cumulative_and_per_op() {
        let metrics = PgCommandMetrics::default();
        metrics.observe(PgCommandOp::Reserve, Duration::from_millis(5));
        metrics.observe(PgCommandOp::Reserve, Duration::from_millis(600));
        metrics.observe(PgCommandOp::Settle, Duration::from_millis(1));
        let stats = metrics.snapshot();
        let reserve = PgCommandOp::Reserve as usize;
        let settle = PgCommandOp::Settle as usize;
        let capacity = PgCommandOp::AcquireCapacity as usize;
        let bucket = |op: usize, upper_ms: u64| {
            let bucket_index = PG_COMMAND_LATENCY_BUCKETS_MS
                .iter()
                .position(|candidate| *candidate == upper_ms)
                .expect("bucket boundary exists");
            stats.buckets[op * PG_COMMAND_LATENCY_BUCKETS_MS.len() + bucket_index]
        };
        assert_eq!(stats.count[reserve], 2);
        assert_eq!(stats.count[settle], 1);
        assert_eq!(stats.count[capacity], 0);
        // The 5 ms observation fits every bucket from 5 ms up; the 600 ms one fits only 1000 ms.
        assert_eq!(bucket(reserve, 5), 1);
        assert_eq!(bucket(reserve, 10), 1);
        assert_eq!(bucket(reserve, 500), 1);
        assert_eq!(bucket(reserve, 1_000), 2);
        assert_eq!(bucket(settle, 1), 1);
        assert_eq!(bucket(capacity, 1_000), 0);
        assert_eq!(
            stats.sum_micros[reserve],
            5_000 + 600_000,
            "sum must collect exact microseconds for the histogram _sum series"
        );
        assert_eq!(stats.sum_micros[settle], 1_000);
    }

    #[test]
    fn pg_command_timer_observes_on_drop() {
        let metrics = PgCommandMetrics::default();
        {
            let _timer = metrics.timer(PgCommandOp::AcquireCapacity);
            std::thread::sleep(Duration::from_millis(2));
        }
        let stats = metrics.snapshot();
        assert_eq!(stats.count[PgCommandOp::AcquireCapacity as usize], 1);
        assert!(stats.sum_micros[PgCommandOp::AcquireCapacity as usize] >= 2_000);
    }

    #[test]
    fn channel_queue_depth_counts_occupied_slots() {
        let (sender, mut receiver) = mpsc::channel::<u8>(4);
        assert_eq!(channel_queue_depth(&sender), 0);
        for value in 0..3 {
            sender.try_send(value).expect("capacity available");
        }
        assert_eq!(channel_queue_depth(&sender), 3);
        receiver.try_recv().expect("queued value");
        assert_eq!(channel_queue_depth(&sender), 2);
    }

    fn anthropic_event(
        request_id: &str,
        api_total_nanousd: i64,
        completed_at: i64,
    ) -> ProviderTurnCalibrationEvent {
        ProviderTurnCalibrationEvent {
            provider: registry::PROVIDER_ANTHROPIC.to_owned(),
            request_id: request_id.to_owned(),
            subject_id: "operator@example.test".to_owned(),
            model_id: "claude-sonnet-4-5".to_owned(),
            service_tier: "standard".to_owned(),
            inference_geo: "global".to_owned(),
            tariff_schedule_id: "anthropic/test/v1".to_owned(),
            priced_ts: completed_at,
            completed_at,
            input_tokens: 1,
            audio_input_tokens: 0,
            cache_read_tokens: 0,
            cached_audio_input_tokens: 0,
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: 0,
            output_tokens: 0,
            thinking_output_tokens: 0,
            image_output_tokens: 0,
            tool_prompt_tokens: 0,
            search_queries: 0,
            grounded_search_prompts: 0,
            api_input_nanousd: api_total_nanousd,
            api_audio_input_nanousd: 0,
            api_cache_read_nanousd: 0,
            api_cached_audio_input_nanousd: 0,
            api_cache_write_5m_nanousd: 0,
            api_cache_write_1h_nanousd: 0,
            api_output_nanousd: 0,
            api_image_output_nanousd: 0,
            api_search_nanousd: 0,
            api_total_nanousd,
        }
    }

    fn gemini_event(
        request_id: &str,
        api_total_nanousd: i64,
        completed_at: i64,
    ) -> ProviderTurnCalibrationEvent {
        ProviderTurnCalibrationEvent {
            provider: registry::PROVIDER_GOOGLE.to_owned(),
            request_id: request_id.to_owned(),
            subject_id: "profile-a".to_owned(),
            model_id: "gemini-2.5-flash".to_owned(),
            service_tier: "standard".to_owned(),
            inference_geo: "global".to_owned(),
            tariff_schedule_id: "google/test/v1".to_owned(),
            priced_ts: completed_at,
            completed_at,
            input_tokens: 1,
            audio_input_tokens: 0,
            cache_read_tokens: 0,
            cached_audio_input_tokens: 0,
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: 0,
            output_tokens: 0,
            thinking_output_tokens: 0,
            image_output_tokens: 0,
            tool_prompt_tokens: 0,
            search_queries: 0,
            grounded_search_prompts: 0,
            api_input_nanousd: api_total_nanousd,
            api_audio_input_nanousd: 0,
            api_cache_read_nanousd: 0,
            api_cached_audio_input_nanousd: 0,
            api_cache_write_5m_nanousd: 0,
            api_cache_write_1h_nanousd: 0,
            api_output_nanousd: 0,
            api_image_output_nanousd: 0,
            api_search_nanousd: 0,
            api_total_nanousd,
        }
    }

    fn anthropic_snapshot(
        window_kind: &str,
        used_fraction_units: i64,
        observed_at: i64,
    ) -> AnthropicQuotaSnapshot {
        AnthropicQuotaSnapshot {
            window_kind: window_kind.to_owned(),
            window_duration_mins: if window_kind == "5h" { 300 } else { 10_080 },
            resets_at: if window_kind == "5h" {
                2_000_000_000
            } else {
                2_000_500_000
            },
            used_fraction_units,
            measurement_resolution_fraction_units: 100_000,
            observed_at,
        }
    }

    fn gemini_snapshot(
        bucket_id: &str,
        window_kind: &str,
        used_fraction_units: i64,
        observed_at: i64,
    ) -> GeminiQuotaSnapshot {
        GeminiQuotaSnapshot {
            bucket_id: bucket_id.to_owned(),
            window_kind: window_kind.to_owned(),
            window_duration_mins: if window_kind == "5h" { 300 } else { 10_080 },
            resets_at: if window_kind == "5h" {
                2_000_000_000
            } else {
                2_000_500_000
            },
            used_fraction_units,
            measurement_resolution_fraction_units: 100_000,
            observed_at,
        }
    }

    fn kimi_event(
        request_id: &str,
        api_total_nanousd: i64,
        completed_at: i64,
    ) -> KimiTurnCalibrationEvent {
        KimiTurnCalibrationEvent {
            request_id: request_id.into(),
            subject_id: "kimi-subject-a".into(),
            plan: "Moderato".into(),
            requested_model: "kimi-for-coding".into(),
            served_model: "kimi-k2.7-code".into(),
            context_mode: "256k".into(),
            reasoning_effort: "high".into(),
            tariff_schedule_id: "moonshot/test/v1".into(),
            priced_ts: completed_at,
            completed_at,
            input_tokens: 1,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            api_input_nanousd: api_total_nanousd,
            api_cache_read_nanousd: 0,
            api_cache_write_nanousd: 0,
            api_output_nanousd: 0,
            api_total_nanousd,
        }
    }

    fn kimi_snapshot(
        duration_secs: i64,
        used: i64,
        limit: i64,
        observed_at: i64,
    ) -> KimiQuotaSnapshot {
        let fraction = registry::kimi_fraction_from_native(used, limit).unwrap();
        KimiQuotaSnapshot {
            window_duration_secs: duration_secs,
            window_name: Some(
                if duration_secs == registry::KIMI_ROLLING_WINDOW_SECS {
                    "rate"
                } else {
                    "weekly"
                }
                .into(),
            ),
            resets_at: if duration_secs == registry::KIMI_ROLLING_WINDOW_SECS {
                2_000_000_000
            } else {
                2_000_500_000
            },
            observed_at,
            native_used_units: used,
            native_limit_units: limit,
            used_fraction_units: fraction.used_fraction_units,
            measurement_resolution_fraction_units: fraction.measurement_resolution_fraction_units,
        }
    }

    fn codex_event(
        request_id: &str,
        api_total_nanousd: i64,
        chatgpt_total_nanocredits: i64,
        completed_at: i64,
    ) -> CodexTurnCalibrationEvent {
        CodexTurnCalibrationEvent {
            request_id: request_id.into(),
            home_id: "home-a".into(),
            model_id: "gpt-5.6-terra".into(),
            service_tier: "standard".into(),
            provider_reported_tier: Some("default".into()),
            api_tariff_schedule_id: "openai/test/v1".into(),
            credit_schedule_id: "chatgpt/test/v1".into(),
            completed_at,
            input_tokens: 1,
            cached_input_tokens: 0,
            cache_write_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            api_input_nanousd: api_total_nanousd,
            api_cached_input_nanousd: 0,
            api_cache_write_nanousd: 0,
            api_output_nanousd: 0,
            api_total_nanousd,
            chatgpt_input_nanocredits: chatgpt_total_nanocredits,
            chatgpt_cached_input_nanocredits: 0,
            chatgpt_output_nanocredits: 0,
            chatgpt_total_nanocredits,
        }
    }

    fn legacy_snapshot(
        request_id: &str,
        account_id: &str,
        official_hold_nano: i64,
        charged_hold_nano: i64,
    ) -> LegacyScalarAdmissionSnapshot {
        let admission_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        LegacyScalarAdmissionSnapshot::new(registry::pricing::LegacyScalarAdmissionSnapshotInput {
            request_id: request_id.into(),
            account_id: account_id.into(),
            provider: registry::pricing::SnapshotProvider::Anthropic,
            requested_model_id: "claude-sonnet-5".into(),
            canonical_model_id: "claude-sonnet-5".into(),
            alias_generation: 1,
            tariff_schedule_id: "anthropic/standard/sonnet-current/v1".into(),
            tariff_priced_ts: admission_ts,
            admission_ts,
            payable_multiplier_bp: 2_000,
            official_hold_nano,
            charged_hold_nano,
            premium_modifiers: registry::pricing::LegacyPremiumModifiers::AnthropicV1 {
                speed: registry::pricing::SnapshotAnthropicSpeed::Standard,
                inference_geo: registry::pricing::SnapshotAnthropicInferenceGeo::Global,
                inference_geo_basis_points: 10_000,
            },
        })
        .unwrap()
    }

    fn strict_track_snapshot(request_id: &str, account_id: &str) -> PolicyAdmissionSnapshot {
        let admission_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        PolicyAdmissionSnapshot::new(registry::pricing::PolicyAdmissionSnapshotInput {
            request_id: request_id.into(),
            account_id: account_id.into(),
            provider: registry::pricing::SnapshotProvider::Anthropic,
            product_id: "main".into(),
            account_class: registry::pricing::AccountClass::B2c,
            requested_model_id: "claude-test".into(),
            canonical_model_id: "claude-test".into(),
            alias_generation: 1,
            rule_id: "track-provider".into(),
            rule_digest: "track-digest".into(),
            rule_scope: registry::pricing::PolicyRuleScope::Provider {
                provider_id: "anthropic".into(),
            },
            pricing_mode: registry::pricing::PricingMode::Track,
            rule_origin: registry::pricing::RuleOrigin::Managed,
            discount_bps: None,
            payable_multiplier_bp: 5_000,
            policy_id: "b2c:global".into(),
            policy_version: 1,
            effective_policy_version: 1,
            source_policy_digest: "source-policy".into(),
            policy_digest: "policy-digest".into(),
            policy_catalog_generation: 1,
            policy_switch_generation: 1,
            admission_catalog_generation: 1,
            admission_catalog_digest: "catalog-digest".into(),
            admission_switch_generation: 1,
            admission_switch_digest: "switch-digest".into(),
            runtime_manifest_generation: 1,
            runtime_manifest_digest: "runtime-manifest".into(),
            tariff_schedule_id: "anthropic/claude-test/v1".into(),
            tariff_priced_ts: admission_ts,
            admission_ts,
            official_hold_nano: 1_000,
            charged_hold_nano: 500,
            track_eligible: true,
            retention_eligible: true,
            commission_eligible: true,
            premium_modifiers: registry::pricing::LegacyPremiumModifiers::AnthropicV1 {
                speed: registry::pricing::SnapshotAnthropicSpeed::Standard,
                inference_geo: registry::pricing::SnapshotAnthropicInferenceGeo::Global,
                inference_geo_basis_points: 10_000,
            },
        })
        .unwrap()
    }

    #[test]
    fn live_pricing_shadow_reader_budget_is_postgres_only() {
        let result = AsyncBilling::start_authority_with_pricing_shadow(
            registry::authority::AuthorityConfig::new(":memory:".to_owned(), None),
            None,
            1,
            1,
            0,
        );
        assert!(result.is_err());
    }

    #[test]
    fn snapshot_reserve_handoff_only_cancels_before_commit_decision() {
        for (initial, expected) in [
            (
                SNAPSHOT_RESERVE_HANDOFF_PENDING,
                SNAPSHOT_RESERVE_HANDOFF_CANCELED,
            ),
            (
                SNAPSHOT_RESERVE_HANDOFF_COMMIT_DECIDED,
                SNAPSHOT_RESERVE_HANDOFF_COMMIT_DECIDED,
            ),
            (
                SNAPSHOT_RESERVE_HANDOFF_COMMITTED,
                SNAPSHOT_RESERVE_HANDOFF_COMMITTED,
            ),
            (
                SNAPSHOT_RESERVE_HANDOFF_CLAIMED,
                SNAPSHOT_RESERVE_HANDOFF_CLAIMED,
            ),
            (
                SNAPSHOT_RESERVE_HANDOFF_FAILED,
                SNAPSHOT_RESERVE_HANDOFF_FAILED,
            ),
            (
                SNAPSHOT_RESERVE_HANDOFF_COMMIT_UNKNOWN,
                SNAPSHOT_RESERVE_HANDOFF_COMMIT_UNKNOWN,
            ),
        ] {
            let handoff = Arc::new(AtomicU8::new(initial));
            drop(SnapshotReserveHandoffGuard {
                handoff: Arc::clone(&handoff),
            });
            assert_eq!(handoff.load(Ordering::Acquire), expected);
        }

        let pending = AtomicU8::new(SNAPSHOT_RESERVE_HANDOFF_PENDING);
        assert!(authorize_snapshot_reserve_commit(&pending));
        assert_eq!(
            pending.load(Ordering::Acquire),
            SNAPSHOT_RESERVE_HANDOFF_COMMIT_DECIDED
        );
        assert!(authorize_snapshot_reserve_commit(&pending));

        let canceled = AtomicU8::new(SNAPSHOT_RESERVE_HANDOFF_CANCELED);
        assert!(!authorize_snapshot_reserve_commit(&canceled));

        let failed_before_commit = AtomicU8::new(SNAPSHOT_RESERVE_HANDOFF_PENDING);
        mark_snapshot_reserve_failed(&failed_before_commit);
        assert_eq!(
            failed_before_commit.load(Ordering::Acquire),
            SNAPSHOT_RESERVE_HANDOFF_FAILED
        );
        let ambiguous_commit = AtomicU8::new(SNAPSHOT_RESERVE_HANDOFF_COMMIT_DECIDED);
        mark_snapshot_reserve_failed(&ambiguous_commit);
        assert_eq!(
            ambiguous_commit.load(Ordering::Acquire),
            SNAPSHOT_RESERVE_HANDOFF_COMMIT_UNKNOWN
        );
    }

    #[tokio::test]
    async fn detached_dispatch_tracker_waits_for_a_backpressured_enqueue() {
        let tracker = Arc::new(DetachedDispatchTracker::default());
        let (writer, mut receiver) = mpsc::channel(1);
        let (first_reply, _first_result) = oneshot::channel();
        assert!(writer.try_send(WriteCmd::Flush(first_reply)).is_ok());
        let (second_reply, _second_result) = oneshot::channel();
        dispatch_detached(&writer, &tracker, WriteCmd::Flush(second_reply));

        let wait_tracker = tracker.clone();
        let waiter = tokio::spawn(async move { wait_tracker.wait_idle().await });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());

        assert!(matches!(receiver.recv().await, Some(WriteCmd::Flush(_))));
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("backpressured detached command never entered the FIFO")
            .unwrap();
        assert!(matches!(receiver.recv().await, Some(WriteCmd::Flush(_))));
    }

    #[tokio::test]
    async fn codex_health_survives_billing_actor_restart() {
        let unique = std::process::id();
        let path = std::env::temp_dir().join(format!("claude-api-codex-health-{unique}.sqlite"));
        let _ = std::fs::remove_file(&path);
        let db = path.to_str().unwrap().to_string();

        {
            let billing = AsyncBilling::start(db.clone(), 1).unwrap();
            // Unknown home reads back healthy: absence of evidence is not evidence of a fault.
            let fresh = billing.load_codex_health("home-a").await.unwrap();
            assert_eq!(fresh.account_state, "healthy");

            billing
                .save_codex_health(
                    "home-a",
                    registry::CodexHomeHealthRow {
                        account_state: "dead".to_string(),
                        auth_fail_streak: 2,
                        first_auth_fail_ts: 1_000,
                        cooling_until: 1_900,
                    },
                    2_000,
                )
                .await
                .unwrap();
        }

        // A new actor over the same authority is what a blue-green handoff looks like to the pool.
        let billing = AsyncBilling::start(db, 1).unwrap();
        let restored = billing.load_codex_health("home-a").await.unwrap();
        assert_eq!(restored.account_state, "dead");
        assert_eq!(restored.auth_fail_streak, 2);
        assert_eq!(restored.cooling_until, 1_900);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn codex_calibration_survives_billing_actor_restart() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-codex-calibration-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();

        let first = AsyncBilling::start(path_string.clone(), 1).unwrap();
        let totals = first
            .record_codex_turn(codex_event(
                "request-1",
                100_000_000_000,
                10_000_000_000,
                100,
            ))
            .await
            .unwrap();
        assert_eq!(totals.spent_nano, 100_000_000_000);
        assert_eq!(totals.spent_nanocredits, Some(10_000_000_000));
        let (_, anchor) = first
            .observe_codex_window("home-a", 300, 2_000_000_000, 10, 10_000_000, 100)
            .await
            .unwrap();
        assert!(anchor.current_capacity_nano.is_none());
        first
            .record_codex_turn(codex_event("request-2", 40_000_000_000, 4_000_000_000, 101))
            .await
            .unwrap();
        let (_, measured) = first
            .observe_codex_window("home-a", 300, 2_000_000_000, 12, 12_000_000, 101)
            .await
            .unwrap();
        assert_eq!(measured.current_capacity_nano, Some(2_000_000_000_000));
        assert_eq!(measured.current_capacity_nanocredits, Some(200_000_000_000));
        assert_eq!(measured.samples, 1);
        assert!(measured.anchor_ready);
        first
            .record_codex_turn(codex_event("request-3", 40_000_000_000, 4_000_000_000, 102))
            .await
            .unwrap();
        let (_, measured) = first
            .observe_codex_window("home-a", 300, 2_000_000_000, 14, 14_000_000, 102)
            .await
            .unwrap();
        assert_eq!(measured.current_capacity_nano, Some(2_000_000_000_000));
        assert_eq!(measured.current_capacity_nanocredits, Some(200_000_000_000));
        assert_eq!(measured.samples, 2);
        let (_, duplicate) = first
            .observe_codex_window("home-a", 300, 2_000_000_000, 14, 14_000_000, 102)
            .await
            .unwrap();
        assert_eq!(duplicate.version, measured.version);
        first.flush().await.unwrap();
        drop(first);

        // Simulate the exact production upgrade case: raw observations are intact while the
        // derived v2 row contains a transient one-interval estimate. The restarted actor must
        // replay raw evidence before returning capacity.
        let connection = registry::open(&path_string).unwrap();
        connection
            .execute(
                "UPDATE codex_window_calibrations SET estimator_version=2, \
                   current_capacity_nano=187994100000,sum_used_sq=1,\
                   sum_used_spend_nano=1879941000,samples=1,observed_points=1,anchor_ready=0 \
                 WHERE home_id='home-a' AND window_duration_mins=300",
                [],
            )
            .unwrap();
        // Migration-first blue-green overlap once allowed an old runtime to append an API-only
        // observation after native-credit tracking had started. The immutable residue must remain
        // auditable, but it must not permanently block a v9 history rebuild.
        connection
            .execute(
                "INSERT INTO codex_window_observations(\
                   home_id,window_duration_mins,resets_at,observed_at,used_percent,\
                   used_fraction_units,gateway_spend_nano,gateway_spend_nanocredits) \
                 VALUES('home-a',300,2000000000,103,14,14000000,180000000000,NULL)",
                [],
            )
            .unwrap();
        drop(connection);

        let restarted = AsyncBilling::start(path_string, 1).unwrap();
        let (spend, restored) = restarted
            .observe_codex_window("home-a", 300, 2_000_000_000, 14, 14_000_000, 103)
            .await
            .unwrap();
        assert_eq!(spend.spent_nano, 180_000_000_000);
        assert_eq!(spend.spent_nanocredits, Some(18_000_000_000));
        assert_eq!(restored.estimator_version, crate::codex::ESTIMATOR_VERSION);
        assert_eq!(restored.current_capacity_nano, Some(2_000_000_000_000));
        assert_eq!(restored.current_capacity_nanocredits, Some(200_000_000_000));
        assert_eq!(restored.observed_at, 103);
        let report = restarted.codex_calibration_report().await.unwrap();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].turns, 3);
        assert_eq!(report[0].api_total_nanousd, 180_000_000_000);
        assert!(restored.version > measured.version);
        restarted.flush().await.unwrap();
        drop(restarted);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn anthropic_admin_turns_are_exact_idempotent_and_calibrate_both_windows() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-anthropic-calibration-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let first = AsyncBilling::start(path_string.clone(), 1).unwrap();

        // No account, key reservation or customer usage_event exists: provider capacity evidence
        // is deliberately independent and therefore includes successful operator/admin traffic.
        let anchor_event = anthropic_event("request-1", 1_000_000_000, 100);
        let (anchor_spend, anchors) = first
            .record_anthropic_turn(
                anchor_event.clone(),
                "max20",
                vec![
                    anthropic_snapshot("5h", 10_000_000, 100),
                    anthropic_snapshot("7d", 20_000_000, 100),
                ],
            )
            .await
            .unwrap();
        assert!(anchor_spend.inserted);
        assert_eq!(anchor_spend.spent_nano, 1_000_000_000);
        assert_eq!(anchors.len(), 2);
        assert!(anchors
            .iter()
            .all(|row| row.current_capacity_nano.is_none()));

        let (replayed_spend, replayed) = first
            .record_anthropic_turn(
                anchor_event,
                "max20",
                vec![
                    anthropic_snapshot("5h", 10_000_000, 100),
                    anthropic_snapshot("7d", 20_000_000, 100),
                ],
            )
            .await
            .unwrap();
        assert!(!replayed_spend.inserted);
        assert_eq!(replayed_spend.spent_nano, 1_000_000_000);
        assert_eq!(
            replayed.iter().map(|row| row.version).collect::<Vec<_>>(),
            anchors.iter().map(|row| row.version).collect::<Vec<_>>(),
        );

        let measured_event = anthropic_event("request-2", 2_000_000_000, 101);
        let (measured_spend, measured) = first
            .record_anthropic_turn(
                measured_event.clone(),
                "max20",
                vec![
                    anthropic_snapshot("5h", 14_000_000, 101),
                    anthropic_snapshot("7d", 21_000_000, 101),
                ],
            )
            .await
            .unwrap();
        assert_eq!(measured_spend.spent_nano, 3_000_000_000);
        assert_eq!(measured[0].window_kind, "5h");
        assert_eq!(measured[0].current_capacity_nano, Some(50_000_000_000));
        assert_eq!(measured[1].window_kind, "7d");
        assert_eq!(measured[1].current_capacity_nano, Some(200_000_000_000));

        let mut conflict = measured_event;
        conflict.input_tokens += 1;
        let error = first
            .record_anthropic_turn(conflict, "max20", Vec::new())
            .await
            .unwrap_err();
        assert!(registry::is_provider_turn_calibration_replay_conflict(
            &error
        ));
        let conflicted_status = first.anthropic_calibration_delivery_status();
        assert_eq!(conflicted_status.pending_events, 0);
        assert_eq!(conflicted_status.dropped_events, 1);
        assert!(!conflicted_status.persistence_ok);

        // A poll can move quota but never advances spend. The first unmatched movement is held for
        // one snapshot; seeing the same point again excludes it as unattributed instead of
        // manufacturing a larger dollar capacity.
        let (_, lagged) = first
            .observe_anthropic_window(
                "operator@example.test",
                "max20",
                "5h",
                300,
                2_000_000_000,
                15_000_000,
                100_000,
                102,
            )
            .await
            .unwrap();
        assert_eq!(lagged.unattributed_fraction_units, 0);
        let (spend, excluded) = first
            .observe_anthropic_window(
                "operator@example.test",
                "max20",
                "5h",
                300,
                2_000_000_000,
                15_000_000,
                100_000,
                103,
            )
            .await
            .unwrap();
        assert_eq!(spend, 3_000_000_000);
        assert_eq!(excluded.unattributed_fraction_units, 1_000_000);
        assert_eq!(excluded.current_capacity_nano, Some(50_000_000_000));

        // A poisoned request id is quarantined; it cannot pin later valid evidence behind the
        // immutable conflict forever.
        let (post_conflict, _) = first
            .record_anthropic_turn(
                anthropic_event("request-after-conflict", 1, 104),
                "max20",
                Vec::new(),
            )
            .await
            .unwrap();
        assert!(post_conflict.inserted);
        assert_eq!(post_conflict.spent_nano, 3_000_000_001);
        assert_eq!(
            first.anthropic_calibration_delivery_status().pending_events,
            0
        );

        first.flush().await.unwrap();
        drop(first);

        let restarted = AsyncBilling::start(path_string, 1).unwrap();
        let (rows, evidence, recent_turns) =
            restarted.anthropic_calibration_report().await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].turns, 3);
        assert_eq!(evidence[0].api_total_nanousd, 3_000_000_001);
        assert_eq!(recent_turns.len(), 3);

        restarted.flush().await.unwrap();
        drop(restarted);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn backend_post_turn_poll_calibrates_without_a_second_customer_request() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-anthropic-post-turn-poll-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let billing = AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap();

        for (kind, duration, used, reset) in [
            ("5h", 300, 10_000_000, 2_000_000_000),
            ("7d", 10_080, 20_000_000, 2_000_500_000),
        ] {
            let (spend, anchor) = billing
                .observe_anthropic_window(
                    "operator@example.test",
                    "max20",
                    kind,
                    duration,
                    reset,
                    used,
                    100_000,
                    100,
                )
                .await
                .unwrap();
            assert_eq!(spend, 0);
            assert!(anchor.current_capacity_nano.is_none());
        }

        // Response headers and the fast post-turn poll can share one wall-clock second. The headers
        // still carry the old fraction; FIFO ordering plus the later changed fraction must be
        // sufficient to finish both intervals without waiting for another customer request.
        let (spend, response_rows) = billing
            .record_anthropic_turn(
                anthropic_event("post-turn-only", 2_000_000_000, 101),
                "max20",
                vec![
                    AnthropicQuotaSnapshot {
                        window_kind: "5h".to_owned(),
                        window_duration_mins: 300,
                        resets_at: 2_000_000_000,
                        used_fraction_units: 10_000_000,
                        measurement_resolution_fraction_units: 100_000,
                        observed_at: 101,
                    },
                    AnthropicQuotaSnapshot {
                        window_kind: "7d".to_owned(),
                        window_duration_mins: 10_080,
                        resets_at: 2_000_500_000,
                        used_fraction_units: 20_000_000,
                        measurement_resolution_fraction_units: 100_000,
                        observed_at: 101,
                    },
                ],
            )
            .await
            .unwrap();
        assert_eq!(spend.spent_nano, 2_000_000_000);
        assert_eq!(response_rows.len(), 2);

        let (_, five_hour) = billing
            .observe_anthropic_window(
                "operator@example.test",
                "max20",
                "5h",
                300,
                2_000_000_000,
                14_000_000,
                100_000,
                101,
            )
            .await
            .unwrap();
        let (_, weekly) = billing
            .observe_anthropic_window(
                "operator@example.test",
                "max20",
                "7d",
                10_080,
                2_000_500_000,
                21_000_000,
                100_000,
                101,
            )
            .await
            .unwrap();
        assert_eq!(five_hour.current_capacity_nano, Some(50_000_000_000));
        assert_eq!(weekly.current_capacity_nano, Some(200_000_000_000));

        let (rows, evidence, recent_turns) = billing.anthropic_calibration_report().await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].turns, 1);
        assert_eq!(recent_turns[0].request_id, "post-turn-only");
        billing.flush().await.unwrap();
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn anthropic_outage_recovery_replays_turns_fifo_before_poll_snapshot() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-anthropic-fifo-recovery-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing = AsyncBilling::start(path_string.clone(), 1).unwrap();
        let control = registry::open(&path_string).unwrap();
        control
            .execute_batch(
                "CREATE TRIGGER reject_anthropic_calibration_turn \
                 BEFORE INSERT ON provider_turn_calibration_events \
                 BEGIN SELECT RAISE(FAIL, 'simulated authority outage'); END;",
            )
            .unwrap();

        let first = billing
            .record_anthropic_turn(
                anthropic_event("fifo-first", 1_000_000_000, 100),
                "max20",
                vec![anthropic_snapshot("5h", 10_000_000, 100)],
            )
            .await;
        assert!(first.is_err());
        let second = billing
            .record_anthropic_turn(
                anthropic_event("fifo-second", 2_000_000_000, 101),
                "max20",
                vec![anthropic_snapshot("5h", 14_000_000, 101)],
            )
            .await;
        assert!(second.is_err());
        assert_eq!(
            billing.anthropic_calibration_delivery_status(),
            AnthropicCalibrationDeliveryStatus {
                pending_events: 2,
                dropped_events: 0,
                persistence_ok: false,
                queue_limit: MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS,
            }
        );

        control
            .execute_batch("DROP TRIGGER reject_anthropic_calibration_turn;")
            .unwrap();
        let (spend, row) = billing
            .observe_anthropic_window(
                "operator@example.test",
                "max20",
                "5h",
                300,
                2_000_000_000,
                14_000_000,
                100_000,
                102,
            )
            .await
            .unwrap();
        assert_eq!(spend, 3_000_000_000);
        assert_eq!(row.current_capacity_nano, Some(50_000_000_000));
        assert_eq!(
            billing.anthropic_calibration_delivery_status(),
            AnthropicCalibrationDeliveryStatus {
                pending_events: 0,
                dropped_events: 0,
                persistence_ok: true,
                queue_limit: MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS,
            }
        );

        let ids = {
            let mut statement = control
                .prepare("SELECT request_id FROM provider_turn_calibration_events ORDER BY rowid")
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(ids, ["fifo-first", "fifo-second"]);

        billing.flush().await.unwrap();
        drop(billing);
        drop(control);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn anthropic_flush_retries_detached_pending_turn_before_shutdown() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-anthropic-shutdown-drain-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing = AsyncBilling::start(path_string.clone(), 1).unwrap();
        let control = registry::open(&path_string).unwrap();
        control
            .execute_batch(
                "CREATE TRIGGER reject_anthropic_calibration_turn \
                 BEFORE INSERT ON provider_turn_calibration_events \
                 BEGIN SELECT RAISE(FAIL, 'simulated authority outage'); END;",
            )
            .unwrap();

        billing.record_anthropic_turn_detached(
            anthropic_event("shutdown-pending", 1_000_000_000, 100),
            "max20",
            vec![anthropic_snapshot("5h", 10_000_000, 100)],
        );
        assert!(billing.flush_once().await.is_err());
        assert_eq!(
            billing
                .anthropic_calibration_delivery_status()
                .pending_events,
            1
        );

        control
            .execute_batch("DROP TRIGGER reject_anthropic_calibration_turn;")
            .unwrap();
        billing.flush().await.unwrap();
        assert_eq!(
            billing.anthropic_calibration_delivery_status(),
            AnthropicCalibrationDeliveryStatus {
                pending_events: 0,
                dropped_events: 0,
                persistence_ok: true,
                queue_limit: MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS,
            }
        );
        let (_, evidence, recent_turns) = billing.anthropic_calibration_report().await.unwrap();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].turns, 1);
        assert_eq!(recent_turns.len(), 1);

        drop(billing);
        drop(control);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn anthropic_pending_queue_is_bounded_and_counts_dropped_evidence() {
        let state = AnthropicCalibrationDeliveryState::default();
        for index in 0..MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS {
            enqueue_anthropic_calibration_turn(
                &state,
                anthropic_event(&format!("bounded-{index}"), 1, 100),
                "max20".to_owned(),
                Vec::new(),
            )
            .unwrap();
        }
        assert!(enqueue_anthropic_calibration_turn(
            &state,
            anthropic_event("bounded-overflow", 1, 100),
            "max20".to_owned(),
            Vec::new(),
        )
        .is_err());
        assert_eq!(
            state
                .queue
                .lock()
                .expect("Anthropic calibration delivery queue lock")
                .pending
                .len(),
            MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS
        );
        assert_eq!(state.dropped_events.load(Ordering::Relaxed), 1);
        assert!(!state.persistence_ok.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn identical_plan_credits_converge_while_api_usd_remains_workload_dependent() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-codex-like-for-like-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let billing = AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap();
        let mut native_capacities = Vec::new();
        let mut api_capacities = Vec::new();
        for (index, interval_api_nano) in [
            40_000_000_000,
            20_000_000_000,
            80_000_000_000,
            10_000_000_000,
        ]
        .into_iter()
        .enumerate()
        {
            let home_id = format!("pro-home-{index}");
            let mut anchor = codex_event(
                &format!("anchor-{index}"),
                10_000_000_000,
                1_000_000_000,
                100,
            );
            anchor.home_id = home_id.clone();
            billing.record_codex_turn(anchor).await.unwrap();
            billing
                .observe_codex_window(&home_id, 300, 2_000_000_000, 10, 10_000_000, 100)
                .await
                .unwrap();

            let mut measured = codex_event(
                &format!("measured-{index}"),
                interval_api_nano,
                4_000_000_000,
                101,
            );
            measured.home_id = home_id.clone();
            billing.record_codex_turn(measured).await.unwrap();
            let (_, row) = billing
                .observe_codex_window(&home_id, 300, 2_000_000_000, 12, 12_000_000, 101)
                .await
                .unwrap();
            native_capacities.push(row.current_capacity_nanocredits.unwrap());
            api_capacities.push(row.current_capacity_nano.unwrap());
        }

        assert_eq!(native_capacities, vec![200_000_000_000; 4]);
        assert_eq!(
            api_capacities,
            vec![
                2_000_000_000_000,
                1_000_000_000_000,
                4_000_000_000_000,
                500_000_000_000,
            ]
        );
        assert_eq!(
            billing
                .codex_calibration_report()
                .await
                .unwrap()
                .iter()
                .map(|row| row.turns)
                .sum::<i64>(),
            8
        );
        billing.flush().await.unwrap();
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn gemini_exact_turns_calibrate_first_interval_and_keep_windows_independent() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-gemini-calibration-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let first = AsyncBilling::start(path_string.clone(), 1).unwrap();

        first
            .record_gemini_turn(
                gemini_event("gemini-1", 10_000, 100),
                "google_ai_pro",
                vec![],
            )
            .await
            .unwrap();
        let (_, anchor) = first
            .observe_gemini_window(
                "profile-a",
                "google_ai_pro",
                "gemini-5h",
                "5h",
                300,
                2_000_000_000,
                1_000,
                1,
                100,
            )
            .await
            .unwrap();
        assert!(anchor.current_capacity_nano.is_none());

        first
            .record_gemini_turn(
                gemini_event("gemini-2", 20_000, 101),
                "google_ai_pro",
                vec![],
            )
            .await
            .unwrap();
        let (_, measured) = first
            .observe_gemini_window(
                "profile-a",
                "google_ai_pro",
                "gemini-5h",
                "5h",
                300,
                2_000_000_000,
                2_000,
                1,
                101,
            )
            .await
            .unwrap();
        assert_eq!(measured.current_capacity_nano, Some(2_000_000_000));
        assert_eq!(measured.observed_spend_nano, 20_000);

        let (_, weekly) = first
            .observe_gemini_window(
                "profile-a",
                "google_ai_pro",
                "gemini-weekly",
                "weekly",
                10_080,
                2_000_500_000,
                500,
                1,
                102,
            )
            .await
            .unwrap();
        assert!(weekly.current_capacity_nano.is_none());
        drop(first);

        let second = AsyncBilling::start(path_string, 1).unwrap();
        let (_, restored) = second
            .observe_gemini_window(
                "profile-a",
                "google_ai_pro",
                "gemini-5h",
                "5h",
                300,
                2_000_000_000,
                2_000,
                1,
                103,
            )
            .await
            .unwrap();
        assert_eq!(restored.current_capacity_nano, Some(2_000_000_000));
        assert_eq!(restored.observed_spend_nano, 20_000);
        assert_eq!(restored.samples, 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn gemini_equal_second_settlement_catch_up_is_not_filtered_as_a_duplicate() {
        let anchor = gemini_observation(
            "profile-a",
            "google_ai_pro",
            &gemini_snapshot("gemini-5h", "5h", 10_000_000, 100),
            1_000,
            "response",
            Some("gemini-anchor"),
        );
        let pending = gemini_observation(
            "profile-a",
            "google_ai_pro",
            &gemini_snapshot("gemini-5h", "5h", 20_000_000, 101),
            1_000,
            "response",
            Some("gemini-pending"),
        );
        let anchor_row = crate::gemini::apply_observation_with_history(None, &[], &anchor).unwrap();
        let pending_row =
            crate::gemini::apply_observation_with_history(Some(anchor_row), &[], &pending).unwrap();
        assert!(gemini_observation_is_stale_or_duplicate(
            &pending_row,
            &pending
        ));

        let catch_up = GeminiExactWindowObservation {
            gateway_spend_nano: 2_001_000,
            observation_source: "poll".to_owned(),
            source_request_id: None,
            ..pending
        };
        assert!(!gemini_observation_is_stale_or_duplicate(
            &pending_row,
            &catch_up
        ));
        let settled =
            crate::gemini::apply_observation_with_history(Some(pending_row), &[], &catch_up)
                .unwrap();
        assert_eq!(settled.samples, 1);
        assert_eq!(settled.current_capacity_nano, Some(20_000_000));
    }

    #[tokio::test]
    async fn gemini_outage_recovery_replays_fifo_before_poll_snapshot() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-gemini-fifo-recovery-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing = AsyncBilling::start(path_string.clone(), 1).unwrap();
        let control = registry::open(&path_string).unwrap();
        control
            .execute_batch(
                "CREATE TRIGGER reject_gemini_calibration_turn \
                 BEFORE INSERT ON provider_turn_calibration_events \
                 WHEN NEW.provider='google' \
                 BEGIN SELECT RAISE(FAIL, 'simulated authority outage'); END;",
            )
            .unwrap();

        let first = billing
            .record_gemini_turn(
                gemini_event("gemini-fifo-first", 1_000_000_000, 100),
                "google_ai_pro",
                vec![gemini_snapshot("gemini-5h", "5h", 10_000_000, 100)],
            )
            .await;
        assert!(first.is_err());
        let second = billing
            .record_gemini_turn(
                gemini_event("gemini-fifo-second", 2_000_000_000, 101),
                "google_ai_pro",
                vec![gemini_snapshot("gemini-5h", "5h", 14_000_000, 101)],
            )
            .await;
        assert!(second.is_err());
        assert_eq!(
            billing.gemini_calibration_delivery_status(),
            GeminiCalibrationDeliveryStatus {
                pending_events: 2,
                dropped_events: 0,
                persistence_ok: false,
                queue_limit: MAX_PENDING_GEMINI_CALIBRATION_EVENTS,
            }
        );

        let blocked_poll = billing
            .observe_gemini_window(
                "profile-a",
                "google_ai_pro",
                "gemini-5h",
                "5h",
                300,
                2_000_000_000,
                14_000_000,
                100_000,
                102,
            )
            .await;
        assert!(blocked_poll.is_err());
        assert_eq!(
            control
                .query_row(
                    "SELECT COUNT(*) FROM gemini_exact_window_observations",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0,
            "a free quota poll must not overtake pending paid-turn evidence"
        );

        control
            .execute_batch("DROP TRIGGER reject_gemini_calibration_turn;")
            .unwrap();
        let (spend, row) = billing
            .observe_gemini_window(
                "profile-a",
                "google_ai_pro",
                "gemini-5h",
                "5h",
                300,
                2_000_000_000,
                14_000_000,
                100_000,
                103,
            )
            .await
            .unwrap();
        assert_eq!(spend, 3_000_000_000);
        assert_eq!(row.current_capacity_nano, Some(50_000_000_000));
        assert_eq!(
            billing.gemini_calibration_delivery_status(),
            GeminiCalibrationDeliveryStatus {
                pending_events: 0,
                dropped_events: 0,
                persistence_ok: true,
                queue_limit: MAX_PENDING_GEMINI_CALIBRATION_EVENTS,
            }
        );
        let ids = {
            let mut statement = control
                .prepare(
                    "SELECT request_id FROM provider_turn_calibration_events \
                     WHERE provider='google' ORDER BY rowid",
                )
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(ids, ["gemini-fifo-first", "gemini-fifo-second"]);

        billing.flush().await.unwrap();
        drop(billing);
        drop(control);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn gemini_replay_conflict_quarantines_only_the_corrupt_event() {
        let connection = registry::open(":memory:").unwrap();
        registry::record_provider_turn_calibration_event(
            &connection,
            &gemini_event("gemini-conflict", 1_000, 100),
        )
        .unwrap();
        let state = GeminiCalibrationDeliveryState::default();
        enqueue_gemini_calibration_turn(
            &state,
            gemini_event("gemini-conflict", 2_000, 100),
            "google_ai_pro".to_owned(),
            Vec::new(),
        )
        .unwrap();
        enqueue_gemini_calibration_turn(
            &state,
            gemini_event("gemini-after-conflict", 3_000, 101),
            "google_ai_pro".to_owned(),
            Vec::new(),
        )
        .unwrap();

        flush_pending_gemini_calibration_turns(&state, None, |turn| {
            let spend = registry::record_provider_turn_calibration_event(&connection, &turn.event)?;
            Ok((spend, Vec::new()))
        })
        .unwrap();

        assert_eq!(
            state
                .queue
                .lock()
                .expect("Gemini calibration delivery queue lock")
                .pending
                .len(),
            0
        );
        assert_eq!(state.dropped_events.load(Ordering::Relaxed), 1);
        assert!(!state.persistence_ok.load(Ordering::Relaxed));
        assert_eq!(
            registry::provider_calibration_subject_spend(
                &connection,
                registry::PROVIDER_GOOGLE,
                "profile-a",
            )
            .unwrap()
            .spent_nano,
            4_000
        );
        let report =
            registry::provider_turn_calibration_report(&connection, registry::PROVIDER_GOOGLE)
                .unwrap();
        assert_eq!(report.iter().map(|row| row.turns).sum::<i64>(), 2);
    }

    #[tokio::test]
    async fn gemini_flush_retries_detached_pending_turn_before_shutdown() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-gemini-shutdown-drain-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing = AsyncBilling::start(path_string.clone(), 1).unwrap();
        let control = registry::open(&path_string).unwrap();
        control
            .execute_batch(
                "CREATE TRIGGER reject_gemini_calibration_turn \
                 BEFORE INSERT ON provider_turn_calibration_events \
                 WHEN NEW.provider='google' \
                 BEGIN SELECT RAISE(FAIL, 'simulated authority outage'); END;",
            )
            .unwrap();

        billing.record_gemini_turn_detached(
            gemini_event("gemini-shutdown-pending", 1_000_000_000, 100),
            "google_ai_pro",
            vec![gemini_snapshot("gemini-5h", "5h", 10_000_000, 100)],
        );
        assert!(billing.flush_once().await.is_err());
        assert_eq!(
            billing.gemini_calibration_delivery_status().pending_events,
            1
        );

        control
            .execute_batch("DROP TRIGGER reject_gemini_calibration_turn;")
            .unwrap();
        billing.flush().await.unwrap();
        assert_eq!(
            billing.gemini_calibration_delivery_status(),
            GeminiCalibrationDeliveryStatus {
                pending_events: 0,
                dropped_events: 0,
                persistence_ok: true,
                queue_limit: MAX_PENDING_GEMINI_CALIBRATION_EVENTS,
            }
        );
        let (_, evidence, recent_turns) = billing.gemini_calibration_report().await.unwrap();
        assert_eq!(evidence.iter().map(|row| row.turns).sum::<i64>(), 1);
        assert_eq!(recent_turns.len(), 1);

        drop(billing);
        drop(control);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn gemini_pending_queue_is_bounded_and_counts_dropped_evidence() {
        let state = GeminiCalibrationDeliveryState::default();
        for index in 0..MAX_PENDING_GEMINI_CALIBRATION_EVENTS {
            enqueue_gemini_calibration_turn(
                &state,
                gemini_event(&format!("gemini-bounded-{index}"), 1, 100),
                "google_ai_pro".to_owned(),
                Vec::new(),
            )
            .unwrap();
        }
        assert!(enqueue_gemini_calibration_turn(
            &state,
            gemini_event("gemini-bounded-overflow", 1, 100),
            "google_ai_pro".to_owned(),
            Vec::new(),
        )
        .is_err());
        assert_eq!(
            state
                .queue
                .lock()
                .expect("Gemini calibration delivery queue lock")
                .pending
                .len(),
            MAX_PENDING_GEMINI_CALIBRATION_EVENTS
        );
        assert_eq!(state.dropped_events.load(Ordering::Relaxed), 1);
        assert!(!state.persistence_ok.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn canceled_sqlite_reserve_handoff_releases_key_allowance() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-billing-handoff-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing = AsyncBilling::start(path_string, 1).unwrap();
        billing.create_account("acct", None, 10_000).await.unwrap();
        assert_eq!(
            billing.topup("acct", 1_000, Some("seed")).await.unwrap(),
            Some(1_000)
        );
        billing
            .issue_key("limited", "acct", None, Some(700), None)
            .await
            .unwrap();

        let handoff = Arc::new(AtomicU8::new(RESERVE_HANDOFF_CANCELED));
        let (reply, response) = oneshot::channel();
        billing
            .writer
            .send(WriteCmd::Reserve {
                request_id: "canceled-before-handoff".into(),
                account_id: "acct".into(),
                key: "limited".into(),
                hold: 500,
                execution: registry::ExecutionAttempt::direct(),
                handoff: Arc::clone(&handoff),
                reply,
            })
            .await
            .unwrap();
        assert!(response.await.is_err());
        billing.flush().await.unwrap();

        let account = billing.account("acct").await.unwrap().unwrap();
        let key = billing.get("limited").await.unwrap().unwrap();
        assert_eq!((account.balance_nano, account.reserved_nano), (1_000, 0));
        assert_eq!(key.reserved_nano, 0);
        assert_eq!(handoff.load(Ordering::Acquire), RESERVE_HANDOFF_REFUNDED);

        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn canceled_sqlite_snapshot_reserve_never_reaches_durable_state() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-snapshot-handoff-cancel-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing = AsyncBilling::start(path_string.clone(), 1).unwrap();
        billing.create_account("acct", None, 2_000).await.unwrap();
        assert_eq!(
            billing.topup("acct", 1_000, Some("seed")).await.unwrap(),
            Some(1_000)
        );
        billing
            .issue_key("limited", "acct", None, Some(700), None)
            .await
            .unwrap();

        let snapshot = legacy_snapshot("snapshot-canceled", "acct", 500, 100);
        let handoff = Arc::new(AtomicU8::new(SNAPSHOT_RESERVE_HANDOFF_CANCELED));
        let (reply, response) = oneshot::channel();
        billing
            .writer
            .send(WriteCmd::ReserveWithLegacySnapshot {
                key: "limited".into(),
                snapshot,
                execution: registry::ExecutionAttempt::direct(),
                handoff: Arc::clone(&handoff),
                reply,
            })
            .await
            .unwrap();
        assert_eq!(
            response.await.unwrap().unwrap(),
            LegacyScalarReserveOutcome::AbortedBeforeCommit
        );
        billing.flush().await.unwrap();

        let account = billing.account("acct").await.unwrap().unwrap();
        let key = billing.get("limited").await.unwrap().unwrap();
        assert_eq!((account.balance_nano, account.reserved_nano), (1_000, 0));
        assert_eq!(key.reserved_nano, 0);
        assert_eq!(
            handoff.load(Ordering::Acquire),
            SNAPSHOT_RESERVE_HANDOFF_CANCELED
        );

        let conn = registry::open(&path_string).unwrap();
        assert!(registry::pricing::sqlite_legacy_scalar_admission_snapshot(
            &conn,
            "snapshot-canceled"
        )
        .unwrap()
        .is_none());
        let durable_rows: (i64, i64) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM billing_reservations \
                          WHERE request_id='snapshot-canceled'), \
                        (SELECT COUNT(*) FROM billing_settlement_outbox \
                          WHERE request_id='snapshot-canceled')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(durable_rows, (0, 0));

        drop(conn);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn detached_zero_settlement_cancels_strict_reserve_and_restores_funding_buckets() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-strict-cancel-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing = AsyncBilling::start(path_string.clone(), 1).unwrap();
        billing
            .create_account("strict-account", None, 5_000)
            .await
            .unwrap();
        assert_eq!(
            billing
                .topup("strict-account", 1_000, Some("strict-seed"))
                .await
                .unwrap(),
            Some(1_000)
        );

        let conn = registry::open(&path_string).unwrap();
        conn.execute_batch(
            "INSERT INTO pricing_catalog_versions(
                 product_id,generation,schema_version,capability_generation,capability_digest,
                 content_digest,created_ts
             ) VALUES('main',1,1,1,'capability','catalog-digest',1);
             INSERT INTO pricing_catalog_entries(
                 product_id,generation,provider_id,canonical_model_id,enabled
             ) VALUES('main',1,'anthropic','claude-test',1);
             INSERT INTO pricing_catalog_heads(product_id,active_generation,updated_ts)
             VALUES('main',1,1);
             INSERT INTO provider_switch_versions(
                 generation,schema_version,capability_generation,capability_digest,
                 content_digest,created_ts
             ) VALUES(1,1,1,'capability','switch-digest',1);
             INSERT INTO provider_switch_entries(
                 generation,provider_id,scope_type,product_id,segment,catalog_generation,enabled
             ) VALUES
                 (1,'anthropic','master','','',NULL,1),
                 (1,'anthropic','segment','main','b2c',1,1);
             INSERT INTO provider_switch_head(singleton,active_generation,updated_ts)
             VALUES(1,1,1);
             INSERT INTO account_policy_versions(
                 account_id,effective_version,policy_id,policy_version,source_policy_digest,
                 owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
                 switch_generation,content_digest,replacement_locked,created_ts
             ) VALUES(
                 'strict-account',1,'b2c:global',1,'source-policy','global_b2c','global','b2c',
                 'main',1,1,1,'policy-digest',0,1
             );
             INSERT INTO account_policy_rules(
                 account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                 canonical_model_id,pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
                 track_eligible,retention_eligible,commission_eligible
             ) VALUES(
                 'strict-account',1,'track-provider','track-digest','provider','anthropic',NULL,
                 'track','managed',NULL,5000,1,1,1
             );
             INSERT INTO account_policy_bindings(
                 account_id,product_id,account_class,active_effective_version,
                 policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
             ) VALUES('strict-account','main','b2c',1,'strict','strict','verified',1);
             INSERT INTO funding_buckets(
                 bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES
                 ('strict-bonus','strict-account','welcome_track_bonus','welcome','track',400,
                  0,0,1,'active',1,1),
                 ('strict-paid','strict-account','paid','seed','any',600,
                  0,0,1,'active',2,2);",
        )
        .unwrap();
        drop(conn);

        let ack = registry::KeyActivationPolicyAck {
            effective_policy_version: 1,
            policy_digest: "policy-digest".into(),
        };
        billing
            .issue_key_with_policy_ack("strict-key", "strict-account", None, None, None, Some(&ack))
            .await
            .unwrap();
        let snapshot = strict_track_snapshot("strict-cancel-request", "strict-account");
        assert!(matches!(
            billing
                .reserve_request_with_policy_snapshot("strict-key", snapshot)
                .await
                .unwrap(),
            PolicyReserveOutcome::Inserted(_)
        ));
        let reserved = billing.account("strict-account").await.unwrap().unwrap();
        assert_eq!((reserved.balance_nano, reserved.reserved_nano), (500, 500));

        billing.settle_detached(
            "strict-cancel-request",
            "strict-account",
            "strict-key",
            500,
            0,
            None,
            None,
        );
        billing.flush().await.unwrap();

        let account = billing.account("strict-account").await.unwrap().unwrap();
        let key = billing.get("strict-key").await.unwrap().unwrap();
        assert_eq!((account.balance_nano, account.reserved_nano), (1_000, 0));
        assert_eq!(
            (account.spent_nano, key.spent_nano, key.reserved_nano),
            (0, 0, 0)
        );
        let conn = registry::open(&path_string).unwrap();
        let buckets = conn
            .prepare(
                "SELECT bucket_id,balance_nano,reserved_nano,spent_nano,status
                   FROM funding_buckets WHERE account_id='strict-account' ORDER BY bucket_id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            buckets,
            vec![
                ("strict-bonus".into(), 400, 0, 0, "active".into()),
                ("strict-paid".into(), 600, 0, 0, "active".into()),
            ]
        );
        let terminal: (String, String, String) = conn
            .query_row(
                "SELECT reservation.state,outbox.state,outbox.disposition
                   FROM billing_reservations reservation
                   JOIN billing_settlement_outbox outbox USING(request_id)
                  WHERE reservation.request_id='strict-cancel-request'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            terminal,
            ("canceled".into(), "done".into(), "cancel".into())
        );

        drop(conn);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn lost_sqlite_snapshot_reserve_reply_stays_active_and_replays_exactly() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-snapshot-handoff-lost-reply-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing = AsyncBilling::start(path_string.clone(), 1).unwrap();
        billing.create_account("acct", None, 2_000).await.unwrap();
        assert_eq!(
            billing.topup("acct", 1_000, Some("seed")).await.unwrap(),
            Some(1_000)
        );
        billing
            .issue_key("limited", "acct", None, Some(700), None)
            .await
            .unwrap();

        let snapshot = legacy_snapshot("snapshot-lost-reply", "acct", 500, 100);
        let handoff = Arc::new(AtomicU8::new(SNAPSHOT_RESERVE_HANDOFF_PENDING));
        let (reply, response) = oneshot::channel();
        drop(response);
        billing
            .writer
            .send(WriteCmd::ReserveWithLegacySnapshot {
                key: "limited".into(),
                snapshot: snapshot.clone(),
                execution: registry::ExecutionAttempt::direct(),
                handoff: Arc::clone(&handoff),
                reply,
            })
            .await
            .unwrap();
        billing.flush().await.unwrap();

        assert_eq!(
            handoff.load(Ordering::Acquire),
            SNAPSHOT_RESERVE_HANDOFF_COMMITTED
        );
        let account = billing.account("acct").await.unwrap().unwrap();
        let key = billing.get("limited").await.unwrap().unwrap();
        assert_eq!((account.balance_nano, account.reserved_nano), (900, 100));
        assert_eq!(key.reserved_nano, 100);

        let conn = registry::open(&path_string).unwrap();
        assert_eq!(
            registry::pricing::sqlite_legacy_scalar_admission_snapshot(
                &conn,
                "snapshot-lost-reply"
            )
            .unwrap(),
            Some(snapshot.clone())
        );
        let durable_state: (String, i64) = conn
            .query_row(
                "SELECT state, \
                        (SELECT COUNT(*) FROM billing_settlement_outbox \
                          WHERE request_id='snapshot-lost-reply') \
                   FROM billing_reservations \
                  WHERE request_id='snapshot-lost-reply'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(durable_state, ("reserved".into(), 0));
        drop(conn);

        assert!(matches!(
            billing
                .reserve_request_with_legacy_snapshot("limited", snapshot)
                .await
                .unwrap(),
            LegacyScalarReserveOutcome::Unchanged(_)
        ));
        let account = billing.account("acct").await.unwrap().unwrap();
        let key = billing.get("limited").await.unwrap().unwrap();
        assert_eq!((account.balance_nano, account.reserved_nano), (900, 100));
        assert_eq!(key.reserved_nano, 100);

        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn kimi_postgres_actor_pairs_spend_before_independent_window_cas() {
        const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;

        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping KIMI PostgreSQL actor matrix: CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let instance_id = format!("kimi-actor-{}-{unique}", std::process::id());
        let mut lock_holder = postgres::Client::connect(&url, postgres::NoTls).unwrap();
        lock_holder
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        let mut pg = registry::pg::PgStore::connect(&url).unwrap();
        pg.migrate().unwrap();
        lock_holder
            .batch_execute(
                "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
                 kimi_calibration_subject_spend,kimi_turn_calibration_events \
                 RESTART IDENTITY CASCADE",
            )
            .unwrap();
        let owner = pg.claim_instance(&instance_id, 60).unwrap();
        drop(pg);

        let billing = AsyncBilling::start_authority(
            registry::authority::AuthorityConfig::Postgres { url: url.clone() },
            Some(owner),
            1,
            0,
        )
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let first = vec![
                kimi_snapshot(registry::KIMI_ROLLING_WINDOW_SECS, 100, 1_000, 100),
                kimi_snapshot(registry::KIMI_WEEKLY_WINDOW_SECS, 200, 1_000, 100),
            ];
            let anchored = billing
                .observe_kimi_windows("kimi-subject-a", "Moderato", first)
                .await
                .unwrap();
            assert_eq!(anchored.len(), 2);
            assert!(anchored
                .iter()
                .all(|row| row.samples == 0 && row.version == 1));

            assert!(billing
                .record_kimi_turn(kimi_event("kimi-actor-turn", 1_000_000_000, 101))
                .await
                .unwrap());
            let second = vec![
                kimi_snapshot(registry::KIMI_ROLLING_WINDOW_SECS, 110, 1_000, 102),
                kimi_snapshot(registry::KIMI_WEEKLY_WINDOW_SECS, 230, 1_000, 102),
            ];
            let measured = billing
                .observe_kimi_windows("kimi-subject-a", "Moderato", second.clone())
                .await
                .unwrap();
            assert_eq!(measured.len(), 2);
            assert!(measured.iter().all(|row| {
                row.samples == 1
                    && row.observed_spend_nano == 1_000_000_000
                    && row.current_capacity_nano.is_some()
                    && row.version == 2
            }));

            // Exact replay is idempotent: no extra immutable row, sample or CAS version.
            let replay = billing
                .observe_kimi_windows("kimi-subject-a", "Moderato", second)
                .await
                .unwrap();
            assert!(replay
                .iter()
                .all(|row| row.samples == 1 && row.version == 2));
            billing.flush().await.unwrap();
        });
        drop(billing);

        let mut pg = registry::pg::PgStore::connect(&url).unwrap();
        assert_eq!(
            pg.kimi_subject_spend("kimi-subject-a").unwrap(),
            1_000_000_000
        );
        for duration in [
            registry::KIMI_ROLLING_WINDOW_SECS,
            registry::KIMI_WEEKLY_WINDOW_SECS,
        ] {
            let history = pg
                .load_kimi_window_observations("kimi-subject-a", "Moderato", duration)
                .unwrap();
            assert_eq!(history.len(), 2);
            assert_eq!(history[0].cumulative_api_spend_nano, 0);
            assert_eq!(history[1].cumulative_api_spend_nano, 1_000_000_000);
        }
        lock_holder
            .batch_execute(
                "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
                 kimi_calibration_subject_spend,kimi_turn_calibration_events \
                 RESTART IDENTITY CASCADE",
            )
            .unwrap();
        lock_holder
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }

    #[tokio::test]
    async fn kimi_calibration_report_is_empty_on_a_sqlite_authority() {
        let billing = AsyncBilling::start_authority(
            registry::authority::AuthorityConfig::new(":memory:".to_owned(), None),
            None,
            1,
            0,
        )
        .unwrap();
        // KIMI calibration authority is PostgreSQL-only: the report is empty, not an error,
        // while the evidence commands themselves keep refusing the SQLite authority.
        assert_eq!(billing.kimi_calibration_report().await.unwrap(), Vec::new());
        assert!(billing
            .record_kimi_turn(kimi_event("kimi-sqlite-turn", 1, 1))
            .await
            .is_err());
    }

    fn glm_event(
        request_id: &str,
        api_total_nanousd: i64,
        native_total_microcredits: i64,
        completed_at: i64,
    ) -> GlmTurnCalibrationEvent {
        GlmTurnCalibrationEvent {
            request_id: request_id.into(),
            subject_id: "glm-subject-a".into(),
            plan: "Pro".into(),
            requested_model: "glm-5.2".into(),
            served_model: "glm-5.2".into(),
            context_mode: "200k".into(),
            reasoning_effort: Some("high".into()),
            api_tariff_schedule_id: "zhipu/zai-open-platform/2026-08-03".into(),
            credit_schedule_id: "zhipu/glm-coding-plan-credits/2026-08-03".into(),
            priced_ts: completed_at,
            completed_at,
            fresh_input_tokens: 1,
            cached_input_tokens: 0,
            cache_write_tokens: 0,
            output_tokens: 1,
            reasoning_tokens: 0,
            api_fresh_input_nanousd: api_total_nanousd / 2,
            api_cached_input_nanousd: 0,
            api_output_nanousd: api_total_nanousd - api_total_nanousd / 2,
            api_total_nanousd,
            native_fresh_input_microcredits: native_total_microcredits / 2,
            native_cached_input_microcredits: 0,
            native_output_microcredits: native_total_microcredits
                - native_total_microcredits / 2,
            native_total_microcredits,
            off_peak: false,
        }
    }

    fn glm_snapshot(
        duration_secs: i64,
        used: i64,
        limit: i64,
        observed_at: i64,
    ) -> GlmQuotaSnapshot {
        let fraction = registry::glm_fraction_from_native(used, limit).unwrap();
        GlmQuotaSnapshot {
            window_duration_secs: duration_secs,
            resets_at: Some(observed_at + duration_secs),
            observed_at,
            native_used_units: Some(used),
            native_limit_units: Some(limit),
            native_remaining_units: Some(limit - used),
            percentage_raw: None,
            used_fraction_units: Some(fraction.used_fraction_units),
            measurement_resolution_fraction_units: Some(
                fraction.measurement_resolution_fraction_units,
            ),
        }
    }

    #[tokio::test]
    async fn glm_calibration_commands_refuse_a_sqlite_authority() {
        let billing = AsyncBilling::start_authority(
            registry::authority::AuthorityConfig::new(":memory:".to_owned(), None),
            None,
            1,
            0,
        )
        .unwrap();
        // GLM calibration is PostgreSQL-only, like KIMI: evidence commands refuse the SQLite
        // authority rather than writing provider evidence somewhere it cannot be paired.
        assert!(billing
            .record_glm_turn(glm_event("glm-sqlite-turn", 2, 2, 1))
            .await
            .is_err());
        assert!(billing
            .observe_glm_windows(
                "glm-subject-a",
                "Pro",
                vec![glm_snapshot(registry::GLM_5H_WINDOW_SECS, 100, 1_000, 100)],
            )
            .await
            .is_err());
    }

    #[test]
    fn glm_postgres_actor_pairs_dual_spend_before_independent_window_cas() {
        const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;

        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping GLM PostgreSQL actor matrix: CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let instance_id = format!("glm-actor-{}-{unique}", std::process::id());
        let mut lock_holder = postgres::Client::connect(&url, postgres::NoTls).unwrap();
        lock_holder
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        let mut pg = registry::pg::PgStore::connect(&url).unwrap();
        pg.migrate().unwrap();
        lock_holder
            .batch_execute(
                "TRUNCATE glm_window_calibrations,glm_window_observations,\
                 glm_calibration_subject_spend,glm_turn_calibration_events \
                 RESTART IDENTITY CASCADE",
            )
            .unwrap();
        let owner = pg.claim_instance(&instance_id, 60).unwrap();
        drop(pg);

        let billing = AsyncBilling::start_authority(
            registry::authority::AuthorityConfig::Postgres { url: url.clone() },
            Some(owner),
            1,
            0,
        )
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let first = vec![
                glm_snapshot(registry::GLM_5H_WINDOW_SECS, 100, 1_000, 100),
                glm_snapshot(registry::GLM_WEEKLY_WINDOW_SECS, 200, 1_000, 100),
            ];
            let anchored = billing
                .observe_glm_windows("glm-subject-a", "Pro", first)
                .await
                .unwrap();
            assert_eq!(anchored.len(), 2);
            assert!(anchored
                .iter()
                .all(|row| row.samples == 0 && row.version == 1));

            assert!(billing
                .record_glm_turn(glm_event("glm-actor-turn", 1_000_000_000, 500_000_000, 101))
                .await
                .unwrap());
            let second = vec![
                glm_snapshot(registry::GLM_5H_WINDOW_SECS, 110, 1_000, 102),
                glm_snapshot(registry::GLM_WEEKLY_WINDOW_SECS, 230, 1_000, 102),
            ];
            let measured = billing
                .observe_glm_windows("glm-subject-a", "Pro", second.clone())
                .await
                .unwrap();
            assert_eq!(measured.len(), 2);
            assert!(measured.iter().all(|row| {
                row.samples == 1
                    && row.observed_spend_api_nanousd == 1_000_000_000
                    && row.observed_spend_native_microcredits == 500_000_000
                    && row.current_capacity_nanousd.is_some()
                    && row.version == 2
            }));

            // Exact replay is idempotent: no extra immutable row, sample or CAS version.
            let replay = billing
                .observe_glm_windows("glm-subject-a", "Pro", second)
                .await
                .unwrap();
            assert!(replay
                .iter()
                .all(|row| row.samples == 1 && row.version == 2));
            billing.flush().await.unwrap();
        });
        drop(billing);

        let mut pg = registry::pg::PgStore::connect(&url).unwrap();
        // The two ledgers advanced independently and exactly; one is never the other rescaled.
        assert_eq!(
            pg.glm_subject_spend("glm-subject-a").unwrap(),
            GlmSubjectSpend {
                spent_api_nanousd: 1_000_000_000,
                spent_native_microcredits: 500_000_000,
            }
        );
        for duration in [
            registry::GLM_5H_WINDOW_SECS,
            registry::GLM_WEEKLY_WINDOW_SECS,
        ] {
            let history = pg
                .load_glm_window_observations("glm-subject-a", "Pro", duration)
                .unwrap();
            assert_eq!(history.len(), 2);
            assert_eq!(history[0].cumulative_api_nanousd, 0);
            assert_eq!(history[0].cumulative_native_microcredits, 0);
            assert_eq!(history[1].cumulative_api_nanousd, 1_000_000_000);
            assert_eq!(history[1].cumulative_native_microcredits, 500_000_000);
        }
        lock_holder
            .batch_execute(
                "TRUNCATE glm_window_calibrations,glm_window_observations,\
                 glm_calibration_subject_spend,glm_turn_calibration_events \
                 RESTART IDENTITY CASCADE",
            )
            .unwrap();
        lock_holder
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }

    #[test]
    fn kimi_postgres_calibration_report_lists_every_subject_window() {
        const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;

        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping KIMI PostgreSQL report matrix: CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let mut lock_holder = postgres::Client::connect(&url, postgres::NoTls).unwrap();
        lock_holder
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        let mut pg = registry::pg::PgStore::connect(&url).unwrap();
        pg.migrate().unwrap();
        lock_holder
            .batch_execute(
                "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
                 kimi_calibration_subject_spend,kimi_turn_calibration_events \
                 RESTART IDENTITY CASCADE",
            )
            .unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let instance_id = format!("kimi-report-{}-{unique}", std::process::id());
        let owner = pg.claim_instance(&instance_id, 60).unwrap();
        drop(pg);

        let billing = AsyncBilling::start_authority(
            registry::authority::AuthorityConfig::Postgres { url: url.clone() },
            Some(owner),
            1,
            0,
        )
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            billing
                .observe_kimi_windows(
                    "kimi-subject-a",
                    "Moderato",
                    vec![
                        kimi_snapshot(registry::KIMI_ROLLING_WINDOW_SECS, 100, 1_000, 100),
                        kimi_snapshot(registry::KIMI_WEEKLY_WINDOW_SECS, 200, 1_000, 100),
                    ],
                )
                .await
                .unwrap();
            billing
                .observe_kimi_windows(
                    "kimi-subject-b",
                    "unreviewed-base-plan",
                    vec![kimi_snapshot(
                        registry::KIMI_ROLLING_WINDOW_SECS,
                        10,
                        1_000,
                        101,
                    )],
                )
                .await
                .unwrap();

            let report = billing.kimi_calibration_report().await.unwrap();
            // Every durable row is reported, across subjects, plans and independent windows.
            assert_eq!(report.len(), 3);
            assert!(report.iter().all(|row| row.samples == 0));
            let subject_a: Vec<_> = report
                .iter()
                .filter(|row| row.subject_id == "kimi-subject-a")
                .collect();
            assert_eq!(subject_a.len(), 2);
            assert!(subject_a
                .iter()
                .any(|row| row.window_duration_secs == registry::KIMI_WEEKLY_WINDOW_SECS
                    && row.native_used_units == 200));
            let subject_b: Vec<_> = report
                .iter()
                .filter(|row| row.subject_id == "kimi-subject-b")
                .collect();
            assert_eq!(subject_b.len(), 1);
            assert_eq!(subject_b[0].plan, "unreviewed-base-plan");
            assert_eq!(subject_b[0].native_used_units, 10);
            billing.flush().await.unwrap();
        });
        drop(billing);

        lock_holder
            .batch_execute(
                "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
                 kimi_calibration_subject_spend,kimi_turn_calibration_events \
                 RESTART IDENTITY CASCADE",
            )
            .unwrap();
        lock_holder
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }

    #[tokio::test]
    async fn kimi_recent_turns_is_empty_on_a_sqlite_authority() {
        let billing = AsyncBilling::start_authority(
            registry::authority::AuthorityConfig::new(":memory:".to_owned(), None),
            None,
            1,
            0,
        )
        .unwrap();
        // KIMI calibration is PostgreSQL-only: the read is empty, not an error.
        assert_eq!(billing.kimi_recent_turns(512).await.unwrap(), Vec::new());
    }

    #[test]
    fn kimi_postgres_recent_turns_read_is_bounded_newest_first_and_exact() {
        const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;

        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping KIMI PostgreSQL recent-turns matrix: CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let mut lock_holder = postgres::Client::connect(&url, postgres::NoTls).unwrap();
        lock_holder
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        let mut pg = registry::pg::PgStore::connect(&url).unwrap();
        pg.migrate().unwrap();
        lock_holder
            .batch_execute(
                "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
                 kimi_calibration_subject_spend,kimi_turn_calibration_events \
                 RESTART IDENTITY CASCADE",
            )
            .unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let instance_id = format!("kimi-turns-{}-{unique}", std::process::id());
        let owner = pg.claim_instance(&instance_id, 60).unwrap();
        drop(pg);

        let billing = AsyncBilling::start_authority(
            registry::authority::AuthorityConfig::Postgres { url: url.clone() },
            Some(owner),
            1,
            0,
        )
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            for (id, total, completed_at) in [
                ("kimi-turn-older", 11_600, 1_800_000_100),
                ("kimi-turn-newer", 22_400, 1_800_000_200),
                ("kimi-turn-middle", 5_000, 1_800_000_150),
            ] {
                billing
                    .record_kimi_turn(kimi_event(id, total, completed_at))
                    .await
                    .unwrap();
            }
            billing.flush().await.unwrap();

            let turns = billing.kimi_recent_turns(512).await.unwrap();
            let ids: Vec<&str> = turns.iter().map(|turn| turn.request_id.as_str()).collect();
            assert_eq!(
                ids,
                ["kimi-turn-newer", "kimi-turn-middle", "kimi-turn-older"],
                "newest first by completed_at"
            );
            // Exact roundtrip: the full usage and money vector survives the read.
            let newer = &turns[0];
            assert_eq!(newer.served_model, "kimi-k2.7-code");
            assert_eq!(newer.api_total_nanousd, 22_400);
            assert_eq!(newer.completed_at, 1_800_000_200);
            assert_eq!(newer.tariff_schedule_id, "moonshot/test/v1");
            // The bound is honored.
            let limited = billing.kimi_recent_turns(2).await.unwrap();
            assert_eq!(limited.len(), 2);
            assert_eq!(limited[0].request_id, "kimi-turn-newer");
            assert_eq!(limited[1].request_id, "kimi-turn-middle");
        });
        drop(billing);

        lock_holder
            .batch_execute(
                "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
                 kimi_calibration_subject_spend,kimi_turn_calibration_events \
                 RESTART IDENTITY CASCADE",
            )
            .unwrap();
        lock_holder
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }

    #[test]
    fn postgres_snapshot_handoff_cancels_precommit_and_replays_lost_reply() {
        const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;

        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping PostgreSQL snapshot actor contract: \
                 CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let account_id = format!("snapshot-actor-account-{}-{unique}", std::process::id());
        let key = format!("snapshot-actor-key-{}-{unique}", std::process::id());
        let canceled_request = format!("snapshot-actor-canceled-{}-{unique}", std::process::id());
        let lost_request = format!("snapshot-actor-lost-{}-{unique}", std::process::id());
        let instance_id = format!("snapshot-actor-owner-{}-{unique}", std::process::id());

        // Share the registry real-PG suite's process-wide destructive lock so package tests cannot
        // truncate this test's authority while its actor threads are running.
        let mut admin = postgres::Client::connect(&url, postgres::NoTls).unwrap();
        admin
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        let mut pg = registry::pg::PgStore::connect(&url).unwrap();
        pg.migrate().unwrap();
        let owner = pg.claim_instance(&instance_id, 60).unwrap();
        drop(pg);

        let billing = AsyncBilling::start_authority(
            registry::authority::AuthorityConfig::Postgres { url: url.clone() },
            Some(owner),
            1,
            0,
        )
        .unwrap();
        let canceled_snapshot = legacy_snapshot(&canceled_request, &account_id, 500, 100);
        let lost_snapshot = legacy_snapshot(&lost_request, &account_id, 500, 100);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            billing
                .create_account(&account_id, None, 2_000)
                .await
                .unwrap();
            assert_eq!(
                billing
                    .topup(&account_id, 1_000, Some("snapshot-actor-seed"))
                    .await
                    .unwrap(),
                Some(1_000)
            );
            billing
                .issue_key(&key, &account_id, None, Some(700), None)
                .await
                .unwrap();

            let canceled_handoff = Arc::new(AtomicU8::new(SNAPSHOT_RESERVE_HANDOFF_CANCELED));
            let (canceled_reply, canceled_response) = oneshot::channel();
            billing
                .writer
                .send(WriteCmd::ReserveWithLegacySnapshot {
                    key: key.clone(),
                    snapshot: canceled_snapshot.clone(),
                    execution: registry::ExecutionAttempt::direct(),
                    handoff: Arc::clone(&canceled_handoff),
                    reply: canceled_reply,
                })
                .await
                .unwrap();
            assert_eq!(
                canceled_response.await.unwrap().unwrap(),
                LegacyScalarReserveOutcome::AbortedBeforeCommit
            );

            let lost_handoff = Arc::new(AtomicU8::new(SNAPSHOT_RESERVE_HANDOFF_PENDING));
            let (lost_reply, lost_response) = oneshot::channel();
            drop(lost_response);
            billing
                .writer
                .send(WriteCmd::ReserveWithLegacySnapshot {
                    key: key.clone(),
                    snapshot: lost_snapshot.clone(),
                    execution: registry::ExecutionAttempt::direct(),
                    handoff: Arc::clone(&lost_handoff),
                    reply: lost_reply,
                })
                .await
                .unwrap();
            billing.flush().await.unwrap();

            assert_eq!(
                lost_handoff.load(Ordering::Acquire),
                SNAPSHOT_RESERVE_HANDOFF_COMMITTED
            );
            let account = billing.account(&account_id).await.unwrap().unwrap();
            let key_row = billing.get(&key).await.unwrap().unwrap();
            assert_eq!((account.balance_nano, account.reserved_nano), (900, 100));
            assert_eq!(key_row.reserved_nano, 100);
            assert!(matches!(
                billing
                    .reserve_request_with_legacy_snapshot(&key, lost_snapshot.clone())
                    .await
                    .unwrap(),
                LegacyScalarReserveOutcome::Unchanged(_)
            ));
        });

        let durable = admin
            .query_one(
                "SELECT r.state, \
                        (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots s \
                          WHERE s.request_id=$1), \
                        (SELECT COUNT(*)::bigint FROM settlement_outbox o \
                          WHERE o.request_id=$1), \
                        (SELECT COUNT(*)::bigint FROM reservations c \
                          WHERE c.request_id=$2) \
                   FROM reservations r WHERE r.request_id=$1",
                &[&lost_request, &canceled_request],
            )
            .unwrap();
        assert_eq!(durable.get::<_, String>(0), "reserved");
        assert_eq!(durable.get::<_, i64>(1), 1);
        assert_eq!(durable.get::<_, i64>(2), 0);
        assert_eq!(durable.get::<_, i64>(3), 0);
        assert_eq!(
            registry::pg::PgStore::connect(&url)
                .unwrap()
                .legacy_scalar_admission_snapshot(&lost_request)
                .unwrap(),
            Some(lost_snapshot)
        );

        drop(runtime);
        drop(billing);
        admin
            .execute("DELETE FROM accounts WHERE id=$1", &[&account_id])
            .unwrap();
        admin
            .execute(
                "DELETE FROM engine_instances WHERE instance_id=$1",
                &[&instance_id],
            )
            .unwrap();
        admin
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }
}
