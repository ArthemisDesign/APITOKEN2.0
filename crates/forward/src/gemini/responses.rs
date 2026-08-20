//! Universal Responses → Gemini generateContent адаптер — этап 4.3
//! docs/engine/UNIFIED_ROUTER.md (решения 1–5). Gemini-зеркало Anthropic-
//! адаптера этапов 4.1–4.2 (`crate::anthropic_responses`): Responses-сторона
//! словаря (item-формы, события SSE, usage, status/incomplete_details)
//! совпадает и закреплена contract-тестами обоих модулей; перевод запроса и
//! разбор GenerateContentResponse — по правилам chat-адаптера этой плоскости
//! (`chat.rs`, этапы 3.3–3.4b), общие хелперы вынесены там в `pub(crate)`.
//!
//! `POST /v1/responses` на Gemini-плоскости. Поток запроса повторяет
//! chat-адаптер: парс Responses-запроса → перевод в GenerateContentRequest
//! JSON → внутренний `Request` на
//! `/v1beta/models/{model}:generateContent|streamGenerateContent?alt=sse` →
//! общий [`gemini_api`] (admission, reserve, affinity, ротация, Code Assist
//! wrapper, usage-settlement — без единого изменения) → перевод ответа:
//! GenerateContentResponse (JSON либо data-only SSE) → Response object /
//! Responses SSE. Router (`crates/router/src/responses.rs`) выполняет
//! model-based dispatch `google/*` и gemini-alias'ов и проксирует тело без
//! изменений (router не менялся с 4.1).
//!
//! Перевод запроса: `instructions` и system/developer items →
//! `systemInstruction` (text-парт на каждый, instructions первым); `input`
//! строка → один user-content, массив items → contents со склейкой
//! одноролевых (общий `merge_or_push`): message item — `{type:"message", …}`
//! или компактная форма `{role, content}` без type; content parts
//! `input_text`/`output_text` → text-парты (склейка через \n), `input_image`
//! → inlineData-парт общим с chat-адаптером переводом (только data: URL —
//! generateContent внешние http(s) ссылки не принимает → честный
//! `400 invalid_request`; `detail` != auto → `400 unsupported_parameter`);
//! replay tool-истории (по образцу 4.2): function_call item → model-content с
//! functionCall-партом (`arguments` JSON-строка парсится в `args` общим
//! `parse_tool_arguments` — отсутствующая/пустая строка `{}`, невалидный JSON
//! и не-object → `400 invalid_request`), function_call_output item →
//! user-content с functionResponse-партом (output строка → общий
//! `function_response_value`: JSON разбирается и заворачивается в
//! `{"result": ...}`, не-JSON — строкой; массив text-партов склеивается
//! через \n, нетекстовые части → 400). Отличие от Anthropic-зеркала:
//! functionResponse ссылается на вызов по ИМЕНИ (functionCall.id на private
//! wire нет), поэтому pairing валидируется адаптером — карта call_id→name по
//! function_call items этой же истории, function_call_output без пары →
//! `400 invalid_request` (как `tool_call_id` без пары в chat-адаптере 3.3).
//! Responses `tools` → `[{"functionDeclarations": …}]` (плоский
//! function-дескриптор → общий `function_declaration`; `strict` снимается;
//! не-function tool → `400 unsupported_parameter`); `tool_choice`
//! auto/required/none/именная функция (плоская форма `{type:"function",
//! name}`) → `toolConfig.functionCallingConfig`; `max_output_tokens` →
//! `generationConfig.maxOutputTokens` только при явном cap; omission оставляет
//! поле отсутствующим до общего model-limit/balance admission;
//! `reasoning.effort` → `generationConfig.thinkingConfig`
//! (`thinkingLevel` проксируется как есть — minimal НЕ клампится, плоскость
//! сама мапит уровень в wire model id; `includeThoughts: true`; невалидное
//! значение → `400 invalid_request`) — как 3.4b chat-адаптера;
//! `text.format` json_schema → `responseMimeType: application/json` +
//! `responseSchema` (обёртка снимается), json_object → `responseMimeType`
//! (у generateContent есть — отличие от Messages, где json_object → 400),
//! не-дефолтная verbosity → `400 unsupported_parameter`. Tool/structured-output
//! schemas проходят общий Code Assist sanitizer, а replayed functionCall-парты
//! получают stateless context-engineering `thoughtSignature` marker; реальные
//! opaque signatures в публичный Responses-контракт по-прежнему не выходят.
//!
//! Capability matrix (решение 3) — те же 9 правил, что у Anthropic-зеркала
//! (`background`, `service_tier`, `truncation`, `include`, `prompt_cache_key`,
//! `safety_identifier`, `user`, `metadata`, `max_tool_calls` с не-дефолтом →
//! `400 unsupported_parameter`), плюс `parallel_tool_calls` (generateContent
//! не умеет ограничивать параллельные вызовы — только дефолт true, как
//! chat-адаптер 3.3). НЕИЗВЕСТНЫЕ top-level поля тоже отклоняются (закрытый
//! список, отличие от Anthropic-плоскости): Code Assist wrapper пропускает
//! только известные поля GenerateContentRequest, поэтому «проксирование»
//! было бы молчаливым выбрасыванием.
//!
//! Временные ограничения (как у Anthropic-зеркала после 4.2;
//! задокументированы в UNIFIED_ROUTER.md, п. 4):
//! - `reasoning` items во входе принимаются и выбрасываются (подписи и
//!   encrypted content в universal lanes не выставляются — решение 4,
//!   реплеить нечего);
//! - `store:true`, `previous_response_id` и `item_reference` →
//!   `400 documented_limitation` (stored responses — только `openai/*`,
//!   решение 5); `POST /v1/responses/input_tokens` остаётся openai-only и
//!   этим адаптером не обслуживается.
//!
//! Перевод ответа — словарь 4.1+4.2, общий с Anthropic-зеркалом. Non-stream:
//! Response object `{id: "resp_*", object: "response", created_at, status,
//! model, output, usage, error, incomplete_details}`; text-парты кандидата
//! склеиваются в ОДИН message item с одним output_text part на позиции
//! первого text-парта (без текста item не создаётся), thought-парты →
//! reasoning items (`rs_*`, один `summary_text` part на thought-парт с
//! непустым текстом; thoughtSignature-only парт пропускается — решение 4),
//! functionCall-парты → function_call items (`fc_*`, arguments — JSON-строка
//! `args`; `call_id` синтезируется `callu_<name>[_N]` — на private wire
//! functionCall.id не приезжает, схема общая с chat-адаптером),
//! сгенерированные `inlineData`-парты (image MIME) → output_image items
//! (`img_*`, image_url — data URL); usage —
//! `promptTokenCount` → input, `candidatesTokenCount`+`thoughtsTokenCount` →
//! output (та же сумма, что тарифицирует metering), `cachedContentTokenCount`
//! → `input_tokens_details.cached_tokens`, `thoughtsTokenCount` →
//! `output_tokens_details.reasoning_tokens`; finishReason/blockReason →
//! status: MAX_TOKENS → incomplete `max_output_tokens`, SAFETY/RECITATION/
//! BLOCKLIST/PROHIBITED_CONTENT/SPII → incomplete `content_filter`, остальное
//! → completed (через общий `map_finish_reason` chat-адаптера). Stream
//! (GenerateContentResponse data-only SSE → Responses SSE, транслятор
//! СНАРУЖИ `gemini_api()` как у chat-адаптера):
//! - первый client-visible кадр → `response.created` + `response.in_progress`
//!   (shell: status "in_progress", output [], error/incomplete_details null);
//! - text-парты → message item lifecycle словаря: `response.output_item.added`
//!   (message item, content []) → `response.content_part.added` (output_text
//!   part, content_index 0) → `response.output_text.delta`* →
//!   `response.output_text.done` → `response.content_part.done` →
//!   `response.output_item.done`;
//! - thought-парты → reasoning item lifecycle словаря 4.2:
//!   `response.output_item.added` (reasoning item, summary []) →
//!   `response.reasoning_summary_part.added` (summary_index 0, пустой
//!   summary_text part) → `response.reasoning_summary_text.delta`* (парт с
//!   одним thoughtSignature дельт не порождает — решение 4) →
//!   `response.reasoning_summary_text.done` →
//!   `response.reasoning_summary_part.done` → `response.output_item.done`;
//! - functionCall-парт приходит целиком (arguments-дельт на wire нет) →
//!   `response.output_item.added` (function_call item, arguments "") → ровно
//!   одна `response.function_call_arguments.delta` с полной строкой →
//!   `response.function_call_arguments.done` → `response.output_item.done`;
//! - сгенерированные `inlineData`-парты (image MIME) → output_image items
//!   (`response.output_item.added` → `response.output_item.done` с data URL —
//!   billed media доставляется клиенту, а не теряется после settlement;
//!   не-image inline-медиа не имеет OpenAI-представления и не фабрикуется);
//! - смена типа контента закрывает открытый item (done-события) — text и
//!   thought вперемешку дают серию items в порядке апстрима; output_index —
//!   плотный собственный счётчик, content_index всегда 0, item-scoped
//!   события несут `item_id`; каждый data object несёт `type` и монотонный
//!   `sequence_number`, lifecycle events оборачивают Response object в
//!   поле `response`;
//! - нормальное завершение Gemini-стрима — finishReason/blockReason + чистый EOF
//!   (message_stop на wire нет): открытый item закрывается и эмитится `response.completed` с полным
//!   Response object (status по сохранённому finishReason/blockReason, usage
//!   из последнего usageMetadata — в Responses include-флага нет, usage
//!   всегда в completed). EOF без terminal evidence → `response.failed`;
//! - mid-stream error-кадр плоскости `{error:{code,message,status}}` и
//!   транспортный сбой → OpenAI `error` event, затем `response.failed`
//!   (status "failed", error {code: google.rpc status, message}) и завершение
//!   стрима; malformed known provider shape проходит тот же failure lifecycle,
//!   неизвестные дополнительные поля допускаются.
//!
//! Все ответы этого пути — OpenAI-совместимый конверт, включая ошибки:
//! синтетические ошибки плоскости и пасsthrough-ошибки апстрима переводятся
//! общим с chat-адаптером `convert_error_response` с сохранением HTTP-статуса
//! (402 LowBalance тоже) и `Retry-After`; нативный `400 API_KEY_INVALID` →
//! `401 authentication_error`.

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

use super::chat::{
    chat_error, code_assist_schema, convert_error_response, function_declaration,
    function_response_value, gemini_image_part, image_url_part, invalid_request, map_finish_reason,
    merge_or_push, parse_tool_arguments, replayed_function_call_part, synthetic_call_id,
    translate_reasoning_effort, unsupported_parameter, CHAT_BODY_LIMIT, RESPONSE_BODY_LIMIT,
};
use super::gemini_api;
use crate::codex::new_id;
use crate::gemini_stream::GeminiStreamState;
use crate::openai_responses_stream::ResponsesEventEncoder;
use crate::proxy::{read_body_limited, without_not_started, BodyReadError};
use crate::state::AppState;
use crate::validation::{optional_bool, optional_positive_u64};

/// Хендлер `POST /v1/responses` (роут регистрируется в server только в
/// `ProviderMode::Gemini`). Точный паттерн `gemini_chat_completions`.
pub async fn gemini_responses(
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
                "invalid_responses_request",
            )
        }
        Err(BodyReadError::Read) => {
            return chat_error(
                StatusCode::BAD_REQUEST,
                "Could not read request body.",
                None,
                Value::Null,
                "invalid_responses_request",
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
                "invalid_responses_request",
            )
        }
    };
    let translated = match translate_responses_request(value) {
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
    crate::execution::inherit_request_context(&parts.extensions, inner.extensions_mut());
    inner
        .extensions_mut()
        .insert(super::api::SuppressedPendingUniversalGeneration);
    let upstream = gemini_api(State(app), ConnectInfo(peer), inner).await;

    if upstream.status() != StatusCode::OK {
        return convert_error_response(upstream).await;
    }
    if translated.stream {
        stream_responses_response(upstream, translated.model)
    } else {
        json_responses_response(upstream, translated.model).await
    }
}

/// Результат перевода Responses-запроса: тело GenerateContentRequest и
/// параметры, нужные для перевода ответа.
#[derive(Debug)]
struct Translated {
    body: Value,
    /// Запрошенная модель с уже снятым `google/`-префиксом — фолбэк для поля
    /// `model` ответа, если плоскость не вернула `modelVersion`.
    model: String,
    stream: bool,
}

/// Перевод Responses-запроса в GenerateContentRequest JSON. Ошибки — готовые
/// OpenAI-shaped ответы (400).
fn translate_responses_request(value: Value) -> Result<Translated, Response> {
    let mut object = match value {
        Value::Object(object) => object,
        _ => {
            return Err(chat_error(
                StatusCode::BAD_REQUEST,
                "Request body must be a JSON object.",
                None,
                Value::Null,
                "invalid_responses_request",
            ))
        }
    };

    check_capability_matrix(&object)?;
    check_stored_limitations(&object)?;

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

    // `instructions` → первый text-парт systemInstruction; system/developer
    // items из input сливаются следом (порядок входа сохраняется).
    let mut system_parts = Vec::new();
    match object.remove("instructions") {
        None | Some(Value::Null) => {}
        Some(Value::String(text)) => {
            if !text.is_empty() {
                system_parts.push(json!({"text": text}));
            }
        }
        _ => {
            return Err(invalid_request(
                "Invalid type for parameter: instructions must be a string.",
                Some("instructions"),
            ))
        }
    }

    let contents = match object.remove("input") {
        Some(Value::String(text)) => {
            if text.is_empty() {
                return Err(invalid_request(
                    "Missing or invalid required parameter: input.",
                    Some("input"),
                ));
            }
            vec![json!({"role": "user", "parts": [{"text": text}]})]
        }
        Some(Value::Array(items)) => translate_input_items(items, &mut system_parts)?,
        _ => {
            return Err(invalid_request(
                "Missing or invalid required parameter: input.",
                Some("input"),
            ))
        }
    };

    let stream = optional_bool(&object, "stream")
        .map_err(|field| invalid_request("stream must be a boolean.", Some(field)))?
        .unwrap_or(false);

    let max_tokens = optional_positive_u64(&object, &["max_output_tokens"]).map_err(|field| {
        invalid_request("max_output_tokens must be a positive integer.", Some(field))
    })?;

    let mut generation_config = Map::new();
    if let Some(max_tokens) = max_tokens {
        generation_config.insert("maxOutputTokens".to_string(), Value::from(max_tokens));
    }
    // Honored-параметры с совпадающими именами.
    for (responses_key, native_key) in [("temperature", "temperature"), ("top_p", "topP")] {
        if let Some(value) = object.get(responses_key).filter(|v| !v.is_null()) {
            generation_config.insert(native_key.to_string(), value.clone());
        }
    }
    // Structured output: text.format → responseMimeType (+responseSchema для
    // json_schema) — как `response_format` chat-адаптера (3.4a).
    if let Some((mime, schema)) = translate_text_format(object.get("text"))? {
        generation_config.insert("responseMimeType".to_string(), Value::String(mime));
        if let Some(schema) = schema {
            generation_config.insert("responseSchema".to_string(), schema);
        }
    }
    // Reasoning: reasoning.effort → thinkingConfig (как reasoning_effort
    // chat-адаптера 3.4b) — соседнее поле того же generationConfig,
    // responseMimeType/responseSchema не затираются.
    if let Some(level) = translate_reasoning(object.get("reasoning"))? {
        generation_config.insert(
            "thinkingConfig".to_string(),
            json!({"thinkingLevel": level, "includeThoughts": true}),
        );
    }

    let mut body = Map::new();
    body.insert("contents".to_string(), Value::Array(contents));
    if !system_parts.is_empty() {
        body.insert(
            "systemInstruction".to_string(),
            json!({"parts": system_parts}),
        );
    }
    body.insert(
        "generationConfig".to_string(),
        Value::Object(generation_config),
    );
    // Tools: Responses `tools[]` → `[{"functionDeclarations": …}]`;
    // tool_choice → toolConfig. Пустой список инструментов и дефолтный
    // tool_choice=auto в тело не вставляются (generateContent и так AUTO).
    if let Some(tools) = object.get("tools").filter(|v| !v.is_null()) {
        let declarations = translate_responses_tools(tools)?;
        if !declarations.is_empty() {
            body.insert(
                "tools".to_string(),
                json!([{"functionDeclarations": declarations}]),
            );
        }
    }
    if let Some(config) = translate_responses_tool_choice(&object)? {
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
    })
}

/// Известные Responses-параметры: honored (переводятся), stored limitations
/// или matrix (отклоняются при не-дефолте). Всё вне списка —
/// `400 unsupported_parameter` (закрытый список, см. translate_responses_request).
const KNOWN_KEYS: &[&str] = &[
    "model",
    "input",
    "instructions",
    "max_output_tokens",
    "stream",
    "temperature",
    "top_p",
    "tools",
    "tool_choice",
    "parallel_tool_calls",
    "reasoning",
    "text",
    // Stored limitations (решение 5): отклонены при не-дефолте, при дефолте —
    // сняты.
    "store",
    "previous_response_id",
    // Capability matrix: отклонены при не-дефолте, при дефолте — сняты.
    "background",
    "service_tier",
    "truncation",
    "include",
    "prompt_cache_key",
    "safety_identifier",
    "user",
    "metadata",
    "max_tool_calls",
];

/// Capability matrix (решение 3): те же 9 правил, что у Anthropic-зеркала
/// (значение не по умолчанию → `400 unsupported_parameter`), плюс
/// `parallel_tool_calls` — generateContent не умеет ограничивать параллельные
/// вызовы (как chat-адаптер 3.3; у Anthropic-зеркала false переводится в
/// `disable_parallel_tool_use`). Порядок правил определяет, какой параметр
/// назовёт ошибка.
fn check_capability_matrix(object: &Map<String, Value>) -> Result<(), Response> {
    let rules: [(&str, fn(&Value) -> bool); 10] = [
        ("background", |v| v.is_null() || v.as_bool() == Some(false)),
        ("service_tier", |v| {
            v.is_null() || v.as_str() == Some("auto") || v.as_str() == Some("default")
        }),
        ("truncation", |v| {
            v.is_null() || v.as_str() == Some("disabled")
        }),
        ("include", |v| {
            v.is_null() || v.as_array().is_some_and(Vec::is_empty)
        }),
        ("prompt_cache_key", |v| v.is_null()),
        ("safety_identifier", |v| v.is_null()),
        ("user", |v| v.is_null()),
        ("metadata", |v| v.is_null()),
        ("max_tool_calls", |v| v.is_null()),
        ("parallel_tool_calls", |v| v.as_bool() == Some(true)),
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

/// Решение 5: stored responses — только `openai/*`. На Gemini-плоскости
/// `store:true` и ссылка на сохранённый ответ — честный
/// `400 documented_limitation`, а не молчаливая потеря семантики (как у
/// Anthropic-зеркала 4.1).
fn check_stored_limitations(object: &Map<String, Value>) -> Result<(), Response> {
    if object.get("store").and_then(Value::as_bool) == Some(true) {
        return Err(documented_limitation(
            "store",
            "Stored responses are supported only for openai/* models.",
        ));
    }
    if object
        .get("previous_response_id")
        .is_some_and(|v| !v.is_null())
    {
        return Err(documented_limitation(
            "previous_response_id",
            "Stored responses are supported only for openai/* models.",
        ));
    }
    Ok(())
}

/// Задокументированное ограничение universal lane (решение 5): отличается от
/// `unsupported_parameter` тем, что возможность существует в протоколе, но
/// сознательно не реализуется на этой плоскости.
fn documented_limitation(param: &str, message: &str) -> Response {
    chat_error(
        StatusCode::BAD_REQUEST,
        message,
        Some(param),
        Value::String("documented_limitation".to_string()),
        "documented_limitation",
    )
}

/// Массив Responses input items → Gemini contents (+ system-парты).
/// Message item — `{type:"message", role, content}` либо компактная форма
/// `{role, content}` без type (её шлёт stock SDK). Подряд идущие contents с
/// одинаковой Gemini-ролью склеиваются общим `merge_or_push` (user/model):
/// generateContent ждёт чередования user/model, а серии functionResponse —
/// один user-content.
fn translate_input_items(
    items: Vec<Value>,
    system_parts: &mut Vec<Value>,
) -> Result<Vec<Value>, Response> {
    let mut contents: Vec<Value> = Vec::new();
    // call_id → имя функции: Gemini functionResponse ссылается на вызов по
    // имени (functionCall.id на private wire нет), поэтому pairing — в отличие
    // от Anthropic-зеркала — валидируется адаптером: карта строится по
    // function_call items этой же истории за один проход.
    let mut call_names: HashMap<String, String> = HashMap::new();
    for item in &items {
        let object = item.as_object().ok_or_else(|| {
            invalid_request("Each input item must be a JSON object.", Some("input"))
        })?;
        match object.get("type").and_then(Value::as_str) {
            Some("message") => translate_message_item(object, system_parts, &mut contents)?,
            None if object.contains_key("role") => {
                translate_message_item(object, system_parts, &mut contents)?
            }
            Some("function_call") => {
                let (call_id, name, part) = function_call_part(object)?;
                call_names.insert(call_id, name);
                merge_or_push(&mut contents, "model", vec![part]);
            }
            Some("function_call_output") => {
                let part = function_call_output_part(object, &call_names)?;
                merge_or_push(&mut contents, "user", vec![part]);
            }
            // Подписи/encrypted reasoning в universal lanes не выставляются
            // (решение 4): реплеить нечего — item принимается и выбрасывается.
            Some("reasoning") => {}
            Some("item_reference") => {
                return Err(documented_limitation(
                    "input",
                    "Stored responses are supported only for openai/* models.",
                ))
            }
            _ => {
                return Err(invalid_request(
                    "Invalid input item: expected a message, function_call, function_call_output or reasoning item.",
                    Some("input"),
                ))
            }
        }
    }
    if contents.is_empty() {
        return Err(invalid_request(
            "input must contain at least one user or assistant message.",
            Some("input"),
        ));
    }
    Ok(contents)
}

/// Один message item из input: system/developer → system-парты,
/// user/assistant → content (роли user/model).
fn translate_message_item(
    object: &Map<String, Value>,
    system_parts: &mut Vec<Value>,
    contents: &mut Vec<Value>,
) -> Result<(), Response> {
    let role = object.get("role").and_then(Value::as_str).ok_or_else(|| {
        invalid_request("Each input message must have a string role.", Some("input"))
    })?;
    match role {
        "system" | "developer" => {
            let text = message_item_text(object.get("content"))?;
            if !text.is_empty() {
                system_parts.push(json!({"text": text}));
            }
        }
        "user" | "assistant" => {
            let parts = message_item_parts(object.get("content"))?;
            let role = if role == "assistant" { "model" } else { "user" };
            merge_or_push(contents, role, parts);
        }
        _ => {
            return Err(invalid_request(
                "Invalid input message role: expected user, assistant, system or developer.",
                Some("input"),
            ))
        }
    }
    Ok(())
}

/// Текст system/developer item'а: строка либо массив text-партов (склеиваются
/// через \n, как в chat-адаптере). Нетекстовых частей в systemInstruction
/// нет — 400.
fn message_item_text(content: Option<&Value>) -> Result<String, Response> {
    match content {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Array(parts)) => {
            let mut texts = Vec::with_capacity(parts.len());
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("input_text") | Some("output_text") => {
                        texts.push(
                            part.get("text")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        );
                    }
                    _ => {
                        return Err(invalid_request(
                            "Invalid input message content: system and developer messages accept text parts only.",
                            Some("input"),
                        ))
                    }
                }
            }
            Ok(texts.join("\n"))
        }
        _ => Err(invalid_request(
            "Invalid input message content: expected a string or an array of parts.",
            Some("input"),
        )),
    }
}

/// Контент user/assistant item'а → массив Gemini-партов. Текстовые части
/// (`input_text`, `output_text`) склеиваются в text-парты (через \n, как в
/// chat-адаптере); `input_image` разрывают склейку, порядок частей
/// сохраняется. Остальные part types (`input_file` и будущие) — 400.
fn message_item_parts(content: Option<&Value>) -> Result<Vec<Value>, Response> {
    let Some(Value::Array(parts)) = content else {
        let text = message_item_text(content)?;
        if text.is_empty() {
            // Пустое сообщение бессмысленно и для Responses-входа — честный
            // 400 (как у Anthropic-зеркала и chat-адаптера).
            return Err(invalid_request(
                "Input message content must not be empty.",
                Some("input"),
            ));
        }
        return Ok(vec![json!({"text": text})]);
    };
    let mut out: Vec<Value> = Vec::new();
    let mut text = String::new();
    for part in parts {
        match part.get("type").and_then(Value::as_str) {
            Some("input_text") | Some("output_text") => {
                let segment = part
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !text.is_empty() && !segment.is_empty() {
                    text.push('\n');
                }
                text.push_str(segment);
            }
            Some("input_image") => {
                if !text.is_empty() {
                    out.push(json!({"text": std::mem::take(&mut text)}));
                }
                out.push(input_image_part(part)?);
            }
            _ => {
                return Err(invalid_request(
                    "Invalid input message content part: expected input_text, output_text or input_image.",
                    Some("input"),
                ))
            }
        }
    }
    if !text.is_empty() {
        out.push(json!({"text": text}));
    }
    if out.is_empty() {
        return Err(invalid_request(
            "Input message content must not be empty.",
            Some("input"),
        ));
    }
    Ok(out)
}

/// Responses `input_image` part → inlineData-парт. Форма part'а
/// (`image_url` — строка, `detail` — sibling) адаптируется под chat-форму, и
/// дальше работает общий с chat-адаптером перевод (только data: URL — http(s)
/// generateContent не принимает → `400 invalid_request`; `detail` != auto →
/// `400 unsupported_parameter`).
fn input_image_part(part: &Value) -> Result<Value, Response> {
    let url = part
        .get("image_url")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            invalid_request(
                "Invalid input_image part: expected an image_url string.",
                Some("input"),
            )
        })?;
    let mut image = json!({"url": url});
    if let Some(detail) = part.get("detail").and_then(Value::as_str) {
        image["detail"] = Value::String(detail.to_string());
    }
    gemini_image_part(&json!({"type": "image_url", "image_url": image}), "input")
}

/// Replay function_call item'а (по образцу 4.2 Anthropic-зеркала) →
/// functionCall-парт model-content'а: `arguments` (JSON-строка) парсится в
/// `args` общим `parse_tool_arguments` (отсутствующая/пустая строка — `{}`,
/// невалидный JSON и не-object → `400 invalid_request`). Отсутствующие и
/// пустые `call_id`/`name` — 400. Возвращает (call_id, name, part): call_id
/// регистрируется в карте для functionResponse (Gemini ссылается по имени,
/// а не по id — отличие от tool_use Messages).
fn function_call_part(object: &Map<String, Value>) -> Result<(String, String, Value), Response> {
    let call_id = required_string(object, "call_id")?.to_string();
    let name = required_string(object, "name")?.to_string();
    let args = parse_tool_arguments(object.get("arguments"), "input")?;
    let part = replayed_function_call_part(&name, args);
    Ok((call_id, name, part))
}

/// Обязательное непустое строковое поле replay item'а.
fn required_string<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str, Response> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            invalid_request(
                &format!("Invalid input item: expected a non-empty {field} string."),
                Some("input"),
            )
        })
}

/// Replay function_call_output item'а → functionResponse-парт user-content'а:
/// имя восстанавливается по call_id из карты, построенной function_call
/// items этой же истории (как восстановление имени по tool_call_id в
/// chat-адаптере 3.3); output — общий `function_response_value` (JSON
/// разбирается, не-JSON заворачивается строкой).
fn function_call_output_part(
    object: &Map<String, Value>,
    call_names: &HashMap<String, String>,
) -> Result<Value, Response> {
    let call_id = required_string(object, "call_id")?;
    let name = call_names.get(call_id).ok_or_else(|| {
        invalid_request(
            "Function_call_output call_id has no matching function_call in this history.",
            Some("input"),
        )
    })?;
    let text = function_call_output_text(object.get("output"))?;
    Ok(json!({
        "functionResponse": {"name": name, "response": function_response_value(&text)}
    }))
}

/// `output` function_call_output item'а: строка либо массив text-партов
/// (`input_text`/`output_text`, склеиваются через \n). Нетекстовые части —
/// 400 (идентично Anthropic-зеркалу 4.2).
fn function_call_output_text(output: Option<&Value>) -> Result<String, Response> {
    match output {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Array(parts)) => {
            let mut texts = Vec::with_capacity(parts.len());
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("input_text") | Some("output_text") => {
                        texts.push(
                            part.get("text")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        );
                    }
                    _ => {
                        return Err(invalid_request(
                            "Invalid function_call_output output: expected text parts only.",
                            Some("input"),
                        ))
                    }
                }
            }
            Ok(texts.join("\n"))
        }
        _ => Err(invalid_request(
            "Invalid function_call_output output: expected a string or an array of parts.",
            Some("input"),
        )),
    }
}

/// Responses `tools[]` → массив functionDeclarations. Поддерживается только
/// function tool: дескриптор в Responses-форме плоский (`{type:"function",
/// name, description, parameters, strict?}` — сам item и есть дескриптор),
/// перевод — общий с chat-адаптером `function_declaration` (`strict`
/// снимается). Любой другой tool type (custom, web_search, file_search, mcp,
/// …) → `400 unsupported_parameter` (как у Anthropic-зеркала).
fn translate_responses_tools(value: &Value) -> Result<Vec<Value>, Response> {
    let tools = value.as_array().ok_or_else(|| {
        invalid_request(
            "Invalid type for parameter: tools must be an array.",
            Some("tools"),
        )
    })?;
    let mut declarations = Vec::with_capacity(tools.len());
    for (index, tool) in tools.iter().enumerate() {
        match tool.get("type").and_then(Value::as_str) {
            Some("function") | None => {}
            Some(_) => return Err(unsupported_parameter("tools")),
        }
        let function = tool.as_object().ok_or_else(|| {
            invalid_request(
                "Invalid entry in parameter: tools must contain function objects.",
                Some("tools"),
            )
        })?;
        declarations.push(function_declaration(
            function,
            "tools",
            &format!("tools.{index}"),
        )?);
    }
    Ok(declarations)
}

/// `tool_choice` → `toolConfig.functionCallingConfig` (как chat-адаптер;
/// именная форма Responses — плоская `{type:"function", name}`).
/// Дефолт (`auto`) не вставляется — generateContent и так AUTO.
/// `parallel_tool_calls: false` сюда не доходит — отклонён capability matrix
/// (у generateContent нет аналога disable_parallel_tool_use).
fn translate_responses_tool_choice(object: &Map<String, Value>) -> Result<Option<Value>, Response> {
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
            let name = named.get("name").and_then(Value::as_str).ok_or_else(|| {
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
        None => None,
    };
    Ok(config.map(|config| json!({"functionCallingConfig": config})))
}

/// `reasoning: {effort}` → `thinkingConfig.thinkingLevel` (общий с
/// chat-адаптером перевод, этап 3.4b): null/отсутствие — выкл, уровень
/// проксируется как есть (minimal НЕ клампится — отличие от Anthropic-
/// зеркала), любое другое не-null значение effort → 400 invalid_request.
fn translate_reasoning(value: Option<&Value>) -> Result<Option<String>, Response> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let Some(effort) = value.get("effort") else {
        return Err(invalid_request(
            "Invalid value for parameter: reasoning (expected an object with effort).",
            Some("reasoning"),
        ));
    };
    translate_reasoning_effort(Some(effort), "reasoning")
}

/// `text` → `(responseMimeType, Option<responseSchema>)` (как
/// `response_format` chat-адаптера 3.4a). Дефолт — `{format:{type:"text"}}`;
/// json_schema переводится снятием обёртки (name/strict/description не
/// проксируются — только схема); json_object у generateContent есть (отличие
/// от Messages, где он → 400) — JSON без схемы; не-дефолтная verbosity
/// (дефолт — "medium") → `400 unsupported_parameter`.
fn translate_text_format(
    value: Option<&Value>,
) -> Result<Option<(String, Option<Value>)>, Response> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    if let Some(verbosity) = value.get("verbosity").filter(|v| !v.is_null()) {
        if verbosity.as_str() != Some("medium") {
            return Err(unsupported_parameter("text"));
        }
    }
    let Some(format) = value.get("format").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    match format.get("type").and_then(Value::as_str) {
        None | Some("text") => Ok(None),
        Some("json_object") => Ok(Some(("application/json".to_string(), None))),
        Some("json_schema") => {
            let schema = format
                .get("schema")
                .filter(|s| s.is_object())
                .ok_or_else(|| {
                    invalid_request(
                        "Invalid text.format: json_schema requires a schema object.",
                        Some("text"),
                    )
                })?;
            Ok(Some((
                "application/json".to_string(),
                Some(code_assist_schema(schema, "text.format.schema")?),
            )))
        }
        Some(_) => Err(unsupported_parameter("text")),
    }
}

/// `status`/`incomplete_details` Response object из Gemini
/// `finishReason`/`blockReason` — через общий с chat-адаптером
/// `map_finish_reason`: length → incomplete `max_output_tokens`,
/// content_filter → incomplete `content_filter`, остальное — completed
/// (зеркало `map_status` Anthropic-адаптера).
fn map_status(finish_reason: Option<&str>) -> (&'static str, Value) {
    match map_finish_reason(finish_reason) {
        "length" => ("incomplete", json!({"reason": "max_output_tokens"})),
        "content_filter" => ("incomplete", json!({"reason": "content_filter"})),
        _ => ("completed", Value::Null),
    }
}

/// Responses `usage` из usageMetadata: input — `promptTokenCount`, output —
/// `candidatesTokenCount` + `thoughtsTokenCount` (та же сумма, что
/// тарифицирует metering в chat-адаптере), cache read отражается в
/// `input_tokens_details.cached_tokens` (только при >0), thoughts — в
/// `output_tokens_details.reasoning_tokens`.
fn map_responses_usage(usage: &Value) -> Value {
    let tokens = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let input = tokens("promptTokenCount");
    let thoughts = tokens("thoughtsTokenCount");
    let output = tokens("candidatesTokenCount").saturating_add(thoughts);
    let total = tokens("totalTokenCount");
    let total = if total > 0 {
        total
    } else {
        input.saturating_add(output)
    };
    let mut mapped = json!({
        "input_tokens": input,
        "output_tokens": output,
        "total_tokens": total,
    });
    let cached = tokens("cachedContentTokenCount");
    if cached > 0 {
        mapped["input_tokens_details"] = json!({"cached_tokens": cached});
    }
    if thoughts > 0 {
        mapped["output_tokens_details"] = json!({"reasoning_tokens": thoughts});
    }
    mapped
}

/// message output item с одним output_text part (форма общая с Anthropic-
/// зеркалом).
fn message_item(text: &str, status: &str) -> Value {
    json!({
        "type": "message",
        "id": new_id("msg"),
        "status": status,
        "role": "assistant",
        "content": [{"type": "output_text", "text": text, "annotations": [], "logprobs": []}],
    })
}

/// function_call output item: `call_id` — синтезированный `callu_<name>[_N]`
/// (на private wire functionCall.id не приезжает — общая с chat-адаптером
/// схема; клиент ссылается на него в function_call_output следующего хода),
/// `id` — свой `fc_*` идентификатор item'а.
fn function_call_item(call_id: &str, name: &str, arguments: &str, status: &str) -> Value {
    json!({
        "type": "function_call",
        "id": new_id("fc"),
        "call_id": call_id,
        "name": name,
        "arguments": arguments,
        "status": status,
    })
}

/// reasoning output item (4.2): один `summary_text` part с текстом
/// thought-парта. encrypted_content не выставляется (решение 4).
fn reasoning_item(text: &str) -> Value {
    json!({
        "type": "reasoning",
        "id": new_id("rs"),
        "summary": [{"type": "summary_text", "text": text}],
    })
}

/// output_image output item: сгенерированный `inlineData` → data URL
/// (`image_uri` не выставляется — публичного URL у нас нет). Billed media
/// доставляется клиенту, а не теряется после settlement.
fn output_image_item(url: &str) -> Value {
    json!({
        "type": "output_image",
        "id": new_id("img"),
        "image_url": url,
    })
}

/// Content-парты кандидата → Responses output items (non-stream): thought-
/// парты с непустым текстом → reasoning items (каждый thought-парт —
/// отдельный item, как thinking-блоки у Anthropic-зеркала; thoughtSignature-
/// only парт пропускается — решение 4), text-парты склеиваются в ОДИН message
/// item (без текста item не создаётся) на позиции первого text-парта,
/// functionCall → function_call items, сгенерированные `inlineData`-парты
/// (image MIME) → output_image items. Items идут в порядке появления
/// партов; неизвестные парты пропускаются.
fn output_items(parts: Option<&Vec<Value>>) -> Vec<Value> {
    let mut output: Vec<Value> = Vec::new();
    let mut text = String::new();
    // Позиция message item — позиция первого text-парта среди остальных items.
    let mut text_at: Option<usize> = None;
    let mut name_counts: HashMap<&str, u64> = HashMap::new();
    for part in parts.into_iter().flatten() {
        // thought-парт: его text — reasoning item, а не message;
        // thoughtSignature всегда выбрасывается (решение 4).
        if part.get("thought").and_then(Value::as_bool) == Some(true) {
            let thought = part.get("text").and_then(Value::as_str).unwrap_or_default();
            if !thought.is_empty() {
                output.push(reasoning_item(thought));
            }
            continue;
        }
        if let Some(t) = part.get("text").and_then(Value::as_str) {
            if text_at.is_none() {
                text_at = Some(output.len());
            }
            text.push_str(t);
            continue;
        }
        if let Some(call) = part.get("functionCall") {
            let name = call.get("name").and_then(Value::as_str).unwrap_or_default();
            let count = name_counts.entry(name).or_insert(0);
            *count += 1;
            let call_id = synthetic_call_id(name, *count);
            let arguments = call
                .get("args")
                .map(|args| args.to_string())
                .unwrap_or_else(|| "{}".to_string());
            output.push(function_call_item(&call_id, name, &arguments, "completed"));
            continue;
        }
        if let Some(inline) = part.get("inlineData") {
            if let Some(url) = image_url_part(inline) {
                output.push(output_image_item(&url));
            }
        }
    }
    if !text.is_empty() {
        match text_at {
            Some(at) => output.insert(at, message_item(&text, "completed")),
            None => output.push(message_item(&text, "completed")),
        }
    }
    output
}

/// Перевод non-stream ответа GenerateContentResponse в Response object
/// (словарь 4.1, общий с Anthropic-зеркалом).
async fn json_responses_response(upstream: Response, requested_model: String) -> Response {
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
    let output = output_items(
        candidate
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(Value::as_array),
    );
    // finishReason кандидата; candidates отсутствуют при блокировке промпта
    // на входе — тогда status берётся из promptFeedback.blockReason
    // (incomplete content_filter с пустым output), как в chat-адаптере.
    let finish = candidate
        .and_then(|c| c.get("finishReason"))
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("promptFeedback")
                .and_then(|f| f.get("blockReason"))
                .and_then(Value::as_str)
        });
    let (status, incomplete_details) = map_status(finish);
    let usage = value
        .get("usageMetadata")
        .map(map_responses_usage)
        .unwrap_or(Value::Null);
    let response_object = json!({
        "id": new_id("resp"),
        "object": "response",
        "created_at": pool::now(),
        "status": status,
        "model": model,
        "output": output,
        "usage": usage,
        "error": Value::Null,
        "incomplete_details": incomplete_details,
    });
    let mut response = axum::Json(response_object).into_response();
    if let Some(request_id) = request_id {
        response.headers_mut().insert("request-id", request_id);
    }
    response
}

type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// Перевод SSE-ответа `gemini_api()` в поток Responses SSE. Транслятор
/// навешивается СНАРУЖИ ответа gemini_api(): usage-settlement внутри
/// плоскости уже протапало оригинальные байты GenerateContentResponse, а
/// mid-stream ошибка приходит санитизированным error-кадром — он переводится
/// в `response.failed` по тому же правилу.
fn stream_responses_response(upstream: Response, requested_model: String) -> Response {
    let request_id = upstream.headers().get("request-id").cloned();
    let stream = upstream
        .into_body()
        .into_data_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let translator = GeminiResponsesSseTranslator::new(Box::pin(stream), requested_model);
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

/// Незакрытый output item в переводе: накопленный text нужен для done-событий
/// и финального Response object. function_call items незакрытых не бывает —
/// functionCall приходит целиком и закрывается на том же кадре.
enum OpenItem {
    Text {
        output_index: u64,
        item_id: String,
        text: String,
    },
    Reasoning {
        output_index: u64,
        item_id: String,
        text: String,
    },
}

/// Потоковый транслятор GenerateContentResponse SSE → Responses SSE (словарь
/// 4.1+4.2 в шапке модуля; кадры плоскости — data-only, без `event:`).
/// Буферизуются только байты одного незакрытого SSE-кадра; готовые события
/// отдаются немедленно (первый чанк не ждёт конца стрима).
struct GeminiResponsesSseTranslator {
    inner: ByteStream,
    buf: BytesMut,
    out: VecDeque<Bytes>,
    events: ResponsesEventEncoder,
    id: String,
    created: i64,
    /// Запрошенная (native) модель — фолбэк, пока/если кадры не сообщили
    /// `modelVersion`.
    requested_model: String,
    served_model: Option<String>,
    /// `response.created`/`response.in_progress` уже отправлены.
    started: bool,
    /// usageMetadata приходит на кадрах нарастающим итогом — в
    /// response.completed уходит последнее значение.
    last_usage: Option<Value>,
    /// finishReason кандидата либо promptFeedback.blockReason — status
    /// финального Response object.
    finish: Option<String>,
    /// Открытый text/reasoning item (закрывается сменой типа контента или
    /// финалом стрима).
    open: Option<OpenItem>,
    /// Плотный счётчик output_index.
    next_output_index: u64,
    /// Финализированные output items — для output в response.completed/failed.
    completed_items: Vec<Value>,
    /// Per-name счётчик синтезируемых call_id — та же схема
    /// `callu_<name>[_N]`, что в non-stream переводе и chat-адаптере.
    name_counts: HashMap<String, u64>,
    source: GeminiStreamState,
    /// Терминальное событие (completed/failed) уже поставлено в `out`.
    finished: bool,
}

impl GeminiResponsesSseTranslator {
    fn new(inner: ByteStream, requested_model: String) -> Self {
        Self {
            inner,
            buf: BytesMut::new(),
            out: VecDeque::new(),
            events: ResponsesEventEncoder::default(),
            id: new_id("resp"),
            created: pool::now(),
            requested_model,
            served_model: None,
            started: false,
            last_usage: None,
            finish: None,
            open: None,
            next_output_index: 0,
            completed_items: Vec::new(),
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

    fn push_event(&mut self, event: &'static str, value: Value) {
        self.out.push_back(self.events.event(event, value));
    }

    fn push_response_event(&mut self, event: &'static str, response: Value) {
        self.out.push_back(self.events.response(event, response));
    }

    /// Response shell для created/in_progress: status in_progress, output [],
    /// usage появляется только в финальном событии.
    fn shell(&self) -> Value {
        json!({
            "id": self.id,
            "object": "response",
            "created_at": self.created,
            "status": "in_progress",
            "model": self.model(),
            "output": [],
            "usage": Value::Null,
            "error": Value::Null,
            "incomplete_details": Value::Null,
        })
    }

    /// Пара начальных событий жизненного цикла — ровно один раз, до первого
    /// клиент-видимого события.
    fn ensure_started(&mut self) {
        if !self.started {
            self.started = true;
            let shell = self.shell();
            self.push_response_event("response.created", shell.clone());
            self.push_response_event("response.in_progress", shell);
        }
    }

    /// Терминальное событие сбоя: response object со status failed и
    /// санитизированной ошибкой апстрима (code — google.rpc status, как в
    /// error-кадре chat-адаптера), затем конец стрима.
    fn push_failed(&mut self, message: &str, code: Option<&str>) {
        if self.finished {
            return;
        }
        self.ensure_started();
        let response = json!({
            "id": self.id,
            "object": "response",
            "created_at": self.created,
            "status": "failed",
            "model": self.model(),
            "output": self.completed_items,
            "usage": Value::Null,
            "error": {"code": code, "message": message},
            "incomplete_details": Value::Null,
        });
        self.push_event(
            "error",
            json!({"code": code, "message": message, "param": Value::Null}),
        );
        self.push_response_event("response.failed", response);
        self.finished = true;
    }

    /// Открыть message item (если ещё не открыт): item.added (content []) →
    /// content_part.added (output_text part, content_index 0). Открытый item
    /// другого типа закрывается done-событиями — смена типа контента Gemini
    /// это граница Responses item'а.
    fn ensure_text_open(&mut self) {
        if matches!(self.open, Some(OpenItem::Text { .. })) {
            return;
        }
        self.ensure_started();
        self.close_open();
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        let item_id = new_id("msg");
        self.push_event(
            "response.output_item.added",
            json!({
                "output_index": output_index,
                "item": {"type": "message", "id": item_id,
                    "status": "in_progress", "role": "assistant",
                    "content": []},
            }),
        );
        self.push_event(
            "response.content_part.added",
            json!({
                "output_index": output_index,
                "item_id": item_id,
                "content_index": 0,
                "part": {"type": "output_text", "text": "",
                    "annotations": [], "logprobs": []},
            }),
        );
        self.open = Some(OpenItem::Text {
            output_index,
            item_id,
            text: String::new(),
        });
    }

    /// Открыть reasoning item (если ещё не открыт): item.added (summary []) →
    /// reasoning_summary_part.added (summary_index 0, пустой summary_text
    /// part) — словарь 4.2.
    fn ensure_reasoning_open(&mut self) {
        if matches!(self.open, Some(OpenItem::Reasoning { .. })) {
            return;
        }
        self.ensure_started();
        self.close_open();
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        let item_id = new_id("rs");
        self.push_event(
            "response.output_item.added",
            json!({
                "output_index": output_index,
                "item": {"type": "reasoning", "id": item_id,
                    "summary": []},
            }),
        );
        self.push_event(
            "response.reasoning_summary_part.added",
            json!({
                "output_index": output_index,
                "item_id": item_id,
                "summary_index": 0,
                "part": {"type": "summary_text", "text": ""},
            }),
        );
        self.open = Some(OpenItem::Reasoning {
            output_index,
            item_id,
            text: String::new(),
        });
    }

    /// Закрыть открытый item done-событиями и зафиксировать его в
    /// completed_items (для output финального Response object).
    fn close_open(&mut self) {
        match self.open.take() {
            Some(OpenItem::Text {
                output_index,
                item_id,
                text,
            }) => {
                self.push_event(
                    "response.output_text.done",
                    json!({
                        "output_index": output_index,
                        "item_id": item_id,
                        "content_index": 0,
                        "text": text,
                        "logprobs": [],
                    }),
                );
                self.push_event(
                    "response.content_part.done",
                    json!({
                        "output_index": output_index,
                        "item_id": item_id,
                        "content_index": 0,
                        "part": {"type": "output_text", "text": text,
                            "annotations": [], "logprobs": []},
                    }),
                );
                let item = json!({
                    "type": "message", "id": item_id, "status": "completed",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": text,
                        "annotations": [], "logprobs": []}],
                });
                self.push_event(
                    "response.output_item.done",
                    json!({"output_index": output_index, "item": item}),
                );
                self.completed_items.push(item);
            }
            Some(OpenItem::Reasoning {
                output_index,
                item_id,
                text,
            }) => {
                self.push_event(
                    "response.reasoning_summary_text.done",
                    json!({
                        "output_index": output_index,
                        "item_id": item_id,
                        "summary_index": 0,
                        "text": text,
                    }),
                );
                self.push_event(
                    "response.reasoning_summary_part.done",
                    json!({
                        "output_index": output_index,
                        "item_id": item_id,
                        "summary_index": 0,
                        "part": {"type": "summary_text", "text": text},
                    }),
                );
                let item = json!({
                    "type": "reasoning", "id": item_id,
                    "summary": [{"type": "summary_text", "text": text}],
                });
                self.push_event(
                    "response.output_item.done",
                    json!({"output_index": output_index, "item": item}),
                );
                self.completed_items.push(item);
            }
            None => {}
        }
    }

    /// functionCall-парт целиком (arguments-дельт на wire нет): item.added
    /// (arguments "") → ровно одна arguments.delta с полной строкой →
    /// arguments.done → item.done. Открытый text/reasoning item закрывается.
    fn push_function_call(&mut self, call_id: &str, name: &str, arguments: &str) {
        self.ensure_started();
        self.close_open();
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        let item_id = new_id("fc");
        self.push_event(
            "response.output_item.added",
            json!({
                "output_index": output_index,
                "item": {"type": "function_call", "id": item_id,
                    "call_id": call_id, "name": name,
                    "arguments": "", "status": "in_progress"},
            }),
        );
        self.push_event(
            "response.function_call_arguments.delta",
            json!({
                "output_index": output_index,
                "item_id": item_id,
                "delta": arguments,
            }),
        );
        self.push_event(
            "response.function_call_arguments.done",
            json!({
                "output_index": output_index,
                "item_id": item_id,
                "arguments": arguments,
            }),
        );
        let item = json!({
            "type": "function_call", "id": item_id, "call_id": call_id,
            "name": name, "arguments": arguments, "status": "completed",
        });
        self.push_event(
            "response.output_item.done",
            json!({"output_index": output_index, "item": item}),
        );
        self.completed_items.push(item);
    }

    /// Сгенерированный inlineData-парт (image MIME) → output_image item:
    /// item.added → item.done (дельт у изображения нет — data URL приходит
    /// целиком). Открытый text/reasoning item закрывается.
    fn push_output_image(&mut self, url: &str) {
        self.ensure_started();
        self.close_open();
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        let item = json!({
            "type": "output_image",
            "id": new_id("img"),
            "image_url": url,
        });
        self.push_event(
            "response.output_item.added",
            json!({"output_index": output_index, "item": item}),
        );
        self.push_event(
            "response.output_item.done",
            json!({"output_index": output_index, "item": item}),
        );
        self.completed_items.push(item);
    }

    /// Терминальное событие успеха после provider finishReason/blockReason + EOF:
    /// закрывается открытый item и эмитится
    /// `response.completed` с полным Response object (status по сохранённому
    /// finishReason/blockReason, usage из последнего usageMetadata).
    fn push_completed(&mut self) {
        if self.finished {
            return;
        }
        self.ensure_started();
        self.close_open();
        let (status, incomplete_details) = map_status(self.finish.as_deref());
        let usage = self
            .last_usage
            .take()
            .map(|usage| map_responses_usage(&usage))
            .unwrap_or(Value::Null);
        let response = json!({
            "id": self.id,
            "object": "response",
            "created_at": self.created,
            "status": status,
            "model": self.model(),
            "output": self.completed_items,
            "usage": usage,
            "error": Value::Null,
            "incomplete_details": incomplete_details,
        });
        self.push_response_event("response.completed", response);
        self.finished = true;
    }

    /// Один data-кадр GenerateContentResponse. Порядок важен: modelVersion и
    /// usageMetadata фиксируются ДО эмиссии событий, чтобы события кадра уже
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
            self.push_failed(message, code);
            return;
        }
        if let Some(model) = data.get("modelVersion").and_then(Value::as_str) {
            self.served_model = Some(model.to_string());
        }
        if let Some(usage) = data.get("usageMetadata") {
            self.last_usage = Some(usage.clone());
        }
        // Блокировка промпта на входе: кандидатов не будет, status финального
        // объекта берётся из blockReason (incomplete content_filter с пустым
        // output) — как finish-чанк content_filter у chat-адаптера.
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
                // thought-парт (этап 3.4b): его text — reasoning-дельта, а не
                // content. thoughtSignature выбрасывается (решение 4): парт с
                // одним thoughtSignature видимого события не порождает.
                if part.get("thought").and_then(Value::as_bool) == Some(true) {
                    if let Some(segment) = part
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|t| !t.is_empty())
                    {
                        self.ensure_reasoning_open();
                        let Some(OpenItem::Reasoning {
                            output_index,
                            item_id,
                            text,
                        }) = &mut self.open
                        else {
                            continue;
                        };
                        text.push_str(segment);
                        let (output_index, item_id) = (*output_index, item_id.clone());
                        self.push_event(
                            "response.reasoning_summary_text.delta",
                            json!({
                                "output_index": output_index,
                                "item_id": item_id,
                                "summary_index": 0,
                                "delta": segment,
                            }),
                        );
                    }
                    continue;
                }
                if let Some(segment) = part
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                {
                    self.ensure_text_open();
                    let Some(OpenItem::Text {
                        output_index,
                        item_id,
                        text,
                    }) = &mut self.open
                    else {
                        continue;
                    };
                    text.push_str(segment);
                    let (output_index, item_id) = (*output_index, item_id.clone());
                    self.push_event(
                        "response.output_text.delta",
                        json!({
                            "output_index": output_index,
                            "item_id": item_id,
                            "content_index": 0,
                            "delta": segment,
                            "logprobs": [],
                        }),
                    );
                    continue;
                }
                // Gemini присылает functionCall целиком (arguments-дельт на
                // wire нет) → item lifecycle с одной дельтой.
                if let Some(call) = part.get("functionCall") {
                    let name = call.get("name").and_then(Value::as_str).unwrap_or_default();
                    let count = self.name_counts.entry(name.to_string()).or_insert(0);
                    *count += 1;
                    let call_id = synthetic_call_id(name, *count);
                    let arguments = call
                        .get("args")
                        .map(|args| args.to_string())
                        .unwrap_or_else(|| "{}".to_string());
                    self.push_function_call(&call_id, name, &arguments);
                    continue;
                }
                // Сгенерированный inlineData (image MIME) → output_image item.
                if let Some(inline) = part.get("inlineData") {
                    if let Some(url) = image_url_part(inline) {
                        self.push_output_image(&url);
                    }
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
                Err(()) => self.push_failed(
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

impl Stream for GeminiResponsesSseTranslator {
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
                    self.push_failed("The provider stream was interrupted.", None);
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
                        self.push_failed(
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    fn ok_translated(value: Value) -> Translated {
        translate_responses_request(value).expect("translation must succeed")
    }

    async fn err_parts(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn expect_err(value: Value) -> (StatusCode, Value) {
        err_parts(translate_responses_request(value).unwrap_err()).await
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

    /// (event, payload) каждого SSE-кадра ответа. Заодно проверяет публичный
    /// wire-контракт: data.type совпадает с SSE event, sequence_number идёт
    /// строго без пропусков. Для lifecycle events возвращает вложенный
    /// Response object, чтобы assertions ниже читались так же, как раньше.
    fn event_frames(output: &str) -> Vec<(String, Value)> {
        let mut expected_sequence = 0_u64;
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
                let data: Value = serde_json::from_str(&data).expect("data is JSON");
                assert_eq!(data["type"], event, "event type mismatch in {frame}");
                assert_eq!(
                    data["sequence_number"], expected_sequence,
                    "event sequence mismatch in {frame}"
                );
                expected_sequence += 1;
                let payload = if matches!(
                    event.as_str(),
                    "response.created"
                        | "response.in_progress"
                        | "response.completed"
                        | "response.failed"
                ) {
                    data["response"].clone()
                } else {
                    data
                };
                (event, payload)
            })
            .collect()
    }

    fn event_names(frames: &[(String, Value)]) -> Vec<&str> {
        frames.iter().map(|(event, _)| event.as_str()).collect()
    }

    fn upstream_json(body: Value) -> Response {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    // ---------- перевод запроса ----------

    #[test]
    fn translates_basic_responses_to_generate_content() {
        let translated = ok_translated(json!({
            "model": "google/gemini-2.5-flash",
            "instructions": "Be terse.",
            "input": "Hello"
        }));
        let body = &translated.body;
        // google/-префикс снят — дальше нативный публичный id.
        assert_eq!(translated.model, "gemini-2.5-flash");
        assert!(!translated.stream);
        assert!(body["generationConfig"].get("maxOutputTokens").is_none());
        assert_eq!(
            body["systemInstruction"],
            json!({"parts": [{"text": "Be terse."}]})
        );
        assert_eq!(
            body["contents"],
            json!([{"role": "user", "parts": [{"text": "Hello"}]}])
        );
        // toolConfig/tools отсутствуют — дефолт AUTO не вставляется.
        assert!(body.get("toolConfig").is_none());
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn null_optional_controls_keep_responses_defaults() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "input": "Hello",
            "stream": null,
            "max_output_tokens": null
        }));
        assert!(!translated.stream);
        assert!(translated.body["generationConfig"]
            .get("maxOutputTokens")
            .is_none());
    }

    #[tokio::test]
    async fn malformed_responses_controls_are_parameter_specific_400s() {
        for (field, value) in [
            ("stream", json!("false")),
            ("max_output_tokens", json!(0)),
            ("max_output_tokens", json!(-1)),
            ("max_output_tokens", json!(1.5)),
            ("max_output_tokens", json!("10")),
            ("max_output_tokens", json!({})),
        ] {
            let mut request = json!({
                "model": "gemini-2.5-flash",
                "input": "Hello"
            });
            request[field] = value;
            let (status, body) = expect_err(request).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(body["error"]["param"], field, "{body}");
        }
    }

    #[test]
    fn instructions_merge_with_system_items_instructions_first() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "instructions": "Be terse.",
            "input": [
                {"type": "message", "role": "system", "content": "System line."},
                {"type": "message", "role": "developer", "content": [
                    {"type": "input_text", "text": "Dev one."},
                    {"type": "input_text", "text": "Dev two."}
                ]},
                {"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "Hello"}
                ]}
            ]
        }));
        // instructions первым, затем system/developer items в порядке входа.
        assert_eq!(
            translated.body["systemInstruction"],
            json!({"parts": [
                {"text": "Be terse."},
                {"text": "System line."},
                {"text": "Dev one.\nDev two."}
            ]})
        );
        assert_eq!(
            translated.body["contents"],
            json!([{"role": "user", "parts": [{"text": "Hello"}]}])
        );
    }

    #[test]
    fn compact_items_and_assistant_output_text_translate() {
        // Компактная форма `{role, content}` без type (её шлёт stock SDK) и
        // output_text в assistant — text-парты; assistant → роль model,
        // одноролевые contents склеиваются.
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "input": [
                {"role": "user", "content": "one"},
                {"role": "user", "content": [{"type": "input_text", "text": "two"}]},
                {"role": "assistant", "content": [
                    {"type": "output_text", "text": "answer"}
                ]}
            ]
        }));
        assert_eq!(
            translated.body["contents"],
            json!([
                {"role": "user", "parts": [{"text": "one"}, {"text": "two"}]},
                {"role": "model", "parts": [{"text": "answer"}]}
            ])
        );
        // instructions отсутствует — systemInstruction нет.
        assert!(translated.body.get("systemInstruction").is_none());
    }

    #[test]
    fn translates_input_image_parts() {
        // data: URL → inlineData (общий перевод chat-адаптера);
        // text-склейка разрывается image-партами.
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "input": [{"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "What is this?"},
                {"type": "input_image", "image_url": "data:image/png;base64,iVBORw0KGgo="},
                {"type": "input_image", "image_url": "data:image/jpeg;base64,/9j/4AAQ", "detail": "auto"},
                {"type": "input_text", "text": "And this?"}
            ]}]
        }));
        assert_eq!(
            translated.body["contents"],
            json!([{"role": "user", "parts": [
                {"text": "What is this?"},
                {"inlineData": {"mimeType": "image/png", "data": "iVBORw0KGgo="}},
                {"inlineData": {"mimeType": "image/jpeg", "data": "/9j/4AAQ"}},
                {"text": "And this?"}
            ]}])
        );
    }

    #[tokio::test]
    async fn rejects_invalid_input_image_parts() {
        for (content, expected) in [
            // http(s) ссылки generateContent не принимает — честный 400
            // (отличие от Anthropic-зеркала: там http(s) → url source).
            (
                json!([{"type": "input_image", "image_url": "https://example.com/cat.jpg"}]),
                "only data: image URLs",
            ),
            // detail != auto — generateContent не умеет.
            (
                json!([{"type": "input_image", "image_url": "data:image/png;base64,aGVsbG8=", "detail": "high"}]),
                "Unsupported parameter",
            ),
            // Битый data: URL.
            (
                json!([{"type": "input_image", "image_url": "data:image/png;plain,xyz"}]),
                "data URL",
            ),
            // Не-image MIME.
            (
                json!([{"type": "input_image", "image_url": "data:text/html;base64,PGI+"}]),
                "image MIME",
            ),
            // Нет image_url.
            (json!([{"type": "input_image"}]), "image_url string"),
        ] {
            let (status, body) = expect_err(json!({
                "model": "gemini-2.5-flash",
                "input": [{"type": "message", "role": "user", "content": content}],
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{content}");
            assert_eq!(body["error"]["param"], "input", "{content}");
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
    fn tools_translate_to_function_declarations() {
        // Responses-форма плоская (дескриптор — сам item); strict снимается,
        // отсутствующие parameters опускаются (общий function_declaration).
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "input": "weather?",
            "tools": [
                {"type": "function", "name": "get_weather", "description": "Current weather",
                 "strict": true,
                 "parameters": {"$schema": "https://json-schema.org/draft/2020-12/schema",
                    "type": "object", "properties": {
                        "city": {"type": "string", "exclusiveMinimum": 1}
                    }}},
                {"type": "function", "name": "no_args"}
            ]
        }));
        assert_eq!(
            translated.body["tools"],
            json!([{"functionDeclarations": [
                {"name": "get_weather", "description": "Current weather",
                 "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}},
                {"name": "no_args"}
            ]}])
        );
    }

    #[tokio::test]
    async fn tool_and_structured_schema_errors_report_the_exact_pointer() {
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "input": "hi",
            "tools": [{"type":"function", "name":"f", "parameters":{
                "type":"object", "dependentRequired":{"x":["y"]}
            }}]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(
            body["error"]["param"],
            "tools.0.parameters/dependentRequired"
        );

        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "input": "hi",
            "text": {"format":{"type":"json_schema", "name":"x", "schema":{
                "type":"object", "unevaluatedProperties":false
            }}}
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(
            body["error"]["param"],
            "text.format.schema/unevaluatedProperties"
        );
    }

    #[tokio::test]
    async fn non_function_tools_are_400_unsupported_parameter() {
        for tool in [
            json!({"type": "custom", "name": "x"}),
            json!({"type": "web_search"}),
            json!({"type": "file_search", "vector_store_ids": ["vs_1"]}),
            json!({"type": "mcp", "server_label": "srv"}),
        ] {
            let (status, body) = expect_err(json!({
                "model": "gemini-2.5-flash",
                "input": "hi",
                "tools": [tool],
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{tool}");
            assert_eq!(body["error"]["code"], "unsupported_parameter", "{tool}");
            assert_eq!(body["error"]["param"], "tools", "{tool}");
        }
    }

    #[test]
    fn tool_choice_variants_map_to_tool_config() {
        // auto — дефолт generateContent, в тело не вставляется.
        let translated = ok_translated(json!({
            "model": "m", "input": "hi", "tool_choice": "auto"
        }));
        assert!(translated.body.get("toolConfig").is_none());

        let translated = ok_translated(json!({
            "model": "m", "input": "hi", "tool_choice": "required"
        }));
        assert_eq!(
            translated.body["toolConfig"],
            json!({"functionCallingConfig": {"mode": "ANY"}})
        );

        let translated = ok_translated(json!({
            "model": "m", "input": "hi", "tool_choice": "none"
        }));
        assert_eq!(
            translated.body["toolConfig"],
            json!({"functionCallingConfig": {"mode": "NONE"}})
        );

        // Именная форма Responses — плоская {type:"function", name}.
        let translated = ok_translated(json!({
            "model": "m", "input": "hi",
            "tool_choice": {"type": "function", "name": "f"}
        }));
        assert_eq!(
            translated.body["toolConfig"],
            json!({"functionCallingConfig": {"mode": "ANY", "allowedFunctionNames": ["f"]}})
        );
    }

    #[tokio::test]
    async fn invalid_tool_choice_is_400() {
        let (status, body) = expect_err(json!({
            "model": "m", "input": "hi", "tool_choice": "sometimes"
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["param"], "tool_choice");

        let (status, _) = expect_err(json!({
            "model": "m", "input": "hi", "tool_choice": {"type": "function"}
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // ---------- reasoning (как 3.4b chat-адаптера) ----------

    #[test]
    fn reasoning_effort_maps_to_thinking_config() {
        // Каждый уровень проксируется как есть (маппинг в wire model id —
        // на плоскости) — отличие от Anthropic-зеркала, где minimal
        // клампится в low. includeThoughts включает thought-парты ответа.
        for level in ["minimal", "low", "medium", "high"] {
            let translated = ok_translated(json!({
                "model": "gemini-2.5-flash",
                "input": "hi",
                "reasoning": {"effort": level},
            }));
            assert_eq!(
                translated.body["generationConfig"]["thinkingConfig"],
                json!({"thinkingLevel": level, "includeThoughts": true}),
                "{level}"
            );
        }
        // null/отсутствие — выкл: thinkingConfig не появляется.
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash", "input": "hi", "reasoning": null,
        }));
        assert!(translated.body["generationConfig"]
            .get("thinkingConfig")
            .is_none());
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash", "input": "hi", "reasoning": {"effort": null},
        }));
        assert!(translated.body["generationConfig"]
            .get("thinkingConfig")
            .is_none());
    }

    #[tokio::test]
    async fn rejects_invalid_reasoning() {
        // Любое не-null значение effort вне minimal|low|medium|high → 400
        // invalid_request (не unsupported_parameter — параметр поддержан).
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash", "input": "hi",
            "reasoning": {"effort": "extreme"},
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert_eq!(body["error"]["param"], "reasoning");
        assert!(body["error"]["code"].is_null());

        // reasoning без effort — битый запрос.
        let (status, _) = expect_err(json!({
            "model": "gemini-2.5-flash", "input": "hi", "reasoning": {},
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // ---------- structured output (text.format) ----------

    #[test]
    fn translates_text_format_json_schema_and_json_object() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "input": "Extract.",
            "text": {"format": {"type": "json_schema", "name": "profile", "strict": true,
                "schema": {"$schema": "https://json-schema.org/draft/2020-12/schema",
                    "type": "object", "properties": {
                        "name": {"type": "string", "exclusiveMaximum": 10}
                    }}}}
        }));
        let config = &translated.body["generationConfig"];
        assert_eq!(config["responseMimeType"], "application/json");
        // Обёртка (name/strict) не проксируется — только сама схема.
        assert_eq!(
            config["responseSchema"],
            json!({"type": "object", "properties": {"name": {"type": "string"}}})
        );
        assert!(translated.body.get("text").is_none());

        // json_object у generateContent есть (отличие от Messages — там 400):
        // JSON без схемы.
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "input": "Extract.",
            "text": {"format": {"type": "json_object"}}
        }));
        let config = &translated.body["generationConfig"];
        assert_eq!(config["responseMimeType"], "application/json");
        assert!(config.get("responseSchema").is_none());
    }

    #[test]
    fn text_format_and_reasoning_share_generation_config() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "input": "Extract.",
            "reasoning": {"effort": "medium"},
            "text": {"format": {"type": "json_schema", "name": "p",
                "schema": {"type": "object"}}}
        }));
        let config = &translated.body["generationConfig"];
        assert_eq!(
            config["thinkingConfig"],
            json!({"thinkingLevel": "medium", "includeThoughts": true})
        );
        assert_eq!(config["responseMimeType"], "application/json");
        assert_eq!(config["responseSchema"], json!({"type": "object"}));
    }

    #[tokio::test]
    async fn rejects_unsupported_text_format_and_verbosity() {
        // Неизвестный format type → unsupported_parameter.
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash", "input": "hi",
            "text": {"format": {"type": "future_format"}}
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "unsupported_parameter");
        assert_eq!(body["error"]["param"], "text");

        // Не-дефолтная verbosity (дефолт — medium) → unsupported_parameter.
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash", "input": "hi", "text": {"verbosity": "low"}
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "unsupported_parameter");
        assert_eq!(body["error"]["param"], "text");

        // Дефолтная verbosity принимается.
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash", "input": "hi",
            "text": {"verbosity": "medium", "format": {"type": "text"}}
        }));
        assert!(translated.body["generationConfig"]
            .get("responseMimeType")
            .is_none());

        // json_schema без schema-объекта — битый запрос.
        let (status, _) = expect_err(json!({
            "model": "gemini-2.5-flash", "input": "hi",
            "text": {"format": {"type": "json_schema", "name": "x"}}
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // ---------- временные ограничения (stored — решение 5) ----------

    #[tokio::test]
    async fn stored_responses_are_documented_limitation() {
        // Решение 5: stored responses — только openai/*.
        for (field, value) in [
            ("store", json!(true)),
            ("previous_response_id", json!("resp_42")),
        ] {
            let (status, body) = expect_err(json!({
                "model": "gemini-2.5-flash", "input": "hi", field: value,
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{field}");
            assert_eq!(body["error"]["code"], "documented_limitation", "{field}");
            assert_eq!(body["error"]["type"], "invalid_request_error", "{field}");
            assert_eq!(body["error"]["param"], field, "{field}");
        }
        // Дефолты принимаются и в тело не проксируются.
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash", "input": "hi",
            "store": false, "previous_response_id": null,
        }));
        assert!(translated.body.get("store").is_none());
        assert!(translated.body.get("previous_response_id").is_none());
    }

    #[tokio::test]
    async fn item_reference_is_documented_limitation_reasoning_item_is_dropped() {
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "input": [
                {"type": "message", "role": "user", "content": "hi"},
                {"type": "item_reference", "id": "resp_42"}
            ],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "documented_limitation");
        assert_eq!(body["error"]["param"], "input");

        // reasoning item (подписи не выставляются — решение 4) принимается и
        // выбрасывается, диалог не ломается.
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "input": [
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "…"}]},
                {"type": "message", "role": "user", "content": "hi"}
            ],
        }));
        assert_eq!(
            translated.body["contents"],
            json!([{"role": "user", "parts": [{"text": "hi"}]}])
        );
    }

    // ---------- replay tool-истории (по образцу 4.2) ----------

    #[test]
    fn function_call_items_replay_to_function_call_and_function_response_parts() {
        // Stored history шлёт message assistant + function_call подряд —
        // одноролевая склейка собирает один model-content с text+functionCall;
        // function_call_output склеивается со следующим user-content.
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "input": [
                {"type": "message", "role": "user", "content": "weather?"},
                {"type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "Let me check."}
                ]},
                {"type": "function_call", "call_id": "callu_get_weather", "name": "get_weather",
                 "arguments": "{\"city\":\"Paris\"}"},
                {"type": "function_call_output", "call_id": "callu_get_weather", "output": "{\"temp\":20}"},
                {"type": "message", "role": "user", "content": "thanks?"}
            ]
        }));
        assert_eq!(
            translated.body["contents"],
            json!([
                {"role": "user", "parts": [{"text": "weather?"}]},
                {"role": "model", "parts": [
                    {"text": "Let me check."},
                    {"functionCall": {"name": "get_weather", "args": {"city": "Paris"}},
                     "thoughtSignature": "context_engineering_is_the_way_to_go"}
                ]},
                {"role": "user", "parts": [
                    {"functionResponse": {"name": "get_weather",
                        "response": {"result": {"temp": 20}}}},
                    {"text": "thanks?"}
                ]}
            ])
        );
    }

    #[test]
    fn function_call_output_parts_glue_and_first_item_model_is_allowed() {
        // output массивом text-партов — склейка через \n; не-JSON output
        // заворачивается строкой; отсутствующая и пустая строка arguments —
        // `{}`; история может начинаться с function_call (model первым) —
        // валидации порядка нет (как в chat-адаптере).
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "input": [
                {"type": "function_call", "call_id": "call_1", "name": "f", "arguments": ""},
                {"type": "function_call", "call_id": "call_2", "name": "g"},
                {"type": "function_call_output", "call_id": "call_1", "output": [
                    {"type": "input_text", "text": "line one"},
                    {"type": "output_text", "text": "line two"}
                ]}
            ]
        }));
        assert_eq!(
            translated.body["contents"],
            json!([
                {"role": "model", "parts": [
                    {"functionCall": {"name": "f", "args": {}},
                     "thoughtSignature": "context_engineering_is_the_way_to_go"},
                    {"functionCall": {"name": "g", "args": {}},
                     "thoughtSignature": "context_engineering_is_the_way_to_go"}
                ]},
                {"role": "user", "parts": [
                    {"functionResponse": {"name": "f",
                        "response": {"result": "line one\nline two"}}}
                ]}
            ])
        );
    }

    #[tokio::test]
    async fn function_call_invalid_arguments_and_missing_fields_are_400() {
        // Невалидный JSON, не-object JSON и не-строка arguments → 400
        // invalid_request (param input).
        for arguments in [json!("not json"), json!("[1]"), json!({"x": 1})] {
            let (status, body) = expect_err(json!({
                "model": "gemini-2.5-flash",
                "input": [
                    {"type": "message", "role": "user", "content": "hi"},
                    {"type": "function_call", "call_id": "call_1", "name": "f",
                     "arguments": arguments}
                ],
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{arguments}");
            assert_eq!(
                body["error"]["type"], "invalid_request_error",
                "{arguments}"
            );
            assert_eq!(body["error"]["param"], "input", "{arguments}");
        }
        // Отсутствующие/пустые call_id и name → 400.
        for item in [
            json!({"type": "function_call", "name": "f", "arguments": "{}"}),
            json!({"type": "function_call", "call_id": "", "name": "f", "arguments": "{}"}),
            json!({"type": "function_call", "call_id": "call_1", "arguments": "{}"}),
            json!({"type": "function_call", "call_id": "call_1", "name": "", "arguments": "{}"}),
            json!({"type": "function_call_output", "output": "done"}),
        ] {
            let (status, body) = expect_err(json!({
                "model": "gemini-2.5-flash",
                "input": [
                    {"type": "message", "role": "user", "content": "hi"},
                    item
                ],
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{item}");
            assert_eq!(body["error"]["param"], "input", "{item}");
        }
        // function_call_output без function_call с таким call_id в этой
        // истории → 400 (functionResponse ссылается по имени — pairing, в
        // отличие от Anthropic-зеркала, валидируется адаптером).
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "input": [
                {"type": "message", "role": "user", "content": "hi"},
                {"type": "function_call_output", "call_id": "call_unknown", "output": "x"}
            ],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no matching function_call"));
        // Нетекстовые части output — 400.
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "input": [
                {"type": "message", "role": "user", "content": "hi"},
                {"type": "function_call", "call_id": "call_1", "name": "f"},
                {"type": "function_call_output", "call_id": "call_1", "output": [
                    {"type": "input_image", "image_url": "data:image/png;base64,iVBORw0KGgo="}
                ]}
            ],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["param"], "input");
    }

    // ---------- capability matrix ----------

    #[tokio::test]
    async fn unsupported_non_default_parameters_are_400() {
        // Те же 9 правил, что у Anthropic-зеркала, плюс parallel_tool_calls
        // (у generateContent нет disable_parallel_tool_use — только дефолт).
        for (field, value) in [
            ("background", json!(true)),
            ("service_tier", json!("flex")),
            ("truncation", json!("auto")),
            ("include", json!(["file_search_call.results"])),
            ("prompt_cache_key", json!("key-1")),
            ("safety_identifier", json!("user-1")),
            ("user", json!("user-1")),
            ("metadata", json!({"k": "v"})),
            ("max_tool_calls", json!(3)),
            ("parallel_tool_calls", json!(false)),
        ] {
            let (status, body) = expect_err(json!({
                "model": "gemini-2.5-flash", "input": "hi", field: value,
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{field}");
            assert_eq!(body["error"]["code"], "unsupported_parameter", "{field}");
            assert_eq!(body["error"]["param"], field, "{field}");
            assert_eq!(body["error"]["type"], "invalid_request_error", "{field}");
        }
    }

    #[test]
    fn default_valued_matrix_parameters_are_accepted() {
        // Stock SDK шлют дефолты пачками — они не должны ломать запрос.
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash",
            "input": "hi",
            "tools": [],
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "background": false,
            "service_tier": "auto",
            "truncation": "disabled",
            "include": [],
            "prompt_cache_key": null,
            "safety_identifier": null,
            "user": null,
            "metadata": null,
            "max_tool_calls": null,
            "store": false,
            "reasoning": null,
            "text": {"format": {"type": "text"}}
        }));
        // Дефолтные matrix-ключи в GenerateContentRequest не проксируются.
        for key in [
            "tools",
            "toolConfig",
            "background",
            "service_tier",
            "truncation",
            "include",
            "store",
            "reasoning",
            "text",
        ] {
            assert!(translated.body.get(key).is_none(), "{key}");
        }
        let config = &translated.body["generationConfig"];
        assert!(config.get("thinkingConfig").is_none());
        assert!(config.get("responseMimeType").is_none());
    }

    #[tokio::test]
    async fn unknown_top_level_field_is_400() {
        // Закрытый список (отличие от Anthropic-плоскости): wrapper апстрима
        // выбросил бы поле молча.
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "input": "hi",
            "future_parameter": 1
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "unsupported_parameter");
        assert_eq!(body["error"]["param"], "future_parameter");
    }

    #[test]
    fn max_output_tokens_maps_to_max_output_tokens() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash", "input": "hi", "max_output_tokens": 500,
        }));
        assert_eq!(translated.body["generationConfig"]["maxOutputTokens"], 500);
        assert!(translated.body.get("max_output_tokens").is_none());

        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash", "input": "hi", "stream": true,
        }));
        assert!(translated.stream);
    }

    #[test]
    fn temperature_and_top_p_are_honored() {
        let translated = ok_translated(json!({
            "model": "gemini-2.5-flash", "input": "hi",
            "temperature": 0.7, "top_p": 0.9,
        }));
        assert_eq!(translated.body["generationConfig"]["temperature"], 0.7);
        assert_eq!(translated.body["generationConfig"]["topP"], 0.9);
    }

    #[tokio::test]
    async fn structural_errors_are_openai_shaped_400() {
        // Нет model.
        let (status, body) = expect_err(json!({"input": "hi"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["param"], "model");
        assert!(body["error"]["code"].is_null());

        // Пустой native id после strip'а префикса.
        let (status, body) = expect_err(json!({
            "model": "google/", "input": "hi"
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["param"], "model");

        // Нет input.
        let (status, body) = expect_err(json!({"model": "gemini-2.5-flash"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["param"], "input");

        // Пустая строка input.
        let (status, _) = expect_err(json!({
            "model": "gemini-2.5-flash", "input": ""
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Пустой массив / только system items — нужен хотя бы один
        // user/assistant content.
        let (status, _) = expect_err(json!({
            "model": "gemini-2.5-flash", "input": []
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "input": [{"type": "message", "role": "system", "content": "x"}]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Пустой контент сообщения.
        let (status, _) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "input": [{"type": "message", "role": "user", "content": ""}]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Неизвестный item type.
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash",
            "input": [{"type": "computer_call", "call_id": "c1"}]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["param"], "input");

        // instructions не строка.
        let (status, body) = expect_err(json!({
            "model": "gemini-2.5-flash", "input": "hi", "instructions": ["x"]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["param"], "instructions");
    }

    // ---------- ответ: unit-маппинги ----------

    #[test]
    fn status_mapping() {
        // Через общий map_finish_reason chat-адаптера: зеркало таблицы
        // Anthropic-адаптера (max_tokens → max_output_tokens, refusal →
        // content_filter).
        assert_eq!(map_status(Some("STOP")).0, "completed");
        assert_eq!(map_status(Some("OTHER")).0, "completed");
        assert_eq!(map_status(None).0, "completed");
        let (status, details) = map_status(Some("MAX_TOKENS"));
        assert_eq!(status, "incomplete");
        assert_eq!(details, json!({"reason": "max_output_tokens"}));
        for reason in [
            "SAFETY",
            "RECITATION",
            "BLOCKLIST",
            "PROHIBITED_CONTENT",
            "SPII",
        ] {
            let (status, details) = map_status(Some(reason));
            assert_eq!(status, "incomplete", "{reason}");
            assert_eq!(details, json!({"reason": "content_filter"}), "{reason}");
        }
    }

    #[test]
    fn usage_mapping_includes_thoughts_and_cached_tokens() {
        let usage = map_responses_usage(&json!({
            "promptTokenCount": 100,
            "candidatesTokenCount": 7,
            "thoughtsTokenCount": 5,
            "totalTokenCount": 112,
            "cachedContentTokenCount": 30
        }));
        assert_eq!(usage["input_tokens"], 100);
        // output = candidates + thoughts (как тарифицирует metering).
        assert_eq!(usage["output_tokens"], 12);
        assert_eq!(usage["total_tokens"], 112);
        assert_eq!(usage["input_tokens_details"]["cached_tokens"], 30);
        assert_eq!(usage["output_tokens_details"]["reasoning_tokens"], 5);

        // total fallback — input+output; details опускаются при нулях.
        let usage = map_responses_usage(&json!({
            "promptTokenCount": 5, "candidatesTokenCount": 2
        }));
        assert_eq!(usage["input_tokens"], 5);
        assert_eq!(usage["output_tokens"], 2);
        assert_eq!(usage["total_tokens"], 7);
        assert!(usage.get("input_tokens_details").is_none());
        assert!(usage.get("output_tokens_details").is_none());
    }

    // ---------- перевод ответа (non-stream) ----------

    #[tokio::test]
    async fn non_stream_response_maps_to_response_object() {
        let upstream = Response::builder()
            .status(StatusCode::OK)
            .header("request-id", "req_abc")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "candidates": [{
                        "content": {"role": "model", "parts": [{"text": "Hello, "}, {"text": "world"}]},
                        "finishReason": "STOP"
                    }],
                    "usageMetadata": {"promptTokenCount": 16, "candidatesTokenCount": 5,
                        "totalTokenCount": 21, "cachedContentTokenCount": 4},
                    "modelVersion": "gemini-2.5-flash-001"
                })
                .to_string(),
            ))
            .unwrap();
        let response = json_responses_response(upstream, "gemini-2.5-flash".into()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("request-id").unwrap(), "req_abc");
        let (_, body) = err_parts(response).await;
        assert_eq!(body["object"], "response");
        assert!(body["id"].as_str().unwrap().starts_with("resp_"));
        assert_eq!(body["status"], "completed");
        assert_eq!(body["model"], "gemini-2.5-flash-001");
        assert!(body["error"].is_null());
        assert!(body["incomplete_details"].is_null());
        // text-парты склеиваются в ОДИН message item с одним output_text part.
        assert_eq!(body["output"].as_array().unwrap().len(), 1);
        let item = &body["output"][0];
        assert_eq!(item["type"], "message");
        assert!(item["id"].as_str().unwrap().starts_with("msg_"));
        assert_eq!(item["status"], "completed");
        assert_eq!(item["role"], "assistant");
        assert_eq!(
            item["content"],
            json!([{"type": "output_text", "text": "Hello, world", "annotations": [], "logprobs": []}])
        );
        assert_eq!(body["usage"]["input_tokens"], 16);
        assert_eq!(body["usage"]["output_tokens"], 5);
        assert_eq!(body["usage"]["total_tokens"], 21);
        assert_eq!(body["usage"]["input_tokens_details"]["cached_tokens"], 4);
    }

    #[tokio::test]
    async fn non_stream_function_calls_map_to_function_call_items() {
        let upstream = upstream_json(json!({
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"functionCall": {"name": "get_weather", "args": {"city": "Paris"}}},
                    {"functionCall": {"name": "get_weather", "args": {"city": "Lyon"}}},
                    {"functionCall": {"name": "no_args"}}
                ]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 12, "candidatesTokenCount": 5, "totalTokenCount": 17},
            "modelVersion": "gemini-2.5-flash-001"
        }));
        let response = json_responses_response(upstream, "gemini-2.5-flash".into()).await;
        let (_, body) = err_parts(response).await;
        assert_eq!(body["status"], "completed");
        // Без текста message item не создаётся; function_call items — в
        // порядке functionCall-партов, call_id — синтезированные
        // callu_<name>[_N] (functionCall.id на private wire нет).
        let output = body["output"].as_array().unwrap();
        assert_eq!(output.len(), 3);
        assert_eq!(output[0]["type"], "function_call");
        assert!(output[0]["id"].as_str().unwrap().starts_with("fc_"));
        assert_eq!(output[0]["call_id"], "callu_get_weather");
        assert_eq!(output[0]["name"], "get_weather");
        assert_eq!(output[0]["arguments"], "{\"city\":\"Paris\"}");
        assert_eq!(output[0]["status"], "completed");
        assert_eq!(output[1]["call_id"], "callu_get_weather_2");
        assert_eq!(output[2]["call_id"], "callu_no_args");
        assert_eq!(output[2]["arguments"], "{}");
    }

    #[tokio::test]
    async fn non_stream_text_and_function_call_keep_both_in_order() {
        let upstream = upstream_json(json!({
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"text": "Let me check."},
                    {"functionCall": {"name": "f", "args": {}}}
                ]},
                "finishReason": "STOP"
            }]
        }));
        let response = json_responses_response(upstream, "gemini-2.5-flash".into()).await;
        let (_, body) = err_parts(response).await;
        let output = body["output"].as_array().unwrap();
        // message item первым, затем function_call.
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["content"][0]["text"], "Let me check.");
        assert_eq!(output[1]["type"], "function_call");
    }

    #[tokio::test]
    async fn non_stream_max_tokens_is_incomplete_safety_is_content_filter() {
        for (finish, reason) in [
            ("MAX_TOKENS", "max_output_tokens"),
            ("SAFETY", "content_filter"),
            ("PROHIBITED_CONTENT", "content_filter"),
        ] {
            let upstream = upstream_json(json!({
                "candidates": [{
                    "content": {"parts": [{"text": "partial"}]},
                    "finishReason": finish
                }],
                "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 9, "totalTokenCount": 12}
            }));
            let response = json_responses_response(upstream, "gemini-2.5-flash".into()).await;
            let (_, body) = err_parts(response).await;
            assert_eq!(body["status"], "incomplete", "{finish}");
            assert_eq!(body["incomplete_details"]["reason"], reason, "{finish}");
        }
    }

    #[tokio::test]
    async fn non_stream_prompt_block_is_incomplete_content_filter() {
        // candidates отсутствуют — status из promptFeedback.blockReason.
        let upstream = upstream_json(json!({
            "promptFeedback": {"blockReason": "SAFETY"},
            "usageMetadata": {"promptTokenCount": 7, "totalTokenCount": 7}
        }));
        let response = json_responses_response(upstream, "gemini-2.5-flash".into()).await;
        let (_, body) = err_parts(response).await;
        assert_eq!(body["status"], "incomplete");
        assert_eq!(body["incomplete_details"]["reason"], "content_filter");
        assert_eq!(body["output"], json!([]));
        assert_eq!(body["usage"]["input_tokens"], 7);
    }

    #[tokio::test]
    async fn non_stream_thought_parts_become_reasoning_items() {
        // thought-парты → reasoning items (4.2): каждый парт с непустым
        // текстом — отдельный item в порядке появления; пустой thought и
        // thoughtSignature-only парт item не порождают (решение 4).
        let upstream = upstream_json(json!({
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"text": "First thought.", "thought": true},
                    {"text": "Answer."},
                    {"text": "Second thought.", "thought": true, "thoughtSignature": "sig_1"},
                    {"text": "", "thought": true},
                    {"thoughtSignature": "sig_2"},
                    {"functionCall": {"name": "f", "args": {}}}
                ]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 9,
                "thoughtsTokenCount": 7, "totalTokenCount": 19}
        }));
        let response = json_responses_response(upstream, "gemini-2.5-flash".into()).await;
        let (_, body) = err_parts(response).await;
        let output = body["output"].as_array().unwrap();
        // [reasoning, message, reasoning, function_call] — порядок партов.
        assert_eq!(output.len(), 4, "{body}");
        assert_eq!(output[0]["type"], "reasoning");
        assert!(output[0]["id"].as_str().unwrap().starts_with("rs_"));
        assert_eq!(
            output[0]["summary"],
            json!([{"type": "summary_text", "text": "First thought."}])
        );
        // Подписи и encrypted_content не выставляются (решение 4).
        assert!(output[0].get("encrypted_content").is_none());
        assert_eq!(output[1]["type"], "message");
        assert_eq!(output[1]["content"][0]["text"], "Answer.");
        assert_eq!(output[2]["type"], "reasoning");
        assert_eq!(output[2]["summary"][0]["text"], "Second thought.");
        assert_eq!(output[3]["type"], "function_call");
        assert!(!body.to_string().contains("sig_"), "{body}");
        // thoughts-токены в output и в reasoning_tokens (как metering).
        assert_eq!(body["usage"]["output_tokens"], 16);
        assert_eq!(
            body["usage"]["output_tokens_details"]["reasoning_tokens"],
            7
        );
    }

    #[tokio::test]
    async fn non_stream_model_falls_back_to_requested() {
        let upstream = upstream_json(json!({
            "candidates": [{"content": {"parts": [{"text": "ok"}]}, "finishReason": "STOP"}]
        }));
        let response = json_responses_response(upstream, "gemini-2.5-flash".into()).await;
        let (_, body) = err_parts(response).await;
        assert_eq!(body["model"], "gemini-2.5-flash");
        // usage отсутствовал → null.
        assert!(body["usage"].is_null());
    }

    #[tokio::test]
    async fn non_stream_malformed_body_is_500() {
        let upstream = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("not json"))
            .unwrap();
        let response = json_responses_response(upstream, "gemini-2.5-flash".into()).await;
        assert!(response
            .headers()
            .get(crate::proxy::EXECUTION_STATE_HEADER)
            .is_none());
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["type"], "server_error");
    }

    #[tokio::test]
    async fn non_stream_generated_image_output_item() {
        // Сгенерированный inlineData (image MIME) → output_image item с data
        // URL: billed media доставляется клиенту, а не теряется после
        // settlement. Позиция — порядок партов: text → message, image →
        // output_image после него.
        let upstream = upstream_json(json!({
            "candidates": [{"content": {"parts": [
                {"text": "Here it is."},
                {"inlineData": {"mimeType": "image/png", "data": "iVBORw0KGgo="}}
            ]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 4, "totalTokenCount": 7}
        }));
        let response = json_responses_response(upstream, "gemini-3.1-flash-image".into()).await;
        let (status, body) = err_parts(response).await;
        assert_eq!(status, StatusCode::OK);
        let output = body["output"].as_array().unwrap();
        assert_eq!(output.len(), 2, "{body}");
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["content"][0]["text"], "Here it is.");
        assert_eq!(output[1]["type"], "output_image");
        assert!(output[1]["id"].as_str().unwrap().starts_with("img_"));
        assert_eq!(output[1]["image_url"], "data:image/png;base64,iVBORw0KGgo=");
        assert!(output[1].get("image_uri").is_none());
        // usage продолжает нести итог, который тарифицирует settlement.
        assert_eq!(body["usage"]["output_tokens"], 4);
    }

    #[tokio::test]
    async fn non_stream_non_image_inline_media_stays_dropped() {
        // Не-image inline-медиа не имеет OpenAI-представления: item не
        // фабрикуется.
        let upstream = upstream_json(json!({
            "candidates": [{"content": {"parts": [
                {"text": "Note."},
                {"inlineData": {"mimeType": "audio/wav", "data": "UklGRg=="}}
            ]}, "finishReason": "STOP"}]
        }));
        let response = json_responses_response(upstream, "gemini-2.5-flash".into()).await;
        let (_, body) = err_parts(response).await;
        let output = body["output"].as_array().unwrap();
        assert_eq!(output.len(), 1, "{body}");
        assert_eq!(output[0]["type"], "message");
    }

    #[test]
    fn local_adapter_errors_mark_execution_not_started() {
        let response = translate_responses_request(json!({})).unwrap_err();
        assert_eq!(
            response
                .headers()
                .get(crate::proxy::EXECUTION_STATE_HEADER)
                .unwrap(),
            crate::proxy::EXECUTION_STATE_NOT_STARTED
        );
    }

    // ---------- SSE-транслятор: contract-тесты словаря событий 4.1–4.2 ----------
    //
    // Каноническая последовательность GenerateContentResponse-кадров → точная
    // последовательность Responses SSE. Responses-сторона обязана совпадать с
    // Anthropic-зеркалом на эквивалентном диалоге (решение 2): имена событий,
    // формы items и shell ниже — те же табличные ожидания, что в
    // `anthropic_responses.rs`.

    const SSE_TEXT_DIALOG: &str = r#"data: {"candidates":[{"content":{"role":"model","parts":[{"text":"Hello"}]}}],"modelVersion":"gemini-2.5-flash-001","usageMetadata":{"promptTokenCount":14,"cachedContentTokenCount":4}}

data: {"candidates":[{"content":{"role":"model","parts":[{"text":", world"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":14,"cachedContentTokenCount":4,"candidatesTokenCount":6,"totalTokenCount":20}}"#;

    #[tokio::test]
    async fn contract_text_dialog_event_dictionary() {
        // text: created → in_progress → item.added → part.added →
        // text.delta* → text.done → part.done → item.done → completed —
        // та же последовательность и shapes, что у Anthropic-зеркала (чистый
        // EOF Gemini-стрима выполняет роль message_stop).
        let translator = GeminiResponsesSseTranslator::new(
            sse_bytes(SSE_TEXT_DIALOG),
            "gemini-2.5-flash".into(),
        );
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        assert_eq!(
            event_names(&frames),
            [
                "response.created",
                "response.in_progress",
                "response.output_item.added",
                "response.content_part.added",
                "response.output_text.delta",
                "response.output_text.delta",
                "response.output_text.done",
                "response.content_part.done",
                "response.output_item.done",
                "response.completed",
            ],
            "{output}"
        );
        // Shell стартовых событий (modelVersion первого кадра зафиксирован
        // до эмиссии — shell уже с сервёной моделью).
        assert_eq!(frames[0].1["object"], "response");
        assert!(frames[0].1["id"].as_str().unwrap().starts_with("resp_"));
        assert_eq!(frames[0].1["status"], "in_progress");
        assert_eq!(frames[0].1["output"], json!([]));
        assert_eq!(frames[0].1["model"], "gemini-2.5-flash-001");
        assert_eq!(frames[0].1, frames[1].1);
        // Item lifecycle: плотный output_index 0, content_index 0, item_id
        // стабилен внутри item'а.
        assert_eq!(frames[2].1["output_index"], 0);
        assert_eq!(frames[2].1["item"]["type"], "message");
        assert_eq!(frames[2].1["item"]["status"], "in_progress");
        assert_eq!(frames[2].1["item"]["content"], json!([]));
        let item_id = frames[2].1["item"]["id"].clone();
        assert!(item_id.as_str().unwrap().starts_with("msg_"));
        assert_eq!(frames[3].1["item_id"], item_id);
        assert_eq!(frames[3].1["content_index"], 0);
        assert_eq!(
            frames[3].1["part"],
            json!({"type": "output_text", "text": "", "annotations": [], "logprobs": []})
        );
        assert_eq!(frames[4].1["delta"], "Hello");
        assert_eq!(frames[4].1["logprobs"], json!([]));
        assert_eq!(frames[5].1["delta"], ", world");
        assert_eq!(frames[6].1["text"], "Hello, world");
        assert_eq!(frames[7].1["part"]["text"], "Hello, world");
        assert_eq!(frames[8].1["item"]["id"], item_id);
        assert_eq!(frames[8].1["item"]["status"], "completed");
        assert_eq!(frames[8].1["item"]["content"][0]["text"], "Hello, world");
        // Финал: полный Response object с usage и статусом completed — те же
        // числа, что в contract-таблице Anthropic-зеркала.
        let completed = &frames[9].1;
        assert_eq!(completed["status"], "completed");
        assert_eq!(completed["id"], frames[0].1["id"]);
        assert_eq!(completed["output"].as_array().unwrap().len(), 1);
        assert_eq!(completed["output"][0]["content"][0]["text"], "Hello, world");
        assert_eq!(completed["usage"]["input_tokens"], 14);
        assert_eq!(
            completed["usage"]["input_tokens_details"]["cached_tokens"],
            4
        );
        assert_eq!(completed["usage"]["output_tokens"], 6);
        assert_eq!(completed["usage"]["total_tokens"], 20);
        assert!(completed["error"].is_null());
        assert!(completed["incomplete_details"].is_null());
    }

    const SSE_FUNCTION_CALL: &str = r#"data: {"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"get_weather","args":{"city":"Paris"}}}]}}],"modelVersion":"gemini-2.5-flash-001","usageMetadata":{"promptTokenCount":10}}

data: {"candidates":[{"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":8,"totalTokenCount":18}}"#;

    #[tokio::test]
    async fn contract_function_call_event_dictionary() {
        // function_call: item.added (arguments "") → arguments.delta →
        // arguments.done → item.done → completed. Gemini присылает
        // functionCall целиком — ровно одна дельта с полной строкой.
        let translator = GeminiResponsesSseTranslator::new(
            sse_bytes(SSE_FUNCTION_CALL),
            "gemini-2.5-flash".into(),
        );
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        assert_eq!(
            event_names(&frames),
            [
                "response.created",
                "response.in_progress",
                "response.output_item.added",
                "response.function_call_arguments.delta",
                "response.function_call_arguments.done",
                "response.output_item.done",
                "response.completed",
            ],
            "{output}"
        );
        let item = &frames[2].1["item"];
        assert_eq!(item["type"], "function_call");
        assert!(item["id"].as_str().unwrap().starts_with("fc_"));
        // call_id синтезирован — на private wire functionCall.id не приезжает.
        assert_eq!(item["call_id"], "callu_get_weather");
        assert_eq!(item["name"], "get_weather");
        assert_eq!(item["arguments"], "");
        assert_eq!(item["status"], "in_progress");
        assert_eq!(frames[3].1["delta"], "{\"city\":\"Paris\"}");
        assert_eq!(frames[3].1["item_id"], item["id"]);
        assert_eq!(frames[4].1["arguments"], "{\"city\":\"Paris\"}");
        assert_eq!(frames[5].1["item"]["status"], "completed");
        assert_eq!(frames[5].1["item"]["arguments"], "{\"city\":\"Paris\"}");
        let completed = &frames[6].1;
        assert_eq!(completed["status"], "completed");
        assert_eq!(completed["output"][0]["type"], "function_call");
        assert_eq!(completed["output"][0]["call_id"], "callu_get_weather");
        assert_eq!(completed["usage"]["output_tokens"], 8);
    }

    #[tokio::test]
    async fn contract_generated_image_event_dictionary() {
        // Сгенерированный inlineData (image MIME): item.added (output_image с
        // data URL) → item.done → completed; дельт у изображения нет.
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Done. \"},{\"inlineData\":{\"mimeType\":\"image/png\",\"data\":\"iVBORw0KGgo=\"}}]}}]}\n",
            "\n",
            "data: {\"candidates\":[{\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":4,\"totalTokenCount\":7}}\n",
        );
        let translator = GeminiResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        assert_eq!(
            event_names(&frames),
            [
                "response.created",
                "response.in_progress",
                "response.output_item.added",
                "response.content_part.added",
                "response.output_text.delta",
                "response.output_text.done",
                "response.content_part.done",
                "response.output_item.done",
                "response.output_item.added",
                "response.output_item.done",
                "response.completed",
            ],
            "{output}"
        );
        let image_item = &frames[8].1["item"];
        assert_eq!(image_item["type"], "output_image");
        assert!(image_item["id"].as_str().unwrap().starts_with("img_"));
        assert_eq!(
            image_item["image_url"],
            "data:image/png;base64,iVBORw0KGgo="
        );
        assert_eq!(frames[9].1["item"], *image_item);
        // Открытый text item закрыт до output_image: плотный output_index 0
        // у message, 1 у изображения.
        assert_eq!(frames[8].1["output_index"], 1);
        let completed = &frames[10].1;
        assert_eq!(completed["status"], "completed");
        assert_eq!(completed["output"][0]["type"], "message");
        assert_eq!(completed["output"][1]["type"], "output_image");
        assert_eq!(
            completed["output"][1]["image_url"],
            "data:image/png;base64,iVBORw0KGgo="
        );
        assert_eq!(completed["usage"]["output_tokens"], 4);
    }

    #[tokio::test]
    async fn contract_text_then_function_call_dense_output_index() {
        // text-парт перед functionCall: text item закрывается done-событиями
        // до открытия function_call; плотный счётчик — message output_index
        // 0, function_call 1 (зеркало contract-таблицы Anthropic-зеркала).
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Checking.\"}]}}]}\n",
            "\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"f\",\"args\":{}}}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"candidatesTokenCount\":4}}\n",
        );
        let translator = GeminiResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        assert_eq!(
            event_names(&frames),
            [
                "response.created",
                "response.in_progress",
                "response.output_item.added",
                "response.content_part.added",
                "response.output_text.delta",
                "response.output_text.done",
                "response.content_part.done",
                "response.output_item.done",
                "response.output_item.added",
                "response.function_call_arguments.delta",
                "response.function_call_arguments.done",
                "response.output_item.done",
                "response.completed",
            ],
            "{output}"
        );
        let added: Vec<u64> = frames
            .iter()
            .filter(|(event, _)| event == "response.output_item.added")
            .map(|(_, data)| data["output_index"].as_u64().unwrap())
            .collect();
        assert_eq!(added, [0, 1], "{output}");
        let completed = frames.last().unwrap();
        assert_eq!(completed.1["output"].as_array().unwrap().len(), 2);
        assert_eq!(completed.1["output"][0]["type"], "message");
        assert_eq!(completed.1["output"][1]["type"], "function_call");
    }

    #[tokio::test]
    async fn contract_two_function_calls_same_name_synthetic_ids() {
        // Per-name счётчик call_id — та же схема `callu_<name>[_N]`, что в
        // non-stream переводе и chat-адаптере.
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"city\":\"Paris\"}}}]}}]}\n",
            "\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"city\":\"Lyon\"}}}]},\"finishReason\":\"STOP\"}]}\n",
        );
        let translator = GeminiResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        let completed = frames.last().unwrap();
        assert_eq!(completed.0, "response.completed");
        let items = completed.1["output"].as_array().unwrap();
        assert_eq!(items.len(), 2, "{output}");
        assert_eq!(items[0]["call_id"], "callu_get_weather");
        assert_eq!(items[1]["call_id"], "callu_get_weather_2");
        assert_eq!(items[1]["arguments"], "{\"city\":\"Lyon\"}");
    }

    #[tokio::test]
    async fn contract_reasoning_event_dictionary() {
        // thought-парты (4.2): item.added (reasoning, summary []) →
        // reasoning_summary_part.added (summary_index 0) → text.delta* →
        // text.done → part.done → item.done. Парт с одним thoughtSignature
        // событий не порождает (решение 4). output_index: reasoning = 0,
        // message = 1 (плотный счётчик).
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Let me think.\",\"thought\":true}]}}],\"modelVersion\":\"gemini-2.5-flash-001\",\"usageMetadata\":{\"promptTokenCount\":10}}\n",
            "\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"thoughtSignature\":\"sig_1\"},{\"text\":\" Done.\",\"thought\":true}]}}]}\n",
            "\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Answer.\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":8,\"thoughtsTokenCount\":5,\"totalTokenCount\":23}}\n",
        );
        let translator =
            GeminiResponsesSseTranslator::new(sse_bytes(events), "gemini-2.5-flash".into());
        let output = collect_stream(translator).await;
        assert!(!output.contains("sig_1"), "{output}");
        let frames = event_frames(&output);
        assert_eq!(
            event_names(&frames),
            [
                "response.created",
                "response.in_progress",
                "response.output_item.added",
                "response.reasoning_summary_part.added",
                "response.reasoning_summary_text.delta",
                "response.reasoning_summary_text.delta",
                "response.reasoning_summary_text.done",
                "response.reasoning_summary_part.done",
                "response.output_item.done",
                "response.output_item.added",
                "response.content_part.added",
                "response.output_text.delta",
                "response.output_text.done",
                "response.content_part.done",
                "response.output_item.done",
                "response.completed",
            ],
            "{output}"
        );
        // Reasoning item lifecycle: output_index 0, summary_index 0, item_id
        // стабилен внутри item'а.
        assert_eq!(frames[2].1["output_index"], 0);
        assert_eq!(frames[2].1["item"]["type"], "reasoning");
        assert!(frames[2].1["item"]["id"]
            .as_str()
            .unwrap()
            .starts_with("rs_"));
        assert_eq!(frames[2].1["item"]["summary"], json!([]));
        let item_id = frames[2].1["item"]["id"].clone();
        assert_eq!(frames[3].1["item_id"], item_id);
        assert_eq!(frames[3].1["output_index"], 0);
        assert_eq!(frames[3].1["summary_index"], 0);
        assert_eq!(
            frames[3].1["part"],
            json!({"type": "summary_text", "text": ""})
        );
        assert_eq!(frames[4].1["delta"], "Let me think.");
        assert_eq!(frames[4].1["summary_index"], 0);
        assert_eq!(frames[5].1["delta"], " Done.");
        assert_eq!(frames[6].1["text"], "Let me think. Done.");
        assert_eq!(frames[7].1["part"]["text"], "Let me think. Done.");
        assert_eq!(frames[8].1["item"]["id"], item_id);
        assert_eq!(
            frames[8].1["item"]["summary"][0]["text"],
            "Let me think. Done."
        );
        // Message item — output_index 1 (плотный счётчик, дыр нет).
        assert_eq!(frames[9].1["output_index"], 1);
        assert_eq!(frames[9].1["item"]["type"], "message");
        // Финал: reasoning item в completed output, thoughtsTokenCount →
        // reasoning_tokens и входит в output_tokens (как metering).
        let completed = &frames[15].1;
        assert_eq!(completed["status"], "completed");
        let items = completed["output"].as_array().unwrap();
        assert_eq!(items.len(), 2, "{output}");
        assert_eq!(items[0]["type"], "reasoning");
        assert_eq!(items[0]["summary"][0]["text"], "Let me think. Done.");
        assert_eq!(items[1]["type"], "message");
        assert_eq!(completed["usage"]["output_tokens"], 13);
        assert_eq!(
            completed["usage"]["output_tokens_details"]["reasoning_tokens"],
            5
        );
        assert_eq!(completed["usage"]["total_tokens"], 23);
    }

    #[tokio::test]
    async fn sse_prompt_block_completes_incomplete_content_filter() {
        // Блокировка промпта на входе: client-visible дельт нет, финал —
        // response.completed со status incomplete/content_filter и пустым
        // output (usage из usageMetadata).
        let events = "data: {\"promptFeedback\":{\"blockReason\":\"SAFETY\"},\"usageMetadata\":{\"promptTokenCount\":5,\"totalTokenCount\":5}}\n";
        let translator = GeminiResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        assert_eq!(
            event_names(&frames),
            [
                "response.created",
                "response.in_progress",
                "response.completed"
            ],
            "{output}"
        );
        let completed = &frames[2].1;
        assert_eq!(completed["status"], "incomplete");
        assert_eq!(
            completed["incomplete_details"],
            json!({"reason": "content_filter"})
        );
        assert_eq!(completed["output"], json!([]));
        assert_eq!(completed["usage"]["input_tokens"], 5);
    }

    #[tokio::test]
    async fn sse_error_frame_becomes_response_failed_and_stream_ends() {
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"}]}}]}\n",
            "\n",
            "data: {\"error\":{\"code\":429,\"message\":\"Quota exceeded\",\"status\":\"RESOURCE_EXHAUSTED\"}}\n",
        );
        let translator = GeminiResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        assert!(!output.contains("response.completed"), "{output}");
        let frames = event_frames(&output);
        let last = frames.last().unwrap();
        assert_eq!(last.0, "response.failed", "{output}");
        assert_eq!(last.1["status"], "failed");
        // code — google.rpc status (как в error-кадре chat-адаптера).
        assert_eq!(last.1["error"]["code"], "RESOURCE_EXHAUSTED");
        assert_eq!(last.1["error"]["message"], "Quota exceeded");
        // Уже эмитированный text item остаётся открытым — в output failed не
        // попадает (done-событий не было).
        assert_eq!(last.1["output"], json!([]));
    }

    #[tokio::test]
    async fn sse_clean_eof_after_finish_emits_completed_with_last_usage() {
        // Нормальное завершение Gemini-стрима — provider finishReason + чистый EOF:
        // response.completed обязателен;
        // usage — последнее usageMetadata нарастающего итога.
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hi\"}]}}],\"usageMetadata\":{\"promptTokenCount\":3}}\n",
            "\n",
            "data: {\"candidates\":[{\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":2,\"totalTokenCount\":5}}\n",
        );
        let translator = GeminiResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        let completed = frames.last().unwrap();
        assert_eq!(completed.0, "response.completed", "{output}");
        assert_eq!(completed.1["status"], "completed");
        assert_eq!(completed.1["usage"]["input_tokens"], 3);
        assert_eq!(completed.1["usage"]["output_tokens"], 2);
        assert_eq!(completed.1["usage"]["total_tokens"], 5);
    }

    #[tokio::test]
    async fn sse_empty_stream_terminates_with_failed() {
        let translator = GeminiResponsesSseTranslator::new(sse_bytes(""), "m".into());
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        assert_eq!(
            event_names(&frames),
            [
                "response.created",
                "response.in_progress",
                "error",
                "response.failed"
            ],
            "{output}"
        );
        assert_eq!(frames[2].1["code"], "protocol_error");
        assert_eq!(frames[2].1["param"], Value::Null);
        assert_eq!(frames[3].1["status"], "failed");
        assert_eq!(frames[3].1["output"], json!([]));
        assert_eq!(frames[3].1["error"]["code"], "protocol_error");
    }

    #[tokio::test]
    async fn sse_transport_error_terminates_with_failed() {
        let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
            Ok(Bytes::from(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"He\"}]}}]}\n\n",
            )),
            Err(std::io::Error::other("reset")),
        ];
        let translator = GeminiResponsesSseTranslator::new(
            Box::pin(futures_util::stream::iter(chunks)),
            "m".into(),
        );
        let output = collect_stream(translator).await;
        assert!(!output.contains("response.completed"), "{output}");
        let frames = event_frames(&output);
        let last = frames.last().unwrap();
        assert_eq!(last.0, "response.failed", "{output}");
        assert_eq!(
            last.1["error"]["message"],
            "The provider stream was interrupted."
        );
    }

    #[tokio::test]
    async fn sse_max_tokens_completes_with_incomplete_status() {
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"partial\"}]}}]}\n",
            "\n",
            "data: {\"candidates\":[{\"finishReason\":\"MAX_TOKENS\"}],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":9,\"totalTokenCount\":12}}\n",
        );
        let translator = GeminiResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        let completed = frames.last().unwrap();
        assert_eq!(completed.0, "response.completed", "{output}");
        assert_eq!(completed.1["status"], "incomplete");
        assert_eq!(
            completed.1["incomplete_details"],
            json!({"reason": "max_output_tokens"})
        );
        assert_eq!(completed.1["output"][0]["content"][0]["text"], "partial");
    }

    #[tokio::test]
    async fn sse_split_frames_across_chunks_are_reassembled() {
        // Кадр, разрезанный посреди JSON двумя сетевыми чанками.
        let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
            Ok(Bytes::from("data: {\"candidates\":[{\"content\":{\"par")),
            Ok(Bytes::from(
                "ts\":[{\"text\":\"split ok\"}]}}]}\n\ndata: {\"candidates\":[{\"fin",
            )),
            Ok(Bytes::from("ishReason\":\"STOP\"}]}\n\n")),
        ];
        let translator = GeminiResponsesSseTranslator::new(
            Box::pin(futures_util::stream::iter(chunks)),
            "m".into(),
        );
        let output = collect_stream(translator).await;
        assert!(output.contains("split ok"), "{output}");
        assert!(output.contains("response.completed"), "{output}");
    }

    #[tokio::test]
    async fn sse_malformed_frame_terminates_with_failed() {
        let events = concat!(
            "data: {not json}\n",
            "\n",
            "data: {\"candidates\":[{\"finishReason\":\"STOP\"}]}\n",
        );
        let translator = GeminiResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        let failed = frames.last().unwrap();
        assert_eq!(failed.0, "response.failed", "{output}");
        assert_eq!(failed.1["error"]["code"], "protocol_error");
    }

    #[tokio::test]
    async fn sse_unterminated_finish_frame_and_unknown_fields_are_accepted() {
        let events = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\",\"futurePartField\":true}]},\"futureCandidateField\":1}],\"futureTopLevelField\":{}}\n\n",
            "data: {\"candidates\":[{\"finishReason\":\"STOP\"}]}"
        );
        let output = collect_stream(GeminiResponsesSseTranslator::new(
            sse_bytes(events),
            "m".into(),
        ))
        .await;
        let frames = event_frames(&output);
        assert_eq!(frames.last().unwrap().0, "response.completed", "{output}");
        assert!(output.contains("ok"), "{output}");
    }
}
