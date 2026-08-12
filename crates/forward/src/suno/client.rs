//! HTTP client for the Suno (suno.com) subscription session-pool plane: the Clerk session
//! wire, the free billing probe, the generation create/poll wire and the per-profile egress
//! client.
//!
//! Contract: `docs/engine/SUNO_PROVIDER.md` §2, §4 and §5.2. This module owns the wire;
//! decisions about what a failure *means* live in [`super::transport`], and which profile to
//! use lives in [`super::selection`]. The Auth Bot's `authbot::suno_session` validates the
//! same wire at intake; the runtime deliberately owns its own client here (authbot is a
//! separate component), reusing its parsing discipline.
//!
//! Every wire fact is `oss-hypothesis` (gcui-art/suno-api, read 2026-08-12), so every parser
//! fails closed on schema deviation:
//!
//! * **Quota counters are raw evidence, never derived at the wire.** `/api/billing/info/`
//!   answers `total_credits_left`/`monthly_limit`/`monthly_usage`/`period` whose semantics are
//!   unproven (manifest §5.2/§6): every counter is kept raw and nullable — unknown is `None`,
//!   never `0` — and a float or string where an integer was reviewed fails closed at decode.
//! * **A success shape is required, never guessed.** Creation answers must name at least one
//!   provider id; a 2xx without one is a lying success envelope and a `Protocol` anomaly, never
//!   a created generation.
//! * **JWTs are never persisted.** They are minted on demand and live in memory only; the
//!   `set-cookie` rotation they may carry is merged back into the sealed cookie by the
//!   single-flight session layer, never here.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use super::transport::{classify_status, UpstreamVerdict};

/// Bound on provider-issued identifiers (session id, JWT, clip ids). Anything longer is a
/// schema deviation and fails closed instead of propagating into a URL path or an envelope.
pub const MAX_PROVIDER_TOKEN_LEN: usize = 8192;

/// Bound on the merged cookie string; mirrors the seal-time bound in
/// `suno_credential::SunoCredential::validate`.
const MAX_COOKIE_LEN: usize = 8192;

/// Extract the Clerk `__client` cookie value from a full cookie string. Its presence with a
/// non-empty value is the local preflight: without it the material cannot mint a JWT at all.
pub fn clerk_client_value(cookie: &str) -> Option<&str> {
    cookie
        .split(';')
        .filter_map(|part| part.split_once('='))
        .find_map(|(name, value)| {
            (name.trim() == suno_credential::SUNO_REQUIRED_COOKIE && !value.trim().is_empty())
                .then(|| value.trim())
        })
}

/// Merge `Set-Cookie` response header values back into the session cookie string.
///
/// A mint (or any session call) may rotate the underlying Clerk material (`oss-hypothesis`,
/// manifest §2), and the rotation is authoritative: each `name=value` pair replaces the
/// same-named entry, an empty value removes the entry, and new names append. Attributes
/// (`Path`, `Expires`, …) are not cookie-jar state for a header replay — only the pair is
/// merged. The merged string stays within the seal-time bound; an over-long or malformed merge
/// fails closed rather than corrupting the sealed envelope.
pub fn merge_set_cookie(cookie: &str, set_cookie: &[&str]) -> Result<String> {
    let mut entries: Vec<(String, String)> = cookie
        .split(';')
        .filter_map(|part| {
            let (name, value) = part.split_once('=')?;
            let name = name.trim();
            let value = value.trim();
            (!name.is_empty() && !value.is_empty())
                .then(|| (name.to_string(), value.to_string()))
        })
        .collect();
    for header in set_cookie {
        // Only the first pair is the cookie; the attributes after the first ';' are metadata.
        let pair = header.split(';').next().unwrap_or_default().trim();
        let Some((name, value)) = pair.split_once('=') else {
            bail!("Suno set-cookie header carries no name=value pair");
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || name.len() > 256 || value.len() > MAX_COOKIE_LEN {
            bail!("Suno set-cookie entry is malformed or oversized");
        }
        // Replace in place (the cookie string's entry order is preserved, so a rotation that
        // changes nothing compares equal and re-seals nothing); an empty value deletes.
        if let Some(position) = entries.iter().position(|(existing, _)| existing == name) {
            entries.remove(position);
            if !value.is_empty() {
                entries.insert(position, (name.to_string(), value.to_string()));
            }
        } else if !value.is_empty() {
            entries.push((name.to_string(), value.to_string()));
        }
    }
    let merged = entries
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    if merged.len() > MAX_COOKIE_LEN {
        bail!("Suno merged cookie exceeds the seal-time bound");
    }
    Ok(merged)
}

/// Outcome of the Clerk session discovery (`GET /v1/client`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionProbe {
    /// The session material was accepted; the active session id was discovered.
    Active { session_id: String },
    /// The auth host rejected the session (HTTP 401/403): the cookie is dead or revoked.
    Invalid,
}

/// Outcome of the short-lived JWT mint (`POST /v1/client/sessions/{sid}/tokens`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JwtMint {
    /// A fresh JWT was minted. It is used for the business-host calls and never persisted.
    Minted { jwt: String },
    /// The auth host rejected the session (HTTP 401/403).
    Invalid,
}

/// Raw billing snapshot: immutable provider-side evidence, semantics unproven
/// (`oss-hypothesis`, manifest §5.2). Every counter is kept raw and nullable; nothing is
/// derived, divided or reinterpreted at the wire.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BillingSnapshot {
    pub total_credits_left: Option<i64>,
    pub monthly_limit: Option<i64>,
    pub monthly_usage: Option<i64>,
    /// The raw `period` value, verbatim. Its format is unproven; when it names a reset it
    /// feeds the observation's `reset_at`.
    pub period_raw: Option<String>,
}

/// Outcome of the free billing probe (`GET /api/billing/info/`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BillingProbe {
    /// The session was accepted; the snapshot is raw quota evidence.
    Valid(BillingSnapshot),
    /// The business host rejected the session (HTTP 401/403).
    Invalid,
}

/// A provider-issued identifier must be bounded before it is substituted into a URL or
/// recorded as evidence.
pub fn checked_provider_token(raw: &str, what: &str) -> Result<String> {
    let token = raw.trim();
    if token.is_empty() || token.len() > MAX_PROVIDER_TOKEN_LEN {
        bail!("Suno {what} is missing or oversized");
    }
    Ok(token.to_string())
}

/// Parse a session-discovery response: HTTP 200 with
/// `response.last_active_session_id`, or the typed auth refusal. Any other shape fails closed.
pub fn parse_session_discovery(status: u16, body: &[u8]) -> Result<SessionProbe> {
    if status == 401 || status == 403 {
        return Ok(SessionProbe::Invalid);
    }
    if status != 200 {
        bail!("Suno session discovery returned HTTP {status}");
    }
    let parsed: Value = serde_json::from_slice(body).context("decode Suno session discovery")?;
    let session_id = parsed
        .get("response")
        .and_then(|response| response.get("last_active_session_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Suno session discovery is missing last_active_session_id"))?;
    Ok(SessionProbe::Active {
        session_id: checked_provider_token(session_id, "session id")?,
    })
}

/// Parse a JWT-mint response: HTTP 200 with a `jwt` field, or the typed auth refusal. The
/// caller reads any `set-cookie` rotation from the response HEADERS — it is not in the body.
pub fn parse_jwt_mint(status: u16, body: &[u8]) -> Result<JwtMint> {
    if status == 401 || status == 403 {
        return Ok(JwtMint::Invalid);
    }
    if status != 200 {
        bail!("Suno JWT mint returned HTTP {status}");
    }
    let parsed: Value = serde_json::from_slice(body).context("decode Suno JWT mint")?;
    let jwt = parsed
        .get("jwt")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Suno JWT mint is missing the jwt field"))?;
    Ok(JwtMint::Minted {
        jwt: checked_provider_token(jwt, "jwt")?,
    })
}

/// A JSON integer counter, or `None` for an absent/null field. A float or string where an
/// integer counter was reviewed is a schema deviation and fails closed at decode time instead
/// of being reinterpreted.
fn raw_counter(value: Option<&Value>) -> Result<Option<i64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_i64()
            .map(Some)
            .ok_or_else(|| anyhow!("Suno billing counter is not an integer")),
        Some(_) => bail!("Suno billing counter is not a number"),
    }
}

/// Parse a billing-probe response. The counters are kept raw and nullable (`oss-hypothesis`,
/// manifest §5.2): no derivation at the wire. Any schema deviation fails closed.
pub fn parse_billing_probe(status: u16, body: &[u8]) -> Result<BillingProbe> {
    if status == 401 || status == 403 {
        return Ok(BillingProbe::Invalid);
    }
    if status != 200 {
        bail!("Suno billing probe returned HTTP {status}");
    }
    let parsed: Value = serde_json::from_slice(body).context("decode Suno billing info")?;
    let period_raw = match parsed.get("period") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => {
            let text = text.trim();
            if text.is_empty() || text.len() > 128 {
                bail!("Suno billing period is empty or oversized");
            }
            Some(text.to_string())
        }
        Some(_) => bail!("Suno billing period is not a string"),
    };
    Ok(BillingProbe::Valid(BillingSnapshot {
        total_credits_left: raw_counter(parsed.get("total_credits_left"))?,
        monthly_limit: raw_counter(parsed.get("monthly_limit"))?,
        monthly_usage: raw_counter(parsed.get("monthly_usage"))?,
        period_raw,
    }))
}

/// Parse the hCaptcha pre-check answer (`POST /api/c/check` `{"ctype":"generation"}` →
/// `{"required": bool}`, `oss-hypothesis`, manifest §4). The field must be a real JSON
/// boolean: a missing or non-boolean `required` is a schema deviation and fails closed —
/// never treated as "not required".
pub fn parse_captcha_check(status: u16, body: &[u8]) -> Result<bool, UpstreamVerdict> {
    let verdict = classify_status(status);
    if verdict != UpstreamVerdict::Ok {
        return Err(verdict);
    }
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|parsed| parsed.get("required").and_then(Value::as_bool))
        .ok_or(UpstreamVerdict::Protocol)
}

/// Lifecycle state of one upstream clip (feed/clip `status`, `oss-hypothesis` manifest §4:
/// ongoing `submitted`/`queued`/`streaming`, finalized `complete`/`error`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipLifecycle {
    Submitted,
    Queued,
    Streaming,
    Complete,
    Error,
}

impl ClipLifecycle {
    /// Whether the clip will never change again. `complete` and `error` are terminal.
    pub fn is_final(self) -> bool {
        matches!(self, Self::Complete | Self::Error)
    }

    /// The provider's exact wire spelling, for our own status projection.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::Queued => "queued",
            Self::Streaming => "streaming",
            Self::Complete => "complete",
            Self::Error => "error",
        }
    }
}

/// The documented status strings; anything else is a contract change and fails closed.
fn parse_clip_lifecycle(raw: &str) -> Option<ClipLifecycle> {
    Some(match raw {
        "submitted" => ClipLifecycle::Submitted,
        "queued" => ClipLifecycle::Queued,
        "streaming" => ClipLifecycle::Streaming,
        "complete" => ClipLifecycle::Complete,
        "error" => ClipLifecycle::Error,
        _ => return None,
    })
}

/// One clip object as the plane acts on it: lifecycle, the model the upstream says it served
/// (raw evidence for the turn event's `served_model`), and the downloadable media URLs.
/// Upstream media URLs are short-lived and never leave the plane (manifest §4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClipState {
    pub id: String,
    pub lifecycle: Option<ClipLifecycle>,
    /// The `model_name` the clip reports, verbatim. `None` while absent — never a fabricated
    /// copy of the requested model (manifest §3).
    pub served_model: Option<String>,
    pub title: Option<String>,
    /// `(name, url)` pairs of the reviewed downloadable media fields.
    pub artifacts: Vec<(String, String)>,
}

/// Output fields the plane downloads on completion. Deliberately a closed allowlist
/// (`oss-hypothesis` clip fields, manifest §4): audio and its cover/video companions.
pub const ARTIFACT_FIELDS: [&str; 3] = ["audio_url", "video_url", "image_url"];

/// Parse one clip object from a feed or clip response.
fn parse_clip_object(value: &Value) -> Result<ClipState, UpstreamVerdict> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= MAX_PROVIDER_TOKEN_LEN)
        .ok_or(UpstreamVerdict::Protocol)?
        .to_string();
    let lifecycle = value
        .get("status")
        .and_then(Value::as_str)
        .and_then(parse_clip_lifecycle)
        .ok_or(UpstreamVerdict::Protocol)?;
    let served_model = value
        .get("model_name")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty() && model.len() <= 256)
        .map(str::to_owned);
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| title.len() <= 1024)
        .map(str::to_owned);
    let mut artifacts = Vec::new();
    for field in ARTIFACT_FIELDS {
        if let Some(url) = value.get(field).and_then(Value::as_str) {
            if !url.is_empty() && url.len() <= 4096 {
                artifacts.push((field.to_string(), url.to_string()));
            }
        }
    }
    Ok(ClipState {
        id,
        lifecycle: Some(lifecycle),
        served_model,
        title,
        artifacts,
    })
}

/// Parse a generation-creation response into the provider ids the drain will poll.
///
/// The reviewed shapes (`oss-hypothesis`, manifest §4): song generation answers a non-empty
/// array of clip objects; the extend/lyrics/stems siblings answer a single object with an `id`.
/// Both and only both are accepted — a 2xx naming no id is a lying success envelope and a
/// `Protocol` anomaly, never a created generation (the money boundary must not be crossed on a
/// guess).
pub fn parse_created_ids(status: u16, body: &[u8]) -> Result<Vec<String>, UpstreamVerdict> {
    let verdict = classify_status(status);
    if verdict != UpstreamVerdict::Ok {
        return Err(verdict);
    }
    let parsed: Value = serde_json::from_slice(body).map_err(|_| UpstreamVerdict::Protocol)?;
    let mut ids = Vec::new();
    if let Some(array) = parsed.as_array() {
        for entry in array {
            let id = entry
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && id.len() <= MAX_PROVIDER_TOKEN_LEN)
                .ok_or(UpstreamVerdict::Protocol)?;
            ids.push(id.to_string());
        }
    } else if let Some(id) = parsed.get("id").and_then(Value::as_str) {
        if id.is_empty() || id.len() > MAX_PROVIDER_TOKEN_LEN {
            return Err(UpstreamVerdict::Protocol);
        }
        ids.push(id.to_string());
    }
    if ids.is_empty() {
        return Err(UpstreamVerdict::Protocol);
    }
    Ok(ids)
}

/// Parse a feed poll (`GET /api/feed/v2?ids=…`): a JSON array of clip objects. A 404 or any
/// non-2xx is classified by status; a malformed array fails closed as `Protocol` — the feed
/// answer is the settlement evidence path, and guessing here would bill on a lie.
pub fn parse_feed_clips(status: u16, body: &[u8]) -> Result<Vec<ClipState>, UpstreamVerdict> {
    let verdict = classify_status(status);
    if verdict != UpstreamVerdict::Ok {
        return Err(verdict);
    }
    let parsed: Value = serde_json::from_slice(body).map_err(|_| UpstreamVerdict::Protocol)?;
    let array = parsed.as_array().ok_or(UpstreamVerdict::Protocol)?;
    array.iter().map(parse_clip_object).collect()
}

/// Parse a single clip read (`GET /api/clip/{clipId}`).
pub fn parse_clip(status: u16, body: &[u8]) -> Result<ClipState, UpstreamVerdict> {
    let verdict = classify_status(status);
    if verdict != UpstreamVerdict::Ok {
        return Err(verdict);
    }
    let parsed: Value = serde_json::from_slice(body).map_err(|_| UpstreamVerdict::Protocol)?;
    parse_clip_object(&parsed)
}

/// A lyrics result (`GET /api/generate/lyrics/{id}`). The status vocabulary is an open
/// `unknown` (manifest §6), so it is kept verbatim: the caller completes only on the exact
/// reviewed `complete`, fails on the exact reviewed `error`, and treats anything else as
/// still pending inside the bounded poll — never as success.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LyricsState {
    pub id: String,
    pub status_raw: Option<String>,
    pub title: Option<String>,
    pub text: Option<String>,
}

/// Parse a lyrics status response. The `id` is required; every other field is optional raw
/// evidence. A missing id fails closed.
pub fn parse_lyrics_state(status: u16, body: &[u8]) -> Result<LyricsState, UpstreamVerdict> {
    let verdict = classify_status(status);
    if verdict != UpstreamVerdict::Ok {
        return Err(verdict);
    }
    let parsed: Value = serde_json::from_slice(body).map_err(|_| UpstreamVerdict::Protocol)?;
    let bounded = |field: &str, max: usize| -> Option<String> {
        parsed
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| value.len() <= max)
            .map(str::to_owned)
    };
    let id = parsed
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= MAX_PROVIDER_TOKEN_LEN)
        .ok_or(UpstreamVerdict::Protocol)?
        .to_string();
    Ok(LyricsState {
        id,
        status_raw: bounded("status", 128),
        title: bounded("title", 1024),
        text: bounded("text", 32 * 1024),
    })
}

/// Build a per-profile client bound to the profile's assigned egress.
///
/// The egress is part of the subscription's identity: the account was opened through it, so
/// traffic from anywhere else looks like a different user to provider risk-control. Every
/// provider call — discovery, mint, generation, quota — goes through this client.
pub fn build_client(
    proxy: &str,
    connect_timeout: Duration,
    read_timeout: Duration,
) -> Result<wreq::Client> {
    let mut builder = wreq::Client::builder()
        .connect_timeout(connect_timeout)
        // A redirect must never carry session material to another origin.
        .redirect(wreq::redirect::Policy::none())
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(60))
        .read_timeout(read_timeout);
    if !proxy.is_empty() {
        builder = builder.proxy(wreq::Proxy::all(proxy).context("configure Suno egress")?);
    }
    builder.build().context("build Suno client")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_discovery_yields_the_active_session_id() {
        let body = br#"{"response":{"last_active_session_id":"sess_2abcdef0123456789"}}"#;
        assert_eq!(
            parse_session_discovery(200, body).unwrap(),
            SessionProbe::Active {
                session_id: "sess_2abcdef0123456789".into()
            }
        );
    }

    #[test]
    fn discovery_rejections_and_schema_deviations_fail_closed() {
        assert_eq!(
            parse_session_discovery(401, b"{}").unwrap(),
            SessionProbe::Invalid
        );
        assert_eq!(
            parse_session_discovery(403, b"{}").unwrap(),
            SessionProbe::Invalid
        );
        assert!(parse_session_discovery(500, b"error").is_err());
        assert!(parse_session_discovery(200, br#"{"response":{}}"#).is_err());
        assert!(
            parse_session_discovery(200, br#"{"response":{"last_active_session_id":""}}"#)
                .is_err()
        );
        assert!(parse_session_discovery(200, b"not json").is_err());
    }

    #[test]
    fn a_valid_mint_yields_a_jwt_and_rejections_are_typed() {
        let body = br#"{"jwt":"header.payload.signature"}"#;
        assert_eq!(
            parse_jwt_mint(200, body).unwrap(),
            JwtMint::Minted {
                jwt: "header.payload.signature".into()
            }
        );
        assert_eq!(parse_jwt_mint(401, b"{}").unwrap(), JwtMint::Invalid);
        assert_eq!(parse_jwt_mint(403, b"{}").unwrap(), JwtMint::Invalid);
        assert!(parse_jwt_mint(200, br#"{}"#).is_err());
        assert!(parse_jwt_mint(200, br#"{"jwt":""}"#).is_err());
        assert!(parse_jwt_mint(502, b"bad gateway").is_err());
    }

    #[test]
    fn a_valid_billing_probe_preserves_the_raw_nullable_counters() {
        let body = br#"{"total_credits_left":2400,"period":"2026-08","monthly_limit":2500,
            "monthly_usage":100,"other_future_field":true}"#;
        let BillingProbe::Valid(snapshot) = parse_billing_probe(200, body).unwrap() else {
            panic!("expected a valid probe");
        };
        assert_eq!(snapshot.total_credits_left, Some(2_400));
        assert_eq!(snapshot.monthly_limit, Some(2_500));
        assert_eq!(snapshot.monthly_usage, Some(100));
        assert_eq!(snapshot.period_raw.as_deref(), Some("2026-08"));
        // Nulls stay null — the semantics are unproven (manifest §5.2), nothing is derived.
        let body = br#"{"total_credits_left":null,"monthly_limit":10000,"monthly_usage":null}"#;
        let BillingProbe::Valid(snapshot) = parse_billing_probe(200, body).unwrap() else {
            panic!("expected a valid probe");
        };
        assert_eq!(snapshot.total_credits_left, None);
        assert_eq!(snapshot.monthly_limit, Some(10_000));
        assert_eq!(snapshot.monthly_usage, None);
        assert_eq!(snapshot.period_raw, None);
    }

    #[test]
    fn billing_schema_deviations_fail_closed() {
        assert_eq!(
            parse_billing_probe(401, b"{}").unwrap(),
            BillingProbe::Invalid
        );
        assert_eq!(
            parse_billing_probe(403, b"{}").unwrap(),
            BillingProbe::Invalid
        );
        // A float or string where an integer counter was reviewed is a contract change.
        assert!(parse_billing_probe(200, br#"{"monthly_limit":2500.5}"#).is_err());
        assert!(parse_billing_probe(200, br#"{"monthly_limit":"2500"}"#).is_err());
        assert!(parse_billing_probe(200, br#"{"period": 202608}"#).is_err());
        assert!(parse_billing_probe(500, b"error").is_err());
        assert!(parse_billing_probe(200, b"not json").is_err());
    }

    #[test]
    fn the_captcha_precheck_requires_a_real_boolean() {
        assert_eq!(parse_captcha_check(200, br#"{"required":false}"#), Ok(false));
        assert_eq!(parse_captcha_check(200, br#"{"required":true}"#), Ok(true));
        // A missing or non-boolean flag fails closed — never treated as "not required".
        assert_eq!(
            parse_captcha_check(200, br#"{}"#),
            Err(UpstreamVerdict::Protocol)
        );
        assert_eq!(
            parse_captcha_check(200, br#"{"required":"yes"}"#),
            Err(UpstreamVerdict::Protocol)
        );
        assert_eq!(
            parse_captcha_check(503, b"down"),
            Err(UpstreamVerdict::Transport)
        );
        assert_eq!(
            parse_captcha_check(401, b"{}"),
            Err(UpstreamVerdict::AuthRefused)
        );
    }

    #[test]
    fn creation_names_at_least_one_provider_id_or_is_no_creation() {
        // Song generation: a non-empty array of clip objects.
        let body = br#"[{"id":"clip-1","status":"queued"},{"id":"clip-2","status":"queued"}]"#;
        assert_eq!(
            parse_created_ids(200, body),
            Ok(vec!["clip-1".to_string(), "clip-2".to_string()])
        );
        // The extend/lyrics/stems siblings: a single object with an id.
        assert_eq!(
            parse_created_ids(200, br#"{"id":"task-1"}"#),
            Ok(vec!["task-1".to_string()])
        );
        // A 2xx naming no id is a lying success envelope: protocol, never a creation.
        assert_eq!(parse_created_ids(200, br#"[]"#), Err(UpstreamVerdict::Protocol));
        assert_eq!(parse_created_ids(200, br#"{}"#), Err(UpstreamVerdict::Protocol));
        assert_eq!(
            parse_created_ids(200, br#"[{"status":"queued"}]"#),
            Err(UpstreamVerdict::Protocol)
        );
        assert_eq!(
            parse_created_ids(200, b"not json"),
            Err(UpstreamVerdict::Protocol)
        );
        // Refusals classify by status (there is no documented business-code layer).
        assert_eq!(
            parse_created_ids(429, b"slow down"),
            Err(UpstreamVerdict::RateLimitedHard)
        );
        assert_eq!(
            parse_created_ids(401, b"{}"),
            Err(UpstreamVerdict::AuthRefused)
        );
    }

    #[test]
    fn the_feed_wire_maps_lifecycle_model_and_artifacts() {
        let body = br#"[{"id":"c1","status":"streaming","model_name":"chirp-v5-5","title":"t"},
            {"id":"c2","status":"complete","model_name":"chirp-v5-5","title":"song",
             "audio_url":"https://cdn.example/a.mp3?sig=1","video_url":"https://cdn.example/v.mp4",
             "image_url":"https://cdn.example/i.jpg","undocumented_extra":"https://cdn.example/x.bin"}]"#;
        let clips = parse_feed_clips(200, body).unwrap();
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].lifecycle, Some(ClipLifecycle::Streaming));
        assert!(!clips[0].lifecycle.unwrap().is_final());
        assert_eq!(clips[1].lifecycle, Some(ClipLifecycle::Complete));
        assert!(clips[1].lifecycle.unwrap().is_final());
        assert_eq!(clips[1].served_model.as_deref(), Some("chirp-v5-5"));
        // Only the reviewed allowlist fields are artifacts; undocumented extras are ignored.
        assert_eq!(
            clips[1].artifacts,
            vec![
                ("audio_url".to_string(), "https://cdn.example/a.mp3?sig=1".to_string()),
                ("video_url".to_string(), "https://cdn.example/v.mp4".to_string()),
                ("image_url".to_string(), "https://cdn.example/i.jpg".to_string()),
            ]
        );
    }

    #[test]
    fn every_documented_clip_status_parses_and_unknown_fails_closed() {
        for (status, terminal) in [
            ("submitted", false),
            ("queued", false),
            ("streaming", false),
            ("complete", true),
            ("error", true),
        ] {
            let body = format!(r#"[{{"id":"c1","status":"{status}"}}]"#);
            let clips = parse_feed_clips(200, body.as_bytes()).unwrap();
            assert_eq!(clips[0].lifecycle.unwrap().is_final(), terminal, "{status}");
        }
        // A status string outside the reviewed five is a contract change: fail closed rather
        // than guess whether it is final.
        let body = br#"[{"id":"c1","status":"paused"}]"#;
        assert_eq!(
            parse_feed_clips(200, body),
            Err(UpstreamVerdict::Protocol)
        );
    }

    #[test]
    fn the_clip_wire_reads_one_object() {
        let body = br#"{"id":"c1","status":"complete","audio_url":"https://cdn.example/a.mp3"}"#;
        let clip = parse_clip(200, body).unwrap();
        assert_eq!(clip.id, "c1");
        assert_eq!(clip.lifecycle, Some(ClipLifecycle::Complete));
        assert_eq!(clip.artifacts.len(), 1);
        assert_eq!(
            parse_clip(404, b"not found"),
            Err(UpstreamVerdict::ClientError)
        );
    }

    #[test]
    fn lyrics_status_is_verbatim_and_needs_its_id() {
        let body = br#"{"id":"lyr_1","status":"complete","title":"t","text":"hello world"}"#;
        let state = parse_lyrics_state(200, body).unwrap();
        assert_eq!(state.id, "lyr_1");
        assert_eq!(state.status_raw.as_deref(), Some("complete"));
        assert_eq!(state.text.as_deref(), Some("hello world"));
        // No id: fail closed.
        assert_eq!(
            parse_lyrics_state(200, br#"{"status":"complete"}"#),
            Err(UpstreamVerdict::Protocol)
        );
        // An unreviewed status is kept verbatim — the caller never treats it as success.
        let body = br#"{"id":"lyr_1","status":"halfway"}"#;
        let state = parse_lyrics_state(200, body).unwrap();
        assert_eq!(state.status_raw.as_deref(), Some("halfway"));
    }

    #[test]
    fn set_cookie_merge_rotates_replaces_and_deletes() {
        let cookie = "__client=old-token; ajs_id=x";
        // Rotation of the Clerk material replaces the entry in place semantically.
        let merged =
            merge_set_cookie(cookie, &["__client=new-token; Path=/; HttpOnly"]).unwrap();
        assert!(merged.contains("__client=new-token"));
        assert!(!merged.contains("old-token"));
        assert!(merged.contains("ajs_id=x"));
        // New entries append; an empty value deletes.
        let merged = merge_set_cookie(
            &merged,
            &["__session=fresh; Path=/", "ajs_id=; Path=/"],
        )
        .unwrap();
        assert!(merged.contains("__session=fresh"));
        assert!(!merged.contains("ajs_id"));
    }

    #[test]
    fn set_cookie_merge_fails_closed_on_malformed_or_oversized_input() {
        assert!(merge_set_cookie("__client=t", &["no-equals-sign"]).is_err());
        let oversized = format!("__client={}", "x".repeat(MAX_PROVIDER_TOKEN_LEN + 1));
        assert!(merge_set_cookie("__client=t", &[&format!("a=b; {oversized}")]).is_ok());
        // The merged string itself must stay inside the seal-time bound.
        let filler = (0..20)
            .map(|i| format!("filler{i}={}", "y".repeat(400)))
            .collect::<Vec<_>>();
        let refs: Vec<&str> = filler.iter().map(String::as_str).collect();
        assert!(merge_set_cookie("__client=t", &refs).is_err());
    }

    #[test]
    fn the_client_cookie_value_is_extracted_before_any_network_call() {
        assert_eq!(
            clerk_client_value("__client=test-token.9f8c7b; ajs_id=x"),
            Some("test-token.9f8c7b")
        );
        assert_eq!(clerk_client_value("ajs_id=x; __session=y"), None);
        assert_eq!(clerk_client_value("__client=; ajs_id=x"), None);
    }

    #[test]
    fn a_client_refuses_a_malformed_egress_instead_of_going_direct() {
        // Falling back to direct egress would make traffic look like a different user than the
        // one who opened the account.
        assert!(build_client("not-a-proxy", Duration::from_secs(5), Duration::from_secs(30)).is_err());
        assert!(build_client("", Duration::from_secs(5), Duration::from_secs(30)).is_ok());
    }
}
