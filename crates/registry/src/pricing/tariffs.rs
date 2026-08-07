//! Engine-side read/write authority for the hot tariff override table (migrations 0036/0037).
//!
//! Compiled `metering` constants are the implicit version 1 of every tariff family; each row of
//! `pricing_tariff_overrides` publishes version >= 2 of one family as data, so a price correction
//! does not require a recompile and redeploy. The table is append-only and strictly sequenced per
//! family by database triggers; this module validates, digests and writes rows through one
//! transaction and reads them back with fail-closed digest verification.
//!
//! The authority is PostgreSQL-only, exactly like the release-v2 pricing producer: the SQLite
//! backend has no entry points here at all. The pure pieces (family validation, payload parsing,
//! digest, resolution) are DB-free and unit-tested in place.
//!
//! i128 money legs are encoded as canonical decimal STRINGS in the payload JSON. `serde_json`
//! numbers cannot carry the full i128 range exactly (a `serde_json::Value` number holds at most
//! i64/u64 without loss, and the workspace unifies the `arbitrary_precision` feature through
//! `crates/forward`, which changes number handling again), so strings are the only representation
//! that is exact, float-rejecting and identical under every feature set. u64/i64 fields stay
//! plain JSON integers. The typed structs below mirror the pub fields of the `metering` price
//! structs one-to-one; registry deliberately does NOT depend on `metering`, so `server`/`forward`
//! convert these mirror structs into the `metering` types when the runtime resolver ships.

use super::require_id;
use anyhow::{bail, Context, Result};
use postgres::{Client, GenericClient, Row};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

/// Clock-skew grace for the determinism rule: a non-seed override may not take effect further in
/// the past than `created_ts - TARIFF_OVERRIDE_CLOCK_SKEW_GRACE_SECS`. Seed rows (version 2, the
/// first override of a family) are exempt and may carry `effective_from = 0`.
pub const TARIFF_OVERRIDE_CLOCK_SKEW_GRACE_SECS: i64 = 60;

const TARIFF_OVERRIDE_PAYLOAD_DIGEST_DOMAIN: &[u8] = b"apitoken:pricing-tariff-override-payload:v2\0";

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// Canonical decimal-string serde for i128 money legs: a JSON number (integer or float) never
/// deserializes, and a non-canonical string (leading zeros, `+`, whitespace) is rejected.
mod i128_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &i128, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i128, D::Error> {
        let raw = String::deserialize(deserializer)?;
        let value: i128 = raw.parse().map_err(|_| {
            serde::de::Error::custom("i128 money legs must be decimal strings")
        })?;
        if raw != value.to_string() {
            return Err(serde::de::Error::custom(
                "i128 money legs must be canonical decimal strings",
            ));
        }
        Ok(value)
    }
}

macro_rules! require_non_negative {
    ($struct:ident, $( $field:ident ),+ $(,)?) => {
        impl $struct {
            fn validate(&self) -> Result<()> {
                $( if self.$field < 0 {
                    bail!("tariff override {} field {} must be non-negative",
                        stringify!($struct), stringify!($field));
                } )+
                Ok(())
            }
        }
    };
}

/// Mirror of `metering::Prices` (Anthropic heuristic branch vector), nanoUSD per token.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnthropicTariffPrices {
    #[serde(with = "i128_string")]
    pub input: i128,
    #[serde(with = "i128_string")]
    pub output: i128,
    #[serde(with = "i128_string")]
    pub cache_read: i128,
    #[serde(with = "i128_string")]
    pub cache_write_5m: i128,
    #[serde(with = "i128_string")]
    pub cache_write_1h: i128,
}

require_non_negative!(
    AnthropicTariffPrices,
    input,
    output,
    cache_read,
    cache_write_5m,
    cache_write_1h,
);

/// Mirror of `metering::GeminiSearchBilling` as a tagged object.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum GeminiTariffSearchBilling {
    PerQuery {
        #[serde(with = "i128_string")]
        nano: i128,
    },
    PerGroundedPrompt {
        #[serde(with = "i128_string")]
        nano: i128,
    },
}

/// Mirror of `metering::GeminiPrices`, nanoUSD per token, including the long-context legs and
/// the Search billing mode.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeminiTariffPrices {
    #[serde(with = "i128_string")]
    pub input: i128,
    #[serde(with = "i128_string")]
    pub audio_input: i128,
    #[serde(with = "i128_string")]
    pub cached_input: i128,
    #[serde(with = "i128_string")]
    pub cached_audio_input: i128,
    #[serde(with = "i128_string")]
    pub output: i128,
    #[serde(with = "i128_string")]
    pub image_output: i128,
    pub long_context_threshold: u64,
    #[serde(with = "i128_string")]
    pub long_input: i128,
    #[serde(with = "i128_string")]
    pub long_audio_input: i128,
    #[serde(with = "i128_string")]
    pub long_cached_input: i128,
    #[serde(with = "i128_string")]
    pub long_cached_audio_input: i128,
    #[serde(with = "i128_string")]
    pub long_output: i128,
    pub search: GeminiTariffSearchBilling,
}

require_non_negative!(
    GeminiTariffPrices,
    input,
    audio_input,
    cached_input,
    cached_audio_input,
    output,
    image_output,
    long_input,
    long_audio_input,
    long_cached_input,
    long_cached_audio_input,
    long_output,
);

/// Mirror of `metering::CodexPrices`, nanoUSD per token plus the published integer modifiers.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CodexTariffPrices {
    #[serde(with = "i128_string")]
    pub input: i128,
    #[serde(with = "i128_string")]
    pub cached_input: i128,
    #[serde(with = "i128_string")]
    pub cache_write_input: i128,
    #[serde(with = "i128_string")]
    pub output: i128,
    pub api_fast_multiplier_basis_points: i64,
    pub long_context_threshold: u64,
    pub long_input_basis_points: i64,
    pub long_output_basis_points: i64,
}

require_non_negative!(
    CodexTariffPrices,
    input,
    cached_input,
    cache_write_input,
    output,
);

/// Mirror of `metering::CodexCreditRates`, nanocredits per token.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CodexTariffCreditRates {
    #[serde(with = "i128_string")]
    pub input: i128,
    #[serde(with = "i128_string")]
    pub cached_input: i128,
    #[serde(with = "i128_string")]
    pub output: i128,
}

require_non_negative!(CodexTariffCreditRates, input, cached_input, output,);

/// Mirror of `metering::GlmPrices`, nanoUSD per token.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlmTariffPrices {
    #[serde(with = "i128_string")]
    pub cached_input: i128,
    #[serde(with = "i128_string")]
    pub input: i128,
    #[serde(with = "i128_string")]
    pub cache_write: i128,
    #[serde(with = "i128_string")]
    pub output: i128,
}

require_non_negative!(GlmTariffPrices, cached_input, input, cache_write, output,);

/// Mirror of `metering::GlmCreditRates`: official credit multipliers stored in tenths.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlmTariffCreditRates {
    #[serde(with = "i128_string")]
    pub input_tenths: i128,
    #[serde(with = "i128_string")]
    pub cached_input_tenths: i128,
    #[serde(with = "i128_string")]
    pub output_tenths: i128,
}

require_non_negative!(
    GlmTariffCreditRates,
    input_tenths,
    cached_input_tenths,
    output_tenths,
);

/// Mirror of `metering::KimiPrices`, nanoUSD per token.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KimiTariffPrices {
    #[serde(with = "i128_string")]
    pub cached_input: i128,
    #[serde(with = "i128_string")]
    pub input: i128,
    #[serde(with = "i128_string")]
    pub cache_write: i128,
    #[serde(with = "i128_string")]
    pub output: i128,
}

require_non_negative!(KimiTariffPrices, cached_input, input, cache_write, output,);

/// Mirror of `metering::OpenAiImagePrices`, nanoUSD per token.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpenAiImageTariffPrices {
    #[serde(with = "i128_string")]
    pub fresh_text_input: i128,
    #[serde(with = "i128_string")]
    pub cached_text_input: i128,
    #[serde(with = "i128_string")]
    pub fresh_image_input: i128,
    #[serde(with = "i128_string")]
    pub cached_image_input: i128,
    #[serde(with = "i128_string")]
    pub image_output: i128,
}

require_non_negative!(
    OpenAiImageTariffPrices,
    fresh_text_input,
    cached_text_input,
    fresh_image_input,
    cached_image_input,
    image_output,
);

/// A payload parsed and validated against the tariff family prefix it was stored under.
///
/// The family prefixes are the exact set the `metering` family-key helpers emit:
/// `anthropic/*`, `google/gemini/<model>`, `openai/codex/<upstream>`,
/// `chatgpt/codex-credits/<upstream>`, `zhipu/glm/<official>`, `zhipu/glm-credits/<official>`,
/// `moonshot/kimi/<official>` and `openai/gpt-image-2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TariffOverridePayload {
    Anthropic(AnthropicTariffPrices),
    Gemini(GeminiTariffPrices),
    Codex(CodexTariffPrices),
    CodexCredits(CodexTariffCreditRates),
    Glm(GlmTariffPrices),
    GlmCredits(GlmTariffCreditRates),
    Kimi(KimiTariffPrices),
    OpenAiImage(OpenAiImageTariffPrices),
}

impl TariffOverridePayload {
    /// Canonical storage projection: the strict typed round-trip, so the stored JSON and its
    /// digest never depend on the caller's whitespace, key order or number formatting.
    pub fn to_canonical_value(&self) -> Result<serde_json::Value> {
        match self {
            Self::Anthropic(prices) => serde_json::to_value(prices),
            Self::Gemini(prices) => serde_json::to_value(prices),
            Self::Codex(prices) => serde_json::to_value(prices),
            Self::CodexCredits(rates) => serde_json::to_value(rates),
            Self::Glm(prices) => serde_json::to_value(prices),
            Self::GlmCredits(rates) => serde_json::to_value(rates),
            Self::Kimi(prices) => serde_json::to_value(prices),
            Self::OpenAiImage(prices) => serde_json::to_value(prices),
        }
        .context("encode tariff override payload")
    }
}

/// Mirror of the migration-0037 CHECK `^[a-z0-9][a-z0-9/._-]{0,127}$` (0036 excluded the dot,
/// which no canonical model id with a version number could satisfy). Hand-rolled: registry has
/// no regex dependency and the rule is deliberately tiny.
pub fn validate_tariff_family(family: &str) -> Result<()> {
    let bytes = family.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        bail!("tariff family must be 1..=128 bytes");
    }
    let first = bytes[0];
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        bail!("tariff family must start with a lowercase ascii letter or digit");
    }
    if !bytes.iter().all(|byte| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'/' | b'_' | b'-' | b'.')
    }) {
        bail!("tariff family allows only lowercase ascii letters, digits and / . _ -");
    }
    Ok(())
}

fn require_family_segment(family: &str, prefix: &str) -> Result<()> {
    if family.len() == prefix.len() {
        bail!("tariff family {family:?} is missing its model segment");
    }
    Ok(())
}

/// Strictly parse and validate an override payload against its tariff family prefix. Unknown
/// families and malformed payloads fail closed; every money leg must be a non-negative integer.
pub fn parse_tariff_override_payload(
    family: &str,
    payload: &serde_json::Value,
) -> Result<TariffOverridePayload> {
    if !payload.is_object() {
        bail!("tariff override payload must be a JSON object");
    }
    if family.strip_prefix("anthropic/").is_some() {
        require_family_segment(family, "anthropic/")?;
        let parsed: AnthropicTariffPrices = serde_json::from_value(payload.clone())
            .context("anthropic tariff override payload")?;
        parsed.validate()?;
        return Ok(TariffOverridePayload::Anthropic(parsed));
    }
    if family.strip_prefix("google/gemini/").is_some() {
        require_family_segment(family, "google/gemini/")?;
        let parsed: GeminiTariffPrices = serde_json::from_value(payload.clone())
            .context("gemini tariff override payload")?;
        parsed.validate()?;
        let search_nano = match parsed.search {
            GeminiTariffSearchBilling::PerQuery { nano }
            | GeminiTariffSearchBilling::PerGroundedPrompt { nano } => nano,
        };
        if search_nano < 0 {
            bail!("tariff override Gemini search billing must be non-negative");
        }
        return Ok(TariffOverridePayload::Gemini(parsed));
    }
    if family.strip_prefix("openai/codex/").is_some() {
        require_family_segment(family, "openai/codex/")?;
        let parsed: CodexTariffPrices = serde_json::from_value(payload.clone())
            .context("codex tariff override payload")?;
        parsed.validate()?;
        if parsed.api_fast_multiplier_basis_points < 0
            || parsed.long_input_basis_points < 0
            || parsed.long_output_basis_points < 0
        {
            bail!("tariff override Codex multipliers must be non-negative");
        }
        return Ok(TariffOverridePayload::Codex(parsed));
    }
    if family == "chatgpt/codex-credits" || family.strip_prefix("chatgpt/codex-credits/").is_some()
    {
        let parsed: CodexTariffCreditRates = serde_json::from_value(payload.clone())
            .context("codex credit override payload")?;
        parsed.validate()?;
        return Ok(TariffOverridePayload::CodexCredits(parsed));
    }
    if family.strip_prefix("zhipu/glm-credits/").is_some() {
        require_family_segment(family, "zhipu/glm-credits/")?;
        let parsed: GlmTariffCreditRates = serde_json::from_value(payload.clone())
            .context("glm credit override payload")?;
        parsed.validate()?;
        return Ok(TariffOverridePayload::GlmCredits(parsed));
    }
    if family.strip_prefix("zhipu/glm/").is_some() {
        require_family_segment(family, "zhipu/glm/")?;
        let parsed: GlmTariffPrices = serde_json::from_value(payload.clone())
            .context("glm tariff override payload")?;
        parsed.validate()?;
        return Ok(TariffOverridePayload::Glm(parsed));
    }
    if family.strip_prefix("moonshot/kimi/").is_some() {
        require_family_segment(family, "moonshot/kimi/")?;
        let parsed: KimiTariffPrices = serde_json::from_value(payload.clone())
            .context("kimi tariff override payload")?;
        parsed.validate()?;
        return Ok(TariffOverridePayload::Kimi(parsed));
    }
    if family == "openai/gpt-image-2" {
        let parsed: OpenAiImageTariffPrices = serde_json::from_value(payload.clone())
            .context("openai image tariff override payload")?;
        parsed.validate()?;
        return Ok(TariffOverridePayload::OpenAiImage(parsed));
    }
    bail!("tariff family {family:?} has no known payload schema");
}

/// Canonical `sha256:v2` digest of an already validated payload. The digest input is the typed
/// round-trip projection, never the caller's raw JSON.
pub fn tariff_override_payload_digest(payload: &TariffOverridePayload) -> Result<String> {
    tariff_override_payload_value_digest(&payload.to_canonical_value()?)
}

/// Digest of a stored payload value, recomputed on every read. Key order is canonical because a
/// `serde_json::Value` object map is sorted (no `preserve_order` feature anywhere in the
/// workspace), and the writer only ever stores typed-projected payloads.
fn tariff_override_payload_value_digest(value: &serde_json::Value) -> Result<String> {
    let encoded =
        serde_json::to_vec(value).context("encode tariff override payload for digest")?;
    let mut hasher = Sha256::new();
    hasher.update(TARIFF_OVERRIDE_PAYLOAD_DIGEST_DOMAIN);
    hasher.update(encoded);
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(format!("sha256:v2:{hex}"))
}

/// One immutable row of `pricing_tariff_overrides`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TariffOverride {
    pub tariff_family: String,
    pub version: i64,
    pub effective_from: i64,
    pub payload: serde_json::Value,
    pub payload_digest: String,
    pub created_ts: i64,
    pub created_by: String,
    pub reason: String,
}

/// Operator-supplied insert request. `created_ts` is registry-owned; the digest is computed
/// here from the typed payload projection, never supplied by the caller.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TariffOverrideInsert {
    pub tariff_family: String,
    pub version: i64,
    pub effective_from: i64,
    pub payload: serde_json::Value,
    pub created_by: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TariffOverrideInsertOutcome {
    Inserted(TariffOverride),
    /// Exact replay: the same family+version already stores the same payload digest and
    /// `effective_from` (operator attribution and reason may differ; the row content is what
    /// defines identity).
    Unchanged(TariffOverride),
    Rejected(TariffOverrideRejection),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TariffOverrideRejection {
    Invalid {
        reason: String,
    },
    /// The family+version key already exists with different content.
    Conflict {
        existing_digest: String,
        existing_effective_from: i64,
    },
    /// The version does not extend the family's strict sequence (the migration-0036 trigger
    /// remains the enforcement backstop for any writer that bypasses this check).
    SequenceViolation {
        expected_next: i64,
    },
}

fn validate_tariff_override_insert(insert: &TariffOverrideInsert) -> Result<()> {
    validate_tariff_family(&insert.tariff_family)?;
    if insert.version < 2 {
        bail!("tariff override version must be >= 2 (compiled constants are version 1)");
    }
    if insert.effective_from < 0 {
        bail!("tariff override effective_from must be non-negative");
    }
    require_id("tariff override operator", &insert.created_by)?;
    require_id("tariff override reason", &insert.reason)?;
    Ok(())
}

/// Resolve the override that prices `family` at `priced_ts`: the greatest version whose
/// `effective_from <= priced_ts`. `None` means no override applies and the compiled constants
/// (implicit version 1) remain the tariff.
pub fn resolve_tariff_override<'a>(
    rows: &'a [TariffOverride],
    family: &str,
    priced_ts: i64,
) -> Option<&'a TariffOverride> {
    rows.iter()
        .filter(|row| row.tariff_family == family && row.effective_from <= priced_ts)
        .max_by_key(|row| row.version)
}

fn tariff_override_from_row(row: &Row) -> Result<TariffOverride> {
    let payload_json: String = row.get(3);
    let payload: serde_json::Value = serde_json::from_str(&payload_json)
        .context("decode stored tariff override payload")?;
    let payload_digest: String = row.get(4);
    let actual_digest = tariff_override_payload_value_digest(&payload)?;
    if actual_digest != payload_digest {
        bail!("stored tariff override failed canonical payload digest verification");
    }
    let version: i32 = row.get(1);
    Ok(TariffOverride {
        tariff_family: row.get(0),
        version: i64::from(version),
        effective_from: row.get(2),
        payload,
        payload_digest,
        created_ts: row.get(5),
        created_by: row.get(6),
        reason: row.get(7),
    })
}

const TARIFF_OVERRIDE_COLUMNS: &str =
    "tariff_family,version,effective_from,payload::text,payload_digest,created_ts,created_by,reason";

/// Read the whole override table ordered by (family, version); it stays tiny by construction.
/// Every stored digest is recomputed and a mismatch fails closed as an integrity error.
pub(crate) fn postgres_list_tariff_overrides<C: GenericClient>(
    client: &mut C,
) -> Result<Vec<TariffOverride>> {
    let rows = client
        .query(
            &format!(
                "SELECT {TARIFF_OVERRIDE_COLUMNS} FROM pricing_tariff_overrides \
                 ORDER BY tariff_family,version"
            ),
            &[],
        )
        .context("read PostgreSQL tariff overrides")?;
    rows.iter().map(tariff_override_from_row).collect()
}

/// Insert one override row in a single transaction: validate, project, digest, apply the
/// determinism rule and rely on the migration-0036 triggers for sequence enforcement. Exact
/// replay is `Unchanged`; the same key with different content is a typed `Conflict`.
pub(crate) fn postgres_insert_tariff_override(
    client: &mut Client,
    insert: &TariffOverrideInsert,
) -> Result<TariffOverrideInsertOutcome> {
    let invalid = |error: anyhow::Error| {
        TariffOverrideInsertOutcome::Rejected(TariffOverrideRejection::Invalid {
            reason: format!("{error:#}"),
        })
    };
    if let Err(error) = validate_tariff_override_insert(insert) {
        return Ok(invalid(error));
    }
    let payload = match parse_tariff_override_payload(&insert.tariff_family, &insert.payload) {
        Ok(payload) => payload,
        Err(error) => return Ok(invalid(error)),
    };
    let canonical = payload.to_canonical_value()?;
    let digest = tariff_override_payload_value_digest(&canonical)?;
    let canonical_json =
        serde_json::to_string(&canonical).context("encode tariff override payload for storage")?;
    let created_ts = now();
    // Determinism: a non-seed override must not reach further into the past than the skew grace,
    // otherwise an operator could silently reprice already-settled history. Seed rows (version 2)
    // are exempt so a family can be introduced with `effective_from = 0`.
    if insert.version > 2
        && insert.effective_from < created_ts - TARIFF_OVERRIDE_CLOCK_SKEW_GRACE_SECS
    {
        return Ok(invalid(anyhow::anyhow!(
            "tariff override effective_from must be >= created_ts - \
             {TARIFF_OVERRIDE_CLOCK_SKEW_GRACE_SECS}s for version > 2"
        )));
    }

    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL tariff override insert")?;
    transaction
        .query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&format!("pricing-tariff-override:{}", insert.tariff_family)],
        )
        .context("lock PostgreSQL tariff override family")?;

    let version_i32 = i32::try_from(insert.version).context("tariff override version overflow")?;
    if let Some(row) = transaction
        .query_opt(
            &format!(
                "SELECT {TARIFF_OVERRIDE_COLUMNS} FROM pricing_tariff_overrides \
                 WHERE tariff_family=$1 AND version=$2"
            ),
            &[&insert.tariff_family, &version_i32],
        )
        .context("read PostgreSQL tariff override for replay")?
    {
        let existing = tariff_override_from_row(&row)?;
        let outcome = if existing.payload_digest == digest
            && existing.effective_from == insert.effective_from
        {
            TariffOverrideInsertOutcome::Unchanged(existing)
        } else {
            TariffOverrideInsertOutcome::Rejected(TariffOverrideRejection::Conflict {
                existing_digest: existing.payload_digest,
                existing_effective_from: existing.effective_from,
            })
        };
        transaction
            .commit()
            .context("commit PostgreSQL tariff override replay")?;
        return Ok(outcome);
    }

    let max_version: Option<i32> = transaction
        .query_one(
            "SELECT MAX(version) FROM pricing_tariff_overrides WHERE tariff_family=$1",
            &[&insert.tariff_family],
        )
        .context("read PostgreSQL tariff override family head")?
        .get(0);
    let expected_next = i64::from(max_version.unwrap_or(1)) + 1;
    if insert.version != expected_next {
        transaction
            .commit()
            .context("commit PostgreSQL tariff override sequence check")?;
        return Ok(TariffOverrideInsertOutcome::Rejected(
            TariffOverrideRejection::SequenceViolation { expected_next },
        ));
    }

    let inserted = transaction.execute(
        "INSERT INTO pricing_tariff_overrides(
             tariff_family,version,effective_from,payload,payload_digest,created_ts,created_by,
             reason
         ) VALUES($1,$2,$3,$4::text::jsonb,$5,$6,$7,$8)",
        &[
            &insert.tariff_family,
            &version_i32,
            &insert.effective_from,
            &canonical_json,
            &digest,
            &created_ts,
            &insert.created_by,
            &insert.reason,
        ],
    );
    match inserted {
        Ok(1) => {}
        Ok(changed) => {
            bail!("PostgreSQL tariff override insert changed {changed} rows");
        }
        Err(error) => {
            let mapped = error.as_db_error().and_then(|db| match db.code().code() {
                // The sequence trigger is the enforcement backstop; a concurrent writer outside
                // this authority surfaces here instead of the precheck above.
                "23514" if db.message().contains("extend the family sequence") => {
                    Some(TariffOverrideRejection::SequenceViolation { expected_next })
                }
                "23514" => Some(TariffOverrideRejection::Invalid {
                    reason: db.message().to_owned(),
                }),
                _ => None,
            });
            let Some(rejection) = mapped else {
                return Err(error).context("insert PostgreSQL tariff override");
            };
            drop(transaction); // roll back the aborted insert transaction
            return Ok(TariffOverrideInsertOutcome::Rejected(rejection));
        }
    }

    transaction
        .commit()
        .context("commit PostgreSQL tariff override insert")?;
    Ok(TariffOverrideInsertOutcome::Inserted(TariffOverride {
        tariff_family: insert.tariff_family.clone(),
        version: insert.version,
        effective_from: insert.effective_from,
        payload: canonical,
        payload_digest: digest,
        created_ts,
        created_by: insert.created_by.clone(),
        reason: insert.reason.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn override_row(family: &str, version: i64, effective_from: i64) -> TariffOverride {
        TariffOverride {
            tariff_family: family.to_owned(),
            version,
            effective_from,
            payload: json!({}),
            payload_digest: format!("sha256:v2:{}", "0".repeat(64)),
            created_ts: 1_000,
            created_by: "operator".to_owned(),
            reason: "unit test".to_owned(),
        }
    }

    #[test]
    fn family_validation_mirrors_the_migration_check() {
        for valid in [
            "anthropic/standard/opus-current",
            "openai/gpt-image-2",
            "google/gemini/gemini-2.5-pro",
            "chatgpt/codex-credits/gpt-5.6-sol",
            "zhipu/glm-credits/glm-5.2",
            "9lives/x_y-z",
        ] {
            validate_tariff_family(valid).unwrap_or_else(|error| panic!("{valid}: {error}"));
        }
        for invalid in [
            "",
            "/leading-slash",
            "-leading-dash",
            "_leading-underscore",
            "UPPER/case",
            "white space/x",
            "unicode/ж",
            &"a".repeat(129),
        ] {
            assert!(validate_tariff_family(invalid).is_err(), "{invalid}");
        }
        assert!(validate_tariff_family(&"a".repeat(128)).is_ok());
    }

    #[test]
    fn anthropic_payload_round_trips_and_rejects_non_strings() {
        let payload = json!({
            "input": "5000",
            "output": "25000",
            "cache_read": "500",
            "cache_write_5m": "6250",
            "cache_write_1h": "10000"
        });
        let parsed =
            parse_tariff_override_payload("anthropic/standard/opus-current", &payload).unwrap();
        let TariffOverridePayload::Anthropic(prices) = parsed else {
            panic!("anthropic payload parsed as {parsed:?}");
        };
        assert_eq!(prices.input, 5_000);
        assert_eq!(prices.output, 25_000);
        assert_eq!(parsed.to_canonical_value().unwrap(), payload);

        // JSON numbers (even exact integers) are not the string encoding; floats are rejected
        // outright, so money can never arrive with a fractional part.
        for bad in [
            json!({"input": 5000, "output": "25000", "cache_read": "500",
                   "cache_write_5m": "6250", "cache_write_1h": "10000"}),
            json!({"input": "5000.5", "output": "25000", "cache_read": "500",
                   "cache_write_5m": "6250", "cache_write_1h": "10000"}),
            json!({"input": "-1", "output": "25000", "cache_read": "500",
                   "cache_write_5m": "6250", "cache_write_1h": "10000"}),
            json!({"input": "05000", "output": "25000", "cache_read": "500",
                   "cache_write_5m": "6250", "cache_write_1h": "10000"}),
            json!({"input": "5000", "output": "25000", "cache_read": "500",
                   "cache_write_5m": "6250", "cache_write_1h": "10000", "extra": "1"}),
            json!([1, 2, 3]),
        ] {
            assert!(
                parse_tariff_override_payload("anthropic/standard/opus-current", &bad).is_err(),
                "{bad} must be rejected"
            );
        }
    }

    #[test]
    fn i128_legs_are_exact_decimal_strings_beyond_the_json_number_range() {
        // Proof of the encoding decision: i128::MAX/4 does not fit i64/u64, so a
        // `serde_json::Value` cannot hold it as an exact number.
        let big = i128::MAX / 4;
        let as_number: serde_json::Value =
            serde_json::from_str(&format!("{{\"v\": {big}}}")).unwrap();
        assert!(as_number["v"].as_i64().is_none() && as_number["v"].as_u64().is_none());

        let payload = json!({
            "input": big.to_string(),
            "output": "25000",
            "cache_read": "500",
            "cache_write_5m": "6250",
            "cache_write_1h": "10000"
        });
        let parsed =
            parse_tariff_override_payload("anthropic/standard/opus-current", &payload).unwrap();
        let TariffOverridePayload::Anthropic(prices) = parsed else {
            panic!("anthropic payload parsed as {parsed:?}");
        };
        assert_eq!(prices.input, big);
        // The canonical projection preserves the full precision through another round trip.
        let reparsed = parse_tariff_override_payload(
            "anthropic/standard/opus-current",
            &parsed.to_canonical_value().unwrap(),
        )
        .unwrap();
        assert_eq!(reparsed, parsed);
    }

    #[test]
    fn gemini_payload_covers_long_context_and_tagged_search() {
        let base = |search: serde_json::Value| {
            json!({
                "input": "1250",
                "audio_input": "1250",
                "cached_input": "125",
                "cached_audio_input": "125",
                "output": "10000",
                "image_output": "0",
                "long_context_threshold": 200000,
                "long_input": "2500",
                "long_audio_input": "2500",
                "long_cached_input": "250",
                "long_cached_audio_input": "250",
                "long_output": "15000",
                "search": search
            })
        };
        let parsed = parse_tariff_override_payload(
            "google/gemini/gemini-2.5-pro",
            &base(json!({"kind": "per_grounded_prompt", "nano": "35000000"})),
        )
        .unwrap();
        let TariffOverridePayload::Gemini(prices) = parsed else {
            panic!("gemini payload parsed as {parsed:?}");
        };
        assert_eq!(prices.long_context_threshold, 200_000);
        assert_eq!(
            prices.search,
            GeminiTariffSearchBilling::PerGroundedPrompt { nano: 35_000_000 }
        );

        let parsed = parse_tariff_override_payload(
            "google/gemini/gemini-3.6-flash",
            &base(json!({"kind": "per_query", "nano": "14000000"})),
        )
        .unwrap();
        assert_eq!(
            parsed,
            TariffOverridePayload::Gemini(GeminiTariffPrices {
                search: GeminiTariffSearchBilling::PerQuery { nano: 14_000_000 },
                ..prices
            })
        );

        for bad_search in [
            json!({"kind": "per_query", "nano": 14000000}),       // number, not string
            json!({"kind": "per_query"}),                          // missing nano
            json!({"kind": "per_query", "nano": "1", "x": "2"}),   // unknown field
            json!({"kind": "per_decade", "nano": "1"}),            // unknown kind
            json!({"kind": "per_query", "nano": "-1"}),            // negative rate
        ] {
            assert!(
                parse_tariff_override_payload(
                    "google/gemini/gemini-2.5-pro",
                    &base(bad_search.clone()),
                )
                .is_err(),
                "{bad_search} must be rejected"
            );
        }
        // The bare provider prefix without a model segment is not a family.
        assert!(parse_tariff_override_payload("google/gemini/", &base(json!({"kind": "per_query", "nano": "1"}))).is_err());
    }

    #[test]
    fn every_family_prefix_parses_its_typed_payload() {
        let cases: &[(&str, serde_json::Value)] = &[
            (
                "openai/codex/gpt-5.6-sol",
                json!({
                    "input": "5000", "cached_input": "500", "cache_write_input": "6250",
                    "output": "30000", "api_fast_multiplier_basis_points": 20000,
                    "long_context_threshold": 272000, "long_input_basis_points": 20000,
                    "long_output_basis_points": 15000
                }),
            ),
            (
                "chatgpt/codex-credits/gpt-5.6-sol",
                json!({"input": "125000", "cached_input": "12500", "output": "750000"}),
            ),
            (
                "chatgpt/codex-credits",
                json!({"input": "125000", "cached_input": "12500", "output": "750000"}),
            ),
            (
                "zhipu/glm/glm-5.2",
                json!({"cached_input": "260", "input": "1400", "cache_write": "1400",
                       "output": "4400"}),
            ),
            (
                "zhipu/glm-credits/glm-5.2",
                json!({"input_tenths": "69", "cached_input_tenths": "17",
                       "output_tenths": "240"}),
            ),
            (
                "moonshot/kimi/kimi-k3",
                json!({"cached_input": "300", "input": "3000", "cache_write": "3000",
                       "output": "15000"}),
            ),
            (
                "openai/gpt-image-2",
                json!({"fresh_text_input": "5000", "cached_text_input": "1250",
                       "fresh_image_input": "8000", "cached_image_input": "2000",
                       "image_output": "30000"}),
            ),
        ];
        for (family, payload) in cases {
            let parsed = parse_tariff_override_payload(family, payload)
                .unwrap_or_else(|error| panic!("{family}: {error}"));
            // The typed projection is byte-stable under a second parse.
            let canonical = parsed.to_canonical_value().unwrap();
            let reparsed = parse_tariff_override_payload(family, &canonical).unwrap();
            assert_eq!(reparsed, parsed, "{family} round trip");
        }
        // Unknown prefixes and cross-schema payloads fail closed.
        for (family, payload) in [
            ("anthropic", json!({})),
            ("openai/codex", json!({})),
            ("unknown/provider/model", json!({})),
            ("zhipu/glm/glm-5.2", json!({"input": "1"})), // GLM shape needs all four legs
        ] {
            assert!(
                parse_tariff_override_payload(family, &payload).is_err(),
                "{family} must reject {payload}"
            );
        }
    }

    #[test]
    fn payload_digest_is_canonical_and_shape_pinned() {
        let parsed = parse_tariff_override_payload(
            "moonshot/kimi/kimi-k3",
            &json!({"cached_input": "300", "input": "3000", "cache_write": "3000",
                    "output": "15000"}),
        )
        .unwrap();
        let digest = tariff_override_payload_digest(&parsed).unwrap();
        assert!(
            digest.starts_with("sha256:v2:") && digest.len() == 10 + 64,
            "{digest}"
        );
        assert!(digest[10..].chars().all(|c| c.is_ascii_hexdigit()));

        // Key order and whitespace of the caller's JSON never reach the digest.
        let reordered: serde_json::Value = serde_json::from_str(
            "{ \"output\": \"15000\",  \"input\": \"3000\",\n\"cache_write\": \"3000\", \"cached_input\": \"300\" }",
        )
        .unwrap();
        let reparsed = parse_tariff_override_payload("moonshot/kimi/kimi-k3", &reordered).unwrap();
        assert_eq!(tariff_override_payload_digest(&reparsed).unwrap(), digest);

        // A different price is a different digest.
        let changed = parse_tariff_override_payload(
            "moonshot/kimi/kimi-k3",
            &json!({"cached_input": "301", "input": "3000", "cache_write": "3000",
                    "output": "15000"}),
        )
        .unwrap();
        assert_ne!(tariff_override_payload_digest(&changed).unwrap(), digest);

        // The digest covers the payload only; the family is part of the row key, not of the
        // digest input (a kimi and a glm payload with identical fields legitimately share it).
        let glm = parse_tariff_override_payload(
            "zhipu/glm/glm-5.2",
            &json!({"cached_input": "300", "input": "3000", "cache_write": "3000",
                    "output": "15000"}),
        )
        .unwrap();
        assert_eq!(tariff_override_payload_digest(&glm).unwrap(), digest);
    }

    #[test]
    fn resolution_picks_the_greatest_effective_version() {
        let family = "anthropic/standard/opus-current";
        let rows = vec![
            override_row(family, 2, 0),
            override_row(family, 3, 1_000),
            override_row(family, 4, 2_000),
            override_row("anthropic/standard/haiku-3", 2, 0),
        ];
        assert_eq!(
            resolve_tariff_override(&rows, family, 0).map(|row| row.version),
            Some(2)
        );
        assert_eq!(
            resolve_tariff_override(&rows, family, 999).map(|row| row.version),
            Some(2)
        );
        assert_eq!(
            resolve_tariff_override(&rows, family, 1_000).map(|row| row.version),
            Some(3)
        );
        assert_eq!(
            resolve_tariff_override(&rows, family, i64::MAX).map(|row| row.version),
            Some(4)
        );
        // A family with no applicable row falls back to the compiled constants (version 1).
        assert_eq!(resolve_tariff_override(&rows, "anthropic/standard/fable-5", 1), None);
        assert_eq!(resolve_tariff_override(&[], family, i64::MAX), None);

        // effective_from is a per-row bound, not monotone across versions: a later version with
        // a later bound never shadows an applicable earlier one.
        let skewed = vec![
            override_row(family, 2, 5_000),
            override_row(family, 3, 9_000),
        ];
        assert_eq!(
            resolve_tariff_override(&skewed, family, 6_000).map(|row| row.version),
            Some(2)
        );
        assert_eq!(resolve_tariff_override(&skewed, family, 4_999), None);
    }

    #[test]
    fn insert_validation_enforces_the_static_rules() {
        let base = TariffOverrideInsert {
            tariff_family: "moonshot/kimi/kimi-k3".to_owned(),
            version: 2,
            effective_from: 0,
            payload: json!({"cached_input": "300", "input": "3000", "cache_write": "3000",
                            "output": "15000"}),
            created_by: "operator".to_owned(),
            reason: "seed".to_owned(),
        };
        validate_tariff_override_insert(&base).unwrap();
        for mutate in [
            (|insert: &mut TariffOverrideInsert| insert.version = 1) as fn(&mut TariffOverrideInsert),
            (|insert| insert.version = -2),
            (|insert| insert.effective_from = -1),
            (|insert| insert.tariff_family = "BAD/family".to_owned()),
            (|insert| insert.created_by = String::new()),
            (|insert| insert.created_by = " padded ".to_owned()),
            (|insert| insert.reason = String::new()),
        ] {
            let mut broken = base.clone();
            mutate(&mut broken);
            assert!(validate_tariff_override_insert(&broken).is_err());
        }
    }
}
