//! Universal Chat Completions → Gemini generateContent адаптер — этапы
//! 3.3–3.4b docs/engine/UNIFIED_ROUTER.md (решения 1–4).
//!
//! `POST /v1/chat/completions` на Gemini-плоскости. Поток запроса: парс
//! chat-запроса → перевод в GenerateContentRequest JSON → внутренний `Request`
//! на `/v1beta/models/{model}:generateContent|streamGenerateContent` → общий
//! [`gemini_api`] (admission, reserve, affinity, ротация, Code Assist wrapper,
//! usage-settlement — без единого изменения) → перевод ответа:
//! GenerateContentResponse SSE → `chat.completion.chunk` либо JSON →
//! `chat.completion`.
//!
//! Capability matrix (решение 3): параметры, которые generateContent не умеет
//! или чьё поведение на private wire не подтверждено (n>1, penalties, logprobs,
//! seed, store, parallel_tool_calls, …), с
//! не-дефолтным значением отклоняются `400 unsupported_parameter`.
//! Мультимодальность и structured output (3.4a): image_url-части user-сообщений
//! с data: URL → inlineData-партам (http(s) ссылки generateContent не принимает —
//! честный 400), `response_format` json_object/json_schema →
//! `generationConfig.responseMimeType`/`responseSchema`.
//! Reasoning (3.4b): `reasoning_effort` minimal|low|medium|high →
//! `generationConfig.thinkingConfig` (`thinkingLevel` проксируется как есть —
//! плоскость сама мапит уровень в wire model id; `includeThoughts: true`;
//! невалидное значение → 400 invalid_request); thought-парты ответа →
//! OpenAI-расширение `reasoning_content` (`thoughtSignature` не выставляется —
//! решение 4). Tool/structured-output schemas рекурсивно приводятся к
//! поддерживаемому Code Assist subset (снимаются `$schema` и числовые
//! `exclusiveMinimum`/`exclusiveMaximum`), а replayed functionCall получает
//! принятый Google context-engineering marker вместо opaque provider signature.
//! В отличие от Anthropic-плоскости, НЕИЗВЕСТНЫЕ top-level поля тоже
//! отклоняются: Code Assist wrapper (`wrap_code_assist_request`) пропускает
//! только закрытый список полей GenerateContentRequest, поэтому «проксирование»
//! было бы молчаливым выбрасыванием — а это хуже честного 400.
//!
//! Все ответы этого пути — OpenAI-совместимый конверт, включая ошибки:
//! Google-конверт `{"error":{"code","message","status"}}` (синтетические
//! ошибки плоскости и санитизированные ошибки апстрима) переводится в
//! `{"error":{"message","type","param","code"}}` с сохранением HTTP-статуса и
//! `Retry-After`. Статус 402 LowBalance сохраняется (контракт docs-portal).
//! Особый случай: нативная плоскость отвечает на невалидный ключ `400
//! API_KEY_INVALID` (как настоящий generativelanguage) — для OpenAI-клиентов
//! это мапится в ожидаемый `401 authentication_error`.

use std::collections::{HashMap, VecDeque};
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

use super::gemini_api;
use super::REPLAYED_FUNCTION_CALL_THOUGHT_SIGNATURE;
use crate::codex::new_id;
use crate::gemini_schema;
use crate::gemini_stream::GeminiStreamState;
use crate::proxy::{
    read_body_limited, with_not_started, without_not_started, BodyReadError, TerminalErrorReason,
    EXECUTION_STATE_HEADER, EXECUTION_STATE_NOT_STARTED,
};
use crate::state::AppState;
use crate::validation::{optional_bool, optional_positive_u64};

/// Лимит тела chat-запроса — как у нативного text-пути плоскости
/// (`GEMINI_TEXT_REQUEST_BODY_LIMIT`). Общий с Responses-адаптером этапа 4.3
/// (`responses.rs`).
pub(crate) const CHAT_BODY_LIMIT: usize = 32 * 1024 * 1024;

/// Верхняя граница буферизации error/non-stream тел ответа `gemini_api()`.
pub(crate) const RESPONSE_BODY_LIMIT: usize = 32 * 1024 * 1024;

/// Хендлер `POST /v1/chat/completions` (роут регистрируется в server только в
/// `ProviderMode::Gemini`).
pub async fn gemini_chat_completions(
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

    // Внутренний запрос на нативную поверхность: admission, reserve, affinity,
    // ротация, Code Assist wrapper и settlement выполняет общий gemini_api().
    // Заголовки клиента сохраняются (authorize читает ключи из них), меняется
    // только content-length/content-type под синтезированное тело.
    let mut headers = parts.headers.clone();
    headers.remove(header::CONTENT_LENGTH);
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
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
    let suffix = if translated.stream {
        "streamGenerateContent?alt=sse"
    } else {
        "generateContent"
    };
    let mut inner = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1beta/models/{}:{suffix}", translated.model))
        .body(Body::from(body_bytes))
        .expect("static request builder is infallible");
    *inner.headers_mut() = headers;
    let upstream = gemini_api(State(app), ConnectInfo(peer), inner).await;

    if upstream.status() != StatusCode::OK {
        return convert_error_response(upstream).await;
    }
    if translated.stream {
        stream_chat_response(upstream, translated.model, translated.include_usage)
    } else {
        json_chat_response(upstream, translated.model).await
    }
}

/// Результат перевода chat-запроса: тело GenerateContentRequest и параметры,
/// нужные для перевода ответа.
#[derive(Debug)]
struct Translated {
    body: Value,
    /// Запрошенная модель с уже снятым `google/`-префиксом — фолбэк для поля
    /// `model` ответа, если плоскость не вернула `modelVersion`.
    model: String,
    stream: bool,
    include_usage: bool,
}

/// Перевод chat-запроса в GenerateContentRequest JSON. Ошибки — готовые
/// OpenAI-shaped ответы (400).
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
    // Namespaced ID резолвится здесь, а не в router: после strip'а admission
    // плоскости видит нативный публичный id (закрытый allowlist config.models).
    let model = model.strip_prefix("google/").unwrap_or(&model).to_string();
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
    let (system_instruction, contents) = translate_messages(messages)?;

    let stream = optional_bool(&object, "stream")
        .map_err(|field| invalid_request("stream must be a boolean.", Some(field)))?
        .unwrap_or(false);
    let include_usage = parse_stream_options(object.get("stream_options"))?;

    let max_tokens = optional_positive_u64(&object, &["max_completion_tokens", "max_tokens"])
        .map_err(|field| {
            invalid_request(&format!("{field} must be a positive integer."), Some(field))
        })?;

    let mut generation_config = Map::new();
    if let Some(max_tokens) = max_tokens {
        generation_config.insert("maxOutputTokens".to_string(), Value::from(max_tokens));
    }
    // Honored-параметры generationConfig.
    for (chat_key, native_key) in [
        ("temperature", "temperature"),
        ("top_p", "topP"),
        ("top_k", "topK"),
    ] {
        if let Some(value) = object.get(chat_key).filter(|v| !v.is_null()) {
            generation_config.insert(native_key.to_string(), value.clone());
        }
    }
    if let Some(stop) = translate_stop(object.get("stop"))? {
        generation_config.insert("stopSequences".to_string(), stop);
    }
    // Structured output (этап 3.4a): response_format → responseMimeType
    // (+responseSchema для json_schema).
    if let Some((mime, schema)) = translate_response_format(object.get("response_format"))? {
        generation_config.insert("responseMimeType".to_string(), Value::String(mime));
        if let Some(schema) = schema {
            generation_config.insert("responseSchema".to_string(), schema);
        }
    }
    // Reasoning (этап 3.4b): reasoning_effort → thinkingConfig — соседнее
    // поле того же generationConfig, responseMimeType/responseSchema не
    // затираются.
    if let Some(level) =
        translate_reasoning_effort(object.get("reasoning_effort"), "reasoning_effort")?
    {
        generation_config.insert(
            "thinkingConfig".to_string(),
            json!({"thinkingLevel": level, "includeThoughts": true}),
        );
    }

    let mut body = Map::new();
    body.insert("contents".to_string(), Value::Array(contents));
    if let Some(system) = system_instruction {
        body.insert("systemInstruction".to_string(), system);
    }
    body.insert(
        "generationConfig".to_string(),
        Value::Object(generation_config),
    );
    if let Some(tools) = object.get("tools").filter(|v| !v.is_null()) {
        let declarations = translate_chat_tools(tools, "tools")?;
        if !declarations.is_empty() {
            body.insert(
                "tools".to_string(),
                json!([{"functionDeclarations": declarations}]),
            );
        }
    } else if let Some(functions) = object.get("functions").filter(|v| !v.is_null()) {
        let declarations = translate_chat_tools(functions, "functions")?;
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
    // Закрытый список (отличие от Anthropic-плоскости): неизвестные top-level
    // поля Code Assist wrapper выбросит молча — честный 400 вместо этого.
    // Известные (honored или matrix-проверенные выше) остаются в object —
    // это норма.
    if let Some(unknown) = object.keys().find(|k| !KNOWN_KEYS.contains(&k.as_str())) {
        return Err(unsupported_parameter(unknown));
    }

    Ok(Translated {
        body: Value::Object(body),
        model,
        stream,
        include_usage,
    })
}

/// Известные chat-параметры: honored (переводятся) или matrix (отклоняются
/// при не-дефолте). Всё вне списка — `400 unsupported_parameter` (закрытый
/// список, см. translate_chat_request).
const KNOWN_KEYS: &[&str] = &[
    "model",
    "messages",
    "max_tokens",
    "max_completion_tokens",
    "stream",
    "stream_options",
    "stop",
    "temperature",
    "top_p",
    "top_k",
    "reasoning_effort",
    "tools",
    "functions",
    "tool_choice",
    "function_call",
    // Capability matrix: отклонены при не-дефолте, при дефолте — сняты.
    "parallel_tool_calls",
    "n",
    "presence_penalty",
    "frequency_penalty",
    "logit_bias",
    "logprobs",
    "top_logprobs",
    "seed",
    "store",
    "metadata",
    "service_tier",
    "modalities",
    "audio",
    "prediction",
    "web_search_options",
    "response_format",
    "user",
];

/// Capability matrix (решение 3): параметр, который generateContent не умеет
/// (или чьё поведение на private wire не подтверждено), с не-дефолтным
/// значением → `400 unsupported_parameter`. Порядок правил определяет, какой
/// параметр назовёт ошибка. reasoning_effort с этапа 3.4b переводится в
/// thinkingConfig (translate_reasoning_effort), а не отклоняется.
fn check_capability_matrix(object: &Map<String, Value>) -> Result<(), Response> {
    let rules: [(&str, fn(&Value) -> bool); 18] = [
        ("parallel_tool_calls", |v| v.as_bool() == Some(true)),
        ("response_format", |v| {
            // text — дефолт; json_object/json_schema переводятся в
            // generationConfig responseMimeType/responseSchema (этап 3.4a).
            v.is_null()
                || matches!(
                    v.get("type").and_then(Value::as_str),
                    Some("text") | Some("json_object") | Some("json_schema")
                )
        }),
        ("store", |v| v.is_null() || v.as_bool() == Some(false)),
        ("metadata", |v| v.is_null()),
        ("n", |v| v.as_u64() == Some(1)),
        ("presence_penalty", |v| v.as_f64() == Some(0.0)),
        ("frequency_penalty", |v| v.as_f64() == Some(0.0)),
        ("logit_bias", |v| {
            v.is_null() || v.as_object().is_some_and(Map::is_empty)
        }),
        ("logprobs", |v| v.is_null() || v.as_bool() == Some(false)),
        ("top_logprobs", |v| v.is_null()),
        ("seed", |v| v.is_null()),
        ("service_tier", |v| {
            v.is_null() || v.as_str() == Some("auto") || v.as_str() == Some("default")
        }),
        ("modalities", |v| {
            v.as_array()
                .is_some_and(|m| m.len() == 1 && m[0].as_str() == Some("text"))
        }),
        ("audio", |v| v.is_null()),
        ("prediction", |v| v.is_null()),
        ("web_search_options", |v| v.is_null()),
        ("user", |v| v.is_null()),
        ("stream_options", |v| {
            v.as_object()
                .is_some_and(|o| o.keys().all(|k| k == "include_usage"))
        }),
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

/// Перевод массива chat-сообщений в `(systemInstruction, contents)`.
/// system/developer → `systemInstruction` (text-парт на сообщение).
/// role tool/function → user-content с functionResponse-партами. Подряд
/// идущие сообщения с одинаковой Gemini-ролью склеиваются (parts
/// конкатенируются): generateContent ждёт чередования user/model, а серии
/// tool-ответов — это один user-content со всеми functionResponse.
fn translate_messages(messages: Vec<Value>) -> Result<(Option<Value>, Vec<Value>), Response> {
    let mut system_parts = Vec::new();
    let mut contents: Vec<Value> = Vec::new();
    // id tool_call → имя функции: Gemini functionResponse ссылается по имени,
    // а chat tool-сообщение несёт только tool_call_id. Карта строится по
    // assistant-сообщениям этой же истории за один проход.
    let mut call_names: HashMap<String, String> = HashMap::new();
    for message in messages {
        let object = message.as_object().ok_or_else(|| {
            invalid_request("Each message must be a JSON object.", Some("messages"))
        })?;
        let role = object.get("role").and_then(Value::as_str).ok_or_else(|| {
            invalid_request("Each message must have a string role.", Some("messages"))
        })?;
        // `name` существует только у legacy function-роли (имя функции);
        // participant-name остальных ролей в generateContent не существует.
        if role != "function" && object.get("name").is_some_and(|v| !v.is_null()) {
            return Err(unsupported_parameter("name"));
        }
        match role {
            "system" | "developer" => {
                let text = message_text(object.get("content"))?;
                if !text.is_empty() {
                    system_parts.push(json!({"text": text}));
                }
            }
            "user" => {
                let parts = user_message_parts(object.get("content"))?;
                merge_or_push(&mut contents, "user", parts);
            }
            "assistant" => {
                let parts = assistant_parts(object, &mut call_names)?;
                if !parts.is_empty() {
                    merge_or_push(&mut contents, "model", parts);
                }
            }
            "tool" => {
                let part = function_response_part(object, &call_names)?;
                merge_or_push(&mut contents, "user", vec![part]);
            }
            "function" => {
                let part = legacy_function_response_part(object)?;
                merge_or_push(&mut contents, "user", vec![part]);
            }
            _ => {
                return Err(invalid_request(
                    "Invalid message role: expected system, developer, user, assistant, tool or function.",
                    Some("messages"),
                ))
            }
        }
    }
    if contents.is_empty() {
        return Err(invalid_request(
            "messages must contain at least one user or assistant message.",
            Some("messages"),
        ));
    }
    let system_instruction = (!system_parts.is_empty()).then(|| json!({"parts": system_parts}));
    Ok((system_instruction, contents))
}

/// Добавить parts в contents, склеивая с последним content той же роли.
/// Общая с Responses-адаптером этапа 4.3 (`responses.rs`).
pub(crate) fn merge_or_push(contents: &mut Vec<Value>, role: &str, parts: Vec<Value>) {
    if let Some(last) = contents.last_mut() {
        if last.get("role").and_then(Value::as_str) == Some(role) {
            let merged = last
                .get_mut("parts")
                .and_then(Value::as_array_mut)
                .expect("translated content parts is an array");
            merged.extend(parts);
            return;
        }
    }
    contents.push(json!({"role": role, "parts": parts}));
}

/// Текст сообщения: строка либо массив text-частей (склеиваются через \n).
/// Нетекстовые части (images и прочий этап 3.4) — 400.
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

/// Контент user-сообщения → parts: text + inlineData image (этап 3.4a).
/// Текстовые части склеиваются в один text-парт (через \n); image разрывают
/// склейку, порядок сохраняется. Поддерживаются только data: URL —
/// generateContent не принимает внешние http(s) ссылки (fileData требует
/// File API upload), поэтому http(s) image — честный 400, а не молчаливое
/// выбрасывание.
fn user_message_parts(content: Option<&Value>) -> Result<Vec<Value>, Response> {
    let Some(Value::Array(parts)) = content else {
        let text = message_text(content)?;
        if text.is_empty() {
            return Err(invalid_request(
                "User message content must not be empty.",
                Some("messages"),
            ));
        }
        return Ok(vec![json!({"text": text})]);
    };
    let mut out: Vec<Value> = Vec::new();
    let mut text = String::new();
    for part in parts {
        match part.get("type").and_then(Value::as_str) {
            Some("text") => {
                let segment = part.get("text").and_then(Value::as_str).unwrap_or_default();
                if !text.is_empty() && !segment.is_empty() {
                    text.push('\n');
                }
                text.push_str(segment);
            }
            Some("image_url") => {
                if !text.is_empty() {
                    out.push(json!({"text": std::mem::take(&mut text)}));
                }
                out.push(gemini_image_part(part, "messages")?);
            }
            _ => return Err(unsupported_parameter("messages")),
        }
    }
    if !text.is_empty() {
        out.push(json!({"text": text}));
    }
    if out.is_empty() {
        return Err(invalid_request(
            "User message content must not be empty.",
            Some("messages"),
        ));
    }
    Ok(out)
}

/// Chat image_url-часть → inlineData-парт. Только data: URL (см.
/// user_message_parts); `detail` != auto — `400 unsupported_parameter`.
/// Общая с Responses-адаптером этапа 4.3 (`responses.rs`); `param` — имя
/// параметра в ошибках (`messages` у chat, `input` у Responses).
pub(crate) fn gemini_image_part(part: &Value, param: &str) -> Result<Value, Response> {
    let image = part
        .get("image_url")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            invalid_request(
                "Invalid image_url part: expected an object with a url string.",
                Some(param),
            )
        })?;
    if let Some(detail) = image.get("detail").and_then(Value::as_str) {
        if detail != "auto" {
            return Err(unsupported_parameter(param));
        }
    }
    let url = image.get("url").and_then(Value::as_str).ok_or_else(|| {
        invalid_request(
            "Invalid image_url part: expected an object with a url string.",
            Some(param),
        )
    })?;
    let Some(data_url) = url.strip_prefix("data:") else {
        return Err(invalid_request(
            "Gemini lane supports only data: image URLs (external image fetch is not supported).",
            Some(param),
        ));
    };
    let (mime_type, data) = data_url.split_once(";base64,").ok_or_else(|| {
        invalid_request(
            "Invalid image_url data URL: expected data:<mime>;base64,<data>.",
            Some(param),
        )
    })?;
    if !mime_type.starts_with("image/") || data.is_empty() {
        return Err(invalid_request(
            "Invalid image_url data URL: expected an image MIME type and base64 data.",
            Some(param),
        ));
    }
    Ok(json!({"inlineData": {"mimeType": mime_type, "data": data}}))
}

/// `response_format` → `(responseMimeType, Option<responseSchema>)` (этап
/// 3.4a). json_object — JSON без схемы; json_schema требует schema-объект.
/// Обёрточные name/strict/description у generateContent отсутствуют —
/// проксируется сама схема.
fn translate_response_format(
    value: Option<&Value>,
) -> Result<Option<(String, Option<Value>)>, Response> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    match value.get("type").and_then(Value::as_str) {
        Some("json_object") => Ok(Some(("application/json".to_string(), None))),
        Some("json_schema") => {
            let schema = value
                .get("json_schema")
                .and_then(|j| j.get("schema"))
                .filter(|s| s.is_object())
                .ok_or_else(|| {
                    invalid_request(
                        "Invalid response_format: json_schema requires a schema object.",
                        Some("response_format"),
                    )
                })?;
            Ok(Some((
                "application/json".to_string(),
                Some(code_assist_schema(
                    schema,
                    "response_format.json_schema.schema",
                )?),
            )))
        }
        // text — дефолт без перевода; остальные типы отклонены matrix.
        _ => Ok(None),
    }
}

/// `reasoning_effort` → `thinkingConfig.thinkingLevel` (этап 3.4b). Уровень
/// проксируется как есть: плоскость сама мапит minimal|low|medium|high в wire
/// model id (валидация строк уровня — в gemini/api.rs). Отсутствие/null —
/// выкл (поле не вставляется); любое другое не-null значение → 400
/// invalid_request. Общая с Responses-адаптером этапа 4.3 (`responses.rs`);
/// `param` — имя параметра в ошибке (`reasoning_effort` у chat, `reasoning`
/// у Responses).
pub(crate) fn translate_reasoning_effort(
    value: Option<&Value>,
    param: &str,
) -> Result<Option<String>, Response> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    match value.as_str() {
        Some(level @ ("minimal" | "low" | "medium" | "high")) => Ok(Some(level.to_string())),
        _ => Err(invalid_request(
            &format!(
                "Invalid value for parameter: {param} (expected minimal, low, medium or high)."
            ),
            Some(param),
        )),
    }
}

/// Assistant-сообщение → parts: text + functionCall (chat `tool_calls[]` и
/// legacy `function_call`). Попутно регистрирует id→name для tool-ответов.
/// Непустой `reasoning_content` без видимого text/tool call — валидный replay
/// ответа этого же адаптера, но без provider signature его нельзя превращать
/// обратно в thought-парт; такой display-only model turn опускается.
fn assistant_parts(
    object: &Map<String, Value>,
    call_names: &mut HashMap<String, String>,
) -> Result<Vec<Value>, Response> {
    let mut parts = Vec::new();
    let text = message_text(object.get("content"))?;
    if !text.is_empty() {
        parts.push(json!({"text": text}));
    }
    if let Some(tool_calls) = object.get("tool_calls").filter(|v| !v.is_null()) {
        let tool_calls = tool_calls.as_array().ok_or_else(|| {
            invalid_request(
                "Invalid type for parameter: tool_calls must be an array.",
                Some("messages"),
            )
        })?;
        for call in tool_calls {
            match call.get("type").and_then(Value::as_str) {
                Some("function") | None => {}
                Some(_) => {
                    return Err(invalid_request(
                        "Invalid tool_calls entry: only function tool calls are supported.",
                        Some("messages"),
                    ))
                }
            }
            let id = call.get("id").and_then(Value::as_str).ok_or_else(|| {
                invalid_request("Invalid tool_calls entry: missing id.", Some("messages"))
            })?;
            let function = call.get("function").ok_or_else(|| {
                invalid_request(
                    "Invalid tool_calls entry: missing function.",
                    Some("messages"),
                )
            })?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    invalid_request(
                        "Invalid tool_calls entry: missing function name.",
                        Some("messages"),
                    )
                })?;
            let args = parse_tool_arguments(function.get("arguments"), "messages")?;
            call_names.insert(id.to_string(), name.to_string());
            parts.push(replayed_function_call_part(name, args));
        }
    }
    if let Some(function_call) = object.get("function_call").filter(|v| !v.is_null()) {
        let name = function_call
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                invalid_request(
                    "Invalid function_call: expected an object with a name.",
                    Some("messages"),
                )
            })?;
        let args = parse_tool_arguments(function_call.get("arguments"), "messages")?;
        // Legacy function-ответы ссылаются по имени напрямую — карта не нужна.
        parts.push(replayed_function_call_part(name, args));
    }
    if parts.is_empty() {
        if object
            .get("reasoning_content")
            .and_then(Value::as_str)
            .is_some_and(|reasoning| !reasoning.is_empty())
        {
            return Ok(parts);
        }
        return Err(invalid_request(
            "Assistant message must have content or tool calls.",
            Some("messages"),
        ));
    }
    Ok(parts)
}

/// `arguments` tool call'а: JSON-строка → object. Пустая строка — `{}`.
/// Общая с Responses-адаптером этапа 4.3 (`responses.rs`); `param` — имя
/// параметра в ошибках (`messages` у chat, `input` у Responses).
pub(crate) fn parse_tool_arguments(value: Option<&Value>, param: &str) -> Result<Value, Response> {
    let raw = match value {
        None | Some(Value::Null) => return Ok(json!({})),
        Some(Value::String(raw)) => raw,
        Some(_) => {
            return Err(invalid_request(
                "Invalid tool call arguments: expected a JSON string.",
                Some(param),
            ))
        }
    };
    if raw.is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str(raw) {
        Ok(Value::Object(arguments)) => Ok(Value::Object(arguments)),
        _ => Err(invalid_request(
            "Invalid tool call arguments: expected a JSON object string.",
            Some(param),
        )),
    }
}

/// functionResponse.response — всегда JSON-object. Типичный tool output —
/// JSON-строка: разбираем и заворачиваем в `{"result": ...}`; не-JSON текст
/// заворачивается строкой. Общая с Responses-адаптером этапа 4.3.
pub(crate) fn function_response_value(text: &str) -> Value {
    match serde_json::from_str::<Value>(text) {
        Ok(parsed) => json!({"result": parsed}),
        Err(_) => json!({"result": text}),
    }
}

/// role "tool" → functionResponse-парт; имя восстанавливается по tool_call_id
/// из карты, построенной assistant-сообщениями этой же истории.
fn function_response_part(
    object: &Map<String, Value>,
    call_names: &HashMap<String, String>,
) -> Result<Value, Response> {
    let id = object
        .get("tool_call_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            invalid_request("Tool message requires a tool_call_id.", Some("messages"))
        })?;
    let name = call_names.get(id).ok_or_else(|| {
        invalid_request(
            "Tool message tool_call_id has no matching tool call in this history.",
            Some("messages"),
        )
    })?;
    let text = message_text(object.get("content"))?;
    Ok(json!({
        "functionResponse": {"name": name, "response": function_response_value(&text)}
    }))
}

/// Legacy role "function" → functionResponse; имя — из поля `name`.
fn legacy_function_response_part(object: &Map<String, Value>) -> Result<Value, Response> {
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_request("Function message requires a name.", Some("messages")))?;
    let text = message_text(object.get("content"))?;
    Ok(json!({
        "functionResponse": {"name": name, "response": function_response_value(&text)}
    }))
}

/// Chat `tools[]` / legacy `functions[]` → массив functionDeclarations.
/// Legacy functions — голые function-объекты без {"type":"function"} обёртки.
/// `parameters` переводятся в поддерживаемый Code Assist subset JSON Schema;
/// отсутствующие — опускаются (поле необязательно).
fn translate_chat_tools(value: &Value, param: &str) -> Result<Vec<Value>, Response> {
    let tools = value.as_array().ok_or_else(|| {
        invalid_request(
            &format!("Invalid type for parameter: {param} must be an array."),
            Some(param),
        )
    })?;
    let mut declarations = Vec::with_capacity(tools.len());
    for (index, tool) in tools.iter().enumerate() {
        let function = if param == "functions" {
            tool.as_object()
        } else {
            match tool.get("type").and_then(Value::as_str) {
                Some("function") => tool.get("function").and_then(Value::as_object),
                _ => None,
            }
        };
        let function = function.ok_or_else(|| {
            invalid_request(
                &format!("Invalid entry in parameter: {param} must contain function objects."),
                Some(param),
            )
        })?;
        let descriptor_path = if param == "functions" {
            format!("functions.{index}")
        } else {
            format!("tools.{index}.function")
        };
        declarations.push(function_declaration(function, param, &descriptor_path)?);
    }
    Ok(declarations)
}

/// Function-дескриптор (`{name, description?, parameters?}`) →
/// functionDeclaration: `parameters` санитизируются под поддерживаемый Code
/// Assist subset JSON Schema, отсутствующие поля опускаются; `strict` и прочие
/// обёрточные поля снимаются (constrained decoding generateContent всегда
/// строгий). Общее ядро chat `tools[]`/`functions[]` и Responses `tools[]`
/// (этап 4.3, `responses.rs`).
pub(crate) fn function_declaration(
    function: &Map<String, Value>,
    param: &str,
    descriptor_path: &str,
) -> Result<Value, Response> {
    let name = function
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            invalid_request(
                &format!("Invalid entry in parameter: {param} entries require a function name."),
                Some(param),
            )
        })?;
    let mut declaration = json!({"name": name});
    match function.get("description") {
        None | Some(Value::Null) => {}
        Some(Value::String(description)) => {
            declaration["description"] = Value::String(description.clone())
        }
        Some(_) => {
            return Err(invalid_request(
                &format!("Invalid type for parameter: {param} description must be a string."),
                Some(param),
            ))
        }
    }
    match function.get("parameters") {
        None | Some(Value::Null) => {}
        Some(schema) if schema.is_object() => {
            declaration["parameters"] =
                code_assist_schema(schema, &format!("{descriptor_path}.parameters"))?
        }
        Some(_) => {
            return Err(invalid_request(
                &format!("Invalid type for parameter: {param} parameters must be an object."),
                Some(param),
            ))
        }
    }
    Ok(declaration)
}

/// Translate a legal JSON Schema into the exact bounded Google `Schema`
/// vocabulary accepted by Code Assist. Unrepresentable constraints fail
/// locally with an exact JSON Pointer suffix in `error.param`.
pub(crate) fn code_assist_schema(schema: &Value, root_path: &str) -> Result<Value, Response> {
    gemini_schema::translate(schema, root_path)
        .map_err(|error| invalid_request(&error.message(), Some(error.path())))
}

/// Builds a replay-safe Gemini model part without persisting or exposing the
/// provider's opaque thought signature.
pub(crate) fn replayed_function_call_part(name: &str, args: Value) -> Value {
    json!({
        "functionCall": {"name": name, "args": args},
        "thoughtSignature": REPLAYED_FUNCTION_CALL_THOUGHT_SIGNATURE,
    })
}

/// `tool_choice` / legacy `function_call` → `toolConfig.functionCallingConfig`.
/// Дефолт (`auto`) не вставляется — generateContent и так AUTO.
fn translate_tool_choice(object: &Map<String, Value>) -> Result<Option<Value>, Response> {
    let config = match object.get("tool_choice").filter(|v| !v.is_null()) {
        Some(Value::String(mode)) => match mode.as_str() {
            "auto" => None,
            "required" => Some(json!({"mode": "ANY"})),
            "none" => Some(json!({"mode": "NONE"})),
            _ => {
                return Err(invalid_request(
                    "Invalid value for parameter: tool_choice.",
                    Some("tool_choice"),
                ))
            }
        },
        Some(Value::Object(named))
            if named.get("type").and_then(Value::as_str) == Some("function") =>
        {
            let name = named
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    invalid_request(
                        "Invalid value for parameter: tool_choice requires a function name.",
                        Some("tool_choice"),
                    )
                })?;
            Some(json!({"mode": "ANY", "allowedFunctionNames": [name]}))
        }
        Some(_) => {
            return Err(invalid_request(
                "Invalid value for parameter: tool_choice.",
                Some("tool_choice"),
            ))
        }
        None => match object.get("function_call").filter(|v| !v.is_null()) {
            Some(Value::String(mode)) => match mode.as_str() {
                "auto" => None,
                "none" => Some(json!({"mode": "NONE"})),
                _ => {
                    return Err(invalid_request(
                        "Invalid value for parameter: function_call.",
                        Some("function_call"),
                    ))
                }
            },
            Some(Value::Object(named)) => {
                let name = named.get("name").and_then(Value::as_str).ok_or_else(|| {
                    invalid_request(
                        "Invalid value for parameter: function_call requires a name.",
                        Some("function_call"),
                    )
                })?;
                Some(json!({"mode": "ANY", "allowedFunctionNames": [name]}))
            }
            Some(_) => {
                return Err(invalid_request(
                    "Invalid value for parameter: function_call.",
                    Some("function_call"),
                ))
            }
            None => None,
        },
    };
    Ok(config.map(|config| json!({"functionCallingConfig": config})))
}

/// `stop`: строка или массив до 5 непустых строк → `stopSequences`
/// (generateContent исполняет их нативно; лимит 5 — публичный предел API).
fn translate_stop(value: Option<&Value>) -> Result<Option<Value>, Response> {
    let value = match value.filter(|v| !v.is_null()) {
        Some(value) => value,
        None => return Ok(None),
    };
    let sequences: Vec<Value> = match value {
        Value::String(s) if !s.is_empty() => vec![Value::String(s.clone())],
        Value::Array(items)
            if !items.is_empty()
                && items.len() <= 5
                && items
                    .iter()
                    .all(|i| i.as_str().is_some_and(|s| !s.is_empty())) =>
        {
            items.clone()
        }
        _ => {
            return Err(invalid_request(
                "Invalid stop: expected a string or an array of up to 5 non-empty strings.",
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
        Some(Value::Object(object)) => optional_bool(object, "include_usage")
            .map(|value| value.unwrap_or(false))
            .map_err(|_| {
                invalid_request(
                    "stream_options.include_usage must be a boolean.",
                    Some("stream_options.include_usage"),
                )
            }),
        _ => Err(invalid_request(
            "Invalid stream_options: expected an object.",
            Some("stream_options"),
        )),
    }
}

/// OpenAI `finish_reason` из Gemini `finishReason`/`blockReason`. Общая с
/// Responses-адаптером этапа 4.3 (`responses.rs`).
pub(crate) fn map_finish_reason(finish_reason: Option<&str>) -> &'static str {
    match finish_reason {
        Some("MAX_TOKENS") => "length",
        Some("SAFETY")
        | Some("RECITATION")
        | Some("BLOCKLIST")
        | Some("PROHIBITED_CONTENT")
        | Some("SPII") => "content_filter",
        // STOP / OTHER / неизвестное — обычная остановка.
        _ => "stop",
    }
}

/// OpenAI `usage` из usageMetadata. completion включает thinking-токены
/// (thoughtsTokenCount — та же сумма, что тарифицирует metering), cache read
/// отражается в `prompt_tokens_details`.
fn map_usage(usage: &Value) -> Value {
    let tokens = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let prompt = tokens("promptTokenCount");
    let candidates = tokens("candidatesTokenCount");
    let thoughts = tokens("thoughtsTokenCount");
    let cached = tokens("cachedContentTokenCount");
    let completion = candidates.saturating_add(thoughts);
    let total = tokens("totalTokenCount");
    let total = if total > 0 {
        total
    } else {
        prompt.saturating_add(completion)
    };
    let mut mapped = json!({
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "total_tokens": total,
    });
    if cached > 0 {
        mapped["prompt_tokens_details"] = json!({"cached_tokens": cached});
    }
    mapped
}

/// OpenAI `type` ошибки по HTTP-статусу (класс ошибки клиент видит стабильно,
/// детальный google.rpc status уезжает в `code`).
fn openai_error_type(status: StatusCode) -> &'static str {
    match status.as_u16() {
        400 | 402 | 404 | 405 | 409 | 413 | 422 => "invalid_request_error",
        401 | 403 => "authentication_error",
        429 => "rate_limit_error",
        _ => "server_error",
    }
}

/// Единая точка синтетических OpenAI-ошибок адаптера. `reason` — статический
/// код для audit-middleware (TerminalErrorReason), как у ошибок плоскости.
/// Общая с Responses-адаптером этапа 4.3 (`responses.rs`).
pub(crate) fn chat_error(
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
    // Direct adapter errors happen before the native request starts. Call sites which
    // rebuild a failure after a successful 2xx must explicitly remove the signal because
    // the plane may already have charged that request.
    with_not_started(response)
}

pub(crate) fn invalid_request(message: &str, param: Option<&str>) -> Response {
    chat_error(
        StatusCode::BAD_REQUEST,
        message,
        param,
        Value::Null,
        "invalid_chat_request",
    )
}

pub(crate) fn unsupported_parameter(param: &str) -> Response {
    chat_error(
        StatusCode::BAD_REQUEST,
        &format!("Unsupported parameter: '{param}' is not supported with this endpoint."),
        Some(param),
        Value::String("unsupported_parameter".to_string()),
        "unsupported_parameter",
    )
}

/// Перевод не-200 ответа `gemini_api()` из Google-конверта в OpenAI-конверт.
/// Статус и `Retry-After` сохраняются; audit-reason пробрасывается в
/// extension. Особый случай: нативный `400 API_KEY_INVALID` (reason
/// `invalid_key`) → `401 authentication_error` — поведение, которого
/// OpenAI-клиент ждёт на невалидный ключ. Общий с Responses-адаптером
/// этапа 4.3 (`responses.rs`).
pub(crate) async fn convert_error_response(upstream: Response) -> Response {
    let status = upstream.status();
    let not_started = !status.is_success()
        && upstream
            .headers()
            .get(EXECUTION_STATE_HEADER)
            .is_some_and(|value| value == EXECUTION_STATE_NOT_STARTED);
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
        .and_then(|e| e.get("status"))
        .and_then(Value::as_str)
        .map(|s| Value::String(s.to_string()))
        .unwrap_or(Value::Null);
    let (status, reason) = if reason == "invalid_key" {
        (StatusCode::UNAUTHORIZED, "invalid_key")
    } else {
        (status, reason)
    };
    let mut response = chat_error(status, message, None, code, reason);
    if let Some(retry_after) = retry_after {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, retry_after);
    }
    if !not_started {
        response = without_not_started(response);
    }
    response
}

type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// Перевод non-stream ответа GenerateContentResponse → `chat.completion`.
async fn json_chat_response(upstream: Response, requested_model: String) -> Response {
    let request_id = upstream.headers().get("request-id").cloned();
    let bytes = match to_bytes(upstream.into_body(), RESPONSE_BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return without_not_started(chat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The provider returned an unreadable response.",
                None,
                Value::Null,
                "internal_response_error",
            ))
        }
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => {
            return without_not_started(chat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The provider returned a malformed response.",
                None,
                Value::Null,
                "internal_response_error",
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
    let (text, reasoning, tool_calls) = candidate_content(candidate);
    // finishReason кандидата; candidates отсутствуют при блокировке промпта
    // на входе — тогда finish берётся из promptFeedback.blockReason
    // (content_filter с пустым content).
    let finish = candidate
        .and_then(|c| c.get("finishReason"))
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("promptFeedback")
                .and_then(|f| f.get("blockReason"))
                .and_then(Value::as_str)
        });
    let finish = map_finish_reason(finish);
    let usage = value
        .get("usageMetadata")
        .map(map_usage)
        .unwrap_or(Value::Null);
    // Контракт OpenAI: при tool calls без текста content — null.
    let content = if text.is_empty() && !tool_calls.is_empty() {
        Value::Null
    } else {
        Value::String(text)
    };
    let mut message = json!({"role": "assistant", "content": content});
    // reasoning_content присутствует, только если thought-парты были.
    if !reasoning.is_empty() {
        message["reasoning_content"] = Value::String(reasoning);
    }
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    let completion = json!({
        "id": new_id("chatcmpl"),
        "object": "chat.completion",
        "created": pool::now(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
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

/// text-парты кандидата склеиваются в content; text thought-партов
/// (`"thought": true`, этап 3.4b) — в reasoning, а не в content
/// (`thoughtSignature` всегда выбрасывается — решение 4); functionCall-парты →
/// `tool_calls` (args сериализуются обратно в JSON-строку arguments —
/// контракт OpenAI). id синтезируются (`callu_<name>`, `_2`, ... при
/// повторах имени): на private wire functionCall.id не приезжает.
fn candidate_content(candidate: Option<&Value>) -> (String, String, Vec<Value>) {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();
    let mut name_counts: HashMap<&str, u64> = HashMap::new();
    let Some(parts) = candidate
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(Value::as_array)
    else {
        return (text, reasoning, tool_calls);
    };
    for part in parts {
        // thought-парт: его text — reasoning, а не content. Парт с одним
        // thoughtSignature (без text/functionCall) игнорируется.
        if part.get("thought").and_then(Value::as_bool) == Some(true) {
            if let Some(t) = part.get("text").and_then(Value::as_str) {
                reasoning.push_str(t);
            }
            continue;
        }
        if let Some(t) = part.get("text").and_then(Value::as_str) {
            text.push_str(t);
            continue;
        }
        if let Some(call) = part.get("functionCall") {
            let name = call.get("name").and_then(Value::as_str).unwrap_or_default();
            let count = name_counts.entry(name).or_insert(0);
            *count += 1;
            let id = synthetic_call_id(name, *count);
            let arguments = call
                .get("args")
                .map(|args| args.to_string())
                .unwrap_or_else(|| "{}".to_string());
            tool_calls.push(json!({
                "id": id,
                "type": "function",
                "function": {"name": name, "arguments": arguments},
            }));
        }
    }
    (text, reasoning, tool_calls)
}

/// Синтезируемый id tool call'а: на private wire functionCall.id не
/// приезжает, поэтому id детерминированные `callu_<name>[_N]`. Общая схема
/// non-stream/stream перевода и Responses-адаптера этапа 4.3.
pub(crate) fn synthetic_call_id(name: &str, ordinal: u64) -> String {
    if ordinal == 1 {
        format!("callu_{name}")
    } else {
        format!("callu_{name}_{ordinal}")
    }
}

/// Перевод SSE-ответа `gemini_api()` в поток `chat.completion.chunk`.
/// Транслятор навешивается СНАРУЖИ: usage/settlement внутри плоскости уже
/// протапали оригинальные байты GenerateContentResponse.
fn stream_chat_response(
    upstream: Response,
    requested_model: String,
    include_usage: bool,
) -> Response {
    let request_id = upstream.headers().get("request-id").cloned();
    let stream = upstream
        .into_body()
        .into_data_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let translator = GeminiChatSseTranslator::new(Box::pin(stream), requested_model, include_usage);
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(translator))
        .unwrap_or_else(|_| {
            without_not_started(chat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error",
                None,
                Value::Null,
                "internal_response_error",
            ))
        });
    if let Some(request_id) = request_id {
        response.headers_mut().insert("request-id", request_id);
    }
    response
}

/// Потоковый транслятор GenerateContentResponse SSE → chat.completion.chunk
/// SSE. Кадры плоскости — data-only (`data: {json}\n\n`, без `event:`).
/// Буферизуются только байты одного незакрытого кадра; готовые чанки
/// отдаются немедленно.
struct GeminiChatSseTranslator {
    inner: ByteStream,
    buf: BytesMut,
    out: VecDeque<Bytes>,
    id: String,
    created: i64,
    /// Запрошенная (native) модель — фолбэк, пока/если кадры не сообщили
    /// `modelVersion`.
    requested_model: String,
    served_model: Option<String>,
    include_usage: bool,
    /// usageMetadata приходит на кадрах нарастающим итогом — в usage-чанк
    /// (на EOF) уходит последнее значение.
    last_usage: Option<Value>,
    role_sent: bool,
    /// Порядковый номер tool_call и per-name счётчик синтезируемых id —
    /// та же схема `callu_<name>[_N]`, что в non-stream переводе.
    next_tool_index: u64,
    name_counts: HashMap<String, u64>,
    source: GeminiStreamState,
    /// Терминальный кадр (`[DONE]` или error) уже поставлен в `out`.
    finished: bool,
}

impl GeminiChatSseTranslator {
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
            last_usage: None,
            role_sent: false,
            next_tool_index: 0,
            name_counts: HashMap::new(),
            source: GeminiStreamState::default(),
            finished: false,
        }
    }

    fn model(&self) -> &str {
        self.served_model
            .as_deref()
            .unwrap_or(&self.requested_model)
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

    /// Первый client-visible кадр открывается role-чанком (конвенция OpenAI).
    fn ensure_role(&mut self) {
        if !self.role_sent {
            self.role_sent = true;
            self.push_chunk(json!({"role": "assistant", "content": ""}), Value::Null);
        }
    }

    /// OpenAI-ошибка внутри стрима: кадр `{"error": ...}` без `[DONE]`
    /// (поведение самого OpenAI при mid-stream сбое).
    fn push_error(&mut self, message: &str, code: Option<&str>) {
        let kind = match code {
            Some("RESOURCE_EXHAUSTED") => "rate_limit_error",
            Some("UNAUTHENTICATED") | Some("PERMISSION_DENIED") => "authentication_error",
            _ => "server_error",
        };
        self.out.push_back(Self::frame(json!({"error": {
            "message": message,
            "type": kind,
            "code": code,
        }})));
        self.finished = true;
    }

    /// Один data-кадр GenerateContentResponse. Порядок важен: modelVersion и
    /// usageMetadata фиксируются ДО эмиссии чанков, чтобы чанки кадра уже
    /// несли сервёную модель.
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
        // Блокировка промпта на входе: кандидатов не будет, стрим завершается
        // finish=content_filter (usage-чанк и [DONE] уйдут на EOF как обычно).
        if let Some(block) = data
            .get("promptFeedback")
            .and_then(|f| f.get("blockReason"))
            .and_then(Value::as_str)
        {
            self.ensure_role();
            let finish = map_finish_reason(Some(block));
            self.push_chunk(json!({}), Value::String(finish.to_string()));
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
        self.ensure_role();
        if let Some(parts) = candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(Value::as_array)
        {
            for part in parts {
                // thought-парт (этап 3.4b): его text — reasoning, а не
                // content → reasoning_content-дельта. thoughtSignature
                // выбрасывается (решение 4): парт с одним thoughtSignature
                // видимого чанка не порождает.
                if part.get("thought").and_then(Value::as_bool) == Some(true) {
                    if let Some(text) = part
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|t| !t.is_empty())
                    {
                        self.push_chunk(json!({"reasoning_content": text}), Value::Null);
                    }
                    continue;
                }
                if let Some(text) = part
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                {
                    self.push_chunk(json!({"content": text}), Value::Null);
                    continue;
                }
                // Gemini присылает functionCall целиком (arguments-дельт на
                // wire нет) → один tool_calls-чанк с полной строкой arguments.
                if let Some(call) = part.get("functionCall") {
                    let name = call.get("name").and_then(Value::as_str).unwrap_or_default();
                    let count = self.name_counts.entry(name.to_string()).or_insert(0);
                    *count += 1;
                    let id = synthetic_call_id(name, *count);
                    let arguments = call
                        .get("args")
                        .map(|args| args.to_string())
                        .unwrap_or_else(|| "{}".to_string());
                    let index = self.next_tool_index;
                    self.next_tool_index += 1;
                    self.push_chunk(
                        json!({"tool_calls": [{
                            "index": index,
                            "id": id,
                            "type": "function",
                            "function": {"name": name, "arguments": arguments},
                        }]}),
                        Value::Null,
                    );
                }
            }
        }
        if let Some(finish) = candidate.get("finishReason").and_then(Value::as_str) {
            let finish = map_finish_reason(Some(finish));
            self.push_chunk(json!({}), Value::String(finish.to_string()));
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

impl Stream for GeminiChatSseTranslator {
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
                // Голый транспортный сбой до сюда доходить не должен
                // (плоскость санитизирует его в error-кадр), но стрим не
                // обрываем молча ни при каком раскладе.
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
                    if !self.source.is_complete() {
                        self.push_error(
                            "The provider stream ended before completion.",
                            Some("protocol_error"),
                        );
                        continue;
                    }
                    // Gemini has no message_stop; a validated finishReason/blockReason plus EOF
                    // is the success boundary.
                    if self.include_usage {
                        if let Some(usage) = self.last_usage.take() {
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
                    }
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

    fn ok_translated(value: Value) -> Translated {
        translate_chat_request(value).expect("translation must succeed")
    }

    async fn err_parts(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn expect_err(value: Value) -> (StatusCode, Value) {
        err_parts(translate_chat_request(value).unwrap_err()).await
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

    fn translate_events(events: &str, include_usage: bool) -> GeminiChatSseTranslator {
        GeminiChatSseTranslator::new(
            sse_bytes(events),
            "gemini-2.5-flash".to_string(),
            include_usage,
        )
    }

    // ---------- перевод запроса ----------

    #[test]
    fn translates_basic_chat_to_generate_content() {
        let translated = ok_translated(json!({
            "model": "google/gemini-2.5-flash",
            "messages": [
                {"role": "system", "content": "Be terse."},
                {"role": "developer", "content": "Extra guard."},
                {"role": "user", "content": "Hello"}
            ]
        }));
        let body = &translated.body;
        // google/-префикс снят — дальше нативный публичный id.
        assert_eq!(translated.model, "gemini-2.5-flash");
        assert!(!translated.stream);
        assert!(!translated.include_usage);
        assert_eq!(
            body["systemInstruction"],
            json!({"parts": [{"text": "Be terse."}, {"text": "Extra guard."}]})
        );
        assert_eq!(
            body["contents"],
            json!([{"role": "user", "parts": [{"text": "Hello"}]}])
        );
        assert!(body["generationConfig"].get("maxOutputTokens").is_none());
        // toolConfig отсутствует — дефолт AUTO не вставляется.
        assert!(body.get("toolConfig").is_none());
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn merges_consecutive_same_role_messages() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [
                {"role": "user", "content": "a"},
                {"role": "user", "content": [{"type": "text", "text": "b"}]},
                {"role": "assistant", "content": "c"},
                {"role": "assistant", "content": "d"},
                {"role": "user", "content": "e"}
            ]
        }));
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
    fn omits_reasoning_only_assistant_replay_without_leaking_thoughts() {
        // Universal Chat не выставляет Gemini thoughtSignature. AI SDK штатно
        // реплеит полученный reasoning как content:"" + reasoning_content;
        // такой turn валиден, но не должен становиться unsigned thought или
        // видимым text-партом в generateContent history.
        let translated = ok_translated(json!({
            "model": "gemini-3.6-flash",
            "messages": [
                {"role": "user", "content": "first"},
                {"role": "assistant", "content": "", "reasoning_content": "private thought"},
                {"role": "user", "content": "continue"}
            ]
        }));
        assert_eq!(
            translated.body["contents"],
            json!([{"role": "user", "parts": [
                {"text": "first"},
                {"text": "continue"}
            ]}])
        );
        assert!(!translated.body.to_string().contains("private thought"));
    }

    #[test]
    fn honors_generation_config_parameters() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "stream": true,
            "stream_options": {"include_usage": true},
            "max_tokens": 100,
            "max_completion_tokens": 200,
            "temperature": 0.7,
            "top_p": 0.9,
            "top_k": 40,
            "stop": ["END", "STOP"]
        }));
        assert!(translated.stream);
        assert!(translated.include_usage);
        let config = &translated.body["generationConfig"];
        // max_completion_tokens приоритетнее max_tokens.
        assert_eq!(config["maxOutputTokens"], 200);
        assert_eq!(config["temperature"], 0.7);
        assert_eq!(config["topP"], 0.9);
        assert_eq!(config["topK"], 40);
        assert_eq!(config["stopSequences"], json!(["END", "STOP"]));
    }

    #[test]
    fn null_optional_controls_keep_defaults_and_allow_legacy_token_alias() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "stream": null,
            "stream_options": {"include_usage": null},
            "max_completion_tokens": null,
            "max_tokens": 77
        }));
        assert!(!translated.stream);
        assert!(!translated.include_usage);
        assert_eq!(translated.body["generationConfig"]["maxOutputTokens"], 77);
    }

    #[tokio::test]
    async fn malformed_optional_controls_are_parameter_specific_400s() {
        for (field, value, expected_param) in [
            ("stream", json!("false"), "stream"),
            ("max_completion_tokens", json!(0), "max_completion_tokens"),
            ("max_completion_tokens", json!(-1), "max_completion_tokens"),
            ("max_completion_tokens", json!(1.5), "max_completion_tokens"),
            ("max_tokens", json!("10"), "max_tokens"),
            ("max_tokens", json!({}), "max_tokens"),
        ] {
            let mut request = json!({
                "model": "gemini-2.5-flash",
                "messages": [{"role": "user", "content": "hi"}]
            });
            request[field] = value;
            let (status, body) = expect_err(request).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(body["error"]["param"], expected_param, "{body}");
        }

        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "stream_options": {"include_usage": "true"}
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["param"], "stream_options.include_usage");

        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "max_completion_tokens": 0,
            "max_tokens": 77
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["param"], "max_completion_tokens");
    }

    #[test]
    fn stop_string_becomes_single_stop_sequence() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "stop": "END"
        }));
        assert_eq!(
            translated.body["generationConfig"]["stopSequences"],
            json!(["END"])
        );
    }

    #[tokio::test]
    async fn rejects_invalid_stop() {
        for stop in [
            json!([]),
            json!(["a", "b", "c", "d", "e", "f"]),
            json!(["a", 1]),
        ] {
            let (status, body) = expect_err(json!({
                "model": "gemini-2.5-flash",
                "messages": [{"role": "user", "content": "hi"}],
                "stop": stop
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["error"]["param"], "stop");
        }
    }

    #[test]
    fn translates_tools_to_function_declarations() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [
                {"type": "function", "function": {
                    "name": "get_weather",
                    "description": "Weather by city",
                    "parameters": {
                        "$schema": "https://json-schema.org/draft/2020-12/schema",
                        "type": "object",
                        "properties": {
                            "city": {"type": "string", "exclusiveMinimum": 1}
                        }
                    }
                }},
                {"type": "function", "function": {"name": "ping"}}
            ]
        }));
        assert_eq!(
            translated.body["tools"],
            json!([{"functionDeclarations": [
                {
                    "name": "get_weather",
                    "description": "Weather by city",
                    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
                },
                {"name": "ping"}
            ]}])
        );
    }

    #[test]
    fn translates_legacy_functions() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "functions": [{"name": "get_weather", "parameters": {"type": "object"}}]
        }));
        assert_eq!(
            translated.body["tools"],
            json!([{"functionDeclarations": [
                {"name": "get_weather", "parameters": {"type": "object"}}
            ]}])
        );
    }

    #[test]
    fn schema_translator_preserves_property_names_and_exactly_rewrites_refs_and_bounds() {
        let schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$defs": {"Mode": {"type": "string", "const": "fast"}},
            "type": "object",
            "description": "unchanged",
            "properties": {
                "$schema": {"$ref": "#/$defs/Mode"},
                "exclusiveMinimum": {"type": "number", "exclusiveMinimum": 1},
                "nested": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {"value": {"type": "number", "exclusiveMinimum": 2}}
                    }
                }
            },
            "required": ["$schema", "exclusiveMinimum"]
        });
        let translated = code_assist_schema(&schema, "tools.0.function.parameters").unwrap();
        assert_eq!(
            translated["properties"]["$schema"],
            json!({"type":"string", "enum":["fast"]})
        );
        assert!(
            translated["properties"]["exclusiveMinimum"]["minimum"]
                .as_f64()
                .unwrap()
                > 1.0
        );
        assert!(
            translated["properties"]["nested"]["items"]["properties"]["value"]["minimum"]
                .as_f64()
                .unwrap()
                > 2.0
        );
        assert!(translated.get("$defs").is_none());
    }

    #[tokio::test]
    async fn tool_and_structured_schema_errors_report_the_exact_pointer() {
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type":"function", "function":{"name":"f", "parameters":{
                "type":"object", "properties":{"x":{"type":"object",
                    "patternProperties":{"^a":{"type":"string"}}}}
            }}}]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(
            body["error"]["param"],
            "tools.0.function.parameters/properties/x/patternProperties"
        );

        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "response_format": {"type":"json_schema", "json_schema":{"name":"x",
                "schema":{"type":"boolean", "const":true}}}
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(
            body["error"]["param"],
            "response_format.json_schema.schema/const"
        );
    }

    #[test]
    fn translates_tool_choice_to_function_calling_config() {
        let cases = [
            (json!("auto"), Value::Null),
            (json!("required"), json!({"mode": "ANY"})),
            (json!("none"), json!({"mode": "NONE"})),
            (
                json!({"type": "function", "function": {"name": "get_weather"}}),
                json!({"mode": "ANY", "allowedFunctionNames": ["get_weather"]}),
            ),
        ];
        for (choice, expected) in cases {
            let translated = ok_translated(json!({
                "model": "gemini-2.5-flash",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{"type": "function", "function": {"name": "get_weather"}}],
                "tool_choice": choice
            }));
            if expected.is_null() {
                assert!(translated.body.get("toolConfig").is_none());
            } else {
                assert_eq!(
                    translated.body["toolConfig"],
                    json!({"functionCallingConfig": expected})
                );
            }
        }
    }

    #[test]
    fn translates_legacy_function_call() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "function_call": {"name": "get_weather"}
        }));
        assert_eq!(
            translated.body["toolConfig"],
            json!({"functionCallingConfig": {"mode": "ANY", "allowedFunctionNames": ["get_weather"]}})
        );
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "function_call": "none"
        }));
        assert_eq!(
            translated.body["toolConfig"],
            json!({"functionCallingConfig": {"mode": "NONE"}})
        );
    }

    #[tokio::test]
    async fn rejects_invalid_tool_choice() {
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": "sometimes"
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["param"], "tool_choice");
    }

    #[test]
    fn translates_tool_history_with_id_name_recovery() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [
                {"role": "user", "content": "weather?"},
                {"role": "assistant", "content": null, "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\": \"Paris\"}"}
                }]},
                {"role": "tool", "tool_call_id": "call_1", "content": "{\"temp\": 20}"},
                {"role": "tool", "tool_call_id": "call_1", "content": "sunny"},
                {"role": "user", "content": "thanks"}
            ]
        }));
        assert_eq!(
            translated.body["contents"],
            json!([
                {"role": "user", "parts": [{"text": "weather?"}]},
                {"role": "model", "parts": [
                    {"functionCall": {"name": "get_weather", "args": {"city": "Paris"}},
                     "thoughtSignature": "context_engineering_is_the_way_to_go"}
                ]},
                {"role": "user", "parts": [
                    {"functionResponse": {"name": "get_weather", "response": {"result": {"temp": 20}}}},
                    {"functionResponse": {"name": "get_weather", "response": {"result": "sunny"}}},
                    {"text": "thanks"}
                ]}
            ])
        );
    }

    #[test]
    fn translates_legacy_function_history() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [
                {"role": "user", "content": "weather?"},
                {"role": "assistant", "function_call": {"name": "get_weather", "arguments": ""}},
                {"role": "function", "name": "get_weather", "content": "sunny"}
            ]
        }));
        assert_eq!(
            translated.body["contents"],
            json!([
                {"role": "user", "parts": [{"text": "weather?"}]},
                {"role": "model", "parts": [
                    {"functionCall": {"name": "get_weather", "args": {}},
                     "thoughtSignature": "context_engineering_is_the_way_to_go"}
                ]},
                {"role": "user", "parts": [
                    {"functionResponse": {"name": "get_weather", "response": {"result": "sunny"}}}
                ]}
            ])
        );
    }

    #[tokio::test]
    async fn rejects_tool_message_without_matching_call() {
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "tool", "tool_call_id": "call_unknown", "content": "x"}
            ]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no matching tool call"));
    }

    #[tokio::test]
    async fn rejects_malformed_tool_arguments() {
        for arguments in [json!("not json"), json!("[1]"), json!(42)] {
            let (status, _) = expect_err(json!({
                "model": "gemini-2.5-flash",
                "messages": [
                    {"role": "user", "content": "hi"},
                    {"role": "assistant", "tool_calls": [{
                        "id": "c1", "type": "function",
                        "function": {"name": "f", "arguments": arguments}
                    }]}
                ]
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }
    }

    // ---------- capability matrix ----------

    #[tokio::test]
    async fn rejects_non_default_matrix_parameters() {
        let cases: [(&str, Value); 18] = [
            ("parallel_tool_calls", json!(false)),
            ("response_format", json!({"type": "future_format"})),
            ("store", json!(true)),
            ("metadata", json!({"a": 1})),
            ("n", json!(2)),
            ("presence_penalty", json!(0.5)),
            ("frequency_penalty", json!(-0.5)),
            ("logit_bias", json!({"123": 1})),
            ("logprobs", json!(true)),
            ("top_logprobs", json!(3)),
            ("seed", json!(42)),
            ("service_tier", json!("flex")),
            ("modalities", json!(["text", "image"])),
            ("audio", json!({"voice": "alloy", "format": "mp3"})),
            ("prediction", json!({"type": "content", "content": "x"})),
            ("web_search_options", json!({})),
            ("user", json!("user-1")),
            ("stream_options", json!({"include_usage": true, "extra": 1})),
        ];
        for (param, value) in cases {
            let (status, body) = expect_err(json!({
                "model": "gemini-2.5-flash",
                "messages": [{"role": "user", "content": "hi"}],
                param: value
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "param {param}");
            assert_eq!(body["error"]["type"], "invalid_request_error");
            assert_eq!(body["error"]["code"], "unsupported_parameter");
            assert_eq!(body["error"]["param"], param);
        }
    }

    #[test]
    fn accepts_default_matrix_parameters() {
        ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "parallel_tool_calls": true,
            "response_format": {"type": "text"},
            "reasoning_effort": null,
            "store": false,
            "metadata": null,
            "n": 1,
            "presence_penalty": 0.0,
            "frequency_penalty": 0,
            "logit_bias": {},
            "logprobs": false,
            "top_logprobs": null,
            "seed": null,
            "service_tier": "auto",
            "modalities": ["text"],
            "audio": null,
            "prediction": null,
            "web_search_options": null,
            "user": null,
            "stream": true,
            "stream_options": {"include_usage": true}
        }));
    }

    #[tokio::test]
    async fn rejects_unknown_top_level_field() {
        // Закрытый список: wrapper апстрима выбросил бы поле молча.
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "future_parameter": 1
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "unsupported_parameter");
        assert_eq!(body["error"]["param"], "future_parameter");
    }

    #[tokio::test]
    async fn rejects_malformed_messages() {
        let cases = [
            json!({"model": "m", "messages": []}),
            json!({"model": "m", "messages": [{"role": "user"}]}),
            json!({"model": "m", "messages": [{"role": "user", "content": ""}]}),
            json!({"model": "m", "messages": [{"role": "assistant", "content": null}]}),
            json!({"model": "m", "messages": [{"role": "assistant", "content": "", "reasoning_content": ""}]}),
            json!({"model": "m", "messages": [{"role": "narrator", "content": "hi"}]}),
            json!({"model": "m", "messages": [{"role": "user", "content": "hi", "name": "bob"}]}),
            json!({"model": "m", "messages": [{"role": "user", "content": [{"type": "image_url", "image_url": {"url": "https://x"}}]}]}),
            json!({"model": "m", "messages": [{"role": "system", "content": "s"}]}),
        ];
        for case in cases {
            let (status, body) = expect_err(case).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(body["error"]["type"], "invalid_request_error");
        }
    }

    #[test]
    fn translates_user_data_image_to_inline_data() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "What is this?"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgo="}},
                {"type": "text", "text": "Be brief."}
            ]}]
        }));
        assert_eq!(
            translated.body["contents"],
            json!([{"role": "user", "parts": [
                {"text": "What is this?"},
                {"inlineData": {"mimeType": "image/png", "data": "iVBORw0KGgo="}},
                {"text": "Be brief."}
            ]}])
        );
    }

    #[tokio::test]
    async fn rejects_invalid_image_parts() {
        for (content, expected) in [
            // http(s) ссылки generateContent не принимает — честный 400.
            (
                json!([{"type": "image_url", "image_url": {"url": "https://example.com/cat.jpg"}}]),
                "only data: image URLs",
            ),
            // detail != auto — generateContent не умеет.
            (
                json!([{"type": "image_url", "image_url": {"url": "data:image/png;base64,aGVsbG8=", "detail": "low"}}]),
                "Unsupported parameter",
            ),
            // Битый data: URL.
            (
                json!([{"type": "image_url", "image_url": {"url": "data:image/png;plain,xyz"}}]),
                "data URL",
            ),
            // Не-image MIME.
            (
                json!([{"type": "image_url", "image_url": {"url": "data:text/html;base64,PGI+"}}]),
                "image MIME",
            ),
        ] {
            let (status, body) = expect_err(json!({
                "model": "gemini-2.5-flash",
                "messages": [{"role": "user", "content": content}],
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{content}");
            assert!(
                body["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains(expected),
                "{content}: {body}"
            );
        }
    }

    #[test]
    fn translates_response_format_to_generation_config() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "Extract."}],
            "response_format": {"type": "json_object"}
        }));
        let config = &translated.body["generationConfig"];
        assert_eq!(config["responseMimeType"], "application/json");
        assert!(config.get("responseSchema").is_none());

        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "Extract."}],
            "response_format": {"type": "json_schema", "json_schema": {
                "name": "profile", "strict": true,
                "schema": {"$schema": "https://json-schema.org/draft/2020-12/schema",
                    "type": "object", "properties": {
                        "name": {"type": "string", "exclusiveMaximum": 10}
                    }}
            }}
        }));
        let config = &translated.body["generationConfig"];
        assert_eq!(config["responseMimeType"], "application/json");
        // Обёртка (name/strict) не проксируется — только сама схема.
        assert_eq!(
            config["responseSchema"],
            json!({"type": "object", "properties": {"name": {"type": "string"}}})
        );
    }

    #[tokio::test]
    async fn rejects_response_format_json_schema_without_schema() {
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "response_format": {"type": "json_schema", "json_schema": {"name": "x"}}
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["param"], "response_format");
    }

    // ---------- reasoning (этап 3.4b) ----------

    #[test]
    fn translates_reasoning_effort_to_thinking_config() {
        // Каждый уровень проксируется как есть (маппинг в wire model id —
        // на плоскости), includeThoughts включает thought-парты ответа.
        for level in ["minimal", "low", "medium", "high"] {
            let translated = ok_translated(json!({
                "model": "gemini-2.5-flash",
                "messages": [{"role": "user", "content": "hi"}],
                "reasoning_effort": level,
            }));
            assert_eq!(
                translated.body["generationConfig"]["thinkingConfig"],
                json!({"thinkingLevel": level, "includeThoughts": true}),
                "{level}"
            );
        }
        // null/отсутствие — выкл: thinkingConfig не появляется.
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "reasoning_effort": null,
        }));
        assert!(translated.body["generationConfig"]
            .get("thinkingConfig")
            .is_none());
    }

    #[test]
    fn reasoning_effort_and_response_format_share_generation_config() {
        // thinkingConfig — соседнее поле generationConfig: structured output
        // (3.4a) не затирается.
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "Extract."}],
            "reasoning_effort": "high",
            "response_format": {"type": "json_object"}
        }));
        let config = &translated.body["generationConfig"];
        assert_eq!(
            config["thinkingConfig"],
            json!({"thinkingLevel": "high", "includeThoughts": true})
        );
        assert_eq!(config["responseMimeType"], "application/json");
    }

    #[tokio::test]
    async fn rejects_invalid_reasoning_effort() {
        // Любое не-null значение вне minimal|low|medium|high → 400
        // invalid_request (не unsupported_parameter — параметр поддержан).
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "reasoning_effort": "extreme"
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert_eq!(body["error"]["param"], "reasoning_effort");
        assert!(body["error"]["code"].is_null());
    }

    // ---------- ответ: unit-маппинги ----------

    #[test]
    fn maps_finish_reasons() {
        assert_eq!(map_finish_reason(Some("MAX_TOKENS")), "length");
        assert_eq!(map_finish_reason(Some("SAFETY")), "content_filter");
        assert_eq!(map_finish_reason(Some("RECITATION")), "content_filter");
        assert_eq!(
            map_finish_reason(Some("PROHIBITED_CONTENT")),
            "content_filter"
        );
        assert_eq!(map_finish_reason(Some("STOP")), "stop");
        assert_eq!(map_finish_reason(Some("OTHER")), "stop");
        assert_eq!(map_finish_reason(None), "stop");
    }

    #[test]
    fn maps_usage_with_thoughts_and_cache() {
        let usage = map_usage(&json!({
            "promptTokenCount": 10,
            "candidatesTokenCount": 5,
            "thoughtsTokenCount": 3,
            "totalTokenCount": 18,
            "cachedContentTokenCount": 4
        }));
        assert_eq!(usage["prompt_tokens"], 10);
        // completion = candidates + thoughts (как тарифицирует metering).
        assert_eq!(usage["completion_tokens"], 8);
        assert_eq!(usage["total_tokens"], 18);
        assert_eq!(usage["prompt_tokens_details"]["cached_tokens"], 4);
    }

    #[test]
    fn maps_usage_total_fallback() {
        let usage = map_usage(&json!({
            "promptTokenCount": 10,
            "candidatesTokenCount": 5
        }));
        assert_eq!(usage["total_tokens"], 15);
        assert!(usage.get("prompt_tokens_details").is_none());
    }

    // ---------- ответ: non-stream ----------

    #[tokio::test]
    async fn json_response_text_and_usage() {
        let upstream = upstream_json(json!({
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "Hi"}, {"text": " there"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 2, "totalTokenCount": 5},
            "modelVersion": "gemini-2.5-flash-001"
        }));
        let response = json_chat_response(upstream, "gemini-2.5-flash".to_string()).await;
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["object"], "chat.completion");
        assert!(body["id"].as_str().unwrap().starts_with("chatcmpl_"));
        assert_eq!(body["model"], "gemini-2.5-flash-001");
        let choice = &body["choices"][0];
        assert_eq!(choice["message"]["role"], "assistant");
        assert_eq!(choice["message"]["content"], "Hi there");
        assert_eq!(choice["finish_reason"], "stop");
        assert_eq!(body["usage"]["prompt_tokens"], 3);
        assert_eq!(body["usage"]["completion_tokens"], 2);
        assert_eq!(body["usage"]["total_tokens"], 5);
    }

    #[tokio::test]
    async fn json_response_function_calls() {
        let upstream = upstream_json(json!({
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"functionCall": {"name": "get_weather", "args": {"city": "Paris"}}},
                    {"functionCall": {"name": "get_weather", "args": {"city": "Lyon"}}}
                ]},
                "finishReason": "STOP"
            }]
        }));
        let response = json_chat_response(upstream, "gemini-2.5-flash".to_string()).await;
        let (_, body) = err_parts(response).await;
        let message = &body["choices"][0]["message"];
        // tool calls без текста → content null (контракт OpenAI).
        assert!(message["content"].is_null());
        let calls = message["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["id"], "callu_get_weather");
        assert_eq!(calls[1]["id"], "callu_get_weather_2");
        assert_eq!(calls[0]["function"]["name"], "get_weather");
        assert_eq!(
            calls[0]["function"]["arguments"].as_str().unwrap(),
            "{\"city\":\"Paris\"}"
        );
    }

    #[tokio::test]
    async fn json_response_prompt_block_is_content_filter() {
        let upstream = upstream_json(json!({
            "promptFeedback": {"blockReason": "SAFETY"},
            "usageMetadata": {"promptTokenCount": 7, "totalTokenCount": 7}
        }));
        let response = json_chat_response(upstream, "gemini-2.5-flash".to_string()).await;
        let (_, body) = err_parts(response).await;
        let choice = &body["choices"][0];
        assert_eq!(choice["finish_reason"], "content_filter");
        assert_eq!(choice["message"]["content"], "");
    }

    #[tokio::test]
    async fn json_response_thought_parts_map_to_reasoning_content() {
        // thought-парты (3.4b) склеиваются в reasoning_content и не смешиваются
        // с content; thoughtSignature выбрасывается, парт с одним
        // thoughtSignature игнорируется.
        let upstream = upstream_json(json!({
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"text": "Thinking. ", "thought": true},
                    {"text": "More.", "thought": true, "thoughtSignature": "sig"},
                    {"thoughtSignature": "sig-only"},
                    {"text": "Answer."}
                ]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 2, "thoughtsTokenCount": 4, "totalTokenCount": 9}
        }));
        let response = json_chat_response(upstream, "gemini-2.5-flash".to_string()).await;
        let (_, body) = err_parts(response).await;
        let message = &body["choices"][0]["message"];
        assert_eq!(message["content"], "Answer.");
        assert_eq!(message["reasoning_content"], "Thinking. More.");
        // thoughts-токены по-прежнему в completion (как тарифицирует metering).
        assert_eq!(body["usage"]["completion_tokens"], 6);
    }

    #[tokio::test]
    async fn json_response_without_thoughts_has_no_reasoning_content() {
        let upstream = upstream_json(json!({
            "candidates": [{"content": {"parts": [{"text": "Plain."}]}, "finishReason": "STOP"}]
        }));
        let response = json_chat_response(upstream, "gemini-2.5-flash".to_string()).await;
        let (_, body) = err_parts(response).await;
        let message = &body["choices"][0]["message"];
        assert_eq!(message["content"], "Plain.");
        assert!(message.get("reasoning_content").is_none(), "{message}");
    }

    #[tokio::test]
    async fn json_response_model_falls_back_to_requested() {
        let upstream = upstream_json(json!({
            "candidates": [{"content": {"parts": [{"text": "ok"}]}, "finishReason": "STOP"}]
        }));
        let response = json_chat_response(upstream, "gemini-2.5-flash".to_string()).await;
        let (_, body) = err_parts(response).await;
        assert_eq!(body["model"], "gemini-2.5-flash");
        // usage отсутствовал → null.
        assert!(body["usage"].is_null());
    }

    #[tokio::test]
    async fn json_response_malformed_body_is_500() {
        let upstream = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("not json"))
            .unwrap();
        let response = json_chat_response(upstream, "gemini-2.5-flash".to_string()).await;
        assert!(response.headers().get(EXECUTION_STATE_HEADER).is_none());
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["type"], "server_error");
    }

    // ---------- ответ: конверсия ошибок ----------

    #[tokio::test]
    async fn invalid_key_maps_to_401() {
        // Нативная плоскость отвечает 400 API_KEY_INVALID; OpenAI-клиент ждёт 401.
        let upstream = upstream_error(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"code":400,"message":"API key not valid.","status":"INVALID_ARGUMENT"}}"#,
            "invalid_key",
        );
        let response = convert_error_response(upstream).await;
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["type"], "authentication_error");
        assert_eq!(body["error"]["message"], "API key not valid.");
        assert_eq!(body["error"]["code"], "INVALID_ARGUMENT");
    }

    #[tokio::test]
    async fn low_balance_keeps_402() {
        let upstream = upstream_error(
            StatusCode::PAYMENT_REQUIRED,
            r#"{"error":{"code":402,"message":"Low balance","status":"FAILED_PRECONDITION"}}"#,
            "low_balance",
        );
        let response = convert_error_response(upstream).await;
        assert!(response.headers().get(EXECUTION_STATE_HEADER).is_none());
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert_eq!(body["error"]["code"], "FAILED_PRECONDITION");
    }

    #[tokio::test]
    async fn rate_limit_keeps_retry_after() {
        let mut upstream = upstream_error(
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"code":429,"message":"Quota exceeded","status":"RESOURCE_EXHAUSTED"}}"#,
            "upstream_rate_limit",
        );
        upstream
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("7"));
        upstream.headers_mut().insert(
            EXECUTION_STATE_HEADER,
            HeaderValue::from_static(EXECUTION_STATE_NOT_STARTED),
        );
        let response = convert_error_response(upstream).await;
        assert_eq!(response.headers().get(header::RETRY_AFTER).unwrap(), "7");
        assert_eq!(
            response.headers().get(EXECUTION_STATE_HEADER).unwrap(),
            EXECUTION_STATE_NOT_STARTED
        );
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body["error"]["type"], "rate_limit_error");
        assert_eq!(body["error"]["code"], "RESOURCE_EXHAUSTED");
    }

    #[tokio::test]
    async fn non_json_error_body_gets_generic_message() {
        let upstream = upstream_error(StatusCode::SERVICE_UNAVAILABLE, "boom", "upstream_error");
        let response = convert_error_response(upstream).await;
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["type"], "server_error");
        assert_eq!(body["error"]["message"], "The provider returned an error.");
        assert!(body["error"]["code"].is_null());
    }

    #[test]
    fn local_adapter_errors_mark_execution_not_started() {
        let response = invalid_request("bad request", Some("model"));
        assert_eq!(
            response.headers().get(EXECUTION_STATE_HEADER).unwrap(),
            EXECUTION_STATE_NOT_STARTED
        );
    }

    // ---------- ответ: stream ----------

    #[tokio::test]
    async fn stream_text_dialog_contract() {
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hello\"}]}}],\"modelVersion\":\"gemini-2.5-flash-001\",\"usageMetadata\":{\"promptTokenCount\":3}}\n",
            "\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" world\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":2,\"totalTokenCount\":5}}\n",
        );
        let output = collect_stream(translate_events(events, true)).await;
        assert!(output.ends_with("data: [DONE]\n\n"));
        let frames = data_frames(&output);
        // role → 2 content-дельты → finish → usage-чанк.
        assert_eq!(frames.len(), 5);
        assert_eq!(
            frames[0]["choices"][0]["delta"],
            json!({"role": "assistant", "content": ""})
        );
        assert_eq!(
            frames[1]["choices"][0]["delta"],
            json!({"content": "Hello"})
        );
        assert_eq!(
            frames[2]["choices"][0]["delta"],
            json!({"content": " world"})
        );
        assert_eq!(frames[3]["choices"][0]["delta"], json!({}));
        assert_eq!(frames[3]["choices"][0]["finish_reason"], "stop");
        assert_eq!(frames[4]["choices"], json!([]));
        assert_eq!(frames[4]["usage"]["prompt_tokens"], 3);
        assert_eq!(frames[4]["usage"]["completion_tokens"], 2);
        // id/created/model стабильны; model — сервёная из modelVersion.
        for frame in &frames {
            assert_eq!(frame["id"], frames[0]["id"]);
            assert_eq!(frame["created"], frames[0]["created"]);
            assert_eq!(frame["model"], "gemini-2.5-flash-001");
            assert_eq!(frame["object"], "chat.completion.chunk");
        }
    }

    #[tokio::test]
    async fn stream_without_include_usage_has_no_usage_chunk() {
        let events = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hi\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":3}}\n";
        let output = collect_stream(translate_events(events, false)).await;
        assert!(output.ends_with("data: [DONE]\n\n"));
        let frames = data_frames(&output);
        assert_eq!(frames.len(), 3);
        assert!(frames.iter().all(|f| f.get("usage").is_none()));
    }

    #[tokio::test]
    async fn stream_thought_parts_become_reasoning_content_deltas() {
        // thought-парты (3.4b): role → reasoning_content-дельты →
        // content-дельта → finish → DONE. Парт с одним thoughtSignature
        // видимого чанка не порождает, thought-текст в content не протекает.
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Thinking. \",\"thought\":true}]}}],\"modelVersion\":\"gemini-2.5-flash-001\"}\n",
            "\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"thoughtSignature\":\"sig\"},{\"text\":\"more \",\"thought\":true},{\"text\":\"Answer.\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":2,\"thoughtsTokenCount\":4,\"totalTokenCount\":9}}\n",
        );
        let output = collect_stream(translate_events(events, true)).await;
        assert!(output.ends_with("data: [DONE]\n\n"));
        let frames = data_frames(&output);
        // role → 2 reasoning-дельты → content → finish → usage-чанк.
        assert_eq!(frames.len(), 6, "{output}");
        assert_eq!(
            frames[0]["choices"][0]["delta"],
            json!({"role": "assistant", "content": ""})
        );
        assert_eq!(
            frames[1]["choices"][0]["delta"],
            json!({"reasoning_content": "Thinking. "})
        );
        assert_eq!(
            frames[2]["choices"][0]["delta"],
            json!({"reasoning_content": "more "})
        );
        assert_eq!(
            frames[3]["choices"][0]["delta"],
            json!({"content": "Answer."})
        );
        assert_eq!(frames[4]["choices"][0]["delta"], json!({}));
        assert_eq!(frames[4]["choices"][0]["finish_reason"], "stop");
        assert_eq!(frames[5]["choices"], json!([]));
        assert_eq!(frames[5]["usage"]["completion_tokens"], 6);
    }

    #[tokio::test]
    async fn stream_function_call_single_chunk() {
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"city\":\"Paris\"}}}]}}]}\n",
            "\n",
            "data: {\"candidates\":[{\"finishReason\":\"STOP\"}]}\n",
        );
        let output = collect_stream(translate_events(events, false)).await;
        let frames = data_frames(&output);
        assert_eq!(frames.len(), 3);
        assert_eq!(
            frames[1]["choices"][0]["delta"],
            json!({"tool_calls": [{
                "index": 0,
                "id": "callu_get_weather",
                "type": "function",
                "function": {"name": "get_weather", "arguments": "{\"city\":\"Paris\"}"}
            }]})
        );
        assert_eq!(frames[2]["choices"][0]["finish_reason"], "stop");
    }

    #[tokio::test]
    async fn stream_error_frame_without_done() {
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"}]}}]}\n",
            "\n",
            "data: {\"error\":{\"code\":429,\"message\":\"Quota exceeded\",\"status\":\"RESOURCE_EXHAUSTED\"}}\n",
        );
        let output = collect_stream(translate_events(events, true)).await;
        assert!(!output.contains("[DONE]"));
        let frames = data_frames(&output);
        let error = frames.last().unwrap();
        assert_eq!(error["error"]["message"], "Quota exceeded");
        assert_eq!(error["error"]["type"], "rate_limit_error");
        assert_eq!(error["error"]["code"], "RESOURCE_EXHAUSTED");
    }

    #[tokio::test]
    async fn stream_prompt_block_finishes_content_filter() {
        let events = "data: {\"promptFeedback\":{\"blockReason\":\"SAFETY\"},\"usageMetadata\":{\"promptTokenCount\":5,\"totalTokenCount\":5}}\n";
        let output = collect_stream(translate_events(events, true)).await;
        assert!(output.ends_with("data: [DONE]\n\n"));
        let frames = data_frames(&output);
        // role → finish content_filter → usage.
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[1]["choices"][0]["finish_reason"], "content_filter");
        assert_eq!(frames[2]["usage"]["prompt_tokens"], 5);
    }

    #[tokio::test]
    async fn stream_clean_eof_without_finish_terminates_with_error() {
        let events = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hi\"}]}}]}\n";
        let output = collect_stream(translate_events(events, false)).await;
        assert!(output.contains("ended before completion"), "{output}");
        assert!(!output.contains("[DONE]"), "{output}");
    }

    #[tokio::test]
    async fn stream_frames_split_across_chunks() {
        let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
            Ok(Bytes::from("data: {\"candidates\":[{\"content\":{\"par")),
            Ok(Bytes::from("ts\":[{\"text\":\"He")),
            Ok(Bytes::from("llo\"}]}}]}\n\ndata: {\"candidates\":[{\"fin")),
            Ok(Bytes::from("ishReason\":\"MAX_TOKENS\"}]}\n\n")),
        ];
        let translator = GeminiChatSseTranslator::new(
            Box::pin(futures_util::stream::iter(chunks)),
            "gemini-2.5-flash".to_string(),
            false,
        );
        let output = collect_stream(translator).await;
        let frames = data_frames(&output);
        assert_eq!(frames.len(), 3);
        assert_eq!(
            frames[1]["choices"][0]["delta"],
            json!({"content": "Hello"})
        );
        assert_eq!(frames[2]["choices"][0]["finish_reason"], "length");
    }

    #[tokio::test]
    async fn stream_malformed_frame_terminates_with_error() {
        let events = concat!(
            "data: {not json}\n",
            "\n",
            "data: {\"candidates\":[{\"finishReason\":\"STOP\"}]}\n",
        );
        let output = collect_stream(translate_events(events, false)).await;
        let frames = data_frames(&output);
        assert_eq!(
            frames.last().unwrap()["error"]["code"],
            "protocol_error",
            "{output}"
        );
        assert!(!output.contains("[DONE]"), "{output}");
    }

    #[tokio::test]
    async fn stream_transport_error_terminates_with_error_frame() {
        let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
            Ok(Bytes::from(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"He\"}]}}]}\n\n",
            )),
            Err(std::io::Error::other("reset")),
        ];
        let translator = GeminiChatSseTranslator::new(
            Box::pin(futures_util::stream::iter(chunks)),
            "gemini-2.5-flash".to_string(),
            false,
        );
        let output = collect_stream(translator).await;
        assert!(!output.contains("[DONE]"));
        let frames = data_frames(&output);
        assert_eq!(
            frames.last().unwrap()["error"]["message"],
            "The provider stream was interrupted."
        );
    }

    #[tokio::test]
    async fn final_unterminated_finish_frame_and_unknown_fields_are_accepted() {
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\",\"futurePartField\":true}]},\"futureCandidateField\":1}],\"futureTopLevelField\":{}}\n\n",
            "data: {\"candidates\":[{\"finishReason\":\"STOP\"}]}"
        );
        let output = collect_stream(translate_events(events, false)).await;
        assert!(output.contains("\"content\":\"ok\""), "{output}");
        assert!(output.ends_with("data: [DONE]\n\n"), "{output}");
    }
}
