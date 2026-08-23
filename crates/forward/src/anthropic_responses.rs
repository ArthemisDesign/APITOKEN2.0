//! Universal Responses → Anthropic Messages адаптер — этапы 4.1–4.2
//! docs/engine/UNIFIED_ROUTER.md (решения 1–5).
//!
//! `POST /v1/responses` на Anthropic-плоскости. Поток запроса повторяет
//! chat-адаптер (`anthropic.rs`, этапы 3.1–3.4b): парс Responses-запроса →
//! перевод в Messages JSON → внутренний `Request` на `/v1/messages` → общий
//! [`forward`] (auth, reserve, ротация, identity-инжект, tee-метеринг,
//! settle — без единого изменения) → перевод ответа: Messages SSE →
//! Responses SSE либо JSON message → Response object. Router
//! (`crates/router/src/responses.rs`) выполняет model-based dispatch и
//! проксирует тело без изменений.
//!
//! Перевод запроса: `instructions` и system/developer items → top-level
//! `system` (instructions первым); `input` строка → одно user-сообщение,
//! массив items → сообщения Messages (message item — `{type:"message", …}`
//! или компактная форма `{role, content}` без type, которую шлёт stock SDK;
//! подряд идущие одноролевые склеиваются); content parts `input_text` и
//! `output_text` → text-блоки, `input_image` → image-блоки (общий с
//! chat-адаптером перевод: data: → base64 source, http(s) → url source,
//! `detail` != auto → 400); replay tool-истории (4.2): function_call item →
//! assistant `tool_use`-блок (`call_id` → `id`, `arguments` JSON-строка
//! парсится в `input` — невалидный JSON и не-object →
//! `400 invalid_request`), function_call_output item → user
//! `tool_result`-блок (`call_id` → `tool_use_id`; `output` строка → text
//! content как есть, массив text-партов склеивается через \n, нетекстовые
//! части → 400); pairing tool_use/tool_result не валидируется — Messages сам
//! честно отвечает 400 на битую историю (как chat-адаптер 3.2); Responses
//! `tools` → Messages `tools[]`
//! (`parameters`→`input_schema`, `strict` снимается — общий перевод
//! function-дескриптора); `tool_choice` auto/required/none/именная функция →
//! Messages `tool_choice`, `parallel_tool_calls:false` →
//! `disable_parallel_tool_use:true`; `max_output_tokens` → `max_tokens`;
//! при отсутствии cap обязательный Messages limit равен нативному потолку
//! модели (64k для Claude ≤4.5, 128k для 4.6+/5); `reasoning.effort` →
//! model-specific `output_config.effort` (minimal клампится в low; Claude 4.6
//! принимает `max`, Claude 4.7+/5 также `xhigh`) + инжект
//! `thinking: {"type":"adaptive","display":"summarized"}` на Claude 4.6+
//! — как 3.4c chat-адаптера: без thinking рассуждения нет вовсе, а дефолтный
//! display=omitted присылает пустые блоки; явный `thinking` клиента не
//! переопределяется (open list его проксирует, инжект не делается). На
//! моделях до 4.6 валидный effort деградирует к model default без обоих
//! несовместимых полей;
//! `text.format` json_schema → `output_config.format` (обёртка
//! name/strict/description снимается, json_object у Messages нет → 400).
//!
//! Capability matrix (решение 3): `background`, `service_tier`, `truncation`,
//! `include`, `prompt_cache_key`, `safety_identifier`, `user`, `metadata`,
//! `max_tool_calls` и не-дефолтная `text.verbosity` (дефолт — "medium") с
//! не-дефолтным значением → `400 unsupported_parameter`; дефолтные
//! принимаются (stock SDK шлют дефолты пачками). Неизвестные поля
//! проксируются в Messages тело (открытый список; валидация — на апстриме).
//!
//! Временные ограничения (после 4.2; задокументированы в UNIFIED_ROUTER.md,
//! п. 4):
//! - `reasoning` items во входе принимаются и выбрасываются (подписи и
//!   encrypted content в universal lanes не выставляются — решение 4,
//!   реплеить нечего);
//! - `store:true`, `previous_response_id` и `item_reference` →
//!   `400 documented_limitation` (stored responses — только `openai/*`,
//!   решение 5); `POST /v1/responses/input_tokens` остаётся openai-only и
//!   этим адаптером не обслуживается.
//!
//! Перевод ответа — словарь 4.1 + reasoning (4.2). Non-stream: Response
//! object `{id: "resp_*", object: "response", created_at, status, model,
//! output, usage, error, incomplete_details}`; text-блоки склеиваются в ОДИН
//! message item с одним output_text part на позиции первого text-блока (без
//! текста item не создаётся), thinking-блоки → reasoning items (`rs_*`, один
//! `summary_text` part с текстом блока; пустой thinking — item не создаётся,
//! redacted_thinking пропускается — решение 4), tool_use-блоки →
//! function_call items (`fc_*`, `call_id` = tool_use id, arguments — JSON
//! строка `input`); items — в порядке появления блоков. Stream (Messages SSE
//! → Responses SSE, транслятор СНАРУЖИ forward() как у chat-адаптера):
//! - `message_start` → `response.created` + `response.in_progress` (shell:
//!   status "in_progress", output [], error/incomplete_details null);
//! - text-блок: `response.output_item.added` (message item, content []) →
//!   `response.content_part.added` (output_text part, content_index 0) →
//!   `response.output_text.delta`* → `response.output_text.done` →
//!   `response.content_part.done` → `response.output_item.done`;
//! - thinking-блок (4.2): `response.output_item.added` (reasoning item,
//!   summary []) → `response.reasoning_summary_part.added` (summary_index 0,
//!   пустой `summary_text` part) → `response.reasoning_summary_text.delta`*
//!   (из thinking_delta; пустые дельты и signature_delta дропаются —
//!   решение 4) → `response.reasoning_summary_text.done` →
//!   `response.reasoning_summary_part.done` → `response.output_item.done`
//!   (item с полной summary);
//! - tool_use-блок: `response.output_item.added` (function_call item,
//!   arguments "") → `response.function_call_arguments.delta`*
//!   (input_json_delta) → `response.function_call_arguments.done` →
//!   `response.output_item.done`;
//! - `message_stop` → `response.completed` с полным Response object (output
//!   собран полностью, usage из message_start+message_delta
//!   (output_tokens_details проксируются — reasoning_tokens), status по
//!   stop_reason — как non-stream);
//! - `event: ping` → SSE comment-кадр `: ping` (heartbeat без событийной
//!   семантики);
//! - mid-stream `event: error` → OpenAI `error` event, затем
//!   `response.failed` (status "failed", error {code, message} из
//!   санитизированной ошибки апстрима — как error frame chat-адаптера) и
//!   завершение стрима; чистый EOF до `message_stop`, транспортный сбой и
//!   malformed known event/type/order проходят тот же failure lifecycle;
//!   неизвестные именованные события игнорируются для forward compatibility;
//! - output_index — плотный собственный счётчик: text/thinking/tool_use
//!   блоки Messages позицию занимают, redacted_thinking и неизвестные —
//!   пропускаются без позиции (решение 4); content_index всегда 0 (один
//!   output_text part на message item). Item-scoped события несут `item_id`;
//!   каждый data object несёт `type` и монотонный `sequence_number`, а
//!   lifecycle events оборачивают Response object в поле `response`.
//! usage всегда в `response.completed` (include-флага в Responses нет).
//!
//! Все ответы этого пути — OpenAI-совместимый конверт, включая ошибки:
//! синтетические ошибки плоскости и пасsthrough-ошибки апстрима переводятся
//! общим с chat-адаптером `convert_error_response` с сохранением HTTP-статуса
//! (402 LowBalance тоже) и `Retry-After`.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes};
#[cfg(test)]
use axum::body::to_bytes;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::BytesMut;
use futures_util::{Stream, StreamExt};
use serde_json::{json, Map, Value};

use crate::anthropic::{
    chat_error, convert_error_response_admitted, image_block, invalid_request, merge_or_push,
    native_max_output_tokens, translate_reasoning_effort_for_model, translate_tool_function,
    unsupported_parameter,
};
#[cfg(test)]
use crate::anthropic::convert_error_response;
use crate::anthropic_stream::AnthropicStreamState;
use crate::codex::new_id;
use crate::openai_responses_stream::ResponsesEventEncoder;
use crate::proxy::{
    collect_response_bytes, forward, read_anthropic_body_bounded, without_not_started,
    BodyAdmitError, UNSUPPORTED_CONTENT_ENCODING_MESSAGE,
};
use crate::request_classification::classify_openai_responses;
use crate::state::AppState;
use crate::validation::{optional_bool, optional_positive_u64};

/// Хендлер `POST /v1/responses`. Роут монтируется на Anthropic-плоскости и на выделенной
/// KIMI-плоскости: unified router проксирует этот путь на origin 8803 без переписывания.
/// Точный паттерн `anthropic_chat_completions`.
pub async fn anthropic_responses(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    let (parts, body) = request.into_parts();
    let bounded_body = match read_anthropic_body_bounded(&app, &parts.headers, body).await {
        Ok(body) => body,
        Err(BodyAdmitError::ContentEncoding) => {
            return chat_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                UNSUPPORTED_CONTENT_ENCODING_MESSAGE,
                None,
                json!("unsupported_content_encoding"),
                "unsupported_content_encoding",
            )
        }
        Err(BodyAdmitError::Storage(bounded_body::StorageError::TooLarge))
        | Err(BodyAdmitError::Storage(bounded_body::StorageError::ArithmeticOverflow)) => {
            return chat_error(
                StatusCode::BAD_REQUEST,
                "Request body exceeds the 32 MiB limit.",
                None,
                Value::Null,
                "invalid_responses_request",
            )
        }
        Err(BodyAdmitError::Storage(bounded_body::StorageError::Io)) => {
            return chat_error(
                StatusCode::BAD_REQUEST,
                "Could not read request body.",
                None,
                Value::Null,
                "invalid_responses_request",
            )
        }
        Err(BodyAdmitError::Storage(_)) => {
            return chat_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Request body storage is unavailable.",
                None,
                Value::Null,
                "server_error",
            )
        }
    };
    let raw = bounded_body.bytes.clone();
    let _body_lease = bounded_body._lease;
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
    // Preserve original accepted client intent before translation strips the namespace, rewrites
    // tools/history and injects Messages defaults. The temporary JSON is dropped here; the typed
    // carrier contains only bounded model/stream/classifier fields.
    let requested_model =
        crate::execution::SynthesizedMessagesOrigin::bounded_requested_model(&value);
    let classification = classify_openai_responses(&value);
    let translated = match translate_responses_request(value) {
        Ok(translated) => translated,
        Err(response) => return response,
    };
    let synthesized_origin = crate::execution::SynthesizedMessagesOrigin::openai_responses(
        requested_model,
        translated.stream,
        classification,
    );

    // Внутренний запрос на /v1/messages: admission, reserve, ротация,
    // identity-инжект, tee-метеринг и settle выполняет общий forward().
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
        Err(e) => {
            elog::error("forward", format!("anthropic request build failed: {e}"));
            return chat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build the upstream request.",
                None,
                Value::Null,
                "internal_response_error",
            );
        }
    };
    let mut inner = Request::builder()
        .method(Method::POST)
        .uri("/v1/messages")
        .body(Body::from(body_bytes))
        .expect("static request builder is infallible");
    *inner.headers_mut() = headers;
    crate::execution::inherit_request_context(&parts.extensions, inner.extensions_mut());
    inner.extensions_mut().insert(synthesized_origin);
    let upstream = forward(State(app.clone()), ConnectInfo(peer), inner).await;

    if upstream.status() != StatusCode::OK {
        return convert_error_response_admitted(upstream, Some(&app)).await;
    }
    if translated.stream {
        stream_responses_response(upstream, translated.model)
    } else {
        json_responses_response_admitted(upstream, translated.model, Some(&app)).await
    }
}

/// Результат перевода Responses-запроса: тело Messages и параметры, нужные
/// для перевода ответа.
#[derive(Debug)]
struct Translated {
    body: Value,
    /// Запрошенная модель с уже снятым `anthropic/`-префиксом — фолбэк для
    /// поля `model` ответа, если апстрим его не вернул.
    model: String,
    stream: bool,
}

/// Перевод Responses-запроса в Messages JSON. Ошибки — готовые OpenAI-shaped
/// ответы (400).
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

    // `instructions` → первый system-блок; system/developer items из input
    // сливаются следом (порядок входа сохраняется).
    let mut system = Vec::new();
    match object.remove("instructions") {
        None | Some(Value::Null) => {}
        Some(Value::String(text)) => {
            if !text.is_empty() {
                system.push(json!({"type": "text", "text": text}));
            }
        }
        _ => {
            return Err(invalid_request(
                "Invalid type for parameter: instructions must be a string.",
                Some("instructions"),
            ))
        }
    }

    let conversation = match object.remove("input") {
        Some(Value::String(text)) => {
            if text.is_empty() {
                return Err(invalid_request(
                    "Missing or invalid required parameter: input.",
                    Some("input"),
                ));
            }
            vec![json!({"role": "user", "content": [{"type": "text", "text": text}]})]
        }
        Some(Value::Array(items)) => translate_input_items(items, &mut system)?,
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

    // Messages requires a concrete max_tokens. When Responses omitted its optional ceiling,
    // materialize the native model maximum so omission stays uncapped like the Codex lane.
    let max_tokens = optional_positive_u64(&object, &["max_output_tokens"])
        .map_err(|field| {
            invalid_request("max_output_tokens must be a positive integer.", Some(field))
        })?
        .unwrap_or_else(|| native_max_output_tokens(&model));

    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(model.clone()));
    body.insert("messages".to_string(), Value::Array(conversation));
    body.insert("max_tokens".to_string(), Value::from(max_tokens));
    body.insert("stream".to_string(), Value::Bool(stream));
    if !system.is_empty() {
        body.insert("system".to_string(), Value::Array(system));
    }
    // Honored-параметры с совпадающими именами.
    for key in ["temperature", "top_p"] {
        if let Some(value) = object.get(key).filter(|v| !v.is_null()) {
            body.insert(key.to_string(), value.clone());
        }
    }
    // Tools: Responses `tools[]` → Messages `tools[]`; tool_choice +
    // parallel_tool_calls=false → Messages tool_choice. Пустой список
    // инструментов и дефолтный tool_choice=auto в тело не вставляются.
    if let Some(tools) = object.get("tools").filter(|v| !v.is_null()) {
        let tools = translate_responses_tools(tools)?;
        if !tools.is_empty() {
            body.insert("tools".to_string(), Value::Array(tools));
        }
    }
    if let Some(choice) = translate_responses_tool_choice(&object)? {
        body.insert("tool_choice".to_string(), choice);
    }
    // Structured output (text.format) и reasoning (reasoning.effort) живут в
    // одном GA output_config, как в chat-адаптере (3.4a/3.4b).
    let mut output_config = Map::new();
    if let Some(format) = translate_text_format(object.get("text"))? {
        output_config.insert("format".to_string(), format);
    }
    let effort = translate_reasoning(object.get("reasoning"), &model)?;
    if let Some(effort) = effort {
        output_config.insert("effort".to_string(), Value::String(effort));
        // thinking-инжект (как 3.4c chat-адаптера): effort без явного thinking
        // включает adaptive thinking с видимой summary — на 4.6+ моделях
        // adaptive выключен по умолчанию (effort без thinking не даёт
        // рассуждения вовсе), а дефолтный display=omitted присылает
        // thinking-блоки с пустым текстом. Явный thinking клиента не
        // переопределяем: open list ниже его проксирует. На старых моделях
        // effort тоже снят: оба поля там отвергаются upstream.
        if object.get("thinking").filter(|v| !v.is_null()).is_none() {
            body.insert(
                "thinking".to_string(),
                json!({"type": "adaptive", "display": "summarized"}),
            );
        }
    }
    if !output_config.is_empty() {
        body.insert("output_config".to_string(), Value::Object(output_config));
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
    })
}

/// Ключи Responses-запроса, снятые переводом или capability matrix. Всё
/// остальное проксируется в Messages тело (открытый список; явный `thinking`
/// клиента — тоже, см. инжект выше).
const CONSUMED_KEYS: &[&str] = &[
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

/// Capability matrix (решение 3): параметр, который Messages не умеет, с
/// не-дефолтным значением → `400 unsupported_parameter`. Порядок правил
/// определяет, какой параметр назовёт ошибка.
fn check_capability_matrix(object: &Map<String, Value>) -> Result<(), Response> {
    let rules: [(&str, fn(&Value) -> bool); 9] = [
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

/// Решение 5: stored responses — только `openai/*`. На Anthropic-плоскости
/// `store:true` и ссылка на сохранённый ответ — честный
/// `400 documented_limitation`, а не молчаливая потеря семантики.
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

/// Массив Responses input items → Messages messages (+ system-блоки).
/// Message item — `{type:"message", role, content}` либо компактная форма
/// `{role, content}` без type (её шлёт stock SDK). Подряд идущие одноролевые
/// сообщения склеиваются: Messages требует чередования user/assistant.
fn translate_input_items(
    items: Vec<Value>,
    system: &mut Vec<Value>,
) -> Result<Vec<Value>, Response> {
    let mut conversation: Vec<Value> = Vec::new();
    for item in &items {
        let object = item.as_object().ok_or_else(|| {
            invalid_request("Each input item must be a JSON object.", Some("input"))
        })?;
        match object.get("type").and_then(Value::as_str) {
            Some("message") => translate_message_item(object, system, &mut conversation)?,
            None if object.contains_key("role") => {
                translate_message_item(object, system, &mut conversation)?
            }
            // Replay истории tool calls (этап 4.2): pairing tool_use/
            // tool_result не валидируем — Messages сам честно отвечает 400
            // (то же поведение у chat-адаптера 3.2).
            Some("function_call") => {
                merge_or_push(&mut conversation, "assistant", vec![function_call_block(object)?])
            }
            Some("function_call_output") => merge_or_push(
                &mut conversation,
                "user",
                vec![function_call_output_block(object)?],
            ),
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
    if conversation.is_empty() {
        return Err(invalid_request(
            "input must contain at least one user or assistant message.",
            Some("input"),
        ));
    }
    Ok(conversation)
}

/// Один message item из input: system/developer → system-блоки,
/// user/assistant → сообщение Messages.
fn translate_message_item(
    object: &Map<String, Value>,
    system: &mut Vec<Value>,
    conversation: &mut Vec<Value>,
) -> Result<(), Response> {
    let role = object.get("role").and_then(Value::as_str).ok_or_else(|| {
        invalid_request("Each input message must have a string role.", Some("input"))
    })?;
    match role {
        "system" | "developer" => {
            let text = message_item_text(object.get("content"))?;
            if !text.is_empty() {
                system.push(json!({"type": "text", "text": text}));
            }
        }
        "user" | "assistant" => {
            let blocks = message_item_blocks(object.get("content"))?;
            merge_or_push(conversation, role, blocks);
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
/// через \n, как в chat-адаптере). Нетекстовых частей в system Messages нет —
/// 400.
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

/// Контент user/assistant item'а → массив Messages-блоков. Текстовые части
/// (`input_text`, `output_text`) склеиваются в text-блоки (через \n, как в
/// chat-адаптере); `input_image` разрывают склейку, порядок частей
/// сохраняется. Остальные part types (`input_file` и будущие) — 400.
fn message_item_blocks(content: Option<&Value>) -> Result<Vec<Value>, Response> {
    let Some(Value::Array(parts)) = content else {
        let text = message_item_text(content)?;
        if text.is_empty() {
            // Messages не принимает пустые text-блоки; пустое сообщение
            // бессмысленно и для Responses-входа — отклоняем честно.
            return Err(invalid_request(
                "Input message content must not be empty.",
                Some("input"),
            ));
        }
        return Ok(vec![json!({"type": "text", "text": text})]);
    };
    let mut blocks: Vec<Value> = Vec::new();
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
                    blocks.push(json!({"type": "text", "text": std::mem::take(&mut text)}));
                }
                blocks.push(input_image_block(part)?);
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
        blocks.push(json!({"type": "text", "text": text}));
    }
    if blocks.is_empty() {
        return Err(invalid_request(
            "Input message content must not be empty.",
            Some("input"),
        ));
    }
    Ok(blocks)
}

/// Responses `input_image` part → Messages image-блок. Форма part'а
/// (`image_url` — строка, `detail` — sibling) адаптируется под chat-форму, и
/// дальше работает общий с chat-адаптером перевод (data: → base64 source,
/// http(s) → url source; `detail` != auto → `400 unsupported_parameter`).
fn input_image_block(part: &Value) -> Result<Value, Response> {
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
    image_block(&json!({"type": "image_url", "image_url": image}), "input")
}

/// Replay function_call item'а (этап 4.2) → assistant `tool_use`-блок:
/// `call_id` → `id`, `arguments` (JSON-строка) парсится в `input` — как
/// `parse_tool_arguments` chat-адаптера (отсутствующая/пустая строка — `{}`,
/// невалидный JSON и не-object → `400 invalid_request`). Отсутствующие и
/// пустые `call_id`/`name` — 400.
fn function_call_block(object: &Map<String, Value>) -> Result<Value, Response> {
    let call_id = required_string(object, "call_id")?;
    let name = required_string(object, "name")?;
    let input = match object.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(Value::String(raw)) if raw.is_empty() => json!({}),
        Some(Value::String(raw)) => match serde_json::from_str(raw) {
            Ok(Value::Object(arguments)) => Value::Object(arguments),
            _ => {
                return Err(invalid_request(
                    "Invalid function_call arguments: expected a JSON object string.",
                    Some("input"),
                ))
            }
        },
        Some(_) => {
            return Err(invalid_request(
                "Invalid function_call arguments: expected a JSON string.",
                Some("input"),
            ))
        }
    };
    Ok(json!({"type": "tool_use", "id": call_id, "name": name, "input": input}))
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

/// Replay function_call_output item'а (этап 4.2) → user `tool_result`-блок
/// (зеркало `tool_result_block` chat-адаптера): `call_id` → `tool_use_id`,
/// `output` строка → text content как есть.
fn function_call_output_block(object: &Map<String, Value>) -> Result<Value, Response> {
    let call_id = required_string(object, "call_id")?;
    let text = function_call_output_text(object.get("output"))?;
    Ok(json!({"type": "tool_result", "tool_use_id": call_id, "content": text}))
}

/// `output` function_call_output item'а: строка либо массив text-партов
/// (`input_text`/`output_text`, склеиваются через \n — по образцу
/// `message_text` chat-адаптера). Нетекстовые части — 400.
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

/// Responses `tools[]` → Messages `tools[]`. Поддерживается только function
/// tool (`{type:"function", name, description, parameters, strict?}`):
/// `parameters` → `input_schema` (отсутствующая схема — пустой object),
/// `strict` снимается (constrained decoding Messages всегда строгий) — общий
/// с chat-адаптером перевод function-дескриптора. Любой другой tool type
/// (custom, web_search, file_search, mcp, …) → `400 unsupported_parameter`.
fn translate_responses_tools(value: &Value) -> Result<Vec<Value>, Response> {
    let tools = value.as_array().ok_or_else(|| {
        invalid_request(
            "Invalid type for parameter: tools must be an array.",
            Some("tools"),
        )
    })?;
    let mut translated = Vec::with_capacity(tools.len());
    for tool in tools {
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
        translated.push(translate_tool_function(function, "tools")?);
    }
    Ok(translated)
}

/// `tool_choice` + `parallel_tool_calls` → Messages `tool_choice` (как
/// chat-адаптер; именная форма Responses — плоская `{type:"function", name}`).
/// Дефолт (`auto` без disable_parallel_tool_use) не вставляется — Messages
/// без tool_choice и так auto.
fn translate_responses_tool_choice(object: &Map<String, Value>) -> Result<Option<Value>, Response> {
    let mut choice = match object.get("tool_choice").filter(|v| !v.is_null()) {
        Some(Value::String(mode)) => match mode.as_str() {
            "auto" => None,
            "required" => Some(json!({"type": "any"})),
            "none" => Some(json!({"type": "none"})),
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
            Some(json!({"type": "tool", "name": name}))
        }
        Some(_) => {
            return Err(invalid_request(
                "Invalid value for parameter: tool_choice.",
                Some("tool_choice"),
            ))
        }
        None => None,
    };
    if object.get("parallel_tool_calls").and_then(Value::as_bool) == Some(false) {
        let mut merged = choice.unwrap_or_else(|| json!({"type": "auto"}));
        merged["disable_parallel_tool_use"] = Value::Bool(true);
        choice = Some(merged);
    }
    Ok(choice)
}

/// `reasoning: {effort}` → `output_config.effort` (общий с chat-адаптером
/// перевод, этап 3.4b): null/отсутствие — выкл, minimal клампится в low,
/// любое другое не-null значение effort → 400 invalid_request.
fn translate_reasoning(value: Option<&Value>, model: &str) -> Result<Option<String>, Response> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let Some(effort) = value.get("effort") else {
        return Err(invalid_request(
            "Invalid value for parameter: reasoning (expected an object with effort).",
            Some("reasoning"),
        ));
    };
    translate_reasoning_effort_for_model(Some(effort), "reasoning", model)
}

/// `text` → `output_config.format` (GA structured outputs, как
/// `response_format` chat-адаптера 3.4a). Дефолт — `{format:{type:"text"}}`;
/// json_schema переводится снятием обёртки (name/strict/description не
/// проксируются — только схема); json_object у Messages нет →
/// `400 unsupported_parameter`; не-дефолтная verbosity (дефолт — "medium") →
/// `400 unsupported_parameter`.
fn translate_text_format(value: Option<&Value>) -> Result<Option<Value>, Response> {
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
            Ok(Some(json!({"type": "json_schema", "schema": schema})))
        }
        Some(_) => Err(unsupported_parameter("text")),
    }
}

/// `status`/`incomplete_details` Response object из Messages `stop_reason`.
fn map_status(stop_reason: Option<&str>) -> (&'static str, Value) {
    match stop_reason {
        Some("max_tokens") | Some("model_context_window_exceeded") => {
            ("incomplete", json!({"reason": "max_output_tokens"}))
        }
        Some("refusal") => ("incomplete", json!({"reason": "content_filter"})),
        // end_turn / stop_sequence / tool_use / pause_turn / отсутствует.
        _ => ("completed", Value::Null),
    }
}

/// Responses `usage` из Messages usage. Входная сторона включает cache
/// creation/read (как prompt в chat-адаптере; биллинг тарифицирует их
/// отдельно внутри), cache read отражается в
/// `input_tokens_details.cached_tokens` (только при >0), thinking-токены — в
/// `output_tokens_details.reasoning_tokens`.
fn map_responses_usage(usage: &Value) -> Value {
    let tokens = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    if [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
        "output_tokens",
    ]
    .iter()
    .any(|key| usage.get(key).and_then(Value::as_u64).is_none())
    {
        elog::warn("forward", "anthropic usage fields missing; zero-filled");
    }
    let input = tokens("input_tokens")
        .saturating_add(tokens("cache_creation_input_tokens"))
        .saturating_add(tokens("cache_read_input_tokens"));
    let output = tokens("output_tokens");
    let mut mapped = json!({
        "input_tokens": input,
        "output_tokens": output,
        "total_tokens": input.saturating_add(output),
    });
    let cache_read = tokens("cache_read_input_tokens");
    if cache_read > 0 {
        mapped["input_tokens_details"] = json!({"cached_tokens": cache_read});
    }
    let reasoning = usage
        .get("output_tokens_details")
        .and_then(|d| d.get("thinking_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if reasoning > 0 {
        mapped["output_tokens_details"] = json!({"reasoning_tokens": reasoning});
    }
    mapped
}

/// message output item с одним output_text part.
fn message_item(text: &str, status: &str) -> Value {
    json!({
        "type": "message",
        "id": new_id("msg"),
        "status": status,
        "role": "assistant",
        "content": [{"type": "output_text", "text": text, "annotations": [], "logprobs": []}],
    })
}

/// function_call output item: `call_id` — tool_use id Messages (клиент
/// ссылается на него в function_call_output следующего хода), `id` — свой
/// `fc_*` идентификатор item'а.
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
/// thinking-блока. encrypted_content не выставляется (решение 4).
fn reasoning_item(text: &str) -> Value {
    json!({
        "type": "reasoning",
        "id": new_id("rs"),
        "summary": [{"type": "summary_text", "text": text}],
    })
}

/// Messages content-блоки → Responses output items (non-stream): thinking →
/// reasoning items (пустой thinking — item не создаётся; redacted_thinking
/// пропускается — решение 4), text-блоки склеиваются в ОДИН message item
/// (без текста item не создаётся) на позиции первого text-блока, tool_use →
/// function_call items. Items идут в порядке появления блоков; неизвестные
/// блоки пропускаются.
fn output_items(blocks: Option<&Vec<Value>>) -> Vec<Value> {
    let mut output: Vec<Value> = Vec::new();
    let mut text = String::new();
    // Позиция message item — позиция первого text-блока среди остальных items.
    let mut text_at: Option<usize> = None;
    for block in blocks.into_iter().flatten() {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if text_at.is_none() {
                    text_at = Some(output.len());
                }
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    text.push_str(t);
                }
            }
            Some("thinking") => {
                let thinking = block
                    .get("thinking")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !thinking.is_empty() {
                    output.push(reasoning_item(thinking));
                }
            }
            Some("tool_use") => {
                let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let missing_input = block.get("input").is_none();
                let input = block
                    .get("input")
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| "{}".to_string());
                if id.is_empty() || name.is_empty() || missing_input {
                    elog::warn(
                        "forward",
                        "anthropic tool_use block missing id/name/input; degraded function_call",
                    );
                }
                output.push(function_call_item(id, name, &input, "completed"));
            }
            _ => {}
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

/// Перевод non-stream Messages-ответа в Response object (словарь 4.1).
#[cfg(test)]
async fn json_responses_response(upstream: Response, requested_model: String) -> Response {
    json_responses_response_admitted(upstream, requested_model, None).await
}

async fn json_responses_response_admitted(
    upstream: Response,
    requested_model: String,
    app: Option<&AppState>,
) -> Response {
    let request_id = upstream.headers().get("request-id").cloned();
    let (bytes, _lease) = match collect_response_bytes(
        app,
        upstream.into_body(),
        api_limits::current::TRANSLATED_NONSTREAM_RESPONSE,
    )
    .await
    {
        Ok(collected) => collected,
        Err(e) => {
            elog::error(
                "forward",
                format!("anthropic response body read failed: {e:?}"),
            );
            return without_not_started(chat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The provider returned an unreadable response.",
                None,
                Value::Null,
                "internal_response_error",
            ));
        }
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(e) => {
            elog::error("forward", format!("anthropic response body not JSON: {e}"));
            return without_not_started(chat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The provider returned a malformed response.",
                None,
                Value::Null,
                "internal_response_error",
            ));
        }
    };
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(&requested_model)
        .to_string();
    let output = output_items(value.get("content").and_then(Value::as_array));
    let (status, incomplete_details) = map_status(value.get("stop_reason").and_then(Value::as_str));
    let usage = value
        .get("usage")
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

/// Перевод SSE-ответа `forward()` в поток Responses SSE. Транслятор
/// навешивается СНАРУЖИ ответа forward(): TeeMeter внутри уже протапал
/// оригинальные Anthropic-байты (usage/settle не меняются), а SseErrorTail
/// гарантирует `event: error` на транспортном обрыве — он переводится в
/// `response.failed` по тому же правилу.
fn stream_responses_response(upstream: Response, requested_model: String) -> Response {
    let request_id = upstream.headers().get("request-id").cloned();
    let stream = upstream
        .into_body()
        .into_data_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let translator = ResponsesSseTranslator::new(Box::pin(stream), requested_model);
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

/// Незакрытый контент-блок Messages в переводе: накопленные text/arguments
/// нужны для done-событий и финального Response object.
enum OpenBlock {
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
    FunctionCall {
        output_index: u64,
        item_id: String,
        call_id: String,
        name: String,
        arguments: String,
    },
}

/// Потоковый транслятор Messages SSE → Responses SSE (словарь 4.1 в шапке
/// модуля). Буферизуются только байты одного незакрытого SSE-кадра; готовые
/// события отдаются немедленно (первый чанк не ждёт конца стрима).
struct ResponsesSseTranslator {
    inner: ByteStream,
    buf: BytesMut,
    out: VecDeque<Bytes>,
    events: ResponsesEventEncoder,
    id: String,
    created: i64,
    /// Запрошенная (native) модель — фолбэк, пока/если `message_start` не
    /// сообщил сервёную.
    requested_model: String,
    served_model: Option<String>,
    /// `response.created`/`response.in_progress` уже отправлены.
    started: bool,
    /// usage из `message_start` (input-сторона + cache поля).
    start_usage: Option<Value>,
    output_tokens: Option<u64>,
    /// `output_tokens_details` из `message_delta` (thinking-токены —
    /// reasoning_tokens в Responses usage).
    output_tokens_details: Option<Value>,
    stop_reason: Option<String>,
    /// Messages block index → незакрытый output item.
    blocks: std::collections::HashMap<u64, OpenBlock>,
    /// Плотный счётчик output_index: text/thinking/tool_use блоки позицию
    /// занимают, redacted_thinking и неизвестные — нет.
    next_output_index: u64,
    /// Финализированные output items — для output в response.completed/failed.
    completed_items: Vec<Value>,
    source: AnthropicStreamState,
    /// Терминальное событие (completed/failed) уже поставлено в `out`.
    finished: bool,
}

impl ResponsesSseTranslator {
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
            start_usage: None,
            output_tokens: None,
            output_tokens_details: None,
            stop_reason: None,
            blocks: std::collections::HashMap::new(),
            next_output_index: 0,
            completed_items: Vec::new(),
            source: AnthropicStreamState::default(),
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
    /// клиент-видимого события (даже если message_start не пришёл).
    fn ensure_started(&mut self) {
        if !self.started {
            self.started = true;
            let shell = self.shell();
            self.push_response_event("response.created", shell.clone());
            self.push_response_event("response.in_progress", shell);
        }
    }

    /// Терминальное событие сбоя: response object со status failed и
    /// санитизированной ошибкой апстрима (code — Anthropic-тип, как в error
    /// frame chat-адаптера), затем конец стрима.
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

    /// Один уже проверенный обязательной source-state machine SSE-кадр.
    fn handle_event(&mut self, event: &str, data: Value) {
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
                self.ensure_started();
            }
            "content_block_start" => {
                self.ensure_started();
                let block_index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
                let block = data.get("content_block");
                match block.and_then(|b| b.get("type")).and_then(Value::as_str) {
                    Some("text") => {
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
                        self.blocks.insert(
                            block_index,
                            OpenBlock::Text {
                                output_index,
                                item_id,
                                text: String::new(),
                            },
                        );
                    }
                    Some("tool_use") => {
                        let output_index = self.next_output_index;
                        self.next_output_index += 1;
                        let item_id = new_id("fc");
                        let call_id = block
                            .and_then(|b| b.get("id"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let name = block
                            .and_then(|b| b.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        if call_id.is_empty() || name.is_empty() {
                            elog::warn(
                                "forward",
                                "anthropic tool_use block missing id/name; degraded function_call",
                            );
                        }
                        self.push_event(
                            "response.output_item.added",
                            json!({
                                "output_index": output_index,
                                "item": {"type": "function_call", "id": item_id,
                                    "call_id": call_id, "name": name,
                                    "arguments": "", "status": "in_progress"},
                            }),
                        );
                        self.blocks.insert(
                            block_index,
                            OpenBlock::FunctionCall {
                                output_index,
                                item_id,
                                call_id,
                                name,
                                arguments: String::new(),
                            },
                        );
                    }
                    Some("thinking") => {
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
                        self.blocks.insert(
                            block_index,
                            OpenBlock::Reasoning {
                                output_index,
                                item_id,
                                text: String::new(),
                            },
                        );
                    }
                    // redacted_thinking и неизвестные блоки пропускаются
                    // (решение 4); позицию output_index они НЕ занимают
                    // (плотный счётчик).
                    _ => {}
                }
            }
            "content_block_delta" => {
                let block_index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
                let Some(delta) = data.get("delta") else {
                    return;
                };
                match self.blocks.get_mut(&block_index) {
                    Some(OpenBlock::Text {
                        output_index,
                        item_id,
                        text,
                    }) if delta.get("type").and_then(Value::as_str) == Some("text_delta") => {
                        if let Some(segment) = delta
                            .get("text")
                            .and_then(Value::as_str)
                            .filter(|t| !t.is_empty())
                        {
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
                        }
                    }
                    Some(OpenBlock::FunctionCall {
                        output_index,
                        item_id,
                        arguments,
                        ..
                    }) if delta.get("type").and_then(Value::as_str) == Some("input_json_delta") => {
                        // partial_json уезжает как есть — это уже сегмент
                        // JSON-строки arguments в терминах Responses.
                        if let Some(partial) = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .filter(|p| !p.is_empty())
                        {
                            arguments.push_str(partial);
                            let (output_index, item_id) = (*output_index, item_id.clone());
                            self.push_event(
                                "response.function_call_arguments.delta",
                                json!({
                                    "output_index": output_index,
                                    "item_id": item_id,
                                    "delta": partial,
                                }),
                            );
                        }
                    }
                    Some(OpenBlock::Reasoning {
                        output_index,
                        item_id,
                        text,
                    }) if delta.get("type").and_then(Value::as_str) == Some("thinking_delta") => {
                        if let Some(segment) = delta
                            .get("thinking")
                            .and_then(Value::as_str)
                            .filter(|t| !t.is_empty())
                        {
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
                    }
                    // signature_delta дропается (решение 4), дельты
                    // пропущенных блоков (redacted_thinking, неизвестные)
                    // выбрасываются.
                    _ => {}
                }
            }
            "content_block_stop" => {
                let block_index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
                match self.blocks.remove(&block_index) {
                    Some(OpenBlock::Text {
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
                    Some(OpenBlock::Reasoning {
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
                    Some(OpenBlock::FunctionCall {
                        output_index,
                        item_id,
                        call_id,
                        name,
                        arguments,
                    }) => {
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
                    None => {}
                }
            }
            "message_delta" => {
                if let Some(usage) = data.get("usage") {
                    if let Some(output) = usage.get("output_tokens").and_then(Value::as_u64) {
                        self.output_tokens = Some(output);
                    }
                    // thinking-токены стрима (reasoning 4.2) — проксируются в
                    // Responses usage (reasoning_tokens), как в non-stream.
                    if let Some(details) = usage.get("output_tokens_details") {
                        self.output_tokens_details = Some(details.clone());
                    }
                }
                if let Some(stop_reason) = data
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    self.stop_reason = Some(stop_reason.to_string());
                }
            }
            "message_stop" => {
                if self.finished {
                    return;
                }
                self.ensure_started();
                let (status, incomplete_details) = map_status(self.stop_reason.as_deref());
                let mut usage = self.start_usage.take().unwrap_or_else(|| json!({}));
                if let Some(output) = self.output_tokens {
                    usage["output_tokens"] = Value::from(output);
                }
                if let Some(details) = self.output_tokens_details.take() {
                    usage["output_tokens_details"] = details;
                }
                let response = json!({
                    "id": self.id,
                    "object": "response",
                    "created_at": self.created,
                    "status": status,
                    "model": self.model(),
                    "output": self.completed_items,
                    "usage": map_responses_usage(&usage),
                    "error": Value::Null,
                    "incomplete_details": incomplete_details,
                });
                self.push_response_event("response.completed", response);
                self.finished = true;
            }
            "ping" => {
                // Keepalive: валидный SSE comment-кадр без событийной семантики.
                self.out.push_back(Bytes::from_static(b": ping\n\n"));
            }
            "error" => {
                let error = data.get("error");
                let message = error
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("The provider returned an error.");
                let code = error.and_then(|e| e.get("type")).and_then(Value::as_str);
                elog::warn(
                    "forward",
                    format!("anthropic mid-stream error event: {message}"),
                );
                self.push_failed(message, code);
            }
            // Неизвестные события не несут client-visible дельт.
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
            match self.source.accept(event, &data) {
                Ok(Some(data)) => self.handle_event(event, data),
                Ok(None) => {}
                Err(e) => {
                    elog::error("forward", format!("anthropic SSE protocol violation: {e}"));
                    self.push_failed(
                        "The provider stream contained a malformed event.",
                        Some("protocol_error"),
                    )
                }
            }
            if self.finished {
                self.buf.clear();
                return;
            }
        }
    }
}

impl Stream for ResponsesSseTranslator {
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
                    self.push_failed("The provider stream was interrupted.", None);
                }
                Poll::Ready(None) => {
                    if self.buf.iter().any(|byte| !byte.is_ascii_whitespace()) {
                        self.buf.extend_from_slice(b"\n\n");
                        self.drain_frames();
                    }
                    if !self.finished {
                        elog::warn("forward", "anthropic stream ended before message_stop");
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
    /// Response object, чтобы assertions ниже читались так же, как раньше;
    /// comment-кадр (`: ping`) sequence не потребляет.
    fn event_frames(output: &str) -> Vec<(String, Value)> {
        let mut expected_sequence = 0_u64;
        output
            .split("\n\n")
            .filter(|frame| !frame.trim().is_empty())
            .map(|frame| {
                if frame.starts_with(':') {
                    return ("comment".to_string(), Value::Null);
                }
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
    fn translates_basic_responses_to_messages() {
        let translated = ok_translated(serde_json::json!({
            "model": "anthropic/claude-opus-4-8",
            "instructions": "Be terse.",
            "input": "Hello"
        }));
        let body = &translated.body;
        assert_eq!(body["model"], "claude-opus-4-8");
        assert_eq!(
            body["max_tokens"],
            native_max_output_tokens("claude-opus-4-8")
        );
        assert_eq!(body["stream"], false);
        assert!(!translated.stream);
        assert_eq!(
            body["system"],
            serde_json::json!([{"type": "text", "text": "Be terse."}])
        );
        assert_eq!(
            body["messages"],
            serde_json::json!([{"role": "user", "content": [{"type": "text", "text": "Hello"}]}])
        );
    }

    #[test]
    fn null_optional_controls_keep_responses_defaults() {
        let translated = ok_translated(serde_json::json!({
            "model": "anthropic/claude-opus-4-8",
            "input": "Hello",
            "stream": null,
            "max_output_tokens": null
        }));
        assert!(!translated.stream);
        assert_eq!(
            translated.body["max_tokens"],
            native_max_output_tokens("claude-opus-4-8")
        );
    }

    #[test]
    fn omitted_responses_limit_uses_legacy_model_ceiling() {
        let translated = ok_translated(serde_json::json!({
            "model": "anthropic/claude-haiku-4-5-20251001",
            "input": "Hello"
        }));
        assert_eq!(translated.body["max_tokens"], 64_000);
    }

    #[tokio::test]
    async fn malformed_responses_controls_are_parameter_specific_400s() {
        for (field, value) in [
            ("stream", serde_json::json!("false")),
            ("max_output_tokens", serde_json::json!(0)),
            ("max_output_tokens", serde_json::json!(-1)),
            ("max_output_tokens", serde_json::json!(1.5)),
            ("max_output_tokens", serde_json::json!("10")),
            ("max_output_tokens", serde_json::json!({})),
        ] {
            let mut request = serde_json::json!({
                "model": "anthropic/claude-opus-4-8",
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
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
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
            translated.body["system"],
            serde_json::json!([
                {"type": "text", "text": "Be terse."},
                {"type": "text", "text": "System line."},
                {"type": "text", "text": "Dev one.\nDev two."}
            ])
        );
        assert_eq!(
            translated.body["messages"],
            serde_json::json!([{"role": "user", "content": [{"type": "text", "text": "Hello"}]}])
        );
    }

    #[test]
    fn compact_items_and_assistant_output_text_translate() {
        // Компактная форма `{role, content}` без type (её шлёт stock SDK) и
        // output_text в assistant — текстовые блоки; одноролевые склеиваются.
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": [
                {"role": "user", "content": "one"},
                {"role": "user", "content": [{"type": "input_text", "text": "two"}]},
                {"role": "assistant", "content": [
                    {"type": "output_text", "text": "answer"}
                ]}
            ]
        }));
        assert_eq!(
            translated.body["messages"],
            serde_json::json!([
                {"role": "user", "content": [
                    {"type": "text", "text": "one"},
                    {"type": "text", "text": "two"}
                ]},
                {"role": "assistant", "content": [{"type": "text", "text": "answer"}]}
            ])
        );
        // instructions отсутствует — system-блоков нет.
        assert!(translated.body.get("system").is_none());
    }

    #[test]
    fn translates_input_image_parts() {
        // data: URL → base64 source, http(s) → url source (общий перевод
        // chat-адаптера); text-склейка разрывается image-блоками.
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": [{"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "What is this?"},
                {"type": "input_image", "image_url": "data:image/png;base64,iVBORw0KGgo="},
                {"type": "input_image", "image_url": "https://example.com/cat.jpg", "detail": "auto"},
                {"type": "input_text", "text": "And this?"}
            ]}]
        }));
        assert_eq!(
            translated.body["messages"],
            serde_json::json!([{"role": "user", "content": [
                {"type": "text", "text": "What is this?"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="}},
                {"type": "image", "source": {"type": "url", "url": "https://example.com/cat.jpg"}},
                {"type": "text", "text": "And this?"}
            ]}])
        );
    }

    #[tokio::test]
    async fn rejects_invalid_input_image_parts() {
        for content in [
            // detail != auto — Messages не умеет.
            serde_json::json!([{"type": "input_image", "image_url": "https://x/y.png", "detail": "high"}]),
            // Битый data: URL.
            serde_json::json!([{"type": "input_image", "image_url": "data:image/png;plain,xyz"}]),
            // Схема вне http(s)/data:.
            serde_json::json!([{"type": "input_image", "image_url": "file:///etc/passwd"}]),
            // Нет image_url.
            serde_json::json!([{"type": "input_image"}]),
        ] {
            let (status, json) = expect_err(serde_json::json!({
                "model": "claude-opus-4-8",
                "input": [{"type": "message", "role": "user", "content": content}],
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{content}");
            assert_eq!(json["error"]["param"], "input", "{content}");
        }
    }

    #[test]
    fn tools_translate_to_messages_tools() {
        // Responses-форма плоская; strict снимается, отсутствующая схема —
        // пустой object (общий перевод function-дескриптора chat-адаптера).
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": "weather?",
            "tools": [
                {"type": "function", "name": "get_weather", "description": "Current weather",
                 "strict": true,
                 "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}},
                {"type": "function", "name": "no_args"}
            ]
        }));
        assert_eq!(
            translated.body["tools"],
            serde_json::json!([
                {"name": "get_weather", "description": "Current weather",
                 "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}}},
                {"name": "no_args", "input_schema": {"type": "object"}}
            ])
        );
    }

    #[tokio::test]
    async fn non_function_tools_are_400_unsupported_parameter() {
        for tool in [
            serde_json::json!({"type": "custom", "name": "x"}),
            serde_json::json!({"type": "web_search"}),
            serde_json::json!({"type": "file_search", "vector_store_ids": ["vs_1"]}),
            serde_json::json!({"type": "mcp", "server_label": "srv"}),
        ] {
            let (status, json) = expect_err(serde_json::json!({
                "model": "claude-opus-4-8",
                "input": "hi",
                "tools": [tool],
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{tool}");
            assert_eq!(json["error"]["code"], "unsupported_parameter", "{tool}");
            assert_eq!(json["error"]["param"], "tools", "{tool}");
        }
    }

    #[test]
    fn tool_choice_variants_map_to_messages_tool_choice() {
        // auto — дефолт Messages, в тело не вставляется.
        let translated = ok_translated(serde_json::json!({
            "model": "m", "input": "hi", "tool_choice": "auto"
        }));
        assert!(translated.body.get("tool_choice").is_none());

        let translated = ok_translated(serde_json::json!({
            "model": "m", "input": "hi", "tool_choice": "required"
        }));
        assert_eq!(
            translated.body["tool_choice"],
            serde_json::json!({"type": "any"})
        );

        let translated = ok_translated(serde_json::json!({
            "model": "m", "input": "hi", "tool_choice": "none"
        }));
        assert_eq!(
            translated.body["tool_choice"],
            serde_json::json!({"type": "none"})
        );

        // Именная форма Responses — плоская {type:"function", name}.
        let translated = ok_translated(serde_json::json!({
            "model": "m", "input": "hi",
            "tool_choice": {"type": "function", "name": "f"}
        }));
        assert_eq!(
            translated.body["tool_choice"],
            serde_json::json!({"type": "tool", "name": "f"})
        );
    }

    #[test]
    fn parallel_tool_calls_false_disables_parallel_use() {
        let translated = ok_translated(serde_json::json!({
            "model": "m", "input": "hi", "parallel_tool_calls": false
        }));
        assert_eq!(
            translated.body["tool_choice"],
            serde_json::json!({"type": "auto", "disable_parallel_tool_use": true})
        );
        let translated = ok_translated(serde_json::json!({
            "model": "m", "input": "hi",
            "tool_choice": "required", "parallel_tool_calls": false
        }));
        assert_eq!(
            translated.body["tool_choice"],
            serde_json::json!({"type": "any", "disable_parallel_tool_use": true})
        );
    }

    #[tokio::test]
    async fn invalid_tool_choice_is_400() {
        let (status, json) = expect_err(serde_json::json!({
            "model": "m", "input": "hi", "tool_choice": "sometimes"
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["param"], "tool_choice");

        let (status, _) = expect_err(serde_json::json!({
            "model": "m", "input": "hi", "tool_choice": {"type": "function"}
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // ---------- reasoning (как 3.4b/3.4c chat-адаптера) ----------

    #[test]
    fn reasoning_effort_maps_to_output_config_and_injects_thinking() {
        // minimal у Messages нет — клампится в low; остальные проксируются.
        // effort без явного thinking включает adaptive thinking с видимой
        // summary (как 3.4c chat-адаптера).
        for (effort, expected) in [
            ("minimal", "low"),
            ("low", "low"),
            ("medium", "medium"),
            ("high", "high"),
            ("xhigh", "xhigh"),
            ("max", "max"),
        ] {
            let translated = ok_translated(serde_json::json!({
                "model": "claude-opus-4-8",
                "input": "hi",
                "reasoning": {"effort": effort},
            }));
            assert_eq!(
                translated.body["output_config"]["effort"],
                serde_json::json!(expected),
                "{effort}"
            );
            assert_eq!(
                translated.body["thinking"],
                serde_json::json!({"type": "adaptive", "display": "summarized"}),
                "{effort}"
            );
        }
        // null/отсутствие — выкл: ни output_config, ни thinking не появляется.
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8", "input": "hi", "reasoning": null,
        }));
        assert!(translated.body.get("output_config").is_none());
        assert!(translated.body.get("thinking").is_none());
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8", "input": "hi", "reasoning": {"effort": null},
        }));
        assert!(translated.body.get("output_config").is_none());
        assert!(translated.body.get("thinking").is_none());
    }

    #[tokio::test]
    async fn claude_4_6_reasoning_accepts_max_but_rejects_xhigh() {
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-6",
            "input": "hi",
            "reasoning": {"effort": "max"},
        }));
        assert_eq!(translated.body["output_config"]["effort"], "max");
        assert_eq!(
            translated.body["thinking"],
            serde_json::json!({"type": "adaptive", "display": "summarized"})
        );

        let (status, json) = expect_err(serde_json::json!({
            "model": "claude-opus-4-6",
            "input": "hi",
            "reasoning": {"effort": "xhigh"},
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["param"], "reasoning");
        assert!(json["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("low, medium, high, max")));
    }

    #[test]
    fn reasoning_preserves_client_thinking() {
        // Явный thinking клиента (open list) не переопределяется инжектом.
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": "hi",
            "reasoning": {"effort": "high"},
            "thinking": {"type": "enabled", "budget_tokens": 2048},
        }));
        assert_eq!(translated.body["output_config"]["effort"], "high");
        assert_eq!(
            translated.body["thinking"],
            serde_json::json!({"type": "enabled", "budget_tokens": 2048})
        );
    }

    #[test]
    fn pre_4_6_reasoning_effort_degrades_to_model_default() {
        for model in [
            "claude-haiku-4-5-20251001",
            "anthropic/claude-sonnet-4-5-20250929",
            "claude-opus-4-20250514",
        ] {
            let translated = ok_translated(serde_json::json!({
                "model": model,
                "input": "title this",
                "reasoning": {"effort": "max"},
            }));
            assert!(translated.body.get("output_config").is_none(), "{model}");
            assert!(translated.body.get("thinking").is_none(), "{model}");
        }

        // text.format остаётся в output_config, даже когда effort снят.
        let translated = ok_translated(serde_json::json!({
            "model": "claude-haiku-4-5-20251001",
            "input": "title this",
            "reasoning": {"effort": "low"},
            "text": {"format": {"type": "json_schema", "name": "title", "strict": true,
                "schema": {"type": "object", "properties": {"title": {"type": "string"}}}}},
        }));
        assert!(translated.body["output_config"].get("effort").is_none());
        assert_eq!(
            translated.body["output_config"]["format"]["type"],
            "json_schema"
        );
        assert!(translated.body.get("thinking").is_none());
    }

    #[tokio::test]
    async fn rejects_invalid_reasoning() {
        // Любое не-null значение effort вне minimal|low|medium|high|xhigh|max → 400
        // invalid_request (не unsupported_parameter — параметр поддержан).
        let (status, json) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8", "input": "hi",
            "reasoning": {"effort": "extreme"},
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert_eq!(json["error"]["param"], "reasoning");
        assert!(json["error"]["code"].is_null());

        // reasoning без effort — битый запрос.
        let (status, _) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8", "input": "hi", "reasoning": {},
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // ---------- structured output (text.format) ----------

    #[test]
    fn translates_text_format_json_schema() {
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": "Extract.",
            "text": {"format": {"type": "json_schema", "name": "profile", "strict": true,
                "schema": {"type": "object", "properties": {"name": {"type": "string"}}}}}
        }));
        // Обёртка (name/strict) не проксируется — только сама схема.
        assert_eq!(
            translated.body["output_config"],
            serde_json::json!({"format": {"type": "json_schema", "schema": {
                "type": "object", "properties": {"name": {"type": "string"}}
            }}})
        );
        assert!(translated.body.get("text").is_none());
    }

    #[test]
    fn text_format_and_reasoning_share_output_config() {
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": "Extract.",
            "reasoning": {"effort": "medium"},
            "text": {"format": {"type": "json_schema", "name": "p",
                "schema": {"type": "object"}}}
        }));
        assert_eq!(translated.body["output_config"]["effort"], "medium");
        assert_eq!(
            translated.body["output_config"]["format"],
            serde_json::json!({"type": "json_schema", "schema": {"type": "object"}})
        );
    }

    #[tokio::test]
    async fn rejects_unsupported_text_format_and_verbosity() {
        // json_object у Messages нет → unsupported_parameter.
        let (status, json) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8", "input": "hi",
            "text": {"format": {"type": "json_object"}}
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"], "unsupported_parameter");
        assert_eq!(json["error"]["param"], "text");

        // Не-дефолтная verbosity (дефолт — medium) → unsupported_parameter.
        let (status, json) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8", "input": "hi", "text": {"verbosity": "low"}
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"], "unsupported_parameter");
        assert_eq!(json["error"]["param"], "text");

        // Дефолтная verbosity принимается.
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8", "input": "hi",
            "text": {"verbosity": "medium", "format": {"type": "text"}}
        }));
        assert!(translated.body.get("output_config").is_none());

        // json_schema без schema-объекта — битый запрос.
        let (status, _) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8", "input": "hi",
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
            ("store", serde_json::json!(true)),
            ("previous_response_id", serde_json::json!("resp_42")),
        ] {
            let (status, json) = expect_err(serde_json::json!({
                "model": "claude-opus-4-8", "input": "hi", field: value,
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{field}");
            assert_eq!(json["error"]["code"], "documented_limitation", "{field}");
            assert_eq!(json["error"]["type"], "invalid_request_error", "{field}");
            assert_eq!(json["error"]["param"], field, "{field}");
        }
        // Дефолты принимаются.
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8", "input": "hi",
            "store": false, "previous_response_id": null,
        }));
        assert!(translated.body.get("store").is_none());
    }

    #[tokio::test]
    async fn item_reference_is_documented_limitation_reasoning_item_is_dropped() {
        let (status, json) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": [
                {"type": "message", "role": "user", "content": "hi"},
                {"type": "item_reference", "id": "resp_42"}
            ],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"], "documented_limitation");
        assert_eq!(json["error"]["param"], "input");

        // reasoning item (подписи не выставляются — решение 4) принимается и
        // выбрасывается, диалог не ломается.
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": [
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "…"}]},
                {"type": "message", "role": "user", "content": "hi"}
            ],
        }));
        assert_eq!(
            translated.body["messages"],
            serde_json::json!([{"role": "user", "content": [{"type": "text", "text": "hi"}]}])
        );
    }

    // ---------- replay tool-истории (4.2) ----------

    #[test]
    fn function_call_items_replay_to_tool_use_and_tool_result_blocks() {
        // Stored history шлёт message assistant + function_call подряд —
        // одноролевая склейка собирает один assistant message с text+tool_use;
        // function_call_output склеивается со следующим user message.
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": [
                {"type": "message", "role": "user", "content": "weather?"},
                {"type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "Let me check."}
                ]},
                {"type": "function_call", "call_id": "call_1", "name": "get_weather",
                 "arguments": "{\"city\":\"Paris\"}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "sunny"},
                {"type": "message", "role": "user", "content": "thanks?"}
            ]
        }));
        assert_eq!(
            translated.body["messages"],
            serde_json::json!([
                {"role": "user", "content": [{"type": "text", "text": "weather?"}]},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Let me check."},
                    {"type": "tool_use", "id": "call_1", "name": "get_weather",
                     "input": {"city": "Paris"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "call_1", "content": "sunny"},
                    {"type": "text", "text": "thanks?"}
                ]}
            ])
        );
    }

    #[test]
    fn function_call_output_parts_glue_and_first_item_assistant_is_allowed() {
        // output массивом text-партов — склейка через \n; отсутствующая и
        // пустая строка arguments — `{}`; история может начинаться с
        // assistant (function_call первым item'ом) — Messages это принимает.
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
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
            translated.body["messages"],
            serde_json::json!([
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "call_1", "name": "f", "input": {}},
                    {"type": "tool_use", "id": "call_2", "name": "g", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "call_1",
                     "content": "line one\nline two"}
                ]}
            ])
        );
    }

    #[tokio::test]
    async fn function_call_invalid_arguments_and_missing_fields_are_400() {
        // Невалидный JSON, не-object JSON и не-строка arguments → 400
        // invalid_request (param input).
        for arguments in [
            serde_json::json!("not json"),
            serde_json::json!("[1]"),
            serde_json::json!({"x": 1}),
        ] {
            let (status, json) = expect_err(serde_json::json!({
                "model": "claude-opus-4-8",
                "input": [
                    {"type": "message", "role": "user", "content": "hi"},
                    {"type": "function_call", "call_id": "call_1", "name": "f",
                     "arguments": arguments}
                ],
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{arguments}");
            assert_eq!(
                json["error"]["type"], "invalid_request_error",
                "{arguments}"
            );
            assert_eq!(json["error"]["param"], "input", "{arguments}");
        }
        // Отсутствующие/пустые call_id и name → 400.
        for item in [
            serde_json::json!({"type": "function_call", "name": "f", "arguments": "{}"}),
            serde_json::json!({"type": "function_call", "call_id": "", "name": "f",
                "arguments": "{}"}),
            serde_json::json!({"type": "function_call", "call_id": "call_1", "arguments": "{}"}),
            serde_json::json!({"type": "function_call", "call_id": "call_1", "name": "",
                "arguments": "{}"}),
            serde_json::json!({"type": "function_call_output", "output": "done"}),
        ] {
            let (status, json) = expect_err(serde_json::json!({
                "model": "claude-opus-4-8",
                "input": [
                    {"type": "message", "role": "user", "content": "hi"},
                    item
                ],
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{item}");
            assert_eq!(json["error"]["param"], "input", "{item}");
        }
        // Нетекстовые части output — 400.
        let (status, json) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": [
                {"type": "message", "role": "user", "content": "hi"},
                {"type": "function_call_output", "call_id": "call_1", "output": [
                    {"type": "input_image", "image_url": "https://x/y.png"}
                ]}
            ],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["param"], "input");
    }

    // ---------- capability matrix ----------

    #[tokio::test]
    async fn unsupported_non_default_parameters_are_400() {
        for (field, value) in [
            ("background", serde_json::json!(true)),
            ("service_tier", serde_json::json!("flex")),
            ("truncation", serde_json::json!("auto")),
            ("include", serde_json::json!(["file_search_call.results"])),
            ("prompt_cache_key", serde_json::json!("key-1")),
            ("safety_identifier", serde_json::json!("user-1")),
            ("user", serde_json::json!("user-1")),
            ("metadata", serde_json::json!({"k": "v"})),
            ("max_tool_calls", serde_json::json!(3)),
        ] {
            let (status, json) = expect_err(serde_json::json!({
                "model": "claude-opus-4-8", "input": "hi", field: value,
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
            "input": "hi",
            "tools": [],
            "tool_choice": "auto",
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
        // Дефолтные matrix-ключи в Messages тело не проксируются.
        for key in [
            "tools",
            "tool_choice",
            "background",
            "service_tier",
            "truncation",
            "include",
            "store",
            "reasoning",
            "text",
            "output_config",
            "thinking",
        ] {
            assert!(translated.body.get(key).is_none(), "{key}");
        }
    }

    #[test]
    fn unknown_fields_proxy_into_messages_body() {
        // Открытый список (решение 3): неизвестные адаптеру поля уходят в
        // Messages тело как есть; валидация — на апстриме.
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": "hi",
            "top_k": 40,
            "future_openai_field": {"x": 1}
        }));
        assert_eq!(translated.body["top_k"], 40);
        assert_eq!(
            translated.body["future_openai_field"],
            serde_json::json!({"x": 1})
        );
    }

    #[test]
    fn max_output_tokens_maps_to_max_tokens() {
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8", "input": "hi", "max_output_tokens": 500,
        }));
        assert_eq!(translated.body["max_tokens"], 500);
        assert!(translated.body.get("max_output_tokens").is_none());

        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8", "input": "hi", "stream": true,
        }));
        assert!(translated.stream);
        assert_eq!(translated.body["stream"], true);
    }

    #[test]
    fn temperature_and_top_p_are_honored() {
        let translated = ok_translated(serde_json::json!({
            "model": "claude-opus-4-8", "input": "hi",
            "temperature": 0.7, "top_p": 0.9,
        }));
        assert_eq!(translated.body["temperature"], 0.7);
        assert_eq!(translated.body["top_p"], 0.9);
    }

    #[tokio::test]
    async fn structural_errors_are_openai_shaped_400() {
        // Нет model.
        let (status, json) = expect_err(serde_json::json!({"input": "hi"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["param"], "model");
        assert!(json["error"]["code"].is_null());

        // Пустой native id после strip'а префикса.
        let (status, json) = expect_err(serde_json::json!({
            "model": "anthropic/", "input": "hi"
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["param"], "model");

        // Нет input.
        let (status, json) = expect_err(serde_json::json!({"model": "claude-opus-4-8"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["param"], "input");

        // Пустая строка input.
        let (status, _) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8", "input": ""
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Пустой массив / только system items — Messages требует хотя бы одно
        // user/assistant сообщение.
        let (status, _) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8", "input": []
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": [{"type": "message", "role": "system", "content": "x"}]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Пустой контент сообщения.
        let (status, _) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": [{"type": "message", "role": "user", "content": ""}]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Неизвестный item type.
        let (status, json) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8",
            "input": [{"type": "computer_call", "call_id": "c1"}]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["param"], "input");

        // instructions не строка.
        let (status, json) = expect_err(serde_json::json!({
            "model": "claude-opus-4-8", "input": "hi", "instructions": ["x"]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["param"], "instructions");
    }

    // ---------- перевод ответа (non-stream) ----------

    #[test]
    fn status_mapping() {
        assert_eq!(map_status(Some("end_turn")).0, "completed");
        assert_eq!(map_status(Some("stop_sequence")).0, "completed");
        assert_eq!(map_status(Some("tool_use")).0, "completed");
        assert_eq!(map_status(Some("pause_turn")).0, "completed");
        assert_eq!(map_status(None).0, "completed");
        let (status, details) = map_status(Some("max_tokens"));
        assert_eq!(status, "incomplete");
        assert_eq!(details, serde_json::json!({"reason": "max_output_tokens"}));
        let (status, details) = map_status(Some("model_context_window_exceeded"));
        assert_eq!(status, "incomplete");
        assert_eq!(details, serde_json::json!({"reason": "max_output_tokens"}));
        let (status, details) = map_status(Some("refusal"));
        assert_eq!(status, "incomplete");
        assert_eq!(details, serde_json::json!({"reason": "content_filter"}));
    }

    #[test]
    fn usage_mapping_includes_cache_and_reasoning_tokens() {
        let usage = map_responses_usage(&serde_json::json!({
            "input_tokens": 100,
            "cache_creation_input_tokens": 20,
            "cache_read_input_tokens": 30,
            "output_tokens": 7,
            "output_tokens_details": {"thinking_tokens": 5}
        }));
        assert_eq!(usage["input_tokens"], 150);
        assert_eq!(usage["output_tokens"], 7);
        assert_eq!(usage["total_tokens"], 157);
        assert_eq!(usage["input_tokens_details"]["cached_tokens"], 30);
        assert_eq!(usage["output_tokens_details"]["reasoning_tokens"], 5);

        let usage = map_responses_usage(&serde_json::json!({
            "input_tokens": 5, "output_tokens": 2
        }));
        assert_eq!(usage["input_tokens"], 5);
        assert_eq!(usage["total_tokens"], 7);
        assert!(usage.get("input_tokens_details").is_none());
        assert!(usage.get("output_tokens_details").is_none());
    }

    #[tokio::test]
    async fn non_stream_response_maps_to_response_object() {
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
                    "usage": {"input_tokens": 12, "cache_read_input_tokens": 4,
                        "output_tokens": 5}
                })
                .to_string(),
            ))
            .unwrap();
        let response = json_responses_response(upstream, "claude-opus-4-8".into()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("request-id").unwrap(), "req_abc");
        let (_, json) = err_parts(response).await;
        assert_eq!(json["object"], "response");
        assert!(json["id"].as_str().unwrap().starts_with("resp_"));
        assert_eq!(json["status"], "completed");
        assert_eq!(json["model"], "claude-opus-4-8-20260101");
        assert!(json["error"].is_null());
        assert!(json["incomplete_details"].is_null());
        // text-блоки склеиваются в ОДИН message item с одним output_text part.
        assert_eq!(json["output"].as_array().unwrap().len(), 1);
        let item = &json["output"][0];
        assert_eq!(item["type"], "message");
        assert!(item["id"].as_str().unwrap().starts_with("msg_"));
        assert_eq!(item["status"], "completed");
        assert_eq!(item["role"], "assistant");
        assert_eq!(
            item["content"],
            serde_json::json!([{"type": "output_text", "text": "Hello, world",
                "annotations": [], "logprobs": []}])
        );
        // input = 12 + cache_read 4; cached_tokens отражён в details.
        assert_eq!(json["usage"]["input_tokens"], 16);
        assert_eq!(json["usage"]["output_tokens"], 5);
        assert_eq!(json["usage"]["total_tokens"], 21);
        assert_eq!(json["usage"]["input_tokens_details"]["cached_tokens"], 4);
    }

    #[tokio::test]
    async fn non_stream_tool_use_maps_to_function_call_items() {
        let upstream = upstream_json(serde_json::json!({
            "id": "msg_1", "type": "message", "role": "assistant",
            "model": "claude-opus-4-8-20260101",
            "content": [
                {"type": "tool_use", "id": "toolu_1", "name": "get_weather",
                 "input": {"city": "Paris"}},
                {"type": "tool_use", "id": "toolu_2", "name": "no_args", "input": {}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 12, "output_tokens": 5}
        }));
        let response = json_responses_response(upstream, "claude-opus-4-8".into()).await;
        let (_, json) = err_parts(response).await;
        assert_eq!(json["status"], "completed");
        // Без текста message item не создаётся; function_call items — в
        // порядке tool_use-блоков, call_id = tool_use id.
        let output = json["output"].as_array().unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0]["type"], "function_call");
        assert!(output[0]["id"].as_str().unwrap().starts_with("fc_"));
        assert_eq!(output[0]["call_id"], "toolu_1");
        assert_eq!(output[0]["name"], "get_weather");
        assert_eq!(output[0]["arguments"], "{\"city\":\"Paris\"}");
        assert_eq!(output[0]["status"], "completed");
        assert_eq!(output[1]["call_id"], "toolu_2");
        assert_eq!(output[1]["arguments"], "{}");
    }

    #[tokio::test]
    async fn non_stream_text_and_tool_use_keep_both_in_order() {
        let upstream = upstream_json(serde_json::json!({
            "id": "msg_1", "type": "message", "role": "assistant",
            "model": "claude-opus-4-8-20260101",
            "content": [
                {"type": "text", "text": "Let me check."},
                {"type": "tool_use", "id": "toolu_1", "name": "f", "input": {}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 3, "output_tokens": 9}
        }));
        let response = json_responses_response(upstream, "claude-opus-4-8".into()).await;
        let (_, json) = err_parts(response).await;
        let output = json["output"].as_array().unwrap();
        // message item первым, затем function_call.
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["content"][0]["text"], "Let me check.");
        assert_eq!(output[1]["type"], "function_call");
    }

    #[tokio::test]
    async fn non_stream_max_tokens_is_incomplete_refusal_is_content_filter() {
        for (stop_reason, reason) in [
            ("max_tokens", "max_output_tokens"),
            ("model_context_window_exceeded", "max_output_tokens"),
            ("refusal", "content_filter"),
        ] {
            let upstream = upstream_json(serde_json::json!({
                "id": "msg_1", "type": "message", "role": "assistant",
                "model": "claude-opus-4-8-20260101",
                "content": [{"type": "text", "text": "partial"}],
                "stop_reason": stop_reason,
                "usage": {"input_tokens": 3, "output_tokens": 9}
            }));
            let response = json_responses_response(upstream, "claude-opus-4-8".into()).await;
            let (_, json) = err_parts(response).await;
            assert_eq!(json["status"], "incomplete", "{stop_reason}");
            assert_eq!(
                json["incomplete_details"]["reason"], reason,
                "{stop_reason}"
            );
        }
    }

    #[tokio::test]
    async fn non_stream_thinking_blocks_become_reasoning_items() {
        // thinking-блоки → reasoning items (4.2): каждый блок — отдельный
        // item в порядке появления среди остальных items; пустой thinking и
        // redacted_thinking item не порождают (решение 4).
        let upstream = upstream_json(serde_json::json!({
            "id": "msg_1", "type": "message", "role": "assistant",
            "model": "claude-opus-4-8-20260101",
            "content": [
                {"type": "thinking", "thinking": "First thought.", "signature": "sig_1"},
                {"type": "text", "text": "Answer."},
                {"type": "thinking", "thinking": "Second thought.", "signature": "sig_2"},
                {"type": "thinking", "thinking": "", "signature": "sig_3"},
                {"type": "redacted_thinking", "data": "opaque"},
                {"type": "tool_use", "id": "toolu_1", "name": "f", "input": {}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 3, "output_tokens": 9,
                "output_tokens_details": {"thinking_tokens": 7}}
        }));
        let response = json_responses_response(upstream, "claude-opus-4-8".into()).await;
        let (_, json) = err_parts(response).await;
        let output = json["output"].as_array().unwrap();
        // [reasoning, message, reasoning, function_call] — порядок блоков.
        assert_eq!(output.len(), 4, "{json}");
        assert_eq!(output[0]["type"], "reasoning");
        assert!(output[0]["id"].as_str().unwrap().starts_with("rs_"));
        assert_eq!(
            output[0]["summary"],
            serde_json::json!([{"type": "summary_text", "text": "First thought."}])
        );
        // Подписи и encrypted_content не выставляются (решение 4).
        assert!(output[0].get("encrypted_content").is_none());
        assert_eq!(output[1]["type"], "message");
        assert_eq!(output[1]["content"][0]["text"], "Answer.");
        assert_eq!(output[2]["type"], "reasoning");
        assert_eq!(output[2]["summary"][0]["text"], "Second thought.");
        assert_eq!(output[3]["type"], "function_call");
        assert!(!json.to_string().contains("sig_"), "{json}");
        assert_eq!(
            json["usage"]["output_tokens_details"]["reasoning_tokens"],
            7
        );
    }

    #[test]
    fn local_adapter_errors_mark_execution_not_started() {
        let response = translate_responses_request(serde_json::json!({})).unwrap_err();
        assert_eq!(
            response
                .headers()
                .get(crate::proxy::EXECUTION_STATE_HEADER)
                .unwrap(),
            crate::proxy::EXECUTION_STATE_NOT_STARTED
        );
    }

    #[tokio::test]
    async fn malformed_2xx_response_does_not_mark_execution_not_started() {
        let upstream = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("not json"))
            .unwrap();
        let response = json_responses_response(upstream, "claude-opus-4-8".into()).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(response
            .headers()
            .get(crate::proxy::EXECUTION_STATE_HEADER)
            .is_none());
    }

    // ---------- SSE-транслятор: contract-тесты словаря событий 4.1–4.2 ----------
    //
    // Каноническая последовательность Messages-событий → точная
    // последовательность Responses SSE. Это и есть контракт событий universal
    // responses (решение 2); Gemini-зеркало (этап 4.3) обязано воспроизводить
    // те же кадры на эквивалентном диалоге.

    const SSE_TEXT_DIALOG: &str = r#"event: message_start
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
    async fn contract_text_dialog_event_dictionary() {
        // text: created → in_progress → item.added → part.added →
        // text.delta* → text.done → part.done → item.done → completed.
        let translator = ResponsesSseTranslator::new(sse_bytes(SSE_TEXT_DIALOG), "m".into());
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
        // Shell стартовых событий.
        assert_eq!(frames[0].1["object"], "response");
        assert!(frames[0].1["id"].as_str().unwrap().starts_with("resp_"));
        assert_eq!(frames[0].1["status"], "in_progress");
        assert_eq!(frames[0].1["output"], serde_json::json!([]));
        assert_eq!(frames[0].1["model"], "claude-opus-4-8-20260101");
        assert_eq!(frames[0].1, frames[1].1);
        // Item lifecycle: плотный output_index 0, content_index 0, item_id
        // стабилен внутри блока.
        assert_eq!(frames[2].1["output_index"], 0);
        assert_eq!(frames[2].1["item"]["type"], "message");
        assert_eq!(frames[2].1["item"]["status"], "in_progress");
        assert_eq!(frames[2].1["item"]["content"], serde_json::json!([]));
        let item_id = frames[2].1["item"]["id"].clone();
        assert!(item_id.as_str().unwrap().starts_with("msg_"));
        assert_eq!(frames[3].1["item_id"], item_id);
        assert_eq!(frames[3].1["content_index"], 0);
        assert_eq!(
            frames[3].1["part"],
            serde_json::json!({"type": "output_text", "text": "", "annotations": [], "logprobs": []})
        );
        assert_eq!(frames[4].1["delta"], "Hello");
        assert_eq!(frames[4].1["logprobs"], serde_json::json!([]));
        assert_eq!(frames[5].1["delta"], ", world");
        assert_eq!(frames[6].1["text"], "Hello, world");
        assert_eq!(frames[7].1["part"]["text"], "Hello, world");
        assert_eq!(frames[8].1["item"]["id"], item_id);
        assert_eq!(frames[8].1["item"]["status"], "completed");
        assert_eq!(frames[8].1["item"]["content"][0]["text"], "Hello, world");
        // Финал: полный Response object с usage и статусом completed.
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

    const SSE_TOOL_CALL: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","model":"claude-opus-4-8-20260101","usage":{"input_tokens":10,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"get_weather","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"Paris\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":8}}

event: message_stop
data: {"type":"message_stop"}"#;

    #[tokio::test]
    async fn contract_tool_call_event_dictionary() {
        // function_call: item.added (arguments "") → arguments.delta* →
        // arguments.done → item.done → completed (stop_reason tool_use →
        // completed).
        let translator = ResponsesSseTranslator::new(sse_bytes(SSE_TOOL_CALL), "m".into());
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        assert_eq!(
            event_names(&frames),
            [
                "response.created",
                "response.in_progress",
                "response.output_item.added",
                "response.function_call_arguments.delta",
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
        assert_eq!(item["call_id"], "toolu_1");
        assert_eq!(item["name"], "get_weather");
        assert_eq!(item["arguments"], "");
        assert_eq!(item["status"], "in_progress");
        assert_eq!(frames[3].1["delta"], "{\"city\":");
        assert_eq!(frames[4].1["delta"], "\"Paris\"}");
        assert_eq!(frames[5].1["arguments"], "{\"city\":\"Paris\"}");
        assert_eq!(frames[6].1["item"]["status"], "completed");
        assert_eq!(frames[6].1["item"]["arguments"], "{\"city\":\"Paris\"}");
        let completed = &frames[7].1;
        assert_eq!(completed["status"], "completed");
        assert_eq!(completed["output"][0]["type"], "function_call");
        assert_eq!(completed["output"][0]["call_id"], "toolu_1");
        assert_eq!(completed["usage"]["output_tokens"], 8);
    }

    #[tokio::test]
    async fn contract_text_then_tool_call_dense_output_index() {
        // text-блок (Messages index 0) перед tool_use (index 1): собственный
        // плотный счётчик — text item output_index 0, function_call 1.
        let events = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"model\":\"x\",\"usage\":{\"input_tokens\":1}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Checking.\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"f\",\"input\":{}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":4}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}"
        );
        let translator = ResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        let completed = frames.last().unwrap();
        assert_eq!(completed.0, "response.completed");
        assert_eq!(completed.1["output"].as_array().unwrap().len(), 2);
        assert_eq!(completed.1["output"][0]["type"], "message");
        assert_eq!(completed.1["output"][1]["type"], "function_call");
        // output_index добавленных items: 0 (message), 1 (function_call).
        let added: Vec<u64> = frames
            .iter()
            .filter(|(event, _)| event == "response.output_item.added")
            .map(|(_, data)| data["output_index"].as_u64().unwrap())
            .collect();
        assert_eq!(added, [0, 1], "{output}");
    }

    #[tokio::test]
    async fn contract_thinking_block_reasoning_event_dictionary() {
        // thinking-блок (4.2): item.added (reasoning, summary []) →
        // reasoning_summary_part.added (summary_index 0) → text.delta* →
        // text.done → part.done → item.done. Пустые thinking_delta и
        // signature_delta дропаются (решение 4). output_index: reasoning = 0,
        // message = 1 (плотный счётчик включает thinking-блоки).
        let events = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"model\":\"x\",\"usage\":{\"input_tokens\":10,\"output_tokens\":1}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Let me think.\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\" Done.\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig_1\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Answer.\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":8,\"output_tokens_details\":{\"thinking_tokens\":5}}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}"
        );
        let translator = ResponsesSseTranslator::new(sse_bytes(events), "m".into());
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
        // стабилен внутри блока.
        assert_eq!(frames[2].1["output_index"], 0);
        assert_eq!(frames[2].1["item"]["type"], "reasoning");
        assert!(frames[2].1["item"]["id"]
            .as_str()
            .unwrap()
            .starts_with("rs_"));
        assert_eq!(frames[2].1["item"]["summary"], serde_json::json!([]));
        let item_id = frames[2].1["item"]["id"].clone();
        assert_eq!(frames[3].1["item_id"], item_id);
        assert_eq!(frames[3].1["output_index"], 0);
        assert_eq!(frames[3].1["summary_index"], 0);
        assert_eq!(
            frames[3].1["part"],
            serde_json::json!({"type": "summary_text", "text": ""})
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
        // Финал: reasoning item в completed output, thinking-токены
        // message_delta → reasoning_tokens.
        let completed = &frames[15].1;
        assert_eq!(completed["status"], "completed");
        let items = completed["output"].as_array().unwrap();
        assert_eq!(items.len(), 2, "{output}");
        assert_eq!(items[0]["type"], "reasoning");
        assert_eq!(items[0]["summary"][0]["text"], "Let me think. Done.");
        assert_eq!(items[1]["type"], "message");
        assert_eq!(
            completed["usage"]["output_tokens_details"]["reasoning_tokens"],
            5
        );
        assert_eq!(completed["usage"]["output_tokens"], 8);
    }

    #[tokio::test]
    async fn contract_redacted_thinking_is_skipped_without_output_index_holes() {
        // redacted_thinking (решение 4) событий не порождает и позицию
        // output_index НЕ занимает.
        let events = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"model\":\"x\",\"usage\":{\"input_tokens\":1}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"opaque\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Answer.\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}"
        );
        let translator = ResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        assert!(!output.contains("opaque"), "{output}");
        let frames = event_frames(&output);
        let added: Vec<&Value> = frames
            .iter()
            .filter(|(event, _)| event == "response.output_item.added")
            .map(|(_, data)| data)
            .collect();
        assert_eq!(added.len(), 1, "{output}");
        assert_eq!(added[0]["output_index"], 0);
        assert_eq!(added[0]["item"]["type"], "message");
        let completed = frames.last().unwrap();
        assert_eq!(completed.1["output"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn sse_ping_becomes_comment_frame() {
        let events = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"model\":\"x\",\"usage\":{\"input_tokens\":1}}}\n\n",
            "event: ping\n",
            "data: {\"type\":\"ping\"}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}"
        );
        let translator = ResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        assert!(output.contains(": ping\n\n"), "{output}");
        let frames = event_frames(&output);
        assert_eq!(
            event_names(&frames),
            [
                "response.created",
                "response.in_progress",
                "comment",
                "response.completed",
            ],
            "{output}"
        );
    }

    #[tokio::test]
    async fn sse_error_event_becomes_response_failed_and_stream_ends() {
        let events = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"model\":\"x\",\"usage\":{\"input_tokens\":1}}}\n\n",
            "event: error\n",
            "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}"
        );
        let translator = ResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        assert!(!output.contains("response.completed"), "{output}");
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
        assert_eq!(frames[2].1["code"], "overloaded_error");
        assert_eq!(frames[2].1["message"], "Overloaded");
        assert_eq!(frames[2].1["param"], Value::Null);
        let failed = &frames[3].1;
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["error"]["code"], "overloaded_error");
        assert_eq!(failed["error"]["message"], "Overloaded");
    }

    #[tokio::test]
    async fn sse_clean_eof_without_message_stop_terminates_with_failed() {
        // Апстрим закрылся раньше протокола: честный response.failed, клиент
        // не висит (зеркало [DONE]-добивки chat-адаптера).
        let events = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}"
        );
        let translator = ResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        assert!(!output.contains("response.completed"), "{output}");
        let frames = event_frames(&output);
        let last = frames.last().unwrap();
        assert_eq!(last.0, "response.failed", "{output}");
        assert_eq!(last.1["status"], "failed");
    }

    #[tokio::test]
    async fn sse_max_tokens_completes_with_incomplete_status() {
        let events = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"model\":\"x\",\"usage\":{\"input_tokens\":3}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"output_tokens\":9}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}"
        );
        let translator = ResponsesSseTranslator::new(sse_bytes(events), "m".into());
        let output = collect_stream(translator).await;
        let frames = event_frames(&output);
        let completed = frames.last().unwrap();
        assert_eq!(completed.0, "response.completed", "{output}");
        assert_eq!(completed.1["status"], "incomplete");
        assert_eq!(
            completed.1["incomplete_details"],
            serde_json::json!({"reason": "max_output_tokens"})
        );
        assert_eq!(completed.1["output"][0]["content"][0]["text"], "partial");
    }

    #[tokio::test]
    async fn sse_split_frames_across_chunks_are_reassembled() {
        // Кадр, разрезанный посреди data-строки двумя сетевыми чанками.
        let full = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"split ok\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let (head, tail) = full.split_at(137);
        let chunks: Vec<Result<Bytes, std::io::Error>> =
            vec![Ok(Bytes::from(head)), Ok(Bytes::from(tail))];
        let translator =
            ResponsesSseTranslator::new(Box::pin(futures_util::stream::iter(chunks)), "m".into());
        let output = collect_stream(translator).await;
        assert!(output.contains("split ok"), "{output}");
        assert!(output.contains("response.completed"), "{output}");
    }

    #[tokio::test]
    async fn malformed_or_mismatched_known_event_fails_closed() {
        for events in [
            "event: message_start\ndata: {not json}\n\n",
            "event: message_start\ndata: {\"type\":\"ping\",\"message\":{}}\n\n",
        ] {
            let output =
                collect_stream(ResponsesSseTranslator::new(sse_bytes(events), "m".into())).await;
            let frames = event_frames(&output);
            assert_eq!(frames.last().unwrap().0, "response.failed", "{output}");
            assert_eq!(frames.last().unwrap().1["error"]["code"], "protocol_error");
        }
    }

    #[tokio::test]
    async fn unknown_named_event_is_ignored_and_unterminated_terminal_frame_is_accepted() {
        let events = concat!(
            "event: future_event\n",
            "data: future-format\n\n",
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}"
        );
        let output =
            collect_stream(ResponsesSseTranslator::new(sse_bytes(events), "m".into())).await;
        assert!(output.contains("response.completed"), "{output}");
        assert!(!output.contains("response.failed"), "{output}");
    }
}
