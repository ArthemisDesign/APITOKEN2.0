//! One ephemeral Codex thread per public API request.
//!
//! Request-local instructions and exact injected Responses items preserve OpenAI's stateless
//! semantics. A Codex thread is never reused as hidden conversational state.

use super::{
    new_id, AppServerEvent, CodexGateway, CodexHome, CodexModel, CodexProcess, HomeSelection,
    ProcessError, TurnRouting, TurnSlot,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CodexUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub total_tokens: u64,
}

impl CodexUsage {
    fn add_assign(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.cached_input_tokens = self
            .cached_input_tokens
            .saturating_add(other.cached_input_tokens);
        self.cache_write_input_tokens = self
            .cache_write_input_tokens
            .saturating_add(other.cache_write_input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.reasoning_output_tokens = self
            .reasoning_output_tokens
            .saturating_add(other.reasoning_output_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
    }

    fn from_value(value: &Value) -> Self {
        let get = |name: &str| value.get(name).and_then(Value::as_u64).unwrap_or(0);
        Self {
            input_tokens: get("inputTokens"),
            cached_input_tokens: get("cachedInputTokens"),
            cache_write_input_tokens: get("cacheWriteInputTokens"),
            output_tokens: get("outputTokens"),
            reasoning_output_tokens: get("reasoningOutputTokens"),
            total_tokens: get("totalTokens"),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CodexTurnRequest {
    pub model: CodexModel,
    /// Opaque tenant-scoped key forwarded to the patched app-server. This replaces the random
    /// ephemeral thread id that stock Codex otherwise uses for every upstream Responses request.
    pub prompt_cache_key: Option<String>,
    /// Replaces Codex's built-in base prompt. `None` means an intentionally empty base.
    pub base_instructions: Option<String>,
    /// Request-owned developer instructions. No Codex CLI instructions are inherited.
    pub developer_instructions: Option<String>,
    /// Exact prior Responses items to append before the new user input.
    pub injected_items: Vec<Value>,
    /// App-server `UserInput[]` for `turn/start`.
    pub turn_input: Vec<Value>,
    /// Canonical experimental dynamic-tool specs.
    pub dynamic_tools: Vec<Value>,
    /// App-server/OpenAI request value (`priority` for Codex Fast mode, otherwise absent).
    pub service_tier: Option<String>,
    pub reasoning_effort: Option<String>,
    pub reasoning_summary: Option<String>,
    pub output_schema: Option<Value>,
    pub verbosity: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum TurnUpdate {
    TextDelta {
        item_id: String,
        delta: String,
    },
    ReasoningSummaryPartAdded {
        item_id: String,
        summary_index: u64,
    },
    ReasoningSummaryDelta {
        item_id: String,
        summary_index: u64,
        delta: String,
    },
    RawItem(Value),
}

#[derive(Clone, Debug)]
pub(crate) struct CodexTurnResult {
    pub output: Vec<Value>,
    pub usage: CodexUsage,
}

impl CodexGateway {
    /// Run one turn on the best available home, rotating on account-fault errors.
    ///
    /// Rotation mirrors the Claude path's blame classification. A usage limit or a dead login is
    /// that home's fault, so the pool moves to another home without spending the transport budget;
    /// a dead child or an RPC timeout is a backend fault and is retried exactly once. Nothing is
    /// ever retried after the first byte has reached the client: the public stream must never
    /// replay or interleave two attempts.
    pub(crate) async fn run_turn(
        &self,
        request: CodexTurnRequest,
        updates: Option<mpsc::Sender<TurnUpdate>>,
        routing: Option<TurnRouting>,
    ) -> Result<CodexTurnResult, ProcessError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(ProcessError::Closed);
        }
        tokio::select! {
            result = self.run_active_turn(request, updates, routing) => result,
            _ = self.turn_abort_requested() => Err(ProcessError::Closed),
        }
    }

    async fn run_active_turn(
        &self,
        mut request: CodexTurnRequest,
        updates: Option<mpsc::Sender<TurnUpdate>>,
        mut routing: Option<TurnRouting>,
    ) -> Result<CodexTurnResult, ProcessError> {
        if request.prompt_cache_key.is_none() {
            request.prompt_cache_key = routing.as_ref().map(TurnRouting::prompt_cache_key);
        }
        let emitted = Arc::new(AtomicBool::new(false));
        let mut tried: Vec<String> = Vec::new();
        let mut transport_retries_left = 1usize;
        let mut last_error: Option<ProcessError> = None;
        loop {
            let preferred = routing
                .as_ref()
                .and_then(|routing| routing.preferred_home());
            let warm = routing
                .as_ref()
                .map(|routing| routing.warm.as_slice())
                .unwrap_or(&[]);
            let place_cache_root = routing.as_ref().is_some_and(TurnRouting::places_cache_root);
            let (home, slot) = match self
                .select_home(&tried, preferred, warm, place_cache_root, true)
                .await
            {
                HomeSelection::Ready(home, slot) => (home, slot),
                HomeSelection::Unavailable { ready_at } => {
                    return Err(
                        last_error.unwrap_or_else(|| ProcessError::UsageLimitExceeded {
                            retry_after: retry_after_from(ready_at),
                        }),
                    );
                }
            };
            tried.push(home.id().to_string());
            let result = home
                .run_turn(request.clone(), updates.clone(), &emitted, slot)
                .await;
            let error = match result {
                Ok(result) => {
                    home.record_spend(super::billing::price_real_nano(
                        &request.model,
                        &result.usage,
                        pool::now(),
                        request.service_tier.is_some(),
                    ))
                    .await;
                    home.mark_turn_healthy();
                    // Pin this conversation's cache lineage to the home that served it, so the next
                    // request in the conversation reuses its warm prompt cache.
                    if let Some(routing) = routing.as_mut() {
                        routing.record_served(home.id()).await;
                    }
                    return Ok(result);
                }
                Err(error) => error,
            };
            home.note_turn_error(&error);
            // The request that just failed is the freshest evidence the pool will ever get about
            // this home. Hand it to the control plane immediately instead of letting the background
            // sweep rediscover the problem a full cadence later — the Claude path does the same
            // with `request_probe` + `probe_poke`.
            self.poke_probe_if_requested().await;
            // A client fault is deterministic: another home would reject it identically.
            if matches!(
                error,
                ProcessError::BadRequest
                    | ProcessError::ContextWindowExceeded
                    | ProcessError::Rpc { .. }
            ) {
                return Err(error);
            }
            // Output already left for the client, so the attempt is no longer replaceable.
            if emitted.load(Ordering::Acquire) {
                return Err(error);
            }
            let account_fault = matches!(
                error,
                ProcessError::UsageLimitExceeded { .. }
                    | ProcessError::AuthenticationRequired
                    | ProcessError::SubscriptionRequired
            );
            if !account_fault {
                if transport_retries_left == 0 {
                    return Err(error);
                }
                transport_retries_left -= 1;
            }
            last_error = Some(error);
        }
    }
}

/// Seconds a client should wait for the pool to recover, bounded to a sane advertised value.
fn retry_after_from(ready_at: Option<i64>) -> Option<u64> {
    let ready_at = ready_at?;
    Some(ready_at.saturating_sub(pool::now()).clamp(1, 7 * 24 * 3600) as u64)
}

impl CodexHome {
    pub(crate) async fn run_turn(
        &self,
        request: CodexTurnRequest,
        updates: Option<mpsc::Sender<TurnUpdate>>,
        emitted: &Arc<AtomicBool>,
        _turn_slot: TurnSlot,
    ) -> Result<CodexTurnResult, ProcessError> {
        let (process, thread_response) = self.start_thread(&request).await?;
        let thread_id = thread_response
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProcessError::Protocol("thread/start response omitted thread.id".to_string())
            })?
            .to_string();
        let served_model = thread_response
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(&request.model.upstream)
            .to_string();
        if served_model != request.model.upstream {
            return Err(ProcessError::Protocol(format!(
                "app-server served unexpected model {served_model:?}"
            )));
        }
        let served_service_tier = thread_response
            .get("serviceTier")
            .and_then(Value::as_str)
            .map(str::to_string);
        let requested_service_tier = request.service_tier.as_deref().unwrap_or("default");
        if served_service_tier.as_deref() != Some(requested_service_tier) {
            // Fast must never degrade silently: reserve and settlement use the requested tier's
            // published subscription-credit multiplier. A pinned app-server/catalog mismatch is
            // safer as a rejected request than standard-speed output charged as Fast (or the
            // inverse).
            return Err(ProcessError::BadRequest);
        }
        let mut events = process.register_turn(&thread_id).await?;
        let mut registration_guard = TurnRegistrationGuard::new(process.clone(), thread_id.clone());

        let result = self
            .run_registered_turn(
                process.clone(),
                &thread_id,
                &mut events,
                request,
                updates,
                emitted,
            )
            .await;
        process.unregister_turn(&thread_id).await;
        registration_guard.disarm();
        // Recycle only a generation whose transport actually closed. A request-scoped protocol
        // response or deadline must not collaterally kill sibling turns on the shared child.
        let recycle = match &result {
            Err(ProcessError::Closed | ProcessError::AuthenticationRequired) => true,
            Err(ProcessError::Protocol(_)) => !process.is_live(),
            // An RPC deadline belongs to the request that missed it. It says nothing about the
            // shared transport: recycling here would kill every sibling turn multiplexed over the
            // same authenticated app-server (the production failure this isolation prevents).
            Err(ProcessError::Timeout(_)) => false,
            _ => false,
        };
        if recycle {
            self.invalidate(&process).await;
        }
        result
    }

    async fn start_thread(
        &self,
        request: &CodexTurnRequest,
    ) -> Result<(Arc<CodexProcess>, Value), ProcessError> {
        let mut params = json!({
            "model": request.model.upstream,
            "cwd": self.config().work_dir,
            "approvalPolicy": "never",
            "sandbox": "read-only",
            // The app-server's explicit standard-tier sentinel is "default". Per-request service
            // tier is entirely owned by the public API body and cannot leak in from a purchased
            // CODEX_HOME.
            "serviceTier": request.service_tier.as_deref().unwrap_or("default"),
            "baseInstructions": request.base_instructions.as_deref().unwrap_or(""),
            "developerInstructions": request.developer_instructions,
            "ephemeral": true,
            "historyMode": "legacy",
            "environments": [],
            "dynamicTools": request.dynamic_tools,
            "experimentalRawEvents": true
        });
        if let Some(verbosity) = &request.verbosity {
            params["config"] = json!({"model_verbosity": verbosity});
        }
        if let Some(prompt_cache_key) = &request.prompt_cache_key {
            params["promptCacheKey"] = Value::String(prompt_cache_key.clone());
        }
        for attempt in 0..2 {
            let process = self.process().await?;
            match process.request("thread/start", params.clone()).await {
                Ok(response) => return Ok((process, response)),
                Err(error @ ProcessError::Closed) => {
                    self.invalidate(&process).await;
                    if attempt != 0 {
                        return Err(error);
                    }
                }
                Err(error @ ProcessError::Protocol(_)) if !process.is_live() => {
                    self.invalidate(&process).await;
                    if attempt != 0 {
                        return Err(error);
                    }
                }
                // A late JSON-RPC response is discarded by the transport, but the app-server and
                // unrelated turns remain valid. Pool-level retry may try another home; a one-home
                // pool simply returns this request error without destroying its only capacity.
                Err(error @ ProcessError::Timeout(_)) => return Err(error),
                Err(error) => return Err(error),
            }
        }
        unreachable!("thread/start retry loop always returns on its second attempt")
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_registered_turn(
        &self,
        process: Arc<CodexProcess>,
        thread_id: &str,
        events: &mut super::process::TurnEvents,
        request: CodexTurnRequest,
        updates: Option<mpsc::Sender<TurnUpdate>>,
        emitted: &Arc<AtomicBool>,
    ) -> Result<CodexTurnResult, ProcessError> {
        let custom_tool_names = request
            .dynamic_tools
            .iter()
            .filter(|tool| tool.get("type").and_then(Value::as_str) == Some("custom"))
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect::<HashSet<_>>();
        if !request.injected_items.is_empty() {
            process
                .request(
                    "thread/inject_items",
                    json!({
                        "threadId": thread_id,
                        "items": request.injected_items
                    }),
                )
                .await?;
        }
        let mut turn_params = json!({
            "threadId": thread_id,
            "input": request.turn_input
        });
        if let Some(effort) = request.reasoning_effort {
            turn_params["effort"] = Value::String(effort);
        }
        if let Some(summary) = request.reasoning_summary {
            turn_params["summary"] = Value::String(summary);
        }
        if let Some(schema) = request.output_schema {
            turn_params["outputSchema"] = schema;
        }
        let turn_response = process.request("turn/start", turn_params).await?;
        let turn_id = turn_response
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProcessError::Protocol("turn/start response omitted turn.id".to_string())
            })?
            .to_string();
        let mut interrupt_guard =
            TurnInterruptGuard::new(process.clone(), thread_id.to_string(), turn_id.clone());

        let timeout = std::time::Duration::from_millis(self.config().turn_timeout_ms.max(1));
        // A silence bound at or above the total deadline simply never fires: the total governs and
        // the turn fails as `turn completion`. That is a coherent configuration, not an error, so
        // it is left alone rather than clamped — clamping would silently relabel the failure.
        let silence_timeout =
            std::time::Duration::from_millis(self.config().turn_silence_timeout_ms.max(1));
        let event_loop = async {
            let mut output = Vec::<Value>::new();
            let mut usage = CodexUsage::default();
            let mut saw_raw_usage = false;
            let mut saw_raw_response_completed = false;
            let mut terminal_tool_call = false;
            let mut interrupt_sent = false;
            loop {
                // Bound silence, not just total duration. A home that stopped answering mid-turn
                // otherwise holds the client for the entire turn deadline before failing, and every
                // one of those ten-minute waits is spent on a subscription that will never reply.
                // The total deadline stays generous so genuine long reasoning is never cut short.
                let event = match tokio::time::timeout(silence_timeout, events.recv()).await {
                    Ok(event) => event?,
                    Err(_) => return Err(ProcessError::Timeout("turn silence")),
                };
                match event {
                    AppServerEvent::Notification { method, params } => {
                        if params
                            .get("turnId")
                            .and_then(Value::as_str)
                            .is_some_and(|candidate| candidate != turn_id)
                        {
                            continue;
                        }
                        match method.as_str() {
                            "item/agentMessage/delta" => {
                                let Some(delta) = params.get("delta").and_then(Value::as_str)
                                else {
                                    continue;
                                };
                                let item_id = params
                                    .get("itemId")
                                    .and_then(Value::as_str)
                                    .unwrap_or("msg_unknown")
                                    .to_string();
                                send_update(
                                    &updates,
                                    emitted,
                                    TurnUpdate::TextDelta {
                                        item_id,
                                        delta: delta.to_string(),
                                    },
                                )
                                .await;
                            }
                            "item/reasoning/summaryPartAdded" => {
                                let (Some(item_id), Some(summary_index)) = (
                                    params.get("itemId").and_then(Value::as_str),
                                    params.get("summaryIndex").and_then(Value::as_u64),
                                ) else {
                                    continue;
                                };
                                send_update(
                                    &updates,
                                    emitted,
                                    TurnUpdate::ReasoningSummaryPartAdded {
                                        item_id: item_id.to_string(),
                                        summary_index,
                                    },
                                )
                                .await;
                            }
                            "item/reasoning/summaryTextDelta" => {
                                let (Some(item_id), Some(summary_index), Some(delta)) = (
                                    params.get("itemId").and_then(Value::as_str),
                                    params.get("summaryIndex").and_then(Value::as_u64),
                                    params.get("delta").and_then(Value::as_str),
                                ) else {
                                    continue;
                                };
                                send_update(
                                    &updates,
                                    emitted,
                                    TurnUpdate::ReasoningSummaryDelta {
                                        item_id: item_id.to_string(),
                                        summary_index,
                                        delta: delta.to_string(),
                                    },
                                )
                                .await;
                            }
                            // Raw reasoning text is hidden chain-of-thought. The public adapter
                            // deliberately exposes only the provider-authored reasoning summary.
                            "item/reasoning/textDelta" => {}
                            "rawResponseItem/completed" => {
                                if let Some(mut item) = params.get("item").cloned() {
                                    ensure_output_item_id(&mut item);
                                    let is_tool_call = matches!(
                                        item.get("type").and_then(Value::as_str),
                                        Some("function_call" | "custom_tool_call")
                                    );
                                    if is_tool_call {
                                        terminal_tool_call = true;
                                    }
                                    // Depending on app-server event ordering, the callback may be
                                    // observed before its raw response item. The callback fallback
                                    // already carries the same public tool call; never emit a
                                    // second item for the same call_id.
                                    let duplicate_tool_call = is_tool_call
                                        && item.get("call_id").and_then(Value::as_str).is_some_and(
                                            |call_id| {
                                                output.iter().any(|candidate| {
                                                    matches!(
                                                        candidate
                                                            .get("type")
                                                            .and_then(Value::as_str),
                                                        Some("function_call" | "custom_tool_call")
                                                    ) && candidate
                                                        .get("call_id")
                                                        .and_then(Value::as_str)
                                                        == Some(call_id)
                                                })
                                            },
                                        );
                                    if !duplicate_tool_call {
                                        send_update(
                                            &updates,
                                            emitted,
                                            TurnUpdate::RawItem(item.clone()),
                                        )
                                        .await;
                                        output.push(item);
                                    }
                                }
                            }
                            "rawResponse/completed" => {
                                saw_raw_response_completed = true;
                                if let Some(raw_usage) =
                                    params.get("usage").filter(|v| !v.is_null())
                                {
                                    usage.add_assign(&CodexUsage::from_value(raw_usage));
                                    saw_raw_usage = true;
                                }
                                if terminal_tool_call && !interrupt_sent {
                                    process
                                        .request(
                                            "turn/interrupt",
                                            json!({"threadId": thread_id, "turnId": turn_id}),
                                        )
                                        .await?;
                                    interrupt_sent = true;
                                }
                            }
                            "thread/tokenUsage/updated" if !saw_raw_usage => {
                                if let Some(last) = params.pointer("/tokenUsage/last") {
                                    usage = CodexUsage::from_value(last);
                                }
                            }
                            "turn/completed" => {
                                let status = params
                                    .pointer("/turn/status")
                                    .and_then(Value::as_str)
                                    .unwrap_or("failed");
                                let deliberate_interrupt =
                                    status == "interrupted" && terminal_tool_call;
                                if status != "completed" && !deliberate_interrupt {
                                    if let Some(error) = classify_turn_error(
                                        &process,
                                        params.pointer("/turn/error/codexErrorInfo"),
                                    )
                                    .await
                                    {
                                        return Err(error);
                                    }
                                    let message = params
                                        .pointer("/turn/error/message")
                                        .and_then(Value::as_str)
                                        .unwrap_or("model turn did not complete");
                                    return Err(ProcessError::Protocol(message.to_string()));
                                }
                                break;
                            }
                            "error" => {
                                if !params
                                    .get("willRetry")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false)
                                {
                                    if let Some(error) = classify_turn_error(
                                        &process,
                                        params.pointer("/error/codexErrorInfo"),
                                    )
                                    .await
                                    {
                                        return Err(error);
                                    }
                                    let message = params
                                        .pointer("/error/message")
                                        .and_then(Value::as_str)
                                        .unwrap_or("model turn failed");
                                    return Err(ProcessError::Protocol(message.to_string()));
                                }
                            }
                            _ => {}
                        }
                    }
                    AppServerEvent::ServerRequest { id, method, params } => {
                        if method != "item/tool/call" {
                            let _ = process
                                .respond_error(id, -32601, "unsupported app-server callback")
                                .await;
                            return Err(ProcessError::Protocol(format!(
                                "unexpected app-server callback {method}"
                            )));
                        }
                        let event_turn_id = params
                            .get("turnId")
                            .and_then(Value::as_str)
                            .unwrap_or(&turn_id);
                        let call_id = params
                            .get("callId")
                            .and_then(Value::as_str)
                            .unwrap_or("call_unknown");
                        if !output.iter().any(|item| {
                            matches!(
                                item.get("type").and_then(Value::as_str),
                                Some("function_call" | "custom_tool_call")
                            ) && item.get("call_id").and_then(Value::as_str) == Some(call_id)
                        }) {
                            let item =
                                callback_tool_call_item(&params, call_id, &custom_tool_names);
                            send_update(&updates, emitted, TurnUpdate::RawItem(item.clone())).await;
                            output.push(item);
                        }
                        terminal_tool_call = true;
                        // Tool execution belongs to the public API client. Keep app-server's
                        // callback pending until the upstream response publishes its authoritative
                        // usage, then interrupt without fabricating a tool result or starting a
                        // follow-up sampling request.
                        if saw_raw_response_completed && !interrupt_sent {
                            process
                                .request(
                                    "turn/interrupt",
                                    json!({"threadId": thread_id, "turnId": event_turn_id}),
                                )
                                .await?;
                            interrupt_sent = true;
                        }
                    }
                }
            }
            Ok(CodexTurnResult { output, usage })
        };

        match tokio::time::timeout(timeout, event_loop).await {
            Ok(Ok(result)) => {
                interrupt_guard.disarm();
                Ok(result)
            }
            Ok(Err(error)) => Err(error),
            Err(_) => {
                // Turn wall-clock exceeded. Interrupt only THIS turn. Even a timed-out interrupt is
                // still request-local and cannot justify killing sibling turns; actual EOF/write
                // failure closes `ProcessShared`, which is the concrete signal to reap the child.
                interrupt_guard.disarm();
                let interrupted = process
                    .request(
                        "turn/interrupt",
                        json!({"threadId": thread_id, "turnId": turn_id.clone()}),
                    )
                    .await;
                if interrupted.is_err() && !process.is_live() {
                    self.invalidate(&process).await;
                }
                Err(ProcessError::Timeout("turn completion"))
            }
        }
    }
}

fn callback_tool_call_item(
    params: &Value,
    call_id: &str,
    custom_tool_names: &HashSet<String>,
) -> Value {
    let name = params
        .get("tool")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let is_custom =
        params.get("namespace").is_none_or(Value::is_null) && custom_tool_names.contains(name);
    let mut item = if is_custom {
        json!({
            "id": new_id("ctc"),
            "type": "custom_tool_call",
            "call_id": call_id,
            "name": name,
            "input": params.get("arguments").and_then(Value::as_str).unwrap_or(""),
            "status": "completed"
        })
    } else {
        let arguments = params
            .get("arguments")
            .map(|arguments| serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string()))
            .unwrap_or_else(|| "{}".to_string());
        json!({
            "id": new_id("fc"),
            "type": "function_call",
            "call_id": call_id,
            "name": name,
            "arguments": arguments,
            "status": "completed"
        })
    };
    if let Some(namespace) = params.get("namespace").and_then(Value::as_str) {
        item["namespace"] = Value::String(namespace.to_string());
    }
    item
}

fn ensure_output_item_id(item: &mut Value) {
    if item.get("id").and_then(Value::as_str).is_some() {
        return;
    }
    let prefix = match item.get("type").and_then(Value::as_str) {
        Some("message") => "msg",
        Some("function_call") => "fc",
        Some("custom_tool_call") => "ctc",
        Some("reasoning") => "rs",
        _ => return,
    };
    item["id"] = Value::String(new_id(prefix));
}

async fn classify_turn_error(process: &CodexProcess, info: Option<&Value>) -> Option<ProcessError> {
    let info = info?;
    let kind = info
        .as_str()
        .or_else(|| info.as_object()?.keys().next().map(String::as_str));
    let http_status = info
        .as_object()
        .and_then(|object| {
            object
                .values()
                .find_map(|value| value.get("httpStatusCode"))
        })
        .and_then(Value::as_u64);
    match (kind, http_status) {
        (Some("contextWindowExceeded"), _) => Some(ProcessError::ContextWindowExceeded),
        (Some("usageLimitExceeded" | "sessionBudgetExceeded"), _) | (_, Some(429)) => {
            Some(ProcessError::UsageLimitExceeded {
                retry_after: process.usage_limit_retry_after().await,
            })
        }
        (Some("badRequest" | "cyberPolicy"), _) | (_, Some(400)) => Some(ProcessError::BadRequest),
        (Some("unauthorized"), _) | (_, Some(401 | 403)) => {
            Some(ProcessError::AuthenticationRequired)
        }
        _ => None,
    }
}

/// Cancellation-safe cleanup for client disconnects and dropped HTTP handlers.
struct TurnInterruptGuard {
    process: Arc<CodexProcess>,
    thread_id: String,
    turn_id: String,
    armed: bool,
}

impl TurnInterruptGuard {
    fn new(process: Arc<CodexProcess>, thread_id: String, turn_id: String) -> Self {
        Self {
            process,
            thread_id,
            turn_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TurnInterruptGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let process = self.process.clone();
        let thread_id = self.thread_id.clone();
        let turn_id = self.turn_id.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = process
                    .request(
                        "turn/interrupt",
                        json!({"threadId": thread_id, "turnId": turn_id}),
                    )
                    .await;
            });
        }
    }
}

struct TurnRegistrationGuard {
    process: Arc<CodexProcess>,
    thread_id: String,
    armed: bool,
}

impl TurnRegistrationGuard {
    fn new(process: Arc<CodexProcess>, thread_id: String) -> Self {
        Self {
            process,
            thread_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TurnRegistrationGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let process = self.process.clone();
        let thread_id = self.thread_id.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                process.unregister_turn(&thread_id).await;
            });
        }
    }
}

/// Deliver one streaming update and record that model output has left this attempt.
///
/// The `emitted` flag is what makes pre-stream retry safe: once a delta has been handed to the
/// public stream the turn is no longer replaceable, and the gateway must surface the failure
/// instead of starting a second attempt that would interleave with it.
async fn send_update(
    updates: &Option<mpsc::Sender<TurnUpdate>>,
    emitted: &Arc<AtomicBool>,
    update: TurnUpdate,
) {
    if let Some(sender) = updates {
        // Preserve every delta while the downstream exists. Once it disconnects, send fails
        // immediately and the upstream event loop continues through authoritative usage/settlement.
        if sender.send(update).await.is_ok() {
            emitted.store(true, Ordering::Release);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::codex::{CodexConfig, CodexPrices, CodexTransport};
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_WORKSPACE: AtomicU64 = AtomicU64::new(1);

    struct TestWorkspace {
        root: PathBuf,
        log: PathBuf,
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn test_model() -> CodexModel {
        CodexModel {
            id: "gpt-5.6".to_string(),
            upstream: "gpt-5.6-sol".to_string(),
            created: 0,
            owned_by: "test".to_string(),
            max_output_tokens: 128_000,
            reasoning_efforts: ["none", "low", "medium", "high", "xhigh", "max"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            fast_multiplier_basis_points: Some(25_000),
            prices: CodexPrices {
                input: 5_000,
                cached_input: 500,
                cache_write_input: 6_250,
                output: 30_000,
                long_context_threshold: 272_000,
                long_input_basis_points: 20_000,
                long_output_basis_points: 15_000,
            },
        }
    }

    fn turn_request(model: CodexModel) -> CodexTurnRequest {
        CodexTurnRequest {
            model,
            prompt_cache_key: None,
            base_instructions: None,
            developer_instructions: Some("Only the caller's developer instruction.".to_string()),
            injected_items: Vec::new(),
            turn_input: vec![json!({"type": "text", "text": "hello"})],
            dynamic_tools: vec![json!({
                "type": "function",
                "name": "get_weather",
                "description": "Get weather",
                "inputSchema": {"type": "object"},
                "deferLoading": false
            })],
            service_tier: None,
            reasoning_effort: Some("medium".to_string()),
            reasoning_summary: Some("auto".to_string()),
            output_schema: None,
            verbosity: None,
        }
    }

    fn fake_gateway(mode: &str) -> (Arc<CodexGateway>, TestWorkspace) {
        fake_gateway_with_request_timeout(mode, 2_000)
    }

    fn fake_gateway_with_request_timeout(
        mode: &str,
        request_timeout_ms: u64,
    ) -> (Arc<CodexGateway>, TestWorkspace) {
        let suffix = NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "claude-api-codex-runner-test-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let log = root.join("requests.jsonl");
        let binary = root.join("fake-codex");
        let script = r#"#!/bin/sh
if [ "${1-}" = "--version" ]; then
  if [ "__MODE__" = "version_timeout" ]; then
    while :; do :; done
  elif [ "__MODE__" = "version_cancel" ]; then
    (trap '' TERM; while :; do sleep 1; done) &
    printf '%s\n' "$!" > "$CODEX_HOME/version-helper-pid"
    while :; do sleep 1; done
  fi
  printf '%s\n' 'codex-cli test'
  exit 0
fi
log_file='__LOG_PATH__'
mode='__MODE__'
# Pool tests run several homes against this one attested binary, so per-home behaviour is selected
# by a file inside each CODEX_HOME rather than by a second script with a different digest.
if [ -f "$CODEX_HOME/mode" ]; then
  mode=$(cat "$CODEX_HOME/mode")
  log_file="$CODEX_HOME/requests.jsonl"
fi
if [ "$mode" = "descendant" ]; then
  (trap '' TERM; while :; do sleep 1; done) &
  printf '%s\n' "$!" > "$CODEX_HOME/helper-pid"
fi
generation=0
concurrent_turns=0
if [ "$mode" = "thread_start_timeout_once" ] || [ "$mode" = "turn_start_timeout_once" ] || [ "$mode" = "startup_cancel_once" ] || [ "$mode" = "invalidate_cancel" ]; then
  generation_file="$CODEX_HOME/generation"
  if [ -f "$generation_file" ]; then
    generation=$(cat "$generation_file")
  fi
  generation=$((generation + 1))
  printf '%s\n' "$generation" > "$generation_file"
  printf '%s\n' "$$" >> "$CODEX_HOME/generation-pids"
fi
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log_file"
  id=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      if [ "$mode" = "startup_cancel_once" ] && [ "$generation" -eq 1 ]; then
        continue
      fi
      printf '{"id":%s,"result":{}}\n' "$id"
      ;;
    *'"method":"account/read"'*)
      printf '{"id":%s,"result":{"account":{"type":"chatgpt"},"requiresOpenaiAuth":true}}\n' "$id"
      ;;
    *'"method":"account/rateLimits/read"'*)
      if [ "$mode" = "rate_limit_timeout" ]; then
        continue
      elif [ "$mode" = "usage_limit" ]; then
        printf '{"id":%s,"result":{"rateLimits":{"primary":{"usedPercent":100,"windowDurationMins":300,"resetsAt":4102444800},"secondary":null,"rateLimitReachedType":"rate_limit_reached","spendControlReached":false}}}\n' "$id"
      elif [ "$mode" = "near_limit" ]; then
        printf '{"id":%s,"result":{"rateLimits":{"primary":{"usedPercent":97,"windowDurationMins":300,"resetsAt":4102444800},"secondary":null,"rateLimitReachedType":null,"spendControlReached":false}}}\n' "$id"
      elif [ "$mode" = "full_window" ]; then
        printf '{"id":%s,"result":{"rateLimits":{"primary":{"usedPercent":100,"windowDurationMins":300,"resetsAt":4102444800},"secondary":null,"rateLimitReachedType":null,"spendControlReached":false}}}\n' "$id"
      else
        printf '{"id":%s,"result":{"rateLimits":{"primary":{"usedPercent":25,"windowDurationMins":300,"resetsAt":4102444800},"secondary":{"usedPercent":10,"windowDurationMins":10080,"resetsAt":4102444800},"rateLimitReachedType":null,"spendControlReached":false}}}\n' "$id"
      fi
      ;;
    *'"method":"thread/start"'*)
      if [ "$mode" = "thread_start_timeout_once" ] && [ "$generation" -eq 1 ]; then
        continue
      elif [ "$mode" = "thread_start_timeout_then_recover" ] && [ ! -f "$CODEX_HOME/thread-start-timed-out" ]; then
        : > "$CODEX_HOME/thread-start-timed-out"
        continue
      elif [ "$mode" = "concurrent_threads" ]; then
        printf '{"id":%s,"result":{"model":"gpt-5.6-sol","serviceTier":"default","thread":{"id":"thread-%s"}}}\n' "$id" "$id"
        continue
      fi
      case "$line" in
        *'"serviceTier":"priority"'*)
          printf '{"id":%s,"result":{"model":"gpt-5.6-sol","serviceTier":"priority","thread":{"id":"thread-1"}}}\n' "$id"
          ;;
        *'"serviceTier":"default"'*)
          printf '{"id":%s,"result":{"model":"gpt-5.6-sol","serviceTier":"default","thread":{"id":"thread-1"}}}\n' "$id"
          ;;
        *)
          printf '{"id":%s,"result":{"model":"gpt-5.6-sol","serviceTier":null,"thread":{"id":"thread-1"}}}\n' "$id"
          ;;
      esac
      ;;
    *'"method":"turn/start"'*)
      if [ "$mode" = "turn_start_timeout_once" ] && [ "$generation" -eq 1 ]; then
        continue
      elif [ "$mode" = "turn_start_timeout_then_recover" ] && [ ! -f "$CODEX_HOME/turn-start-timed-out" ]; then
        : > "$CODEX_HOME/turn-start-timed-out"
        continue
      fi
      if [ "$mode" = "concurrent_threads" ]; then
        concurrent_turns=$((concurrent_turns + 1))
        concurrent_turn="$concurrent_turns"
        thread_id=$(printf '%s\n' "$line" | sed -n 's/.*"threadId":"\([^"]*\)".*/\1/p')
        turn_id="turn-$concurrent_turn"
        printf '{"id":%s,"result":{"turn":{"id":"%s"}}}\n' "$id" "$turn_id"
        : > "$CODEX_HOME/turn-started-$concurrent_turn"
        (
          while [ ! -f "$CODEX_HOME/release-turn-$concurrent_turn" ]; do
            sleep 0.01
          done
          printf '{"method":"item/agentMessage/delta","params":{"threadId":"%s","turnId":"%s","itemId":"msg-%s","delta":"hello-%s"}}\n' "$thread_id" "$turn_id" "$concurrent_turn" "$concurrent_turn"
          printf '{"method":"rawResponseItem/completed","params":{"threadId":"%s","turnId":"%s","item":{"type":"message","id":"msg-%s","role":"assistant","content":[{"type":"output_text","text":"hello-%s"}]}}}\n' "$thread_id" "$turn_id" "$concurrent_turn" "$concurrent_turn"
          printf '{"method":"rawResponse/completed","params":{"threadId":"%s","turnId":"%s","usage":{"inputTokens":101,"cachedInputTokens":41,"cacheWriteInputTokens":7,"outputTokens":23,"reasoningOutputTokens":11,"totalTokens":124}}}\n' "$thread_id" "$turn_id"
          printf '{"method":"turn/completed","params":{"threadId":"%s","turnId":"%s","turn":{"status":"completed"}}}\n' "$thread_id" "$turn_id"
        ) &
        continue
      fi
      printf '{"id":%s,"result":{"turn":{"id":"turn-1"}}}\n' "$id"
      if [ "$mode" = "text" ] || [ "$mode" = "near_limit" ] || [ "$mode" = "rate_limit_timeout" ] || [ "$mode" = "thread_start_timeout_once" ] || [ "$mode" = "turn_start_timeout_once" ] || [ "$mode" = "thread_start_timeout_then_recover" ] || [ "$mode" = "turn_start_timeout_then_recover" ]; then
        printf '%s\n' '{"method":"item/agentMessage/delta","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"msg-1","delta":"hello"}}'
        printf '%s\n' '{"method":"rawResponseItem/completed","params":{"threadId":"thread-1","turnId":"turn-1","item":{"type":"message","id":"msg-1","role":"assistant","content":[{"type":"output_text","text":"hello"}]}}}'
        printf '%s\n' '{"method":"rawResponse/completed","params":{"threadId":"thread-1","turnId":"turn-1","usage":{"inputTokens":101,"cachedInputTokens":41,"cacheWriteInputTokens":7,"outputTokens":23,"reasoningOutputTokens":11,"totalTokens":124}}}'
        printf '%s\n' '{"method":"turn/completed","params":{"threadId":"thread-1","turnId":"turn-1","turn":{"status":"completed"}}}'
      elif [ "$mode" = "reasoning" ]; then
        printf '%s\n' '{"method":"item/reasoning/summaryPartAdded","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"rs-1","summaryIndex":0}}'
        printf '%s\n' '{"method":"item/reasoning/summaryTextDelta","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"rs-1","summaryIndex":0,"delta":"Checked"}}'
        printf '%s\n' '{"method":"item/reasoning/textDelta","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"rs-1","contentIndex":0,"delta":"private chain of thought"}}'
        printf '%s\n' '{"method":"rawResponseItem/completed","params":{"threadId":"thread-1","turnId":"turn-1","item":{"type":"reasoning","id":"rs-1","summary":[{"type":"summary_text","text":"Checked"}],"content":[{"type":"reasoning_text","text":"private chain of thought"}],"encrypted_content":"ciphertext"}}}'
        printf '%s\n' '{"method":"rawResponse/completed","params":{"threadId":"thread-1","turnId":"turn-1","usage":{"inputTokens":10,"cachedInputTokens":0,"cacheWriteInputTokens":0,"outputTokens":5,"reasoningOutputTokens":4,"totalTokens":15}}}'
        printf '%s\n' '{"method":"turn/completed","params":{"threadId":"thread-1","turnId":"turn-1","turn":{"status":"completed"}}}'
      elif [ "$mode" = "tool" ]; then
        printf '%s\n' '{"id":"callback-1","method":"item/tool/call","params":{"threadId":"thread-1","turnId":"turn-1","callId":"call-1","tool":"get_weather","arguments":{"city":"Tbilisi"}}}'
        printf '%s\n' '{"method":"rawResponseItem/completed","params":{"threadId":"thread-1","turnId":"turn-1","item":{"id":"fc-upstream","type":"function_call","call_id":"call-1","name":"get_weather","arguments":"{\"city\":\"Tbilisi\"}","status":"completed"}}}'
        printf '%s\n' '{"method":"rawResponse/completed","params":{"threadId":"thread-1","turnId":"turn-1","usage":{"inputTokens":19,"cachedInputTokens":3,"cacheWriteInputTokens":0,"outputTokens":8,"reasoningOutputTokens":2,"totalTokens":27}}}'
      elif [ "$mode" = "parallel_tool" ]; then
        printf '%s\n' '{"id":"callback-1","method":"item/tool/call","params":{"threadId":"thread-1","turnId":"turn-1","callId":"call-1","tool":"get_weather","arguments":{"city":"Tbilisi"}}}'
        printf '%s\n' '{"method":"rawResponseItem/completed","params":{"threadId":"thread-1","turnId":"turn-1","item":{"id":"fc-upstream-1","type":"function_call","call_id":"call-1","name":"get_weather","arguments":"{\"city\":\"Tbilisi\"}","status":"completed"}}}'
        printf '%s\n' '{"id":"callback-2","method":"item/tool/call","params":{"threadId":"thread-1","turnId":"turn-1","callId":"call-2","tool":"get_weather","arguments":{"city":"Paris"}}}'
        printf '%s\n' '{"method":"rawResponseItem/completed","params":{"threadId":"thread-1","turnId":"turn-1","item":{"id":"fc-upstream-2","type":"function_call","call_id":"call-2","name":"get_weather","arguments":"{\"city\":\"Paris\"}","status":"completed"}}}'
        printf '%s\n' '{"method":"rawResponse/completed","params":{"threadId":"thread-1","turnId":"turn-1","usage":{"inputTokens":23,"cachedInputTokens":3,"cacheWriteInputTokens":0,"outputTokens":14,"reasoningOutputTokens":2,"totalTokens":37}}}'
      elif [ "$mode" = "custom_tool" ]; then
        printf '%s\n' '{"id":"callback-1","method":"item/tool/call","params":{"threadId":"thread-1","turnId":"turn-1","callId":"call-custom-1","tool":"exec","arguments":"text('\''ok'\'')"}}'
        printf '%s\n' '{"method":"rawResponseItem/completed","params":{"threadId":"thread-1","turnId":"turn-1","item":{"id":"ctc-upstream","type":"custom_tool_call","call_id":"call-custom-1","name":"exec","input":"text('\''ok'\'')","status":"completed"}}}'
        printf '%s\n' '{"method":"rawResponse/completed","params":{"threadId":"thread-1","turnId":"turn-1","usage":{"inputTokens":29,"cachedInputTokens":5,"cacheWriteInputTokens":0,"outputTokens":9,"reasoningOutputTokens":3,"totalTokens":38}}}'
      elif [ "$mode" = "usage_limit" ]; then
        printf '%s\n' '{"method":"error","params":{"threadId":"thread-1","turnId":"turn-1","error":{"message":"sensitive upstream diagnostic","codexErrorInfo":"usageLimitExceeded"},"willRetry":false}}'
      elif [ "$mode" = "die" ]; then
        exit 0
      elif [ "$mode" = "stream_then_die" ]; then
        printf '%s\n' '{"method":"item/agentMessage/delta","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"msg-1","delta":"partial"}}'
        exit 0
      else
        printf '%s\n' '{"method":"item/agentMessage/delta","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"msg-1","delta":"ready"}}'
      fi
      ;;
    *'"method":"turn/interrupt"'*)
      printf '{"id":%s,"result":{}}\n' "$id"
      if [ "$mode" = "tool" ] || [ "$mode" = "parallel_tool" ] || [ "$mode" = "custom_tool" ]; then
        printf '%s\n' '{"method":"turn/completed","params":{"threadId":"thread-1","turnId":"turn-1","turn":{"status":"interrupted"}}}'
      fi
      ;;
    *'"method":"thread/inject_items"'*)
      printf '{"id":%s,"result":{}}\n' "$id"
      ;;
  esac
done
"#
        .replace("__LOG_PATH__", log.to_str().unwrap())
        .replace("__MODE__", mode);
        std::fs::write(&binary, script.as_bytes()).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let digest = format!("{:x}", Sha256::digest(script.as_bytes()));
        let ownership_lock = root.join("ownership.lock");
        std::fs::write(&ownership_lock, []).unwrap();
        let config = CodexConfig {
            enabled: true,
            transport: CodexTransport::OwnedChild,
            ownership_lock_file: ownership_lock.to_str().unwrap().to_string(),
            binary: binary.to_str().unwrap().to_string(),
            binary_sha256: digest,
            expected_version: "codex-cli test".to_string(),
            homes: vec![root.to_str().unwrap().to_string()],
            homes_dir: None,
            work_dir: root.to_str().unwrap().to_string(),
            // The workspace test runner executes several fake child processes in parallel.
            // Leave enough scheduler headroom for the digest + version preflight under CI load;
            // individual JSON-RPC and turn deadlines below remain short.
            startup_timeout_ms: if mode == "version_timeout" {
                100
            } else {
                10_000
            },
            request_timeout_ms,
            turn_timeout_ms: match mode {
                "turn_completion_timeout" => 200,
                "concurrent_threads" => 10_000,
                _ => 2_000,
            },
            // Fixtures exercise the TOTAL deadline, so silence is set far above it and never
            // fires: these cases keep testing exactly what they always tested. The dedicated
            // silence test below sets it below the total instead.
            turn_silence_timeout_ms: match mode {
                "turn_silence_timeout" => 150,
                _ => 600_000,
            },
            health_probe_interval_secs: 300,
            reserve_overhead_tokens: 0,
            history_ttl_secs: 600,
            history_local_cap: 32,
            history_redis_url: None,
            history_secret: Some("test".to_string()),
            history_redis_timeout_ms: 10,
            child_proxy_env: BTreeMap::new(),
            models: vec![test_model()],
        };
        (
            Arc::new(CodexGateway::new(config).unwrap()),
            TestWorkspace { root, log },
        )
    }

    /// A pool of homes served by one attested binary, each with its own scripted behaviour.
    ///
    /// Returns the gateway, the workspace, and each home's request log so a test can prove which
    /// home actually served the turn.
    fn fake_pool_gateway(modes: &[&str]) -> (Arc<CodexGateway>, TestWorkspace, Vec<PathBuf>) {
        let (gateway, workspace) = fake_gateway("text");
        drop(gateway);
        let root = workspace.root.clone();
        let binary = root.join("fake-codex");
        let script = std::fs::read_to_string(&binary).unwrap();
        let digest = format!("{:x}", Sha256::digest(script.as_bytes()));
        // `fake_gateway` can still have detached cleanup holding its fence after the last Arc is
        // dropped. The pool fixture is a distinct provider instance, so do not race that cleanup.
        let ownership_lock = root.join("pool-ownership.lock");
        std::fs::write(&ownership_lock, []).unwrap();
        let mut homes = Vec::with_capacity(modes.len());
        let mut logs = Vec::with_capacity(modes.len());
        for (index, mode) in modes.iter().enumerate() {
            let home = root.join(format!("home{index}"));
            std::fs::create_dir(&home).unwrap();
            std::fs::write(home.join("mode"), mode.as_bytes()).unwrap();
            logs.push(home.join("requests.jsonl"));
            homes.push(home.to_str().unwrap().to_string());
        }
        let config = CodexConfig {
            enabled: true,
            transport: CodexTransport::OwnedChild,
            ownership_lock_file: ownership_lock.to_str().unwrap().to_string(),
            binary: binary.to_str().unwrap().to_string(),
            binary_sha256: digest,
            expected_version: "codex-cli test".to_string(),
            homes,
            homes_dir: Some(root.join("pool").to_str().unwrap().to_string()),
            work_dir: root.to_str().unwrap().to_string(),
            startup_timeout_ms: 10_000,
            request_timeout_ms: 2_000,
            turn_timeout_ms: 2_000,
            turn_silence_timeout_ms: 2_000,
            health_probe_interval_secs: 300,
            reserve_overhead_tokens: 0,
            history_ttl_secs: 600,
            history_local_cap: 32,
            history_redis_url: None,
            history_secret: Some("test".to_string()),
            history_redis_timeout_ms: 10,
            child_proxy_env: BTreeMap::new(),
            models: vec![test_model()],
        };
        (
            Arc::new(CodexGateway::new(config).unwrap()),
            workspace,
            logs,
        )
    }

    fn served_turn(log: &Path) -> bool {
        logged_requests(log)
            .iter()
            .any(|request| request.get("method").and_then(Value::as_str) == Some("turn/start"))
    }

    fn logged_requests(path: &Path) -> Vec<Value> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn process_exists(pid: u32) -> bool {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    async fn wait_for_method(path: &Path, method: &str) {
        for _ in 0..100 {
            if logged_requests(path)
                .iter()
                .any(|request| request.get("method").and_then(Value::as_str) == Some(method))
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("fake app-server did not receive {method}");
    }

    async fn wait_for_path(path: &Path) {
        for _ in 0..300 {
            if path.exists() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let requests = std::fs::read_to_string(path.parent().unwrap().join("requests.jsonl"))
            .unwrap_or_default();
        panic!(
            "fake app-server did not create {}; requests:\n{requests}",
            path.display()
        );
    }

    #[tokio::test]
    async fn full_transport_preserves_prompt_boundary_events_and_exact_usage() {
        let (gateway, workspace) = fake_gateway("text");
        let (updates_tx, mut updates_rx) = mpsc::channel(8);
        let result = gateway
            .run_turn(turn_request(test_model()), Some(updates_tx), None)
            .await
            .unwrap();

        assert_eq!(result.output.len(), 1);
        assert_eq!(result.output[0]["content"][0]["text"], "hello");
        assert_eq!(result.usage.input_tokens, 101);
        assert_eq!(result.usage.cached_input_tokens, 41);
        assert_eq!(result.usage.cache_write_input_tokens, 7);
        assert_eq!(result.usage.output_tokens, 23);
        assert_eq!(result.usage.reasoning_output_tokens, 11);
        assert_eq!(result.usage.total_tokens, 124);
        assert!(matches!(
            updates_rx.recv().await,
            Some(TurnUpdate::TextDelta { item_id, delta })
                if item_id == "msg-1" && delta == "hello"
        ));
        assert!(matches!(
            updates_rx.recv().await,
            Some(TurnUpdate::RawItem(item)) if item["id"] == "msg-1"
        ));

        let requests = logged_requests(&workspace.log);
        let limits = gateway.operational_status().await.rate_limits.unwrap();
        assert_eq!(limits.primary.unwrap().used_percent, 25);
        assert_eq!(limits.secondary.unwrap().window_duration_mins, Some(10_080));
        assert!(!limits.reached);
        let initialize = requests
            .iter()
            .find(|request| request["method"] == "initialize")
            .unwrap();
        assert_eq!(
            initialize["params"]["clientInfo"]["name"],
            "apitoken_openai_compat"
        );
        let thread_start = requests
            .iter()
            .find(|request| request["method"] == "thread/start")
            .unwrap();
        assert_eq!(thread_start["params"]["baseInstructions"], "");
        assert_eq!(
            thread_start["params"]["developerInstructions"],
            "Only the caller's developer instruction."
        );
        assert_eq!(
            thread_start["params"]["dynamicTools"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            thread_start["params"]["dynamicTools"][0]["name"],
            "get_weather"
        );
        assert_eq!(thread_start["params"]["serviceTier"], "default");
        let turn_start = requests
            .iter()
            .find(|request| request["method"] == "turn/start")
            .unwrap();
        assert_eq!(turn_start["params"]["effort"], "medium");
        assert_eq!(turn_start["params"]["summary"], "auto");
        assert_eq!(turn_start["params"]["input"][0]["text"], "hello");
    }

    #[tokio::test]
    async fn fast_service_tier_reaches_thread_start_as_priority() {
        let (gateway, workspace) = fake_gateway("text");
        let mut request = turn_request(test_model());
        request.service_tier = Some("priority".to_string());

        gateway.run_turn(request, None, None).await.unwrap();

        let requests = logged_requests(&workspace.log);
        let thread_start = requests
            .iter()
            .find(|request| request["method"] == "thread/start")
            .unwrap();
        assert_eq!(thread_start["params"]["serviceTier"], "priority");
    }

    /// Agent terminals (opencode, OpenAI SDKs, Codex clients) attach clipboard screenshots as
    /// megabyte-scale `data:` URLs. The app-server `UserInput::Image { url, .. }` contract
    /// requires the payload to reach `turn/start` byte-for-byte, so assert the full data URL,
    /// the `detail` passthrough, and the part order survive the JSON-RPC framing.
    #[tokio::test]
    async fn turn_start_carries_image_inputs_verbatim() {
        let (gateway, workspace) = fake_gateway("text");
        let data_url = format!("data:image/png;base64,{}", "A".repeat(200_000));
        let mut request = turn_request(test_model());
        request.turn_input = vec![
            json!({"type": "image", "url": data_url, "detail": "high"}),
            json!({"type": "text", "text": "what is in this image?"}),
        ];
        gateway.run_turn(request, None, None).await.unwrap();

        let requests = logged_requests(&workspace.log);
        let turn_start = requests
            .iter()
            .find(|request| request["method"] == "turn/start")
            .unwrap();
        assert_eq!(
            turn_start["params"]["input"][0],
            json!({"type": "image", "url": data_url, "detail": "high"})
        );
        assert_eq!(
            turn_start["params"]["input"][1],
            json!({"type": "text", "text": "what is in this image?"})
        );
    }

    #[tokio::test]
    async fn preflight_capacity_admits_a_healthy_pool() {
        // A healthy pool must pass the streaming pre-check so the SSE stream opens normally; the
        // negative (all-limited) path shares select_home with run_turn's usage-limit rotation.
        let (gateway, _workspace) = fake_gateway("text");
        assert!(gateway.preflight_capacity().await.is_ok());
    }

    #[tokio::test]
    async fn served_turn_credits_real_spend_but_first_window_snapshot_stays_unknown() {
        let (gateway, _workspace) = fake_gateway("text");
        let result = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap();
        assert_eq!(result.usage.input_tokens, 101);

        let status = gateway.operational_status().await;
        let home = status.homes.first().expect("one fake home");
        // 53*5000 + 41*500 + 7*6250 + 23*30000 = 1_019_250 nanoUSD at the pinned catalog rates.
        let expected_usd = 1_019_250.0 / 1e9;
        assert!(
            (home.spend_usd_total - expected_usd).abs() < 1e-9,
            "spend {} != {expected_usd}",
            home.spend_usd_total
        );

        // Both real durations are reported independently. One snapshot is only an anchor, so the
        // API must return unknown rather than inventing the former $1500 weekly prior.
        let primary = home
            .capacities
            .iter()
            .find(|capacity| capacity.slot == "primary")
            .unwrap();
        assert_eq!(primary.window_minutes, Some(300));
        assert_eq!(primary.source, "unknown");
        assert_eq!(primary.cap_usd, None);
        assert_eq!(primary.remaining_usd, None);
        let weekly = home
            .capacities
            .iter()
            .find(|capacity| capacity.slot == "secondary")
            .unwrap();
        assert_eq!(weekly.window_minutes, Some(10_080));
        assert_eq!(weekly.source, "unknown");
        assert_eq!(weekly.cap_usd, None);
        assert_eq!(weekly.remaining_usd, None);
        assert!(!home.calibration_persistence_ok);
    }

    #[tokio::test]
    async fn closed_downstream_update_channel_still_drains_authoritative_usage() {
        let (gateway, _workspace) = fake_gateway("text");
        let (updates_tx, updates_rx) = mpsc::channel(1);
        drop(updates_rx);

        let result = gateway
            .run_turn(turn_request(test_model()), Some(updates_tx), None)
            .await
            .unwrap();

        assert_eq!(result.output[0]["content"][0]["text"], "hello");
        assert_eq!(result.usage.input_tokens, 101);
        assert_eq!(result.usage.output_tokens, 23);
        assert_eq!(result.usage.total_tokens, 124);
    }

    #[tokio::test]
    async fn customer_function_call_is_returned_and_native_turn_is_interrupted() {
        let (gateway, workspace) = fake_gateway("tool");
        let result = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap();

        assert_eq!(result.output.len(), 1);
        assert_eq!(result.output[0]["type"], "function_call");
        assert!(result.output[0]["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("fc_")));
        assert_eq!(result.output[0]["call_id"], "call-1");
        assert_eq!(result.output[0]["name"], "get_weather");
        assert_eq!(result.output[0]["arguments"], r#"{"city":"Tbilisi"}"#);
        assert_eq!(result.usage.input_tokens, 19);
        assert_eq!(result.usage.cached_input_tokens, 3);
        assert_eq!(result.usage.output_tokens, 8);
        assert_eq!(result.usage.reasoning_output_tokens, 2);
        assert_eq!(result.usage.total_tokens, 27);
        let requests = logged_requests(&workspace.log);
        assert!(requests
            .iter()
            .any(|request| request["method"] == "turn/interrupt"));
        assert!(!requests.iter().any(|request| request["id"] == "callback-1"));
    }

    #[tokio::test]
    async fn parallel_customer_function_calls_are_returned_once_each() {
        let (gateway, workspace) = fake_gateway("parallel_tool");
        let result = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap();

        assert_eq!(result.output.len(), 2);
        assert_eq!(result.output[0]["call_id"], "call-1");
        assert_eq!(result.output[1]["call_id"], "call-2");
        assert_eq!(result.output[0]["arguments"], r#"{"city":"Tbilisi"}"#);
        assert_eq!(result.output[1]["arguments"], r#"{"city":"Paris"}"#);
        assert_eq!(result.usage.input_tokens, 23);
        assert_eq!(result.usage.output_tokens, 14);
        assert_eq!(result.usage.total_tokens, 37);
        let requests = logged_requests(&workspace.log);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "turn/interrupt")
                .count(),
            1
        );
        assert!(!requests
            .iter()
            .any(|request| request["id"] == "callback-1" || request["id"] == "callback-2"));
    }

    #[tokio::test]
    async fn customer_custom_tool_call_is_returned_as_raw_input() {
        let (gateway, workspace) = fake_gateway("custom_tool");
        let mut request = turn_request(test_model());
        request.dynamic_tools = vec![json!({
            "type": "custom",
            "name": "exec",
            "description": "Execute source",
            "format": {
                "type": "grammar",
                "syntax": "lark",
                "definition": "start: /[\\s\\S]+/"
            }
        })];
        let result = gateway.run_turn(request, None, None).await.unwrap();

        assert_eq!(result.output.len(), 1);
        assert_eq!(result.output[0]["type"], "custom_tool_call");
        assert!(result.output[0]["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("ctc_")));
        assert_eq!(result.output[0]["call_id"], "call-custom-1");
        assert_eq!(result.output[0]["name"], "exec");
        assert_eq!(result.output[0]["input"], "text('ok')");
        assert_eq!(result.usage.input_tokens, 29);
        assert_eq!(result.usage.output_tokens, 9);
        assert_eq!(result.usage.total_tokens, 38);
        let requests = logged_requests(&workspace.log);
        assert!(requests
            .iter()
            .any(|request| request["method"] == "turn/interrupt"));
        assert!(!requests.iter().any(|request| request["id"] == "callback-1"));
    }

    #[test]
    fn callback_fallback_distinguishes_namespaced_function_and_custom_calls() {
        let custom_tool_names = HashSet::from(["exec".to_string()]);
        let function = callback_tool_call_item(
            &json!({
                "tool": "list_agents",
                "namespace": "collaboration",
                "arguments": {"path_prefix": "/root"}
            }),
            "call-function",
            &custom_tool_names,
        );
        assert_eq!(function["type"], "function_call");
        assert_eq!(function["namespace"], "collaboration");
        assert_eq!(function["arguments"], r#"{"path_prefix":"/root"}"#);

        let custom = callback_tool_call_item(
            &json!({"tool": "exec", "arguments": "text('ok')"}),
            "call-custom",
            &custom_tool_names,
        );
        assert_eq!(custom["type"], "custom_tool_call");
        assert_eq!(custom["input"], "text('ok')");

        let string_function = callback_tool_call_item(
            &json!({"tool": "echo", "arguments": "plain text"}),
            "call-string-function",
            &custom_tool_names,
        );
        assert_eq!(string_function["type"], "function_call");
        assert_eq!(string_function["arguments"], r#""plain text""#);
    }

    #[tokio::test]
    async fn reasoning_summary_streams_but_raw_chain_of_thought_does_not() {
        let (gateway, _workspace) = fake_gateway("reasoning");
        let (updates_tx, mut updates_rx) = mpsc::channel(8);
        let result = gateway
            .run_turn(turn_request(test_model()), Some(updates_tx), None)
            .await
            .unwrap();

        assert!(matches!(
            updates_rx.recv().await,
            Some(TurnUpdate::ReasoningSummaryPartAdded {
                item_id,
                summary_index: 0
            }) if item_id == "rs-1"
        ));
        assert!(matches!(
            updates_rx.recv().await,
            Some(TurnUpdate::ReasoningSummaryDelta {
                item_id,
                summary_index: 0,
                delta
            }) if item_id == "rs-1" && delta == "Checked"
        ));
        assert!(matches!(
            updates_rx.recv().await,
            Some(TurnUpdate::RawItem(item)) if item["id"] == "rs-1"
        ));
        assert!(updates_rx.recv().await.is_none());
        assert_eq!(
            result.output[0]["content"][0]["text"],
            "private chain of thought"
        );
        assert_eq!(result.usage.reasoning_output_tokens, 4);
    }

    #[tokio::test]
    async fn dropping_an_inflight_turn_sends_interrupt() {
        let (gateway, workspace) = fake_gateway("hang");
        let (updates_tx, mut updates_rx) = mpsc::channel(8);
        let run_gateway = gateway.clone();
        let task = tokio::spawn(async move {
            run_gateway
                .run_turn(turn_request(test_model()), Some(updates_tx), None)
                .await
        });
        assert!(matches!(
            updates_rx.recv().await,
            Some(TurnUpdate::TextDelta { delta, .. }) if delta == "ready"
        ));

        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        wait_for_method(&workspace.log, "turn/interrupt").await;
    }

    #[tokio::test]
    async fn cancelled_startup_is_reaped_before_the_next_generation_starts() {
        let (gateway, workspace) = fake_gateway("startup_cancel_once");
        let home = gateway.homes().await.into_iter().next().unwrap();
        let first_home = home.clone();
        let first = tokio::spawn(async move { first_home.process().await });
        let pids = workspace.root.join("generation-pids");
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if pids.is_file() {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the first fake generation did not start");
        let first_pid = std::fs::read_to_string(&pids)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_string();
        first.abort();
        assert!(matches!(first.await, Err(error) if error.is_cancelled()));

        tokio::time::timeout(std::time::Duration::from_secs(3), home.process())
            .await
            .unwrap()
            .unwrap();
        assert!(
            !Command::new("/bin/kill")
                .args(["-0", &first_pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success()),
            "the cancelled generation was still alive when its replacement became ready"
        );
        gateway.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_reaps_codex_descendants_before_returning() {
        let (gateway, workspace) = fake_gateway("descendant");
        let home = gateway.homes().await.into_iter().next().unwrap();
        home.process().await.unwrap();
        let helper_path = workspace.root.join("helper-pid");
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !helper_path.is_file() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the fake Codex helper did not start");
        let helper_pid = std::fs::read_to_string(helper_path)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        assert!(process_exists(helper_pid));

        gateway.shutdown().await;

        assert!(!process_exists(helper_pid));
    }

    #[tokio::test]
    async fn shutdown_deadline_cancels_a_residual_tracked_turn() {
        let (gateway, _workspace) = fake_gateway("hang");
        let permit = gateway.track_background_task().unwrap();
        let (updates_tx, mut updates_rx) = mpsc::channel(8);
        let run_gateway = gateway.clone();
        let turn = tokio::spawn(async move {
            let _permit = permit;
            run_gateway
                .run_turn(turn_request(test_model()), Some(updates_tx), None)
                .await
        });
        assert!(matches!(
            updates_rx.recv().await,
            Some(TurnUpdate::TextDelta { delta, .. }) if delta == "ready"
        ));

        gateway
            .shutdown_until(Some(
                tokio::time::Instant::now() + std::time::Duration::from_millis(50),
            ))
            .await;

        assert!(matches!(turn.await.unwrap(), Err(ProcessError::Closed)));
    }

    #[tokio::test]
    async fn shutdown_deadline_is_hard_even_if_a_tracked_task_never_exits() {
        let (gateway, _workspace) = fake_gateway("text");
        let permit = gateway.track_background_task().unwrap();

        tokio::time::timeout(
            std::time::Duration::from_millis(500),
            gateway.shutdown_until(Some(
                tokio::time::Instant::now() + std::time::Duration::from_millis(25),
            )),
        )
        .await
        .expect("shutdown waited without a bound after its advertised deadline");

        drop(permit);
    }

    #[tokio::test]
    async fn cancelled_invalidation_cannot_overlap_process_generations() {
        let (gateway, workspace) = fake_gateway("invalidate_cancel");
        let home = gateway.homes().await.into_iter().next().unwrap();
        let process = home.process().await.unwrap();
        let pids = workspace.root.join("generation-pids");
        let first_pid = std::fs::read_to_string(&pids)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse::<u32>()
            .unwrap();

        let invalidating_home = home.clone();
        let invalidating_process = process.clone();
        let invalidation = tokio::spawn(async move {
            invalidating_home.invalidate(&invalidating_process).await;
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if !process.is_live() {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("invalidation did not begin shutting down the old generation");
        invalidation.abort();
        let _ = invalidation.await;

        tokio::time::timeout(std::time::Duration::from_secs(3), home.process())
            .await
            .expect("replacement remained fenced after the detached reaper completed")
            .unwrap();
        assert!(
            !process_exists(first_pid),
            "a replacement became ready before the cancelled invalidation reaped its predecessor"
        );
        assert_eq!(
            std::fs::read_to_string(&pids).unwrap().lines().count(),
            2,
            "exactly one replacement generation must be started"
        );

        gateway.shutdown().await;
    }

    #[tokio::test]
    async fn timed_out_version_probe_is_killed_and_reaped() {
        let (gateway, workspace) = fake_gateway("version_timeout");
        let home = gateway.homes().await.into_iter().next().unwrap();
        assert!(matches!(
            home.process().await,
            Err(ProcessError::Timeout("version check"))
        ));
        let processes = Command::new("ps")
            .args(["-axo", "command="])
            .output()
            .unwrap();
        let processes = String::from_utf8_lossy(&processes.stdout);
        assert!(
            !processes.contains(workspace.root.join("fake-codex").to_str().unwrap()),
            "timed-out codex --version process was not reaped"
        );
    }

    #[tokio::test]
    async fn cancelled_version_probe_kills_its_process_group() {
        let (gateway, workspace) = fake_gateway("version_cancel");
        let home = gateway.homes().await.into_iter().next().unwrap();
        let process_home = home.clone();
        let startup = tokio::spawn(async move { process_home.process().await });
        let helper_path = workspace.root.join("version-helper-pid");
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !helper_path.is_file() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the version helper did not start");
        let helper_pid = std::fs::read_to_string(helper_path)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        assert!(process_exists(helper_pid));

        startup.abort();
        assert!(matches!(startup.await, Err(error) if error.is_cancelled()));
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while process_exists(helper_pid) {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the cancelled version helper remained alive");
        gateway.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_waits_for_tracked_stream_tasks_and_blocks_new_ones() {
        let (gateway, _workspace) = fake_gateway("text");
        let permit = gateway.track_background_task().unwrap();
        let shutdown_gateway = gateway.clone();
        let shutdown = tokio::spawn(async move { shutdown_gateway.shutdown().await });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(!shutdown.is_finished());
        drop(permit);
        tokio::time::timeout(std::time::Duration::from_secs(1), shutdown)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            gateway.track_background_task(),
            Err(ProcessError::Closed)
        ));
    }

    #[tokio::test]
    async fn structured_usage_limit_is_classified_without_exposing_upstream_text() {
        let (gateway, _workspace) = fake_gateway("usage_limit");
        let error = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ProcessError::UsageLimitExceeded {
                retry_after: Some(604_800)
            }
        ));
        assert!(!error.to_string().contains("sensitive upstream diagnostic"));
    }

    /// A purchased account must start serving without a restart, a config edit or root — that is
    /// the whole point of scanning a directory instead of listing homes in the environment.
    #[tokio::test]
    async fn an_account_finished_after_startup_joins_the_live_pool() {
        let (gateway, workspace, _logs) = fake_pool_gateway(&["text"]);
        gateway.preflight().await.unwrap();
        assert_eq!(gateway.operational_status().await.homes.len(), 1);

        // The authbot creates the directory first and authenticates it a moment later. While the
        // device flow is outstanding the home must stay invisible, or requests would be routed to
        // an account nobody has finished buying.
        let pool_dir = workspace.root.join("pool");
        std::fs::create_dir_all(&pool_dir).unwrap();
        let bought = pool_dir.join("acct-new");
        std::fs::create_dir_all(&bought).unwrap();
        std::fs::write(bought.join("mode"), b"text").unwrap();
        gateway.rediscover().await;
        assert_eq!(
            gateway.operational_status().await.homes.len(),
            1,
            "a half-finished purchase must not join the pool"
        );

        std::fs::write(bought.join("auth.json"), b"{}").unwrap();
        gateway.rediscover().await;
        let status = gateway.operational_status().await;
        assert_eq!(
            status.homes.len(),
            2,
            "the finished account joined the pool"
        );

        // And it can actually serve: the new home starts its own attested child on demand.
        let result = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap();
        assert_eq!(result.usage.input_tokens, 101);
    }

    /// Removing a directory retires the account; an unreadable scan must never empty the pool.
    #[tokio::test]
    async fn a_removed_account_leaves_the_pool_but_a_failed_scan_never_empties_it() {
        let (gateway, workspace, _logs) = fake_pool_gateway(&["text"]);
        let pool_dir = workspace.root.join("pool");
        std::fs::create_dir_all(&pool_dir).unwrap();
        let bought = pool_dir.join("acct-temp");
        std::fs::create_dir_all(&bought).unwrap();
        std::fs::write(bought.join("mode"), b"text").unwrap();
        std::fs::write(bought.join("auth.json"), b"{}").unwrap();
        gateway.rediscover().await;
        assert_eq!(gateway.operational_status().await.homes.len(), 2);
        let bought_home = gateway
            .homes()
            .await
            .into_iter()
            .find(|home| home.path() == bought.to_str().unwrap())
            .unwrap();
        bought_home.process().await.unwrap();

        std::fs::remove_dir_all(&bought).unwrap();
        gateway.rediscover().await;
        assert_eq!(gateway.operational_status().await.homes.len(), 1);
        assert!(bought_home.live_process().await.is_none());

        // The explicit home has no directory to scan away, so the pool keeps serving from it.
        std::fs::remove_dir_all(&pool_dir).unwrap();
        gateway.rediscover().await;
        assert_eq!(gateway.operational_status().await.homes.len(), 1);
    }

    #[tokio::test]
    async fn replacing_a_home_directory_retires_the_old_child_before_publication() {
        let (gateway, workspace, _logs) = fake_pool_gateway(&["text"]);
        let pool_dir = workspace.root.join("pool");
        std::fs::create_dir_all(&pool_dir).unwrap();
        let bought = pool_dir.join("acct-replaced");
        std::fs::create_dir(&bought).unwrap();
        std::fs::write(bought.join("mode"), b"text").unwrap();
        std::fs::write(bought.join("auth.json"), b"{}").unwrap();
        gateway.rediscover().await;
        let old = gateway
            .homes()
            .await
            .into_iter()
            .find(|home| home.path() == bought.to_str().unwrap())
            .unwrap();
        old.process().await.unwrap();

        std::fs::remove_dir_all(&bought).unwrap();
        std::fs::create_dir(&bought).unwrap();
        std::fs::write(bought.join("mode"), b"text").unwrap();
        std::fs::write(bought.join("auth.json"), b"{}").unwrap();
        assert!(!old.identity_is_current());
        gateway.rediscover().await;
        assert!(old.live_process().await.is_none());
        let replacement = gateway
            .homes()
            .await
            .into_iter()
            .find(|home| home.path() == bought.to_str().unwrap())
            .unwrap();
        assert!(!Arc::ptr_eq(&old, &replacement));
        replacement.process().await.unwrap();
        gateway.shutdown().await;
    }

    #[tokio::test]
    async fn rediscovery_rescans_proxy_metadata_after_a_blocked_retirement() {
        let (gateway, workspace, _logs) = fake_pool_gateway(&["text"]);
        let pool_dir = workspace.root.join("pool");
        std::fs::create_dir_all(&pool_dir).unwrap();
        let bought = pool_dir.join("acct-proxy-change");
        std::fs::create_dir(&bought).unwrap();
        std::fs::write(bought.join("mode"), b"text").unwrap();
        std::fs::write(bought.join("auth.json"), b"{}").unwrap();
        let proxy_file = bought.join("proxy.url");
        std::fs::write(&proxy_file, b"http://first.example:8080").unwrap();
        std::fs::set_permissions(&proxy_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        gateway.rediscover().await;
        let old = gateway
            .homes()
            .await
            .into_iter()
            .find(|home| home.path() == bought.to_str().unwrap())
            .unwrap();
        let slot = old.acquire_turn().unwrap();

        std::fs::write(&proxy_file, b"http://second.example:8080").unwrap();
        let reconcile_gateway = gateway.clone();
        let reconcile = tokio::spawn(async move { reconcile_gateway.rediscover().await });
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while !old.retired.load(Ordering::Acquire) {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the old home did not begin retirement");
        std::fs::write(&proxy_file, b"http://third.example:8080").unwrap();
        drop(slot);
        reconcile.await.unwrap();

        let replacement = gateway
            .homes()
            .await
            .into_iter()
            .find(|home| home.path() == bought.to_str().unwrap())
            .unwrap();
        assert_eq!(
            replacement.spec.proxy.as_deref(),
            Some("http://third.example:8080")
        );
        gateway.shutdown().await;
    }

    #[tokio::test]
    async fn a_home_at_its_usage_limit_rotates_to_a_healthy_home() {
        let (gateway, _workspace, logs) = fake_pool_gateway(&["usage_limit", "text"]);
        let result = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap();
        assert_eq!(result.usage.input_tokens, 101);
        assert!(served_turn(&logs[0]), "the limited home was tried first");
        assert!(served_turn(&logs[1]), "the healthy home served the request");
        // The limited home leaves the rotation until its window resets, exactly like a cooling
        // Claude subscription.
        let status = gateway.operational_status().await;
        assert!(status.homes[0].cooling_until > pool::now());
        assert_eq!(status.homes[1].cooling_until, 0);
        assert_eq!(status.available, 1);
    }

    #[tokio::test]
    async fn every_home_limited_returns_one_retryable_wait_not_a_provider_error() {
        let (gateway, _workspace, _logs) = fake_pool_gateway(&["usage_limit", "usage_limit"]);
        let error = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap_err();
        // The client is told to retry after the soonest reset, never that an account was banned.
        assert!(matches!(
            error,
            ProcessError::UsageLimitExceeded {
                retry_after: Some(seconds)
            } if seconds > 0
        ));
        let status = gateway.operational_status().await;
        assert_eq!(status.available, 0);
        assert!(status.soonest_ready.is_some());
    }

    #[tokio::test]
    async fn a_full_window_leaves_rotation_without_an_explicit_reached_flag() {
        let (gateway, _workspace, logs) = fake_pool_gateway(&["full_window", "text"]);
        // Preflight publishes the 100% snapshot; the provider reports no reached verdict at all.
        gateway.preflight().await.unwrap();
        gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap();
        assert!(
            !served_turn(&logs[0]),
            "an exhausted subscription must not be tried"
        );
        assert!(served_turn(&logs[1]), "the healthy home served the request");
        let status = gateway.operational_status().await;
        assert!(status.homes[0].limit_reached, "panel must not read active");
        assert!(!status.homes[1].limit_reached);
        assert_eq!(status.available, 1);
    }

    #[tokio::test]
    async fn every_home_at_a_full_window_returns_a_retryable_wait() {
        let (gateway, _workspace, logs) = fake_pool_gateway(&["full_window", "full_window"]);
        gateway.preflight().await.unwrap();
        let error = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap_err();
        // Fail closed with the window reset, never a turn burned on a subscription that is spent.
        assert!(matches!(
            error,
            ProcessError::UsageLimitExceeded {
                retry_after: Some(seconds)
            } if seconds > 0
        ));
        assert!(logs.iter().all(|log| !served_turn(log)));
        let status = gateway.operational_status().await;
        assert_eq!(status.available, 0);
        assert!(status.soonest_ready.is_some());
    }

    #[tokio::test]
    async fn high_observed_percent_without_reached_flag_remains_routable() {
        let (gateway, _workspace, logs) = fake_pool_gateway(&["near_limit", "text"]);
        // Preflight populates the 97% observational snapshot without a provider reached flag.
        gateway.preflight().await.unwrap();
        gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap();
        assert!(served_turn(&logs[0]), "97% is not an admission restriction");
        let status = gateway.operational_status().await;
        assert_eq!(status.available, 2);
    }

    #[tokio::test]
    async fn equal_load_independent_turns_fan_out_across_homes() {
        let (gateway, _workspace, logs) = fake_pool_gateway(&["text", "text"]);

        for _ in 0..4 {
            gateway
                .run_turn(turn_request(test_model()), None, None)
                .await
                .unwrap();
        }

        let turn_counts = logs
            .iter()
            .map(|log| {
                logged_requests(log)
                    .iter()
                    .filter(|request| request["method"] == "turn/start")
                    .count()
            })
            .collect::<Vec<_>>();
        assert_eq!(turn_counts, vec![2, 2]);
    }

    #[tokio::test]
    async fn one_home_multiplexes_independent_threads_concurrently() {
        let (gateway, workspace) = fake_gateway_with_request_timeout("concurrent_threads", 200);
        gateway.preflight().await.unwrap();

        let first_gateway = gateway.clone();
        let first = tokio::spawn(async move {
            first_gateway
                .run_turn(turn_request(test_model()), None, None)
                .await
        });
        wait_for_path(&workspace.root.join("turn-started-1")).await;

        let second_gateway = gateway.clone();
        let second = tokio::spawn(async move {
            second_gateway
                .run_turn(turn_request(test_model()), None, None)
                .await
        });
        wait_for_path(&workspace.root.join("turn-started-2")).await;

        let status = gateway.operational_status().await;
        assert_eq!(status.homes.len(), 1);
        assert_eq!(status.homes[0].inflight, 2);

        std::fs::write(workspace.root.join("release-turn-1"), []).unwrap();
        let first = first.await.unwrap().unwrap();
        assert_eq!(first.output[0]["content"][0]["text"], "hello-1");

        std::fs::write(workspace.root.join("release-turn-2"), []).unwrap();
        let second = second.await.unwrap().unwrap();
        assert_eq!(second.output[0]["content"][0]["text"], "hello-2");
        assert_eq!(first.usage.input_tokens, 101);
        assert_eq!(second.usage.input_tokens, 101);
        assert_eq!(
            logged_requests(&workspace.log)
                .iter()
                .filter(|request| request["method"] == "thread/start")
                .count(),
            2
        );
        assert!(gateway.operational_status().await.process_live);
    }

    #[tokio::test]
    async fn streaming_preflight_does_not_consume_the_rotation_slot() {
        let (gateway, _workspace, logs) = fake_pool_gateway(&["text", "text"]);

        for _ in 0..4 {
            gateway.preflight_capacity().await.unwrap();
            gateway
                .run_turn(turn_request(test_model()), None, None)
                .await
                .unwrap();
        }

        let turn_counts = logs
            .iter()
            .map(|log| {
                logged_requests(log)
                    .iter()
                    .filter(|request| request["method"] == "turn/start")
                    .count()
            })
            .collect::<Vec<_>>();
        assert_eq!(turn_counts, vec![2, 2]);
    }

    #[tokio::test]
    async fn a_dead_child_is_retried_once_on_another_home() {
        let (gateway, _workspace, logs) = fake_pool_gateway(&["die", "text"]);
        let result = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap();
        assert_eq!(result.usage.input_tokens, 101);
        assert!(served_turn(&logs[0]));
        assert!(served_turn(&logs[1]));
    }

    #[tokio::test]
    async fn a_stopped_child_is_not_reported_as_available_capacity() {
        let (gateway, _workspace) = fake_gateway("die");

        assert!(gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .is_err());

        let status = gateway.operational_status().await;
        assert!(!status.process_live);
        assert_eq!(status.available, 0);
        assert!(!status.homes[0].process_live);
    }

    #[tokio::test]
    async fn an_observational_rate_limit_timeout_keeps_the_authenticated_child_live() {
        let (gateway, _workspace) = fake_gateway_with_request_timeout("rate_limit_timeout", 200);

        let result = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap();

        assert_eq!(result.usage.input_tokens, 101);
        assert!(gateway.operational_status().await.process_live);
    }

    #[tokio::test]
    async fn a_turn_completion_timeout_interrupts_only_that_turn_and_keeps_the_child_live() {
        // The child accepts the turn but never sends turn/completed, so the event loop hits the
        // turn deadline. A single stuck turn must not recycle the shared child (which would fail
        // every sibling turn); it is interrupted while the child stays live for other turns.
        // turn_timeout is forced to 200ms for this mode so the deadline fires fast; the request
        // timeout stays generous so the follow-up interrupt RPC has room to be acked.
        let (gateway, workspace) =
            fake_gateway_with_request_timeout("turn_completion_timeout", 2_000);
        let home = gateway.homes().await.into_iter().next().unwrap();
        let process = home.process().await.unwrap();

        let error = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap_err();

        assert!(matches!(error, ProcessError::Timeout("turn completion")));
        assert!(
            process.is_live(),
            "the shared child must survive one turn timeout"
        );
        assert!(gateway.operational_status().await.process_live);
        wait_for_method(&workspace.log, "turn/interrupt").await;
    }

    #[tokio::test]
    async fn upstream_silence_fails_the_turn_long_before_the_total_deadline() {
        // The child accepts the turn and then says nothing at all. Waiting out the full turn
        // deadline would hold the client for as long as a reasoning model is allowed to think —
        // ten minutes in production — even though the home stopped answering in the first second.
        // Silence is bounded separately so the failure surfaces promptly, while the total deadline
        // stays generous enough never to cut short a turn that is genuinely still working.
        let (gateway, workspace) = fake_gateway_with_request_timeout("turn_silence_timeout", 2_000);
        let home = gateway.homes().await.into_iter().next().unwrap();
        let process = home.process().await.unwrap();

        let started = std::time::Instant::now();
        let error = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap_err();
        let elapsed = started.elapsed();

        assert!(matches!(error, ProcessError::Timeout("turn silence")));
        assert!(
            elapsed < std::time::Duration::from_millis(1_500),
            "silence must not wait out the total deadline (took {elapsed:?})"
        );
        // Same invariant as any other deadline: one stuck turn never recycles the shared child,
        // because that would fail every sibling turn multiplexed over it.
        assert!(process.is_live());
        wait_for_method(&workspace.log, "turn/interrupt").await;
        // And the deadline is now recorded, so a home that keeps doing this leaves rotation.
        assert!(home.health().deadline_streak >= 1);
    }

    #[tokio::test]
    async fn a_thread_start_timeout_does_not_destroy_the_shared_child() {
        let (gateway, workspace) =
            fake_gateway_with_request_timeout("thread_start_timeout_then_recover", 200);
        let home = gateway.homes().await.into_iter().next().unwrap();
        let process = home.process().await.unwrap();

        let error = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap_err();

        assert!(matches!(error, ProcessError::Timeout("thread/start")));
        assert!(process.is_live());
        assert!(gateway.operational_status().await.process_live);

        let result = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap();
        assert_eq!(result.usage.input_tokens, 101);
        assert!(Arc::ptr_eq(&process, &home.process().await.unwrap()));
        assert_eq!(
            logged_requests(&workspace.log)
                .iter()
                .filter(|request| request["method"] == "thread/start")
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn a_turn_start_timeout_does_not_destroy_the_shared_child() {
        let (gateway, workspace) =
            fake_gateway_with_request_timeout("turn_start_timeout_then_recover", 200);
        let home = gateway.homes().await.into_iter().next().unwrap();
        let process = home.process().await.unwrap();

        let error = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap_err();
        assert!(matches!(error, ProcessError::Timeout("turn/start")));
        assert!(process.is_live());
        assert!(gateway.operational_status().await.homes[0].process_live);

        let result = gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap();
        assert_eq!(result.usage.input_tokens, 101);
        assert!(Arc::ptr_eq(&process, &home.process().await.unwrap()));
        assert_eq!(
            logged_requests(&workspace.log)
                .iter()
                .filter(|request| request["method"] == "turn/start")
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn a_transport_fault_does_not_quarantine_the_subscription() {
        let (gateway, _workspace, _logs) = fake_pool_gateway(&["die", "text"]);
        gateway
            .run_turn(turn_request(test_model()), None, None)
            .await
            .unwrap();
        let status = gateway.operational_status().await;
        // The child is the fault, not the account: cool briefly, never for the auth quarantine.
        let cooling_for = status.homes[0].cooling_until - pool::now();
        assert!(
            cooling_for > 0 && cooling_for <= 10,
            "transport cooling was {cooling_for}s"
        );
        assert!(status.homes[0].auth_ok);
    }

    #[tokio::test]
    async fn a_failure_after_the_first_delta_is_never_retried_on_another_home() {
        let (gateway, _workspace, logs) = fake_pool_gateway(&["stream_then_die", "text"]);
        let (updates_tx, mut updates_rx) = mpsc::channel(8);
        let run = gateway.run_turn(turn_request(test_model()), Some(updates_tx), None);
        let collect = async {
            let mut seen = Vec::new();
            while let Some(update) = updates_rx.recv().await {
                seen.push(update);
            }
            seen
        };
        let (result, updates) = tokio::join!(run, collect);
        assert!(result.is_err());
        assert_eq!(updates.len(), 1, "one delta reached the public stream");
        assert!(
            !served_turn(&logs[1]),
            "a second attempt would interleave with output the client already has"
        );
    }

    #[tokio::test]
    async fn codex_infer_pins_a_conversation_to_one_home_and_isolates_tenants() {
        // Store-level proof that the shared AffinityStore treats a Codex request exactly like a
        // Claude one: the same tenant + conversation resolves to the same home, and another tenant
        // with byte-identical text is a distinct lineage that never inherits the pin.
        use crate::affinity::AffinityStore;
        use axum::http::HeaderMap;
        let store = AffinityStore::new(None, Some("secret"), 600, 300, 50).unwrap();
        let items = vec![json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "hi"}]
        })];
        let input = store
            .infer_codex(
                "acct-a",
                &HeaderMap::new(),
                "gpt-5.6",
                Some("sys"),
                &[],
                &items,
                None,
            )
            .unwrap();
        assert!(store.resolve(&input).await.is_none());
        let resolution = store.claim(&input, "home-abc").await;
        assert_eq!(resolution.home, "home-abc");
        assert_eq!(store.resolve(&input).await.unwrap().home, "home-abc");

        let other_tenant = store
            .infer_codex(
                "acct-b",
                &HeaderMap::new(),
                "gpt-5.6",
                Some("sys"),
                &[],
                &items,
                None,
            )
            .unwrap();
        assert!(
            store.resolve(&other_tenant).await.is_none(),
            "a different tenant must not inherit the pinned home"
        );
    }

    #[tokio::test]
    async fn shared_cache_root_immediately_seeds_two_homes_then_reuses_warmth() {
        use crate::affinity::AffinityStore;
        use axum::http::HeaderMap;
        let (gateway, _workspace, logs) = fake_pool_gateway(&["text", "text"]);
        gateway.preflight().await.unwrap();
        let store = Arc::new(AffinityStore::new(None, Some("secret"), 600, 300, 50).unwrap());
        let instructions = "stable shared OpenAI instruction root ".repeat(192);

        for message in ["independent one", "independent two", "independent three"] {
            let items = vec![json!({
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": message}]
            })];
            let input = store
                .infer_codex(
                    "tenant",
                    &HeaderMap::new(),
                    "gpt-5.6",
                    Some(&instructions),
                    &[],
                    &items,
                    None,
                )
                .unwrap();
            let resolution = store.resolve(&input).await;
            assert!(
                resolution.is_none(),
                "independent transcripts must not inherit a conversation pin"
            );
            let warm = store.warm_homes(&input).await;
            let routing = TurnRouting::new(store.clone(), input, resolution, warm);
            let mut request = turn_request(test_model());
            request.developer_instructions = Some(instructions.clone());
            request.dynamic_tools.clear();
            gateway
                .run_turn(request, None, Some(routing))
                .await
                .unwrap();
        }

        let turn_counts = logs
            .iter()
            .map(|log| {
                logged_requests(log)
                    .iter()
                    .filter(|request| request["method"] == "turn/start")
                    .count()
            })
            .collect::<Vec<_>>();
        assert_eq!(turn_counts.iter().sum::<usize>(), 3);
        assert!(
            turn_counts.iter().all(|count| *count > 0),
            "the second independent session must seed the other competitive home"
        );
        let stats = store.stats();
        assert_eq!(stats.cache_root_cold_placements, 2);
        assert_eq!(stats.cache_root_warm_placements, 1);
    }

    #[tokio::test]
    async fn a_pinned_conversation_routes_to_its_home_over_the_least_loaded_one() {
        // Cache-first routing: a conversation pinned to home #1 must be served there even though the
        // least-loaded tie-break would otherwise pick home #0 (lower order). This is the whole point
        // of the affinity layer — reuse the warm home's prompt cache instead of spreading by load.
        use crate::affinity::AffinityStore;
        use axum::http::HeaderMap;
        let (gateway, _workspace, logs) = fake_pool_gateway(&["text", "text"]);
        gateway.preflight().await.unwrap();
        let status = gateway.operational_status().await;
        let home1_id = status.homes[1].id.clone();

        let store = Arc::new(AffinityStore::new(None, Some("secret"), 600, 300, 50).unwrap());
        let items = vec![json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "hello"}]
        })];
        let input = store
            .infer_codex(
                "tenant",
                &HeaderMap::new(),
                "gpt-5.6",
                None,
                &[],
                &items,
                None,
            )
            .unwrap();
        let resolution = store.claim(&input, &home1_id).await;
        let routing = TurnRouting::new(store.clone(), input.clone(), Some(resolution), Vec::new());
        let expected_prompt_cache_key = routing.prompt_cache_key();

        gateway
            .run_turn(turn_request(test_model()), None, Some(routing))
            .await
            .unwrap();

        assert!(served_turn(&logs[1]), "the pinned home served the turn");
        assert!(
            !served_turn(&logs[0]),
            "the least-loaded home was correctly overridden by the conversation pin"
        );
        let requests = logged_requests(&logs[1]);
        let thread_start = requests
            .iter()
            .find(|request| request["method"] == "thread/start")
            .unwrap();
        assert_eq!(
            thread_start["params"]["promptCacheKey"],
            expected_prompt_cache_key
        );
        assert_eq!(expected_prompt_cache_key.len(), 129);
        assert!(!expected_prompt_cache_key.contains("tenant"));
    }
}
