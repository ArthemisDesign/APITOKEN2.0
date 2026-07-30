//! Automatic cache-lineage affinity for transparent Anthropic-compatible clients.
//!
//! This layer deliberately remains an optimization: local memory is the L1 and Redis is an
//! optional shared L2. Loss, eviction, or unavailability may reduce prompt-cache hits but can never
//! authorize money or subscription capacity. PostgreSQL capacity leases remain the final gate.

use axum::http::HeaderMap;
use redis::aio::ConnectionManager;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Mutex as AsyncMutex, RwLock};

const HASH_CONTEXT: &str = "claude-api/cache-affinity/v1";
const REDIS_PREFIX: &str = "claude-api:aff:v1";
const REDIS_ROOT_PREFIX: &str = "claude-api:aff:v2";
const DEFAULT_LOCAL_CAP: usize = 100_000;
const CACHE_ROOT_MIN_BYTES: usize = 4 * 1024;
const CACHE_ROOT_TTL_5M: u64 = 5 * 60;
const CACHE_ROOT_TTL_1H: u64 = 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AffinitySource {
    Native,
    Transcript,
    New,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AliasKind {
    Native,
    Transcript,
}

#[derive(Clone, Debug)]
struct Alias {
    digest: String,
    kind: AliasKind,
}

#[derive(Clone, Debug)]
struct CacheRoot {
    digest: String,
    ttl_secs: u64,
}

/// Opaque request-derived identity. It contains only keyed digests, never customer content or IDs.
#[derive(Clone, Debug)]
pub struct AffinityInput {
    account_tag: String,
    aliases: Vec<Alias>,
    cache_root: Option<CacheRoot>,
    pub cacheable_bytes: usize,
}

impl AffinityInput {
    fn primary(&self) -> &Alias {
        // Construction requires at least one message and therefore at least one transcript alias.
        &self.aliases[0]
    }

    pub(crate) fn primary_lineage(&self) -> &str {
        &self.primary().digest
    }

    /// Scope an opaque keyed lineage to its tenant before deriving provider-owned session
    /// continuity. Affinity storage already namespaces aliases by `account_tag`; the provider wire
    /// identity must preserve the same boundary so two tenants reusing an explicit session id can
    /// never receive the same upstream session UUID.
    pub(crate) fn provider_lineage(&self, lineage: &str) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"provider-lineage-v1");
        hasher.update(self.account_tag.as_bytes());
        hasher.update(&[0]);
        hasher.update(lineage.as_bytes());
        hex_digest(hasher.finalize().as_bytes())
    }

    /// A strong client/harness session ID is an isolation boundary. Once present, transcript or
    /// cache similarity may guide placement but can never make it inherit another session.
    fn resolution_aliases(&self) -> &[Alias] {
        if self.primary().kind == AliasKind::Native {
            let native_count = self
                .aliases
                .iter()
                .take_while(|alias| alias.kind == AliasKind::Native)
                .count();
            &self.aliases[..native_count]
        } else {
            &self.aliases
        }
    }

    fn has_cache_root(&self) -> bool {
        self.cache_root.is_some()
    }

    /// Stable, tenant-scoped and content-opaque key for the upstream prompt-cache router.
    ///
    /// A native client key is authoritative. Otherwise a large shared system/tools root is the
    /// best cache key; without one, an already-resolved conversation keeps the lineage selected
    /// on its first request. The account tag prevents two gateway tenants from sharing an
    /// upstream cache namespace even when they choose identical public keys and prompt prefixes.
    pub(crate) fn prompt_cache_key(&self, resolved_session_id: Option<&str>) -> String {
        let lineage = if self.primary().kind == AliasKind::Native {
            self.primary().digest.as_str()
        } else if let Some(root) = &self.cache_root {
            root.digest.as_str()
        } else if let Some(session_id) = resolved_session_id {
            session_id
        } else {
            self.primary().digest.as_str()
        };
        format!("{}:{lineage}", self.account_tag)
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
    pub cache_root_writes: u64,
    pub cache_root_warm_placements: u64,
    pub cache_root_cold_placements: u64,
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
    cache_root_writes: AtomicU64,
    cache_root_warm_placements: AtomicU64,
    cache_root_cold_placements: AtomicU64,
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
    roots: HashMap<String, HashMap<String, i64>>,
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
            "session-id",
            "x-thread-id",
            "x-session-affinity",
        ] {
            let Some(value) = headers.get(header).and_then(|value| value.to_str().ok()) else {
                continue;
            };
            let value = value.trim();
            if !valid_native_id(value) {
                continue;
            }
            aliases.push(Alias {
                // Normalize across transports: the same harness can move an ID between a generic
                // session header and a provider-specific one without losing its warm home.
                digest: self.digest_parts(b"native", &[value.as_bytes()]),
                kind: AliasKind::Native,
            });
            // One-release compatibility bridge for active v1 header-specific bindings. It is still
            // a strong alias and therefore cannot fall through to transcript/cache similarity.
            aliases.push(Alias {
                digest: self.digest_parts(b"native", &[header.as_bytes(), value.as_bytes()]),
                kind: AliasKind::Native,
            });
            break;
        }
        if aliases.is_empty() {
            let metadata = body.get("metadata").and_then(Value::as_object);
            for field in ["session_id", "conversation_id", "thread_id"] {
                let value = body.get(field).and_then(Value::as_str).or_else(|| {
                    metadata
                        .and_then(|values| values.get(field))
                        .and_then(Value::as_str)
                });
                let Some(value) = value.map(str::trim).filter(|value| valid_native_id(value))
                else {
                    continue;
                };
                aliases.push(Alias {
                    digest: self.digest_parts(b"native", &[value.as_bytes()]),
                    kind: AliasKind::Native,
                });
                break;
            }
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
        let explicit_root_ttl = ["system", "tools"]
            .iter()
            .filter_map(|field| body.get(*field))
            .filter_map(cache_control_ttl)
            .min();
        let cache_root =
            if root_cacheable_bytes >= CACHE_ROOT_MIN_BYTES || explicit_root_ttl.is_some() {
                Some(CacheRoot {
                    digest: hex_digest(&base_digest),
                    ttl_secs: explicit_root_ttl.unwrap_or(CACHE_ROOT_TTL_5M),
                })
            } else {
                None
            };
        Some(AffinityInput {
            account_tag,
            aliases,
            cache_root,
            cacheable_bytes,
        })
    }

    /// Infer a cache lineage for an OpenAI-compatible (Codex) request without any client opt-in.
    ///
    /// The provider speaks a different wire shape than Anthropic, but the cache-lineage question is
    /// identical: which home already holds this tenant's warm prompt prefix? Rather than duplicate
    /// the digest, transcript-chaining and cache-root machinery, this projects the Codex request
    /// onto the same canonical `{model, system, tools, messages}` shape `infer` consumes, so both
    /// providers share one implementation, one Redis namespace and one behaviour to reason about.
    /// `items` is the exact ordered conversation the model will see (history prefix + this turn);
    /// `instructions` is the combined base/developer instruction; `tools` are the dynamic tools.
    pub fn infer_codex(
        &self,
        account_scope: &str,
        headers: &HeaderMap,
        model: &str,
        instructions: Option<&str>,
        tools: &[Value],
        items: &[Value],
        prompt_cache_key: Option<&str>,
    ) -> Option<AffinityInput> {
        if items.is_empty() {
            return None;
        }
        let body = serde_json::json!({
            "model": model,
            "system": instructions,
            "tools": tools,
            "messages": items,
            // `infer` treats a native session id as a strong lineage boundary. Projecting the
            // public OpenAI key here gives it the same semantics while all stored/upstream values
            // remain keyed digests rather than customer identifiers.
            "session_id": prompt_cache_key,
        });
        self.infer(account_scope, headers, &body)
    }

    /// Project a native Gemini request onto the provider-independent cache lineage. Only keyed
    /// digests leave this process; raw contents, headers and tenant identity are never persisted.
    pub fn infer_gemini(
        &self,
        account_scope: &str,
        headers: &HeaderMap,
        model: &str,
        body: &Value,
    ) -> Option<AffinityInput> {
        let contents = body.get("contents")?.as_array()?;
        if contents.is_empty() {
            return None;
        }
        let projected = serde_json::json!({
            "model": model,
            "system": body.get("systemInstruction"),
            "tools": body.get("tools"),
            "thinking": body.pointer("/generationConfig/thinkingConfig"),
            "context_management": {
                "toolConfig": body.get("toolConfig"),
                "safetySettings": body.get("safetySettings"),
                "responseMimeType": body.pointer("/generationConfig/responseMimeType"),
                "responseSchema": body.pointer("/generationConfig/responseSchema")
            },
            "messages": contents,
        });
        self.infer(account_scope, headers, &projected)
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

    /// Return every live subscription known to hold this request's shared system/tools cache. A
    /// cache root is deliberately only a soft placement hint: it never resolves a conversation.
    pub async fn warm_homes(&self, input: &AffinityInput) -> Vec<String> {
        if input.cache_root.is_none() {
            return Vec::new();
        }
        let mut homes: HashSet<String> = self.local_warm_homes(input).into_iter().collect();
        match self.redis_warm_homes(input).await {
            Ok(redis_homes) => homes.extend(redis_homes),
            Err(()) => {
                self.stats.redis_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        if !homes.is_empty() {
            self.stats.cache_root_hits.fetch_add(1, Ordering::Relaxed);
        }
        let mut homes: Vec<String> = homes.into_iter().collect();
        homes.sort_unstable();
        homes
    }

    /// Record cache warmth only after a successful upstream response. L1 is updated synchronously;
    /// Redis is written in the background so an affinity optimization never delays response bytes.
    pub fn mark_cache_warm(self: &Arc<Self>, input: &AffinityInput, home: &str) {
        if input.cache_root.is_none() {
            return;
        }
        self.local_mark_cache_warm(input, home);
        self.stats.cache_root_writes.fetch_add(1, Ordering::Relaxed);
        if self.client.is_none() {
            return;
        }
        let this = Arc::clone(self);
        let input = input.clone();
        let home = home.to_string();
        tokio::spawn(async move {
            if this.redis_mark_cache_warm(&input, &home).await.is_err() {
                this.stats.redis_errors.fetch_add(1, Ordering::Relaxed);
            }
        });
    }

    /// Record whether a newly placed cacheable request reused an already-warm home or deliberately
    /// seeded a cold one. Aggregate counters reveal placement behavior without exposing identities.
    pub fn record_cache_root_placement(&self, input: &AffinityInput, warm: bool) {
        if !input.has_cache_root() {
            return;
        }
        let counter = if warm {
            &self.stats.cache_root_warm_placements
        } else {
            &self.stats.cache_root_cold_placements
        };
        counter.fetch_add(1, Ordering::Relaxed);
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
            cache_root_writes: self.stats.cache_root_writes.load(Ordering::Relaxed),
            cache_root_warm_placements: self
                .stats
                .cache_root_warm_placements
                .load(Ordering::Relaxed),
            cache_root_cold_placements: self
                .stats
                .cache_root_cold_placements
                .load(Ordering::Relaxed),
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
            AffinitySource::New => {}
        }
    }

    fn local_key(account_tag: &str, value: &str) -> String {
        format!("{account_tag}:{value}")
    }

    fn local_resolve(&self, input: &AffinityInput) -> Option<AffinityResolution> {
        let now = pool::now();
        let mut state = self.local.lock().unwrap_or_else(|error| error.into_inner());
        for alias in input.resolution_aliases() {
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
                session_id: binding.session_id,
                home: session.home,
                source: source_for(alias.kind),
                account_tag: input.account_tag.clone(),
            });
        }
        None
    }

    fn local_warm_homes(&self, input: &AffinityInput) -> Vec<String> {
        let Some(root) = &input.cache_root else {
            return Vec::new();
        };
        let now = pool::now();
        let key = Self::local_key(&input.account_tag, &root.digest);
        let mut state = self.local.lock().unwrap_or_else(|error| error.into_inner());
        let Some(homes) = state.roots.get_mut(&key) else {
            return Vec::new();
        };
        homes.retain(|_, expires_at| *expires_at > now);
        let result = homes.keys().cloned().collect();
        if homes.is_empty() {
            state.roots.remove(&key);
        }
        result
    }

    fn local_mark_cache_warm(&self, input: &AffinityInput, home: &str) {
        let Some(root) = &input.cache_root else {
            return;
        };
        let now = pool::now();
        let expires_at = now.saturating_add(root.ttl_secs as i64);
        let key = Self::local_key(&input.account_tag, &root.digest);
        let mut state = self.local.lock().unwrap_or_else(|error| error.into_inner());
        if state.roots.len() >= self.local_cap && !state.roots.contains_key(&key) {
            state.roots.retain(|_, homes| {
                homes.retain(|_, expiry| *expiry > now);
                !homes.is_empty()
            });
            if state.roots.len() >= self.local_cap {
                let remove = state.roots.len() / 4 + 1;
                let keys: Vec<String> = state.roots.keys().take(remove).cloned().collect();
                for key in keys {
                    state.roots.remove(&key);
                }
            }
        }
        state
            .roots
            .entry(key)
            .or_default()
            .insert(home.to_string(), expires_at);
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

    fn redis_root_key(account_tag: &str, root: &str) -> String {
        format!("{REDIS_ROOT_PREFIX}:{{{account_tag}}}:r:{root}")
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
            .resolution_aliases()
            .iter()
            .map(|alias| Self::redis_alias_key(&input.account_tag, &alias.digest))
            .collect();
        let aliases = input.resolution_aliases();
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
                        session_id,
                        home,
                        source: source_for(aliases[index].kind),
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

    async fn redis_warm_homes(&self, input: &AffinityInput) -> Result<Vec<String>, ()> {
        let Some(root) = &input.cache_root else {
            return Ok(Vec::new());
        };
        let Some(mut connection) = self.connection().await? else {
            return Ok(Vec::new());
        };
        let key = Self::redis_root_key(&input.account_tag, &root.digest);
        let now = pool::now();
        let query = async {
            let _: i64 = redis::cmd("ZREMRANGEBYSCORE")
                .arg(&key)
                .arg("-inf")
                .arg(now)
                .query_async(&mut connection)
                .await
                .map_err(|_| ())?;
            redis::cmd("ZRANGEBYSCORE")
                .arg(&key)
                .arg(now.saturating_add(1))
                .arg("+inf")
                .query_async::<Vec<String>>(&mut connection)
                .await
                .map_err(|_| ())
        };
        match tokio::time::timeout(self.timeout, query).await {
            Ok(Ok(homes)) => Ok(homes),
            _ => {
                self.reset_connection().await;
                Err(())
            }
        }
    }

    async fn redis_mark_cache_warm(&self, input: &AffinityInput, home: &str) -> Result<(), ()> {
        let Some(root) = &input.cache_root else {
            return Ok(());
        };
        let Some(mut connection) = self.connection().await? else {
            return Ok(());
        };
        let key = Self::redis_root_key(&input.account_tag, &root.digest);
        let now = pool::now();
        let expires_at = now.saturating_add(root.ttl_secs as i64);
        let script = redis::Script::new(
            "redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1]); \
             redis.call('ZADD', KEYS[1], ARGV[2], ARGV[3]); \
             local current = redis.call('TTL', KEYS[1]); \
             local wanted = tonumber(ARGV[4]); \
             if current < wanted then redis.call('EXPIRE', KEYS[1], wanted); end; \
             return 1",
        );
        let mut invocation = script.prepare_invoke();
        invocation
            .key(key)
            .arg(now)
            .arg(expires_at)
            .arg(home)
            .arg(root.ttl_secs);
        match tokio::time::timeout(
            self.timeout,
            invocation.invoke_async::<i64>(&mut connection),
        )
        .await
        {
            Ok(Ok(_)) => Ok(()),
            _ => {
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
    }
}

fn valid_native_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512
}

fn cache_control_ttl(value: &Value) -> Option<u64> {
    match value {
        Value::Array(values) => values.iter().filter_map(cache_control_ttl).min(),
        Value::Object(values) => {
            let local = values.get("cache_control").map(|control| {
                if control
                    .get("ttl")
                    .and_then(Value::as_str)
                    .is_some_and(|ttl| ttl.eq_ignore_ascii_case("1h"))
                {
                    CACHE_ROOT_TTL_1H
                } else {
                    CACHE_ROOT_TTL_5M
                }
            });
            local
                .into_iter()
                .chain(values.values().filter_map(cache_control_ttl).min())
                .min()
        }
        _ => None,
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

    fn store() -> Arc<AffinityStore> {
        Arc::new(
            AffinityStore::new(
                None,
                Some("test-secret-at-least-32-characters"),
                3600,
                3600,
                20,
            )
            .unwrap(),
        )
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

    #[test]
    fn codex_prompt_cache_key_is_stable_opaque_and_tenant_scoped() {
        let store = store();
        let headers = HeaderMap::new();
        let first_items = vec![json!({"role":"user","content":"first"})];
        let second_items = vec![json!({"role":"user","content":"unrelated"})];
        let first = store
            .infer_codex(
                "acct-1",
                &headers,
                "gpt-5.6",
                None,
                &[],
                &first_items,
                Some("customer-cache-key"),
            )
            .unwrap();
        let second = store
            .infer_codex(
                "acct-1",
                &headers,
                "gpt-5.6",
                None,
                &[],
                &second_items,
                Some("customer-cache-key"),
            )
            .unwrap();
        let other_tenant = store
            .infer_codex(
                "acct-2",
                &headers,
                "gpt-5.6",
                None,
                &[],
                &first_items,
                Some("customer-cache-key"),
            )
            .unwrap();

        let first_key = first.prompt_cache_key(None);
        assert_eq!(first_key, second.prompt_cache_key(None));
        assert_ne!(first_key, other_tenant.prompt_cache_key(None));
        assert_eq!(first_key.len(), 129);
        assert!(!first_key.contains("acct-1"));
        assert!(!first_key.contains("customer-cache-key"));
    }

    #[tokio::test]
    async fn cache_root_is_soft_and_never_resolves_a_new_conversation() {
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
        store.remember(&first, &first_resolution).await;
        store.mark_cache_warm(&first, "opaque-home");

        let second = store.infer("acct", &headers, &body("two")).unwrap();
        assert!(store.resolve(&second).await.is_none());
        assert_eq!(store.warm_homes(&second).await, vec!["opaque-home"]);
    }

    #[tokio::test]
    async fn distinct_native_sessions_never_inherit_matching_transcript() {
        let store = store();
        let body = json!({
            "model":"claude-test",
            "system":[{"type":"text","text":"shared root","cache_control":{"type":"ephemeral"}}],
            "messages":[{"role":"user","content":"identical first turn"}]
        });
        let headers = |session: &'static str| {
            let mut headers = HeaderMap::new();
            headers.insert("x-session-id", HeaderValue::from_static(session));
            headers
        };
        let first = store.infer("acct", &headers("session-a"), &body).unwrap();
        let first_resolution = store.claim(&first, "home-a").await;
        store.remember(&first, &first_resolution).await;
        store.mark_cache_warm(&first, "home-a");

        let second = store.infer("acct", &headers("session-b"), &body).unwrap();
        assert!(store.resolve(&second).await.is_none());
        assert_eq!(store.warm_homes(&second).await, vec!["home-a"]);

        let second_resolution = store.claim(&second, "home-b").await;
        assert_ne!(second_resolution.session_id, first_resolution.session_id);
        assert_eq!(store.resolve(&second).await.unwrap().home, "home-b");
    }

    #[tokio::test]
    async fn cache_root_tracks_multiple_warm_homes() {
        let store = store();
        let input = store
            .infer(
                "acct",
                &HeaderMap::new(),
                &json!({
                    "system":[{"type":"text","text":"shared","cache_control":{"type":"ephemeral"}}],
                    "messages":[{"role":"user","content":"one"}]
                }),
            )
            .unwrap();
        store.mark_cache_warm(&input, "home-b");
        store.mark_cache_warm(&input, "home-a");
        store.record_cache_root_placement(&input, true);
        store.record_cache_root_placement(&input, false);
        assert_eq!(store.warm_homes(&input).await, vec!["home-a", "home-b"]);
        assert_eq!(store.stats().cache_root_writes, 2);
        assert_eq!(store.stats().cache_root_warm_placements, 1);
        assert_eq!(store.stats().cache_root_cold_placements, 1);
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
    fn generic_body_session_ids_are_supported_and_transport_normalized() {
        let store = store();
        let body_input = store
            .infer(
                "acct",
                &HeaderMap::new(),
                &json!({
                    "metadata":{"session_id":"portable-session"},
                    "messages":[{"role":"user","content":"hi"}]
                }),
            )
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-conversation-id",
            HeaderValue::from_static("portable-session"),
        );
        let header_input = store
            .infer(
                "acct",
                &headers,
                &json!({"messages":[{"role":"user","content":"different"}]}),
            )
            .unwrap();
        assert_eq!(body_input.primary().kind, AliasKind::Native);
        assert_eq!(body_input.primary().digest, header_input.primary().digest);
    }

    #[tokio::test]
    async fn normalized_native_id_adopts_an_active_legacy_header_binding() {
        let store = store();
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-session-id",
            HeaderValue::from_static("already-running-session"),
        );
        let current = store
            .infer(
                "acct",
                &headers,
                &json!({"messages":[{"role":"user","content":"hi"}]}),
            )
            .unwrap();
        let mut legacy = current.clone();
        legacy.aliases.remove(0);
        let claimed = store.claim(&legacy, "existing-home").await;

        let resolved = store.resolve(&current).await.unwrap();
        assert_eq!(resolved.session_id, claimed.session_id);
        assert_eq!(resolved.home, "existing-home");
        assert_eq!(resolved.source, AffinitySource::Native);
    }

    #[test]
    fn cache_root_ttl_follows_anthropic_cache_control() {
        let store = store();
        let body = |ttl: Option<&str>| {
            let mut control = json!({"type":"ephemeral"});
            if let Some(ttl) = ttl {
                control["ttl"] = json!(ttl);
            }
            json!({
                "system":[{"type":"text","text":"shared","cache_control":control}],
                "messages":[{"role":"user","content":"hi"}]
            })
        };
        assert_eq!(
            store
                .infer("acct", &HeaderMap::new(), &body(None))
                .unwrap()
                .cache_root
                .unwrap()
                .ttl_secs,
            CACHE_ROOT_TTL_5M
        );
        assert_eq!(
            store
                .infer("acct", &HeaderMap::new(), &body(Some("1h")))
                .unwrap()
                .cache_root
                .unwrap()
                .ttl_secs,
            CACHE_ROOT_TTL_1H
        );
        let mixed = json!({
            "system":[
                {"type":"text","text":"long","cache_control":{"type":"ephemeral","ttl":"1h"}},
                {"type":"text","text":"short","cache_control":{"type":"ephemeral"}}
            ],
            "messages":[{"role":"user","content":"hi"}]
        });
        assert_eq!(
            store
                .infer("acct", &HeaderMap::new(), &mixed)
                .unwrap()
                .cache_root
                .unwrap()
                .ttl_secs,
            CACHE_ROOT_TTL_5M
        );
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

        let first = Arc::new(
            AffinityStore::new(Some(url), Some("shared-test-secret"), 3600, 60, 200).unwrap(),
        );
        let second = Arc::new(
            AffinityStore::new(Some(url), Some("shared-test-secret"), 3600, 60, 200).unwrap(),
        );
        let third = Arc::new(
            AffinityStore::new(Some(url), Some("shared-test-secret"), 3600, 60, 200).unwrap(),
        );
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

        let root_body = |session: &str, message: &str| {
            json!({
                "model":"claude-test",
                "session_id":session,
                "system":[{"type":"text","text":"shared Redis root","cache_control":{"type":"ephemeral"}}],
                "messages":[{"role":"user","content":message}]
            })
        };
        let root_first = first
            .infer(
                "root-account",
                &HeaderMap::new(),
                &root_body("root-session-a", "one"),
            )
            .unwrap();
        let root_claim = first.claim(&root_first, &home).await;
        first.remember(&root_first, &root_claim).await;
        first.mark_cache_warm(&root_first, &home);
        let root_second = third
            .infer(
                "root-account",
                &HeaderMap::new(),
                &root_body("root-session-b", "two"),
            )
            .unwrap();
        assert!(third.resolve(&root_second).await.is_none());
        let mut root_homes = Vec::new();
        for _ in 0..50 {
            root_homes = third.warm_homes(&root_second).await;
            if root_homes == vec![home.clone()] {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(root_homes, vec![home.clone()]);

        second.mark_cache_warm(&root_second, &competing_home);
        for _ in 0..50 {
            root_homes = third.warm_homes(&root_second).await;
            if root_homes.len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let mut expected_homes = vec![competing_home.clone(), home.clone()];
        expected_homes.sort_unstable();
        assert_eq!(root_homes, expected_homes);

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
            if let Ok(values) = redis::cmd("ZRANGE")
                .arg(key)
                .arg(0)
                .arg(-1)
                .query_async::<Vec<String>>(&mut connection)
                .await
            {
                dump.push_str(&values.join(" "));
            }
        }
        assert!(!dump.contains("private-account"));
        assert!(!dump.contains("private-native-session"));
        assert!(!dump.contains("private prompt text"));
        assert!(!dump.contains("root-account"));
        assert!(!dump.contains("shared Redis root"));
        assert!(!dump.contains("root-session-a"));
        assert!(!dump.contains("root-session-b"));
        assert!(!dump.contains("private-subscription"));
        assert!(!dump.contains("other-subscription"));
    }
}
