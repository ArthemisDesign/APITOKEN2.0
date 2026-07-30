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
    pub seller_chat: i64, // адресат оффера (0 = не задан)
}

#[derive(Clone, Debug)]
pub struct GeminiOAuthSession {
    pub state: String,
    pub chat_id: i64,
    pub sealed_payload: String,
    pub expires_ts: i64,
}

fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
                created_by INTEGER, ts INTEGER DEFAULT 0);
             CREATE TABLE IF NOT EXISTS responses(
                offer_id INTEGER, uid INTEGER, status TEXT DEFAULT '', address TEXT DEFAULT '',
                ts INTEGER DEFAULT 0, PRIMARY KEY(offer_id, uid));
             CREATE TABLE IF NOT EXISTS admin_state(
                chat_id INTEGER PRIMARY KEY, step TEXT, product TEXT DEFAULT '');
             CREATE TABLE IF NOT EXISTS gemini_oauth_sessions(
                state TEXT PRIMARY KEY,
                chat_id INTEGER NOT NULL UNIQUE,
                sealed_payload TEXT NOT NULL,
                expires_ts INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                ts INTEGER NOT NULL DEFAULT 0);",
        )?;
        let _ = c.execute("ALTER TABLE users ADD COLUMN hproxy TEXT DEFAULT ''", []); // мягкая миграция
                                                                                      // Legacy Developer-API builds added `hproject`. It is intentionally ignored: OAuth
                                                                                      // identity/project data now exists only inside the encrypted credential envelope.
        let _ = c.execute("ALTER TABLE users ADD COLUMN hproject TEXT DEFAULT ''", []);
        // IPRoyal order behind a bot-issued handover proxy, kept until official Gemini CLI OAuth
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
            "ALTER TABLE admin_state ADD COLUMN seller_chat INTEGER DEFAULT 0",
            [],
        ); // выбранный продавец
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
    ) -> Result<()> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1 OR expires_ts<?2",
            rusqlite::params![chat_id, now()],
        )?;
        tx.execute(
            "INSERT INTO gemini_oauth_sessions(state,chat_id,sealed_payload,expires_ts,status,ts)
             VALUES(?1,?2,?3,?4,'pending',?5)",
            rusqlite::params![state, chat_id, sealed_payload, expires_ts, now()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Claim an OAuth callback exactly once. A repeated callback cannot exchange the same code or
    /// race a second credential publication.
    pub fn claim_gemini_oauth(&self, state: &str) -> Result<Option<GeminiOAuthSession>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let session = tx
            .query_row(
                "SELECT state,chat_id,sealed_payload,expires_ts FROM gemini_oauth_sessions
                 WHERE state=?1 AND status='pending' AND expires_ts>=?2",
                rusqlite::params![state, now()],
                |row| {
                    Ok(GeminiOAuthSession {
                        state: row.get(0)?,
                        chat_id: row.get(1)?,
                        sealed_payload: row.get(2)?,
                        expires_ts: row.get(3)?,
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
    pub fn cancel_gemini_oauth(&self, chat_id: i64) -> Result<()> {
        let mut connection = self.c.lock().unwrap();
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![chat_id],
        )?;
        transaction.execute(
            "UPDATE users SET want='gm_gproxy', hproxy='' WHERE chat_id=?1",
            rusqlite::params![chat_id],
        )?;
        transaction.commit()?;
        Ok(())
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
        let c = self.c.lock().unwrap();
        c.execute(
            "INSERT INTO offers(product,price,created_by,seller_chat,ts) VALUES(?1,?2,?3,?4,?5)",
            rusqlite::params![product, price, by, seller_chat, now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn get_offer(&self, id: i64) -> Result<Option<Offer>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,product,price,created_by,COALESCE(seller_chat,0) FROM offers WHERE id=?1",
            rusqlite::params![id],
            |r| {
                Ok(Offer {
                    id: r.get(0)?,
                    product: r.get(1)?,
                    price: r.get(2)?,
                    created_by: r.get(3)?,
                    seller_chat: r.get(4)?,
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

    // ── машина создания оффера (persisted) ────────────────────────────────────
    pub fn get_admin_state(&self, chat: i64) -> Result<Option<(String, String, i64)>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT step,product,COALESCE(seller_chat,0) FROM admin_state WHERE chat_id=?1",
            rusqlite::params![chat],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?)
    }
    pub fn set_admin_state(
        &self,
        chat: i64,
        step: &str,
        product: &str,
        seller_chat: i64,
    ) -> Result<()> {
        self.c.lock().unwrap().execute(
            "INSERT INTO admin_state(chat_id,step,product,seller_chat) VALUES(?1,?2,?3,?4)
             ON CONFLICT(chat_id) DO UPDATE SET step=excluded.step, product=excluded.product, seller_chat=excluded.seller_chat",
            rusqlite::params![chat, step, product, seller_chat])?;
        Ok(())
    }
    pub fn clear_admin_state(&self, chat: i64) -> Result<bool> {
        let n = self.c.lock().unwrap().execute(
            "DELETE FROM admin_state WHERE chat_id=?1",
            rusqlite::params![chat],
        )?;
        Ok(n > 0)
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
        let dir = format!(
            "{}/authbot_test_{}",
            std::env::temp_dir().display(),
            std::process::id()
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
        s.start_gemini_oauth(222, "pending-state", "sealed", now() + 60)
            .unwrap();
        s.cancel_gemini_oauth(222).unwrap();
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
}
