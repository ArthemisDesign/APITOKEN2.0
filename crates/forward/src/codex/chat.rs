//! OpenAI-compatible Chat Completions adapter over the same Responses/app-server core.
//!
//! The adapter is deliberately lenient: parameters that the app-server cannot honor are
//! accepted and ignored instead of rejected, so stock SDKs and agent terminals never fail on
//! defaults they send. Both streaming and non-streaming calls use exact upstream token usage
//! for settlement.

use super::api::{
    json_response, normalize_output_item, parse_responses_request, prepare_turn, ApiError,
    PreparedTurn, MAX_INSTRUCTIONS_BYTES, OPENAI_BODY_LIMIT, STREAM_FRAME_SEND_TIMEOUT,
};
use super::billing::begin_admission;
use super::{new_id, CodexGateway, CodexTurnResult, CodexUsage, TurnUpdate};
use crate::state::AppState;
use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::Response;
use bytes::Bytes;
use futures_util::Stream;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

struct ParsedChat {
    responses: super::api::ParsedResponsesRequest,
    base_instructions: Option<String>,
    include_usage: bool,
    stop: Vec<String>,
    /// Approximate output character budget derived from max_tokens/max_completion_tokens at
    /// ~4 chars per token. The transport cannot cap generation, so the cap is enforced on the
    /// delivered text with finish_reason "length"; settlement always uses authoritative usage.
    max_output_chars: Option<usize>,
}

type TranslatedChatMessages = (Vec<Value>, Option<String>, Option<String>);

pub async fn completions(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    let Some(gateway) = app.codex.as_ref().cloned() else {
        return ApiError::not_found("The requested endpoint is not enabled.", None::<String>)
            .into_response();
    };
    let (parts, body) = request.into_parts();
    let pending = match begin_admission(&app, &parts.headers, &peer).await {
        Ok(pending) => pending,
        Err(error) => return ApiError::from(error).into_response(),
    };
    let raw = match to_bytes(body, OPENAI_BODY_LIMIT).await {
        Ok(raw) => raw,
        Err(_) => {
            return ApiError::invalid("Request body exceeds the 8 MiB limit.", None::<String>)
                .into_response()
        }
    };
    let value: Value = match serde_json::from_slice(&raw) {
        Ok(value) => value,
        Err(_) => {
            return ApiError::invalid("Invalid JSON in request body.", None::<String>)
                .into_response()
        }
    };
    let parsed = match parse_chat_request(&gateway, value) {
        Ok(parsed) => parsed,
        Err(error) => return error.into_response(),
    };
    let tenant_scope = pending.tenant_scope().to_string();
    let mut prepared = match prepare_turn(&gateway, &tenant_scope, parsed.responses).await {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    prepared.turn.base_instructions = parsed.base_instructions;
    // Chat has two distinct instruction roles: system replaces the base prompt above, while
    // developer remains an explicit developer-history item in the patched app-server context.
    prepared.turn.developer_instructions = prepared.request.instructions.clone();
    let routing =
        super::api::build_turn_routing(&app, &tenant_scope, &parts.headers, &prepared).await;
    let admission = match pending
        .reserve(
            &app,
            &prepared.request.public_model,
            prepared.estimated_input_tokens,
            gateway.config().reserve_overhead_tokens,
        )
        .await
    {
        Ok(admission) => admission,
        Err(error) => return ApiError::from(error).into_response(),
    };
    let completion_id = new_id("chatcmpl");
    let created = pool::now();

    if prepared.request.stream {
        if let Err(error) = admission.mark_delivering().await {
            return ApiError::from(error).into_response();
        }
        return stream_chat(
            gateway,
            prepared,
            admission,
            completion_id,
            created,
            parsed.include_usage,
            parsed.stop,
            parsed.max_output_chars,
            routing,
        );
    }

    let result = match gateway.run_turn(prepared.turn.clone(), None, routing).await {
        Ok(result) => result,
        Err(error) => return ApiError::from(error).into_response(),
    };
    if let Err(error) = admission.mark_delivering().await {
        return ApiError::from(error).into_response();
    }
    let response = completed_chat(
        &prepared,
        &result,
        &completion_id,
        created,
        &parsed.stop,
        parsed.max_output_chars,
    );
    admission.settle(&prepared.request.public_model, &result.usage);
    let mut http_response = json_response(StatusCode::OK, response, &completion_id);
    super::api::insert_extra_headers(
        &mut http_response,
        super::api::ratelimit_headers(&gateway).await,
    );
    http_response
}

fn parse_chat_request(gateway: &CodexGateway, value: Value) -> Result<ParsedChat, ApiError> {
    let object = value
        .as_object()
        .ok_or_else(|| ApiError::invalid("Request body must be a JSON object.", None::<String>))?;
    // SDK compatibility: any parameter the transport cannot honor (sampling controls, token
    // caps, stop sequences, seeds, logprobs, multi-choice, store, service_tier, future fields)
    // is accepted and ignored rather than rejected, so stock SDKs and agent terminals never
    // fail on parameters they send by default.

    let messages = object
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ApiError::invalid(
                "Missing or invalid required parameter: messages.",
                Some("messages".to_string()),
            )
        })?;
    if messages.is_empty() {
        return Err(ApiError::invalid(
            "messages must not be empty.",
            Some("messages".to_string()),
        ));
    }
    let (input, base_instructions, developer_instructions) = translate_messages(messages)?;
    if base_instructions
        .as_ref()
        .is_some_and(|value| value.len() > MAX_INSTRUCTIONS_BYTES)
    {
        return Err(ApiError::invalid(
            "Combined system instructions exceed the 1 MiB limit.",
            Some("messages".to_string()),
        ));
    }
    if developer_instructions
        .as_ref()
        .is_some_and(|value| value.len() > MAX_INSTRUCTIONS_BYTES)
    {
        return Err(ApiError::invalid(
            "Combined developer instructions exceed the 1 MiB limit.",
            Some("messages".to_string()),
        ));
    }
    if input.is_empty() {
        return Err(ApiError::invalid(
            "messages must contain at least one user, assistant, or tool message.",
            Some("messages".to_string()),
        ));
    }

    let mut responses = Map::new();
    responses.insert(
        "model".to_string(),
        object.get("model").cloned().unwrap_or(Value::Null),
    );
    responses.insert("input".to_string(), Value::Array(input));
    if let Some(instructions) = developer_instructions {
        responses.insert("instructions".to_string(), Value::String(instructions));
    }
    if let Some(stream) = object.get("stream") {
        responses.insert("stream".to_string(), stream.clone());
    }
    if let Some(parallel) = object.get("parallel_tool_calls") {
        responses.insert("parallel_tool_calls".to_string(), parallel.clone());
    }
    if let Some(metadata) = object.get("metadata") {
        responses.insert("metadata".to_string(), metadata.clone());
    }
    // Chat retrieval endpoints are not implemented, so claiming to store a completion would be
    // misleading. `store=false` is validated below and omitted here.
    responses.insert("store".to_string(), Value::Bool(false));

    if let Some(effort) = object
        .get("reasoning_effort")
        .filter(|value| !value.is_null())
    {
        responses.insert("reasoning".to_string(), json!({"effort": effort}));
    }
    let response_format = object
        .get("response_format")
        .filter(|value| !value.is_null());
    let verbosity = object.get("verbosity").filter(|value| !value.is_null());
    if response_format.is_some() || verbosity.is_some() {
        let mut text = match response_format {
            Some(format) => translate_response_format(format)?,
            None => json!({}),
        };
        if let Some(verbosity) = verbosity {
            text["verbosity"] = verbosity.clone();
        }
        responses.insert("text".to_string(), text);
    }
    if let Some(tools) = object.get("tools").filter(|value| !value.is_null()) {
        responses.insert("tools".to_string(), translate_chat_tools(tools)?);
    } else if let Some(functions) = object.get("functions").filter(|value| !value.is_null()) {
        // Legacy `functions` surface: identical schema minus the {"type":"function"} wrapper.
        responses.insert("tools".to_string(), translate_legacy_functions(functions)?);
    }
    if let Some(choice) = object.get("tool_choice").filter(|value| !value.is_null()) {
        responses.insert("tool_choice".to_string(), choice.clone());
    } else if let Some(choice) = object.get("function_call").filter(|value| !value.is_null()) {
        // Legacy `function_call`: only "none" is enforceable; everything else degrades to auto.
        let mapped = match choice.as_str() {
            Some("none") => Value::String("none".to_string()),
            _ => Value::String("auto".to_string()),
        };
        responses.insert("tool_choice".to_string(), mapped);
    }

    let stop = parse_stop_sequences(object.get("stop"))?;
    let max_output_chars = parse_max_output_chars(object)?;
    let include_usage = parse_stream_options(object.get("stream_options"))?;
    let responses = parse_responses_request(gateway, Value::Object(responses))?;
    Ok(ParsedChat {
        responses,
        base_instructions,
        include_usage,
        stop,
        max_output_chars,
    })
}

/// `stop` accepts a single string or an array of up to 4 non-empty strings, like the official
/// endpoint. Sequences are honored on the delivered text (see StopFilter), not upstream.
fn parse_stop_sequences(value: Option<&Value>) -> Result<Vec<String>, ApiError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(Vec::new());
    };
    let raw: Vec<&str> = match value {
        Value::String(single) => vec![single.as_str()],
        Value::Array(items) => {
            let mut collected = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                collected.push(item.as_str().ok_or_else(|| {
                    ApiError::invalid(
                        "stop sequences must be strings.",
                        Some(format!("stop.{index}")),
                    )
                })?);
            }
            collected
        }
        _ => {
            return Err(ApiError::invalid(
                "stop must be a string or an array of strings.",
                Some("stop".to_string()),
            ))
        }
    };
    if raw.len() > 4 {
        return Err(ApiError::invalid(
            "stop may contain at most 4 sequences.",
            Some("stop".to_string()),
        ));
    }
    Ok(raw
        .into_iter()
        .filter(|sequence| !sequence.is_empty())
        .map(str::to_string)
        .collect())
}

fn parse_max_output_chars(object: &Map<String, Value>) -> Result<Option<usize>, ApiError> {
    const CHARS_PER_TOKEN: u64 = 4;
    let value = object
        .get("max_completion_tokens")
        .filter(|value| !value.is_null())
        .or_else(|| object.get("max_tokens").filter(|value| !value.is_null()));
    let Some(value) = value else {
        return Ok(None);
    };
    let tokens = value.as_u64().ok_or_else(|| {
        ApiError::invalid(
            "max_completion_tokens must be a positive integer.",
            Some("max_completion_tokens".to_string()),
        )
    })?;
    Ok(Some(
        usize::try_from(tokens.saturating_mul(CHARS_PER_TOKEN)).unwrap_or(usize::MAX),
    ))
}

fn translate_legacy_functions(value: &Value) -> Result<Value, ApiError> {
    let functions = value.as_array().ok_or_else(|| {
        ApiError::invalid("functions must be an array.", Some("functions".to_string()))
    })?;
    let tools: Vec<Value> = functions
        .iter()
        .map(|function| json!({"type": "function", "function": function}))
        .collect();
    translate_chat_tools(&Value::Array(tools))
}

fn parse_stream_options(value: Option<&Value>) -> Result<bool, ApiError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(false);
    };
    let object = value.as_object().ok_or_else(|| {
        ApiError::invalid(
            "stream_options must be an object.",
            Some("stream_options".to_string()),
        )
    })?;
    // Unknown stream_options fields are ignored for SDK compatibility.
    match object.get("include_usage") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(ApiError::invalid(
            "stream_options.include_usage must be a boolean.",
            Some("stream_options.include_usage".to_string()),
        )),
    }
}

fn translate_messages(messages: &[Value]) -> Result<TranslatedChatMessages, ApiError> {
    let mut input = Vec::new();
    let mut system = Vec::new();
    let mut developer = Vec::new();
    let mut call_ids = HashSet::new();
    for (index, message) in messages.iter().enumerate() {
        let object = message.as_object().ok_or_else(|| {
            ApiError::invalid(
                "Each message must be an object.",
                Some(format!("messages.{index}")),
            )
        })?;
        // The `name` participant hint has no transport equivalent; accepted and ignored.
        let role = object.get("role").and_then(Value::as_str).ok_or_else(|| {
            ApiError::invalid(
                "Each message requires a valid role.",
                Some(format!("messages.{index}.role")),
            )
        })?;
        match role {
            "system" | "developer" => {
                let text = chat_content_text(
                    object.get("content"),
                    false,
                    &format!("messages.{index}.content"),
                )?
                .ok_or_else(|| {
                    ApiError::invalid(
                        "Instruction message content must not be null.",
                        Some(format!("messages.{index}.content")),
                    )
                })?;
                if role == "system" {
                    system.push(text);
                } else {
                    developer.push(text);
                }
            }
            "user" => {
                let parts = chat_user_content_parts(
                    object.get("content"),
                    &format!("messages.{index}.content"),
                )?;
                input.push(json!({
                    "type": "message",
                    "role": "user",
                    "content": parts
                }));
            }
            "assistant" => {
                // refusal/audio fields carry no model-visible history text; accepted and
                // ignored so replayed official conversations never fail.
                let content = chat_content_text(
                    object.get("content"),
                    true,
                    &format!("messages.{index}.content"),
                )?;
                if let Some(content) = content.as_ref().filter(|content| !content.is_empty()) {
                    input.push(chat_message_item("assistant", content));
                }
                if let Some(tool_calls) = object.get("tool_calls").filter(|value| !value.is_null())
                {
                    let tool_calls = tool_calls.as_array().ok_or_else(|| {
                        ApiError::invalid(
                            "assistant.tool_calls must be an array.",
                            Some(format!("messages.{index}.tool_calls")),
                        )
                    })?;
                    for (tool_index, call) in tool_calls.iter().enumerate() {
                        let call = call.as_object().ok_or_else(|| {
                            ApiError::invalid(
                                "Each tool call must be an object.",
                                Some(format!("messages.{index}.tool_calls.{tool_index}")),
                            )
                        })?;
                        let call_id = required_chat_string(
                            call,
                            "id",
                            &format!("messages.{index}.tool_calls.{tool_index}.id"),
                        )?;
                        if !call_ids.insert(call_id.to_string()) {
                            return Err(ApiError::invalid(
                                format!("Duplicate tool call id {call_id:?}."),
                                Some(format!("messages.{index}.tool_calls.{tool_index}.id")),
                            ));
                        }
                        if call.get("type").and_then(Value::as_str) != Some("function") {
                            return Err(ApiError::invalid(
                                "Only function tool calls are supported.",
                                Some(format!("messages.{index}.tool_calls.{tool_index}.type")),
                            ));
                        }
                        let function =
                            call.get("function")
                                .and_then(Value::as_object)
                                .ok_or_else(|| {
                                    ApiError::invalid(
                                        "tool_call.function must be an object.",
                                        Some(format!(
                                            "messages.{index}.tool_calls.{tool_index}.function"
                                        )),
                                    )
                                })?;
                        let name = required_chat_string(
                            function,
                            "name",
                            &format!("messages.{index}.tool_calls.{tool_index}.function.name"),
                        )?;
                        let arguments = required_chat_string(
                            function,
                            "arguments",
                            &format!("messages.{index}.tool_calls.{tool_index}.function.arguments"),
                        )?;
                        input.push(json!({
                            "type": "function_call",
                            "call_id": call_id,
                            "name": name,
                            "arguments": arguments,
                            "status": "completed"
                        }));
                    }
                }
                if content.is_none() && object.get("tool_calls").is_none() {
                    return Err(ApiError::invalid(
                        "Assistant message requires content or tool_calls.",
                        Some(format!("messages.{index}")),
                    ));
                }
            }
            "tool" => {
                let call_id = object
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ApiError::invalid(
                            "Tool message requires tool_call_id.",
                            Some(format!("messages.{index}.tool_call_id")),
                        )
                    })?;
                let output = chat_content_text(
                    object.get("content"),
                    false,
                    &format!("messages.{index}.content"),
                )?
                .ok_or_else(|| {
                    ApiError::invalid(
                        "Tool message content must not be null.",
                        Some(format!("messages.{index}.content")),
                    )
                })?;
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output
                }));
            }
            _ => {
                return Err(ApiError::invalid(
                    format!("Message role {role:?} is not supported."),
                    Some(format!("messages.{index}.role")),
                ))
            }
        }
    }
    Ok((
        input,
        join_instructions(system),
        join_instructions(developer),
    ))
}

fn required_chat_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    param: &str,
) -> Result<&'a str, ApiError> {
    object.get(field).and_then(Value::as_str).ok_or_else(|| {
        ApiError::invalid(
            format!("{param} must be a string."),
            Some(param.to_string()),
        )
    })
}

fn chat_content_text(
    value: Option<&Value>,
    allow_null: bool,
    param: &str,
) -> Result<Option<String>, ApiError> {
    let Some(value) = value else {
        return if allow_null {
            Ok(None)
        } else {
            Err(ApiError::invalid(
                format!("{param} is required."),
                Some(param.to_string()),
            ))
        };
    };
    match value {
        Value::Null if allow_null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        Value::Array(parts) => {
            let mut text = String::new();
            for (index, part) in parts.iter().enumerate() {
                let part = part.as_object().ok_or_else(|| {
                    ApiError::invalid(
                        "Content parts must be objects.",
                        Some(format!("{param}.{index}")),
                    )
                })?;
                if !matches!(
                    part.get("type").and_then(Value::as_str),
                    Some("text" | "input_text" | "output_text")
                ) {
                    return Err(ApiError::invalid(
                        "Only text content parts are supported.",
                        Some(format!("{param}.{index}.type")),
                    ));
                }
                let part_text = part.get("text").and_then(Value::as_str).ok_or_else(|| {
                    ApiError::invalid(
                        "Text content part requires text.",
                        Some(format!("{param}.{index}.text")),
                    )
                })?;
                text.push_str(part_text);
            }
            Ok(Some(text))
        }
        _ => Err(ApiError::invalid(
            format!("{param} must be a string or text-part array."),
            Some(param.to_string()),
        )),
    }
}

/// User messages may mix text and images. Parts are normalized to the canonical Responses
/// content shape (`input_text` / `input_image`) so history injection and the turn input split
/// handle them uniformly.
fn chat_user_content_parts(value: Option<&Value>, param: &str) -> Result<Vec<Value>, ApiError> {
    let value = value.ok_or_else(|| {
        ApiError::invalid(format!("{param} is required."), Some(param.to_string()))
    })?;
    match value {
        Value::Null => Err(ApiError::invalid(
            "User message content must not be null.",
            Some(param.to_string()),
        )),
        Value::String(text) => Ok(vec![json!({"type": "input_text", "text": text})]),
        Value::Array(parts) => {
            let mut normalized = Vec::with_capacity(parts.len());
            for (index, part) in parts.iter().enumerate() {
                let part = part.as_object().ok_or_else(|| {
                    ApiError::invalid(
                        "Content parts must be objects.",
                        Some(format!("{param}.{index}")),
                    )
                })?;
                let part_param = format!("{param}.{index}");
                match part.get("type").and_then(Value::as_str) {
                    Some("text" | "input_text") => {
                        let text = part.get("text").and_then(Value::as_str).ok_or_else(|| {
                            ApiError::invalid(
                                "Text content part requires text.",
                                Some(format!("{part_param}.text")),
                            )
                        })?;
                        normalized.push(json!({"type": "input_text", "text": text}));
                    }
                    Some("image_url" | "input_image") => {
                        normalized.push(super::api::canonical_image_part(
                            part.get("image_url")
                                .or_else(|| part.get("url"))
                                .ok_or_else(|| {
                                    ApiError::invalid(
                                        "Image content part requires image_url.",
                                        Some(format!("{part_param}.image_url")),
                                    )
                                })?,
                            part.get("detail"),
                            &part_param,
                        )?);
                    }
                    Some(other) => {
                        return Err(ApiError::invalid(
                            format!("Content part type {other:?} is not supported."),
                            Some(format!("{part_param}.type")),
                        ));
                    }
                    None => {
                        return Err(ApiError::invalid(
                            "Content part requires a type.",
                            Some(part_param),
                        ));
                    }
                }
            }
            Ok(normalized)
        }
        _ => Err(ApiError::invalid(
            format!("{param} must be a string or a content-part array."),
            Some(param.to_string()),
        )),
    }
}

fn chat_message_item(role: &str, text: &str) -> Value {
    json!({
        "type": "message",
        "role": role,
        "content": [{
            "type": if role == "assistant" { "output_text" } else { "input_text" },
            "text": text
        }]
    })
}

fn join_instructions(parts: Vec<String>) -> Option<String> {
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn translate_response_format(value: &Value) -> Result<Value, ApiError> {
    let object = value.as_object().ok_or_else(|| {
        ApiError::invalid(
            "response_format must be an object.",
            Some("response_format".to_string()),
        )
    })?;
    match object.get("type").and_then(Value::as_str) {
        Some("text") => Ok(json!({"format": {"type": "text"}})),
        Some("json_object") => Ok(json!({"format": {"type": "json_object"}})),
        Some("json_schema") => {
            let schema = object
                .get("json_schema")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    ApiError::invalid(
                        "response_format.json_schema must be an object.",
                        Some("response_format.json_schema".to_string()),
                    )
                })?;
            let schema_value = schema.get("schema").cloned().ok_or_else(|| {
                ApiError::invalid(
                    "response_format.json_schema.schema is required.",
                    Some("response_format.json_schema.schema".to_string()),
                )
            })?;
            let mut format = json!({
                "type": "json_schema",
                "schema": schema_value,
                "name": schema.get("name").cloned().unwrap_or_else(|| Value::String("response".to_string()))
            });
            if let Some(description) = schema.get("description") {
                format["description"] = description.clone();
            }
            if let Some(strict) = schema.get("strict") {
                format["strict"] = strict.clone();
            }
            Ok(json!({"format": format}))
        }
        _ => Err(ApiError::invalid(
            "response_format.type must be text, json_object, or json_schema.",
            Some("response_format.type".to_string()),
        )),
    }
}

fn translate_chat_tools(value: &Value) -> Result<Value, ApiError> {
    let tools = value
        .as_array()
        .ok_or_else(|| ApiError::invalid("tools must be an array.", Some("tools".to_string())))?;
    let mut translated = Vec::with_capacity(tools.len());
    for (index, tool) in tools.iter().enumerate() {
        let tool = tool.as_object().ok_or_else(|| {
            ApiError::invalid(
                "Each tool must be an object.",
                Some(format!("tools.{index}")),
            )
        })?;
        if tool.get("type").and_then(Value::as_str) != Some("function") {
            return Err(ApiError::invalid(
                "Only function tools are supported.",
                Some(format!("tools.{index}.type")),
            ));
        }
        let function = tool
            .get("function")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ApiError::invalid(
                    "tools.function must be an object.",
                    Some(format!("tools.{index}.function")),
                )
            })?;
        let name = required_chat_string(function, "name", &format!("tools.{index}.function.name"))?;
        let mut translated_tool = json!({
            "type": "function",
            "name": name,
            "description": function.get("description").cloned().unwrap_or_else(|| Value::String(String::new())),
            "parameters": function.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object", "properties": {}}))
        });
        if let Some(strict) = function.get("strict") {
            translated_tool["strict"] = strict.clone();
        }
        translated.push(translated_tool);
    }
    Ok(Value::Array(translated))
}

/// Applies client stop sequences and the approximate output budget to completed text. The
/// stop sequence itself is removed, matching the official endpoint. Returns the truncated text
/// and whether the budget (not a stop sequence) caused the cut.
fn enforce_output_limits(
    mut text: String,
    stop: &[String],
    max_chars: Option<usize>,
) -> (String, bool) {
    if let Some(cut) = stop
        .iter()
        .filter_map(|sequence| text.find(sequence.as_str()))
        .min()
    {
        text.truncate(cut);
    }
    let mut capped = false;
    if let Some(budget) = max_chars {
        if text.len() > budget {
            let mut boundary = budget;
            while !text.is_char_boundary(boundary) {
                boundary -= 1;
            }
            text.truncate(boundary);
            capped = true;
        }
    }
    (text, capped)
}

/// Incremental stop-sequence matcher for streaming text. Holds back at most
/// `longest_stop - 1` bytes so a sequence straddling delta boundaries is still detected, and
/// reports the cut position without emitting the sequence itself.
struct StopFilter {
    stops: Vec<String>,
    hold_back: usize,
    pending: String,
    triggered: bool,
}

impl StopFilter {
    fn new(stops: Vec<String>) -> Option<Self> {
        if stops.is_empty() {
            return None;
        }
        let hold_back = stops
            .iter()
            .map(|sequence| sequence.len())
            .max()
            .unwrap_or(0)
            .saturating_sub(1);
        Some(Self {
            stops,
            hold_back,
            pending: String::new(),
            triggered: false,
        })
    }

    /// Feeds a delta; returns the text safe to emit now (empty when everything is held back or
    /// the filter already triggered).
    fn push(&mut self, delta: &str) -> String {
        if self.triggered {
            return String::new();
        }
        self.pending.push_str(delta);
        if let Some(cut) = self
            .stops
            .iter()
            .filter_map(|sequence| self.pending.find(sequence.as_str()))
            .min()
        {
            let out = self.pending[..cut].to_string();
            self.triggered = true;
            return out;
        }
        if self.pending.len() > self.hold_back {
            let mut boundary = self.pending.len() - self.hold_back;
            while !self.pending.is_char_boundary(boundary) {
                boundary -= 1;
            }
            let out = self.pending[..boundary].to_string();
            self.pending.replace_range(..boundary, "");
            return out;
        }
        String::new()
    }

    /// Drains the held-back tail at end of stream (no-op when a stop already triggered).
    fn finish(&mut self) -> String {
        if self.triggered {
            String::new()
        } else {
            std::mem::take(&mut self.pending)
        }
    }
}

/// Joins provider reasoning summaries for clients that surface thinking via `reasoning_content`.
fn reasoning_summary_text(result: &CodexTurnResult) -> Option<String> {
    let mut text = String::new();
    for item in result
        .output
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("reasoning"))
    {
        for part in item
            .get("summary")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                text.push_str(part_text);
            }
        }
    }
    (!text.is_empty()).then_some(text)
}

fn completed_chat(
    prepared: &PreparedTurn,
    result: &CodexTurnResult,
    completion_id: &str,
    created: i64,
    stop: &[String],
    max_output_chars: Option<usize>,
) -> Value {
    let (content, tool_calls) = chat_output(result);
    let reasoning_content = reasoning_summary_text(result);
    let (content, capped) = enforce_output_limits(content, stop, max_output_chars);
    let finish_reason = if !tool_calls.is_empty() {
        "tool_calls"
    } else if capped {
        "length"
    } else {
        "stop"
    };
    json!({
        "id": completion_id,
        "object": "chat.completion",
        "created": created,
        "model": prepared.request.public_model.id,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": if content.is_empty() { Value::Null } else { Value::String(content) },
                "refusal": Value::Null,
                "annotations": [],
                "audio": Value::Null,
                "reasoning_content": reasoning_content,
                "tool_calls": if tool_calls.is_empty() { Value::Null } else { Value::Array(tool_calls) }
            },
            "logprobs": Value::Null,
            "finish_reason": finish_reason
        }],
        "usage": chat_usage(&result.usage),
        "service_tier": "default",
        "system_fingerprint": Value::Null
    })
}

fn chat_output(result: &CodexTurnResult) -> (String, Vec<Value>) {
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    for item in result.output.iter().filter_map(normalize_output_item) {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                for text in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                {
                    content.push_str(text);
                }
            }
            Some("function_call") => {
                tool_calls.push(json!({
                    "id": item.get("call_id").cloned().unwrap_or_else(|| Value::String(new_id("call"))),
                    "type": "function",
                    "function": {
                        "name": item.get("name").cloned().unwrap_or_else(|| Value::String("unknown".to_string())),
                        "arguments": item.get("arguments").cloned().unwrap_or_else(|| Value::String("{}".to_string()))
                    }
                }));
            }
            _ => {}
        }
    }
    (content, tool_calls)
}

fn chat_usage(usage: &CodexUsage) -> Value {
    json!({
        "prompt_tokens": usage.input_tokens,
        "completion_tokens": usage.output_tokens,
        "total_tokens": usage.total_tokens,
        "prompt_tokens_details": {
            "cached_tokens": usage.cached_input_tokens,
            "audio_tokens": 0
        },
        "completion_tokens_details": {
            "reasoning_tokens": usage.reasoning_output_tokens,
            "audio_tokens": 0,
            "accepted_prediction_tokens": 0,
            "rejected_prediction_tokens": 0
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn stream_chat(
    gateway: Arc<CodexGateway>,
    prepared: PreparedTurn,
    admission: super::billing::CodexAdmission,
    completion_id: String,
    created: i64,
    include_usage: bool,
    stop: Vec<String>,
    max_output_chars: Option<usize>,
    routing: Option<super::TurnRouting>,
) -> Response {
    let task_permit = match gateway.track_background_task() {
        Ok(permit) => permit,
        Err(error) => return ApiError::from(error).into_response(),
    };
    let (frame_tx, frame_rx) = mpsc::channel::<Bytes>(128);
    let request_id_header = completion_id.clone();
    tokio::spawn(async move {
        let _task_permit = task_permit;
        // Rebind after the permit so early returns drop billing admission before the shutdown permit.
        let admission = admission;
        if !send_chat_frame(
            &frame_tx,
            chat_chunk(
                &prepared,
                &completion_id,
                created,
                json!({"role": "assistant", "content": ""}),
                Value::Null,
            ),
        )
        .await
        {
            return;
        }

        let (update_tx, mut update_rx) = mpsc::channel(512);
        let run_gateway = gateway.clone();
        let turn = prepared.turn.clone();
        let run =
            tokio::spawn(async move { run_gateway.run_turn(turn, Some(update_tx), routing).await });
        let mut tool_index = 0usize;
        let mut emitted_tools = HashSet::new();
        let mut downstream_closed = false;
        let mut stop_filter = StopFilter::new(stop);
        let mut emitted_chars = 0usize;
        let mut length_capped = false;
        let mut heartbeat = tokio::time::interval(super::api::SSE_HEARTBEAT_INTERVAL);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        heartbeat.tick().await;

        // Enforces the approximate output budget on text leaving for the client. Returns the
        // delta to emit (empty when the budget is exhausted or the stop filter cut the stream).
        let shape_text = |text: String,
                          stop_filter: &mut Option<StopFilter>,
                          emitted_chars: &mut usize,
                          length_capped: &mut bool|
         -> String {
            let text = match stop_filter {
                Some(filter) => filter.push(&text),
                None => text,
            };
            if let Some(budget) = max_output_chars {
                let remaining = budget.saturating_sub(*emitted_chars);
                if text.len() > remaining {
                    let mut boundary = remaining;
                    while !text.is_char_boundary(boundary) {
                        boundary -= 1;
                    }
                    *length_capped = true;
                    *emitted_chars = budget;
                    return text[..boundary].to_string();
                }
                *emitted_chars += text.len();
            }
            text
        };

        loop {
            tokio::select! {
                _ = frame_tx.closed() => {
                    downstream_closed = true;
                    break;
                }
                _ = heartbeat.tick() => {
                    if !send_chat_frame(
                        &frame_tx,
                        chat_chunk(
                            &prepared,
                            &completion_id,
                            created,
                            json!({}),
                            Value::Null,
                        ),
                    )
                    .await
                    {
                        downstream_closed = true;
                        break;
                    }
                    continue;
                }
                update = update_rx.recv() => {
                    let Some(update) = update else { break };
                    let delta = match update {
                        TurnUpdate::TextDelta { delta, .. } => {
                            let shaped = shape_text(
                                delta,
                                &mut stop_filter,
                                &mut emitted_chars,
                                &mut length_capped,
                            );
                            if shaped.is_empty() {
                                continue;
                            }
                            json!({"content": shaped})
                        }
                        TurnUpdate::ReasoningSummaryDelta { delta, .. } => {
                            json!({"reasoning_content": delta})
                        }
                        TurnUpdate::RawItem(item)
                            if item.get("type").and_then(Value::as_str) == Some("function_call") =>
                        {
                            let key = item
                                .get("call_id")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                                .unwrap_or_else(|| new_id("call"));
                            if !emitted_tools.insert(key.clone()) {
                                continue;
                            }
                            let delta = json!({
                                "tool_calls": [{
                                    "index": tool_index,
                                    "id": key,
                                    "type": "function",
                                    "function": {
                                        "name": item.get("name").cloned().unwrap_or_else(|| Value::String("unknown".to_string())),
                                        "arguments": item.get("arguments").cloned().unwrap_or_else(|| Value::String("{}".to_string()))
                                    }
                                }]
                            });
                            tool_index += 1;
                            delta
                        }
                        TurnUpdate::ReasoningSummaryPartAdded { .. } => continue,
                        TurnUpdate::RawItem(_) => continue,
                    };
                    if !send_chat_frame(
                        &frame_tx,
                        chat_chunk(&prepared, &completion_id, created, delta, Value::Null),
                    )
                    .await
                    {
                        downstream_closed = true;
                        break;
                    }
                }
            }
        }

        // Flush any held-back stop-filter tail unless the client already left. The tail no
        // longer goes through the filter; only the output budget still applies.
        if !downstream_closed {
            if let Some(filter) = stop_filter.as_mut() {
                let tail = filter.finish();
                if !tail.is_empty() {
                    let shaped = match max_output_chars {
                        Some(budget) => {
                            let remaining = budget.saturating_sub(emitted_chars);
                            if tail.len() > remaining {
                                let mut boundary = remaining;
                                while boundary > 0 && !tail.is_char_boundary(boundary) {
                                    boundary -= 1;
                                }
                                length_capped = true;
                                tail[..boundary].to_string()
                            } else {
                                tail
                            }
                        }
                        None => tail,
                    };
                    if !shaped.is_empty()
                        && !send_chat_frame(
                            &frame_tx,
                            chat_chunk(
                                &prepared,
                                &completion_id,
                                created,
                                json!({"content": shaped}),
                                Value::Null,
                            ),
                        )
                        .await
                    {
                        downstream_closed = true;
                    }
                }
            }
        }

        if downstream_closed {
            // Stop buffering public deltas but keep the already-started provider turn alive until
            // its authoritative usage arrives. This mirrors Claude's disconnect drain and prevents
            // a client from receiving a partial stream while escaping settlement.
            drop(update_rx);
        }

        let result = match run.await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => {
                eprintln!("Codex chat stream failed [{}]", error.diagnostic_class());
                let _ = send_chat_error(&frame_tx).await;
                return;
            }
            Err(_) => {
                eprintln!("Codex chat stream task failed [join]");
                let _ = send_chat_error(&frame_tx).await;
                return;
            }
        };
        let (_, authoritative_tools) = chat_output(&result);
        let finish_reason = if !authoritative_tools.is_empty() {
            "tool_calls"
        } else if length_capped {
            "length"
        } else {
            "stop"
        };
        admission.settle(&prepared.request.public_model, &result.usage);
        if downstream_closed {
            return;
        }
        if !send_chat_frame(
            &frame_tx,
            chat_chunk(
                &prepared,
                &completion_id,
                created,
                json!({}),
                Value::String(finish_reason.to_string()),
            ),
        )
        .await
        {
            return;
        }
        if include_usage
            && !send_chat_frame(
                &frame_tx,
                json!({
                    "id": completion_id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "model": prepared.request.public_model.id,
                    "choices": [],
                    "usage": chat_usage(&result.usage),
                    "service_tier": "default",
                    "system_fingerprint": Value::Null
                }),
            )
            .await
        {
            return;
        }
        let _ = send_chat_bytes(&frame_tx, Bytes::from_static(b"data: [DONE]\n\n")).await;
    });

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .header("x-request-id", request_id_header)
        .body(Body::from_stream(ChatReceiverStream { receiver: frame_rx }))
        .unwrap()
}

fn chat_chunk(
    prepared: &PreparedTurn,
    completion_id: &str,
    created: i64,
    delta: Value,
    finish_reason: Value,
) -> Value {
    json!({
        "id": completion_id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": prepared.request.public_model.id,
        "choices": [{
            "index": 0,
            "delta": delta,
            "logprobs": Value::Null,
            "finish_reason": finish_reason
        }],
        "usage": Value::Null,
        "service_tier": "default",
        "system_fingerprint": Value::Null
    })
}

async fn send_chat_frame(sender: &mpsc::Sender<Bytes>, value: Value) -> bool {
    let data = serde_json::to_string(&value).unwrap_or_else(|_| {
        r#"{"error":{"message":"serialization failed","type":"server_error","code":"server_error"}}"#
            .to_string()
    });
    send_chat_bytes(sender, Bytes::from(format!("data: {data}\n\n"))).await
}

async fn send_chat_bytes(sender: &mpsc::Sender<Bytes>, frame: Bytes) -> bool {
    matches!(
        tokio::time::timeout(STREAM_FRAME_SEND_TIMEOUT, sender.send(frame)).await,
        Ok(Ok(()))
    )
}

async fn send_chat_error(sender: &mpsc::Sender<Bytes>) -> bool {
    if !send_chat_frame(
        sender,
        json!({
            "error": {
                "message": "The model stream terminated unexpectedly.",
                "type": "server_error",
                "param": Value::Null,
                "code": "server_error"
            }
        }),
    )
    .await
    {
        return false;
    }
    send_chat_bytes(sender, Bytes::from_static(b"data: [DONE]\n\n")).await
}

struct ChatReceiverStream {
    receiver: mpsc::Receiver<Bytes>,
}

impl Stream for ChatReceiverStream {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx).map(|item| item.map(Ok))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::{CodexConfig, CodexModel, CodexPrices};
    use std::collections::BTreeMap;

    fn gateway() -> CodexGateway {
        let ownership_lock =
            std::env::temp_dir().join(format!("{}.lock", new_id("codex-chat-test")));
        std::fs::write(&ownership_lock, []).unwrap();
        CodexGateway::new(CodexConfig {
            enabled: true,
            ownership_lock_file: ownership_lock.to_str().unwrap().to_string(),
            binary: "/tmp/codex".to_string(),
            binary_sha256: "0".repeat(64),
            expected_version: "codex-cli test".to_string(),
            homes: vec!["/tmp/home".to_string()],
            homes_dir: None,
            work_dir: "/tmp/work".to_string(),
            startup_timeout_ms: 1,
            request_timeout_ms: 1,
            turn_timeout_ms: 1,
            max_concurrent_turns: 1,
            admit_below_used_percent: 95,
            window_cap_usd_prior: 1_500.0,
            health_probe_interval_secs: 300,
            reserve_overhead_tokens: 1,
            history_ttl_secs: 60,
            history_local_cap: 8,
            history_redis_url: None,
            history_secret: None,
            history_redis_timeout_ms: 1,
            child_proxy_env: BTreeMap::new(),
            models: vec![CodexModel {
                id: "gpt-5.6".to_string(),
                upstream: "gpt-5.6-sol".to_string(),
                created: 0,
                owned_by: "test".to_string(),
                max_output_tokens: 128_000,
                reasoning_efforts: vec!["none".to_string(), "high".to_string()],
                prices: CodexPrices {
                    input: 5_000,
                    cached_input: 500,
                    cache_write_input: 6_250,
                    output: 30_000,
                    long_context_threshold: 272_000,
                    long_input_basis_points: 20_000,
                    long_output_basis_points: 15_000,
                },
            }],
        })
        .unwrap()
    }

    #[test]
    fn chat_system_and_developer_replace_codex_instructions() {
        let parsed = parse_chat_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "messages": [
                    {"role": "system", "content": "SYSTEM"},
                    {"role": "developer", "content": "DEVELOPER"},
                    {"role": "user", "content": "hello"}
                ]
            }),
        )
        .unwrap();
        assert_eq!(parsed.base_instructions.as_deref(), Some("SYSTEM"));
        assert_eq!(parsed.responses.instructions.as_deref(), Some("DEVELOPER"));
        assert_eq!(parsed.responses.input.turn_input.len(), 1);
    }

    #[test]
    fn chat_function_history_translates_without_text_fabrication() {
        let (items, _, _) = translate_messages(&[
            json!({"role":"assistant","content":null,"tool_calls":[{
                "id":"call_1","type":"function",
                "function":{"name":"weather","arguments":"{\"city\":\"Paris\"}"}
            }]}),
            json!({"role":"tool","tool_call_id":"call_1","content":"sunny"}),
        ])
        .unwrap();
        assert_eq!(items[0]["type"], "function_call");
        assert_eq!(items[1]["type"], "function_call_output");
        assert_eq!(items[1]["output"], "sunny");
    }

    #[test]
    fn unsupported_sampling_is_accepted_and_ignored() {
        let parsed = parse_chat_request(
            &gateway(),
            json!({
                "model":"gpt-5.6",
                "messages":[{"role":"user","content":"hello"}],
                "temperature":0.2,
                "top_p":0.5,
                "max_tokens":256,
                "max_completion_tokens":256,
                "stop":["\n"],
                "presence_penalty":0.5,
                "frequency_penalty":-0.5,
                "logprobs":true,
                "top_logprobs":3,
                "seed":42,
                "user":"end-user",
                "n":2,
                "store":true,
                "service_tier":"flex",
                "some_future_field":{"anything":true}
            }),
        )
        .expect("parameters the transport cannot honor must be ignored, not rejected");
        assert_eq!(parsed.responses.input.turn_input.len(), 1);
    }

    #[test]
    fn chat_user_image_parts_translate_to_canonical_input_image() {
        let parsed = parse_chat_request(
            &gateway(),
            json!({
                "model":"gpt-5.6",
                "messages":[
                    {"role":"user","content":[
                        {"type":"text","text":"what is in this image?"},
                        {"type":"image_url","image_url":{"url":"data:image/png;base64,iVBORw0KGgo=","detail":"high"}}
                    ]},
                    {"role":"assistant","content":"a cat"},
                    {"role":"user","content":[
                        {"type":"image_url","image_url":"https://example.com/dog.png"},
                        {"type":"text","text":"and this one?"}
                    ]}
                ]
            }),
        )
        .expect("image content must translate");
        // First user message is history: canonical Responses input_image parts.
        let history = &parsed.responses.input.prior_items[0];
        assert_eq!(history["content"][1]["type"], "input_image");
        assert_eq!(
            history["content"][1]["image_url"],
            "data:image/png;base64,iVBORw0KGgo="
        );
        assert_eq!(history["content"][1]["detail"], "high");
        // Final user message becomes the turn input: app-server image inputs.
        let turn_input = &parsed.responses.input.turn_input;
        assert_eq!(turn_input[0]["type"], "image");
        assert_eq!(turn_input[0]["url"], "https://example.com/dog.png");
        assert_eq!(turn_input[1], json!({"type":"text","text":"and this one?"}));
    }

    #[test]
    fn explicit_chat_sampling_defaults_are_accepted() {
        let parsed = parse_chat_request(
            &gateway(),
            json!({
                "model":"gpt-5.6",
                "messages":[{"role":"user","content":"hello"}],
                "temperature":1,
                "top_p":1.0,
                "presence_penalty":0,
                "frequency_penalty":0,
                "logprobs":false,
                "top_logprobs":0,
                "n":1
            }),
        )
        .expect("official defaults must not be rejected");
        assert_eq!(parsed.responses.input.turn_input.len(), 1);
    }

    #[test]
    fn chat_effort_and_verbosity_translate_to_responses_controls() {
        let parsed = parse_chat_request(
            &gateway(),
            json!({
                "model":"gpt-5.6",
                "messages":[{"role":"user","content":"hello"}],
                "reasoning_effort":"high",
                "verbosity":"low"
            }),
        )
        .expect("supported generation controls must translate");
        assert_eq!(parsed.responses.input.turn_input.len(), 1);
    }

    #[test]
    fn invalid_chat_verbosity_is_rejected() {
        let error = parse_chat_request(
            &gateway(),
            json!({
                "model":"gpt-5.6",
                "messages":[{"role":"user","content":"hello"}],
                "verbosity":"maximum"
            }),
        )
        .err()
        .expect("invalid verbosity must not be ignored");
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert!(error.message.contains("text.verbosity"));
    }

    #[test]
    fn stop_sequences_and_max_tokens_parse_and_shape_output() {
        let parsed = parse_chat_request(
            &gateway(),
            json!({
                "model":"gpt-5.6",
                "messages":[{"role":"user","content":"hello"}],
                "stop":["\n\n", "END"],
                "max_completion_tokens":10
            }),
        )
        .expect("stop and token caps must parse");
        assert_eq!(parsed.stop, vec!["\n\n".to_string(), "END".to_string()]);
        assert_eq!(parsed.max_output_chars, Some(40));

        let single = parse_chat_request(
            &gateway(),
            json!({
                "model":"gpt-5.6",
                "messages":[{"role":"user","content":"hello"}],
                "stop":"halt",
                "max_tokens":5
            }),
        )
        .expect("a lone stop string must parse");
        assert_eq!(single.stop, vec!["halt".to_string()]);
        assert_eq!(single.max_output_chars, Some(20));

        let too_many = parse_chat_request(
            &gateway(),
            json!({
                "model":"gpt-5.6",
                "messages":[{"role":"user","content":"hello"}],
                "stop":["a","b","c","d","e"]
            }),
        )
        .err()
        .expect("more than four stop sequences must be rejected like the official API");
        assert_eq!(too_many.status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn enforce_output_limits_cuts_at_stop_then_budget() {
        let (text, capped) =
            enforce_output_limits("hello END world".to_string(), &["END".to_string()], None);
        assert_eq!(text, "hello ");
        assert!(!capped);

        let (text, capped) = enforce_output_limits("x".repeat(100), &[], Some(40));
        assert_eq!(text.len(), 40);
        assert!(capped);

        // Stop wins before the budget and never reports a length cap.
        let (text, capped) =
            enforce_output_limits("abSTOPcdefgh".to_string(), &["STOP".to_string()], Some(4));
        assert_eq!(text, "ab");
        assert!(!capped);

        // The char budget never splits a multi-byte character.
        let (text, capped) = enforce_output_limits("ééééé".to_string(), &[], Some(5));
        assert_eq!(text, "éé");
        assert!(capped);
    }

    #[test]
    fn stop_filter_catches_sequences_straddling_deltas() {
        let mut filter = StopFilter::new(vec!["STOP".to_string()]).unwrap();
        // " ST" is held back as a potential stop prefix; emitted text joins to "hello ".
        assert_eq!(filter.push("hello ST"), "hello");
        assert_eq!(filter.push("OP world"), " ");
        assert!(filter.triggered);
        assert_eq!(filter.push("more"), "");
        assert_eq!(filter.finish(), "");

        let mut filter = StopFilter::new(vec!["END".to_string()]).unwrap();
        // hold_back = 2: every push flushes all but the last two bytes; joins to the full text.
        assert_eq!(filter.push("abc"), "a");
        assert_eq!(filter.push("def"), "bcd");
        assert_eq!(filter.finish(), "ef");

        assert!(StopFilter::new(Vec::new()).is_none());
    }

    #[test]
    fn legacy_functions_translate_to_tools() {
        let parsed = parse_chat_request(
            &gateway(),
            json!({
                "model":"gpt-5.6",
                "messages":[{"role":"user","content":"weather?"}],
                "functions":[{
                    "name":"get_weather",
                    "description":"Get weather",
                    "parameters":{"type":"object","properties":{"city":{"type":"string"}}}
                }],
                "function_call":"auto"
            }),
        )
        .expect("legacy functions must translate");
        assert_eq!(parsed.responses.dynamic_tools.len(), 1);
        assert_eq!(parsed.responses.dynamic_tools[0]["name"], "get_weather");

        let none = parse_chat_request(
            &gateway(),
            json!({
                "model":"gpt-5.6",
                "messages":[{"role":"user","content":"weather?"}],
                "functions":[{"name":"get_weather"}],
                "function_call":"none"
            }),
        )
        .expect("function_call=none must map to tool_choice=none");
        assert_eq!(none.responses.tool_choice, json!("none"));
        assert!(none.responses.dynamic_tools.is_empty());
    }

    #[test]
    fn named_participants_and_refusal_history_are_accepted() {
        let parsed = parse_chat_request(
            &gateway(),
            json!({
                "model":"gpt-5.6",
                "messages":[
                    {"role":"user","name":"alice","content":"hello"},
                    {"role":"assistant","content":"hi","refusal":null,"audio":null},
                    {"role":"user","content":"again"}
                ]
            }),
        )
        .expect("name/refusal/audio fields must be accepted and ignored");
        assert_eq!(parsed.responses.input.turn_input.len(), 1);
    }

    #[test]
    fn reasoning_summary_joins_for_reasoning_content() {
        let result = CodexTurnResult {
            output: vec![
                json!({
                    "type":"reasoning",
                    "summary":[
                        {"type":"summary_text","text":"first"},
                        {"type":"summary_text","text":"second"}
                    ]
                }),
                json!({
                    "type":"message",
                    "role":"assistant",
                    "content":[{"type":"output_text","text":"answer"}]
                }),
            ],
            usage: CodexUsage::default(),
        };
        assert_eq!(
            reasoning_summary_text(&result).as_deref(),
            Some("first\n\nsecond")
        );
        let empty = CodexTurnResult {
            output: Vec::new(),
            usage: CodexUsage::default(),
        };
        assert!(reasoning_summary_text(&empty).is_none());
    }

    #[test]
    fn structured_output_schema_is_preserved() {
        let translated = translate_response_format(&json!({
            "type":"json_schema",
            "json_schema":{"name":"answer","strict":true,"schema":{"type":"object"}}
        }))
        .unwrap();
        assert_eq!(translated["format"]["name"], "answer");
        assert_eq!(translated["format"]["strict"], true);
        assert_eq!(translated["format"]["schema"]["type"], "object");
    }
}
