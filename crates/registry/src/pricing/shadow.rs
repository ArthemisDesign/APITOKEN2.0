//! Backend-neutral immutable contract for dormant pricing shadow evaluations.
//!
//! This module owns canonical identities and validation only. It does not resolve a policy, read
//! current heads, enqueue work, change admission, or participate in charging. A future producer
//! must obtain the actual snapshot from the provider-owned admission path and the resolved
//! evidence from one coherent [`super::PricingReadBundle`].

use super::{
    require_id, AccountClass, AccountPolicyRuleSpec, LegacyScalarAdmissionSnapshot,
    PolicyRuleScope, PricingMode, RuleOrigin, SnapshotProvider, VersionTarget,
    PRICING_SCHEMA_VERSION,
};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

const SHADOW_ID_MAX_BYTES: usize = 512;
const SHADOW_DIAGNOSTIC_MAX_BYTES: usize = 4_096;
const SHADOW_DIAGNOSTIC_STORAGE_MAX_BYTES: usize = 32_768;
const SHADOW_DIAGNOSTIC_MAX_DEPTH: usize = 64;
const SHADOW_DIAGNOSTIC_MAX_ITEMS: usize = 1_024;
const SHADOW_MANIFEST_MAX_CAPABILITIES: usize = 128;
const SHADOW_INPUT_MAX_BYTES: usize = 128 * 1_024;
const RUNTIME_MANIFEST_DIGEST_DOMAIN: &[u8] = b"claude-api/pricing/runtime-pricing-manifest/v1\0";
const SHADOW_EVALUATION_DIGEST_DOMAIN: &[u8] =
    b"claude-api/pricing/shadow-admission-evaluation/v1\0";

/// Exact actual-price identity copied only from a validated legacy admission snapshot.
///
/// The admission timestamp is retained privately to reject work enqueued before its actual
/// snapshot. It is already committed by `actual_snapshot_digest` and is therefore not duplicated
/// as a shadow-table column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShadowActualSnapshotRef {
    request_id: String,
    account_id: String,
    provider: SnapshotProvider,
    requested_model_id: String,
    canonical_model_id: String,
    alias_generation: i64,
    admission_ts: i64,
    authorized_multiplier_bp: i64,
    official_hold_nano: i64,
    legacy_hold_nano: i64,
    actual_snapshot_digest: String,
}

/// Stable producer-facing classification for the shadow eligibility gate.
///
/// `BalanceCappedActual` is retained as a wire/metrics compatibility value for binaries from the
/// first rollout. Current snapshots with a lower, balance-capped actual are eligible and use the
/// exact cap-aware comparison below; the variant is no longer emitted by this implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadowEligibilityError {
    InvalidActualSnapshot,
    InvalidEnqueueTimestamp,
    EnqueuedBeforeAdmission,
    InvalidActualAmount,
    BalanceCappedActual,
}

impl std::fmt::Display for ShadowEligibilityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidActualSnapshot => "shadow actual snapshot is invalid",
            Self::InvalidEnqueueTimestamp => "shadow enqueue timestamp must be positive",
            Self::EnqueuedBeforeAdmission => {
                "shadow work cannot be enqueued before actual admission"
            }
            Self::InvalidActualAmount => "shadow actual amount cannot be represented exactly",
            Self::BalanceCappedActual => {
                "balance-capped legacy hold was rejected by an older shadow contract"
            }
        })
    }
}

impl std::error::Error for ShadowEligibilityError {}

impl ShadowActualSnapshotRef {
    pub fn from_snapshot(snapshot: &LegacyScalarAdmissionSnapshot) -> Result<Self> {
        snapshot.validate()?;
        Ok(Self {
            request_id: snapshot.request_id().to_owned(),
            account_id: snapshot.account_id().to_owned(),
            provider: snapshot.provider(),
            requested_model_id: snapshot.requested_model_id().to_owned(),
            canonical_model_id: snapshot.canonical_model_id().to_owned(),
            alias_generation: snapshot.alias_generation(),
            admission_ts: snapshot.admission_ts(),
            authorized_multiplier_bp: snapshot.payable_multiplier_bp(),
            official_hold_nano: snapshot.official_hold_nano(),
            legacy_hold_nano: snapshot.charged_hold_nano(),
            actual_snapshot_digest: snapshot.snapshot_digest().as_str().to_owned(),
        })
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

    pub const fn authorized_multiplier_bp(&self) -> i64 {
        self.authorized_multiplier_bp
    }

    pub const fn official_hold_nano(&self) -> i64 {
        self.official_hold_nano
    }

    pub const fn legacy_hold_nano(&self) -> i64 {
        self.legacy_hold_nano
    }

    pub fn actual_snapshot_digest(&self) -> &str {
        &self.actual_snapshot_digest
    }

    /// Validate the immutable actual reference before a future producer may enqueue shadow work.
    ///
    /// This is the single registry-owned gate for timestamp ordering and exact actual-amount
    /// classification. A legacy hold below its uncapped scalar quote is a proven funding cap and
    /// remains eligible; a hold above that quote is malformed. This performs no reads or writes.
    pub fn validate_shadow_eligibility(
        &self,
        enqueued_ts: i64,
    ) -> std::result::Result<(), ShadowEligibilityError> {
        validate_shadow_eligibility_fields(
            self.admission_ts,
            self.official_hold_nano,
            self.authorized_multiplier_bp,
            self.legacy_hold_nano,
            enqueued_ts,
        )
    }

    /// Apply the same registry-owned gate directly to a validated snapshot before the producer
    /// spends rate budget or clones the bounded work item.
    pub fn validate_snapshot_shadow_eligibility(
        snapshot: &LegacyScalarAdmissionSnapshot,
        enqueued_ts: i64,
    ) -> std::result::Result<(), ShadowEligibilityError> {
        snapshot
            .validate()
            .map_err(|_| ShadowEligibilityError::InvalidActualSnapshot)?;
        validate_shadow_eligibility_fields(
            snapshot.admission_ts(),
            snapshot.official_hold_nano(),
            snapshot.payable_multiplier_bp(),
            snapshot.charged_hold_nano(),
            enqueued_ts,
        )
    }
}

fn validate_shadow_eligibility_fields(
    admission_ts: i64,
    official_hold_nano: i64,
    authorized_multiplier_bp: i64,
    legacy_hold_nano: i64,
    enqueued_ts: i64,
) -> std::result::Result<(), ShadowEligibilityError> {
    if enqueued_ts <= 0 {
        return Err(ShadowEligibilityError::InvalidEnqueueTimestamp);
    }
    if enqueued_ts < admission_ts {
        return Err(ShadowEligibilityError::EnqueuedBeforeAdmission);
    }
    let uncapped = apply_multiplier_nano(official_hold_nano, authorized_multiplier_bp)
        .map_err(|_| ShadowEligibilityError::InvalidActualAmount)?;
    if legacy_hold_nano > uncapped {
        return Err(ShadowEligibilityError::InvalidActualAmount);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PricingRuntimeCapabilityEvidence {
    pricing_schema_version: i64,
    capability_generation: i64,
    capability_digest: String,
}

impl PricingRuntimeCapabilityEvidence {
    pub fn new(
        pricing_schema_version: i64,
        capability_generation: i64,
        capability_digest: impl Into<String>,
    ) -> Result<Self> {
        let capability = Self {
            pricing_schema_version,
            capability_generation,
            capability_digest: capability_digest.into(),
        };
        capability.validate()?;
        Ok(capability)
    }

    pub const fn pricing_schema_version(&self) -> i64 {
        self.pricing_schema_version
    }

    pub const fn capability_generation(&self) -> i64 {
        self.capability_generation
    }

    pub fn capability_digest(&self) -> &str {
        &self.capability_digest
    }

    fn validate(&self) -> Result<()> {
        if self.pricing_schema_version <= 0 || self.capability_generation <= 0 {
            bail!("runtime pricing capability generations must be positive");
        }
        require_shadow_id("runtime pricing capability digest", &self.capability_digest)
    }
}

/// Full trusted runtime evidence whose digest is computed from the canonical member set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingRuntimeManifestEvidence {
    manifest_generation: i64,
    manifest_digest: String,
    capabilities: Vec<PricingRuntimeCapabilityEvidence>,
}

impl PricingRuntimeManifestEvidence {
    pub fn new(
        manifest_generation: i64,
        mut capabilities: Vec<PricingRuntimeCapabilityEvidence>,
    ) -> Result<Self> {
        if manifest_generation <= 0 {
            bail!("runtime pricing manifest generation must be positive");
        }
        if capabilities.is_empty() {
            bail!("runtime pricing manifest must contain at least one capability");
        }
        if capabilities.len() > SHADOW_MANIFEST_MAX_CAPABILITIES {
            bail!("runtime pricing manifest exceeds the capability limit");
        }
        for capability in &capabilities {
            capability.validate()?;
        }
        capabilities.sort();
        for pair in capabilities.windows(2) {
            if pair[0].pricing_schema_version == pair[1].pricing_schema_version
                && pair[0].capability_generation == pair[1].capability_generation
            {
                bail!(
                    "runtime pricing manifest has duplicate or ambiguous schema/generation identity"
                );
            }
        }

        let mut manifest = Self {
            manifest_generation,
            manifest_digest: String::new(),
            capabilities,
        };
        manifest.manifest_digest = manifest.compute_digest();
        Ok(manifest)
    }

    pub const fn manifest_generation(&self) -> i64 {
        self.manifest_generation
    }

    pub fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }

    pub fn capabilities(&self) -> &[PricingRuntimeCapabilityEvidence] {
        &self.capabilities
    }

    pub fn supports(&self, dependency: &PricingShadowDependency) -> bool {
        self.capabilities
            .binary_search_by(|capability| {
                (
                    capability.pricing_schema_version,
                    capability.capability_generation,
                    capability.capability_digest.as_str(),
                )
                    .cmp(&(
                        dependency.pricing_schema_version,
                        dependency.capability_generation,
                        dependency.capability_digest.as_str(),
                    ))
            })
            .is_ok()
    }

    fn compute_digest(&self) -> String {
        let mut encoder = CanonicalDigestEncoder::new(RUNTIME_MANIFEST_DIGEST_DOMAIN);
        encoder.i64(1, 1);
        encoder.i64(2, self.manifest_generation);
        encoder.i64(3, self.capabilities.len() as i64);
        for capability in &self.capabilities {
            encoder.i64(10, capability.pricing_schema_version);
            encoder.i64(11, capability.capability_generation);
            encoder.string(12, &capability.capability_digest);
        }
        encoder.finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingShadowDependency {
    pub target: VersionTarget,
    pub pricing_schema_version: i64,
    pub capability_generation: i64,
    pub capability_digest: String,
}

impl PricingShadowDependency {
    fn validate(&self, label: &str) -> Result<()> {
        if self.target.version <= 0
            || self.pricing_schema_version <= 0
            || self.capability_generation <= 0
        {
            bail!("{label} generations and schema must be positive");
        }
        require_shadow_id(
            &format!("{label} content digest"),
            &self.target.content_digest,
        )?;
        require_shadow_id(
            &format!("{label} capability digest"),
            &self.capability_digest,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingShadowLineage {
    pub catalog: PricingShadowDependency,
    pub switches: PricingShadowDependency,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingShadowPolicyIdentity {
    pub target: VersionTarget,
    pub policy_id: String,
    pub policy_version: i64,
    pub source_policy_digest: String,
    pub schema_version: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingShadowComparison {
    Equal,
    Different,
}

impl PricingShadowComparison {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Equal => "equal",
            Self::Different => "different",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "equal" => Ok(Self::Equal),
            "different" => Ok(Self::Different),
            _ => bail!("stored resolved shadow evaluation has invalid comparison"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingShadowResolvedInput {
    pub observed_multiplier_bp: i64,
    pub product_id: String,
    pub account_class: AccountClass,
    pub policy: PricingShadowPolicyIdentity,
    pub policy_lineage: PricingShadowLineage,
    pub admission_lineage: PricingShadowLineage,
    pub rule: AccountPolicyRuleSpec,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingShadowResolved {
    pub observed_multiplier_bp: i64,
    pub product_id: String,
    pub account_class: AccountClass,
    pub policy: PricingShadowPolicyIdentity,
    pub policy_lineage: PricingShadowLineage,
    pub admission_lineage: PricingShadowLineage,
    pub rule: AccountPolicyRuleSpec,
    policy_hold_nano: i64,
    comparison: PricingShadowComparison,
}

impl PricingShadowResolved {
    pub fn new(
        actual: &ShadowActualSnapshotRef,
        input: PricingShadowResolvedInput,
    ) -> Result<Self> {
        validate_rule(actual, &input.rule)?;
        let policy_hold_nano = policy_hold_nano(actual, input.rule.payable_multiplier_bp)?;
        let comparison = if policy_hold_nano == actual.legacy_hold_nano {
            PricingShadowComparison::Equal
        } else {
            PricingShadowComparison::Different
        };
        let resolved = Self {
            observed_multiplier_bp: input.observed_multiplier_bp,
            product_id: input.product_id,
            account_class: input.account_class,
            policy: input.policy,
            policy_lineage: input.policy_lineage,
            admission_lineage: input.admission_lineage,
            rule: input.rule,
            policy_hold_nano,
            comparison,
        };
        resolved.validate(actual, PRICING_SCHEMA_VERSION, None)?;
        Ok(resolved)
    }

    pub const fn policy_hold_nano(&self) -> i64 {
        self.policy_hold_nano
    }

    pub const fn comparison(&self) -> PricingShadowComparison {
        self.comparison
    }

    fn validate(
        &self,
        actual: &ShadowActualSnapshotRef,
        evaluator_schema_version: i64,
        manifest: Option<&PricingRuntimeManifestEvidence>,
    ) -> Result<()> {
        if !(0..=10_000).contains(&self.observed_multiplier_bp) {
            bail!("observed shadow multiplier must be between zero and 10000 basis points");
        }
        require_shadow_id("shadow product id", &self.product_id)?;
        if self.policy.target.version <= 0 || self.policy.policy_version <= 0 {
            bail!("shadow policy versions must be positive");
        }
        require_shadow_id("shadow policy id", &self.policy.policy_id)?;
        require_shadow_id(
            "shadow source policy digest",
            &self.policy.source_policy_digest,
        )?;
        require_shadow_id("shadow policy digest", &self.policy.target.content_digest)?;
        if self.policy.schema_version != evaluator_schema_version {
            bail!("shadow policy schema does not match evaluator schema");
        }

        validate_lineage(
            "policy",
            &self.policy_lineage,
            evaluator_schema_version,
            true,
        )?;
        validate_lineage(
            "admission",
            &self.admission_lineage,
            evaluator_schema_version,
            false,
        )?;
        validate_rule(actual, &self.rule)?;
        let expected_hold = policy_hold_nano(actual, self.rule.payable_multiplier_bp)?;
        if self.policy_hold_nano != expected_hold {
            bail!("stored shadow policy hold does not match the selected rule");
        }
        let expected_comparison = if expected_hold == actual.legacy_hold_nano {
            PricingShadowComparison::Equal
        } else {
            PricingShadowComparison::Different
        };
        if self.comparison != expected_comparison {
            bail!("stored shadow comparison does not match the exact holds");
        }

        if let Some(manifest) = manifest {
            for (label, dependency) in [
                ("policy catalog", &self.policy_lineage.catalog),
                ("policy switches", &self.policy_lineage.switches),
                ("admission catalog", &self.admission_lineage.catalog),
                ("admission switches", &self.admission_lineage.switches),
            ] {
                if !manifest.supports(dependency) {
                    bail!("{label} capability is absent from the runtime manifest");
                }
            }
        }
        Ok(())
    }
}

macro_rules! rejection_codes {
    ($( $variant:ident => $value:literal ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(usize)]
        pub enum PricingShadowRejectionCode {
            $( $variant, )+
        }

        impl PricingShadowRejectionCode {
            pub const ALL: &'static [Self] = &[$( Self::$variant, )+];

            pub const fn metric_index(self) -> usize {
                self as usize
            }

            pub const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $value, )+
                }
            }

            fn from_db(value: &str) -> Result<Self> {
                match value {
                    $( $value => Ok(Self::$variant), )+
                    _ => bail!("stored shadow rejection has an unknown reason code"),
                }
            }
        }
    };
}

rejection_codes!(
    InvalidRequest => "invalid_request",
    InvalidRuntimeManifest => "invalid_runtime_manifest",
    AccountMismatch => "account_mismatch",
    NoPolicyBinding => "no_policy_binding",
    InactivePolicy => "inactive_policy",
    MissingPolicyCatalog => "missing_policy_catalog",
    MissingPolicySwitches => "missing_policy_switches",
    MissingAdmissionCatalog => "missing_admission_catalog",
    MissingAdmissionSwitches => "missing_admission_switches",
    PolicySchemaMismatch => "policy_schema_mismatch",
    PolicyCatalogSchemaMismatch => "policy_catalog_schema_mismatch",
    PolicySwitchSchemaMismatch => "policy_switch_schema_mismatch",
    AdmissionCatalogSchemaMismatch => "admission_catalog_schema_mismatch",
    AdmissionSwitchSchemaMismatch => "admission_switch_schema_mismatch",
    PolicyCatalogTargetMismatch => "policy_catalog_target_mismatch",
    AdmissionCatalogTargetMismatch => "admission_catalog_target_mismatch",
    PolicySwitchTargetMismatch => "policy_switch_target_mismatch",
    UnsupportedPolicyCatalogCapability => "unsupported_policy_catalog_capability",
    UnsupportedPolicySwitchCapability => "unsupported_policy_switch_capability",
    UnsupportedAdmissionCatalogCapability => "unsupported_admission_catalog_capability",
    UnsupportedAdmissionSwitchCapability => "unsupported_admission_switch_capability",
    InvalidPolicyCatalog => "invalid_policy_catalog",
    InvalidPolicySwitches => "invalid_policy_switches",
    InvalidAdmissionCatalog => "invalid_admission_catalog",
    InvalidAdmissionSwitches => "invalid_admission_switches",
    InvalidPolicyContract => "invalid_policy_contract",
    PolicyModelNotInCatalog => "policy_model_not_in_catalog",
    AdmissionModelNotInCatalog => "admission_model_not_in_catalog",
    PolicyModelDisabled => "policy_model_disabled",
    AdmissionModelDisabled => "admission_model_disabled",
    MissingPolicyMasterSwitch => "missing_policy_master_switch",
    MissingAdmissionMasterSwitch => "missing_admission_master_switch",
    PolicyMasterSwitchDisabled => "policy_master_switch_disabled",
    AdmissionMasterSwitchDisabled => "admission_master_switch_disabled",
    MissingPolicyScopedSwitch => "missing_policy_scoped_switch",
    MissingAdmissionScopedSwitch => "missing_admission_scoped_switch",
    PolicyScopedSwitchTargetMismatch => "policy_scoped_switch_target_mismatch",
    AdmissionScopedSwitchTargetMismatch => "admission_scoped_switch_target_mismatch",
    PolicyScopedSwitchDisabled => "policy_scoped_switch_disabled",
    AdmissionScopedSwitchDisabled => "admission_scoped_switch_disabled",
    MissingRule => "missing_rule",
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum PricingShadowReadErrorCode {
    PricingReadFailed,
    EvaluationTimeout,
    EvaluationCancelled,
    InvalidActualSnapshot,
    InvalidResolvedAmount,
}

impl PricingShadowReadErrorCode {
    pub const ALL: &'static [Self] = &[
        Self::PricingReadFailed,
        Self::EvaluationTimeout,
        Self::EvaluationCancelled,
        Self::InvalidActualSnapshot,
        Self::InvalidResolvedAmount,
    ];

    pub const fn metric_index(self) -> usize {
        self as usize
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PricingReadFailed => "pricing_read_failed",
            Self::EvaluationTimeout => "evaluation_timeout",
            Self::EvaluationCancelled => "evaluation_cancelled",
            Self::InvalidActualSnapshot => "invalid_actual_snapshot",
            Self::InvalidResolvedAmount => "invalid_resolved_amount",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "pricing_read_failed" => Ok(Self::PricingReadFailed),
            "evaluation_timeout" => Ok(Self::EvaluationTimeout),
            "evaluation_cancelled" => Ok(Self::EvaluationCancelled),
            "invalid_actual_snapshot" => Ok(Self::InvalidActualSnapshot),
            "invalid_resolved_amount" => Ok(Self::InvalidResolvedAmount),
            _ => bail!("stored shadow read error has an unknown reason code"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PricingShadowEvaluationOutcome {
    Resolved(Box<PricingShadowResolved>),
    Rejected {
        reason: PricingShadowRejectionCode,
        observed_multiplier_bp: i64,
    },
    ReadError {
        reason: PricingShadowReadErrorCode,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShadowDiagnosticContext(Value);

impl ShadowDiagnosticContext {
    pub fn new(value: Value) -> Result<Self> {
        if !value.is_object() {
            bail!("shadow diagnostic context must be a JSON object");
        }
        validate_diagnostic_jsonb_compatibility(&value)?;
        let encoded = serde_json::to_string(&value).context("encode shadow diagnostic context")?;
        if encoded.len() > SHADOW_DIAGNOSTIC_MAX_BYTES {
            bail!("shadow diagnostic context exceeds the encoded size limit");
        }
        Ok(Self(value))
    }

    pub fn empty() -> Self {
        Self(Value::Object(Default::default()))
    }

    pub fn value(&self) -> &Value {
        &self.0
    }

    pub(crate) fn to_json(&self) -> Result<String> {
        let encoded = serde_json::to_string(&self.0).context("encode shadow diagnostic context")?;
        if encoded.len() > SHADOW_DIAGNOSTIC_MAX_BYTES {
            bail!("shadow diagnostic context exceeds the encoded size limit");
        }
        Ok(encoded)
    }

    pub(crate) fn from_json(value: &str) -> Result<Self> {
        // PostgreSQL JSONB renders insignificant spaces which are absent from the canonical
        // compact serde encoding accepted at write time. Bound raw storage text separately, then
        // apply the authoritative compact limit after parsing.
        if value.len() > SHADOW_DIAGNOSTIC_STORAGE_MAX_BYTES {
            bail!("stored shadow diagnostic context exceeds the storage safety limit");
        }
        Self::new(serde_json::from_str(value).context("decode stored shadow diagnostic context")?)
    }
}

fn validate_diagnostic_jsonb_compatibility(value: &Value) -> Result<()> {
    // PostgreSQL JSONB cannot represent U+0000 even though JSON and SQLite JSON text can encode it
    // as `\u0000`. Validate iteratively so a deeply nested, still byte-bounded diagnostic cannot
    // consume the Rust call stack.
    let mut pending = vec![(value, 0_usize)];
    let mut item_count = 0_usize;
    while let Some((value, depth)) = pending.pop() {
        item_count = item_count.saturating_add(1);
        if item_count > SHADOW_DIAGNOSTIC_MAX_ITEMS {
            bail!("shadow diagnostic JSON exceeds the item limit");
        }
        if depth > SHADOW_DIAGNOSTIC_MAX_DEPTH {
            bail!("shadow diagnostic JSON exceeds the nesting limit");
        }
        match value {
            Value::String(value) if value.as_bytes().contains(&0) => {
                bail!("shadow diagnostic JSON strings must not contain a NUL byte")
            }
            Value::Array(values) => pending.extend(values.iter().map(|value| (value, depth + 1))),
            Value::Object(values) => {
                if values.keys().any(|key| key.as_bytes().contains(&0)) {
                    bail!("shadow diagnostic JSON keys must not contain a NUL byte");
                }
                pending.extend(values.values().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validated write candidate. Manifest members are intentionally not persisted; their canonical
/// digest and all four resolved dependency pins are persisted after insert-time membership proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingShadowAdmissionEvaluationInput {
    actual: ShadowActualSnapshotRef,
    evaluator_schema_version: i64,
    runtime_manifest: PricingRuntimeManifestEvidence,
    enqueued_ts: i64,
    evaluated_ts: i64,
    outcome: PricingShadowEvaluationOutcome,
    diagnostic_context: ShadowDiagnosticContext,
}

impl PricingShadowAdmissionEvaluationInput {
    pub fn new(
        actual: ShadowActualSnapshotRef,
        evaluator_schema_version: i64,
        runtime_manifest: PricingRuntimeManifestEvidence,
        enqueued_ts: i64,
        evaluated_ts: i64,
        outcome: PricingShadowEvaluationOutcome,
        diagnostic_context: ShadowDiagnosticContext,
    ) -> Result<Self> {
        let input = Self {
            actual,
            evaluator_schema_version,
            runtime_manifest,
            enqueued_ts,
            evaluated_ts,
            outcome,
            diagnostic_context,
        };
        input.validate()?;
        Ok(input)
    }

    pub fn actual(&self) -> &ShadowActualSnapshotRef {
        &self.actual
    }

    pub fn runtime_manifest(&self) -> &PricingRuntimeManifestEvidence {
        &self.runtime_manifest
    }

    pub fn outcome(&self) -> &PricingShadowEvaluationOutcome {
        &self.outcome
    }

    fn validate(&self) -> Result<()> {
        if self.evaluator_schema_version != PRICING_SCHEMA_VERSION {
            bail!("unsupported shadow evaluator schema version");
        }
        if self.enqueued_ts <= 0 || self.evaluated_ts <= 0 || self.evaluated_ts < self.enqueued_ts {
            bail!("shadow evaluation timestamps are invalid");
        }
        self.actual.validate_shadow_eligibility(self.enqueued_ts)?;
        validate_digest(
            "runtime manifest digest",
            &self.runtime_manifest.manifest_digest,
        )?;
        match &self.outcome {
            PricingShadowEvaluationOutcome::Resolved(resolved) => resolved.validate(
                &self.actual,
                self.evaluator_schema_version,
                Some(&self.runtime_manifest),
            )?,
            PricingShadowEvaluationOutcome::Rejected {
                observed_multiplier_bp,
                ..
            } => {
                if !(0..=10_000).contains(observed_multiplier_bp) {
                    bail!("observed shadow multiplier must be between zero and 10000 basis points");
                }
            }
            PricingShadowEvaluationOutcome::ReadError { .. } => {}
        }
        self.diagnostic_context.to_json()?;
        if self.encoded_size_estimate() > SHADOW_INPUT_MAX_BYTES {
            bail!("shadow evaluation input exceeds the total size limit");
        }
        Ok(())
    }

    fn encoded_size_estimate(&self) -> usize {
        let mut size = self.actual.request_id.len()
            + self.actual.account_id.len()
            + self.actual.requested_model_id.len()
            + self.actual.canonical_model_id.len()
            + self.actual.actual_snapshot_digest.len()
            + self.runtime_manifest.manifest_digest.len()
            + self
                .diagnostic_context
                .to_json()
                .map_or(usize::MAX, |json| json.len());
        for capability in &self.runtime_manifest.capabilities {
            size = size.saturating_add(capability.capability_digest.len() + 24);
        }
        if let PricingShadowEvaluationOutcome::Resolved(resolved) = &self.outcome {
            size = size
                .saturating_add(resolved.product_id.len())
                .saturating_add(resolved.policy.policy_id.len())
                .saturating_add(resolved.policy.source_policy_digest.len())
                .saturating_add(resolved.policy.target.content_digest.len())
                .saturating_add(resolved.rule.rule_id.len())
                .saturating_add(resolved.rule.rule_digest.len());
            for dependency in [
                &resolved.policy_lineage.catalog,
                &resolved.policy_lineage.switches,
                &resolved.admission_lineage.catalog,
                &resolved.admission_lineage.switches,
            ] {
                size = size
                    .saturating_add(dependency.target.content_digest.len())
                    .saturating_add(dependency.capability_digest.len());
            }
        }
        size
    }

    pub(crate) fn to_evaluation(&self) -> Result<PricingShadowAdmissionEvaluation> {
        self.validate()?;
        PricingShadowAdmissionEvaluation::new(
            self.actual.clone(),
            self.evaluator_schema_version,
            self.runtime_manifest.manifest_generation,
            self.runtime_manifest.manifest_digest.clone(),
            self.enqueued_ts,
            self.evaluated_ts,
            self.outcome.clone(),
            self.diagnostic_context.clone(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShadowEvaluationDigestV1(String);

impl ShadowEvaluationDigestV1 {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingShadowAdmissionEvaluation {
    actual: ShadowActualSnapshotRef,
    evaluator_schema_version: i64,
    runtime_manifest_generation: i64,
    runtime_manifest_digest: String,
    enqueued_ts: i64,
    evaluated_ts: i64,
    outcome: PricingShadowEvaluationOutcome,
    diagnostic_context: ShadowDiagnosticContext,
    evaluation_digest: ShadowEvaluationDigestV1,
}

impl PricingShadowAdmissionEvaluation {
    #[allow(clippy::too_many_arguments)]
    fn new(
        actual: ShadowActualSnapshotRef,
        evaluator_schema_version: i64,
        runtime_manifest_generation: i64,
        runtime_manifest_digest: String,
        enqueued_ts: i64,
        evaluated_ts: i64,
        outcome: PricingShadowEvaluationOutcome,
        diagnostic_context: ShadowDiagnosticContext,
    ) -> Result<Self> {
        let mut evaluation = Self {
            actual,
            evaluator_schema_version,
            runtime_manifest_generation,
            runtime_manifest_digest,
            enqueued_ts,
            evaluated_ts,
            outcome,
            diagnostic_context,
            evaluation_digest: ShadowEvaluationDigestV1(String::new()),
        };
        evaluation.validate_shape()?;
        evaluation.evaluation_digest = ShadowEvaluationDigestV1(evaluation.compute_digest());
        Ok(evaluation)
    }

    pub fn actual(&self) -> &ShadowActualSnapshotRef {
        &self.actual
    }

    pub const fn evaluator_schema_version(&self) -> i64 {
        self.evaluator_schema_version
    }

    pub const fn runtime_manifest_generation(&self) -> i64 {
        self.runtime_manifest_generation
    }

    pub fn runtime_manifest_digest(&self) -> &str {
        &self.runtime_manifest_digest
    }

    pub const fn enqueued_ts(&self) -> i64 {
        self.enqueued_ts
    }

    pub const fn evaluated_ts(&self) -> i64 {
        self.evaluated_ts
    }

    pub fn outcome(&self) -> &PricingShadowEvaluationOutcome {
        &self.outcome
    }

    pub fn diagnostic_context(&self) -> &ShadowDiagnosticContext {
        &self.diagnostic_context
    }

    pub fn evaluation_digest(&self) -> &ShadowEvaluationDigestV1 {
        &self.evaluation_digest
    }

    pub(crate) fn same_semantics(&self, other: &Self) -> bool {
        self.actual == other.actual
            && self.evaluator_schema_version == other.evaluator_schema_version
            && self.runtime_manifest_generation == other.runtime_manifest_generation
            && self.runtime_manifest_digest == other.runtime_manifest_digest
            && self.outcome == other.outcome
    }

    pub(crate) fn classify_existing(
        &self,
        existing: PricingShadowAdmissionEvaluation,
    ) -> Result<PricingShadowEvaluationWrite> {
        if self.evaluation_digest == existing.evaluation_digest {
            if !self.same_semantics(&existing) {
                bail!("shadow evaluation digest collision or inconsistent canonical payload");
            }
            return Ok(PricingShadowEvaluationWrite::Unchanged(Box::new(existing)));
        }
        Ok(PricingShadowEvaluationWrite::Conflict(
            PricingShadowEvaluationConflict::ExistingSemanticResult,
        ))
    }

    fn validate_shape(&self) -> Result<()> {
        if self.evaluator_schema_version != PRICING_SCHEMA_VERSION {
            bail!("unsupported stored shadow evaluator schema version");
        }
        if self.runtime_manifest_generation <= 0 {
            bail!("stored runtime manifest generation must be positive");
        }
        validate_digest(
            "stored runtime manifest digest",
            &self.runtime_manifest_digest,
        )?;
        if self.enqueued_ts <= 0 || self.evaluated_ts <= 0 || self.evaluated_ts < self.enqueued_ts {
            bail!("stored shadow evaluation timestamps are invalid");
        }
        self.actual.validate_shadow_eligibility(self.enqueued_ts)?;
        match &self.outcome {
            PricingShadowEvaluationOutcome::Resolved(resolved) => {
                resolved.validate(&self.actual, self.evaluator_schema_version, None)?
            }
            PricingShadowEvaluationOutcome::Rejected {
                observed_multiplier_bp,
                ..
            } if !(0..=10_000).contains(observed_multiplier_bp) => {
                bail!("stored observed multiplier is out of range")
            }
            PricingShadowEvaluationOutcome::Rejected { .. }
            | PricingShadowEvaluationOutcome::ReadError { .. } => {}
        }
        self.diagnostic_context.to_json()?;
        Ok(())
    }

    fn compute_digest(&self) -> String {
        let mut encoder = CanonicalDigestEncoder::new(SHADOW_EVALUATION_DIGEST_DOMAIN);
        encoder.i64(1, self.evaluator_schema_version);
        encoder.string(2, &self.actual.request_id);
        encoder.string(3, &self.actual.account_id);
        encoder.string(4, "legacy_scalar");
        encoder.string(5, &self.actual.actual_snapshot_digest);
        encoder.string(6, self.actual.provider.as_str());
        encoder.string(7, &self.actual.requested_model_id);
        encoder.string(8, &self.actual.canonical_model_id);
        encoder.i64(9, self.actual.alias_generation);
        encoder.i64(10, self.runtime_manifest_generation);
        encoder.string(11, &self.runtime_manifest_digest);
        encoder.i64(12, self.actual.authorized_multiplier_bp);
        encoder.i64(13, self.actual.official_hold_nano);
        encoder.i64(14, self.actual.legacy_hold_nano);

        match &self.outcome {
            PricingShadowEvaluationOutcome::Resolved(resolved) => {
                encoder.string(20, "resolved");
                encoder.bool(21, true);
                encoder.i64(22, resolved.observed_multiplier_bp);
                encoder.string(30, &resolved.product_id);
                encoder.string(31, resolved.account_class.as_str());
                encoder.i64(32, resolved.policy.target.version);
                encoder.string(33, &resolved.policy.policy_id);
                encoder.i64(34, resolved.policy.policy_version);
                encoder.string(35, &resolved.policy.source_policy_digest);
                encoder.string(36, &resolved.policy.target.content_digest);
                encoder.i64(37, resolved.policy.schema_version);
                feed_dependency(&mut encoder, 40, &resolved.policy_lineage.catalog);
                feed_dependency(&mut encoder, 50, &resolved.policy_lineage.switches);
                feed_dependency(&mut encoder, 60, &resolved.admission_lineage.catalog);
                feed_dependency(&mut encoder, 70, &resolved.admission_lineage.switches);
                let (scope, provider, canonical) = resolved.rule.scope.db_parts();
                encoder.string(80, &resolved.rule.rule_id);
                encoder.string(81, &resolved.rule.rule_digest);
                encoder.string(82, scope);
                encoder.string(83, provider);
                encoder.bool(84, canonical.is_some());
                if let Some(canonical) = canonical {
                    encoder.string(85, canonical);
                }
                encoder.string(86, resolved.rule.pricing_mode.as_str());
                encoder.string(87, resolved.rule.rule_origin.as_str());
                encoder.bool(88, resolved.rule.discount_bps.is_some());
                if let Some(discount_bps) = resolved.rule.discount_bps {
                    encoder.i64(89, discount_bps);
                }
                encoder.i64(90, resolved.rule.payable_multiplier_bp);
                encoder.bool(91, resolved.rule.track_eligible);
                encoder.bool(92, resolved.rule.retention_eligible);
                encoder.bool(93, resolved.rule.commission_eligible);
                encoder.i64(94, resolved.policy_hold_nano);
                encoder.string(95, resolved.comparison.as_str());
            }
            PricingShadowEvaluationOutcome::Rejected {
                reason,
                observed_multiplier_bp,
            } => {
                encoder.string(20, "rejected");
                encoder.bool(21, true);
                encoder.i64(22, *observed_multiplier_bp);
                encoder.string(23, reason.as_str());
                encoder.string(95, "not_comparable");
            }
            PricingShadowEvaluationOutcome::ReadError { reason } => {
                encoder.string(20, "read_error");
                encoder.bool(21, false);
                encoder.string(23, reason.as_str());
                encoder.string(95, "not_comparable");
            }
        }
        encoder.finish()
    }

    pub(crate) fn storage_row(&self) -> Result<PricingShadowStorageRow> {
        let diagnostic_context = self.diagnostic_context.to_json()?;
        let mut row = PricingShadowStorageRow {
            request_id: self.actual.request_id.clone(),
            account_id: self.actual.account_id.clone(),
            actual_snapshot_kind: "legacy_scalar".into(),
            actual_snapshot_digest: self.actual.actual_snapshot_digest.clone(),
            provider_id: self.actual.provider.as_str().into(),
            requested_model_id: self.actual.requested_model_id.clone(),
            canonical_model_id: self.actual.canonical_model_id.clone(),
            alias_generation: self.actual.alias_generation,
            evaluator_schema_version: self.evaluator_schema_version,
            runtime_manifest_generation: self.runtime_manifest_generation,
            runtime_manifest_digest: self.runtime_manifest_digest.clone(),
            enqueued_ts: self.enqueued_ts,
            evaluated_ts: self.evaluated_ts,
            outcome: String::new(),
            reason_code: None,
            authorized_multiplier_bp: self.actual.authorized_multiplier_bp,
            observed_multiplier_bp: None,
            official_hold_nano: self.actual.official_hold_nano,
            legacy_hold_nano: self.actual.legacy_hold_nano,
            product_id: None,
            account_class: None,
            effective_policy_version: None,
            policy_id: None,
            policy_version: None,
            source_policy_digest: None,
            policy_digest: None,
            policy_schema_version: None,
            policy_catalog_generation: None,
            policy_catalog_schema_version: None,
            policy_catalog_capability_generation: None,
            policy_catalog_capability_digest: None,
            policy_catalog_digest: None,
            policy_switch_generation: None,
            policy_switch_schema_version: None,
            policy_switch_capability_generation: None,
            policy_switch_capability_digest: None,
            policy_switch_digest: None,
            admission_catalog_generation: None,
            admission_catalog_schema_version: None,
            admission_catalog_capability_generation: None,
            admission_catalog_capability_digest: None,
            admission_catalog_digest: None,
            admission_switch_generation: None,
            admission_switch_schema_version: None,
            admission_switch_capability_generation: None,
            admission_switch_capability_digest: None,
            admission_switch_digest: None,
            rule_id: None,
            rule_digest: None,
            rule_scope: None,
            pricing_mode: None,
            rule_origin: None,
            discount_bps: None,
            payable_multiplier_bp: None,
            track_eligible: None,
            retention_eligible: None,
            commission_eligible: None,
            policy_hold_nano: None,
            comparison_result: "not_comparable".into(),
            diagnostic_context,
            evaluation_digest: self.evaluation_digest.0.clone(),
        };
        match &self.outcome {
            PricingShadowEvaluationOutcome::Resolved(resolved) => {
                row.outcome = "resolved".into();
                row.observed_multiplier_bp = Some(resolved.observed_multiplier_bp);
                row.product_id = Some(resolved.product_id.clone());
                row.account_class = Some(resolved.account_class.as_str().into());
                row.effective_policy_version = Some(resolved.policy.target.version);
                row.policy_id = Some(resolved.policy.policy_id.clone());
                row.policy_version = Some(resolved.policy.policy_version);
                row.source_policy_digest = Some(resolved.policy.source_policy_digest.clone());
                row.policy_digest = Some(resolved.policy.target.content_digest.clone());
                row.policy_schema_version = Some(resolved.policy.schema_version);
                copy_dependency_to_row(
                    &mut row,
                    DependencySlot::PolicyCatalog,
                    &resolved.policy_lineage.catalog,
                );
                copy_dependency_to_row(
                    &mut row,
                    DependencySlot::PolicySwitch,
                    &resolved.policy_lineage.switches,
                );
                copy_dependency_to_row(
                    &mut row,
                    DependencySlot::AdmissionCatalog,
                    &resolved.admission_lineage.catalog,
                );
                copy_dependency_to_row(
                    &mut row,
                    DependencySlot::AdmissionSwitch,
                    &resolved.admission_lineage.switches,
                );
                let (scope, _, _) = resolved.rule.scope.db_parts();
                row.rule_id = Some(resolved.rule.rule_id.clone());
                row.rule_digest = Some(resolved.rule.rule_digest.clone());
                row.rule_scope = Some(scope.into());
                row.pricing_mode = Some(resolved.rule.pricing_mode.as_str().into());
                row.rule_origin = Some(resolved.rule.rule_origin.as_str().into());
                row.discount_bps = resolved.rule.discount_bps;
                row.payable_multiplier_bp = Some(resolved.rule.payable_multiplier_bp);
                row.track_eligible = Some(resolved.rule.track_eligible);
                row.retention_eligible = Some(resolved.rule.retention_eligible);
                row.commission_eligible = Some(resolved.rule.commission_eligible);
                row.policy_hold_nano = Some(resolved.policy_hold_nano);
                row.comparison_result = resolved.comparison.as_str().into();
            }
            PricingShadowEvaluationOutcome::Rejected {
                reason,
                observed_multiplier_bp,
            } => {
                row.outcome = "rejected".into();
                row.reason_code = Some(reason.as_str().into());
                row.observed_multiplier_bp = Some(*observed_multiplier_bp);
            }
            PricingShadowEvaluationOutcome::ReadError { reason } => {
                row.outcome = "read_error".into();
                row.reason_code = Some(reason.as_str().into());
            }
        }
        Ok(row)
    }

    pub(crate) fn from_storage(
        actual_snapshot: &LegacyScalarAdmissionSnapshot,
        row: PricingShadowStorageRow,
    ) -> Result<Self> {
        let actual = ShadowActualSnapshotRef::from_snapshot(actual_snapshot)?;
        row.verify_actual_projection(&actual)?;
        let outcome = row.decode_outcome(&actual)?;
        let diagnostic_context = ShadowDiagnosticContext::from_json(&row.diagnostic_context)?;
        let evaluation = Self::new(
            actual,
            row.evaluator_schema_version,
            row.runtime_manifest_generation,
            row.runtime_manifest_digest,
            row.enqueued_ts,
            row.evaluated_ts,
            outcome,
            diagnostic_context,
        )?;
        if evaluation.evaluation_digest.0 != row.evaluation_digest {
            bail!("stored shadow evaluation failed canonical digest verification");
        }
        Ok(evaluation)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingShadowEvaluationConflict {
    ExistingSemanticResult,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PricingShadowEvaluationWrite {
    Inserted(Box<PricingShadowAdmissionEvaluation>),
    Unchanged(Box<PricingShadowAdmissionEvaluation>),
    Conflict(PricingShadowEvaluationConflict),
}

#[derive(Clone, Debug)]
pub(crate) struct PricingShadowStorageRow {
    pub request_id: String,
    pub account_id: String,
    pub actual_snapshot_kind: String,
    pub actual_snapshot_digest: String,
    pub provider_id: String,
    pub requested_model_id: String,
    pub canonical_model_id: String,
    pub alias_generation: i64,
    pub evaluator_schema_version: i64,
    pub runtime_manifest_generation: i64,
    pub runtime_manifest_digest: String,
    pub enqueued_ts: i64,
    pub evaluated_ts: i64,
    pub outcome: String,
    pub reason_code: Option<String>,
    pub authorized_multiplier_bp: i64,
    pub observed_multiplier_bp: Option<i64>,
    pub official_hold_nano: i64,
    pub legacy_hold_nano: i64,
    pub product_id: Option<String>,
    pub account_class: Option<String>,
    pub effective_policy_version: Option<i64>,
    pub policy_id: Option<String>,
    pub policy_version: Option<i64>,
    pub source_policy_digest: Option<String>,
    pub policy_digest: Option<String>,
    pub policy_schema_version: Option<i64>,
    pub policy_catalog_generation: Option<i64>,
    pub policy_catalog_schema_version: Option<i64>,
    pub policy_catalog_capability_generation: Option<i64>,
    pub policy_catalog_capability_digest: Option<String>,
    pub policy_catalog_digest: Option<String>,
    pub policy_switch_generation: Option<i64>,
    pub policy_switch_schema_version: Option<i64>,
    pub policy_switch_capability_generation: Option<i64>,
    pub policy_switch_capability_digest: Option<String>,
    pub policy_switch_digest: Option<String>,
    pub admission_catalog_generation: Option<i64>,
    pub admission_catalog_schema_version: Option<i64>,
    pub admission_catalog_capability_generation: Option<i64>,
    pub admission_catalog_capability_digest: Option<String>,
    pub admission_catalog_digest: Option<String>,
    pub admission_switch_generation: Option<i64>,
    pub admission_switch_schema_version: Option<i64>,
    pub admission_switch_capability_generation: Option<i64>,
    pub admission_switch_capability_digest: Option<String>,
    pub admission_switch_digest: Option<String>,
    pub rule_id: Option<String>,
    pub rule_digest: Option<String>,
    pub rule_scope: Option<String>,
    pub pricing_mode: Option<String>,
    pub rule_origin: Option<String>,
    pub discount_bps: Option<i64>,
    pub payable_multiplier_bp: Option<i64>,
    pub track_eligible: Option<bool>,
    pub retention_eligible: Option<bool>,
    pub commission_eligible: Option<bool>,
    pub policy_hold_nano: Option<i64>,
    pub comparison_result: String,
    pub diagnostic_context: String,
    pub evaluation_digest: String,
}

impl PricingShadowStorageRow {
    fn verify_actual_projection(&self, actual: &ShadowActualSnapshotRef) -> Result<()> {
        if self.request_id != actual.request_id
            || self.account_id != actual.account_id
            || self.actual_snapshot_kind != "legacy_scalar"
            || self.actual_snapshot_digest != actual.actual_snapshot_digest
            || self.provider_id != actual.provider.as_str()
            || self.requested_model_id != actual.requested_model_id
            || self.canonical_model_id != actual.canonical_model_id
            || self.alias_generation != actual.alias_generation
            || self.authorized_multiplier_bp != actual.authorized_multiplier_bp
            || self.official_hold_nano != actual.official_hold_nano
            || self.legacy_hold_nano != actual.legacy_hold_nano
        {
            bail!("stored shadow evaluation does not exactly reference its actual snapshot");
        }
        Ok(())
    }

    fn decode_outcome(
        &self,
        actual: &ShadowActualSnapshotRef,
    ) -> Result<PricingShadowEvaluationOutcome> {
        match self.outcome.as_str() {
            "resolved" => {
                if self.reason_code.is_some() {
                    bail!("stored resolved shadow evaluation has a reason code");
                }
                let observed_multiplier_bp =
                    required(self.observed_multiplier_bp, "observed multiplier")?;
                let policy = PricingShadowPolicyIdentity {
                    target: VersionTarget::new(
                        required(self.effective_policy_version, "effective policy version")?,
                        required_ref(&self.policy_digest, "policy digest")?.clone(),
                    ),
                    policy_id: required_ref(&self.policy_id, "policy id")?.clone(),
                    policy_version: required(self.policy_version, "policy version")?,
                    source_policy_digest: required_ref(
                        &self.source_policy_digest,
                        "source policy digest",
                    )?
                    .clone(),
                    schema_version: required(self.policy_schema_version, "policy schema version")?,
                };
                let scope_type = required_ref(&self.rule_scope, "rule scope")?;
                let scope = PolicyRuleScope::from_db(
                    scope_type,
                    self.provider_id.clone(),
                    (scope_type == "model").then(|| self.canonical_model_id.clone()),
                )?;
                let rule = AccountPolicyRuleSpec {
                    rule_id: required_ref(&self.rule_id, "rule id")?.clone(),
                    rule_digest: required_ref(&self.rule_digest, "rule digest")?.clone(),
                    scope,
                    pricing_mode: PricingMode::from_db(required_ref(
                        &self.pricing_mode,
                        "pricing mode",
                    )?)?,
                    rule_origin: RuleOrigin::from_db(required_ref(
                        &self.rule_origin,
                        "rule origin",
                    )?)?,
                    discount_bps: self.discount_bps,
                    payable_multiplier_bp: required(
                        self.payable_multiplier_bp,
                        "payable multiplier",
                    )?,
                    track_eligible: required(self.track_eligible, "track eligibility")?,
                    retention_eligible: required(self.retention_eligible, "retention eligibility")?,
                    commission_eligible: required(
                        self.commission_eligible,
                        "commission eligibility",
                    )?,
                };
                let resolved = PricingShadowResolved {
                    observed_multiplier_bp,
                    product_id: required_ref(&self.product_id, "product id")?.clone(),
                    account_class: AccountClass::from_db(required_ref(
                        &self.account_class,
                        "account class",
                    )?)?,
                    policy,
                    policy_lineage: PricingShadowLineage {
                        catalog: dependency_from_row(self, DependencySlot::PolicyCatalog)?,
                        switches: dependency_from_row(self, DependencySlot::PolicySwitch)?,
                    },
                    admission_lineage: PricingShadowLineage {
                        catalog: dependency_from_row(self, DependencySlot::AdmissionCatalog)?,
                        switches: dependency_from_row(self, DependencySlot::AdmissionSwitch)?,
                    },
                    rule,
                    policy_hold_nano: required(self.policy_hold_nano, "policy hold")?,
                    comparison: PricingShadowComparison::from_db(&self.comparison_result)?,
                };
                resolved.validate(actual, self.evaluator_schema_version, None)?;
                Ok(PricingShadowEvaluationOutcome::Resolved(Box::new(resolved)))
            }
            "rejected" => {
                self.require_failure_shape()?;
                let reason =
                    PricingShadowRejectionCode::from_db(self.reason_code.as_deref().ok_or_else(
                        || anyhow!("stored shadow rejection is missing its reason"),
                    )?)?;
                Ok(PricingShadowEvaluationOutcome::Rejected {
                    reason,
                    observed_multiplier_bp: self.observed_multiplier_bp.ok_or_else(|| {
                        anyhow!("stored shadow rejection is missing observed multiplier")
                    })?,
                })
            }
            "read_error" => {
                self.require_failure_shape()?;
                if self.observed_multiplier_bp.is_some() {
                    bail!("stored shadow read error has an observed multiplier");
                }
                let reason =
                    PricingShadowReadErrorCode::from_db(self.reason_code.as_deref().ok_or_else(
                        || anyhow!("stored shadow read error is missing its reason"),
                    )?)?;
                Ok(PricingShadowEvaluationOutcome::ReadError { reason })
            }
            _ => bail!("stored shadow evaluation has an unknown outcome"),
        }
    }

    fn require_failure_shape(&self) -> Result<()> {
        if self.comparison_result != "not_comparable"
            || self.product_id.is_some()
            || self.account_class.is_some()
            || self.effective_policy_version.is_some()
            || self.policy_id.is_some()
            || self.policy_version.is_some()
            || self.source_policy_digest.is_some()
            || self.policy_digest.is_some()
            || self.policy_schema_version.is_some()
            || self.policy_catalog_generation.is_some()
            || self.policy_catalog_schema_version.is_some()
            || self.policy_catalog_capability_generation.is_some()
            || self.policy_catalog_capability_digest.is_some()
            || self.policy_catalog_digest.is_some()
            || self.policy_switch_generation.is_some()
            || self.policy_switch_schema_version.is_some()
            || self.policy_switch_capability_generation.is_some()
            || self.policy_switch_capability_digest.is_some()
            || self.policy_switch_digest.is_some()
            || self.admission_catalog_generation.is_some()
            || self.admission_catalog_schema_version.is_some()
            || self.admission_catalog_capability_generation.is_some()
            || self.admission_catalog_capability_digest.is_some()
            || self.admission_catalog_digest.is_some()
            || self.admission_switch_generation.is_some()
            || self.admission_switch_schema_version.is_some()
            || self.admission_switch_capability_generation.is_some()
            || self.admission_switch_capability_digest.is_some()
            || self.admission_switch_digest.is_some()
            || self.rule_id.is_some()
            || self.rule_digest.is_some()
            || self.rule_scope.is_some()
            || self.pricing_mode.is_some()
            || self.rule_origin.is_some()
            || self.discount_bps.is_some()
            || self.payable_multiplier_bp.is_some()
            || self.track_eligible.is_some()
            || self.retention_eligible.is_some()
            || self.commission_eligible.is_some()
            || self.policy_hold_nano.is_some()
        {
            bail!("stored failed shadow evaluation contains resolved evidence");
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum DependencySlot {
    PolicyCatalog,
    PolicySwitch,
    AdmissionCatalog,
    AdmissionSwitch,
}

fn copy_dependency_to_row(
    row: &mut PricingShadowStorageRow,
    slot: DependencySlot,
    dependency: &PricingShadowDependency,
) {
    let values = (
        Some(dependency.target.version),
        Some(dependency.pricing_schema_version),
        Some(dependency.capability_generation),
        Some(dependency.capability_digest.clone()),
        Some(dependency.target.content_digest.clone()),
    );
    match slot {
        DependencySlot::PolicyCatalog => {
            (
                row.policy_catalog_generation,
                row.policy_catalog_schema_version,
                row.policy_catalog_capability_generation,
                row.policy_catalog_capability_digest,
                row.policy_catalog_digest,
            ) = values;
        }
        DependencySlot::PolicySwitch => {
            (
                row.policy_switch_generation,
                row.policy_switch_schema_version,
                row.policy_switch_capability_generation,
                row.policy_switch_capability_digest,
                row.policy_switch_digest,
            ) = values;
        }
        DependencySlot::AdmissionCatalog => {
            (
                row.admission_catalog_generation,
                row.admission_catalog_schema_version,
                row.admission_catalog_capability_generation,
                row.admission_catalog_capability_digest,
                row.admission_catalog_digest,
            ) = values;
        }
        DependencySlot::AdmissionSwitch => {
            (
                row.admission_switch_generation,
                row.admission_switch_schema_version,
                row.admission_switch_capability_generation,
                row.admission_switch_capability_digest,
                row.admission_switch_digest,
            ) = values;
        }
    }
}

fn dependency_from_row(
    row: &PricingShadowStorageRow,
    slot: DependencySlot,
) -> Result<PricingShadowDependency> {
    let (generation, schema, capability_generation, capability_digest, digest, label) = match slot {
        DependencySlot::PolicyCatalog => (
            row.policy_catalog_generation,
            row.policy_catalog_schema_version,
            row.policy_catalog_capability_generation,
            &row.policy_catalog_capability_digest,
            &row.policy_catalog_digest,
            "policy catalog",
        ),
        DependencySlot::PolicySwitch => (
            row.policy_switch_generation,
            row.policy_switch_schema_version,
            row.policy_switch_capability_generation,
            &row.policy_switch_capability_digest,
            &row.policy_switch_digest,
            "policy switches",
        ),
        DependencySlot::AdmissionCatalog => (
            row.admission_catalog_generation,
            row.admission_catalog_schema_version,
            row.admission_catalog_capability_generation,
            &row.admission_catalog_capability_digest,
            &row.admission_catalog_digest,
            "admission catalog",
        ),
        DependencySlot::AdmissionSwitch => (
            row.admission_switch_generation,
            row.admission_switch_schema_version,
            row.admission_switch_capability_generation,
            &row.admission_switch_capability_digest,
            &row.admission_switch_digest,
            "admission switches",
        ),
    };
    Ok(PricingShadowDependency {
        target: VersionTarget::new(
            required(generation, &format!("{label} generation"))?,
            required_ref(digest, &format!("{label} digest"))?.clone(),
        ),
        pricing_schema_version: required(schema, &format!("{label} schema"))?,
        capability_generation: required(
            capability_generation,
            &format!("{label} capability generation"),
        )?,
        capability_digest: required_ref(capability_digest, &format!("{label} capability digest"))?
            .clone(),
    })
}

fn validate_lineage(
    label: &str,
    lineage: &PricingShadowLineage,
    evaluator_schema_version: i64,
    require_same_capability: bool,
) -> Result<()> {
    lineage.catalog.validate(&format!("{label} catalog"))?;
    lineage.switches.validate(&format!("{label} switches"))?;
    if lineage.catalog.pricing_schema_version != evaluator_schema_version
        || lineage.switches.pricing_schema_version != evaluator_schema_version
    {
        bail!("{label} lineage schema does not match evaluator schema");
    }
    if require_same_capability
        && (lineage.catalog.capability_generation != lineage.switches.capability_generation
            || lineage.catalog.capability_digest != lineage.switches.capability_digest)
    {
        bail!("policy catalog and switch capability pins do not match");
    }
    Ok(())
}

fn validate_rule(actual: &ShadowActualSnapshotRef, rule: &AccountPolicyRuleSpec) -> Result<()> {
    require_shadow_id("shadow rule id", &rule.rule_id)?;
    require_shadow_id("shadow rule digest", &rule.rule_digest)?;
    require_shadow_id("shadow rule provider id", rule.scope.provider_id())?;
    if rule.scope.provider_id() != actual.provider.as_str() {
        bail!("shadow rule provider does not match the actual fixed provider");
    }
    if let Some(canonical_model_id) = rule.scope.canonical_model_id() {
        require_shadow_id("shadow rule canonical model id", canonical_model_id)?;
        if canonical_model_id != actual.canonical_model_id {
            bail!("shadow model rule does not match the actual canonical model");
        }
    }
    match (rule.pricing_mode, rule.rule_origin) {
        (PricingMode::Track, RuleOrigin::Managed) => {
            if rule.discount_bps.is_some()
                || !(0..=10_000).contains(&rule.payable_multiplier_bp)
                || !rule.track_eligible
                || !rule.retention_eligible
            {
                bail!("invalid managed track shadow rule");
            }
        }
        (PricingMode::Discount, RuleOrigin::Managed) => {
            let Some(discount_bps) = rule.discount_bps else {
                bail!("managed shadow discount requires discount basis points");
            };
            if !(0..=9_500).contains(&discount_bps)
                || discount_bps % 100 != 0
                || rule.payable_multiplier_bp != 10_000 - discount_bps
                || rule.track_eligible
                || rule.retention_eligible
                || rule.commission_eligible
            {
                bail!("invalid managed discount shadow rule");
            }
        }
        (PricingMode::Discount, RuleOrigin::Legacy) => {
            if rule.discount_bps.is_some()
                || !(1..=10_000).contains(&rule.payable_multiplier_bp)
                || rule.track_eligible
                || rule.retention_eligible
                || rule.commission_eligible
            {
                bail!("invalid legacy discount shadow rule");
            }
        }
        (PricingMode::Track, RuleOrigin::Legacy) => {
            bail!("legacy track shadow rules are not supported")
        }
    }
    if rule.commission_eligible && rule.pricing_mode != PricingMode::Track {
        bail!("shadow commission eligibility requires track mode");
    }
    Ok(())
}

fn apply_multiplier_nano(amount: i64, multiplier_bp: i64) -> Result<i64> {
    if amount < 0 || !(0..=10_000).contains(&multiplier_bp) {
        bail!("invalid amount or multiplier for shadow hold calculation");
    }
    let multiplied = (amount as i128)
        .checked_mul(multiplier_bp as i128)
        .and_then(|value| value.checked_add(5_000))
        .context("shadow hold calculation overflow")?
        / 10_000;
    i64::try_from(multiplied).context("shadow hold does not fit integer nanodollars")
}

/// Resolve the policy hold against the same immutable funding ceiling that constrained the actual
/// legacy reserve. When the stored actual is below the checked scalar quote, the admission balance
/// was exactly that stored hold: both scalar and policy candidates therefore share
/// `min(candidate, legacy_hold_nano)`. An uncapped actual leaves the policy candidate unchanged.
fn policy_hold_nano(actual: &ShadowActualSnapshotRef, multiplier_bp: i64) -> Result<i64> {
    let policy_uncapped = apply_multiplier_nano(actual.official_hold_nano, multiplier_bp)?;
    let scalar_uncapped =
        apply_multiplier_nano(actual.official_hold_nano, actual.authorized_multiplier_bp)?;
    Ok(if actual.legacy_hold_nano < scalar_uncapped {
        policy_uncapped.min(actual.legacy_hold_nano)
    } else {
        policy_uncapped
    })
}

fn require_shadow_id(label: &str, value: &str) -> Result<()> {
    require_id(label, value)?;
    if value.as_bytes().contains(&0) {
        bail!("{label} must not contain a NUL byte");
    }
    if value.len() > SHADOW_ID_MAX_BYTES {
        bail!("{label} exceeds the byte limit");
    }
    Ok(())
}

fn validate_digest(label: &str, value: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:v1:") else {
        bail!("{label} has an unsupported canonical format");
    };
    if hex.len() != 64
        || !hex
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        bail!("{label} has an invalid canonical SHA-256 value");
    }
    Ok(())
}

fn required<T>(value: Option<T>, label: &str) -> Result<T> {
    value.ok_or_else(|| anyhow!("stored resolved shadow evaluation is missing {label}"))
}

fn required_ref<'a, T>(value: &'a Option<T>, label: &str) -> Result<&'a T> {
    value
        .as_ref()
        .ok_or_else(|| anyhow!("stored resolved shadow evaluation is missing {label}"))
}

fn feed_dependency(
    encoder: &mut CanonicalDigestEncoder,
    base: u16,
    dependency: &PricingShadowDependency,
) {
    encoder.i64(base, dependency.target.version);
    encoder.string(base + 1, &dependency.target.content_digest);
    encoder.i64(base + 2, dependency.pricing_schema_version);
    encoder.i64(base + 3, dependency.capability_generation);
    encoder.string(base + 4, &dependency.capability_digest);
}

struct CanonicalDigestEncoder(Sha256);

impl CanonicalDigestEncoder {
    fn new(domain: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(domain);
        Self(hasher)
    }

    fn field(&mut self, tag: u16, payload: &[u8]) {
        self.0.update(tag.to_be_bytes());
        self.0.update((payload.len() as u64).to_be_bytes());
        self.0.update(payload);
    }

    fn string(&mut self, tag: u16, value: &str) {
        self.field(tag, value.as_bytes());
    }

    fn i64(&mut self, tag: u16, value: i64) {
        self.field(tag, &value.to_be_bytes());
    }

    fn bool(&mut self, tag: u16, value: bool) {
        self.field(tag, &[u8::from(value)]);
    }

    fn finish(self) -> String {
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
    use crate::pricing::{
        LegacyPremiumModifiers, LegacyScalarAdmissionSnapshotInput, SnapshotAnthropicInferenceGeo,
        SnapshotAnthropicSpeed,
    };
    use serde_json::json;

    fn snapshot() -> LegacyScalarAdmissionSnapshot {
        LegacyScalarAdmissionSnapshot::new(LegacyScalarAdmissionSnapshotInput {
            request_id: "shadow-request-1".into(),
            account_id: "shadow-account-1".into(),
            provider: SnapshotProvider::Anthropic,
            requested_model_id: "claude-sonnet-5".into(),
            canonical_model_id: "claude-sonnet-5".into(),
            alias_generation: 7,
            tariff_schedule_id: "anthropic/standard/sonnet-5/v1".into(),
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
        })
        .unwrap()
    }

    fn capability(generation: i64, digest: &str) -> PricingRuntimeCapabilityEvidence {
        PricingRuntimeCapabilityEvidence::new(PRICING_SCHEMA_VERSION, generation, digest).unwrap()
    }

    fn manifest() -> PricingRuntimeManifestEvidence {
        PricingRuntimeManifestEvidence::new(
            9,
            vec![capability(2, "capability-b"), capability(1, "capability-a")],
        )
        .unwrap()
    }

    fn dependency(
        version: i64,
        capability_generation: i64,
        prefix: &str,
    ) -> PricingShadowDependency {
        PricingShadowDependency {
            target: VersionTarget::new(version, format!("{prefix}-digest")),
            pricing_schema_version: PRICING_SCHEMA_VERSION,
            capability_generation,
            capability_digest: if capability_generation == 1 {
                "capability-a".into()
            } else {
                "capability-b".into()
            },
        }
    }

    fn resolved(actual: &ShadowActualSnapshotRef) -> PricingShadowEvaluationOutcome {
        let resolved = PricingShadowResolved::new(
            actual,
            PricingShadowResolvedInput {
                observed_multiplier_bp: 2_000,
                product_id: "main".into(),
                account_class: AccountClass::B2c,
                policy: PricingShadowPolicyIdentity {
                    target: VersionTarget::new(3, "policy-digest"),
                    policy_id: "policy-main".into(),
                    policy_version: 4,
                    source_policy_digest: "source-policy-digest".into(),
                    schema_version: PRICING_SCHEMA_VERSION,
                },
                policy_lineage: PricingShadowLineage {
                    catalog: dependency(1, 1, "policy-catalog"),
                    switches: dependency(1, 1, "policy-switch"),
                },
                admission_lineage: PricingShadowLineage {
                    catalog: dependency(2, 2, "admission-catalog"),
                    switches: dependency(2, 2, "admission-switch"),
                },
                rule: AccountPolicyRuleSpec {
                    rule_id: "rule-anthropic".into(),
                    rule_digest: "rule-digest".into(),
                    scope: PolicyRuleScope::Provider {
                        provider_id: "anthropic".into(),
                    },
                    pricing_mode: PricingMode::Discount,
                    rule_origin: RuleOrigin::Managed,
                    discount_bps: Some(8_000),
                    payable_multiplier_bp: 2_000,
                    track_eligible: false,
                    retention_eligible: false,
                    commission_eligible: false,
                },
            },
        )
        .unwrap();
        PricingShadowEvaluationOutcome::Resolved(Box::new(resolved))
    }

    fn evaluation(outcome: PricingShadowEvaluationOutcome) -> PricingShadowAdmissionEvaluation {
        let actual = ShadowActualSnapshotRef::from_snapshot(&snapshot()).unwrap();
        PricingShadowAdmissionEvaluationInput::new(
            actual,
            PRICING_SCHEMA_VERSION,
            manifest(),
            1_788_220_801,
            1_788_220_802,
            outcome,
            ShadowDiagnosticContext::new(json!({"attempt": 1})).unwrap(),
        )
        .unwrap()
        .to_evaluation()
        .unwrap()
    }

    #[test]
    fn runtime_manifest_is_canonical_and_rejects_ambiguous_members() {
        let forward = manifest();
        let reverse = PricingRuntimeManifestEvidence::new(
            9,
            vec![capability(1, "capability-a"), capability(2, "capability-b")],
        )
        .unwrap();
        assert_eq!(forward, reverse);
        assert_eq!(
            forward.manifest_digest(),
            "sha256:v1:9f58922d1589e72ad25f424952267368aa8a86b52b48c90fcedf05c5a7b4ac1c"
        );
        assert!(PricingRuntimeManifestEvidence::new(
            9,
            vec![
                capability(1, "capability-a"),
                capability(1, "capability-other")
            ],
        )
        .is_err());
    }

    #[test]
    fn evaluation_digests_have_stable_variant_vectors() {
        let actual = ShadowActualSnapshotRef::from_snapshot(&snapshot()).unwrap();
        let resolved = evaluation(resolved(&actual));
        let rejected = evaluation(PricingShadowEvaluationOutcome::Rejected {
            reason: PricingShadowRejectionCode::MissingRule,
            observed_multiplier_bp: 2_000,
        });
        let read_error = evaluation(PricingShadowEvaluationOutcome::ReadError {
            reason: PricingShadowReadErrorCode::PricingReadFailed,
        });
        assert_eq!(
            resolved.evaluation_digest().as_str(),
            "sha256:v1:7079474371840cc08a2b106a1b975335626ee8b5658b5892da49ff0963125dd5"
        );
        assert_eq!(
            rejected.evaluation_digest().as_str(),
            "sha256:v1:eddf6cccd245cfdd0de555850dcc766b49db1f6f695eb52863cda383c05d1399"
        );
        assert_eq!(
            read_error.evaluation_digest().as_str(),
            "sha256:v1:3ac2bd5bb02bf0b49a51144851525ef2ba0b55d73ab3dc98c3b8e101c11a09ee"
        );
    }

    #[test]
    fn timestamps_and_diagnostics_are_not_semantic_identity() {
        let outcome = PricingShadowEvaluationOutcome::Rejected {
            reason: PricingShadowRejectionCode::MissingRule,
            observed_multiplier_bp: 2_000,
        };
        let first = evaluation(outcome.clone());
        let actual = ShadowActualSnapshotRef::from_snapshot(&snapshot()).unwrap();
        let second = PricingShadowAdmissionEvaluationInput::new(
            actual,
            PRICING_SCHEMA_VERSION,
            manifest(),
            1_788_220_810,
            1_788_220_820,
            outcome,
            ShadowDiagnosticContext::new(json!({"different": true})).unwrap(),
        )
        .unwrap()
        .to_evaluation()
        .unwrap();
        assert_eq!(first.evaluation_digest, second.evaluation_digest);
        assert!(first.same_semantics(&second));
        assert_ne!(first.enqueued_ts, second.enqueued_ts);
        assert_ne!(first.diagnostic_context, second.diagnostic_context);
    }

    #[test]
    fn funding_cap_is_applied_exactly_to_scalar_and_policy_candidates() {
        let mut input = LegacyScalarAdmissionSnapshotInput {
            request_id: "shadow-capped-request".into(),
            account_id: "shadow-account-1".into(),
            provider: SnapshotProvider::Anthropic,
            requested_model_id: "claude-sonnet-5".into(),
            canonical_model_id: "claude-sonnet-5".into(),
            alias_generation: 7,
            tariff_schedule_id: "anthropic/standard/sonnet-5/v1".into(),
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
        };
        input.charged_hold_nano = 90_000_000;
        let capped = LegacyScalarAdmissionSnapshot::new(input).unwrap();
        let actual = ShadowActualSnapshotRef::from_snapshot(&capped).unwrap();

        assert!(actual.validate_shadow_eligibility(1_788_220_801).is_ok());
        assert_eq!(policy_hold_nano(&actual, 1_000).unwrap(), 50_000_000);
        assert_eq!(policy_hold_nano(&actual, 2_000).unwrap(), 90_000_000);
        assert_eq!(policy_hold_nano(&actual, 3_000).unwrap(), 90_000_000);

        let PricingShadowEvaluationOutcome::Resolved(resolved) = resolved(&actual) else {
            panic!("expected resolved comparison")
        };
        assert_eq!(resolved.policy_hold_nano(), 90_000_000);
        assert_eq!(resolved.comparison(), PricingShadowComparison::Equal);
    }

    #[test]
    fn evaluation_digest_covers_every_semantic_group() {
        let actual = ShadowActualSnapshotRef::from_snapshot(&snapshot()).unwrap();
        let base = evaluation(resolved(&actual));
        let base_digest = base.compute_digest();

        macro_rules! changes_digest {
            ($label:literal, $mutation:expr) => {{
                let mut changed = base.clone();
                ($mutation)(&mut changed);
                assert_ne!(base_digest, changed.compute_digest(), $label);
            }};
        }

        changes_digest!("request", |value: &mut PricingShadowAdmissionEvaluation| {
            value.actual.request_id.push_str("-changed")
        });
        changes_digest!("account", |value: &mut PricingShadowAdmissionEvaluation| {
            value.actual.account_id.push_str("-changed")
        });
        changes_digest!(
            "provider",
            |value: &mut PricingShadowAdmissionEvaluation| {
                value.actual.provider = SnapshotProvider::OpenAi
            }
        );
        changes_digest!(
            "requested model",
            |value: &mut PricingShadowAdmissionEvaluation| {
                value.actual.requested_model_id.push_str("-changed")
            }
        );
        changes_digest!(
            "canonical model",
            |value: &mut PricingShadowAdmissionEvaluation| {
                value.actual.canonical_model_id.push_str("-changed")
            }
        );
        changes_digest!(
            "alias generation",
            |value: &mut PricingShadowAdmissionEvaluation| { value.actual.alias_generation += 1 }
        );
        changes_digest!(
            "actual digest",
            |value: &mut PricingShadowAdmissionEvaluation| {
                value.actual.actual_snapshot_digest.push('a')
            }
        );
        changes_digest!(
            "evaluator schema",
            |value: &mut PricingShadowAdmissionEvaluation| { value.evaluator_schema_version += 1 }
        );
        changes_digest!(
            "manifest generation",
            |value: &mut PricingShadowAdmissionEvaluation| {
                value.runtime_manifest_generation += 1
            }
        );
        changes_digest!(
            "manifest digest",
            |value: &mut PricingShadowAdmissionEvaluation| {
                value.runtime_manifest_digest.push('a')
            }
        );
        changes_digest!(
            "authorized multiplier",
            |value: &mut PricingShadowAdmissionEvaluation| {
                value.actual.authorized_multiplier_bp += 1
            }
        );
        changes_digest!(
            "official hold",
            |value: &mut PricingShadowAdmissionEvaluation| { value.actual.official_hold_nano += 1 }
        );
        changes_digest!(
            "legacy hold",
            |value: &mut PricingShadowAdmissionEvaluation| { value.actual.legacy_hold_nano += 1 }
        );

        macro_rules! mutate_resolved {
            ($label:literal, $mutation:expr) => {
                changes_digest!($label, |value: &mut PricingShadowAdmissionEvaluation| {
                    let PricingShadowEvaluationOutcome::Resolved(resolved) = &mut value.outcome
                    else {
                        unreachable!()
                    };
                    ($mutation)(resolved.as_mut());
                });
            };
        }

        mutate_resolved!(
            "observed multiplier",
            |value: &mut PricingShadowResolved| { value.observed_multiplier_bp += 1 }
        );
        mutate_resolved!("product", |value: &mut PricingShadowResolved| {
            value.product_id.push_str("-changed")
        });
        mutate_resolved!("account class", |value: &mut PricingShadowResolved| {
            value.account_class = AccountClass::B2b
        });
        mutate_resolved!(
            "policy effective version",
            |value: &mut PricingShadowResolved| { value.policy.target.version += 1 }
        );
        mutate_resolved!("policy digest", |value: &mut PricingShadowResolved| {
            value.policy.target.content_digest.push_str("-changed")
        });
        mutate_resolved!("policy id", |value: &mut PricingShadowResolved| {
            value.policy.policy_id.push_str("-changed")
        });
        mutate_resolved!("policy version", |value: &mut PricingShadowResolved| {
            value.policy.policy_version += 1
        });
        mutate_resolved!(
            "source policy digest",
            |value: &mut PricingShadowResolved| {
                value.policy.source_policy_digest.push_str("-changed")
            }
        );
        mutate_resolved!("policy schema", |value: &mut PricingShadowResolved| {
            value.policy.schema_version += 1
        });

        for slot in [
            DependencySlot::PolicyCatalog,
            DependencySlot::PolicySwitch,
            DependencySlot::AdmissionCatalog,
            DependencySlot::AdmissionSwitch,
        ] {
            for field in 0..5 {
                let mut changed = base.clone();
                let PricingShadowEvaluationOutcome::Resolved(resolved) = &mut changed.outcome
                else {
                    unreachable!()
                };
                let dependency = match slot {
                    DependencySlot::PolicyCatalog => &mut resolved.policy_lineage.catalog,
                    DependencySlot::PolicySwitch => &mut resolved.policy_lineage.switches,
                    DependencySlot::AdmissionCatalog => &mut resolved.admission_lineage.catalog,
                    DependencySlot::AdmissionSwitch => &mut resolved.admission_lineage.switches,
                };
                match field {
                    0 => dependency.target.version += 1,
                    1 => dependency.target.content_digest.push_str("-changed"),
                    2 => dependency.pricing_schema_version += 1,
                    3 => dependency.capability_generation += 1,
                    4 => dependency.capability_digest.push_str("-changed"),
                    _ => unreachable!(),
                }
                assert_ne!(base_digest, changed.compute_digest(), "lineage dependency");
            }
        }

        mutate_resolved!("rule id", |value: &mut PricingShadowResolved| {
            value.rule.rule_id.push_str("-changed")
        });
        mutate_resolved!("rule digest", |value: &mut PricingShadowResolved| {
            value.rule.rule_digest.push_str("-changed")
        });
        mutate_resolved!("rule scope", |value: &mut PricingShadowResolved| {
            value.rule.scope = PolicyRuleScope::Model {
                provider_id: "anthropic".into(),
                canonical_model_id: "claude-sonnet-5".into(),
            }
        });
        mutate_resolved!("pricing mode", |value: &mut PricingShadowResolved| {
            value.rule.pricing_mode = PricingMode::Track
        });
        mutate_resolved!("rule origin", |value: &mut PricingShadowResolved| {
            value.rule.rule_origin = RuleOrigin::Legacy
        });
        mutate_resolved!("discount", |value: &mut PricingShadowResolved| {
            value.rule.discount_bps = None
        });
        mutate_resolved!("payable multiplier", |value: &mut PricingShadowResolved| {
            value.rule.payable_multiplier_bp += 1
        });
        mutate_resolved!("track eligibility", |value: &mut PricingShadowResolved| {
            value.rule.track_eligible = true
        });
        mutate_resolved!(
            "retention eligibility",
            |value: &mut PricingShadowResolved| { value.rule.retention_eligible = true }
        );
        mutate_resolved!(
            "commission eligibility",
            |value: &mut PricingShadowResolved| { value.rule.commission_eligible = true }
        );
        mutate_resolved!("policy hold", |value: &mut PricingShadowResolved| {
            value.policy_hold_nano += 1
        });
        mutate_resolved!("comparison", |value: &mut PricingShadowResolved| {
            value.comparison = PricingShadowComparison::Different
        });

        changes_digest!(
            "outcome and reason",
            |value: &mut PricingShadowAdmissionEvaluation| {
                value.outcome = PricingShadowEvaluationOutcome::Rejected {
                    reason: PricingShadowRejectionCode::MissingRule,
                    observed_multiplier_bp: 2_000,
                }
            }
        );
    }

    #[test]
    fn missing_manifest_member_and_overstated_actual_fail_closed() {
        let snapshot = snapshot();
        let actual = ShadowActualSnapshotRef::from_snapshot(&snapshot).unwrap();
        let only_policy_capability =
            PricingRuntimeManifestEvidence::new(9, vec![capability(1, "capability-a")]).unwrap();
        assert!(PricingShadowAdmissionEvaluationInput::new(
            actual.clone(),
            PRICING_SCHEMA_VERSION,
            only_policy_capability,
            1_788_220_801,
            1_788_220_802,
            resolved(&actual),
            ShadowDiagnosticContext::empty(),
        )
        .is_err());

        let mut overstated_input = LegacyScalarAdmissionSnapshotInput {
            request_id: "capped".into(),
            account_id: "shadow-account-1".into(),
            provider: SnapshotProvider::Anthropic,
            requested_model_id: "claude-sonnet-5".into(),
            canonical_model_id: "claude-sonnet-5".into(),
            alias_generation: 7,
            tariff_schedule_id: "anthropic/standard/sonnet-5/v1".into(),
            tariff_priced_ts: 1_788_220_799,
            admission_ts: 1_788_220_800,
            payable_multiplier_bp: 2_000,
            official_hold_nano: 500_000_000,
            charged_hold_nano: 100_000_001,
            premium_modifiers: LegacyPremiumModifiers::AnthropicV1 {
                speed: SnapshotAnthropicSpeed::Standard,
                inference_geo: SnapshotAnthropicInferenceGeo::Global,
                inference_geo_basis_points: 10_000,
            },
        };
        let overstated = LegacyScalarAdmissionSnapshot::new(overstated_input.clone()).unwrap();
        let overstated_actual = ShadowActualSnapshotRef::from_snapshot(&overstated).unwrap();
        assert!(PricingShadowAdmissionEvaluationInput::new(
            overstated_actual,
            PRICING_SCHEMA_VERSION,
            manifest(),
            1_788_220_801,
            1_788_220_802,
            PricingShadowEvaluationOutcome::ReadError {
                reason: PricingShadowReadErrorCode::InvalidActualSnapshot,
            },
            ShadowDiagnosticContext::empty(),
        )
        .is_err());

        overstated_input.charged_hold_nano = 100_000_000;
        assert!(LegacyScalarAdmissionSnapshot::new(overstated_input).is_ok());

        assert!(ShadowDiagnosticContext::new(json!({"bad": "contains\u{0}nul"})).is_err());
        let mut nul_key = serde_json::Map::new();
        nul_key.insert("bad\u{0}key".into(), Value::Bool(true));
        assert!(ShadowDiagnosticContext::new(Value::Object(nul_key)).is_err());

        let mut too_deep = Value::Null;
        for _ in 0..=SHADOW_DIAGNOSTIC_MAX_DEPTH {
            too_deep = Value::Array(vec![too_deep]);
        }
        assert!(ShadowDiagnosticContext::new(json!({"deep": too_deep})).is_err());
        assert!(ShadowDiagnosticContext::new(json!({
            "items": vec![false; SHADOW_DIAGNOSTIC_MAX_ITEMS]
        }))
        .is_err());
    }
}
