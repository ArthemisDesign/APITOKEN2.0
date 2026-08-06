//! Anthropic Messages adapter ("Anthropic Skin") over the native Codex Responses core —
//! stage 5.1 of docs/engine/UNIFIED_ROUTER.md (decision 6: stage 5 mirrors stages 3–4).
//!
//! `POST /v1/messages` and `POST /v1/messages/count_tokens` on the OpenAI plane. The router
//! (`crates/router/src/messages.rs`) performs model-based dispatch and proxies the body
//! unchanged; this adapter strips the `openai/` namespace prefix, translates the Messages
//! request into a Responses request and runs it through the exact same turn pipeline as the
//! Chat adapter (`chat.rs`): admission reserve, affinity routing, provider turn,
//! authoritative-usage settlement. The dictionary is the mirror of `anthropic_responses.rs`
//! (stages 4.1+4.2): the same item forms, replay rules and unsigned reasoning.
//!
//! Request translation: top-level `system` (string or text blocks, joined with "\n\n") →
//! `instructions`; user text/image blocks → `input_text`/`input_image` parts (base64 source →
//! data: URL, url source → URL verbatim — the shared `canonical_image_part` of `api.rs`, same
//! as `chat.rs`); assistant text → `output_text` parts; assistant `tool_use` →
//! `function_call` items (`id` → `call_id`, `input` object → JSON `arguments` string — the
//! mirror of the 4.2 replay); user `tool_result` → `function_call_output` items
//! (`tool_use_id` → `call_id`; string content as-is, text part arrays joined with "\n",
//! non-text parts → 400; `is_error` accepted and ignored — the text still conveys it);
//! `thinking`/`redacted_thinking` input blocks are dropped (decision 6, no thinking replay);
//! `tools[]` → Responses function tools (`input_schema` → `parameters`); `tool_choice`
//! auto/any/none/tool → auto/required/none/named function (`disable_parallel_tool_use: true`
//! → `parallel_tool_calls: false`); `max_tokens` → `max_output_tokens` (it also bounds the
//! delivered text at ~4 chars/token, same policy as the Chat adapter); `stop_sequences` are
//! honored on the delivered text via the shared `StopFilter` (the transport cannot stop
//! generation upstream); `thinking` → `reasoning.effort` (lossy, documented: `disabled` and
//! `adaptive` → model default; `enabled` budget < 4096 → "low", < 16384 → "medium", otherwise
//! "high"; an effort the model does not advertise degrades to its default inside the shared
//! Responses parser).
//!
//! Capability matrix (decision 3, fail-closed modulo defaults): stateful or unknown
//! `cache_control` anywhere (system, content blocks, tools — Codex prompt caching is automatic),
//! stateful or unknown `context_management`, `mcp_servers`, `container` →
//! `400 invalid_request_error` naming the parameter in the message. Claude Code's bounded
//! no-op context form (`clear_thinking_20251015` with `keep:"all"`) and an empty edits list are
//! accepted and ignored; the adapter already drops replayed thinking and never claims server-side
//! context mutation. Claude Code's exact `{type:"ephemeral"}` cache breakpoints are also accepted
//! and removed because Codex caching is automatic; extended cache policy remains fail-closed.
//! Native `output_config.effort` maps to Responses `reasoning.effort`, while
//! `output_config.format` maps to `text.format` with the same JSON Schema; both are bounded to the
//! exact Messages GA shapes used by current Claude Code. `metadata` (including `user_id`),
//! `temperature`/`top_p`/`top_k` and unknown fields are accepted and ignored, matching the Chat
//! adapter's leniency for controls the transport cannot honor. Every error
//! of this endpoint — adapter validation, shared parser, admission, billing — is rebuilt in
//! the Anthropic envelope (`{"type":"error","error":{"type":...,"message":...}}`) with status
//! and `Retry-After` preserved, because the endpoint speaks Messages (Claude Code recovers
//! by error text).
//!
//! Response translation (mirror of 4.1+4.2): output `message` items → text blocks (joined;
//! client stop sequences and the output budget are enforced on the joined text exactly like
//! `chat.rs`), `function_call` → `tool_use` blocks (`call_id` → `id`, `arguments` parsed into
//! `input`, malformed JSON degrades to `{}`), `reasoning` → `thinking` blocks WITHOUT
//! signature (summary parts joined with "\n\n"); usage → Messages usage (cache write/read →
//! `cache_creation_input_tokens`/`cache_read_input_tokens` when > 0, reasoning tokens →
//! `output_tokens_details.thinking_tokens`); stop reason: a function_call in output →
//! `tool_use`, output budget cut → `max_tokens`, matched stop sequence → `stop_sequence`,
//! otherwise `end_turn`. Streaming (`stream:true`): `message_start` with zeroed usage
//! (authoritative usage exists only at turn end — documented limitation) → per-block
//! `content_block_start`/`content_block_delta` (`text_delta`, `thinking_delta` for reasoning,
//! `input_json_delta` for arguments)/`content_block_stop` → `message_delta` (stop reason +
//! authoritative usage) → `message_stop`; the heartbeat emits `event: ping`; a mid-stream
//! provider failure emits `event: error`. The response is never buffered as a whole (Claude
//! Code requirement); on client disconnect the provider turn keeps running to its
//! authoritative usage for settlement, as in `chat.rs`.
//!
//! `anthropic-version`/`anthropic-beta` headers and `?beta=true` are tolerated and never
//! proxied upstream (the upstream request is built by the turn runner from the translated
//! body, not by header passthrough).
//!
//! count_tokens: the same Messages parse + shared `parse_responses_request`/`prepare_turn`
//! pipeline yields the reserve-grade input estimate (the `openai_input_tokens` logic, no
//! network self-call). `max_tokens` is optional there because the official endpoint does not
//! require it.

use super::api::{
    normalize_output_item, parse_responses_request, prepare_turn, ApiError, PreparedTurn,
    MAX_INSTRUCTIONS_BYTES, OPENAI_BODY_LIMIT,
};
use super::billing::begin_admission;
use super::chat::{
    enforce_output_limits, output_chars_for, send_chat_bytes, ChatReceiverStream, StopFilter,
};
use super::{new_id, CodexGateway, CodexTurnResult, CodexUsage, TurnUpdate};
use crate::proxy::{with_not_started, without_not_started, TerminalErrorReason};
use crate::state::AppState;
use crate::validation::optional_bool;
use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

// ---------- Anthropic-envelope errors ----------

/// Anthropic-envelope error (`{"type":"error","error":{...}}`) with a privacy-safe static
/// audit reason, mirroring the native lane's `local_err` shape (`proxy.rs`).
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
    // Normal skin errors are pre-execution. An external fallback failure is different: the
    // ClaudeStore send already crossed the execution boundary, so preserving `not_started` would
    // let the router replay an ambiguously billable request on another provider.
    if reason == "claudestore_fallback_failed" {
        response
    } else {
        with_not_started(response)
    }
}

/// 400 adapter-validation error; the parameter name lives inside the message (the Anthropic
/// envelope has no `param` field).
fn invalid_request(message: impl Into<String>) -> Response {
    skin_error(
        StatusCode::BAD_REQUEST,
        "invalid_request_error",
        message,
        "invalid_messages_request",
        None,
    )
}

/// Rebuilds an OpenAI-envelope `ApiError` from the shared Responses pipeline in the Anthropic
/// envelope this endpoint speaks. Status and `Retry-After` are preserved; the Anthropic error
/// type is chosen by status, mirroring the native lane's authentic triples (`proxy.rs`):
/// 401 → authentication_error, 402 → invalid_request_error (balance is legitimate account
/// state), 503 → retryable 529 overloaded_error, everything else server-side → api_error.
fn anthropic_error(error: ApiError) -> Response {
    let retry_after = error.retry_after;
    let reason = error.reason;
    let (status, kind, message) = match error.status.as_u16() {
        400 => (
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            error.message,
        ),
        401 => (
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            "invalid x-api-key".to_string(),
        ),
        402 => (
            StatusCode::PAYMENT_REQUIRED,
            "invalid_request_error",
            error.message,
        ),
        404 => (StatusCode::NOT_FOUND, "not_found_error", error.message),
        429 => (
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_error",
            error.message,
        ),
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
    };
    skin_error(status, kind, message, reason, retry_after)
}

/// 200 JSON response with the Anthropic-style `request-id` header.
fn skin_json_response(body: Value, request_id: &str) -> Response {
    let mut response = (StatusCode::OK, axum::Json(body)).into_response();
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response.headers_mut().insert("request-id", value);
    }
    response
}

// ---------- request translation (Messages → Responses) ----------

/// Result of the Messages→Responses request translation: the Responses-shaped request value
/// for the shared parser plus the client-side delivery controls the transport cannot enforce
/// upstream (stop sequences and the approximate output budget, same as `chat.rs`).
#[derive(Debug)]
struct ParsedSkin {
    responses: Value,
    stop: Vec<String>,
    max_output_chars: Option<usize>,
}

/// Translates a Messages request body into the Responses request value. Validation errors are
/// ready-made Anthropic-envelope responses. When `require_max_tokens` is false (count_tokens),
/// a missing `max_tokens` is tolerated: the official token-counting endpoint does not require
/// it and the input estimate does not use it.
fn translate_messages_request(
    value: Value,
    require_max_tokens: bool,
) -> Result<ParsedSkin, Response> {
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
    // The namespaced ID is resolved here, not in the router and not in metering — the mirror
    // of the `anthropic/` strip in `anthropic_responses.rs`.
    let model = model.strip_prefix("openai/").unwrap_or(&model).to_string();
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

    let instructions = translate_system(object.remove("system"))?;
    if instructions
        .as_ref()
        .is_some_and(|value| value.len() > MAX_INSTRUCTIONS_BYTES)
    {
        return Err(invalid_request(
            "System instructions exceed the 1 MiB limit (parameter: system).",
        ));
    }

    let messages = match object.remove("messages") {
        Some(Value::Array(messages)) if !messages.is_empty() => messages,
        _ => {
            return Err(invalid_request(
                "Missing or invalid required parameter: messages must be a non-empty array.",
            ))
        }
    };
    let input = translate_messages(&messages)?;
    if input.is_empty() {
        return Err(invalid_request(
            "messages must contain at least one text, image, tool_use or tool_result block.",
        ));
    }

    let mut responses = Map::new();
    responses.insert("model".to_string(), Value::String(model));
    responses.insert("input".to_string(), Value::Array(input));
    // Anthropic-native harnesses opt into Fast with `speed: "fast"`; OpenAI-compatible
    // harnesses commonly preserve `service_tier` even when they use a Messages wire. Accept both
    // public spellings and hand the canonical request value to the shared Responses admission so
    // reserve, settlement and public usage evidence all use the effective Fast tier.
    let requested_fast = object
        .get("speed")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("fast"))
        || object
            .get("service_tier")
            .and_then(Value::as_str)
            .is_some_and(|value| {
                value.eq_ignore_ascii_case("fast") || value.eq_ignore_ascii_case("priority")
            });
    if requested_fast {
        responses.insert("service_tier".to_string(), json!("priority"));
    }
    if let Some(instructions) = instructions {
        responses.insert("instructions".to_string(), Value::String(instructions));
    }
    // Stored responses are not claimed through this adapter (same stance as `chat.rs`).
    responses.insert("store".to_string(), Value::Bool(false));
    if max_tokens > 0 {
        responses.insert("max_output_tokens".to_string(), Value::from(max_tokens));
    }
    let stream = optional_bool(&object, "stream")
        .map_err(|_| invalid_request("stream must be a boolean."))?
        .unwrap_or(false);
    if stream {
        responses.insert("stream".to_string(), Value::Bool(true));
    }
    if let Some(tools) = object.get("tools").filter(|value| !value.is_null()) {
        let tools = translate_tools(tools)?;
        if !tools.is_empty() {
            responses.insert("tools".to_string(), Value::Array(tools));
        }
    }
    let (tool_choice, parallel_tool_calls) = translate_tool_choice(&object)?;
    if let Some(tool_choice) = tool_choice {
        responses.insert("tool_choice".to_string(), tool_choice);
    }
    if let Some(parallel) = parallel_tool_calls {
        responses.insert("parallel_tool_calls".to_string(), Value::Bool(parallel));
    }
    let (output_effort, output_format) = translate_output_config(object.get("output_config"))?;
    let thinking_effort = translate_thinking(object.get("thinking"))?;
    if let Some(effort) = output_effort.or(thinking_effort) {
        responses.insert("reasoning".to_string(), json!({"effort": effort}));
    }
    if let Some(format) = output_format {
        responses.insert("text".to_string(), json!({"format": format}));
    }
    let stop = parse_stop_sequences(object.get("stop_sequences"))?;

    Ok(ParsedSkin {
        responses: Value::Object(responses),
        stop,
        max_output_chars: output_chars_for(Some(max_tokens).filter(|tokens| *tokens > 0)),
    })
}

/// Capability matrix (decision 3): Messages parameters the Codex transport cannot honor are
/// rejected when set to a non-default value. `metadata` and sampling controls are handled by
/// the lenient open list below (accepted and ignored, like `chat.rs`).
fn check_capability_matrix(object: &Map<String, Value>) -> Result<(), Response> {
    let rules: [(&str, fn(&Value) -> bool); 3] = [
        ("context_management", is_ignorable_context_management),
        ("mcp_servers", |value| {
            value.is_null() || value.as_array().is_some_and(Vec::is_empty)
        }),
        ("container", |value| value.is_null()),
    ];
    for (param, is_default) in rules {
        if let Some(value) = object.get(param) {
            if !is_default(value) {
                return Err(invalid_request(format!(
                    "Unsupported parameter: '{param}' is not supported with this endpoint."
                )));
            }
        }
    }
    Ok(())
}

/// The stateless Responses transport cannot apply Anthropic context edits. Claude Code 2.1.220
/// nevertheless sends this exact no-op form on every generation request. Accept only an empty
/// edit list or the observed `clear_thinking` + `keep:"all"` form; every stateful, extended or
/// future shape stays fail-closed until it has authoritative semantics. Replayed thinking blocks
/// are already dropped by this adapter, so ignoring the accepted form does not create hidden
/// server-side conversation state.
fn is_ignorable_context_management(value: &Value) -> bool {
    if value.is_null() {
        return true;
    }
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.len() != 1 {
        return false;
    }
    let Some(edits) = object.get("edits").and_then(Value::as_array) else {
        return false;
    };
    match edits.as_slice() {
        [] => true,
        [edit] => edit.as_object().is_some_and(|edit| {
            edit.len() == 2
                && edit.get("type").and_then(Value::as_str) == Some("clear_thinking_20251015")
                && edit.get("keep").and_then(Value::as_str) == Some("all")
        }),
        _ => false,
    }
}

/// Messages GA `output_config` → native Responses generation controls. The Codex transport
/// supports both dimensions exactly: effort is parsed by the shared model capability resolver,
/// and a JSON Schema becomes the Responses output schema. Keep the accepted object closed so a
/// future Messages control cannot be silently dropped or misrepresented.
fn translate_output_config(
    value: Option<&Value>,
) -> Result<(Option<String>, Option<Value>), Response> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok((None, None));
    };
    let object = value.as_object().ok_or_else(|| {
        invalid_request("Invalid type for parameter: output_config must be an object.")
    })?;
    if let Some(unknown) = object
        .keys()
        .find(|key| !matches!(key.as_str(), "effort" | "format"))
    {
        return Err(invalid_request(format!(
            "Unsupported parameter: 'output_config.{unknown}' is not supported with this endpoint."
        )));
    }

    let effort =
        match object.get("effort") {
            None | Some(Value::Null) => None,
            Some(Value::String(effort)) if matches!(effort.as_str(), "low" | "medium" | "high") => {
                Some(effort.clone())
            }
            Some(_) => return Err(invalid_request(
                "Invalid value for parameter: output_config.effort must be low, medium, or high.",
            )),
        };

    let format = match object.get("format") {
        None | Some(Value::Null) => None,
        Some(Value::Object(format))
            if format.len() == 2
                && format.get("type").and_then(Value::as_str) == Some("json_schema")
                && format.get("schema").is_some_and(Value::is_object) =>
        {
            Some(Value::Object(format.clone()))
        }
        Some(_) => {
            return Err(invalid_request(
                "Invalid value for parameter: output_config.format must be a json_schema with a schema object.",
            ))
        }
    };
    Ok((effort, format))
}

/// Codex prompt caching is automatic and cannot be steered by client breakpoints. Current Claude
/// Code nevertheless annotates system and user blocks with the exact Anthropic ephemeral marker.
/// Accept and remove only that stateless hint; any extended retention or future policy remains
/// fail-closed instead of being silently misrepresented.
fn reject_cache_control(block: &Value, param: &str) -> Result<(), Response> {
    if block
        .get("cache_control")
        .is_some_and(|value| !is_ignorable_cache_control(value))
    {
        return Err(invalid_request(format!(
            "Unsupported parameter: 'cache_control' is not supported with this endpoint (in {param})."
        )));
    }
    Ok(())
}

fn is_ignorable_cache_control(value: &Value) -> bool {
    value.is_null()
        || value.as_object().is_some_and(|object| {
            object.len() == 1 && object.get("type").and_then(Value::as_str) == Some("ephemeral")
        })
}

/// Top-level `system` → joined instruction text: a plain string or an array of text blocks
/// joined with "\n\n" (the `chat.rs` system join). Any other block type or an unsupported
/// `cache_control` shape → 400.
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

/// Messages conversation → Responses input items. User/assistant messages carry their
/// text/image parts as message items; tool_use/tool_result blocks become standalone
/// function_call/function_call_output items in block order (the mirror of the 4.2 replay,
/// which merges them back into Messages messages).
fn translate_messages(messages: &[Value]) -> Result<Vec<Value>, Response> {
    let mut input = Vec::new();
    let mut call_ids = HashSet::new();
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
        match role {
            "user" => translate_user_content(object.get("content"), &param, &mut input)?,
            "assistant" => translate_assistant_content(
                object.get("content"),
                &param,
                &mut input,
                &mut call_ids,
            )?,
            _ => {
                return Err(invalid_request(format!(
                    "Message role {role:?} is not supported ({param}.role)."
                )))
            }
        }
    }
    Ok(input)
}

/// Accumulated input parts of one user message, flushed as a message item before every
/// function_call_output and at the end of the message (block order is preserved).
fn flush_user_parts(input: &mut Vec<Value>, parts: &mut Vec<Value>) {
    if !parts.is_empty() {
        input.push(json!({
            "type": "message",
            "role": "user",
            "content": std::mem::take(parts)
        }));
    }
}

fn translate_user_content(
    content: Option<&Value>,
    param: &str,
    input: &mut Vec<Value>,
) -> Result<(), Response> {
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
            input.push(json!({
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": text}]
            }));
        }
        Value::Array(blocks) => {
            let mut parts: Vec<Value> = Vec::new();
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
                        parts.push(json!({"type": "input_text", "text": text}));
                    }
                    Some("image") => parts.push(translate_image_block(block, &block_param)?),
                    Some("tool_result") => {
                        flush_user_parts(input, &mut parts);
                        input.push(translate_tool_result(block, &block_param)?);
                    }
                    // thinking/redacted_thinking in the input are dropped (decision 6: no
                    // thinking replay for non-Claude models).
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
            flush_user_parts(input, &mut parts);
        }
        _ => {
            return Err(invalid_request(format!(
                "Message content must be a string or a content-block array ({param}.content)."
            )))
        }
    }
    Ok(())
}

/// Messages image block → canonical Responses `input_image` part (the shared
/// `canonical_image_part` of `api.rs`): base64 source → data: URL, url source → URL verbatim.
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
    let url = match source.get("type").and_then(Value::as_str) {
        Some("base64") => {
            let media_type = required("media_type")?;
            let data = required("data")?;
            format!("data:{media_type};base64,{data}")
        }
        Some("url") => required("url")?.to_string(),
        _ => {
            return Err(invalid_request(format!(
                "Image source type is not supported: only base64 and url are supported ({param}.source.type)."
            )))
        }
    };
    super::api::canonical_image_part(&Value::String(url), None, param).map_err(anthropic_error)
}

/// user `tool_result` block → `function_call_output` item (mirror of the 4.2 replay):
/// `tool_use_id` → `call_id`; string content as-is, text part arrays joined with "\n",
/// non-text parts → 400. `is_error` has no Responses equivalent and is accepted and ignored.
fn translate_tool_result(block: &Value, param: &str) -> Result<Value, Response> {
    let tool_use_id = block
        .get("tool_use_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            invalid_request(format!(
                "tool_result requires a non-empty tool_use_id ({param}.tool_use_id)."
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
        "type": "function_call_output",
        "call_id": tool_use_id,
        "output": output
    }))
}

/// Accumulated output_text parts of one assistant message, flushed before every
/// function_call and at the end of the message.
fn flush_assistant_parts(input: &mut Vec<Value>, parts: &mut Vec<Value>) {
    if !parts.is_empty() {
        input.push(json!({
            "type": "message",
            "role": "assistant",
            "content": std::mem::take(parts)
        }));
    }
}

fn translate_assistant_content(
    content: Option<&Value>,
    param: &str,
    input: &mut Vec<Value>,
    call_ids: &mut HashSet<String>,
) -> Result<(), Response> {
    let content = content.ok_or_else(|| {
        invalid_request(format!("Message content is required ({param}.content)."))
    })?;
    match content {
        Value::String(text) => {
            if !text.is_empty() {
                input.push(json!({
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": text}]
                }));
            }
        }
        Value::Array(blocks) => {
            let mut parts: Vec<Value> = Vec::new();
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
                            parts.push(json!({"type": "output_text", "text": text}));
                        }
                    }
                    Some("tool_use") => {
                        flush_assistant_parts(input, &mut parts);
                        input.push(translate_tool_use(block, &block_param, call_ids)?);
                    }
                    // thinking/redacted_thinking in the input are dropped (decision 6).
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
            flush_assistant_parts(input, &mut parts);
        }
        _ => {
            return Err(invalid_request(format!(
                "Message content must be a string or a content-block array ({param}.content)."
            )))
        }
    }
    Ok(())
}

/// assistant `tool_use` block → `function_call` item (mirror of the 4.2 replay): `id` →
/// `call_id`, `input` object → JSON `arguments` string, status completed.
fn translate_tool_use(
    block: &Value,
    param: &str,
    call_ids: &mut HashSet<String>,
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
    if !call_ids.insert(id.to_string()) {
        return Err(invalid_request(format!(
            "Duplicate tool_use id {id:?} ({param}.id)."
        )));
    }
    let name = required("name")?;
    let tool_input = match block.get("input") {
        None | Some(Value::Null) => json!({}),
        Some(value @ Value::Object(_)) => value.clone(),
        _ => {
            return Err(invalid_request(format!(
                "tool_use input must be an object ({param}.input)."
            )))
        }
    };
    let arguments = serde_json::to_string(&tool_input).unwrap_or_else(|_| "{}".to_string());
    Ok(json!({
        "type": "function_call",
        "call_id": id,
        "name": name,
        "arguments": arguments,
        "status": "completed"
    }))
}

/// Messages `tools[]` → Responses function tools: only custom tools (the default type) are
/// supported; `input_schema` → `parameters`. Server tools (web_search etc.) and an unsupported
/// `cache_control` shape → 400.
fn translate_tools(value: &Value) -> Result<Vec<Value>, Response> {
    let tools = value
        .as_array()
        .ok_or_else(|| invalid_request("Invalid type for parameter: tools must be an array."))?;
    let mut translated = Vec::with_capacity(tools.len());
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
        let mut out = json!({"type": "function", "name": name});
        if let Some(description) = object.get("description").and_then(Value::as_str) {
            out["description"] = Value::String(description.to_string());
        }
        out["parameters"] = object
            .get("input_schema")
            .cloned()
            .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
        translated.push(out);
    }
    Ok(translated)
}

/// Messages `tool_choice` → Responses `tool_choice` + `parallel_tool_calls`: auto → omitted
/// (the default), any → "required", none → "none", tool → the named function form;
/// `disable_parallel_tool_use: true` → `parallel_tool_calls: false`. The mirror of
/// `translate_responses_tool_choice` in `anthropic_responses.rs`.
fn translate_tool_choice(
    object: &Map<String, Value>,
) -> Result<(Option<Value>, Option<bool>), Response> {
    let Some(choice) = object.get("tool_choice").filter(|value| !value.is_null()) else {
        return Ok((None, None));
    };
    let choice = choice.as_object().ok_or_else(|| {
        invalid_request("Invalid type for parameter: tool_choice must be an object.")
    })?;
    let disable_parallel = choice
        .get("disable_parallel_tool_use")
        .and_then(Value::as_bool)
        == Some(true);
    let mapped = match choice.get("type").and_then(Value::as_str) {
        Some("auto") => None,
        Some("any") => Some(Value::String("required".to_string())),
        Some("none") => Some(Value::String("none".to_string())),
        Some("tool") => {
            let name = choice.get("name").and_then(Value::as_str).ok_or_else(|| {
                invalid_request("Invalid value for parameter: tool_choice requires a tool name.")
            })?;
            Some(json!({"type": "function", "name": name}))
        }
        _ => return Err(invalid_request("Invalid value for parameter: tool_choice.")),
    };
    Ok((mapped, disable_parallel.then_some(false)))
}

/// Messages `thinking` → Responses `reasoning.effort` (simplest documented mapping, lossy):
/// `disabled` and `adaptive` → the model default (no reasoning field — adaptive means the
/// model decides; `display` is irrelevant because signatures are never exposed, decision 6);
/// `enabled` maps the budget onto effort thresholds: < 4096 → "low", < 16384 → "medium",
/// otherwise "high". Budgets below the Messages minimum (1024) → 400. An effort the model
/// does not advertise degrades to the model default inside the shared Responses parser (same
/// leniency as `chat.rs` reasoning_effort).
fn translate_thinking(value: Option<&Value>) -> Result<Option<String>, Response> {
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
            Ok(Some(
                match budget {
                    0..=4095 => "low",
                    4096..=16383 => "medium",
                    _ => "high",
                }
                .to_string(),
            ))
        }
        _ => Err(invalid_request(
            "Invalid value for parameter: thinking.type.",
        )),
    }
}

/// `stop_sequences` are honored on the delivered text via the shared `StopFilter` (the
/// transport cannot stop generation upstream — same enforcement point as `chat.rs`).
fn parse_stop_sequences(value: Option<&Value>) -> Result<Vec<String>, Response> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(Vec::new());
    };
    let items = value.as_array().ok_or_else(|| {
        invalid_request("Invalid type for parameter: stop_sequences must be an array of strings.")
    })?;
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        out.push(
            item.as_str().ok_or_else(|| {
                invalid_request(format!("stop_sequences.{index} must be a string."))
            })?,
        );
    }
    Ok(out
        .into_iter()
        .filter(|sequence| !sequence.is_empty())
        .map(str::to_string)
        .collect())
}

// ---------- response translation (Responses → Messages) ----------

/// Messages usage from the authoritative turn usage (mirror of `map_responses_usage` in
/// `anthropic_responses.rs`): cache write/read surface only when non-zero, reasoning tokens
/// land in `output_tokens_details.thinking_tokens`.
fn messages_usage(usage: &CodexUsage, effective_service_tier: Option<&str>) -> Value {
    let mut mapped = json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "service_tier": if effective_service_tier == Some("priority") {
            "priority"
        } else {
            "standard"
        },
    });
    if usage.cache_write_input_tokens > 0 {
        mapped["cache_creation_input_tokens"] = Value::from(usage.cache_write_input_tokens);
    }
    if usage.cached_input_tokens > 0 {
        mapped["cache_read_input_tokens"] = Value::from(usage.cached_input_tokens);
    }
    if usage.reasoning_output_tokens > 0 {
        mapped["output_tokens_details"] = json!({"thinking_tokens": usage.reasoning_output_tokens});
    }
    mapped
}

/// Summary text of one reasoning output item: parts joined with "\n\n" (the same join
/// `chat.rs` uses for `reasoning_content`).
fn reasoning_item_text(item: &Value) -> String {
    let mut text = String::new();
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
    text
}

/// Output items → Messages content blocks (mirror of `output_items` in
/// `anthropic_responses.rs`): message items contribute their output_text parts to one joined
/// text block at the position of the first message item, function_call → tool_use blocks,
/// reasoning → thinking blocks without signature (empty reasoning yields no block). Returns
/// the blocks, the joined text, whether a tool call is present and the slot the joined text
/// block belongs at (the first message item's position; None when no message item exists).
fn content_blocks(output: &[Value]) -> (Vec<Value>, String, bool, Option<usize>) {
    let mut blocks: Vec<Value> = Vec::new();
    let mut text = String::new();
    let mut text_at: Option<usize> = None;
    let mut has_tool_use = false;
    for item in output.iter().filter_map(normalize_output_item) {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                if text_at.is_none() {
                    text_at = Some(blocks.len());
                }
                for part in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                        text.push_str(part_text);
                    }
                }
            }
            Some("function_call") => {
                has_tool_use = true;
                let id = item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| new_id("toolu"));
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let tool_input = item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                    .filter(|value| value.is_object())
                    .unwrap_or_else(|| json!({}));
                blocks.push(json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": tool_input
                }));
            }
            Some("reasoning") => {
                let thinking = reasoning_item_text(&item);
                if !thinking.is_empty() {
                    blocks.push(json!({"type": "thinking", "thinking": thinking}));
                }
            }
            _ => {}
        }
    }
    (blocks, text, has_tool_use, text_at)
}

/// Completed non-stream Messages response (the mirror of `json_responses_response` of stage
/// 4.1). Client stop sequences and the approximate output budget are enforced on the joined
/// text exactly like `chat.rs` (the transport cannot cap generation upstream).
fn completed_message(
    prepared: &PreparedTurn,
    result: &CodexTurnResult,
    message_id: &str,
    stop: &[String],
    max_output_chars: Option<usize>,
) -> Value {
    let (mut blocks, text, has_tool_use, text_at) = content_blocks(&result.output);
    let matched_stop = stop
        .iter()
        .find(|sequence| text.contains(sequence.as_str()))
        .cloned();
    let (text, capped) = enforce_output_limits(text, stop, max_output_chars);
    if !text.is_empty() {
        // The joined text block sits at the position of the first message item (the mirror of
        // the stage-4.1 rule); non-empty text implies a message item, so None is defensive.
        let position = text_at.unwrap_or(blocks.len()).min(blocks.len());
        blocks.insert(position, json!({"type": "text", "text": text}));
    }
    let stop_reason = if has_tool_use {
        "tool_use"
    } else if capped {
        "max_tokens"
    } else if matched_stop.is_some() {
        "stop_sequence"
    } else {
        "end_turn"
    };
    json!({
        "id": message_id,
        "type": "message",
        "role": "assistant",
        "model": prepared.request.public_model.id,
        "content": blocks,
        "stop_reason": stop_reason,
        "stop_sequence": matched_stop.map(Value::String).unwrap_or(Value::Null),
        "usage": messages_usage(&result.usage, result.effective_service_tier.as_deref()),
    })
}

// ---------- streaming (Responses turn updates → Messages SSE) ----------

/// The kind of the not-yet-closed Messages content block in the SSE translation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OpenBlockKind {
    Text,
    Thinking,
    ToolUse,
}

/// Incremental Messages SSE block state machine. Buffers nothing: every turn update becomes
/// ready-made frames immediately (dense block indices; a new block kind closes the previous
/// one, so interleaving text/reasoning/tool calls stays a valid Messages stream).
struct BlockEmitter {
    open: Option<(OpenBlockKind, u64)>,
    next_index: u64,
}

impl BlockEmitter {
    fn new() -> Self {
        Self {
            open: None,
            next_index: 0,
        }
    }

    fn close_open(&mut self) -> Vec<(String, Value)> {
        match self.open.take() {
            Some((_, index)) => vec![(
                "content_block_stop".to_string(),
                json!({"type": "content_block_stop", "index": index}),
            )],
            None => Vec::new(),
        }
    }

    fn ensure_open(&mut self, kind: OpenBlockKind, content_block: Value) -> Vec<(String, Value)> {
        if self.open.is_some_and(|(current, _)| current == kind) {
            return Vec::new();
        }
        let mut out = self.close_open();
        let index = self.next_index;
        self.next_index += 1;
        self.open = Some((kind, index));
        out.push((
            "content_block_start".to_string(),
            json!({"type": "content_block_start", "index": index, "content_block": content_block}),
        ));
        out
    }

    fn delta(&self, delta: Value) -> (String, Value) {
        let index = self.open.map(|(_, index)| index).unwrap_or(0);
        (
            "content_block_delta".to_string(),
            json!({"type": "content_block_delta", "index": index, "delta": delta}),
        )
    }

    fn text_delta(&mut self, text: &str) -> Vec<(String, Value)> {
        let mut out = self.ensure_open(OpenBlockKind::Text, json!({"type": "text", "text": ""}));
        out.push(self.delta(json!({"type": "text_delta", "text": text})));
        out
    }

    fn thinking_delta(&mut self, text: &str) -> Vec<(String, Value)> {
        let mut out = self.ensure_open(
            OpenBlockKind::Thinking,
            json!({"type": "thinking", "thinking": ""}),
        );
        out.push(self.delta(json!({"type": "thinking_delta", "thinking": text})));
        out
    }

    /// A complete function_call item arrives as one RawItem: the whole block lifecycle
    /// (start → arguments delta → stop) is emitted immediately.
    fn function_call(
        &mut self,
        call_id: &str,
        name: &str,
        arguments: &str,
    ) -> Vec<(String, Value)> {
        let mut out = self.ensure_open(
            OpenBlockKind::ToolUse,
            json!({"type": "tool_use", "id": call_id, "name": name, "input": {}}),
        );
        out.push(self.delta(json!({"type": "input_json_delta", "partial_json": arguments})));
        out.extend(self.close_open());
        out
    }
}

/// Messages SSE frame: `event:` + `data:` (typed events, as in the native lane).
async fn send_skin_frame(sender: &mpsc::Sender<Bytes>, event: &str, value: Value) -> bool {
    let data = serde_json::to_string(&value).unwrap_or_else(|_| {
        r#"{"type":"error","error":{"type":"api_error","message":"serialization failed"}}"#
            .to_string()
    });
    send_chat_bytes(
        sender,
        Bytes::from(format!("event: {event}\ndata: {data}\n\n")),
    )
    .await
}

/// Mid-stream failure: Anthropic-shaped `event: error` and the stream ends (the mirror of the
/// stage-4.1 `response.failed` rule).
async fn send_skin_error(sender: &mpsc::Sender<Bytes>) -> bool {
    send_skin_frame(
        sender,
        "error",
        json!({
            "type": "error",
            "error": {"type": "api_error", "message": "The model stream terminated unexpectedly."}
        }),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn stream_messages(
    gateway: Arc<CodexGateway>,
    prepared: PreparedTurn,
    admission: super::billing::CodexAdmission,
    message_id: String,
    stop: Vec<String>,
    max_output_chars: Option<usize>,
    routing: Option<super::TurnRouting>,
) -> Response {
    let task_permit = match gateway.track_background_task() {
        Ok(permit) => permit,
        Err(error) => return anthropic_error(ApiError::from(error)),
    };
    let (frame_tx, frame_rx) = mpsc::channel::<Bytes>(128);
    let request_id_header = message_id.clone();
    tokio::spawn(async move {
        let _task_permit = task_permit;
        // Rebind after the permit so early returns drop billing admission before the shutdown permit.
        let admission = admission;
        // message_start opens the stream. Usage is zeroed: authoritative usage exists only at
        // turn end (reported in message_delta) — a documented limitation of this adapter.
        if !send_skin_frame(
            &frame_tx,
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": message_id,
                    "type": "message",
                    "role": "assistant",
                    "model": prepared.request.public_model.id,
                    "content": [],
                    "stop_reason": Value::Null,
                    "stop_sequence": Value::Null,
                    "usage": {
                        "input_tokens": 0,
                        "output_tokens": 0,
                        "service_tier": prepared
                            .request
                            .service_tier
                            .as_deref()
                            .unwrap_or("standard")
                    }
                }
            }),
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
        let mut emitter = BlockEmitter::new();
        let mut emitted_tools = HashSet::new();
        let mut downstream_closed = false;
        let mut stop_filter = StopFilter::new(stop);
        let mut emitted_chars = 0usize;
        let mut length_capped = false;
        let mut heartbeat = tokio::time::interval(super::api::SSE_HEARTBEAT_INTERVAL);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        heartbeat.tick().await;

        // Enforces client stop sequences and the approximate output budget on text leaving for
        // the client (identical policy to `stream_chat`). Returns the delta safe to emit.
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
                    if !send_chat_bytes(&frame_tx, Bytes::from_static(b"event: ping\ndata: {}\n\n")).await {
                        downstream_closed = true;
                        break;
                    }
                    continue;
                }
                update = update_rx.recv() => {
                    let Some(update) = update else { break };
                    let frames = match update {
                        TurnUpdate::TextDelta { delta, .. } => {
                            let shaped = shape_text(
                                delta,
                                &mut stop_filter,
                                &mut emitted_chars,
                                &mut length_capped,
                            );
                            if shaped.is_empty() {
                                Vec::new()
                            } else {
                                emitter.text_delta(&shaped)
                            }
                        }
                        TurnUpdate::ReasoningSummaryPartAdded { summary_index, .. } => {
                            if summary_index == 0 {
                                // A new reasoning item opens a thinking block (closing any
                                // current one). No delta yet.
                                emitter.ensure_open(
                                    OpenBlockKind::Thinking,
                                    json!({"type": "thinking", "thinking": ""}),
                                )
                            } else {
                                // An extra summary part of the open item continues the same
                                // block; parts are joined with "\n\n" (mirror of the
                                // non-stream reasoning join).
                                if emitter.open.is_some_and(|(kind, _)| kind == OpenBlockKind::Thinking) {
                                    emitter.thinking_delta("\n\n")
                                } else {
                                    Vec::new()
                                }
                            }
                        }
                        TurnUpdate::ReasoningSummaryDelta { delta, .. } => {
                            if delta.is_empty() {
                                Vec::new()
                            } else {
                                emitter.thinking_delta(&delta)
                            }
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
                                Vec::new()
                            } else {
                                emitter.function_call(
                                    &key,
                                    item.get("name").and_then(Value::as_str).unwrap_or("unknown"),
                                    item.get("arguments").and_then(Value::as_str).unwrap_or("{}"),
                                )
                            }
                        }
                        TurnUpdate::RawItem(_) => Vec::new(),
                    };
                    let mut failed = false;
                    for (event, data) in frames {
                        if !send_skin_frame(&frame_tx, &event, data).await {
                            downstream_closed = true;
                            failed = true;
                            break;
                        }
                    }
                    if failed {
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
                    if !shaped.is_empty() {
                        for (event, data) in emitter.text_delta(&shaped) {
                            if !send_skin_frame(&frame_tx, &event, data).await {
                                downstream_closed = true;
                                break;
                            }
                        }
                    }
                }
            }
        }

        if downstream_closed {
            // Stop emitting public deltas but keep the already-started provider turn alive
            // until its authoritative usage arrives (the same settlement drain as `chat.rs`).
            drop(update_rx);
        }

        let result = match run.await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => {
                elog::error(
                    "codex",
                    format!(
                        "Codex messages skin stream failed [{}]",
                        error.diagnostic_class()
                    ),
                );
                let _ = send_skin_error(&frame_tx).await;
                return;
            }
            Err(_) => {
                elog::error("codex", "Codex messages skin stream task failed [join]");
                let _ = send_skin_error(&frame_tx).await;
                return;
            }
        };
        let has_tool_use = content_blocks(&result.output).2;
        let stop_triggered = stop_filter.as_ref().is_some_and(StopFilter::triggered);
        let stop_reason = if has_tool_use {
            "tool_use"
        } else if length_capped {
            "max_tokens"
        } else if stop_triggered {
            "stop_sequence"
        } else {
            "end_turn"
        };
        admission.settle(
            &prepared.request.public_model,
            &result.usage,
            prepared.request.max_output_tokens,
            result.effective_service_tier.as_deref() == Some("priority"),
        );
        if downstream_closed {
            return;
        }
        // Close the last open block, then the terminal delta/stop pair.
        for (event, data) in emitter.close_open() {
            if !send_skin_frame(&frame_tx, &event, data).await {
                return;
            }
        }
        if !send_skin_frame(
            &frame_tx,
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": stop_reason, "stop_sequence": Value::Null},
                "usage": messages_usage(
                    &result.usage,
                    result.effective_service_tier.as_deref()
                )
            }),
        )
        .await
        {
            return;
        }
        let _ = send_skin_frame(&frame_tx, "message_stop", json!({"type": "message_stop"})).await;
    });

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .header("request-id", request_id_header)
        .body(Body::from_stream(ChatReceiverStream::new(frame_rx)))
        .unwrap_or_else(|_| {
            // Spawn-таск с admission уже запущен и может settle'ить фактику (charge) —
            // условия not_started не выполнены, заголовок из skin_error снимаем.
            without_not_started(skin_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                "Internal server error",
                "internal_response_error",
                None,
            ))
        })
}

// ---------- handlers ----------

/// Buffered JSON request body under the OpenAI-plane limit; failures are Anthropic-envelope 400s.
async fn read_messages_body(body: Body) -> Result<Value, Response> {
    let raw = match to_bytes(body, OPENAI_BODY_LIMIT).await {
        Ok(raw) => raw,
        Err(_) => {
            return Err(skin_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "Request body exceeds the 8 MiB limit.",
                "invalid_request_body",
                None,
            ))
        }
    };
    match serde_json::from_slice(&raw) {
        Ok(value) => Ok(value),
        Err(_) => Err(skin_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "Invalid JSON in request body.",
            "invalid_request_body",
            None,
        )),
    }
}

/// Handler `POST /v1/messages` on the OpenAI plane (registered only in `ProviderMode::OpenAi`).
/// The flow mirrors `chat::completions` exactly: admission → parse → prepare → reserve →
/// run → settle; only the wire envelopes differ (Messages in, Messages out).
pub async fn messages(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    let Some(gateway) = app.codex.as_ref().cloned() else {
        return skin_error(
            StatusCode::NOT_FOUND,
            "not_found_error",
            "The requested endpoint is not enabled.",
            "unsupported_endpoint",
            None,
        );
    };
    let (parts, body) = request.into_parts();
    let pending = match begin_admission(&app, &parts.headers, &peer).await {
        Ok(pending) => pending,
        Err(error) => {
            if matches!(error, crate::codex::billing::AdmissionError::Unavailable) {
                elog::warn("codex", "codex admission unavailable for messages");
            }
            return anthropic_error(ApiError::from(error));
        }
    };
    let value = match read_messages_body(body).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let translated = match translate_messages_request(value, true) {
        Ok(translated) => translated,
        Err(response) => return response,
    };
    let parsed = match parse_responses_request(&gateway, translated.responses) {
        Ok(parsed) => parsed,
        Err(error) => return anthropic_error(error),
    };
    let tenant_scope = pending.tenant_scope().to_string();
    let mut prepared = match prepare_turn(&gateway, &tenant_scope, parsed).await {
        Ok(prepared) => prepared,
        Err(error) => return anthropic_error(error),
    };
    // Top-level `system` is the request-owned base instruction: it replaces Codex's base
    // prompt via `instructions` (already wired through prepare_turn); there is no separate
    // developer channel on the Messages surface.
    prepared.turn.base_instructions = prepared.request.instructions.clone();
    let routing =
        super::api::build_turn_routing(&app, &tenant_scope, &parts.headers, &prepared).await;
    let admission = match pending
        .reserve(
            &app,
            &prepared.request.public_model,
            prepared.estimated_input_tokens,
            prepared.request.max_output_tokens,
            gateway.config().reserve_overhead_tokens,
            prepared.request.service_tier.is_some(),
        )
        .await
    {
        Ok(admission) => admission,
        Err(error) => return anthropic_error(ApiError::from(error)),
    };
    let message_id = new_id("msg");

    if prepared.request.stream {
        // Reject before opening the SSE stream if the whole pool is genuinely unavailable, so the client
        // sees a retryable error instead of a 200 that fails mid-stream.
        if let Err(error) = gateway
            .preflight_capacity(&prepared.request.public_model)
            .await
        {
            return anthropic_error(ApiError::from(error));
        }
        if let Err(error) = admission.mark_delivering().await {
            elog::error("codex", "codex delivery marker failed");
            return anthropic_error(ApiError::from(error));
        }
        return stream_messages(
            gateway,
            prepared,
            admission,
            message_id,
            translated.stop,
            translated.max_output_chars,
            routing,
        )
        .await;
    }

    let result = match gateway.run_turn(prepared.turn.clone(), None, routing).await {
        Ok(result) => result,
        Err(error) => return anthropic_error(ApiError::from(error)),
    };
    if let Err(error) = admission.mark_delivering().await {
        elog::error("codex", "codex delivery marker failed");
        return anthropic_error(ApiError::from(error));
    }
    let response = completed_message(
        &prepared,
        &result,
        &message_id,
        &translated.stop,
        translated.max_output_chars,
    );
    admission.settle(
        &prepared.request.public_model,
        &result.usage,
        prepared.request.max_output_tokens,
        result.effective_service_tier.as_deref() == Some("priority"),
    );
    skin_json_response(response, &message_id)
}

/// Handler `POST /v1/messages/count_tokens` on the OpenAI plane: the same Messages parse plus
/// the shared `parse_responses_request`/`prepare_turn` pipeline yields the reserve-grade input
/// estimate (the `openai_input_tokens` logic) without any network self-call.
pub async fn count_tokens(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    let Some(gateway) = app.codex.as_ref().cloned() else {
        return skin_error(
            StatusCode::NOT_FOUND,
            "not_found_error",
            "The requested endpoint is not enabled.",
            "unsupported_endpoint",
            None,
        );
    };
    let (parts, body) = request.into_parts();
    let pending = match begin_admission(&app, &parts.headers, &peer).await {
        Ok(pending) => pending,
        Err(error) => return anthropic_error(ApiError::from(error)),
    };
    let value = match read_messages_body(body).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let translated = match translate_messages_request(value, false) {
        Ok(translated) => translated,
        Err(response) => return response,
    };
    let parsed = match parse_responses_request(&gateway, translated.responses) {
        Ok(parsed) => parsed,
        Err(error) => return anthropic_error(error),
    };
    let prepared = match prepare_turn(&gateway, pending.tenant_scope(), parsed).await {
        Ok(prepared) => prepared,
        Err(error) => return anthropic_error(error),
    };
    skin_json_response(
        json!({"input_tokens": prepared.estimated_input_tokens}),
        &new_id("req"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::ProcessError;

    fn ok_translated(value: Value) -> ParsedSkin {
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
    async fn skin_errors_mark_the_execution_not_started() {
        // Отказы Anthropic-конверта skin — до границы доставки (reserve возвращает дроп
        // admission → HoldGuard, ни байта клиенту не ушло): каждый несёт not_started.
        // 200-ответ skin_json_response — не несёт никогда.
        for response in [
            skin_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "overloaded_error",
                "Overloaded",
                "test_reason",
                Some(2),
            ),
            invalid_request("bad request"),
            anthropic_error(ApiError::from(
                crate::codex::billing::AdmissionError::LowBalance,
            )),
            anthropic_error(ApiError::from(
                crate::codex::billing::AdmissionError::Unauthorized,
            )),
            translate_messages_request(json!({"model": "gpt-5.6"}), true).unwrap_err(),
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
        let ok = skin_json_response(json!({"id": "msg_1"}), "msg_1");
        assert!(ok
            .headers()
            .get(crate::proxy::EXECUTION_STATE_HEADER)
            .is_none());

        let external = anthropic_error(ApiError::from(ProcessError::ExternalFallbackFailed {
            local: Box::new(ProcessError::UsageLimitExceeded {
                retry_after: Some(42),
            }),
        }));
        assert_eq!(external.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(external.headers().get(header::RETRY_AFTER).unwrap(), "42");
        assert!(external
            .headers()
            .get(crate::proxy::EXECUTION_STATE_HEADER)
            .is_none());
    }

    // ---------- request translation ----------

    #[test]
    fn translates_basic_messages_to_responses() {
        let parsed = ok_translated(json!({
            "model": "openai/gpt-5.6",
            "system": "Be terse.",
            "max_tokens": 256,
            "messages": [{"role": "user", "content": "Hello"}]
        }));
        let body = &parsed.responses;
        assert_eq!(body["model"], "gpt-5.6");
        assert_eq!(body["instructions"], "Be terse.");
        assert_eq!(body["max_output_tokens"], 256);
        assert_eq!(body["store"], false);
        assert!(body.get("stream").is_none());
        assert_eq!(parsed.max_output_chars, Some(1024));
        assert_eq!(
            body["input"],
            json!([{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hello"}]}])
        );
    }

    #[tokio::test]
    async fn messages_stream_requires_a_boolean() {
        let (status, body) = expect_err(json!({
            "model": "gpt-5.6",
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

        let parsed = ok_translated(json!({
            "model": "gpt-5.6",
            "max_tokens": 256,
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": null
        }));
        assert!(parsed.responses.get("stream").is_none());
    }

    #[test]
    fn messages_fast_aliases_translate_to_priority_service_tier() {
        for (field, value) in [
            ("speed", "fast"),
            ("service_tier", "fast"),
            ("service_tier", "priority"),
        ] {
            let mut body = json!({
                "model": "openai/gpt-5.6",
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "Hello"}]
            });
            body[field] = json!(value);
            let translated = ok_translated(body);
            assert_eq!(
                translated.responses["service_tier"], "priority",
                "{field}={value}"
            );
        }

        let standard = ok_translated(json!({
            "model": "openai/gpt-5.6",
            "max_tokens": 16,
            "messages": [{"role": "user", "content": "Hello"}],
            "speed": "standard",
            "service_tier": "default"
        }));
        assert!(standard.responses.get("service_tier").is_none());
    }

    #[test]
    fn system_blocks_join() {
        let parsed = ok_translated(json!({
            "model": "gpt-5.6",
            "system": [
                {"type": "text", "text": "one"},
                {"type": "text", "text": "two"}
            ],
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert_eq!(parsed.responses["instructions"], "one\n\ntwo");

        // Строковый system пустым не считается.
        let parsed = ok_translated(json!({
            "model": "gpt-5.6", "system": "", "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}]
        }));
        assert!(parsed.responses.get("instructions").is_none());
    }

    #[test]
    fn claude_code_ephemeral_cache_controls_are_accepted_and_removed() {
        for body in [
            json!({"model": "gpt-5.6", "max_tokens": 1,
                "system": [
                    {"type": "text", "text": "a"},
                    {"type": "text", "text": "b", "cache_control": {"type": "ephemeral"}},
                    {"type": "text", "text": "c", "cache_control": {"type": "ephemeral"}}
                ],
                "messages": [{"role": "user", "content": "hi"}]}),
            json!({"model": "gpt-5.6", "max_tokens": 1, "messages": [{"role": "user", "content": [
                {"type": "text", "text": "hi", "cache_control": {"type": "ephemeral"}}]}]}),
            json!({"model": "gpt-5.6", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
                "tools": [{"name": "f", "cache_control": {"type": "ephemeral"}}]}),
        ] {
            let parsed = ok_translated(body);
            assert!(!parsed.responses.to_string().contains("cache_control"));
        }
    }

    #[tokio::test]
    async fn cache_control_extended_shapes_stay_fail_closed_everywhere() {
        for body in [
            json!({"model": "gpt-5.6", "max_tokens": 1,
                "system": [{"type": "text", "text": "x",
                    "cache_control": {"type": "ephemeral", "ttl": "1h"}}],
                "messages": [{"role": "user", "content": "hi"}]}),
            json!({"model": "gpt-5.6", "max_tokens": 1, "messages": [{"role": "user", "content": [
                {"type": "text", "text": "hi", "cache_control": {"type": "persistent"}}]}]}),
            json!({"model": "gpt-5.6", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
                "tools": [{"name": "f", "cache_control": "ephemeral"}]}),
        ] {
            let (status, json) = expect_err(body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
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
    fn user_images_translate_to_input_image_parts() {
        let parsed = ok_translated(json!({
            "model": "gpt-5.6",
            "max_tokens": 1,
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "What is this?"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="}},
                {"type": "image", "source": {"type": "url", "url": "https://example.com/cat.jpg"}},
                {"type": "text", "text": "And this?"}
            ]}]
        }));
        assert_eq!(
            parsed.responses["input"],
            json!([{"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "What is this?"},
                {"type": "input_image", "image_url": "data:image/png;base64,iVBORw0KGgo="},
                {"type": "input_image", "image_url": "https://example.com/cat.jpg"},
                {"type": "input_text", "text": "And this?"}
            ]}])
        );
    }

    #[tokio::test]
    async fn rejects_invalid_image_sources() {
        for source in [
            json!({"type": "base64", "media_type": "image/png"}),
            json!({"type": "url", "url": "file:///etc/passwd"}),
            json!({"type": "s3", "location": "x"}),
        ] {
            let (status, json) = expect_err(json!({
                "model": "gpt-5.6", "max_tokens": 1,
                "messages": [{"role": "user", "content": [
                    {"type": "image", "source": source}]}],
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
            assert_eq!(json["error"]["type"], "invalid_request_error", "{json}");
        }
    }

    /// Зеркало 4.2 replay: assistant tool_use + user tool_result → те же item forms, которые
    /// `anthropic_responses.rs` производит из Messages (function_call с arguments-строкой,
    /// function_call_output).
    #[test]
    fn tool_history_replays_as_function_items() {
        let parsed = ok_translated(json!({
            "model": "gpt-5.6",
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
        let input = parsed.responses["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[1]["role"], "assistant");
        assert_eq!(
            input[1]["content"][0],
            json!({"type": "output_text", "text": "Let me check."})
        );
        assert_eq!(
            input[2],
            json!({"type": "function_call", "call_id": "toolu_1", "name": "weather",
                "arguments": "{\"city\":\"Paris\"}", "status": "completed"})
        );
        assert_eq!(
            input[3],
            json!({"type": "function_call_output", "call_id": "toolu_1", "output": "sunny"})
        );
        // Текст после tool_result остаётся отдельным user message item (порядок блоков).
        assert_eq!(input[4]["role"], "user");
        assert_eq!(
            input[4]["content"][0],
            json!({"type": "input_text", "text": "thanks"})
        );
    }

    #[test]
    fn tool_result_array_content_joins_with_newlines() {
        let parsed = ok_translated(json!({
            "model": "gpt-5.6", "max_tokens": 1,
            "messages": [{"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "is_error": true, "content": [
                    {"type": "text", "text": "line one"},
                    {"type": "text", "text": "line two"}
                ]}
            ]}]
        }));
        // is_error принимается и игнорируется; text-партc склеиваются через \n.
        assert_eq!(
            parsed.responses["input"][0],
            json!({"type": "function_call_output", "call_id": "t1", "output": "line one\nline two"})
        );
    }

    #[tokio::test]
    async fn duplicate_tool_use_id_and_bad_tool_result_are_400() {
        let (status, _) = expect_err(json!({
            "model": "gpt-5.6", "max_tokens": 1,
            "messages": [
                {"role": "assistant", "content": [{"type": "tool_use", "id": "t", "name": "a", "input": {}}]},
                {"role": "assistant", "content": [{"type": "tool_use", "id": "t", "name": "b", "input": {}}]}
            ]
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Нетекстовый контент tool_result — 400 (зеркало function_call_output 4.2).
        let (status, json) = expect_err(json!({
            "model": "gpt-5.6", "max_tokens": 1,
            "messages": [{"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t", "content": [{"type": "image", "source": {}}]}]}],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("tool_result"));
    }

    #[test]
    fn thinking_and_redacted_thinking_input_blocks_are_dropped() {
        let parsed = ok_translated(json!({
            "model": "gpt-5.6", "max_tokens": 1,
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "secret", "signature": "sig"},
                    {"type": "redacted_thinking", "data": "opaque"},
                    {"type": "text", "text": "answer"}
                ]},
                {"role": "user", "content": "next"}
            ]
        }));
        let input = parsed.responses["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(
            input[0],
            json!({"type": "message", "role": "assistant",
                "content": [{"type": "output_text", "text": "answer"}]})
        );
    }

    #[test]
    fn tools_translate_to_responses_function_tools() {
        let parsed = ok_translated(json!({
            "model": "gpt-5.6", "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [
                {"name": "get_weather", "description": "Current weather",
                 "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}}},
                {"type": "custom", "name": "no_args"}
            ]
        }));
        assert_eq!(
            parsed.responses["tools"],
            json!([
                {"type": "function", "name": "get_weather", "description": "Current weather",
                 "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}},
                {"type": "function", "name": "no_args",
                 "parameters": {"type": "object", "properties": {}}}
            ])
        );
    }

    #[tokio::test]
    async fn server_tools_are_400() {
        let (status, json) = expect_err(json!({
            "model": "gpt-5.6", "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "web_search_20250305", "name": "web_search"}],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert!(json["error"]["message"].as_str().unwrap().contains("tools"));
    }

    #[test]
    fn tool_choice_variants_map_to_responses() {
        // auto — дефолт, в тело не вставляется.
        let parsed = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": {"type": "auto"}
        }));
        assert!(parsed.responses.get("tool_choice").is_none());

        let parsed = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": {"type": "any"}
        }));
        assert_eq!(parsed.responses["tool_choice"], "required");

        let parsed = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": {"type": "none"}
        }));
        assert_eq!(parsed.responses["tool_choice"], "none");

        let parsed = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": {"type": "tool", "name": "f"}
        }));
        assert_eq!(
            parsed.responses["tool_choice"],
            json!({"type": "function", "name": "f"})
        );

        // disable_parallel_tool_use → parallel_tool_calls:false (зеркало 4.1).
        let parsed = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": {"type": "auto", "disable_parallel_tool_use": true}
        }));
        assert_eq!(parsed.responses["parallel_tool_calls"], false);
    }

    #[test]
    fn thinking_maps_to_reasoning_effort() {
        let effort = |thinking: Value| {
            ok_translated(json!({
                "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
                "thinking": thinking
            }))
            .responses
            .get("reasoning")
            .cloned()
        };
        assert_eq!(effort(json!({"type": "disabled"})), None);
        assert_eq!(
            effort(json!({"type": "adaptive", "display": "summarized"})),
            None
        );
        assert_eq!(
            effort(json!({"type": "enabled", "budget_tokens": 1024})),
            Some(json!({"effort": "low"}))
        );
        assert_eq!(
            effort(json!({"type": "enabled", "budget_tokens": 8000})),
            Some(json!({"effort": "medium"}))
        );
        assert_eq!(
            effort(json!({"type": "enabled", "budget_tokens": 32000})),
            Some(json!({"effort": "high"}))
        );
    }

    #[tokio::test]
    async fn invalid_thinking_is_400() {
        for thinking in [
            json!({"type": "enabled"}),
            json!({"type": "enabled", "budget_tokens": 512}),
            json!({"type": "sometimes"}),
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
    async fn capability_matrix_rejects_unsupported_values() {
        for (param, value) in [
            (
                "context_management",
                json!({"edits": [{"type": "clear_thinking_20251015", "keep": "none"}]}),
            ),
            ("mcp_servers", json!([{"name": "srv"}])),
            ("container", json!({"id": "c"})),
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
        for context_management in [
            json!(null),
            json!({"edits": []}),
            json!({"edits": [{"type": "clear_thinking_20251015", "keep": "all"}]}),
        ] {
            let parsed = ok_translated(json!({
                "model": "m", "max_tokens": 1,
                "messages": [{"role": "user", "content": "hi"}],
                "context_management": context_management,
                "mcp_servers": []
            }));
            assert!(parsed.responses.get("context_management").is_none());
        }
    }

    #[test]
    fn output_config_maps_claude_code_effort_and_json_schema() {
        let schema = json!({
            "type": "object",
            "properties": {"title": {"type": "string"}},
            "required": ["title"],
            "additionalProperties": false
        });
        let parsed = ok_translated(json!({
            "model": "m", "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}],
            "thinking": {"type": "adaptive", "display": "omitted"},
            "output_config": {
                "effort": "high",
                "format": {"type": "json_schema", "schema": schema}
            }
        }));
        assert_eq!(parsed.responses["reasoning"], json!({"effort": "high"}));
        assert_eq!(
            parsed.responses["text"],
            json!({"format": {"type": "json_schema", "schema": schema}})
        );
    }

    #[tokio::test]
    async fn output_config_unknown_or_unrepresentable_shapes_stay_fail_closed() {
        for value in [
            json!("high"),
            json!({"effort": "max"}),
            json!({"effort": 3}),
            json!({"format": {"type": "json_schema"}}),
            json!({"format": {"type": "json_schema", "schema": []}}),
            json!({"format": {"type": "json_schema", "schema": {}, "future": true}}),
            json!({"verbosity": "high"}),
        ] {
            let (status, json) = expect_err(json!({
                "model": "m", "max_tokens": 1,
                "messages": [{"role": "user", "content": "hi"}],
                "output_config": value
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
            assert_eq!(json["error"]["type"], "invalid_request_error", "{json}");
            assert!(
                json["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("output_config"),
                "{json}"
            );
        }
    }

    #[tokio::test]
    async fn context_management_near_misses_stay_fail_closed() {
        for value in [
            json!({"edits": [{"type": "clear_thinking_20251015", "keep": "all", "future": true}]}),
            json!({"edits": [{"type": "clear_tool_uses_20250919", "keep": "all"}]}),
            json!({"edits": [
                {"type": "clear_thinking_20251015", "keep": "all"},
                {"type": "clear_thinking_20251015", "keep": "all"}
            ]}),
            json!({"edits": [], "future": true}),
            json!([]),
        ] {
            let (status, json) = expect_err(json!({
                "model": "m", "max_tokens": 1,
                "messages": [{"role": "user", "content": "hi"}],
                "context_management": value
            }))
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
            assert_eq!(json["error"]["type"], "invalid_request_error", "{json}");
            assert!(
                json["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("context_management"),
                "{json}"
            );
        }
    }

    #[test]
    fn metadata_and_sampling_controls_are_accepted_and_ignored() {
        let parsed = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "metadata": {"user_id": "user_123"},
            "temperature": 0.2, "top_p": 0.5, "top_k": 40,
            "some_future_field": {"anything": true}
        }));
        // В Responses-значение они не попадают (транспорт их не умеет — как в chat.rs).
        assert!(parsed.responses.get("metadata").is_none());
        assert!(parsed.responses.get("temperature").is_none());
        assert!(parsed.responses.get("some_future_field").is_none());
    }

    #[test]
    fn stop_sequences_parse_and_empty_ones_drop() {
        let parsed = ok_translated(json!({
            "model": "m", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}],
            "stop_sequences": ["\n\n", "END", ""]
        }));
        assert_eq!(parsed.stop, vec!["\n\n".to_string(), "END".to_string()]);
    }

    #[tokio::test]
    async fn missing_required_fields_are_anthropic_shaped_400() {
        for body in [
            json!({"max_tokens": 1, "messages": [{"role": "user", "content": "hi"}]}),
            json!({"model": "m", "messages": [{"role": "user", "content": "hi"}]}),
            json!({"model": "m", "max_tokens": 1}),
            json!({"model": "m", "max_tokens": 1, "messages": []}),
            json!({"model": "openai/", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}]}),
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

    // ---------- response translation ----------

    fn result_with(output: Vec<Value>, usage: CodexUsage) -> CodexTurnResult {
        CodexTurnResult {
            output,
            usage,
            effective_service_tier: None,
            provider_reported_service_tier: None,
        }
    }

    /// Зеркало словаря 4.1: message item → text-блок, function_call → tool_use (arguments
    /// парсятся в input), reasoning → thinking без signature, usage → Messages usage.
    #[test]
    fn content_blocks_mirror_the_responses_dictionary() {
        let result = result_with(
            vec![
                json!({"type": "reasoning", "id": "rs_1", "summary": [
                    {"type": "summary_text", "text": "first"},
                    {"type": "summary_text", "text": "second"}
                ]}),
                json!({"type": "message", "role": "assistant", "id": "msg_1", "content": [
                    {"type": "output_text", "text": "answer"}
                ]}),
                json!({"type": "function_call", "id": "fc_1", "call_id": "call_1",
                    "name": "weather", "arguments": "{\"city\":\"Paris\"}", "status": "completed"}),
            ],
            CodexUsage {
                input_tokens: 100,
                cached_input_tokens: 40,
                cache_write_input_tokens: 10,
                output_tokens: 20,
                reasoning_output_tokens: 5,
                total_tokens: 120,
            },
        );
        let (blocks, text, has_tool_use, text_at) = content_blocks(&result.output);
        assert_eq!(text, "answer");
        assert!(has_tool_use);
        // Первый message item шёл после reasoning → text-блок встаёт на позицию 1.
        assert_eq!(text_at, Some(1));
        assert_eq!(
            blocks,
            vec![
                json!({"type": "thinking", "thinking": "first\n\nsecond"}),
                json!({"type": "tool_use", "id": "call_1", "name": "weather",
                 "input": {"city": "Paris"}}),
            ]
        );
        // thinking-блок без signature (решение 6).
        assert!(blocks[0].get("signature").is_none());

        let usage = messages_usage(&result.usage, Some("priority"));
        assert_eq!(usage["input_tokens"], 100);
        assert_eq!(usage["output_tokens"], 20);
        assert_eq!(usage["cache_creation_input_tokens"], 10);
        assert_eq!(usage["cache_read_input_tokens"], 40);
        assert_eq!(usage["output_tokens_details"]["thinking_tokens"], 5);
        assert_eq!(usage["service_tier"], "priority");
    }

    #[test]
    fn function_call_with_malformed_arguments_degrades_to_empty_input() {
        let result = result_with(
            vec![json!({"type": "function_call", "call_id": "c", "name": "f",
                "arguments": "not json"})],
            CodexUsage::default(),
        );
        let (blocks, _, _, _) = content_blocks(&result.output);
        assert_eq!(blocks[0]["input"], json!({}));
    }

    #[test]
    fn messages_usage_omits_zero_cache_and_reasoning() {
        let usage = messages_usage(
            &CodexUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                ..CodexUsage::default()
            },
            None,
        );
        assert_eq!(
            usage,
            json!({"input_tokens": 10, "output_tokens": 5, "service_tier": "standard"})
        );
    }

    // ---------- SSE block emitter ----------

    fn frame_names(frames: &[(String, Value)]) -> Vec<&str> {
        frames.iter().map(|(event, _)| event.as_str()).collect()
    }

    #[test]
    fn emitter_produces_dense_block_lifecycle() {
        let mut emitter = BlockEmitter::new();
        let mut all: Vec<(String, Value)> = Vec::new();
        all.extend(emitter.text_delta("hello"));
        all.extend(emitter.text_delta(" world"));
        all.extend(emitter.function_call("call_1", "weather", "{\"city\":\"Paris\"}"));
        all.extend(emitter.text_delta("done"));
        all.extend(emitter.close_open());

        assert_eq!(
            frame_names(&all),
            [
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
            ]
        );
        // Плотные индексы: text=0, tool_use=1, text=2; смена типа блока сначала закрывает
        // предыдущий (stop перед start), function_call приходит целиком → start/delta/stop сразу.
        assert_eq!(all[0].1["index"], 0);
        assert_eq!(
            all[0].1["content_block"],
            json!({"type": "text", "text": ""})
        );
        assert_eq!(
            all[1].1["delta"],
            json!({"type": "text_delta", "text": "hello"})
        );
        assert_eq!(all[2].1["index"], 0);
        assert_eq!(
            all[3],
            (
                "content_block_stop".to_string(),
                json!({"type": "content_block_stop", "index": 0})
            )
        );
        assert_eq!(all[4].1["index"], 1);
        assert_eq!(
            all[4].1["content_block"],
            json!({"type": "tool_use", "id": "call_1", "name": "weather", "input": {}})
        );
        assert_eq!(
            all[5].1["delta"],
            json!({"type": "input_json_delta", "partial_json": "{\"city\":\"Paris\"}"})
        );
        assert_eq!(
            all[6],
            (
                "content_block_stop".to_string(),
                json!({"type": "content_block_stop", "index": 1})
            )
        );
        assert_eq!(all[7].1["index"], 2);
        assert_eq!(
            all[9],
            (
                "content_block_stop".to_string(),
                json!({"type": "content_block_stop", "index": 2})
            )
        );
    }

    #[test]
    fn emitter_interleaves_thinking_and_text_blocks() {
        let mut emitter = BlockEmitter::new();
        let mut all: Vec<(String, Value)> = Vec::new();
        // Новый reasoning item открывает thinking-блок (через PartAdded summary_index 0).
        all.extend(emitter.ensure_open(
            OpenBlockKind::Thinking,
            json!({"type": "thinking", "thinking": ""}),
        ));
        all.extend(emitter.thinking_delta("plan"));
        all.extend(emitter.text_delta("answer"));
        all.extend(emitter.close_open());

        assert_eq!(
            frame_names(&all),
            [
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
                "content_block_start",
                "content_block_delta",
                "content_block_stop"
            ]
        );
        assert_eq!(
            all[1].1["delta"],
            json!({"type": "thinking_delta", "thinking": "plan"})
        );
        // Открытие text-блока сначала закрывает thinking-блок (stop index 0).
        assert_eq!(
            all[2],
            (
                "content_block_stop".to_string(),
                json!({"type": "content_block_stop", "index": 0})
            )
        );
        assert_eq!(all[3].1["index"], 1);
        assert_eq!(
            all[4].1["delta"],
            json!({"type": "text_delta", "text": "answer"})
        );
        assert_eq!(
            all[5],
            (
                "content_block_stop".to_string(),
                json!({"type": "content_block_stop", "index": 1})
            )
        );
    }

    #[test]
    fn stop_filter_integration_shapes_stream_text() {
        // Контракт StopFilter (общий с chat.rs): хвост длиной max(stop)-1 байт удерживается,
        // стоп-последовательность срабатывает на склейке с held-back хвостом и вырезается.
        let mut filter = StopFilter::new(vec!["END".to_string()]).unwrap();
        assert_eq!(filter.push("hello "), "hell");
        assert_eq!(filter.push("END world"), "o ");
        assert!(filter.triggered());
        assert_eq!(filter.finish(), "");
    }
}
