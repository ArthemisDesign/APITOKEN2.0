//! Асинхронный биллинг поверх синхронного `registry` — БЕЗ блокировки async-воркеров.
//!
//! Проблема: rusqlite синхронна. Вызвать её прямо в axum-хендлере = заблокировать tokio-воркер на
//! время запроса к БД; под нагрузкой воркеры встают на мьютексе → рантайм застревает.
//!
//! Решение (актор): ВЫДЕЛЕННЫЙ OS-поток владеет соединением и крутит блокирующий цикл `blocking_recv`.
//! Async-код шлёт команду в `mpsc` и `.await`-ит `oneshot`-ответ — воркеры НЕ блокируются ни на миг.
//! SQLite — single-writer, поэтому один поток-владелец идеально сериализует записи без SQLITE_BUSY.
//! Пропускная способность одного потока (индексированные запросы, микросекунды) — десятки тысяч
//! операций/с, с огромным запасом над 1000 юзеров (~3k оп/с). Read-параллелизм (доп. потоки на WAL)
//! добавим, если упрёмся; сейчас узкое место — не throughput, а НЕблокировка рантайма.
//!
//! RAII-возвраты (`HoldGuard::drop`, `TeeMeter::finalize`) живут в СИНХРОННОМ контексте (Drop не умеет
//! await). Для них `settle_detached` просто шлёт команду (`mpsc::send` не блокирует и не требует
//! рантайма) — актор применит позже. Гарантия денег: даже если процесс упадёт до применения, `reconcile`
//! на старте вернёт осиротевший резерв. Ничего не теряется, ничего не застревает.

use registry::{AccountRow, BillingTotals, KeyAuth, KeyRow};
use tokio::sync::{mpsc, oneshot};

enum Cmd {
    KeyAuth(String, oneshot::Sender<Option<KeyAuth>>),
    KeyGet(String, oneshot::Sender<Option<KeyRow>>),
    Account(String, oneshot::Sender<Option<AccountRow>>),
    Reserve { account_id: String, hold: i64, reply: oneshot::Sender<Option<i64>> },
    Settle {
        account_id: String, key: String, hold: i64, actual: i64, reference: Option<String>,
        reply: Option<oneshot::Sender<Option<i64>>>, // None → fire-and-forget (RAII из Drop)
    },
    Topup { account_id: String, amount: i64, reference: Option<String>, reply: oneshot::Sender<Option<i64>> },
    Totals(oneshot::Sender<BillingTotals>),
}

/// Async-фасад биллинга. Клонируется (внутри `Arc`) во все хендлеры; шлёт команды в DB-поток.
pub struct AsyncBilling {
    tx: mpsc::UnboundedSender<Cmd>,
}

impl AsyncBilling {
    /// Поднять DB-поток. `open` (миграции + PRAGMA WAL) выполняется НА ЭТОМ ПОТОКЕ — синхронный
    /// SQLite не касается async-рантайма никогда.
    pub fn start(db_path: String) -> anyhow::Result<Self> {
        let conn = registry::open(&db_path)?;
        let (tx, mut rx) = mpsc::unbounded_channel::<Cmd>();
        std::thread::Builder::new().name("billing-db".into()).spawn(move || {
            while let Some(cmd) = rx.blocking_recv() {
                match cmd {
                    Cmd::KeyAuth(k, r) => { let _ = r.send(registry::key_account(&conn, &k).ok().flatten()); }
                    Cmd::KeyGet(k, r) => { let _ = r.send(registry::key_get(&conn, &k).ok().flatten()); }
                    Cmd::Account(id, r) => { let _ = r.send(registry::account_get(&conn, &id).ok().flatten()); }
                    Cmd::Reserve { account_id, hold, reply } => {
                        let _ = reply.send(registry::account_reserve(&conn, &account_id, hold).ok().flatten());
                    }
                    Cmd::Settle { account_id, key, hold, actual, reference, reply } => {
                        let res = registry::account_settle(&conn, &account_id, &key, hold, actual, reference.as_deref())
                            .ok().flatten();
                        if let Some(r) = reply { let _ = r.send(res); }
                    }
                    Cmd::Topup { account_id, amount, reference, reply } => {
                        let _ = reply.send(
                            registry::account_topup(&conn, &account_id, amount, reference.as_deref()).ok().flatten());
                    }
                    Cmd::Totals(r) => { let _ = r.send(registry::billing_totals(&conn)); }
                }
            }
            // канал закрыт (сервер гасится) — поток завершается сам
        })?;
        Ok(AsyncBilling { tx })
    }

    pub async fn key_auth(&self, key: &str) -> Option<KeyAuth> {
        let (r, rx) = oneshot::channel();
        self.tx.send(Cmd::KeyAuth(key.into(), r)).ok()?;
        rx.await.ok().flatten()
    }
    pub async fn get(&self, key: &str) -> Option<KeyRow> {
        let (r, rx) = oneshot::channel();
        self.tx.send(Cmd::KeyGet(key.into(), r)).ok()?;
        rx.await.ok().flatten()
    }
    pub async fn account(&self, id: &str) -> Option<AccountRow> {
        let (r, rx) = oneshot::channel();
        self.tx.send(Cmd::Account(id.into(), r)).ok()?;
        rx.await.ok().flatten()
    }
    pub async fn reserve(&self, account_id: &str, hold: i64) -> Option<i64> {
        let (r, rx) = oneshot::channel();
        self.tx.send(Cmd::Reserve { account_id: account_id.into(), hold, reply: r }).ok()?;
        rx.await.ok().flatten()
    }
    pub async fn settle(&self, account_id: &str, key: &str, hold: i64, actual: i64, reference: Option<&str>) -> Option<i64> {
        let (r, rx) = oneshot::channel();
        self.tx.send(Cmd::Settle {
            account_id: account_id.into(), key: key.into(), hold, actual,
            reference: reference.map(|s| s.into()), reply: Some(r),
        }).ok()?;
        rx.await.ok().flatten()
    }
    /// Списание/возврат БЕЗ ожидания — для RAII в синхронном контексте (Drop/finalize). `mpsc::send`
    /// не блокирует и не требует рантайма; актор применит. Осиротевшее при краше вернёт `reconcile`.
    pub fn settle_detached(&self, account_id: &str, key: &str, hold: i64, actual: i64, reference: Option<&str>) {
        let _ = self.tx.send(Cmd::Settle {
            account_id: account_id.into(), key: key.into(), hold, actual,
            reference: reference.map(|s| s.into()), reply: None,
        });
    }
    pub async fn topup(&self, account_id: &str, amount: i64, reference: Option<&str>) -> Option<i64> {
        let (r, rx) = oneshot::channel();
        self.tx.send(Cmd::Topup {
            account_id: account_id.into(), amount, reference: reference.map(|s| s.into()), reply: r,
        }).ok()?;
        rx.await.ok().flatten()
    }
    pub async fn totals(&self) -> BillingTotals {
        let (r, rx) = oneshot::channel();
        if self.tx.send(Cmd::Totals(r)).is_err() { return BillingTotals::default(); }
        rx.await.unwrap_or_default()
    }
}
