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
//! рантайма). Гарантия денег: осиротевшее при краше вернёт `reconcile` на старте. Ничего не застревает.

use registry::{AccountRow, BillingTotals, KeyAuth, KeyPolicyUpdate, KeyRow};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

const RESERVE_HANDOFF_PENDING: u8 = 0;
const RESERVE_HANDOFF_COMMITTED: u8 = 1;
const RESERVE_HANDOFF_CLAIMED: u8 = 2;
const RESERVE_HANDOFF_CANCELED: u8 = 3;
const RESERVE_HANDOFF_REFUNDING: u8 = 4;
const RESERVE_HANDOFF_REFUNDED: u8 = 5;
const RESERVE_HANDOFF_FAILED: u8 = 6;

// Закрывает окно отмены, пока `reserve().await` ещё не передал владение резервом вызывающему коду.
// AUDIT-TODO(C54): заменить account-level компенсацию на durable reservation ID + idempotent cancel/settle.
struct ReserveHandoffGuard<'a> {
    writer: &'a mpsc::UnboundedSender<WriteCmd>,
    request_id: String,
    account_id: String,
    key: String,
    hold: i64,
    handoff: Arc<AtomicU8>,
}

impl ReserveHandoffGuard<'_> {
    fn claim(&self) -> bool {
        self.handoff.compare_exchange(
            RESERVE_HANDOFF_COMMITTED,
            RESERVE_HANDOFF_CLAIMED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ).is_ok()
    }
}

impl Drop for ReserveHandoffGuard<'_> {
    fn drop(&mut self) {
        loop {
            match self.handoff.load(Ordering::Acquire) {
                RESERVE_HANDOFF_PENDING => {
                    if self.handoff.compare_exchange(
                        RESERVE_HANDOFF_PENDING,
                        RESERVE_HANDOFF_CANCELED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ).is_ok() {
                        return;
                    }
                }
                RESERVE_HANDOFF_COMMITTED => {
                    if self.handoff.compare_exchange(
                        RESERVE_HANDOFF_COMMITTED,
                        RESERVE_HANDOFF_CANCELED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ).is_ok() {
                        let _ = self.writer.send(WriteCmd::CancelReserve {
                            request_id: self.request_id.clone(),
                            account_id: self.account_id.clone(),
                            key: self.key.clone(),
                            hold: self.hold,
                            handoff: Arc::clone(&self.handoff),
                        });
                        return;
                    }
                }
                RESERVE_HANDOFF_CLAIMED | RESERVE_HANDOFF_CANCELED
                | RESERVE_HANDOFF_REFUNDING | RESERVE_HANDOFF_REFUNDED
                | RESERVE_HANDOFF_FAILED => return,
                _ => return,
            }
        }
    }
}

enum WriteCmd {
    Reserve {
        request_id: String,
        account_id: String,
        key: String,
        hold: i64,
        handoff: Arc<AtomicU8>,
        reply: oneshot::Sender<Option<i64>>,
    },
    CancelReserve { request_id: String, account_id: String, key: String, hold: i64, handoff: Arc<AtomicU8> },
    Settle {
        request_id: String, account_id: String, key: String, hold: i64, actual: i64, reference: Option<String>,
        usage: Option<registry::UsageEventInput>, // разбивка токенов/модели (аналитика), если есть
        reply: Option<oneshot::Sender<Option<i64>>>, // None → fire-and-forget (RAII из Drop)
    },
    Topup { account_id: String, amount: i64, reference: Option<String>, reply: oneshot::Sender<Option<i64>> },
    /// Control-плоскость (редкие управляющие записи из `/admin/*`) — через ТОТ ЖЕ writer, чтобы
    /// сохранить дисциплину единственного писателя (никаких гонок/BUSY с reserve/settle).
    CreateAccount { id: String, handle: Option<String>, mult_bp: i64, reply: oneshot::Sender<bool> },
    IssueKey {
        key: String, account_id: String, label: Option<String>, spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>, reply: oneshot::Sender<bool>,
    },
    AccountStatus { id: String, status: String, reply: oneshot::Sender<usize> },
    AccountMultiplier { id: String, mult_bp: i64, reply: oneshot::Sender<usize> },
    KeyStatus { key: String, status: String, reply: oneshot::Sender<usize> },
    KeyStatusById { key_id: String, status: String, reply: oneshot::Sender<usize> },
    KeyLabelById { key_id: String, label: String, reply: oneshot::Sender<usize> },
    KeyPolicyById {
        account_id: String, key_id: String, spend_limit_nano: Option<i64>, expires_ts: Option<i64>,
        reply: oneshot::Sender<Option<KeyPolicyUpdate>>,
    },
    MarkDelivering { request_id: String, lease_secs: i64, reply: oneshot::Sender<bool> },
    AcquireCapacity {
        lease_id: String, request_id: String, email: String, lease_secs: i64,
        max_inflight: i64, util_cap: f64,
        reply: oneshot::Sender<Option<registry::pg::CapacityLease>>,
    },
    ReleaseCapacity { lease_id: String },
    /// Барьер: writer FIFO → когда Flush обработан, ВСЕ прежние команды (settle) применены.
    /// Для дренажа очереди на graceful shutdown (иначе последние списания потерялись бы).
    Flush(oneshot::Sender<()>),
}

enum ReadCmd {
    KeyAuth(String, oneshot::Sender<Option<KeyAuth>>),
    KeyGet(String, oneshot::Sender<Option<KeyRow>>),
    Account(String, oneshot::Sender<Option<AccountRow>>),
    AccountByHandle(String, oneshot::Sender<Option<AccountRow>>),
    Totals(oneshot::Sender<BillingTotals>),
    AccountsList(oneshot::Sender<Vec<AccountRow>>),
    KeysByAccount(String, oneshot::Sender<Vec<KeyRow>>),
    Ledger(String, i64, oneshot::Sender<Vec<registry::LedgerRow>>),
    LedgerAfter(String, i64, i64, oneshot::Sender<Vec<registry::LedgerRow>>),
    UsageByModel(String, i64, oneshot::Sender<Vec<registry::UsageModelAgg>>),
}

/// Async-фасад биллинга: writer-канал + пул reader-каналов. Клонируется (в `Arc`) во все хендлеры.
pub struct AsyncBilling {
    writer: mpsc::UnboundedSender<WriteCmd>,
    readers: Vec<mpsc::UnboundedSender<ReadCmd>>,
    rr: AtomicUsize, // round-robin по читателям
    // TTL-кэш key_auth (key→account). key→account статичен; кэш срезает read/запрос под нагрузкой.
    // МОНЕЙ-БЕЗОПАСНО: авторитет баланса — атомарный reserve (перечитывает в БД), кэш лишь оценка cap.
    // Полностью чистится при ЛЮБОЙ смене статуса ключа/аккаунта → отозванный ключ живёт ≤ TTL. ttl=0 → выкл.
    auth_cache: StdMutex<HashMap<String, (KeyAuth, Instant)>>,
    auth_ttl: Duration,
}

impl AsyncBilling {
    pub fn start_authority(
        config: registry::authority::AuthorityConfig,
        owner: Option<registry::pg::Owner>,
        readers: usize,
        auth_ttl_ms: u64,
    ) -> anyhow::Result<Self> {
        match config {
            registry::authority::AuthorityConfig::Sqlite { path } =>
                Self::start_with(path, readers, auth_ttl_ms),
            registry::authority::AuthorityConfig::Postgres { url } => {
                let owner = owner.ok_or_else(|| anyhow::anyhow!("PostgreSQL billing requires owner epoch"))?;
                Self::start_postgres(url, owner, readers, auth_ttl_ms)
            }
        }
    }

    /// Поднять writer-поток + `readers` reader-потоков. `open` (миграции + PRAGMA WAL) — на этих
    /// потоках; синхронный SQLite не касается async-рантайма никогда.
    pub fn start(db_path: String, readers: usize) -> anyhow::Result<Self> {
        Self::start_with(db_path, readers, 0)
    }

    /// `auth_ttl_ms` — TTL кэша key_auth в мс (0 = кэш выключен).
    pub fn start_with(db_path: String, readers: usize, auth_ttl_ms: u64) -> anyhow::Result<Self> {
        let readers = readers.max(1);
        // writer
        let (wtx, mut wrx) = mpsc::unbounded_channel::<WriteCmd>();
        {
            let conn = registry::open(&db_path)?;
            std::thread::Builder::new().name("billing-writer".into()).spawn(move || {
                // Батч group-commit ограничен сверху: bounded окно потери на краше (незакоммиченная
                // пачка) + bounded латентность/память. Под низкой нагрузкой пачка=1 (канал пуст).
                const MAX_WRITE_BATCH: usize = 256;
                let is_hot = |c: &WriteCmd| matches!(c, WriteCmd::Reserve { .. } | WriteCmd::Settle { .. });
                let refund_canceled_reserve = |_request_id: &str, account_id: &str, key: &str,
                                               hold: i64, handoff: &AtomicU8| {
                    if handoff.compare_exchange(
                        RESERVE_HANDOFF_CANCELED,
                        RESERVE_HANDOFF_REFUNDING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ).is_err() {
                        return;
                    }
                    match registry::account_settle(&conn, account_id, key, hold, 0, None, None) {
                        Ok(Some(_)) => handoff.store(RESERVE_HANDOFF_REFUNDED, Ordering::Release),
                        Ok(None) => {
                            eprintln!("billing reserve cancellation refund failed: account not found");
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
                                      reply: oneshot::Sender<Option<i64>>, res: Option<i64>| {
                    if res.is_none() {
                        handoff.store(RESERVE_HANDOFF_FAILED, Ordering::Release);
                        let _ = reply.send(None);
                        return;
                    }
                    match handoff.compare_exchange(
                        RESERVE_HANDOFF_PENDING,
                        RESERVE_HANDOFF_COMMITTED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => {
                            if reply.send(res).is_err() {
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
                // AUDIT-TODO(C56): persist idempotent pending settlements and retry them until commit.
                let settle_one = |account_id: &str, key: &str, hold: i64, actual: i64,
                                  reference: Option<&str>, usage: Option<&registry::UsageEventInput>| {
                    match registry::account_settle(&conn, account_id, key, hold, actual, reference, usage) {
                        Ok(Some(balance)) => Some(balance),
                        Ok(None) => {
                            eprintln!(
                                "billing settlement failed: account missing (hold={hold}, actual={actual})"
                            );
                            None
                        }
                        Err(err) => {
                            eprintln!(
                                "billing settlement database failure (hold={hold}, actual={actual}): {err:#}"
                            );
                            None
                        }
                    }
                };
                // НЕ-горячие (топап/контрол/Flush) — по-одному (редкие; у топапа идемпотентный rollback
                // несовместим с общей транзакцией). Прежнее поведение.
                let apply_nonhot = |cmd: WriteCmd| match cmd {
                    WriteCmd::CancelReserve { request_id, account_id, key, hold, handoff } => {
                        refund_canceled_reserve(&request_id, &account_id, &key, hold, &handoff);
                    }
                    WriteCmd::Topup { account_id, amount, reference, reply } => {
                        let _ = reply.send(registry::account_topup(&conn, &account_id, amount, reference.as_deref()).ok().flatten());
                    }
                    WriteCmd::CreateAccount { id, handle, mult_bp, reply } => { let _ = reply.send(registry::account_create(&conn, &id, handle.as_deref(), mult_bp).is_ok()); }
                    WriteCmd::IssueKey { key, account_id, label, spend_limit_nano, expires_ts, reply } => {
                        let _ = reply.send(registry::key_issue_with_policy(
                            &conn,&key,&account_id,label.as_deref(),spend_limit_nano,expires_ts,
                        ).is_ok());
                    }
                    WriteCmd::AccountStatus { id, status, reply } => { let _ = reply.send(registry::account_set_status(&conn, &id, &status).unwrap_or(0)); }
                    WriteCmd::AccountMultiplier { id, mult_bp, reply } => { let _ = reply.send(registry::account_set_mult_bp(&conn, &id, mult_bp).unwrap_or(0)); }
                    WriteCmd::KeyStatus { key, status, reply } => { let _ = reply.send(registry::key_set_status(&conn, &key, &status).unwrap_or(0)); }
                    WriteCmd::KeyStatusById { key_id, status, reply } => { let _ = reply.send(registry::key_set_status_by_id(&conn, &key_id, &status).unwrap_or(0)); }
                    WriteCmd::KeyLabelById { key_id, label, reply } => { let _ = reply.send(registry::key_set_label_by_id(&conn, &key_id, &label).unwrap_or(0)); }
                    WriteCmd::KeyPolicyById { account_id, key_id, spend_limit_nano, expires_ts, reply } => {
                        let _ = reply.send(registry::key_set_policy_by_id(
                            &conn,&account_id,&key_id,spend_limit_nano,expires_ts,
                        ).ok());
                    }
                    WriteCmd::MarkDelivering { reply, .. } => { let _ = reply.send(true); }
                    WriteCmd::AcquireCapacity { lease_id, request_id, email, lease_secs, reply, .. } => {
                        let _ = reply.send(Some(registry::pg::CapacityLease {
                            lease_id, request_id, subscription_email: email,
                            lease_until: pool::now().saturating_add(lease_secs.max(1)),
                        }));
                    }
                    WriteCmd::ReleaseCapacity { .. } => {}
                    WriteCmd::Flush(r) => { let _ = r.send(()); }
                    WriteCmd::Reserve { .. } | WriteCmd::Settle { .. } => {}
                };
                // Одна горячая команда в СВОЕЙ транзакции (fallback батча) + reply/refund как раньше.
                let apply_hot_single = |cmd: WriteCmd| match cmd {
                    WriteCmd::Reserve { request_id, account_id, key, hold, handoff, reply } => {
                        let res = registry::account_reserve_for_key(&conn, &account_id, &key, hold).ok().flatten();
                        finish_reserve(request_id, account_id, key, hold, handoff, reply, res);
                    }
                    WriteCmd::Settle { account_id, key, hold, actual, reference, usage, reply, .. } => {
                        let usage_ref = if actual > 0 { usage.as_ref() } else { None };
                        let res = settle_one(
                            &account_id, &key, hold, actual, reference.as_deref(), usage_ref,
                        );
                        if let Some(r) = reply { let _ = r.send(res); }
                    }
                    _ => {}
                };
                while let Some(first) = wrx.blocking_recv() {
                    if !is_hot(&first) { apply_nonhot(first); continue; }
                    // собираем contiguous-run горячих команд (FIFO не нарушаем: не-hot команду,
                    // вытянутую при дренаже, откладываем в trailer и применяем сразу после пачки).
                    let mut batch = vec![first];
                    let mut trailer: Option<WriteCmd> = None;
                    while batch.len() < MAX_WRITE_BATCH {
                        match wrx.try_recv() {
                            Ok(c) if is_hot(&c) => batch.push(c),
                            Ok(c) => { trailer = Some(c); break; }
                            Err(_) => break,
                        }
                    }
                    let ops: Vec<registry::HotOp> = batch.iter().map(|c| match c {
                        WriteCmd::Reserve { account_id, key, hold, .. } =>
                            registry::HotOp::Reserve { account_id, key, hold: *hold },
                        WriteCmd::Settle { account_id, key, hold, actual, reference, usage, .. } =>
                            registry::HotOp::Settle { account_id, key, hold: *hold, actual: *actual,
                                reference: reference.as_deref(), usage: if *actual > 0 { usage.as_ref() } else { None } },
                        _ => unreachable!(),
                    }).collect();
                    match registry::apply_hot_batch(&conn, &ops) {
                        Ok(results) => {
                            drop(ops); // снять borrow с batch перед consume
                            for (cmd, res) in batch.into_iter().zip(results) {
                                match cmd {
                                    WriteCmd::Reserve { request_id, account_id, key, hold, handoff, reply } => {
                                        finish_reserve(request_id, account_id, key, hold, handoff, reply, res);
                                    }
                                    WriteCmd::Settle { hold, actual, reply, .. } => {
                                        // `registry::apply_hot_batch` currently folds per-op DB errors into
                                        // `None`; surface that loss instead of silently treating it as success.
                                        if res.is_none() {
                                            eprintln!(
                                                "billing settlement failed in group commit (hold={hold}, actual={actual})"
                                            );
                                        }
                                        if let Some(r) = reply { let _ = r.send(res); }
                                    }
                                    _ => unreachable!(),
                                }
                            }
                        }
                        // Ошибка BEGIN/COMMIT (редко) → пачка НЕ применена → по-одному (безопасный fallback).
                        Err(_) => { drop(ops); for cmd in batch { apply_hot_single(cmd); } }
                    }
                    if let Some(c) = trailer { apply_nonhot(c); }
                }
                eprintln!("⚠ billing-writer поток завершён (все sender'ы дропнуты)"); // супервизия
            })?;
        }
        // reader-пул
        let mut rtxs = Vec::with_capacity(readers);
        for i in 0..readers {
            let (rtx, mut rrx) = mpsc::unbounded_channel::<ReadCmd>();
            let conn = registry::open(&db_path)?; // своё read-соединение (WAL параллелит чтения)
            std::thread::Builder::new().name(format!("billing-reader-{i}")).spawn(move || {
                while let Some(cmd) = rrx.blocking_recv() {
                    match cmd {
                        ReadCmd::KeyAuth(k, r) => { let _ = r.send(registry::key_account(&conn, &k).ok().flatten()); }
                        ReadCmd::KeyGet(k, r) => { let _ = r.send(registry::key_get(&conn, &k).ok().flatten()); }
                        ReadCmd::Account(id, r) => { let _ = r.send(registry::account_get(&conn, &id).ok().flatten()); }
                        ReadCmd::AccountByHandle(handle, r) => {
                            let _ = r.send(registry::account_by_handle(&conn, &handle).ok().flatten());
                        }
                        ReadCmd::Totals(r) => { let _ = r.send(registry::billing_totals(&conn)); }
                        ReadCmd::AccountsList(r) => { let _ = r.send(registry::account_list(&conn).unwrap_or_default()); }
                        ReadCmd::KeysByAccount(id, r) => { let _ = r.send(registry::keys_by_account(&conn, &id).unwrap_or_default()); }
                        ReadCmd::Ledger(id, lim, r) => { let _ = r.send(registry::ledger_recent(&conn, &id, lim).unwrap_or_default()); }
                        ReadCmd::LedgerAfter(id, after, lim, r) => {
                            let _ = r.send(registry::ledger_after(&conn, &id, after, lim).unwrap_or_default());
                        }
                        ReadCmd::UsageByModel(id, since, r) => {
                            let _ = r.send(registry::usage_by_model(&conn, &id, since).unwrap_or_default());
                        }
                    }
                }
                eprintln!("⚠ billing-reader-{i} поток завершён");
            })?;
            rtxs.push(rtx);
        }
        Ok(AsyncBilling {
            writer: wtx, readers: rtxs, rr: AtomicUsize::new(0),
            auth_cache: StdMutex::new(HashMap::new()),
            auth_ttl: Duration::from_millis(auth_ttl_ms),
        })
    }

    fn start_postgres(
        url: String,
        owner: registry::pg::Owner,
        readers: usize,
        auth_ttl_ms: u64,
    ) -> anyhow::Result<Self> {
        const RESERVATION_LEASE_SECS: i64 = 3600;
        let readers = readers.max(1);
        let (wtx, mut wrx) = mpsc::unbounded_channel::<WriteCmd>();
        {
            let mut pg = registry::pg::PgStore::connect(&url)?;
            let writer_url = url.clone();
            let writer_owner = owner.clone();
            std::thread::Builder::new().name("billing-pg-writer".into()).spawn(move || {
                let reconnect = |url: &str, owner: &registry::pg::Owner| -> registry::pg::PgStore {
                    loop {
                        match registry::pg::PgStore::connect(url) {
                            Ok(mut next) => match next.heartbeat_instance(owner, 30) {
                                Ok(true) => return next,
                                Ok(false) => {
                                    eprintln!("billing PostgreSQL owner was fenced; refusing stale writes");
                                }
                                Err(err) => eprintln!("billing PostgreSQL heartbeat after reconnect failed: {err:#}"),
                            },
                            Err(err) => eprintln!("billing PostgreSQL reconnect failed: {err:#}"),
                        }
                        std::thread::sleep(Duration::from_millis(500));
                    }
                };
                while let Some(cmd) = wrx.blocking_recv() {
                    match cmd {
                        WriteCmd::Reserve { request_id, account_id, key, hold, handoff, reply } => {
                            let result = loop {
                                match pg.reserve_request(
                                    &writer_owner, &request_id, &account_id, &key, hold,
                                    RESERVATION_LEASE_SECS,
                                ) {
                                    Ok(result) => break result,
                                    Err(err) => {
                                        eprintln!("billing PostgreSQL reserve failed, retrying: {err:#}");
                                        pg = reconnect(&writer_url, &writer_owner);
                                    }
                                }
                            };
                            if result.is_none() {
                                handoff.store(RESERVE_HANDOFF_FAILED, Ordering::Release);
                                let _ = reply.send(None);
                                continue;
                            }
                            match handoff.compare_exchange(
                                RESERVE_HANDOFF_PENDING, RESERVE_HANDOFF_COMMITTED,
                                Ordering::AcqRel, Ordering::Acquire,
                            ) {
                                Ok(_) => {
                                    if reply.send(result).is_err() {
                                        let _ = handoff.compare_exchange(
                                            RESERVE_HANDOFF_COMMITTED, RESERVE_HANDOFF_CANCELED,
                                            Ordering::AcqRel, Ordering::Acquire,
                                        );
                                        loop {
                                            match pg.cancel_request(&request_id) {
                                                Ok(_) => { handoff.store(RESERVE_HANDOFF_REFUNDED, Ordering::Release); break; }
                                                Err(err) => {
                                                    eprintln!("billing PostgreSQL canceled reserve retry: {err:#}");
                                                    pg = reconnect(&writer_url, &writer_owner);
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(RESERVE_HANDOFF_CANCELED) => {
                                    loop {
                                        match pg.cancel_request(&request_id) {
                                            Ok(_) => { handoff.store(RESERVE_HANDOFF_REFUNDED, Ordering::Release); break; }
                                            Err(err) => {
                                                eprintln!("billing PostgreSQL reserve handoff cancel retry: {err:#}");
                                                pg = reconnect(&writer_url, &writer_owner);
                                            }
                                        }
                                    }
                                }
                                Err(state) => eprintln!("billing PostgreSQL reserve handoff unexpected state {state}"),
                            }
                        }
                        WriteCmd::CancelReserve { request_id, handoff, .. } => {
                            if handoff.compare_exchange(
                                RESERVE_HANDOFF_CANCELED, RESERVE_HANDOFF_REFUNDING,
                                Ordering::AcqRel, Ordering::Acquire,
                            ).is_err() { continue; }
                            loop {
                                match pg.cancel_request(&request_id) {
                                    Ok(_) => { handoff.store(RESERVE_HANDOFF_REFUNDED, Ordering::Release); break; }
                                    Err(err) => {
                                        eprintln!("billing PostgreSQL cancellation retry: {err:#}");
                                        handoff.store(RESERVE_HANDOFF_CANCELED, Ordering::Release);
                                        pg = reconnect(&writer_url, &writer_owner);
                                        let _ = handoff.compare_exchange(
                                            RESERVE_HANDOFF_CANCELED, RESERVE_HANDOFF_REFUNDING,
                                            Ordering::AcqRel, Ordering::Acquire,
                                        );
                                    }
                                }
                            }
                        }
                        WriteCmd::Settle { request_id, actual, reference, usage, reply, .. } => {
                            let result = loop {
                                match pg.settle_request(&request_id, actual, reference.as_deref(), usage.as_ref()) {
                                    Ok(result) => break result,
                                    Err(err) => {
                                        eprintln!("billing PostgreSQL settlement retry: {err:#}");
                                        pg = reconnect(&writer_url, &writer_owner);
                                    }
                                }
                            };
                            if let Some(reply) = reply { let _ = reply.send(result); }
                        }
                        WriteCmd::MarkDelivering { request_id, lease_secs, reply } => {
                            let result = loop {
                                match pg.mark_delivering(&writer_owner, &request_id, lease_secs) {
                                    Ok(ok) => break ok,
                                    Err(err) => {
                                        eprintln!("billing PostgreSQL delivery marker retry: {err:#}");
                                        pg = reconnect(&writer_url, &writer_owner);
                                    }
                                }
                            };
                            let _ = reply.send(result);
                        }
                        WriteCmd::AcquireCapacity { lease_id, request_id, email, lease_secs,
                                                    max_inflight, util_cap, reply } => {
                            let result = match pg.acquire_capacity(
                                &writer_owner,&lease_id,&request_id,&email,lease_secs,max_inflight,util_cap,
                            ) {
                                Ok(result) => result,
                                Err(err) => {
                                    eprintln!("capacity lease acquisition failed closed: {err:#}");
                                    pg = reconnect(&writer_url, &writer_owner);
                                    None
                                }
                            };
                            let _ = reply.send(result);
                        }
                        WriteCmd::ReleaseCapacity { lease_id } => {
                            loop {
                                match pg.release_capacity(&writer_owner, &lease_id) {
                                    Ok(_) => break,
                                    Err(err) => {
                                        eprintln!("capacity lease release retry: {err:#}");
                                        pg = reconnect(&writer_url, &writer_owner);
                                    }
                                }
                            }
                        }
                        WriteCmd::Topup { account_id, amount, reference, reply } => {
                            let _ = reply.send(pg.account_topup(&account_id, amount, reference.as_deref()).ok().flatten());
                        }
                        WriteCmd::CreateAccount { id, handle, mult_bp, reply } => {
                            let _ = reply.send(pg.account_create(&id,handle.as_deref(),mult_bp).is_ok());
                        }
                        WriteCmd::IssueKey { key, account_id, label, spend_limit_nano, expires_ts, reply } => {
                            let _ = reply.send(pg.key_issue_with_policy(
                                &key,&account_id,label.as_deref(),spend_limit_nano,expires_ts,
                            ).is_ok());
                        }
                        WriteCmd::AccountStatus { id, status, reply } => {
                            let _ = reply.send(pg.account_set_status(&id,&status).unwrap_or(0));
                        }
                        WriteCmd::AccountMultiplier { id, mult_bp, reply } => {
                            let _ = reply.send(pg.account_set_mult_bp(&id,mult_bp).unwrap_or(0));
                        }
                        WriteCmd::KeyStatus { key, status, reply } => {
                            let _ = reply.send(pg.key_set_status(&key,&status).unwrap_or(0));
                        }
                        WriteCmd::KeyStatusById { key_id, status, reply } => {
                            let _ = reply.send(pg.key_set_status_by_id(&key_id,&status).unwrap_or(0));
                        }
                        WriteCmd::KeyLabelById { key_id, label, reply } => {
                            let _ = reply.send(pg.key_set_label_by_id(&key_id,&label).unwrap_or(0));
                        }
                        WriteCmd::KeyPolicyById { account_id, key_id, spend_limit_nano, expires_ts, reply } => {
                            let _ = reply.send(pg.key_set_policy_by_id(
                                &account_id,&key_id,spend_limit_nano,expires_ts,
                            ).ok());
                        }
                        WriteCmd::Flush(reply) => {
                            loop {
                                match pg.drain_outbox(10_000) {
                                    Ok(0) => break,
                                    Ok(_) => continue,
                                    Err(err) => {
                                        eprintln!("billing PostgreSQL outbox drain retry: {err:#}");
                                        pg = reconnect(&writer_url, &writer_owner);
                                    }
                                }
                            }
                            let _ = reply.send(());
                        }
                    }
                }
                eprintln!("billing-pg-writer thread stopped");
            })?;
        }

        let mut rtxs = Vec::with_capacity(readers);
        for i in 0..readers {
            let (rtx, mut rrx) = mpsc::unbounded_channel::<ReadCmd>();
            let mut pg = registry::pg::PgStore::connect(&url)?;
            let reader_url = url.clone();
            std::thread::Builder::new().name(format!("billing-pg-reader-{i}")).spawn(move || {
                while let Some(cmd) = rrx.blocking_recv() {
                    macro_rules! answer {
                        ($reply:expr, $call:expr, $fallback:expr) => {{
                            match $call {
                                Ok(value) => { let _ = $reply.send(value); }
                                Err(err) => {
                                    eprintln!("billing PostgreSQL read failed closed: {err:#}");
                                    if let Ok(next) = registry::pg::PgStore::connect(&reader_url) { pg = next; }
                                    let _ = $reply.send($fallback);
                                }
                            }
                        }};
                    }
                    match cmd {
                        ReadCmd::KeyAuth(k,r) => answer!(r,pg.key_account(&k),None),
                        ReadCmd::KeyGet(k,r) => answer!(r,pg.key_get(&k),None),
                        ReadCmd::Account(id,r) => answer!(r,pg.account_get(&id),None),
                        ReadCmd::AccountByHandle(handle,r) => answer!(r,pg.account_by_handle(&handle),None),
                        ReadCmd::Totals(r) => answer!(r,pg.billing_totals(),BillingTotals::default()),
                        ReadCmd::AccountsList(r) => answer!(r,pg.account_list(),Vec::new()),
                        ReadCmd::KeysByAccount(id,r) => answer!(r,pg.keys_by_account(&id),Vec::new()),
                        ReadCmd::Ledger(id,lim,r) => answer!(r,pg.ledger_recent(&id,lim),Vec::new()),
                        ReadCmd::LedgerAfter(id,after,lim,r) => answer!(r,pg.ledger_after(&id,after,lim),Vec::new()),
                        ReadCmd::UsageByModel(id,since,r) => answer!(r,pg.usage_by_model(&id,since),Vec::new()),
                    }
                }
            })?;
            rtxs.push(rtx);
        }
        Ok(AsyncBilling {
            writer: wtx, readers: rtxs, rr: AtomicUsize::new(0),
            auth_cache: StdMutex::new(HashMap::new()), auth_ttl: Duration::from_millis(auth_ttl_ms),
        })
    }

    fn auth_cache_clear(&self) {
        if !self.auth_ttl.is_zero() {
            self.auth_cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
        }
    }

    fn reader(&self) -> &mpsc::UnboundedSender<ReadCmd> {
        let i = self.rr.fetch_add(1, Ordering::Relaxed) % self.readers.len();
        &self.readers[i]
    }

    pub async fn key_auth(&self, key: &str) -> Option<KeyAuth> {
        // Policies are mutable. Even an unrestricted cached key can gain a limit or expiry on a
        // different engine instance, so authorization must always read the shared authority.
        let (r, rx) = oneshot::channel();
        self.reader().send(ReadCmd::KeyAuth(key.into(), r)).ok()?;
        rx.await.ok().flatten()
    }
    pub async fn get(&self, key: &str) -> Option<KeyRow> {
        let (r, rx) = oneshot::channel();
        self.reader().send(ReadCmd::KeyGet(key.into(), r)).ok()?;
        rx.await.ok().flatten()
    }
    pub async fn account(&self, id: &str) -> Option<AccountRow> {
        let (r, rx) = oneshot::channel();
        self.reader().send(ReadCmd::Account(id.into(), r)).ok()?;
        rx.await.ok().flatten()
    }
    pub async fn account_by_handle(&self, handle: &str) -> Option<AccountRow> {
        let (r, rx) = oneshot::channel();
        self.reader().send(ReadCmd::AccountByHandle(handle.into(), r)).ok()?;
        rx.await.ok().flatten()
    }
    pub async fn totals(&self) -> BillingTotals {
        let (r, rx) = oneshot::channel();
        if self.reader().send(ReadCmd::Totals(r)).is_err() { return BillingTotals::default(); }
        rx.await.unwrap_or_default()
    }
    pub async fn accounts(&self) -> Vec<AccountRow> {
        let (r, rx) = oneshot::channel();
        if self.reader().send(ReadCmd::AccountsList(r)).is_err() { return Vec::new(); }
        rx.await.unwrap_or_default()
    }
    pub async fn keys_by_account(&self, account_id: &str) -> Vec<KeyRow> {
        let (r, rx) = oneshot::channel();
        if self.reader().send(ReadCmd::KeysByAccount(account_id.into(), r)).is_err() { return Vec::new(); }
        rx.await.unwrap_or_default()
    }
    pub async fn ledger(&self, account_id: &str, limit: i64) -> Vec<registry::LedgerRow> {
        let (r, rx) = oneshot::channel();
        if self.reader().send(ReadCmd::Ledger(account_id.into(), limit, r)).is_err() { return Vec::new(); }
        rx.await.unwrap_or_default()
    }
    pub async fn ledger_after(&self, account_id: &str, after_id: i64, limit: i64) -> Vec<registry::LedgerRow> {
        let (r, rx) = oneshot::channel();
        if self.reader().send(ReadCmd::LedgerAfter(account_id.into(), after_id, limit, r)).is_err() {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }
    pub async fn reserve_request(&self, request_id: &str, account_id: &str, key: &str, hold: i64) -> Option<i64> {
        let (r, rx) = oneshot::channel();
        let handoff = Arc::new(AtomicU8::new(RESERVE_HANDOFF_PENDING));
        let guard = ReserveHandoffGuard {
            writer: &self.writer,
            request_id: request_id.into(),
            account_id: account_id.into(),
            key: key.into(),
            hold,
            handoff: Arc::clone(&handoff),
        };
        self.writer.send(WriteCmd::Reserve {
            request_id: request_id.into(), account_id: account_id.into(), key: key.into(),
            hold, handoff, reply: r,
        }).ok()?;
        match rx.await {
            Ok(Some(balance)) if guard.claim() => Some(balance),
            Ok(_) | Err(_) => None,
        }
    }
    pub async fn settle_request(&self, request_id: &str, account_id: &str, key: &str,
                                hold: i64, actual: i64, reference: Option<&str>) -> Option<i64> {
        let (r, rx) = oneshot::channel();
        self.writer.send(WriteCmd::Settle {
            request_id: request_id.into(), account_id: account_id.into(), key: key.into(), hold, actual,
            reference: reference.map(|s| s.into()), usage: None, reply: Some(r),
        }).ok()?;
        rx.await.ok().flatten()
    }
    /// Списание/возврат БЕЗ ожидания — для RAII в синхронном контексте (Drop/finalize). `mpsc::send`
    /// не блокирует и не требует рантайма; writer применит. Осиротевшее при краше вернёт `reconcile`.
    /// `usage` — разбивка токенов/модели (аналитика), пишется рядом с charge, если передана.
    pub fn settle_detached(&self, request_id: &str, account_id: &str, key: &str, hold: i64, actual: i64,
                           reference: Option<&str>, usage: Option<registry::UsageEventInput>) {
        let _ = self.writer.send(WriteCmd::Settle {
            request_id: request_id.into(), account_id: account_id.into(), key: key.into(), hold, actual,
            reference: reference.map(|s| s.into()), usage, reply: None,
        });
    }
    pub async fn mark_delivering(&self, request_id: &str, lease_secs: i64) -> bool {
        let (reply, rx) = oneshot::channel();
        if self.writer.send(WriteCmd::MarkDelivering {
            request_id: request_id.into(), lease_secs, reply,
        }).is_err() { return false; }
        rx.await.unwrap_or(false)
    }
    pub async fn acquire_capacity(&self, lease_id: &str, request_id: &str, email: &str,
                                  lease_secs: i64, max_inflight: i64, util_cap: f64)
        -> Option<registry::pg::CapacityLease>
    {
        let (reply, rx) = oneshot::channel();
        self.writer.send(WriteCmd::AcquireCapacity {
            lease_id: lease_id.into(), request_id: request_id.into(), email: email.into(),
            lease_secs, max_inflight, util_cap, reply,
        }).ok()?;
        rx.await.ok().flatten()
    }
    pub fn release_capacity(&self, lease_id: &str) {
        let _ = self.writer.send(WriteCmd::ReleaseCapacity { lease_id: lease_id.into() });
    }
    /// Агрегат usage по модели за окно (ts ≥ since_ts) — для клиентского дашборда `/account/usage`.
    pub async fn usage_by_model(&self, account_id: &str, since_ts: i64) -> Vec<registry::UsageModelAgg> {
        let (r, rx) = oneshot::channel();
        if self.reader().send(ReadCmd::UsageByModel(account_id.into(), since_ts, r)).is_err() { return Vec::new(); }
        rx.await.unwrap_or_default()
    }
    pub async fn topup(&self, account_id: &str, amount: i64, reference: Option<&str>) -> Option<i64> {
        let (r, rx) = oneshot::channel();
        self.writer.send(WriteCmd::Topup {
            account_id: account_id.into(), amount, reference: reference.map(|s| s.into()), reply: r,
        }).ok()?;
        rx.await.ok().flatten()
    }
    // --- Control-плоскость (`/admin/*`) — редкие управляющие операции через writer ---
    pub async fn create_account(&self, id: &str, handle: Option<&str>, mult_bp: i64) -> bool {
        let (r, rx) = oneshot::channel();
        if self.writer.send(WriteCmd::CreateAccount {
            id: id.into(), handle: handle.map(|s| s.into()), mult_bp, reply: r,
        }).is_err() { return false; }
        rx.await.unwrap_or(false)
    }
    pub async fn issue_key(&self, key: &str, account_id: &str, label: Option<&str>,
                           spend_limit_nano: Option<i64>, expires_ts: Option<i64>) -> bool {
        let (r, rx) = oneshot::channel();
        if self.writer.send(WriteCmd::IssueKey {
            key: key.into(), account_id: account_id.into(), label: label.map(|s| s.into()),
            spend_limit_nano, expires_ts, reply: r,
        }).is_err() { return false; }
        rx.await.unwrap_or(false)
    }
    pub async fn account_status(&self, id: &str, status: &str) -> usize {
        let (r, rx) = oneshot::channel();
        if self.writer.send(WriteCmd::AccountStatus { id: id.into(), status: status.into(), reply: r }).is_err() { return 0; }
        let n = rx.await.unwrap_or(0);
        self.auth_cache_clear(); // статус аккаунта влияет на active в key_auth → сброс кэша
        n
    }
    pub async fn account_multiplier(&self, id: &str, mult_bp: i64) -> usize {
        let (r, rx) = oneshot::channel();
        if self.writer.send(WriteCmd::AccountMultiplier { id: id.into(), mult_bp, reply: r }).is_err() {
            return 0;
        }
        let n = rx.await.unwrap_or(0);
        self.auth_cache_clear(); // mult_bp кэшируется в KeyAuth → сброс кэша
        n
    }
    pub async fn key_status(&self, key: &str, status: &str) -> usize {
        let (r, rx) = oneshot::channel();
        if self.writer.send(WriteCmd::KeyStatus { key: key.into(), status: status.into(), reply: r }).is_err() { return 0; }
        let n = rx.await.unwrap_or(0);
        self.auth_cache_clear(); // отзыв/включение ключа → сброс кэша (отозванный живёт ≤ TTL иначе)
        n
    }
    pub async fn key_status_by_id(&self, key_id: &str, status: &str) -> usize {
        let (r, rx) = oneshot::channel();
        if self.writer.send(WriteCmd::KeyStatusById {
            key_id: key_id.into(), status: status.into(), reply: r,
        }).is_err() { return 0; }
        let n = rx.await.unwrap_or(0);
        self.auth_cache_clear();
        n
    }
    pub async fn key_label_by_id(&self, key_id: &str, label: &str) -> usize {
        let (r, rx) = oneshot::channel();
        if self.writer.send(WriteCmd::KeyLabelById {
            key_id: key_id.into(), label: label.into(), reply: r,
        }).is_err() { return 0; }
        rx.await.unwrap_or(0)
    }
    pub async fn key_policy_by_id(
        &self,
        account_id: &str,
        key_id: &str,
        spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>,
    ) -> Option<KeyPolicyUpdate> {
        let (reply, result) = oneshot::channel();
        if self.writer.send(WriteCmd::KeyPolicyById {
            account_id: account_id.into(), key_id: key_id.into(), spend_limit_nano, expires_ts, reply,
        }).is_err() { return None; }
        let updated = result.await.ok().flatten();
        if matches!(updated, Some(KeyPolicyUpdate::Updated)) { self.auth_cache_clear(); }
        updated
    }
    /// Дренаж очереди writer'а (барьер): ждёт, пока ВСЕ ранее поставленные команды (в т.ч.
    /// fire-and-forget `settle_detached`) применятся. Вызывать на graceful shutdown ПОСЛЕ дренажа
    /// стримов — тогда их финальные списания не потеряются при выходе процесса.
    pub async fn flush(&self) {
        let (r, rx) = oneshot::channel();
        if self.writer.send(WriteCmd::Flush(r)).is_ok() {
            let _ = rx.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn canceled_sqlite_reserve_handoff_releases_key_allowance() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-billing-handoff-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing = AsyncBilling::start(path_string, 1).unwrap();
        assert!(billing.create_account("acct", None, 10_000).await);
        assert_eq!(billing.topup("acct", 1_000, Some("seed")).await, Some(1_000));
        assert!(billing.issue_key("limited", "acct", None, Some(700), None).await);

        let handoff = Arc::new(AtomicU8::new(RESERVE_HANDOFF_CANCELED));
        let (reply, response) = oneshot::channel();
        billing.writer.send(WriteCmd::Reserve {
            request_id: "canceled-before-handoff".into(),
            account_id: "acct".into(),
            key: "limited".into(),
            hold: 500,
            handoff: Arc::clone(&handoff),
            reply,
        }).unwrap();
        assert!(response.await.is_err());
        billing.flush().await;

        let account = billing.account("acct").await.unwrap();
        let key = billing.get("limited").await.unwrap();
        assert_eq!((account.balance_nano, account.reserved_nano), (1_000, 0));
        assert_eq!(key.reserved_nano, 0);
        assert_eq!(handoff.load(Ordering::Acquire), RESERVE_HANDOFF_REFUNDED);

        drop(billing);
        let _ = std::fs::remove_file(path);
    }
}
