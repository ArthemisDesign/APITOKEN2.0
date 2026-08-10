//! Typed, backend-neutral persistence contract for versioned multi-provider pricing.
//!
//! Stage 3A stores immutable catalog, switch and account-policy versions and changes their explicit
//! heads with compare-and-set. Stage 3B adds one transactionally coherent, read-only bundle for a
//! pure resolver in `forward`, including both the immutable dependencies pinned by the active
//! policy and the independently moving admission heads. Nothing here participates in live request
//! admission, charging, key issuance, HTTP, policy resolution, or production shadow execution.

mod snapshots;
mod tariffs;

pub(crate) use snapshots::validate_request_lifecycle_prune_cutoff;
pub use snapshots::{
    CanonicalDigestV1, LegacyPremiumModifiers, LegacyScalarAdmissionSnapshot,
    LegacyScalarAdmissionSnapshotInput, LegacyScalarIdempotencyWindowError,
    LegacyScalarReserveConflict, LegacyScalarReserveOutcome, LegacyScalarReserveReceipt,
    SnapshotAnthropicInferenceGeo, SnapshotAnthropicSpeed, SnapshotGeminiContextRate,
    SnapshotGeminiSearchBilling, SnapshotOpenAiContextTier, SnapshotOpenAiImageOperation,
    SnapshotOpenAiServiceTier, SnapshotProvider, LEGACY_SCALAR_REPLAY_MAX_AGE_SECS,
    LEGACY_SCALAR_SNAPSHOT_SCHEMA_VERSION, PRICING_REQUEST_LIFECYCLE_MIN_RETENTION_SECS,
};

pub(crate) use tariffs::{postgres_insert_tariff_override, postgres_list_tariff_overrides};
pub use tariffs::{
    parse_tariff_override_payload, resolve_tariff_override, tariff_override_payload_digest,
    validate_tariff_family, AnthropicTariffPrices, CodexTariffCreditRates, CodexTariffPrices,
    GeminiTariffPrices, GeminiTariffSearchBilling, GlmTariffCreditRates, GlmTariffPrices,
    KimiTariffPrices, OpenAiImageTariffPrices, TariffOverride, TariffOverrideInsert,
    TariffOverrideInsertOutcome, TariffOverridePayload, TariffOverrideRejection,
    TARIFF_OVERRIDE_CLOCK_SKEW_GRACE_SECS,
};


use anyhow::{bail, Result};

/// Advisory-lock name that serializes account-shape writes against each other. It kept account
/// creation from racing a pricing release; the release is gone, but the serialization is cheap and
/// still the reason two concurrent creates cannot interleave.
pub(crate) const ACCOUNT_WRITE_LOCK: &str = "engine:account-write";

/// Reject an empty or padded identifier before it becomes part of an immutable identity.
pub(crate) fn require_id(label: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.trim() != value {
        bail!("{label} must be non-empty and contain no surrounding whitespace");
    }
    Ok(())
}
