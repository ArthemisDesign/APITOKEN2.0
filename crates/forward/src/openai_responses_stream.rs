//! Canonical OpenAI Responses SSE event encoder shared by translated provider planes.
//!
//! Every `data:` object is a typed event with a monotonic `sequence_number`. Lifecycle events
//! carry the Response object under `response`; consumers must never infer the event type only from
//! the optional SSE `event:` field because official SDKs dispatch on `data.type`.

use axum::body::Bytes;
use serde_json::{json, Value};

#[derive(Debug, Default)]
pub(crate) struct ResponsesEventEncoder {
    next_sequence: u64,
}

impl ResponsesEventEncoder {
    pub(crate) fn event(&mut self, event: &'static str, mut fields: Value) -> Bytes {
        let object = fields
            .as_object_mut()
            .expect("Responses event fields are always an object");
        debug_assert!(!object.contains_key("type"));
        debug_assert!(!object.contains_key("sequence_number"));
        object.insert("type".to_string(), Value::String(event.to_string()));
        object.insert(
            "sequence_number".to_string(),
            Value::from(self.next_sequence),
        );
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .expect("Responses event sequence cannot overflow");

        Bytes::from(format!("event: {event}\ndata: {fields}\n\n"))
    }

    pub(crate) fn response(&mut self, event: &'static str, response: Value) -> Bytes {
        self.event(event, json!({"response": response}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(frame: &Bytes) -> Value {
        let text = std::str::from_utf8(frame).unwrap();
        let line = text
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap();
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn events_are_typed_and_strictly_monotonic() {
        let mut encoder = ResponsesEventEncoder::default();
        let first = data(&encoder.event("response.output_text.delta", json!({"delta": "a"})));
        let second = data(&encoder.event("response.output_text.done", json!({"text": "a"})));

        assert_eq!(first["type"], "response.output_text.delta");
        assert_eq!(first["sequence_number"], 0);
        assert_eq!(first["delta"], "a");
        assert_eq!(second["type"], "response.output_text.done");
        assert_eq!(second["sequence_number"], 1);
    }

    #[test]
    fn lifecycle_events_wrap_the_response_object() {
        let mut encoder = ResponsesEventEncoder::default();
        let event = data(&encoder.response(
            "response.completed",
            json!({"id": "resp_1", "object": "response"}),
        ));

        assert_eq!(event["type"], "response.completed");
        assert_eq!(event["sequence_number"], 0);
        assert_eq!(event["response"]["id"], "resp_1");
        assert!(event.get("id").is_none());
    }
}
