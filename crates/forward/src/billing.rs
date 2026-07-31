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

use registry::pricing::{LegacyScalarAdmissionSnapshot, LegacyScalarReserveOutcome};
use registry::{
    AccountRow, BillingTotals, CodexCalibrationRow, CodexWindowObservation, KeyAuth,
    KeyPolicyUpdate, KeyRow,
};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, Notify};

/// Billing queues are deliberately bounded. The request path applies async backpressure instead of
/// retaining an arbitrary number of commands while PostgreSQL/SQLite is unavailable.
const WRITE_QUEUE_CAPACITY: usize = 4_096;
const READ_QUEUE_CAPACITY: usize = 1_024;
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

enum WriteCmd {
    CodexCreditSpend {
        home_id: String,
        delta_nano: i64,
        updated_ts: i64,
        reply: oneshot::Sender<anyhow::Result<i64>>,
    },
    CodexObserveWindow {
        home_id: String,
        window_duration_mins: i64,
        resets_at: i64,
        used_percent: i64,
        observed_at: i64,
        reply: oneshot::Sender<anyhow::Result<(i64, CodexCalibrationRow)>>,
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
    KeyAuth(String, oneshot::Sender<anyhow::Result<Option<KeyAuth>>>),
    KeyGet(String, oneshot::Sender<anyhow::Result<Option<KeyRow>>>),
    Account(String, oneshot::Sender<anyhow::Result<Option<AccountRow>>>),
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
    SpendByAccount(
        i64,
        i64,
        oneshot::Sender<anyhow::Result<Vec<registry::SpendAccountAgg>>>,
    ),
    SpendByProvider(
        i64,
        oneshot::Sender<anyhow::Result<Vec<registry::SpendProviderAgg>>>,
    ),
}

/// Async-фасад биллинга: writer-канал + пул reader-каналов. Клонируется (в `Arc`) во все хендлеры.
pub struct AsyncBilling {
    writer: mpsc::Sender<WriteCmd>,
    detached: Arc<DetachedDispatchTracker>,
    readers: Vec<mpsc::Sender<ReadCmd>>,
    rr: AtomicUsize, // round-robin по читателям
}

impl AsyncBilling {
    pub(crate) fn track_detached_work(&self) -> DetachedDispatchGuard {
        self.detached.begin()
    }

    /// Persist exact official-price spend for one Codex home and return its durable cumulative
    /// total. This is provider-capacity evidence, independent of whether the customer turn was
    /// billable (admin turns consume the same subscription window).
    pub async fn credit_codex_spend(
        &self,
        home_id: &str,
        delta_nano: i64,
        updated_ts: i64,
    ) -> anyhow::Result<i64> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::CodexCreditSpend {
                home_id: home_id.into(),
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

    /// Pair a provider snapshot with durable cumulative spend and CAS-persist the pure estimator.
    /// Duration/reset are required so an unknown window is never guessed as a weekly window.
    pub async fn observe_codex_window(
        &self,
        home_id: &str,
        window_duration_mins: i64,
        resets_at: i64,
        used_percent: i64,
        observed_at: i64,
    ) -> anyhow::Result<(i64, CodexCalibrationRow)> {
        let (reply, result) = oneshot::channel();
        self.writer
            .send(WriteCmd::CodexObserveWindow {
                home_id: home_id.into(),
                window_duration_mins,
                resets_at,
                used_percent,
                observed_at,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("billing writer unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("billing writer stopped"))?
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
                    match registry::sqlite_settle_request(
                        &conn, request_id, account_id, key, hold, 0,
                        Some("reserve-handoff-canceled"), None,
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
                    WriteCmd::CodexCreditSpend { home_id, delta_nano, updated_ts, reply } => {
                        let _ = reply.send(registry::credit_codex_home_spend(
                            &conn, &home_id, delta_nano, updated_ts,
                        ));
                    }
                    WriteCmd::CodexObserveWindow {
                        home_id, window_duration_mins, resets_at, used_percent, observed_at, reply,
                    } => {
                        let result = (|| {
                            let spend_nano = registry::codex_home_spend(&conn, &home_id)?;
                            let observation = CodexWindowObservation {
                                home_id: home_id.clone(),
                                window_duration_mins,
                                resets_at,
                                observed_at,
                                used_percent,
                                gateway_spend_nano: spend_nano,
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
                                    return Ok((spend_nano, existing.clone()));
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
                                );
                                if let Some(version) = registry::save_codex_calibration(
                                    &conn, &state, &observation,
                                )? {
                                    state.version = version;
                                    return Ok((spend_nano, state));
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
                    WriteCmd::CancelReserve { request_id, account_id, key, hold, handoff } => {
                        refund_canceled_reserve(&request_id, &account_id, &key, hold, &handoff);
                    }
                    WriteCmd::Settle {
                        request_id, account_id, key, hold, actual, reference, usage, reply,
                    } => {
                        let usage_ref = if actual > 0 { usage.as_ref() } else { None };
                        let result = registry::sqlite_settle_request(
                            &conn, &request_id, &account_id, &key, hold, actual,
                            reference.as_deref(), usage_ref,
                        );
                        if let Err(error) = &result {
                            eprintln!("billing SQLite settlement persisted/retryable failure: {error:#}");
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
                            ReadCmd::KeyAuth(k, r) => {
                                let _ = r.send(registry::key_account(&conn, &k));
                            }
                            ReadCmd::KeyGet(k, r) => {
                                let _ = r.send(registry::key_get(&conn, &k));
                            }
                            ReadCmd::Account(id, r) => {
                                let _ = r.send(registry::account_get(&conn, &id));
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
                            ReadCmd::SpendByAccount(since, lim, r) => {
                                let _ = r.send(registry::spend_by_account(&conn, since, lim));
                            }
                            ReadCmd::SpendByProvider(since, r) => {
                                let _ = r.send(registry::spend_by_provider(&conn, since));
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
        {
            let mut pg = registry::pg::PgStore::connect(&url)?;
            let writer_url = url.clone();
            let writer_owner = owner.clone();
            std::thread::Builder::new().name("billing-pg-writer".into()).spawn(move || {
                while let Some(cmd) = wrx.blocking_recv() {
                    match cmd {
                        WriteCmd::CodexCreditSpend {
                            home_id, delta_nano, updated_ts, reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "Codex spend credit",
                                |pg| pg.credit_codex_home_spend(
                                    &home_id, delta_nano, updated_ts,
                                ),
                            );
                            let _ = reply.send(result);
                        }
                        WriteCmd::CodexObserveWindow {
                            home_id, window_duration_mins, resets_at, used_percent, observed_at,
                            reply,
                        } => {
                            let result = run_pg_with_retry(
                                &mut pg,
                                &writer_url,
                                &writer_owner,
                                "Codex window observation",
                                |pg| {
                                    let spend_nano = pg.codex_home_spend(&home_id)?;
                                    let observation = CodexWindowObservation {
                                        home_id: home_id.clone(),
                                        window_duration_mins,
                                        resets_at,
                                        observed_at,
                                        used_percent,
                                        gateway_spend_nano: spend_nano,
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
                                            return Ok((spend_nano, existing.clone()));
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
                                            );
                                        if let Some(version) =
                                            pg.save_codex_calibration(&state, &observation)?
                                        {
                                            state.version = version;
                                            return Ok((spend_nano, state));
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
                                |pg| pg.settle_request(
                                    &request_id, actual, reference.as_deref(), usage.as_ref(),
                                ),
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
                            ReadCmd::KeyAuth(k, r) => answer!(r, pg.key_account(&k)),
                            ReadCmd::KeyGet(k, r) => answer!(r, pg.key_get(&k)),
                            ReadCmd::Account(id, r) => answer!(r, pg.account_get(&id)),
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
                            ReadCmd::SpendByAccount(since, lim, r) => {
                                answer!(r, pg.spend_by_account(since, lim))
                            }
                            ReadCmd::SpendByProvider(since, r) => {
                                answer!(r, pg.spend_by_provider(since))
                            }
                        }
                    }
                })?;
            rtxs.push(rtx);
        }
        Ok(AsyncBilling {
            writer: wtx,
            detached: Arc::new(DetachedDispatchTracker::default()),
            readers: rtxs,
            rr: AtomicUsize::new(0),
        })
    }

    fn reader(&self) -> &mpsc::Sender<ReadCmd> {
        let i = self.rr.fetch_add(1, Ordering::Relaxed) % self.readers.len();
        &self.readers[i]
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
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::SpendByAccount(since_ts, limit, r))
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
        let (r, rx) = oneshot::channel();
        self.reader()
            .send(ReadCmd::SpendByProvider(since_ts, r))
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
        assert_eq!(
            first
                .credit_codex_spend("home-a", 100_000_000_000, 100)
                .await
                .unwrap(),
            100_000_000_000
        );
        let (_, anchor) = first
            .observe_codex_window("home-a", 300, 2_000_000_000, 10, 100)
            .await
            .unwrap();
        assert!(anchor.current_capacity_nano.is_none());
        first
            .credit_codex_spend("home-a", 40_000_000_000, 101)
            .await
            .unwrap();
        let (_, measured) = first
            .observe_codex_window("home-a", 300, 2_000_000_000, 12, 101)
            .await
            .unwrap();
        assert!(measured.current_capacity_nano.is_none());
        assert!(measured.anchor_ready);
        first
            .credit_codex_spend("home-a", 40_000_000_000, 102)
            .await
            .unwrap();
        let (_, measured) = first
            .observe_codex_window("home-a", 300, 2_000_000_000, 14, 102)
            .await
            .unwrap();
        assert_eq!(measured.current_capacity_nano, Some(2_000_000_000_000));
        let (_, duplicate) = first
            .observe_codex_window("home-a", 300, 2_000_000_000, 14, 102)
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
            .observe_codex_window("home-a", 300, 2_000_000_000, 14, 103)
            .await
            .unwrap();
        assert_eq!(spend, 180_000_000_000);
        assert_eq!(restored.estimator_version, crate::codex::ESTIMATOR_VERSION);
        assert_eq!(restored.current_capacity_nano, Some(2_000_000_000_000));
        assert!(restored.version > measured.version);
        restarted.flush().await.unwrap();
        drop(restarted);
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
