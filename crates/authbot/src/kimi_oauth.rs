//! KIMI (Kimi Code) device-code OAuth acquisition for the Auth Bot.
//!
//! Pure protocol unit: it performs the provider exchange, validates the resulting identity and
//! hands back a sealed credential. It owns no Telegram state, no seller job, no payout and no
//! roster publication — those belong to `bot.rs` and `db.rs`, and wiring them is a separate
//! dependent change so this module can be reviewed and tested on its own.
//!
//! Contract: `docs/engine/PROVIDER_ONBOARDING.md` §7; provider facts and their evidence labels:
//! `docs/engine/KIMI_PROVIDER.md` §2.
//!
//! The flow is RFC 8628 device authorization, which is the right shape for a seller handoff: the
//! seller only ever sees a short user code and a verification URL, and never sends a password,
//! a 2FA code, a cookie or a token to the operator.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use kimi_credential::{
    KimiCredential, KimiCredentialKind, KIMI_CODE_BASE_URL, KIMI_DEVICE_AUTHORIZATION_PATH,
    KIMI_DEVICE_GRANT_TYPE, KIMI_IDENTITY_PATH, KIMI_OAUTH_HOST, KIMI_OFFICIAL_OAUTH_CLIENT_ID,
    KIMI_STATUS_NORMAL, KIMI_TOKEN_PATH,
};
use serde::Deserialize;
use zeroize::Zeroizing;

/// Bound on any single provider call. A hung acquisition must not pin a seller job forever.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Floor on the poll interval, so a hostile or buggy `interval` cannot make us hammer the
/// provider. The provider's own value is honoured whenever it is larger.
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Hard ceiling on one acquisition, independent of the provider's `expires_in`.
const MAX_ACQUISITION: Duration = Duration::from_secs(15 * 60);

/// What the seller must be shown to authorize the device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceAuthorization {
    /// Short code the seller confirms in the browser.
    pub user_code: String,
    /// Opaque handle we poll with. Never shown to the seller.
    pub device_code: Zeroizing<String>,
    pub verification_uri: String,
    /// URL with the code already embedded; this is what the bot sends.
    pub verification_uri_complete: String,
    pub expires_in: Option<i64>,
    pub interval: Duration,
}

impl DeviceAuthorization {
    /// Everything the bot may put in a Telegram message. Deliberately excludes `device_code`.
    pub fn seller_prompt(&self) -> (&str, &str) {
        (&self.user_code, &self.verification_uri_complete)
    }
}

/// Outcome of one poll of the token endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DevicePoll {
    /// Seller has not finished yet; keep waiting.
    Pending,
    /// Provider asked us to back off; the interval has already been increased.
    SlowDown,
    /// The device code expired. The bot must restart the flow with a new generation.
    Expired,
    /// The seller refused. Terminal, and never a reason to pay out.
    Denied,
    Granted(Box<TokenSet>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenSet {
    pub access_token: Zeroizing<String>,
    pub refresh_token: Zeroizing<String>,
    pub expires_at: i64,
    pub scope: String,
}

/// Non-secret identity, as published by `/me`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KimiIdentity {
    pub subject_id: String,
    pub plan_name: String,
    pub plan_level: i64,
    pub status: String,
    pub region: String,
}

#[derive(Deserialize)]
struct DeviceAuthorizationResponse {
    user_code: Option<String>,
    device_code: Option<String>,
    verification_uri: Option<String>,
    verification_uri_complete: Option<String>,
    expires_in: Option<i64>,
    interval: Option<i64>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    scope: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct IdentityResponse {
    user_id: Option<String>,
    user_level: Option<i64>,
    user_level_name: Option<String>,
    status: Option<String>,
    region: Option<String>,
}

/// Parse a device authorization response, refusing anything that is missing a load-bearing field.
///
/// Split out from the HTTP call so the wire contract is testable without a network.
pub fn parse_device_authorization(body: &[u8]) -> Result<DeviceAuthorization> {
    let parsed: DeviceAuthorizationResponse =
        serde_json::from_slice(body).context("decode KIMI device authorization")?;
    let user_code = non_empty(parsed.user_code)
        .ok_or_else(|| anyhow!("KIMI device authorization response is missing user_code"))?;
    let device_code = non_empty(parsed.device_code)
        .ok_or_else(|| anyhow!("KIMI device authorization response is missing device_code"))?;
    // Without the complete URI we would have to build one, and a hand-built verification URL is
    // exactly the kind of thing that quietly sends a seller to the wrong place.
    let verification_uri_complete =
        non_empty(parsed.verification_uri_complete).ok_or_else(|| {
            anyhow!("KIMI device authorization response is missing verification_uri_complete")
        })?;
    let interval = parsed
        .interval
        .filter(|value| *value > 0)
        .map(|value| Duration::from_secs(value as u64))
        .unwrap_or(MIN_POLL_INTERVAL)
        .max(MIN_POLL_INTERVAL);
    Ok(DeviceAuthorization {
        user_code,
        device_code: Zeroizing::new(device_code),
        verification_uri: non_empty(parsed.verification_uri).unwrap_or_default(),
        verification_uri_complete,
        expires_in: parsed.expires_in.filter(|value| *value > 0),
        interval,
    })
}

/// Parse a token response into a poll outcome.
///
/// `now_unix` is passed in rather than read from the clock so the expiry computation is testable.
pub fn parse_token_response(status: u16, body: &[u8], now_unix: i64) -> Result<DevicePoll> {
    let parsed: TokenResponse =
        serde_json::from_slice(body).context("decode KIMI token response")?;
    if let Some(error) = parsed.error.as_deref() {
        return Ok(match error {
            "authorization_pending" => DevicePoll::Pending,
            "slow_down" => DevicePoll::SlowDown,
            "expired_token" => DevicePoll::Expired,
            "access_denied" => DevicePoll::Denied,
            // An unrecognised OAuth error is not treated as "keep polling": that would spin until
            // the acquisition deadline and hide a real contract change.
            other => bail!("KIMI device authorization failed: {other}"),
        });
    }
    if status != 200 {
        bail!("KIMI token endpoint returned HTTP {status}");
    }
    let access_token = non_empty(parsed.access_token)
        .ok_or_else(|| anyhow!("KIMI token response is missing access_token"))?;
    // The provider rotates the refresh family on every issuance. A response without one would
    // leave a credential that cannot survive its first refresh.
    let refresh_token = non_empty(parsed.refresh_token)
        .ok_or_else(|| anyhow!("KIMI token response is missing refresh_token"))?;
    let expires_in = parsed
        .expires_in
        .filter(|value| *value > 0)
        .ok_or_else(|| anyhow!("KIMI token response is missing expires_in"))?;
    Ok(DevicePoll::Granted(Box::new(TokenSet {
        access_token: Zeroizing::new(access_token),
        refresh_token: Zeroizing::new(refresh_token),
        expires_at: now_unix
            .checked_add(expires_in)
            .ok_or_else(|| anyhow!("KIMI token expiry overflow"))?,
        scope: parsed.scope.unwrap_or_default(),
    })))
}

/// Parse and validate `/me`.
///
/// Every field this refuses is one that would corrupt calibration or routing later: an empty
/// subject breaks quota attribution, an empty plan collapses distinct cohorts into one, and a
/// non-normal status means the account cannot serve traffic at all.
pub fn parse_identity(body: &[u8]) -> Result<KimiIdentity> {
    let parsed: IdentityResponse = serde_json::from_slice(body).context("decode KIMI identity")?;
    let subject_id =
        non_empty(parsed.user_id).ok_or_else(|| anyhow!("KIMI identity is missing user_id"))?;
    let plan_name = non_empty(parsed.user_level_name)
        .ok_or_else(|| anyhow!("KIMI identity is missing user_level_name"))?;
    let status =
        non_empty(parsed.status).ok_or_else(|| anyhow!("KIMI identity is missing status"))?;
    if status != KIMI_STATUS_NORMAL {
        bail!("KIMI account status is not routable");
    }
    Ok(KimiIdentity {
        subject_id,
        plan_name,
        plan_level: parsed.user_level.unwrap_or_default(),
        status,
        region: non_empty(parsed.region).unwrap_or_default(),
    })
}

/// Build the credential that will be sealed and published.
///
/// The identity is taken from `/me` and never from anything the seller typed.
pub fn credential_from(
    tokens: &TokenSet,
    identity: &KimiIdentity,
    proxy_url: &str,
) -> Result<KimiCredential> {
    let credential = KimiCredential {
        version: 1,
        kind: KimiCredentialKind::Oauth,
        access_token: tokens.access_token.to_string(),
        refresh_token: tokens.refresh_token.to_string(),
        expires_at: tokens.expires_at,
        scope: tokens.scope.clone(),
        subject_id: identity.subject_id.clone(),
        plan_name: identity.plan_name.clone(),
        plan_level: identity.plan_level,
        status: identity.status.clone(),
        region: identity.region.clone(),
        proxy_url: proxy_url.to_string(),
    };
    credential.validate()?;
    Ok(credential)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}

fn client(proxy_url: &str) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().timeout(HTTP_TIMEOUT);
    if !proxy_url.is_empty() {
        // The seller's assigned egress must be used for the whole acquisition: opening the
        // account from one IP and authorizing from another is what trips provider risk checks.
        builder = builder
            .proxy(reqwest::Proxy::all(proxy_url).context("configure KIMI acquisition proxy")?);
    }
    builder.build().context("build KIMI acquisition client")
}

fn oauth_url(path: &str) -> String {
    format!("{}{path}", KIMI_OAUTH_HOST.trim_end_matches('/'))
}

/// Start a device authorization on the seller's assigned egress.
pub async fn request_device_authorization(proxy_url: &str) -> Result<DeviceAuthorization> {
    let response = client(proxy_url)?
        .post(oauth_url(KIMI_DEVICE_AUTHORIZATION_PATH))
        .form(&[("client_id", KIMI_OFFICIAL_OAUTH_CLIENT_ID)])
        .send()
        .await
        .context("request KIMI device authorization")?;
    let status = response.status().as_u16();
    let body = response
        .bytes()
        .await
        .context("read KIMI device authorization")?;
    if status != 200 {
        // The provider body may echo account details, so it is never surfaced verbatim.
        bail!("KIMI device authorization returned HTTP {status}");
    }
    parse_device_authorization(&body)
}

/// Poll once for the seller's decision.
pub async fn poll_device_token(
    proxy_url: &str,
    device_code: &str,
    now_unix: i64,
) -> Result<DevicePoll> {
    let response = client(proxy_url)?
        .post(oauth_url(KIMI_TOKEN_PATH))
        .form(&[
            ("client_id", KIMI_OFFICIAL_OAUTH_CLIENT_ID),
            ("device_code", device_code),
            ("grant_type", KIMI_DEVICE_GRANT_TYPE),
        ])
        .send()
        .await
        .context("poll KIMI device token")?;
    let status = response.status().as_u16();
    let body = response.bytes().await.context("read KIMI device token")?;
    parse_token_response(status, &body, now_unix)
}

/// Fetch and validate the account identity behind an access token.
pub async fn fetch_identity(proxy_url: &str, access_token: &str) -> Result<KimiIdentity> {
    let response = client(proxy_url)?
        .get(format!(
            "{}{KIMI_IDENTITY_PATH}",
            KIMI_CODE_BASE_URL.trim_end_matches('/')
        ))
        .bearer_auth(access_token)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .context("fetch KIMI identity")?;
    let status = response.status().as_u16();
    let body = response.bytes().await.context("read KIMI identity")?;
    if status != 200 {
        bail!("KIMI identity endpoint returned HTTP {status}");
    }
    parse_identity(&body)
}

/// The deadline for one acquisition: the provider's own expiry, bounded by our ceiling.
pub fn acquisition_deadline(authorization: &DeviceAuthorization) -> Duration {
    authorization
        .expires_in
        .and_then(|value| u64::try_from(value).ok())
        .map(Duration::from_secs)
        .unwrap_or(MAX_ACQUISITION)
        .min(MAX_ACQUISITION)
}

/// Back-off applied when the provider says `slow_down`.
pub fn backed_off(interval: Duration) -> Duration {
    interval
        .checked_add(MIN_POLL_INTERVAL)
        .unwrap_or(MIN_POLL_INTERVAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn device_authorization_requires_every_load_bearing_field() {
        let ok = parse_device_authorization(
            br#"{"user_code":"ABCD-1234","device_code":"dev-1",
                 "verification_uri":"https://auth.kimi.com/device",
                 "verification_uri_complete":"https://auth.kimi.com/device?code=ABCD-1234",
                 "expires_in":600,"interval":5}"#,
        )
        .unwrap();
        assert_eq!(ok.user_code, "ABCD-1234");
        assert_eq!(ok.interval, Duration::from_secs(5));

        for missing in [
            br#"{"device_code":"d","verification_uri_complete":"u"}"#.as_slice(),
            br#"{"user_code":"c","verification_uri_complete":"u"}"#.as_slice(),
            br#"{"user_code":"c","device_code":"d"}"#.as_slice(),
        ] {
            assert!(parse_device_authorization(missing).is_err());
        }
    }

    #[test]
    fn the_seller_prompt_never_contains_the_device_code() {
        let authorization = parse_device_authorization(
            br#"{"user_code":"ABCD-1234","device_code":"secret-device-code",
                 "verification_uri_complete":"https://auth.kimi.com/device?code=ABCD-1234"}"#,
        )
        .unwrap();
        let (code, url) = authorization.seller_prompt();
        assert!(!code.contains("secret-device-code"));
        assert!(!url.contains("secret-device-code"));
    }

    #[test]
    fn a_hostile_interval_cannot_make_us_hammer_the_provider() {
        let fast = parse_device_authorization(
            br#"{"user_code":"c","device_code":"d","verification_uri_complete":"u","interval":0}"#,
        )
        .unwrap();
        assert_eq!(fast.interval, MIN_POLL_INTERVAL);

        let slow = parse_device_authorization(
            br#"{"user_code":"c","device_code":"d","verification_uri_complete":"u","interval":30}"#,
        )
        .unwrap();
        assert_eq!(slow.interval, Duration::from_secs(30));
    }

    #[test]
    fn oauth_error_codes_map_to_distinct_outcomes() {
        let cases = [
            (r#"{"error":"authorization_pending"}"#, DevicePoll::Pending),
            (r#"{"error":"slow_down"}"#, DevicePoll::SlowDown),
            (r#"{"error":"expired_token"}"#, DevicePoll::Expired),
            (r#"{"error":"access_denied"}"#, DevicePoll::Denied),
        ];
        for (body, expected) in cases {
            assert_eq!(
                parse_token_response(400, body.as_bytes(), NOW).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn an_unknown_oauth_error_stops_instead_of_spinning() {
        // Treating this as Pending would poll until the deadline and hide a contract change.
        assert!(parse_token_response(400, br#"{"error":"invalid_client"}"#, NOW).is_err());
    }

    #[test]
    fn a_grant_without_a_refresh_token_is_refused() {
        // The refresh family rotates, so a grant with no refresh token dies on first refresh.
        assert!(
            parse_token_response(200, br#"{"access_token":"a","expires_in":3600}"#, NOW).is_err()
        );
    }

    #[test]
    fn a_complete_grant_computes_an_absolute_expiry() {
        let poll = parse_token_response(
            200,
            br#"{"access_token":"a","refresh_token":"r","expires_in":3600,"scope":"coding"}"#,
            NOW,
        )
        .unwrap();
        let DevicePoll::Granted(tokens) = poll else {
            panic!("expected a grant");
        };
        assert_eq!(tokens.expires_at, NOW + 3600);
        assert_eq!(tokens.scope, "coding");
    }

    #[test]
    fn identity_must_carry_a_subject_and_a_paid_plan() {
        let ok = parse_identity(
            br#"{"user_id":"u_123","user_level":30,"user_level_name":"Vivace",
                 "status":"USER_STATUS_NORMAL","region":"REGION_CN","email":"a@b.c"}"#,
        )
        .unwrap();
        assert_eq!(ok.subject_id, "u_123");
        assert_eq!(ok.plan_name, "Vivace");
        assert_eq!(ok.plan_level, 30);

        // An empty plan would collapse distinct calibration cohorts into one.
        assert!(parse_identity(
            br#"{"user_id":"u_1","user_level_name":"","status":"USER_STATUS_NORMAL"}"#
        )
        .is_err());
        assert!(
            parse_identity(br#"{"user_level_name":"Vivace","status":"USER_STATUS_NORMAL"}"#)
                .is_err()
        );
    }

    #[test]
    fn a_non_normal_account_is_refused_before_anything_is_sealed() {
        assert!(parse_identity(
            br#"{"user_id":"u_1","user_level_name":"Vivace","status":"USER_STATUS_BANNED"}"#
        )
        .is_err());
    }

    #[test]
    fn identity_pii_is_not_carried_into_the_parsed_struct() {
        let identity = parse_identity(
            br#"{"user_id":"u_1","user_level_name":"Vivace","status":"USER_STATUS_NORMAL",
                 "email":"seller@example.com","phone":{"country_code":"86","number":"176"},
                 "nickname":"moonwalker"}"#,
        )
        .unwrap();
        let rendered = format!("{identity:?}");
        assert!(!rendered.contains("seller@example.com"));
        assert!(!rendered.contains("moonwalker"));
        assert!(!rendered.contains("176"));
    }

    #[test]
    fn an_acquired_credential_takes_its_identity_from_me() {
        let tokens = TokenSet {
            access_token: Zeroizing::new("access".into()),
            refresh_token: Zeroizing::new("refresh".into()),
            expires_at: NOW + 3600,
            scope: "coding".into(),
        };
        let identity = KimiIdentity {
            subject_id: "u_1".into(),
            plan_name: "Vivace".into(),
            plan_level: 30,
            status: KIMI_STATUS_NORMAL.into(),
            region: "REGION_CN".into(),
        };
        let credential = credential_from(&tokens, &identity, "").unwrap();
        assert_eq!(credential.subject_id, "u_1");
        assert_eq!(credential.plan_name, "Vivace");
        assert_eq!(credential.refresh_token, "refresh");
        // Sealing and publication belong to kimi_roster, which owns the filesystem contract.
    }

    #[test]
    fn a_bad_proxy_fails_the_acquisition_rather_than_leaking_to_direct_egress() {
        // Silently falling back to direct egress would authorize from a different IP than the
        // one the account was opened on.
        let credential = credential_from(
            &TokenSet {
                access_token: Zeroizing::new("a".into()),
                refresh_token: Zeroizing::new("r".into()),
                expires_at: NOW + 60,
                scope: String::new(),
            },
            &KimiIdentity {
                subject_id: "u_1".into(),
                plan_name: "Vivace".into(),
                plan_level: 1,
                status: KIMI_STATUS_NORMAL.into(),
                region: String::new(),
            },
            "file:///etc/passwd",
        );
        assert!(credential.is_err());
    }

    #[test]
    fn the_acquisition_deadline_is_bounded_by_our_own_ceiling() {
        let mut authorization = parse_device_authorization(
            br#"{"user_code":"c","device_code":"d","verification_uri_complete":"u"}"#,
        )
        .unwrap();
        assert_eq!(acquisition_deadline(&authorization), MAX_ACQUISITION);

        authorization.expires_in = Some(60);
        assert_eq!(
            acquisition_deadline(&authorization),
            Duration::from_secs(60)
        );

        // A provider expiry longer than our ceiling must not pin a seller job.
        authorization.expires_in = Some(86_400);
        assert_eq!(acquisition_deadline(&authorization), MAX_ACQUISITION);
    }

    #[test]
    fn slow_down_increases_the_interval() {
        assert_eq!(backed_off(Duration::from_secs(5)), Duration::from_secs(10));
    }
}
