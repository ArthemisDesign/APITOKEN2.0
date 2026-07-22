//! Automatic cache-lineage affinity for transparent Anthropic-compatible clients.
//!
//! This layer deliberately remains an optimization: local memory is the L1 and Redis is an
//! optional shared L2. Loss, eviction, or unavailability may reduce prompt-cache hits but can never
//! authorize money or subscription capacity. PostgreSQL capacity leases remain the final gate.

use axum::http::HeaderMap;
use redis::aio::ConnectionManager;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::{Mutex as AsyncMutex, RwLock};

const HASH_CONTEXT: &str = "claude-api/cache-affinity/v1";
const REDIS_PREFIX: &str = "claude-api:aff:v1";
const DEFAULT_LOCAL_CAP: usize = 100_000;
const CACHE_ROOT_MIN_BYTES: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AffinitySource {
    Native,
    Transcript,
    CacheRoot,
    New,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AliasKind {
    Native,
    Transcript,
    CacheRoot,
}

#[derive(Clone, Debug)]
struct Alias {
    digest: String,
    kind: AliasKind,
}

/// Opaque request-derived identity. It contains only keyed digests, never customer content or IDs.
#[derive(Clone, Debug)]
pub struct AffinityInput {
    account_tag: String,
    aliases: Vec<Alias>,
    pub cacheable_bytes: usize,
}

impl AffinityInput {
    fn primary(&self) -> &Alias {
        // Construction requires at least one message and therefore at least one transcript alias.
        &self.aliases[0]
    }
}

#[derive(Clone, Debug)]
pub struct AffinityResolution {
    pub session_id: String,
    pub home: String,
    pub source: AffinitySource,
    account_tag: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AffinityStats {
    pub local_hits: u64,
    pub redis_hits: u64,
    pub misses: u64,
    pub redis_errors: u64,
    pub native_hits: u64,
    pub transcript_hits: u64,
    pub cache_root_hits: u64,
    pub claims: u64,
    pub rebinds: u64,
}

#[derive(Default)]
struct Stats {
    local_hits: AtomicU64,
    redis_hits: AtomicU64,
    misses: AtomicU64,
    redis_errors: AtomicU64,
    native_hits: AtomicU64,
    transcript_hits: AtomicU64,
    cache_root_hits: AtomicU64,
    claims: AtomicU64,
    rebinds: AtomicU64,
}

#[derive(Clone)]
struct LocalAlias {
    session_id: String,
    expires_at: i64,
}

#[derive(Clone)]
struct LocalSession {
    home: String,
    expires_at: i64,
}

#[derive(Default)]
struct LocalState {
    aliases: HashMap<String, LocalAlias>,
    sessions: HashMap<String, LocalSession>,
}

/// Shared, fail-open affinity store. Redis connection creation is lazy so an unavailable Redis at
/// process start never prevents engine readiness; later requests automatically retry connection.
pub struct AffinityStore {
    key: [u8; 32],
    ttl_secs: u64,
    local_ttl_secs: i64,
    timeout: Duration,
    local_cap: usize,
    local: Mutex<LocalState>,
    client: Option<redis::Client>,
    connection: RwLock<Option<ConnectionManager>>,
    connect_lock: AsyncMutex<()>,
    redis_retry_after: AtomicI64,
    stats: Stats,
}

impl AffinityStore {
    pub fn new(
        redis_url: Option<&str>,
        secret: Option<&str>,
        ttl_secs: u64,
        local_ttl_secs: u64,
        timeout_ms: u64,
    ) -> anyhow::Result<Self> {
        let key = match secret.filter(|value| !value.is_empty()) {
            Some(value) => blake3::derive_key(HASH_CONTEXT, value.as_bytes()),
            None if redis_url.is_some() => {
                anyhow::bail!(
                    "CLAUDE_API_AFFINITY_SECRET is required when Redis affinity is enabled"
                )
            }
            None => {
                let mut random = [0u8; 32];
                getrandom::fill(&mut random)
                    .map_err(|_| anyhow::anyhow!("operating-system CSPRNG unavailable"))?;
                random
            }
        };
        let client = match redis_url {
            Some(url) => Some(
                redis::Client::open(url)
                    .map_err(|_| anyhow::anyhow!("CLAUDE_API_REDIS_URL is invalid"))?,
            ),
            None => None,
        };
        Ok(Self {
            key,
            ttl_secs: ttl_secs.clamp(60, 24 * 3600),
            local_ttl_secs: local_ttl_secs.clamp(1, 3600) as i64,
            timeout: Duration::from_millis(timeout_ms.clamp(1, 500)),
            local_cap: DEFAULT_LOCAL_CAP,
            local: Mutex::new(LocalState::default()),
            client,
            connection: RwLock::new(None),
            connect_lock: AsyncMutex::new(()),
            redis_retry_after: AtomicI64::new(0),
            stats: Stats::default(),
        })
    }

    pub fn redis_configured(&self) -> bool {
        self.client.is_some()
    }

    /// Opaque stable identifier for a subscription home. Redis never receives the raw registry ID.
    pub fn home_id(&self, subscription_id: &str) -> String {
        self.digest_parts(b"home", &[subscription_id.as_bytes()])
    }

    /// Infer a cache lineage without any client opt-in. Native harness IDs take precedence; normal
    /// API clients are recognized by rolling hashes of their canonical message-history prefixes.
    pub fn infer(
        &self,
        account_scope: &str,
        headers: &HeaderMap,
        body: &Value,
    ) -> Option<AffinityInput> {
        let messages = body.get("messages")?.as_array()?;
        if messages.is_empty() {
            return None;
        }

        let account_tag = self.digest_parts(b"account", &[account_scope.as_bytes()]);
        let mut aliases = Vec::with_capacity(messages.len() + 1);
        for header in [
            "x-claude-code-session-id",
            "x-conversation-id",
            "x-session-id",
        ] {
            let Some(value) = headers.get(header).and_then(|value| value.to_str().ok()) else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() || value.len() > 512 {
                continue;
            }
            aliases.push(Alias {
                digest: self.digest_parts(b"native", &[header.as_bytes(), value.as_bytes()]),
                kind: AliasKind::Native,
            });
            break;
        }

        let mut base = blake3::Hasher::new_keyed(&self.key);
        base.update(b"cache-base-v1");
        for field in ["model", "system", "tools", "thinking", "context_management"] {
            base.update(field.as_bytes());
            base.update(&[0]);
            match body.get(field) {
                Some(value) => feed_canonical_json(&mut base, value),
                None => {
                    base.update(b"missing");
                }
            };
            base.update(&[0xff]);
        }
        let base_digest = *base.finalize().as_bytes();
        let mut chain = base_digest;
        let mut transcript = Vec::with_capacity(messages.len());
        let mut cacheable_bytes = 0usize;
        for message in messages {
            let mut next = blake3::Hasher::new_keyed(&self.key);
            next.update(b"message-chain-v1");
            next.update(&chain);
            feed_canonical_json(&mut next, message);
            chain = *next.finalize().as_bytes();
            transcript.push(Alias {
                digest: hex_digest(&chain),
                kind: AliasKind::Transcript,
            });
            cacheable_bytes = cacheable_bytes
                .saturating_add(serde_json::to_vec(message).map(|v| v.len()).unwrap_or(0));
        }
        let root_cacheable_bytes = ["system", "tools"]
            .iter()
            .filter_map(|field| body.get(*field))
            .filter_map(|value| serde_json::to_vec(value).ok())
            .map(|value| value.len())
            .sum::<usize>();
        cacheable_bytes = cacheable_bytes.saturating_add(root_cacheable_bytes);
        // Longest transcript first: exact retries match immediately; continuations fall back to the
        // deepest prefix observed on the preceding request. Native remains the highest-confidence key.
        transcript.reverse();
        aliases.extend(transcript);
        let explicit_root = ["system", "tools"]
            .iter()
            .filter_map(|field| body.get(*field))
            .any(contains_cache_control);
        if root_cacheable_bytes >= CACHE_ROOT_MIN_BYTES || explicit_root {
            aliases.push(Alias {
                digest: hex_digest(&base_digest),
                kind: AliasKind::CacheRoot,
            });
        }
        Some(AffinityInput {
            account_tag,
            aliases,
            cacheable_bytes,
        })
    }

    pub async fn resolve(&self, input: &AffinityInput) -> Option<AffinityResolution> {
        if let Some(found) = self.local_resolve(input) {
            self.stats.local_hits.fetch_add(1, Ordering::Relaxed);
            self.record_source(found.source);
            return Some(found);
        }
        match self.redis_resolve(input).await {
            Ok(Some(found)) => {
                self.stats.redis_hits.fetch_add(1, Ordering::Relaxed);
                self.record_source(found.source);
                self.local_store_resolution(input, &found, true);
                Some(found)
            }
            Ok(None) => {
                self.stats.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
            Err(()) => {
                self.stats.redis_errors.fetch_add(1, Ordering::Relaxed);
                self.stats.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Claim a new affinity. Redis races are resolved with SET NX; the winner's home is returned.
    /// Local storage is updated first, so Redis failure still provides per-process stickiness.
    pub async fn claim(&self, input: &AffinityInput, proposed_home: &str) -> AffinityResolution {
        if let Some(found) = self.local_resolve(input) {
            return found;
        }
        let session_id = input.primary().digest.clone();
        let mut resolution = AffinityResolution {
            session_id: session_id.clone(),
            home: proposed_home.to_string(),
            source: AffinitySource::New,
            account_tag: input.account_tag.clone(),
        };
        self.local_store_resolution(input, &resolution, false);

        match self.redis_claim(input, &session_id, proposed_home).await {
            Ok(Some(winner)) => {
                resolution = winner;
                self.local_store_resolution(input, &resolution, true);
            }
            Ok(None) => {}
            Err(()) => {
                self.stats.redis_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.stats.claims.fetch_add(1, Ordering::Relaxed);
        resolution
    }

    /// Add every newly observed transcript prefix to an existing affinity. This happens from the
    /// incoming request, so response bytes never need buffering beyond the existing usage meter.
    pub async fn remember(&self, input: &AffinityInput, resolution: &AffinityResolution) {
        self.local_store_resolution(input, resolution, false);
        if self.redis_remember(input, resolution).await.is_err() {
            self.stats.redis_errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Compare-and-set the preferred home. A failed Redis update is harmless: the local process uses
    /// the new home and a later request revalidates any stale shared preference against PostgreSQL.
    pub async fn rebind(&self, resolution: &mut AffinityResolution, new_home: &str) {
        if resolution.home == new_home {
            return;
        }
        let old_home = resolution.home.clone();
        resolution.home = new_home.to_string();
        self.local_rebind(resolution, &old_home);
        match self.redis_rebind(resolution, &old_home).await {
            Ok(Some(shared_home)) if shared_home != resolution.home => {
                let attempted_home = resolution.home.clone();
                resolution.home = shared_home;
                self.local_rebind(resolution, &attempted_home);
            }
            Ok(_) => {}
            Err(()) => {
                self.stats.redis_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.stats.rebinds.fetch_add(1, Ordering::Relaxed);
    }

    pub fn stats(&self) -> AffinityStats {
        AffinityStats {
            local_hits: self.stats.local_hits.load(Ordering::Relaxed),
            redis_hits: self.stats.redis_hits.load(Ordering::Relaxed),
            misses: self.stats.misses.load(Ordering::Relaxed),
            redis_errors: self.stats.redis_errors.load(Ordering::Relaxed),
            native_hits: self.stats.native_hits.load(Ordering::Relaxed),
            transcript_hits: self.stats.transcript_hits.load(Ordering::Relaxed),
            cache_root_hits: self.stats.cache_root_hits.load(Ordering::Relaxed),
            claims: self.stats.claims.load(Ordering::Relaxed),
            rebinds: self.stats.rebinds.load(Ordering::Relaxed),
        }
    }

    fn record_source(&self, source: AffinitySource) {
        match source {
            AffinitySource::Native => {
                self.stats.native_hits.fetch_add(1, Ordering::Relaxed);
            }
            AffinitySource::Transcript => {
                self.stats.transcript_hits.fetch_add(1, Ordering::Relaxed);
            }
            AffinitySource::CacheRoot => {
                self.stats.cache_root_hits.fetch_add(1, Ordering::Relaxed);
            }
            AffinitySource::New => {}
        }
    }

    fn local_key(account_tag: &str, value: &str) -> String {
        format!("{account_tag}:{value}")
    }

    fn local_resolve(&self, input: &AffinityInput) -> Option<AffinityResolution> {
        let now = pool::now();
        let mut state = self.local.lock().unwrap_or_else(|error| error.into_inner());
        for alias in &input.aliases {
            let key = Self::local_key(&input.account_tag, &alias.digest);
            let Some(binding) = state.aliases.get(&key).cloned() else {
                continue;
            };
            if binding.expires_at <= now {
                state.aliases.remove(&key);
                continue;
            }
            let session_key = Self::local_key(&input.account_tag, &binding.session_id);
            let Some(session) = state.sessions.get(&session_key).cloned() else {
                state.aliases.remove(&key);
                continue;
            };
            if session.expires_at <= now {
                state.sessions.remove(&session_key);
                state.aliases.remove(&key);
                continue;
            }
            return Some(AffinityResolution {
                // A cache root is shared placement knowledge, not a conversation identity. Fork a
                // fresh lineage immediately so later rebinds never drag unrelated conversations.
                session_id: if alias.kind == AliasKind::CacheRoot {
                    input.primary().digest.clone()
                } else {
                    binding.session_id
                },
                home: session.home,
                source: source_for(alias.kind),
                account_tag: input.account_tag.clone(),
            });
        }
        None
    }

    fn local_store_resolution(
        &self,
        input: &AffinityInput,
        resolution: &AffinityResolution,
        overwrite_transcript: bool,
    ) {
        let expires_at = pool::now().saturating_add(self.local_ttl_secs);
        let mut state = self.local.lock().unwrap_or_else(|error| error.into_inner());
        if state.aliases.len() >= self.local_cap || state.sessions.len() >= self.local_cap {
            let now = pool::now();
            state.aliases.retain(|_, value| value.expires_at > now);
            state.sessions.retain(|_, value| value.expires_at > now);
            if state.aliases.len() >= self.local_cap {
                let remove = state.aliases.len() / 4 + 1;
                let keys: Vec<String> = state.aliases.keys().take(remove).cloned().collect();
                for key in keys {
                    state.aliases.remove(&key);
                }
            }
            if state.sessions.len() >= self.local_cap {
                let remove = state.sessions.len() / 4 + 1;
                let keys: Vec<String> = state.sessions.keys().take(remove).cloned().collect();
                for key in keys {
                    state.sessions.remove(&key);
                }
            }
        }
        let session_key = Self::local_key(&input.account_tag, &resolution.session_id);
        let session = LocalSession {
            home: resolution.home.clone(),
            expires_at,
        };
        if overwrite_transcript {
            state.sessions.insert(session_key, session);
        } else {
            state
                .sessions
                .entry(session_key)
                .and_modify(|existing| {
                    if existing.home == resolution.home {
                        existing.expires_at = expires_at;
                    }
                })
                .or_insert(session);
        }
        for alias in &input.aliases {
            let key = Self::local_key(&input.account_tag, &alias.digest);
            let binding = LocalAlias {
                session_id: resolution.session_id.clone(),
                expires_at,
            };
            if alias.kind == AliasKind::Native || overwrite_transcript {
                state.aliases.insert(key, binding);
            } else {
                state.aliases.entry(key).or_insert(binding);
            }
        }
    }

    fn local_rebind(&self, resolution: &AffinityResolution, old_home: &str) {
        let key = Self::local_key(&resolution.account_tag, &resolution.session_id);
        let mut state = self.local.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(session) = state.sessions.get_mut(&key) {
            if session.home == old_home {
                session.home = resolution.home.clone();
                session.expires_at = pool::now().saturating_add(self.local_ttl_secs);
            }
        }
    }

    fn redis_alias_key(account_tag: &str, alias: &str) -> String {
        format!("{REDIS_PREFIX}:{{{account_tag}}}:a:{alias}")
    }

    fn redis_session_key(account_tag: &str, session: &str) -> String {
        format!("{REDIS_PREFIX}:{{{account_tag}}}:s:{session}")
    }

    async fn connection(&self) -> Result<Option<ConnectionManager>, ()> {
        let Some(client) = &self.client else {
            return Ok(None);
        };
        if self.redis_retry_after.load(Ordering::Relaxed) > pool::now() {
            return Err(());
        }
        if let Some(connection) = self.connection.read().await.clone() {
            return Ok(Some(connection));
        }
        let _guard = self.connect_lock.lock().await;
        if self.redis_retry_after.load(Ordering::Relaxed) > pool::now() {
            return Err(());
        }
        if let Some(connection) = self.connection.read().await.clone() {
            return Ok(Some(connection));
        }
        let manager =
            match tokio::time::timeout(self.timeout, client.get_connection_manager()).await {
                Ok(Ok(manager)) => manager,
                _ => {
                    self.redis_retry_after
                        .store(pool::now().saturating_add(1), Ordering::Relaxed);
                    return Err(());
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

    async fn redis_resolve(&self, input: &AffinityInput) -> Result<Option<AffinityResolution>, ()> {
        let Some(mut connection) = self.connection().await? else {
            return Ok(None);
        };
        let keys: Vec<String> = input
            .aliases
            .iter()
            .map(|alias| Self::redis_alias_key(&input.account_tag, &alias.digest))
            .collect();
        let query = async {
            let values: Vec<Option<String>> = redis::cmd("MGET")
                .arg(&keys)
                .query_async(&mut connection)
                .await
                .map_err(|_| ())?;
            for (index, session_id) in values.into_iter().enumerate() {
                let Some(session_id) = session_id else {
                    continue;
                };
                let home: Option<String> = redis::cmd("GET")
                    .arg(Self::redis_session_key(&input.account_tag, &session_id))
                    .query_async(&mut connection)
                    .await
                    .map_err(|_| ())?;
                if let Some(home) = home.filter(|home| !home.is_empty()) {
                    return Ok(Some(AffinityResolution {
                        session_id: if input.aliases[index].kind == AliasKind::CacheRoot {
                            input.primary().digest.clone()
                        } else {
                            session_id
                        },
                        home,
                        source: source_for(input.aliases[index].kind),
                        account_tag: input.account_tag.clone(),
                    }));
                }
            }
            Ok(None)
        };
        match tokio::time::timeout(self.timeout, query).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(())) => {
                self.reset_connection().await;
                Err(())
            }
            Err(_) => {
                self.reset_connection().await;
                Err(())
            }
        }
    }

    async fn redis_claim(
        &self,
        input: &AffinityInput,
        session_id: &str,
        proposed_home: &str,
    ) -> Result<Option<AffinityResolution>, ()> {
        let Some(mut connection) = self.connection().await? else {
            return Ok(None);
        };
        let primary = input.primary();
        let session_key = Self::redis_session_key(&input.account_tag, session_id);
        let alias_key = Self::redis_alias_key(&input.account_tag, &primary.digest);
        let ttl = self.ttl_secs;
        let query = async {
            // Both keys share the account hash tag. The script publishes alias+home atomically;
            // concurrent engines either observe the prior winner or install exactly one proposal.
            let script = redis::Script::new(
                "local existing = redis.call('GET', KEYS[1]); \
                 if existing then return existing; end; \
                 redis.call('SET', KEYS[2], ARGV[2], 'NX', 'EX', ARGV[3]); \
                 redis.call('EXPIRE', KEYS[2], ARGV[3]); \
                 redis.call('SET', KEYS[1], ARGV[1], 'EX', ARGV[3]); \
                 return ARGV[1]",
            );
            let winner: String = script
                .key(&alias_key)
                .key(&session_key)
                .arg(session_id)
                .arg(proposed_home)
                .arg(ttl)
                .invoke_async(&mut connection)
                .await
                .map_err(|_| ())?;
            let winner_key = Self::redis_session_key(&input.account_tag, &winner);
            let home: Option<String> = redis::cmd("GET")
                .arg(&winner_key)
                .query_async(&mut connection)
                .await
                .map_err(|_| ())?;
            if home.is_none() {
                let cleanup = redis::Script::new(
                    "if redis.call('GET', KEYS[1]) == ARGV[1] then \
                       return redis.call('DEL', KEYS[1]); \
                     end; return 0",
                );
                cleanup
                    .key(&alias_key)
                    .arg(&winner)
                    .invoke_async::<i64>(&mut connection)
                    .await
                    .map_err(|_| ())?;
                return Ok(None);
            }
            Ok(home.map(|home| AffinityResolution {
                session_id: winner,
                home,
                source: if primary.kind == AliasKind::Native {
                    AffinitySource::Native
                } else {
                    AffinitySource::New
                },
                account_tag: input.account_tag.clone(),
            }))
        };
        let result = match tokio::time::timeout(self.timeout, query).await {
            Ok(result) => result,
            Err(_) => {
                self.reset_connection().await;
                return Err(());
            }
        };
        if result.is_err() {
            self.reset_connection().await;
        }
        let winner = result?;
        if let Some(winner) = &winner {
            let _ = self.redis_remember(input, winner).await;
        }
        Ok(winner)
    }

    async fn redis_remember(
        &self,
        input: &AffinityInput,
        resolution: &AffinityResolution,
    ) -> Result<(), ()> {
        let Some(mut connection) = self.connection().await? else {
            return Ok(());
        };
        let ttl = self.ttl_secs;
        let query = async {
            let session_key = Self::redis_session_key(&input.account_tag, &resolution.session_id);
            let _: Option<String> = redis::cmd("SET")
                .arg(&session_key)
                .arg(&resolution.home)
                .arg("NX")
                .arg("EX")
                .arg(ttl)
                .query_async(&mut connection)
                .await
                .map_err(|_| ())?;
            let _: i64 = redis::cmd("EXPIRE")
                .arg(&session_key)
                .arg(ttl)
                .query_async(&mut connection)
                .await
                .map_err(|_| ())?;
            let mut pipe = redis::pipe();
            for alias in &input.aliases {
                let key = Self::redis_alias_key(&input.account_tag, &alias.digest);
                if alias.kind == AliasKind::Native {
                    pipe.cmd("SET")
                        .arg(key)
                        .arg(&resolution.session_id)
                        .arg("EX")
                        .arg(ttl)
                        .ignore();
                } else {
                    pipe.cmd("SET")
                        .arg(key)
                        .arg(&resolution.session_id)
                        .arg("NX")
                        .arg("EX")
                        .arg(ttl)
                        .ignore();
                }
            }
            pipe.query_async::<()>(&mut connection)
                .await
                .map_err(|_| ())?;
            Ok(())
        };
        let result = match tokio::time::timeout(self.timeout, query).await {
            Ok(result) => result,
            Err(_) => {
                self.reset_connection().await;
                return Err(());
            }
        };
        if result.is_err() {
            self.reset_connection().await;
        }
        result
    }

    async fn redis_rebind(
        &self,
        resolution: &AffinityResolution,
        old_home: &str,
    ) -> Result<Option<String>, ()> {
        let Some(mut connection) = self.connection().await? else {
            return Ok(None);
        };
        let script = redis::Script::new(
            "local current = redis.call('GET', KEYS[1]); \
             if (not current) or current == ARGV[1] then \
               redis.call('SET', KEYS[1], ARGV[2], 'EX', ARGV[3]); return ARGV[2]; \
             end; return current",
        );
        let mut invocation = script.prepare_invoke();
        invocation
            .key(Self::redis_session_key(
                &resolution.account_tag,
                &resolution.session_id,
            ))
            .arg(old_home)
            .arg(&resolution.home)
            .arg(self.ttl_secs);
        match tokio::time::timeout(
            self.timeout,
            invocation.invoke_async::<String>(&mut connection),
        )
        .await
        {
            Ok(Ok(home)) => Ok(Some(home)),
            _ => {
                self.reset_connection().await;
                Err(())
            }
        }
    }

    fn digest_parts(&self, domain: &[u8], parts: &[&[u8]]) -> String {
        let mut hasher = blake3::Hasher::new_keyed(&self.key);
        hasher.update(domain);
        for part in parts {
            hasher.update(&(part.len() as u64).to_le_bytes());
            hasher.update(part);
        }
        hex_digest(hasher.finalize().as_bytes())
    }
}

fn source_for(kind: AliasKind) -> AffinitySource {
    match kind {
        AliasKind::Native => AffinitySource::Native,
        AliasKind::Transcript => AffinitySource::Transcript,
        AliasKind::CacheRoot => AffinitySource::CacheRoot,
    }
}

fn contains_cache_control(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(contains_cache_control),
        Value::Object(values) => {
            values.contains_key("cache_control") || values.values().any(contains_cache_control)
        }
        _ => false,
    }
}

fn hex_digest(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Stable JSON encoding independent of map insertion/serialization order. Arrays remain ordered.
fn feed_canonical_json(hasher: &mut blake3::Hasher, value: &Value) {
    match value {
        Value::Null => {
            hasher.update(b"n");
        }
        Value::Bool(value) => {
            hasher.update(b"b");
            hasher.update(&[*value as u8]);
        }
        Value::Number(value) => {
            hasher.update(b"d");
            hasher.update(value.to_string().as_bytes());
        }
        Value::String(value) => {
            hasher.update(b"s");
            hasher.update(&(value.len() as u64).to_le_bytes());
            hasher.update(value.as_bytes());
        }
        Value::Array(values) => {
            hasher.update(b"[");
            hasher.update(&(values.len() as u64).to_le_bytes());
            for value in values {
                feed_canonical_json(hasher, value);
            }
            hasher.update(b"]");
        }
        Value::Object(values) => {
            hasher.update(b"{");
            hasher.update(&(values.len() as u64).to_le_bytes());
            let mut keys: Vec<&String> = values.keys().collect();
            keys.sort_unstable();
            for key in keys {
                hasher.update(&(key.len() as u64).to_le_bytes());
                hasher.update(key.as_bytes());
                feed_canonical_json(hasher, &values[key]);
            }
            hasher.update(b"}");
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use serde_json::json;

    fn store() -> AffinityStore {
        AffinityStore::new(
            None,
            Some("test-secret-at-least-32-characters"),
            3600,
            3600,
            20,
        )
        .unwrap()
    }

    #[test]
    fn transcript_prefix_survives_appended_turns() {
        let store = store();
        let headers = HeaderMap::new();
        let first = json!({
            "model":"claude-test", "system":"stable", "tools":[{"name":"lookup"}],
            "messages":[{"role":"user","content":"first"}]
        });
        let second = json!({
            "model":"claude-test", "system":"stable", "tools":[{"name":"lookup"}],
            "messages":[
                {"role":"user","content":"first"},
                {"role":"assistant","content":"answer"},
                {"role":"user","content":"second"}
            ]
        });
        let a = store.infer("acct-1", &headers, &first).unwrap();
        let b = store.infer("acct-1", &headers, &second).unwrap();
        assert_eq!(
            a.aliases.last().unwrap().digest,
            b.aliases.last().unwrap().digest
        );
        assert_ne!(a.aliases[0].digest, b.aliases[0].digest);
    }

    #[test]
    fn account_and_cache_shape_namespace_transcripts() {
        let store = store();
        let headers = HeaderMap::new();
        let a = json!({"model":"a","messages":[{"role":"user","content":"same"}]});
        let b = json!({"model":"b","messages":[{"role":"user","content":"same"}]});
        let a1 = store.infer("acct-1", &headers, &a).unwrap();
        let a2 = store.infer("acct-2", &headers, &a).unwrap();
        let b1 = store.infer("acct-1", &headers, &b).unwrap();
        assert_ne!(a1.account_tag, a2.account_tag);
        assert_ne!(a1.primary().digest, b1.primary().digest);
    }

    #[tokio::test]
    async fn explicit_cache_root_warms_new_conversation_then_forks_lineage() {
        let store = store();
        let headers = HeaderMap::new();
        let body = |message: &str| {
            json!({
                "model":"claude-test",
                "system":[{"type":"text","text":"shared root","cache_control":{"type":"ephemeral"}}],
                "messages":[{"role":"user","content":message}]
            })
        };
        let first = store.infer("acct", &headers, &body("one")).unwrap();
        let first_resolution = store.claim(&first, "opaque-home").await;

        let second = store.infer("acct", &headers, &body("two")).unwrap();
        let root_resolution = store.resolve(&second).await.unwrap();
        assert_eq!(root_resolution.source, AffinitySource::CacheRoot);
        assert_eq!(root_resolution.home, first_resolution.home);
        assert_ne!(root_resolution.session_id, first_resolution.session_id);

        store.remember(&second, &root_resolution).await;
        let continued = store.resolve(&second).await.unwrap();
        assert_eq!(continued.source, AffinitySource::Transcript);
        assert_eq!(continued.session_id, root_resolution.session_id);
    }

    #[test]
    fn native_session_has_priority_without_exposing_raw_value() {
        let store = store();
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-claude-code-session-id",
            HeaderValue::from_static("raw-private-session"),
        );
        let input = store
            .infer(
                "acct",
                &headers,
                &json!({"messages":[{"role":"user","content":"hi"}]}),
            )
            .unwrap();
        assert_eq!(input.primary().kind, AliasKind::Native);
        assert!(!input.primary().digest.contains("raw-private-session"));
        assert!(!store
            .home_id("private-subscription@example.test")
            .contains("private"));
    }

    #[test]
    fn redis_requires_stable_keyed_hash_secret() {
        assert!(AffinityStore::new(Some("redis://127.0.0.1/"), None, 3600, 60, 20).is_err());
    }

    #[tokio::test]
    async fn local_store_claims_remembers_and_rebinds() {
        let store = store();
        let headers = HeaderMap::new();
        let first = store
            .infer(
                "acct",
                &headers,
                &json!({"messages":[{"role":"user","content":"one"}]}),
            )
            .unwrap();
        let mut resolution = store.claim(&first, "sub-a").await;
        assert_eq!(resolution.home, "sub-a");
        assert_eq!(store.resolve(&first).await.unwrap().home, "sub-a");

        let continued = store
            .infer(
                "acct",
                &headers,
                &json!({"messages":[
                    {"role":"user","content":"one"},
                    {"role":"assistant","content":"answer"},
                    {"role":"user","content":"two"}
                ]}),
            )
            .unwrap();
        store.remember(&continued, &resolution).await;
        assert_eq!(store.resolve(&continued).await.unwrap().home, "sub-a");
        store.rebind(&mut resolution, "sub-b").await;
        assert_eq!(store.resolve(&continued).await.unwrap().home, "sub-b");
    }

    #[tokio::test]
    async fn redis_outage_fails_open_to_local_affinity() {
        let store = AffinityStore::new(
            Some("redis://127.0.0.1:1/"),
            Some("outage-test-secret"),
            3600,
            60,
            5,
        )
        .unwrap();
        let input = store
            .infer(
                "acct",
                &HeaderMap::new(),
                &json!({"messages":[{"role":"user","content":"one"}]}),
            )
            .unwrap();
        let claimed = store.claim(&input, "opaque-home").await;
        assert_eq!(claimed.home, "opaque-home");
        assert_eq!(store.resolve(&input).await.unwrap().home, "opaque-home");
        assert!(store.stats().redis_errors >= 1);
    }

    #[tokio::test]
    async fn growing_classic_api_history_stays_on_its_subscription() {
        let store = store();
        let make_sub = |email: &str| registry::Sub {
            email: email.to_string(),
            token: "token".to_string(),
            proxy: String::new(),
            fleet: "prod".to_string(),
            plan: "max20".to_string(),
        };
        let pool = pool::Pool::new(
            vec![make_sub("a@example.test"), make_sub("b@example.test")],
            pool::Reserve::new(0.05, 0.05, 0.0),
            50.0,
            1500.0,
        );
        pool.set_util("a@example.test", Some(0.05), Some(0.05), None, None, None);
        pool.set_util("b@example.test", Some(0.30), Some(0.30), None, None, None);

        let first = store
            .infer(
                "account",
                &HeaderMap::new(),
                &json!({"model":"claude-test","messages":[{"role":"user","content":"one"}]}),
            )
            .unwrap();
        let hinted = pool.peek_affinity_home().unwrap();
        let resolution = store.claim(&first, &store.home_id(&hinted.email)).await;
        let first_home =
            match pool.route_affinity(&resolution.home, 0, true, |email| store.home_id(email)) {
                pool::AffinityRoute::Selected { sub, .. } => sub.email,
                other => panic!("unexpected first route: {other:?}"),
            };
        pool.mark_done(&first_home);

        let continued = store
            .infer(
                "account",
                &HeaderMap::new(),
                &json!({"model":"claude-test","messages":[
                    {"role":"user","content":"one"},
                    {"role":"assistant","content":"answer"},
                    {"role":"user","content":"two"}
                ]}),
            )
            .unwrap();
        let resolved = store.resolve(&continued).await.unwrap();
        match pool.route_affinity(&resolved.home, 0, false, |email| store.home_id(email)) {
            pool::AffinityRoute::Selected { sub, disposition } => {
                assert_eq!(sub.email, first_home);
                assert_eq!(disposition, pool::AffinityDisposition::Pinned);
                pool.mark_done(&sub.email);
            }
            other => panic!("unexpected continuation route: {other:?}"),
        }
    }

    #[tokio::test]
    #[ignore = "requires Redis at redis://default:test-affinity@127.0.0.1:16379/15"]
    async fn redis_shares_opaque_affinity_across_processes() {
        let url = "redis://default:test-affinity@127.0.0.1:16379/15";
        let client = redis::Client::open(url).unwrap();
        let mut connection = client.get_connection_manager().await.unwrap();
        redis::cmd("FLUSHDB")
            .query_async::<()>(&mut connection)
            .await
            .unwrap();

        let first =
            AffinityStore::new(Some(url), Some("shared-test-secret"), 3600, 60, 200).unwrap();
        let second =
            AffinityStore::new(Some(url), Some("shared-test-secret"), 3600, 60, 200).unwrap();
        let third =
            AffinityStore::new(Some(url), Some("shared-test-secret"), 3600, 60, 200).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-claude-code-session-id",
            HeaderValue::from_static("private-native-session"),
        );
        let body = json!({
            "model":"claude-test",
            "messages":[{"role":"user","content":"private prompt text"}]
        });
        let input = first.infer("private-account", &headers, &body).unwrap();
        let home = first.home_id("private-subscription@example.test");
        let competing_home = second.home_id("other-subscription@example.test");
        let second_input = second.infer("private-account", &headers, &body).unwrap();
        let (claimed, competing) = tokio::join!(
            first.claim(&input, &home),
            second.claim(&second_input, &competing_home)
        );
        assert_eq!(
            claimed.home, competing.home,
            "Redis claim must have one winner"
        );
        first.remember(&input, &claimed).await;

        let same_input = third.infer("private-account", &headers, &body).unwrap();
        let resolved = third.resolve(&same_input).await.unwrap();
        assert_eq!(resolved.home, claimed.home);
        assert_eq!(resolved.source, AffinitySource::Native);
        assert_eq!(third.stats().redis_hits, 1);

        let root_body = |message: &str| {
            json!({
                "model":"claude-test",
                "system":[{"type":"text","text":"shared Redis root","cache_control":{"type":"ephemeral"}}],
                "messages":[{"role":"user","content":message}]
            })
        };
        let root_first = first
            .infer("root-account", &HeaderMap::new(), &root_body("one"))
            .unwrap();
        let root_claim = first.claim(&root_first, &home).await;
        first.remember(&root_first, &root_claim).await;
        let root_second = third
            .infer("root-account", &HeaderMap::new(), &root_body("two"))
            .unwrap();
        let root_hit = third.resolve(&root_second).await.unwrap();
        assert_eq!(root_hit.source, AffinitySource::CacheRoot);
        assert_ne!(root_hit.session_id, root_claim.session_id);

        let keys: Vec<String> = redis::cmd("KEYS")
            .arg("*")
            .query_async(&mut connection)
            .await
            .unwrap();
        let mut dump = keys.join(" ");
        for key in &keys {
            if let Ok(Some(value)) = redis::cmd("GET")
                .arg(key)
                .query_async::<Option<String>>(&mut connection)
                .await
            {
                dump.push_str(&value);
            }
        }
        assert!(!dump.contains("private-account"));
        assert!(!dump.contains("private-native-session"));
        assert!(!dump.contains("private prompt text"));
        assert!(!dump.contains("root-account"));
        assert!(!dump.contains("shared Redis root"));
        assert!(!dump.contains("private-subscription"));
        assert!(!dump.contains("other-subscription"));
    }
}
