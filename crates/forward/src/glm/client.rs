//! HTTP client for the GLM (Zhipu AI / Z.ai) Coding Plan plane: the quota poll and the wire
//! shape of the quota endpoint. There is no token refresh client — the credential is a static
//! API key and nothing here negotiates with an OAuth host.
//!
//! Contract: `docs/engine/GLM_PROVIDER.md` §2.1, §4 and §5.2. This module owns the wire;
//! decisions about what a failure *means* live in [`super::transport`], and which profile to
//! use lives in [`super::selection`].
//!
//! Two provider facts shape the parsing below:
//!
//! * **The endpoint rejects under HTTP 200.** An invalid key comes back as HTTP 200 with
//!   `code: 401` in the body (oss-hypothesis, manifest §2.1/§4.2), so validity is decided on
//!   the business code, never on the status line.
//! * **Counter units are unproven** (manifest §5.2/§6.3): `unit`/`number`/`usage`/
//!   `currentValue`/`remaining` are kept raw as `Option` and are never reinterpreted or
//!   divided by token prices. A derived used-fraction exists only for the one documented form
//!   (credits `TIME_LIMIT` with `number > 0`), and even that is `oss-hypothesis`-grade: the
//!   live gate may refute it, so the raw fields stay the primary evidence.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use registry::{glm_fraction_from_native, GLM_5H_WINDOW_SECS, GLM_WEEKLY_WINDOW_SECS};
use serde::Deserialize;
use serde_json::Value;

/// One plan window exactly as the quota endpoint returned it. Raw evidence: every field is
/// optional and uninterpreted (`unknown` units, manifest §6).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuotaLimit {
    /// Raw `type` discriminator (`TIME_LIMIT`, `TOKENS_LIMIT`, …).
    pub limit_type: String,
    pub unit: Option<i64>,
    pub number: Option<i64>,
    pub usage: Option<i64>,
    pub current_value: Option<i64>,
    pub remaining: Option<i64>,
    pub percentage: Option<f64>,
    /// Provider-side next reset, epoch milliseconds (raw).
    pub next_reset_time_ms: Option<i64>,
}

/// One quota window mapped onto the documented credits shape, for calibration and steering.
///
/// The raw counters ride along unchanged; the derived fraction pair is either both `Some`
/// (the documented credits form) or both `None` — never one without the other.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuotaWindow {
    /// Exact native duration in seconds: [`GLM_5H_WINDOW_SECS`] or [`GLM_WEEKLY_WINDOW_SECS`].
    /// The window's identity.
    pub duration_secs: i64,
    pub used_units: Option<i64>,
    pub limit_units: Option<i64>,
    pub remaining_units: Option<i64>,
    /// `nextResetTime` normalised to Unix seconds when the provider supplied it; a rolling
    /// window may not name one.
    pub resets_at: Option<i64>,
    /// Derived only for the documented credits form (`TIME_LIMIT` with `number > 0`):
    /// `used_fraction = usage / number`. Evidence level `oss-hypothesis` — unit semantics are
    /// unproven (manifest §6.3) and the live gate may refute this reading.
    pub used_fraction_units: Option<i64>,
    pub measurement_resolution_fraction_units: Option<i64>,
}

/// A parsed quota snapshot: raw limits plus the mapped windows and the per-model attribution
/// evidence (`usageDetails[].modelCode`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuotaSnapshot {
    pub limits: Vec<QuotaLimit>,
    pub windows: Vec<QuotaWindow>,
    pub model_codes: Vec<String>,
}

/// Outcome of the free quota probe.
#[derive(Clone, Debug, PartialEq)]
pub enum QuotaProbe {
    /// The key was accepted; the snapshot is quota evidence.
    Valid(QuotaSnapshot),
    /// The provider rejected the key. The trap of this endpoint is that rejection arrives as
    /// **HTTP 200 with `code: 401` in the body**, so this is decided on the business code,
    /// never on the HTTP status.
    Invalid,
}

#[derive(Deserialize)]
struct QuotaResponse {
    code: Option<i64>,
    success: Option<bool>,
    data: Option<QuotaData>,
    // `msg` is deliberately not carried: provider text may echo account details and is never
    // surfaced to logs.
}

#[derive(Deserialize)]
struct QuotaData {
    #[serde(default)]
    limits: Vec<QuotaLimitWire>,
    #[serde(default, rename = "usageDetails")]
    usage_details: Vec<QuotaUsageDetailWire>,
}

#[derive(Deserialize)]
struct QuotaLimitWire {
    #[serde(rename = "type")]
    limit_type: Option<String>,
    unit: Option<i64>,
    number: Option<i64>,
    usage: Option<i64>,
    #[serde(rename = "currentValue")]
    current_value: Option<i64>,
    remaining: Option<i64>,
    percentage: Option<f64>,
    #[serde(rename = "nextResetTime")]
    next_reset_time_ms: Option<i64>,
}

#[derive(Deserialize)]
struct QuotaUsageDetailWire {
    #[serde(rename = "modelCode")]
    model_code: Option<String>,
}

/// Parse a quota probe response, deciding validity on the **business code** (oss-hypothesis
/// wire contract, `docs/engine/GLM_PROVIDER.md` §4.2). Same contract as the Auth Bot side
/// (`authbot::glm_key::parse_quota_probe`), so onboarding and runtime read the wire identically.
///
/// Split out from the HTTP call so the wire contract is testable without a network.
pub fn parse_quota_probe(status: u16, body: &[u8]) -> Result<QuotaProbe> {
    let parsed: QuotaResponse =
        serde_json::from_slice(body).context("decode GLM quota response")?;
    // The invalid-key trap: HTTP 200 with `code: 401` in the body. Checked before anything
    // else so a proxy that surfaces a real HTTP 401 lands in the same class.
    if parsed.code == Some(401) {
        return Ok(QuotaProbe::Invalid);
    }
    if parsed.success == Some(false) {
        return Ok(QuotaProbe::Invalid);
    }
    // A business code that is neither a rejection nor a known success marker is a contract
    // change: fail closed instead of trusting whatever `data` carries.
    if let Some(code) = parsed.code {
        if code != 0 && code != 200 {
            bail!("GLM quota endpoint returned unrecognised business code {code}");
        }
    }
    if parsed.success != Some(true) {
        bail!("GLM quota response is missing its success flag");
    }
    if status != 200 {
        bail!("GLM quota endpoint returned HTTP {status}");
    }
    let data = parsed
        .data
        .ok_or_else(|| anyhow!("GLM quota response is missing data"))?;

    let limits: Vec<QuotaLimit> = data
        .limits
        .into_iter()
        .map(|wire| QuotaLimit {
            limit_type: wire.limit_type.unwrap_or_default(),
            unit: wire.unit,
            number: wire.number,
            usage: wire.usage,
            current_value: wire.current_value,
            remaining: wire.remaining,
            percentage: wire.percentage,
            next_reset_time_ms: wire.next_reset_time_ms,
        })
        .collect();
    let windows = map_windows(&limits)?;
    let model_codes = data
        .usage_details
        .into_iter()
        .filter_map(|wire| wire.model_code)
        .filter(|code| !code.is_empty())
        .collect();

    Ok(QuotaProbe::Valid(QuotaSnapshot {
        limits,
        windows,
        model_codes,
    }))
}

/// Map raw limits onto the documented credits windows. Anything incompatible with the
/// individual credits shape fails closed (`docs/engine/GLM_PROVIDER.md` §5.3): a `TOKENS_LIMIT`
/// or otherwise unknown window type is Team/legacy mechanics, a `TIME_LIMIT` window whose
/// `unit` names neither the 5-hour nor the weekly window is a contract change, and two windows
/// of the same duration would collide on the calibration primary key.
fn map_windows(limits: &[QuotaLimit]) -> Result<Vec<QuotaWindow>> {
    let mut windows = Vec::with_capacity(limits.len());
    for limit in limits {
        if limit.limit_type != "TIME_LIMIT" {
            bail!("GLM quota payload carries a non-credits window type");
        }
        let duration_secs = match limit.unit {
            Some(5) => GLM_5H_WINDOW_SECS,
            Some(7) => GLM_WEEKLY_WINDOW_SECS,
            _ => bail!("GLM quota window has an unrecognised time unit"),
        };
        if windows
            .iter()
            .any(|window: &QuotaWindow| window.duration_secs == duration_secs)
        {
            bail!("GLM quota payload repeats a window duration");
        }
        windows.push(QuotaWindow {
            duration_secs,
            used_units: limit.usage,
            limit_units: limit.number,
            remaining_units: limit.remaining,
            resets_at: limit.next_reset_time_ms.and_then(epoch_ms_to_seconds),
            ..QuotaWindow::default()
        });
    }
    // The derived fraction is computed in a second pass so a counter anomaly on one window
    // can never discard the raw evidence of the other.
    for (window, limit) in windows.iter_mut().zip(limits.iter()) {
        let (Some(used), Some(total)) = (limit.usage, limit.number) else {
            continue;
        };
        if total <= 0 || used < 0 || used > total {
            // Raw fields are kept; an unproven-unit counter anomaly must not fail the whole
            // snapshot, but it must not fabricate a fraction either.
            continue;
        }
        let derived = glm_fraction_from_native(used, total)?;
        window.used_fraction_units = Some(derived.used_fraction_units);
        window.measurement_resolution_fraction_units =
            Some(derived.measurement_resolution_fraction_units);
    }
    Ok(windows)
}

/// Epoch milliseconds to Unix seconds. A non-positive or overflowing value names no reset —
/// the raw field on the limit stays the evidence.
fn epoch_ms_to_seconds(epoch_ms: i64) -> Option<i64> {
    if epoch_ms <= 0 {
        return None;
    }
    Some(epoch_ms / 1_000)
}

/// Best-effort reset instant for a quota-wall error body (`1308`/`1310`).
///
/// Two shapes are tried, in order: an `error.nextResetTime` epoch-milliseconds field, and the
/// documented «reset at {next_flush_time}» marker in `error.message` parsed as strict RFC3339.
/// The exact message format is unproven (manifest §6.6), so anything unparseable returns
/// `None` — the caller then cools by the window's documented duration rather than trusting a
/// guess.
pub fn quota_wall_reset(payload: &Value) -> Option<i64> {
    let error = payload.get("error")?;
    if let Some(ms) = match error.get("nextResetTime") {
        Some(Value::Number(number)) => number.as_i64(),
        Some(Value::String(text)) => text.trim().parse::<i64>().ok(),
        _ => None,
    } {
        if let Some(seconds) = epoch_ms_to_seconds(ms) {
            return Some(seconds);
        }
    }
    let message = error.get("message")?.as_str()?;
    let marker = message.find("reset at ")? + "reset at ".len();
    let candidate = message[marker..].trim().trim_end_matches('.');
    parse_rfc3339_seconds(candidate).filter(|timestamp| *timestamp > 0)
}

/// Strict RFC3339-to-Unix conversion for provider reset timestamps. Fractional seconds are
/// accepted but ignored because the durable window authority is second-granular.
fn parse_rfc3339_seconds(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let number = |start: usize, end: usize| value.get(start..end)?.parse::<i64>().ok();
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if !(1..=days_in_month).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut timezone_index = 19usize;
    if bytes.get(timezone_index) == Some(&b'.') {
        timezone_index += 1;
        let fraction_start = timezone_index;
        while bytes.get(timezone_index).is_some_and(u8::is_ascii_digit) {
            timezone_index += 1;
        }
        if timezone_index == fraction_start {
            return None;
        }
    }
    let offset = match bytes.get(timezone_index).copied()? {
        b'Z' if timezone_index + 1 == bytes.len() => 0,
        sign @ (b'+' | b'-') if timezone_index + 6 == bytes.len() => {
            if bytes[timezone_index + 3] != b':' {
                return None;
            }
            let hours = value
                .get(timezone_index + 1..timezone_index + 3)?
                .parse::<i64>()
                .ok()?;
            let minutes = value
                .get(timezone_index + 4..timezone_index + 6)?
                .parse::<i64>()
                .ok()?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let seconds = hours * 3_600 + minutes * 60;
            if sign == b'+' {
                seconds
            } else {
                -seconds
            }
        }
        _ => return None,
    };
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    days.checked_mul(86_400)?
        .checked_add(hour * 3_600 + minute * 60 + second)?
        .checked_sub(offset)
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
        builder = builder.proxy(wreq::Proxy::all(proxy).context("configure GLM egress")?);
    }
    builder.build().context("build GLM client")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_documented_payload_parses_raw_fields_windows_and_model_codes() {
        let body = br#"{
            "code": 200, "msg": "success", "success": true,
            "data": {
                "limits": [
                    {"type":"TIME_LIMIT","unit":5,"number":2000,"usage":120,
                     "currentValue":1880,"remaining":1880,"percentage":6.0,
                     "nextResetTime":1800003600000},
                    {"type":"TIME_LIMIT","unit":7,"number":10000,"usage":0,
                     "currentValue":10000,"remaining":10000,"percentage":0,
                     "nextResetTime":1800604800000}
                ],
                "usageDetails": [{"modelCode":"glm-4.7"},{"modelCode":"glm-5.2"}]
            }
        }"#;
        let QuotaProbe::Valid(snapshot) = parse_quota_probe(200, body).unwrap() else {
            panic!("expected a valid probe");
        };
        // Raw layer: uninterpreted, every field preserved (unknown units, manifest §6).
        assert_eq!(snapshot.limits.len(), 2);
        let five_hour = &snapshot.limits[0];
        assert_eq!(five_hour.limit_type, "TIME_LIMIT");
        assert_eq!(five_hour.unit, Some(5));
        assert_eq!(five_hour.number, Some(2_000));
        assert_eq!(five_hour.usage, Some(120));
        assert_eq!(five_hour.current_value, Some(1_880));
        assert_eq!(five_hour.remaining, Some(1_880));
        assert_eq!(five_hour.percentage, Some(6.0));
        assert_eq!(five_hour.next_reset_time_ms, Some(1_800_003_600_000));

        // Mapped windows: 5h and weekly, with the derived fraction on the documented form.
        assert_eq!(snapshot.windows.len(), 2);
        let window = &snapshot.windows[0];
        assert_eq!(window.duration_secs, GLM_5H_WINDOW_SECS);
        assert_eq!(window.used_units, Some(120));
        assert_eq!(window.limit_units, Some(2_000));
        assert_eq!(window.resets_at, Some(1_800_003_600));
        // 120/2000 == 6%, one native unit resolves to 0.05%.
        assert_eq!(window.used_fraction_units, Some(6_000_000));
        assert_eq!(window.measurement_resolution_fraction_units, Some(50_000));

        let weekly = &snapshot.windows[1];
        assert_eq!(weekly.duration_secs, GLM_WEEKLY_WINDOW_SECS);
        assert_eq!(weekly.used_fraction_units, Some(0));
        assert_eq!(weekly.resets_at, Some(1_800_604_800));

        assert_eq!(snapshot.model_codes, ["glm-4.7", "glm-5.2"]);
    }

    #[test]
    fn http_200_with_business_code_401_is_an_invalid_key() {
        // The trap of this endpoint: rejection arrives as HTTP 200, so the business code
        // decides — never the status.
        let body = br#"{"code":401,"msg":"invalid api key","success":false,"data":null}"#;
        assert_eq!(parse_quota_probe(200, body).unwrap(), QuotaProbe::Invalid);
        // The same business code on a real HTTP 401 lands in the same class.
        assert_eq!(parse_quota_probe(401, body).unwrap(), QuotaProbe::Invalid);
    }

    #[test]
    fn success_false_is_an_invalid_key_even_with_http_200() {
        let body = br#"{"code":200,"msg":"rejected","success":false,"data":null}"#;
        assert_eq!(parse_quota_probe(200, body).unwrap(), QuotaProbe::Invalid);
    }

    #[test]
    fn malformed_and_contract_changed_payloads_fail_closed() {
        // Not JSON at all.
        assert!(parse_quota_probe(200, b"not json").is_err());
        // Missing success flag.
        assert!(parse_quota_probe(200, br#"{"code":200,"data":{}}"#).is_err());
        // Unrecognised business code: a contract change, never trusted data.
        assert!(parse_quota_probe(200, br#"{"code":1308,"success":true,"data":{}}"#).is_err());
        // A non-200 status on an otherwise valid envelope is transport-class.
        let body = br#"{"code":200,"success":true,"data":{"limits":[]}}"#;
        assert!(parse_quota_probe(500, body).is_err());
        // Missing data object.
        assert!(parse_quota_probe(200, br#"{"code":200,"success":true}"#).is_err());
    }

    #[test]
    fn credits_incompatible_window_shapes_fail_closed() {
        // Team token mechanics: TOKENS_LIMIT is not the individual credits shape (§5.3).
        let team = br#"{"code":200,"success":true,"data":{"limits":[
            {"type":"TOKENS_LIMIT","unit":5,"number":60000000,"usage":1}]}}"#;
        assert!(parse_quota_probe(200, team).is_err());
        // A TIME_LIMIT window whose unit names neither documented window.
        let unknown_unit = br#"{"code":200,"success":true,"data":{"limits":[
            {"type":"TIME_LIMIT","unit":30,"number":1000,"usage":1}]}}"#;
        assert!(parse_quota_probe(200, unknown_unit).is_err());
        // Two windows of the same duration would collide on the calibration key.
        let duplicate = br#"{"code":200,"success":true,"data":{"limits":[
            {"type":"TIME_LIMIT","unit":5,"number":1000,"usage":1},
            {"type":"TIME_LIMIT","unit":5,"number":2000,"usage":2}]}}"#;
        assert!(parse_quota_probe(200, duplicate).is_err());
    }

    #[test]
    fn the_derived_fraction_exists_only_for_the_proven_form() {
        // usage above number: raw evidence kept, no fabricated fraction.
        let body = br#"{"code":200,"success":true,"data":{"limits":[
            {"type":"TIME_LIMIT","unit":5,"number":100,"usage":120}]}}"#;
        let QuotaProbe::Valid(snapshot) = parse_quota_probe(200, body).unwrap() else {
            panic!("expected a valid probe");
        };
        assert_eq!(snapshot.windows[0].used_units, Some(120));
        assert_eq!(snapshot.windows[0].used_fraction_units, None);
        assert_eq!(
            snapshot.windows[0].measurement_resolution_fraction_units,
            None
        );

        // No readable total: same rule.
        let body = br#"{"code":200,"success":true,"data":{"limits":[
            {"type":"TIME_LIMIT","unit":5,"usage":120}]}}"#;
        let QuotaProbe::Valid(snapshot) = parse_quota_probe(200, body).unwrap() else {
            panic!("expected a valid probe");
        };
        assert_eq!(snapshot.windows[0].limit_units, None);
        assert_eq!(snapshot.windows[0].used_fraction_units, None);
    }

    #[test]
    fn reset_times_normalise_from_epoch_ms_to_seconds() {
        assert_eq!(epoch_ms_to_seconds(1_800_003_600_000), Some(1_800_003_600));
        assert_eq!(epoch_ms_to_seconds(0), None);
        assert_eq!(epoch_ms_to_seconds(-5), None);
        // A window that names no reset keeps `resets_at` empty rather than inventing one.
        let body = br#"{"code":200,"success":true,"data":{"limits":[
            {"type":"TIME_LIMIT","unit":5,"number":100,"usage":1}]}}"#;
        let QuotaProbe::Valid(snapshot) = parse_quota_probe(200, body).unwrap() else {
            panic!("expected a valid probe");
        };
        assert_eq!(snapshot.windows[0].resets_at, None);
    }

    #[test]
    fn a_quota_wall_reset_comes_from_next_reset_time_or_the_documented_marker() {
        // Structured field wins.
        let body = json!({"error": {"code": "1308", "message": "quota exhausted",
            "nextResetTime": 1800003600000i64}});
        assert_eq!(quota_wall_reset(&body), Some(1_800_003_600));
        // The documented «reset at …» message marker, strict RFC3339.
        let body = json!({"error": {"code": "1308",
            "message": "5-hour quota exhausted, reset at 2038-01-19T03:14:07Z."}});
        assert_eq!(quota_wall_reset(&body), Some(2_147_483_647));
        // An unparseable message names no reset: the caller cools by the documented window
        // duration instead of trusting a guess.
        let body = json!({"error": {"code": "1308", "message": "quota exhausted"}});
        assert_eq!(quota_wall_reset(&body), None);
        let body = json!({"error": {"code": "1310",
            "message": "weekly quota exhausted, reset at next week"}});
        assert_eq!(quota_wall_reset(&body), None);
    }

    #[test]
    fn reset_parser_accepts_offsets_and_fractional_seconds() {
        assert_eq!(
            parse_rfc3339_seconds("2000-01-01T05:00:00.125+05:00"),
            Some(946_684_800)
        );
        for invalid in [
            "2000-01-01T00:00:00",
            "2000-13-01T00:00:00Z",
            "2000-01-01T00:00:00+24:00",
            "2000-02-30T00:00:00Z",
        ] {
            assert_eq!(parse_rfc3339_seconds(invalid), None, "accepted {invalid}");
        }
    }

    #[test]
    fn a_client_refuses_a_malformed_egress_instead_of_going_direct() {
        // Falling back to direct egress would make traffic look like a different user than the
        // one who opened the account.
        assert!(build_client(
            "not-a-proxy",
            Duration::from_secs(5),
            Duration::from_secs(30)
        )
        .is_err());
        assert!(build_client("", Duration::from_secs(5), Duration::from_secs(30)).is_ok());
    }
}
