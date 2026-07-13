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
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{mpsc, oneshot};

enum WriteCmd {
    Reserve { account_id: String, hold: i64, reply: oneshot::Sender<Option<i64>> },
    Settle {
        account_id: String, key: String, hold: i64, actual: i64, reference: Option<String>,
        reply: Option<oneshot::Sender<Option<i64>>>, // None → fire-and-forget (RAII из Drop)
    },
    Topup { account_id: String, amount: i64, reference: Option<String>, reply: oneshot::Sender<Option<i64>> },
    /// Барьер: writer FIFO → когда Flush обработан, ВСЕ прежние команды (settle) применены.
    /// Для дренажа очереди на graceful shutdown (иначе последние списания потерялись бы).
    Flush(oneshot::Sender<()>),
}

enum ReadCmd {
    KeyAuth(String, oneshot::Sender<Option<KeyAuth>>),
    KeyGet(String, oneshot::Sender<Option<KeyRow>>),
    Account(String, oneshot::Sender<Option<AccountRow>>),
    Totals(oneshot::Sender<BillingTotals>),
}

/// Async-фасад биллинга: writer-канал + пул reader-каналов. Клонируется (в `Arc`) во все хендлеры.
pub struct AsyncBilling {
    writer: mpsc::UnboundedSender<WriteCmd>,
    readers: Vec<mpsc::UnboundedSender<ReadCmd>>,
    rr: AtomicUsize, // round-robin по читателям
}

impl AsyncBilling {
    /// Поднять writer-поток + `readers` reader-потоков. `open` (миграции + PRAGMA WAL) — на этих
    /// потоках; синхронный SQLite не касается async-рантайма никогда.
    pub fn start(db_path: String, readers: usize) -> anyhow::Result<Self> {
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
                                let _ = registry::account_settle(&conn, &account_id, "", hold, 0, None);
                            }
                        }
                        WriteCmd::Settle { account_id, key, hold, actual, reference, reply } => {
                            let res = registry::account_settle(&conn, &account_id, &key, hold, actual, reference.as_deref())
                                .ok().flatten();
                            if let Some(r) = reply { let _ = r.send(res); }
                        }
                        WriteCmd::Topup { account_id, amount, reference, reply } => {
                            let _ = reply.send(
                                registry::account_topup(&conn, &account_id, amount, reference.as_deref()).ok().flatten());
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
                        ReadCmd::Totals(r) => { let _ = r.send(registry::billing_totals(&conn)); }
                    }
                }
                eprintln!("⚠ billing-reader-{i} поток завершён");
            })?;
            rtxs.push(rtx);
        }
        Ok(AsyncBilling { writer: wtx, readers: rtxs, rr: AtomicUsize::new(0) })
    }

    fn reader(&self) -> &mpsc::UnboundedSender<ReadCmd> {
        let i = self.rr.fetch_add(1, Ordering::Relaxed) % self.readers.len();
        &self.readers[i]
    }

    pub async fn key_auth(&self, key: &str) -> Option<KeyAuth> {
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
    pub async fn totals(&self) -> BillingTotals {
        let (r, rx) = oneshot::channel();
        if self.reader().send(ReadCmd::Totals(r)).is_err() { return BillingTotals::default(); }
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
            reference: reference.map(|s| s.into()), reply: Some(r),
        }).ok()?;
        rx.await.ok().flatten()
    }
    /// Списание/возврат БЕЗ ожидания — для RAII в синхронном контексте (Drop/finalize). `mpsc::send`
    /// не блокирует и не требует рантайма; writer применит. Осиротевшее при краше вернёт `reconcile`.
    pub fn settle_detached(&self, account_id: &str, key: &str, hold: i64, actual: i64, reference: Option<&str>) {
        let _ = self.writer.send(WriteCmd::Settle {
            account_id: account_id.into(), key: key.into(), hold, actual,
            reference: reference.map(|s| s.into()), reply: None,
        });
    }
    pub async fn topup(&self, account_id: &str, amount: i64, reference: Option<&str>) -> Option<i64> {
        let (r, rx) = oneshot::channel();
        self.writer.send(WriteCmd::Topup {
            account_id: account_id.into(), amount, reference: reference.map(|s| s.into()), reply: r,
        }).ok()?;
        rx.await.ok().flatten()
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
