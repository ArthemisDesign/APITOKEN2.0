//! Bounded response history used to implement `previous_response_id`.
//!
//! Local memory is the fast path. Optional Redis gives blue/green slots a shared state surface;
//! unlike cache affinity, this payload contains customer text, so Redis values are authenticated
//! and encrypted before leaving the process.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::{Mutex as AsyncMutex, RwLock};

const HISTORY_KEY_CONTEXT: &str = "claude-api/codex-history/encryption/v1";
const SCOPE_KEY_CONTEXT: &str = "claude-api/codex-history/scope/v1";
const REDIS_PREFIX: &str = "claude-api:codex-history:v1";
const MAX_SERIALIZED_HISTORY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StoredHistory {
    pub response_id: String,
    pub model: String,
    /// Exact Responses API items, including encrypted reasoning state where the provider emitted it.
    pub items: Vec<Value>,
    pub created_at: i64,
    /// How many leading `items` were the request input (the rest is this response's output).
    /// Absent in entries written before retrieval support; they fall back to returning all items.
    #[serde(default)]
    pub input_count: Option<usize>,
    /// The completed public response object, stored so `GET /v1/responses/{id}` can return the
    /// exact document the client originally received. Absent in older entries.
    #[serde(default)]
    pub response: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryError {
    NotFound,
    WrongTenant,
    TooLarge,
    Corrupt,
    Unavailable,
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::NotFound => "response history not found",
            Self::WrongTenant => "response history belongs to another tenant",
            Self::TooLarge => "response history exceeds the configured limit",
            Self::Corrupt => "response history failed integrity validation",
            Self::Unavailable => "response history store is unavailable",
        };
        f.write_str(message)
    }
}

impl std::error::Error for HistoryError {}

#[derive(Clone)]
struct LocalEntry {
    scope_tag: [u8; 32],
    history: StoredHistory,
    expires_at: i64,
}

#[derive(Serialize, Deserialize)]
struct RedisEnvelope {
    scope_tag: [u8; 32],
    history: StoredHistory,
}

/// Observable outcomes of the shared history store.
///
/// Unlike cache affinity, losing an entry here is visible to the customer: `prepare_turn` maps a
/// missing `previous_response_id` to a 400 that well-behaved clients treat as permanent, so they
/// discard a conversation the customer already paid to build. That failure was previously
/// unmeasured — the store had no counters at all and a degraded write only reached stderr.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryStats {
    pub local_hits: u64,
    pub redis_hits: u64,
    /// Reads that found nothing anywhere. With Redis configured this is the counter that turns
    /// into a customer-visible 400, so a rising rate means conversations are being dropped.
    pub misses: u64,
    pub redis_errors: u64,
    pub writes: u64,
    /// Writes that reached the local cache but never made it into the shared store. The turn still
    /// succeeds, so this is silent unless counted: the next request on another slot cannot resolve
    /// the conversation.
    pub write_failures: u64,
    pub wrong_tenant: u64,
    pub corrupt: u64,
}

#[derive(Default)]
struct Stats {
    local_hits: AtomicU64,
    redis_hits: AtomicU64,
    misses: AtomicU64,
    redis_errors: AtomicU64,
    writes: AtomicU64,
    write_failures: AtomicU64,
    wrong_tenant: AtomicU64,
    corrupt: AtomicU64,
}

/// Shared history store with lazy Redis connection setup. Redis loss never authorizes a cross-tenant
/// lookup: an unavailable shared store is reported distinctly from a genuine unknown id.
pub(crate) struct HistoryStore {
    encryption_key: [u8; 32],
    scope_key: [u8; 32],
    ttl_secs: u64,
    local_cap: usize,
    timeout: Duration,
    local: Mutex<HashMap<String, LocalEntry>>,
    client: Option<redis::Client>,
    connection: RwLock<Option<ConnectionManager>>,
    connect_lock: AsyncMutex<()>,
    redis_retry_after: AtomicI64,
    stats: Stats,
}

impl HistoryStore {
    pub(crate) fn new(
        redis_url: Option<&str>,
        secret: Option<&str>,
        ttl_secs: u64,
        local_cap: usize,
        timeout_ms: u64,
    ) -> anyhow::Result<Self> {
        let seed = match secret.filter(|value| !value.is_empty()) {
            Some(value) => value.as_bytes().to_vec(),
            None if redis_url.is_some() => {
                anyhow::bail!(
                    "Codex history encryption secret is required when shared Redis history is enabled"
                )
            }
            None => {
                let mut random = [0u8; 32];
                getrandom::fill(&mut random)
                    .map_err(|_| anyhow::anyhow!("operating-system CSPRNG unavailable"))?;
                random.to_vec()
            }
        };
        let client = redis_url
            .map(redis::Client::open)
            .transpose()
            .map_err(|_| anyhow::anyhow!("Codex history Redis URL is invalid"))?;
        Ok(Self {
            encryption_key: blake3::derive_key(HISTORY_KEY_CONTEXT, &seed),
            scope_key: blake3::derive_key(SCOPE_KEY_CONTEXT, &seed),
            // Allow up to the real API's 30-day response retention so an operator can match it;
            // the local cap still bounds in-memory growth independently of the TTL.
            ttl_secs: ttl_secs.clamp(60, 30 * 24 * 3600),
            local_cap: local_cap.clamp(16, 100_000),
            timeout: Duration::from_millis(timeout_ms.clamp(1, 5_000)),
            local: Mutex::new(HashMap::new()),
            client,
            connection: RwLock::new(None),
            connect_lock: AsyncMutex::new(()),
            redis_retry_after: AtomicI64::new(0),
            stats: Stats::default(),
        })
    }

    pub(crate) fn stats(&self) -> HistoryStats {
        HistoryStats {
            local_hits: self.stats.local_hits.load(Ordering::Relaxed),
            redis_hits: self.stats.redis_hits.load(Ordering::Relaxed),
            misses: self.stats.misses.load(Ordering::Relaxed),
            redis_errors: self.stats.redis_errors.load(Ordering::Relaxed),
            writes: self.stats.writes.load(Ordering::Relaxed),
            write_failures: self.stats.write_failures.load(Ordering::Relaxed),
            wrong_tenant: self.stats.wrong_tenant.load(Ordering::Relaxed),
            corrupt: self.stats.corrupt.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn redis_configured(&self) -> bool {
        self.client.is_some()
    }

    pub(crate) async fn put(
        &self,
        tenant_scope: &str,
        history: StoredHistory,
    ) -> Result<(), HistoryError> {
        let plaintext = serde_json::to_vec(&RedisEnvelope {
            scope_tag: self.scope_tag(tenant_scope),
            history: history.clone(),
        })
        .map_err(|_| HistoryError::Corrupt)?;
        if plaintext.len() > MAX_SERIALIZED_HISTORY_BYTES {
            return Err(HistoryError::TooLarge);
        }

        self.local_put(tenant_scope, history.clone());
        self.stats.writes.fetch_add(1, Ordering::Relaxed);
        let Some(mut connection) = self.connection().await.inspect_err(|_| {
            self.stats.write_failures.fetch_add(1, Ordering::Relaxed);
        })?
        else {
            return Ok(());
        };
        let ciphertext = self.encrypt(&history.response_id, &plaintext)?;
        let key = Self::redis_key(&history.response_id);
        let mut command = redis::cmd("SET");
        command
            .arg(key)
            .arg(ciphertext)
            .arg("EX")
            .arg(self.ttl_secs);
        let query = command.query_async::<()>(&mut connection);
        match tokio::time::timeout(self.timeout, query).await {
            Ok(Ok(())) => Ok(()),
            _ => {
                self.stats.redis_errors.fetch_add(1, Ordering::Relaxed);
                self.stats.write_failures.fetch_add(1, Ordering::Relaxed);
                self.reset_connection().await;
                Err(HistoryError::Unavailable)
            }
        }
    }

    pub(crate) async fn get(
        &self,
        tenant_scope: &str,
        response_id: &str,
    ) -> Result<StoredHistory, HistoryError> {
        if let Some(entry) = self.local_get(response_id) {
            if entry.scope_tag != self.scope_tag(tenant_scope) {
                self.stats.wrong_tenant.fetch_add(1, Ordering::Relaxed);
                return Err(HistoryError::WrongTenant);
            }
            self.stats.local_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(entry.history);
        }

        let Some(mut connection) = self.connection().await.inspect_err(|_| {
            self.stats.redis_errors.fetch_add(1, Ordering::Relaxed);
        })?
        else {
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
            return Err(HistoryError::NotFound);
        };
        let mut command = redis::cmd("GET");
        command.arg(Self::redis_key(response_id));
        let query = command.query_async::<Option<Vec<u8>>>(&mut connection);
        let encrypted = match tokio::time::timeout(self.timeout, query).await {
            Ok(Ok(value)) => value,
            _ => {
                self.stats.redis_errors.fetch_add(1, Ordering::Relaxed);
                self.reset_connection().await;
                return Err(HistoryError::Unavailable);
            }
        }
        .ok_or_else(|| {
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
            HistoryError::NotFound
        })?;
        let plaintext = self.decrypt(response_id, &encrypted).inspect_err(|_| {
            self.stats.corrupt.fetch_add(1, Ordering::Relaxed);
        })?;
        let envelope: RedisEnvelope = serde_json::from_slice(&plaintext).map_err(|_| {
            self.stats.corrupt.fetch_add(1, Ordering::Relaxed);
            HistoryError::Corrupt
        })?;
        if envelope.scope_tag != self.scope_tag(tenant_scope) {
            self.stats.wrong_tenant.fetch_add(1, Ordering::Relaxed);
            return Err(HistoryError::WrongTenant);
        }
        if envelope.history.response_id != response_id {
            self.stats.corrupt.fetch_add(1, Ordering::Relaxed);
            return Err(HistoryError::Corrupt);
        }
        self.stats.redis_hits.fetch_add(1, Ordering::Relaxed);
        self.local_put(tenant_scope, envelope.history.clone());
        Ok(envelope.history)
    }

    /// Removes a stored response for `store=true` deletion semantics. Tenant binding is enforced
    /// by requiring a successful scoped read first; deleting another tenant's id is impossible
    /// because the id cannot be read in the first place.
    pub(crate) async fn delete(
        &self,
        tenant_scope: &str,
        response_id: &str,
    ) -> Result<(), HistoryError> {
        self.get(tenant_scope, response_id).await?;
        {
            let mut local = self
                .local
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            local.remove(response_id);
        }
        let Some(mut connection) = self.connection().await.inspect_err(|_| {
            self.stats.redis_errors.fetch_add(1, Ordering::Relaxed);
        })?
        else {
            return Ok(());
        };
        let mut command = redis::cmd("DEL");
        command.arg(Self::redis_key(response_id));
        let query = command.query_async::<()>(&mut connection);
        match tokio::time::timeout(self.timeout, query).await {
            Ok(Ok(())) => Ok(()),
            _ => {
                self.stats.redis_errors.fetch_add(1, Ordering::Relaxed);
                self.reset_connection().await;
                Err(HistoryError::Unavailable)
            }
        }
    }

    fn local_put(&self, tenant_scope: &str, history: StoredHistory) {
        let now = pool::now();
        let mut local = self
            .local
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        local.retain(|_, entry| entry.expires_at > now);
        if local.len() >= self.local_cap && !local.contains_key(&history.response_id) {
            if let Some(oldest) = local
                .iter()
                .min_by_key(|(_, entry)| entry.expires_at)
                .map(|(id, _)| id.clone())
            {
                local.remove(&oldest);
            }
        }
        local.insert(
            history.response_id.clone(),
            LocalEntry {
                scope_tag: self.scope_tag(tenant_scope),
                history,
                expires_at: now.saturating_add(self.ttl_secs as i64),
            },
        );
    }

    fn local_get(&self, response_id: &str) -> Option<LocalEntry> {
        let now = pool::now();
        let mut local = self
            .local
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let expired = local
            .get(response_id)
            .is_some_and(|entry| entry.expires_at <= now);
        if expired {
            local.remove(response_id);
            return None;
        }
        local.get(response_id).cloned()
    }

    fn scope_tag(&self, tenant_scope: &str) -> [u8; 32] {
        *blake3::keyed_hash(&self.scope_key, tenant_scope.as_bytes()).as_bytes()
    }

    fn encrypt(&self, response_id: &str, plaintext: &[u8]) -> Result<Vec<u8>, HistoryError> {
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&self.encryption_key));
        let mut nonce = [0u8; 24];
        getrandom::fill(&mut nonce).map_err(|_| HistoryError::Unavailable)?;
        let mut output = nonce.to_vec();
        let encrypted = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: response_id.as_bytes(),
                },
            )
            .map_err(|_| HistoryError::Corrupt)?;
        output.extend_from_slice(&encrypted);
        Ok(output)
    }

    fn decrypt(&self, response_id: &str, ciphertext: &[u8]) -> Result<Vec<u8>, HistoryError> {
        let (nonce, encrypted) = ciphertext
            .split_at_checked(24)
            .ok_or(HistoryError::Corrupt)?;
        XChaCha20Poly1305::new(Key::from_slice(&self.encryption_key))
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: encrypted,
                    aad: response_id.as_bytes(),
                },
            )
            .map_err(|_| HistoryError::Corrupt)
    }

    fn redis_key(response_id: &str) -> String {
        format!("{REDIS_PREFIX}:{response_id}")
    }

    async fn connection(&self) -> Result<Option<ConnectionManager>, HistoryError> {
        let Some(client) = &self.client else {
            return Ok(None);
        };
        if self.redis_retry_after.load(Ordering::Relaxed) > pool::now() {
            return Err(HistoryError::Unavailable);
        }
        if let Some(connection) = self.connection.read().await.clone() {
            return Ok(Some(connection));
        }
        let _guard = self.connect_lock.lock().await;
        if let Some(connection) = self.connection.read().await.clone() {
            return Ok(Some(connection));
        }
        let manager =
            match tokio::time::timeout(self.timeout, client.get_connection_manager()).await {
                Ok(Ok(manager)) => manager,
                _ => {
                    self.redis_retry_after
                        .store(pool::now().saturating_add(1), Ordering::Relaxed);
                    return Err(HistoryError::Unavailable);
                }
            };
        self.redis_retry_after.store(0, Ordering::Relaxed);
        *self.connection.write().await = Some(manager.clone());
        Ok(Some(manager))
    }

    async fn reset_connection(&self) {
        *self.connection.write().await = None;
        self.redis_retry_after
            .store(pool::now().saturating_add(1), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> HistoryStore {
        HistoryStore::new(None, Some("test-only-secret"), 600, 32, 20).unwrap()
    }

    fn history(id: &str) -> StoredHistory {
        StoredHistory {
            response_id: id.to_string(),
            model: "gpt-5.6-sol".to_string(),
            items: vec![serde_json::json!({
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "secret"}]
            })],
            created_at: 1,
            input_count: Some(0),
            response: None,
        }
    }

    #[tokio::test]
    async fn local_history_is_tenant_bound() {
        let store = store();
        store.put("account-a", history("resp_1")).await.unwrap();
        assert_eq!(
            store.get("account-a", "resp_1").await.unwrap(),
            history("resp_1")
        );
        assert_eq!(
            store.get("account-b", "resp_1").await.unwrap_err(),
            HistoryError::WrongTenant
        );
    }

    #[tokio::test]
    async fn delete_removes_history_and_stays_tenant_bound() {
        let store = store();
        store.put("account-a", history("resp_1")).await.unwrap();
        assert_eq!(
            store.delete("account-b", "resp_1").await.unwrap_err(),
            HistoryError::WrongTenant
        );
        store.delete("account-a", "resp_1").await.unwrap();
        assert_eq!(
            store.get("account-a", "resp_1").await.unwrap_err(),
            HistoryError::NotFound
        );
        assert_eq!(
            store.delete("account-a", "resp_1").await.unwrap_err(),
            HistoryError::NotFound
        );
    }

    #[test]
    fn older_entries_without_retrieval_fields_still_deserialize() {
        let store = store();
        let legacy = br#"{
            "scope_tag": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
            "history": {
                "response_id": "resp_old",
                "model": "gpt-5.6-sol",
                "items": [],
                "created_at": 1
            }
        }"#;
        let envelope: RedisEnvelope = serde_json::from_slice(legacy).unwrap();
        assert_eq!(envelope.history.input_count, None);
        assert_eq!(envelope.history.response, None);
        let _ = store;
    }

    #[test]
    fn encrypted_envelope_is_bound_to_response_id() {
        let store = store();
        let plaintext = br#"{"hello":"world"}"#;
        let encrypted = store.encrypt("resp_a", plaintext).unwrap();
        assert_eq!(store.decrypt("resp_a", &encrypted).unwrap(), plaintext);
        assert_eq!(
            store.decrypt("resp_b", &encrypted).unwrap_err(),
            HistoryError::Corrupt
        );
        assert!(!encrypted.windows(5).any(|window| window == b"hello"));
    }

    /// Losing a stored response is reported to the client as a 400 that well-behaved clients treat
    /// as permanent, so they discard a conversation the customer already paid for. Before these
    /// counters existed that failure was invisible: the store had no metrics and a degraded write
    /// only reached stderr. Pin each outcome to its own counter so an operator can tell a genuine
    /// unknown id apart from eviction or a broken shared store.
    #[tokio::test]
    async fn history_outcomes_are_counted_separately() {
        let store = store();
        assert_eq!(store.stats(), HistoryStats::default());

        store.put("account-a", history("resp_1")).await.unwrap();
        assert_eq!(store.stats().writes, 1);
        // No Redis is configured, so a local-only write is complete rather than degraded.
        assert_eq!(store.stats().write_failures, 0);

        store.get("account-a", "resp_1").await.unwrap();
        assert_eq!(store.stats().local_hits, 1);

        assert_eq!(
            store.get("account-b", "resp_1").await.unwrap_err(),
            HistoryError::WrongTenant
        );
        assert_eq!(store.stats().wrong_tenant, 1);
        assert_eq!(
            store.stats().local_hits,
            1,
            "a cross-tenant read must never count as a hit"
        );

        assert_eq!(
            store.get("account-a", "resp_missing").await.unwrap_err(),
            HistoryError::NotFound
        );
        assert_eq!(
            store.stats().misses,
            1,
            "an unknown id is the counter that becomes a customer-visible 400"
        );
        assert_eq!(store.stats().redis_errors, 0);
        assert_eq!(store.stats().corrupt, 0);
    }

    /// A shared store that cannot be reached must be distinguishable from an unknown id. Conflating
    /// them is what turns a Redis outage into "your conversation does not exist" for every client
    /// mid-conversation, which is precisely what happens during a routine blue/green deploy.
    #[tokio::test]
    async fn unreachable_shared_store_is_counted_as_error_not_miss() {
        let store = HistoryStore::new(
            Some("redis://127.0.0.1:1/"),
            Some("outage-test-secret"),
            600,
            32,
            5,
        )
        .unwrap();
        assert!(store.redis_configured());

        // The write still lands locally so the turn itself succeeds, but the shared copy is lost.
        assert_eq!(
            store.put("account-a", history("resp_1")).await.unwrap_err(),
            HistoryError::Unavailable
        );
        assert_eq!(store.stats().writes, 1);
        assert_eq!(store.stats().write_failures, 1);

        // A local hit must not consult Redis at all.
        store.get("account-a", "resp_1").await.unwrap();
        assert_eq!(store.stats().local_hits, 1);

        let errors_before = store.stats().redis_errors;
        assert_eq!(
            store.get("account-a", "resp_absent").await.unwrap_err(),
            HistoryError::Unavailable,
            "an unreachable store must not be reported as a genuine unknown id"
        );
        assert!(store.stats().redis_errors > errors_before);
        assert_eq!(
            store.stats().misses,
            0,
            "an outage must never be counted as a miss"
        );
    }
}
