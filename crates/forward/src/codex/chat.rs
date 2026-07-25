//! OpenAI-compatible Chat Completions adapter over the same Responses/app-server core.
//!
//! The adapter is deliberately strict: parameters that the app-server cannot honor are rejected
//! instead of being silently ignored. Both streaming and non-streaming calls use exact upstream
//! token usage for settlement.

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
        );
    }

    let result = match gateway.run_turn(prepared.turn.clone(), None).await {
        Ok(result) => result,
        Err(error) => return ApiError::from(error).into_response(),
    };
    if let Err(error) = admission.mark_delivering().await {
        return ApiError::from(error).into_response();
    }
    let response = completed_chat(&prepared, &result, &completion_id, created);
    admission.settle(&prepared.request.public_model, &result.usage);
    json_response(StatusCode::OK, response, &completion_id)
}

fn parse_chat_request(gateway: &CodexGateway, value: Value) -> Result<ParsedChat, ApiError> {
    let object = value
        .as_object()
        .ok_or_else(|| ApiError::invalid("Request body must be a JSON object.", None::<String>))?;
    let supported = [
        "model",
        "messages",
        "stream",
        "stream_options",
        "tools",
        "tool_choice",
        "parallel_tool_calls",
        "response_format",
        "reasoning_effort",
        "metadata",
        "store",
        "n",
        "temperature",
        "top_p",
        "max_tokens",
        "max_completion_tokens",
        "stop",
        "presence_penalty",
        "frequency_penalty",
        "logprobs",
        "top_logprobs",
        "seed",
        "user",
        "service_tier",
    ];
    if let Some(field) = object
        .keys()
        .find(|field| !supported.iter().any(|supported| field == supported))
    {
        return Err(ApiError::invalid(
            format!("Parameter {field:?} is not supported by this endpoint."),
            Some(field.clone()),
        ));
    }
    validate_default_only_chat_parameters(object)?;

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
    if let Some(format) = object
        .get("response_format")
        .filter(|value| !value.is_null())
    {
        responses.insert("text".to_string(), translate_response_format(format)?);
    }
    if let Some(tools) = object.get("tools").filter(|value| !value.is_null()) {
        responses.insert("tools".to_string(), translate_chat_tools(tools)?);
    }
    if let Some(choice) = object.get("tool_choice").filter(|value| !value.is_null()) {
        responses.insert("tool_choice".to_string(), choice.clone());
    }

    let include_usage = parse_stream_options(object.get("stream_options"))?;
    let responses = parse_responses_request(gateway, Value::Object(responses))?;
    Ok(ParsedChat {
        responses,
        base_instructions,
        include_usage,
    })
}

fn validate_default_only_chat_parameters(object: &Map<String, Value>) -> Result<(), ApiError> {
    if object
        .get("n")
        .is_some_and(|value| !value.is_null() && value.as_u64() != Some(1))
    {
        return Err(unsupported_non_default("n"));
    }
    for field in [
        "max_tokens",
        "max_completion_tokens",
        "stop",
        "seed",
        "user",
    ] {
        if object.get(field).is_some_and(|value| !value.is_null()) {
            return Err(unsupported_non_default(field));
        }
    }
    for field in ["temperature", "top_p"] {
        if object
            .get(field)
            .is_some_and(|value| !value.is_null() && value.as_f64() != Some(1.0))
        {
            return Err(unsupported_non_default(field));
        }
    }
    for field in ["presence_penalty", "frequency_penalty"] {
        if object.get(field).is_some_and(|value| {
            !value.is_null() && value.as_f64().is_none_or(|number| number != 0.0)
        }) {
            return Err(unsupported_non_default(field));
        }
    }
    if object
        .get("logprobs")
        .is_some_and(|value| !value.is_null() && value.as_bool().is_none_or(|enabled| enabled))
    {
        return Err(unsupported_non_default("logprobs"));
    }
    if object
        .get("top_logprobs")
        .is_some_and(|value| !value.is_null() && value.as_u64().is_none_or(|count| count != 0))
    {
        return Err(unsupported_non_default("top_logprobs"));
    }
    if object
        .get("store")
        .is_some_and(|value| !value.is_null() && value.as_bool().is_none_or(|store| store))
    {
        return Err(ApiError::invalid(
            "store=true is not supported because Chat Completions retrieval is not exposed.",
            Some("store".to_string()),
        ));
    }
    if object.get("service_tier").is_some_and(|value| {
        !value.is_null() && !matches!(value.as_str(), Some("auto" | "default"))
    }) {
        return Err(unsupported_non_default("service_tier"));
    }
    Ok(())
}

fn unsupported_non_default(field: &str) -> ApiError {
    ApiError::invalid(
        format!("Non-default {field} is not supported by this endpoint and was not ignored."),
        Some(field.to_string()),
    )
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
    if let Some(field) = object
        .keys()
        .find(|field| field.as_str() != "include_usage")
    {
        return Err(ApiError::invalid(
            format!("stream_options.{field} is not supported."),
            Some(format!("stream_options.{field}")),
        ));
    }
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
        if object.get("name").is_some_and(|value| !value.is_null()) {
            return Err(ApiError::invalid(
                "Named chat participants are not supported.",
                Some(format!("messages.{index}.name")),
            ));
        }
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
                let text = chat_content_text(
                    object.get("content"),
                    false,
                    &format!("messages.{index}.content"),
                )?
                .ok_or_else(|| {
                    ApiError::invalid(
                        "User message content must not be null.",
                        Some(format!("messages.{index}.content")),
                    )
                })?;
                input.push(chat_message_item("user", &text));
            }
            "assistant" => {
                if object.get("refusal").is_some_and(|value| !value.is_null())
                    || object.get("audio").is_some_and(|value| !value.is_null())
                {
                    return Err(ApiError::invalid(
                        "Assistant refusal/audio history is not supported by this text endpoint.",
                        Some(format!("messages.{index}")),
                    ));
                }
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

fn completed_chat(
    prepared: &PreparedTurn,
    result: &CodexTurnResult,
    completion_id: &str,
    created: i64,
) -> Value {
    let (content, tool_calls) = chat_output(result);
    let finish_reason = if tool_calls.is_empty() {
        "stop"
    } else {
        "tool_calls"
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

fn stream_chat(
    gateway: Arc<CodexGateway>,
    prepared: PreparedTurn,
    admission: super::billing::CodexAdmission,
    completion_id: String,
    created: i64,
    include_usage: bool,
) -> Response {
    let (frame_tx, frame_rx) = mpsc::channel::<Bytes>(128);
    let request_id_header = completion_id.clone();
    tokio::spawn(async move {
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
        let run = tokio::spawn(async move { run_gateway.run_turn(turn, Some(update_tx)).await });
        let mut tool_index = 0usize;
        let mut emitted_tools = HashSet::new();
        let mut downstream_closed = false;

        loop {
            tokio::select! {
                _ = frame_tx.closed() => {
                    downstream_closed = true;
                    break;
                }
                update = update_rx.recv() => {
                    let Some(update) = update else { break };
                    let delta = match update {
                        TurnUpdate::TextDelta { delta, .. } => json!({"content": delta}),
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
                        TurnUpdate::ReasoningSummaryPartAdded { .. }
                        | TurnUpdate::ReasoningSummaryDelta { .. } => continue,
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
        let finish_reason = if authoritative_tools.is_empty() {
            "stop"
        } else {
            "tool_calls"
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
        CodexGateway::new(CodexConfig {
            enabled: true,
            binary: "/tmp/codex".to_string(),
            binary_sha256: "0".repeat(64),
            expected_version: "codex-cli test".to_string(),
            homes: vec!["/tmp/home".to_string()],
            work_dir: "/tmp/work".to_string(),
            startup_timeout_ms: 1,
            request_timeout_ms: 1,
            turn_timeout_ms: 1,
            max_concurrent_turns: 1,
            admit_below_used_percent: 95,
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
    fn unsupported_sampling_is_rejected_not_ignored() {
        let error = parse_chat_request(
            &gateway(),
            json!({
                "model":"gpt-5.6",
                "messages":[{"role":"user","content":"hello"}],
                "temperature":0.2
            }),
        )
        .err()
        .expect("must reject");
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert!(!error.message.contains("Codex"));
        assert!(!error.message.contains("app-server"));
        assert!(!error.message.contains("ChatGPT"));
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
