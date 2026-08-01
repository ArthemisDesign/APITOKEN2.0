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
    LegacyScalarAdmissionSnapshot, LegacyScalarReserveOutcome, PolicyActiveExpectation,
    PolicyAdmissionSnapshot, PolicyReserveOutcome, PricingCatalogSpec, PricingMutation,
    PricingReadBundle, PricingShadowAdmissionEvaluationInput, PricingShadowEvaluationWrite,
    ProviderSwitchSpec, VersionTarget,
};
use registry::{
    AccountFundingSnapshot, AccountRow, BillingTotals, CodexCalibrationRow,
    CodexHomeCalibrationSpend, CodexTurnCalibrationAggregate, CodexTurnCalibrationEvent,
    CodexWindowObservation, GeminiCalibrationRow, GeminiWindowObservation, KeyActivationPolicyAck,
    KeyAuth, KeyPolicyUpdate, KeyRow,
};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, Notify};

/// Billing queues are deliberately bounded. The request path applies async backpressure instead of
/// retaining an arbitrary number of commands while PostgreSQL/SQLite is unavailable.
const WRITE_QUEUE_CAPACITY: usize = 4_096;
const READ_QUEUE_CAPACITY: usize = 1_024;
const PRICING_SHADOW_READ_QUEUE_CAPACITY: usize = 256;
const PG_OPERATION_RETRY_DEADLINE: Duration = Duration::from_secs(5);

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
    handoff: &AtomicU8,
    lease_secs: i64,
) -> anyhow::Result<LegacyScalarReserveOutcome> {
    let deadline = Instant::now() + PG_OPERATION_RETRY_DEADLINE;
    loop {
        match pg.reserve_request_with_legacy_snapshot_guarded(
            owner,
            key,
            lease_secs,
            snapshot,
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
    handoff: &AtomicU8,
    lease_secs: i64,
) -> anyhow::Result<PolicyReserveOutcome> {
    let deadline = Instant::now() + PG_OPERATION_RETRY_DEADLINE;
    loop {
        match pg.reserve_request_with_policy_snapshot_guarded(
            owner,
            key,
            lease_secs,
            snapshot,
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

enum WriteCmd {
    GeminiCreditSpend {
        profile_id: String,
        delta_nano: i64,
        updated_ts: i64,
        reply: oneshot::Sender<anyhow::Result<i64>>,
    },
    GeminiObserveWindow {
        profile_id: String,
        bucket_id: String,
        window_kind: String,
        window_duration_mins: i64,
        resets_at: i64,
        used_fraction_units: i64,
        observed_at: i64,
        reply: oneshot::Sender<anyhow::Result<(i64, GeminiCalibrationRow)>>,
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
        handoff: Arc<AtomicU8>,
        reply: oneshot::Sender<anyhow::Result<Option<i64>>>,
    },
    ReserveWithLegacySnapshot {
        key: String,
        snapshot: LegacyScalarAdmissionSnapshot,
        handoff: Arc<AtomicU8>,
        reply: oneshot::Sender<anyhow::Result<LegacyScalarReserveOutcome>>,
    },
    ReserveWithPolicySnapshot {
        key: String,
        snapshot: PolicyAdmissionSnapshot,
        handoff: Arc<AtomicU8>,
        reply: oneshot::Sender<anyhow::Result<PolicyReserveOutcome>>,
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
    ActivateAccountPolicy {
        activation: AccountPolicyActivationSpec,
        expectation: PolicyActiveExpectation,
        reply: oneshot::Sender<anyhow::Result<PricingMutation>>,
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
        max_inflight: i64,
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
    CodexCalibrationReport(oneshot::Sender<anyhow::Result<Vec<CodexTurnCalibrationAggregate>>>),
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

/// Async-фасад биллинга: writer-канал + пул reader-каналов. Клонируется (в `Arc`) во все хендлеры.
pub struct AsyncBilling {
    writer: mpsc::Sender<WriteCmd>,
    detached: Arc<DetachedDispatchTracker>,
    readers: Vec<mpsc::Sender<ReadCmd>>,
    rr: AtomicUsize, // round-robin по читателям
    /// PostgreSQL-only connections reserved for evaluation-time shadow reads. They never share
    /// the customer authorization reader budget and are absent from live SQLite composition.
    pricing_shadow_readers: Vec<mpsc::Sender<PricingShadowReadCmd>>,
    pricing_shadow_rr: AtomicUsize,
}

impl AsyncBilling {
    pub(crate) fn track_detached_work(&self) -> DetachedDispatchGuard {
        self.detached.begin()
    }

    /// Persist exact official-price spend for every successful Gemini generation, including
    /// unmetered admin traffic, and return the durable cumulative total for the serving profile.
    pub async fn credit_gemini_spend(
        &self,
        profile_id: &str,
        delta_nano: i64,
        updated_ts: i64,
    ) -> anyhow::Result<i64> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::GeminiCreditSpend {
                profile_id: profile_id.into(),
                delta_nano,
                updated_ts,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }

    /// Pair one exact provider quota-summary snapshot with durable cumulative profile spend.
    #[allow(clippy::too_many_arguments)]
    pub async fn observe_gemini_window(
        &self,
        profile_id: &str,
        bucket_id: &str,
        window_kind: &str,
        window_duration_mins: i64,
        resets_at: i64,
        used_fraction_units: i64,
        observed_at: i64,
    ) -> anyhow::Result<(i64, GeminiCalibrationRow)> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::GeminiObserveWindow {
                profile_id: profile_id.into(),
                bucket_id: bucket_id.into(),
                window_kind: window_kind.into(),
                window_duration_mins,
                resets_at,
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
        {
            let conn = registry::open(&db_path)?;
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
                while let Some(cmd) = wrx.blocking_recv() {
                    match cmd {
                    WriteCmd::GeminiCreditSpend { profile_id, delta_nano, updated_ts, reply } => {
                        let _ = reply.send(registry::credit_gemini_profile_spend(
                            &conn, &profile_id, delta_nano, updated_ts,
                        ));
                    }
                    WriteCmd::GeminiObserveWindow {
                        profile_id, bucket_id, window_kind, window_duration_mins, resets_at,
                        used_fraction_units, observed_at, reply,
                    } => {
                        let result = (|| {
                            let spend_nano = registry::gemini_profile_spend(&conn, &profile_id)?;
                            let observation = GeminiWindowObservation {
                                profile_id: profile_id.clone(),
                                bucket_id: bucket_id.clone(),
                                window_kind: window_kind.clone(),
                                window_duration_mins,
                                resets_at,
                                observed_at,
                                used_fraction_units,
                                gateway_spend_nano: spend_nano,
                            };
                            loop {
                                let existing = registry::load_gemini_calibration(
                                    &conn, &profile_id, &bucket_id,
                                )?;
                                if let Some(existing) = existing.as_ref().filter(|row| {
                                    row.estimator_version == crate::gemini::ESTIMATOR_VERSION
                                        && observed_at <= row.observed_at
                                }) {
                                    return Ok((spend_nano, existing.clone()));
                                }
                                let history = if existing.as_ref().is_some_and(|row| {
                                    row.estimator_version != crate::gemini::ESTIMATOR_VERSION
                                }) {
                                    registry::load_gemini_window_observations(
                                        &conn, &profile_id, &bucket_id,
                                    )?
                                } else {
                                    Vec::new()
                                };
                                let mut state = crate::gemini::apply_observation_with_history(
                                    existing, &history, &observation,
                                )?;
                                if let Some(version) = registry::save_gemini_calibration(
                                    &conn, &state, &observation,
                                )? {
                                    state.version = version;
                                    return Ok((spend_nano, state));
                                }
                            }
                        })();
                        let _ = reply.send(result);
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
                    WriteCmd::Reserve { request_id, account_id, key, hold, handoff, reply } => {
                        let result = registry::sqlite_reserve_request(
                            &conn, &request_id, &account_id, &key, hold, RESERVATION_LEASE_SECS,
                        );
                        finish_reserve(request_id, account_id, key, hold, handoff, reply, result);
                    }
                    WriteCmd::ReserveWithLegacySnapshot { key, snapshot, handoff, reply } => {
                        if handoff.load(Ordering::Acquire) == SNAPSHOT_RESERVE_HANDOFF_CANCELED {
                            let _ = reply.send(Ok(
                                LegacyScalarReserveOutcome::AbortedBeforeCommit,
                            ));
                            continue;
                        }
                        let result = registry::sqlite_reserve_request_with_legacy_snapshot_guarded(
                            &conn,
                            &key,
                            RESERVATION_LEASE_SECS,
                            &snapshot,
                            || authorize_snapshot_reserve_commit(&handoff),
                        );
                        finish_snapshot_reserve(&handoff, reply, result);
                    }
                    WriteCmd::ReserveWithPolicySnapshot { key, snapshot, handoff, reply } => {
                        if handoff.load(Ordering::Acquire) == SNAPSHOT_RESERVE_HANDOFF_CANCELED {
                            let _ = reply.send(Ok(PolicyReserveOutcome::AbortedBeforeCommit));
                            continue;
                        }
                        let result = registry::sqlite_reserve_request_with_policy_snapshot_guarded(
                            &conn,
                            &key,
                            RESERVATION_LEASE_SECS,
                            &snapshot,
                            || authorize_snapshot_reserve_commit(&handoff),
                        );
                        finish_policy_snapshot_reserve(&handoff, reply, result);
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
                    WriteCmd::ActivateAccountPolicy { activation, expectation, reply } => {
                        let _ = reply.send(registry::pricing::sqlite_activate_account_policy(
                            &conn, &activation, &expectation,
                        ));
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
                        let result = registry::sqlite_reconcile_expired(&conn, 10_000).map(|_| ());
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
                            ReadCmd::CodexCalibrationReport(reply) => {
                                let _ = reply.send(registry::codex_turn_calibration_report(&conn));
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
            readers: rtxs,
            rr: AtomicUsize::new(0),
            pricing_shadow_readers: Vec::new(),
            pricing_shadow_rr: AtomicUsize::new(0),
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
        {
            let mut pg = registry::pg::PgStore::connect(&url)?;
            let writer_url = url.clone();
            let writer_owner = owner.clone();
            std::thread::Builder::new().name("billing-pg-writer".into()).spawn(move || {
                while let Some(cmd) = wrx.blocking_recv() {
                    match cmd {
                        WriteCmd::GeminiCreditSpend {
                            profile_id, delta_nano, updated_ts, reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "Gemini spend credit",
                                |pg| pg.credit_gemini_profile_spend(
                                    &profile_id, delta_nano, updated_ts,
                                ),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::GeminiObserveWindow {
                            profile_id, bucket_id, window_kind, window_duration_mins, resets_at,
                            used_fraction_units, observed_at, reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "Gemini window observation",
                                |pg| {
                                    let spend_nano = pg.gemini_profile_spend(&profile_id)?;
                                    let observation = GeminiWindowObservation {
                                        profile_id: profile_id.clone(),
                                        bucket_id: bucket_id.clone(),
                                        window_kind: window_kind.clone(),
                                        window_duration_mins,
                                        resets_at,
                                        observed_at,
                                        used_fraction_units,
                                        gateway_spend_nano: spend_nano,
                                    };
                                    loop {
                                        let existing = pg.load_gemini_calibration(
                                            &profile_id, &bucket_id,
                                        )?;
                                        if let Some(existing) = existing.as_ref().filter(|row| {
                                            row.estimator_version
                                                == crate::gemini::ESTIMATOR_VERSION
                                                && observed_at <= row.observed_at
                                        }) {
                                            return Ok((spend_nano, existing.clone()));
                                        }
                                        let history = if existing.as_ref().is_some_and(|row| {
                                            row.estimator_version
                                                != crate::gemini::ESTIMATOR_VERSION
                                        }) {
                                            pg.load_gemini_window_observations(
                                                &profile_id, &bucket_id,
                                            )?
                                        } else {
                                            Vec::new()
                                        };
                                        let mut state =
                                            crate::gemini::apply_observation_with_history(
                                                existing, &history, &observation,
                                            )?;
                                        if let Some(version) =
                                            pg.save_gemini_calibration(&state, &observation)?
                                        {
                                            state.version = version;
                                            return Ok((spend_nano, state));
                                        }
                                    }
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
                        WriteCmd::Reserve { request_id, account_id, key, hold, handoff, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "reserve",
                                |pg| pg.reserve_request(
                                    &writer_owner, &request_id, &account_id, &key, hold,
                                    RESERVATION_LEASE_SECS,
                                ),
                            );
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
                        WriteCmd::ReserveWithLegacySnapshot { key, snapshot, handoff, reply } => {
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
                        WriteCmd::ReserveWithPolicySnapshot { key, snapshot, handoff, reply } => {
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
                            let result = run_pg_with_retry(
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
                            );
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
                                                    max_inflight, util_cap, reply } => {
                            let result = run_pg_with_retry(
                                &mut pg, &writer_url, &writer_owner, "capacity acquisition",
                                |pg| pg.acquire_capacity(
                                    &writer_owner,&lease_id,&request_id,&email,lease_secs,max_inflight,util_cap,
                                ),
                            );
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
                            let result = loop {
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
                            };
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
                            ReadCmd::CodexCalibrationReport(reply) => {
                                answer!(reply, pg.codex_turn_calibration_report())
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
            readers: rtxs,
            rr: AtomicUsize::new(0),
            pricing_shadow_readers: pricing_shadow_rtxs,
            pricing_shadow_rr: AtomicUsize::new(0),
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
        let (reply, result) = oneshot::channel();
        let handoff = Arc::new(AtomicU8::new(SNAPSHOT_RESERVE_HANDOFF_PENDING));
        let guard = SnapshotReserveHandoffGuard {
            handoff: Arc::clone(&handoff),
        };
        self.writer
            .send(WriteCmd::ReserveWithLegacySnapshot {
                key: key.into(),
                snapshot,
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
        let (reply, result) = oneshot::channel();
        let handoff = Arc::new(AtomicU8::new(SNAPSHOT_RESERVE_HANDOFF_PENDING));
        let guard = SnapshotReserveHandoffGuard {
            handoff: Arc::clone(&handoff),
        };
        self.writer
            .send(WriteCmd::ReserveWithPolicySnapshot {
                key: key.into(),
                snapshot,
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
        max_inflight: i64,
        util_cap: f64,
    ) -> anyhow::Result<Option<registry::pg::CapacityLease>> {
        let (reply, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::AcquireCapacity {
                lease_id: lease_id.into(),
                request_id: request_id.into(),
                email: email.into(),
                lease_secs,
                max_inflight,
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
    /// Дренаж очереди writer'а (барьер): сначала ждёт, пока backpressure-waiters поставят все
    /// detached-команды в очередь, затем ждёт их применения. Вызывать на graceful shutdown ПОСЛЕ
    /// дренажа стримов — тогда их финальные списания не потеряются при выходе процесса.
    pub async fn flush(&self) -> anyhow::Result<()> {
        self.detached.wait_idle().await;
        let (r, rx) = oneshot::channel();
        self.writer
            .send(WriteCmd::Flush(r))
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

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
    async fn gemini_calibration_credits_admin_spend_and_keeps_windows_independent() {
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

        // Spend credit is independent from a customer reservation/usage_event and therefore also
        // covers successful admin traffic.
        first
            .credit_gemini_spend("profile-a", 10_000, 100)
            .await
            .unwrap();
        let (_, anchor) = first
            .observe_gemini_window(
                "profile-a",
                "gemini-5h",
                "5h",
                300,
                2_000_000_000,
                1_000,
                100,
            )
            .await
            .unwrap();
        assert!(anchor.current_capacity_nano.is_none());

        first
            .credit_gemini_spend("profile-a", 20_000, 101)
            .await
            .unwrap();
        let (_, censored) = first
            .observe_gemini_window(
                "profile-a",
                "gemini-5h",
                "5h",
                300,
                2_000_000_000,
                2_000,
                101,
            )
            .await
            .unwrap();
        assert!(censored.anchor_ready);
        assert!(censored.current_capacity_nano.is_none());

        first
            .credit_gemini_spend("profile-a", 20_000, 102)
            .await
            .unwrap();
        let (_, measured) = first
            .observe_gemini_window(
                "profile-a",
                "gemini-5h",
                "5h",
                300,
                2_000_000_000,
                3_000,
                102,
            )
            .await
            .unwrap();
        assert_eq!(measured.current_capacity_nano, Some(2_000_000_000));
        assert_eq!(measured.observed_spend_nano, 20_000);

        let (_, weekly) = first
            .observe_gemini_window(
                "profile-a",
                "gemini-weekly",
                "weekly",
                10_080,
                2_000_500_000,
                500,
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
                "gemini-5h",
                "5h",
                300,
                2_000_000_000,
                3_000,
                103,
            )
            .await
            .unwrap();
        assert_eq!(restored.current_capacity_nano, Some(2_000_000_000));
        assert_eq!(restored.observed_spend_nano, 20_000);
        assert_eq!(restored.samples, 1);
        let _ = std::fs::remove_file(path);
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
