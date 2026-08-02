//! Fail-closed validation and terminal-state tracking for translated GenerateContent SSE.

use serde_json::{Map, Value};

#[derive(Default)]
pub(crate) struct GeminiStreamState {
    completed: bool,
}

impl GeminiStreamState {
    pub(crate) fn accept(&mut self, data: &str) -> Result<Value, ()> {
        let value: Value = serde_json::from_str(data).map_err(|_| ())?;
        let object = value.as_object().ok_or(())?;
        if let Some(usage) = object.get("usageMetadata") {
            validate_usage(usage)?;
        }
        validate_optional_string(object, "modelVersion")?;

        if let Some(error) = object.get("error") {
            let error = error.as_object().ok_or(())?;
            error.get("message").and_then(Value::as_str).ok_or(())?;
            validate_optional_string(error, "status")?;
            if error.get("code").is_some_and(|code| code.as_i64().is_none()) {
                return Err(());
            }
            return Ok(value);
        }

        if let Some(feedback) = object.get("promptFeedback") {
            let feedback = feedback.as_object().ok_or(())?;
            if let Some(reason) = feedback.get("blockReason") {
                if reason.as_str().filter(|reason| !reason.is_empty()).is_none() {
                    return Err(());
                }
                if reason != "BLOCK_REASON_UNSPECIFIED" {
                    self.completed = true;
                }
            }
        }

        if let Some(candidates) = object.get("candidates") {
            let candidates = candidates.as_array().ok_or(())?;
            for candidate in candidates {
                let candidate = candidate.as_object().ok_or(())?;
                if let Some(content) = candidate.get("content") {
                    validate_content(content)?;
                }
                if let Some(reason) = candidate.get("finishReason") {
                    if reason.as_str().filter(|reason| !reason.is_empty()).is_none() {
                        return Err(());
                    }
                    if reason != "FINISH_REASON_UNSPECIFIED" {
                        self.completed = true;
                    }
                }
            }
        }

        Ok(value)
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.completed
    }
}

fn validate_content(value: &Value) -> Result<(), ()> {
    let content = value.as_object().ok_or(())?;
    validate_optional_string(content, "role")?;
    if let Some(parts) = content.get("parts") {
        let parts = parts.as_array().ok_or(())?;
        for part in parts {
            let part = part.as_object().ok_or(())?;
            validate_optional_string(part, "text")?;
            if part.get("thought").is_some_and(|value| !value.is_boolean()) {
                return Err(());
            }
            validate_optional_string(part, "thoughtSignature")?;
            if let Some(call) = part.get("functionCall") {
                let call = call.as_object().ok_or(())?;
                call.get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.is_empty())
                    .ok_or(())?;
                if call.get("args").is_some_and(|args| !args.is_object()) {
                    return Err(());
                }
            }
        }
    }
    Ok(())
}

fn validate_usage(value: &Value) -> Result<(), ()> {
    let usage = value.as_object().ok_or(())?;
    for field in [
        "promptTokenCount",
        "candidatesTokenCount",
        "totalTokenCount",
        "cachedContentTokenCount",
        "thoughtsTokenCount",
    ] {
        if usage.get(field).is_some_and(|value| value.as_u64().is_none()) {
            return Err(());
        }
    }
    Ok(())
}

fn validate_optional_string(object: &Map<String, Value>, field: &str) -> Result<(), ()> {
    if object.get(field).is_some_and(|value| !value.is_string()) {
        Err(())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_provider_terminal_evidence_completes_the_source_stream() {
        let mut state = GeminiStreamState::default();
        state
            .accept(r#"{"candidates":[{"content":{"parts":[{"text":"partial"}]}}]}"#)
            .unwrap();
        assert!(!state.is_complete());
        state
            .accept(r#"{"candidates":[{"finishReason":"STOP"}],"usageMetadata":{}}"#)
            .unwrap();
        assert!(state.is_complete());

        let mut blocked = GeminiStreamState::default();
        blocked
            .accept(r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#)
            .unwrap();
        assert!(blocked.is_complete());
    }

    #[test]
    fn malformed_json_and_known_shapes_fail_closed() {
        for data in [
            "not-json",
            "[]",
            r#"{"candidates":"bad"}"#,
            r#"{"candidates":[{"content":{"parts":[{"text":7}]}}]}"#,
            r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"f","args":"bad"}}]}}]}"#,
            r#"{"candidates":[{"finishReason":7}]}"#,
            r#"{"usageMetadata":{"promptTokenCount":"7"}}"#,
            r#"{"error":{"code":"429","message":"bad","status":"RESOURCE_EXHAUSTED"}}"#,
            r#"{"error":{"status":"INTERNAL"}}"#,
        ] {
            assert!(GeminiStreamState::default().accept(data).is_err(), "{data}");
        }
    }

    #[test]
    fn unspecified_enums_are_valid_but_not_terminal_evidence() {
        for data in [
            r#"{"candidates":[{"finishReason":"FINISH_REASON_UNSPECIFIED"}]}"#,
            r#"{"promptFeedback":{"blockReason":"BLOCK_REASON_UNSPECIFIED"}}"#,
        ] {
            let mut state = GeminiStreamState::default();
            state.accept(data).unwrap();
            assert!(!state.is_complete(), "{data}");
        }
    }
}
