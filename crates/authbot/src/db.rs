//! Персистентное состояние бота в SQLite (переживает рестарт — в отличие от JSON/памяти
//! старого бота). Пользователи, офферы, отклики, и МАШИНА создания оффера (admin_state).
//!
//! Доступ из конкурентных задач — через Mutex<Connection>. Операции синхронные и короткие,
//! `.await` под локом не держим.

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;

pub struct Store {
    c: Mutex<Connection>,
}

#[derive(Clone, Debug, Default)]
pub struct UserRow {
    pub chat_id: i64,
    pub uid: i64,
    pub username: String,
    pub status: String,    // new | pending | approved | rejected | pending_admin
    pub role: String,      // "" | admin
    pub address: String,   // BEP-20
    pub want: String, // ожидаемый ввод (reg_address | ho_* | cx_* | gm_gproxy | gm_ready | gm_wait)
    pub hproxy: String, // прокси аккаунта при передаче доступа (handover)
    pub hproxy_order: i64, // IPRoyal order id за handover-прокси (0 = ручной/внешний)
}

#[derive(Clone, Debug)]
pub struct Offer {
    pub id: i64,
    pub product: String,
    pub price: String,
    pub created_by: i64,
    pub seller_chat: i64,     // адресат оффера (0 = не задан)
    pub proxy_source: String, // buyer | seller | legacy
    pub buyer_proxy: String,  // прокси покупателя, если proxy_source=buyer
}

#[derive(Clone, Debug, Default)]
pub struct AdminState {
    pub chat_id: i64,
    pub step: String,
    pub product: String,
    pub seller_chat: i64,
    pub mode: String, // single | batch
    pub quantity: i64,
    pub unit_price: String,
    pub proxy_source: String, // buyer | seller
    pub draft_proxies: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PurchaseBatch {
    pub id: i64,
    pub product: String,
    pub unit_price: String,
    pub quantity: i64,
    pub total_price: String,
    pub created_by: i64,
    pub seller_chat: i64,
    pub proxy_source: String, // buyer | seller
    pub status: String, // offered | accepted | paying | paid | processing | paused | completed | rejected | cancelled
    pub payment_tx: String,
    pub current_item: i64, // 1-based; 0 until payment
}

#[derive(Clone, Debug)]
pub struct BatchOverview {
    pub batch: PurchaseBatch,
    pub completed: i64,
    pub remaining: i64,
}

#[derive(Clone, Debug)]
pub struct BatchItem {
    pub id: i64,
    pub batch_id: i64,
    pub item_no: i64, // 1-based position in the batch
    pub product: String,
    pub price: String,
    pub proxy: String,
    pub status: String, // pending | processing | completed
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchCompletion {
    pub batch_id: i64,
    pub item_no: i64,
    pub total: i64,
    pub completed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SellerJobRef {
    pub kind: String, // offer | batch
    pub offer_id: i64,
    pub batch_id: i64,
    pub item_no: i64,
    pub token: String, // unique activation generation; prevents stale-callback ABA
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SellerJob {
    pub seller_chat: i64,
    pub reference: SellerJobRef,
    pub product: String,
    pub phase: String, // accepted | paying | processing
    pub total: i64,
}

impl SellerJob {
    pub fn job_ref(&self) -> SellerJobRef {
        self.reference.clone()
    }
}

#[derive(Clone, Debug)]
pub struct GeminiOAuthSession {
    pub state: String,
    pub chat_id: i64,
    pub sealed_payload: String,
    pub expires_ts: i64,
    pub job: Option<SellerJobRef>,
}

fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn seller_job_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SellerJob> {
    Ok(SellerJob {
        seller_chat: row.get(0)?,
        reference: SellerJobRef {
            kind: row.get(1)?,
            offer_id: row.get(2)?,
            batch_id: row.get(3)?,
            item_no: row.get(4)?,
            token: row.get(5)?,
        },
        product: row.get(6)?,
        phase: row.get(7)?,
        total: row.get(8)?,
    })
}

impl Store {
    pub fn open(path: &str) -> Result<Store> {
        let path_ref = std::path::Path::new(path);
        if path != ":memory:" {
            if let Ok(metadata) = std::fs::symlink_metadata(path_ref) {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    bail!("authbot state database must be a regular non-symlink file");
                }
            }
        }
        if let Some(dir) = path_ref.parent() {
            if !dir.as_os_str().is_empty() {
                let existed = dir.exists();
                std::fs::create_dir_all(dir).context("create authbot state directory")?;
                let metadata =
                    std::fs::symlink_metadata(dir).context("stat authbot state directory")?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    bail!("authbot state directory must be a real directory");
                }
                if !existed {
                    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
                } else if metadata.permissions().mode() & 0o077 != 0 {
                    bail!("authbot state directory must not be accessible by group or others");
                }
            }
        }
        let c = Connection::open(path)?;
        if path != ":memory:" {
            std::fs::set_permissions(path_ref, std::fs::Permissions::from_mode(0o600))?;
        }
        c.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;
             CREATE TABLE IF NOT EXISTS users(
                chat_id INTEGER PRIMARY KEY, uid INTEGER, username TEXT DEFAULT '',
                status TEXT DEFAULT 'new', role TEXT DEFAULT '', address TEXT DEFAULT '',
                want TEXT DEFAULT '', hproxy TEXT DEFAULT '', ts INTEGER DEFAULT 0);
             CREATE TABLE IF NOT EXISTS offers(
                id INTEGER PRIMARY KEY AUTOINCREMENT, product TEXT, price TEXT,
                created_by INTEGER, ts INTEGER DEFAULT 0,
                proxy_source TEXT DEFAULT 'legacy', buyer_proxy TEXT DEFAULT '');
             CREATE TABLE IF NOT EXISTS responses(
                offer_id INTEGER, uid INTEGER, status TEXT DEFAULT '', address TEXT DEFAULT '',
                ts INTEGER DEFAULT 0, PRIMARY KEY(offer_id, uid));
             CREATE TABLE IF NOT EXISTS admin_state(
                chat_id INTEGER PRIMARY KEY, step TEXT, product TEXT DEFAULT '',
                mode TEXT DEFAULT 'single', quantity INTEGER DEFAULT 1,
                unit_price TEXT DEFAULT '', proxy_source TEXT DEFAULT '',
                draft_proxies TEXT DEFAULT '');
             CREATE TABLE IF NOT EXISTS gemini_oauth_sessions(
                state TEXT PRIMARY KEY,
                chat_id INTEGER NOT NULL UNIQUE,
                sealed_payload TEXT NOT NULL,
                expires_ts INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                job_kind TEXT NOT NULL DEFAULT '',
                job_offer_id INTEGER NOT NULL DEFAULT 0,
                job_batch_id INTEGER NOT NULL DEFAULT 0,
                job_item_no INTEGER NOT NULL DEFAULT 0,
                job_token TEXT NOT NULL DEFAULT '',
                ts INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS purchase_batches(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                product TEXT NOT NULL, unit_price TEXT NOT NULL,
                quantity INTEGER NOT NULL, total_price TEXT NOT NULL,
                created_by INTEGER NOT NULL, seller_chat INTEGER NOT NULL,
                proxy_source TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'offered',
                payment_tx TEXT DEFAULT '', current_item INTEGER NOT NULL DEFAULT 0,
                ts INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS batch_items(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                batch_id INTEGER NOT NULL, item_no INTEGER NOT NULL,
                product TEXT NOT NULL, price TEXT NOT NULL,
                proxy TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'pending',
                UNIQUE(batch_id, item_no));
             CREATE TABLE IF NOT EXISTS seller_jobs(
                seller_chat INTEGER PRIMARY KEY,
                kind TEXT NOT NULL, offer_id INTEGER NOT NULL DEFAULT 0,
                batch_id INTEGER NOT NULL DEFAULT 0, item_no INTEGER NOT NULL DEFAULT 0,
                job_token TEXT NOT NULL DEFAULT '',
                product TEXT NOT NULL, phase TEXT NOT NULL,
                ts INTEGER NOT NULL DEFAULT 0);",
        )?;
        let _ = c.execute("ALTER TABLE users ADD COLUMN hproxy TEXT DEFAULT ''", []); // мягкая миграция
                                                                                      // Legacy Developer-API builds added `hproject`. It is intentionally ignored: OAuth
                                                                                      // identity/project data now exists only inside the encrypted credential envelope.
        let _ = c.execute("ALTER TABLE users ADD COLUMN hproject TEXT DEFAULT ''", []);
        // IPRoyal order behind a bot-issued handover proxy, kept until Antigravity OAuth
        // seals the proxy/order pair into its one-use PKCE session.
        let _ = c.execute(
            "ALTER TABLE users ADD COLUMN hproxy_order INTEGER DEFAULT 0",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_oauth_sessions ADD COLUMN sealed_payload TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE offers ADD COLUMN proxy_issued INTEGER DEFAULT 0",
            [],
        ); // 1 оффер = 1 прокси
        let _ = c.execute(
            "ALTER TABLE offers ADD COLUMN seller_chat INTEGER DEFAULT 0",
            [],
        ); // адресный оффер: кому
        let _ = c.execute(
            "ALTER TABLE offers ADD COLUMN proxy_source TEXT DEFAULT 'legacy'",
            [],
        ); // buyer | seller | legacy
        let _ = c.execute(
            "ALTER TABLE offers ADD COLUMN buyer_proxy TEXT DEFAULT ''",
            [],
        ); // секрет прокси покупателя для одиночного оффера
        let _ = c.execute(
            "ALTER TABLE admin_state ADD COLUMN seller_chat INTEGER DEFAULT 0",
            [],
        ); // выбранный продавец
        let _ = c.execute(
            "ALTER TABLE admin_state ADD COLUMN mode TEXT DEFAULT 'single'",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE admin_state ADD COLUMN quantity INTEGER DEFAULT 1",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE admin_state ADD COLUMN unit_price TEXT DEFAULT ''",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE admin_state ADD COLUMN proxy_source TEXT DEFAULT ''",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE admin_state ADD COLUMN draft_proxies TEXT DEFAULT ''",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_oauth_sessions ADD COLUMN job_kind TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_oauth_sessions ADD COLUMN job_offer_id INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_oauth_sessions ADD COLUMN job_batch_id INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_oauth_sessions ADD COLUMN job_item_no INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_oauth_sessions ADD COLUMN job_token TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE seller_jobs ADD COLUMN job_token TEXT NOT NULL DEFAULT ''",
            [],
        );
        c.execute(
            "UPDATE seller_jobs SET job_token=lower(hex(randomblob(16))) WHERE job_token=''",
            [],
        )?;
        Ok(Store { c: Mutex::new(c) })
    }

    // ── пользователи ─────────────────────────────────────────────────────────
    pub fn register_user(&self, chat: i64, uid: i64, username: &str) -> Result<UserRow> {
        let c = self.c.lock().unwrap();
        c.execute(
            "INSERT INTO users(chat_id, uid, username, status, ts) VALUES(?1,?2,?3,'new',?4)
             ON CONFLICT(chat_id) DO UPDATE SET uid=excluded.uid, username=excluded.username",
            rusqlite::params![chat, uid, username, now()],
        )?;
        drop(c);
        Ok(self.get_user(chat)?.unwrap_or_default())
    }

    pub fn get_user(&self, chat: i64) -> Result<Option<UserRow>> {
        let c = self.c.lock().unwrap();
        let r = c.query_row(
            "SELECT chat_id,uid,username,status,role,address,want,hproxy,hproxy_order FROM users WHERE chat_id=?1",
            rusqlite::params![chat],
            |r| Ok(UserRow {
                chat_id: r.get(0)?, uid: r.get(1)?, username: r.get(2)?, status: r.get(3)?,
                role: r.get(4)?, address: r.get(5)?, want: r.get(6)?, hproxy: r.get(7)?,
                hproxy_order: r.get(8)?,
    }),
        ).optional()?;
        Ok(r)
    }

    pub fn set_status(&self, chat: i64, status: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE users SET status=?1 WHERE chat_id=?2",
            rusqlite::params![status, chat],
        )?;
        Ok(())
    }
    pub fn set_role(&self, chat: i64, role: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE users SET role=?1 WHERE chat_id=?2",
            rusqlite::params![role, chat],
        )?;
        Ok(())
    }
    pub fn set_address(&self, chat: i64, addr: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE users SET address=?1 WHERE chat_id=?2",
            rusqlite::params![addr, chat],
        )?;
        Ok(())
    }
    pub fn set_want(&self, chat: i64, want: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE users SET want=?1 WHERE chat_id=?2",
            rusqlite::params![want, chat],
        )?;
        Ok(())
    }
    pub fn set_hproxy(&self, chat: i64, hproxy: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE users SET hproxy=?1 WHERE chat_id=?2",
            rusqlite::params![hproxy, chat],
        )?;
        Ok(())
    }
    pub fn set_hproxy_order(&self, chat: i64, order_id: i64) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE users SET hproxy_order=?1 WHERE chat_id=?2",
            rusqlite::params![order_id, chat],
        )?;
        Ok(())
    }
    /// Persist a short-lived PKCE transaction so an authbot restart does not strand a seller in
    /// the browser. This table never contains Google access/refresh tokens or account identity.
    pub fn start_gemini_oauth(
        &self,
        chat_id: i64,
        state: &str,
        sealed_payload: &str,
        expires_ts: i64,
        proxy_order_id: i64,
    ) -> Result<Option<SellerJobRef>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1 OR expires_ts<?2",
            rusqlite::params![chat_id, now()],
        )?;
        let mut job = tx
            .query_row(
                "SELECT kind,offer_id,batch_id,item_no,job_token
                 FROM seller_jobs WHERE seller_chat=?1 AND phase='processing'",
                rusqlite::params![chat_id],
                |row| {
                    Ok(SellerJobRef {
                        kind: row.get(0)?,
                        offer_id: row.get(1)?,
                        batch_id: row.get(2)?,
                        item_no: row.get(3)?,
                        token: row.get(4)?,
                    })
                },
            )
            .optional()?;
        if let Some(current) = job.as_mut() {
            let changed = tx.execute(
                "UPDATE seller_jobs SET job_token=lower(hex(randomblob(16))),ts=?3
                 WHERE seller_chat=?1 AND job_token=?2 AND phase='processing'",
                rusqlite::params![chat_id, current.token, now()],
            )?;
            if changed != 1 {
                tx.rollback()?;
                bail!("active seller job changed while starting Gemini OAuth");
            }
            current.token = tx.query_row(
                "SELECT job_token FROM seller_jobs WHERE seller_chat=?1",
                rusqlite::params![chat_id],
                |row| row.get(0),
            )?;
        }
        let job = job.unwrap_or(SellerJobRef {
            kind: String::new(),
            offer_id: 0,
            batch_id: 0,
            item_no: 0,
            token: String::new(),
        });
        let bound_job = (!job.kind.is_empty()).then(|| job.clone());
        if let Some(expected) = bound_job.as_ref() {
            let changed = tx.execute(
                "UPDATE users SET want='gm_wait',hproxy='',hproxy_order=?1 WHERE chat_id=?2
                   AND EXISTS (
                       SELECT 1 FROM seller_jobs
                       WHERE seller_chat=?2 AND kind=?3 AND offer_id=?4 AND batch_id=?5
                         AND item_no=?6 AND job_token=?7 AND phase='processing')",
                rusqlite::params![
                    proxy_order_id,
                    chat_id,
                    expected.kind,
                    expected.offer_id,
                    expected.batch_id,
                    expected.item_no,
                    expected.token
                ],
            )?;
            if changed != 1 {
                tx.rollback()?;
                bail!("active seller job changed while persisting Gemini OAuth");
            }
        }
        tx.execute(
            "INSERT INTO gemini_oauth_sessions(
                state,chat_id,sealed_payload,expires_ts,status,ts,
                job_kind,job_offer_id,job_batch_id,job_item_no,job_token)
             VALUES(?1,?2,?3,?4,'pending',?5,?6,?7,?8,?9,?10)",
            rusqlite::params![
                state,
                chat_id,
                sealed_payload,
                expires_ts,
                now(),
                job.kind,
                job.offer_id,
                job.batch_id,
                job.item_no,
                job.token
            ],
        )?;
        tx.commit()?;
        Ok(bound_job)
    }

    /// Claim an OAuth callback exactly once. A repeated callback cannot exchange the same code or
    /// race a second credential publication.
    pub fn claim_gemini_oauth(&self, state: &str) -> Result<Option<GeminiOAuthSession>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let session = tx
            .query_row(
                "SELECT state,chat_id,sealed_payload,expires_ts,
                        job_kind,job_offer_id,job_batch_id,job_item_no,job_token
                 FROM gemini_oauth_sessions
                 WHERE state=?1 AND status='pending' AND expires_ts>=?2",
                rusqlite::params![state, now()],
                |row| {
                    let kind: String = row.get(4)?;
                    let offer_id: i64 = row.get(5)?;
                    let batch_id: i64 = row.get(6)?;
                    let item_no: i64 = row.get(7)?;
                    let token: String = row.get(8)?;
                    Ok(GeminiOAuthSession {
                        state: row.get(0)?,
                        chat_id: row.get(1)?,
                        sealed_payload: row.get(2)?,
                        expires_ts: row.get(3)?,
                        job: (!kind.is_empty()).then(|| SellerJobRef {
                            kind,
                            offer_id,
                            batch_id,
                            item_no,
                            token,
                        }),
                    })
                },
            )
            .optional()?;
        if session.is_some() {
            tx.execute(
                "UPDATE gemini_oauth_sessions SET status='processing' WHERE state=?1 AND status='pending'",
                rusqlite::params![state],
            )?;
        }
        tx.commit()?;
        Ok(session)
    }

    pub fn finish_gemini_oauth(&self, state: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "DELETE FROM gemini_oauth_sessions WHERE state=?1",
            rusqlite::params![state],
        )?;
        Ok(())
    }

    pub fn fail_gemini_oauth(&self, state: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "DELETE FROM gemini_oauth_sessions WHERE state=?1",
            rusqlite::params![state],
        )?;
        Ok(())
    }

    /// Cancel every outstanding OAuth capability for this chat before allowing a fresh flow. The
    /// non-secret IPRoyal order id stays attached so retrying the issued proxy preserves renewal.
    pub fn cancel_gemini_oauth(
        &self,
        chat_id: i64,
        expected: Option<&SellerJobRef>,
    ) -> Result<bool> {
        let mut connection = self.c.lock().unwrap();
        let transaction = connection.transaction()?;
        let current = if let Some(expected) = expected {
            transaction.execute(
                "UPDATE seller_jobs SET job_token=lower(hex(randomblob(16))),ts=?7
                 WHERE seller_chat=?1 AND kind=?2 AND offer_id=?3 AND batch_id=?4
                   AND item_no=?5 AND job_token=?6 AND phase='processing'",
                rusqlite::params![
                    chat_id,
                    expected.kind,
                    expected.offer_id,
                    expected.batch_id,
                    expected.item_no,
                    expected.token,
                    now()
                ],
            )? == 1
        } else {
            // Legacy unbound OAuth sessions can still be cancelled, but never let that path
            // mutate a seller who now owns an exact single/batch job.
            !transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM seller_jobs WHERE seller_chat=?1)",
                rusqlite::params![chat_id],
                |row| row.get::<_, bool>(0),
            )?
        };
        if !current {
            transaction.rollback()?;
            return Ok(false);
        }
        transaction.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![chat_id],
        )?;
        transaction.execute(
            "UPDATE users SET want='gm_gproxy', hproxy='' WHERE chat_id=?1",
            rusqlite::params![chat_id],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    /// A Claude OAuth child cannot survive a bot restart. Keep the proxy, but ask for email again
    /// so the seller receives a fresh authorization session instead of getting stuck at ho_code.
    pub fn recover_interrupted_handoffs(&self) -> Result<usize> {
        Ok(self
            .c
            .lock()
            .unwrap()
            .execute("UPDATE users SET want='ho_email' WHERE want='ho_code'", [])?)
    }

    /// Normalize every removed Gemini custom-client wizard state to the single official-CLI proxy
    /// step. A retained bot-issued proxy lets the bot show account preparation without asking for
    /// the proxy again; authorization starts only after the seller confirms that the account is ready.
    pub fn recover_legacy_gemini_handoffs(&self) -> Result<usize> {
        Ok(self.c.lock().unwrap().execute(
            "UPDATE users SET want='gm_gproxy' WHERE want IN ('gm_proxy','gm_auth','gm_gid','gm_gsecret')",
            [],
        )?)
    }

    /// chat_id одобренных продавцов (для рассылки офферов).
    pub fn approved_sellers(&self) -> Result<Vec<i64>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare("SELECT chat_id FROM users WHERE status='approved'")?;
        let rows = s.query_map([], |r| r.get::<_, i64>(0))?;
        Ok(rows.filter_map(|x| x.ok()).collect())
    }

    pub fn by_status(&self, status: &str) -> Result<Vec<UserRow>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare("SELECT chat_id,uid,username,status,role,address,want,hproxy,hproxy_order FROM users WHERE status=?1")?;
        let rows = s.query_map(rusqlite::params![status], |r| {
            Ok(UserRow {
                chat_id: r.get(0)?,
                uid: r.get(1)?,
                username: r.get(2)?,
                status: r.get(3)?,
                role: r.get(4)?,
                address: r.get(5)?,
                want: r.get(6)?,
                hproxy: r.get(7)?,
                hproxy_order: r.get(8)?,
            })
        })?;
        Ok(rows.filter_map(|x| x.ok()).collect())
    }

    /// Есть ли рантайм-админ с таким uid/username (role='admin').
    pub fn is_persisted_admin(&self, uid: i64, username: &str) -> Result<bool> {
        let c = self.c.lock().unwrap();
        let n: i64 = c.query_row(
            "SELECT COUNT(*) FROM users WHERE role='admin' AND (uid=?1 OR (username<>'' AND lower(username)=lower(?2)))",
            rusqlite::params![uid, username], |r| r.get(0))?;
        Ok(n > 0)
    }

    // ── офферы ───────────────────────────────────────────────────────────────
    pub fn create_offer(
        &self,
        product: &str,
        price: &str,
        by: i64,
        seller_chat: i64,
    ) -> Result<i64> {
        self.create_offer_with_proxy(product, price, by, seller_chat, "legacy", "")
    }

    pub fn create_offer_with_proxy(
        &self,
        product: &str,
        price: &str,
        by: i64,
        seller_chat: i64,
        proxy_source: &str,
        buyer_proxy: &str,
    ) -> Result<i64> {
        let c = self.c.lock().unwrap();
        c.execute(
            "INSERT INTO offers(product,price,created_by,seller_chat,proxy_source,buyer_proxy,ts)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                product,
                price,
                by,
                seller_chat,
                proxy_source,
                buyer_proxy,
                now()
            ],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn get_offer(&self, id: i64) -> Result<Option<Offer>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,product,price,created_by,COALESCE(seller_chat,0),
                    COALESCE(proxy_source,'legacy'),COALESCE(buyer_proxy,'')
             FROM offers WHERE id=?1",
            rusqlite::params![id],
            |r| {
                Ok(Offer {
                    id: r.get(0)?,
                    product: r.get(1)?,
                    price: r.get(2)?,
                    created_by: r.get(3)?,
                    seller_chat: r.get(4)?,
                    proxy_source: r.get(5)?,
                    buyer_proxy: r.get(6)?,
                })
            },
        )
        .optional()?)
    }

    pub fn set_response(&self, offer_id: i64, uid: i64, status: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "INSERT INTO responses(offer_id,uid,status,ts) VALUES(?1,?2,?3,?4)
             ON CONFLICT(offer_id,uid) DO UPDATE SET status=excluded.status",
            rusqlite::params![offer_id, uid, status, now()],
        )?;
        Ok(())
    }

    pub fn decide_offer(&self, offer_id: i64, uid: i64, status: &str) -> Result<bool> {
        if !matches!(status, "accepted" | "rejected") {
            bail!("offer decision must be accepted or rejected");
        }
        let changed = self.c.lock().unwrap().execute(
            "INSERT OR IGNORE INTO responses(offer_id,uid,status,ts) VALUES(?1,?2,?3,?4)",
            rusqlite::params![offer_id, uid, status, now()],
        )?;
        Ok(changed == 1)
    }

    /// Accept and reserve this seller in one transaction. Waiting for an address/payment is part
    /// of the deal lifecycle, so a second single or batch cannot be accepted in the meantime.
    pub fn accept_offer(&self, offer_id: i64, seller_chat: i64, seller_uid: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let product = tx
            .query_row(
                "SELECT product FROM offers
                 WHERE id=?1 AND (COALESCE(seller_chat,0)=0 OR seller_chat=?2)",
                rusqlite::params![offer_id, seller_chat],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(product) = product else {
            tx.rollback()?;
            return Ok(false);
        };
        let job_changed = tx.execute(
            "INSERT INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT ?1,'offer',?2,0,0,lower(hex(randomblob(16))),?3,'accepted',?4
             WHERE NOT EXISTS (SELECT 1 FROM seller_jobs WHERE seller_chat=?1)
               AND NOT EXISTS (
                   SELECT 1 FROM purchase_batches
                   WHERE seller_chat=?1 AND status IN ('accepted','paying','paid','processing'))
               AND NOT EXISTS (
                   SELECT 1 FROM responses r
                   JOIN offers o ON o.id=r.offer_id
                   JOIN users u ON u.uid=r.uid
                   WHERE u.chat_id=?1 AND r.status IN ('accepted','paying'))",
            rusqlite::params![seller_chat, offer_id, product, now()],
        )?;
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        let response_changed = tx.execute(
            "INSERT OR IGNORE INTO responses(offer_id,uid,status,ts)
             VALUES(?1,?2,'accepted',?3)",
            rusqlite::params![offer_id, seller_uid, now()],
        )?;
        if response_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Правило «1 оффер = 1 прокси»: пометить, что по офферу прокси уже выпущен.
    pub fn mark_offer_proxy_issued(&self, offer_id: i64) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE offers SET proxy_issued=1 WHERE id=?1",
            rusqlite::params![offer_id],
        )?;
        Ok(())
    }
    /// Выпускался ли уже прокси по этому офферу.
    pub fn offer_proxy_issued(&self, offer_id: i64) -> Result<bool> {
        let c = self.c.lock().unwrap();
        let n: i64 = c
            .query_row(
                "SELECT COALESCE(proxy_issued,0) FROM offers WHERE id=?1",
                rusqlite::params![offer_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        Ok(n > 0)
    }

    pub fn response_status(&self, offer_id: i64, uid: i64) -> Result<Option<String>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT status FROM responses WHERE offer_id=?1 AND uid=?2",
            rusqlite::params![offer_id, uid],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
    }

    pub fn accepted_offers_for_seller(&self, seller_chat: i64) -> Result<Vec<Offer>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare(
            "SELECT o.id,o.product,o.price,o.created_by,COALESCE(o.seller_chat,0),
                    COALESCE(o.proxy_source,'legacy'),COALESCE(o.buyer_proxy,'')
             FROM offers o
             WHERE o.seller_chat=?1
               AND EXISTS (SELECT 1 FROM responses r
                           WHERE r.offer_id=o.id AND r.status='accepted')
             ORDER BY o.id",
        )?;
        let rows = s.query_map(rusqlite::params![seller_chat], |r| {
            Ok(Offer {
                id: r.get(0)?,
                product: r.get(1)?,
                price: r.get(2)?,
                created_by: r.get(3)?,
                seller_chat: r.get(4)?,
                proxy_source: r.get(5)?,
                buyer_proxy: r.get(6)?,
            })
        })?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    // ── единая активная работа продавца ─────────────────────────────────────

    /// Single-offer и batch используют один persisted lock. Поэтому глобальные seller fields
    /// (`want`/`hproxy`) всегда относятся ровно к одной явно названной работе.
    pub fn active_seller_job(&self, seller_chat: i64) -> Result<Option<SellerJob>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT j.seller_chat,j.kind,j.offer_id,j.batch_id,j.item_no,j.job_token,
                    j.product,j.phase,
                    CASE WHEN j.kind='batch' THEN COALESCE(b.quantity,0) ELSE 1 END
             FROM seller_jobs j
             LEFT JOIN purchase_batches b ON b.id=j.batch_id
             WHERE j.seller_chat=?1",
            rusqlite::params![seller_chat],
            seller_job_from_row,
        )
        .optional()?)
    }

    pub fn active_seller_jobs(&self) -> Result<Vec<SellerJob>> {
        let c = self.c.lock().unwrap();
        let mut statement = c.prepare(
            "SELECT j.seller_chat,j.kind,j.offer_id,j.batch_id,j.item_no,j.job_token,
                    j.product,j.phase,
                    CASE WHEN j.kind='batch' THEN COALESCE(b.quantity,0) ELSE 1 END
             FROM seller_jobs j
             LEFT JOIN purchase_batches b ON b.id=j.batch_id
             ORDER BY j.ts,j.seller_chat",
        )?;
        let rows = statement.query_map([], seller_job_from_row)?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    /// Start a new authorization attempt for the same work. Rotating the generation makes every
    /// callback from an earlier retry stale even though source/id/item are otherwise unchanged.
    pub fn rotate_seller_job_token(
        &self,
        seller_chat: i64,
        expected: &SellerJobRef,
    ) -> Result<Option<SellerJobRef>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let changed = tx.execute(
            "UPDATE seller_jobs SET job_token=lower(hex(randomblob(16))),ts=?6
             WHERE seller_chat=?1 AND kind=?2 AND offer_id=?3 AND batch_id=?4
               AND item_no=?5 AND job_token=?7 AND phase='processing'",
            rusqlite::params![
                seller_chat,
                expected.kind,
                expected.offer_id,
                expected.batch_id,
                expected.item_no,
                now(),
                expected.token
            ],
        )?;
        if changed != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        let updated = tx.query_row(
            "SELECT kind,offer_id,batch_id,item_no,job_token
             FROM seller_jobs WHERE seller_chat=?1",
            rusqlite::params![seller_chat],
            |row| {
                Ok(SellerJobRef {
                    kind: row.get(0)?,
                    offer_id: row.get(1)?,
                    batch_id: row.get(2)?,
                    item_no: row.get(3)?,
                    token: row.get(4)?,
                })
            },
        )?;
        tx.commit()?;
        Ok(Some(updated))
    }

    pub fn set_want_for_seller_job(
        &self,
        seller_chat: i64,
        expected: &SellerJobRef,
        want: &str,
    ) -> Result<bool> {
        let changed = self.c.lock().unwrap().execute(
            "UPDATE users SET want=?1 WHERE chat_id=?2
               AND EXISTS (
                   SELECT 1 FROM seller_jobs
                   WHERE seller_chat=?2 AND kind=?3 AND offer_id=?4 AND batch_id=?5
                     AND item_no=?6 AND job_token=?7 AND phase='processing')",
            rusqlite::params![
                want,
                seller_chat,
                expected.kind,
                expected.offer_id,
                expected.batch_id,
                expected.item_no,
                expected.token
            ],
        )?;
        Ok(changed == 1)
    }

    pub fn set_handoff_state_for_seller_job(
        &self,
        seller_chat: i64,
        expected: &SellerJobRef,
        want: &str,
        proxy: &str,
        proxy_order_id: i64,
    ) -> Result<bool> {
        let changed = self.c.lock().unwrap().execute(
            "UPDATE users SET want=?1,hproxy=?2,hproxy_order=?3 WHERE chat_id=?4
               AND EXISTS (
                   SELECT 1 FROM seller_jobs
                   WHERE seller_chat=?4 AND kind=?5 AND offer_id=?6 AND batch_id=?7
                     AND item_no=?8 AND job_token=?9 AND phase='processing')",
            rusqlite::params![
                want,
                proxy,
                proxy_order_id,
                seller_chat,
                expected.kind,
                expected.offer_id,
                expected.batch_id,
                expected.item_no,
                expected.token
            ],
        )?;
        Ok(changed == 1)
    }

    /// Expand-only rollout compatibility: active batches win, then one already accepted deal per
    /// otherwise idle seller is restored. Existing locks are never overwritten.
    pub fn recover_seller_jobs(&self) -> Result<usize> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let processing = tx.execute(
            "INSERT OR IGNORE INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT b.seller_chat,'batch',0,b.id,b.current_item,
                    lower(hex(randomblob(16))),i.product,'processing',?1
             FROM purchase_batches b
             JOIN batch_items i ON i.batch_id=b.id AND i.item_no=b.current_item
             WHERE b.status='processing' AND i.status='processing'",
            rusqlite::params![now()],
        )?;
        let payments = tx.execute(
            "INSERT OR IGNORE INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT seller_chat,'batch',0,id,0,lower(hex(randomblob(16))),product,'paying',?1
             FROM purchase_batches
             WHERE status IN ('paying','paid')",
            rusqlite::params![now()],
        )?;
        let accepted_batches = tx.execute(
            "INSERT OR IGNORE INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT seller_chat,'batch',0,id,0,lower(hex(randomblob(16))),product,'accepted',ts
             FROM purchase_batches
             WHERE status='accepted'
             ORDER BY ts,id",
            [],
        )?;
        let accepted_offers = tx.execute(
            "INSERT OR IGNORE INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT u.chat_id,'offer',o.id,0,0,lower(hex(randomblob(16))),
                    o.product,'accepted',r.ts
             FROM responses r
             JOIN users u ON u.uid=r.uid
             JOIN offers o ON o.id=r.offer_id
             WHERE r.status='accepted'
             ORDER BY r.ts,o.id",
            [],
        )?;
        tx.commit()?;
        Ok(processing + payments + accepted_batches + accepted_offers)
    }

    /// Reserve the seller before the blockchain call. The response and seller lock move together,
    /// so two callbacks cannot pay/start a single offer concurrently with a batch.
    pub fn claim_offer_payment(&self, offer_id: i64, seller_chat: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let offer = tx
            .query_row(
                "SELECT o.product,u.uid
                 FROM offers o JOIN users u ON u.chat_id=?2
                 WHERE o.id=?1 AND (COALESCE(o.seller_chat,0)=0 OR o.seller_chat=?2)",
                rusqlite::params![offer_id, seller_chat],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        let Some((product, seller_uid)) = offer else {
            tx.rollback()?;
            return Ok(false);
        };
        let response_changed = tx.execute(
            "UPDATE responses SET status='paying',ts=?3
             WHERE offer_id=?1 AND uid=?2 AND status='accepted'",
            rusqlite::params![offer_id, seller_uid, now()],
        )?;
        if response_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        let mut job_changed = tx.execute(
            "UPDATE seller_jobs SET phase='paying',ts=?3
             WHERE seller_chat=?1 AND kind='offer' AND offer_id=?2 AND phase='accepted'",
            rusqlite::params![seller_chat, offer_id, now()],
        )?;
        if job_changed == 0 {
            // Compatibility for an offer accepted before seller_jobs existed.
            job_changed = tx.execute(
                "INSERT INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT ?1,'offer',?2,0,0,lower(hex(randomblob(16))),?3,'paying',?4
             WHERE NOT EXISTS (SELECT 1 FROM seller_jobs WHERE seller_chat=?1)
               AND NOT EXISTS (
                   SELECT 1 FROM purchase_batches
                   WHERE seller_chat=?1
                     AND status IN ('accepted','paying','paid','processing'))",
                rusqlite::params![seller_chat, offer_id, product, now()],
            )?;
        }
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn mark_offer_paid(&self, offer_id: i64, seller_chat: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let seller_uid = tx
            .query_row(
                "SELECT uid FROM users WHERE chat_id=?1",
                rusqlite::params![seller_chat],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(seller_uid) = seller_uid else {
            tx.rollback()?;
            return Ok(false);
        };
        let response_changed = tx.execute(
            "UPDATE responses SET status='paid',ts=?3
             WHERE offer_id=?1 AND uid=?2 AND status='paying'",
            rusqlite::params![offer_id, seller_uid, now()],
        )?;
        let job_changed = tx.execute(
            "UPDATE seller_jobs SET phase='processing',ts=?3
             WHERE seller_chat=?1 AND kind='offer' AND offer_id=?2 AND phase='paying'",
            rusqlite::params![seller_chat, offer_id, now()],
        )?;
        if response_changed != 1 || job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.commit()?;
        Ok(true)
    }

    /// Manual retry is allowed only after an admin has checked that the uncertain transaction did
    /// not land. The accepted deal keeps reserving the seller while retry is prepared.
    pub fn reset_offer_payment(&self, offer_id: i64, seller_chat: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let seller_uid = tx
            .query_row(
                "SELECT uid FROM users WHERE chat_id=?1",
                rusqlite::params![seller_chat],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(seller_uid) = seller_uid else {
            tx.rollback()?;
            return Ok(false);
        };
        let response_changed = tx.execute(
            "UPDATE responses SET status='accepted',ts=?3
             WHERE offer_id=?1 AND uid=?2 AND status='paying'",
            rusqlite::params![offer_id, seller_uid, now()],
        )?;
        let job_changed = tx.execute(
            "UPDATE seller_jobs SET phase='accepted',ts=?3
             WHERE seller_chat=?1 AND kind='offer' AND offer_id=?2 AND phase='paying'",
            rusqlite::params![seller_chat, offer_id, now()],
        )?;
        if response_changed != 1 || job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.commit()?;
        Ok(true)
    }

    /// Complete only the exact offer captured when its handoff started. A delayed callback from a
    /// different flow cannot clear or advance the seller's current work.
    pub fn finish_offer_job(
        &self,
        seller_chat: i64,
        offer_id: i64,
        job_token: &str,
    ) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let seller_uid = tx
            .query_row(
                "SELECT uid FROM users WHERE chat_id=?1",
                rusqlite::params![seller_chat],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(seller_uid) = seller_uid else {
            tx.rollback()?;
            return Ok(false);
        };
        let response_changed = tx.execute(
            "UPDATE responses SET status='completed',ts=?3
             WHERE offer_id=?1 AND uid=?2 AND status='paid'
               AND EXISTS (
                   SELECT 1 FROM seller_jobs
                   WHERE seller_chat=?4 AND kind='offer' AND offer_id=?1
                     AND job_token=?5 AND phase='processing')",
            rusqlite::params![offer_id, seller_uid, now(), seller_chat, job_token],
        )?;
        if response_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        let job_changed = tx.execute(
            "DELETE FROM seller_jobs
             WHERE seller_chat=?1 AND kind='offer' AND offer_id=?2
               AND job_token=?3 AND phase='processing'",
            rusqlite::params![seller_chat, offer_id, job_token],
        )?;
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.commit()?;
        Ok(true)
    }

    // ── машина создания оффера (persisted) ────────────────────────────────────
    pub fn get_admin_state(&self, chat: i64) -> Result<Option<(String, String, i64)>> {
        Ok(self
            .get_admin_flow(chat)?
            .map(|state| (state.step, state.product, state.seller_chat)))
    }

    pub fn get_admin_flow(&self, chat: i64) -> Result<Option<AdminState>> {
        let c = self.c.lock().unwrap();
        let state = c
            .query_row(
                "SELECT chat_id,step,product,COALESCE(seller_chat,0),
                        COALESCE(mode,'single'),COALESCE(quantity,1),COALESCE(unit_price,''),
                        COALESCE(proxy_source,''),COALESCE(draft_proxies,'')
                 FROM admin_state WHERE chat_id=?1",
                rusqlite::params![chat],
                |r| {
                    let raw_proxies: String = r.get(8)?;
                    let draft_proxies = serde_json::from_str(&raw_proxies).unwrap_or_default();
                    Ok(AdminState {
                        chat_id: r.get(0)?,
                        step: r.get(1)?,
                        product: r.get(2)?,
                        seller_chat: r.get(3)?,
                        mode: r.get(4)?,
                        quantity: r.get(5)?,
                        unit_price: r.get(6)?,
                        proxy_source: r.get(7)?,
                        draft_proxies,
                    })
                },
            )
            .optional()?;
        Ok(state)
    }

    pub fn set_admin_state(
        &self,
        chat: i64,
        step: &str,
        product: &str,
        seller_chat: i64,
    ) -> Result<()> {
        self.set_admin_flow(&AdminState {
            chat_id: chat,
            step: step.to_string(),
            product: product.to_string(),
            seller_chat,
            mode: "single".into(),
            quantity: 1,
            unit_price: String::new(),
            proxy_source: String::new(),
            draft_proxies: Vec::new(),
        })
    }

    pub fn set_admin_flow(&self, state: &AdminState) -> Result<()> {
        let draft_proxies = serde_json::to_string(&state.draft_proxies)?;
        self.c.lock().unwrap().execute(
            "INSERT INTO admin_state(chat_id,step,product,seller_chat,mode,quantity,unit_price,proxy_source,draft_proxies)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(chat_id) DO UPDATE SET
                step=excluded.step, product=excluded.product, seller_chat=excluded.seller_chat,
                mode=excluded.mode, quantity=excluded.quantity, unit_price=excluded.unit_price,
                proxy_source=excluded.proxy_source, draft_proxies=excluded.draft_proxies",
            rusqlite::params![
                state.chat_id,
                state.step,
                state.product,
                state.seller_chat,
                state.mode,
                state.quantity,
                state.unit_price,
                state.proxy_source,
                draft_proxies
            ],
        )?;
        Ok(())
    }
    pub fn clear_admin_state(&self, chat: i64) -> Result<bool> {
        let n = self.c.lock().unwrap().execute(
            "DELETE FROM admin_state WHERE chat_id=?1",
            rusqlite::params![chat],
        )?;
        Ok(n > 0)
    }

    // ── batch-покупки ─────────────────────────────────────────────────────────
    pub fn create_batch(
        &self,
        product: &str,
        unit_price: &str,
        quantity: i64,
        total_price: &str,
        by: i64,
        seller_chat: i64,
        proxy_source: &str,
        proxies: &[String],
    ) -> Result<i64> {
        if !(2..=100).contains(&quantity) {
            bail!("batch quantity must be between 2 and 100");
        }
        match proxy_source {
            "buyer" if proxies.len() == quantity as usize => {}
            "buyer" => bail!("buyer-proxy batch must contain one proxy per item"),
            "seller" if proxies.is_empty() => {}
            "seller" => bail!("seller-proxy batch cannot contain buyer proxies"),
            _ => bail!("unknown batch proxy source"),
        }
        if proxies.iter().any(|proxy| proxy.trim().is_empty()) {
            bail!("batch proxies must not be empty");
        }
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO purchase_batches(product,unit_price,quantity,total_price,created_by,seller_chat,proxy_source,status,ts)
             VALUES(?1,?2,?3,?4,?5,?6,?7,'offered',?8)",
            rusqlite::params![
                product,
                unit_price,
                quantity,
                total_price,
                by,
                seller_chat,
                proxy_source,
                now()
            ],
        )?;
        let batch_id = tx.last_insert_rowid();
        for item_no in 1..=quantity {
            let proxy = proxies
                .get((item_no - 1) as usize)
                .map(String::as_str)
                .unwrap_or("");
            tx.execute(
                "INSERT INTO batch_items(batch_id,item_no,product,price,proxy,status)
                 VALUES(?1,?2,?3,?4,?5,'pending')",
                rusqlite::params![batch_id, item_no, product, unit_price, proxy],
            )?;
        }
        tx.commit()?;
        Ok(batch_id)
    }

    pub fn get_batch(&self, id: i64) -> Result<Option<PurchaseBatch>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,product,unit_price,quantity,total_price,created_by,seller_chat,
                    proxy_source,status,COALESCE(payment_tx,''),COALESCE(current_item,0)
             FROM purchase_batches WHERE id=?1",
            rusqlite::params![id],
            |r| {
                Ok(PurchaseBatch {
                    id: r.get(0)?,
                    product: r.get(1)?,
                    unit_price: r.get(2)?,
                    quantity: r.get(3)?,
                    total_price: r.get(4)?,
                    created_by: r.get(5)?,
                    seller_chat: r.get(6)?,
                    proxy_source: r.get(7)?,
                    status: r.get(8)?,
                    payment_tx: r.get(9)?,
                    current_item: r.get(10)?,
                })
            },
        )
        .optional()?)
    }

    pub fn get_batch_item(&self, batch_id: i64, item_no: i64) -> Result<Option<BatchItem>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,batch_id,item_no,product,price,COALESCE(proxy,''),status
             FROM batch_items WHERE batch_id=?1 AND item_no=?2",
            rusqlite::params![batch_id, item_no],
            |r| {
                Ok(BatchItem {
                    id: r.get(0)?,
                    batch_id: r.get(1)?,
                    item_no: r.get(2)?,
                    product: r.get(3)?,
                    price: r.get(4)?,
                    proxy: r.get(5)?,
                    status: r.get(6)?,
                })
            },
        )
        .optional()?)
    }

    pub fn batch_items(&self, batch_id: i64) -> Result<Vec<BatchItem>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare(
            "SELECT id,batch_id,item_no,product,price,COALESCE(proxy,''),status
             FROM batch_items WHERE batch_id=?1 ORDER BY item_no",
        )?;
        let rows = s.query_map(rusqlite::params![batch_id], |r| {
            Ok(BatchItem {
                id: r.get(0)?,
                batch_id: r.get(1)?,
                item_no: r.get(2)?,
                product: r.get(3)?,
                price: r.get(4)?,
                proxy: r.get(5)?,
                status: r.get(6)?,
            })
        })?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    /// Open batches for `/jobs`. Completed/rejected/cancelled history stays in SQLite but does not
    /// clutter the operational queue. `seller_chat=0` returns the admin-wide view.
    pub fn open_batch_overviews(&self, seller_chat: i64) -> Result<Vec<BatchOverview>> {
        let c = self.c.lock().unwrap();
        let mut statement = c.prepare(
            "SELECT b.id,b.product,b.unit_price,b.quantity,b.total_price,b.created_by,
                    b.seller_chat,b.proxy_source,b.status,COALESCE(b.payment_tx,''),
                    COALESCE(b.current_item,0),
                    SUM(CASE WHEN i.status='completed' THEN 1 ELSE 0 END)
             FROM purchase_batches b
             JOIN batch_items i ON i.batch_id=b.id
             WHERE b.status IN ('offered','accepted','paying','paid','processing','paused')
               AND (?1=0 OR b.seller_chat=?1)
             GROUP BY b.id
             ORDER BY CASE b.status WHEN 'processing' THEN 0 WHEN 'paused' THEN 1 ELSE 2 END,
                      b.ts,b.id",
        )?;
        let rows = statement.query_map(rusqlite::params![seller_chat], |row| {
            let quantity = row.get::<_, i64>(3)?;
            let completed = row.get::<_, i64>(11)?;
            Ok(BatchOverview {
                batch: PurchaseBatch {
                    id: row.get(0)?,
                    product: row.get(1)?,
                    unit_price: row.get(2)?,
                    quantity,
                    total_price: row.get(4)?,
                    created_by: row.get(5)?,
                    seller_chat: row.get(6)?,
                    proxy_source: row.get(7)?,
                    status: row.get(8)?,
                    payment_tx: row.get(9)?,
                    current_item: row.get(10)?,
                },
                completed,
                remaining: quantity - completed,
            })
        })?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    pub fn paused_batch_for_seller(&self, seller_chat: i64) -> Result<Option<PurchaseBatch>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,product,unit_price,quantity,total_price,created_by,seller_chat,
                        proxy_source,status,COALESCE(payment_tx,''),COALESCE(current_item,0)
                 FROM purchase_batches
                 WHERE seller_chat=?1 AND status='paused' ORDER BY id LIMIT 1",
            rusqlite::params![seller_chat],
            |row| {
                Ok(PurchaseBatch {
                    id: row.get(0)?,
                    product: row.get(1)?,
                    unit_price: row.get(2)?,
                    quantity: row.get(3)?,
                    total_price: row.get(4)?,
                    created_by: row.get(5)?,
                    seller_chat: row.get(6)?,
                    proxy_source: row.get(7)?,
                    status: row.get(8)?,
                    payment_tx: row.get(9)?,
                    current_item: row.get(10)?,
                })
            },
        )
        .optional()?)
    }

    pub fn accept_batch(&self, batch_id: i64, seller_chat: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let product = tx
            .query_row(
                "SELECT product FROM purchase_batches
                 WHERE id=?1 AND seller_chat=?2 AND status='offered'",
                rusqlite::params![batch_id, seller_chat],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(product) = product else {
            tx.rollback()?;
            return Ok(false);
        };
        let job_changed = tx.execute(
            "INSERT INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT ?1,'batch',0,?2,0,lower(hex(randomblob(16))),?3,'accepted',?4
             WHERE NOT EXISTS (SELECT 1 FROM seller_jobs WHERE seller_chat=?1)
               AND NOT EXISTS (
                   SELECT 1 FROM purchase_batches
                   WHERE seller_chat=?1 AND id<>?2
                     AND status IN ('accepted','paying','paid','processing','paused'))
               AND NOT EXISTS (
                   SELECT 1 FROM responses r
                   JOIN users u ON u.uid=r.uid
                   WHERE u.chat_id=?1 AND r.status IN ('accepted','paying'))",
            rusqlite::params![seller_chat, batch_id, product, now()],
        )?;
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        let batch_changed = tx.execute(
            "UPDATE purchase_batches SET status='accepted',ts=?3
             WHERE id=?1 AND seller_chat=?2 AND status='offered'",
            rusqlite::params![batch_id, seller_chat, now()],
        )?;
        if batch_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn reject_batch(&self, batch_id: i64, seller_chat: i64) -> Result<bool> {
        let changed = self.c.lock().unwrap().execute(
            "UPDATE purchase_batches SET status='rejected',ts=?3
             WHERE id=?1 AND seller_chat=?2 AND status='offered'",
            rusqlite::params![batch_id, seller_chat, now()],
        )?;
        Ok(changed == 1)
    }

    /// Claim payment before calling the blockchain. Double-clicks and concurrent callbacks can
    /// therefore never send two payments or overlap this batch with a single offer.
    pub fn claim_batch_payment(&self, batch_id: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let batch = tx
            .query_row(
                "SELECT seller_chat,product FROM purchase_batches
                 WHERE id=?1 AND status='accepted'",
                rusqlite::params![batch_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((seller_chat, product)) = batch else {
            tx.rollback()?;
            return Ok(false);
        };
        let mut job_changed = tx.execute(
            "UPDATE seller_jobs SET phase='paying',ts=?3
             WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2
               AND item_no=0 AND phase='accepted'",
            rusqlite::params![seller_chat, batch_id, now()],
        )?;
        if job_changed == 0 {
            // Compatibility for a batch accepted before seller_jobs existed.
            job_changed = tx.execute(
                "INSERT INTO seller_jobs(
                    seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
                 SELECT ?1,'batch',0,?2,0,lower(hex(randomblob(16))),?3,'paying',?4
                 WHERE NOT EXISTS (SELECT 1 FROM seller_jobs WHERE seller_chat=?1)
                   AND NOT EXISTS (
                       SELECT 1 FROM purchase_batches
                       WHERE seller_chat=?1 AND id<>?2
                         AND status IN ('accepted','paying','paid','processing','paused'))
                   AND NOT EXISTS (
                       SELECT 1 FROM responses r
                       JOIN users u ON u.uid=r.uid
                       WHERE u.chat_id=?1 AND r.status IN ('accepted','paying'))",
                rusqlite::params![seller_chat, batch_id, product, now()],
            )?;
        }
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        let batch_changed = tx.execute(
            "UPDATE purchase_batches SET status='paying',ts=?2
             WHERE id=?1 AND status='accepted'",
            rusqlite::params![batch_id, now()],
        )?;
        if batch_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn mark_batch_paid(&self, batch_id: i64, tx_hash: &str) -> Result<bool> {
        let changed = self.c.lock().unwrap().execute(
            "UPDATE purchase_batches SET status='paid',payment_tx=?1,ts=?3
             WHERE id=?2 AND status='paying'",
            rusqlite::params![tx_hash, batch_id, now()],
        )?;
        Ok(changed == 1)
    }

    pub fn reset_batch_payment(&self, batch_id: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let seller_chat = tx
            .query_row(
                "SELECT seller_chat FROM purchase_batches WHERE id=?1 AND status='paying'",
                rusqlite::params![batch_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(seller_chat) = seller_chat else {
            tx.rollback()?;
            return Ok(false);
        };
        let changed = tx.execute(
            "UPDATE purchase_batches SET status='accepted',ts=?2
             WHERE id=?1 AND status='paying'",
            rusqlite::params![batch_id, now()],
        )?;
        let job_changed = tx.execute(
            "UPDATE seller_jobs SET phase='accepted',ts=?3
             WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2
               AND item_no=0 AND phase='paying'",
            rusqlite::params![seller_chat, batch_id, now()],
        )?;
        if changed != 1 || job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.commit()?;
        Ok(true)
    }

    /// A process can stop after claiming a payment but before the subprocess returns. Keep the
    /// claim locked until an admin explicitly verifies the chain and releases it for retry; this
    /// avoids silently turning an uncertain blockchain operation into a duplicate payment.
    pub fn batches_needing_payment_review(&self) -> Result<Vec<PurchaseBatch>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare(
            "SELECT id,product,unit_price,quantity,total_price,created_by,seller_chat,
                    proxy_source,status,COALESCE(payment_tx,''),COALESCE(current_item,0)
             FROM purchase_batches WHERE status='paying' ORDER BY id",
        )?;
        let rows = s.query_map([], |r| {
            Ok(PurchaseBatch {
                id: r.get(0)?,
                product: r.get(1)?,
                unit_price: r.get(2)?,
                quantity: r.get(3)?,
                total_price: r.get(4)?,
                created_by: r.get(5)?,
                seller_chat: r.get(6)?,
                proxy_source: r.get(7)?,
                status: r.get(8)?,
                payment_tx: r.get(9)?,
                current_item: r.get(10)?,
            })
        })?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    pub fn start_batch_item(&self, batch_id: i64, item_no: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let batch = tx
            .query_row(
                "SELECT seller_chat,product,status,current_item
                 FROM purchase_batches WHERE id=?1",
                rusqlite::params![batch_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((seller_chat, product, batch_status, current_item)) = batch else {
            tx.rollback()?;
            return Ok(false);
        };
        let item_status = tx
            .query_row(
                "SELECT status FROM batch_items WHERE batch_id=?1 AND item_no=?2",
                rusqlite::params![batch_id, item_no],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let initial = batch_status == "paid"
            && current_item == 0
            && item_no == 1
            && item_status.as_deref() == Some("pending");
        let resumed = batch_status == "processing"
            && current_item == item_no
            && matches!(item_status.as_deref(), Some("pending" | "processing"));
        if !initial && !resumed {
            tx.rollback()?;
            return Ok(false);
        }
        let existing_job = tx
            .query_row(
                "SELECT kind,batch_id,item_no,job_token
                 FROM seller_jobs WHERE seller_chat=?1",
                rusqlite::params![seller_chat],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        if let Some((kind, active_batch, active_item, _)) = existing_job.as_ref() {
            if kind != "batch"
                || *active_batch != batch_id
                || (*active_item != 0 && *active_item != item_no)
            {
                tx.rollback()?;
                return Ok(false);
            }
        }
        if initial || item_status.as_deref() == Some("pending") {
            let batch_changed = tx.execute(
                "UPDATE purchase_batches
                 SET status='processing',current_item=?2,ts=?3
                 WHERE id=?1 AND (
                    (status='paid' AND current_item=0 AND ?2=1)
                    OR (status='processing' AND current_item=?2))",
                rusqlite::params![batch_id, item_no, now()],
            )?;
            let item_changed = tx.execute(
                "UPDATE batch_items SET status='processing'
                 WHERE batch_id=?1 AND item_no=?2 AND status='pending'",
                rusqlite::params![batch_id, item_no],
            )?;
            if batch_changed != 1 || item_changed != 1 {
                tx.rollback()?;
                return Ok(false);
            }
        }
        if existing_job.is_some() {
            let changed = tx.execute(
                "UPDATE seller_jobs
                 SET job_token=CASE
                        WHEN item_no=?3 AND job_token<>'' THEN job_token
                        ELSE lower(hex(randomblob(16)))
                     END,
                     item_no=?3,product=?4,phase='processing',ts=?5
                 WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2
                   AND item_no IN (0,?3)",
                rusqlite::params![seller_chat, batch_id, item_no, product, now()],
            )?;
            if changed != 1 {
                tx.rollback()?;
                return Ok(false);
            }
        } else {
            tx.execute(
                "INSERT INTO seller_jobs(
                    seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
                 VALUES(?1,'batch',0,?2,?3,lower(hex(randomblob(16))),?4,'processing',?5)",
                rusqlite::params![seller_chat, batch_id, item_no, product, now()],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Finish one item and atomically move the cursor to the next one. The next item is started
    /// by the bot after the successful handoff, so Telegram/network work stays outside SQLite.
    pub fn finish_batch_item(
        &self,
        batch_id: i64,
        item_no: i64,
        job_token: &str,
    ) -> Result<Option<BatchCompletion>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let seller_chat = tx
            .query_row(
                "SELECT seller_chat FROM purchase_batches WHERE id=?1",
                rusqlite::params![batch_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(seller_chat) = seller_chat else {
            tx.rollback()?;
            return Ok(None);
        };
        let changed = tx.execute(
            "UPDATE batch_items SET status='completed'
             WHERE batch_id=?1 AND item_no=?2 AND status='processing'
               AND EXISTS (
                   SELECT 1 FROM purchase_batches
                   WHERE id=?1 AND status='processing' AND current_item=?2
               )
               AND EXISTS (
                   SELECT 1 FROM seller_jobs
                   WHERE seller_chat=?3 AND kind='batch' AND batch_id=?1
                     AND item_no=?2 AND job_token=?4 AND phase='processing'
               )",
            rusqlite::params![batch_id, item_no, seller_chat, job_token],
        )?;
        if changed != 1 {
            tx.commit()?;
            return Ok(None);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        let (total, current): (i64, i64) = tx.query_row(
            "SELECT quantity,current_item FROM purchase_batches WHERE id=?1",
            rusqlite::params![batch_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if current >= total {
            let batch_changed = tx.execute(
                "UPDATE purchase_batches
                 SET status='completed',current_item=?2,ts=?3
                 WHERE id=?1 AND status='processing' AND current_item=?4",
                rusqlite::params![batch_id, total + 1, now(), item_no],
            )?;
            if batch_changed != 1 {
                tx.rollback()?;
                return Ok(None);
            }
            let job_changed = tx.execute(
                "DELETE FROM seller_jobs
                 WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2
                   AND item_no=?3 AND job_token=?4 AND phase='processing'",
                rusqlite::params![seller_chat, batch_id, item_no, job_token],
            )?;
            if job_changed != 1 {
                tx.rollback()?;
                return Ok(None);
            }
            tx.commit()?;
            return Ok(Some(BatchCompletion {
                batch_id,
                item_no,
                total,
                completed: true,
            }));
        }
        let next_item = current + 1;
        let next_product = tx.query_row(
            "SELECT product FROM batch_items WHERE batch_id=?1 AND item_no=?2",
            rusqlite::params![batch_id, next_item],
            |row| row.get::<_, String>(0),
        )?;
        let batch_changed = tx.execute(
            "UPDATE purchase_batches SET current_item=?2,ts=?3
             WHERE id=?1 AND status='processing' AND current_item=?4",
            rusqlite::params![batch_id, next_item, now(), item_no],
        )?;
        if batch_changed != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        // Keep the seller reserved continuously between positions. The old implementation
        // deleted this row and recreated it when the next Telegram prompt was sent, leaving a
        // small window where another single/batch callback could occupy the seller.
        let job_changed = tx.execute(
            "UPDATE seller_jobs
             SET item_no=?5,job_token=lower(hex(randomblob(16))),product=?6,
                 phase='processing',ts=?7
             WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2
               AND item_no=?3 AND job_token=?4 AND phase='processing'",
            rusqlite::params![
                seller_chat,
                batch_id,
                item_no,
                job_token,
                next_item,
                next_product,
                now()
            ],
        )?;
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        tx.commit()?;
        Ok(Some(BatchCompletion {
            batch_id,
            item_no,
            total,
            completed: false,
        }))
    }

    /// Pause an in-progress batch immediately. Completed items stay completed; the current
    /// unfinished item goes back to `pending`, and the seller lock is released for a single job.
    /// Every in-flight callback is invalidated by deleting its exact seller job generation.
    pub fn pause_batch(&self, batch_id: i64, seller_chat: i64) -> Result<Option<i64>> {
        let mut connection = self.c.lock().unwrap();
        let transaction = connection.transaction()?;
        let current_item = transaction
            .query_row(
                "SELECT current_item FROM purchase_batches
                 WHERE id=?1 AND seller_chat=?2 AND status='processing' AND current_item>0",
                rusqlite::params![batch_id, seller_chat],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(current_item) = current_item else {
            transaction.rollback()?;
            return Ok(None);
        };
        let item_status = transaction
            .query_row(
                "SELECT status FROM batch_items WHERE batch_id=?1 AND item_no=?2",
                rusqlite::params![batch_id, current_item],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if !matches!(item_status.as_deref(), Some("pending" | "processing")) {
            transaction.rollback()?;
            return Ok(None);
        }
        let job_changed = transaction.execute(
            "DELETE FROM seller_jobs
             WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2
               AND item_no=?3 AND phase='processing'",
            rusqlite::params![seller_chat, batch_id, current_item],
        )?;
        if job_changed != 1 {
            transaction.rollback()?;
            return Ok(None);
        }
        transaction.execute(
            "UPDATE batch_items SET status='pending'
             WHERE batch_id=?1 AND item_no=?2 AND status='processing'",
            rusqlite::params![batch_id, current_item],
        )?;
        let batch_changed = transaction.execute(
            "UPDATE purchase_batches SET status='paused',ts=?3
             WHERE id=?1 AND seller_chat=?2 AND status='processing'",
            rusqlite::params![batch_id, seller_chat, now()],
        )?;
        if batch_changed != 1 {
            transaction.rollback()?;
            return Ok(None);
        }
        transaction.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        transaction.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        transaction.commit()?;
        Ok(Some(current_item))
    }

    /// Resume the exact pending position only when the seller is currently free. The new seller
    /// job receives a fresh generation before any Telegram instruction is sent.
    pub fn resume_paused_batch(&self, batch_id: i64, seller_chat: i64) -> Result<Option<i64>> {
        let mut connection = self.c.lock().unwrap();
        let transaction = connection.transaction()?;
        let batch = transaction
            .query_row(
                "SELECT current_item,product FROM purchase_batches
                 WHERE id=?1 AND seller_chat=?2 AND status='paused' AND current_item>0",
                rusqlite::params![batch_id, seller_chat],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((current_item, product)) = batch else {
            transaction.rollback()?;
            return Ok(None);
        };
        let item_pending = transaction.query_row(
            "SELECT status='pending' FROM batch_items WHERE batch_id=?1 AND item_no=?2",
            rusqlite::params![batch_id, current_item],
            |row| row.get::<_, bool>(0),
        )?;
        if !item_pending {
            transaction.rollback()?;
            return Ok(None);
        }
        let job_changed = transaction.execute(
            "INSERT INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT ?1,'batch',0,?2,?3,lower(hex(randomblob(16))),?4,'processing',?5
             WHERE NOT EXISTS (SELECT 1 FROM seller_jobs WHERE seller_chat=?1)
               AND NOT EXISTS (
                   SELECT 1 FROM purchase_batches
                   WHERE seller_chat=?1 AND id<>?2
                     AND status IN ('accepted','paying','paid','processing'))
               AND NOT EXISTS (
                   SELECT 1 FROM responses r
                   JOIN users u ON u.uid=r.uid
                   WHERE u.chat_id=?1 AND r.status IN ('accepted','paying'))",
            rusqlite::params![seller_chat, batch_id, current_item, product, now()],
        )?;
        if job_changed != 1 {
            transaction.rollback()?;
            return Ok(None);
        }
        let batch_changed = transaction.execute(
            "UPDATE purchase_batches SET status='processing',ts=?3
             WHERE id=?1 AND seller_chat=?2 AND status='paused'",
            rusqlite::params![batch_id, seller_chat, now()],
        )?;
        if batch_changed != 1 {
            transaction.rollback()?;
            return Ok(None);
        }
        transaction.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        transaction.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        transaction.commit()?;
        Ok(Some(current_item))
    }

    /// Remove a batch from the operational queue without destroying payment/audit history.
    /// Returns whether this action released the seller's active batch job.
    pub fn archive_batch(&self, batch_id: i64) -> Result<Option<bool>> {
        let mut connection = self.c.lock().unwrap();
        let transaction = connection.transaction()?;
        let batch = transaction
            .query_row(
                "SELECT seller_chat,status,current_item FROM purchase_batches
                 WHERE id=?1 AND status IN
                    ('offered','accepted','paid','processing','paused','rejected')",
                rusqlite::params![batch_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((seller_chat, status, current_item)) = batch else {
            transaction.rollback()?;
            return Ok(None);
        };
        let active_job = transaction
            .query_row(
                "SELECT kind,batch_id FROM seller_jobs WHERE seller_chat=?1",
                rusqlite::params![seller_chat],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        let releases_job = active_job
            .as_ref()
            .is_some_and(|(kind, active_batch)| kind == "batch" && *active_batch == batch_id);
        if active_job.is_some() && !releases_job && status != "paused" && status != "offered" {
            transaction.rollback()?;
            return Ok(None);
        }
        if current_item > 0 {
            transaction.execute(
                "UPDATE batch_items SET status='pending'
                 WHERE batch_id=?1 AND item_no=?2 AND status='processing'",
                rusqlite::params![batch_id, current_item],
            )?;
        }
        let batch_changed = transaction.execute(
            "UPDATE purchase_batches SET status='cancelled',ts=?2
             WHERE id=?1 AND status=?3",
            rusqlite::params![batch_id, now(), status],
        )?;
        if batch_changed != 1 {
            transaction.rollback()?;
            return Ok(None);
        }
        if releases_job {
            let job_changed = transaction.execute(
                "DELETE FROM seller_jobs
                 WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2",
                rusqlite::params![seller_chat, batch_id],
            )?;
            if job_changed != 1 {
                transaction.rollback()?;
                return Ok(None);
            }
            transaction.execute(
                "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
                rusqlite::params![seller_chat],
            )?;
            transaction.execute(
                "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
                rusqlite::params![seller_chat],
            )?;
        }
        transaction.commit()?;
        Ok(Some(releases_job))
    }

    /// Admin-only recovery primitive for a position that an older bot version marked complete
    /// from an unrelated single-offer callback. It rewinds exactly one step and invalidates every
    /// in-flight input/OAuth capability for the later item. The paid batch itself stays paid.
    pub fn rewind_batch_to_previous(&self, batch_id: i64, seller_chat: i64) -> Result<Option<i64>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let current = tx
            .query_row(
                "SELECT current_item FROM purchase_batches
                 WHERE id=?1 AND seller_chat=?2 AND status='processing' AND current_item>1",
                rusqlite::params![batch_id, seller_chat],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(current) = current else {
            tx.rollback()?;
            return Ok(None);
        };
        let active_job = tx
            .query_row(
                "SELECT kind,batch_id,item_no FROM seller_jobs WHERE seller_chat=?1",
                rusqlite::params![seller_chat],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((kind, active_batch, active_item)) = active_job.as_ref() {
            if kind != "batch" || *active_batch != batch_id || *active_item != current {
                tx.rollback()?;
                return Ok(None);
            }
        }
        let previous = current - 1;
        let current_changed = tx.execute(
            "UPDATE batch_items SET status='pending'
             WHERE batch_id=?1 AND item_no=?2 AND status='processing'",
            rusqlite::params![batch_id, current],
        )?;
        let previous_changed = tx.execute(
            "UPDATE batch_items SET status='pending'
             WHERE batch_id=?1 AND item_no=?2 AND status='completed'",
            rusqlite::params![batch_id, previous],
        )?;
        let batch_changed = tx.execute(
            "UPDATE purchase_batches SET current_item=?2,ts=?3
             WHERE id=?1 AND seller_chat=?4 AND status='processing' AND current_item=?5",
            rusqlite::params![batch_id, previous, now(), seller_chat, current],
        )?;
        if current_changed != 1 || previous_changed != 1 || batch_changed != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        let product = tx.query_row(
            "SELECT product FROM batch_items WHERE batch_id=?1 AND item_no=?2",
            rusqlite::params![batch_id, previous],
            |row| row.get::<_, String>(0),
        )?;
        let job_changed = if active_job.is_some() {
            tx.execute(
                "UPDATE seller_jobs
                 SET item_no=?4,job_token=lower(hex(randomblob(16))),product=?5,
                     phase='processing',ts=?6
                 WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2 AND item_no=?3",
                rusqlite::params![seller_chat, batch_id, current, previous, product, now()],
            )?
        } else {
            tx.execute(
                "INSERT INTO seller_jobs(
                    seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
                 VALUES(?1,'batch',0,?2,?3,lower(hex(randomblob(16))),?4,'processing',?5)",
                rusqlite::params![seller_chat, batch_id, previous, product, now()],
            )?
        };
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.commit()?;
        Ok(Some(previous))
    }

    pub fn active_batch_for_seller(&self, seller_chat: i64) -> Result<Option<PurchaseBatch>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,product,unit_price,quantity,total_price,created_by,seller_chat,
                    proxy_source,status,COALESCE(payment_tx,''),COALESCE(current_item,0)
             FROM purchase_batches WHERE seller_chat=?1 AND status='processing'
             ORDER BY id LIMIT 1",
            rusqlite::params![seller_chat],
            |r| {
                Ok(PurchaseBatch {
                    id: r.get(0)?,
                    product: r.get(1)?,
                    unit_price: r.get(2)?,
                    quantity: r.get(3)?,
                    total_price: r.get(4)?,
                    created_by: r.get(5)?,
                    seller_chat: r.get(6)?,
                    proxy_source: r.get(7)?,
                    status: r.get(8)?,
                    payment_tx: r.get(9)?,
                    current_item: r.get(10)?,
                })
            },
        )
        .optional()?)
    }

    pub fn accepted_batches_for_seller(&self, seller_chat: i64) -> Result<Vec<PurchaseBatch>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare(
            "SELECT id,product,unit_price,quantity,total_price,created_by,seller_chat,
                    proxy_source,status,COALESCE(payment_tx,''),COALESCE(current_item,0)
             FROM purchase_batches WHERE seller_chat=?1 AND status='accepted' ORDER BY id",
        )?;
        let rows = s.query_map(rusqlite::params![seller_chat], |r| {
            Ok(PurchaseBatch {
                id: r.get(0)?,
                product: r.get(1)?,
                unit_price: r.get(2)?,
                quantity: r.get(3)?,
                total_price: r.get(4)?,
                created_by: r.get(5)?,
                seller_chat: r.get(6)?,
                proxy_source: r.get(7)?,
                status: r.get(8)?,
                payment_tx: r.get(9)?,
                current_item: r.get(10)?,
            })
        })?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    /// Batches that were paid or were moving to the next item when the process stopped. The
    /// caller decides whether to resend an instruction based on the seller's persisted `want`.
    pub fn batches_needing_resume(&self) -> Result<Vec<(PurchaseBatch, BatchItem)>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare(
            "SELECT b.id,b.product,b.unit_price,b.quantity,b.total_price,b.created_by,b.seller_chat,
                    b.proxy_source,b.status,COALESCE(b.payment_tx,''),COALESCE(b.current_item,0),
                    i.id,i.batch_id,i.item_no,i.product,i.price,COALESCE(i.proxy,''),i.status
             FROM purchase_batches b
             JOIN batch_items i ON i.batch_id=b.id
                AND i.item_no=CASE WHEN b.current_item=0 THEN 1 ELSE b.current_item END
             WHERE b.status IN ('paid','processing')
               AND i.status IN ('pending','processing')
             ORDER BY b.id",
        )?;
        let rows = s.query_map([], |r| {
            Ok((
                PurchaseBatch {
                    id: r.get(0)?,
                    product: r.get(1)?,
                    unit_price: r.get(2)?,
                    quantity: r.get(3)?,
                    total_price: r.get(4)?,
                    created_by: r.get(5)?,
                    seller_chat: r.get(6)?,
                    proxy_source: r.get(7)?,
                    status: r.get(8)?,
                    payment_tx: r.get(9)?,
                    current_item: r.get(10)?,
                },
                BatchItem {
                    id: r.get(11)?,
                    batch_id: r.get(12)?,
                    item_no: r.get(13)?,
                    product: r.get(14)?,
                    price: r.get(15)?,
                    proxy: r.get(16)?,
                    status: r.get(17)?,
                },
            ))
        })?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    pub fn counts(&self) -> (i64, i64) {
        let c = self.c.lock().unwrap();
        let u = c
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
            .unwrap_or(0);
        let o = c
            .query_row("SELECT COUNT(*) FROM offers", [], |r| r.get(0))
            .unwrap_or(0);
        (u, o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = format!(
            "{}/authbot_test_{}_{}",
            std::env::temp_dir().display(),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let _ = std::fs::remove_dir_all(&dir);
        format!("{dir}/authbot.db")
    }

    #[test]
    fn state_survives_restart() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        {
            let s = Store::open(&p).unwrap();
            s.register_user(111, 111, "seller").unwrap();
            s.set_status(111, "approved").unwrap();
            s.set_want(111, "ho_code").unwrap();
            s.register_user(222, 222, "gemini-seller").unwrap();
            s.set_want(222, "gm_proxy").unwrap();
            s.register_user(333, 333, "legacy-gemini-seller").unwrap();
            s.set_want(333, "gm_auth").unwrap();
            s.set_admin_state(999, "seller", "Claude Max20x", 0)
                .unwrap(); // «в процессе» создания оффера
            s.set_admin_state(999, "price", "Claude Max20x", 111)
                .unwrap(); // продавец выбран
            let oid = s.create_offer("Claude Max20x", "$20", 999, 111).unwrap();
            assert_eq!(oid, 1);
        }
        // «рестарт» бота = новое открытие той же БД
        let s = Store::open(&p).unwrap();
        assert_eq!(s.recover_interrupted_handoffs().unwrap(), 1);
        assert_eq!(s.recover_legacy_gemini_handoffs().unwrap(), 2);
        assert_eq!(s.get_user(111).unwrap().unwrap().status, "approved");
        assert_eq!(s.get_user(111).unwrap().unwrap().want, "ho_email");
        assert_eq!(s.get_user(222).unwrap().unwrap().want, "gm_gproxy");
        assert_eq!(s.get_user(333).unwrap().unwrap().want, "gm_gproxy");
        s.set_hproxy_order(222, 42).unwrap();
        s.start_gemini_oauth(222, "pending-state", "sealed", now() + 60, 0)
            .unwrap();
        assert!(s.cancel_gemini_oauth(222, None).unwrap());
        assert!(s.claim_gemini_oauth("pending-state").unwrap().is_none());
        assert_eq!(s.get_user(222).unwrap().unwrap().hproxy_order, 42);
        assert_eq!(s.approved_sellers().unwrap(), vec![111]);
        // машина создания оффера НЕ потеряна (это и был баг Python-версии)
        let (step, product, seller) = s.get_admin_state(999).unwrap().unwrap();
        assert_eq!(step, "price");
        assert_eq!(product, "Claude Max20x");
        assert_eq!(seller, 111);
        let o = s.get_offer(1).unwrap().unwrap();
        assert_eq!(o.product, "Claude Max20x");
        assert_eq!(o.seller_chat, 111);
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn batch_has_one_proxy_per_item_and_advances_atomically() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        let s = Store::open(&p).unwrap();
        let proxies = vec![
            "http://u1:p1@1.1.1.1:1001".to_string(),
            "http://u2:p2@2.2.2.2:1002".to_string(),
            "http://u3:p3@3.3.3.3:1003".to_string(),
        ];
        let id = s
            .create_batch("ChatGPT Plus", "$20", 3, "$60", 999, 111, "buyer", &proxies)
            .unwrap();
        assert_eq!(s.get_batch(id).unwrap().unwrap().status, "offered");
        assert_eq!(s.batch_items(id).unwrap().len(), 3);
        assert_eq!(s.get_batch_item(id, 2).unwrap().unwrap().proxy, proxies[1]);
        assert!(!s.start_batch_item(id, 2).unwrap());
        assert!(s.accept_batch(id, 111).unwrap());
        assert!(s.claim_batch_payment(id).unwrap());
        s.mark_batch_paid(id, "0xtest").unwrap();
        let resume = s.batches_needing_resume().unwrap();
        assert_eq!(resume.len(), 1);
        assert_eq!(resume[0].1.item_no, 1);
        assert!(s.start_batch_item(id, 1).unwrap());
        assert!(!s.start_batch_item(id, 3).unwrap());
        assert_eq!(
            s.active_batch_for_seller(111)
                .unwrap()
                .unwrap()
                .current_item,
            1
        );

        let first_token = s.active_seller_job(111).unwrap().unwrap().reference.token;
        let first = s.finish_batch_item(id, 1, &first_token).unwrap().unwrap();
        assert_eq!(
            first,
            BatchCompletion {
                batch_id: id,
                item_no: 1,
                total: 3,
                completed: false
            }
        );
        assert!(s.start_batch_item(id, 2).unwrap());
        assert_eq!(
            s.get_batch_item(id, 2).unwrap().unwrap().status,
            "processing"
        );
        let second_token = s.active_seller_job(111).unwrap().unwrap().reference.token;
        assert!(
            !s.finish_batch_item(id, 2, &second_token)
                .unwrap()
                .unwrap()
                .completed
        );
        assert!(s.start_batch_item(id, 3).unwrap());
        let third_token = s.active_seller_job(111).unwrap().unwrap().reference.token;
        assert!(
            s.finish_batch_item(id, 3, &third_token)
                .unwrap()
                .unwrap()
                .completed
        );
        assert!(s.active_batch_for_seller(111).unwrap().is_none());
        assert!(s.finish_batch_item(id, 3, &third_token).unwrap().is_none());

        let queued = s
            .create_batch("ChatGPT Plus", "$20", 2, "$40", 999, 111, "seller", &[])
            .unwrap();
        assert!(s.accept_batch(queued, 111).unwrap());
        assert!(s.claim_batch_payment(queued).unwrap());
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn seller_cannot_accept_two_batches_at_once() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        let s = Store::open(&p).unwrap();
        let first = s
            .create_batch("Claude Pro", "$20", 2, "$40", 999, 111, "seller", &[])
            .unwrap();
        let second = s
            .create_batch("Claude Pro", "$20", 2, "$40", 999, 111, "seller", &[])
            .unwrap();
        assert!(s.accept_batch(first, 111).unwrap());
        assert!(!s.accept_batch(second, 111).unwrap());
        let accepted = s.active_seller_job(111).unwrap().unwrap();
        assert_eq!(accepted.reference.batch_id, first);
        assert_eq!(accepted.phase, "accepted");
        assert!(s.claim_batch_payment(first).unwrap());
        assert_eq!(s.batches_needing_payment_review().unwrap().len(), 1);
        assert!(!s.claim_batch_payment(second).unwrap());
        assert!(s.reset_batch_payment(first).unwrap());
        assert_eq!(s.get_batch(first).unwrap().unwrap().status, "accepted");
        assert_eq!(s.active_seller_job(111).unwrap().unwrap().phase, "accepted");
        assert!(s.claim_batch_payment(first).unwrap());
        s.mark_batch_paid(first, "0xfirst").unwrap();
        assert!(!s.claim_batch_payment(second).unwrap());
        assert!(s.start_batch_item(first, 1).unwrap());
        assert!(!s.claim_batch_payment(second).unwrap());
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn seller_reservation_starts_at_acceptance_across_single_and_batch() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "seller-one").unwrap();
        let offer = s
            .create_offer_with_proxy("ChatGPT Plus", "$20", 999, 111, "seller", "")
            .unwrap();
        let batch = s
            .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
            .unwrap();
        assert!(s.accept_offer(offer, 111, 111).unwrap());
        assert!(!s.accept_batch(batch, 111).unwrap());
        let job = s.active_seller_job(111).unwrap().unwrap();
        assert_eq!(job.reference.kind, "offer");
        assert_eq!(job.reference.offer_id, offer);
        assert_eq!(job.phase, "accepted");

        s.register_user(222, 222, "seller-two").unwrap();
        let batch = s
            .create_batch("Claude Pro", "$20", 2, "$40", 999, 222, "seller", &[])
            .unwrap();
        let offer = s
            .create_offer_with_proxy("Claude Pro", "$20", 999, 222, "seller", "")
            .unwrap();
        assert!(s.accept_batch(batch, 222).unwrap());
        assert!(!s.accept_offer(offer, 222, 222).unwrap());
        let job = s.active_seller_job(222).unwrap().unwrap();
        assert_eq!(job.reference.kind, "batch");
        assert_eq!(job.reference.batch_id, batch);
        assert_eq!(job.phase, "accepted");
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn single_offer_and_batch_share_one_exact_seller_lock() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "seller").unwrap();
        let offer = s
            .create_offer_with_proxy("ChatGPT Plus", "$20", 999, 111, "seller", "")
            .unwrap();
        let batch = s
            .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
            .unwrap();
        assert!(s.accept_batch(batch, 111).unwrap());
        assert!(s.claim_batch_payment(batch).unwrap());
        assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
        assert!(s.start_batch_item(batch, 1).unwrap());
        s.set_response(offer, 111, "accepted").unwrap();

        assert!(!s.claim_offer_payment(offer, 111).unwrap());
        assert_eq!(
            s.response_status(offer, 111).unwrap().as_deref(),
            Some("accepted")
        );
        let job = s.active_seller_job(111).unwrap().unwrap();
        assert_eq!(job.reference.kind, "batch");
        assert_eq!(job.reference.batch_id, batch);
        assert_eq!(job.reference.item_no, 1);
        assert_eq!(s.get_batch(batch).unwrap().unwrap().current_item, 1);
        assert_eq!(
            s.get_batch_item(batch, 1).unwrap().unwrap().status,
            "processing"
        );
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn unrelated_single_completion_cannot_advance_a_batch() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "seller").unwrap();
        let offer = s
            .create_offer_with_proxy("ChatGPT Plus", "$20", 999, 111, "seller", "")
            .unwrap();
        s.set_response(offer, 111, "paid").unwrap();
        let batch = s
            .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
            .unwrap();
        assert!(s.accept_batch(batch, 111).unwrap());
        assert!(s.claim_batch_payment(batch).unwrap());
        assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
        assert!(s.start_batch_item(batch, 1).unwrap());

        assert!(!s.finish_offer_job(111, offer, "stale-offer").unwrap());
        assert_eq!(s.get_batch(batch).unwrap().unwrap().current_item, 1);
        assert_eq!(
            s.get_batch_item(batch, 1).unwrap().unwrap().status,
            "processing"
        );
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn paused_batch_allows_one_single_then_resumes_the_exact_item() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "seller").unwrap();
        let batch = s
            .create_batch("Google AI Pro", "$20", 3, "$60", 999, 111, "seller", &[])
            .unwrap();
        assert!(s.accept_batch(batch, 111).unwrap());
        assert!(s.claim_batch_payment(batch).unwrap());
        assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
        assert!(s.start_batch_item(batch, 1).unwrap());
        let first = s.active_seller_job(111).unwrap().unwrap().job_ref();
        assert!(s
            .finish_batch_item(batch, 1, &first.token)
            .unwrap()
            .is_some());
        assert!(s.start_batch_item(batch, 2).unwrap());
        let interrupted = s.active_seller_job(111).unwrap().unwrap().job_ref();
        assert!(s
            .set_handoff_state_for_seller_job(
                111,
                &interrupted,
                "gm_ready",
                "http://buyer:proxy@1.2.3.4:8080",
                0,
            )
            .unwrap());

        assert_eq!(s.pause_batch(batch, 111).unwrap(), Some(2));
        assert_eq!(s.get_batch(batch).unwrap().unwrap().status, "paused");
        assert_eq!(
            s.get_batch_item(batch, 2).unwrap().unwrap().status,
            "pending"
        );
        assert!(s.active_seller_job(111).unwrap().is_none());
        assert_eq!(s.get_user(111).unwrap().unwrap().want, "");
        assert!(s
            .finish_batch_item(batch, 2, &interrupted.token)
            .unwrap()
            .is_none());

        let second_batch = s
            .create_batch("Claude Pro", "$20", 2, "$40", 999, 111, "seller", &[])
            .unwrap();
        assert!(!s.accept_batch(second_batch, 111).unwrap());
        let offer = s
            .create_offer_with_proxy("ChatGPT Plus", "$20", 999, 111, "seller", "")
            .unwrap();
        assert!(s.accept_offer(offer, 111, 111).unwrap());
        assert!(s.resume_paused_batch(batch, 111).unwrap().is_none());
        assert!(s.claim_offer_payment(offer, 111).unwrap());
        assert!(s.mark_offer_paid(offer, 111).unwrap());
        let single = s.active_seller_job(111).unwrap().unwrap().job_ref();
        assert!(s.finish_offer_job(111, offer, &single.token).unwrap());

        assert_eq!(s.resume_paused_batch(batch, 111).unwrap(), Some(2));
        let resumed = s.active_seller_job(111).unwrap().unwrap();
        assert_eq!(resumed.reference.kind, "batch");
        assert_eq!(resumed.reference.batch_id, batch);
        assert_eq!(resumed.reference.item_no, 2);
        assert_ne!(resumed.reference.token, interrupted.token);
        assert!(s.start_batch_item(batch, 2).unwrap());
        let overview = s.open_batch_overviews(111).unwrap();
        let overview = overview
            .iter()
            .find(|overview| overview.batch.id == batch)
            .unwrap();
        assert_eq!(overview.completed, 1);
        assert_eq!(overview.remaining, 2);
        assert_eq!(s.archive_batch(batch).unwrap(), Some(true));
        assert_eq!(s.get_batch(batch).unwrap().unwrap().status, "cancelled");
        assert_eq!(
            s.get_batch_item(batch, 2).unwrap().unwrap().status,
            "pending"
        );
        assert!(s.active_seller_job(111).unwrap().is_none());
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn archiving_a_paused_batch_does_not_clear_the_active_single() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "seller").unwrap();
        let batch = s
            .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
            .unwrap();
        assert!(s.accept_batch(batch, 111).unwrap());
        assert!(s.claim_batch_payment(batch).unwrap());
        assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
        assert!(s.start_batch_item(batch, 1).unwrap());
        assert_eq!(s.pause_batch(batch, 111).unwrap(), Some(1));

        let offer = s
            .create_offer_with_proxy("Claude Pro", "$20", 999, 111, "seller", "")
            .unwrap();
        assert!(s.accept_offer(offer, 111, 111).unwrap());
        assert!(s.claim_offer_payment(offer, 111).unwrap());
        assert!(s.mark_offer_paid(offer, 111).unwrap());
        let single = s.active_seller_job(111).unwrap().unwrap().job_ref();
        assert!(s
            .set_handoff_state_for_seller_job(
                111,
                &single,
                "ho_email",
                "http://seller:proxy@1.2.3.4:8080",
                0,
            )
            .unwrap());

        assert_eq!(s.archive_batch(batch).unwrap(), Some(false));
        assert_eq!(s.get_batch(batch).unwrap().unwrap().status, "cancelled");
        let still_single = s.active_seller_job(111).unwrap().unwrap();
        assert_eq!(still_single.reference.kind, "offer");
        assert_eq!(still_single.reference.offer_id, offer);
        assert_eq!(s.get_user(111).unwrap().unwrap().want, "ho_email");
        assert!(s.open_batch_overviews(111).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn paused_batch_survives_restart_without_relocking_the_seller() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        let batch;
        {
            let s = Store::open(&p).unwrap();
            s.register_user(111, 111, "seller").unwrap();
            batch = s
                .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
                .unwrap();
            assert!(s.accept_batch(batch, 111).unwrap());
            assert!(s.claim_batch_payment(batch).unwrap());
            assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
            assert!(s.start_batch_item(batch, 1).unwrap());
            assert_eq!(s.pause_batch(batch, 111).unwrap(), Some(1));
        }
        let s = Store::open(&p).unwrap();
        assert_eq!(s.recover_seller_jobs().unwrap(), 0);
        assert!(s.active_seller_job(111).unwrap().is_none());
        assert_eq!(s.get_batch(batch).unwrap().unwrap().status, "paused");
        assert_eq!(s.resume_paused_batch(batch, 111).unwrap(), Some(1));
        assert!(s.start_batch_item(batch, 1).unwrap());
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn admin_can_rewind_only_the_previous_batch_position() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "seller").unwrap();
        let offer = s
            .create_offer_with_proxy("ChatGPT Plus", "$20", 999, 111, "seller", "")
            .unwrap();
        let batch = s
            .create_batch("Google AI Pro", "$20", 3, "$60", 999, 111, "seller", &[])
            .unwrap();
        assert!(s.accept_batch(batch, 111).unwrap());
        assert!(s.claim_batch_payment(batch).unwrap());
        assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
        assert!(s.start_batch_item(batch, 1).unwrap());
        let first_token = s.active_seller_job(111).unwrap().unwrap().reference.token;
        assert!(s
            .finish_batch_item(batch, 2, &first_token)
            .unwrap()
            .is_none());
        assert!(s
            .finish_batch_item(batch, 1, &first_token)
            .unwrap()
            .is_some());
        let queued = s.active_seller_job(111).unwrap().unwrap();
        assert_eq!(queued.reference.kind, "batch");
        assert_eq!(queued.reference.batch_id, batch);
        assert_eq!(queued.reference.item_no, 2);
        assert_ne!(queued.reference.token, first_token);
        assert!(!s.accept_offer(offer, 111, 111).unwrap());
        assert!(s.start_batch_item(batch, 2).unwrap());

        assert_eq!(s.rewind_batch_to_previous(batch, 111).unwrap(), Some(1));
        assert_eq!(s.get_batch(batch).unwrap().unwrap().current_item, 1);
        assert_eq!(
            s.get_batch_item(batch, 1).unwrap().unwrap().status,
            "pending"
        );
        assert_eq!(
            s.get_batch_item(batch, 2).unwrap().unwrap().status,
            "pending"
        );
        let rewound = s.active_seller_job(111).unwrap().unwrap();
        assert_eq!(rewound.reference.kind, "batch");
        assert_eq!(rewound.reference.batch_id, batch);
        assert_eq!(rewound.reference.item_no, 1);
        assert_ne!(first_token, rewound.reference.token);
        assert!(s.start_batch_item(batch, 1).unwrap());
        let rewound_token = s.active_seller_job(111).unwrap().unwrap().reference.token;
        assert_ne!(first_token, rewound_token);
        let rewound_job = s.active_seller_job(111).unwrap().unwrap().job_ref();
        assert!(s
            .set_handoff_state_for_seller_job(
                111,
                &rewound_job,
                "gm_ready",
                "http://new:proxy@1.2.3.4:8080",
                0,
            )
            .unwrap());
        let mut stale_job = rewound_job.clone();
        stale_job.token = first_token.clone();
        assert!(!s
            .set_want_for_seller_job(111, &stale_job, "cx_email")
            .unwrap());
        assert_eq!(s.get_user(111).unwrap().unwrap().want, "gm_ready");
        assert!(s
            .finish_batch_item(batch, 1, &first_token)
            .unwrap()
            .is_none());
        assert_eq!(s.get_batch(batch).unwrap().unwrap().current_item, 1);
        assert!(s.rewind_batch_to_previous(batch, 111).unwrap().is_none());
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn rollout_recovers_the_exact_inflight_batch_position() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        let batch;
        {
            let s = Store::open(&p).unwrap();
            batch = s
                .create_batch("Google AI Pro", "$20", 5, "$100", 999, 111, "seller", &[])
                .unwrap();
            assert!(s.accept_batch(batch, 111).unwrap());
            assert!(s.claim_batch_payment(batch).unwrap());
            assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
            assert!(s.start_batch_item(batch, 1).unwrap());
            let first_token = s.active_seller_job(111).unwrap().unwrap().reference.token;
            assert!(s
                .finish_batch_item(batch, 1, &first_token)
                .unwrap()
                .is_some());
            assert!(s.start_batch_item(batch, 2).unwrap());
            // Simulate the pre-seller_jobs production schema while preserving batch progress.
            s.c.lock()
                .unwrap()
                .execute("DELETE FROM seller_jobs", [])
                .unwrap();
        }
        let s = Store::open(&p).unwrap();
        assert_eq!(s.recover_seller_jobs().unwrap(), 1);
        let job = s.active_seller_job(111).unwrap().unwrap();
        assert_eq!(job.reference.kind, "batch");
        assert_eq!(job.reference.batch_id, batch);
        assert_eq!(job.reference.item_no, 2);
        assert_eq!(job.total, 5);
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn uncertain_single_payment_stays_locked_until_admin_review() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        {
            let s = Store::open(&p).unwrap();
            s.register_user(111, 111, "seller").unwrap();
            let offer = s
                .create_offer_with_proxy("ChatGPT Plus", "$20", 999, 111, "seller", "")
                .unwrap();
            s.set_response(offer, 111, "accepted").unwrap();
            assert!(s.claim_offer_payment(offer, 111).unwrap());
        }
        let s = Store::open(&p).unwrap();
        let job = s.active_seller_job(111).unwrap().unwrap();
        assert_eq!(job.reference.kind, "offer");
        assert_eq!(job.phase, "paying");
        assert!(!s.claim_offer_payment(job.reference.offer_id, 111).unwrap());
        assert!(s.reset_offer_payment(job.reference.offer_id, 111).unwrap());
        let accepted = s.active_seller_job(111).unwrap().unwrap();
        assert_eq!(accepted.reference.offer_id, job.reference.offer_id);
        assert_eq!(accepted.phase, "accepted");
        assert_eq!(
            s.response_status(job.reference.offer_id, 111)
                .unwrap()
                .as_deref(),
            Some("accepted")
        );
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn gemini_oauth_session_keeps_the_exact_seller_job() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "seller").unwrap();
        let offer = s
            .create_offer_with_proxy("Google AI Pro", "$20", 999, 111, "seller", "")
            .unwrap();
        s.set_response(offer, 111, "accepted").unwrap();
        assert!(s.claim_offer_payment(offer, 111).unwrap());
        assert!(s.mark_offer_paid(offer, 111).unwrap());
        s.start_gemini_oauth(111, "bound-state", "sealed", now() + 60, 0)
            .unwrap();
        let expected_job = s.active_seller_job(111).unwrap().unwrap().job_ref();
        let session = s.claim_gemini_oauth("bound-state").unwrap().unwrap();
        assert_eq!(session.job, Some(expected_job.clone()));
        assert!(s.cancel_gemini_oauth(111, Some(&expected_job)).unwrap());
        let retry_job = s.active_seller_job(111).unwrap().unwrap().job_ref();
        assert_ne!(retry_job.token, expected_job.token);
        assert!(!s.cancel_gemini_oauth(111, Some(&expected_job)).unwrap());
        assert!(!s.finish_offer_job(111, offer, &expected_job.token).unwrap());
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn uncertain_batch_payment_stays_locked_across_restart() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        {
            let s = Store::open(&p).unwrap();
            let id = s
                .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
                .unwrap();
            assert!(s.accept_batch(id, 111).unwrap());
            assert!(s.claim_batch_payment(id).unwrap());
        }
        let s = Store::open(&p).unwrap();
        let review = s.batches_needing_payment_review().unwrap();
        assert_eq!(review.len(), 1);
        assert_eq!(review[0].status, "paying");
        assert!(s.archive_batch(review[0].id).unwrap().is_none());
        assert!(s.reset_batch_payment(review[0].id).unwrap());
        assert_eq!(
            s.get_batch(review[0].id).unwrap().unwrap().status,
            "accepted"
        );
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }

    #[test]
    fn admin_batch_draft_survives_restart_without_losing_proxy_order() {
        let p = tmp();
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
        {
            let s = Store::open(&p).unwrap();
            s.set_admin_flow(&AdminState {
                chat_id: 999,
                step: "batch_proxies".into(),
                product: "Claude Pro".into(),
                seller_chat: 111,
                mode: "batch".into(),
                quantity: 2,
                unit_price: "$20".into(),
                proxy_source: "buyer".into(),
                draft_proxies: vec!["http://u:p@1.1.1.1:80".into()],
            })
            .unwrap();
        }
        let s = Store::open(&p).unwrap();
        let state = s.get_admin_flow(999).unwrap().unwrap();
        assert_eq!(state.mode, "batch");
        assert_eq!(state.step, "batch_proxies");
        assert_eq!(state.draft_proxies, vec!["http://u:p@1.1.1.1:80"]);
        let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    }
}
