//! Router-owned provider preferences plus the engine-owned account-policy preflight client.
//!
//! Preferences are deterministic request planning. Account policy is intentionally not duplicated
//! here: the router sends one bounded canonical chain to the fixed provider runtimes and accepts
//! the first valid, exact ordered-subset response. Credentials and decisions live only for the
//! request and are never logged or cached.

use std::collections::HashSet;
use std::time::Duration;

use axum::http::HeaderMap;
use serde::{Deserialize, Deserializer, Serialize};

use crate::bounded;
use crate::catalog::{Plane, PlaneOrigins};
use crate::error::Lane;

const SCHEMA_VERSION: u64 = 1;
pub const MAX_CANDIDATES: usize = 32;
const MAX_BODY_BYTES: usize = 64 * 1024;
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(2);
const PREFLIGHT_PATH: &str = "/internal/router/policy/preflight";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderNamespace {
    Anthropic,
    OpenAi,
    Google,
    Kimi,
}

impl ProviderNamespace {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Google => "google",
            Self::Kimi => "kimi",
        }
    }

    pub fn lane(self) -> Lane {
        match self {
            Self::Anthropic => Lane::Anthropic,
            Self::OpenAi => Lane::OpenAi,
            Self::Google => Lane::Gemini,
            // KIMI speaks the Anthropic Messages protocol. `Lane` selects the wire
            // envelope, not the serving origin. Dataplane origin is `plane()`.
            Self::Kimi => Lane::Anthropic,
        }
    }

    pub fn plane(self) -> Plane {
        match self {
            Self::Anthropic => Plane::Anthropic,
            Self::OpenAi => Plane::OpenAi,
            Self::Google => Plane::Gemini,
            Self::Kimi => Plane::Kimi,
        }
    }
}

#[derive(Debug, Default)]
struct OptionalProviderList(Option<Vec<ProviderNamespace>>);

impl<'de> Deserialize<'de> for OptionalProviderList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Vec::<ProviderNamespace>::deserialize(deserializer).map(|values| Self(Some(values)))
    }
}

fn default_allow_fallbacks() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPreferences {
    #[serde(default)]
    order: OptionalProviderList,
    #[serde(default)]
    only: OptionalProviderList,
    #[serde(default)]
    ignore: OptionalProviderList,
    #[serde(default = "default_allow_fallbacks")]
    allow_fallbacks: bool,
}

impl Default for ProviderPreferences {
    fn default() -> Self {
        Self {
            order: OptionalProviderList::default(),
            only: OptionalProviderList::default(),
            ignore: OptionalProviderList::default(),
            allow_fallbacks: true,
        }
    }
}

impl ProviderPreferences {
    pub fn parse(value: &serde_json::Value) -> Result<Self, ()> {
        let parsed: Self = serde_json::from_value(value.clone()).map_err(|_| ())?;
        for values in [&parsed.order, &parsed.only, &parsed.ignore] {
            if values
                .0
                .as_ref()
                .is_some_and(|values| has_duplicates(values))
            {
                return Err(());
            }
        }
        if let (Some(only), Some(ignore)) = (&parsed.only.0, &parsed.ignore.0) {
            if only.iter().any(|provider| ignore.contains(provider)) {
                return Err(());
            }
        }
        Ok(parsed)
    }

    pub fn allows(&self, provider: ProviderNamespace) -> bool {
        if self
            .only
            .0
            .as_ref()
            .is_some_and(|only| !only.contains(&provider))
        {
            return false;
        }
        !self
            .ignore
            .0
            .as_ref()
            .is_some_and(|ignore| ignore.contains(&provider))
    }

    pub fn order_rank(&self, provider: ProviderNamespace) -> usize {
        let Some(order) = &self.order.0 else {
            return 0;
        };
        order
            .iter()
            .position(|candidate| *candidate == provider)
            .unwrap_or(order.len())
    }

    pub fn allow_fallbacks(&self) -> bool {
        self.allow_fallbacks
    }
}

fn has_duplicates(values: &[ProviderNamespace]) -> bool {
    let mut seen = HashSet::with_capacity(values.len());
    values.iter().any(|value| !seen.insert(*value))
}

pub struct PolicyCandidate<'a> {
    pub id: &'a str,
    pub provider: ProviderNamespace,
    pub canonical_model_id: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreflightError {
    Unauthorized,
    Unavailable,
    Restricted,
}

#[derive(Serialize)]
struct PreflightRequest<'a> {
    schema_version: u64,
    candidates: Vec<WireCandidate<'a>>,
}

#[derive(Serialize)]
struct WireCandidate<'a> {
    id: &'a str,
    provider_id: &'static str,
    canonical_model_id: &'a str,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum PolicyMode {
    Unrestricted,
    Strict,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreflightResponse {
    schema_version: u64,
    mode: PolicyMode,
    allowed: Vec<String>,
}

/// Apply the engine-owned account policy once to the final logical chain.
pub async fn preflight(
    client: &reqwest::Client,
    origins: &PlaneOrigins<'_>,
    auth: &HeaderMap,
    candidates: &[PolicyCandidate<'_>],
) -> Result<Vec<String>, PreflightError> {
    if candidates.is_empty() || candidates.len() > MAX_CANDIDATES {
        return Err(PreflightError::Unavailable);
    }
    let request = PreflightRequest {
        schema_version: SCHEMA_VERSION,
        candidates: candidates
            .iter()
            .map(|candidate| WireCandidate {
                id: candidate.id,
                provider_id: candidate.provider.as_str(),
                canonical_model_id: candidate.canonical_model_id,
            })
            .collect(),
    };
    let body = serde_json::to_vec(&request).map_err(|_| PreflightError::Unavailable)?;
    if body.len() > MAX_BODY_BYTES {
        return Err(PreflightError::Unavailable);
    }

    for plane in origin_order(candidates) {
        let origin = origins.for_plane(plane);
        let response = match client
            .post(format!("{origin}{PREFLIGHT_PATH}"))
            .headers(auth.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .timeout(PREFLIGHT_TIMEOUT)
            .body(body.clone())
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => continue,
        };
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(PreflightError::Unauthorized);
        }
        if !response.status().is_success() {
            continue;
        }
        let Ok(bytes) = bounded::response_bytes(response, MAX_BODY_BYTES).await else {
            continue;
        };
        let Ok(response) = serde_json::from_slice::<PreflightResponse>(&bytes) else {
            continue;
        };
        let Some(allowed) = validate_response(candidates, response) else {
            continue;
        };
        if allowed.is_empty() {
            return Err(PreflightError::Restricted);
        }
        return Ok(allowed);
    }
    elog::warn("router-policy", "policy authority unavailable");
    Err(PreflightError::Unavailable)
}

fn origin_order(candidates: &[PolicyCandidate<'_>]) -> Vec<Plane> {
    let mut order = Vec::with_capacity(Plane::ALL.len());
    for plane in candidates
        .iter()
        .map(|candidate| candidate.provider.plane())
        .chain(Plane::ALL)
    {
        if !order.contains(&plane) {
            order.push(plane);
        }
    }
    order
}

fn validate_response(
    candidates: &[PolicyCandidate<'_>],
    response: PreflightResponse,
) -> Option<Vec<String>> {
    if response.schema_version != SCHEMA_VERSION || response.allowed.len() > candidates.len() {
        return None;
    }
    let mut previous = None;
    for id in &response.allowed {
        let index = candidates.iter().position(|candidate| candidate.id == id)?;
        if previous.is_some_and(|previous| index <= previous) {
            return None;
        }
        previous = Some(index);
    }
    if response.mode == PolicyMode::Unrestricted
        && response
            .allowed
            .iter()
            .map(String::as_str)
            .ne(candidates.iter().map(|candidate| candidate.id))
    {
        return None;
    }
    Some(response.allowed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate<'a>(id: &'a str, provider: ProviderNamespace) -> PolicyCandidate<'a> {
        PolicyCandidate {
            id,
            provider,
            canonical_model_id: id.split_once('/').unwrap().1,
        }
    }

    #[test]
    fn provider_preferences_are_strict_and_reject_ambiguous_filters() {
        let valid = ProviderPreferences::parse(&serde_json::json!({
            "order": ["openai", "anthropic"],
            "only": ["openai", "google"],
            "ignore": ["anthropic"],
            "allow_fallbacks": false
        }))
        .unwrap();
        assert!(valid.allows(ProviderNamespace::OpenAi));
        assert!(!valid.allows(ProviderNamespace::Anthropic));
        assert_eq!(valid.order_rank(ProviderNamespace::OpenAi), 0);
        assert!(!valid.allow_fallbacks());

        for invalid in [
            serde_json::json!(null),
            serde_json::json!({"unknown": true}),
            serde_json::json!({"order": ["openai", "openai"]}),
            serde_json::json!({"only": ["openai"], "ignore": ["openai"]}),
            serde_json::json!({"only": null}),
            serde_json::json!({"allow_fallbacks": null}),
            serde_json::json!({"sort": "price"}),
            serde_json::json!({"order": ["cohere"]}),
        ] {
            assert!(ProviderPreferences::parse(&invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn policy_response_must_be_an_exact_ordered_subset() {
        let candidates = [
            candidate("anthropic/a", ProviderNamespace::Anthropic),
            candidate("openai/b", ProviderNamespace::OpenAi),
            candidate("google/c", ProviderNamespace::Google),
        ];
        let response = |mode, allowed: &[&str]| PreflightResponse {
            schema_version: 1,
            mode,
            allowed: allowed.iter().map(|id| (*id).to_string()).collect(),
        };
        assert_eq!(
            validate_response(
                &candidates,
                response(PolicyMode::Strict, &["anthropic/a", "google/c"])
            )
            .unwrap(),
            ["anthropic/a", "google/c"]
        );
        assert!(validate_response(
            &candidates,
            response(PolicyMode::Strict, &["google/c", "anthropic/a"])
        )
        .is_none());
        assert!(validate_response(
            &candidates,
            response(PolicyMode::Strict, &["anthropic/a", "anthropic/a"])
        )
        .is_none());
        assert!(validate_response(
            &candidates,
            response(PolicyMode::Strict, &["openai/unknown"])
        )
        .is_none());
        assert!(validate_response(
            &candidates,
            response(PolicyMode::Unrestricted, &["anthropic/a"])
        )
        .is_none());
    }

    #[test]
    fn policy_origin_order_starts_with_the_first_candidate_lane() {
        let candidates = [
            candidate("openai/a", ProviderNamespace::OpenAi),
            candidate("google/b", ProviderNamespace::Google),
        ];
        assert_eq!(
            origin_order(&candidates),
            [Plane::OpenAi, Plane::Gemini, Plane::Anthropic, Plane::Kimi]
        );
    }

    #[test]
    fn policy_origin_order_sends_kimi_candidates_to_the_kimi_plane() {
        let candidates = [candidate("kimi/k3", ProviderNamespace::Kimi)];
        assert_eq!(
            origin_order(&candidates),
            [Plane::Kimi, Plane::Anthropic, Plane::OpenAi, Plane::Gemini]
        );
    }
}
