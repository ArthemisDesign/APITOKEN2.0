//! Fail-closed validation and source-protocol state for translated Anthropic SSE.

use std::collections::HashSet;

use serde_json::Value;

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
    pub(crate) fn accept(&mut self, event: &str, data: &str) -> Result<Option<Value>, ()> {
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
            return if event.is_empty() { Err(()) } else { Ok(None) };
        }

        let value: Value = serde_json::from_str(data).map_err(|_| ())?;
        let object = value.as_object().ok_or(())?;
        if object.get("type").and_then(Value::as_str) != Some(event) {
            return Err(());
        }

        match event {
            "message_start" => {
                if self.started || !self.open_blocks.is_empty() || self.saw_message_delta {
                    return Err(());
                }
                object.get("message").and_then(Value::as_object).ok_or(())?;
                self.started = true;
            }
            "ping" => {}
            "error" => {
                let error = object.get("error").and_then(Value::as_object).ok_or(())?;
                error.get("message").and_then(Value::as_str).ok_or(())?;
            }
            "content_block_start" => {
                self.require_started_before_terminal()?;
                let index = required_u64(object.get("index"))?;
                let block = object
                    .get("content_block")
                    .and_then(Value::as_object)
                    .ok_or(())?;
                block.get("type").and_then(Value::as_str).ok_or(())?;
                if !self.open_blocks.insert(index) {
                    return Err(());
                }
            }
            "content_block_delta" => {
                self.require_started_before_terminal()?;
                let index = required_u64(object.get("index"))?;
                if !self.open_blocks.contains(&index) {
                    return Err(());
                }
                let delta = object.get("delta").and_then(Value::as_object).ok_or(())?;
                delta.get("type").and_then(Value::as_str).ok_or(())?;
            }
            "content_block_stop" => {
                self.require_started_before_terminal()?;
                let index = required_u64(object.get("index"))?;
                if !self.open_blocks.remove(&index) {
                    return Err(());
                }
            }
            "message_delta" => {
                self.require_started_before_terminal()?;
                if !self.open_blocks.is_empty() {
                    return Err(());
                }
                let delta = object.get("delta").and_then(Value::as_object).ok_or(())?;
                delta.get("stop_reason").and_then(Value::as_str).ok_or(())?;
                object.get("usage").and_then(Value::as_object).ok_or(())?;
                self.saw_message_delta = true;
            }
            "message_stop" => {
                self.require_started()?;
                if !self.open_blocks.is_empty() || !self.saw_message_delta {
                    return Err(());
                }
            }
            _ => unreachable!(),
        }
        Ok(Some(value))
    }

    fn require_started_before_terminal(&self) -> Result<(), ()> {
        if self.started && !self.saw_message_delta {
            Ok(())
        } else {
            Err(())
        }
    }

    fn require_started(&self) -> Result<(), ()> {
        if self.started { Ok(()) } else { Err(()) }
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
            ("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#),
            ("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}"#),
            ("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
            ("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{}}"#),
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
            ("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lost"}}"#),
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
