//! One ephemeral Codex thread per public API request.
//!
//! Request-local instructions and exact injected Responses items preserve OpenAI's stateless
//! semantics. A Codex thread is never reused as hidden conversational state.

use super::{
    new_id, AppServerEvent, CodexGateway, CodexHome, CodexModel, CodexProcess, HomeSelection,
    ProcessError,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, OwnedSemaphorePermit};

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
    ) -> Result<CodexTurnResult, ProcessError> {
        let emitted = Arc::new(AtomicBool::new(false));
        let mut tried: Vec<String> = Vec::new();
        let mut transport_retries_left = 1usize;
        let mut last_error: Option<ProcessError> = None;
        loop {
            let (home, permit) = match self.select_home(&tried).await {
                HomeSelection::Ready(home, permit) => (home, permit),
                HomeSelection::Busy => {
                    return Err(last_error.unwrap_or(ProcessError::Busy));
                }
                HomeSelection::Unavailable { ready_at } => {
                    return Err(last_error.unwrap_or_else(|| ProcessError::UsageLimitExceeded {
                        retry_after: retry_after_from(ready_at),
                    }));
                }
            };
            tried.push(home.id().to_string());
            let result = home
                .run_turn(request.clone(), updates.clone(), &emitted, permit)
                .await;
            let error = match result {
                Ok(result) => {
                    home.mark_healthy();
                    return Ok(result);
                }
                Err(error) => error,
            };
            home.note_turn_error(&error);
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
        _turn_permit: OwnedSemaphorePermit,
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
        let mut events = process.register_turn(&thread_id).await?;
        let mut registration_guard = TurnRegistrationGuard::new(process.clone(), thread_id.clone());

        let result = self
            .run_registered_turn(
                process.clone(),
                &thread_id,
                &mut events.receiver,
                request,
                updates,
                emitted,
            )
            .await;
        process.unregister_turn(&thread_id).await;
        registration_guard.disarm();
        if matches!(
            result,
            Err(ProcessError::Closed
                | ProcessError::Protocol(_)
                | ProcessError::AuthenticationRequired)
        ) {
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
        let mut last_error = ProcessError::Closed;
        for attempt in 0..2 {
            let process = self.process().await?;
            match process.request("thread/start", params.clone()).await {
                Ok(response) => return Ok((process, response)),
                Err(error @ (ProcessError::Closed | ProcessError::Protocol(_))) if attempt == 0 => {
                    self.invalidate(&process).await;
                    last_error = error;
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error)
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_registered_turn(
        &self,
        process: Arc<CodexProcess>,
        thread_id: &str,
        receiver: &mut mpsc::Receiver<AppServerEvent>,
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
        let event_loop = async {
            let mut output = Vec::<Value>::new();
            let mut usage = CodexUsage::default();
            let mut saw_raw_usage = false;
            let mut saw_raw_response_completed = false;
            let mut terminal_tool_call = false;
            let mut interrupt_sent = false;
            let mut completed = false;

            while let Some(event) = receiver.recv().await {
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
                                completed = true;
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
                    AppServerEvent::Closed(error) => return Err(error),
                }
            }
            if !completed {
                return Err(ProcessError::Closed);
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
                let _ = process
                    .request(
                        "turn/interrupt",
                        json!({"threadId": thread_id, "turnId": turn_id}),
                    )
                    .await;
                interrupt_guard.disarm();
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
    use crate::codex::{CodexConfig, CodexPrices};
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
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
            reasoning_effort: Some("medium".to_string()),
            reasoning_summary: Some("auto".to_string()),
            output_schema: None,
            verbosity: None,
        }
    }

    fn fake_gateway(mode: &str) -> (Arc<CodexGateway>, TestWorkspace) {
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
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log_file"
  id=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"id":%s,"result":{}}\n' "$id"
      ;;
    *'"method":"account/read"'*)
      printf '{"id":%s,"result":{"account":{"type":"chatgpt"},"requiresOpenaiAuth":true}}\n' "$id"
      ;;
    *'"method":"account/rateLimits/read"'*)
      if [ "$mode" = "usage_limit" ]; then
        printf '{"id":%s,"result":{"rateLimits":{"primary":{"usedPercent":100,"windowDurationMins":300,"resetsAt":4102444800},"secondary":null,"rateLimitReachedType":"rate_limit_reached","spendControlReached":false}}}\n' "$id"
      elif [ "$mode" = "near_limit" ]; then
        printf '{"id":%s,"result":{"rateLimits":{"primary":{"usedPercent":97,"windowDurationMins":300,"resetsAt":4102444800},"secondary":null,"rateLimitReachedType":null,"spendControlReached":false}}}\n' "$id"
      else
        printf '{"id":%s,"result":{"rateLimits":{"primary":{"usedPercent":25,"windowDurationMins":300,"resetsAt":4102444800},"secondary":{"usedPercent":10,"windowDurationMins":10080,"resetsAt":4102444800},"rateLimitReachedType":null,"spendControlReached":false}}}\n' "$id"
      fi
      ;;
    *'"method":"thread/start"'*)
      printf '{"id":%s,"result":{"model":"gpt-5.6-sol","thread":{"id":"thread-1"}}}\n' "$id"
      ;;
    *'"method":"turn/start"'*)
      printf '{"id":%s,"result":{"turn":{"id":"turn-1"}}}\n' "$id"
      if [ "$mode" = "text" ] || [ "$mode" = "near_limit" ]; then
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
        let config = CodexConfig {
            enabled: true,
            binary: binary.to_str().unwrap().to_string(),
            binary_sha256: digest,
            expected_version: "codex-cli test".to_string(),
            homes: vec![root.to_str().unwrap().to_string()],
            homes_dir: None,
            work_dir: root.to_str().unwrap().to_string(),
            // The workspace test runner executes several fake child processes in parallel.
            // Leave enough scheduler headroom for the digest + version preflight under CI load;
            // individual JSON-RPC and turn deadlines below remain short.
            startup_timeout_ms: 10_000,
            request_timeout_ms: 2_000,
            turn_timeout_ms: 2_000,
            max_concurrent_turns: 1,
            admit_below_used_percent: 95,
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
        let _ = gateway;
        let root = workspace.root.clone();
        let binary = root.join("fake-codex");
        let script = std::fs::read_to_string(&binary).unwrap();
        let digest = format!("{:x}", Sha256::digest(script.as_bytes()));
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
            binary: binary.to_str().unwrap().to_string(),
            binary_sha256: digest,
            expected_version: "codex-cli test".to_string(),
            homes,
            homes_dir: Some(root.join("pool").to_str().unwrap().to_string()),
            work_dir: root.to_str().unwrap().to_string(),
            startup_timeout_ms: 10_000,
            request_timeout_ms: 2_000,
            turn_timeout_ms: 2_000,
            max_concurrent_turns: 1,
            admit_below_used_percent: 95,
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

    #[tokio::test]
    async fn full_transport_preserves_prompt_boundary_events_and_exact_usage() {
        let (gateway, workspace) = fake_gateway("text");
        let (updates_tx, mut updates_rx) = mpsc::channel(8);
        let result = gateway
            .run_turn(turn_request(test_model()), Some(updates_tx))
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
        let turn_start = requests
            .iter()
            .find(|request| request["method"] == "turn/start")
            .unwrap();
        assert_eq!(turn_start["params"]["effort"], "medium");
        assert_eq!(turn_start["params"]["summary"], "auto");
        assert_eq!(turn_start["params"]["input"][0]["text"], "hello");
    }

    #[tokio::test]
    async fn closed_downstream_update_channel_still_drains_authoritative_usage() {
        let (gateway, _workspace) = fake_gateway("text");
        let (updates_tx, updates_rx) = mpsc::channel(1);
        drop(updates_rx);

        let result = gateway
            .run_turn(turn_request(test_model()), Some(updates_tx))
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
            .run_turn(turn_request(test_model()), None)
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
            .run_turn(turn_request(test_model()), None)
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
        let result = gateway.run_turn(request, None).await.unwrap();

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
            .run_turn(turn_request(test_model()), Some(updates_tx))
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
                .run_turn(turn_request(test_model()), Some(updates_tx))
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
    async fn structured_usage_limit_is_classified_without_exposing_upstream_text() {
        let (gateway, _workspace) = fake_gateway("usage_limit");
        let error = gateway
            .run_turn(turn_request(test_model()), None)
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
        assert_eq!(status.homes.len(), 2, "the finished account joined the pool");

        // And it can actually serve: the new home starts its own attested child on demand.
        let result = gateway
            .run_turn(turn_request(test_model()), None)
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

        std::fs::remove_dir_all(&bought).unwrap();
        gateway.rediscover().await;
        assert_eq!(gateway.operational_status().await.homes.len(), 1);

        // The explicit home has no directory to scan away, so the pool keeps serving from it.
        std::fs::remove_dir_all(&pool_dir).unwrap();
        gateway.rediscover().await;
        assert_eq!(gateway.operational_status().await.homes.len(), 1);
    }

    #[tokio::test]
    async fn a_home_at_its_usage_limit_rotates_to_a_healthy_home() {
        let (gateway, _workspace, logs) = fake_pool_gateway(&["usage_limit", "text"]);
        let result = gateway
            .run_turn(turn_request(test_model()), None)
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
            .run_turn(turn_request(test_model()), None)
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
    async fn window_headroom_keeps_a_nearly_exhausted_home_out_of_rotation() {
        let (gateway, _workspace, logs) = fake_pool_gateway(&["near_limit", "text"]);
        // Preflight is what populates the snapshot the admission gate reads, exactly as in serve.
        gateway.preflight().await.unwrap();
        gateway
            .run_turn(turn_request(test_model()), None)
            .await
            .unwrap();
        assert!(
            !served_turn(&logs[0]),
            "a home at 97% must not be admitted below the 95% headroom"
        );
        assert!(served_turn(&logs[1]));
        let status = gateway.operational_status().await;
        assert_eq!(status.available, 1);
    }

    #[tokio::test]
    async fn a_dead_child_is_retried_once_on_another_home() {
        let (gateway, _workspace, logs) = fake_pool_gateway(&["die", "text"]);
        let result = gateway
            .run_turn(turn_request(test_model()), None)
            .await
            .unwrap();
        assert_eq!(result.usage.input_tokens, 101);
        assert!(served_turn(&logs[0]));
        assert!(served_turn(&logs[1]));
    }

    #[tokio::test]
    async fn a_transport_fault_does_not_quarantine_the_subscription() {
        let (gateway, _workspace, _logs) = fake_pool_gateway(&["die", "text"]);
        gateway
            .run_turn(turn_request(test_model()), None)
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
        let run = gateway.run_turn(turn_request(test_model()), Some(updates_tx));
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
}
