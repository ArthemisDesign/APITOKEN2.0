//! Default-off last-resort GPT transport through ClaudeStore API3.
//!
//! This is not a second public provider and never participates in local ChatGPT-home health,
//! affinity, quota, or calibration. The gateway calls it at most once after the local Codex pool
//! has reached a terminal pre-byte result. Customer reserve/settlement remains owned by the normal
//! Codex admission path and uses authoritative terminal OpenAI usage from this stream.

use super::runner::build_responses_body;
use super::{CodexHome, CodexTurnRequest, CodexTurnResult, ProcessError, TurnEvents, TurnUpdate};
use crate::config::ClaudeStoreFallbackConfig;
use serde_json::Value;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;

/// The only ClaudeStore GPT ids with an official public Responses contract as of 2026-08-03.
/// This allow-list is deliberately compile-fixed: model discovery or a future ClaudeStore rollout
/// cannot silently expand a paid emergency route without research and a controlled live gate.
const SUPPORTED_MODELS: [&str; 2] = ["gpt-5.5", "gpt-5.4"];

pub(crate) struct ClaudeStoreCodexFallback {
    config: ClaudeStoreFallbackConfig,
    client: wreq::Client,
    turn_timeout: Option<std::time::Duration>,
    silence_timeout: std::time::Duration,
}

impl ClaudeStoreCodexFallback {
    pub(crate) fn new(
        config: ClaudeStoreFallbackConfig,
        request_timeout_ms: u64,
        turn_timeout_ms: u64,
        turn_silence_timeout_ms: u64,
    ) -> Result<Self, ProcessError> {
        let client = wreq::Client::builder()
            .emulation(crate::nodetls::bun_emulation())
            // A fallback credential is process-wide, not profile-owned. Never inherit a local
            // subscription proxy or ambient HTTP(S)_PROXY identity across this trust boundary.
            .no_proxy()
            .connect_timeout(std::time::Duration::from_millis(
                request_timeout_ms.max(1).min(30_000),
            ))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .read_timeout(std::time::Duration::from_millis(
                turn_silence_timeout_ms.max(30_000),
            ))
            .build()
            .map_err(|error| {
                ProcessError::InvalidConfig(format!("ClaudeStore HTTP client: {error}"))
            })?;
        Ok(Self {
            config,
            client,
            turn_timeout: (turn_timeout_ms > 0)
                .then(|| std::time::Duration::from_millis(turn_timeout_ms)),
            silence_timeout: std::time::Duration::from_millis(turn_silence_timeout_ms.max(1)),
        })
    }

    pub(crate) fn supports_model(&self, public_model: &str) -> bool {
        SUPPORTED_MODELS.contains(&public_model)
    }

    pub(crate) async fn run_turn(
        &self,
        request: &CodexTurnRequest,
        updates: Option<mpsc::Sender<TurnUpdate>>,
        emitted: &Arc<AtomicBool>,
    ) -> Result<CodexTurnResult, ProcessError> {
        if !self.supports_model(&request.model.id) {
            return Err(ProcessError::InvalidConfig(
                "ClaudeStore fallback model is outside the reviewed allow-list".to_string(),
            ));
        }

        let mut body = build_responses_body(request);
        // The local ChatGPT transport may map a public id to a private native slug. ClaudeStore's
        // public contract accepts the public id, so no local/private model mapping crosses this
        // trust boundary.
        body["model"] = Value::String(request.model.id.clone());

        // Count only the configured fallback generation submission, immediately before `.send()`.
        if let Some(attempts) = &request.attempts {
            attempts.record_send();
        }
        let response = self
            .client
            .post(format!(
                "{}/v1/responses",
                self.config.base_url().trim_end_matches('/')
            ))
            .header(
                wreq::header::AUTHORIZATION,
                format!("Bearer {}", self.config.api_key()),
            )
            .header(wreq::header::ACCEPT, "text/event-stream")
            .header(wreq::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    ProcessError::Timeout("ClaudeStore turn start")
                } else {
                    ProcessError::Closed
                }
            })?;
        if !response.status().is_success() {
            return Err(ProcessError::Protocol(format!(
                "ClaudeStore returned HTTP {}",
                response.status().as_u16()
            )));
        }

        let mut events = TurnEvents::from_external_response(response);
        let result = CodexHome::consume_turn_events(
            None,
            &mut events,
            updates,
            emitted,
            request.service_tier.as_deref() == Some("priority"),
            self.turn_timeout,
            self.silence_timeout,
        )
        .await?;
        if result.usage.total_tokens == 0
            || result.usage.input_tokens == 0
            || result.usage.total_tokens
                < result
                    .usage
                    .input_tokens
                    .saturating_add(result.usage.output_tokens)
        {
            return Err(ProcessError::Protocol(
                "ClaudeStore response omitted authoritative terminal usage".to_string(),
            ));
        }
        Ok(result)
    }
}
