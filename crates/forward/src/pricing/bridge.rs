//! Side-effect-free preflight for the default-off legacy-scalar snapshot bridge.
//!
//! This module owns only an impossible-to-misconfigure mode and a versioned deterministic sampler.
//! It has no clock, database, HTTP or metrics. The live reserve callers supply only their trusted
//! fixed provider and engine-owned request ID; provider snapshot builders remain beside their
//! respective legacy quote implementations so this common layer cannot invent prices.

use registry::pricing::SnapshotProvider;
use sha2::{Digest, Sha256};

const SAMPLER_DOMAIN_V1: &[u8] = b"claude-api/pricing/legacy-scalar-bridge-sampler/v1\0";
const SAMPLER_BUCKETS: u16 = 10_000;
// Keep aligned with registry's backend-neutral snapshot identity bound. This pre-money check only
// classifies the expected oversized fallback; the authoritative constructor still revalidates it.
const SNAPSHOT_ID_MAX_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PricingBridgeMode {
    Disabled,
    Sampled { sample_bp: u16 },
}

/// Validated bridge rollout mode. Its private representation cannot express enabled-at-zero or a
/// non-zero sample while disabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PricingBridgeConfig {
    mode: PricingBridgeMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingBridgeConfigError {
    SampleOutOfRange,
    DisabledWithSample,
    EnabledWithoutSample,
}

impl PricingBridgeConfigError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::SampleOutOfRange => "sample_out_of_range",
            Self::DisabledWithSample => "disabled_with_sample",
            Self::EnabledWithoutSample => "enabled_without_sample",
        }
    }
}

impl PricingBridgeConfig {
    pub const fn disabled() -> Self {
        Self {
            mode: PricingBridgeMode::Disabled,
        }
    }

    pub fn from_parts(enabled: bool, sample_bp: i64) -> Result<Self, PricingBridgeConfigError> {
        if !(0..=i64::from(SAMPLER_BUCKETS)).contains(&sample_bp) {
            return Err(PricingBridgeConfigError::SampleOutOfRange);
        }
        match (enabled, sample_bp) {
            (false, 0) => Ok(Self::disabled()),
            (false, _) => Err(PricingBridgeConfigError::DisabledWithSample),
            (true, 0) => Err(PricingBridgeConfigError::EnabledWithoutSample),
            (true, sample_bp) => Ok(Self {
                mode: PricingBridgeMode::Sampled {
                    sample_bp: sample_bp as u16,
                },
            }),
        }
    }

    pub const fn enabled(self) -> bool {
        matches!(self.mode, PricingBridgeMode::Sampled { .. })
    }

    pub const fn sample_bp(self) -> u16 {
        match self.mode {
            PricingBridgeMode::Disabled => 0,
            PricingBridgeMode::Sampled { sample_bp } => sample_bp,
        }
    }

    pub(crate) fn decision(
        self,
        provider: SnapshotProvider,
        request_id: &EnginePricingRequestId,
    ) -> PricingBridgeDecision {
        match self.mode {
            PricingBridgeMode::Disabled => {
                PricingBridgeDecision::Fallback(PricingBridgeFallbackReason::BridgeDisabled)
            }
            PricingBridgeMode::Sampled { sample_bp }
                if sampler_bucket_v1(provider, request_id) < sample_bp =>
            {
                PricingBridgeDecision::Selected
            }
            PricingBridgeMode::Sampled { .. } => {
                PricingBridgeDecision::Fallback(PricingBridgeFallbackReason::NotSampled)
            }
        }
    }
}

impl Default for PricingBridgeConfig {
    fn default() -> Self {
        Self::disabled()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingBridgeDecision {
    Selected,
    Fallback(PricingBridgeFallbackReason),
}

/// Expected pre-money eligibility outcome. A fallback preserves the byte-identical legacy reserve
/// path; hard invariant errors stay outside this enum and must not be disguised as eligibility.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PricingBridgePrepare<T> {
    Eligible(T),
    Fallback(PricingBridgeFallbackReason),
}

/// Stable, low-cardinality reasons shared by the bridge decision and provider builders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingBridgeFallbackReason {
    BridgeDisabled,
    NotSampled,
    UnsupportedModelIdentity,
    UnsupportedModifier,
    SnapshotIdentityOversized,
    OfficialHoldOutOfRange,
}

impl PricingBridgeFallbackReason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::BridgeDisabled => "bridge_disabled",
            Self::NotSampled => "not_sampled",
            Self::UnsupportedModelIdentity => "unsupported_model_identity",
            Self::UnsupportedModifier => "unsupported_modifier",
            Self::SnapshotIdentityOversized => "snapshot_identity_oversized",
            Self::OfficialHoldOutOfRange => "official_hold_out_of_range",
        }
    }
}

/// An engine-owned CSPRNG UUIDv4. Keeping this constructor crate-private prevents a public adapter
/// from accidentally sampling on a client/upstream identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EnginePricingRequestId(String);

impl EnginePricingRequestId {
    pub(crate) fn from_engine_uuid_v4(value: &str) -> Option<Self> {
        let bytes = value.as_bytes();
        if bytes.len() != 36
            || bytes[8] != b'-'
            || bytes[13] != b'-'
            || bytes[18] != b'-'
            || bytes[23] != b'-'
            || bytes[14] != b'4'
            || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        {
            return None;
        }
        if bytes.iter().enumerate().any(|(index, byte)| {
            !matches!(index, 8 | 13 | 18 | 23) && !matches!(*byte, b'0'..=b'9' | b'a'..=b'f')
        }) {
            return None;
        }
        Some(Self(value.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn snapshot_identity_is_oversized(value: &str) -> bool {
    value.len() > SNAPSHOT_ID_MAX_BYTES
}

fn sampler_bucket_v1(provider: SnapshotProvider, request_id: &EnginePricingRequestId) -> u16 {
    fn field(hasher: &mut Sha256, value: &[u8]) {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value);
    }

    let mut hasher = Sha256::new();
    hasher.update(SAMPLER_DOMAIN_V1);
    field(&mut hasher, provider.as_str().as_bytes());
    field(&mut hasher, request_id.as_str().as_bytes());
    let digest = hasher.finalize();
    let value = u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("SHA-256 prefix is eight bytes"),
    );
    (value % u64::from(SAMPLER_BUCKETS)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_id() -> EnginePricingRequestId {
        EnginePricingRequestId::from_engine_uuid_v4("123e4567-e89b-42d3-a456-426614174000").unwrap()
    }

    #[test]
    fn config_has_only_disabled_or_nonzero_bounded_sample_states() {
        assert_eq!(
            PricingBridgeConfig::default(),
            PricingBridgeConfig::disabled()
        );
        assert!(!PricingBridgeConfig::disabled().enabled());
        assert_eq!(PricingBridgeConfig::disabled().sample_bp(), 0);
        assert_eq!(
            PricingBridgeConfig::from_parts(false, 1),
            Err(PricingBridgeConfigError::DisabledWithSample)
        );
        assert_eq!(
            PricingBridgeConfig::from_parts(true, 0),
            Err(PricingBridgeConfigError::EnabledWithoutSample)
        );
        for invalid in [-1, 10_001, i64::MAX] {
            assert_eq!(
                PricingBridgeConfig::from_parts(true, invalid),
                Err(PricingBridgeConfigError::SampleOutOfRange)
            );
        }
        for valid in [1, 5_000, 10_000] {
            let config = PricingBridgeConfig::from_parts(true, valid).unwrap();
            assert!(config.enabled());
            assert_eq!(config.sample_bp(), valid as u16);
        }
    }

    #[test]
    fn engine_request_id_accepts_only_canonical_lowercase_uuid_v4() {
        assert!(EnginePricingRequestId::from_engine_uuid_v4(
            "123e4567-e89b-42d3-a456-426614174000"
        )
        .is_some());
        for invalid in [
            "123e4567-e89b-12d3-a456-426614174000",
            "123e4567-e89b-42d3-7456-426614174000",
            "123E4567-E89B-42D3-A456-426614174000",
            "123e4567e89b42d3a456426614174000",
            "client-request",
        ] {
            assert!(EnginePricingRequestId::from_engine_uuid_v4(invalid).is_none());
        }
    }

    #[test]
    fn sampler_v1_is_stable_provider_separated_and_uses_exact_boundary() {
        let request = request_id();
        let anthropic = sampler_bucket_v1(SnapshotProvider::Anthropic, &request);
        let openai = sampler_bucket_v1(SnapshotProvider::OpenAi, &request);
        let google = sampler_bucket_v1(SnapshotProvider::Google, &request);
        assert_eq!(anthropic, 5_862);
        assert_eq!(openai, 9_992);
        assert_eq!(google, 2_942);

        let below = PricingBridgeConfig::from_parts(true, i64::from(anthropic)).unwrap();
        assert_eq!(
            below.decision(SnapshotProvider::Anthropic, &request),
            PricingBridgeDecision::Fallback(PricingBridgeFallbackReason::NotSampled)
        );
        let selected = PricingBridgeConfig::from_parts(true, i64::from(anthropic) + 1).unwrap();
        assert_eq!(
            selected.decision(SnapshotProvider::Anthropic, &request),
            PricingBridgeDecision::Selected
        );
        assert_eq!(
            PricingBridgeConfig::disabled().decision(SnapshotProvider::Anthropic, &request),
            PricingBridgeDecision::Fallback(PricingBridgeFallbackReason::BridgeDisabled)
        );
        assert_eq!(
            PricingBridgeConfig::from_parts(true, 10_000)
                .unwrap()
                .decision(SnapshotProvider::Anthropic, &request),
            PricingBridgeDecision::Selected
        );
    }

    #[test]
    fn fallback_reason_codes_are_stable_and_low_cardinality() {
        assert_eq!(
            PricingBridgeFallbackReason::BridgeDisabled.code(),
            "bridge_disabled"
        );
        assert_eq!(
            PricingBridgeFallbackReason::NotSampled.code(),
            "not_sampled"
        );
        assert_eq!(
            PricingBridgeFallbackReason::UnsupportedModelIdentity.code(),
            "unsupported_model_identity"
        );
        assert_eq!(
            PricingBridgeFallbackReason::UnsupportedModifier.code(),
            "unsupported_modifier"
        );
        assert_eq!(
            PricingBridgeFallbackReason::SnapshotIdentityOversized.code(),
            "snapshot_identity_oversized"
        );
        assert_eq!(
            PricingBridgeFallbackReason::OfficialHoldOutOfRange.code(),
            "official_hold_out_of_range"
        );
    }
}
