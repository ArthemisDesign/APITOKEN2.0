//! Единый агрегированный каталог `/v1/models` (этап 1b
//! docs/engine/UNIFIED_ROUTER.md).
//!
//! Router собирает живые каталоги трёх плоскостей (Anthropic envelope
//! `data[]`, OpenAI envelope `data[]`, Gemini envelope `models[]`), нормализует
//! в OpenAI-совместимый список с namespaced ID (`anthropic/claude-*`,
//! `openai/gpt-*`, `google/gemini-*`) и нативными aliases.
//!
//! Деградация (решение открытого вопроса документа): ответ никогда не ждёт
//! больше ~2 с на плоскость и не блокируется падением одной из них. Свежие
//! данные кэшируются на CATALOG_TTL; при недоступности плоскости отдаётся
//! last-good кэш (без TTL-ограничения), а namespace деградировавших плоскостей
//! перечисляется в заголовке `x-apitoken-catalog-degraded`. Плоскость без
//! ни разу не полученного каталога просто отсутствует в списке и тоже
//! попадает в этот заголовок. Если данных нет ни от одной плоскости — 503.
//! 401/403 плоскости пробрасывается клиенту как единый 401: ключ проверяет
//! общий billing authority, «частично валидного» ключа не бывает.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use serde_json::Value;

use crate::error::Lane;
use crate::metrics::{CatalogRefreshOutcome, RouterMetrics};
use crate::{bounded, bounded::ReadError};

/// Время жизни свежего кэша: частые клиенты (Claude Code discovery раз в
/// сессию) не порождают по три запроса в плоскости на каждый свой запрос.
pub const CATALOG_TTL: Duration = Duration::from_secs(30);
/// Верхняя граница ожидания одной плоскости. Три плоскости опрашиваются
/// конкурентно, поэтому худший ответ укладывается в требование Claude Code
/// «discovery быстрее трёх секунд».
pub const PLANE_FETCH_TIMEOUT: Duration = Duration::from_secs(2);
/// A reviewed provider catalog is small. This cap bounds both streamed and declared response
/// bodies before JSON parsing, while leaving ample room for 1,024 metadata-rich models.
const MAX_CATALOG_BODY_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_MODELS_PER_PLANE: usize = 1024;
pub const MAX_MODEL_STRING_BYTES: usize = 256;

/// Заголовок-маркер частичного каталога: comma-joined namespace'ы плоскостей,
/// отвечающих устаревшими данными или отсутствующих.
pub const DEGRADED_HEADER: &str = "x-apitoken-catalog-degraded";

/// Namespace'ы каталога. Это семейства моделей, а не гарантия единственного
/// исполнителя (см. «Модели и каталог» в UNIFIED_ROUTER.md).
pub const NS_ANTHROPIC: &str = "anthropic";
pub const NS_OPENAI: &str = "openai";
pub const NS_GOOGLE: &str = "google";

const REASONING_EFFORTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];
const ANTHROPIC_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
const SERVICE_TIERS: &[&str] = &["standard", "priority"];
const MODALITIES: &[&str] = &["text", "image", "audio"];
/// Public token limits are consumed as JavaScript numbers by harnesses. A u32
/// ceiling is far above every reviewed provider context while keeping hostile
/// catalog values bounded and exactly representable.
const MAX_TOKEN_LIMIT: u64 = u32::MAX as u64;

/// Плоскость по явному namespace-префиксу модели — общая для обоих universal
/// dispatch'ей (`chat.rs`, `responses.rs`; их dispatch-правила обязаны
/// совпадать). Каталог не опрашивается: admission плоскости сам резолвит
/// namespaced ID (решение 1). Модель без префикса или с неизвестным префиксом
/// уходит в alias-поиск по каталогу.
pub(crate) fn namespace_lane(model: &str) -> Option<Lane> {
    let (prefix, native) = model.split_once('/')?;
    if native.is_empty() {
        return None;
    }
    match prefix {
        NS_ANTHROPIC => Some(Lane::Anthropic),
        NS_OPENAI => Some(Lane::OpenAi),
        NS_GOOGLE => Some(Lane::Gemini),
        _ => None,
    }
}

/// Одна модель единого каталога.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogEntry {
    /// Namespaced ID (`anthropic/claude-opus-4-8`).
    pub id: String,
    /// Нативный ID плоскости. Он нужен для body rewrite и pricing preflight
    /// даже если совпадающий публичный alias снят из-за коллизии.
    pub native_id: String,
    /// Только глобально однозначные публичные aliases. Коллизия снимает alias
    /// со всех записей, но не затрагивает namespaced ID и native_id.
    pub aliases: Vec<String>,
    pub display_name: Option<String>,
    /// Authoritative normalized token limits from the provider plane.
    pub limits: Option<CatalogLimits>,
    /// Ordered OpenAI-compatible reasoning variants. `Some([])` is an
    /// authoritative statement that the model has no supported effort.
    pub reasoning_efforts: Option<Vec<String>>,
    /// Ordered execution tiers supported by this model.
    pub service_tiers: Option<Vec<String>>,
    /// Ordered input/output modalities accepted by the unified OpenAI-compatible adapter.
    pub input_modalities: Option<Vec<String>>,
    pub output_modalities: Option<Vec<String>>,
    /// Adapter-level controls. `None` means that the producer did not publish an authoritative
    /// answer; it must never be promoted to `true` by a consumer heuristic.
    pub tool_calling: Option<bool>,
    pub structured_outputs: Option<bool>,
    pub reasoning: Option<bool>,
    pub streaming: Option<bool>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CatalogLimits {
    pub context: Option<u64>,
    pub input: Option<u64>,
    pub output: Option<u64>,
}

impl CatalogEntry {
    /// OpenAI-совместимое JSON-представление. `created: 0` — как у
    /// OpenAI-плоскости: engine не публикует дату создания модели.
    pub fn to_json(&self, owned_by: &str) -> serde_json::Value {
        let mut obj = serde_json::json!({
            "id": self.id,
            "object": "model",
            "created": 0,
            "owned_by": owned_by,
            "aliases": self.aliases,
        });
        if let Some(name) = &self.display_name {
            obj["name"] = Value::String(name.clone());
        }
        if let Some(efforts) = &self.reasoning_efforts {
            obj["reasoning_efforts"] = serde_json::json!(efforts);
        }
        if let Some(tiers) = &self.service_tiers {
            obj["service_tiers"] = serde_json::json!(tiers);
        }
        let mut apitoken = serde_json::Map::new();
        if let Some(limits) = &self.limits {
            let mut normalized = serde_json::Map::new();
            for (name, value) in [
                ("context", limits.context),
                ("input", limits.input),
                ("output", limits.output),
            ] {
                if let Some(value) = value {
                    normalized.insert(name.to_string(), Value::from(value));
                }
            }
            if !normalized.is_empty() {
                apitoken.insert("limits".to_string(), Value::Object(normalized));
            }
        }
        let mut capabilities = serde_json::Map::new();
        if let Some(efforts) = &self.reasoning_efforts {
            capabilities.insert("reasoning_efforts".to_string(), serde_json::json!(efforts));
        }
        if let Some(tiers) = &self.service_tiers {
            capabilities.insert("service_tiers".to_string(), serde_json::json!(tiers));
        }
        for (name, values) in [
            ("input_modalities", &self.input_modalities),
            ("output_modalities", &self.output_modalities),
        ] {
            if let Some(values) = values {
                capabilities.insert(name.to_string(), serde_json::json!(values));
            }
        }
        for (name, value) in [
            ("tool_calling", self.tool_calling),
            ("structured_outputs", self.structured_outputs),
            ("reasoning", self.reasoning),
            ("streaming", self.streaming),
        ] {
            if let Some(value) = value {
                capabilities.insert(name.to_string(), Value::Bool(value));
            }
        }
        if !capabilities.is_empty() {
            apitoken.insert("capabilities".to_string(), Value::Object(capabilities));
        }
        if !apitoken.is_empty() {
            obj["apitoken"] = Value::Object(apitoken);
        }
        obj
    }
}

/// Last-good снимок каталога одной плоскости.
#[derive(Clone)]
struct PlaneCache {
    entries: Vec<CatalogEntry>,
    fetched_at: Instant,
}

/// In-memory кэш трёх плоскостей. Statelessness router'а относится к
/// запросам и деньгам; кэш каталога — деградационный и не влияет на биллинг.
pub struct Catalog {
    anthropic: PlaneState,
    openai: PlaneState,
    gemini: PlaneState,
    ttl: Duration,
}

struct PlaneState {
    cache: Mutex<Option<PlaneCache>>,
    refresh: tokio::sync::Mutex<()>,
    /// Incremented only for provider-wide failed/oversized refreshes. A caller that waited behind
    /// that exact in-flight attempt reuses its stale/missing result instead of starting a convoy;
    /// a later independent request still retries immediately. Credential-specific 401 is never
    /// shared across callers.
    failed_refresh_generation: AtomicU64,
}

impl PlaneState {
    fn new() -> Self {
        Self {
            cache: Mutex::new(None),
            refresh: tokio::sync::Mutex::new(()),
            failed_refresh_generation: AtomicU64::new(0),
        }
    }
}

/// Чем ответила плоскость на опрос каталога.
#[derive(Clone, Debug, Eq, PartialEq)]
enum PlaneOutcome {
    /// Актуальные данные (свежий fetch или кэш внутри TTL).
    Fresh(Vec<CatalogEntry>),
    /// Fetch не удался, отдаём last-good кэш.
    Stale(Vec<CatalogEntry>),
    /// Ключ отклонён: пробрасываем единый 401, кэш не трогаем.
    AuthRejected,
    /// Fetch не удался и кэша никогда не было.
    Missing,
}

impl PlaneOutcome {
    fn entries(&self) -> Option<&[CatalogEntry]> {
        match self {
            PlaneOutcome::Fresh(e) | PlaneOutcome::Stale(e) => Some(e),
            _ => None,
        }
    }
}

/// Итог одного опроса всех плоскостей.
pub struct Aggregate {
    /// Объединённые записи в детерминированном порядке: anthropic, openai, google.
    pub entries: Vec<(String, CatalogEntry)>,
    /// Namespace'ы, обслуженные устаревшими данными или отсутствующие.
    pub degraded: Vec<&'static str>,
    /// Ключ отклонён хотя бы одной плоскостью.
    pub auth_rejected: bool,
}

impl Catalog {
    pub fn new() -> Self {
        Self::with_ttl(CATALOG_TTL)
    }

    /// Каталог с нестандартным TTL — для тестов деградационных веток.
    pub(crate) fn with_ttl(ttl: Duration) -> Self {
        Catalog {
            anthropic: PlaneState::new(),
            openai: PlaneState::new(),
            gemini: PlaneState::new(),
            ttl,
        }
    }

    fn plane(&self, lane: Lane) -> &PlaneState {
        match lane {
            Lane::Anthropic => &self.anthropic,
            Lane::OpenAi => &self.openai,
            Lane::Gemini => &self.gemini,
        }
    }

    fn read(&self, lane: Lane) -> Option<PlaneCache> {
        self.plane(lane)
            .cache
            .lock()
            .expect("catalog cache poisoned")
            .clone()
    }

    fn write(&self, lane: Lane, entries: Vec<CatalogEntry>) {
        *self
            .plane(lane)
            .cache
            .lock()
            .expect("catalog cache poisoned") = Some(PlaneCache {
            entries,
            fetched_at: Instant::now(),
        });
    }

    /// Deterministic per-plane skew prevents a warm aggregate request from expiring every provider
    /// snapshot on the same instant. Zero remains zero for expiry tests.
    fn plane_ttl(&self, lane: Lane) -> Duration {
        let multiplier = match lane {
            Lane::Anthropic => 0.9,
            Lane::OpenAi => 1.0,
            Lane::Gemini => 1.1,
        };
        self.ttl.mul_f64(multiplier)
    }

    fn fresh(&self, lane: Lane) -> Option<PlaneCache> {
        self.read(lane)
            .filter(|cache| cache.fetched_at.elapsed() < self.plane_ttl(lane))
    }

    /// Опрос одной плоскости: сначала свежий кэш, затем живой fetch с
    /// коротким таймаутом, затем last-good. Никаких ретраев и health-check'ов.
    async fn plane_entries(
        &self,
        client: &reqwest::Client,
        origin: &str,
        lane: Lane,
        auth: &HeaderMap,
        metrics: &RouterMetrics,
    ) -> PlaneOutcome {
        if let Some(cache) = self.fresh(lane) {
            metrics.catalog_cache_hit(lane);
            return PlaneOutcome::Fresh(cache.entries);
        }
        let plane = self.plane(lane);
        let observed_failed_generation = plane
            .failed_refresh_generation
            .load(AtomicOrdering::Acquire);
        let _refresh = plane.refresh.lock().await;
        // Singleflight followers recheck after the leader publishes its snapshot.
        if let Some(cache) = self.fresh(lane) {
            metrics.catalog_cache_hit(lane);
            return PlaneOutcome::Fresh(cache.entries);
        }
        // Reuse only a failure that completed while this caller waited. This coalesces one failed
        // wave without adding negative-cache/circuit-breaker state between independent requests.
        if plane
            .failed_refresh_generation
            .load(AtomicOrdering::Acquire)
            != observed_failed_generation
        {
            return match self.read(lane) {
                Some(cache) => PlaneOutcome::Stale(cache.entries),
                None => PlaneOutcome::Missing,
            };
        }
        match fetch_plane(client, origin, lane, auth).await {
            FetchResult::Entries(entries) => {
                metrics.catalog_refresh(lane, CatalogRefreshOutcome::Success);
                self.write(lane, entries.clone());
                PlaneOutcome::Fresh(entries)
            }
            FetchResult::AuthRejected => {
                metrics.catalog_refresh(lane, CatalogRefreshOutcome::AuthRejected);
                PlaneOutcome::AuthRejected
            }
            FetchResult::Failed => {
                plane
                    .failed_refresh_generation
                    .fetch_add(1, AtomicOrdering::Release);
                metrics.catalog_refresh(lane, CatalogRefreshOutcome::Failed);
                match self.read(lane) {
                    Some(cache) => PlaneOutcome::Stale(cache.entries),
                    None => PlaneOutcome::Missing,
                }
            }
            FetchResult::Oversized => {
                plane
                    .failed_refresh_generation
                    .fetch_add(1, AtomicOrdering::Release);
                metrics.catalog_refresh(lane, CatalogRefreshOutcome::Oversized);
                match self.read(lane) {
                    Some(cache) => PlaneOutcome::Stale(cache.entries),
                    None => PlaneOutcome::Missing,
                }
            }
        }
    }

    /// Конкурентный опрос всех плоскостей и сборка единого каталога.
    pub async fn aggregate(
        &self,
        client: &reqwest::Client,
        origins: &PlaneOrigins<'_>,
        auth: &HeaderMap,
        metrics: &RouterMetrics,
    ) -> Aggregate {
        let (anthropic, openai, gemini) = tokio::join!(
            self.plane_entries(client, origins.anthropic, Lane::Anthropic, auth, metrics),
            self.plane_entries(client, origins.openai, Lane::OpenAi, auth, metrics),
            self.plane_entries(client, origins.gemini, Lane::Gemini, auth, metrics),
        );
        let mut aggregate = Aggregate {
            entries: Vec::new(),
            degraded: Vec::new(),
            auth_rejected: false,
        };
        for (namespace, lane, outcome) in [
            (NS_ANTHROPIC, Lane::Anthropic, anthropic),
            (NS_OPENAI, Lane::OpenAi, openai),
            (NS_GOOGLE, Lane::Gemini, gemini),
        ] {
            match &outcome {
                PlaneOutcome::AuthRejected => aggregate.auth_rejected = true,
                PlaneOutcome::Stale(_) | PlaneOutcome::Missing => {
                    aggregate.degraded.push(namespace);
                    metrics.catalog_degraded(lane);
                }
                PlaneOutcome::Fresh(_) => {}
            }
            if let Some(entries) = outcome.entries() {
                aggregate
                    .entries
                    .extend(entries.iter().map(|e| (namespace.to_string(), e.clone())));
            }
        }
        aggregate
    }
}

/// Origins трёх плоскостей — срез конфига для опроса каталога.
pub struct PlaneOrigins<'a> {
    pub anthropic: &'a str,
    pub openai: &'a str,
    pub gemini: &'a str,
}

/// Результат живого fetch'а каталога плоскости.
enum FetchResult {
    Entries(Vec<CatalogEntry>),
    AuthRejected,
    Failed,
    Oversized,
}

async fn fetch_plane(
    client: &reqwest::Client,
    origin: &str,
    lane: Lane,
    auth: &HeaderMap,
) -> FetchResult {
    let url = match lane {
        Lane::Anthropic => format!("{origin}/v1/models?limit=1000"),
        Lane::OpenAi => format!("{origin}/v1/models"),
        Lane::Gemini => format!("{origin}/v1beta/models?pageSize=1000"),
    };
    let response = match client
        .get(&url)
        .headers(auth.clone())
        .timeout(PLANE_FETCH_TIMEOUT)
        .send()
        .await
    {
        Ok(response) => response,
        Err(e) => {
            elog::warn(
                "router-catalog",
                format!("plane {lane:?} catalog fetch failed: {e}"),
            );
            return FetchResult::Failed;
        }
    };
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return FetchResult::AuthRejected;
    }
    if !status.is_success() {
        elog::warn(
            "router-catalog",
            format!(
                "plane {lane:?} catalog fetch failed: non-success status {}",
                status.as_u16()
            ),
        );
        return FetchResult::Failed;
    }
    let bytes = match bounded::response_bytes(response, MAX_CATALOG_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(ReadError::Oversized) => {
            elog::warn(
                "router-catalog",
                format!("plane {lane:?} catalog fetch failed: oversized body"),
            );
            return FetchResult::Oversized;
        }
        Err(ReadError::Transport) => {
            elog::warn(
                "router-catalog",
                format!("plane {lane:?} catalog fetch failed: body transport"),
            );
            return FetchResult::Failed;
        }
    };
    let body: Value = match serde_json::from_slice(&bytes) {
        Ok(body) => body,
        Err(_) => {
            elog::warn(
                "router-catalog",
                format!("plane {lane:?} catalog fetch failed: invalid JSON"),
            );
            return FetchResult::Failed;
        }
    };
    let model_count = match lane {
        Lane::Anthropic | Lane::OpenAi => body.get("data").and_then(Value::as_array),
        Lane::Gemini => body.get("models").and_then(Value::as_array),
    };
    if model_count.is_some_and(|models| models.len() > MAX_MODELS_PER_PLANE) {
        elog::warn(
            "router-catalog",
            format!("plane {lane:?} catalog fetch failed: too many models"),
        );
        return FetchResult::Oversized;
    }
    let entries = match lane {
        Lane::Anthropic => parse_anthropic(&body),
        Lane::OpenAi => parse_openai(&body),
        Lane::Gemini => parse_gemini(&body),
    };
    let Ok(entries) = entries else {
        elog::warn(
            "router-catalog",
            format!("plane {lane:?} catalog fetch failed: malformed catalog"),
        );
        return FetchResult::Failed;
    };
    if entries.is_empty() {
        // Пустой каталог от живой плоскости — аномалия конфигурации; не
        // закрепляем его в кэше, чтобы не опустошать единый каталог.
        elog::error(
            "router-catalog",
            format!("plane {lane:?} returned an empty catalog"),
        );
        return FetchResult::Failed;
    }
    FetchResult::Entries(entries)
}

type ParseResult = Result<Vec<CatalogEntry>, ()>;

fn bounded_model_id(value: Option<&str>) -> Result<Option<String>, ()> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.trim().is_empty() {
        return Ok(None);
    }
    if value.trim() != value
        || value.len() > MAX_MODEL_STRING_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(());
    }
    Ok(Some(value.to_string()))
}

fn bounded_display_name(value: Option<String>) -> Result<Option<String>, ()> {
    match value {
        Some(value)
            if value.is_empty()
                || value.trim() != value
                || value.len() > MAX_MODEL_STRING_BYTES
                || value.chars().any(char::is_control) =>
        {
            Err(())
        }
        value => Ok(value),
    }
}

fn namespaced_id(namespace: &str, native_id: &str) -> Result<String, ()> {
    let id = format!("{namespace}/{native_id}");
    (id.len() <= MAX_MODEL_STRING_BYTES).then_some(id).ok_or(())
}

fn positive_limit(object: &serde_json::Map<String, Value>, field: &str) -> Result<Option<u64>, ()> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    match value.as_u64() {
        Some(value) if (1..=MAX_TOKEN_LIMIT).contains(&value) => Ok(Some(value)),
        _ => Err(()),
    }
}

fn closed_list(
    object: &serde_json::Map<String, Value>,
    field: &str,
    allowed: &[&str],
) -> Result<Option<Vec<String>>, ()> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    let values = value.as_array().ok_or(())?;
    if values.len() > allowed.len() {
        return Err(());
    }
    let mut seen = HashSet::new();
    let mut parsed = Vec::with_capacity(values.len());
    for value in values {
        let value = value.as_str().ok_or(())?;
        if !allowed.contains(&value) || !seen.insert(value) {
            return Err(());
        }
        parsed.push(value.to_string());
    }
    Ok(Some(parsed))
}

fn optional_bool(object: &serde_json::Map<String, Value>, field: &str) -> Result<Option<bool>, ()> {
    match object.get(field) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(()),
    }
}

#[derive(Default)]
struct OwnedMetadata {
    limits: Option<CatalogLimits>,
    reasoning_efforts: Option<Vec<String>>,
    service_tiers: Option<Vec<String>>,
    input_modalities: Option<Vec<String>>,
    output_modalities: Option<Vec<String>>,
    tool_calling: Option<bool>,
    structured_outputs: Option<bool>,
    reasoning: Option<bool>,
    streaming: Option<bool>,
}

fn parse_owned_metadata(model: &serde_json::Map<String, Value>) -> Result<OwnedMetadata, ()> {
    let Some(apitoken) = model.get("apitoken") else {
        return Ok(OwnedMetadata::default());
    };
    let apitoken = apitoken.as_object().ok_or(())?;
    let limits = match apitoken.get("limits") {
        None => None,
        Some(value) => {
            let value = value.as_object().ok_or(())?;
            let limits = CatalogLimits {
                context: positive_limit(value, "context")?,
                input: positive_limit(value, "input")?,
                output: positive_limit(value, "output")?,
            };
            if let (Some(context), Some(input)) = (limits.context, limits.input) {
                if context < input {
                    return Err(());
                }
            }
            (limits.context.is_some() || limits.input.is_some() || limits.output.is_some())
                .then_some(limits)
        }
    };
    let mut metadata = OwnedMetadata {
        limits,
        ..OwnedMetadata::default()
    };
    match apitoken.get("capabilities") {
        None => {}
        Some(value) => {
            let value = value.as_object().ok_or(())?;
            metadata.reasoning_efforts =
                closed_list(value, "reasoning_efforts", REASONING_EFFORTS)?;
            metadata.service_tiers = closed_list(value, "service_tiers", SERVICE_TIERS)?;
            metadata.input_modalities = closed_list(value, "input_modalities", MODALITIES)?;
            metadata.output_modalities = closed_list(value, "output_modalities", MODALITIES)?;
            metadata.tool_calling = optional_bool(value, "tool_calling")?;
            metadata.structured_outputs = optional_bool(value, "structured_outputs")?;
            let explicit_reasoning = optional_bool(value, "reasoning")?;
            if explicit_reasoning == Some(false)
                && metadata
                    .reasoning_efforts
                    .as_ref()
                    .is_some_and(|efforts| efforts.iter().any(|effort| effort != "none"))
            {
                return Err(());
            }
            metadata.reasoning = explicit_reasoning.or_else(|| {
                metadata
                    .reasoning_efforts
                    .as_ref()
                    .map(|efforts| efforts.iter().any(|effort| effort != "none"))
            });
            metadata.streaming = optional_bool(value, "streaming")?;
        }
    }
    Ok(metadata)
}

fn parse_anthropic_efforts(
    model: &serde_json::Map<String, Value>,
) -> Result<Option<Vec<String>>, ()> {
    let Some(capabilities) = model.get("capabilities") else {
        return Ok(None);
    };
    let capabilities = capabilities.as_object().ok_or(())?;
    let Some(effort) = capabilities.get("effort") else {
        return Ok(None);
    };
    let effort = effort.as_object().ok_or(())?;
    let supported = effort.get("supported").and_then(Value::as_bool).ok_or(())?;
    let mut parsed = Vec::new();
    for level in ANTHROPIC_EFFORTS {
        let Some(value) = effort.get(*level) else {
            continue;
        };
        let level_supported = value
            .as_object()
            .and_then(|value| value.get("supported"))
            .and_then(Value::as_bool)
            .ok_or(())?;
        if level_supported {
            if !supported {
                return Err(());
            }
            parsed.push((*level).to_string());
        }
    }
    Ok(Some(parsed))
}

fn anthropic_adapter_efforts(model: &str) -> Vec<String> {
    let version = [
        "claude-opus-",
        "claude-sonnet-",
        "claude-haiku-",
        "claude-fable-",
    ]
    .into_iter()
    .find_map(|prefix| model.strip_prefix(prefix));
    let Some(version) = version else {
        return Vec::new();
    };
    let mut parts = version.split('-');
    let Some(major) = parts.next().and_then(|part| part.parse::<u16>().ok()) else {
        return Vec::new();
    };
    let efforts = if major > 4 {
        &["low", "medium", "high", "xhigh", "max"][..]
    } else if major == 4 {
        let Some(minor) = parts.next() else {
            return Vec::new();
        };
        // Date-based Claude 4 IDs are not semantic version 4.<date>.
        if minor.len() > 2 {
            return Vec::new();
        }
        match minor.parse::<u16>() {
            Ok(6) => &["low", "medium", "high", "max"][..],
            Ok(7..) => &["low", "medium", "high", "xhigh", "max"][..],
            _ => &[],
        }
    } else {
        &[]
    };
    efforts.iter().map(|value| (*value).to_string()).collect()
}

fn anthropic_capability_supported(
    model: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Option<bool>, ()> {
    let Some(capabilities) = model.get("capabilities") else {
        return Ok(None);
    };
    let capabilities = capabilities.as_object().ok_or(())?;
    let Some(capability) = capabilities.get(field) else {
        return Ok(None);
    };
    let capability = capability.as_object().ok_or(())?;
    capability
        .get("supported")
        .and_then(Value::as_bool)
        .map(Some)
        .ok_or(())
}

fn unique_plane_entries(entries: Vec<CatalogEntry>) -> ParseResult {
    let mut ids = HashSet::with_capacity(entries.len());
    if entries.iter().all(|entry| ids.insert(entry.id.as_str())) {
        Ok(entries)
    } else {
        Err(())
    }
}

fn anthropic_aliases(id: &str) -> Vec<String> {
    let mut aliases = vec![id.to_string()];
    if id == "claude-haiku-4-5-20251001" {
        aliases.push("claude-haiku-4-5".to_string());
    }
    aliases
}

fn parse_anthropic(body: &Value) -> ParseResult {
    let models = body.get("data").and_then(Value::as_array).ok_or(())?;
    if models.len() > MAX_MODELS_PER_PLANE {
        return Err(());
    }
    unique_plane_entries(
        models
            .iter()
            .filter_map(|model| {
                let model = match model.as_object() {
                    Some(model) => model,
                    None => return Some(Err(())),
                };
                let id = match bounded_model_id(model.get("id").and_then(Value::as_str)) {
                    Ok(Some(id)) => id,
                    Ok(None) => return None,
                    Err(()) => return Some(Err(())),
                };
                let input = match positive_limit(model, "max_input_tokens") {
                    Ok(value) => value,
                    Err(()) => return Some(Err(())),
                };
                let output = match positive_limit(model, "max_tokens") {
                    Ok(value) => value,
                    Err(()) => return Some(Err(())),
                };
                let limits = (input.is_some() || output.is_some()).then_some(CatalogLimits {
                    context: input,
                    input,
                    output,
                });
                let native_efforts = match parse_anthropic_efforts(model) {
                    Ok(value) => value,
                    Err(()) => return Some(Err(())),
                };
                let image_input = match anthropic_capability_supported(model, "image_input") {
                    Ok(value) => value,
                    Err(()) => return Some(Err(())),
                };
                let structured_outputs =
                    match anthropic_capability_supported(model, "structured_outputs") {
                        Ok(value) => value,
                        Err(()) => return Some(Err(())),
                    };
                let reasoning = match anthropic_capability_supported(model, "thinking") {
                    Ok(value) => value,
                    Err(()) => return Some(Err(())),
                };
                let namespaced_id = match namespaced_id(NS_ANTHROPIC, &id) {
                    Ok(id) => id,
                    Err(()) => return Some(Err(())),
                };
                let display_name = match bounded_display_name(
                    model
                        .get("display_name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                ) {
                    Ok(name) => name,
                    Err(()) => return Some(Err(())),
                };
                let mut reasoning_efforts = anthropic_adapter_efforts(&id);
                if let Some(native_efforts) = native_efforts {
                    reasoning_efforts.retain(|effort| native_efforts.contains(effort));
                }
                Some(Ok(CatalogEntry {
                    id: namespaced_id,
                    native_id: id.clone(),
                    aliases: anthropic_aliases(&id),
                    display_name,
                    limits,
                    reasoning_efforts: Some(reasoning_efforts),
                    service_tiers: Some(vec!["standard".to_string()]),
                    input_modalities: Some(if image_input == Some(true) {
                        vec!["text".to_string(), "image".to_string()]
                    } else {
                        vec!["text".to_string()]
                    }),
                    output_modalities: Some(vec!["text".to_string()]),
                    tool_calling: Some(true),
                    structured_outputs,
                    reasoning,
                    streaming: Some(true),
                }))
            })
            .collect::<ParseResult>()?,
    )
}

fn parse_openai(body: &Value) -> ParseResult {
    parse_owned_models(
        body.get("data").and_then(Value::as_array),
        NS_OPENAI,
        |model| model.get("id").and_then(Value::as_str).map(str::to_string),
        |model| {
            model
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        },
    )
}

fn parse_gemini(body: &Value) -> ParseResult {
    parse_owned_models(
        body.get("models").and_then(Value::as_array),
        NS_GOOGLE,
        |model| {
            model
                .get("name")
                .and_then(Value::as_str)
                .map(|name| name.strip_prefix("models/").unwrap_or(name).to_string())
        },
        |model| {
            model
                .get("displayName")
                .and_then(Value::as_str)
                .map(str::to_string)
        },
    )
}

fn parse_owned_models<Id, Display>(
    models: Option<&Vec<Value>>,
    namespace: &str,
    id_of: Id,
    display_of: Display,
) -> ParseResult
where
    Id: Fn(&serde_json::Map<String, Value>) -> Option<String>,
    Display: Fn(&serde_json::Map<String, Value>) -> Option<String>,
{
    let models = models.ok_or(())?;
    if models.len() > MAX_MODELS_PER_PLANE {
        return Err(());
    }
    unique_plane_entries(
        models
            .iter()
            .filter_map(|model| {
                let model = match model.as_object() {
                    Some(model) => model,
                    None => return Some(Err(())),
                };
                let id = match bounded_model_id(id_of(model).as_deref()) {
                    Ok(Some(id)) => id,
                    Ok(None) => return None,
                    Err(()) => return Some(Err(())),
                };
                let metadata = match parse_owned_metadata(model) {
                    Ok(metadata) => metadata,
                    Err(()) => return Some(Err(())),
                };
                let namespaced_id = match namespaced_id(namespace, &id) {
                    Ok(id) => id,
                    Err(()) => return Some(Err(())),
                };
                let display_name = match bounded_display_name(display_of(model)) {
                    Ok(name) => name,
                    Err(()) => return Some(Err(())),
                };
                Some(Ok(CatalogEntry {
                    id: namespaced_id,
                    native_id: id.clone(),
                    aliases: vec![id],
                    display_name,
                    limits: metadata.limits,
                    reasoning_efforts: metadata.reasoning_efforts,
                    service_tiers: metadata.service_tiers,
                    input_modalities: metadata.input_modalities,
                    output_modalities: metadata.output_modalities,
                    tool_calling: metadata.tool_calling,
                    structured_outputs: metadata.structured_outputs,
                    reasoning: metadata.reasoning,
                    streaming: metadata.streaming,
                }))
            })
            .collect::<ParseResult>()?,
    )
}

/// Дедупликация по namespaced ID с сохранением порядка.
pub fn dedup(entries: Vec<(String, CatalogEntry)>) -> Vec<(String, CatalogEntry)> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        if seen.insert(entry.1.id.clone()) {
            out.push(entry);
        }
    }
    let namespaced: HashSet<_> = out.iter().map(|(_, entry)| entry.id.clone()).collect();
    let mut alias_counts = HashMap::<String, usize>::new();
    for (_, entry) in &out {
        for alias in &entry.aliases {
            *alias_counts.entry(alias.clone()).or_default() += 1;
        }
    }
    for (_, entry) in &mut out {
        entry
            .aliases
            .retain(|alias| alias_counts.get(alias) == Some(&1) && !namespaced.contains(alias));
    }
    out
}

/// Поиск модели по namespaced ID или нативному alias.
pub fn find<'a>(
    entries: &'a [(String, CatalogEntry)],
    requested: &str,
) -> Option<&'a (String, CatalogEntry)> {
    entries.iter().find(|(_, e)| e.id == requested).or_else(|| {
        entries
            .iter()
            .find(|(_, e)| e.aliases.iter().any(|alias| alias == requested))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::extract::Request;
    use axum::response::Response;
    use axum::routing::any;
    use axum::Router;
    use futures_util::future::join_all;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn namespace_lane_maps_known_prefixes() {
        assert_eq!(
            namespace_lane("anthropic/claude-opus-4-8"),
            Some(Lane::Anthropic)
        );
        assert_eq!(namespace_lane("openai/gpt-5.6"), Some(Lane::OpenAi));
        assert_eq!(namespace_lane("google/gemini-2.5-pro"), Some(Lane::Gemini));
    }

    #[test]
    fn namespace_lane_falls_through_to_alias_lookup() {
        // Нативные alias'ы и неизвестные префиксы решает каталог.
        assert_eq!(namespace_lane("claude-opus-4-8"), None);
        assert_eq!(namespace_lane("gpt-5.6"), None);
        assert_eq!(namespace_lane("cohere/command-x"), None);
        // Пустой native ID после префикса — не namespaced модель, а 404 через
        // alias-поиск (каталог такой записи не содержит).
        assert_eq!(namespace_lane("anthropic/"), None);
        assert_eq!(namespace_lane(""), None);
    }

    #[test]
    fn anthropic_envelope_maps_to_namespaced_entries() {
        let body = serde_json::json!({
            "data": [
                {"type": "model", "id": "claude-opus-4-8", "display_name": "Claude Opus 4.8",
                 "max_input_tokens": 1000000, "max_tokens": 128000,
                 "capabilities": {"image_input": {"supported": true},
                    "structured_outputs": {"supported": true},
                    "thinking": {"supported": true},
                    "effort": {"supported": true,
                    "low": {"supported": true}, "medium": {"supported": true},
                    "high": {"supported": true}, "xhigh": {"supported": true},
                    "max": {"supported": true}}}},
                {"type": "model", "id": "claude-sonnet-4-6", "display_name": "Claude Sonnet 4.6",
                 "max_input_tokens": 1000000, "max_tokens": 128000,
                 "capabilities": {"image_input": {"supported": true},
                    "structured_outputs": {"supported": true},
                    "thinking": {"supported": true},
                    "effort": {"supported": true,
                    "low": {"supported": true}, "medium": {"supported": true},
                    "high": {"supported": true}, "xhigh": {"supported": false},
                    "max": {"supported": true}}}}
            ],
            "has_more": false
        });
        let entries = parse_anthropic(&body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "anthropic/claude-opus-4-8");
        assert_eq!(entries[0].native_id, "claude-opus-4-8");
        assert_eq!(entries[0].aliases, ["claude-opus-4-8"]);
        assert_eq!(entries[0].display_name.as_deref(), Some("Claude Opus 4.8"));
        assert_eq!(
            entries[0].limits,
            Some(CatalogLimits {
                context: Some(1_000_000),
                input: Some(1_000_000),
                output: Some(128_000),
            })
        );
        assert_eq!(
            entries[0].reasoning_efforts.as_deref(),
            Some(
                ["low", "medium", "high", "xhigh", "max"]
                    .map(String::from)
                    .as_slice()
            )
        );
        assert_eq!(
            entries[1].reasoning_efforts.as_deref(),
            Some(
                ["low", "medium", "high", "max"]
                    .map(String::from)
                    .as_slice()
            )
        );
        assert_eq!(
            entries[0].service_tiers.as_deref(),
            Some(["standard".to_string()].as_slice())
        );
        assert_eq!(
            entries[0].input_modalities.as_deref(),
            Some(["text".to_string(), "image".to_string()].as_slice())
        );
        assert_eq!(
            entries[0].output_modalities.as_deref(),
            Some(["text".to_string()].as_slice())
        );
        assert_eq!(entries[0].tool_calling, Some(true));
        assert_eq!(entries[0].structured_outputs, Some(true));
        assert_eq!(entries[0].reasoning, Some(true));
        assert_eq!(entries[0].streaming, Some(true));
    }

    #[test]
    fn dated_haiku_entry_publishes_the_bare_alias() {
        let body = serde_json::json!({
            "data": [
                {"type": "model", "id": "claude-haiku-4-5-20251001",
                 "display_name": "Claude Haiku 4.5",
                 "max_input_tokens": 200000, "max_tokens": 64000}
            ],
            "has_more": false
        });
        let entries = parse_anthropic(&body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "anthropic/claude-haiku-4-5-20251001");
        assert_eq!(entries[0].native_id, "claude-haiku-4-5-20251001");
        assert_eq!(
            entries[0].aliases,
            ["claude-haiku-4-5-20251001", "claude-haiku-4-5"]
        );
    }

    #[test]
    fn anthropic_efforts_match_the_unified_adapter_not_the_native_name_alone() {
        assert!(anthropic_adapter_efforts("claude-opus-4-5-20251101").is_empty());
        assert_eq!(
            anthropic_adapter_efforts("claude-sonnet-4-6"),
            ["low", "medium", "high", "max"].map(String::from)
        );
        assert_eq!(
            anthropic_adapter_efforts("claude-opus-4-7"),
            ["low", "medium", "high", "xhigh", "max"].map(String::from)
        );
        assert_eq!(
            anthropic_adapter_efforts("claude-opus-5"),
            ["low", "medium", "high", "xhigh", "max"].map(String::from)
        );
        assert!(anthropic_adapter_efforts("claude-opus-4-20250514").is_empty());
    }

    #[test]
    fn openai_envelope_maps_to_namespaced_entries() {
        let body = serde_json::json!({"object": "list", "data": [
            {"id": "gpt-5.6", "object": "model", "created": 0, "owned_by": "apitoken",
             "apitoken": {"limits": {"context": 400000, "input": 272000, "output": 128000},
                 "capabilities": {"reasoning_efforts": ["none", "low", "max"],
                                  "service_tiers": ["standard", "priority"],
                                  "input_modalities": ["text", "image"],
                                  "output_modalities": ["text"],
                                  "tool_calling": true,
                                  "structured_outputs": true,
                                  "streaming": true}}},
            {"id": "text-embedding-4", "object": "model", "created": 0, "owned_by": "apitoken"}
        ]});
        let entries = parse_openai(&body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "openai/gpt-5.6");
        assert_eq!(entries[0].native_id, "gpt-5.6");
        assert_eq!(entries[0].display_name, None);
        assert_eq!(entries[0].limits.as_ref().unwrap().input, Some(272_000));
        assert_eq!(
            entries[0].reasoning_efforts.as_deref(),
            Some(["none", "low", "max"].map(String::from).as_slice())
        );
        assert_eq!(
            entries[0].service_tiers.as_deref(),
            Some(["standard", "priority"].map(String::from).as_slice())
        );
        assert_eq!(
            entries[0].input_modalities.as_deref(),
            Some(["text".to_string(), "image".to_string()].as_slice())
        );
        assert_eq!(entries[0].tool_calling, Some(true));
        assert_eq!(entries[0].structured_outputs, Some(true));
        assert_eq!(entries[0].reasoning, Some(true));
        assert_eq!(entries[0].streaming, Some(true));
        assert_eq!(entries[1].limits, None);
        assert_eq!(entries[1].service_tiers, None);
    }

    /// An image-only model carries a capability block that is `false`/empty almost everywhere.
    /// Those are authoritative statements, not missing metadata, so none of them may be dropped or
    /// promoted: a client picks the right endpoint by reading `output_modalities`.
    #[test]
    fn openai_image_models_keep_their_image_only_capabilities() {
        let body = serde_json::json!({"object": "list", "data": [
            {"id": "gpt-image-2", "object": "model", "created": 0, "owned_by": "apitoken",
             "apitoken": {"endpoints": ["/v1/images/generations", "/v1/images/edits"],
                 "capabilities": {"reasoning_efforts": [],
                                  "service_tiers": ["standard"],
                                  "input_modalities": ["text", "image"],
                                  "output_modalities": ["image"],
                                  "tool_calling": false,
                                  "structured_outputs": false,
                                  "reasoning": false,
                                  "streaming": false}}}
        ]});
        let entries = parse_openai(&body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "openai/gpt-image-2");
        assert_eq!(entries[0].aliases, vec!["gpt-image-2".to_string()]);
        assert_eq!(
            entries[0].output_modalities.as_deref(),
            Some(["image".to_string()].as_slice())
        );
        assert_eq!(entries[0].reasoning_efforts.as_deref(), Some([].as_slice()));
        assert_eq!(entries[0].tool_calling, Some(false));
        assert_eq!(entries[0].reasoning, Some(false));
        assert_eq!(entries[0].streaming, Some(false));
        assert_eq!(entries[0].limits, None);
    }

    #[test]
    fn gemini_envelope_strips_models_prefix() {        let body = serde_json::json!({"models": [
            {"name": "models/gemini-2.5-pro", "displayName": "Gemini 2.5 Pro",
             "supportedGenerationMethods": ["generateContent"],
             "apitoken": {"limits": {"context": 1048576, "input": 1048576, "output": 65536},
                 "capabilities": {"reasoning_efforts": ["low", "medium", "high"],
                                  "service_tiers": ["standard"],
                                  "input_modalities": ["text", "image", "audio"],
                                  "output_modalities": ["text"],
                                  "tool_calling": true,
                                  "structured_outputs": true,
                                  "streaming": true}}},
            {"name": "models/gemini-2.5-flash", "displayName": "Gemini 2.5 Flash"}
        ]});
        let entries = parse_gemini(&body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "google/gemini-2.5-pro");
        assert_eq!(entries[0].native_id, "gemini-2.5-pro");
        assert_eq!(entries[0].display_name.as_deref(), Some("Gemini 2.5 Pro"));
        assert_eq!(entries[0].limits.as_ref().unwrap().output, Some(65_536));
        assert_eq!(
            entries[0].service_tiers.as_deref(),
            Some(["standard"].map(String::from).as_slice())
        );
        assert_eq!(
            entries[0].input_modalities.as_deref(),
            Some(["text".to_string(), "image".to_string(), "audio".to_string()].as_slice())
        );
        assert_eq!(entries[0].reasoning, Some(true));
    }

    #[test]
    fn parsers_accept_missing_legacy_metadata_without_guessing() {
        let mixed = serde_json::json!({"data": [
            {"id": ""}, {"id": "  "}, {"id": "claude-haiku-4-5"}, {"no_id": true}
        ]});
        let entries = parse_anthropic(&mixed).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].native_id, "claude-haiku-4-5");
        assert_eq!(entries[0].limits, None);
        assert_eq!(entries[0].reasoning_efforts, Some(vec![]));
        assert_eq!(entries[0].input_modalities, Some(vec!["text".to_string()]));
        assert_eq!(entries[0].tool_calling, Some(true));
    }

    #[test]
    fn malformed_authoritative_metadata_fails_the_plane_closed() {
        assert!(parse_anthropic(&serde_json::json!({"unexpected": true})).is_err());
        assert!(parse_openai(&serde_json::json!({"data": "not-an-array"})).is_err());
        assert!(parse_gemini(&serde_json::json!({"models": [1]})).is_err());
        for bad in [
            serde_json::json!({"data":[{"id":"gpt-x","apitoken":{"limits":{"context":0}}}]}),
            serde_json::json!({"data":[{"id":"gpt-x","apitoken":{"limits":{"input":10,"context":9}}}]}),
            serde_json::json!({"data":[{"id":"gpt-x","apitoken":{"capabilities":{"reasoning_efforts":["ultra"]}}}]}),
            serde_json::json!({"data":[{"id":"gpt-x","apitoken":{"capabilities":{"service_tiers":["standard","standard"]}}}]}),
            serde_json::json!({"data":[{"id":"gpt-x","apitoken":{"capabilities":{"input_modalities":["text","video"]}}}]}),
            serde_json::json!({"data":[{"id":"gpt-x","apitoken":{"capabilities":{"output_modalities":["text","text"]}}}]}),
            serde_json::json!({"data":[{"id":"gpt-x","apitoken":{"capabilities":{"tool_calling":"yes"}}}]}),
            serde_json::json!({"data":[{"id":"gpt-x","apitoken":{"capabilities":{"structured_outputs":1}}}]}),
            serde_json::json!({"data":[{"id":"gpt-x","apitoken":{"capabilities":{"reasoning":"yes"}}}]}),
            serde_json::json!({"data":[{"id":"gpt-x","apitoken":{"capabilities":{"reasoning":false,"reasoning_efforts":["high"]}}}]}),
            serde_json::json!({"data":[{"id":"gpt-x","apitoken":{"capabilities":{"streaming":null}}}]}),
            serde_json::json!({"data":[{"id":" gpt-x"}]}),
            serde_json::json!({"data":[{"id":"gpt-x","name":" GPT X"}]}),
            serde_json::json!({"data":[{"id":"gpt-x"},{"id":"gpt-x"}]}),
        ] {
            assert!(parse_openai(&bad).is_err(), "{bad}");
        }
        let conflicting = serde_json::json!({"data":[{"id":"claude-x","capabilities":{
            "effort":{"supported":false,"low":{"supported":true}}}}]});
        assert!(parse_anthropic(&conflicting).is_err());
        let malformed_anthropic = serde_json::json!({"data":[{"id":"claude-x","capabilities":{
            "image_input":{"supported":"yes"}}}]});
        assert!(parse_anthropic(&malformed_anthropic).is_err());
        assert!(parse_anthropic(&serde_json::json!({"data":[
            {"id":"claude-x"},{"id":"claude-x"}
        ]}))
        .is_err());
    }

    #[test]
    fn catalog_count_ids_and_display_names_are_bounded() {
        let too_many = vec![serde_json::json!({"id": "gpt-x"}); MAX_MODELS_PER_PLANE + 1];
        assert!(parse_openai(&serde_json::json!({"data": too_many})).is_err());
        let oversized = "x".repeat(MAX_MODEL_STRING_BYTES + 1);
        assert!(parse_openai(&serde_json::json!({"data": [{"id": oversized}]})).is_err());
        assert!(parse_gemini(&serde_json::json!({
            "models": [{"name": "models/gemini-x", "displayName": oversized}]
        }))
        .is_err());
        let longest_namespaced = "x".repeat(MAX_MODEL_STRING_BYTES);
        assert!(parse_openai(&serde_json::json!({"data": [{"id": longest_namespaced}]})).is_err());
    }

    #[tokio::test]
    async fn expired_catalog_refresh_is_singleflight_per_plane() {
        let calls = Arc::new(AtomicUsize::new(0));
        let state = calls.clone();
        let app = Router::new().fallback(any(move |request: Request| {
            let state = state.clone();
            async move {
                state.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(30)).await;
                let body = match request.uri().query() {
                    Some(query) if query.contains("pageSize") => {
                        serde_json::json!({"models": [{"name": "models/gemini-x"}]})
                    }
                    Some(_) => serde_json::json!({"data": [{"id": "claude-x"}]}),
                    None => serde_json::json!({"data": [{"id": "gpt-x"}]}),
                };
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap()
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let origin = format!("http://{address}");
        let origins = PlaneOrigins {
            anthropic: &origin,
            openai: &origin,
            gemini: &origin,
        };
        let catalog = Catalog::with_ttl(Duration::from_millis(20));
        let metrics = RouterMetrics::new();
        let client = reqwest::Client::new();
        catalog
            .aggregate(&client, &origins, &HeaderMap::new(), &metrics)
            .await;
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        tokio::time::sleep(Duration::from_millis(30)).await;

        let auth = HeaderMap::new();
        let aggregates =
            join_all((0..10).map(|_| catalog.aggregate(&client, &origins, &auth, &metrics))).await;
        assert!(aggregates
            .iter()
            .all(|aggregate| aggregate.entries.len() == 3));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            6,
            "one leader per provider plane must refresh after TTL"
        );
    }

    #[tokio::test]
    async fn failed_catalog_refresh_is_shared_by_waiting_followers() {
        let calls = Arc::new(AtomicUsize::new(0));
        let state = calls.clone();
        let app = Router::new().fallback(any(move || {
            let state = state.clone();
            async move {
                state.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(30)).await;
                Response::builder()
                    .status(reqwest::StatusCode::SERVICE_UNAVAILABLE)
                    .body(Body::empty())
                    .unwrap()
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let origin = format!("http://{address}");
        let origins = PlaneOrigins {
            anthropic: &origin,
            openai: &origin,
            gemini: &origin,
        };
        let catalog = Catalog::with_ttl(Duration::ZERO);
        let metrics = RouterMetrics::new();
        let client = reqwest::Client::new();
        let auth = HeaderMap::new();

        let aggregates =
            join_all((0..10).map(|_| catalog.aggregate(&client, &origins, &auth, &metrics))).await;
        assert!(aggregates
            .iter()
            .all(|aggregate| { aggregate.entries.is_empty() && aggregate.degraded.len() == 3 }));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "one failed fetch per plane must be shared by concurrent followers"
        );

        catalog.aggregate(&client, &origins, &auth, &metrics).await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            6,
            "a later independent request must retry without a negative-cache delay"
        );
    }

    fn entry(id: &str, alias: &str) -> (String, CatalogEntry) {
        let namespace = id.split_once('/').unwrap().0.to_string();
        (
            namespace,
            CatalogEntry {
                id: id.into(),
                native_id: alias.into(),
                aliases: vec![alias.into()],
                display_name: None,
                limits: None,
                reasoning_efforts: None,
                service_tiers: None,
                input_modalities: None,
                output_modalities: None,
                tool_calling: None,
                structured_outputs: None,
                reasoning: None,
                streaming: None,
            },
        )
    }

    #[test]
    fn dedup_keeps_first_occurrence_order() {
        let deduped = dedup(vec![
            entry("x/1", "a"),
            entry("x/2", "b"),
            entry("x/1", "a"),
            entry("x/3", "c"),
        ]);
        let ids: Vec<_> = deduped.iter().map(|(_, e)| e.id.as_str()).collect();
        assert_eq!(ids, ["x/1", "x/2", "x/3"]);
    }

    #[test]
    fn ambiguous_alias_is_removed_from_every_entry() {
        let entries = dedup(vec![
            entry("anthropic/shared", "shared"),
            entry("openai/shared", "shared"),
            entry("google/unique", "unique"),
        ]);
        assert!(entries[0].1.aliases.is_empty());
        assert!(entries[1].1.aliases.is_empty());
        assert_eq!(entries[2].1.aliases, ["unique"]);
        assert!(find(&entries, "shared").is_none());
        assert_eq!(find(&entries, "anthropic/shared").unwrap().0, "anthropic");
        assert_eq!(find(&entries, "openai/shared").unwrap().0, "openai");
    }

    #[test]
    fn find_resolves_namespaced_id_and_native_alias() {
        let entries = vec![
            entry("anthropic/claude-opus-4-8", "claude-opus-4-8"),
            entry("openai/gpt-5.6", "gpt-5.6"),
        ];
        assert!(find(&entries, "anthropic/claude-opus-4-8").is_some());
        assert!(find(&entries, "claude-opus-4-8").is_some());
        assert_eq!(find(&entries, "gpt-5.6").unwrap().0, "openai");
        assert!(find(&entries, "gemini-2.5-pro").is_none());
        assert!(find(&entries, "").is_none());
    }

    #[test]
    fn entry_json_shape_is_openai_compatible() {
        let entry = CatalogEntry {
            id: "anthropic/claude-opus-4-8".into(),
            native_id: "claude-opus-4-8".into(),
            aliases: vec!["claude-opus-4-8".into()],
            display_name: Some("Claude Opus 4.8".into()),
            limits: Some(CatalogLimits {
                context: Some(1_000_000),
                input: Some(1_000_000),
                output: Some(128_000),
            }),
            reasoning_efforts: Some(
                ["low", "medium", "high", "xhigh", "max"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            ),
            service_tiers: None,
            input_modalities: Some(vec!["text".into(), "image".into()]),
            output_modalities: Some(vec!["text".into()]),
            tool_calling: Some(true),
            structured_outputs: Some(true),
            reasoning: Some(true),
            streaming: Some(true),
        };
        let json = entry.to_json("anthropic");
        assert_eq!(json["id"], "anthropic/claude-opus-4-8");
        assert_eq!(json["object"], "model");
        assert_eq!(json["created"], 0);
        assert_eq!(json["owned_by"], "anthropic");
        assert_eq!(json["aliases"][0], "claude-opus-4-8");
        assert_eq!(json["name"], "Claude Opus 4.8");
        assert_eq!(
            json["reasoning_efforts"],
            serde_json::json!(["low", "medium", "high", "xhigh", "max"])
        );
        assert_eq!(json["apitoken"]["limits"]["context"], 1_000_000);
        assert_eq!(
            json["apitoken"]["capabilities"]["reasoning_efforts"],
            json["reasoning_efforts"]
        );
        assert_eq!(
            json["apitoken"]["capabilities"]["input_modalities"],
            serde_json::json!(["text", "image"])
        );
        assert_eq!(json["apitoken"]["capabilities"]["tool_calling"], true);
        assert_eq!(json["apitoken"]["capabilities"]["structured_outputs"], true);
        assert_eq!(json["apitoken"]["capabilities"]["reasoning"], true);
        assert_eq!(json["apitoken"]["capabilities"]["streaming"], true);
        let bare = CatalogEntry {
            id: "openai/gpt-5.6".into(),
            native_id: "gpt-5.6".into(),
            aliases: vec![],
            display_name: None,
            limits: None,
            reasoning_efforts: None,
            service_tiers: Some(vec!["standard".into(), "priority".into()]),
            input_modalities: None,
            output_modalities: None,
            tool_calling: None,
            structured_outputs: None,
            reasoning: None,
            streaming: None,
        };
        assert!(bare.to_json("openai").get("name").is_none());
        assert_eq!(bare.to_json("openai")["aliases"], serde_json::json!([]));
        assert_eq!(
            bare.to_json("openai")["service_tiers"],
            serde_json::json!(["standard", "priority"])
        );
    }
}
