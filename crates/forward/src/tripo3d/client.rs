//! HTTP client for the Tripo3D (VAST / Holymolly) prepaid API plane: the balance probe, the
//! task-creation/poll wire and the per-profile egress client.
//!
//! Contract: `docs/engine/TRIPO3D_PROVIDER.md` §2, §4 and §5.2. This module owns the wire;
//! decisions about what a failure *means* live in [`super::transport`], and which profile to
//! use lives in [`super::selection`].
//!
//! Two provider facts shape the parsing below:
//!
//! * **Balance values are raw evidence, never floats.** The endpoint answers
//!   `data: {"balance": float, "frozen": float}` whose unit is unproven (manifest §5.2/§6), so
//!   the raw decimal text is the authority: `serde_json`'s arbitrary-precision `Number` keeps
//!   the exact token, and nothing here divides or reinterprets it.
//! * **The money authority of a turn is the task's `consumed_credit`.** It is captured as raw
//!   decimal text (its precision is an open `unknown`, manifest §6.2) and converted to integer
//!   millicredits by a strict, float-free parser; anything finer than a millicredit or negative
//!   fails closed as a contract anomaly.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use super::transport::{classify_status, error_business_code, UpstreamVerdict};

/// Raw balance halves exactly as the endpoint returned them. Unit semantics are unproven
/// (manifest §6.1), so the text is the evidence and nothing is derived here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BalanceSnapshot {
    pub balance_raw: String,
    pub frozen_raw: String,
}

/// Outcome of the free balance probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BalanceProbe {
    /// The key was accepted; the snapshot is balance evidence.
    Valid(BalanceSnapshot),
    /// The provider rejected the key (a documented HTTP 401; a `tcli_` Client ID lands here).
    Invalid,
}

/// Outcome of a task-creation attempt. The money boundary is `Created`: from that point the
/// task is owned by the creating profile (per-key isolation, manifest §2) and no rotation is
/// possible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreateOutcome {
    /// `code: 0` with a non-empty `data.task_id`.
    Created(String),
    /// The provider refused before creating anything; the verdict classifies the refusal.
    Refused(UpstreamVerdict),
}

/// Lifecycle state of one upstream task (manifest §4: ongoing `queued`/`running`, finalized
/// `success`/`failed`/`banned`/`expired`/`cancelled`/`unknown`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskLifecycle {
    Queued,
    Running,
    Success,
    Failed,
    Banned,
    Expired,
    Cancelled,
    /// The provider's own "cannot determine" terminal state.
    Unknown,
}

impl TaskLifecycle {
    /// Whether the task will never change again (manifest §4: finalized set).
    pub fn is_final(self) -> bool {
        matches!(
            self,
            Self::Success | Self::Failed | Self::Banned | Self::Expired | Self::Cancelled | Self::Unknown
        )
    }

    /// The provider's exact wire spelling, for our own status projection.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Banned => "banned",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }
}

/// One polled task object: the fields the plane acts on, plus the raw `consumed_credit` money
/// evidence and the downloadable artifact URLs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskState {
    pub lifecycle: Option<TaskLifecycle>,
    /// Provider progress 0–100; meaningless outside `running`/`success` (manifest §4).
    pub progress: Option<i64>,
    /// Raw decimal text of the authoritative consumption. Absent until the provider reports it.
    pub consumed_credit_raw: Option<String>,
    /// Provider error code on a failed task (raw evidence only; never surfaced to customers).
    pub error_code: Option<i64>,
    /// `(name, url)` pairs of the documented downloadable output fields. The URLs are
    /// short-lived (≤60 s, manifest §5.4) and never leave the plane.
    pub artifacts: Vec<(String, String)>,
}

/// Output fields the plane downloads on success. Deliberately a closed allowlist: the provider
/// documents occasional undocumented extra fields (manifest §4), and only the four reviewed
/// model/preview fields of the admitted task shapes become our stored artifacts.
pub const ARTIFACT_FIELDS: [&str; 4] = ["model", "base_model", "pbr_model", "rendered_image"];

/// The documented status strings; anything else is a contract change and fails closed.
fn parse_lifecycle(raw: &str) -> Option<TaskLifecycle> {
    Some(match raw {
        "queued" => TaskLifecycle::Queued,
        "running" => TaskLifecycle::Running,
        "success" => TaskLifecycle::Success,
        "failed" => TaskLifecycle::Failed,
        "banned" => TaskLifecycle::Banned,
        "expired" => TaskLifecycle::Expired,
        "cancelled" => TaskLifecycle::Cancelled,
        "unknown" => TaskLifecycle::Unknown,
        _ => return None,
    })
}

/// A JSON number's exact token, via the arbitrary-precision representation. Strings are NOT
/// accepted: the documented wire is numeric, and a quoted number is a contract change.
fn raw_number(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// Parse a balance probe response. Validity is decided on the documented contract: HTTP 401 is
/// an invalid key; a valid answer is HTTP 200 with `code: 0` and numeric `balance`/`frozen`
/// halves, preserved verbatim as text. Any other shape fails closed.
pub fn parse_balance_probe(status: u16, body: &[u8]) -> Result<BalanceProbe> {
    if status == 401 {
        return Ok(BalanceProbe::Invalid);
    }
    let parsed: Value = serde_json::from_slice(body).context("decode Tripo3D balance response")?;
    if error_business_code(&parsed) == Some(401) {
        return Ok(BalanceProbe::Invalid);
    }
    if status != 200 {
        bail!("Tripo3D balance endpoint returned HTTP {status}");
    }
    if error_business_code(&parsed) != Some(0) {
        bail!("Tripo3D balance endpoint returned no documented success envelope");
    }
    let data = parsed
        .get("data")
        .ok_or_else(|| anyhow!("Tripo3D balance response is missing data"))?;
    let balance_raw = data
        .get("balance")
        .and_then(raw_number)
        .ok_or_else(|| anyhow!("Tripo3D balance half is not a raw number"))?;
    let frozen_raw = data
        .get("frozen")
        .and_then(raw_number)
        .ok_or_else(|| anyhow!("Tripo3D frozen half is not a raw number"))?;
    Ok(BalanceProbe::Valid(BalanceSnapshot {
        balance_raw,
        frozen_raw,
    }))
}

/// Parse a task-creation response. `retry_after` is the parsed `Retry-After` header when the
/// provider sent one; the caller uses it for exact hard-wall cooling. The upstream body is
/// bounded by the caller before this runs.
pub fn parse_create_task(
    status: u16,
    body: &[u8],
) -> Result<CreateOutcome, UpstreamVerdict> {
    let parsed: Option<Value> = serde_json::from_slice(body).ok();
    let code = parsed.as_ref().and_then(error_business_code);
    let verdict = classify_status(status, code);
    if verdict != UpstreamVerdict::Ok {
        return Err(verdict);
    }
    let task_id = parsed
        .as_ref()
        .and_then(|value| value.get("data"))
        .and_then(|data| data.get("task_id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    match task_id {
        Some(task_id) => Ok(CreateOutcome::Created(task_id)),
        // `code: 0` without a task id is a lying success envelope: a contract change, never a
        // created task.
        None => Err(UpstreamVerdict::Protocol),
    }
}

/// Parse a task poll response into the plane's lifecycle view.
///
/// A 404 here is special: tasks are per-key isolated (manifest §2), so polling another
/// profile's task answers "task not found". The gateway pins a created task to its profile, so
/// a 404 on the pinned profile is a provider-side loss — classified `Protocol`, never a silent
/// re-poll forever.
pub fn parse_task_poll(status: u16, body: &[u8]) -> Result<TaskState, UpstreamVerdict> {
    let parsed: Option<Value> = serde_json::from_slice(body).ok();
    let code = parsed.as_ref().and_then(error_business_code);
    let verdict = classify_status(status, code);
    if verdict != UpstreamVerdict::Ok {
        return Err(verdict);
    }
    let data = parsed
        .as_ref()
        .and_then(|value| value.get("data"))
        .ok_or(UpstreamVerdict::Protocol)?;
    let lifecycle = data
        .get("status")
        .and_then(Value::as_str)
        .and_then(parse_lifecycle)
        .ok_or(UpstreamVerdict::Protocol)?;
    let consumed_credit_raw = data.get("consumed_credit").and_then(raw_number);
    let error_code = data
        .get("error_code")
        .and_then(Value::as_i64);
    let progress = data.get("progress").and_then(Value::as_i64);
    let mut artifacts = Vec::new();
    if let Some(output) = data.get("output") {
        for field in ARTIFACT_FIELDS {
            if let Some(url) = output.get(field).and_then(Value::as_str) {
                if !url.is_empty() {
                    artifacts.push((field.to_string(), url.to_string()));
                }
            }
        }
    }
    Ok(TaskState {
        lifecycle: Some(lifecycle),
        progress,
        consumed_credit_raw,
        error_code,
        artifacts,
    })
}

/// Convert the raw `consumed_credit` text to integer millicredits (credits × 1e3).
///
/// Strict and float-free: digits with at most one `.`, no sign, no exponent, at most three
/// decimal places (finer than a millicredit is unrepresentable and fails closed), non-negative.
/// The precision of the provider field is an open `unknown` (manifest §6.2), so anything the
/// parser cannot represent exactly is a typed anomaly, never a rounding.
pub fn millicredits_from_raw(raw: &str) -> Option<i64> {
    let (integer, fraction) = match raw.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (raw, ""),
    };
    if integer.is_empty() || !integer.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) || fraction.len() > 3 {
        return None;
    }
    let whole: i64 = integer.parse().ok()?;
    let fraction_value: i64 = if fraction.is_empty() {
        0
    } else {
        fraction.parse().ok()?
    };
    let scale = 10i64.checked_pow(3 - fraction.len() as u32)?;
    whole
        .checked_mul(1_000)?
        .checked_add(fraction_value.checked_mul(scale)?)
}

/// Build a per-profile client bound to the profile's assigned egress.
///
/// The egress is part of the subscription's identity: the account was opened through it, so
/// traffic from anywhere else looks like a different user to provider risk-control.
pub fn build_client(
    proxy: &str,
    connect_timeout: Duration,
    read_timeout: Duration,
) -> Result<wreq::Client> {
    let mut builder = wreq::Client::builder()
        .connect_timeout(connect_timeout)
        // A redirect must never carry a subscription key to another origin.
        .redirect(wreq::redirect::Policy::none())
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(60))
        .read_timeout(read_timeout);
    if !proxy.is_empty() {
        builder = builder.proxy(wreq::Proxy::all(proxy).context("configure Tripo3D egress")?);
    }
    builder.build().context("build Tripo3D client")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_balance_payload_preserves_raw_halves() {
        let body = br#"{"code":0,"data":{"balance":4850.5,"frozen":20}}"#;
        let BalanceProbe::Valid(snapshot) = parse_balance_probe(200, body).unwrap() else {
            panic!("expected a valid probe");
        };
        // Verbatim text, never a float round-trip: 4850.5 keeps its exact token.
        assert_eq!(snapshot.balance_raw, "4850.5");
        assert_eq!(snapshot.frozen_raw, "20");
    }

    #[test]
    fn http_401_is_an_invalid_key() {
        let body = br#"{"code":401,"message":"invalid api key"}"#;
        assert_eq!(parse_balance_probe(401, body).unwrap(), BalanceProbe::Invalid);
        // The business-code form lands in the same class even under HTTP 200.
        assert_eq!(parse_balance_probe(200, body).unwrap(), BalanceProbe::Invalid);
    }

    #[test]
    fn malformed_and_contract_changed_balance_payloads_fail_closed() {
        assert!(parse_balance_probe(200, b"not json").is_err());
        // Quoted numbers are not the documented wire.
        assert!(parse_balance_probe(200, br#"{"code":0,"data":{"balance":"50","frozen":0}}"#).is_err());
        // Missing halves.
        assert!(parse_balance_probe(200, br#"{"code":0,"data":{"balance":50}}"#).is_err());
        // No documented success envelope.
        assert!(parse_balance_probe(200, br#"{"code":7,"data":{"balance":50,"frozen":0}}"#).is_err());
        // A non-200 status on an otherwise valid envelope is transport-class.
        assert!(parse_balance_probe(500, br#"{"code":0,"data":{"balance":50,"frozen":0}}"#).is_err());
    }

    #[test]
    fn a_created_task_names_its_upstream_id() {
        let body = br#"{"code":0,"data":{"task_id":"07764597-9c93-4eb9-92b6-4ea96a8c7d1a"}}"#;
        assert_eq!(
            parse_create_task(200, body),
            Ok(CreateOutcome::Created(
                "07764597-9c93-4eb9-92b6-4ea96a8c7d1a".into()
            ))
        );
    }

    #[test]
    fn creation_refusals_classify_on_the_business_code() {
        let wall = br#"{"code":2000,"message":"concurrency exceeded"}"#;
        assert_eq!(
            parse_create_task(429, wall),
            Err(UpstreamVerdict::RateLimitedHard)
        );
        let balance = br#"{"code":2010,"message":"insufficient balance"}"#;
        assert_eq!(
            parse_create_task(403, balance),
            Err(UpstreamVerdict::InsufficientBalance)
        );
        let key = br#"{"code":401,"message":"invalid key"}"#;
        assert_eq!(parse_create_task(401, key), Err(UpstreamVerdict::AuthRefused));
        // `code: 0` without a task id is a lying success, not a creation.
        assert_eq!(
            parse_create_task(200, br#"{"code":0,"data":{}}"#),
            Err(UpstreamVerdict::Protocol)
        );
    }

    #[test]
    fn the_poll_wire_maps_lifecycle_progress_money_and_artifacts() {
        let body = br#"{"code":0,"data":{"task_id":"t1","type":"image_to_model","status":"running",
            "progress":42,"consumed_credit":0,"queuing_num":-1,"running_left_time":30}}"#;
        let state = parse_task_poll(200, body).unwrap();
        assert_eq!(state.lifecycle, Some(TaskLifecycle::Running));
        assert_eq!(state.progress, Some(42));
        assert_eq!(state.consumed_credit_raw.as_deref(), Some("0"));
        assert!(state.artifacts.is_empty());

        let done = br#"{"code":0,"data":{"task_id":"t1","type":"image_to_model","status":"success",
            "progress":100,"consumed_credit":30,"output":{"model":"https://cdn.example/m.glb?sig=1",
            "pbr_model":"https://cdn.example/m_pbr.glb?sig=2","rendered_image":"https://cdn.example/r.jpg?sig=3",
            "undocumented_extra":"https://cdn.example/x.bin"}}}"#;
        let state = parse_task_poll(200, done).unwrap();
        assert_eq!(state.lifecycle, Some(TaskLifecycle::Success));
        assert!(state.lifecycle.unwrap().is_final());
        assert_eq!(state.consumed_credit_raw.as_deref(), Some("30"));
        // Only the reviewed allowlist fields are artifacts; undocumented extras are ignored.
        assert_eq!(
            state.artifacts,
            vec![
                ("model".to_string(), "https://cdn.example/m.glb?sig=1".to_string()),
                ("pbr_model".to_string(), "https://cdn.example/m_pbr.glb?sig=2".to_string()),
                ("rendered_image".to_string(), "https://cdn.example/r.jpg?sig=3".to_string()),
            ]
        );
    }

    #[test]
    fn every_documented_finalized_status_parses_and_is_final() {
        for status in ["success", "failed", "banned", "expired", "cancelled", "unknown"] {
            let body = format!(
                r#"{{"code":0,"data":{{"task_id":"t1","status":"{status}","progress":0,"consumed_credit":0}}}}"#
            );
            let state = parse_task_poll(200, body.as_bytes()).unwrap();
            assert!(state.lifecycle.unwrap().is_final(), "{status}");
        }
        for status in ["queued", "running"] {
            let body = format!(
                r#"{{"code":0,"data":{{"task_id":"t1","status":"{status}","progress":0}}}}"#
            );
            let state = parse_task_poll(200, body.as_bytes()).unwrap();
            assert!(!state.lifecycle.unwrap().is_final(), "{status}");
        }
    }

    #[test]
    fn an_undocumented_status_or_lying_envelope_is_a_protocol_anomaly() {
        // A status string outside the documented eight is a contract change: fail closed
        // rather than guess whether it is final.
        let body = br#"{"code":0,"data":{"task_id":"t1","status":"paused"}}"#;
        assert_eq!(parse_task_poll(200, body), Err(UpstreamVerdict::Protocol));
        assert_eq!(
            parse_task_poll(200, br#"{"code":0}"#),
            Err(UpstreamVerdict::Protocol)
        );
        // A 404 on a pinned task is per-key isolation evidence, not a lifecycle.
        assert_eq!(
            parse_task_poll(404, br#"{"code":1404,"message":"task not found"}"#),
            Err(UpstreamVerdict::ClientError)
        );
    }

    #[test]
    fn consumed_credit_converts_to_exact_millicredits() {
        assert_eq!(millicredits_from_raw("30"), Some(30_000));
        assert_eq!(millicredits_from_raw("0.5"), Some(500));
        assert_eq!(millicredits_from_raw("0.125"), Some(125));
        assert_eq!(millicredits_from_raw("0"), Some(0));
        // Finer than a millicredit, signed, exponent or junk: fail closed, never rounded.
        assert_eq!(millicredits_from_raw("0.0001"), None);
        assert_eq!(millicredits_from_raw("-1"), None);
        assert_eq!(millicredits_from_raw("1e2"), None);
        assert_eq!(millicredits_from_raw(""), None);
        assert_eq!(millicredits_from_raw("1."), Some(1_000));
        assert_eq!(millicredits_from_raw(".5"), None);
    }

    #[test]
    fn a_client_refuses_a_malformed_egress_instead_of_going_direct() {
        // Falling back to direct egress would make traffic look like a different user than the
        // one who opened the account.
        assert!(build_client("not-a-proxy", Duration::from_secs(5), Duration::from_secs(30)).is_err());
        assert!(build_client("", Duration::from_secs(5), Duration::from_secs(30)).is_ok());
    }
}
