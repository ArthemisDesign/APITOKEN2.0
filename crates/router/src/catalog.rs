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

use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use serde_json::Value;

use crate::error::Lane;

/// Время жизни свежего кэша: частые клиенты (Claude Code discovery раз в
/// сессию) не порождают по три запроса в плоскости на каждый свой запрос.
pub const CATALOG_TTL: Duration = Duration::from_secs(30);
/// Верхняя граница ожидания одной плоскости. Три плоскости опрашиваются
/// конкурентно, поэтому худший ответ укладывается в требование Claude Code
/// «discovery быстрее трёх секунд».
pub const PLANE_FETCH_TIMEOUT: Duration = Duration::from_secs(2);

/// Заголовок-маркер частичного каталога: comma-joined namespace'ы плоскостей,
/// отвечающих устаревшими данными или отсутствующих.
pub const DEGRADED_HEADER: &str = "x-apitoken-catalog-degraded";

/// Namespace'ы каталога. Это семейства моделей, а не гарантия единственного
/// исполнителя (см. «Модели и каталог» в UNIFIED_ROUTER.md).
pub const NS_ANTHROPIC: &str = "anthropic";
pub const NS_OPENAI: &str = "openai";
pub const NS_GOOGLE: &str = "google";

const ANTHROPIC_EFFORTS_4_6: &[&str] = &["low", "medium", "high", "max"];
const ANTHROPIC_EFFORTS_FULL: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// Router-owned discovery metadata for the OpenAI-compatible surface. The
/// native Anthropic model list has no effort capability field, so the unified
/// catalog publishes the live-verified model boundary explicitly: 4.6 has
/// `max` but no `xhigh`, while 4.7+ and 5 have both. Older/unknown ids publish
/// an authoritative empty list rather than inviting clients to guess.
fn anthropic_reasoning_efforts(model: &str) -> &'static [&'static str] {
    let version = [
        "claude-opus-",
        "claude-sonnet-",
        "claude-haiku-",
        "claude-fable-",
    ]
    .into_iter()
    .find_map(|prefix| model.strip_prefix(prefix));
    let Some(version) = version else {
        return &[];
    };
    let mut parts = version.split('-');
    let Some(major) = parts.next().and_then(|part| part.parse::<u16>().ok()) else {
        return &[];
    };
    if major > 4 {
        return ANTHROPIC_EFFORTS_FULL;
    }
    if major < 4 {
        return &[];
    }
    let Some(minor) = parts.next() else {
        return &[];
    };
    if minor.len() > 2 {
        return &[];
    }
    match minor.parse::<u16>() {
        Ok(6) => ANTHROPIC_EFFORTS_4_6,
        Ok(7..) => ANTHROPIC_EFFORTS_FULL,
        _ => &[],
    }
}

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
    /// Нативный ID плоскости (`claude-opus-4-8`) — однозначный alias.
    pub alias: String,
    pub display_name: Option<String>,
    /// Ordered OpenAI-compatible reasoning variants. `Some([])` is an
    /// authoritative statement that this Anthropic model has no GA adaptive
    /// effort; `None` means this catalog parser does not own such metadata.
    pub reasoning_efforts: Option<Vec<String>>,
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
            "aliases": [self.alias],
        });
        if let Some(name) = &self.display_name {
            obj["name"] = Value::String(name.clone());
        }
        if let Some(efforts) = &self.reasoning_efforts {
            obj["reasoning_efforts"] = serde_json::json!(efforts);
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
    anthropic: Mutex<Option<PlaneCache>>,
    openai: Mutex<Option<PlaneCache>>,
    gemini: Mutex<Option<PlaneCache>>,
    ttl: Duration,
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
            anthropic: Mutex::new(None),
            openai: Mutex::new(None),
            gemini: Mutex::new(None),
            ttl,
        }
    }

    fn cache(&self, lane: Lane) -> &Mutex<Option<PlaneCache>> {
        match lane {
            Lane::Anthropic => &self.anthropic,
            Lane::OpenAi => &self.openai,
            Lane::Gemini => &self.gemini,
        }
    }

    fn read(&self, lane: Lane) -> Option<PlaneCache> {
        self.cache(lane).lock().expect("catalog cache poisoned").clone()
    }

    fn write(&self, lane: Lane, entries: Vec<CatalogEntry>) {
        *self.cache(lane).lock().expect("catalog cache poisoned") =
            Some(PlaneCache { entries, fetched_at: Instant::now() });
    }

    /// Опрос одной плоскости: сначала свежий кэш, затем живой fetch с
    /// коротким таймаутом, затем last-good. Никаких ретраев и health-check'ов.
    async fn plane_entries(
        &self,
        client: &reqwest::Client,
        origin: &str,
        lane: Lane,
        auth: &HeaderMap,
    ) -> PlaneOutcome {
        if let Some(cache) = self.read(lane) {
            if cache.fetched_at.elapsed() < self.ttl {
                return PlaneOutcome::Fresh(cache.entries);
            }
        }
        match fetch_plane(client, origin, lane, auth).await {
            FetchResult::Entries(entries) => {
                self.write(lane, entries.clone());
                PlaneOutcome::Fresh(entries)
            }
            FetchResult::AuthRejected => PlaneOutcome::AuthRejected,
            FetchResult::Failed => match self.read(lane) {
                Some(cache) => PlaneOutcome::Stale(cache.entries),
                None => PlaneOutcome::Missing,
            },
        }
    }

    /// Конкурентный опрос всех плоскостей и сборка единого каталога.
    pub async fn aggregate(
        &self,
        client: &reqwest::Client,
        origins: &PlaneOrigins<'_>,
        auth: &HeaderMap,
    ) -> Aggregate {
        let (anthropic, openai, gemini) = tokio::join!(
            self.plane_entries(client, origins.anthropic, Lane::Anthropic, auth),
            self.plane_entries(client, origins.openai, Lane::OpenAi, auth),
            self.plane_entries(client, origins.gemini, Lane::Gemini, auth),
        );
        let mut aggregate = Aggregate { entries: Vec::new(), degraded: Vec::new(), auth_rejected: false };
        for (namespace, outcome) in [
            (NS_ANTHROPIC, anthropic),
            (NS_OPENAI, openai),
            (NS_GOOGLE, gemini),
        ] {
            match &outcome {
                PlaneOutcome::AuthRejected => aggregate.auth_rejected = true,
                PlaneOutcome::Stale(_) | PlaneOutcome::Missing => aggregate.degraded.push(namespace),
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
        Err(_) => return FetchResult::Failed,
    };
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return FetchResult::AuthRejected;
    }
    if !status.is_success() {
        return FetchResult::Failed;
    }
    let body: Value = match response.json().await {
        Ok(body) => body,
        Err(_) => return FetchResult::Failed,
    };
    let entries = match lane {
        Lane::Anthropic => parse_anthropic(&body),
        Lane::OpenAi => parse_openai(&body),
        Lane::Gemini => parse_gemini(&body),
    };
    if entries.is_empty() {
        // Пустой каталог от живой плоскости — аномалия конфигурации; не
        // закрепляем его в кэше, чтобы не опустошать единый каталог.
        return FetchResult::Failed;
    }
    FetchResult::Entries(entries)
}

fn parse_anthropic(body: &Value) -> Vec<CatalogEntry> {
    body["data"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.trim().to_string();
                    if id.is_empty() {
                        return None;
                    }
                    Some(CatalogEntry {
                        id: format!("{NS_ANTHROPIC}/{id}"),
                        reasoning_efforts: Some(
                            anthropic_reasoning_efforts(&id)
                                .iter()
                                .map(|effort| (*effort).to_string())
                                .collect(),
                        ),
                        alias: id,
                        display_name: m["display_name"].as_str().map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_openai(body: &Value) -> Vec<CatalogEntry> {
    body["data"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.trim().to_string();
                    if id.is_empty() {
                        return None;
                    }
                    Some(CatalogEntry {
                        id: format!("{NS_OPENAI}/{id}"),
                        alias: id,
                        display_name: None,
                        reasoning_efforts: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_gemini(body: &Value) -> Vec<CatalogEntry> {
    body["models"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .filter_map(|m| {
                    let name = m["name"].as_str()?;
                    let id = name.strip_prefix("models/").unwrap_or(name).trim().to_string();
                    if id.is_empty() {
                        return None;
                    }
                    Some(CatalogEntry {
                        id: format!("{NS_GOOGLE}/{id}"),
                        alias: id,
                        display_name: m["displayName"].as_str().map(str::to_string),
                        reasoning_efforts: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Дедупликация по namespaced ID с сохранением порядка.
pub fn dedup(entries: Vec<(String, CatalogEntry)>) -> Vec<(String, CatalogEntry)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        if seen.insert(entry.1.id.clone()) {
            out.push(entry);
        }
    }
    out
}

/// Поиск модели по namespaced ID или нативному alias.
pub fn find<'a>(
    entries: &'a [(String, CatalogEntry)],
    requested: &str,
) -> Option<&'a (String, CatalogEntry)> {
    entries
        .iter()
        .find(|(_, e)| e.id == requested)
        .or_else(|| entries.iter().find(|(_, e)| e.alias == requested))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_lane_maps_known_prefixes() {
        assert_eq!(namespace_lane("anthropic/claude-opus-4-8"), Some(Lane::Anthropic));
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
                 "created_at": "2026-01-01T00:00:00Z"},
                {"type": "model", "id": "claude-sonnet-5", "display_name": "Claude Sonnet 5",
                 "created_at": "2026-02-01T00:00:00Z"}
            ],
            "has_more": false, "first_id": "claude-opus-4-8", "last_id": "claude-sonnet-5"
        });
        let entries = parse_anthropic(&body);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "anthropic/claude-opus-4-8");
        assert_eq!(entries[0].alias, "claude-opus-4-8");
        assert_eq!(entries[0].display_name.as_deref(), Some("Claude Opus 4.8"));
        assert_eq!(
            entries[0].reasoning_efforts.as_deref(),
            Some(
                ["low", "medium", "high", "xhigh", "max"]
                    .map(String::from)
                    .as_slice()
            )
        );
        assert_eq!(entries[1].id, "anthropic/claude-sonnet-5");
    }

    #[test]
    fn anthropic_reasoning_efforts_preserve_the_4_6_boundary() {
        assert_eq!(
            anthropic_reasoning_efforts("claude-opus-5"),
            ["low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(
            anthropic_reasoning_efforts("claude-sonnet-4-6"),
            ["low", "medium", "high", "max"]
        );
        for model in [
            "claude-haiku-4-5-20251001",
            "claude-opus-4-20250514",
            "unknown-model",
        ] {
            assert!(anthropic_reasoning_efforts(model).is_empty(), "{model}");
        }
    }

    #[test]
    fn openai_envelope_maps_to_namespaced_entries() {
        let body = serde_json::json!({"object": "list", "data": [
            {"id": "gpt-5.6", "object": "model", "created": 0, "owned_by": "apitoken"},
            {"id": "gpt-5.5", "object": "model", "created": 0, "owned_by": "apitoken"}
        ]});
        let entries = parse_openai(&body);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "openai/gpt-5.6");
        assert_eq!(entries[0].alias, "gpt-5.6");
        assert_eq!(entries[0].display_name, None);
    }

    #[test]
    fn gemini_envelope_strips_models_prefix() {
        let body = serde_json::json!({"models": [
            {"name": "models/gemini-2.5-pro", "displayName": "Gemini 2.5 Pro",
             "supportedGenerationMethods": ["generateContent"]},
            {"name": "models/gemini-2.5-flash", "displayName": "Gemini 2.5 Flash"}
        ]});
        let entries = parse_gemini(&body);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "google/gemini-2.5-pro");
        assert_eq!(entries[0].alias, "gemini-2.5-pro");
        assert_eq!(entries[0].display_name.as_deref(), Some("Gemini 2.5 Pro"));
    }

    #[test]
    fn parsers_skip_malformed_items_and_bad_shapes() {
        assert!(parse_anthropic(&serde_json::json!({"unexpected": true})).is_empty());
        assert!(parse_openai(&serde_json::json!({"data": "not-an-array"})).is_empty());
        assert!(parse_gemini(&serde_json::json!({"models": [{"noName": 1}]})).is_empty());
        let mixed = serde_json::json!({"data": [
            {"id": ""}, {"id": "  "}, {"id": "claude-haiku-4-5"}, {"no_id": true}
        ]});
        let entries = parse_anthropic(&mixed);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "claude-haiku-4-5");
    }

    #[test]
    fn dedup_keeps_first_occurrence_order() {
        let e = |id: &str| ("anthropic".to_string(), CatalogEntry {
            id: id.into(), alias: "a".into(), display_name: None,
            reasoning_efforts: None });
        let deduped = dedup(vec![e("x/1"), e("x/2"), e("x/1"), e("x/3")]);
        let ids: Vec<_> = deduped.iter().map(|(_, e)| e.id.as_str()).collect();
        assert_eq!(ids, ["x/1", "x/2", "x/3"]);
    }

    #[test]
    fn find_resolves_namespaced_id_and_native_alias() {
        let entries = vec![
            ("anthropic".to_string(), CatalogEntry {
                id: "anthropic/claude-opus-4-8".into(),
                alias: "claude-opus-4-8".into(), display_name: None,
                reasoning_efforts: None }),
            ("openai".to_string(), CatalogEntry {
                id: "openai/gpt-5.6".into(), alias: "gpt-5.6".into(), display_name: None,
                reasoning_efforts: None }),
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
            alias: "claude-opus-4-8".into(),
            display_name: Some("Claude Opus 4.8".into()),
            reasoning_efforts: Some(
                ["low", "medium", "high", "xhigh", "max"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            ),
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
        let bare = CatalogEntry { id: "openai/gpt-5.6".into(), alias: "gpt-5.6".into(), display_name: None, reasoning_efforts: None };
        assert!(bare.to_json("openai").get("name").is_none());
    }
}
