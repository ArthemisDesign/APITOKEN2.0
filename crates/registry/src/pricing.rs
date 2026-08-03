//! Typed, backend-neutral persistence contract for versioned multi-provider pricing.
//!
//! Stage 3A stores immutable catalog, switch and account-policy versions and changes their explicit
//! heads with compare-and-set. Stage 3B adds one transactionally coherent, read-only bundle for a
//! pure resolver in `forward`, including both the immutable dependencies pinned by the active
//! policy and the independently moving admission heads. Nothing here participates in live request
//! admission, charging, key issuance, HTTP, policy resolution, or production shadow execution.

mod policy;
pub(crate) mod postgres;
mod release_v2;
mod shadow;
mod snapshots;
mod sqlite;

pub(crate) use policy::PolicySnapshotLookup;
pub use policy::{
    PolicyAdmissionSnapshot, PolicyAdmissionSnapshotInput, PolicyReserveConflict,
    PolicyReserveOutcome, PolicyReserveReceipt, POLICY_ADMISSION_SNAPSHOT_SCHEMA_VERSION,
};
pub(crate) use release_v2::build_pricing_request_snapshot_v2;
pub use release_v2::{
    validate_pricing_release_activation_v2, validate_pricing_release_assignment_extension_v2,
    validate_pricing_release_policy_v2, validate_pricing_release_recovery_link_v2,
    validate_pricing_release_v2, BillingModeV2, LegacyPricingPathClosedV2,
    PricingReleaseActivationEvidenceV2, PricingReleaseActivationKindV2,
    PricingReleaseActivationOutcomeV2, PricingReleaseActivationReceiptV2,
    PricingReleaseActivationRejectionV2, PricingReleaseActivationRequestV2,
    PricingReleaseAssignmentExtensionMemberV2, PricingReleaseAssignmentExtensionV2,
    PricingReleaseAssignmentV2, PricingReleaseHeadExpectationV2, PricingReleaseHeadV2,
    PricingReleaseInventoryAccountV2, PricingReleaseInventoryPageV2, PricingReleaseKindV2,
    PricingReleasePolicyRuleV2, PricingReleasePolicyV2, PricingReleaseProvisioningActivationV2,
    PricingReleaseProvisioningContextV2, PricingReleaseProvisioningRecoveryV2,
    PricingReleaseProvisioningReleaseV2, PricingReleaseQuoteV2, PricingReleaseRecoveryLinkV2,
    PricingReleaseReserveConflictV2, PricingReleaseReserveOutcomeV2,
    PricingReleaseReserveReceiptV2, PricingReleaseResolutionV2, PricingReleaseRuleScopeV2,
    PricingReleaseV2, PricingRequestSnapshotV2, FUNDING_SCHEMA_VERSION_V2,
    PRICING_RELEASE_RUNTIME_DIGEST_V2, PRICING_RELEASE_SCHEMA_VERSION,
};
pub(crate) use shadow::PricingShadowStorageRow;
pub use shadow::{
    PricingRuntimeCapabilityEvidence, PricingRuntimeManifestEvidence,
    PricingShadowAdmissionEvaluation, PricingShadowAdmissionEvaluationInput,
    PricingShadowComparison, PricingShadowDependency, PricingShadowEvaluationConflict,
    PricingShadowEvaluationOutcome, PricingShadowEvaluationWrite, PricingShadowLineage,
    PricingShadowPolicyIdentity, PricingShadowReadErrorCode, PricingShadowRejectionCode,
    PricingShadowResolved, PricingShadowResolvedInput, ShadowActualSnapshotRef,
    ShadowDiagnosticContext, ShadowEligibilityError, ShadowEvaluationDigestV1,
};
pub(crate) use snapshots::{
    validate_legacy_snapshot_request_id, validate_request_lifecycle_prune_cutoff,
    LegacyScalarSnapshotLookup,
};
pub use snapshots::{
    CanonicalDigestV1, LegacyPremiumModifiers, LegacyScalarAdmissionSnapshot,
    LegacyScalarAdmissionSnapshotInput, LegacyScalarIdempotencyWindowError,
    LegacyScalarReserveConflict, LegacyScalarReserveOutcome, LegacyScalarReserveReceipt,
    SnapshotAnthropicInferenceGeo, SnapshotAnthropicSpeed, SnapshotGeminiContextRate,
    SnapshotGeminiSearchBilling, SnapshotOpenAiContextTier, SnapshotOpenAiServiceTier,
    SnapshotProvider, LEGACY_SCALAR_REPLAY_MAX_AGE_SECS, LEGACY_SCALAR_SNAPSHOT_SCHEMA_VERSION,
    PRICING_REQUEST_LIFECYCLE_MIN_RETENTION_SECS,
};
pub(crate) use sqlite::{
    sqlite_insert_legacy_scalar_admission_snapshot, sqlite_insert_policy_admission_snapshot,
    sqlite_legacy_scalar_snapshot_lookup, sqlite_policy_snapshot_lookup,
};

pub use sqlite::{
    sqlite_account_policy_by_version, sqlite_activate_account_policy,
    sqlite_activate_pricing_catalog, sqlite_activate_provider_switches,
    sqlite_active_account_policy, sqlite_active_pricing_catalog, sqlite_active_provider_switches,
    sqlite_insert_pricing_shadow_admission_evaluation, sqlite_legacy_scalar_admission_snapshot,
    sqlite_locked_openkeys_policy_transition, sqlite_policy_admission_snapshot,
    sqlite_prepare_account_policy, sqlite_prepare_pricing_catalog,
    sqlite_prepare_provider_switches, sqlite_pricing_catalog_by_generation,
    sqlite_pricing_read_bundle, sqlite_pricing_shadow_admission_evaluation,
    sqlite_provider_switches_by_generation,
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PRICING_SCHEMA_VERSION: i64 = 1;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VersionTarget {
    pub version: i64,
    pub content_digest: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveExpectation {
    Absent,
    Exact(VersionTarget),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivePolicyTarget {
    pub target: VersionTarget,
    pub binding: AccountPolicyBindingSpec,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyActiveExpectation {
    Unbound,
    Inactive(AccountPolicyBindingSpec),
    Exact(ActivePolicyTarget),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyBindingState {
    Unbound,
    Inactive(AccountPolicyBindingSpec),
    Active(ActivePolicyTarget),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingMutation {
    Stored,
    Applied,
    Unchanged,
    Rejected(PricingRejection),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingRejection {
    Invalid { reason: String },
    MissingDependency { dependency: String },
    Stale { actual: Option<VersionTarget> },
    VersionConflict,
    CasMismatch { actual: Option<VersionTarget> },
    PolicyCasMismatch { actual: PolicyBindingState },
    Locked,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingCatalogSpec {
    pub product_id: String,
    pub generation: i64,
    pub schema_version: i64,
    pub capability_generation: i64,
    pub capability_digest: String,
    pub content_digest: String,
    pub entries: Vec<PricingCatalogEntrySpec>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingCatalogEntrySpec {
    pub provider_id: String,
    pub canonical_model_id: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderSwitchSpec {
    pub generation: i64,
    pub schema_version: i64,
    pub capability_generation: i64,
    pub capability_digest: String,
    pub content_digest: String,
    pub entries: Vec<ProviderSwitchEntrySpec>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderSwitchEntrySpec {
    pub provider_id: String,
    pub scope: ProviderSwitchScope,
    pub catalog_generation: Option<i64>,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSwitchScope {
    Master,
    Product {
        product_id: String,
    },
    Segment {
        product_id: String,
        segment: PolicySegment,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicySegment {
    B2c,
    B2b,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountPolicySpec {
    pub account_id: String,
    pub effective_version: i64,
    pub policy_id: String,
    pub policy_version: i64,
    pub source_policy_digest: String,
    pub owner_type: PolicyOwnerType,
    pub owner_id: String,
    pub account_class: AccountClass,
    pub product_id: String,
    pub schema_version: i64,
    pub catalog_generation: i64,
    pub switch_generation: i64,
    pub content_digest: String,
    pub replacement_locked: bool,
    pub rules: Vec<AccountPolicyRuleSpec>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountPolicyRuleSpec {
    pub rule_id: String,
    pub rule_digest: String,
    pub scope: PolicyRuleScope,
    pub pricing_mode: PricingMode,
    pub rule_origin: RuleOrigin,
    pub discount_bps: Option<i64>,
    pub payable_multiplier_bp: i64,
    pub track_eligible: bool,
    pub retention_eligible: bool,
    pub commission_eligible: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyRuleScope {
    Provider {
        provider_id: String,
    },
    Model {
        provider_id: String,
        canonical_model_id: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountPolicyBindingSpec {
    pub policy_enforcement: PolicyEnforcement,
    pub funding_enforcement: FundingEnforcement,
    pub reconciliation_state: ReconciliationState,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountPolicyActivationSpec {
    pub account_id: String,
    pub effective_version: i64,
    pub content_digest: String,
    pub binding: AccountPolicyBindingSpec,
}

/// One narrowly constrained atomic replacement of an immutable legacy OpenKeys policy.
///
/// Generic policy prepare/activate deliberately keep treating every replacement-locked lineage as
/// immutable. This request names the complete managed 1:1 successor and the exact active legacy
/// target/binding that must still be current when the successor is inserted and shadow-bound.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedOpenKeysPolicyTransitionSpec {
    pub policy: AccountPolicySpec,
    pub expected_active: ActivePolicyTarget,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveAccountPolicy {
    pub policy: AccountPolicySpec,
    pub binding: AccountPolicyBindingSpec,
}

/// Account policy state observed by one backend transaction.
///
/// This is deliberately a read-only Stage 3B building block. It does not resolve a rule or alter
/// admission, billing, heads, bindings, or any other durable state.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingPolicySnapshot {
    Unbound,
    Inactive {
        product_id: String,
        account_class: AccountClass,
        binding: AccountPolicyBindingSpec,
    },
    Active(ActiveAccountPolicy),
}

/// Live account scalar, current policy and both pricing lineages from one database snapshot.
///
/// `policy_catalog` and `policy_switches` are the exact immutable versions pinned by an active
/// policy. `admission_catalog` and `admission_switches` are the independently moving active heads
/// which constrain new admissions. Keeping both pairs prevents normal catalog -> switches ->
/// policy choreography from making the older active policy internally inconsistent. An unbound
/// account has no product context, so all dependency fields are `None`; an inactive binding has
/// no policy dependencies and uses its bound product to read whichever admission heads exist.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReadBundle {
    pub account_id: String,
    /// Live legacy scalar read in the same transaction as policy/catalog/switch state.
    pub account_multiplier_bp: i64,
    pub policy: PricingPolicySnapshot,
    pub policy_catalog: Option<PricingCatalogSpec>,
    pub policy_switches: Option<ProviderSwitchSpec>,
    pub admission_catalog: Option<PricingCatalogSpec>,
    pub admission_switches: Option<ProviderSwitchSpec>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyOwnerType {
    GlobalB2c,
    B2bClient,
    OpenKeys,
    Service,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountClass {
    B2c,
    B2b,
    OpenKeys,
    Service,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyEnforcement {
    LegacyScalar,
    Shadow,
    Strict,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingEnforcement {
    LegacySingle,
    Shadow,
    Strict,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationState {
    Pending,
    Verified,
    Exception,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingMode {
    Track,
    Discount,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleOrigin {
    Managed,
    Legacy,
}

impl VersionTarget {
    pub fn new(version: i64, content_digest: impl Into<String>) -> Self {
        Self {
            version,
            content_digest: content_digest.into(),
        }
    }
}

impl PricingCatalogSpec {
    pub fn target(&self) -> VersionTarget {
        VersionTarget::new(self.generation, self.content_digest.clone())
    }
}

impl ProviderSwitchSpec {
    pub fn target(&self) -> VersionTarget {
        VersionTarget::new(self.generation, self.content_digest.clone())
    }
}

impl AccountPolicySpec {
    pub fn target(&self) -> VersionTarget {
        VersionTarget::new(self.effective_version, self.content_digest.clone())
    }
}

impl ActiveAccountPolicy {
    pub fn target(&self) -> ActivePolicyTarget {
        ActivePolicyTarget {
            target: self.policy.target(),
            binding: self.binding.clone(),
        }
    }
}

impl AccountPolicyActivationSpec {
    pub fn target(&self) -> ActivePolicyTarget {
        ActivePolicyTarget {
            target: VersionTarget::new(self.effective_version, self.content_digest.clone()),
            binding: self.binding.clone(),
        }
    }
}

impl LockedOpenKeysPolicyTransitionSpec {
    pub fn desired_active(&self) -> ActivePolicyTarget {
        ActivePolicyTarget {
            target: self.policy.target(),
            binding: locked_openkeys_transition_binding(),
        }
    }
}

impl PolicySegment {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::B2c => "b2c",
            Self::B2b => "b2b",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self> {
        match value {
            "b2c" => Ok(Self::B2c),
            "b2b" => Ok(Self::B2b),
            other => bail!("unknown policy segment {other:?}"),
        }
    }
}

impl ProviderSwitchScope {
    pub(crate) fn db_parts(&self) -> (&'static str, &str, &str) {
        match self {
            Self::Master => ("master", "", ""),
            Self::Product { product_id } => ("product", product_id, ""),
            Self::Segment {
                product_id,
                segment,
            } => ("segment", product_id, segment.as_str()),
        }
    }

    pub(crate) fn from_db(scope_type: &str, product_id: String, segment: String) -> Result<Self> {
        match scope_type {
            "master" if product_id.is_empty() && segment.is_empty() => Ok(Self::Master),
            "product" if !product_id.is_empty() && segment.is_empty() => {
                Ok(Self::Product { product_id })
            }
            "segment" if !product_id.is_empty() => Ok(Self::Segment {
                product_id,
                segment: PolicySegment::from_db(&segment)?,
            }),
            _ => bail!(
                "invalid provider switch scope type={scope_type:?} product={product_id:?} \
                 segment={segment:?}"
            ),
        }
    }

    pub(crate) fn sort_key(&self) -> (String, String, String) {
        let (scope_type, product_id, segment) = self.db_parts();
        (
            scope_type.to_owned(),
            product_id.to_owned(),
            segment.to_owned(),
        )
    }
}

impl PolicyRuleScope {
    pub fn provider_id(&self) -> &str {
        match self {
            Self::Provider { provider_id } | Self::Model { provider_id, .. } => provider_id,
        }
    }

    pub fn canonical_model_id(&self) -> Option<&str> {
        match self {
            Self::Provider { .. } => None,
            Self::Model {
                canonical_model_id, ..
            } => Some(canonical_model_id),
        }
    }

    pub(crate) fn db_parts(&self) -> (&'static str, &str, Option<&str>) {
        match self {
            Self::Provider { provider_id } => ("provider", provider_id, None),
            Self::Model {
                provider_id,
                canonical_model_id,
            } => ("model", provider_id, Some(canonical_model_id)),
        }
    }

    pub(crate) fn from_db(
        scope_type: &str,
        provider_id: String,
        canonical_model_id: Option<String>,
    ) -> Result<Self> {
        match (scope_type, canonical_model_id) {
            ("provider", None) => Ok(Self::Provider { provider_id }),
            ("model", Some(canonical_model_id)) if !canonical_model_id.is_empty() => {
                Ok(Self::Model {
                    provider_id,
                    canonical_model_id,
                })
            }
            _ => bail!("invalid stored account policy rule scope"),
        }
    }

    pub(crate) fn sort_key(&self) -> (String, String, String) {
        let (scope_type, provider_id, canonical_model_id) = self.db_parts();
        (
            provider_id.to_owned(),
            scope_type.to_owned(),
            canonical_model_id.unwrap_or("").to_owned(),
        )
    }
}

macro_rules! string_enum {
    ($type:ty, $( $variant:path => $value:literal ),+ $(,)?) => {
        impl $type {
            pub fn as_str(self) -> &'static str {
                match self {
                    $( $variant => $value, )+
                }
            }

            pub(crate) fn from_db(value: &str) -> Result<Self> {
                match value {
                    $( $value => Ok($variant), )+
                    other => bail!("unknown {} value {other:?}", stringify!($type)),
                }
            }
        }
    };
}

string_enum!(
    PolicyOwnerType,
    PolicyOwnerType::GlobalB2c => "global_b2c",
    PolicyOwnerType::B2bClient => "b2b_client",
    PolicyOwnerType::OpenKeys => "openkeys",
    PolicyOwnerType::Service => "service",
);
string_enum!(
    AccountClass,
    AccountClass::B2c => "b2c",
    AccountClass::B2b => "b2b",
    AccountClass::OpenKeys => "openkeys",
    AccountClass::Service => "service",
);
string_enum!(
    PolicyEnforcement,
    PolicyEnforcement::LegacyScalar => "legacy_scalar",
    PolicyEnforcement::Shadow => "shadow",
    PolicyEnforcement::Strict => "strict",
);
string_enum!(
    FundingEnforcement,
    FundingEnforcement::LegacySingle => "legacy_single",
    FundingEnforcement::Shadow => "shadow",
    FundingEnforcement::Strict => "strict",
);
string_enum!(
    ReconciliationState,
    ReconciliationState::Pending => "pending",
    ReconciliationState::Verified => "verified",
    ReconciliationState::Exception => "exception",
);
string_enum!(
    PricingMode,
    PricingMode::Track => "track",
    PricingMode::Discount => "discount",
);
string_enum!(
    RuleOrigin,
    RuleOrigin::Managed => "managed",
    RuleOrigin::Legacy => "legacy",
);

pub(crate) fn require_id(label: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.trim() != value {
        bail!("{label} must be non-empty and contain no surrounding whitespace");
    }
    Ok(())
}

pub(crate) fn normalize_catalog(spec: &PricingCatalogSpec) -> PricingCatalogSpec {
    let mut normalized = spec.clone();
    normalized.entries.sort_by(|left, right| {
        (&left.provider_id, &left.canonical_model_id)
            .cmp(&(&right.provider_id, &right.canonical_model_id))
    });
    normalized
}

pub(crate) fn normalize_switches(spec: &ProviderSwitchSpec) -> ProviderSwitchSpec {
    let mut normalized = spec.clone();
    normalized.entries.sort_by(|left, right| {
        (&left.provider_id, left.scope.sort_key())
            .cmp(&(&right.provider_id, right.scope.sort_key()))
    });
    normalized
}

pub(crate) fn normalize_policy(spec: &AccountPolicySpec) -> AccountPolicySpec {
    let mut normalized = spec.clone();
    normalized.rules.sort_by(|left, right| {
        (left.scope.sort_key(), &left.rule_id).cmp(&(right.scope.sort_key(), &right.rule_id))
    });
    normalized
}

pub fn validate_version_target(target: &VersionTarget) -> Result<()> {
    if target.version <= 0 {
        bail!("target version must be positive");
    }
    require_id("target content digest", &target.content_digest)
}

pub fn validate_active_expectation(expectation: &ActiveExpectation) -> Result<()> {
    if let ActiveExpectation::Exact(target) = expectation {
        validate_version_target(target)?;
    }
    Ok(())
}

pub fn validate_policy_active_expectation(expectation: &PolicyActiveExpectation) -> Result<()> {
    match expectation {
        PolicyActiveExpectation::Unbound => {}
        PolicyActiveExpectation::Inactive(binding) => {
            validate_account_policy_binding(binding)?;
        }
        PolicyActiveExpectation::Exact(active) => {
            validate_version_target(&active.target)?;
            validate_account_policy_binding(&active.binding)?;
        }
    }
    Ok(())
}

pub fn validate_pricing_catalog(spec: &PricingCatalogSpec) -> Result<()> {
    require_id("product id", &spec.product_id)?;
    require_id("capability digest", &spec.capability_digest)?;
    require_id("catalog content digest", &spec.content_digest)?;
    if spec.generation <= 0 {
        bail!("catalog generation must be positive");
    }
    if spec.capability_generation <= 0 {
        bail!("catalog capability generation must be positive");
    }
    if spec.schema_version != PRICING_SCHEMA_VERSION {
        bail!(
            "unsupported pricing catalog schema version {}",
            spec.schema_version
        );
    }

    let mut identities = BTreeSet::new();
    for entry in &spec.entries {
        require_id("catalog provider id", &entry.provider_id)?;
        require_id("catalog canonical model id", &entry.canonical_model_id)?;
        if !identities.insert((&entry.provider_id, &entry.canonical_model_id)) {
            bail!(
                "duplicate catalog entry for provider {:?} model {:?}",
                entry.provider_id,
                entry.canonical_model_id
            );
        }
    }
    Ok(())
}

pub fn validate_provider_switches(spec: &ProviderSwitchSpec) -> Result<()> {
    require_id("provider switch capability digest", &spec.capability_digest)?;
    require_id("provider switch content digest", &spec.content_digest)?;
    if spec.generation <= 0 {
        bail!("provider switch generation must be positive");
    }
    if spec.capability_generation <= 0 {
        bail!("provider switch capability generation must be positive");
    }
    if spec.schema_version != PRICING_SCHEMA_VERSION {
        bail!(
            "unsupported provider switch schema version {}",
            spec.schema_version
        );
    }

    let mut identities = BTreeSet::new();
    let mut masters = BTreeSet::new();
    let mut scoped_providers = BTreeSet::new();
    let mut product_generations = BTreeMap::new();
    for entry in &spec.entries {
        require_id("provider switch provider id", &entry.provider_id)?;
        match &entry.scope {
            ProviderSwitchScope::Master => {
                if entry.catalog_generation.is_some() {
                    bail!("master provider switch must not pin a product catalog");
                }
                masters.insert(entry.provider_id.as_str());
            }
            ProviderSwitchScope::Product { product_id }
            | ProviderSwitchScope::Segment { product_id, .. } => {
                require_id("provider switch product id", product_id)?;
                let Some(generation) = entry.catalog_generation else {
                    bail!("product and segment provider switches require a catalog generation");
                };
                if generation <= 0 {
                    bail!("product and segment provider switches require a catalog generation");
                }
                if product_generations
                    .insert(product_id.as_str(), generation)
                    .is_some_and(|existing| existing != generation)
                {
                    bail!("one switch generation cannot pin two catalogs for the same product");
                }
                scoped_providers.insert(entry.provider_id.as_str());
            }
        }
        let (scope_type, product_id, segment) = entry.scope.db_parts();
        if !identities.insert((&entry.provider_id, scope_type, product_id, segment)) {
            bail!("duplicate provider switch entry");
        }
    }
    if let Some(provider) = scoped_providers
        .into_iter()
        .find(|provider| !masters.contains(provider))
    {
        bail!("provider {provider:?} has scoped switches without a master switch");
    }
    Ok(())
}

pub fn validate_account_policy_binding(binding: &AccountPolicyBindingSpec) -> Result<()> {
    let policy_strict = binding.policy_enforcement == PolicyEnforcement::Strict;
    let funding_strict = binding.funding_enforcement == FundingEnforcement::Strict;
    if policy_strict || funding_strict {
        if !policy_strict
            || !funding_strict
            || binding.reconciliation_state != ReconciliationState::Verified
        {
            bail!("strict policy and funding enforcement require one verified atomic binding");
        }
    }
    Ok(())
}

pub fn validate_account_policy_activation(spec: &AccountPolicyActivationSpec) -> Result<()> {
    require_id("account id", &spec.account_id)?;
    validate_version_target(&VersionTarget::new(
        spec.effective_version,
        spec.content_digest.clone(),
    ))?;
    validate_account_policy_binding(&spec.binding)
}

/// The only binding this transition may produce. It cannot change live pricing or funding before
/// the global release-head cutover: the successor is evaluated in shadow while legacy funding
/// remains authoritative.
pub fn locked_openkeys_transition_binding() -> AccountPolicyBindingSpec {
    AccountPolicyBindingSpec {
        policy_enforcement: PolicyEnforcement::Shadow,
        funding_enforcement: FundingEnforcement::LegacySingle,
        reconciliation_state: ReconciliationState::Verified,
    }
}

pub fn validate_locked_openkeys_policy_transition(
    transition: &LockedOpenKeysPolicyTransitionSpec,
) -> Result<()> {
    validate_account_policy_shape(&transition.policy)?;
    validate_version_target(&transition.expected_active.target)?;
    validate_account_policy_binding(&transition.expected_active.binding)?;

    let expected_effective_version = transition
        .expected_active
        .target
        .version
        .checked_add(1)
        .context("locked OpenKeys effective version overflow")?;
    if transition.policy.effective_version != expected_effective_version {
        bail!("locked OpenKeys successor effective version must advance exactly once");
    }
    if transition.policy.owner_type != PolicyOwnerType::OpenKeys
        || transition.policy.account_class != AccountClass::OpenKeys
        || transition.policy.product_id != "openkeys"
    {
        bail!("locked OpenKeys successor must preserve the OpenKeys owner and product class");
    }
    if transition.policy.replacement_locked {
        bail!("locked OpenKeys successor must not retain the replacement lock");
    }
    if transition.policy.schema_version != PRICING_SCHEMA_VERSION {
        bail!("locked OpenKeys successor must use the current pricing schema");
    }
    if transition.policy.rules.is_empty() {
        bail!("locked OpenKeys successor must contain provider rules");
    }
    for rule in &transition.policy.rules {
        if !matches!(&rule.scope, PolicyRuleScope::Provider { .. })
            || rule.pricing_mode != PricingMode::Discount
            || rule.rule_origin != RuleOrigin::Managed
            || rule.discount_bps != Some(0)
            || rule.payable_multiplier_bp != 10_000
            || rule.track_eligible
            || rule.retention_eligible
            || rule.commission_eligible
        {
            bail!("locked OpenKeys successor permits only managed zero-discount provider rules");
        }
    }
    Ok(())
}

pub(crate) fn validate_locked_openkeys_successor_identity(
    transition: &LockedOpenKeysPolicyTransitionSpec,
    locked: &AccountPolicySpec,
) -> Result<()> {
    validate_locked_openkeys_policy_transition(transition)?;
    if locked.target() != transition.expected_active.target
        || !locked.replacement_locked
        || locked.owner_type != PolicyOwnerType::OpenKeys
        || locked.account_class != AccountClass::OpenKeys
        || locked.product_id != "openkeys"
    {
        bail!("expected active policy is not the exact replacement-locked legacy OpenKeys source");
    }
    let expected_policy_version = locked
        .policy_version
        .checked_add(1)
        .context("locked OpenKeys policy version overflow")?;
    if transition.policy.policy_version != expected_policy_version {
        bail!("locked OpenKeys successor policy version must advance exactly once");
    }
    if transition.policy.account_id != locked.account_id
        || transition.policy.policy_id != locked.policy_id
        || transition.policy.owner_type != locked.owner_type
        || transition.policy.owner_id != locked.owner_id
        || transition.policy.account_class != locked.account_class
        || transition.policy.product_id != locked.product_id
    {
        bail!("locked OpenKeys successor changed immutable policy identity");
    }
    Ok(())
}

fn expected_account_class(owner_type: PolicyOwnerType) -> AccountClass {
    match owner_type {
        PolicyOwnerType::GlobalB2c => AccountClass::B2c,
        PolicyOwnerType::B2bClient => AccountClass::B2b,
        PolicyOwnerType::OpenKeys => AccountClass::OpenKeys,
        PolicyOwnerType::Service => AccountClass::Service,
    }
}

fn required_switch_scope(account_class: AccountClass, product_id: &str) -> ProviderSwitchScope {
    match account_class {
        AccountClass::B2c => ProviderSwitchScope::Segment {
            product_id: product_id.to_owned(),
            segment: PolicySegment::B2c,
        },
        AccountClass::B2b => ProviderSwitchScope::Segment {
            product_id: product_id.to_owned(),
            segment: PolicySegment::B2b,
        },
        AccountClass::OpenKeys | AccountClass::Service => ProviderSwitchScope::Product {
            product_id: product_id.to_owned(),
        },
    }
}

pub fn validate_account_policy_shape(spec: &AccountPolicySpec) -> Result<()> {
    require_id("account id", &spec.account_id)?;
    require_id("policy id", &spec.policy_id)?;
    require_id("source policy digest", &spec.source_policy_digest)?;
    require_id("policy owner id", &spec.owner_id)?;
    require_id("policy product id", &spec.product_id)?;
    require_id("policy content digest", &spec.content_digest)?;
    if spec.effective_version <= 0 || spec.policy_version <= 0 {
        bail!("policy and effective versions must be positive");
    }
    if spec.schema_version != PRICING_SCHEMA_VERSION {
        bail!(
            "unsupported account policy schema version {}",
            spec.schema_version
        );
    }
    if spec.catalog_generation <= 0 || spec.switch_generation <= 0 {
        bail!("policy dependency generations must be positive");
    }
    if spec.account_class != expected_account_class(spec.owner_type) {
        bail!("policy owner type and immutable account class do not match");
    }
    match spec.owner_type {
        PolicyOwnerType::GlobalB2c | PolicyOwnerType::B2bClient if spec.product_id != "main" => {
            bail!("B2C and B2B policies must use the main product");
        }
        PolicyOwnerType::OpenKeys if spec.product_id != "openkeys" => {
            bail!("OpenKeys policy must use the openkeys product");
        }
        _ => {}
    }
    if spec.owner_type == PolicyOwnerType::B2bClient && spec.rules.is_empty() {
        bail!("B2B account policy must contain at least one rule");
    }
    if spec.owner_type == PolicyOwnerType::OpenKeys && spec.rules.is_empty() {
        bail!("OpenKeys account policy must contain at least one provider rule");
    }
    Ok(())
}

pub fn validate_account_policy(
    spec: &AccountPolicySpec,
    catalog: &PricingCatalogSpec,
    switches: &ProviderSwitchSpec,
    legacy_multiplier_bp: Option<i64>,
) -> Result<()> {
    validate_pricing_catalog(catalog)?;
    validate_provider_switches(switches)?;
    validate_account_policy_shape(spec)?;
    if spec.product_id != catalog.product_id || spec.catalog_generation != catalog.generation {
        bail!("account policy does not target the supplied catalog generation");
    }
    if spec.switch_generation != switches.generation {
        bail!("account policy does not target the supplied switch generation");
    }
    if catalog.capability_generation != switches.capability_generation
        || catalog.capability_digest != switches.capability_digest
    {
        bail!("policy catalog and switch capability pins do not match");
    }
    let catalog_models: BTreeSet<(&str, &str)> = catalog
        .entries
        .iter()
        .map(|entry| {
            (
                entry.provider_id.as_str(),
                entry.canonical_model_id.as_str(),
            )
        })
        .collect();
    let catalog_providers: BTreeSet<&str> = catalog_models
        .iter()
        .map(|(provider, _)| *provider)
        .collect();
    let enabled_catalog_providers: BTreeSet<&str> = catalog
        .entries
        .iter()
        .filter(|entry| entry.enabled)
        .map(|entry| entry.provider_id.as_str())
        .collect();
    let master_providers: BTreeSet<&str> = switches
        .entries
        .iter()
        .filter_map(|entry| {
            matches!(entry.scope, ProviderSwitchScope::Master).then_some(entry.provider_id.as_str())
        })
        .collect();
    let required_scope = required_switch_scope(spec.account_class, &spec.product_id);
    let scoped_providers: BTreeSet<&str> = switches
        .entries
        .iter()
        .filter_map(|entry| {
            (entry.scope == required_scope
                && entry.catalog_generation == Some(spec.catalog_generation))
            .then_some(entry.provider_id.as_str())
        })
        .collect();

    let mut rule_ids = BTreeSet::new();
    let mut scopes = BTreeSet::new();
    let mut origins = BTreeSet::new();
    let mut legacy_multipliers = BTreeSet::new();
    let mut rule_providers = BTreeSet::new();

    for rule in &spec.rules {
        require_id("policy rule id", &rule.rule_id)?;
        require_id("policy rule digest", &rule.rule_digest)?;
        if !rule_ids.insert(rule.rule_id.as_str()) {
            bail!("duplicate policy rule id {:?}", rule.rule_id);
        }
        if !scopes.insert(rule.scope.sort_key()) {
            bail!("duplicate policy rule scope");
        }
        origins.insert(rule.rule_origin);

        let provider_id = rule.scope.provider_id();
        rule_providers.insert(provider_id);
        require_id("policy rule provider id", provider_id)?;
        if !master_providers.contains(provider_id) || !scoped_providers.contains(provider_id) {
            bail!("policy rule provider {provider_id:?} lacks the required pinned switch scope");
        }
        match &rule.scope {
            PolicyRuleScope::Provider { provider_id } => {
                if !catalog_providers.contains(provider_id.as_str()) {
                    bail!("provider rule references a provider absent from the product catalog");
                }
            }
            PolicyRuleScope::Model {
                provider_id,
                canonical_model_id,
            } => {
                require_id("policy rule canonical model id", canonical_model_id)?;
                if !catalog_models.contains(&(provider_id.as_str(), canonical_model_id.as_str())) {
                    bail!("model rule references a model absent from the product catalog");
                }
                if spec.owner_type == PolicyOwnerType::OpenKeys {
                    bail!("OpenKeys pricing cannot differ by model");
                }
            }
        }

        match (rule.pricing_mode, rule.rule_origin) {
            (PricingMode::Track, RuleOrigin::Managed) => {
                if spec.owner_type != PolicyOwnerType::GlobalB2c
                    || rule.discount_bps.is_some()
                    || !(0..=10_000).contains(&rule.payable_multiplier_bp)
                    || !rule.track_eligible
                    || !rule.retention_eligible
                {
                    bail!("invalid track policy rule");
                }
            }
            (PricingMode::Discount, RuleOrigin::Managed) => {
                let Some(discount_bps) = rule.discount_bps else {
                    bail!("managed discount rule requires discount basis points");
                };
                if !(0..=9_500).contains(&discount_bps)
                    || discount_bps % 100 != 0
                    || rule.payable_multiplier_bp != 10_000 - discount_bps
                    || rule.track_eligible
                    || rule.retention_eligible
                    || rule.commission_eligible
                {
                    bail!("invalid managed discount policy rule");
                }
                if spec.owner_type == PolicyOwnerType::OpenKeys && discount_bps != 0 {
                    bail!("current OpenKeys policy must use a zero-percent discount");
                }
            }
            (PricingMode::Discount, RuleOrigin::Legacy) => {
                if spec.owner_type != PolicyOwnerType::OpenKeys
                    || rule.discount_bps.is_some()
                    || !(1..=10_000).contains(&rule.payable_multiplier_bp)
                    || rule.track_eligible
                    || rule.retention_eligible
                    || rule.commission_eligible
                {
                    bail!("invalid immutable legacy OpenKeys rule");
                }
                legacy_multipliers.insert(rule.payable_multiplier_bp);
            }
            (PricingMode::Track, RuleOrigin::Legacy) => {
                bail!("legacy track rules are not supported");
            }
        }
        if rule.commission_eligible && rule.pricing_mode != PricingMode::Track {
            bail!("commission eligibility requires track mode");
        }
    }

    let has_legacy = origins.contains(&RuleOrigin::Legacy);
    if spec.owner_type == PolicyOwnerType::OpenKeys {
        if let Some(missing_provider) = enabled_catalog_providers
            .iter()
            .find(|provider| !rule_providers.contains(*provider))
        {
            bail!(
                "OpenKeys policy is missing enabled catalog provider {:?}",
                missing_provider
            );
        }
    }
    if has_legacy && origins.len() != 1 {
        bail!("managed and legacy policy rules cannot be mixed");
    }
    if has_legacy {
        if !spec.replacement_locked {
            bail!("legacy OpenKeys policy must be replacement-locked");
        }
        if legacy_multipliers.len() != 1 {
            bail!("legacy OpenKeys rules must use one exact multiplier");
        }
        let expected = legacy_multiplier_bp
            .context("legacy OpenKeys validation requires the live account multiplier")?;
        if legacy_multipliers.first().copied() != Some(expected) {
            bail!("legacy OpenKeys multiplier does not match the live account multiplier");
        }
    } else if spec.replacement_locked {
        bail!("replacement lock is reserved for immutable legacy OpenKeys policies");
    }
    Ok(())
}

pub(crate) fn invalid(error: anyhow::Error) -> PricingMutation {
    PricingMutation::Rejected(PricingRejection::Invalid {
        reason: format!("{error:#}"),
    })
}

pub(crate) fn missing(dependency: impl Into<String>) -> PricingMutation {
    PricingMutation::Rejected(PricingRejection::MissingDependency {
        dependency: dependency.into(),
    })
}

pub(crate) fn expectation_matches(
    expectation: &ActiveExpectation,
    actual: Option<&VersionTarget>,
) -> bool {
    match (expectation, actual) {
        (ActiveExpectation::Absent, None) => true,
        (ActiveExpectation::Exact(expected), Some(actual)) => expected == actual,
        _ => false,
    }
}

pub(crate) fn policy_expectation_matches(
    expectation: &PolicyActiveExpectation,
    actual: &PolicyBindingState,
) -> bool {
    match (expectation, actual) {
        (PolicyActiveExpectation::Unbound, PolicyBindingState::Unbound) => true,
        (PolicyActiveExpectation::Inactive(expected), PolicyBindingState::Inactive(actual)) => {
            expected == actual
        }
        (PolicyActiveExpectation::Exact(expected), PolicyBindingState::Active(actual)) => {
            expected == actual
        }
        _ => false,
    }
}

pub(crate) fn required_catalog_generations(switches: &ProviderSwitchSpec) -> BTreeMap<String, i64> {
    let mut products = BTreeMap::new();
    for entry in &switches.entries {
        let (product_id, generation) = match (&entry.scope, entry.catalog_generation) {
            (
                ProviderSwitchScope::Product { product_id }
                | ProviderSwitchScope::Segment { product_id, .. },
                Some(generation),
            ) => (product_id, generation),
            _ => continue,
        };
        products.insert(product_id.clone(), generation);
    }
    products
}
