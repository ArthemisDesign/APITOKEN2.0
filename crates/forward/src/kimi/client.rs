//! HTTP client for the KIMI (Kimi Code) plane: token refresh and the quota poll.
//!
//! Contract: `docs/engine/KIMI_PROVIDER.md` §2 and §5.2. This module owns the wire; decisions
//! about what a failure *means* live in [`super::transport`], and which profile to use lives in
//! [`super::selection`].
//!
//! **No TLS impersonation.** Gemini needs a pinned client fingerprint because Google gates on it.
//! KIMI documents pointing third-party tools at its endpoint directly, so a plain client is the
//! honest choice; adding an impersonation layer would be unproven complexity on a route the
//! provider explicitly supports.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use kimi_credential::{KIMI_OAUTH_HOST, KIMI_OFFICIAL_OAUTH_CLIENT_ID, KIMI_TOKEN_PATH};
use registry::{
    kimi_fraction_from_native, kimi_window_duration_secs, KIMI_WEEKLY_WINDOW_SECS,
};
use serde_json::Value;

/// One quota window exactly as the provider reported it.
///
/// Raw integers are kept verbatim: their unit is not normatively documented, so they are evidence
/// rather than a quantity we may reinterpret. The derived fraction sits beside them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaWindow {
    /// Exact native duration in seconds. The window's identity.
    pub duration_secs: i64,
    /// Provider label, audit metadata only.
    pub name: Option<String>,
    pub used_units: i64,
    pub limit_units: i64,
    pub used_fraction_units: i64,
    pub measurement_resolution_fraction_units: i64,
    /// Exact RFC3339 reset instant normalized to Unix seconds.
    pub resets_at: i64,
}

/// Extra Usage wallet: real money, a ledger of its own that mixes with neither the API-dollar nor
/// the native-quota accounting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoosterWallet {
    pub balance_cents: i64,
    pub total_cents: i64,
    pub currency: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuotaSnapshot {
    pub windows: Vec<QuotaWindow>,
    pub wallet: Option<BoosterWallet>,
}

/// Fixed-point divisor the provider uses for wallet amounts: value / 1_000_000 == cents.
const WALLET_FIXED_POINT: i64 = 1_000_000;

fn as_int(value: Option<&Value>) -> Option<i64> {
    match value? {
        // The provider serialises these as decimal strings; accept a number too rather than
        // dropping a window over a representation change.
        Value::String(text) => text.parse::<i64>().ok(),
        Value::Number(number) => number.as_i64(),
        _ => None,
    }
}

fn reset_from(raw: &Value) -> Result<i64> {
    let value = raw
        .get("resetTime")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| anyhow!("KIMI quota window has no reset time"))?;
    parse_rfc3339_seconds(value)
        .filter(|timestamp| *timestamp > 0)
        .ok_or_else(|| anyhow!("KIMI quota window has an invalid reset time"))
}

fn window_from(detail: &Value, duration_secs: i64, name: Option<String>) -> Result<QuotaWindow> {
    let used = as_int(detail.get("used")).unwrap_or(0);
    let limit = as_int(detail.get("limit"))
        .ok_or_else(|| anyhow!("KIMI quota window has no limit"))?;
    let derived = kimi_fraction_from_native(used, limit)?;
    Ok(QuotaWindow {
        duration_secs,
        name,
        used_units: used,
        limit_units: limit,
        used_fraction_units: derived.used_fraction_units,
        measurement_resolution_fraction_units: derived.measurement_resolution_fraction_units,
        resets_at: reset_from(detail)?,
    })
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

/// Parse a `/usages` payload.
///
/// The summary object carries the plan's weekly quota but omits its window, so the documented
/// seven-day duration is applied explicitly rather than synthesised silently — the mapping is a
/// recorded fact, not an inference. Entries in `limits` carry their own duration and are trusted
/// verbatim; an entry whose time unit we do not recognise is skipped rather than coerced, because
/// a wrong duration would merge two independent windows into one calibration row.
pub fn parse_usage(payload: &Value) -> Result<QuotaSnapshot> {
    let mut windows = Vec::new();

    if let Some(summary) = payload.get("usage") {
        if summary.is_object() {
            windows.push(
                window_from(summary, KIMI_WEEKLY_WINDOW_SECS, None)
                    .context("KIMI weekly quota summary")?,
            );
        }
    }

    if let Some(entries) = payload.get("limits").and_then(Value::as_array) {
        for entry in entries {
            let Some(detail) = entry.get("detail").filter(|value| value.is_object()) else {
                continue;
            };
            let Some(window) = entry.get("window") else {
                continue;
            };
            let Some(duration) = as_int(window.get("duration")) else {
                continue;
            };
            let unit = window.get("timeUnit").and_then(Value::as_str).unwrap_or("");
            let Ok(duration_secs) = kimi_window_duration_secs(duration, unit) else {
                // Unknown unit: record nothing rather than guess a duration.
                continue;
            };
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_string);
            windows.push(window_from(detail, duration_secs, name)?);
        }
    }

    // Two entries of the same duration would collide on the calibration primary key.
    let mut seen = std::collections::HashSet::new();
    for window in &windows {
        if !seen.insert(window.duration_secs) {
            bail!("KIMI quota payload repeats a window duration");
        }
    }

    Ok(QuotaSnapshot {
        windows,
        wallet: parse_wallet(payload.get("boosterWallet")),
    })
}

fn parse_wallet(raw: Option<&Value>) -> Option<BoosterWallet> {
    let raw = raw?;
    let balance = raw.get("balance")?;
    if balance.get("type").and_then(Value::as_str) != Some("BOOSTER") {
        return None;
    }
    let total = as_int(balance.get("amount"))?;
    if total <= 0 {
        return None;
    }
    let left = as_int(balance.get("amountLeft")).unwrap_or(0);
    let currency = raw
        .get("monthlyChargeLimit")
        .or_else(|| raw.get("monthlyUsed"))
        .and_then(|money| money.get("currency"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .unwrap_or("USD")
        .to_string();
    Some(BoosterWallet {
        balance_cents: to_cents(left),
        total_cents: to_cents(total),
        currency,
    })
}

/// Fixed point to whole cents. A positive sub-cent amount rounds up to one cent so a non-empty
/// wallet never renders as empty.
fn to_cents(value: i64) -> i64 {
    if value > 0 && value < WALLET_FIXED_POINT {
        return 1;
    }
    value / WALLET_FIXED_POINT
}

/// A refreshed token set. Both halves are always present: the family rotates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefreshedTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub scope: String,
}

/// Parse a refresh response.
///
/// A response without a new refresh token is refused. The provider invalidates the presented one
/// on every exchange, so accepting it would leave the profile pinned to a token that is already
/// dead and fail on its next refresh instead of here, where the cause is still visible.
pub fn parse_refresh(payload: &Value, now_unix: i64) -> Result<RefreshedTokens> {
    let access_token = payload
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| anyhow!("KIMI refresh response is missing access_token"))?;
    let refresh_token = payload
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| anyhow!("KIMI refresh response did not rotate the refresh token"))?;
    let expires_in = payload
        .get("expires_in")
        .and_then(|value| match value {
            Value::Number(number) => number.as_i64(),
            Value::String(text) => text.parse::<i64>().ok(),
            _ => None,
        })
        .filter(|seconds| *seconds > 0)
        .ok_or_else(|| anyhow!("KIMI refresh response has no usable expires_in"))?;
    Ok(RefreshedTokens {
        access_token: access_token.to_string(),
        refresh_token: refresh_token.to_string(),
        expires_at: now_unix
            .checked_add(expires_in)
            .ok_or_else(|| anyhow!("KIMI token expiry overflow"))?,
        scope: payload
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// Form body for a refresh exchange.
pub fn refresh_form(refresh_token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("client_id", KIMI_OFFICIAL_OAUTH_CLIENT_ID.to_string()),
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
    ]
}

pub fn refresh_url() -> String {
    format!("{}{KIMI_TOKEN_PATH}", KIMI_OAUTH_HOST.trim_end_matches('/'))
}

/// Build a per-profile client bound to the profile's assigned egress.
///
/// The egress is part of the subscription's identity: the account was opened through it, so
/// traffic from anywhere else looks like a different user to the provider.
pub fn build_client(proxy: &str, connect_timeout: Duration, read_timeout: Duration) -> Result<wreq::Client> {
    let mut builder = wreq::Client::builder()
        .connect_timeout(connect_timeout)
        // The subscription endpoint identifies the official CLI by its User-Agent; a bare client
        // fingerprint risks looking like an unrelated bot.
        .user_agent(kimi_credential::KIMI_CODE_CLI_USER_AGENT)
        // A redirect must never carry a subscription bearer to another origin.
        .redirect(wreq::redirect::Policy::none())
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(60))
        .read_timeout(read_timeout);
    if !proxy.is_empty() {
        builder = builder
            .proxy(wreq::Proxy::all(proxy).context("configure KIMI egress")?);
    }
    builder.build().context("build KIMI client")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FIVE_HOURS: i64 = 18_000;

    #[test]
    fn the_documented_payload_parses_into_independent_windows() {
        let payload = json!({
            "usage": {"used": "40", "limit": "1000", "resetTime": "2026-08-10T05:20:51Z"},
            "limits": [{
                "name": "rate",
                "window": {"duration": 300, "timeUnit": "TIME_UNIT_MINUTE"},
                "detail": {"used": "1", "limit": "100", "resetTime": "2026-08-03T10:00:00Z"}
            }]
        });
        let snapshot = parse_usage(&payload).unwrap();
        assert_eq!(snapshot.windows.len(), 2);

        let weekly = &snapshot.windows[0];
        assert_eq!(weekly.duration_secs, KIMI_WEEKLY_WINDOW_SECS);
        assert_eq!(weekly.used_units, 40);
        assert_eq!(weekly.limit_units, 1_000);
        assert_eq!(weekly.resets_at, 1_786_339_251);
        // 40/1000 == 4%, and one native unit is 0.1% — far finer than a whole percent.
        assert_eq!(weekly.used_fraction_units, 4_000_000);
        assert_eq!(weekly.measurement_resolution_fraction_units, 100_000);

        let rolling = &snapshot.windows[1];
        assert_eq!(rolling.duration_secs, FIVE_HOURS);
        assert_eq!(rolling.name.as_deref(), Some("rate"));
        // limit=100 means one unit is a whole percent.
        assert_eq!(rolling.measurement_resolution_fraction_units, 1_000_000);
    }

    #[test]
    fn the_weekly_summary_gets_its_documented_duration_rather_than_a_guess() {
        // The backend omits the window on the summary; the seven-day duration is a recorded fact
        // from the provider's own docs, applied explicitly.
        let payload = json!({"usage": {
            "used": "0", "limit": "10", "resetTime": "2099-01-01T00:00:00Z"
        }});
        let snapshot = parse_usage(&payload).unwrap();
        assert_eq!(snapshot.windows[0].duration_secs, KIMI_WEEKLY_WINDOW_SECS);
    }

    #[test]
    fn an_unknown_time_unit_is_skipped_rather_than_coerced() {
        // A wrong duration would merge two independent windows into one calibration row.
        let payload = json!({
            "limits": [{
                "window": {"duration": 1, "timeUnit": "TIME_UNIT_FORTNIGHT"},
                "detail": {"used": "1", "limit": "10"}
            }]
        });
        assert!(parse_usage(&payload).unwrap().windows.is_empty());
    }

    #[test]
    fn a_repeated_window_duration_fails_closed() {
        // Two rows of the same duration would collide on the calibration primary key.
        let payload = json!({
            "limits": [
                {"window": {"duration": 5, "timeUnit": "TIME_UNIT_HOUR"},
                 "detail": {"used": "1", "limit": "10", "resetTime": "2099-01-01T00:00:00Z"}},
                {"window": {"duration": 300, "timeUnit": "TIME_UNIT_MINUTE"},
                 "detail": {"used": "2", "limit": "10", "resetTime": "2099-01-01T00:00:00Z"}}
            ]
        });
        assert!(parse_usage(&payload).is_err());
    }

    #[test]
    fn integers_are_accepted_as_strings_or_numbers() {
        let payload = json!({"usage": {
            "used": 40, "limit": 1000, "resetTime": "2099-01-01T00:00:00Z"
        }});
        assert_eq!(parse_usage(&payload).unwrap().windows[0].used_units, 40);
    }

    #[test]
    fn a_window_without_a_limit_fails_closed() {
        // Without a denominator there is no fraction and no resolution; inventing one would
        // fabricate calibration evidence.
        let payload = json!({"usage": {"used": "40"}});
        assert!(parse_usage(&payload).is_err());
    }

    #[test]
    fn used_above_limit_fails_closed() {
        let payload = json!({"usage": {
            "used": "11", "limit": "10", "resetTime": "2099-01-01T00:00:00Z"
        }});
        assert!(parse_usage(&payload).is_err());
    }

    #[test]
    fn a_missing_or_invalid_reset_fails_closed() {
        for payload in [
            json!({"usage": {"used": "1", "limit": "10"}}),
            json!({"usage": {
                "used": "1", "limit": "10", "resetTime": "2099-02-30T00:00:00Z"
            }}),
        ] {
            assert!(parse_usage(&payload).is_err());
        }
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
            "2000-01-01T00:00:00.Z",
        ] {
            assert_eq!(parse_rfc3339_seconds(invalid), None, "accepted {invalid}");
        }
    }

    #[test]
    fn an_empty_payload_is_an_empty_snapshot_not_an_error() {
        let snapshot = parse_usage(&json!({})).unwrap();
        assert!(snapshot.windows.is_empty());
        assert!(snapshot.wallet.is_none());
    }

    #[test]
    fn the_booster_wallet_is_read_as_whole_cents_in_its_own_currency() {
        let payload = json!({
            "boosterWallet": {
                "balance": {"type": "BOOSTER", "amount": 25_000_000, "amountLeft": 3_000_000},
                "monthlyChargeLimit": {"priceInCents": 0, "currency": "CNY"}
            }
        });
        let wallet = parse_usage(&payload).unwrap().wallet.unwrap();
        assert_eq!(wallet.total_cents, 25);
        assert_eq!(wallet.balance_cents, 3);
        assert_eq!(wallet.currency, "CNY");
    }

    #[test]
    fn a_non_empty_wallet_never_rounds_down_to_empty() {
        let payload = json!({
            "boosterWallet": {
                "balance": {"type": "BOOSTER", "amount": 10_000_000, "amountLeft": 1}
            }
        });
        let wallet = parse_usage(&payload).unwrap().wallet.unwrap();
        assert_eq!(wallet.balance_cents, 1);
        assert_eq!(wallet.currency, "USD");
    }

    #[test]
    fn a_wallet_of_another_type_is_ignored() {
        let payload = json!({
            "boosterWallet": {"balance": {"type": "SOMETHING_ELSE", "amount": 100}}
        });
        assert!(parse_usage(&payload).unwrap().wallet.is_none());
    }

    #[test]
    fn a_refresh_without_a_new_refresh_token_is_refused() {
        // The presented token is already dead by the time the response arrives, so accepting this
        // would fail later with no visible cause.
        let payload = json!({"access_token": "a", "expires_in": 3600});
        let error = parse_refresh(&payload, 100).unwrap_err();
        assert!(error.to_string().contains("did not rotate"));
    }

    #[test]
    fn a_complete_refresh_yields_an_absolute_expiry() {
        let payload = json!({
            "access_token": "a2", "refresh_token": "r2", "expires_in": 3600, "scope": "coding"
        });
        let tokens = parse_refresh(&payload, 1_000).unwrap();
        assert_eq!(tokens.access_token, "a2");
        assert_eq!(tokens.refresh_token, "r2");
        assert_eq!(tokens.expires_at, 4_600);
        assert_eq!(tokens.scope, "coding");
    }

    #[test]
    fn a_refresh_without_a_usable_lifetime_is_refused() {
        for payload in [
            json!({"access_token": "a", "refresh_token": "r"}),
            json!({"access_token": "a", "refresh_token": "r", "expires_in": 0}),
            json!({"access_token": "a", "refresh_token": "r", "expires_in": -5}),
        ] {
            assert!(parse_refresh(&payload, 100).is_err());
        }
    }

    #[test]
    fn the_refresh_exchange_targets_the_official_client_and_host() {
        assert_eq!(refresh_url(), "https://auth.kimi.com/api/oauth/token");
        let form = refresh_form("r1");
        assert!(form.contains(&("grant_type", "refresh_token".to_string())));
        assert!(form.contains(&("refresh_token", "r1".to_string())));
        assert!(form
            .iter()
            .any(|(key, value)| *key == "client_id" && value == KIMI_OFFICIAL_OAUTH_CLIENT_ID));
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
