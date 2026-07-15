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

use registry::{AccountRow, BillingTotals, KeyAuth, KeyRow};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

enum WriteCmd {
    Reserve { account_id: String, hold: i64, reply: oneshot::Sender<Option<i64>> },
    Settle {
        account_id: String, key: String, hold: i64, actual: i64, reference: Option<String>,
        usage: Option<registry::UsageEventInput>, // разбивка токенов/модели (аналитика), если есть
        reply: Option<oneshot::Sender<Option<i64>>>, // None → fire-and-forget (RAII из Drop)
    },
    Topup { account_id: String, amount: i64, reference: Option<String>, reply: oneshot::Sender<Option<i64>> },
    /// Control-плоскость (редкие управляющие записи из `/admin/*`) — через ТОТ ЖЕ writer, чтобы
    /// сохранить дисциплину единственного писателя (никаких гонок/BUSY с reserve/settle).
    CreateAccount { id: String, handle: Option<String>, mult_bp: i64, reply: oneshot::Sender<bool> },
    IssueKey { key: String, account_id: String, label: Option<String>, reply: oneshot::Sender<bool> },
    AccountStatus { id: String, status: String, reply: oneshot::Sender<usize> },
    AccountMultiplier { id: String, mult_bp: i64, reply: oneshot::Sender<usize> },
    KeyStatus { key: String, status: String, reply: oneshot::Sender<usize> },
    KeyStatusById { key_id: String, status: String, reply: oneshot::Sender<usize> },
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
                while let Some(cmd) = wrx.blocking_recv() {
                    match cmd {
                        WriteCmd::Reserve { account_id, hold, reply } => {
                            let res = registry::account_reserve(&conn, &account_id, hold).ok().flatten();
                            let committed = res.is_some();
                            if reply.send(res).is_err() && committed {
                                // Клиент отменил запрос ВО ВРЕМЯ reserve().await → получатель ответа
                                // отвалился, а резерв УЖЕ закоммичен. Без рефанда hold завис бы в
                                // reserved_nano до рестарта (замороженные деньги клиента). Возвращаем сразу.
                                let _ = registry::account_settle(&conn, &account_id, "", hold, 0, None, None);
                            }
                        }
                        WriteCmd::Settle { account_id, key, hold, actual, reference, usage, reply } => {
                            // usage_event пишется ВНУТРИ той же транзакции settle (один коммит на запрос
                            // вместо двух) — только при реальном списании (actual>0), best-effort.
                            let usage_ref = if actual > 0 { usage.as_ref() } else { None };
                            let res = registry::account_settle(&conn, &account_id, &key, hold, actual, reference.as_deref(), usage_ref)
                                .ok().flatten();
                            if let Some(r) = reply { let _ = r.send(res); }
                        }
                        WriteCmd::Topup { account_id, amount, reference, reply } => {
                            let _ = reply.send(
                                registry::account_topup(&conn, &account_id, amount, reference.as_deref()).ok().flatten());
                        }
                        WriteCmd::CreateAccount { id, handle, mult_bp, reply } => {
                            let ok = registry::account_create(&conn, &id, handle.as_deref(), mult_bp).is_ok();
                            let _ = reply.send(ok);
                        }
                        WriteCmd::IssueKey { key, account_id, label, reply } => {
                            let ok = registry::key_issue(&conn, &key, &account_id, label.as_deref()).is_ok();
                            let _ = reply.send(ok);
                        }
                        WriteCmd::AccountStatus { id, status, reply } => {
                            let n = registry::account_set_status(&conn, &id, &status).unwrap_or(0);
                            let _ = reply.send(n);
                        }
                        WriteCmd::AccountMultiplier { id, mult_bp, reply } => {
                            let n = registry::account_set_mult_bp(&conn, &id, mult_bp).unwrap_or(0);
                            let _ = reply.send(n);
                        }
                        WriteCmd::KeyStatus { key, status, reply } => {
                            let n = registry::key_set_status(&conn, &key, &status).unwrap_or(0);
                            let _ = reply.send(n);
                        }
                        WriteCmd::KeyStatusById { key_id, status, reply } => {
                            let n = registry::key_set_status_by_id(&conn, &key_id, &status).unwrap_or(0);
                            let _ = reply.send(n);
                        }
                        WriteCmd::Flush(r) => { let _ = r.send(()); } // барьер обработан → очередь дренирована
                    }
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
        // TTL-кэш: под нагрузкой множество запросов одного ключа коллапсируют в один read.
        if !self.auth_ttl.is_zero() {
            if let Some((auth, at)) = self.auth_cache.lock().unwrap_or_else(|e| e.into_inner()).get(key) {
                if at.elapsed() < self.auth_ttl { return Some(auth.clone()); }
            }
        }
        let (r, rx) = oneshot::channel();
        self.reader().send(ReadCmd::KeyAuth(key.into(), r)).ok()?;
        let auth = rx.await.ok().flatten()?;
        if !self.auth_ttl.is_zero() {
            self.auth_cache.lock().unwrap_or_else(|e| e.into_inner()).insert(key.into(), (auth.clone(), Instant::now()));
        }
        Some(auth)
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
    pub async fn reserve(&self, account_id: &str, hold: i64) -> Option<i64> {
        let (r, rx) = oneshot::channel();
        self.writer.send(WriteCmd::Reserve { account_id: account_id.into(), hold, reply: r }).ok()?;
        rx.await.ok().flatten()
    }
    pub async fn settle(&self, account_id: &str, key: &str, hold: i64, actual: i64, reference: Option<&str>) -> Option<i64> {
        let (r, rx) = oneshot::channel();
        self.writer.send(WriteCmd::Settle {
            account_id: account_id.into(), key: key.into(), hold, actual,
            reference: reference.map(|s| s.into()), usage: None, reply: Some(r),
        }).ok()?;
        rx.await.ok().flatten()
    }
    /// Списание/возврат БЕЗ ожидания — для RAII в синхронном контексте (Drop/finalize). `mpsc::send`
    /// не блокирует и не требует рантайма; writer применит. Осиротевшее при краше вернёт `reconcile`.
    /// `usage` — разбивка токенов/модели (аналитика), пишется рядом с charge, если передана.
    pub fn settle_detached(&self, account_id: &str, key: &str, hold: i64, actual: i64,
                           reference: Option<&str>, usage: Option<registry::UsageEventInput>) {
        let _ = self.writer.send(WriteCmd::Settle {
            account_id: account_id.into(), key: key.into(), hold, actual,
            reference: reference.map(|s| s.into()), usage, reply: None,
        });
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
    pub async fn issue_key(&self, key: &str, account_id: &str, label: Option<&str>) -> bool {
        let (r, rx) = oneshot::channel();
        if self.writer.send(WriteCmd::IssueKey {
            key: key.into(), account_id: account_id.into(), label: label.map(|s| s.into()), reply: r,
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
