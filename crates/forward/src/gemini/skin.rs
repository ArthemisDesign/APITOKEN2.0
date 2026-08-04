//! Anthropic Messages adapter ("Anthropic Skin") поверх native Gemini generateContent core —
//! этап 5.2 docs/engine/UNIFIED_ROUTER.md (решение 6: этап 5 зеркалит этапы 3–4).
//! Gemini-зеркало Codex skin этапа 5.1 (`crate::codex::skin`): Messages-сторона словаря
//! идентична (system/messages/tools/tool_choice/thinking/capability matrix, Messages SSE на
//! выходе, Anthropic-конверт ошибок — contract-тесты обоих модулей на эквивалентном входе),
//! перевод запроса и разбор GenerateContentResponse — по правилам chat/responses-адаптеров
//! этой плоскости (`chat.rs`, этапы 3.3–3.4b; `responses.rs`, этап 4.3); общие хелперы
//! (`merge_or_push`, `function_response_value`, `map_finish_reason`, константы лимитов)
//! переиспользуются из `gemini/chat.rs` без изменения его логики.
//!
//! `POST /v1/messages` и `POST /v1/messages/count_tokens` на Gemini-плоскости. Поток запроса
//! повторяет chat/responses-адаптеры: парс Messages-запроса → перевод в GenerateContentRequest
//! JSON → внутренний `Request` на
//! `/v1beta/models/{model}:generateContent|streamGenerateContent?alt=sse|countTokens` → общий
//! [`gemini_api`] (admission, reserve, affinity, ротация, Code Assist wrapper, usage-settlement
//! — без единого изменения) → перевод ответа: GenerateContentResponse (JSON либо data-only
//! SSE) → Messages JSON / Messages SSE. Router (`crates/router/src/messages.rs`) выполняет
//! model-based dispatch `google/*` и gemini-alias'ов и проксирует тело без изменений (router
//! не менялся с 5.1).
//!
//! Перевод запроса: strip `google/`-префикса model ДО admission (как chat.rs); top-level
//! `system` (строка или text-блоки, склейка "\n\n" — как 5.1) → `systemInstruction` с одним
//! text-партом (не-дефолт `cache_control` в любом блоке → 400, как 5.1); `messages[]` →
//! contents со склейкой одноролевых общим `merge_or_push` (user/model): text-блоки →
//! text-парты (склейка через \n, как chat.rs), image-блоки: base64 source → inlineData-парт,
//! url source → `400 invalid_request` (generateContent внешние ссылки не принимает — как
//! 3.3/3.4a; на Codex-плоскости 5.1 url принимается — плоскостное отличие), assistant → роль
//! model, `tool_use` → functionCall-парт (`input` object → `args` object — НЕ JSON-строка,
//! отличие от Codex-стороны 5.1; невалидный input → 400), `tool_result` → functionResponse-парт
//! в user-content (имя восстанавливается по карте id→name tool_use-блоков этой же истории —
//! паттерн 3.3; tool_result без пары → 400, как 4.3; `is_error` принимается и игнорируется —
//! текст и так это передаёт); thinking/redacted_thinking входа выбрасываются (решение 6);
//! `tools[]` → `[{"functionDeclarations": …}]` (только custom tools; `input_schema` →
//! `parameters` как есть, отсутствующая — опускается, как у `function_declaration` chat.rs);
//! `tool_choice` → `toolConfig.functionCallingConfig` (auto не вставляется — generateContent и
//! так AUTO; any → ANY, none → NONE, tool → ANY + `allowedFunctionNames`;
//! `disable_parallel_tool_use: true` → 400 — generateContent не умеет ограничивать
//! параллельные вызовы, стойка 4.3); `max_tokens` → `generationConfig.maxOutputTokens`
//! (обязателен для /v1/messages, для count_tokens опционален — официальный endpoint его не
//! требует); sampling-контроли `temperature`/`top_p`/`top_k` проксируются в generationConfig
//! (generateContent их умеет — как chat.rs; на Codex-плоскости 5.1 они игнорируются, т.к.
//! транспорт их не умеет — плоскостное отличие); `stop_sequences` →
//! `generationConfig.stopSequences` (нативное исполнение, как `stop` у chat.rs; пустые строки
//! выбрасываются — как 5.1; больше публичного лимита 5 → 400). Отличие от 5.1: stop_reason
//! `stop_sequence` на этой плоскости не различим (нативный stop даёт тот же finishReason STOP,
//! что и обычная остановка) — `stop_reason`/`stop_sequence` при срабатывании stop_sequences
//! будет `end_turn`/null, текст обрезан апстримом корректно.
//! `thinking` → `generationConfig.thinkingConfig` по маппингу 5.1 (те же пороги budget→level:
//! `disabled`/`adaptive` → поле не вставляется; `enabled` с budget < 4096 → "low", < 16384 →
//! "medium", иначе "high"; budget < 1024 → 400) + `includeThoughts: true` (как 3.4b).
//!
//! Capability matrix (решение 3, fail-closed modulo defaults) — те же 4 правила, что в 5.1:
//! не-дефолтный `cache_control` где угодно (system, content-блоки, tools), `context_management`,
//! `mcp_servers`, `container`, `output_config` → `400 invalid_request_error` с именем параметра
//! в message (в Anthropic-конверте поля param нет). `metadata` (включая `user_id`) принимается
//! и игнорируется — Claude Code шлёт `metadata.user_id` (та же leniency, что 5.1). НЕИЗВЕСТНЫЕ
//! top-level поля отклоняются (закрытый список, отличие от Codex-плоскости 5.1 — как chat.rs):
//! Code Assist wrapper пропускает только известные поля GenerateContentRequest, поэтому
//! «проксирование» было бы молчаливым выбрасыванием — честный 400 лучше.
//!
//! Перевод ответа — словарь 5.1 поверх форм 3.3/4.3: `msg_*` id, model из `modelVersion`
//! (фолбэк — запрошенная), text-парты кандидата склеиваются в ОДИН text-блок на позиции
//! первого text-парта (без текста блок не создаётся), thought-парты (`"thought": true`) →
//! thinking-блоки БЕЗ signature (thoughtSignature-only парт пропускается — решение 4/6),
//! functionCall-парты → tool_use-блоки (`args` object → `input` object как есть; id
//! синтезируется `toolu_<name>[_N]` — на private wire functionCall.id не приезжает, схема
//! `callu_<name>[_N]` chat.rs с префиксом 5.1); usageMetadata → Messages usage:
//! `promptTokenCount` → input_tokens, `candidatesTokenCount` + `thoughtsTokenCount` →
//! output_tokens (та же сумма, что тарифицирует metering в chat.rs), `cachedContentTokenCount`
//! → `cache_read_input_tokens` (только при >0), thoughts →
//! `output_tokens_details.thinking_tokens` (только при >0); finishReason/blockReason →
//! stop_reason через общий `map_finish_reason`: functionCall в ответе → `tool_use`,
//! MAX_TOKENS → `max_tokens`, SAFETY/RECITATION/BLOCKLIST/PROHIBITED_CONTENT/SPII → `refusal`
//! (у Messages есть stop_reason refusal — тот же класс, что content_filter у 3.3/4.3),
//! остальное → `end_turn`; блокировка промпта на входе (promptFeedback.blockReason) → refusal
//! с пустым content.
//!
//! Stream (GenerateContentResponse data-only SSE → Messages SSE, транслятор СНАРУЖИ
//! `gemini_api()` как у chat/responses-адаптеров; usage-settlement внутри плоскости уже
//! протапало оригинальные байты): `message_start` с нулевым usage (authoritative usage только
//! в `message_delta` — как 5.1) → плотные content_block_start/delta/stop (смена типа контента
//! закрывает предыдущий блок; thought-дельты → thinking_delta) → `message_delta`
//! (stop_reason + usage) → `message_stop`. functionCall приходит целиком (аргумент-дельт на
//! wire нет) → tool_use-блок с ПОЛНЫМ input в content_block_start и сразу content_block_stop,
//! БЕЗ input_json_delta (как 3.3 отдаёт tool_calls одним чанком; SDK накапливает input начиная
//! со start-значения). Нормальное завершение Gemini-стрима — provider
//! finishReason/blockReason + чистый EOF (message_stop на wire нет): терминальная пара
//! message_delta/message_stop эмитится на EOF. EOF без terminal evidence — отказ.
//! Heartbeat `event: ping` каждые 15 с —
//! как 5.1 (на Gemini wire ping-кадров нет, генерируется локально). Mid-stream error-кадр
//! плоскости `{error:{code,message,status}}` и транспортный сбой → `event: error`
//! Anthropic-формата (тип по google.rpc status: RESOURCE_EXHAUSTED → rate_limit_error,
//! UNAUTHENTICATED/PERMISSION_DENIED → authentication_error, иначе api_error — как 3.3) и
//! конец стрима; malformed known provider shape → `event: error`, неизвестные дополнительные
//! JSON-поля допускаются.
//!
//! Все ответы этого пути — Anthropic-совместимый конверт `{"type":"error","error":{…}}`,
//! включая ошибки: синтетические ошибки плоскости и пасsthrough-ошибки апстрима (Google-конверт
//! `{"error":{"code","message","status"}}`) пересобираются с сохранением HTTP-статуса и
//! `Retry-After` (503 → 529 `overloaded_error`, 402 LowBalance сохраняется — Claude Code
//! восстанавливается по тексту ошибки); нативный `400 API_KEY_INVALID` →
//! `401 authentication_error` (как 3.3/4.3). Заголовки `anthropic-version`/`anthropic-beta` и
//! `?beta=true` tolerated и никогда не проксируются upstream (внутренний запрос собирается из
//! переведённого тела, заголовки клиента читает только authorize/affinity).
//!
//! count_tokens: тот же Messages parse + перевод в GenerateContentRequest → внутренний запрос
//! `/v1beta/models/{model}:countTokens` через общий `gemini_api()` (нативная операция quota-free
//! и без reserve — см. `gemini/pool.rs`; `max_tokens` там опционален и опускается) →
//! `totalTokens` → `{"input_tokens": N}`. Dispatch в router остаётся native-Anthropic
//! байт-прокси (его перевод на dispatch — отдельный поздний подэтап, вне 5.2).
//!
//! Code Assist compatibility: `input_schema` рекурсивно санитизируется от неподдерживаемых
//! `$schema` и числовых `exclusiveMinimum`/`exclusiveMaximum`; replayed `tool_use` становится
//! functionCall-партом с принятым Google context-engineering `thoughtSignature` marker. Это
//! сохраняет stateless Messages skin и делает многоходовый tool cycle исполнимым, не раскрывая
//! реальные opaque provider signatures в ответах.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::BytesMut;
use futures_util::{Stream, StreamExt};
use serde_json::{json, Map, Value};

use super::chat::{
    function_response_value, map_finish_reason, merge_or_push, replayed_function_call_part,
    CHAT_BODY_LIMIT, RESPONSE_BODY_LIMIT,
};
use super::gemini_api;
use crate::codex::new_id;
use crate::gemini_schema;
use crate::gemini_stream::GeminiStreamState;
use crate::proxy::{
    read_body_limited, with_not_started, without_not_started, BodyReadError, TerminalErrorReason,
};
use crate::state::AppState;
use crate::validation::optional_bool;

/// Интервал heartbeat `event: ping` в Messages SSE — как у Codex skin 5.1
/// (на Gemini wire ping-кадров нет, генерируется локально).
const SSE_PING_INTERVAL: Duration = Duration::from_secs(15);

// ---------- Anthropic-конверт ошибок ----------

/// Anthropic-конверт ошибки (`{"type":"error","error":{…}}`) с privacy-safe статическим
/// audit-reason — форма `local_err` нативной плоскости (`proxy.rs`), как у Codex skin 5.1.
fn skin_error(
    status: StatusCode,
    kind: &'static str,
    message: impl Into<String>,
    reason: &'static str,
    retry_after: Option<u64>,
) -> Response {
    let body = json!({"type": "error", "error": {"type": kind, "message": message.into()}});
    let mut response = (status, axum::Json(body)).into_response();
    if let Some(seconds) = retry_after {
        if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
    }
    response
        .extensions_mut()
        .insert(TerminalErrorReason(reason));
    // Все прямые вызовы skin_error происходят до границы доставки: локальная валидация
    // ещё не запускала native request, а convert_error_response получает только не-2xx
    // gemini_api, чей admission гарантированно refund/cancel. Ошибки разбора уже успешного
    // 2xx и fallback сборки SSE снимают заголовок через without_not_started ниже: там
    // request уже мог стать billable.
    with_not_started(response)
}

/// 400 ошибки валидации адаптера; имя параметра живёт внутри message (в Anthropic-конверте
/// поля param нет — как 5.1).
fn invalid_request(message: impl Into<String>) -> Response {
    skin_error(
        StatusCode::BAD_REQUEST,
        "invalid_request_error",
        message,
        "invalid_messages_request",
        None,
    )
}

/// Capability matrix / закрытый список: текст идентичен chat.rs (`unsupported_parameter`),
/// конверт — Anthropic (invalid_request_error, как у matrix-ошибок 5.1).
fn unsupported_parameter(param: &str) -> Response {
    invalid_request(format!(
        "Unsupported parameter: '{param}' is not supported with this endpoint."
    ))
}

/// Anthropic-тип ошибки по HTTP-статусу — зеркало `anthropic_error` Codex skin 5.1
/// (аутентичные триплеты нативной плоскости `proxy.rs`).
fn anthropic_error_parts(
    status: StatusCode,
    message: String,
) -> (StatusCode, &'static str, String) {
    match status.as_u16() {
        400 => (StatusCode::BAD_REQUEST, "invalid_request_error", message),
        401 => (StatusCode::UNAUTHORIZED, "authentication_error", message),
        402 => (
            StatusCode::PAYMENT_REQUIRED,
            "invalid_request_error",
            message,
        ),
        404 => (StatusCode::NOT_FOUND, "not_found_error", message),
        429 => (StatusCode::TOO_MANY_REQUESTS, "rate_limit_error", message),
        503 => (
            StatusCode::from_u16(529).expect("529 is a valid HTTP status code"),
            "overloaded_error",
            "Overloaded".to_string(),
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "api_error",
            "Internal server error".to_string(),
        ),
    }
}

/// Перевод не-200 ответа `gemini_api()` из Google-конверта в Anthropic-конверт. Статус и
/// `Retry-After` сохраняются; audit-reason пробрасывается в extension. Особый случай — как в
/// chat.rs: нативный `400 API_KEY_INVALID` (reason `invalid_key`) → `401 authentication_error`.
async fn convert_error_response(upstream: Response) -> Response {
    let status = upstream.status();
    let reason = upstream
        .extensions()
        .get::<TerminalErrorReason>()
        .map(|r| r.0)
        .unwrap_or("upstream_error_response");
    let retry_after = upstream
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let bytes = to_bytes(upstream.into_body(), RESPONSE_BODY_LIMIT)
        .await
        .unwrap_or_default();
    let parsed = serde_json::from_slice::<Value>(&bytes).ok();
    let message = parsed
        .as_ref()
        .and_then(|v| v.get("error"))
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("The provider returned an error.")
        .to_string();
    let (status, reason) = if reason == "invalid_key" {
        (StatusCode::UNAUTHORIZED, "invalid_key")
    } else {
        (status, reason)
    };
    let (status, kind, message) = anthropic_error_parts(status, message);
    skin_error(status, kind, message, reason, retry_after)
}

// ---------- перевод запроса (Messages → GenerateContentRequest) ----------

/// Результат перевода Messages-запроса: тело GenerateContentRequest и параметры, нужные для
/// внутреннего запроса и перевода ответа.
#[derive(Debug)]
struct Translated {
    body: Value,
    /// Запрошенная модель с уже снятым `google/`-префиксом — фолбэк для поля `model`
    /// ответа, если плоскость не вернула `modelVersion`.
    model: String,
    stream: bool,
}

/// Перевод Messages-запроса в GenerateContentRequest JSON. Ошибки — готовые Anthropic-shaped
/// ответы (400). Когда `require_max_tokens` false (count_tokens), отсутствующий `max_tokens`
/// tolerated: официальный token-counting endpoint его не требует, а нативный countTokens он не
/// нужен (generationConfig тогда опускается, если пуст).
fn translate_messages_request(
    value: Value,
    require_max_tokens: bool,
) -> Result<Translated, Response> {
    let mut object = match value {
        Value::Object(object) => object,
        _ => return Err(invalid_request("Request body must be a JSON object.")),
    };

    check_capability_matrix(&object)?;

    let model = match object.remove("model") {
        Some(Value::String(model)) => model,
        _ => {
            return Err(invalid_request(
                "Missing or invalid required parameter: model.",
            ))
        }
    };
    // Namespaced ID резолвится здесь, а не в router: после strip'а admission плоскости видит
    // нативный публичный id (закрытый allowlist config.models) — зеркало strip'а 5.1.
    let model = model.strip_prefix("google/").unwrap_or(&model).to_string();
    if model.is_empty() {
        return Err(invalid_request(
            "Missing or invalid required parameter: model.",
        ));
    }

    let max_tokens = match object.remove("max_tokens") {
        Some(value) => value.as_u64().filter(|tokens| *tokens > 0).ok_or_else(|| {
            invalid_request("Invalid type for parameter: max_tokens must be a positive integer.")
        })?,
        None if require_max_tokens => {
            return Err(invalid_request(
                "Missing or invalid required parameter: max_tokens.",
            ))
        }
        None => 0,
    };

    let system = translate_system(object.remove("system"))?;

    let messages = match object.remove("messages") {
        Some(Value::Array(messages)) if !messages.is_empty() => messages,
        _ => {
            return Err(invalid_request(
                "Missing or invalid required parameter: messages must be a non-empty array.",
            ))
        }
    };
    let contents = translate_messages(&messages)?;
    if contents.is_empty() {
        return Err(invalid_request(
            "messages must contain at least one text, image, tool_use or tool_result block.",
        ));
    }

    let stream = optional_bool(&object, "stream")
        .map_err(|_| invalid_request("stream must be a boolean."))?
        .unwrap_or(false);

    let mut generation_config = Map::new();
    if max_tokens > 0 {
        generation_config.insert("maxOutputTokens".to_string(), Value::from(max_tokens));
    }
    // Honored sampling-контроли (generateContent их умеет — как chat.rs).
    for (messages_key, native_key) in [
        ("temperature", "temperature"),
        ("top_p", "topP"),
        ("top_k", "topK"),
    ] {
        if let Some(value) = object.get(messages_key).filter(|v| !v.is_null()) {
            generation_config.insert(native_key.to_string(), value.clone());
        }
    }
    if let Some(stop) = translate_stop_sequences(object.get("stop_sequences"))? {
        generation_config.insert("stopSequences".to_string(), stop);
    }
    // thinking → thinkingConfig по маппингу 5.1 (те же пороги budget→level) +
    // includeThoughts: true (как 3.4b) — соседнее поле того же generationConfig.
    if let Some(thinking) = translate_thinking(object.get("thinking"))? {
        generation_config.insert("thinkingConfig".to_string(), thinking);
    }

    let mut body = Map::new();
    body.insert("contents".to_string(), Value::Array(contents));
    if let Some(system) = system {
        body.insert(
            "systemInstruction".to_string(),
            json!({"parts": [{"text": system}]}),
        );
    }
    if !generation_config.is_empty() {
        body.insert(
            "generationConfig".to_string(),
            Value::Object(generation_config),
        );
    }
    if let Some(tools) = object.get("tools").filter(|v| !v.is_null()) {
        let declarations = translate_tools(tools)?;
        if !declarations.is_empty() {
            body.insert(
                "tools".to_string(),
                json!([{"functionDeclarations": declarations}]),
            );
        }
    }
    if let Some(config) = translate_tool_choice(&object)? {
        body.insert("toolConfig".to_string(), config);
    }
    // Закрытый список (отличие от Codex-плоскости 5.1 — как chat.rs): неизвестные top-level
    // поля Code Assist wrapper выбросит молча — честный 400 вместо этого. Известные (honored
    // или matrix-проверенные выше) остаются в object — это норма.
    if let Some(unknown) = object.keys().find(|k| !KNOWN_KEYS.contains(&k.as_str())) {
        return Err(unsupported_parameter(unknown));
    }

    Ok(Translated {
        body: Value::Object(body),
        model,
        stream,
    })
}

/// Известные Messages-параметры: honored (переводятся), ignored (`metadata`) или matrix
/// (отклоняются при не-дефолте). Всё вне списка — `400 unsupported_parameter` (закрытый
/// список, см. translate_messages_request).
const KNOWN_KEYS: &[&str] = &[
    "model",
    "messages",
    "max_tokens",
    "system",
    "stream",
    "stop_sequences",
    "temperature",
    "top_p",
    "top_k",
    "tools",
    "tool_choice",
    "thinking",
    // Принимается и игнорируется (Claude Code шлёт metadata.user_id) — leniency 5.1.
    "metadata",
    // Capability matrix: отклонены при не-дефолте, при дефолте — сняты.
    "context_management",
    "mcp_servers",
    "container",
    "output_config",
];

/// Capability matrix (решение 3) — те же 4 правила, что у Codex skin 5.1: параметры, которые
/// generateContent не умеет, с не-дефолтным значением → `400 invalid_request_error` с именем
/// параметра. Порядок правил определяет, какой параметр назовёт ошибка.
fn check_capability_matrix(object: &Map<String, Value>) -> Result<(), Response> {
    let rules: [(&str, fn(&Value) -> bool); 4] = [
        ("context_management", |value| value.is_null()),
        ("mcp_servers", |value| {
            value.is_null() || value.as_array().is_some_and(Vec::is_empty)
        }),
        ("container", |value| value.is_null()),
        ("output_config", |value| value.is_null()),
    ];
    for (param, is_default) in rules {
        if let Some(value) = object.get(param) {
            if !is_default(value) {
                return Err(unsupported_parameter(param));
            }
        }
    }
    // Закрытый список: имена известных параметров проверены, неизвестные — 400.
    debug_assert!(rules.iter().all(|(name, _)| KNOWN_KEYS.contains(name)));
    Ok(())
}

/// Не-дефолтный `cache_control` отклоняется везде (system, content-блоки, tools): Gemini
/// prompt caching не управляется клиентскими breakpoints (как 5.1 у Codex).
fn reject_cache_control(block: &Value, param: &str) -> Result<(), Response> {
    if block
        .get("cache_control")
        .is_some_and(|value| !value.is_null())
    {
        return Err(invalid_request(format!(
            "Unsupported parameter: 'cache_control' is not supported with this endpoint (in {param})."
        )));
    }
    Ok(())
}

/// Top-level `system` → склеенный instruction-текст: строка либо массив text-блоков через
/// "\n\n" (склейка 5.1). Любой другой тип блока или не-дефолтный `cache_control` → 400.
fn translate_system(value: Option<Value>) -> Result<Option<String>, Response> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok((!text.is_empty()).then_some(text)),
        Some(Value::Array(blocks)) => {
            let mut parts = Vec::with_capacity(blocks.len());
            for (index, block) in blocks.iter().enumerate() {
                let param = format!("system.{index}");
                reject_cache_control(block, &param)?;
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let text = block.get("text").and_then(Value::as_str).ok_or_else(|| {
                            invalid_request(format!(
                                "System text block requires text ({param}.text)."
                            ))
                        })?;
                        parts.push(text.to_string());
                    }
                    _ => {
                        return Err(invalid_request(
                            "Invalid system content: only text blocks are supported.",
                        ))
                    }
                }
            }
            let joined = parts.join("\n\n");
            Ok((!joined.is_empty()).then_some(joined))
        }
        _ => Err(invalid_request(
            "Invalid type for parameter: system must be a string or an array of text blocks.",
        )),
    }
}

/// Messages conversation → Gemini contents. Подряд идущие contents с одинаковой Gemini-ролью
/// склеиваются общим `merge_or_push` (user/model): generateContent ждёт чередования
/// user/model, а серии functionResponse — один user-content. Карта id→name строится по
/// tool_use-блокам этой же истории за один проход (functionResponse ссылается по имени —
/// паттерн 3.3/4.3).
fn translate_messages(messages: &[Value]) -> Result<Vec<Value>, Response> {
    let mut contents: Vec<Value> = Vec::new();
    let mut call_names: HashMap<String, String> = HashMap::new();
    for (index, message) in messages.iter().enumerate() {
        let object = message.as_object().ok_or_else(|| {
            invalid_request(format!(
                "Each message must be an object (messages.{index})."
            ))
        })?;
        let param = format!("messages.{index}");
        let role = object.get("role").and_then(Value::as_str).ok_or_else(|| {
            invalid_request(format!(
                "Each message requires a valid role ({param}.role)."
            ))
        })?;
        let parts = match role {
            "user" => user_content_parts(object.get("content"), &param, &call_names)?,
            "assistant" => assistant_content_parts(object.get("content"), &param, &mut call_names)?,
            _ => {
                return Err(invalid_request(format!(
                    "Message role {role:?} is not supported ({param}.role)."
                )))
            }
        };
        // Сообщение из одних выброшенных thinking-блоков не порождает content (пустой parts
        // на wire невалиден); полностью пустая история ловится проверкой вызывающего.
        if !parts.is_empty() {
            merge_or_push(&mut contents, role_gemini(role), parts);
        }
    }
    Ok(contents)
}

/// Gemini-роль Messages-роли (assistant → model).
fn role_gemini(role: &str) -> &'static str {
    if role == "assistant" {
        "model"
    } else {
        "user"
    }
}

/// Контент user-сообщения → Gemini-парты. Текстовые блоки склеиваются в text-парты (через
/// \n, как chat.rs); image и tool_result разрывают склейку, порядок блоков сохраняется.
fn user_content_parts(
    content: Option<&Value>,
    param: &str,
    call_names: &HashMap<String, String>,
) -> Result<Vec<Value>, Response> {
    let content = content.ok_or_else(|| {
        invalid_request(format!("Message content is required ({param}.content)."))
    })?;
    match content {
        Value::String(text) => {
            if text.is_empty() {
                return Err(invalid_request(format!(
                    "Message content must not be empty ({param}.content)."
                )));
            }
            Ok(vec![json!({"text": text})])
        }
        Value::Array(blocks) => {
            let mut out: Vec<Value> = Vec::new();
            let mut text = String::new();
            for (block_index, block) in blocks.iter().enumerate() {
                let block_param = format!("{param}.content.{block_index}");
                reject_cache_control(block, &block_param)?;
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let segment =
                            block.get("text").and_then(Value::as_str).ok_or_else(|| {
                                invalid_request(format!(
                                    "Text block requires text ({block_param}.text)."
                                ))
                            })?;
                        if !text.is_empty() && !segment.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(segment);
                    }
                    Some("image") => {
                        flush_text_part(&mut out, &mut text);
                        out.push(translate_image_block(block, &block_param)?);
                    }
                    Some("tool_result") => {
                        flush_text_part(&mut out, &mut text);
                        out.push(translate_tool_result(block, &block_param, call_names)?);
                    }
                    // thinking/redacted_thinking во входе выбрасываются (решение 6: реплея
                    // thinking для non-Claude моделей нет).
                    Some("thinking" | "redacted_thinking") => {}
                    Some(other) => {
                        return Err(invalid_request(format!(
                            "Content block type {other:?} is not supported ({block_param}.type)."
                        )))
                    }
                    None => {
                        return Err(invalid_request(format!(
                            "Content block requires a type ({block_param}.type)."
                        )))
                    }
                }
            }
            flush_text_part(&mut out, &mut text);
            Ok(out)
        }
        _ => Err(invalid_request(format!(
            "Message content must be a string or a content-block array ({param}.content)."
        ))),
    }
}

/// Сбросить накопленный текст склейки в text-парт (склейка chat.rs: пустой текст партов не
/// порождает).
fn flush_text_part(out: &mut Vec<Value>, text: &mut String) {
    if !text.is_empty() {
        out.push(json!({"text": std::mem::take(text)}));
    }
}

/// Messages image-блок → inlineData-парт. Только base64 source — generateContent внешние
/// http(s) ссылки не принимает (fileData требует File API upload, которого на плоскости нет),
/// поэтому url source → честный 400 (как 3.3/3.4a), а не молчаливое выбрасывание.
fn translate_image_block(block: &Value, param: &str) -> Result<Value, Response> {
    let source = block
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            invalid_request(format!(
                "Image block requires a source object ({param}.source)."
            ))
        })?;
    let required = |field: &str| {
        source
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                invalid_request(format!(
                    "Image source requires a non-empty {field} ({param}.source.{field})."
                ))
            })
    };
    match source.get("type").and_then(Value::as_str) {
        Some("base64") => {
            let media_type = required("media_type")?;
            if !media_type.starts_with("image/") {
                return Err(invalid_request(format!(
                    "Image source media_type must be an image MIME type ({param}.source.media_type)."
                )));
            }
            let data = required("data")?;
            Ok(json!({"inlineData": {"mimeType": media_type, "data": data}}))
        }
        Some("url") => Err(invalid_request(format!(
            "Gemini lane supports only base64 image sources (external image URLs are not supported) ({param}.source.type)."
        ))),
        _ => Err(invalid_request(format!(
            "Image source type is not supported: only base64 is supported ({param}.source.type)."
        ))),
    }
}

/// user `tool_result`-блок → functionResponse-парт: имя восстанавливается по `tool_use_id`
/// из карты tool_use-блоков этой же истории (functionResponse ссылается по имени, а не по id
/// — паттерн 3.3/4.3, в отличие от Codex-стороны 5.1 pairing валидируется); content строка
/// как есть либо text-блоки через \n, нетекстовые части → 400; output — общий
/// `function_response_value` (JSON разбирается, не-JSON заворачивается строкой). `is_error`
/// не имеет эквивалента в generateContent и принимается и игнорируется (как 5.1).
fn translate_tool_result(
    block: &Value,
    param: &str,
    call_names: &HashMap<String, String>,
) -> Result<Value, Response> {
    let tool_use_id = block
        .get("tool_use_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            invalid_request(format!(
                "tool_result requires a non-empty tool_use_id ({param}.tool_use_id)."
            ))
        })?;
    let name = call_names.get(tool_use_id).ok_or_else(|| {
        invalid_request(format!(
            "tool_result tool_use_id has no matching tool_use in this history ({param}.tool_use_id)."
        ))
    })?;
    let output = match block.get("content") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => {
            let mut texts = Vec::with_capacity(parts.len());
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") => texts.push(
                        part.get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    ),
                    _ => {
                        return Err(invalid_request(format!(
                            "tool_result content supports text blocks only ({param}.content)."
                        )))
                    }
                }
            }
            texts.join("\n")
        }
        _ => {
            return Err(invalid_request(format!(
                "tool_result content must be a string or an array of text blocks ({param}.content)."
            )))
        }
    };
    Ok(json!({
        "functionResponse": {"name": name, "response": function_response_value(&output)}
    }))
}

/// Контент assistant-сообщения → Gemini-парты: text-блоки → text-парты (пустые пропускаются,
/// как 5.1), tool_use → functionCall-парты. Попутно регистрирует id→name для tool_result.
fn assistant_content_parts(
    content: Option<&Value>,
    param: &str,
    call_names: &mut HashMap<String, String>,
) -> Result<Vec<Value>, Response> {
    let content = content.ok_or_else(|| {
        invalid_request(format!("Message content is required ({param}.content)."))
    })?;
    match content {
        Value::String(text) => {
            if text.is_empty() {
                return Ok(Vec::new());
            }
            Ok(vec![json!({"text": text})])
        }
        Value::Array(blocks) => {
            let mut out: Vec<Value> = Vec::new();
            for (block_index, block) in blocks.iter().enumerate() {
                let block_param = format!("{param}.content.{block_index}");
                reject_cache_control(block, &block_param)?;
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let text = block.get("text").and_then(Value::as_str).ok_or_else(|| {
                            invalid_request(format!(
                                "Text block requires text ({block_param}.text)."
                            ))
                        })?;
                        if !text.is_empty() {
                            out.push(json!({"text": text}));
                        }
                    }
                    Some("tool_use") => {
                        out.push(translate_tool_use(block, &block_param, call_names)?);
                    }
                    // thinking/redacted_thinking во входе выбрасываются (решение 6).
                    Some("thinking" | "redacted_thinking") => {}
                    Some(other) => {
                        return Err(invalid_request(format!(
                            "Content block type {other:?} is not supported ({block_param}.type)."
                        )))
                    }
                    None => {
                        return Err(invalid_request(format!(
                            "Content block requires a type ({block_param}.type)."
                        )))
                    }
                }
            }
            Ok(out)
        }
        _ => Err(invalid_request(format!(
            "Message content must be a string or a content-block array ({param}.content)."
        ))),
    }
}

/// assistant `tool_use`-блок → functionCall-парт model-content'а: `id` регистрируется в карте
/// для tool_result (дубликат → 400, как 5.1), `input` object → `args` object КАК ЕСТЬ (НЕ
/// JSON-строка — отличие от Codex-стороны 5.1: functionCall.args — объект на wire).
fn translate_tool_use(
    block: &Value,
    param: &str,
    call_names: &mut HashMap<String, String>,
) -> Result<Value, Response> {
    let required = |field: &str| {
        block
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                invalid_request(format!(
                    "tool_use requires a non-empty {field} ({param}.{field})."
                ))
            })
    };
    let id = required("id")?;
    let name = required("name")?;
    if call_names
        .insert(id.to_string(), name.to_string())
        .is_some()
    {
        return Err(invalid_request(format!(
            "Duplicate tool_use id {id:?} ({param}.id)."
        )));
    }
    let args = match block.get("input") {
        None | Some(Value::Null) => json!({}),
        Some(value @ Value::Object(_)) => value.clone(),
        _ => {
            return Err(invalid_request(format!(
                "tool_use input must be an object ({param}.input)."
            )))
        }
    };
    Ok(replayed_function_call_part(name, args))
}

/// Messages `tools[]` → массив functionDeclarations: только custom tools (дефолтный type);
/// `input_schema` → поддерживаемый Code Assist subset `parameters`, отсутствующая опускается
/// (как у общего `function_declaration` chat.rs). Server tools (web_search и пр.),
/// не-дефолтный `cache_control`, не-объект `input_schema` → 400 (как 5.1).
fn translate_tools(value: &Value) -> Result<Vec<Value>, Response> {
    let tools = value
        .as_array()
        .ok_or_else(|| invalid_request("Invalid type for parameter: tools must be an array."))?;
    let mut declarations = Vec::with_capacity(tools.len());
    for (index, tool) in tools.iter().enumerate() {
        let param = format!("tools.{index}");
        let object = tool
            .as_object()
            .ok_or_else(|| invalid_request(format!("Each tool must be an object ({param}).")))?;
        match object.get("type").and_then(Value::as_str) {
            None | Some("custom") => {}
            Some(_) => {
                return Err(invalid_request(format!(
                    "Unsupported parameter: only custom tools are supported with this endpoint ({param}.type)."
                )))
            }
        }
        reject_cache_control(tool, &param)?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                invalid_request(format!("Tool requires a non-empty name ({param}.name)."))
            })?;
        let mut declaration = json!({"name": name});
        match object.get("description") {
            None | Some(Value::Null) => {}
            Some(Value::String(description)) => {
                declaration["description"] = Value::String(description.clone())
            }
            Some(_) => {
                return Err(invalid_request(format!(
                    "Invalid type for parameter: tools description must be a string ({param}.description)."
                )))
            }
        }
        match object.get("input_schema") {
            None | Some(Value::Null) => {}
            Some(schema) if schema.is_object() => {
                let schema_path = format!("{param}.input_schema");
                declaration["parameters"] = gemini_schema::translate(schema, &schema_path)
                    .map_err(|error| invalid_request(error.message()))?
            }
            Some(_) => {
                return Err(invalid_request(format!(
                    "Invalid type for parameter: tools input_schema must be an object ({param}.input_schema)."
                )))
            }
        }
        declarations.push(declaration);
    }
    Ok(declarations)
}

/// Messages `tool_choice` → `toolConfig.functionCallingConfig`: auto → omitted (дефолт
/// generateContent AUTO — как chat.rs), any → ANY, none → NONE, tool → ANY +
/// `allowedFunctionNames`. `disable_parallel_tool_use: true` → 400: generateContent не умеет
/// ограничивать параллельные вызовы (стойка 4.3; у Codex-плоскости 5.1 есть
/// `parallel_tool_calls` — плоскостное отличие).
fn translate_tool_choice(object: &Map<String, Value>) -> Result<Option<Value>, Response> {
    let Some(choice) = object.get("tool_choice").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let choice = choice.as_object().ok_or_else(|| {
        invalid_request("Invalid type for parameter: tool_choice must be an object.")
    })?;
    if choice
        .get("disable_parallel_tool_use")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Err(invalid_request(
            "Unsupported parameter: 'tool_choice.disable_parallel_tool_use' is not supported with this endpoint.",
        ));
    }
    let config = match choice.get("type").and_then(Value::as_str) {
        Some("auto") => None,
        Some("any") => Some(json!({"mode": "ANY"})),
        Some("none") => Some(json!({"mode": "NONE"})),
        Some("tool") => {
            let name = choice.get("name").and_then(Value::as_str).ok_or_else(|| {
                invalid_request("Invalid value for parameter: tool_choice requires a tool name.")
            })?;
            Some(json!({"mode": "ANY", "allowedFunctionNames": [name]}))
        }
        _ => return Err(invalid_request("Invalid value for parameter: tool_choice.")),
    };
    Ok(config.map(|config| json!({"functionCallingConfig": config})))
}

/// Messages `thinking` → `generationConfig.thinkingConfig` по маппингу 5.1 (простейший
/// задокументированный, lossy): `disabled` и `adaptive` → дефолт модели (поле не вставляется);
/// `enabled` мапит budget на уровень: < 4096 → "low", < 16384 → "medium", иначе "high";
/// budget ниже минимума Messages (1024) → 400. `includeThoughts: true` — как 3.4b
/// (thought-парты ответа переводятся в thinking-блоки; `thoughtSignature` не выставляется —
/// решение 4/6).
fn translate_thinking(value: Option<&Value>) -> Result<Option<Value>, Response> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let object = value.as_object().ok_or_else(|| {
        invalid_request("Invalid type for parameter: thinking must be an object.")
    })?;
    match object.get("type").and_then(Value::as_str) {
        Some("disabled") | Some("adaptive") => Ok(None),
        Some("enabled") => {
            let budget = object
                .get("budget_tokens")
                .and_then(Value::as_u64)
                .filter(|budget| *budget > 0)
                .ok_or_else(|| {
                    invalid_request(
                        "Invalid type for parameter: thinking.budget_tokens must be a positive integer.",
                    )
                })?;
            if budget < 1024 {
                return Err(invalid_request(
                    "Invalid value for parameter: thinking.budget_tokens must be at least 1024.",
                ));
            }
            let level = match budget {
                0..=4095 => "low",
                4096..=16383 => "medium",
                _ => "high",
            };
            Ok(Some(
                json!({"thinkingLevel": level, "includeThoughts": true}),
            ))
        }
        _ => Err(invalid_request(
            "Invalid value for parameter: thinking.type.",
        )),
    }
}

/// `stop_sequences` → `generationConfig.stopSequences` (generateContent исполняет их нативно
/// — как `stop` у chat.rs): массив строк, пустые выбрасываются (leniency 5.1), больше
/// публичного лимита 5 → 400. Пустой итог — поле не вставляется.
fn translate_stop_sequences(value: Option<&Value>) -> Result<Option<Value>, Response> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let items = value.as_array().ok_or_else(|| {
        invalid_request("Invalid type for parameter: stop_sequences must be an array of strings.")
    })?;
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let sequence = item
            .as_str()
            .ok_or_else(|| invalid_request(format!("stop_sequences.{index} must be a string.")))?;
        if !sequence.is_empty() {
            out.push(Value::String(sequence.to_string()));
        }
    }
    if out.len() > 5 {
        return Err(invalid_request(
            "Invalid value for parameter: stop_sequences supports at most 5 sequences with this endpoint.",
        ));
    }
    Ok((!out.is_empty()).then_some(Value::Array(out)))
}

// ---------- перевод ответа (GenerateContentResponse → Messages) ----------

/// Messages usage из usageMetadata (маппинг 3.3 в полях Messages usage): input —
/// `promptTokenCount`, output — `candidatesTokenCount` + `thoughtsTokenCount` (та же сумма,
/// что тарифицирует metering), cache read — `cache_read_input_tokens` (только при >0),
/// thoughts — `output_tokens_details.thinking_tokens` (только при >0, как 5.1).
fn messages_usage(usage: &Value) -> Value {
    let tokens = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let thoughts = tokens("thoughtsTokenCount");
    let mut mapped = json!({
        "input_tokens": tokens("promptTokenCount"),
        "output_tokens": tokens("candidatesTokenCount").saturating_add(thoughts),
    });
    let cached = tokens("cachedContentTokenCount");
    if cached > 0 {
        mapped["cache_read_input_tokens"] = Value::from(cached);
    }
    if thoughts > 0 {
        mapped["output_tokens_details"] = json!({"thinking_tokens": thoughts});
    }
    mapped
}

/// Синтезируемый id tool_use-блока: на private wire functionCall.id не приезжает, поэтому id
/// детерминированные `toolu_<name>[_N]` — схема `callu_<name>[_N]` chat.rs с префиксом,
/// который 5.1 использует для Messages tool_use.
fn synthetic_tool_id(name: &str, ordinal: u64) -> String {
    if ordinal == 1 {
        format!("toolu_{name}")
    } else {
        format!("toolu_{name}_{ordinal}")
    }
}

/// Messages `stop_reason` из признаков ответа (через общий `map_finish_reason` chat.rs):
/// functionCall в ответе → `tool_use` (как 5.1 — tool call важнее остальных причин),
/// MAX_TOKENS → `max_tokens`, класс content_filter (SAFETY/RECITATION/BLOCKLIST/
/// PROHIBITED_CONTENT/SPII) → `refusal`, остальное (STOP/OTHER/неизвестное/отсутствие) →
/// `end_turn`. `stop_sequence` всегда null: нативный stop не различим от обычной остановки
/// (см. шапку модуля).
fn stop_reason(has_tool_use: bool, finish: Option<&str>) -> &'static str {
    if has_tool_use {
        return "tool_use";
    }
    match map_finish_reason(finish) {
        "length" => "max_tokens",
        "content_filter" => "refusal",
        _ => "end_turn",
    }
}

/// Content-парты кандидата → Messages content blocks (словарь 5.1 поверх форм 3.3/4.3):
/// text-парты склеиваются в ОДИН text-блок на позиции первого text-парта (без текста блок не
/// создаётся), thought-парты с непустым текстом → thinking-блоки БЕЗ signature
/// (thoughtSignature-only парт пропускается — решение 4/6), functionCall → tool_use-блоки
/// (`args` object → `input` object; id — синтезируемый `toolu_<name>[_N]`). Блоки идут в
/// порядке партов; неизвестные парты пропускаются. Возвращает блоки и признак tool_use.
fn content_blocks(parts: Option<&Vec<Value>>) -> (Vec<Value>, bool) {
    let mut blocks: Vec<Value> = Vec::new();
    let mut text = String::new();
    // Позиция text-блока — позиция первого text-парта среди остальных блоков (зеркало 4.1/5.1).
    let mut text_at: Option<usize> = None;
    let mut has_tool_use = false;
    let mut name_counts: HashMap<&str, u64> = HashMap::new();
    for part in parts.into_iter().flatten() {
        // thought-парт: его text — thinking-блок, а не text; thoughtSignature всегда
        // выбрасывается (решение 4/6).
        if part.get("thought").and_then(Value::as_bool) == Some(true) {
            let thinking = part.get("text").and_then(Value::as_str).unwrap_or_default();
            if !thinking.is_empty() {
                blocks.push(json!({"type": "thinking", "thinking": thinking}));
            }
            continue;
        }
        if let Some(segment) = part.get("text").and_then(Value::as_str) {
            if text_at.is_none() {
                text_at = Some(blocks.len());
            }
            text.push_str(segment);
            continue;
        }
        if let Some(call) = part.get("functionCall") {
            has_tool_use = true;
            let name = call.get("name").and_then(Value::as_str).unwrap_or_default();
            let count = name_counts.entry(name).or_insert(0);
            *count += 1;
            let id = synthetic_tool_id(name, *count);
            let input = call
                .get("args")
                .filter(|args| args.is_object())
                .cloned()
                .unwrap_or_else(|| json!({}));
            blocks.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input
            }));
        }
    }
    if !text.is_empty() {
        // Непустой text подразумевает text-парт, а значит text_at — Some; None — защита.
        let position = text_at.unwrap_or(blocks.len()).min(blocks.len());
        blocks.insert(position, json!({"type": "text", "text": text}));
    }
    (blocks, has_tool_use)
}

/// Перевод non-stream ответа GenerateContentResponse → Messages message (словарь 5.1).
async fn json_messages_response(upstream: Response, requested_model: String) -> Response {
    let request_id = upstream.headers().get("request-id").cloned();
    let bytes = match to_bytes(upstream.into_body(), RESPONSE_BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return without_not_started(skin_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                "The provider returned an unreadable response.",
                "internal_response_error",
                None,
            ))
        }
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => {
            return without_not_started(skin_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                "The provider returned a malformed response.",
                "internal_response_error",
                None,
            ))
        }
    };
    let model = value
        .get("modelVersion")
        .and_then(Value::as_str)
        .unwrap_or(&requested_model)
        .to_string();
    let candidate = value
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first());
    let (blocks, has_tool_use) = content_blocks(
        candidate
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(Value::as_array),
    );
    // finishReason кандидата; candidates отсутствуют при блокировке промпта на входе — тогда
    // причина берётся из promptFeedback.blockReason (refusal с пустым content), как 3.3/4.3.
    let finish = candidate
        .and_then(|c| c.get("finishReason"))
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("promptFeedback")
                .and_then(|f| f.get("blockReason"))
                .and_then(Value::as_str)
        });
    let usage = value
        .get("usageMetadata")
        .map(messages_usage)
        .unwrap_or(Value::Null);
    let message = json!({
        "id": new_id("msg"),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": blocks,
        "stop_reason": stop_reason(has_tool_use, finish),
        "stop_sequence": Value::Null,
        "usage": usage,
    });
    let mut response = axum::Json(message).into_response();
    if let Some(request_id) = request_id {
        response.headers_mut().insert("request-id", request_id);
    }
    response
}

/// Перевод ответа native `:countTokens` (`{"totalTokens": N, …}`) → Messages-ответ
/// `{"input_tokens": N}`. Остальные поля native-ответа (cachedContentTokenCount и пр.) у
/// Messages count_tokens контракта нет — опускаются.
async fn count_tokens_json_response(upstream: Response) -> Response {
    let request_id = upstream.headers().get("request-id").cloned();
    let bytes = match to_bytes(upstream.into_body(), RESPONSE_BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return without_not_started(skin_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                "The provider returned an unreadable response.",
                "internal_response_error",
                None,
            ))
        }
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => {
            return without_not_started(skin_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                "The provider returned a malformed response.",
                "internal_response_error",
                None,
            ))
        }
    };
    let total = value.get("totalTokens").and_then(Value::as_u64);
    let Some(total) = total else {
        return without_not_started(skin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "api_error",
            "The provider returned a malformed response.",
            "internal_response_error",
            None,
        ));
    };
    let mut response = axum::Json(json!({"input_tokens": total})).into_response();
    if let Some(request_id) = request_id {
        response.headers_mut().insert("request-id", request_id);
    }
    response
}

// ---------- stream (GenerateContentResponse SSE → Messages SSE) ----------

type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// Тип ещё не закрытого Messages content block в SSE-переводе. tool_use-блоков среди
/// незакрытых не бывает — functionCall приходит целиком и закрывается на том же кадре.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OpenBlockKind {
    Text,
    Thinking,
}

/// Потоковый транслятор GenerateContentResponse SSE → Messages SSE (словарь 5.1 в шапке
/// модуля; кадры плоскости — data-only, без `event:`). Буферизуются только байты одного
/// незакрытого SSE-кадра; готовые события отдаются немедленно (первый чанк не ждёт конца
/// стрима — требование Claude Code).
struct GeminiMessagesSseTranslator {
    inner: ByteStream,
    buf: BytesMut,
    out: VecDeque<Bytes>,
    /// `msg_*` id сообщения — общий для message_start и всех событий стрима.
    id: String,
    /// Запрошенная (native) модель — фолбэк, пока/если кадры не сообщили `modelVersion`.
    requested_model: String,
    served_model: Option<String>,
    /// `message_start` уже отправлен.
    started: bool,
    /// usageMetadata приходит на кадрах нарастающим итогом — в message_delta уходит последнее.
    last_usage: Option<Value>,
    /// finishReason кандидата либо promptFeedback.blockReason — stop_reason message_delta.
    finish: Option<String>,
    /// Открытый text/thinking-блок (закрывается сменой типа контента или финалом стрима).
    open: Option<(OpenBlockKind, u64)>,
    /// Плотный счётчик block index.
    next_index: u64,
    /// В стриме был functionCall → stop_reason tool_use.
    has_tool_use: bool,
    /// Per-name счётчик синтезируемых tool id — та же схема `toolu_<name>[_N]`, что в
    /// non-stream переводе.
    name_counts: HashMap<String, u64>,
    /// Heartbeat `event: ping` (на Gemini wire ping-кадров нет — генерируется локально).
    heartbeat: tokio::time::Interval,
    source: GeminiStreamState,
    /// Терминальное событие (message_stop/error) уже поставлено в `out`.
    finished: bool,
}

impl GeminiMessagesSseTranslator {
    fn new(inner: ByteStream, requested_model: String, ping_interval: Duration) -> Self {
        let mut heartbeat =
            tokio::time::interval_at(tokio::time::Instant::now() + ping_interval, ping_interval);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        Self {
            inner,
            buf: BytesMut::new(),
            out: VecDeque::new(),
            id: new_id("msg"),
            requested_model,
            served_model: None,
            started: false,
            last_usage: None,
            finish: None,
            open: None,
            next_index: 0,
            has_tool_use: false,
            name_counts: HashMap::new(),
            heartbeat,
            source: GeminiStreamState::default(),
            finished: false,
        }
    }

    fn model(&self) -> &str {
        self.served_model
            .as_deref()
            .unwrap_or(&self.requested_model)
    }

    /// SSE-кадр события Messages: `event:` + `data:` (типизированные события — как 5.1 и
    /// нативная плоскость).
    fn frame(event: &str, value: Value) -> Bytes {
        let mut frame = String::with_capacity(256);
        frame.push_str("event: ");
        frame.push_str(event);
        frame.push_str("\ndata: ");
        frame.push_str(&value.to_string());
        frame.push_str("\n\n");
        Bytes::from(frame)
    }

    fn push_event(&mut self, event: &str, value: Value) {
        self.out.push_back(Self::frame(event, value));
    }

    /// `message_start` — ровно один раз, до первого клиент-видимого события. Usage нулевой:
    /// authoritative usage существует только в конце стрима (message_delta) — задокументированное
    /// ограничение 5.1, унаследованное здесь.
    fn ensure_started(&mut self) {
        if !self.started {
            self.started = true;
            let id = self.id.clone();
            let model = self.model().to_string();
            self.push_event(
                "message_start",
                json!({
                    "type": "message_start",
                    "message": {
                        "id": id,
                        "type": "message",
                        "role": "assistant",
                        "model": model,
                        "content": [],
                        "stop_reason": Value::Null,
                        "stop_sequence": Value::Null,
                        "usage": {"input_tokens": 0, "output_tokens": 0}
                    }
                }),
            );
        }
    }

    /// Закрыть открытый блок (content_block_stop). Смена типа контента Gemini — граница
    /// Messages-блока (плотные индексы, как 5.1).
    fn close_open(&mut self) {
        if let Some((_, index)) = self.open.take() {
            self.push_event(
                "content_block_stop",
                json!({"type": "content_block_stop", "index": index}),
            );
        }
    }

    /// Открыть блок (если ещё не открыт блок этого типа): сначала закрывается предыдущий.
    fn ensure_open(&mut self, kind: OpenBlockKind, content_block: Value) {
        if self.open.is_some_and(|(current, _)| current == kind) {
            return;
        }
        self.ensure_started();
        self.close_open();
        let index = self.next_index;
        self.next_index += 1;
        self.open = Some((kind, index));
        self.push_event(
            "content_block_start",
            json!({"type": "content_block_start", "index": index, "content_block": content_block}),
        );
    }

    fn delta(&mut self, delta: Value) {
        let index = self.open.map(|(_, index)| index).unwrap_or(0);
        let frame = Self::frame(
            "content_block_delta",
            json!({"type": "content_block_delta", "index": index, "delta": delta}),
        );
        self.out.push_back(frame);
    }

    fn text_delta(&mut self, text: &str) {
        self.ensure_open(OpenBlockKind::Text, json!({"type": "text", "text": ""}));
        self.delta(json!({"type": "text_delta", "text": text}));
    }

    fn thinking_delta(&mut self, text: &str) {
        self.ensure_open(
            OpenBlockKind::Thinking,
            json!({"type": "thinking", "thinking": ""}),
        );
        self.delta(json!({"type": "thinking_delta", "thinking": text}));
    }

    /// functionCall-парт целиком (аргумент-дельт на wire нет): открытый блок закрывается,
    /// tool_use-блок эмитится с ПОЛНЫМ input в content_block_start и сразу закрывается —
    /// БЕЗ input_json_delta (как 3.3 отдаёт tool_calls одним чанком).
    fn push_function_call(&mut self, call: &Value) {
        let name = call.get("name").and_then(Value::as_str).unwrap_or_default();
        let count = self.name_counts.entry(name.to_string()).or_insert(0);
        *count += 1;
        let id = synthetic_tool_id(name, *count);
        let input = call
            .get("args")
            .filter(|args| args.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        self.has_tool_use = true;
        self.ensure_started();
        self.close_open();
        let index = self.next_index;
        self.next_index += 1;
        self.push_event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {"type": "tool_use", "id": id, "name": name, "input": input}
            }),
        );
        self.push_event(
            "content_block_stop",
            json!({"type": "content_block_stop", "index": index}),
        );
    }

    /// Mid-stream отказ: Anthropic-shaped `event: error` и конец стрима (зеркало правила 5.1;
    /// тип по google.rpc status — как error-кадр chat.rs).
    fn push_error(&mut self, message: &str, code: Option<&str>) {
        if self.finished {
            return;
        }
        self.ensure_started();
        let kind = match code {
            Some("RESOURCE_EXHAUSTED") => "rate_limit_error",
            Some("UNAUTHENTICATED") | Some("PERMISSION_DENIED") => "authentication_error",
            _ => "api_error",
        };
        self.push_event(
            "error",
            json!({
                "type": "error",
                "error": {"type": kind, "message": message}
            }),
        );
        self.finished = true;
    }

    /// Терминальная пара message_delta/message_stop после provider finishReason/blockReason + EOF.
    /// Gemini message_stop на wire не имеет. Открытый блок закрывается,
    /// message_delta несёт stop_reason и authoritative usage из последнего usageMetadata.
    fn push_completed(&mut self) {
        if self.finished {
            return;
        }
        self.ensure_started();
        self.close_open();
        let usage = self
            .last_usage
            .take()
            .map(|usage| messages_usage(&usage))
            .unwrap_or(Value::Null);
        let reason = stop_reason(self.has_tool_use, self.finish.as_deref());
        self.push_event(
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": reason, "stop_sequence": Value::Null},
                "usage": usage
            }),
        );
        self.push_event("message_stop", json!({"type": "message_stop"}));
        self.finished = true;
    }

    /// Один data-кадр GenerateContentResponse. Порядок важен: modelVersion и usageMetadata
    /// фиксируются ДО эмиссии событий, чтобы события кадра уже несли сервёную модель.
    fn handle_data(&mut self, data: Value) {
        // Санитизированная mid-stream ошибка плоскости:
        // {error: {code, message, status}} — терминал стрима.
        if let Some(error) = data.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("The provider returned an error.");
            let code = error.get("status").and_then(Value::as_str);
            self.push_error(message, code);
            return;
        }
        if let Some(model) = data.get("modelVersion").and_then(Value::as_str) {
            self.served_model = Some(model.to_string());
        }
        if let Some(usage) = data.get("usageMetadata") {
            self.last_usage = Some(usage.clone());
        }
        // Блокировка промпта на входе: кандидатов не будет, stop_reason финального
        // message_delta берётся из blockReason (refusal с пустым content) — как 3.3/4.3.
        if let Some(block) = data
            .get("promptFeedback")
            .and_then(|f| f.get("blockReason"))
            .and_then(Value::as_str)
        {
            self.ensure_started();
            self.finish = Some(block.to_string());
            return;
        }
        let Some(candidate) = data
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|candidates| candidates.first())
        else {
            // usage-only / model-only кадр — client-visible дельт не несёт.
            return;
        };
        if let Some(parts) = candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(Value::as_array)
        {
            for part in parts {
                // thought-парт (3.4b): его text — thinking-дельта, а не text-дельта.
                // thoughtSignature выбрасывается (решение 4/6): парт с одним thoughtSignature
                // видимого события не порождает.
                if part.get("thought").and_then(Value::as_bool) == Some(true) {
                    if let Some(segment) = part
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|t| !t.is_empty())
                    {
                        self.thinking_delta(segment);
                    }
                    continue;
                }
                if let Some(segment) = part
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                {
                    self.text_delta(segment);
                    continue;
                }
                // Gemini присылает functionCall целиком (аргумент-дельт на wire нет) →
                // tool_use-блок с полным input без input_json_delta.
                if let Some(call) = part.get("functionCall") {
                    self.push_function_call(call);
                }
            }
        }
        if let Some(finish) = candidate.get("finishReason").and_then(Value::as_str) {
            self.finish = Some(finish.to_string());
        }
    }

    /// Вытащить из буфера все закрытые SSE-кадры (разделитель `\n\n`).
    fn drain_frames(&mut self) {
        loop {
            let Some(at) = self.buf.windows(2).position(|w| w == b"\n\n") else {
                return;
            };
            let frame = self.buf.split_to(at);
            let _crlf = self.buf.split_to(2.min(self.buf.len()));
            let text = String::from_utf8_lossy(&frame);
            let mut data_lines: Vec<String> = Vec::new();
            for line in text.split('\n') {
                let line = line.strip_suffix('\r').unwrap_or(line);
                if let Some(value) = line.strip_prefix("data:") {
                    data_lines.push(value.trim_start().to_string());
                }
            }
            if data_lines.is_empty() {
                continue;
            }
            let data = data_lines.join("\n");
            match self.source.accept(&data) {
                Ok(data) => self.handle_data(data),
                Err(()) => self.push_error(
                    "The provider stream contained a malformed frame.",
                    Some("protocol_error"),
                ),
            }
            if self.finished {
                self.buf.clear();
                return;
            }
        }
    }
}

impl Stream for GeminiMessagesSseTranslator {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(bytes) = self.out.pop_front() {
                return Poll::Ready(Some(Ok(bytes)));
            }
            if self.finished {
                return Poll::Ready(None);
            }
            // Heartbeat `event: ping` между кадрами апстрима (как 5.1): Claude Code держит
            // стрим живым по ping, на Gemini wire ping-кадров нет.
            if self.heartbeat.poll_tick(cx).is_ready() {
                self.out
                    .push_back(Bytes::from_static(b"event: ping\ndata: {}\n\n"));
                continue;
            }
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    self.buf.extend_from_slice(&chunk);
                    self.drain_frames();
                }
                // Голый транспортный сбой до сюда доходить не должен (плоскость санитизирует
                // его в error-кадр), но стрим не обрываем молча ни при каком раскладе.
                Poll::Ready(Some(Err(_))) => {
                    self.push_error("The provider stream was interrupted.", None);
                }
                Poll::Ready(None) => {
                    if self.buf.iter().any(|byte| !byte.is_ascii_whitespace()) {
                        self.buf.extend_from_slice(b"\n\n");
                        self.drain_frames();
                    }
                    if self.finished {
                        continue;
                    }
                    if self.source.is_complete() {
                        self.push_completed();
                    } else {
                        self.push_error(
                            "The provider stream ended before completion.",
                            Some("protocol_error"),
                        );
                    }
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// Перевод SSE-ответа `gemini_api()` в поток Messages SSE. Транслятор навешивается СНАРУЖИ
/// ответа gemini_api(): usage-settlement внутри плоскости уже протапало оригинальные байты
/// GenerateContentResponse, а mid-stream ошибка приходит санитизированным error-кадром — он
/// переводится в `event: error` по тому же правилу.
fn stream_messages_response(upstream: Response, requested_model: String) -> Response {
    let request_id = upstream.headers().get("request-id").cloned();
    let stream = upstream
        .into_body()
        .into_data_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let translator =
        GeminiMessagesSseTranslator::new(Box::pin(stream), requested_model, SSE_PING_INTERVAL);
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(Body::from_stream(translator))
        .unwrap_or_else(|_| {
            // gemini_api уже вернул 2xx и мог durable отметить delivery/charge; ошибка
            // локальной сборки ответа не даёт права объявлять попытку не начатой.
            without_not_started(skin_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                "Internal server error",
                "internal_response_error",
                None,
            ))
        });
    if let Some(request_id) = request_id {
        response.headers_mut().insert("request-id", request_id);
    }
    response
}

// ---------- handlers ----------

/// Буферизованное JSON-тело Messages-запроса под лимитом плоскости (32 MiB — общий
/// `CHAT_BODY_LIMIT` universal lanes Gemini); ошибки — Anthropic-конверт 400.
async fn read_messages_body(body: Body) -> Result<Value, Response> {
    let raw = match read_body_limited(body, CHAT_BODY_LIMIT).await {
        Ok(raw) => raw,
        Err(BodyReadError::TooLarge) => {
            return Err(invalid_request("Request body exceeds the 32 MiB limit."))
        }
        Err(BodyReadError::Read) => return Err(invalid_request("Could not read request body.")),
    };
    match serde_json::from_slice(&raw) {
        Ok(value) => Ok(value),
        Err(_) => Err(invalid_request("Invalid JSON in request body.")),
    }
}

/// Общий хвост обоих handlers: внутренний запрос на нативную поверхность. Admission, reserve,
/// affinity, ротация, Code Assist wrapper и settlement выполняет общий gemini_api().
/// Заголовки клиента сохраняются (authorize читает ключи из них), меняется только
/// content-length/content-type под синтезированное тело; anthropic-version/anthropic-beta
/// upstream не проксируются (тело собирается wrapper'ом из переведённого JSON).
async fn run_inner(
    app: AppState,
    peer: SocketAddr,
    headers: axum::http::HeaderMap,
    suffix: &str,
    translated: &Translated,
) -> Response {
    let mut headers = headers;
    headers.remove(header::CONTENT_LENGTH);
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let body_bytes = match serde_json::to_vec(&translated.body) {
        Ok(bytes) => bytes,
        Err(_) => {
            return skin_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                "Failed to build the upstream request.",
                "internal_response_error",
                None,
            )
        }
    };
    let mut inner = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1beta/models/{}:{suffix}", translated.model))
        .body(Body::from(body_bytes))
        .expect("static request builder is infallible");
    *inner.headers_mut() = headers;
    gemini_api(State(app), ConnectInfo(peer), inner).await
}

/// Хендлер `POST /v1/messages` на Gemini-плоскости (роут регистрируется в server только в
/// `ProviderMode::Gemini`). Точный паттерн `gemini_chat_completions`/`gemini_responses`.
pub async fn gemini_messages_skin(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    let (parts, body) = request.into_parts();
    let value = match read_messages_body(body).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let translated = match translate_messages_request(value, true) {
        Ok(translated) => translated,
        Err(response) => return response,
    };
    let suffix = if translated.stream {
        "streamGenerateContent?alt=sse"
    } else {
        "generateContent"
    };
    let upstream = run_inner(app, peer, parts.headers, suffix, &translated).await;
    if upstream.status() != StatusCode::OK {
        return convert_error_response(upstream).await;
    }
    if translated.stream {
        stream_messages_response(upstream, translated.model)
    } else {
        json_messages_response(upstream, translated.model).await
    }
}

/// Хендлер `POST /v1/messages/count_tokens` на Gemini-плоскости: тот же Messages parse +
/// перевод в GenerateContentRequest → внутренний запрос `/v1beta/models/{model}:countTokens`
/// через общий `gemini_api()` (нативная операция quota-free и без reserve) → `totalTokens` →
/// `{"input_tokens": N}`. `max_tokens` здесь опционален — официальный endpoint его не требует;
/// `stream` игнорируется (считаются только входные токены).
pub async fn gemini_messages_count_tokens(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    let (parts, body) = request.into_parts();
    let value = match read_messages_body(body).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let translated = match translate_messages_request(value, false) {
        Ok(translated) => translated,
        Err(response) => return response,
    };
    let upstream = run_inner(app, peer, parts.headers, "countTokens", &translated).await;
    if upstream.status() != StatusCode::OK {
        return convert_error_response(upstream).await;
    }
    count_tokens_json_response(upstream).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    fn ok_translated(value: Value) -> Translated {
        translate_messages_request(value, true).expect("translation must succeed")
    }

    async fn err_parts(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn expect_err(value: Value) -> (StatusCode, Value) {
        err_parts(translate_messages_request(value, true).unwrap_err()).await
    }

    #[tokio::test]
    async fn messages_stream_requires_a_boolean() {
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "max_tokens": 256,
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": "false"
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["type"], "error");
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("stream"));
        assert!(body["error"].get("param").is_none());

        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "max_tokens": 256,
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": null
        }));
        assert!(!translated.stream);
    }

    fn upstream_json(value: Value) -> Response {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .unwrap()
    }

    fn upstream_error(status: StatusCode, body: &str, reason: &'static str) -> Response {
        let mut response = Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        response
            .extensions_mut()
            .insert(TerminalErrorReason(reason));
        response
    }

    fn sse_bytes(events: &str) -> ByteStream {
        let chunks: Vec<Result<Bytes, std::io::Error>> = events
            .split("\n\n")
            .filter(|frame| !frame.trim().is_empty())
            .map(|frame| Ok(Bytes::from(format!("{frame}\n\n"))))
            .collect();
        Box::pin(futures_util::stream::iter(chunks))
    }

    async fn collect_stream(stream: impl Stream<Item = Result<Bytes, std::io::Error>>) -> String {
        let mut out = String::new();
        tokio::pin!(stream);
        while let Some(chunk) = stream.next().await {
            out.push_str(&String::from_utf8_lossy(&chunk.unwrap()));
        }
        out
    }

    /// (event, data) каждого SSE-кадра ответа — тот же разбор, что в contract-тестах
    /// Codex skin 5.1 и Responses-адаптера 4.3.
    fn event_frames(output: &str) -> Vec<(String, Value)> {
        output
            .split("\n\n")
            .filter(|frame| !frame.trim().is_empty())
            .map(|frame| {
                let mut event = String::new();
                let mut data = String::new();
                for line in frame.split('\n') {
                    if let Some(value) = line.strip_prefix("event:") {
                        event = value.trim_start().to_string();
                    } else if let Some(value) = line.strip_prefix("data:") {
                        data = value.trim_start().to_string();
                    }
                }
                (event, serde_json::from_str(&data).expect("data is JSON"))
            })
            .collect()
    }

    fn event_names(frames: &[(String, Value)]) -> Vec<&str> {
        frames.iter().map(|(event, _)| event.as_str()).collect()
    }

    fn translate_events(events: &str) -> GeminiMessagesSseTranslator {
        GeminiMessagesSseTranslator::new(
            sse_bytes(events),
            "gemini-2.5-flash".to_string(),
            SSE_PING_INTERVAL,
        )
    }

    #[tokio::test]
    async fn skin_errors_mark_execution_not_started_but_post_success_rebuilds_do_not() {
        // Локальная валидация и конвертация не-2xx gemini_api происходят до delivery:
        // reserve refund/cancel, поэтому Anthropic skin сохраняет внутренний контракт.
        for response in [
            skin_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "overloaded_error",
                "Overloaded",
                "test_reason",
                Some(2),
            ),
            invalid_request("bad request"),
            convert_error_response(upstream_error(
                StatusCode::TOO_MANY_REQUESTS,
                r#"{"error":{"code":429,"message":"quota","status":"RESOURCE_EXHAUSTED"}}"#,
                "quota_exhausted",
            ))
            .await,
            translate_messages_request(json!({"model": "google/gemini-2.5-flash"}), true)
                .unwrap_err(),
        ] {
            assert!(!response.status().is_success());
            assert_eq!(
                response
                    .headers()
                    .get(crate::proxy::EXECUTION_STATE_HEADER)
                    .unwrap(),
                crate::proxy::EXECUTION_STATE_NOT_STARTED
            );
        }

        // После 2xx native request уже мог стать billable. Ошибка адаптера при разборе
        // такого ответа обязана fail closed: без not_started, значит router не ретраит.
        for response in [
            json_messages_response(
                upstream_error(StatusCode::OK, "not json", "unused"),
                "gemini-2.5-flash".to_string(),
            )
            .await,
            count_tokens_json_response(upstream_json(json!({"missing": "totalTokens"}))).await,
        ] {
            assert!(!response.status().is_success());
            assert!(response
                .headers()
                .get(crate::proxy::EXECUTION_STATE_HEADER)
                .is_none());
        }
    }

    // ---------- перевод запроса ----------

    #[test]
    fn translates_basic_messages_to_generate_content() {
        let translated = ok_translated(json!({
            "model": "google/gemini-2.5-flash",
            "system": "Be terse.",
            "max_tokens": 256,
            "messages": [{"role": "user", "content": "Hello"}]
        }));
        let body = &translated.body;
        // google/-префикс снят — дальше нативный публичный id.
        assert_eq!(translated.model, "gemini-2.5-flash");
        assert!(!translated.stream);
        assert_eq!(
            body["systemInstruction"],
            json!({"parts": [{"text": "Be terse."}]})
        );
        assert_eq!(
            body["contents"],
            json!([{"role": "user", "parts": [{"text": "Hello"}]}])
        );
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 256);
        // toolConfig/tools отсутствуют — дефолт AUTO не вставляется; stream в тело не идёт
        // (флаг выбирает только suffix внутреннего URI).
        assert!(body.get("toolConfig").is_none());
        assert!(body.get("tools").is_none());
        assert!(body.get("stream").is_none());
    }

    #[test]
    fn system_blocks_join_and_empty_string_is_omitted() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "system": [
                {"type": "text", "text": "one"},
                {"type": "text", "text": "two"}
            ],
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}]
        }));
        // Склейка \n\n — как instructions у Codex skin 5.1; один text-парт systemInstruction.
        assert_eq!(
            translated.body["systemInstruction"],
            json!({"parts": [{"text": "one\n\ntwo"}]})
        );

        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash", "system": "", "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert!(translated.body.get("systemInstruction").is_none());
    }

    #[tokio::test]
    async fn cache_control_anywhere_is_400() {
        // null — дефолт и проходит (как 5.1).
        ok_translated(json!({
            "model": "gemini-2.5-flash", "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}],
            "system": [{"type": "text", "text": "x", "cache_control": null}],
        }));
        // Не-дефолт — в system, на content-блоке и на tool — 400 с именем параметра.
        for body in [
            json!({"model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
                "system": [{"type": "text", "text": "x", "cache_control": {"type": "ephemeral"}}]}),
            json!({"model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": [
                {"type": "text", "text": "hi", "cache_control": {"type": "ephemeral"}}]}]}),
            json!({"model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
                "tools": [{"name": "f", "cache_control": {"type": "ephemeral"}}]}),
        ] {
            let (status, json) = expect_err(body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
            assert_eq!(json["type"], "error");
            assert_eq!(json["error"]["type"], "invalid_request_error");
            assert!(
                json["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("cache_control"),
                "{json}"
            );
        }
    }

    #[test]
    fn user_images_translate_to_inline_data() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "max_tokens": 1,
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "What is this?"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="}},
                {"type": "text", "text": "And this?"}
            ]}]
        }));
        // base64 source → inlineData; text-склейка разрывается image-партом (как chat.rs).
        assert_eq!(
            translated.body["contents"],
            json!([{"role": "user", "parts": [
                {"text": "What is this?"},
                {"inlineData": {"mimeType": "image/png", "data": "iVBORw0KGgo="}},
                {"text": "And this?"}
            ]}])
        );
    }

    #[tokio::test]
    async fn rejects_invalid_image_sources() {
        for (source, expected) in [
            // url source → 400 (generateContent ссылки не принимает — как 3.3/3.4a; на
            // Codex-плоскости 5.1 url принимается — плоскостное отличие).
            (
                json!({"type": "url", "url": "https://example.com/cat.jpg"}),
                "base64",
            ),
            (json!({"type": "base64", "media_type": "image/png"}), "data"),
            (
                json!({"type": "base64", "media_type": "text/html", "data": "PGI+"}),
                "image MIME",
            ),
            (json!({"type": "s3", "location": "x"}), "source type"),
        ] {
            let (status, json) = expect_err(json!({
                "model": "gemini-2.5-flash", "max_tokens": 1,
                "messages": [{"role": "user", "content": [
                    {"type": "image", "source": source}]}],
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
            assert_eq!(json["error"]["type"], "invalid_request_error", "{json}");
            assert!(
                json["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains(expected),
                "{json}"
            );
        }
    }

    /// Зеркало contract-теста 5.1 `tool_history_replays_as_function_items` при эквивалентном
    /// входе: tool_use → functionCall (args — OBJECT, не строка), tool_result →
    /// functionResponse в user-content с именем из карты этой же истории.
    #[test]
    fn tool_history_replays_as_function_parts() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "max_tokens": 1,
            "messages": [
                {"role": "user", "content": "weather?"},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Let me check."},
                    {"type": "tool_use", "id": "toolu_1", "name": "weather",
                     "input": {"city": "Paris"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_1", "content": "sunny"},
                    {"type": "text", "text": "thanks"}
                ]}
            ]
        }));
        assert_eq!(
            translated.body["contents"],
            json!([
                {"role": "user", "parts": [{"text": "weather?"}]},
                {"role": "model", "parts": [
                    {"text": "Let me check."},
                    {"functionCall": {"name": "weather", "args": {"city": "Paris"}},
                     "thoughtSignature": "context_engineering_is_the_way_to_go"}
                ]},
                // functionResponse и текст после него — один user-content (склейка
                // одноролевых, как серия tool-ответов в chat.rs 3.3).
                {"role": "user", "parts": [
                    {"functionResponse": {"name": "weather", "response": {"result": "sunny"}}},
                    {"text": "thanks"}
                ]}
            ])
        );
    }

    #[test]
    fn tool_result_array_content_joins_with_newlines() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash", "max_tokens": 1,
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "t1", "name": "f", "input": {}}]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "is_error": true, "content": [
                        {"type": "text", "text": "line one"},
                        {"type": "text", "text": "line two"}
                    ]}
                ]}
            ]
        }));
        // is_error принимается и игнорируется (как 5.1); text-парты склеиваются через \n;
        // не-JSON текст заворачивается строкой общим function_response_value.
        assert_eq!(
            translated.body["contents"][1],
            json!({"role": "user", "parts": [
                {"functionResponse": {"name": "f", "response": {"result": "line one\nline two"}}}
            ]})
        );
    }

    #[test]
    fn tool_result_json_content_is_parsed_into_result() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash", "max_tokens": 1,
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "t1", "name": "f", "input": {}}]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "{\"temp\": 20}"}]}
            ]
        }));
        assert_eq!(
            translated.body["contents"][1]["parts"][0],
            json!({"functionResponse": {"name": "f", "response": {"result": {"temp": 20}}}})
        );
    }

    #[tokio::test]
    async fn tool_pairing_is_validated_unlike_codex_skin() {
        // tool_result без пары в истории → 400 (паттерн 3.3/4.3: functionResponse ссылается
        // по имени; у Codex-стороны 5.1 pairing не валидируется — плоскостное отличие).
        let (status, json) = expect_err(json!({
            "model": "m", "max_tokens": 1,
            "messages": [{"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "unknown", "content": "x"}]}],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no matching tool_use"));

        // Дубликат tool_use id → 400 (как 5.1).
        let (status, _) = expect_err(json!({
            "model": "m", "max_tokens": 1,
            "messages": [
                {"role": "assistant", "content": [{"type": "tool_use", "id": "t", "name": "a", "input": {}}]},
                {"role": "assistant", "content": [{"type": "tool_use", "id": "t", "name": "b", "input": {}}]}
            ]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Нетекстовый контент tool_result → 400 (как 5.1).
        let (status, json) = expect_err(json!({
            "model": "m", "max_tokens": 1,
            "messages": [
                {"role": "assistant", "content": [{"type": "tool_use", "id": "t", "name": "f", "input": {}}]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t", "content": [{"type": "image", "source": {}}]}]}
            ]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("tool_result"));

        // tool_use input не object → 400.
        let (status, _) = expect_err(json!({
            "model": "m", "max_tokens": 1,
            "messages": [{"role": "assistant", "content": [
                {"type": "tool_use", "id": "t", "name": "f", "input": "not an object"}]}],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn thinking_and_redacted_thinking_input_blocks_are_dropped() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash", "max_tokens": 1,
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "secret", "signature": "sig"},
                    {"type": "redacted_thinking", "data": "opaque"},
                    {"type": "text", "text": "answer"}
                ]},
                {"role": "user", "content": "next"}
            ]
        }));
        let contents = translated.body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 2);
        assert_eq!(
            contents[0],
            json!({"role": "model", "parts": [{"text": "answer"}]})
        );
    }

    #[test]
    fn tools_translate_to_function_declarations() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash", "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [
                {"name": "get_weather", "description": "Current weather",
                 "input_schema": {"$schema": "https://json-schema.org/draft/2020-12/schema",
                    "type": "object", "properties": {
                        "city": {"type": "string", "exclusiveMaximum": 10}
                    }}},
                {"type": "custom", "name": "no_args"}
            ]
        }));
        assert_eq!(
            translated.body["tools"],
            json!([{"functionDeclarations": [
                {"name": "get_weather", "description": "Current weather",
                 "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}},
                // input_schema отсутствует → parameters опускается (как function_declaration).
                {"name": "no_args"}
            ]}])
        );
    }

    #[tokio::test]
    async fn tool_schema_errors_report_the_exact_pointer() {
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "max_tokens": 32,
            "messages": [{"role":"user", "content":"hi"}],
            "tools": [{"name":"f", "input_schema":{
                "type":"object", "propertyNames":{"type":"string", "pattern":"^x"}
            }}]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("tools.0.input_schema/propertyNames"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn server_tools_and_bad_input_schema_are_400() {
        let (status, json) = expect_err(json!({
            "model": "m", "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "web_search_20250305", "name": "web_search"}],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert!(json["error"]["message"].as_str().unwrap().contains("tools"));

        let (status, _) = expect_err(json!({
            "model": "m", "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"name": "f", "input_schema": "not an object"}],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn tool_choice_variants_map_to_function_calling_config() {
        // auto — дефолт generateContent, toolConfig не вставляется.
        let translated = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": {"type": "auto"}
        }));
        assert!(translated.body.get("toolConfig").is_none());

        let translated = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": {"type": "any"}
        }));
        assert_eq!(
            translated.body["toolConfig"],
            json!({"functionCallingConfig": {"mode": "ANY"}})
        );

        let translated = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": {"type": "none"}
        }));
        assert_eq!(
            translated.body["toolConfig"],
            json!({"functionCallingConfig": {"mode": "NONE"}})
        );

        let translated = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": {"type": "tool", "name": "f"}
        }));
        assert_eq!(
            translated.body["toolConfig"],
            json!({"functionCallingConfig": {"mode": "ANY", "allowedFunctionNames": ["f"]}})
        );

        // disable_parallel_tool_use: false (дефолт) принимается и не влияет.
        let translated = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": {"type": "auto", "disable_parallel_tool_use": false}
        }));
        assert!(translated.body.get("toolConfig").is_none());
    }

    #[tokio::test]
    async fn disable_parallel_tool_use_and_invalid_tool_choice_are_400() {
        // generateContent не умеет ограничивать параллельные вызовы (стойка 4.3); у
        // Codex-плоскости 5.1 false переводится в parallel_tool_calls — плоскостное отличие.
        let (status, json) = expect_err(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": {"type": "auto", "disable_parallel_tool_use": true}
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("disable_parallel_tool_use"));

        for choice in [
            json!({"type": "sometimes"}),
            json!("auto"),
            json!({"type": "tool"}),
        ] {
            let (status, _) = expect_err(json!({
                "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
                "tool_choice": choice
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{choice}");
        }
    }

    #[test]
    fn thinking_maps_to_thinking_config() {
        let config = |thinking: Value| {
            ok_translated(json!({
                "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
                "thinking": thinking
            }))
            .body
            .get("generationConfig")
            .and_then(|config| config.get("thinkingConfig"))
            .cloned()
        };
        // Те же пороги budget→level, что в 5.1 (+ includeThoughts: true — как 3.4b).
        assert_eq!(config(json!({"type": "disabled"})), None);
        assert_eq!(
            config(json!({"type": "adaptive", "display": "summarized"})),
            None
        );
        assert_eq!(
            config(json!({"type": "enabled", "budget_tokens": 1024})),
            Some(json!({"thinkingLevel": "low", "includeThoughts": true}))
        );
        assert_eq!(
            config(json!({"type": "enabled", "budget_tokens": 8000})),
            Some(json!({"thinkingLevel": "medium", "includeThoughts": true}))
        );
        assert_eq!(
            config(json!({"type": "enabled", "budget_tokens": 32000})),
            Some(json!({"thinkingLevel": "high", "includeThoughts": true}))
        );
    }

    #[tokio::test]
    async fn invalid_thinking_is_400() {
        for thinking in [
            json!({"type": "enabled"}),
            json!({"type": "enabled", "budget_tokens": 512}),
            json!({"type": "sometimes"}),
            json!("enabled"),
        ] {
            let (status, _) = expect_err(json!({
                "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
                "thinking": thinking
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{thinking}");
        }
    }

    #[tokio::test]
    async fn capability_matrix_rejects_non_default_values() {
        // Те же 4 правила, что у Codex skin 5.1, при эквивалентном входе.
        for (param, value) in [
            ("context_management", json!({"edits": []})),
            ("mcp_servers", json!([{"name": "srv"}])),
            ("container", json!({"id": "c"})),
            ("output_config", json!({"effort": "high"})),
        ] {
            let (status, json) = expect_err(json!({
                "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
                param: value,
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{param}");
            assert_eq!(json["error"]["type"], "invalid_request_error", "{param}");
            assert!(
                json["error"]["message"].as_str().unwrap().contains(param),
                "{json}"
            );
        }
        // Дефолтные значения (пустой mcp_servers) принимаются.
        ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "mcp_servers": []
        }));
    }

    #[test]
    fn metadata_accepted_and_ignored_sampling_honored() {
        let translated = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "metadata": {"user_id": "user_123"},
            "temperature": 0.2, "top_p": 0.5, "top_k": 40
        }));
        // metadata в GenerateContentRequest не попадает (как 5.1), sampling-контроли
        // проксируются в generationConfig (generateContent их умеет — как chat.rs; на
        // Codex-плоскости 5.1 игнорируются — плоскостное отличие).
        assert!(translated.body.get("metadata").is_none());
        let config = &translated.body["generationConfig"];
        assert_eq!(config["temperature"], 0.2);
        assert_eq!(config["topP"], 0.5);
        assert_eq!(config["topK"], 40);
    }

    #[tokio::test]
    async fn unknown_top_level_field_is_400() {
        // Закрытый список (отличие от Codex-плоскости 5.1 — как chat.rs): wrapper апстрима
        // выбросил бы поле молча.
        let (status, json) = expect_err(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "some_future_field": {"anything": true}
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert!(json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("some_future_field"));
    }

    #[test]
    fn stop_sequences_translate_natively() {
        let translated = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "stop_sequences": ["\n\n", "END", ""]
        }));
        // Пустые выбрасываются (leniency 5.1); нативное исполнение — как stop у chat.rs.
        assert_eq!(
            translated.body["generationConfig"]["stopSequences"],
            json!(["\n\n", "END"])
        );
    }

    #[tokio::test]
    async fn invalid_stop_sequences_are_400() {
        for stop in [
            json!(["a", "b", "c", "d", "e", "f"]), // больше публичного лимита 5 generateContent
            json!(["a", 1]),
            json!("END"),
        ] {
            let (status, _) = expect_err(json!({
                "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
                "stop_sequences": stop
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{stop}");
        }
    }

    #[tokio::test]
    async fn missing_required_fields_are_anthropic_shaped_400() {
        // Те же кейсы, что в contract-тесте 5.1 (namespace-префикс — google/).
        for body in [
            json!({"max_tokens": 1, "messages": [{"role": "user", "content": "hi"}]}),
            json!({"model": "m", "messages": [{"role": "user", "content": "hi"}]}),
            json!({"model": "m", "max_tokens": 1}),
            json!({"model": "m", "max_tokens": 1, "messages": []}),
            json!({"model": "google/", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}]}),
            json!({"model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": ""}]}),
            json!({"model": "m", "max_tokens": 0, "messages": [{"role": "user", "content": "hi"}]}),
        ] {
            let (status, json) = expect_err(body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
            assert_eq!(json["type"], "error", "{json}");
            assert_eq!(json["error"]["type"], "invalid_request_error", "{json}");
            assert!(
                json["error"].get("param").is_none(),
                "Anthropic envelope has no param: {json}"
            );
        }
    }

    #[tokio::test]
    async fn fully_dropped_history_is_400() {
        // Сообщение из одних выброшенных thinking-блоков не порождает content; полностью
        // пустая история — 400 (как 5.1).
        let (status, _) = expect_err(json!({
            "model": "m", "max_tokens": 1,
            "messages": [{"role": "assistant", "content": [
                {"type": "thinking", "thinking": "secret", "signature": "sig"}]}],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn same_role_messages_merge() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash", "max_tokens": 1,
            "messages": [
                {"role": "user", "content": "a"},
                {"role": "user", "content": [{"type": "text", "text": "b"}]},
                {"role": "assistant", "content": "c"},
                {"role": "assistant", "content": "d"},
                {"role": "user", "content": "e"}
            ]
        }));
        // Склейка одноролевых общим merge_or_push (как chat.rs 3.3); тексты внутри одного
        // user-сообщения — отдельные text-парты не склеиваются через границу сообщения.
        assert_eq!(
            translated.body["contents"],
            json!([
                {"role": "user", "parts": [{"text": "a"}, {"text": "b"}]},
                {"role": "model", "parts": [{"text": "c"}, {"text": "d"}]},
                {"role": "user", "parts": [{"text": "e"}]}
            ])
        );
    }

    #[test]
    fn max_tokens_is_optional_for_count_tokens() {
        // count_tokens (require_max_tokens = false): maxOutputTokens и пустой
        // generationConfig опускаются; stream-флаг игнорируется count-хендлером.
        let translated = translate_messages_request(
            json!({
                "model": "google/gemini-2.5-flash",
                "messages": [{"role": "user", "content": "hi"}],
                "stream": true
            }),
            false,
        )
        .expect("translation must succeed");
        assert!(translated.body.get("generationConfig").is_none());
        assert!(translated.stream);

        let translated = translate_messages_request(
            json!({
                "model": "gemini-2.5-flash",
                "messages": [{"role": "user", "content": "hi"}],
                "thinking": {"type": "enabled", "budget_tokens": 8000}
            }),
            false,
        )
        .expect("translation must succeed");
        let config = &translated.body["generationConfig"];
        assert!(config.get("maxOutputTokens").is_none());
        assert_eq!(
            config["thinkingConfig"],
            json!({"thinkingLevel": "medium", "includeThoughts": true})
        );
    }

    // ---------- перевод ответа: unit-маппинги ----------

    #[test]
    fn maps_stop_reasons() {
        // functionCall в ответе важнее остальных причин (как 5.1).
        assert_eq!(stop_reason(true, Some("STOP")), "tool_use");
        assert_eq!(stop_reason(true, Some("MAX_TOKENS")), "tool_use");
        assert_eq!(stop_reason(false, Some("MAX_TOKENS")), "max_tokens");
        // Класс content_filter (map_finish_reason chat.rs) → refusal Messages.
        assert_eq!(stop_reason(false, Some("SAFETY")), "refusal");
        assert_eq!(stop_reason(false, Some("PROHIBITED_CONTENT")), "refusal");
        assert_eq!(stop_reason(false, Some("STOP")), "end_turn");
        assert_eq!(stop_reason(false, Some("OTHER")), "end_turn");
        assert_eq!(stop_reason(false, None), "end_turn");
    }

    #[test]
    fn messages_usage_maps_gemini_fields() {
        let usage = messages_usage(&json!({
            "promptTokenCount": 10,
            "candidatesTokenCount": 5,
            "thoughtsTokenCount": 3,
            "totalTokenCount": 18,
            "cachedContentTokenCount": 4
        }));
        assert_eq!(usage["input_tokens"], 10);
        // output = candidates + thoughts (как тарифицирует metering).
        assert_eq!(usage["output_tokens"], 8);
        assert_eq!(usage["cache_read_input_tokens"], 4);
        assert_eq!(usage["output_tokens_details"]["thinking_tokens"], 3);

        // Нулевые cache/thoughts полей не порождают (как 5.1).
        let usage = messages_usage(&json!({"promptTokenCount": 10, "candidatesTokenCount": 5}));
        assert_eq!(usage, json!({"input_tokens": 10, "output_tokens": 5}));
    }

    /// Зеркало словаря 5.1 поверх форм 3.3/4.3: text-парты → один text-блок на позиции
    /// первого text-парта, functionCall → tool_use (args object → input, id toolu_<name>[_N]),
    /// thought → thinking без signature.
    #[test]
    fn content_blocks_mirror_the_messages_dictionary() {
        let parts = vec![
            json!({"text": "first ", "thought": true}),
            json!({"text": "second", "thought": true, "thoughtSignature": "sig"}),
            json!({"thoughtSignature": "sig-only"}),
            json!({"text": "answer"}),
            json!({"functionCall": {"name": "weather", "args": {"city": "Paris"}}}),
            json!({"functionCall": {"name": "weather", "args": {"city": "Lyon"}}}),
        ];
        let (blocks, has_tool_use) = content_blocks(Some(&parts));
        assert!(has_tool_use);
        assert_eq!(
            blocks,
            vec![
                // Каждый thought-парт — отдельный thinking-блок БЕЗ signature (решение 4/6);
                // thoughtSignature-only парт пропускается.
                json!({"type": "thinking", "thinking": "first "}),
                json!({"type": "thinking", "thinking": "second"}),
                json!({"type": "text", "text": "answer"}),
                json!({"type": "tool_use", "id": "toolu_weather", "name": "weather",
                    "input": {"city": "Paris"}}),
                json!({"type": "tool_use", "id": "toolu_weather_2", "name": "weather",
                    "input": {"city": "Lyon"}}),
            ]
        );
        assert!(blocks[0].get("signature").is_none());

        // Без текста text-блок не создаётся; functionCall без args → input {}.
        let parts = vec![json!({"functionCall": {"name": "f"}})];
        let (blocks, _) = content_blocks(Some(&parts));
        assert_eq!(
            blocks,
            vec![json!({"type": "tool_use", "id": "toolu_f", "name": "f", "input": {}})]
        );
    }

    // ---------- перевод ответа: non-stream ----------

    #[tokio::test]
    async fn json_response_text_usage_and_model() {
        let upstream = upstream_json(json!({
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "Hi"}, {"text": " there"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 2, "totalTokenCount": 5},
            "modelVersion": "gemini-2.5-flash-001"
        }));
        let response = json_messages_response(upstream, "gemini-2.5-flash".to_string()).await;
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["id"].as_str().unwrap().starts_with("msg_"));
        assert_eq!(body["type"], "message");
        assert_eq!(body["role"], "assistant");
        assert_eq!(body["model"], "gemini-2.5-flash-001");
        assert_eq!(
            body["content"],
            json!([{"type": "text", "text": "Hi there"}])
        );
        assert_eq!(body["stop_reason"], "end_turn");
        assert!(body["stop_sequence"].is_null());
        assert_eq!(body["usage"]["input_tokens"], 3);
        assert_eq!(body["usage"]["output_tokens"], 2);
    }

    #[tokio::test]
    async fn json_response_tool_use_and_max_tokens() {
        let upstream = upstream_json(json!({
            "candidates": [{
                "content": {"parts": [
                    {"functionCall": {"name": "get_weather", "args": {"city": "Paris"}}}
                ]},
                "finishReason": "STOP"
            }]
        }));
        let response = json_messages_response(upstream, "gemini-2.5-flash".to_string()).await;
        let (_, body) = err_parts(response).await;
        assert_eq!(body["stop_reason"], "tool_use");
        assert_eq!(
            body["content"][0],
            json!({"type": "tool_use", "id": "toolu_get_weather", "name": "get_weather",
                "input": {"city": "Paris"}})
        );

        let upstream = upstream_json(json!({
            "candidates": [{
                "content": {"parts": [{"text": "cut"}]},
                "finishReason": "MAX_TOKENS"
            }]
        }));
        let response = json_messages_response(upstream, "gemini-2.5-flash".to_string()).await;
        let (_, body) = err_parts(response).await;
        assert_eq!(body["stop_reason"], "max_tokens");
    }

    #[tokio::test]
    async fn json_response_safety_and_prompt_block_are_refusal() {
        let upstream = upstream_json(json!({
            "candidates": [{
                "content": {"parts": [{"text": "partial"}]},
                "finishReason": "SAFETY"
            }]
        }));
        let response = json_messages_response(upstream, "gemini-2.5-flash".to_string()).await;
        let (_, body) = err_parts(response).await;
        assert_eq!(body["stop_reason"], "refusal");
        assert_eq!(body["content"][0]["text"], "partial");

        // Блокировка промпта на входе: кандидатов нет — refusal с пустым content.
        let upstream = upstream_json(json!({
            "promptFeedback": {"blockReason": "SAFETY"},
            "usageMetadata": {"promptTokenCount": 7, "totalTokenCount": 7}
        }));
        let response = json_messages_response(upstream, "gemini-2.5-flash".to_string()).await;
        let (_, body) = err_parts(response).await;
        assert_eq!(body["stop_reason"], "refusal");
        assert_eq!(body["content"], json!([]));
        assert_eq!(body["usage"]["input_tokens"], 7);
        // model fallback — запрошенная (modelVersion не приехал).
        assert_eq!(body["model"], "gemini-2.5-flash");
    }

    #[tokio::test]
    async fn json_response_thought_parts_map_to_thinking_blocks() {
        let upstream = upstream_json(json!({
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"text": "Thinking. ", "thought": true},
                    {"text": "Answer."}
                ]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 2,
                "thoughtsTokenCount": 4, "totalTokenCount": 9}
        }));
        let response = json_messages_response(upstream, "gemini-2.5-flash".to_string()).await;
        let (_, body) = err_parts(response).await;
        assert_eq!(
            body["content"],
            json!([
                {"type": "thinking", "thinking": "Thinking. "},
                {"type": "text", "text": "Answer."}
            ])
        );
        // thoughts-токены в output (как тарифицирует metering) и в details.
        assert_eq!(body["usage"]["output_tokens"], 6);
        assert_eq!(body["usage"]["output_tokens_details"]["thinking_tokens"], 4);
    }

    #[tokio::test]
    async fn json_response_malformed_body_is_500() {
        let upstream = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("not json"))
            .unwrap();
        let response = json_messages_response(upstream, "gemini-2.5-flash".to_string()).await;
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["type"], "api_error");
    }

    // ---------- конверсия ошибок (Google → Anthropic) ----------

    #[tokio::test]
    async fn invalid_key_maps_to_401() {
        // Нативная плоскость отвечает 400 API_KEY_INVALID; Messages-клиент ждёт 401.
        let upstream = upstream_error(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"code":400,"message":"API key not valid.","status":"INVALID_ARGUMENT"}}"#,
            "invalid_key",
        );
        let response = convert_error_response(upstream).await;
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["type"], "error");
        assert_eq!(body["error"]["type"], "authentication_error");
        assert_eq!(body["error"]["message"], "API key not valid.");
    }

    #[tokio::test]
    async fn google_400_maps_to_invalid_request_error() {
        let upstream = upstream_error(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"code":400,"message":"Invalid JSON payload.","status":"INVALID_ARGUMENT"}}"#,
            "gemini_request_rejected",
        );
        let response = convert_error_response(upstream).await;
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert_eq!(body["error"]["message"], "Invalid JSON payload.");
    }

    #[tokio::test]
    async fn low_balance_keeps_402() {
        let upstream = upstream_error(
            StatusCode::PAYMENT_REQUIRED,
            r#"{"error":{"code":402,"message":"The account balance is insufficient for this request.","status":"FAILED_PRECONDITION"}}"#,
            "billing_limit",
        );
        let response = convert_error_response(upstream).await;
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("balance"));
    }

    #[tokio::test]
    async fn rate_limit_keeps_retry_after() {
        let mut upstream = upstream_error(
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"code":429,"message":"Resource has been exhausted. Please retry later.","status":"RESOURCE_EXHAUSTED"}}"#,
            "gemini_capacity_exhausted",
        );
        upstream
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("7"));
        let response = convert_error_response(upstream).await;
        assert_eq!(response.headers().get(header::RETRY_AFTER).unwrap(), "7");
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body["error"]["type"], "rate_limit_error");
    }

    #[tokio::test]
    async fn unavailable_maps_to_529_overloaded() {
        let upstream = upstream_error(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":{"code":503,"message":"The service is currently unavailable. Please retry shortly.","status":"UNAVAILABLE"}}"#,
            "gemini_profiles_unavailable",
        );
        let response = convert_error_response(upstream).await;
        let (status, body) = err_parts(response).await;
        // 503 → retryable 529 overloaded_error (как 5.1 — Claude Code восстанавливается).
        assert_eq!(status, StatusCode::from_u16(529).unwrap());
        assert_eq!(body["error"]["type"], "overloaded_error");
        assert_eq!(body["error"]["message"], "Overloaded");
    }

    #[tokio::test]
    async fn server_error_maps_to_sanitized_api_error() {
        let upstream = upstream_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"error":{"code":500,"message":"internal details","status":"INTERNAL"}}"#,
            "upstream_error",
        );
        let response = convert_error_response(upstream).await;
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["type"], "api_error");
        // Внутренности апстрима не протекают (санитайзер, как у local_err).
        assert_eq!(body["error"]["message"], "Internal server error");
    }

    // ---------- count_tokens ----------

    #[tokio::test]
    async fn count_tokens_response_maps_total_tokens() {
        let upstream = upstream_json(json!({"totalTokens": 42, "cachedContentTokenCount": 7}));
        let response = count_tokens_json_response(upstream).await;
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::OK);
        // Messages-контракт count_tokens — только input_tokens.
        assert_eq!(body, json!({"input_tokens": 42}));
    }

    #[tokio::test]
    async fn count_tokens_malformed_response_is_500() {
        let upstream = upstream_json(json!({"unexpected": true}));
        let response = count_tokens_json_response(upstream).await;
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["type"], "api_error");
    }

    // ---------- stream ----------

    #[tokio::test]
    async fn stream_text_dialog_contract() {
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hello\"}]}}],\"modelVersion\":\"gemini-2.5-flash-001\",\"usageMetadata\":{\"promptTokenCount\":3}}\n",
            "\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" world\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":2,\"totalTokenCount\":5}}\n",
        );
        let output = collect_stream(translate_events(events)).await;
        let frames = event_frames(&output);
        // message_start → text start/delta* → stop → message_delta → message_stop.
        assert_eq!(
            event_names(&frames),
            [
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ]
        );
        let message = &frames[0].1["message"];
        assert!(message["id"].as_str().unwrap().starts_with("msg_"));
        // message_start несёт сервёную модель (modelVersion зафиксирован до эмиссии) и
        // нулевой usage (authoritative usage — только в message_delta, как 5.1).
        assert_eq!(message["model"], "gemini-2.5-flash-001");
        assert_eq!(
            message["usage"],
            json!({"input_tokens": 0, "output_tokens": 0})
        );
        assert_eq!(frames[1].1["index"], 0);
        assert_eq!(
            frames[1].1["content_block"],
            json!({"type": "text", "text": ""})
        );
        assert_eq!(
            frames[2].1["delta"],
            json!({"type": "text_delta", "text": "Hello"})
        );
        assert_eq!(
            frames[3].1["delta"],
            json!({"type": "text_delta", "text": " world"})
        );
        assert_eq!(
            frames[4].1,
            json!({"type": "content_block_stop", "index": 0})
        );
        assert_eq!(frames[5].1["delta"]["stop_reason"], "end_turn");
        assert_eq!(
            frames[5].1["usage"],
            json!({"input_tokens": 3, "output_tokens": 2})
        );
        assert_eq!(frames[6].1, json!({"type": "message_stop"}));
    }

    #[tokio::test]
    async fn stream_thought_parts_become_thinking_deltas() {
        // thought-парты → thinking-блок со своими дельтами; thoughtSignature-only парт
        // события не порождает (решение 4/6); смена типа закрывает предыдущий блок.
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Thinking. \",\"thought\":true}]}}],\"modelVersion\":\"gemini-2.5-flash-001\"}\n",
            "\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"thoughtSignature\":\"sig\"},{\"text\":\"more\",\"thought\":true},{\"text\":\"Answer.\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":2,\"thoughtsTokenCount\":4,\"totalTokenCount\":9}}\n",
        );
        let output = collect_stream(translate_events(events)).await;
        let frames = event_frames(&output);
        assert_eq!(
            event_names(&frames),
            [
                "message_start",
                "content_block_start", // thinking (index 0)
                "content_block_delta", // "Thinking. "
                "content_block_delta", // "more" — тот же блок
                "content_block_stop",
                "content_block_start", // text (index 1)
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ]
        );
        assert_eq!(
            frames[1].1["content_block"],
            json!({"type": "thinking", "thinking": ""})
        );
        assert_eq!(
            frames[2].1["delta"],
            json!({"type": "thinking_delta", "thinking": "Thinking. "})
        );
        assert_eq!(
            frames[3].1["delta"],
            json!({"type": "thinking_delta", "thinking": "more"})
        );
        assert_eq!(frames[3].1["index"], 0);
        assert_eq!(frames[5].1["index"], 1);
        assert_eq!(
            frames[6].1["delta"],
            json!({"type": "text_delta", "text": "Answer."})
        );
        assert_eq!(
            frames[8].1["usage"]["output_tokens_details"]["thinking_tokens"],
            4
        );
    }

    #[tokio::test]
    async fn stream_function_call_block_without_input_json_delta() {
        // functionCall приходит целиком: tool_use-блок с ПОЛНЫМ input в content_block_start
        // и сразу stop — БЕЗ input_json_delta (аргумент-дельт на wire нет, как 3.3).
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Let me check.\"}]}}]}\n",
            "\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"city\":\"Paris\"}}}]}}]}\n",
            "\n",
            "data: {\"candidates\":[{\"finishReason\":\"STOP\"}]}\n",
        );
        let output = collect_stream(translate_events(events)).await;
        let frames = event_frames(&output);
        assert_eq!(
            event_names(&frames),
            [
                "message_start",
                "content_block_start", // text (index 0)
                "content_block_delta",
                "content_block_stop",
                "content_block_start", // tool_use (index 1)
                "content_block_stop",
                "message_delta",
                "message_stop",
            ]
        );
        assert_eq!(
            frames[4].1["content_block"],
            json!({"type": "tool_use", "id": "toolu_get_weather", "name": "get_weather",
                "input": {"city": "Paris"}})
        );
        // Ни одной input_json_delta во всём стриме.
        assert!(!output.contains("input_json_delta"), "{output}");
        assert_eq!(frames[6].1["delta"]["stop_reason"], "tool_use");
    }

    #[tokio::test]
    async fn stream_finish_reasons_map_to_stop_reason() {
        let events = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"cut\"}]},\"finishReason\":\"MAX_TOKENS\"}]}\n";
        let output = collect_stream(translate_events(events)).await;
        let frames = event_frames(&output);
        let delta = frames
            .iter()
            .find(|(event, _)| event == "message_delta")
            .unwrap();
        assert_eq!(delta.1["delta"]["stop_reason"], "max_tokens");

        // Блокировка промпта на входе: refusal с пустым стримом блоков.
        let events = "data: {\"promptFeedback\":{\"blockReason\":\"SAFETY\"},\"usageMetadata\":{\"promptTokenCount\":5,\"totalTokenCount\":5}}\n";
        let output = collect_stream(translate_events(events)).await;
        let frames = event_frames(&output);
        assert_eq!(
            event_names(&frames),
            ["message_start", "message_delta", "message_stop"]
        );
        assert_eq!(frames[1].1["delta"]["stop_reason"], "refusal");
        assert_eq!(frames[1].1["usage"]["input_tokens"], 5);
    }

    #[tokio::test]
    async fn stream_clean_eof_without_finish_terminates_with_error() {
        let events = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hi\"}]}}]}\n";
        let output = collect_stream(translate_events(events)).await;
        let frames = event_frames(&output);
        assert_eq!(frames.last().unwrap().0, "error", "{output}");
        assert_eq!(frames.last().unwrap().1["error"]["type"], "api_error");
        assert!(!event_names(&frames).contains(&"message_stop"));
    }

    #[tokio::test]
    async fn stream_error_frame_maps_to_anthropic_error_event() {
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"}]}}]}\n",
            "\n",
            "data: {\"error\":{\"code\":429,\"message\":\"Quota exceeded\",\"status\":\"RESOURCE_EXHAUSTED\"}}\n",
        );
        let output = collect_stream(translate_events(events)).await;
        let frames = event_frames(&output);
        // message_stop после error нет — error терминален.
        assert_eq!(frames.last().unwrap().0, "error");
        assert_eq!(
            frames.last().unwrap().1,
            json!({"type": "error",
                "error": {"type": "rate_limit_error", "message": "Quota exceeded"}})
        );
        assert!(!event_names(&frames).contains(&"message_stop"));
    }

    #[tokio::test]
    async fn stream_transport_error_terminates_with_error_event() {
        let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
            Ok(Bytes::from(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"He\"}]}}]}\n\n",
            )),
            Err(std::io::Error::other("reset")),
        ];
        let translator = GeminiMessagesSseTranslator::new(
            Box::pin(futures_util::stream::iter(chunks)),
            "gemini-2.5-flash".to_string(),
            SSE_PING_INTERVAL,
        );
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        assert_eq!(
            frames.last().unwrap().1,
            json!({"type": "error",
                "error": {"type": "api_error", "message": "The provider stream was interrupted."}})
        );
    }

    #[tokio::test]
    async fn stream_frames_split_across_chunks_are_reassembled() {
        let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
            Ok(Bytes::from("data: {\"candidates\":[{\"content\":{\"par")),
            Ok(Bytes::from("ts\":[{\"text\":\"He")),
            Ok(Bytes::from("llo\"}]}}]}\n\ndata: {\"candidates\":[{\"fin")),
            Ok(Bytes::from("ishReason\":\"MAX_TOKENS\"}]}\n\n")),
        ];
        let translator = GeminiMessagesSseTranslator::new(
            Box::pin(futures_util::stream::iter(chunks)),
            "gemini-2.5-flash".to_string(),
            SSE_PING_INTERVAL,
        );
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        let delta = frames
            .iter()
            .find(|(_, data)| data["delta"]["text"].is_string())
            .unwrap();
        assert_eq!(
            delta.1["delta"],
            json!({"type": "text_delta", "text": "Hello"})
        );
        let message_delta = frames
            .iter()
            .find(|(event, _)| event == "message_delta")
            .unwrap();
        assert_eq!(message_delta.1["delta"]["stop_reason"], "max_tokens");
    }

    #[tokio::test]
    async fn stream_malformed_frame_terminates_with_error() {
        let events = concat!(
            "data: {not json}\n\n",
            "data: {\"candidates\":[{\"finishReason\":\"STOP\"}]}\n"
        );
        let output = collect_stream(translate_events(events)).await;
        let frames = event_frames(&output);
        assert_eq!(frames.last().unwrap().0, "error", "{output}");
        assert!(!event_names(&frames).contains(&"message_stop"));
    }

    #[tokio::test]
    async fn stream_unterminated_finish_frame_and_unknown_fields_are_accepted() {
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\",\"futurePartField\":true}]},\"futureCandidateField\":1}],\"futureTopLevelField\":{}}\n\n",
            "data: {\"candidates\":[{\"finishReason\":\"STOP\"}]}"
        );
        let output = collect_stream(translate_events(events)).await;
        let frames = event_frames(&output);
        assert_eq!(frames.last().unwrap().0, "message_stop", "{output}");
        assert!(output.contains("ok"), "{output}");
    }

    #[tokio::test]
    async fn stream_heartbeat_pings_while_upstream_is_silent() {
        // Heartbeat `event: ping` между кадрами апстрима (как 5.1): на Gemini wire
        // ping-кадров нет, генерируется локально. В тесте интервал укорочен.
        let inner: ByteStream = Box::pin(futures_util::stream::pending());
        let mut translator = GeminiMessagesSseTranslator::new(
            inner,
            "gemini-2.5-flash".to_string(),
            Duration::from_millis(5),
        );
        let frame = tokio::time::timeout(Duration::from_secs(2), translator.next())
            .await
            .expect("ping must arrive without upstream frames")
            .expect("stream is alive")
            .expect("ping frame is bytes");
        assert_eq!(frame.as_ref(), b"event: ping\ndata: {}\n\n");
    }
}
