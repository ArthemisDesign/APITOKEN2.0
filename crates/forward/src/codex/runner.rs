//! One stateless native generation per public API request.
//!
//! Request-local instructions and exact replayed Responses items preserve OpenAI's stateless
//! semantics. A conversation is never hidden server-side: every turn carries its full input.

use super::{
    new_id, AppServerEvent, AuthContext, CodexGateway, CodexHome, CodexModel, HomeSelection,
    ProcessError, TurnEvents, TurnRouting, TurnSlot,
};
use crate::metrics::Metrics;
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
    /// Opaque tenant-scoped key forwarded upstream. This replaces the random ephemeral thread id
    /// a stock client otherwise uses for every request.
    pub prompt_cache_key: Option<String>,
    /// Replaces the client's built-in base prompt. `None` means an intentionally empty base.
    pub base_instructions: Option<String>,
    /// Request-owned developer instructions, replayed as a leading developer message.
    pub developer_instructions: Option<String>,
    /// Exact prior Responses items to replay before the new user input.
    pub injected_items: Vec<Value>,
    /// New user input parts: `{"type": "text", "text"}` and `{"type": "image", "url"}`.
    pub turn_input: Vec<Value>,
    /// Canonical dynamic-tool specs (`function` with `inputSchema`, `custom` with Lark grammar).
    pub dynamic_tools: Vec<Value>,
    /// OpenAI request value (`priority` for Codex Fast mode, otherwise absent).
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
    /// Effective product tier delivered by the gateway. On the private ChatGPT Codex backend a
    /// successful `priority` request is Fast even though `response.completed.service_tier` commonly
    /// remains `default`; official Codex maintainers explicitly document that field as unsuitable
    /// for end-to-end Fast verification, and the production A/B shows the advertised 1.5x cadence.
    pub effective_service_tier: Option<String>,
    /// Diagnostic copy of `response.completed.response.service_tier`. This is retained for wire
    /// drift monitoring only and must not drive placement, public responses, or settlement.
    pub provider_reported_service_tier: Option<String>,
}

/// Resolve the customer-visible and billable tier after a successful turn.
///
/// ChatGPT-authenticated Codex routes Fast server-side. Its completed payload commonly reports
/// `default` for both Standard and measurably accelerated Fast turns, so that provider field is
/// diagnostic only. The accepted request is the product contract: `priority` is Fast and an
/// absent tier is Standard.
fn effective_service_tier(
    requested_fast: bool,
    _provider_reported_tier: Option<&str>,
) -> &'static str {
    if requested_fast {
        "priority"
    } else {
        "default"
    }
}

/// The native backend caps `prompt_cache_key` at 64 bytes (verified live: 129-byte affinity
/// keys are rejected with `string_above_max_length`). Client keys up to that size pass through
/// verbatim; anything longer collapses to a keyed 64-hex digest, preserving tenant-level cache
/// affinity without exposing the composite affinity key's structure upstream.
pub(crate) fn bounded_cache_key(key: &str) -> String {
    if key.len() <= 64 {
        key.to_string()
    } else {
        blake3::hash(key.as_bytes()).to_hex().to_string()
    }
}

/// Assemble the exact upstream Responses body for one stateless turn.
///
/// The body contains only what the client owns: explicit base instructions, the replayed history
/// and new input, and the client's declared tools. No personality, environment or project context
/// is ever added — the same boundary the removed app-server patch enforced, now enforced by
/// construction: nothing else exists to send.
pub(crate) fn build_responses_body(request: &CodexTurnRequest) -> Value {
    let mut input: Vec<Value> = Vec::with_capacity(request.injected_items.len() + 2);
    if let Some(developer) = &request.developer_instructions {
        input.push(json!({
            "type": "message",
            "role": "developer",
            "content": [{"type": "input_text", "text": developer}],
        }));
    }
    input.extend(request.injected_items.iter().cloned());
    let mut content: Vec<Value> = Vec::with_capacity(request.turn_input.len());
    for part in &request.turn_input {
        match part.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    content.push(json!({"type": "input_text", "text": text}));
                }
            }
            Some("image") => {
                if let Some(url) = part.get("url").and_then(Value::as_str) {
                    content.push(json!({"type": "input_image", "image_url": url}));
                }
            }
            _ => {}
        }
    }
    if !content.is_empty() {
        input.push(json!({"type": "message", "role": "user", "content": content}));
    }
    let tools: Vec<Value> = request
        .dynamic_tools
        .iter()
        .filter_map(|tool| match tool.get("type").and_then(Value::as_str) {
            Some("function") => Some(json!({
                "type": "function",
                "name": tool.get("name").cloned().unwrap_or(Value::Null),
                "description": tool.get("description").cloned().unwrap_or(Value::Null),
                "parameters": tool.get("inputSchema").cloned().unwrap_or(json!({"type": "object"})),
                "strict": false,
            })),
            // Custom (Lark grammar) tools already carry the public Responses shape.
            Some("custom") => Some(tool.clone()),
            _ => None,
        })
        .collect();
    let mut body = json!({
        "model": request.model.upstream,
        "instructions": request.base_instructions.as_deref().unwrap_or(""),
        "input": input,
        "tools": tools,
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "store": false,
        "stream": true,
        // Official Codex is stateless (`store:false`) and always requests the encrypted reasoning
        // continuation item. Public adapters still expose it only when the customer explicitly
        // requested that include; retaining it upstream preserves multi-turn reasoning continuity.
        "include": ["reasoning.encrypted_content"],
    });
    if let Some(key) = &request.prompt_cache_key {
        body["prompt_cache_key"] = Value::String(bounded_cache_key(key));
    }
    if let Some(tier) = &request.service_tier {
        body["service_tier"] = Value::String(tier.clone());
    }
    if request.reasoning_effort.is_some() || request.reasoning_summary.is_some() {
        let mut reasoning = json!({});
        if let Some(effort) = &request.reasoning_effort {
            reasoning["effort"] = Value::String(effort.clone());
        }
        if let Some(summary) = &request.reasoning_summary {
            reasoning["summary"] = Value::String(summary.clone());
        }
        body["reasoning"] = reasoning;
    }
    let mut text = json!({});
    if let Some(schema) = &request.output_schema {
        // `parse_text` encodes a public `json_object` request as this marker schema.
        text["format"] = if schema == &json!({"type": "object", "additionalProperties": true}) {
            json!({"type": "json_object"})
        } else {
            json!({"type": "json_schema", "name": "response_format", "schema": schema, "strict": false})
        };
    }
    if let Some(verbosity) = &request.verbosity {
        text["verbosity"] = Value::String(verbosity.clone());
    }
    if !text.as_object().is_none_or(|object| object.is_empty()) {
        body["text"] = text;
    }
    body
}

impl CodexGateway {
    /// Run one turn on the best available home, rotating on account-fault errors.
    ///
    /// Rotation mirrors the Claude path's blame classification. A usage limit or a dead login is
    /// that home's fault, so the pool moves to another home without spending the transport budget;
    /// an upstream outage is a backend fault and is retried exactly once. Nothing is ever retried
    /// after the first byte has reached the client: the public stream must never replay or
    /// interleave two attempts.
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
        let fast_model = (request.service_tier.as_deref() == Some("priority"))
            .then(|| request.model.upstream.as_str());
        // Stable across every home/transport retry of this logical turn and never forwarded
        // upstream. Registry uses it as the immutable event idempotency boundary.
        let calibration_request_id = new_id("cal");
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
                .select_home(&tried, preferred, warm, place_cache_root, true, fast_model)
                .await
            {
                HomeSelection::Ready(home, slot) => (home, slot),
                HomeSelection::Unavailable { ready_at } => {
                    elog::warn("codex", "codex pool exhausted: no home available");
                    let local = last_error.unwrap_or_else(|| ProcessError::UsageLimitExceeded {
                        retry_after: retry_after_from(ready_at),
                    });
                    return self
                        .run_claudestore_after_local_terminal(&request, updates, &emitted, local)
                        .await;
                }
            };
            tried.push(home.id().to_string());
            let result = home
                .run_turn(request.clone(), updates.clone(), &emitted, slot)
                .await;
            let error = match result {
                Ok(result) => {
                    if let Some(model) = fast_model {
                        home.observe_fast_result(
                            model,
                            result.provider_reported_service_tier.as_deref(),
                        );
                    }
                    let completed_at = pool::now();
                    let fast = result.effective_service_tier.as_deref() == Some("priority");
                    let spend_ready = match super::billing::price_calibration_event(
                        &calibration_request_id,
                        home.id(),
                        &request.model,
                        &result.usage,
                        completed_at,
                        fast,
                        result.provider_reported_service_tier.as_deref(),
                    ) {
                        Ok(event) => home.record_calibration_event(event).await,
                        Err(error) => {
                            home.reject_calibration_event(&error);
                            false
                        }
                    };
                    // The served turn's response headers are the freshest window snapshot this
                    // home will ever publish. Persist the exact turn first so the quota observation
                    // cannot classify its provider movement as foreign usage. A failed writer keeps
                    // the snapshot cached; the health sweep drains the FIFO and replays it later.
                    if spend_ready {
                        home.ingest_turn_snapshot().await;
                    }
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
                ProcessError::BadRequest | ProcessError::ContextWindowExceeded
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
                    return self
                        .run_claudestore_after_local_terminal(&request, updates, &emitted, error)
                        .await;
                }
                transport_retries_left -= 1;
            }
            last_error = Some(error);
        }
    }

    async fn run_claudestore_after_local_terminal(
        &self,
        request: &CodexTurnRequest,
        updates: Option<mpsc::Sender<TurnUpdate>>,
        emitted: &Arc<AtomicBool>,
        local: ProcessError,
    ) -> Result<CodexTurnResult, ProcessError> {
        let Some(fallback) = self
            .claudestore_fallback
            .as_ref()
            .filter(|fallback| fallback.supports_model(&request.model.id))
        else {
            return Err(local);
        };
        // A local delta already delivered to the public adapter makes replacement unsafe. This is
        // checked in the normal loop too; keep the guard here so future terminal branches cannot
        // accidentally start a second billable execution after output.
        if emitted.load(Ordering::Acquire) {
            return Err(local);
        }

        Metrics::inc(&self.metrics.claudestore_fallback_attempts);
        match fallback.run_turn(request, updates, emitted).await {
            Ok(result) => {
                Metrics::inc(&self.metrics.claudestore_fallback_successes);
                Ok(result)
            }
            Err(error) => {
                Metrics::inc(&self.metrics.claudestore_fallback_failures);
                elog::error(
                    "codex",
                    format!("ClaudeStore Codex fallback failed [{}]", error.diagnostic_class()),
                );
                Err(ProcessError::ExternalFallbackFailed {
                    local: Box::new(local),
                })
            }
        }
    }
}

/// Seconds a client should wait for the pool to recover, bounded to a sane advertised value.
fn retry_after_from(ready_at: Option<i64>) -> Option<u64> {
    let ready_at = ready_at?;
    Some(ready_at.saturating_sub(pool::now()).clamp(1, 7 * 24 * 3600) as u64)
}

impl CodexHome {
    /// Run one native turn on this home.
    ///
    /// A pre-stream 401 earns exactly one forced refresh and one retry on the same home (the
    /// Gemini pool's policy): a token that died of old age is not yet evidence against the
    /// subscription. A second rejection is an account fault and the pool rotates.
    pub(crate) async fn run_turn(
        &self,
        request: CodexTurnRequest,
        updates: Option<mpsc::Sender<TurnUpdate>>,
        emitted: &Arc<AtomicBool>,
        _turn_slot: TurnSlot,
    ) -> Result<CodexTurnResult, ProcessError> {
        let body = build_responses_body(&request);
        let mut refresh_retry_left = 1usize;
        let mut rejected_token: Option<String> = None;
        loop {
            let token = match &rejected_token {
                // A 401 already cost this attempt its token: reuse a concurrent winner if one
                // exists, otherwise force exactly one refresh for this home.
                Some(rejected) => self.access_token_after_rejection(rejected).await?,
                None => self.access_token().await?,
            };
            let account_id = self.credential.lock().await.account_id.clone();
            let auth = AuthContext {
                access_token: token,
                account_id,
            };
            let rejected_now = auth.access_token.as_str().to_string();
            let mut events = match self
                .transport()
                .run_turn(
                    &auth,
                    body.clone(),
                    request.prompt_cache_key.as_deref(),
                    self.rate_limits.clone(),
                )
                .await
            {
                Ok(events) => events,
                Err(error) => {
                    if matches!(error, ProcessError::AuthenticationRequired)
                        && refresh_retry_left > 0
                        && !emitted.load(Ordering::Acquire)
                    {
                        refresh_retry_left -= 1;
                        rejected_token = Some(rejected_now);
                        self.note_turn_error(&error);
                        continue;
                    }
                    return Err(error);
                }
            };
            return self
                .run_registered_turn(
                    &mut events,
                    updates,
                    emitted,
                    request.service_tier.as_deref() == Some("priority"),
                )
                .await;
        }
    }

    async fn run_registered_turn(
        &self,
        events: &mut TurnEvents,
        updates: Option<mpsc::Sender<TurnUpdate>>,
        emitted: &Arc<AtomicBool>,
        requested_fast: bool,
    ) -> Result<CodexTurnResult, ProcessError> {
        Self::consume_turn_events(
            Some(self),
            events,
            updates,
            emitted,
            requested_fast,
            (self.config().turn_timeout_ms > 0)
                .then(|| std::time::Duration::from_millis(self.config().turn_timeout_ms)),
            std::time::Duration::from_millis(self.config().turn_silence_timeout_ms.max(1)),
        )
        .await
    }

    /// Consume the shared Responses-SSE vocabulary for either a local ChatGPT home or the external
    /// ClaudeStore emergency transport. The optional home is used only to attach its authoritative
    /// retry-after snapshot to native subscription failures; external turns must never mutate or
    /// read a local home's quota/calibration identity.
    pub(super) async fn consume_turn_events(
        home: Option<&CodexHome>,
        events: &mut TurnEvents,
        updates: Option<mpsc::Sender<TurnUpdate>>,
        emitted: &Arc<AtomicBool>,
        requested_fast: bool,
        timeout: Option<std::time::Duration>,
        silence_timeout: std::time::Duration,
    ) -> Result<CodexTurnResult, ProcessError> {
        // A silence bound at or above the total deadline simply never fires: the total governs and
        // the turn fails as `turn completion`. That is a coherent configuration, not an error, so it
        // is left alone rather than clamped — clamping would silently relabel the failure.
        let event_loop = async {
            let mut output = Vec::<Value>::new();
            let mut completed_output_items = HashSet::<(String, String)>::new();
            let mut usage = CodexUsage::default();
            let mut saw_raw_usage = false;
            let provider_reported_service_tier = loop {
                // Bound silence, not just total duration. A home that stopped answering mid-turn
                // otherwise holds the client for the entire turn deadline before failing, and every
                // one of those ten-minute waits is spent on a subscription that will never reply.
                // The total deadline stays generous so genuine long reasoning is never cut short.
                let event = match tokio::time::timeout(silence_timeout, events.recv()).await {
                    Ok(event) => event?,
                    Err(_) => return Err(ProcessError::Timeout("turn silence")),
                };
                let AppServerEvent::Notification { method, params } = event;
                match method.as_str() {
                    "item/agentMessage/delta" => {
                        let Some(delta) = params.get("delta").and_then(Value::as_str) else {
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
                            // Completed items are protocol entities, not text blobs. Suppress a
                            // repeated completion by its native identity for messages/reasoning
                            // and by call identity for tools; legitimate equal text under another
                            // item id remains untouched.
                            if claim_completed_output(&item, &mut completed_output_items) {
                                send_update(&updates, emitted, TurnUpdate::RawItem(item.clone()))
                                    .await;
                                output.push(item);
                            }
                        }
                    }
                    "rawResponse/completed" => {
                        if let Some(raw_usage) = params.get("usage").filter(|v| !v.is_null()) {
                            usage.add_assign(&CodexUsage::from_value(raw_usage));
                            saw_raw_usage = true;
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
                        if status == "completed" {
                            // ChatGPT Codex routing accepts `priority` but normally leaves this
                            // response field at `default`, including on demonstrably 1.5x Fast
                            // streams. Preserve it as diagnostics and derive the effective product
                            // tier from the accepted request below.
                            break params
                                .pointer("/turn/serviceTier")
                                .and_then(Value::as_str)
                                .map(str::to_string);
                        }
                        if let Some(error) =
                            classify_turn_error(home, params.pointer("/turn/error/codexErrorInfo"))
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
                    "error" => {
                        if !params
                            .get("willRetry")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        {
                            if let Some(error) =
                                classify_turn_error(home, params.pointer("/error/codexErrorInfo"))
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
            };
            let effective_service_tier = Some(
                effective_service_tier(requested_fast, provider_reported_service_tier.as_deref())
                    .to_string(),
            );
            Ok(CodexTurnResult {
                output,
                usage,
                effective_service_tier,
                provider_reported_service_tier,
            })
        };

        // No total deadline unless an operator asks for one. Silence above already answers "has
        // this home stopped replying"; a total bound could only answer "has this taken too long",
        // which is a question about the customer's task, not about our transport, and every value
        // we could pick is a guess some legitimate task exceeds.
        let Some(timeout) = timeout else {
            return event_loop.await;
        };
        match tokio::time::timeout(timeout, event_loop).await {
            Ok(result) => result,
            Err(_) => Err(ProcessError::Timeout("turn completion")),
        }
    }
}

/// Map the wire error vocabulary onto the pool's blame classes. Retry-after comes from this
/// home's freshest window snapshot, never from an upstream text message.
async fn classify_turn_error(
    home: Option<&CodexHome>,
    info: Option<&Value>,
) -> Option<ProcessError> {
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
                retry_after: match home {
                    Some(home) => home.usage_limit_retry_after().await,
                    None => None,
                },
            })
        }
        (Some("badRequest" | "cyberPolicy"), _) | (_, Some(400)) => Some(ProcessError::BadRequest),
        (Some("unauthorized"), _) | (_, Some(401 | 403)) => {
            Some(ProcessError::AuthenticationRequired)
        }
        _ => None,
    }
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

fn claim_completed_output(item: &Value, completed: &mut HashSet<(String, String)>) -> bool {
    let Some(kind) = item.get("type").and_then(Value::as_str) else {
        return true;
    };
    let identity = match kind {
        "function_call" | "custom_tool_call" => item
            .get("call_id")
            .and_then(Value::as_str)
            .or_else(|| item.get("id").and_then(Value::as_str)),
        _ => item
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| item.get("call_id").and_then(Value::as_str)),
    };
    let Some(identity) = identity.filter(|value| !value.is_empty()) else {
        return true;
    };
    completed.insert((kind.to_string(), identity.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

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
            input_modalities: vec!["text".to_string(), "image".to_string()],
            output_modalities: vec!["text".to_string()],
            tool_calling: true,
            structured_outputs: true,
            fast_multiplier_basis_points: Some(25_000),
            prices: crate::codex::CodexPrices {
                input: 5_000,
                cached_input: 500,
                cache_write_input: 6_250,
                output: 30_000,
                api_fast_multiplier_basis_points: 25_000,
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

    #[test]
    fn cache_key_stays_within_the_upstream_limit() {
        assert_eq!(bounded_cache_key("short-key"), "short-key");
        let affinity_shaped = format!("{}:{}", "a".repeat(64), "b".repeat(64));
        let bounded = bounded_cache_key(&affinity_shaped);
        assert_eq!(bounded.len(), 64);
        assert!(bounded.bytes().all(|byte| byte.is_ascii_hexdigit()));
        // Deterministic and tenant-stable, never the raw composite key.
        assert_eq!(bounded, bounded_cache_key(&affinity_shaped));
        assert_ne!(bounded, affinity_shaped);
    }

    #[test]
    fn body_contains_only_client_owned_context() {
        let body = build_responses_body(&turn_request(test_model()));
        assert_eq!(body["model"], "gpt-5.6-sol");
        assert_eq!(body["instructions"], "");
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(body["tool_choice"], "auto");
        // Developer instructions lead, then the user message; nothing else exists to send.
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "developer");
        assert_eq!(input[1]["role"], "user");
        assert_eq!(input[1]["content"][0]["type"], "input_text");
        assert_eq!(input[1]["content"][0]["text"], "hello");
        // The dynamic function becomes a plain Responses tool with no client execution hints.
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["parameters"]["type"], "object");
        assert_eq!(body["tools"][0]["strict"], false);
        assert!(body["tools"][0].get("inputSchema").is_none());
        assert!(body["tools"][0].get("deferLoading").is_none());
        assert_eq!(body["reasoning"]["effort"], "medium");
        assert_eq!(body["reasoning"]["summary"], "auto");
        assert!(body.get("service_tier").is_none());
        let text = serde_json::to_string(&body).unwrap();
        for forbidden in [
            "personality",
            "environment",
            "project",
            "plugin",
            "permission",
        ] {
            assert!(!text.contains(forbidden), "body leaks {forbidden}");
        }
    }

    #[test]
    fn body_replays_history_and_images_verbatim() {
        let mut request = turn_request(test_model());
        request.injected_items = vec![json!({"type": "message", "role": "user", "content": [
            {"type": "input_text", "text": "earlier"}
        ]})];
        request.turn_input = vec![
            json!({"type": "text", "text": "now"}),
            json!({"type": "image", "url": "data:image/png;base64,AAAA"}),
        ];
        let body = build_responses_body(&request);
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 3);
        assert_eq!(input[1]["content"][0]["text"], "earlier");
        assert_eq!(input[2]["content"][0]["type"], "input_text");
        assert_eq!(input[2]["content"][1]["type"], "input_image");
        assert_eq!(
            input[2]["content"][1]["image_url"],
            "data:image/png;base64,AAAA"
        );
    }

    #[test]
    fn body_maps_output_contracts() {
        let mut request = turn_request(test_model());
        request.service_tier = Some("priority".to_string());
        request.prompt_cache_key = Some("tenant-digest".to_string());
        request.output_schema = Some(json!({"type": "object", "additionalProperties": true}));
        request.verbosity = Some("low".to_string());
        let body = build_responses_body(&request);
        assert_eq!(body["service_tier"], "priority");
        assert_eq!(body["prompt_cache_key"], "tenant-digest");
        assert_eq!(body["text"]["format"]["type"], "json_object");
        assert_eq!(body["text"]["verbosity"], "low");

        request.output_schema =
            Some(json!({"type": "object", "properties": {"a": {"type": "string"}}}));
        let body = build_responses_body(&request);
        assert_eq!(body["text"]["format"]["type"], "json_schema");
        assert_eq!(body["text"]["format"]["strict"], false);
        assert_eq!(
            body["text"]["format"]["schema"]["properties"]["a"]["type"],
            "string"
        );
    }

    #[test]
    fn accepted_priority_stays_effective_fast_when_provider_reports_default() {
        assert_eq!(effective_service_tier(true, Some("default")), "priority");

        // A Standard request remains Standard even if the non-authoritative diagnostic field
        // changes in a future backend rollout.
        assert_eq!(effective_service_tier(false, Some("priority")), "default");
    }

    #[test]
    fn completed_items_are_deduplicated_by_protocol_identity_not_content() {
        let mut completed = HashSet::new();
        let message = json!({"type": "message", "id": "msg_1", "content": []});
        assert!(claim_completed_output(&message, &mut completed));
        assert!(!claim_completed_output(&message, &mut completed));

        // Equal text in a distinct output item remains a distinct model result.
        let equal_text_new_item = json!({"type": "message", "id": "msg_2", "content": []});
        assert!(claim_completed_output(&equal_text_new_item, &mut completed));

        let reasoning = json!({"type": "reasoning", "id": "rs_1", "summary": []});
        assert!(claim_completed_output(&reasoning, &mut completed));
        assert!(!claim_completed_output(&reasoning, &mut completed));

        let tool = json!({
            "type": "function_call",
            "id": "fc_first",
            "call_id": "call_1",
            "name": "lookup",
            "arguments": "{}"
        });
        let replay_with_drifted_item_id = json!({
            "type": "function_call",
            "id": "fc_second",
            "call_id": "call_1",
            "name": "lookup",
            "arguments": "{}"
        });
        assert!(claim_completed_output(&tool, &mut completed));
        assert!(!claim_completed_output(
            &replay_with_drifted_item_id,
            &mut completed
        ));
    }
}
