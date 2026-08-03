//! Key-scoped catalog pricing client.
//!
//! The provider runtimes own authentication, account policy and tariff projection. The router
//! sends the current request credential and the bounded aggregate catalog over the loopback-only
//! producer contract, validates the closed response schema and returns only an in-request overlay.
//! Neither credentials nor personalized rates are cached.

use std::time::Duration;

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::bounded;
use crate::catalog::PlaneOrigins;
use crate::error::Lane;

const SCHEMA_VERSION: u64 = 1;
const UNIT: &str = "nano_usd_per_million_tokens";
const MAX_CANDIDATES_PER_REQUEST: usize = 256;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const PRICING_TIMEOUT: Duration = Duration::from_secs(2);
const PRICING_PATH: &str = "/internal/router/catalog/pricing";

pub struct PricingCandidate<'a> {
    pub id: &'a str,
    pub provider_id: &'a str,
    pub model_id: &'a str,
}

#[derive(Serialize)]
struct PricingRequest<'a> {
    schema_version: u64,
    candidates: Vec<WireCandidate<'a>>,
}

#[derive(Serialize)]
struct WireCandidate<'a> {
    id: &'a str,
    provider_id: &'a str,
    model_id: &'a str,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum PricingMode {
    Admin,
    Legacy,
    Strict,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PricingResponse {
    schema_version: u64,
    unit: String,
    mode: PricingMode,
    entries: Vec<PricingEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RateCard {
    input: String,
    output: String,
    cache_read: String,
    cache_write: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_write_1h: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ContextTier {
    threshold_tokens: u64,
    standard: RateCard,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<RateCard>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingEntry {
    id: String,
    standard: RateCard,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<RateCard>,
    #[serde(skip_serializing_if = "Option::is_none")]
    long_context: Option<ContextTier>,
}

impl PricingEntry {
    /// Public namespaced metadata. Integer strings stay exact on the wire; clients may convert
    /// them only at the final display boundary required by their native model schema.
    pub fn public_json(&self) -> serde_json::Value {
        serde_json::json!({
            "unit": UNIT,
            "standard": self.standard,
            "priority": self.priority,
            "long_context": self.long_context,
        })
    }
}

pub struct PricingOverlay {
    entries: Vec<PricingEntry>,
}

impl PricingOverlay {
    pub fn entry(&self, id: &str) -> Option<&PricingEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PricingError {
    Unauthorized,
    Unavailable,
}

pub async fn fetch(
    client: &reqwest::Client,
    origins: &PlaneOrigins<'_>,
    auth: &HeaderMap,
    candidates: &[PricingCandidate<'_>],
) -> Result<PricingOverlay, PricingError> {
    if candidates.is_empty() {
        return Err(PricingError::Unavailable);
    }
    let mut entries = Vec::new();
    for chunk in candidates.chunks(MAX_CANDIDATES_PER_REQUEST) {
        entries.extend(fetch_chunk(client, origins, auth, chunk).await?);
    }
    Ok(PricingOverlay { entries })
}

async fn fetch_chunk(
    client: &reqwest::Client,
    origins: &PlaneOrigins<'_>,
    auth: &HeaderMap,
    candidates: &[PricingCandidate<'_>],
) -> Result<Vec<PricingEntry>, PricingError> {
    let request = PricingRequest {
        schema_version: SCHEMA_VERSION,
        candidates: candidates
            .iter()
            .map(|candidate| WireCandidate {
                id: candidate.id,
                provider_id: candidate.provider_id,
                model_id: candidate.model_id,
            })
            .collect(),
    };
    let body = serde_json::to_vec(&request).map_err(|_| PricingError::Unavailable)?;
    if body.len() > MAX_BODY_BYTES {
        return Err(PricingError::Unavailable);
    }

    for lane in origin_order(candidates) {
        let origin = match lane {
            Lane::Anthropic => origins.anthropic,
            Lane::OpenAi => origins.openai,
            Lane::Gemini => origins.gemini,
        };
        let response = match client
            .post(format!("{origin}{PRICING_PATH}"))
            .headers(auth.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .timeout(PRICING_TIMEOUT)
            .body(body.clone())
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => continue,
        };
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(PricingError::Unauthorized);
        }
        if !response.status().is_success() {
            continue;
        }
        let Ok(bytes) = bounded::response_bytes(response, MAX_BODY_BYTES).await else {
            continue;
        };
        let Ok(response) = serde_json::from_slice::<PricingResponse>(&bytes) else {
            continue;
        };
        let Some(entries) = validate_response(candidates, response) else {
            continue;
        };
        return Ok(entries);
    }
    Err(PricingError::Unavailable)
}

fn origin_order(candidates: &[PricingCandidate<'_>]) -> Vec<Lane> {
    let mut order = Vec::with_capacity(3);
    for lane in candidates
        .iter()
        .filter_map(|candidate| match candidate.provider_id {
            "anthropic" => Some(Lane::Anthropic),
            "openai" => Some(Lane::OpenAi),
            "google" => Some(Lane::Gemini),
            _ => None,
        })
        .chain([Lane::Anthropic, Lane::OpenAi, Lane::Gemini])
    {
        if !order.contains(&lane) {
            order.push(lane);
        }
    }
    order
}

fn validate_response(
    candidates: &[PricingCandidate<'_>],
    response: PricingResponse,
) -> Option<Vec<PricingEntry>> {
    if response.schema_version != SCHEMA_VERSION
        || response.unit != UNIT
        || response.entries.len() > candidates.len()
    {
        return None;
    }
    let _mode = response.mode;
    let mut previous = None;
    for entry in &response.entries {
        let index = candidates
            .iter()
            .position(|candidate| candidate.id == entry.id)?;
        if previous.is_some_and(|previous| index <= previous)
            || !valid_rate_card(&entry.standard)
            || entry
                .priority
                .as_ref()
                .is_some_and(|card| !valid_rate_card(card))
            || entry.long_context.as_ref().is_some_and(|tier| {
                tier.threshold_tokens == 0
                    || !valid_rate_card(&tier.standard)
                    || tier
                        .priority
                        .as_ref()
                        .is_some_and(|card| !valid_rate_card(card))
            })
        {
            return None;
        }
        previous = Some(index);
    }
    Some(response.entries)
}

fn valid_rate_card(card: &RateCard) -> bool {
    [
        Some(card.input.as_str()),
        Some(card.output.as_str()),
        Some(card.cache_read.as_str()),
        Some(card.cache_write.as_str()),
        card.cache_write_1h.as_deref(),
    ]
    .into_iter()
    .flatten()
    .all(canonical_nonnegative_integer)
}

fn canonical_nonnegative_integer(value: &str) -> bool {
    value == "0"
        || (value.as_bytes().first().is_some_and(u8::is_ascii_digit)
            && value.as_bytes()[0] != b'0'
            && value.as_bytes().iter().all(u8::is_ascii_digit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post;
    use axum::{Json, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn candidate<'a>(id: &'a str, provider_id: &'a str) -> PricingCandidate<'a> {
        PricingCandidate {
            id,
            provider_id,
            model_id: id.split_once('/').unwrap().1,
        }
    }

    fn card(value: &str) -> RateCard {
        RateCard {
            input: value.to_owned(),
            output: value.to_owned(),
            cache_read: value.to_owned(),
            cache_write: value.to_owned(),
            cache_write_1h: None,
        }
    }

    fn response(entries: Vec<PricingEntry>) -> PricingResponse {
        PricingResponse {
            schema_version: 1,
            unit: UNIT.to_owned(),
            mode: PricingMode::Legacy,
            entries,
        }
    }

    #[test]
    fn pricing_response_requires_exact_unit_canonical_integers_and_ordered_subset() {
        let candidates = [
            candidate("anthropic/a", "anthropic"),
            candidate("openai/b", "openai"),
            candidate("google/c", "google"),
        ];
        let entry = |id: &str, value: &str| PricingEntry {
            id: id.to_owned(),
            standard: card(value),
            priority: None,
            long_context: None,
        };
        assert_eq!(
            validate_response(
                &candidates,
                response(vec![
                    entry("anthropic/a", "0"),
                    entry("google/c", "1200000000")
                ])
            )
            .unwrap()
            .len(),
            2
        );
        assert!(validate_response(
            &candidates,
            response(vec![entry("google/c", "1"), entry("anthropic/a", "1")])
        )
        .is_none());
        assert!(
            validate_response(&candidates, response(vec![entry("anthropic/a", "01")])).is_none()
        );
        let mut wrong_unit = response(vec![entry("anthropic/a", "1")]);
        wrong_unit.unit = "usd_per_token".to_owned();
        assert!(validate_response(&candidates, wrong_unit).is_none());
    }

    #[test]
    fn pricing_origin_order_starts_with_catalog_provider_and_is_fixed() {
        let candidates = [
            candidate("openai/a", "openai"),
            candidate("google/b", "google"),
        ];
        assert_eq!(
            origin_order(&candidates),
            [Lane::OpenAi, Lane::Gemini, Lane::Anthropic]
        );
    }

    #[tokio::test]
    async fn catalogs_larger_than_one_authority_request_are_chunked_and_merged() {
        let calls = Arc::new(AtomicUsize::new(0));
        let state = calls.clone();
        let app = Router::new().route(
            PRICING_PATH,
            post(move |Json(request): Json<serde_json::Value>| {
                let state = state.clone();
                async move {
                    state.fetch_add(1, Ordering::SeqCst);
                    let entries: Vec<_> = request["candidates"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|candidate| {
                            serde_json::json!({
                                "id": candidate["id"],
                                "standard": {
                                    "input": "1", "output": "1", "cache_read": "1",
                                    "cache_write": "1"
                                },
                                "priority": null,
                                "long_context": null
                            })
                        })
                        .collect();
                    Json(serde_json::json!({
                        "schema_version": 1,
                        "unit": UNIT,
                        "mode": "legacy",
                        "entries": entries
                    }))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let origin = format!("http://{address}");
        let origins = PlaneOrigins {
            anthropic: "http://127.0.0.1:0",
            openai: &origin,
            gemini: "http://127.0.0.1:0",
        };

        let ids: Vec<_> = (0..257)
            .map(|index| format!("openai/model-{index}"))
            .collect();
        let candidates: Vec<_> = ids
            .iter()
            .map(|id| PricingCandidate {
                id,
                provider_id: "openai",
                model_id: id.split_once('/').unwrap().1,
            })
            .collect();
        let overlay = fetch(
            &reqwest::Client::new(),
            &origins,
            &HeaderMap::new(),
            &candidates,
        )
        .await
        .unwrap();
        assert!(overlay.entry("openai/model-0").is_some());
        assert!(overlay.entry("openai/model-256").is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
