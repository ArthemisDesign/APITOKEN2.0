//! Model-based dispatch для `POST /v1/responses` — этап 4.1
//! docs/engine/UNIFIED_ROUTER.md (решение 1 «перевод живёт в плоскостях»).
//!
//! Ровно та же дисциплина, что у chat-диспатча (`chat.rs`, этап 3.1): router
//! выбирает плоскость по полю `model` тела запроса: явный namespace-префикс
//! (`anthropic/…`, `openai/…`, `google/…`) — напрямую, без опроса каталога;
//! нативный alias (`claude-opus-4-8`) — через кэшированный каталог. Тело
//! дальше проксируется БЕЗ изменений: namespaced ID резолвит admission
//! плоскости, перевод Responses→native делает адаптер плоскости (этап 4.1 —
//! `crates/forward/src/anthropic_responses.rs`).
//!
//! Единственная буферизация — тело ЗАПРОСА с лимитом (нужно одно поле
//! `model`); тело ОТВЕТА, включая SSE, стримится через тот же
//! `proxy::proxy_request`, что и native lanes (инвариант 4 не затронут).
//!
//! Синтетические ошибки этого пути — всегда в OpenAI-конверте, независимо от
//! целевой плоскости (как у chat-диспатча): universal-клиент говорит на
//! Responses, а адаптер плоскости отвечает тем же конвертом.
//!
//! Stored responses endpoints (`POST /v1/responses/input_tokens`,
//! `GET/DELETE /v1/responses/{id}`, `GET /v1/responses/{id}/input_items`)
//! dispatch НЕ используют: они остаются на OpenAI-плоскости (stored responses
//! — только `openai/*`, решение 5; token counting пока тоже — задокументированное
//! ограничение этапа 4.1).

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::response::Response;

use crate::catalog::{self, NS_ANTHROPIC, NS_GOOGLE, NS_OPENAI};
use crate::error::{self, Lane};
use crate::{proxy, AppState};

/// Лимит буферизации тела запроса — тот же 32 MiB, что у chat-диспатча
/// (потолок наибольшей плоскости): router не вводит более низкий предел,
/// свои лимиты плоскости применяют сами после проксирования.
const RESPONSES_BODY_LIMIT: usize = 32 * 1024 * 1024;

/// Хендлер `POST /v1/responses`.
pub async fn proxy_responses(State(state): State<Arc<AppState>>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let bytes = match to_bytes(body, RESPONSES_BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return error::invalid_chat_request(
                "Request body exceeds the 32 MiB limit.",
                None,
            )
        }
    };
    let model = match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(value) => match value.get("model").and_then(|m| m.as_str()) {
            Some(model) if !model.is_empty() => model.to_string(),
            _ => {
                return error::invalid_chat_request(
                    "Missing or invalid required parameter: model.",
                    Some("model"),
                )
            }
        },
        Err(_) => return error::invalid_chat_request("Invalid JSON in request body.", None),
    };

    let lane = match namespace_lane(&model) {
        Some(lane) => lane,
        None => {
            // Alias: плоскость определяет единый каталог (TTL-кэш + last-good,
            // как у GET /v1/models). Auth passthrough — ключ клиента verbatim.
            let auth = proxy::auth_passthrough(&parts.headers);
            let aggregate = state
                .catalog
                .aggregate(&state.client, &crate::origins(&state), &auth)
                .await;
            if aggregate.auth_rejected {
                return crate::auth_rejected_response();
            }
            let entries = catalog::dedup(aggregate.entries);
            if entries.is_empty() {
                return error::catalog_unavailable();
            }
            match catalog::find(&entries, &model) {
                Some((namespace, _)) => match namespace.as_str() {
                    NS_ANTHROPIC => Lane::Anthropic,
                    NS_OPENAI => Lane::OpenAi,
                    NS_GOOGLE => Lane::Gemini,
                    // Namespace'ы каталога фиксированы в aggregate; ветка —
                    // страховка от будущего расширения без правки dispatch'а.
                    _ => return error::model_not_found(&model),
                },
                None => return error::model_not_found(&model),
            }
        }
    };

    let origin = match lane {
        Lane::Anthropic => &state.cfg.anthropic_origin,
        Lane::OpenAi => &state.cfg.openai_origin,
        Lane::Gemini => &state.cfg.gemini_origin,
    };
    let req = Request::from_parts(parts, Body::from(bytes));
    // Lane::OpenAi задаёт только форму router-local 502: на universal-пути она
    // OpenAI-совместима независимо от выбранной плоскости.
    proxy::proxy_request(&state.client, origin, Lane::OpenAi, req).await
}

/// Плоскость по явному namespace-префиксу модели. Каталог не опрашивается:
/// admission плоскости сам резолвит namespaced ID (решение 1). Модель без
/// префикса или с неизвестным префиксом уходит в alias-поиск по каталогу.
/// Локальная копия `chat::namespace_lane` (там приватная): dispatch-правила
/// обеих universal lanes обязаны совпадать.
fn namespace_lane(model: &str) -> Option<Lane> {
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
}
