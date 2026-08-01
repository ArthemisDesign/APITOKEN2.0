//! Model-based dispatch для `POST /v1/messages` — этап 5.1
//! docs/engine/UNIFIED_ROUTER.md (решение 1 «перевод живёт в плоскостях»,
//! решение 6 «этап 5 зеркалит 3–4»).
//!
//! Ровно та же дисциплина, что у chat/responses-диспатчей (`chat.rs`, этап
//! 3.1; `responses.rs`, этап 4.1): router выбирает плоскость по полю `model`
//! тела запроса: явный namespace-префикс (`anthropic/…`, `openai/…`,
//! `google/…`) — напрямую, без опроса каталога; нативный alias
//! (`claude-opus-4-8`) — через кэшированный каталог. Тело дальше проксируется
//! БЕЗ изменений: для `anthropic/*` и claude-alias это байт-идентичный
//! passthrough native lane на 8790 (контракт не меняется), для `openai/*`
//! namespaced ID резолвит admission Codex-плоскости, а перевод Messages→
//! Codex Responses делает skin-адаптер плоскости
//! (`crates/forward/src/codex/skin.rs`).
//!
//! Единственная буферизация — тело ЗАПРОСА с лимитом (нужно одно поле
//! `model`); тело ОТВЕТА, включая SSE, стримится через тот же
//! `proxy::proxy_request`, что и native lanes (инвариант 4 не затронут).
//!
//! Отличие от chat/responses-диспатчей — конверт синтетических ошибок этого
//! пути: он всегда Anthropic-совместимый
//! (`{"type":"error","error":{"type":...,"message":...}}`), независимо от
//! целевой плоскости, потому что входной контракт — Messages (зеркало
//! OpenAI-конверта этапов 3–4; Claude Code восстанавливается по тексту
//! ошибки, чужой конверт оборачивать нельзя). По той же причине router-local
//! 502 на этом пути шейпится как `Lane::Anthropic` независимо от плоскости.
//!
//! `POST /v1/messages/count_tokens` dispatch НЕ использует: он остаётся
//! байт-прокси native Anthropic lane до 5.2 (token counting на Codex-плоскости
//! — внутренний вызов input_tokens-логики, роут регистрируется в плоскости,
//! не в router — см. п. 5 документа).

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::response::Response;

use crate::catalog::{self, NS_ANTHROPIC, NS_GOOGLE, NS_OPENAI};
use crate::error::{self, Lane};
use crate::{proxy, AppState};

/// Лимит буферизации тела запроса — тот же 32 MiB, что у chat/responses
/// dispatch'ей (потолок наибольшей плоскости): router не вводит более низкий
/// предел, свои лимиты плоскости применяют сами после проксирования.
const MESSAGES_BODY_LIMIT: usize = 32 * 1024 * 1024;

/// Хендлер `POST /v1/messages`.
pub async fn proxy_messages(State(state): State<Arc<AppState>>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let bytes = match to_bytes(body, MESSAGES_BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return error::invalid_messages_request("Request body exceeds the 32 MiB limit.")
        }
    };
    let model = match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(value) => match value.get("model").and_then(|m| m.as_str()) {
            Some(model) if !model.is_empty() => model.to_string(),
            _ => {
                return error::invalid_messages_request(
                    "Missing or invalid required parameter: model.",
                )
            }
        },
        Err(_) => return error::invalid_messages_request("Invalid JSON in request body."),
    };

    let lane = match catalog::namespace_lane(&model) {
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
                return error::messages_auth_rejected();
            }
            let entries = catalog::dedup(aggregate.entries);
            if entries.is_empty() {
                return error::messages_catalog_unavailable();
            }
            match catalog::find(&entries, &model) {
                Some((namespace, _)) => match namespace.as_str() {
                    NS_ANTHROPIC => Lane::Anthropic,
                    NS_OPENAI => Lane::OpenAi,
                    NS_GOOGLE => Lane::Gemini,
                    // Namespace'ы каталога фиксированы в aggregate; ветка —
                    // страховка от будущего расширения без правки dispatch'а.
                    _ => return error::messages_model_not_found(&model),
                },
                None => return error::messages_model_not_found(&model),
            }
        }
    };

    let origin = match lane {
        Lane::Anthropic => &state.cfg.anthropic_origin,
        Lane::OpenAi => &state.cfg.openai_origin,
        Lane::Gemini => &state.cfg.gemini_origin,
    };
    let req = Request::from_parts(parts, Body::from(bytes));
    // Lane::Anthropic задаёт только форму router-local 502: на universal
    // messages-пути она Anthropic-совместима независимо от выбранной плоскости
    // (входной контракт — Messages).
    proxy::proxy_request(&state.client, origin, Lane::Anthropic, req).await
}
