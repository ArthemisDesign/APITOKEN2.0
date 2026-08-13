//! Privacy-bounded diagnostics for Google 429 responses.
//!
//! Provider error bodies are private even when the HTTP status is not: prose and metadata may
//! contain project, account or customer context. This module keeps only closed machine classes and
//! process-keyed correlation fingerprints. It never decides routing, retry or cooling.

use axum::http::HeaderMap;
use serde_json::Value;
use std::sync::OnceLock;

const MAX_MACHINE_FIELD: usize = 64;
const MAX_DETAIL_TYPES: usize = 8;
const MAX_DIAGNOSTIC_BODY_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RateLimitDiagnostic {
    body_state: &'static str,
    google_status: &'static str,
    error_reason: &'static str,
    error_reason_hash: String,
    error_domain_class: &'static str,
    error_domain_hash: String,
    detail_types: String,
    metadata_fields: String,
    message_class: &'static str,
    message_hash: String,
    quota_subject_hash: String,
    quota_description_hash: String,
    error_fingerprint: String,
    retry_hint_source: Option<&'static str>,
    retry_hint_secs: Option<i64>,
}

impl RateLimitDiagnostic {
    pub(crate) fn from_body(headers: Option<&HeaderMap>, body: &[u8]) -> Self {
        // Generation bodies have a much larger public response allowance. Diagnostic parsing must
        // stay independently bounded so an abnormal provider error cannot be cloned/hashed at that
        // response ceiling merely because its HTTP status is 429.
        let (body_state, value) = if body.is_empty() {
            ("missing", None)
        } else if body.len() > MAX_DIAGNOSTIC_BODY_BYTES {
            ("oversized", None)
        } else {
            match serde_json::from_slice::<Value>(body) {
                Ok(value) => ("parsed", Some(value)),
                Err(_) => ("malformed", None),
            }
        };
        let mut diagnostic = Self::from_value(headers, value.as_ref());
        diagnostic.body_state = body_state;
        diagnostic
    }

    pub(crate) fn from_bounded_value(
        headers: Option<&HeaderMap>,
        value: Option<&Value>,
        encoded_len: usize,
    ) -> Self {
        if encoded_len > MAX_DIAGNOSTIC_BODY_BYTES {
            let mut diagnostic = Self::from_value(headers, None);
            diagnostic.body_state = "oversized";
            return diagnostic;
        }
        Self::from_value(headers, value)
    }

    pub(crate) fn from_value(headers: Option<&HeaderMap>, value: Option<&Value>) -> Self {
        let error = value.and_then(|value| value.get("error"));
        let google_status = canonical_google_status(
            error
                .and_then(|error| error.get("status"))
                .and_then(Value::as_str),
        );
        let message = error
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str);
        let error_code = error
            .and_then(|error| error.get("code"))
            .and_then(Value::as_u64);
        let details = error
            .and_then(|error| error.get("details"))
            .and_then(Value::as_array);
        let error_info = details
            .into_iter()
            .flatten()
            .find(|detail| detail_type(detail) == Some("google.rpc.ErrorInfo"));
        let quota_failure = details
            .into_iter()
            .flatten()
            .find(|detail| detail_type(detail) == Some("google.rpc.QuotaFailure"));
        let violation = quota_failure
            .and_then(|detail| detail.get("violations"))
            .and_then(Value::as_array)
            .and_then(|violations| violations.first());
        let raw_error_reason = error_info
            .and_then(|detail| detail.get("reason"))
            .and_then(Value::as_str);
        let domain = error_info
            .and_then(|detail| detail.get("domain"))
            .and_then(Value::as_str);
        let mut detail_types = details
            .into_iter()
            .flatten()
            .filter_map(detail_type)
            .filter(|kind| {
                matches!(
                    *kind,
                    "google.rpc.ErrorInfo"
                        | "google.rpc.QuotaFailure"
                        | "google.rpc.RetryInfo"
                        | "google.rpc.Help"
                        | "google.rpc.LocalizedMessage"
                )
            })
            .map(str::to_string)
            .collect::<Vec<_>>();
        detail_types.sort_unstable();
        detail_types.dedup();
        detail_types.truncate(MAX_DETAIL_TYPES);
        let detail_types = joined_or_none(detail_types);
        let metadata_fields = error_info
            .and_then(|detail| detail.get("metadata"))
            .and_then(Value::as_object)
            .map(|metadata| {
                let mut fields = metadata
                    .keys()
                    .filter_map(|key| match key.as_str() {
                        "consumer" => Some("consumer"),
                        "quota_limit" | "quotaLimit" => Some("quota_limit"),
                        "quota_limit_value" | "quotaLimitValue" => Some("quota_limit_value"),
                        "quota_location" | "quotaLocation" => Some("quota_location"),
                        "service" => Some("service"),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                fields.sort_unstable();
                fields.dedup();
                joined_or_none(fields.into_iter().map(str::to_string).collect())
            })
            .unwrap_or_else(|| "none".to_string());
        let retry_hint = retry_after_header_delay(headers)
            .map(|delay| ("header", delay))
            .or_else(|| {
                value
                    .and_then(retry_info_delay)
                    .map(|delay| ("retry_info", delay))
            });

        Self {
            body_state: if value.is_some() { "parsed" } else { "missing" },
            google_status,
            error_reason: reason_class(raw_error_reason),
            error_reason_hash: raw_error_reason
                .map(correlation_hash)
                .unwrap_or_else(|| "none".to_string()),
            error_domain_class: domain_class(domain),
            error_domain_hash: domain
                .map(correlation_hash)
                .unwrap_or_else(|| "none".to_string()),
            detail_types,
            metadata_fields,
            message_class: message_class(message, google_status, error_code),
            message_hash: message
                .map(correlation_hash)
                .unwrap_or_else(|| "none".to_string()),
            quota_subject_hash: violation
                .and_then(|violation| violation.get("subject"))
                .and_then(Value::as_str)
                .map(correlation_hash)
                .unwrap_or_else(|| "none".to_string()),
            quota_description_hash: violation
                .and_then(|violation| violation.get("description"))
                .and_then(Value::as_str)
                .map(correlation_hash)
                .unwrap_or_else(|| "none".to_string()),
            error_fingerprint: value
                .map(sanitized_error_fingerprint)
                .unwrap_or_else(|| "none".to_string()),
            retry_hint_source: retry_hint.map(|hint| hint.0),
            retry_hint_secs: retry_hint.map(|hint| hint.1),
        }
    }

    pub(crate) fn fields(&self, applied_cool_secs: i64) -> AppliedFields<'_> {
        AppliedFields {
            diagnostic: self,
            applied_cool_secs,
        }
    }
}

pub(crate) struct AppliedFields<'a> {
    diagnostic: &'a RateLimitDiagnostic,
    applied_cool_secs: i64,
}

impl std::fmt::Display for AppliedFields<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let diagnostic = self.diagnostic;
        write!(
            f,
            "diagnostic_body={} google_status={} error_reason={} error_reason_hash={} error_domain_class={} error_domain_hash={} detail_types={} metadata_fields={} message_class={} message_hash={} quota_subject_hash={} quota_description_hash={} error_fingerprint={} retry_hint_source={} retry_hint_secs={} applied_cool_secs={}",
            diagnostic.body_state,
            diagnostic.google_status,
            diagnostic.error_reason,
            diagnostic.error_reason_hash,
            diagnostic.error_domain_class,
            diagnostic.error_domain_hash,
            diagnostic.detail_types,
            diagnostic.metadata_fields,
            diagnostic.message_class,
            diagnostic.message_hash,
            diagnostic.quota_subject_hash,
            diagnostic.quota_description_hash,
            diagnostic.error_fingerprint,
            diagnostic.retry_hint_source.unwrap_or("none"),
            diagnostic
                .retry_hint_secs
                .map_or_else(|| "none".to_string(), |value| value.to_string()),
            self.applied_cool_secs,
        )
    }
}

pub(crate) fn retry_after_header_delay(headers: Option<&HeaderMap>) -> Option<i64> {
    headers?
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<i64>().ok())
        .map(|seconds| seconds.clamp(1, 86_400))
}

pub(crate) fn retry_info_delay(value: &Value) -> Option<i64> {
    let delay = value
        .pointer("/error/details")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|detail| {
            detail
                .get("@type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.ends_with("google.rpc.RetryInfo"))
        })?
        .get("retryDelay")?
        .as_str()?
        .strip_suffix('s')?
        .parse::<f64>()
        .ok()?;
    delay
        .is_finite()
        .then(|| (delay.ceil() as i64).clamp(1, 86_400))
}

fn detail_type(detail: &Value) -> Option<&str> {
    detail
        .get("@type")
        .and_then(Value::as_str)
        .and_then(|kind| kind.strip_prefix("type.googleapis.com/"))
}

fn reason_class(value: Option<&str>) -> &'static str {
    match value {
        Some("RATE_LIMIT_EXCEEDED") => "RATE_LIMIT_EXCEEDED",
        Some("QUOTA_EXCEEDED") => "QUOTA_EXCEEDED",
        Some("RESOURCE_EXHAUSTED") => "RESOURCE_EXHAUSTED",
        Some("USER_RATE_LIMIT_EXCEEDED") => "USER_RATE_LIMIT_EXCEEDED",
        Some("DAILY_LIMIT_EXCEEDED") => "DAILY_LIMIT_EXCEEDED",
        Some("CONCURRENT_LIMIT_EXCEEDED") => "CONCURRENT_LIMIT_EXCEEDED",
        Some("CAPACITY_EXHAUSTED") => "CAPACITY_EXHAUSTED",
        Some("MODEL_CAPACITY_EXCEEDED") => "MODEL_CAPACITY_EXCEEDED",
        Some("MODEL_RATE_LIMIT_EXCEEDED") => "MODEL_RATE_LIMIT_EXCEEDED",
        Some("BACKEND_UNAVAILABLE") => "BACKEND_UNAVAILABLE",
        Some("SERVICE_UNAVAILABLE") => "SERVICE_UNAVAILABLE",
        Some(value)
            if !value.is_empty()
                && value.len() <= MAX_MACHINE_FIELD
                && value.bytes().all(|byte| {
                    byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                }) =>
        {
            "other"
        }
        Some(_) => "invalid",
        None => "none",
    }
}

fn canonical_google_status(value: Option<&str>) -> &'static str {
    match value {
        Some("OK") => "OK",
        Some("CANCELLED") => "CANCELLED",
        Some("UNKNOWN") => "UNKNOWN",
        Some("INVALID_ARGUMENT") => "INVALID_ARGUMENT",
        Some("DEADLINE_EXCEEDED") => "DEADLINE_EXCEEDED",
        Some("NOT_FOUND") => "NOT_FOUND",
        Some("ALREADY_EXISTS") => "ALREADY_EXISTS",
        Some("PERMISSION_DENIED") => "PERMISSION_DENIED",
        Some("UNAUTHENTICATED") => "UNAUTHENTICATED",
        Some("RESOURCE_EXHAUSTED") => "RESOURCE_EXHAUSTED",
        Some("FAILED_PRECONDITION") => "FAILED_PRECONDITION",
        Some("ABORTED") => "ABORTED",
        Some("OUT_OF_RANGE") => "OUT_OF_RANGE",
        Some("UNIMPLEMENTED") => "UNIMPLEMENTED",
        Some("INTERNAL") => "INTERNAL",
        Some("UNAVAILABLE") => "UNAVAILABLE",
        Some("DATA_LOSS") => "DATA_LOSS",
        _ => "unknown",
    }
}

fn domain_class(value: Option<&str>) -> &'static str {
    match value {
        Some("googleapis.com") => "googleapis",
        Some(value) if value.ends_with(".googleapis.com") => "googleapis_service",
        Some("google.com") => "google",
        Some(value) if value.ends_with(".google.com") => "google_service",
        Some(_) => "other",
        None => "none",
    }
}

fn message_class(
    value: Option<&str>,
    google_status: &'static str,
    error_code: Option<u64>,
) -> &'static str {
    let Some(value) = value else {
        return if google_status == "RESOURCE_EXHAUSTED" || error_code == Some(429) {
            "resource_exhausted"
        } else {
            "none"
        };
    };
    if contains_ascii_case_insensitive(value.as_bytes(), b"quota") {
        "quota"
    } else if contains_ascii_case_insensitive(value.as_bytes(), b"rate limit")
        || contains_ascii_case_insensitive(value.as_bytes(), b"rate-limit")
        || contains_ascii_case_insensitive(value.as_bytes(), b"too many request")
    {
        "rate_limit"
    } else if contains_ascii_case_insensitive(value.as_bytes(), b"temporarily unavailable")
        || contains_ascii_case_insensitive(value.as_bytes(), b"backend")
        || contains_ascii_case_insensitive(value.as_bytes(), b"overload")
        || contains_ascii_case_insensitive(value.as_bytes(), b"unavailable")
    {
        "backend_unavailable"
    } else if contains_ascii_case_insensitive(value.as_bytes(), b"resource")
        && contains_ascii_case_insensitive(value.as_bytes(), b"exhaust")
    {
        "resource_exhausted"
    } else if contains_ascii_case_insensitive(value.as_bytes(), b"capacity") {
        "capacity"
    } else if google_status == "RESOURCE_EXHAUSTED" || error_code == Some(429) {
        "resource_exhausted"
    } else {
        "other"
    }
}

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| {
        window
            .iter()
            .zip(needle)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    })
}

fn sanitized_error_fingerprint(value: &Value) -> String {
    let mut sanitized = value.clone();
    if let Some(error) = sanitized.get_mut("error").and_then(Value::as_object_mut) {
        error.remove("message");
        if let Some(details) = error.get_mut("details").and_then(Value::as_array_mut) {
            for detail in details {
                let Some(detail) = detail.as_object_mut() else {
                    continue;
                };
                detail.remove("metadata");
                if let Some(violations) = detail.get_mut("violations").and_then(Value::as_array_mut)
                {
                    for violation in violations {
                        if let Some(violation) = violation.as_object_mut() {
                            violation.remove("subject");
                            violation.remove("description");
                        }
                    }
                }
            }
        }
    }
    correlation_hash(&sanitized.to_string())
}

fn correlation_hash(value: &str) -> String {
    static KEY: OnceLock<Option<[u8; 32]>> = OnceLock::new();
    let Some(key) = KEY
        .get_or_init(|| {
            let mut key = [0u8; 32];
            getrandom::fill(&mut key).ok().map(|_| key)
        })
        .as_ref()
    else {
        return "unavailable".to_string();
    };
    let digest = blake3::keyed_hash(key, value.as_bytes());
    let mut hash = String::with_capacity(16);
    for byte in &digest.as_bytes()[..8] {
        use std::fmt::Write as _;
        let _ = write!(hash, "{byte:02x}");
    }
    hash
}

fn joined_or_none(values: Vec<String>) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use serde_json::json;

    #[test]
    fn keeps_closed_machine_evidence_and_keyed_correlations_without_private_prose() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("7"));
        let first = json!({
            "error": {
                "code": 429,
                "status": "RESOURCE_EXHAUSTED",
                "message": "project paid-project-01 owner@example.invalid is temporarily unavailable",
                "details": [
                    {
                        "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                        "reason": "RATE_LIMIT_EXCEEDED",
                        "domain": "cloudcode-pa.googleapis.com",
                        "metadata": {"consumer": "projects/paid-project-01", "service": "cloudcode-pa.googleapis.com"}
                    },
                    {
                        "@type": "type.googleapis.com/google.rpc.QuotaFailure",
                        "violations": [{
                            "subject": "quotaTypes/generate_content",
                            "description": "private quota description paid-project-01"
                        }]
                    }
                ]
            }
        });
        let second = json!({
            "error": {
                "code": 429,
                "status": "RESOURCE_EXHAUSTED",
                "message": "a different private message and owner",
                "details": [
                    {
                        "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                        "reason": "RATE_LIMIT_EXCEEDED",
                        "domain": "cloudcode-pa.googleapis.com",
                        "metadata": {"consumer": "projects/another-private-project", "service": "cloudcode-pa.googleapis.com"}
                    },
                    {
                        "@type": "type.googleapis.com/google.rpc.QuotaFailure",
                        "violations": [{
                            "subject": "quotaTypes/generate_content",
                            "description": "private quota description paid-project-01"
                        }]
                    }
                ]
            }
        });
        let left = RateLimitDiagnostic::from_value(Some(&headers), Some(&first));
        let right = RateLimitDiagnostic::from_value(Some(&headers), Some(&second));
        let rendered = left.fields(7).to_string();
        assert!(rendered.contains("google_status=RESOURCE_EXHAUSTED"));
        assert!(rendered.contains("error_reason=RATE_LIMIT_EXCEEDED"));
        assert!(rendered.contains("error_domain_class=googleapis_service"));
        assert!(rendered.contains("message_class=backend_unavailable"));
        assert!(rendered.contains("retry_hint_source=header retry_hint_secs=7 applied_cool_secs=7"));
        assert_eq!(left.error_fingerprint, right.error_fingerprint);
        assert_ne!(left.message_hash, right.message_hash);
        assert_eq!(left.quota_subject_hash, right.quota_subject_hash);
        for private in [
            "paid-project-01",
            "another-private-project",
            "owner@example.invalid",
            "private quota description",
        ] {
            assert!(!rendered.contains(private));
        }
    }

    #[test]
    fn rejects_unbounded_reason_and_reports_missing_retry_hint() {
        let value = json!({
            "error": {
                "status": "RESOURCE_EXHAUSTED",
                "details": [{
                    "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                    "reason": "unsafe reason with spaces owner@example.invalid",
                    "domain": "private-project.example"
                }]
            }
        });
        let diagnostic = RateLimitDiagnostic::from_value(None, Some(&value));
        let rendered = diagnostic.fields(60).to_string();
        assert!(rendered.contains("error_reason=invalid"));
        assert!(rendered.contains("error_reason_hash="));
        assert!(rendered.contains("error_domain_class=other"));
        assert!(rendered.contains("message_class=resource_exhausted"));
        assert!(
            rendered.contains("retry_hint_source=none retry_hint_secs=none applied_cool_secs=60")
        );
        assert!(!rendered.contains("owner@example.invalid"));
        assert!(!rendered.contains("private-project.example"));
    }

    #[test]
    fn retry_info_delay_rounds_up_like_generation_cooling() {
        let value = json!({
            "error": {
                "details": [{
                    "@type": "type.googleapis.com/google.rpc.RetryInfo",
                    "retryDelay": "2.25s"
                }]
            }
        });
        assert_eq!(retry_info_delay(&value), Some(3));
    }

    #[test]
    fn provider_hint_and_applied_probe_cooling_remain_distinct() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("2"));
        let diagnostic = RateLimitDiagnostic::from_value(Some(&headers), None);
        assert!(diagnostic
            .fields(60)
            .to_string()
            .contains("retry_hint_source=header retry_hint_secs=2 applied_cool_secs=60"));
    }

    #[test]
    fn oversized_error_body_is_not_parsed_for_diagnostics() {
        let body = vec![b'x'; MAX_DIAGNOSTIC_BODY_BYTES + 1];
        let diagnostic = RateLimitDiagnostic::from_body(None, &body);
        let rendered = diagnostic.fields(60).to_string();
        assert!(rendered.contains("diagnostic_body=oversized"));
        assert!(rendered.contains("google_status=unknown"));
        assert!(rendered.contains("error_fingerprint=none"));
    }
}
