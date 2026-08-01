//! Universal Chat Completions → Anthropic Messages адаптер — этап 3.1
//! docs/engine/UNIFIED_ROUTER.md (решения 1–3).
//!
//! `POST /v1/chat/completions` на Anthropic-плоскости. Поток запроса:
//! парс chat-запроса → перевод в Messages JSON → внутренний `Request` на
//! `/v1/messages` → общий [`forward`] (auth, reserve, ротация, identity-инжект,
//! tee-метеринг, settle — без единого изменения) → перевод ответа: Messages
//! SSE → `chat.completion.chunk` либо JSON message → `chat.completion`.
//!
//! Capability matrix (решение 3): параметры, которые Messages не умеет
//! (n>1, penalties, logprobs, seed, store, tools до этапа 3.2, structured
//! output и reasoning до этапа 3.4, …), с не-дефолтным значением отклоняются
//! `400 unsupported_parameter`; с дефолтным — принимаются (stock SDK шлют
//! дефолты пачками). Неизвестные адаптеру поля проксируются в Messages тело
//! как есть (открытый список; валидация — на апстриме).
//!
//! Все ответы этого пути — OpenAI-совместимый конверт, включая ошибки:
//! синтетические ошибки плоскости (`local_err`) и пасsthrough-ошибки апстрима
//! переводятся из Anthropic-конверта `{"type":"error",...}` в
//! `{"error":{"message","type","param","code"}}` с сохранением HTTP-статуса и
//! `Retry-After`. Статус 402 LowBalance сохраняется (контракт docs-portal).

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::BytesMut;
use futures_util::{Stream, StreamExt};
use serde_json::{json, Map, Value};

use crate::codex::new_id;
use crate::proxy::{forward, read_body_limited, BodyReadError, TerminalErrorReason};
use crate::state::AppState;

/// Лимит тела chat-запроса — тот же 32 MiB, что у native `/v1/messages`
/// (публичный предел Anthropic Messages; images этапа 3.4 в него вписываются).
const CHAT_BODY_LIMIT: usize = 32 * 1024 * 1024;

/// Верхняя граница буферизации error/non-stream тел ответа `forward()`.
/// Длинный non-stream completion укладывается в тот же 32 MiB предел.
const RESPONSE_BODY_LIMIT: usize = 32 * 1024 * 1024;

/// Дефолт `max_tokens`, когда chat-запрос его не задал: системная конвенция
/// reserve-пути (proxy.rs трактует отсутствующий `max_tokens` как 4096).
const DEFAULT_MAX_TOKENS: u64 = 4096;

/// Хендлер `POST /v1/chat/completions` (роут регистрируется в server только в
/// `ProviderMode::Anthropic`).
pub async fn anthropic_chat_completions(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    let (parts, body) = request.into_parts();
    let raw = match read_body_limited(body, CHAT_BODY_LIMIT).await {
        Ok(raw) => raw,
        Err(BodyReadError::TooLarge) => {
            return chat_error(
                StatusCode::BAD_REQUEST,
                "Request body exceeds the 32 MiB limit.",
                None,
                Value::Null,
                "invalid_chat_request",
            )
        }
        Err(BodyReadError::Read) => {
            return chat_error(
                StatusCode::BAD_REQUEST,
                "Could not read request body.",
                None,
                Value::Null,
                "invalid_chat_request",
            )
        }
    };
    let value: Value = match serde_json::from_slice(&raw) {
        Ok(value) => value,
        Err(_) => {
            return chat_error(
                StatusCode::BAD_REQUEST,
                "Invalid JSON in request body.",
                None,
                Value::Null,
                "invalid_chat_request",
            )
        }
    };
    let translated = match translate_chat_request(value) {
        Ok(translated) => translated,
        Err(response) => return response,
    };

    // Внутренний запрос на /v1/messages: admission, reserve, ротация,
    // identity-инжект, tee-метеринг и settle выполняет общий forward().
    // Заголовки клиента сохраняются (authorize читает ключи из них), меняется
    // только content-length/content-type под синтезированное тело.
    let mut headers = parts.headers.clone();
    headers.remove(header::CONTENT_LENGTH);
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let body_bytes = match serde_json::to_vec(&translated.body) {
        Ok(bytes) => bytes,
        Err(_) => {
            return chat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build the upstream request.",
                None,
                Value::Null,
                "internal_response_error",
            )
        }
    };
    let mut inner = Request::builder()
        .method(Method::POST)
        .uri("/v1/messages")
        .body(Body::from(body_bytes))
        .expect("static request builder is infallible");
    *inner.headers_mut() = headers;
    let upstream = forward(State(app), ConnectInfo(peer), inner).await;

    if upstream.status() != StatusCode::OK {
        return convert_error_response(upstream).await;
    }
    if translated.stream {
        stream_chat_response(upstream, translated.model, translated.include_usage)
    } else {
        json_chat_response(upstream).await
    }
}

/// Результат перевода chat-запроса: тело Messages и параметры, нужные для
/// перевода ответа.
#[derive(Debug)]
struct Translated {
    body: Value,
    /// Запрошенная модель с уже снятым `anthropic/`-префиксом — фолбэк для
    /// поля `model` ответа, если апстрим его не вернул.
    model: String,
    stream: bool,
    include_usage: bool,
}

/// Перевод chat-запроса в Messages JSON. Ошибки — готовые OpenAI-shaped
/// ответы (400).
fn translate_chat_request(value: Value) -> Result<Translated, Response> {
    let mut object = match value {
        Value::Object(object) => object,
        _ => {
            return Err(chat_error(
                StatusCode::BAD_REQUEST,
                "Request body must be a JSON object.",
                None,
                Value::Null,
                "invalid_chat_request",
            ))
        }
    };

    check_capability_matrix(&object)?;

    let model = match object.remove("model") {
        Some(Value::String(model)) => model,
        _ => {
            return Err(invalid_request(
                "Missing or invalid required parameter: model.",
                Some("model"),
            ))
        }
    };
    // Namespaced ID резолвится здесь, а не в router и не в metering: после
    // strip'а reserve (fuzzy) и strict (exact canonical) видят нативный id.
    let model = model
        .strip_prefix("anthropic/")
        .unwrap_or(&model)
        .to_string();
    if model.is_empty() {
        return Err(invalid_request(
            "Missing or invalid required parameter: model.",
            Some("model"),
        ));
    }

    let messages = match object.remove("messages") {
        Some(Value::Array(messages)) if !messages.is_empty() => messages,
        _ => {
            return Err(invalid_request(
                "Missing or invalid required parameter: messages.",
                Some("messages"),
            ))
        }
    };
    let (system, conversation) = translate_messages(messages)?;

    let stream = object
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_usage = parse_stream_options(object.get("stream_options"))?;

    let max_tokens = match object
        .get("max_completion_tokens")
        .or_else(|| object.get("max_tokens"))
        .and_then(Value::as_u64)
    {
        Some(tokens) if tokens > 0 => tokens,
        _ => DEFAULT_MAX_TOKENS,
    };

    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(model.clone()));
    body.insert("messages".to_string(), Value::Array(conversation));
    body.insert("max_tokens".to_string(), Value::from(max_tokens));
    body.insert("stream".to_string(), Value::Bool(stream));
    if !system.is_empty() {
        body.insert("system".to_string(), Value::Array(system));
    }
    if let Some(stop) = translate_stop(object.get("stop"))? {
        body.insert("stop_sequences".to_string(), stop);
    }
    // Honored-параметры с совпадающими именами.
    for key in ["temperature", "top_p"] {
        if let Some(value) = object.get(key).filter(|v| !v.is_null()) {
            body.insert(key.to_string(), value.clone());
        }
    }
    if let Some(user) = object.get("user") {
        match user.as_str() {
            Some(user) => {
                body.insert(
                    "metadata".to_string(),
                    json!({"user_id": user}),
                );
            }
            None => {
                return Err(invalid_request(
                    "Invalid type for parameter: user must be a string.",
                    Some("user"),
                ))
            }
        }
    }
    // Открытый список (решение 3): неизвестные адаптеру поля проксируются в
    // Messages тело. Известные consumed/matrix-ключи сюда не попадают.
    for (key, value) in &object {
        if !CONSUMED_KEYS.contains(&key.as_str()) && !body.contains_key(key) {
            body.insert(key.clone(), value.clone());
        }
    }

    Ok(Translated {
        body: Value::Object(body),
        model,
        stream,
        include_usage,
    })
}

/// Ключи chat-запроса, снятые переводом или capability matrix. Всё остальное
/// проксируется в Messages тело (открытый список).
const CONSUMED_KEYS: &[&str] = &[
    "model",
    "messages",
    "max_tokens",
    "max_completion_tokens",
    "stream",
    "stream_options",
    "stop",
    "temperature",
    "top_p",
    "user",
    // Capability matrix: отклонены при не-дефолте, при дефолте — сняты.
    "n",
    "presence_penalty",
    "frequency_penalty",
    "logit_bias",
    "logprobs",
    "top_logprobs",
    "seed",
    "parallel_tool_calls",
    "store",
    "metadata",
    "service_tier",
    "modalities",
    "audio",
    "prediction",
    "web_search_options",
    "response_format",
    "tools",
    "tool_choice",
    "functions",
    "function_call",
    "reasoning_effort",
];

/// Capability matrix (решение 3): параметр, который Messages не умеет, с
/// не-дефолтным значением → `400 unsupported_parameter`. Порядок правил
/// определяет, какой параметр назовёт ошибка.
fn check_capability_matrix(object: &Map<String, Value>) -> Result<(), Response> {
    let rules: [(&str, fn(&Value) -> bool); 22] = [
        ("tools", |v| v.is_null() || v.as_array().is_some_and(Vec::is_empty)),
        ("functions", |v| v.is_null() || v.as_array().is_some_and(Vec::is_empty)),
        ("tool_choice", |v| v.is_null() || v.as_str() == Some("auto")),
        ("function_call", |v| v.is_null() || v.as_str() == Some("auto")),
        ("parallel_tool_calls", |v| v.as_bool() == Some(true)),
        ("response_format", |v| {
            v.is_null() || v.get("type").and_then(Value::as_str) == Some("text")
        }),
        ("reasoning_effort", |v| v.is_null()),
        ("store", |v| v.is_null() || v.as_bool() == Some(false)),
        ("metadata", |v| v.is_null()),
        ("n", |v| v.as_u64() == Some(1)),
        ("presence_penalty", |v| v.as_f64() == Some(0.0)),
        ("frequency_penalty", |v| v.as_f64() == Some(0.0)),
        ("logit_bias", |v| v.is_null() || v.as_object().is_some_and(Map::is_empty)),
        ("logprobs", |v| v.is_null() || v.as_bool() == Some(false)),
        ("top_logprobs", |v| v.is_null()),
        ("seed", |v| v.is_null()),
        ("service_tier", |v| {
            v.is_null() || v.as_str() == Some("auto") || v.as_str() == Some("default")
        }),
        ("modalities", |v| {
            v.as_array().is_some_and(|m| m.len() == 1 && m[0].as_str() == Some("text"))
        }),
        ("audio", |v| v.is_null()),
        ("prediction", |v| v.is_null()),
        ("web_search_options", |v| v.is_null()),
        ("stream_options", |v| {
            v.as_object().is_some_and(|o| o.keys().all(|k| k == "include_usage"))
        }),
    ];
    for (param, is_default) in rules {
        if let Some(value) = object.get(param) {
            if !is_default(value) {
                return Err(unsupported_parameter(param));
            }
        }
    }
    Ok(())
}

/// Перевод массива chat-сообщений в `(system blocks, messages)`.
/// system/developer → top-level `system` Messages (массив text-блоков).
/// Подряд идущие сообщения одной роли склеиваются: Messages требует
/// чередования user/assistant, а OpenAI-клиенты шлют последовательные
/// одноролевые сообщения штатно.
fn translate_messages(messages: Vec<Value>) -> Result<(Vec<Value>, Vec<Value>), Response> {
    let mut system = Vec::new();
    let mut conversation: Vec<Value> = Vec::new();
    for message in messages {
        let object = message.as_object().ok_or_else(|| {
            invalid_request("Each message must be a JSON object.", Some("messages"))
        })?;
        let role = object.get("role").and_then(Value::as_str).ok_or_else(|| {
            invalid_request("Each message must have a string role.", Some("messages"))
        })?;
        // Message-level поля, относящиеся к tools (этап 3.2) или
        // не существующие в Messages: не-дефолт → 400.
        for field in ["tool_calls", "function_call", "tool_call_id"] {
            if object.get(field).is_some_and(|v| !v.is_null()) {
                return Err(unsupported_parameter(field));
            }
        }
        if object.get("name").is_some_and(|v| !v.is_null()) {
            return Err(unsupported_parameter("name"));
        }
        match role {
            "system" | "developer" => {
                let text = message_text(object.get("content"))?;
                system.push(json!({"type": "text", "text": text}));
            }
            "user" | "assistant" => {
                let blocks = message_blocks(role, object.get("content"))?;
                if let Some(last) = conversation.last_mut() {
                    if last.get("role").and_then(Value::as_str) == Some(role) {
                        // Склейка одноролевых подряд: блоки конкатенируются.
                        let merged = last
                            .get_mut("content")
                            .and_then(Value::as_array_mut)
                            .expect("translated message content is an array");
                        merged.extend(blocks);
                        continue;
                    }
                }
                conversation.push(json!({"role": role, "content": blocks}));
            }
            // tool/function-роли — этап 3.2; сейчас fail-closed.
            "tool" | "function" => return Err(unsupported_parameter("messages")),
            _ => {
                return Err(invalid_request(
                    "Invalid message role: expected system, developer, user or assistant.",
                    Some("messages"),
                ))
            }
        }
    }
    if conversation.is_empty() {
        return Err(invalid_request(
            "messages must contain at least one user or assistant message.",
            Some("messages"),
        ));
    }
    Ok((system, conversation))
}

/// Текст system/developer-сообщения: строка либо массив text-частей
/// (склеиваются через \n). Нетекстовые части — 400 (этапы 3.2–3.4).
fn message_text(content: Option<&Value>) -> Result<String, Response> {
    match content {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Array(parts)) => {
            let mut texts = Vec::with_capacity(parts.len());
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        texts.push(
                            part.get("text")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        );
                    }
                    _ => return Err(unsupported_parameter("messages")),
                }
            }
            Ok(texts.join("\n"))
        }
        _ => Err(invalid_request(
            "Invalid message content: expected a string or an array of parts.",
            Some("messages"),
        )),
    }
}

/// Контент user/assistant-сообщения → массив Messages-блоков. В 3.1
/// поддерживается только текст; image/audio/file/refusal-части идут в
/// `400 unsupported_parameter` (этапы 3.2–3.4).
fn message_blocks(role: &str, content: Option<&Value>) -> Result<Vec<Value>, Response> {
    let text = message_text(content)?;
    if text.is_empty() && role == "user" {
        // Messages не принимает пустые text-блоки; пустое user-сообщение
        // бессмысленно и для chat-входа — отклоняем честно.
        return Err(invalid_request(
            "User message content must not be empty.",
            Some("messages"),
        ));
    }
    Ok(vec![json!({"type": "text", "text": text})])
}

/// `stop`: строка или массив до 4 непустых строк → `stop_sequences`
/// (Messages исполняет их нативно, эмуляция не нужна).
fn translate_stop(value: Option<&Value>) -> Result<Option<Value>, Response> {
    let value = match value.filter(|v| !v.is_null()) {
        Some(value) => value,
        None => return Ok(None),
    };
    let sequences: Vec<Value> = match value {
        Value::String(s) if !s.is_empty() => vec![Value::String(s.clone())],
        Value::Array(items)
            if !items.is_empty()
                && items.len() <= 4
                && items
                    .iter()
                    .all(|i| i.as_str().is_some_and(|s| !s.is_empty())) =>
        {
            items.clone()
        }
        _ => {
            return Err(invalid_request(
                "Invalid stop: expected a string or an array of up to 4 non-empty strings.",
                Some("stop"),
            ))
        }
    };
    Ok(Some(Value::Array(sequences)))
}

/// `stream_options`: honor `include_usage`; любой другой ключ отклонён в
/// capability matrix.
fn parse_stream_options(value: Option<&Value>) -> Result<bool, Response> {
    match value.filter(|v| !v.is_null()) {
        None => Ok(false),
        Some(Value::Object(object)) => Ok(object
            .get("include_usage")
            .and_then(Value::as_bool)
            .unwrap_or(false)),
        _ => Err(invalid_request(
            "Invalid stream_options: expected an object.",
            Some("stream_options"),
        )),
    }
}

/// OpenAI `finish_reason` из Anthropic `stop_reason`.
fn map_finish_reason(stop_reason: Option<&str>) -> &'static str {
    match stop_reason {
        Some("max_tokens") | Some("model_context_window_exceeded") => "length",
        Some("tool_use") => "tool_calls",
        // end_turn / stop_sequence / refusal / неизвестное — обычная остановка.
        _ => "stop",
    }
}

/// OpenAI `usage` из Messages usage. Входная сторона включает cache
/// creation/read (биллинг тарифицирует их отдельно внутри; клиенту —
/// суммарный prompt), cache read отражается в `prompt_tokens_details`.
fn map_usage(usage: &Value) -> Value {
    let tokens = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let input = tokens("input_tokens");
    let cache_creation = tokens("cache_creation_input_tokens");
    let cache_read = tokens("cache_read_input_tokens");
    let output = tokens("output_tokens");
    let prompt = input
        .saturating_add(cache_creation)
        .saturating_add(cache_read);
    let mut mapped = json!({
        "prompt_tokens": prompt,
        "completion_tokens": output,
        "total_tokens": prompt.saturating_add(output),
    });
    if cache_read > 0 {
        mapped["prompt_tokens_details"] = json!({"cached_tokens": cache_read});
    }
    mapped
}

/// OpenAI `type` ошибки по HTTP-статусу (класс ошибки клиент видит стабильно,
/// детальный Anthropic-тип уезжает в `code`).
fn openai_error_type(status: StatusCode) -> &'static str {
    match status.as_u16() {
        400 | 402 | 404 | 405 | 409 | 413 | 422 => "invalid_request_error",
        401 | 403 => "authentication_error",
        429 => "rate_limit_error",
        _ => "server_error",
    }
}

/// Единая точка синтетических OpenAI-ошибок адаптера. `reason` — статический
/// код для audit-middleware (TerminalErrorReason), как у local_err.
fn chat_error(
    status: StatusCode,
    message: &str,
    param: Option<&str>,
    code: Value,
    reason: &'static str,
) -> Response {
    let mut response = (
        status,
        axum::Json(json!({"error": {
            "message": message,
            "type": openai_error_type(status),
            "param": param,
            "code": code,
        }})),
    )
        .into_response();
    response
        .extensions_mut()
        .insert(TerminalErrorReason(reason));
    response
}

fn invalid_request(message: &str, param: Option<&str>) -> Response {
    chat_error(
        StatusCode::BAD_REQUEST,
        message,
        param,
        Value::Null,
        "invalid_chat_request",
    )
}

fn unsupported_parameter(param: &str) -> Response {
    chat_error(
        StatusCode::BAD_REQUEST,
        &format!("Unsupported parameter: '{param}' is not supported with this endpoint."),
        Some(param),
        Value::String("unsupported_parameter".to_string()),
        "unsupported_parameter",
    )
}

/// Перевод не-200 ответа `forward()` (наш `local_err` или пасsthrough-ошибка
/// апстрима) из Anthropic-конверта в OpenAI-конверт. Статус и `Retry-After`
/// сохраняются; audit-reason `local_err` пробрасывается в extension.
async fn convert_error_response(upstream: Response) -> Response {
    let status = upstream.status();
    let reason = upstream
        .extensions()
        .get::<TerminalErrorReason>()
        .map(|r| r.0)
        .unwrap_or("upstream_error_response");
    let retry_after = upstream.headers().get(header::RETRY_AFTER).cloned();
    let bytes = to_bytes(upstream.into_body(), RESPONSE_BODY_LIMIT)
        .await
        .unwrap_or_default();
    let parsed = serde_json::from_slice::<Value>(&bytes).ok();
    let inner = parsed.as_ref().and_then(|v| v.get("error"));
    let message = inner
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("The provider returned an error.");
    let code = inner
        .and_then(|e| e.get("type"))
        .and_then(Value::as_str)
        .map(|t| Value::String(t.to_string()))
        .unwrap_or(Value::Null);
    let mut response = chat_error(status, message, None, code, reason);
    if let Some(retry_after) = retry_after {
        response.headers_mut().insert(header::RETRY_AFTER, retry_after);
    }
    response
}

/// Перевод non-stream Messages-ответа в `chat.completion`.
async fn json_chat_response(upstream: Response) -> Response {
    let request_id = upstream.headers().get("request-id").cloned();
    let bytes = match to_bytes(upstream.into_body(), RESPONSE_BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return chat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The provider returned an unreadable response.",
                None,
                Value::Null,
                "internal_response_error",
            )
        }
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => {
            return chat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The provider returned a malformed response.",
                None,
                Value::Null,
                "internal_response_error",
            )
        }
    };
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let text = value
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .unwrap_or_default();
    let finish = map_finish_reason(value.get("stop_reason").and_then(Value::as_str));
    let usage = value
        .get("usage")
        .map(map_usage)
        .unwrap_or(Value::Null);
    let completion = json!({
        "id": new_id("chatcmpl"),
        "object": "chat.completion",
        "created": pool::now(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": finish,
        }],
        "usage": usage,
    });
    let mut response = axum::Json(completion).into_response();
    if let Some(request_id) = request_id {
        response.headers_mut().insert("request-id", request_id);
    }
    response
}

type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// Перевод SSE-ответа `forward()` в поток `chat.completion.chunk`.
/// Транслятор навешивается СНАРУЖИ ответа forward(): TeeMeter внутри уже
/// протапал оригинальные Anthropic-байты (usage/settle не меняются), а
/// SseErrorTail гарантирует `event: error` на транспортном обрыве — он
/// переводится в OpenAI error frame по тому же правилу.
fn stream_chat_response(upstream: Response, requested_model: String, include_usage: bool) -> Response {
    let request_id = upstream.headers().get("request-id").cloned();
    let stream = upstream
        .into_body()
        .into_data_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let translator = ChatSseTranslator::new(Box::pin(stream), requested_model, include_usage);
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(translator))
        .unwrap_or_else(|_| {
            chat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error",
                None,
                Value::Null,
                "internal_response_error",
            )
        });
    if let Some(request_id) = request_id {
        response.headers_mut().insert("request-id", request_id);
    }
    response
}

/// Потоковый транслятор Messages SSE → chat.completion.chunk SSE.
/// Буферизуются только байты одного незакрытого SSE-кадра; готовые чанки
/// отдаются немедленно (первый чанк не ждёт конца стрима).
struct ChatSseTranslator {
    inner: ByteStream,
    buf: BytesMut,
    out: VecDeque<Bytes>,
    id: String,
    created: i64,
    /// Запрошенная (native) модель — фолбэк, пока/если `message_start` не
    /// сообщил сервёную.
    requested_model: String,
    served_model: Option<String>,
    include_usage: bool,
    /// usage из `message_start` (input-сторона + cache поля).
    start_usage: Option<Value>,
    output_tokens: Option<u64>,
    role_sent: bool,
    /// Терминальный кадр (`[DONE]` или error) уже поставлен в `out`.
    finished: bool,
}

impl ChatSseTranslator {
    fn new(inner: ByteStream, requested_model: String, include_usage: bool) -> Self {
        Self {
            inner,
            buf: BytesMut::new(),
            out: VecDeque::new(),
            id: new_id("chatcmpl"),
            created: pool::now(),
            requested_model,
            served_model: None,
            include_usage,
            start_usage: None,
            output_tokens: None,
            role_sent: false,
            finished: false,
        }
    }

    fn model(&self) -> &str {
        self.served_model.as_deref().unwrap_or(&self.requested_model)
    }

    fn frame(value: Value) -> Bytes {
        let mut frame = String::with_capacity(256);
        frame.push_str("data: ");
        frame.push_str(&value.to_string());
        frame.push_str("\n\n");
        Bytes::from(frame)
    }

    fn chunk(&self, delta: Value, finish_reason: Value) -> Value {
        json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model(),
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish_reason}],
        })
    }

    fn push_chunk(&mut self, delta: Value, finish_reason: Value) {
        let chunk = self.chunk(delta, finish_reason);
        self.out.push_back(Self::frame(chunk));
    }

    /// OpenAI-ошибка внутри стрима: кадр `{"error": ...}` без `[DONE]`
    /// (поведение самого OpenAI при mid-stream сбое).
    fn push_error(&mut self, message: &str, code: Option<&str>) {
        let kind = match code {
            Some("rate_limit_error") => "rate_limit_error",
            _ => "server_error",
        };
        self.out.push_back(Self::frame(json!({"error": {
            "message": message,
            "type": kind,
            "code": code,
        }})));
        self.finished = true;
    }

    /// Один SSE-кадр (`event:` + `data:`). Неразборчивые кадры пропускаются:
    /// протокол апстрима может вырасти новыми событиями, стрим от этого
    /// ломаться не должен.
    fn handle_event(&mut self, event: &str, data: &str) {
        let data: Value = match serde_json::from_str(data) {
            Ok(data) => data,
            Err(_) => return,
        };
        match event {
            "message_start" => {
                if let Some(message) = data.get("message") {
                    if let Some(model) = message.get("model").and_then(Value::as_str) {
                        self.served_model = Some(model.to_string());
                    }
                    if let Some(usage) = message.get("usage") {
                        self.start_usage = Some(usage.clone());
                    }
                }
                if !self.role_sent {
                    self.role_sent = true;
                    self.push_chunk(json!({"role": "assistant", "content": ""}), Value::Null);
                }
            }
            "content_block_delta" => {
                let delta = data.get("delta");
                if delta.and_then(|d| d.get("type")).and_then(Value::as_str)
                    == Some("text_delta")
                {
                    if let Some(text) = delta
                        .and_then(|d| d.get("text"))
                        .and_then(Value::as_str)
                        .filter(|t| !t.is_empty())
                    {
                        self.push_chunk(json!({"content": text}), Value::Null);
                    }
                }
                // thinking/redacted/tool deltas — этапы 3.2/3.4; здесь их
                // быть не может (адаптер не запрашивает), пропускаем.
            }
            "message_delta" => {
                if let Some(output) = data
                    .get("usage")
                    .and_then(|u| u.get("output_tokens"))
                    .and_then(Value::as_u64)
                {
                    self.output_tokens = Some(output);
                }
                if let Some(stop_reason) = data
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    let finish = map_finish_reason(Some(stop_reason));
                    self.push_chunk(json!({}), Value::String(finish.to_string()));
                }
            }
            "message_stop" => {
                if self.include_usage {
                    let mut usage = self.start_usage.take().unwrap_or_else(|| json!({}));
                    if let Some(output) = self.output_tokens {
                        usage["output_tokens"] = Value::from(output);
                    }
                    let chunk = json!({
                        "id": self.id,
                        "object": "chat.completion.chunk",
                        "created": self.created,
                        "model": self.model(),
                        "choices": [],
                        "usage": map_usage(&usage),
                    });
                    self.out.push_back(Self::frame(chunk));
                }
                self.out.push_back(Bytes::from_static(b"data: [DONE]\n\n"));
                self.finished = true;
            }
            "ping" => {
                // Keepalive: пустой delta-чанк, как heartbeat OpenAI-плоскости.
                self.push_chunk(json!({}), Value::Null);
            }
            "error" => {
                let error = data.get("error");
                let message = error
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("The provider returned an error.");
                let code = error
                    .and_then(|e| e.get("type"))
                    .and_then(Value::as_str);
                self.push_error(message, code);
            }
            // content_block_start/stop и неизвестные события не несут
            // client-visible дельт для text-only профиля 3.1.
            _ => {}
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
            let mut event = "";
            let mut data_lines: Vec<String> = Vec::new();
            for line in text.split('\n') {
                let line = line.strip_suffix('\r').unwrap_or(line);
                if let Some(value) = line.strip_prefix("event:") {
                    event = value.trim_start();
                } else if let Some(value) = line.strip_prefix("data:") {
                    data_lines.push(value.trim_start().to_string());
                }
            }
            if event.is_empty() && data_lines.is_empty() {
                continue;
            }
            let data = data_lines.join("\n");
            self.handle_event(event, &data);
            if self.finished {
                self.buf.clear();
                return;
            }
        }
    }
}

impl Stream for ChatSseTranslator {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(bytes) = self.out.pop_front() {
                return Poll::Ready(Some(Ok(bytes)));
            }
            if self.finished {
                return Poll::Ready(None);
            }
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    self.buf.extend_from_slice(&chunk);
                    self.drain_frames();
                }
                // SseErrorTail внутри forward() уже подменил транспортный сбой
                // на `event: error`; голый Err сюда доходить не должен, но
                // стрим не обрываем молча ни при каком раскладе.
                Poll::Ready(Some(Err(_))) => {
                    self.push_error("The provider stream was interrupted.", None);
                }
                Poll::Ready(None) => {
                    // Чистый EOF без message_stop (апстрим закрылся раньше
                    // протокола): добиваем [DONE], чтобы клиент не висел.
                    self.out.push_back(Bytes::from_static(b"data: [DONE]\n\n"));
                    self.finished = true;
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    fn chat_request(value: serde_json::Value) -> Value {
        value
    }

    fn ok_translated(value: Value) -> Translated {
        translate_chat_request(chat_request(value)).expect("translation must succeed")
    }

    async fn err_parts(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn expect_err(value: Value) -> (StatusCode, Value) {
        err_parts(translate_chat_request(chat_request(value)).unwrap_err()).await
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

    fn data_frames(output: &str) -> Vec<Value> {
        output
            .split("\n\n")
            .filter(|frame| !frame.trim().is_empty())
            .filter_map(|frame| {
                frame
                    .strip_prefix("data: ")
                    .and_then(|data| serde_json::from_str(data).ok())
            })
            .collect()
    }

    // ---------- перевод запроса ----------

    #[test]
    fn translates_basic_chat_to_messages() {
        let translated = ok_translated(serde_json::json!({
            "model": "anthropic/claude-opus-4-8",
            "messages": [
                {"role": "system", "content": "Be terse."},
                {"role": "developer", "content": "Extra guard."},
                {"role": "user", "content": "Hello"}
            ]
        }));
        let body = &translated.body;
        assert_eq!(body["model"], "claude-opus-4-8");
        assert_eq!(body["max_tokens"], DEFAULT_MAX_TOKENS);
        assert_eq!(body["stream"], false);
        assert!(!translated.include_usage);
        assert_eq!(
            body["system"],
            serde_json::json!([
                {"type": "text", "text": "Be terse."},
                {"type": "text", "text": "Extra guard."}
            ])
        );
        assert_eq!(
            body["messages"],
            serde_json::json!([{"role": "user", "content": [{"type": "text", "text": "Hello"}]}])
        );
    }

    #[test]
    fn max_completion_tokens_wins_over_legacy_max_tokens() {
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100,
            "max_completion_tokens": 500
        }));
        assert_eq!(translated.body["max_tokens"], 500);

        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100
        }));
        assert_eq!(translated.body["max_tokens"], 100);
    }

    #[test]
    fn merges_consecutive_same_role_messages() {
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "messages": [
                {"role": "user", "content": "one"},
                {"role": "user", "content": [{"type": "text", "text": "two"}]},
                {"role": "assistant", "content": "answer"},
                {"role": "user", "content": "three"}
            ]
        }));
        assert_eq!(
            translated.body["messages"],
            serde_json::json!([
                {"role": "user", "content": [
                    {"type": "text", "text": "one"},
                    {"type": "text", "text": "two"}
                ]},
                {"role": "assistant", "content": [{"type": "text", "text": "answer"}]},
                {"role": "user", "content": [{"type": "text", "text": "three"}]}
            ])
        );
    }

    #[test]
    fn stop_temperature_top_p_and_user_are_honored() {
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "messages": [{"role": "user", "content": "hi"}],
            "stop": ["END", "STOP"],
            "temperature": 0.7,
            "top_p": 0.9,
            "user": "user-42"
        }));
        assert_eq!(translated.body["stop_sequences"], serde_json::json!(["END", "STOP"]));
        assert_eq!(translated.body["temperature"], 0.7);
        assert_eq!(translated.body["top_p"], 0.9);
        assert_eq!(translated.body["metadata"], serde_json::json!({"user_id": "user-42"}));

        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "messages": [{"role": "user", "content": "hi"}],
            "stop": "END"
        }));
        assert_eq!(translated.body["stop_sequences"], serde_json::json!(["END"]));
    }

    #[test]
    fn unknown_fields_proxy_into_messages_body() {
        // Открытый список (решение 3): неизвестные адаптеру поля уходят в
        // Messages тело как есть; валидация — на апстриме.
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "messages": [{"role": "user", "content": "hi"}],
            "top_k": 40,
            "future_openai_field": {"x": 1}
        }));
        assert_eq!(translated.body["top_k"], 40);
        assert_eq!(translated.body["future_openai_field"], serde_json::json!({"x": 1}));
    }

    #[test]
    fn stream_flags_are_parsed() {
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "messages": [{"role": "user", "content": "hi"}],
            "stream": true,
            "stream_options": {"include_usage": true}
        }));
        assert!(translated.stream);
        assert!(translated.include_usage);
        assert_eq!(translated.body["stream"], true);
    }

    // ---------- capability matrix ----------

    #[tokio::test]
    async fn unsupported_non_default_parameters_are_400() {
        for (field, value) in [
            ("tools", serde_json::json!([{"type": "function", "function": {"name": "f"}}])),
            ("tool_choice", serde_json::json!("required")),
            ("response_format", serde_json::json!({"type": "json_object"})),
            ("reasoning_effort", serde_json::json!("low")),
            ("store", serde_json::json!(true)),
            ("metadata", serde_json::json!({"k": "v"})),
            ("n", serde_json::json!(2)),
            ("presence_penalty", serde_json::json!(0.5)),
            ("frequency_penalty", serde_json::json!(-1.0)),
            ("logit_bias", serde_json::json!({"42": 1})),
            ("logprobs", serde_json::json!(true)),
            ("seed", serde_json::json!(7)),
            ("parallel_tool_calls", serde_json::json!(false)),
            ("service_tier", serde_json::json!("flex")),
            ("modalities", serde_json::json!(["text", "audio"])),
            ("audio", serde_json::json!({"voice": "alloy"})),
            ("web_search_options", serde_json::json!({})),
            ("stream_options", serde_json::json!({"future_option": true})),
        ] {
            let (status, json) = expect_err(serde_json::json!({
                "model": "claude-opus-4-8",
                "messages": [{"role": "user", "content": "hi"}],
                field: value,
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{field}");
            assert_eq!(json["error"]["code"], "unsupported_parameter", "{field}");
            assert_eq!(json["error"]["param"], field, "{field}");
            assert_eq!(json["error"]["type"], "invalid_request_error", "{field}");
        }
    }

    #[test]
    fn default_valued_matrix_parameters_are_accepted() {
        // Stock SDK шлют дефолты пачками — они не должны ломать запрос.
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [],
            "tool_choice": "auto",
            "n": 1,
            "presence_penalty": 0,
            "frequency_penalty": 0,
            "logit_bias": {},
            "logprobs": false,
            "seed": null,
            "store": false,
            "metadata": null,
            "parallel_tool_calls": true,
            "service_tier": "auto",
            "response_format": {"type": "text"},
            "reasoning_effort": null,
            "modalities": ["text"]
        }));
        // Дефолтные matrix-ключи в Messages тело не проксируются.
        for key in ["tools", "tool_choice", "n", "store", "service_tier", "response_format"] {
            assert!(translated.body.get(key).is_none(), "{key}");
        }
    }

    #[tokio::test]
    async fn message_level_tool_and_media_fields_are_400() {
        for message in [
            serde_json::json!({"role": "tool", "tool_call_id": "call_1", "content": "x"}),
            serde_json::json!({"role": "assistant", "content": null, "tool_calls": [{"id": "c"}]}),
            serde_json::json!({"role": "user", "content": [{"type": "image_url", "image_url": {"url": "https://x"}}]}),
            serde_json::json!({"role": "user", "content": "hi", "name": "alice"}),
        ] {
            let (status, json) = expect_err(serde_json::json!({
                "model": "claude-opus-4-8",
                "messages": [{"role": "user", "content": "hi"}, message],
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{message}");
            assert_eq!(json["error"]["code"], "unsupported_parameter", "{message}");
        }
    }

    #[tokio::test]
    async fn structural_errors_are_openai_shaped_400() {
        // Нет model.
        let (status, json) = expect_err(serde_json::json!({
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["param"], "model");
        assert!(json["error"]["code"].is_null());

        // Пустой native id после strip'а префикса.
        let (status, json) = expect_err(serde_json::json!({
            "model": "anthropic/",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["param"], "model");

        // Нет/пустые messages.
        let (status, json) = expect_err(serde_json::json!({"model": "claude-opus-4-8"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["param"], "messages");
        let (status, _) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8",
            "messages": []
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Только system — Messages требует хотя бы одно user/assistant.
        let (status, _) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8",
            "messages": [{"role": "system", "content": "x"}]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Пустой user-контент.
        let (status, _) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8",
            "messages": [{"role": "user", "content": ""}]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Невалидный stop.
        let (status, json) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8",
            "messages": [{"role": "user", "content": "hi"}],
            "stop": ["a", "b", "c", "d", "e"]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["param"], "stop");
    }

    // ---------- перевод ответа ----------

    #[test]
    fn finish_reason_mapping() {
        assert_eq!(map_finish_reason(Some("end_turn")), "stop");
        assert_eq!(map_finish_reason(Some("stop_sequence")), "stop");
        assert_eq!(map_finish_reason(Some("max_tokens")), "length");
        assert_eq!(map_finish_reason(Some("model_context_window_exceeded")), "length");
        assert_eq!(map_finish_reason(Some("tool_use")), "tool_calls");
        assert_eq!(map_finish_reason(None), "stop");
    }

    #[test]
    fn usage_mapping_includes_cache_tokens() {
        let usage = map_usage(&serde_json::json!({
            "input_tokens": 100,
            "cache_creation_input_tokens": 20,
            "cache_read_input_tokens": 30,
            "output_tokens": 7
        }));
        assert_eq!(usage["prompt_tokens"], 150);
        assert_eq!(usage["completion_tokens"], 7);
        assert_eq!(usage["total_tokens"], 157);
        assert_eq!(usage["prompt_tokens_details"]["cached_tokens"], 30);

        let usage = map_usage(&serde_json::json!({"input_tokens": 5, "output_tokens": 2}));
        assert_eq!(usage["prompt_tokens"], 5);
        assert!(usage.get("prompt_tokens_details").is_none());
    }

    #[tokio::test]
    async fn error_response_converts_to_openai_envelope() {
        let make = |status: StatusCode, body: &str| {
            let mut response = Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap();
            response
                .extensions_mut()
                .insert(TerminalErrorReason("billing_limit"));
            response
        };

        // Anthropic-shaped 402 LowBalance: статус и тип сохраняются, code уносит
        // исходный anthropic type, reason extension доезжает до audit.
        let upstream = make(
            StatusCode::PAYMENT_REQUIRED,
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"insufficient balance"}}"#,
        );
        let response = convert_error_response(upstream).await;
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        assert_eq!(
            response.extensions().get::<TerminalErrorReason>().map(|r| r.0),
            Some("billing_limit")
        );
        let (_, json) = err_parts(response).await;
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert_eq!(json["error"]["message"], "insufficient balance");
        assert_eq!(json["error"]["code"], "invalid_request_error");

        // 401 → authentication_error; 529 → server_error; не-JSON тело →
        // обезличенное сообщение.
        let upstream = make(
            StatusCode::UNAUTHORIZED,
            r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
        );
        let (_, json) = err_parts(convert_error_response(upstream).await).await;
        assert_eq!(json["error"]["type"], "authentication_error");
        assert_eq!(json["error"]["code"], "authentication_error");

        let upstream = make(
            StatusCode::from_u16(529).unwrap(),
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        );
        let (status, json) = err_parts(convert_error_response(upstream).await).await;
        assert_eq!(status.as_u16(), 529);
        assert_eq!(json["error"]["type"], "server_error");
        assert_eq!(json["error"]["code"], "overloaded_error");

        let upstream = make(StatusCode::BAD_GATEWAY, "<html>bad gateway</html>");
        let (_, json) = err_parts(convert_error_response(upstream).await).await;
        assert_eq!(json["error"]["type"], "server_error");
        assert_eq!(json["error"]["message"], "The provider returned an error.");
        assert!(json["error"]["code"].is_null());
    }

    #[tokio::test]
    async fn error_response_preserves_retry_after() {
        let upstream = Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header(header::RETRY_AFTER, "17")
            .body(Body::from(
                r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#,
            ))
            .unwrap();
        let response = convert_error_response(upstream).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get(header::RETRY_AFTER).unwrap(), "17");
        let (_, json) = err_parts(response).await;
        assert_eq!(json["error"]["type"], "rate_limit_error");
    }

    #[tokio::test]
    async fn non_stream_response_maps_to_chat_completion() {
        let upstream = Response::builder()
            .status(StatusCode::OK)
            .header("request-id", "req_abc")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "id": "msg_1",
                    "type": "message",
                    "role": "assistant",
                    "model": "claude-opus-4-8-20260101",
                    "content": [
                        {"type": "text", "text": "Hello, "},
                        {"type": "text", "text": "world"}
                    ],
                    "stop_reason": "end_turn",
                    "usage": {"input_tokens": 12, "output_tokens": 5}
                })
                .to_string(),
            ))
            .unwrap();
        let response = json_chat_response(upstream).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("request-id").unwrap(), "req_abc");
        let (_, json) = err_parts(response).await;
        assert_eq!(json["object"], "chat.completion");
        assert!(json["id"].as_str().unwrap().starts_with("chatcmpl_"));
        assert_eq!(json["model"], "claude-opus-4-8-20260101");
        assert_eq!(json["choices"][0]["message"]["role"], "assistant");
        assert_eq!(json["choices"][0]["message"]["content"], "Hello, world");
        assert_eq!(json["choices"][0]["finish_reason"], "stop");
        assert_eq!(json["usage"]["prompt_tokens"], 12);
        assert_eq!(json["usage"]["completion_tokens"], 5);
        assert_eq!(json["usage"]["total_tokens"], 17);
    }

    // ---------- SSE-транслятор ----------

    const SSE_DIALOG: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","model":"claude-opus-4-8-20260101","usage":{"input_tokens":10,"cache_read_input_tokens":4,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":", world"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":6}}

event: message_stop
data: {"type":"message_stop"}"#;

    #[tokio::test]
    async fn sse_dialog_translates_to_chat_chunks() {
        let translator = ChatSseTranslator::new(sse_bytes(SSE_DIALOG), "claude-opus-4-8".into(), false);
        let output = collect_stream(translator).await;
        assert!(output.ends_with("data: [DONE]\n\n"), "{output}");
        let frames = data_frames(&output);
        // role-чанк, 2 контентных, финиш-чанк; usage-чанка нет (include_usage=false).
        assert_eq!(frames.len(), 4, "{output}");
        assert_eq!(frames[0]["choices"][0]["delta"], serde_json::json!({"role": "assistant", "content": ""}));
        assert_eq!(frames[1]["choices"][0]["delta"], serde_json::json!({"content": "Hello"}));
        assert_eq!(frames[2]["choices"][0]["delta"], serde_json::json!({"content": ", world"}));
        assert_eq!(frames[3]["choices"][0]["finish_reason"], "stop");
        // id/created/model стабильны по всему стриму; model — сервёная из message_start.
        for frame in &frames {
            assert!(frame["id"].as_str().unwrap().starts_with("chatcmpl_"));
            assert_eq!(frame["id"], frames[0]["id"]);
            assert_eq!(frame["object"], "chat.completion.chunk");
            assert_eq!(frame["model"], "claude-opus-4-8-20260101");
        }
    }

    #[tokio::test]
    async fn sse_usage_chunk_honors_include_usage() {
        let translator = ChatSseTranslator::new(sse_bytes(SSE_DIALOG), "claude-opus-4-8".into(), true);
        let output = collect_stream(translator).await;
        let frames = data_frames(&output);
        assert_eq!(frames.len(), 5, "{output}");
        let usage = &frames[4];
        assert_eq!(usage["choices"], serde_json::json!([]));
        // prompt = input 10 + cache_read 4; completion — финальный output 6.
        assert_eq!(usage["usage"]["prompt_tokens"], 14);
        assert_eq!(usage["usage"]["completion_tokens"], 6);
        assert_eq!(usage["usage"]["total_tokens"], 20);
        assert_eq!(usage["usage"]["prompt_tokens_details"]["cached_tokens"], 4);
    }

    #[tokio::test]
    async fn sse_ping_becomes_heartbeat_chunk() {
        let events = "event: ping\ndata: {\"type\":\"ping\"}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}";
        let translator = ChatSseTranslator::new(sse_bytes(events), "m".into(), false);
        let output = collect_stream(translator).await;
        let frames = data_frames(&output);
        assert_eq!(frames.len(), 1, "{output}");
        assert_eq!(frames[0]["choices"][0]["delta"], serde_json::json!({}));
        assert!(frames[0]["choices"][0]["finish_reason"].is_null());
        assert!(output.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn sse_error_event_becomes_openai_error_frame_without_done() {
        let events = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"model\":\"x\",\"usage\":{\"input_tokens\":1}}}\n\n",
            "event: error\n",
            "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}"
        );
        let translator = ChatSseTranslator::new(sse_bytes(events), "m".into(), false);
        let output = collect_stream(translator).await;
        assert!(!output.contains("[DONE]"), "{output}");
        let frames = data_frames(&output);
        assert_eq!(frames.len(), 2, "{output}");
        assert_eq!(frames[1]["error"]["type"], "server_error");
        assert_eq!(frames[1]["error"]["code"], "overloaded_error");
        assert_eq!(frames[1]["error"]["message"], "Overloaded");
    }

    #[tokio::test]
    async fn sse_clean_eof_without_message_stop_still_terminates_with_done() {
        let events = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}";
        let translator = ChatSseTranslator::new(sse_bytes(events), "m".into(), false);
        let output = collect_stream(translator).await;
        assert!(output.contains("partial"), "{output}");
        assert!(output.ends_with("data: [DONE]\n\n"), "{output}");
    }

    #[tokio::test]
    async fn sse_transport_error_is_not_silent() {
        let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
            Ok(Bytes::from("event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"a\"}}\n\n")),
            Err(std::io::Error::other("boom")),
        ];
        let translator = ChatSseTranslator::new(Box::pin(futures_util::stream::iter(chunks)), "m".into(), false);
        let output = collect_stream(translator).await;
        assert!(output.contains("\"error\""), "{output}");
        assert!(!output.contains("[DONE]"), "{output}");
    }

    #[tokio::test]
    async fn sse_split_frames_across_chunks_are_reassembled() {
        // Кадр, разрезанный посреди data-строки двумя сетевыми чанками.
        let full = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"split ok\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
        let (head, tail) = full.split_at(37);
        let chunks: Vec<Result<Bytes, std::io::Error>> =
            vec![Ok(Bytes::from(head)), Ok(Bytes::from(tail))];
        let translator = ChatSseTranslator::new(Box::pin(futures_util::stream::iter(chunks)), "m".into(), false);
        let output = collect_stream(translator).await;
        assert!(output.contains("split ok"), "{output}");
        assert!(output.ends_with("data: [DONE]\n\n"), "{output}");
    }
}
