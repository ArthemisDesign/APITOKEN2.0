//! Canonical, backend-neutral identities for immutable pricing admission snapshots.
//!
//! The database JSON columns are projections only. Durable identity is computed from typed fields
//! with an explicit binary encoding so PostgreSQL JSONB normalization and SQLite text ordering can
//! never produce different digests for the same request.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

pub const LEGACY_SCALAR_SNAPSHOT_SCHEMA_VERSION: i64 = 1;
/// Maximum supported replay age for a live legacy-scalar snapshot.
///
/// Engine request IDs are internal CSPRNG UUIDv4 values and are never accepted from a client or
/// upstream. One day is orders of magnitude longer than the bounded actor retry and one-hour
/// reservation lease, while remaining strictly below the independent 30-day lifecycle retention.
/// That gap prevents a prune/replay race at the retention boundary without permanent tombstones.
pub const LEGACY_SCALAR_REPLAY_MAX_AGE_SECS: i64 = 24 * 60 * 60;
/// Minimum retention for terminal request machinery that owns pricing snapshot children.
pub const PRICING_REQUEST_LIFECYCLE_MIN_RETENTION_SECS: i64 = 30 * 24 * 60 * 60;
const _: () =
    assert!(PRICING_REQUEST_LIFECYCLE_MIN_RETENTION_SECS > LEGACY_SCALAR_REPLAY_MAX_AGE_SECS);
const SNAPSHOT_ID_MAX_BYTES: usize = 512;
const TARIFF_ID_MAX_BYTES: usize = 256;
const PREMIUM_MODIFIERS_MAX_BYTES: usize = 1_024;
const SNAPSHOT_DIGEST_DOMAIN: &[u8] = b"claude-api/pricing/legacy-scalar-admission-snapshot/v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyScalarIdempotencyWindowError {
    InvalidTrustedTimestamp,
    AdmissionFromFuture,
    Expired,
}

pub(crate) fn validate_request_lifecycle_prune_cutoff(
    older_than_ts: i64,
    trusted_now_ts: i64,
) -> Result<()> {
    if trusted_now_ts <= 0 {
        bail!("trusted request lifecycle retention clock is invalid");
    }
    let newest_safe_cutoff = trusted_now_ts
        .checked_sub(PRICING_REQUEST_LIFECYCLE_MIN_RETENTION_SECS)
        .context("request lifecycle retention cutoff overflow")?;
    if older_than_ts > newest_safe_cutoff {
        bail!("request lifecycle cutoff violates minimum pricing retention");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotProvider {
    Anthropic,
    OpenAi,
}

impl SnapshotProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self> {
        match value {
            "anthropic" => Ok(Self::Anthropic),
            "openai" => Ok(Self::OpenAi),
            _ => bail!("stored pricing snapshot has an unsupported provider"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotAnthropicSpeed {
    Standard,
    Fast,
}

impl SnapshotAnthropicSpeed {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Fast => "fast",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotAnthropicInferenceGeo {
    Global,
    Us,
}

impl SnapshotAnthropicInferenceGeo {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Us => "us",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotOpenAiServiceTier {
    Standard,
    Fast,
}

impl SnapshotOpenAiServiceTier {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Fast => "fast",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotOpenAiContextTier {
    Standard,
    Long,
}

impl SnapshotOpenAiContextTier {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Long => "long",
        }
    }
}

/// Versioned request-side modifiers that affected the official reserve tariff.
///
/// The tagged JSON representation is persisted for inspection, while the digest below consumes
/// the typed variants directly. Adding a new provider or modifier contract therefore requires an
/// explicit versioned code change instead of accepting an arbitrary client object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum LegacyPremiumModifiers {
    #[serde(rename = "anthropic_v1")]
    AnthropicV1 {
        speed: SnapshotAnthropicSpeed,
        inference_geo: SnapshotAnthropicInferenceGeo,
        inference_geo_basis_points: i64,
    },
    #[serde(rename = "openai_v1")]
    OpenAiV1 {
        service_tier: SnapshotOpenAiServiceTier,
        service_tier_multiplier_basis_points: i64,
        context_tier: SnapshotOpenAiContextTier,
        input_multiplier_basis_points: i64,
        output_multiplier_basis_points: i64,
    },
}

impl LegacyPremiumModifiers {
    pub fn validate_for_provider(&self, provider: SnapshotProvider) -> Result<()> {
        match (provider, self) {
            (
                SnapshotProvider::Anthropic,
                Self::AnthropicV1 {
                    inference_geo,
                    inference_geo_basis_points,
                    ..
                },
            ) => {
                let expected = match inference_geo {
                    SnapshotAnthropicInferenceGeo::Global => 10_000,
                    SnapshotAnthropicInferenceGeo::Us => 11_000,
                };
                if *inference_geo_basis_points != expected {
                    bail!("invalid Anthropic inference geography multiplier");
                }
            }
            (
                SnapshotProvider::OpenAi,
                Self::OpenAiV1 {
                    service_tier,
                    service_tier_multiplier_basis_points,
                    context_tier,
                    input_multiplier_basis_points,
                    output_multiplier_basis_points,
                },
            ) => {
                let service_tier_valid = match service_tier {
                    SnapshotOpenAiServiceTier::Standard => {
                        *service_tier_multiplier_basis_points == 10_000
                    }
                    SnapshotOpenAiServiceTier::Fast => {
                        matches!(*service_tier_multiplier_basis_points, 20_000 | 25_000)
                    }
                };
                if !service_tier_valid {
                    bail!("invalid OpenAI service-tier multiplier");
                }
                let context_tier_valid = match context_tier {
                    SnapshotOpenAiContextTier::Standard => {
                        *input_multiplier_basis_points == 10_000
                            && *output_multiplier_basis_points == 10_000
                    }
                    SnapshotOpenAiContextTier::Long => {
                        *input_multiplier_basis_points == 20_000
                            && *output_multiplier_basis_points == 15_000
                    }
                };
                if !context_tier_valid {
                    bail!("invalid OpenAI context-tier multipliers");
                }
            }
            _ => bail!("pricing snapshot provider and premium modifiers do not match"),
        }
        Ok(())
    }

    pub(crate) fn to_canonical_json(&self) -> Result<String> {
        let encoded = serde_json::to_string(self).context("encode pricing premium modifiers")?;
        if encoded.len() > PREMIUM_MODIFIERS_MAX_BYTES {
            bail!("pricing premium modifiers exceed the encoded size limit");
        }
        Ok(encoded)
    }

    pub(crate) fn from_json(value: &str) -> Result<Self> {
        if value.len() > PREMIUM_MODIFIERS_MAX_BYTES {
            bail!("stored pricing premium modifiers exceed the encoded size limit");
        }
        let decoded: Self =
            serde_json::from_str(value).context("decode stored pricing premium modifiers")?;
        // Serialization is also a boundedness check independent of backend JSON formatting.
        decoded.to_canonical_json()?;
        Ok(decoded)
    }

    pub(crate) fn feed_digest(&self, encoder: &mut CanonicalDigestEncoder) {
        match self {
            Self::AnthropicV1 {
                speed,
                inference_geo,
                inference_geo_basis_points,
            } => {
                encoder.string(18, "anthropic_v1");
                encoder.string(19, speed.as_str());
                encoder.string(20, inference_geo.as_str());
                encoder.i64(21, *inference_geo_basis_points);
            }
            Self::OpenAiV1 {
                service_tier,
                service_tier_multiplier_basis_points,
                context_tier,
                input_multiplier_basis_points,
                output_multiplier_basis_points,
            } => {
                encoder.string(18, "openai_v1");
                encoder.string(22, service_tier.as_str());
                encoder.i64(23, *service_tier_multiplier_basis_points);
                encoder.string(24, context_tier.as_str());
                encoder.i64(25, *input_multiplier_basis_points);
                encoder.i64(26, *output_multiplier_basis_points);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyScalarAdmissionSnapshotInput {
    pub request_id: String,
    pub account_id: String,
    pub provider: SnapshotProvider,
    pub requested_model_id: String,
    pub canonical_model_id: String,
    pub alias_generation: i64,
    pub tariff_schedule_id: String,
    pub tariff_priced_ts: i64,
    pub admission_ts: i64,
    pub payable_multiplier_bp: i64,
    pub official_hold_nano: i64,
    pub charged_hold_nano: i64,
    pub premium_modifiers: LegacyPremiumModifiers,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalDigestV1(String);

impl CanonicalDigestV1 {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Full immutable legacy-scalar admission identity persisted beside one reservation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyScalarAdmissionSnapshot {
    pub(crate) schema_version: i64,
    pub(crate) request_id: String,
    pub(crate) account_id: String,
    pub(crate) provider: SnapshotProvider,
    pub(crate) requested_model_id: String,
    pub(crate) canonical_model_id: String,
    pub(crate) alias_generation: i64,
    pub(crate) tariff_schedule_id: String,
    pub(crate) tariff_priced_ts: i64,
    pub(crate) admission_ts: i64,
    pub(crate) payable_multiplier_bp: i64,
    pub(crate) official_hold_nano: i64,
    pub(crate) charged_hold_nano: i64,
    pub(crate) premium_modifiers: LegacyPremiumModifiers,
    snapshot_digest: CanonicalDigestV1,
}

impl LegacyScalarAdmissionSnapshot {
    /// Construct a backend-neutral snapshot value.
    ///
    /// This validates the persistence shape and provider/modifier pairing. It deliberately cannot
    /// prove model/tariff provenance: a future live bridge must populate the input only from the
    /// provider-owned canonicalizer in `metering`, never from client-controlled strings.
    pub fn new(input: LegacyScalarAdmissionSnapshotInput) -> Result<Self> {
        validate_input(&input)?;
        let mut snapshot = Self {
            schema_version: LEGACY_SCALAR_SNAPSHOT_SCHEMA_VERSION,
            request_id: input.request_id,
            account_id: input.account_id,
            provider: input.provider,
            requested_model_id: input.requested_model_id,
            canonical_model_id: input.canonical_model_id,
            alias_generation: input.alias_generation,
            tariff_schedule_id: input.tariff_schedule_id,
            tariff_priced_ts: input.tariff_priced_ts,
            admission_ts: input.admission_ts,
            payable_multiplier_bp: input.payable_multiplier_bp,
            official_hold_nano: input.official_hold_nano,
            charged_hold_nano: input.charged_hold_nano,
            premium_modifiers: input.premium_modifiers,
            snapshot_digest: CanonicalDigestV1(String::new()),
        };
        snapshot.snapshot_digest = CanonicalDigestV1(snapshot.compute_digest());
        Ok(snapshot)
    }

    pub fn snapshot_digest(&self) -> &CanonicalDigestV1 {
        &self.snapshot_digest
    }

    pub const fn schema_version(&self) -> i64 {
        self.schema_version
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    pub const fn provider(&self) -> SnapshotProvider {
        self.provider
    }

    pub fn requested_model_id(&self) -> &str {
        &self.requested_model_id
    }

    pub fn canonical_model_id(&self) -> &str {
        &self.canonical_model_id
    }

    pub const fn alias_generation(&self) -> i64 {
        self.alias_generation
    }

    pub fn tariff_schedule_id(&self) -> &str {
        &self.tariff_schedule_id
    }

    pub const fn tariff_priced_ts(&self) -> i64 {
        self.tariff_priced_ts
    }

    pub const fn admission_ts(&self) -> i64 {
        self.admission_ts
    }

    pub const fn payable_multiplier_bp(&self) -> i64 {
        self.payable_multiplier_bp
    }

    pub const fn official_hold_nano(&self) -> i64 {
        self.official_hold_nano
    }

    pub const fn charged_hold_nano(&self) -> i64 {
        self.charged_hold_nano
    }

    pub fn premium_modifiers(&self) -> &LegacyPremiumModifiers {
        &self.premium_modifiers
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != LEGACY_SCALAR_SNAPSHOT_SCHEMA_VERSION {
            bail!("unsupported legacy scalar snapshot schema version");
        }
        validate_input(&self.as_input())?;
        if self.snapshot_digest.0 != self.compute_digest() {
            bail!("legacy scalar snapshot digest does not match its typed payload");
        }
        Ok(())
    }

    /// Validate the bounded live-replay contract against a trusted runtime timestamp.
    ///
    /// This is deliberately separate from structural/digest validation: historical rows remain
    /// readable forever, while only a live reserve attempt is constrained by the retention-backed
    /// replay window. The boundary is exclusive: an age equal to 24 hours is expired. Terminal
    /// request machinery is retained for 30 days, leaving a deliberate safety gap before pruning.
    pub fn validate_idempotency_window_at(
        &self,
        trusted_now_ts: i64,
    ) -> std::result::Result<(), LegacyScalarIdempotencyWindowError> {
        if trusted_now_ts <= 0 {
            return Err(LegacyScalarIdempotencyWindowError::InvalidTrustedTimestamp);
        }
        let age = trusted_now_ts
            .checked_sub(self.admission_ts)
            .ok_or(LegacyScalarIdempotencyWindowError::AdmissionFromFuture)?;
        if age < 0 {
            return Err(LegacyScalarIdempotencyWindowError::AdmissionFromFuture);
        }
        if age >= LEGACY_SCALAR_REPLAY_MAX_AGE_SECS {
            return Err(LegacyScalarIdempotencyWindowError::Expired);
        }
        Ok(())
    }

    pub(crate) fn from_stored(
        schema_version: i64,
        input: LegacyScalarAdmissionSnapshotInput,
        snapshot_digest: String,
    ) -> Result<Self> {
        let expected = Self::new(input)?;
        if schema_version != LEGACY_SCALAR_SNAPSHOT_SCHEMA_VERSION
            || snapshot_digest != expected.snapshot_digest.0
        {
            bail!("stored legacy scalar snapshot failed canonical digest verification");
        }
        Ok(expected)
    }

    pub(crate) fn premium_modifiers_json(&self) -> Result<String> {
        self.premium_modifiers.to_canonical_json()
    }

    fn as_input(&self) -> LegacyScalarAdmissionSnapshotInput {
        LegacyScalarAdmissionSnapshotInput {
            request_id: self.request_id.clone(),
            account_id: self.account_id.clone(),
            provider: self.provider,
            requested_model_id: self.requested_model_id.clone(),
            canonical_model_id: self.canonical_model_id.clone(),
            alias_generation: self.alias_generation,
            tariff_schedule_id: self.tariff_schedule_id.clone(),
            tariff_priced_ts: self.tariff_priced_ts,
            admission_ts: self.admission_ts,
            payable_multiplier_bp: self.payable_multiplier_bp,
            official_hold_nano: self.official_hold_nano,
            charged_hold_nano: self.charged_hold_nano,
            premium_modifiers: self.premium_modifiers.clone(),
        }
    }

    fn compute_digest(&self) -> String {
        let mut encoder = CanonicalDigestEncoder::new(SNAPSHOT_DIGEST_DOMAIN);
        encoder.i64(1, self.schema_version);
        encoder.string(2, &self.request_id);
        encoder.string(3, &self.account_id);
        encoder.string(4, "legacy_scalar");
        encoder.string(5, "legacy_scalar");
        encoder.string(6, "legacy");
        encoder.string(7, self.provider.as_str());
        encoder.string(8, &self.requested_model_id);
        encoder.string(9, &self.canonical_model_id);
        encoder.i64(10, self.alias_generation);
        encoder.string(11, &self.tariff_schedule_id);
        encoder.i64(12, self.tariff_priced_ts);
        encoder.i64(13, self.admission_ts);
        encoder.i64(14, self.payable_multiplier_bp);
        encoder.i64(15, self.official_hold_nano);
        encoder.i64(16, self.charged_hold_nano);
        self.premium_modifiers.feed_digest(&mut encoder);
        encoder.finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyScalarReserveConflict {
    ReservationIdentity,
    ExistingReservationWithoutSnapshot,
    ExistingNonLegacySnapshot,
    SnapshotPayload,
    TerminalReservation,
    ExpiredIdempotencyWindow,
    AdmissionTimestampInFuture,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyScalarReserveReceipt {
    pub balance_after_reserve_nano: i64,
    pub snapshot: LegacyScalarAdmissionSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LegacyScalarReserveOutcome {
    NotReserved,
    /// A caller-owned commit gate rejected this attempt. The transaction was rolled back and no
    /// reservation or snapshot from this attempt became durable.
    AbortedBeforeCommit,
    Inserted(LegacyScalarReserveReceipt),
    Unchanged(LegacyScalarReserveReceipt),
    Conflict(LegacyScalarReserveConflict),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyScalarSnapshotLookup {
    Missing,
    NonLegacy,
    Legacy(Box<LegacyScalarAdmissionSnapshot>),
}

fn validate_input(input: &LegacyScalarAdmissionSnapshotInput) -> Result<()> {
    validate_legacy_snapshot_request_id(&input.request_id)?;
    require_bounded_id(
        "snapshot account id",
        &input.account_id,
        SNAPSHOT_ID_MAX_BYTES,
    )?;
    require_bounded_id(
        "snapshot requested model id",
        &input.requested_model_id,
        SNAPSHOT_ID_MAX_BYTES,
    )?;
    require_bounded_id(
        "snapshot canonical model id",
        &input.canonical_model_id,
        SNAPSHOT_ID_MAX_BYTES,
    )?;
    require_bounded_id(
        "snapshot tariff schedule id",
        &input.tariff_schedule_id,
        TARIFF_ID_MAX_BYTES,
    )?;
    if input.alias_generation <= 0 {
        bail!("snapshot alias generation must be positive");
    }
    if input.tariff_priced_ts <= 0 || input.admission_ts <= 0 {
        bail!("snapshot timestamps must be positive");
    }
    if input.tariff_priced_ts > input.admission_ts {
        bail!("snapshot tariff pricing timestamp cannot follow admission");
    }
    if !(0..=10_000).contains(&input.payable_multiplier_bp) {
        bail!("snapshot multiplier must be between zero and 10000 basis points");
    }
    if input.official_hold_nano < 0 || input.charged_hold_nano < 0 {
        bail!("snapshot holds must be non-negative integer nanodollars");
    }
    input
        .premium_modifiers
        .validate_for_provider(input.provider)?;
    input.premium_modifiers.to_canonical_json()?;
    Ok(())
}

pub(crate) fn validate_legacy_snapshot_request_id(value: &str) -> Result<()> {
    require_bounded_id("snapshot request id", value, SNAPSHOT_ID_MAX_BYTES)
}

pub(crate) fn require_bounded_id(label: &str, value: &str, max_bytes: usize) -> Result<()> {
    if value.is_empty() || value.trim() != value {
        bail!("{label} must be non-empty and contain no surrounding whitespace");
    }
    if value.as_bytes().contains(&0) {
        bail!("{label} must not contain a NUL byte");
    }
    if value.len() > max_bytes {
        bail!("{label} exceeds the byte limit");
    }
    Ok(())
}

pub(crate) struct CanonicalDigestEncoder(Sha256);

impl CanonicalDigestEncoder {
    pub(crate) fn new(domain: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(domain);
        Self(hasher)
    }

    pub(crate) fn field(&mut self, tag: u16, payload: &[u8]) {
        self.0.update(tag.to_be_bytes());
        self.0.update((payload.len() as u64).to_be_bytes());
        self.0.update(payload);
    }

    pub(crate) fn string(&mut self, tag: u16, value: &str) {
        self.field(tag, value.as_bytes());
    }

    pub(crate) fn i64(&mut self, tag: u16, value: i64) {
        self.field(tag, &value.to_be_bytes());
    }

    pub(crate) fn finish(self) -> String {
        let digest = self.0.finalize();
        let mut hex = String::with_capacity(64);
        for byte in digest {
            let _ = write!(hex, "{byte:02x}");
        }
        format!("sha256:v1:{hex}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anthropic_input() -> LegacyScalarAdmissionSnapshotInput {
        LegacyScalarAdmissionSnapshotInput {
            request_id: "request-1".into(),
            account_id: "account-1".into(),
            provider: SnapshotProvider::Anthropic,
            requested_model_id: "claude-sonnet-5".into(),
            canonical_model_id: "claude-sonnet-5".into(),
            alias_generation: 1,
            tariff_schedule_id: "anthropic/standard/sonnet-5-intro/v1".into(),
            tariff_priced_ts: 1_788_220_799,
            admission_ts: 1_788_220_800,
            payable_multiplier_bp: 2_000,
            official_hold_nano: 500_000_000,
            charged_hold_nano: 100_000_000,
            premium_modifiers: LegacyPremiumModifiers::AnthropicV1 {
                speed: SnapshotAnthropicSpeed::Standard,
                inference_geo: SnapshotAnthropicInferenceGeo::Global,
                inference_geo_basis_points: 10_000,
            },
        }
    }

    fn openai_input() -> LegacyScalarAdmissionSnapshotInput {
        LegacyScalarAdmissionSnapshotInput {
            request_id: "request-openai-1".into(),
            account_id: "account-openai-1".into(),
            provider: SnapshotProvider::OpenAi,
            requested_model_id: "gpt-5.6".into(),
            canonical_model_id: "gpt-5.6-sol".into(),
            alias_generation: 1,
            tariff_schedule_id: "openai/gpt-5.6-sol/epoch-0/v1".into(),
            tariff_priced_ts: 1_788_220_799,
            admission_ts: 1_788_220_800,
            payable_multiplier_bp: 2_000,
            official_hold_nano: 500_000_000,
            charged_hold_nano: 100_000_000,
            premium_modifiers: LegacyPremiumModifiers::OpenAiV1 {
                service_tier: SnapshotOpenAiServiceTier::Fast,
                service_tier_multiplier_basis_points: 25_000,
                context_tier: SnapshotOpenAiContextTier::Long,
                input_multiplier_basis_points: 20_000,
                output_multiplier_basis_points: 15_000,
            },
        }
    }

    #[test]
    fn legacy_snapshot_digest_has_a_stable_golden_vector() {
        let snapshot = LegacyScalarAdmissionSnapshot::new(anthropic_input()).unwrap();
        assert_eq!(
            snapshot.snapshot_digest().as_str(),
            "sha256:v1:63636a9922d081a641c56a67d63a6a579a5db65368d0cefdadea0f135a4536d1"
        );
        snapshot.validate().unwrap();
    }

    #[test]
    fn openai_legacy_snapshot_digest_has_a_stable_golden_vector() {
        let snapshot = LegacyScalarAdmissionSnapshot::new(openai_input()).unwrap();
        assert_eq!(
            snapshot.snapshot_digest().as_str(),
            "sha256:v1:e0a2d6d1053f0de667fcf9cadf159ddd0a085c271418c10c1cd1fd9c92ae8227"
        );
        snapshot.validate().unwrap();
    }

    #[test]
    fn legacy_snapshot_digest_covers_timestamps_amounts_and_modifiers() {
        let baseline = LegacyScalarAdmissionSnapshot::new(anthropic_input()).unwrap();
        let mut variants = Vec::new();

        let mut changed = anthropic_input();
        changed.request_id.push_str("-changed");
        variants.push(changed);
        let mut changed = anthropic_input();
        changed.account_id.push_str("-changed");
        variants.push(changed);
        let mut changed = anthropic_input();
        changed.requested_model_id.push_str("-changed");
        variants.push(changed);
        let mut changed = anthropic_input();
        changed.canonical_model_id.push_str("-changed");
        variants.push(changed);
        let mut changed = anthropic_input();
        changed.alias_generation += 1;
        variants.push(changed);
        let mut changed = anthropic_input();
        changed.tariff_schedule_id.push_str("-changed");
        variants.push(changed);
        let mut changed = anthropic_input();
        changed.tariff_priced_ts += 1;
        variants.push(changed);
        let mut changed = anthropic_input();
        changed.admission_ts += 1;
        variants.push(changed);
        let mut changed = anthropic_input();
        changed.payable_multiplier_bp += 1;
        variants.push(changed);
        let mut changed = anthropic_input();
        changed.official_hold_nano += 1;
        variants.push(changed);
        let mut changed = anthropic_input();
        changed.charged_hold_nano += 1;
        variants.push(changed);
        let mut changed = anthropic_input();
        changed.premium_modifiers = LegacyPremiumModifiers::AnthropicV1 {
            speed: SnapshotAnthropicSpeed::Fast,
            inference_geo: SnapshotAnthropicInferenceGeo::Global,
            inference_geo_basis_points: 10_000,
        };
        variants.push(changed);

        for changed in variants {
            let changed = LegacyScalarAdmissionSnapshot::new(changed).unwrap();
            assert_ne!(baseline.snapshot_digest(), changed.snapshot_digest());
        }
    }

    #[test]
    fn premium_modifiers_are_strictly_typed_and_provider_bound() {
        let anthropic = LegacyPremiumModifiers::AnthropicV1 {
            speed: SnapshotAnthropicSpeed::Fast,
            inference_geo: SnapshotAnthropicInferenceGeo::Us,
            inference_geo_basis_points: 11_000,
        };
        let encoded = anthropic.to_canonical_json().unwrap();
        assert_eq!(
            LegacyPremiumModifiers::from_json(&encoded).unwrap(),
            anthropic
        );
        assert!(LegacyPremiumModifiers::from_json(
            r#"{"kind":"anthropic_v1","speed":"fast","inference_geo":"us","inference_geo_basis_points":11000,"extra":true}"#
        )
        .is_err());
        assert!(anthropic
            .validate_for_provider(SnapshotProvider::OpenAi)
            .is_err());

        let openai = LegacyPremiumModifiers::OpenAiV1 {
            service_tier: SnapshotOpenAiServiceTier::Fast,
            service_tier_multiplier_basis_points: 25_000,
            context_tier: SnapshotOpenAiContextTier::Long,
            input_multiplier_basis_points: 20_000,
            output_multiplier_basis_points: 15_000,
        };
        let encoded = openai.to_canonical_json().unwrap();
        assert!(encoded.contains(r#""kind":"openai_v1""#));
        assert_eq!(LegacyPremiumModifiers::from_json(&encoded).unwrap(), openai);
        openai
            .validate_for_provider(SnapshotProvider::OpenAi)
            .unwrap();

        let invalid_long = LegacyPremiumModifiers::OpenAiV1 {
            service_tier: SnapshotOpenAiServiceTier::Standard,
            service_tier_multiplier_basis_points: 10_000,
            context_tier: SnapshotOpenAiContextTier::Long,
            input_multiplier_basis_points: 10_000,
            output_multiplier_basis_points: 10_000,
        };
        assert!(invalid_long
            .validate_for_provider(SnapshotProvider::OpenAi)
            .is_err());
    }

    #[test]
    fn stored_snapshot_digest_is_recomputed_instead_of_trusted() {
        let input = anthropic_input();
        let digest = LegacyScalarAdmissionSnapshot::new(input.clone())
            .unwrap()
            .snapshot_digest()
            .as_str()
            .to_owned();
        assert!(LegacyScalarAdmissionSnapshot::from_stored(
            LEGACY_SCALAR_SNAPSHOT_SCHEMA_VERSION,
            input.clone(),
            digest
        )
        .is_ok());
        assert!(LegacyScalarAdmissionSnapshot::from_stored(
            LEGACY_SCALAR_SNAPSHOT_SCHEMA_VERSION,
            input,
            "sha256:v1:wrong".into()
        )
        .is_err());
    }

    #[test]
    fn snapshot_identifiers_reject_cross_backend_incompatible_nul_bytes() {
        let mut input = anthropic_input();
        input.requested_model_id.push('\0');
        assert!(LegacyScalarAdmissionSnapshot::new(input).is_err());
    }

    #[test]
    fn snapshot_rejects_tariff_timestamp_after_admission() {
        let mut input = anthropic_input();
        input.tariff_priced_ts = input.admission_ts + 1;
        assert!(LegacyScalarAdmissionSnapshot::new(input).is_err());
    }

    #[test]
    fn live_idempotency_window_is_typed_and_expires_at_the_replay_boundary() {
        let snapshot = LegacyScalarAdmissionSnapshot::new(anthropic_input()).unwrap();
        let admitted = snapshot.admission_ts();

        assert_eq!(snapshot.validate_idempotency_window_at(admitted), Ok(()));
        assert_eq!(
            snapshot
                .validate_idempotency_window_at(admitted + LEGACY_SCALAR_REPLAY_MAX_AGE_SECS - 1),
            Ok(())
        );
        assert_eq!(
            snapshot.validate_idempotency_window_at(admitted + LEGACY_SCALAR_REPLAY_MAX_AGE_SECS),
            Err(LegacyScalarIdempotencyWindowError::Expired)
        );
        assert_eq!(
            snapshot.validate_idempotency_window_at(admitted - 1),
            Err(LegacyScalarIdempotencyWindowError::AdmissionFromFuture)
        );
        assert_eq!(
            snapshot.validate_idempotency_window_at(0),
            Err(LegacyScalarIdempotencyWindowError::InvalidTrustedTimestamp)
        );
    }

    #[test]
    fn request_lifecycle_prune_cutoff_enforces_the_retention_gap() {
        let trusted_now = PRICING_REQUEST_LIFECYCLE_MIN_RETENTION_SECS + 1_000;
        assert!(validate_request_lifecycle_prune_cutoff(
            trusted_now - PRICING_REQUEST_LIFECYCLE_MIN_RETENTION_SECS,
            trusted_now,
        )
        .is_ok());
        assert!(validate_request_lifecycle_prune_cutoff(
            trusted_now - PRICING_REQUEST_LIFECYCLE_MIN_RETENTION_SECS + 1,
            trusted_now,
        )
        .is_err());
        assert!(validate_request_lifecycle_prune_cutoff(0, 0).is_err());
    }
}
