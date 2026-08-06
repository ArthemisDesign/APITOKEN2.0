//! Fail-closed validation and source-protocol state for translated Anthropic SSE.

use std::collections::HashSet;
use std::fmt;

use serde_json::Value;

/// Why a source SSE frame failed validation. The payload names the offending event and the
/// violated check so a caller can log the actual protocol violation instead of a generic one.
#[derive(Debug)]
pub(crate) struct StreamError(pub(crate) String);

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Default)]
pub(crate) struct AnthropicStreamState {
    started: bool,
    open_blocks: HashSet<u64>,
    saw_message_delta: bool,
}

impl AnthropicStreamState {
    /// Validate a known Messages SSE event and advance the mandatory lifecycle. Unknown named
    /// events remain forward-compatible and are ignored; a missing event name, malformed JSON,
    /// mismatched `type`, or impossible ordering is a protocol failure.
    pub(crate) fn accept(&mut self, event: &str, data: &str) -> Result<Option<Value>, StreamError> {
        let known = matches!(
            event,
            "message_start"
                | "content_block_start"
                | "content_block_delta"
                | "content_block_stop"
                | "message_delta"
                | "message_stop"
                | "ping"
                | "error"
        );
        if !known {
            return if event.is_empty() {
                Err(StreamError(format!("empty event name")))
            } else {
                Ok(None)
            };
        }

        let value: Value = serde_json::from_str(data)
            .map_err(|_| StreamError(format!("{event}: body not valid JSON")))?;
        let object = value
            .as_object()
            .ok_or_else(|| StreamError(format!("{event}: json body not an object")))?;
        if object.get("type").and_then(Value::as_str) != Some(event) {
            return Err(StreamError(format!("{event}: type field does not match event name")));
        }

        match event {
            "message_start" => {
                if self.started || !self.open_blocks.is_empty() || self.saw_message_delta {
                    return Err(StreamError(format!(
                        "{event}: state lifecycle violation: stream already started"
                    )));
                }
                object
                    .get("message")
                    .and_then(Value::as_object)
                    .ok_or_else(|| StreamError(format!("{event}: missing message object")))?;
                self.started = true;
            }
            "ping" => {}
            "error" => {
                let error = object
                    .get("error")
                    .and_then(Value::as_object)
                    .ok_or_else(|| StreamError(format!("{event}: missing error object")))?;
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .ok_or_else(|| StreamError(format!("{event}: missing error message")))?;
            }
            "content_block_start" => {
                self.require_started_before_terminal(event)?;
                let index = required_u64(object.get("index"))
                    .map_err(|_| StreamError(format!("{event}: missing index field")))?;
                let block = object
                    .get("content_block")
                    .and_then(Value::as_object)
                    .ok_or_else(|| StreamError(format!("{event}: missing content_block object")))?;
                block
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or_else(|| StreamError(format!("{event}: missing content_block type")))?;
                if !self.open_blocks.insert(index) {
                    return Err(StreamError(format!(
                        "{event}: state lifecycle violation: duplicate open block index {index}"
                    )));
                }
            }
            "content_block_delta" => {
                self.require_started_before_terminal(event)?;
                let index = required_u64(object.get("index"))
                    .map_err(|_| StreamError(format!("{event}: missing index field")))?;
                if !self.open_blocks.contains(&index) {
                    return Err(StreamError(format!(
                        "{event}: state lifecycle violation: block {index} not open"
                    )));
                }
                let delta = object
                    .get("delta")
                    .and_then(Value::as_object)
                    .ok_or_else(|| StreamError(format!("{event}: missing delta object")))?;
                delta
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or_else(|| StreamError(format!("{event}: missing delta type")))?;
            }
            "content_block_stop" => {
                self.require_started_before_terminal(event)?;
                let index = required_u64(object.get("index"))
                    .map_err(|_| StreamError(format!("{event}: missing index field")))?;
                if !self.open_blocks.remove(&index) {
                    return Err(StreamError(format!(
                        "{event}: state lifecycle violation: block {index} not open"
                    )));
                }
            }
            "message_delta" => {
                self.require_started_before_terminal(event)?;
                if !self.open_blocks.is_empty() {
                    return Err(StreamError(format!(
                        "{event}: state lifecycle violation: open blocks remain"
                    )));
                }
                let delta = object
                    .get("delta")
                    .and_then(Value::as_object)
                    .ok_or_else(|| StreamError(format!("{event}: missing delta object")))?;
                delta
                    .get("stop_reason")
                    .and_then(Value::as_str)
                    .ok_or_else(|| StreamError(format!("{event}: missing delta stop_reason")))?;
                object
                    .get("usage")
                    .and_then(Value::as_object)
                    .ok_or_else(|| StreamError(format!("{event}: missing usage object")))?;
                self.saw_message_delta = true;
            }
            "message_stop" => {
                self.require_started(event)?;
                if !self.open_blocks.is_empty() || !self.saw_message_delta {
                    return Err(StreamError(format!(
                        "{event}: state lifecycle violation: open blocks remain or no message_delta"
                    )));
                }
            }
            _ => unreachable!(),
        }
        Ok(Some(value))
    }

    fn require_started_before_terminal(&self, event: &str) -> Result<(), StreamError> {
        if self.started && !self.saw_message_delta {
            Ok(())
        } else {
            Err(StreamError(format!(
                "{event}: state lifecycle violation: terminal event before started stream or after message_delta"
            )))
        }
    }

    fn require_started(&self, event: &str) -> Result<(), StreamError> {
        if self.started {
            Ok(())
        } else {
            Err(StreamError(format!(
                "{event}: state lifecycle violation: stream not started"
            )))
        }
    }
}

fn required_u64(value: Option<&Value>) -> Result<u64, ()> {
    value.and_then(Value::as_u64).ok_or(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_lifecycle_completes_and_unknown_named_events_remain_forward_compatible() {
        let mut state = AnthropicStreamState::default();
        assert_eq!(state.accept("future_event", "not-json").unwrap(), None);
        for (event, data) in [
            ("message_start", r#"{"type":"message_start","message":{}}"#),
            (
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            ),
            (
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}"#,
            ),
            (
                "content_block_stop",
                r#"{"type":"content_block_stop","index":0}"#,
            ),
            (
                "message_delta",
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{}}"#,
            ),
            ("message_stop", r#"{"type":"message_stop"}"#),
        ] {
            assert!(state.accept(event, data).unwrap().is_some(), "{event}");
        }
    }

    #[test]
    fn malformed_mismatched_and_out_of_order_mandatory_events_fail_closed() {
        for (event, data) in [
            ("", r#"{"type":"message_start","message":{}}"#),
            ("message_start", "not-json"),
            ("message_start", r#"{"type":"ping","message":{}}"#),
            (
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lost"}}"#,
            ),
        ] {
            assert!(AnthropicStreamState::default().accept(event, data).is_err());
        }

        let mut state = AnthropicStreamState::default();
        state
            .accept("message_start", r#"{"type":"message_start","message":{}}"#)
            .unwrap();
        assert!(state
            .accept("message_stop", r#"{"type":"message_stop"}"#)
            .is_err());
    }
}
