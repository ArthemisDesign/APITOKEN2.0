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

mod request_facts;

use registry::pricing::{TariffOverride, TariffOverrideInsert, TariffOverrideInsertOutcome};
use registry::request_facts::{
    DeliveryState, ProviderTerminalClass, RequestFactAdmission, RequestFactTerminalEvidence,
    TerminalRequestFact,
};
use registry::{
    AccountRow, AnthropicCalibrationRow, AnthropicWindowObservation, BillingTotals,
    CodexCalibrationRow, CodexHomeCalibrationSpend, CodexTurnCalibrationAggregate,
    CodexTurnCalibrationEvent, CodexWindowObservation, GeminiExactCalibrationRow,
    GeminiExactWindowObservation, GlmCalibrationRow, GlmSubjectSpend, GlmTurnCalibrationEvent,
    GlmWindowObservation, KeyAuth, KeyPolicyUpdate, KeyRow, KimiCalibrationRow,
    KimiTurnCalibrationEvent, KimiWindowObservation, ProviderCalibrationSubjectSpend,
    ProviderTurnCalibrationAggregate, ProviderTurnCalibrationEvent, SunoCalibrationRow,
    SunoSubjectSpend, SunoWindowObservation, Tripo3dBalanceObservation, Tripo3dCalibrationRow,
    Tripo3dSubjectSpend,
};
use request_facts::TerminalRequestFactInbox;
pub use request_facts::{
    RequestFactDeliverySnapshot, RequestFactPersistenceHealth, TerminalRequestFactSubmission,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, Notify};

/// Billing queues are deliberately bounded. The request path applies async backpressure instead of
/// retaining an arbitrary number of commands while PostgreSQL/SQLite is unavailable.
const WRITE_QUEUE_CAPACITY: usize = 4_096;
const READ_QUEUE_CAPACITY: usize = 1_024;
const PG_OPERATION_RETRY_DEADLINE: Duration = Duration::from_secs(5);
const MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS: usize = 4_096;
const MAX_PENDING_GEMINI_CALIBRATION_EVENTS: usize = 4_096;

type AdminChanges = Arc<RwLock<Option<tokio::sync::broadcast::Sender<crate::AdminChange>>>>;

fn publish_admin_change(
    changes: &AdminChanges,
    resources: &[&'static str],
    reason: &'static str,
) {
    let sender = changes.read().unwrap_or_else(|error| error.into_inner());
    if let Some(sender) = sender.as_ref() {
        let _ = sender.send(crate::AdminChange::engine(resources, reason));
    }
}

#[cfg(test)]
mod admin_change_tests {
    use super::*;

    #[test]
    fn admin_change_is_emitted_only_after_a_publisher_is_attached() {
        let changes: AdminChanges = Arc::new(RwLock::new(None));
        publish_admin_change(&changes, &["/overview"], "settlement");

        let (sender, mut receiver) = tokio::sync::broadcast::channel(4);
        *changes.write().unwrap() = Some(sender);
        publish_admin_change(&changes, &["/overview"], "settlement");

        let event = receiver.try_recv().expect("change is published");
        assert_eq!(event.resources, vec!["/overview"]);
        assert_eq!(event.reason, Some("settlement"));
    }
}

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

/// One parsed Tripo3D balance-endpoint snapshot before the writer pairs it with the subject's
/// durable cumulative dual-ledger spend (same single-writer hop as GLM). The provider halves are
/// raw decimal TEXT — their unit is unproven (`docs/engine/TRIPO3D_PROVIDER.md` §5.2/§6.1), so
/// the parsed micro-units stay `None` until a live run proves the unit: unknown is `None`,
/// never `0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tripo3dBalanceSnapshot {
    pub observed_at: i64,
    pub balance_raw: String,
    pub frozen_raw: String,
    pub balance_micro_units: Option<i64>,
    pub frozen_micro_units: Option<i64>,
}

/// Build the immutable Tripo3D balance observation: provider-side raw halves from the snapshot
/// plus the subject's exact durable cumulative dual ledgers, which only this serial writer may
/// read. The observation is source-tagged: a free poll invents no request id, a response-carried
/// read names the turn that carried it.
fn tripo3d_observation(
    subject_id: &str,
    cohort: &str,
    snapshot: &Tripo3dBalanceSnapshot,
    spend: Tripo3dSubjectSpend,
    source: &str,
    source_request_id: Option<&str>,
) -> Tripo3dBalanceObservation {
    Tripo3dBalanceObservation {
        subject_id: subject_id.to_owned(),
        cohort: cohort.to_owned(),
        observed_at: snapshot.observed_at,
        balance_raw: snapshot.balance_raw.clone(),
        frozen_raw: snapshot.frozen_raw.clone(),
        balance_micro_units: snapshot.balance_micro_units,
        frozen_micro_units: snapshot.frozen_micro_units,
        cumulative_api_nanousd: spend.spent_api_nanousd,
        cumulative_native_millicredits: spend.spent_native_millicredits,
        observation_source: source.to_owned(),
        source_request_id: source_request_id.map(str::to_owned),
    }
}

fn observe_tripo3d_postgres(
    pg: &mut registry::pg::PgStore,
    observation: &Tripo3dBalanceObservation,
) -> anyhow::Result<Tripo3dCalibrationRow> {
    loop {
        let existing =
            pg.load_tripo3d_calibration(&observation.subject_id, &observation.cohort)?;
        let history = if existing
            .as_ref()
            .is_some_and(|row| row.estimator_version != crate::tripo3d_calibration::ESTIMATOR_VERSION)
        {
            pg.load_tripo3d_balance_observations(&observation.subject_id, &observation.cohort)?
        } else {
            Vec::new()
        };
        let mut state = crate::tripo3d_calibration::apply_observation_with_history(
            existing,
            &history,
            observation,
        )?;
        // The save validates the state/observation pair and applies the estimator CAS.
        if let Some(version) = pg.save_tripo3d_calibration(&state, observation)? {
            state.version = version;
            return Ok(state);
        }
    }
}

/// One parsed Suno billing-endpoint snapshot before the writer pairs it with the subject's
/// durable cumulative dual-ledger spend (same single-writer hop as GLM/Tripo3D). The counters
/// are raw provider evidence — semantics unproven (`docs/engine/SUNO_PROVIDER.md` §5.2/§6), so
/// they are preserved verbatim and unknown stays `None`, never `0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SunoQuotaSnapshot {
    pub observed_at: i64,
    pub total_credits_left: Option<i64>,
    pub monthly_limit: Option<i64>,
    pub monthly_usage: Option<i64>,
    pub period_raw: Option<String>,
}

/// Derive the exact window identity from the raw `period` field: a strict `YYYY-MM` calendar
/// month names its own reset anchor (the month's first instant) and its own exact duration in
/// seconds — evidence-derived, never a synthetic 30/31-day constant (migration 0050). Any
/// other shape is unproven semantics and fails closed (`None`): the observation is then not
/// writable at all, because the schema keys windows by their exact observed duration.
fn suno_month_window_from_period(period_raw: &str) -> Option<(i64, i64)> {
    let bytes = period_raw.as_bytes();
    if bytes.len() != 7 || bytes[4] != b'-' {
        return None;
    }
    let year: i64 = period_raw.get(..4)?.parse().ok()?;
    let month: i64 = period_raw.get(5..7)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1970..=9999).contains(&year) {
        return None;
    }
    let start = days_from_civil(year, month, 1)?.checked_mul(86_400)?;
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let end = days_from_civil(next_year, next_month, 1)?.checked_mul(86_400)?;
    Some((start, end - start))
}

/// Days since the unix epoch for a civil date (Howard Hinnant's algorithm), checked.
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    let shifted_year = if month <= 2 { year - 1 } else { year };
    let era = shifted_year.div_euclid(400);
    let year_of_era = shifted_year.rem_euclid(400);
    let month_prime = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = 365 * year_of_era + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era.checked_mul(146_097)?
        .checked_add(day_of_era)?
        .checked_sub(719_468)
}

/// Build the immutable Suno window observation: provider-side raw counters from the snapshot
/// plus the subject's exact durable cumulative dual ledgers, which only this serial writer may
/// read. The observation is source-tagged: a free poll invents no request id, a
/// response-carried read names the turn that carried it. The derived quota-path fraction stays
/// `None` until the endpoint's field semantics are proven live; the estimator's native-ledger
/// path drives until then.
fn suno_observation(
    subject_id: &str,
    plan: &str,
    snapshot: &SunoQuotaSnapshot,
    spend: SunoSubjectSpend,
    source: &str,
    source_request_id: Option<&str>,
) -> Option<SunoWindowObservation> {
    let (reset_at, window_duration_secs) =
        suno_month_window_from_period(snapshot.period_raw.as_deref()?)?;
    Some(SunoWindowObservation {
        subject_id: subject_id.to_owned(),
        plan: plan.to_owned(),
        window_duration_secs,
        reset_at: Some(reset_at),
        observed_at: snapshot.observed_at,
        native_limit_units: snapshot.monthly_limit,
        native_used_units: snapshot.monthly_usage,
        native_remaining_units: snapshot.total_credits_left,
        period_raw: snapshot.period_raw.clone(),
        used_fraction_units: None,
        measurement_resolution_fraction_units: None,
        cumulative_api_nanousd: spend.spent_api_nanousd,
        cumulative_native_millicredits: spend.spent_native_millicredits,
        observation_source: source.to_owned(),
        source_request_id: source_request_id.map(str::to_owned),
    })
}

fn observe_suno_postgres(
    pg: &mut registry::pg::PgStore,
    observation: &SunoWindowObservation,
) -> anyhow::Result<SunoCalibrationRow> {
    loop {
        let existing = pg.load_suno_calibration(
            &observation.subject_id,
            &observation.plan,
            observation.window_duration_secs,
        )?;
        let history = if existing
            .as_ref()
            .is_some_and(|row| row.estimator_version != crate::suno_calibration::ESTIMATOR_VERSION)
        {
            pg.load_suno_window_observations(
                &observation.subject_id,
                &observation.plan,
                observation.window_duration_secs,
            )?
        } else {
            Vec::new()
        };
        let mut state = crate::suno_calibration::apply_observation_with_history(
            existing,
            &history,
            observation,
        )?;
        // The save validates the state/observation pair and applies the estimator CAS.
        if let Some(version) = pg.save_suno_calibration(&state, observation)? {
            state.version = version;
            return Ok(state);
        }
    }
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
                elog::error(
                    "billing",
                    "Gemini calibration event quarantined after immutable replay conflict",
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
                elog::error(
                    "billing",
                    "Anthropic calibration event quarantined after immutable replay conflict",
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

// Закрывает окно отмены, пока `reserve().await` ещё не передал владение резервом вызывающему коду.
// Компенсация адресует durable request_id, поэтому повторный cancel/settle идемпотентен и не может
// вернуть резерв другого параллельного запроса того же аккаунта.
fn reserve_handoff_cancel_evidence(
    admitted_at: Option<i64>,
    current_epoch_seconds: i64,
) -> Option<RequestFactTerminalEvidence> {
    admitted_at.map(|admitted_at| RequestFactTerminalEvidence {
        terminal_at: current_epoch_seconds.max(admitted_at),
        http_status_code: None,
        provider_terminal_class: ProviderTerminalClass::Unknown,
        delivery_state: DeliveryState::NotStarted,
        downstream_disconnect: None,
        upstream_request_id: None,
        first_public_byte_at: None,
        internal_attempt_count: None,
        failure_class: None,
        tool_calls_in_output: None,
    })
}

struct ReserveHandoffGuard<'a> {
    writer: &'a mpsc::Sender<WriteCmd>,
    detached: Arc<DetachedDispatchTracker>,
    request_id: String,
    account_id: String,
    key: String,
    hold: i64,
    request_fact_admitted_at: Option<i64>,
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
                                terminal_evidence: reserve_handoff_cancel_evidence(
                                    self.request_fact_admitted_at,
                                    pool::now(),
                                ),
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
                        elog::error(
                            "billing",
                            "billing writer stopped before a detached command was queued",
                        );
                    }
                });
            } else {
                let _ = std::thread::Builder::new()
                    .name("billing-backpressure".into())
                    .spawn(move || {
                        let _dispatch = dispatch;
                        if writer.blocking_send(cmd).is_err() {
                            elog::error(
                                "billing",
                                "billing writer stopped before a detached command was queued",
                            );
                        }
                    });
            }
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            elog::error(
                "billing",
                "billing writer stopped before a detached command was queued",
            );
        }
    }
}

fn cancel_postgres_request(
    pg: &mut registry::pg::PgStore,
    request_id: &str,
    terminal_evidence: Option<&RequestFactTerminalEvidence>,
) -> anyhow::Result<Option<i64>> {
    match terminal_evidence {
        Some(evidence) => pg.cancel_request_with_request_fact(request_id, evidence),
        None => pg.cancel_request(request_id),
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
                elog::warn(
                    "billing",
                    format!("billing PostgreSQL {operation} transient failure: {error:#}"),
                );
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
    Tripo3dRecordTurn {
        turn: crate::tripo3d::queue::PendingTurn,
        reply: oneshot::Sender<anyhow::Result<bool>>,
    },
    Tripo3dObserveBalance {
        subject_id: String,
        cohort: String,
        snapshot: Tripo3dBalanceSnapshot,
        reply: oneshot::Sender<anyhow::Result<Tripo3dCalibrationRow>>,
    },
    SunoRecordTurn {
        turn: crate::suno::queue::PendingTurn,
        reply: oneshot::Sender<anyhow::Result<bool>>,
    },
    SunoObserveQuota {
        subject_id: String,
        plan: String,
        snapshot: SunoQuotaSnapshot,
        reply: oneshot::Sender<anyhow::Result<Option<SunoCalibrationRow>>>,
    },
    CodexRecordTurn {
        event: CodexTurnCalibrationEvent,
        reply: oneshot::Sender<anyhow::Result<CodexHomeCalibrationSpend>>,
    },
    CodexLoadHealth {
        home_id: String,
        reply: oneshot::Sender<anyhow::Result<registry::CodexHomeHealthRow>>,
    },
    /// Operator routability switch for roster-backed fleets. Read on roster load and on the
    /// pools' refresh tick; written from the admin route.
    PoolMemberSetDisabled {
        provider: &'static str,
        member_id: String,
        disabled: bool,
        hidden: bool,
        actor: String,
        reason: String,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    PoolMemberDisabled {
        provider: &'static str,
        reply: oneshot::Sender<anyhow::Result<std::collections::HashMap<String, bool>>>,
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
        pricing: Option<registry::ReservationPricing>,
        request_fact: Option<RequestFactAdmission>,
        handoff: Arc<AtomicU8>,
        reply: oneshot::Sender<anyhow::Result<Option<i64>>>,
    },
    InsertTariffOverride {
        insert: TariffOverrideInsert,
        reply: oneshot::Sender<anyhow::Result<TariffOverrideInsertOutcome>>,
    },
    CancelReserve {
        request_id: String,
        account_id: String,
        key: String,
        hold: i64,
        terminal_evidence: Option<RequestFactTerminalEvidence>,
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
        terminal_evidence: Option<RequestFactTerminalEvidence>,
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
    /// Set or clear one per-provider discount override. `mult_bp: None` removes the row, so the
    /// account falls back to its default multiplier.
    AccountProviderDiscount {
        id: String,
        provider_id: String,
        mult_bp: Option<i64>,
        reply: oneshot::Sender<anyhow::Result<bool>>,
    },
    KeyStatus {
        key: String,
        status: String,
        reply: oneshot::Sender<anyhow::Result<usize>>,
    },
    KeyStatusById {
        key_id: String,
        status: String,
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
        record_request_fact: bool,
        reply: oneshot::Sender<anyhow::Result<bool>>,
    },
    /// Durable "this turn has cost at least X so far", so a death before settlement no longer
    /// forces the reconciler to charge the ceiling. Fire-and-forget: a lost checkpoint only costs
    /// accuracy on a turn that also has to die, and must never slow the answer down.
    CheckpointMeasured {
        request_id: String,
        measured_nano: i64,
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
    GlmCalibrationReport(oneshot::Sender<anyhow::Result<Vec<GlmCalibrationRow>>>),
    Tripo3dCalibrationReport(oneshot::Sender<anyhow::Result<Vec<Tripo3dCalibrationRow>>>),
    SunoCalibrationReport(oneshot::Sender<anyhow::Result<Vec<SunoCalibrationRow>>>),
    KeyAuth(String, oneshot::Sender<anyhow::Result<Option<KeyAuth>>>),
    KeyGet(String, oneshot::Sender<anyhow::Result<Option<KeyRow>>>),
    Account(String, oneshot::Sender<anyhow::Result<Option<AccountRow>>>),
    /// Per-provider discount rows of one account, for the control-plane listing.
    AccountProviderDiscounts(String, oneshot::Sender<anyhow::Result<Vec<(String, i64)>>>),
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
    ListTariffOverrides(oneshot::Sender<anyhow::Result<Vec<TariffOverride>>>),
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

/// Latency of the single-writer PostgreSQL money commands, measured around `run_pg_with_retry`
/// so the observation covers reconnect and retry — the budget the request path actually pays.
/// Owned here rather than in `forward::Metrics` because the billing writer starts before that
/// struct exists; the `/metrics` handler reads a snapshot through `pg_command_stats`. Bucket
/// boundaries match the pricing-bridge histogram so operator thresholds stay comparable, and the
/// array sizes stay within what `#[derive(Default)]` can initialize.
pub const PG_COMMAND_LATENCY_BUCKETS_MS: [u64; 10] = [1, 2, 5, 10, 25, 50, 100, 250, 500, 1_000];

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
    terminal_request_facts: TerminalRequestFactInbox,
    readers: Vec<mpsc::Sender<ReadCmd>>,
    rr: AtomicUsize, // round-robin по читателям
    /// PostgreSQL-only connections reserved for evaluation-time shadow reads. They never share
    /// the customer authorization reader budget and are absent from live SQLite composition.
    /// Present only for the PostgreSQL authority; the SQLite fallback keeps no latency stats
    /// because it is never the production hot path.
    pg_command: Option<Arc<PgCommandMetrics>>,
    admin_changes: AdminChanges,
}

impl AsyncBilling {
    pub fn set_admin_changes(
        &self,
        sender: tokio::sync::broadcast::Sender<crate::AdminChange>,
    ) {
        *self
            .admin_changes
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(sender);
    }
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
        self.pg_command.as_deref().map(PgCommandMetrics::snapshot)
    }

    /// Nonblocking, fail-open submission for already-terminal post-auth/nonbillable facts. This
    /// queue owns no money command and is intentionally excluded from `flush`.
    pub fn try_submit_terminal_request_fact(
        &self,
        fact: TerminalRequestFact,
    ) -> TerminalRequestFactSubmission {
        self.terminal_request_facts.submit(fact)
    }

    pub fn request_fact_delivery_snapshot(&self) -> RequestFactDeliverySnapshot {
        self.terminal_request_facts.snapshot()
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
                elog::error(
                    "billing",
                    format!("Anthropic calibration evidence dropped: {error:#}"),
                );
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
                elog::error(
                    "billing",
                    format!("Gemini calibration evidence dropped: {error:#}"),
                );
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
    pub async fn kimi_calibration_report(&self) -> anyhow::Result<Vec<KimiCalibrationRow>> {
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

    /// Read every durable GLM window calibration row for the admin operational projection.
    ///
    /// Rows are keyed by provider subject; joining them to opaque roster ids is the caller's
    /// concern (the gateway owns that mapping). GLM calibration is PostgreSQL-only, so a SQLite
    /// authority reports an empty fleet rather than an error.
    pub async fn glm_calibration_report(&self) -> anyhow::Result<Vec<GlmCalibrationRow>> {
        let (reply, result) = oneshot::channel();
        let reader = &self.readers[self.rr.fetch_add(1, Ordering::Relaxed) % self.readers.len()];
        reader
            .send(ReadCmd::GlmCalibrationReport(reply))
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

    /// Read every durable Tripo3D balance-track calibration row for the admin operational
    /// projection.
    ///
    /// Rows are keyed by provider subject; joining them to opaque roster ids is the caller's
    /// concern (the gateway owns that mapping). Tripo3D calibration is PostgreSQL-only, so a
    /// SQLite authority reports an empty fleet rather than an error.
    pub async fn tripo3d_calibration_report(&self) -> anyhow::Result<Vec<Tripo3dCalibrationRow>> {
        let (reply, result) = oneshot::channel();
        let reader = &self.readers[self.rr.fetch_add(1, Ordering::Relaxed) % self.readers.len()];
        reader
            .send(ReadCmd::Tripo3dCalibrationReport(reply))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    /// Persist one immutable Tripo3D turn (a finalized upstream task) and advance the subject's
    /// exact dual ledgers (native millicredits AND the fixed-rate API nanoUSD image).
    ///
    /// Codex/Gemini pairing discipline: the entry carries the post-turn balance read taken in
    /// the task's wake, and this single writer command records the turn FIRST, then pairs the
    /// balance observation with the cumulative ledgers that already include it (`response`
    /// source naming the turn's request id) — an observation is never written against a spend
    /// total its own task has not reached. The gateway owns the bounded FIFO because it must
    /// gate balance polling on a fully drained queue; exact request-id replay is a no-op and a
    /// different payload is a permanent typed conflict.
    pub(crate) async fn record_tripo3d_turn(
        &self,
        turn: crate::tripo3d::queue::PendingTurn,
    ) -> anyhow::Result<bool> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::Tripo3dRecordTurn { turn, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Pair one Tripo3D balance snapshot with exact durable subject dual-ledger spend and
    /// advance the balance track through immutable observation + estimator CAS.
    ///
    /// The caller has already drained the gateway's bounded turn FIFO and keeps that drain
    /// barrier held until this command returns. The writer still owns the spend read so no
    /// async caller can accidentally construct an observation from a stale total.
    pub(crate) async fn observe_tripo3d_balance(
        &self,
        subject_id: &str,
        cohort: &str,
        snapshot: Tripo3dBalanceSnapshot,
    ) -> anyhow::Result<Tripo3dCalibrationRow> {
        if subject_id.is_empty() || cohort.is_empty() {
            anyhow::bail!("invalid Tripo3D balance observation command");
        }
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::Tripo3dObserveBalance {
                subject_id: subject_id.to_owned(),
                cohort: cohort.to_owned(),
                snapshot,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Read every durable Suno monthly-window calibration row for the admin operational
    /// projection.
    ///
    /// Rows are keyed by provider subject; joining them to opaque roster ids is the caller's
    /// concern (the gateway owns that mapping). Suno calibration is PostgreSQL-only, so a
    /// SQLite authority reports an empty fleet rather than an error.
    pub async fn suno_calibration_report(&self) -> anyhow::Result<Vec<SunoCalibrationRow>> {
        let (reply, result) = oneshot::channel();
        let reader = &self.readers[self.rr.fetch_add(1, Ordering::Relaxed) % self.readers.len()];
        reader
            .send(ReadCmd::SunoCalibrationReport(reply))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    /// Persist one immutable Suno turn (a finalized upstream generation) and advance the
    /// subject's exact dual ledgers (native millicredits AND the fixed-rate API nanoUSD image).
    ///
    /// Codex/Gemini pairing discipline: the entry carries the post-turn billing read taken in
    /// the generation's wake — the observation that attributes (or fails to attribute) the
    /// credit delta — and this single writer command records the turn FIRST, then pairs the
    /// quota observation with the cumulative ledgers that already include it (`response`
    /// source naming the turn's request id). The gateway owns the bounded FIFO because it must
    /// gate quota polling on a fully drained queue; exact request-id replay is a no-op and a
    /// different payload is a permanent typed conflict.
    pub(crate) async fn record_suno_turn(
        &self,
        turn: crate::suno::queue::PendingTurn,
    ) -> anyhow::Result<bool> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::SunoRecordTurn { turn, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Pair one Suno quota snapshot with exact durable subject dual-ledger spend and advance
    /// the monthly-window track through immutable observation + estimator CAS.
    ///
    /// The caller has already drained the gateway's bounded turn FIFO and keeps that drain
    /// barrier held until this command returns. The writer still owns the spend read so no
    /// async caller can accidentally construct an observation from a stale total. Returns
    /// `Ok(None)` when the snapshot's raw `period` names no exact window identity: an unknown
    /// duration fails closed and nothing is written (migration 0050 discipline).
    pub(crate) async fn observe_suno_quota(
        &self,
        subject_id: &str,
        plan: &str,
        snapshot: SunoQuotaSnapshot,
    ) -> anyhow::Result<Option<SunoCalibrationRow>> {
        if subject_id.is_empty() || plan.is_empty() {
            anyhow::bail!("invalid Suno quota observation command");
        }
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::SunoObserveQuota {
                subject_id: subject_id.to_owned(),
                plan: plan.to_owned(),
                snapshot,
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

    /// Pull a roster-backed pool member out of rotation, or put it back. The roster itself stays
    /// the Auth Bot's authority; this is the engine's separate, durable say over routability.
    pub async fn pool_member_set_disabled(
        &self,
        provider: &'static str,
        member_id: &str,
        disabled: bool,
        hidden: bool,
        actor: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::PoolMemberSetDisabled {
                provider,
                member_id: member_id.into(),
                disabled,
                hidden,
                actor: actor.into(),
                reason: reason.into(),
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Disabled members mapped to whether the operator also hid them from the board.
    pub async fn pool_member_disables(
        &self,
        provider: &'static str,
    ) -> anyhow::Result<std::collections::HashMap<String, bool>> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::PoolMemberDisabled { provider, reply })
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
        match config {
            registry::authority::AuthorityConfig::Sqlite { path } => {
                Self::start_with(path, readers, 0)
            }
            registry::authority::AuthorityConfig::Postgres { url } => {
                let owner = owner
                    .ok_or_else(|| anyhow::anyhow!("PostgreSQL billing requires owner epoch"))?;
                Self::start_postgres(url, owner, readers, 0)
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
        let admin_changes = Arc::new(RwLock::new(None));
        {
            let conn = registry::open(&db_path)?;
            let writer_anthropic_delivery = Arc::clone(&anthropic_calibration_delivery);
            let writer_gemini_delivery = Arc::clone(&gemini_calibration_delivery);
            let writer_admin_changes = Arc::clone(&admin_changes);
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
                            elog::error("billing", "billing reserve cancellation did not produce a balance");
                            handoff.store(RESERVE_HANDOFF_CANCELED, Ordering::Release);
                        }
                        Err(err) => {
                            elog::error(
                                "billing",
                                format!("billing reserve cancellation refund failed: {err:#}"),
                            );
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
                            elog::error(
                                "billing",
                                format!("billing reserve handoff entered unexpected state {state}"),
                            );
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
                            if result.is_ok() {
                                publish_admin_change(
                                    &writer_admin_changes,
                                    &["/subs", "/capacity", "/overview"],
                                    "anthropic_turn",
                                );
                            }
                            let _ = reply.send(result);
                        } else {
                            let result = flush_pending_anthropic_calibration_turns(
                                &writer_anthropic_delivery,
                                None,
                                &persist_anthropic_turn,
                            );
                            if result.is_ok() {
                                publish_admin_change(
                                    &writer_admin_changes,
                                    &["/subs", "/capacity", "/overview"],
                                    "anthropic_turn",
                                );
                            } else if let Err(error) = result {
                                elog::error(
                                    "billing",
                                    format!(
                                        "Anthropic calibration persistence deferred with FIFO head retained: {error:#}"
                                    ),
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
                            if result.is_ok() {
                                publish_admin_change(
                                    &writer_admin_changes,
                                    &["/gemini-subs", "/capacity", "/overview"],
                                    "gemini_turn",
                                );
                            }
                            let _ = reply.send(result);
                        } else {
                            let result = flush_pending_gemini_calibration_turns(
                                &writer_gemini_delivery,
                                None,
                                &persist_gemini_turn,
                            );
                            if result.is_ok() {
                                publish_admin_change(
                                    &writer_admin_changes,
                                    &["/gemini-subs", "/capacity", "/overview"],
                                    "gemini_turn",
                                );
                            } else if let Err(error) = result {
                                elog::error(
                                    "billing",
                                    format!(
                                        "Gemini calibration persistence deferred with FIFO head retained: {error:#}"
                                    ),
                                );
                            }
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
                    WriteCmd::Tripo3dRecordTurn { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "Tripo3D calibration authority requires PostgreSQL"
                        )));
                    }
                    WriteCmd::Tripo3dObserveBalance { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "Tripo3D calibration authority requires PostgreSQL"
                        )));
                    }
                    WriteCmd::SunoRecordTurn { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "Suno calibration authority requires PostgreSQL"
                        )));
                    }
                    WriteCmd::SunoObserveQuota { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "Suno calibration authority requires PostgreSQL"
                        )));
                    }
                    WriteCmd::CodexRecordTurn { event, reply } => {
                        let result = registry::record_codex_turn_calibration_event(
                            &conn, &event,
                        );
                        if result.is_ok() {
                            publish_admin_change(
                                &writer_admin_changes,
                                &["/codex-subs", "/capacity", "/overview"],
                                "codex_turn",
                            );
                        }
                        let _ = reply.send(result);
                    }
                    WriteCmd::CodexLoadHealth { home_id, reply } => {
                        let _ = reply.send(registry::load_codex_home_health(&conn, &home_id));
                    }
                    // The disable store is Stage 2 authority only: the SQLite importer path has no
                    // such table, and silently reporting "nothing disabled" would put a member the
                    // operator pulled straight back into rotation. Fail closed instead.
                    WriteCmd::PoolMemberSetDisabled { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "pool member disable requires PostgreSQL authority"
                        )));
                    }
                    WriteCmd::PoolMemberDisabled { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "pool member disable requires PostgreSQL authority"
                        )));
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
                    WriteCmd::Reserve {
                        request_id, account_id, key, hold, execution, pricing, handoff, reply, ..
                    } => {
                        let result = match pricing.as_ref() {
                            Some(pricing) => registry::sqlite_reserve_priced_request_for_execution(
                                &conn, &request_id, &account_id, &key, hold,
                                RESERVATION_LEASE_SECS, &execution, pricing,
                            ),
                            None => registry::sqlite_reserve_request_for_execution(
                                &conn, &request_id, &account_id, &key, hold,
                                RESERVATION_LEASE_SECS, &execution,
                            ),
                        };
                        finish_reserve(request_id, account_id, key, hold, handoff, reply, result);
                    }
                    WriteCmd::InsertTariffOverride { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "tariff override authority requires PostgreSQL"
                        )));
                    }
                    WriteCmd::CancelReserve {
                        request_id, account_id, key, hold, handoff, ..
                    } => {
                        refund_canceled_reserve(&request_id, &account_id, &key, hold, &handoff);
                    }
                    WriteCmd::Settle {
                        request_id, account_id, key, hold, actual, reference, usage, reply, ..
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
                            elog::error(
                                "billing",
                                format!(
                                    "billing SQLite settlement persisted/retryable failure: {error:#}"
                                ),
                            );
                        }
                        if result.is_ok() {
                            publish_admin_change(
                                &writer_admin_changes,
                                &["/overview", "/spend-stats", "/settlement-health"],
                                "settlement",
                            );
                        }
                        if let Some(reply) = reply { let _ = reply.send(result); }
                    }
                    WriteCmd::Topup { account_id, amount, reference, reply } => {
                        let _ = reply.send(registry::account_topup(&conn, &account_id, amount, reference.as_deref()));
                    }
                    WriteCmd::CreateAccount { id, handle, mult_bp, reply } => { let _ = reply.send(registry::account_create(&conn, &id, handle.as_deref(), mult_bp)); }
                    WriteCmd::IssueKey { key, account_id, label, spend_limit_nano, expires_ts, reply } => {
                        let _ = reply.send(registry::key_issue_with_policy(
                            &conn,&key,&account_id,label.as_deref(),spend_limit_nano,expires_ts,
                        ));
                    }
                    WriteCmd::AccountStatus { id, status, reply } => { let _ = reply.send(registry::account_set_status(&conn, &id, &status)); }
                    WriteCmd::AccountMultiplier { id, mult_bp, reply } => { let _ = reply.send(registry::account_set_mult_bp(&conn, &id, mult_bp)); }
                    WriteCmd::AccountProviderDiscount { id, provider_id, mult_bp, reply } => {
                        let result = match mult_bp {
                            Some(mult_bp) => registry::set_account_provider_discount(
                                &conn, &id, &provider_id, mult_bp, pool::now(),
                            ).map(|()| true),
                            None => registry::clear_account_provider_discount(&conn, &id, &provider_id),
                        };
                        let _ = reply.send(result);
                    }
                    WriteCmd::KeyStatus { key, status, reply } => { let _ = reply.send(registry::key_set_status(&conn, &key, &status)); }
                    WriteCmd::KeyStatusById { key_id, status, reply } => { let _ = reply.send(registry::key_set_status_by_id(&conn, &key_id, &status)); }
                    WriteCmd::KeyLabelById { key_id, label, reply } => { let _ = reply.send(registry::key_set_label_by_id(&conn, &key_id, &label)); }
                    WriteCmd::KeyPolicyById { account_id, key_id, spend_limit_nano, expires_ts, reply } => {
                        let _ = reply.send(registry::key_set_policy_by_id(
                            &conn,&account_id,&key_id,spend_limit_nano,expires_ts,
                        ));
                    }
                    WriteCmd::LedgerAck { consumer, account_id, last_id, reply } => {
                        let _ = reply.send(registry::ledger_ack(&conn, &consumer, &account_id, last_id));
                    }
                    WriteCmd::MarkDelivering { request_id, lease_secs, reply, .. } => {
                        let _ = reply.send(registry::sqlite_mark_delivering(
                            &conn, &request_id, lease_secs,
                        ));
                    }
                    // The SQLite fallback has no cross-process reconciler, so there is no reader for
                    // a checkpoint and nothing to protect against.
                    WriteCmd::CheckpointMeasured { .. } => {}
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
                            registry::sqlite_reconcile_expired(
                                &conn,
                                10_000,
                                crate::settlement_policy::charge_hold_on_unknown_usage(),
                            )
                            .map(|_| ())
                        });
                        let _ = reply.send(result);
                    }
                    }
                }
                elog::info("billing", "billing-writer поток завершён (все sender'ы дропнуты)"); // супервизия
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
                            ReadCmd::GlmCalibrationReport(reply) => {
                                // GLM calibration authority is PostgreSQL-only; a SQLite
                                // authority simply has no rows to report.
                                let _ = reply.send(Ok(Vec::new()));
                            }
                            ReadCmd::Tripo3dCalibrationReport(reply) => {
                                // Tripo3D calibration authority is PostgreSQL-only; a SQLite
                                // authority simply has no rows to report.
                                let _ = reply.send(Ok(Vec::new()));
                            }
                            ReadCmd::SunoCalibrationReport(reply) => {
                                // Suno calibration authority is PostgreSQL-only; a SQLite
                                // authority simply has no rows to report.
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
                            ReadCmd::AccountProviderDiscounts(id, r) => {
                                let _ = r.send(registry::account_provider_discounts(&conn, &id));
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
                            ReadCmd::ListTariffOverrides(reply) => {
                                let _ = reply.send(Err(anyhow::anyhow!(
                                    "tariff override authority requires PostgreSQL"
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
                    elog::info("billing", format!("billing-reader-{i} поток завершён"));
                })?;
            rtxs.push(rtx);
        }
        Ok(AsyncBilling {
            writer: wtx,
            detached: Arc::new(DetachedDispatchTracker::default()),
            anthropic_calibration_delivery,
            gemini_calibration_delivery,
            terminal_request_facts: TerminalRequestFactInbox::disabled(),
            readers: rtxs,
            rr: AtomicUsize::new(0),
            pg_command: None,
            admin_changes,
        })
    }

    fn start_postgres(
        url: String,
        owner: registry::pg::Owner,
        readers: usize,
        _auth_ttl_ms: u64,
    ) -> anyhow::Result<Self> {
        const RESERVATION_LEASE_SECS: i64 = 3600;
        let readers = readers.max(1);
        let (wtx, mut wrx) = mpsc::channel::<WriteCmd>(WRITE_QUEUE_CAPACITY);
        let anthropic_calibration_delivery = Arc::new(AnthropicCalibrationDeliveryState::default());
        let gemini_calibration_delivery = Arc::new(GeminiCalibrationDeliveryState::default());
        let pg_command = Arc::new(PgCommandMetrics::default());
        let admin_changes = Arc::new(RwLock::new(None));
        // This analytics-only writer opens its distinct connection lazily on the first submitted
        // terminal fact. Connection exhaustion can therefore never block money-authority startup.
        let terminal_request_facts =
            TerminalRequestFactInbox::start_postgres(url.clone(), PG_OPERATION_RETRY_DEADLINE);
        {
            let mut pg = registry::pg::PgStore::connect(&url)?;
            let writer_url = url.clone();
            let writer_owner = owner.clone();
            let writer_anthropic_delivery = Arc::clone(&anthropic_calibration_delivery);
            let writer_gemini_delivery = Arc::clone(&gemini_calibration_delivery);
            let writer_pg_command = Arc::clone(&pg_command);
            let writer_admin_changes = Arc::clone(&admin_changes);
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
                                if result.is_ok() {
                                    publish_admin_change(
                                        &writer_admin_changes,
                                        &["/subs", "/capacity", "/overview"],
                                        "anthropic_turn",
                                    );
                                }
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
                                if result.is_ok() {
                                    publish_admin_change(
                                        &writer_admin_changes,
                                        &["/subs", "/capacity", "/overview"],
                                        "anthropic_turn",
                                    );
                                } else if let Err(error) = result {
                                    elog::error(
                                        "billing",
                                        format!(
                                            "Anthropic calibration persistence deferred with FIFO head retained: {error:#}"
                                        ),
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
                                if result.is_ok() {
                                    publish_admin_change(
                                        &writer_admin_changes,
                                        &["/gemini-subs", "/capacity", "/overview"],
                                        "gemini_turn",
                                    );
                                }
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
                                if result.is_ok() {
                                    publish_admin_change(
                                        &writer_admin_changes,
                                        &["/gemini-subs", "/capacity", "/overview"],
                                        "gemini_turn",
                                    );
                                } else if let Err(error) = result {
                                    elog::error(
                                        "billing",
                                        format!(
                                            "Gemini calibration persistence deferred with FIFO head retained: {error:#}"
                                        ),
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
                            if result.is_ok() {
                                publish_admin_change(
                                    &writer_admin_changes,
                                    &["/kimi-subs", "/capacity", "/overview"],
                                    "kimi_turn",
                                );
                            }
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
                            if result.is_ok() {
                                publish_admin_change(
                                    &writer_admin_changes,
                                    &["/glm-subs", "/capacity", "/overview"],
                                    "glm_turn",
                                );
                            }
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
                        WriteCmd::Tripo3dRecordTurn { turn, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "Tripo3D turn calibration event",
                                |pg| {
                                    let event = &turn.event;
                                    // The turn lands FIRST so its spend is inside the cumulative
                                    // ledgers the paired balance observation then reads.
                                    let recorded = pg.record_tripo3d_turn(event)?;
                                    if let Some(snapshot) = &turn.balance {
                                        let spend = pg.tripo3d_subject_spend(&event.subject_id)?;
                                        let observation = tripo3d_observation(
                                            &event.subject_id,
                                            &event.cohort,
                                            snapshot,
                                            spend,
                                            "response",
                                            Some(&event.request_id),
                                        );
                                        observe_tripo3d_postgres(pg, &observation)?;
                                    }
                                    Ok(recorded)
                                },
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::Tripo3dObserveBalance {
                            subject_id,
                            cohort,
                            snapshot,
                            reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "Tripo3D balance observation",
                                |pg| {
                                    // Durable dual-ledger spend is read FIRST; only then may a
                                    // balance observation/CAS pair itself with it. Reversing the
                                    // order would let an observation see a balance that an
                                    // earlier settled task has not yet reached.
                                    let spend = pg.tripo3d_subject_spend(&subject_id)?;
                                    let observation = tripo3d_observation(
                                        &subject_id,
                                        &cohort,
                                        &snapshot,
                                        spend,
                                        "poll",
                                        None,
                                    );
                                    observe_tripo3d_postgres(pg, &observation)
                                },
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::SunoRecordTurn { turn, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "Suno turn calibration event",
                                |pg| {
                                    let event = &turn.event;
                                    // The turn lands FIRST so its spend is inside the cumulative
                                    // ledgers the paired quota observation then reads.
                                    let recorded = pg.record_suno_turn(event)?;
                                    if let Some(snapshot) = &turn.billing {
                                        let spend = pg.suno_subject_spend(&event.subject_id)?;
                                        // No derivable window identity (an absent/unparseable
                                        // `period`) fails closed: the turn event still
                                        // persists, but no observation is keyed on an unknown
                                        // duration.
                                        if let Some(observation) = suno_observation(
                                            &event.subject_id,
                                            &event.plan,
                                            snapshot,
                                            spend,
                                            "response",
                                            Some(&event.request_id),
                                        ) {
                                            observe_suno_postgres(pg, &observation)?;
                                        }
                                    }
                                    Ok(recorded)
                                },
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::SunoObserveQuota {
                            subject_id,
                            plan,
                            snapshot,
                            reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "Suno quota observation",
                                |pg| {
                                    // Durable dual-ledger spend is read FIRST; only then may a
                                    // quota observation/CAS pair itself with it. Reversing the
                                    // order would let an observation see quota that an earlier
                                    // settled generation has not yet reached.
                                    let spend = pg.suno_subject_spend(&subject_id)?;
                                    let Some(observation) = suno_observation(
                                        &subject_id,
                                        &plan,
                                        &snapshot,
                                        spend,
                                        "poll",
                                        None,
                                    ) else {
                                        // No derivable window identity: fail closed, write
                                        // nothing, and tell the caller honestly.
                                        return Ok(None);
                                    };
                                    observe_suno_postgres(pg, &observation).map(Some)
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
                            if result.is_ok() {
                                publish_admin_change(
                                    &writer_admin_changes,
                                    &["/codex-subs", "/capacity", "/overview"],
                                    "codex_turn",
                                );
                            }
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
                        WriteCmd::PoolMemberSetDisabled {
                            provider,
                            member_id,
                            disabled,
                            hidden,
                            actor,
                            reason,
                            reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "pool member disable write",
                                |pg| {
                                    pg.pool_member_set_disabled(
                                        provider, &member_id, disabled, hidden, &actor, &reason,
                                    )
                                },
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::PoolMemberDisabled { provider, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "pool member disable read",
                                |pg| pg.pool_member_disables(provider),
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
                        WriteCmd::Reserve {
                            request_id,
                            account_id,
                            key,
                            hold,
                            execution,
                            pricing,
                            request_fact,
                            handoff,
                            reply,
                        } => {
                            let result = {
                                let _timer = writer_pg_command.timer(PgCommandOp::Reserve);
                                run_pg_with_retry(
                                    &mut pg,
                                    &writer_url,
                                    &writer_owner,
                                    "reserve",
                                    |pg| match (pricing.as_ref(), request_fact.as_ref()) {
                                        (Some(pricing), Some(fact)) => pg
                                            .reserve_priced_request_for_execution_with_fact(
                                                &writer_owner,
                                                &request_id,
                                                &account_id,
                                                &key,
                                                hold,
                                                RESERVATION_LEASE_SECS,
                                                &execution,
                                                pricing,
                                                fact,
                                            ),
                                        (Some(pricing), None) => pg
                                            .reserve_priced_request_for_execution(
                                                &writer_owner,
                                                &request_id,
                                                &account_id,
                                                &key,
                                                hold,
                                                RESERVATION_LEASE_SECS,
                                                &execution,
                                                pricing,
                                            ),
                                        (None, Some(fact)) => pg
                                            .reserve_request_for_execution_with_fact(
                                                &writer_owner,
                                                &request_id,
                                                &account_id,
                                                &key,
                                                hold,
                                                RESERVATION_LEASE_SECS,
                                                &execution,
                                                fact,
                                            ),
                                        (None, None) => pg.reserve_request_for_execution(
                                            &writer_owner,
                                            &request_id,
                                            &account_id,
                                            &key,
                                            hold,
                                            RESERVATION_LEASE_SECS,
                                            &execution,
                                        ),
                                    },
                                )
                            };
                            let result = match result {
                                Ok(result) => result,
                                Err(error) => {
                                    elog::error(
                                        "billing",
                                        format!("billing PostgreSQL reserve failed: {error:#}"),
                                    );
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
                            let request_fact_admitted_at =
                                request_fact.as_ref().map(|fact| fact.admitted_at);
                            match handoff.compare_exchange(
                                RESERVE_HANDOFF_PENDING,
                                RESERVE_HANDOFF_COMMITTED,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            ) {
                                Ok(_) => {
                                    if reply.send(Ok(result)).is_err() {
                                        let _ = handoff.compare_exchange(
                                            RESERVE_HANDOFF_COMMITTED,
                                            RESERVE_HANDOFF_CANCELED,
                                            Ordering::AcqRel,
                                            Ordering::Acquire,
                                        );
                                        let terminal_evidence = reserve_handoff_cancel_evidence(
                                            request_fact_admitted_at,
                                            pool::now(),
                                        );
                                        match run_pg_with_retry(
                                            &mut pg,
                                            &writer_url,
                                            &writer_owner,
                                            "canceled reserve",
                                            |pg| {
                                                cancel_postgres_request(
                                                    pg,
                                                    &request_id,
                                                    terminal_evidence.as_ref(),
                                                )
                                            },
                                        ) {
                                            Ok(_) => handoff.store(
                                                RESERVE_HANDOFF_REFUNDED,
                                                Ordering::Release,
                                            ),
                                            Err(error) => elog::error(
                                                "billing",
                                                format!(
                                                    "billing PostgreSQL canceled reserve failed: {error:#}"
                                                ),
                                            ),
                                        }
                                    }
                                }
                                Err(RESERVE_HANDOFF_CANCELED) => {
                                    let terminal_evidence = reserve_handoff_cancel_evidence(
                                        request_fact_admitted_at,
                                        pool::now(),
                                    );
                                    match run_pg_with_retry(
                                        &mut pg,
                                        &writer_url,
                                        &writer_owner,
                                        "reserve handoff cancel",
                                        |pg| {
                                            cancel_postgres_request(
                                                pg,
                                                &request_id,
                                                terminal_evidence.as_ref(),
                                            )
                                        },
                                    ) {
                                        Ok(_) => handoff.store(
                                            RESERVE_HANDOFF_REFUNDED,
                                            Ordering::Release,
                                        ),
                                        Err(error) => elog::error(
                                            "billing",
                                            format!(
                                                "billing PostgreSQL reserve handoff cancel failed: {error:#}"
                                            ),
                                        ),
                                    }
                                }
                                Err(state) => elog::error(
                                    "billing",
                                    format!(
                                        "billing PostgreSQL reserve handoff unexpected state {state}"
                                    ),
                                ),
                            }
                        }
                        WriteCmd::InsertTariffOverride { insert, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "tariff override insert",
                                |pg| pg.insert_tariff_override(&insert),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::CancelReserve {
                            request_id,
                            terminal_evidence,
                            handoff,
                            ..
                        } => {
                            if handoff
                                .compare_exchange(
                                    RESERVE_HANDOFF_CANCELED,
                                    RESERVE_HANDOFF_REFUNDING,
                                    Ordering::AcqRel,
                                    Ordering::Acquire,
                                )
                                .is_err()
                            {
                                continue;
                            }
                            match run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "cancellation",
                                |pg| {
                                    cancel_postgres_request(
                                        pg,
                                        &request_id,
                                        terminal_evidence.as_ref(),
                                    )
                                },
                            ) {
                                Ok(_) => handoff.store(RESERVE_HANDOFF_REFUNDED, Ordering::Release),
                                Err(error) => {
                                    elog::error(
                                        "billing",
                                        format!("billing PostgreSQL cancellation failed: {error:#}"),
                                    );
                                    handoff.store(RESERVE_HANDOFF_CANCELED, Ordering::Release);
                                }
                            }
                        }
                        WriteCmd::Settle {
                            request_id,
                            actual,
                            reference,
                            usage,
                            terminal_evidence,
                            reply,
                            ..
                        } => {
                            let result = {
                                let _timer = writer_pg_command.timer(PgCommandOp::Settle);
                                run_pg_with_retry(
                                    &mut pg,
                                    &writer_url,
                                    &writer_owner,
                                    "settlement",
                                    |pg| match terminal_evidence.as_ref() {
                                        Some(evidence) if actual == 0 && usage.is_none() => pg
                                            .cancel_request_with_request_fact(
                                                &request_id,
                                                evidence,
                                            ),
                                        Some(evidence) => pg.settle_request_with_request_fact(
                                            &request_id,
                                            actual,
                                            reference.as_deref(),
                                            usage.as_ref(),
                                            evidence,
                                        ),
                                        None if actual == 0 && usage.is_none() => {
                                            pg.cancel_request(&request_id)
                                        }
                                        None => pg.settle_request(
                                            &request_id,
                                            actual,
                                            reference.as_deref(),
                                            usage.as_ref(),
                                        ),
                                    },
                                )
                            };
                            if let Err(error) = &result {
                                elog::error(
                                    "billing",
                                    format!("billing PostgreSQL settlement failed: {error:#}"),
                                );
                            }
                            if result.is_ok() {
                                publish_admin_change(
                                    &writer_admin_changes,
                                    &["/overview", "/spend-stats", "/settlement-health"],
                                    "settlement",
                                );
                            }
                            if let Some(reply) = reply { let _ = reply.send(result); }
                        }
                        WriteCmd::MarkDelivering {
                            request_id,
                            lease_secs,
                            record_request_fact,
                            reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "delivery marker",
                                |pg| {
                                    if record_request_fact {
                                        pg.mark_delivering_with_request_fact(
                                            &writer_owner,
                                            &request_id,
                                            lease_secs,
                                        )
                                    } else {
                                        pg.mark_delivering(
                                            &writer_owner,
                                            &request_id,
                                            lease_secs,
                                        )
                                    }
                                },
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::CheckpointMeasured { request_id, measured_nano } => {
                            let _ = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "measured-cost checkpoint",
                                |pg| pg.checkpoint_measured(
                                    &writer_owner, &request_id, measured_nano,
                                ),
                            );
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
                                elog::error(
                                    "billing",
                                    format!("capacity lease release failed: {error:#}"),
                                );
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
                        WriteCmd::IssueKey { key, account_id, label, spend_limit_nano, expires_ts, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "key issuance",
                                |pg| pg.key_issue_with_policy(
                                    &key,&account_id,label.as_deref(),spend_limit_nano,expires_ts,
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
                        WriteCmd::AccountProviderDiscount { id, provider_id, mult_bp, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "account provider discount update",
                                |pg| match mult_bp {
                                    Some(mult_bp) => pg
                                        .set_account_provider_discount(&id, &provider_id, mult_bp, pool::now())
                                        .map(|()| true),
                                    None => pg.clear_account_provider_discount(&id, &provider_id),
                                },
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::KeyStatus { key, status, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "key status update",
                                |pg| pg.key_set_status(&key,&status),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::KeyStatusById { key_id, status, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "key status update",
                                |pg| pg.key_set_status_by_id(&key_id,&status),
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
                                            elog::error(
                                                "billing",
                                                format!("billing PostgreSQL outbox drain failed: {error:#}"),
                                            );
                                            break Err(error);
                                        }
                                    }
                                }
                            });
                            let _ = reply.send(result);
                        }
                    }
                }
                elog::info("billing", "billing-pg-writer thread stopped");
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
                                        elog::error(
                                            "billing",
                                            format!(
                                                "billing PostgreSQL read failed closed: {err:#}"
                                            ),
                                        );
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
                            ReadCmd::GlmCalibrationReport(reply) => {
                                answer!(reply, pg.list_glm_calibrations())
                            }
                            ReadCmd::Tripo3dCalibrationReport(reply) => {
                                answer!(reply, pg.list_tripo3d_calibrations())
                            }
                            ReadCmd::SunoCalibrationReport(reply) => {
                                answer!(reply, pg.list_suno_calibrations())
                            }
                            ReadCmd::KeyAuth(k, r) => answer!(r, pg.key_account(&k)),
                            ReadCmd::KeyGet(k, r) => answer!(r, pg.key_get(&k)),
                            ReadCmd::Account(id, r) => answer!(r, pg.account_get(&id)),
                            ReadCmd::AccountProviderDiscounts(id, r) => {
                                answer!(r, pg.account_provider_discounts(&id))
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
                            ReadCmd::ListTariffOverrides(reply) => {
                                answer!(reply, pg.list_tariff_overrides())
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
        Ok(AsyncBilling {
            writer: wtx,
            detached: Arc::new(DetachedDispatchTracker::default()),
            anthropic_calibration_delivery,
            gemini_calibration_delivery,
            terminal_request_facts,
            readers: rtxs,
            rr: AtomicUsize::new(0),
            pg_command: Some(pg_command),
            admin_changes,
        })
    }

    fn reader(&self) -> &mpsc::Sender<ReadCmd> {
        let i = self.rr.fetch_add(1, Ordering::Relaxed) % self.readers.len();
        &self.readers[i]
    }

    /// Every hot tariff override row, ordered by (family, version). Read path, like the other
    /// pricing reads; the authority is PostgreSQL-only (the SQLite fallback answers with a typed
    /// unavailability error).
    pub async fn list_tariff_overrides(&self) -> anyhow::Result<Vec<TariffOverride>> {
        let (reply, result) = oneshot::channel();
        self.reader()
            .send(ReadCmd::ListTariffOverrides(reply))
            .await
            .map_err(|_| anyhow::anyhow!("billing reader unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing reader stopped"))?
    }

    /// Append one hot tariff override version through the single-writer actor, exactly like the
    /// other control-plane pricing writes.
    pub async fn insert_tariff_override(
        &self,
        insert: TariffOverrideInsert,
    ) -> anyhow::Result<TariffOverrideInsertOutcome> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::InsertTariffOverride { insert, reply })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
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
    /// Per-provider discount overrides of one account (empty when a single default prices it).
    pub async fn account_provider_discounts(&self, id: &str) -> anyhow::Result<Vec<(String, i64)>> {
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::AccountProviderDiscounts(id.into(), r))
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
        self.reserve_request_for_execution_with_pricing(
            request_id, account_id, key, hold, execution, None, None,
        )
        .await
    }

    pub async fn reserve_request_for_execution_with_fact(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
        execution: registry::ExecutionAttempt,
        request_fact: RequestFactAdmission,
    ) -> anyhow::Result<Option<i64>> {
        self.reserve_request_for_execution_with_pricing(
            request_id,
            account_id,
            key,
            hold,
            execution,
            None,
            Some(request_fact),
        )
        .await
    }

    pub async fn reserve_priced_request_for_execution(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
        execution: registry::ExecutionAttempt,
        provider: &str,
        payable_multiplier_bp: i64,
    ) -> anyhow::Result<Option<i64>> {
        let pricing = registry::ReservationPricing::new(provider, payable_multiplier_bp)?;
        self.reserve_request_for_execution_with_pricing(
            request_id,
            account_id,
            key,
            hold,
            execution,
            Some(pricing),
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn reserve_priced_request_for_execution_with_fact(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
        execution: registry::ExecutionAttempt,
        provider: &str,
        payable_multiplier_bp: i64,
        request_fact: RequestFactAdmission,
    ) -> anyhow::Result<Option<i64>> {
        let pricing = registry::ReservationPricing::new(provider, payable_multiplier_bp)?;
        self.reserve_request_for_execution_with_pricing(
            request_id,
            account_id,
            key,
            hold,
            execution,
            Some(pricing),
            Some(request_fact),
        )
        .await
    }

    fn validate_request_fact_reservation_binding(
        fact: &RequestFactAdmission,
        request_id: &str,
        account_id: &str,
        execution: &registry::ExecutionAttempt,
    ) -> anyhow::Result<()> {
        fact.validate()?;
        if fact.billing_request_id != request_id {
            anyhow::bail!("request fact does not match the billing request identity");
        }
        if fact.account_id != account_id {
            anyhow::bail!("request fact does not match the billing account identity");
        }
        if fact.execution_group_id.as_deref() != execution.group_id() {
            anyhow::bail!("request fact does not match the execution group identity");
        }
        if fact.attempt != execution.attempt() {
            anyhow::bail!("request fact does not match the execution attempt identity");
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn reserve_request_for_execution_with_pricing(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
        execution: registry::ExecutionAttempt,
        pricing: Option<registry::ReservationPricing>,
        request_fact: Option<RequestFactAdmission>,
    ) -> anyhow::Result<Option<i64>> {
        if let Some(fact) = request_fact.as_ref() {
            Self::validate_request_fact_reservation_binding(
                fact, request_id, account_id, &execution,
            )?;
        }
        let request_fact_admitted_at = request_fact.as_ref().map(|fact| fact.admitted_at);
        let (reply, result) = oneshot::channel();
        let handoff = Arc::new(AtomicU8::new(RESERVE_HANDOFF_PENDING));
        let guard = ReserveHandoffGuard {
            writer: &self.writer,
            detached: self.detached.clone(),
            request_id: request_id.into(),
            account_id: account_id.into(),
            key: key.into(),
            hold,
            request_fact_admitted_at,
            handoff: Arc::clone(&handoff),
        };
        self.writer
            .send(WriteCmd::Reserve {
                request_id: request_id.into(),
                account_id: account_id.into(),
                key: key.into(),
                hold,
                execution,
                pricing,
                request_fact,
                handoff,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        match result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))??
        {
            Some(balance) if guard.claim() => Ok(Some(balance)),
            Some(_) => Err(anyhow::anyhow!("reservation handoff was canceled")),
            None => Ok(None),
        }
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
        self.settle_request_inner(
            request_id, account_id, key, hold, actual, reference, None, None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn settle_request_with_request_fact(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
        actual: i64,
        reference: Option<&str>,
        terminal_evidence: RequestFactTerminalEvidence,
    ) -> anyhow::Result<Option<i64>> {
        self.settle_request_inner(
            request_id,
            account_id,
            key,
            hold,
            actual,
            reference,
            None,
            Some(terminal_evidence),
        )
        .await
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
        self.settle_request_inner(
            request_id, account_id, key, hold, actual, reference, usage, None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments, dead_code)]
    pub(crate) async fn settle_request_with_usage_and_request_fact(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
        actual: i64,
        reference: Option<&str>,
        usage: Option<registry::UsageEventInput>,
        terminal_evidence: RequestFactTerminalEvidence,
    ) -> anyhow::Result<Option<i64>> {
        self.settle_request_inner(
            request_id,
            account_id,
            key,
            hold,
            actual,
            reference,
            usage,
            Some(terminal_evidence),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn settle_request_inner(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
        actual: i64,
        reference: Option<&str>,
        usage: Option<registry::UsageEventInput>,
        terminal_evidence: Option<RequestFactTerminalEvidence>,
    ) -> anyhow::Result<Option<i64>> {
        if let Some(evidence) = terminal_evidence.as_ref() {
            evidence.validate(0)?;
        }
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
                terminal_evidence,
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
        self.dispatch_settlement(
            request_id, account_id, key, hold, actual, reference, usage, None,
        );
    }

    #[allow(clippy::too_many_arguments, dead_code)]
    pub(crate) fn settle_detached_with_request_fact(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
        actual: i64,
        reference: Option<&str>,
        usage: Option<registry::UsageEventInput>,
        terminal_evidence: RequestFactTerminalEvidence,
    ) -> anyhow::Result<()> {
        terminal_evidence.validate(0)?;
        self.dispatch_settlement(
            request_id,
            account_id,
            key,
            hold,
            actual,
            reference,
            usage,
            Some(terminal_evidence),
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_settlement(
        &self,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold: i64,
        actual: i64,
        reference: Option<&str>,
        usage: Option<registry::UsageEventInput>,
        terminal_evidence: Option<RequestFactTerminalEvidence>,
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
                reference: reference.map(str::to_string),
                usage,
                terminal_evidence,
                reply: None,
            },
        );
    }

    /// Publish the measured cost of a turn that is still streaming.
    ///
    /// Detached on purpose: this runs on the hot path of a live answer, and a customer must never
    /// wait on a bookkeeping write. Losing one checkpoint costs a little accuracy on a turn that
    /// also has to die before settlement; blocking the stream would cost every turn.
    pub fn checkpoint_measured_detached(&self, request_id: &str, measured_nano: i64) {
        dispatch_detached(
            &self.writer,
            &self.detached,
            WriteCmd::CheckpointMeasured {
                request_id: request_id.into(),
                measured_nano,
            },
        );
    }

    pub async fn mark_delivering(&self, request_id: &str, lease_secs: i64) -> anyhow::Result<bool> {
        self.mark_delivering_inner(request_id, lease_secs, false)
            .await
    }

    pub async fn mark_delivering_with_request_fact(
        &self,
        request_id: &str,
        lease_secs: i64,
    ) -> anyhow::Result<bool> {
        self.mark_delivering_inner(request_id, lease_secs, true)
            .await
    }

    async fn mark_delivering_inner(
        &self,
        request_id: &str,
        lease_secs: i64,
        record_request_fact: bool,
    ) -> anyhow::Result<bool> {
        let (reply, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::MarkDelivering {
                request_id: request_id.into(),
                lease_secs,
                record_request_fact,
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
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::IssueKey {
                key: key.into(),
                account_id: account_id.into(),
                label: label.map(|s| s.into()),
                spend_limit_nano,
                expires_ts,
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
    /// Set (`Some`) or clear (`None`) one provider discount override for an account. Returns
    /// whether a row was written or removed.
    pub async fn account_provider_discount(
        &self,
        id: &str,
        provider_id: &str,
        mult_bp: Option<i64>,
    ) -> anyhow::Result<bool> {
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::AccountProviderDiscount {
                id: id.into(),
                provider_id: provider_id.into(),
                mult_bp,
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    pub async fn key_status(&self, key: &str, status: &str) -> anyhow::Result<usize> {
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::KeyStatus {
                key: key.into(),
                status: status.into(),
                reply: r,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
    pub async fn key_status_by_id(&self, key_id: &str, status: &str) -> anyhow::Result<usize> {
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::KeyStatusById {
                key_id: key_id.into(),
                status: status.into(),
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
                    elog::warn(
                        "billing",
                        format!(
                            "Provider calibration shutdown drain waiting for authority recovery: {error:#}"
                        ),
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
mod tests;
