//! Dormant, side-effect-free pricing-policy resolver.
//!
//! This module deliberately has no runtime caller yet. It consumes one transactionally read
//! registry bundle plus identities fixed by the provider runtime, and either returns one exact
//! rule or a typed fail-closed reason. It does not read a database, inspect HTTP input, calculate
//! token costs, reserve money, emit telemetry, or change admission.

use registry::pricing::{
    validate_account_policy, validate_account_policy_binding, validate_account_policy_shape,
    validate_pricing_catalog, validate_provider_switches, AccountClass, AccountPolicyBindingSpec,
    AccountPolicyRuleSpec, PolicyRuleScope, PricingCatalogSpec, PricingPolicySnapshot,
    PricingReadBundle, PricingRuntimeManifestEvidence, ProviderSwitchScope, ProviderSwitchSpec,
    VersionTarget, PRICING_SCHEMA_VERSION,
};
use std::collections::BTreeMap;

mod bridge;
mod shadow;

pub use bridge::{
    PricingBridgeConfig, PricingBridgeConfigError, PricingBridgeDecision,
    PricingBridgeFallbackReason,
};
pub(crate) use bridge::{
    snapshot_identity_is_oversized, EnginePricingRequestId, PricingBridgePrepare,
};
pub use shadow::{
    build_pricing_shadow_evaluation, PricingShadowEvaluationSource, PricingShadowReadFailure,
    PricingShadowWorkItem,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuntimePricingCapability {
    pub pricing_schema_version: i64,
    pub capability_generation: i64,
    pub capability_digest: String,
}

/// Runtime-owned set of pricing contracts understood by this evaluator binary.
///
/// This identity is intentionally separate from request data. A future live caller must construct
/// it from trusted runtime configuration, never from client-controlled HTTP input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePricingManifest {
    pub manifest_generation: i64,
    pub manifest_digest: String,
    pub capabilities: Vec<RuntimePricingCapability>,
}

impl RuntimePricingManifest {
    /// Derive the resolver view from registry-owned canonical manifest evidence.
    ///
    /// A production-facing builder must use this path instead of accepting a second caller-owned
    /// manifest identity. The evidence has already canonicalized, sorted and bounded its members.
    pub fn from_evidence(evidence: &PricingRuntimeManifestEvidence) -> Self {
        Self {
            manifest_generation: evidence.manifest_generation(),
            manifest_digest: evidence.manifest_digest().to_owned(),
            capabilities: evidence
                .capabilities()
                .iter()
                .map(|capability| RuntimePricingCapability {
                    pricing_schema_version: capability.pricing_schema_version(),
                    capability_generation: capability.capability_generation(),
                    capability_digest: capability.capability_digest().to_owned(),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingResolutionRequest {
    pub account_id: String,
    /// Fixed by the API/runtime plane. Never derive this value from a client header or body.
    pub provider_id: String,
    /// Public model identity supplied by the client; retained for later snapshot attribution only.
    pub requested_model_id: String,
    /// Exact canonical model identity resolved by the provider adapter before policy resolution.
    pub canonical_model_id: String,
}

/// Exact immutable identity of one catalog or switch dependency used by a resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPricingDependency {
    pub target: VersionTarget,
    pub pricing_schema_version: i64,
    pub capability_generation: i64,
    pub capability_digest: String,
}

/// The catalog/switch pair observed for one side of the dual-lineage decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPricingLineage {
    pub catalog: ResolvedPricingDependency,
    pub switches: ResolvedPricingDependency,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPricingRule {
    pub product_id: String,
    pub account_class: AccountClass,
    pub account_multiplier_bp: i64,
    pub provider_id: String,
    pub requested_model_id: String,
    pub canonical_model_id: String,
    pub evaluator_schema_version: i64,
    pub runtime_manifest_generation: i64,
    pub runtime_manifest_digest: String,
    pub policy_schema_version: i64,
    pub policy_lineage: ResolvedPricingLineage,
    pub admission_lineage: ResolvedPricingLineage,
    pub policy_target: VersionTarget,
    pub policy_id: String,
    pub policy_version: i64,
    pub source_policy_digest: String,
    pub binding: AccountPolicyBindingSpec,
    pub rule: AccountPolicyRuleSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingResolutionLineage {
    Policy,
    Admission,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingDependencyKind {
    Catalog,
    Switches,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingResolutionRejection {
    InvalidRequest,
    InvalidRuntimeManifest,
    AccountMismatch,
    NoPolicyBinding,
    InactivePolicy,
    MissingDependency {
        lineage: PricingResolutionLineage,
        dependency: PricingDependencyKind,
    },
    PolicySchemaMismatch,
    SchemaMismatch {
        lineage: PricingResolutionLineage,
        dependency: PricingDependencyKind,
    },
    CatalogTargetMismatch {
        lineage: PricingResolutionLineage,
    },
    PolicySwitchTargetMismatch,
    CapabilityNotInManifest {
        lineage: PricingResolutionLineage,
        dependency: PricingDependencyKind,
    },
    InvalidDependency {
        lineage: PricingResolutionLineage,
        dependency: PricingDependencyKind,
    },
    InvalidPolicyContract,
    ModelNotInCatalog {
        lineage: PricingResolutionLineage,
    },
    ModelDisabled {
        lineage: PricingResolutionLineage,
    },
    MissingMasterSwitch {
        lineage: PricingResolutionLineage,
    },
    MasterSwitchDisabled {
        lineage: PricingResolutionLineage,
    },
    MissingScopedSwitch {
        lineage: PricingResolutionLineage,
    },
    PolicyScopedSwitchTargetMismatch,
    AdmissionScopedSwitchTargetMismatch,
    ScopedSwitchDisabled {
        lineage: PricingResolutionLineage,
    },
    MissingRule,
}

impl PricingResolutionRejection {
    /// Stable, low-cardinality telemetry value. It intentionally contains no account/model IDs.
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidRuntimeManifest => "invalid_runtime_manifest",
            Self::AccountMismatch => "account_mismatch",
            Self::NoPolicyBinding => "no_policy_binding",
            Self::InactivePolicy => "inactive_policy",
            Self::MissingDependency {
                lineage: PricingResolutionLineage::Policy,
                dependency: PricingDependencyKind::Catalog,
            } => "missing_policy_catalog",
            Self::MissingDependency {
                lineage: PricingResolutionLineage::Policy,
                dependency: PricingDependencyKind::Switches,
            } => "missing_policy_switches",
            Self::MissingDependency {
                lineage: PricingResolutionLineage::Admission,
                dependency: PricingDependencyKind::Catalog,
            } => "missing_admission_catalog",
            Self::MissingDependency {
                lineage: PricingResolutionLineage::Admission,
                dependency: PricingDependencyKind::Switches,
            } => "missing_admission_switches",
            Self::PolicySchemaMismatch => "policy_schema_mismatch",
            Self::SchemaMismatch {
                lineage: PricingResolutionLineage::Policy,
                dependency: PricingDependencyKind::Catalog,
            } => "policy_catalog_schema_mismatch",
            Self::SchemaMismatch {
                lineage: PricingResolutionLineage::Policy,
                dependency: PricingDependencyKind::Switches,
            } => "policy_switch_schema_mismatch",
            Self::SchemaMismatch {
                lineage: PricingResolutionLineage::Admission,
                dependency: PricingDependencyKind::Catalog,
            } => "admission_catalog_schema_mismatch",
            Self::SchemaMismatch {
                lineage: PricingResolutionLineage::Admission,
                dependency: PricingDependencyKind::Switches,
            } => "admission_switch_schema_mismatch",
            Self::CatalogTargetMismatch {
                lineage: PricingResolutionLineage::Policy,
            } => "policy_catalog_target_mismatch",
            Self::CatalogTargetMismatch {
                lineage: PricingResolutionLineage::Admission,
            } => "admission_catalog_target_mismatch",
            Self::PolicySwitchTargetMismatch => "policy_switch_target_mismatch",
            Self::CapabilityNotInManifest {
                lineage: PricingResolutionLineage::Policy,
                dependency: PricingDependencyKind::Catalog,
            } => "unsupported_policy_catalog_capability",
            Self::CapabilityNotInManifest {
                lineage: PricingResolutionLineage::Policy,
                dependency: PricingDependencyKind::Switches,
            } => "unsupported_policy_switch_capability",
            Self::CapabilityNotInManifest {
                lineage: PricingResolutionLineage::Admission,
                dependency: PricingDependencyKind::Catalog,
            } => "unsupported_admission_catalog_capability",
            Self::CapabilityNotInManifest {
                lineage: PricingResolutionLineage::Admission,
                dependency: PricingDependencyKind::Switches,
            } => "unsupported_admission_switch_capability",
            Self::InvalidDependency {
                lineage: PricingResolutionLineage::Policy,
                dependency: PricingDependencyKind::Catalog,
            } => "invalid_policy_catalog",
            Self::InvalidDependency {
                lineage: PricingResolutionLineage::Policy,
                dependency: PricingDependencyKind::Switches,
            } => "invalid_policy_switches",
            Self::InvalidDependency {
                lineage: PricingResolutionLineage::Admission,
                dependency: PricingDependencyKind::Catalog,
            } => "invalid_admission_catalog",
            Self::InvalidDependency {
                lineage: PricingResolutionLineage::Admission,
                dependency: PricingDependencyKind::Switches,
            } => "invalid_admission_switches",
            Self::InvalidPolicyContract => "invalid_policy_contract",
            Self::ModelNotInCatalog {
                lineage: PricingResolutionLineage::Policy,
            } => "policy_model_not_in_catalog",
            Self::ModelNotInCatalog {
                lineage: PricingResolutionLineage::Admission,
            } => "admission_model_not_in_catalog",
            Self::ModelDisabled {
                lineage: PricingResolutionLineage::Policy,
            } => "policy_model_disabled",
            Self::ModelDisabled {
                lineage: PricingResolutionLineage::Admission,
            } => "admission_model_disabled",
            Self::MissingMasterSwitch {
                lineage: PricingResolutionLineage::Policy,
            } => "missing_policy_master_switch",
            Self::MissingMasterSwitch {
                lineage: PricingResolutionLineage::Admission,
            } => "missing_admission_master_switch",
            Self::MasterSwitchDisabled {
                lineage: PricingResolutionLineage::Policy,
            } => "policy_master_switch_disabled",
            Self::MasterSwitchDisabled {
                lineage: PricingResolutionLineage::Admission,
            } => "admission_master_switch_disabled",
            Self::MissingScopedSwitch {
                lineage: PricingResolutionLineage::Policy,
            } => "missing_policy_scoped_switch",
            Self::MissingScopedSwitch {
                lineage: PricingResolutionLineage::Admission,
            } => "missing_admission_scoped_switch",
            Self::PolicyScopedSwitchTargetMismatch => "policy_scoped_switch_target_mismatch",
            Self::AdmissionScopedSwitchTargetMismatch => "admission_scoped_switch_target_mismatch",
            Self::ScopedSwitchDisabled {
                lineage: PricingResolutionLineage::Policy,
            } => "policy_scoped_switch_disabled",
            Self::ScopedSwitchDisabled {
                lineage: PricingResolutionLineage::Admission,
            } => "admission_scoped_switch_disabled",
            Self::MissingRule => "missing_rule",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PricingResolution {
    Resolved(ResolvedPricingRule),
    Rejected(PricingResolutionRejection),
}

fn rejected(reason: PricingResolutionRejection) -> PricingResolution {
    PricingResolution::Rejected(reason)
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.trim() == value
}

fn valid_runtime_manifest(manifest: &RuntimePricingManifest) -> bool {
    if manifest.manifest_generation <= 0
        || !valid_id(&manifest.manifest_digest)
        || manifest.capabilities.is_empty()
    {
        return false;
    }

    let mut identities = BTreeMap::new();
    for capability in &manifest.capabilities {
        if capability.pricing_schema_version <= 0
            || capability.capability_generation <= 0
            || !valid_id(&capability.capability_digest)
        {
            return false;
        }
        let key = (
            capability.pricing_schema_version,
            capability.capability_generation,
        );
        if identities
            .insert(key, capability.capability_digest.as_str())
            .is_some()
        {
            // An exact duplicate is not a set, while two digests for one schema/generation make
            // the generation identity ambiguous. Both shapes fail closed.
            return false;
        }
    }
    true
}

fn manifest_supports(
    manifest: &RuntimePricingManifest,
    pricing_schema_version: i64,
    capability_generation: i64,
    capability_digest: &str,
) -> bool {
    manifest.capabilities.iter().any(|capability| {
        capability.pricing_schema_version == pricing_schema_version
            && capability.capability_generation == capability_generation
            && capability.capability_digest == capability_digest
    })
}

fn required_scope(account_class: AccountClass, product_id: &str) -> ProviderSwitchScope {
    use registry::pricing::PolicySegment;

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

fn validate_catalog_dependency(
    catalog: &PricingCatalogSpec,
    manifest: &RuntimePricingManifest,
    lineage: PricingResolutionLineage,
) -> Result<(), PricingResolutionRejection> {
    if catalog.schema_version != PRICING_SCHEMA_VERSION {
        return Err(PricingResolutionRejection::SchemaMismatch {
            lineage,
            dependency: PricingDependencyKind::Catalog,
        });
    }
    if validate_pricing_catalog(catalog).is_err() {
        return Err(PricingResolutionRejection::InvalidDependency {
            lineage,
            dependency: PricingDependencyKind::Catalog,
        });
    }
    if !manifest_supports(
        manifest,
        catalog.schema_version,
        catalog.capability_generation,
        &catalog.capability_digest,
    ) {
        return Err(PricingResolutionRejection::CapabilityNotInManifest {
            lineage,
            dependency: PricingDependencyKind::Catalog,
        });
    }
    Ok(())
}

fn validate_switch_dependency(
    switches: &ProviderSwitchSpec,
    manifest: &RuntimePricingManifest,
    lineage: PricingResolutionLineage,
) -> Result<(), PricingResolutionRejection> {
    if switches.schema_version != PRICING_SCHEMA_VERSION {
        return Err(PricingResolutionRejection::SchemaMismatch {
            lineage,
            dependency: PricingDependencyKind::Switches,
        });
    }
    if validate_provider_switches(switches).is_err() {
        return Err(PricingResolutionRejection::InvalidDependency {
            lineage,
            dependency: PricingDependencyKind::Switches,
        });
    }
    if !manifest_supports(
        manifest,
        switches.schema_version,
        switches.capability_generation,
        &switches.capability_digest,
    ) {
        return Err(PricingResolutionRejection::CapabilityNotInManifest {
            lineage,
            dependency: PricingDependencyKind::Switches,
        });
    }
    Ok(())
}

fn gate_lineage(
    lineage: PricingResolutionLineage,
    catalog: &PricingCatalogSpec,
    switches: &ProviderSwitchSpec,
    account_class: AccountClass,
    product_id: &str,
    provider_id: &str,
    canonical_model_id: &str,
    alternate_scoped_catalog_generation: Option<i64>,
) -> Result<(), PricingResolutionRejection> {
    let Some(master) = switches.entries.iter().find(|entry| {
        entry.provider_id == provider_id && matches!(entry.scope, ProviderSwitchScope::Master)
    }) else {
        return Err(PricingResolutionRejection::MissingMasterSwitch { lineage });
    };
    if !master.enabled {
        return Err(PricingResolutionRejection::MasterSwitchDisabled { lineage });
    }

    let required_scope = required_scope(account_class, product_id);
    let Some(scoped) = switches
        .entries
        .iter()
        .find(|entry| entry.provider_id == provider_id && entry.scope == required_scope)
    else {
        return Err(PricingResolutionRejection::MissingScopedSwitch { lineage });
    };
    let matches_current_catalog = scoped.catalog_generation == Some(catalog.generation);
    let matches_alternate_catalog = alternate_scoped_catalog_generation
        .is_some_and(|generation| scoped.catalog_generation == Some(generation));
    if !matches_current_catalog && !matches_alternate_catalog {
        return Err(match lineage {
            PricingResolutionLineage::Policy => {
                PricingResolutionRejection::PolicyScopedSwitchTargetMismatch
            }
            PricingResolutionLineage::Admission => {
                PricingResolutionRejection::AdmissionScopedSwitchTargetMismatch
            }
        });
    }
    if !scoped.enabled {
        return Err(PricingResolutionRejection::ScopedSwitchDisabled { lineage });
    }

    let Some(model) = catalog.entries.iter().find(|entry| {
        entry.provider_id == provider_id && entry.canonical_model_id == canonical_model_id
    }) else {
        return Err(PricingResolutionRejection::ModelNotInCatalog { lineage });
    };
    if !model.enabled {
        return Err(PricingResolutionRejection::ModelDisabled { lineage });
    }
    Ok(())
}

fn catalog_identity(catalog: &PricingCatalogSpec) -> ResolvedPricingDependency {
    ResolvedPricingDependency {
        target: catalog.target(),
        pricing_schema_version: catalog.schema_version,
        capability_generation: catalog.capability_generation,
        capability_digest: catalog.capability_digest.clone(),
    }
}

fn switch_identity(switches: &ProviderSwitchSpec) -> ResolvedPricingDependency {
    ResolvedPricingDependency {
        target: switches.target(),
        pricing_schema_version: switches.schema_version,
        capability_generation: switches.capability_generation,
        capability_digest: switches.capability_digest.clone(),
    }
}

/// Resolve an immutable pricing rule from one coherent registry snapshot.
///
/// The function is intentionally stricter than the future shadow caller: every malformed or torn
/// dependency becomes a typed rejection. A later runtime integration may observe this result, but
/// must not turn it into live admission or charging behavior without a separate rollout.
pub fn resolve_pricing(
    bundle: &PricingReadBundle,
    request: &PricingResolutionRequest,
    manifest: &RuntimePricingManifest,
) -> PricingResolution {
    if !valid_id(&request.account_id)
        || !valid_id(&request.provider_id)
        || !valid_id(&request.requested_model_id)
        || !valid_id(&request.canonical_model_id)
    {
        return rejected(PricingResolutionRejection::InvalidRequest);
    }
    if !valid_runtime_manifest(manifest) {
        return rejected(PricingResolutionRejection::InvalidRuntimeManifest);
    }
    if bundle.account_id != request.account_id {
        return rejected(PricingResolutionRejection::AccountMismatch);
    }

    let active = match &bundle.policy {
        PricingPolicySnapshot::Unbound => {
            return rejected(PricingResolutionRejection::NoPolicyBinding)
        }
        PricingPolicySnapshot::Inactive { .. } => {
            return rejected(PricingResolutionRejection::InactivePolicy)
        }
        PricingPolicySnapshot::Active(active) => active,
    };
    if active.policy.account_id != request.account_id {
        return rejected(PricingResolutionRejection::AccountMismatch);
    }

    let Some(policy_catalog) = bundle.policy_catalog.as_ref() else {
        return rejected(PricingResolutionRejection::MissingDependency {
            lineage: PricingResolutionLineage::Policy,
            dependency: PricingDependencyKind::Catalog,
        });
    };
    let Some(policy_switches) = bundle.policy_switches.as_ref() else {
        return rejected(PricingResolutionRejection::MissingDependency {
            lineage: PricingResolutionLineage::Policy,
            dependency: PricingDependencyKind::Switches,
        });
    };
    let Some(admission_catalog) = bundle.admission_catalog.as_ref() else {
        return rejected(PricingResolutionRejection::MissingDependency {
            lineage: PricingResolutionLineage::Admission,
            dependency: PricingDependencyKind::Catalog,
        });
    };
    let Some(admission_switches) = bundle.admission_switches.as_ref() else {
        return rejected(PricingResolutionRejection::MissingDependency {
            lineage: PricingResolutionLineage::Admission,
            dependency: PricingDependencyKind::Switches,
        });
    };

    if active.policy.schema_version != PRICING_SCHEMA_VERSION {
        return rejected(PricingResolutionRejection::PolicySchemaMismatch);
    }
    if validate_account_policy_shape(&active.policy).is_err()
        || validate_account_policy_binding(&active.binding).is_err()
    {
        return rejected(PricingResolutionRejection::InvalidPolicyContract);
    }

    if policy_catalog.product_id != active.policy.product_id
        || policy_catalog.generation != active.policy.catalog_generation
    {
        return rejected(PricingResolutionRejection::CatalogTargetMismatch {
            lineage: PricingResolutionLineage::Policy,
        });
    }
    if policy_switches.generation != active.policy.switch_generation {
        return rejected(PricingResolutionRejection::PolicySwitchTargetMismatch);
    }
    if admission_catalog.product_id != active.policy.product_id {
        return rejected(PricingResolutionRejection::CatalogTargetMismatch {
            lineage: PricingResolutionLineage::Admission,
        });
    }

    if let Err(reason) =
        validate_catalog_dependency(policy_catalog, manifest, PricingResolutionLineage::Policy)
    {
        return rejected(reason);
    }
    if let Err(reason) =
        validate_switch_dependency(policy_switches, manifest, PricingResolutionLineage::Policy)
    {
        return rejected(reason);
    }
    if let Err(reason) = validate_catalog_dependency(
        admission_catalog,
        manifest,
        PricingResolutionLineage::Admission,
    ) {
        return rejected(reason);
    }
    if let Err(reason) = validate_switch_dependency(
        admission_switches,
        manifest,
        PricingResolutionLineage::Admission,
    ) {
        return rejected(reason);
    }

    if let Err(reason) = gate_lineage(
        PricingResolutionLineage::Policy,
        policy_catalog,
        policy_switches,
        active.policy.account_class,
        &active.policy.product_id,
        &request.provider_id,
        &request.canonical_model_id,
        None,
    ) {
        return rejected(reason);
    }

    if validate_account_policy(
        &active.policy,
        policy_catalog,
        policy_switches,
        Some(bundle.account_multiplier_bp),
    )
    .is_err()
    {
        return rejected(PricingResolutionRejection::InvalidPolicyContract);
    }

    // Admission dependencies are intentionally independent of the policy pins. In particular, an
    // S1 switch may still pin C1 while the current admission catalog is already C2. That overlap is
    // valid only while C1 is also the policy catalog; accepting any unrelated third generation, or
    // accepting S1 after policy already advanced to C2, would admit models outside the current
    // switch contract.
    if let Err(reason) = gate_lineage(
        PricingResolutionLineage::Admission,
        admission_catalog,
        admission_switches,
        active.policy.account_class,
        &active.policy.product_id,
        &request.provider_id,
        &request.canonical_model_id,
        Some(policy_catalog.generation),
    ) {
        return rejected(reason);
    }

    let exact = active.policy.rules.iter().find(|rule| {
        matches!(
            &rule.scope,
            PolicyRuleScope::Model {
                provider_id,
                canonical_model_id,
            } if provider_id == &request.provider_id
                && canonical_model_id == &request.canonical_model_id
        )
    });
    let provider = active.policy.rules.iter().find(|rule| {
        matches!(
            &rule.scope,
            PolicyRuleScope::Provider { provider_id }
                if provider_id == &request.provider_id
        )
    });
    let Some(rule) = exact.or(provider) else {
        return rejected(PricingResolutionRejection::MissingRule);
    };

    PricingResolution::Resolved(ResolvedPricingRule {
        product_id: active.policy.product_id.clone(),
        account_class: active.policy.account_class,
        account_multiplier_bp: bundle.account_multiplier_bp,
        provider_id: request.provider_id.clone(),
        requested_model_id: request.requested_model_id.clone(),
        canonical_model_id: request.canonical_model_id.clone(),
        evaluator_schema_version: PRICING_SCHEMA_VERSION,
        runtime_manifest_generation: manifest.manifest_generation,
        runtime_manifest_digest: manifest.manifest_digest.clone(),
        policy_schema_version: active.policy.schema_version,
        policy_lineage: ResolvedPricingLineage {
            catalog: catalog_identity(policy_catalog),
            switches: switch_identity(policy_switches),
        },
        admission_lineage: ResolvedPricingLineage {
            catalog: catalog_identity(admission_catalog),
            switches: switch_identity(admission_switches),
        },
        policy_target: active.policy.target(),
        policy_id: active.policy.policy_id.clone(),
        policy_version: active.policy.policy_version,
        source_policy_digest: active.policy.source_policy_digest.clone(),
        binding: active.binding.clone(),
        rule: rule.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use registry::pricing::{
        AccountPolicySpec, ActiveAccountPolicy, FundingEnforcement, PolicyEnforcement,
        PolicyOwnerType, PolicySegment, PricingCatalogEntrySpec, PricingCatalogSpec, PricingMode,
        ProviderSwitchEntrySpec, ProviderSwitchSpec, ReconciliationState, RuleOrigin,
    };

    const CAPABILITY_GENERATION: i64 = 17;
    const CAPABILITY_DIGEST: &str = "capability-17";
    const NEXT_CAPABILITY_GENERATION: i64 = 18;
    const NEXT_CAPABILITY_DIGEST: &str = "capability-18";

    fn provider_rule(provider_id: &str, multiplier: i64) -> AccountPolicyRuleSpec {
        AccountPolicyRuleSpec {
            rule_id: format!("{provider_id}-provider"),
            rule_digest: format!("{provider_id}-provider-digest"),
            scope: PolicyRuleScope::Provider {
                provider_id: provider_id.to_owned(),
            },
            pricing_mode: PricingMode::Discount,
            rule_origin: RuleOrigin::Managed,
            discount_bps: Some(10_000 - multiplier),
            payable_multiplier_bp: multiplier,
            track_eligible: false,
            retention_eligible: false,
            commission_eligible: false,
        }
    }

    fn model_rule(provider_id: &str, model_id: &str, multiplier: i64) -> AccountPolicyRuleSpec {
        let mut rule = provider_rule(provider_id, multiplier);
        rule.rule_id = format!("{provider_id}-{model_id}");
        rule.rule_digest = format!("{provider_id}-{model_id}-digest");
        rule.scope = PolicyRuleScope::Model {
            provider_id: provider_id.to_owned(),
            canonical_model_id: model_id.to_owned(),
        };
        rule
    }

    fn legacy_provider_rule(provider_id: &str, multiplier: i64) -> AccountPolicyRuleSpec {
        AccountPolicyRuleSpec {
            rule_id: format!("legacy-{provider_id}"),
            rule_digest: format!("legacy-{provider_id}-digest"),
            scope: PolicyRuleScope::Provider {
                provider_id: provider_id.to_owned(),
            },
            pricing_mode: PricingMode::Discount,
            rule_origin: RuleOrigin::Legacy,
            discount_bps: None,
            payable_multiplier_bp: multiplier,
            track_eligible: false,
            retention_eligible: false,
            commission_eligible: false,
        }
    }

    fn bundle() -> PricingReadBundle {
        let catalog = PricingCatalogSpec {
            product_id: "main".to_owned(),
            generation: 3,
            schema_version: PRICING_SCHEMA_VERSION,
            capability_generation: CAPABILITY_GENERATION,
            capability_digest: CAPABILITY_DIGEST.to_owned(),
            content_digest: "catalog-3".to_owned(),
            entries: vec![
                PricingCatalogEntrySpec {
                    provider_id: "anthropic".to_owned(),
                    canonical_model_id: "claude-sonnet-4".to_owned(),
                    enabled: true,
                },
                PricingCatalogEntrySpec {
                    provider_id: "anthropic".to_owned(),
                    canonical_model_id: "claude-opus-4".to_owned(),
                    enabled: true,
                },
                PricingCatalogEntrySpec {
                    provider_id: "openai".to_owned(),
                    canonical_model_id: "gpt-5.6-sol".to_owned(),
                    enabled: true,
                },
            ],
        };
        let switches = ProviderSwitchSpec {
            generation: 5,
            schema_version: PRICING_SCHEMA_VERSION,
            capability_generation: CAPABILITY_GENERATION,
            capability_digest: CAPABILITY_DIGEST.to_owned(),
            content_digest: "switches-5".to_owned(),
            entries: ["anthropic", "openai"]
                .into_iter()
                .flat_map(|provider_id| {
                    [
                        ProviderSwitchEntrySpec {
                            provider_id: provider_id.to_owned(),
                            scope: ProviderSwitchScope::Master,
                            catalog_generation: None,
                            enabled: true,
                        },
                        ProviderSwitchEntrySpec {
                            provider_id: provider_id.to_owned(),
                            scope: ProviderSwitchScope::Segment {
                                product_id: "main".to_owned(),
                                segment: PolicySegment::B2b,
                            },
                            catalog_generation: Some(3),
                            enabled: true,
                        },
                    ]
                })
                .collect(),
        };
        let policy = AccountPolicySpec {
            account_id: "acct".to_owned(),
            effective_version: 7,
            policy_id: "b2b-policy".to_owned(),
            policy_version: 4,
            source_policy_digest: "commerce-policy-4".to_owned(),
            owner_type: PolicyOwnerType::B2bClient,
            owner_id: "client".to_owned(),
            account_class: AccountClass::B2b,
            product_id: "main".to_owned(),
            schema_version: PRICING_SCHEMA_VERSION,
            catalog_generation: 3,
            switch_generation: 5,
            content_digest: "effective-policy-7".to_owned(),
            replacement_locked: false,
            rules: vec![
                provider_rule("anthropic", 8_000),
                model_rule("anthropic", "claude-opus-4", 6_000),
                model_rule("openai", "gpt-5.6-sol", 7_000),
            ],
        };
        PricingReadBundle {
            account_id: "acct".to_owned(),
            account_multiplier_bp: 8_000,
            policy: PricingPolicySnapshot::Active(ActiveAccountPolicy {
                policy,
                binding: AccountPolicyBindingSpec {
                    policy_enforcement: PolicyEnforcement::Shadow,
                    funding_enforcement: FundingEnforcement::LegacySingle,
                    reconciliation_state: ReconciliationState::Verified,
                },
            }),
            policy_catalog: Some(catalog.clone()),
            policy_switches: Some(switches.clone()),
            admission_catalog: Some(catalog),
            admission_switches: Some(switches),
        }
    }

    fn choreography_bundles() -> Vec<PricingReadBundle> {
        let c1_s1_p1 = bundle();

        let mut catalog_v2 = c1_s1_p1
            .admission_catalog
            .as_ref()
            .expect("C1 admission catalog")
            .clone();
        catalog_v2.generation = 4;
        catalog_v2.capability_generation = NEXT_CAPABILITY_GENERATION;
        catalog_v2.capability_digest = NEXT_CAPABILITY_DIGEST.to_owned();
        catalog_v2.content_digest = "catalog-4".to_owned();
        catalog_v2.entries.push(PricingCatalogEntrySpec {
            provider_id: "anthropic".to_owned(),
            canonical_model_id: "claude-future".to_owned(),
            enabled: true,
        });

        let mut switches_v2 = c1_s1_p1
            .admission_switches
            .as_ref()
            .expect("S1 admission switches")
            .clone();
        switches_v2.generation = 6;
        switches_v2.capability_generation = NEXT_CAPABILITY_GENERATION;
        switches_v2.capability_digest = NEXT_CAPABILITY_DIGEST.to_owned();
        switches_v2.content_digest = "switches-6".to_owned();
        for entry in &mut switches_v2.entries {
            if !matches!(entry.scope, ProviderSwitchScope::Master) {
                entry.catalog_generation = Some(catalog_v2.generation);
            }
        }

        let mut c2_s1_p1 = c1_s1_p1.clone();
        c2_s1_p1.admission_catalog = Some(catalog_v2.clone());

        let mut c2_s2_p1 = c2_s1_p1.clone();
        c2_s2_p1.admission_switches = Some(switches_v2.clone());

        let mut c2_s2_p2 = c2_s2_p1.clone();
        c2_s2_p2.policy_catalog = Some(catalog_v2);
        c2_s2_p2.policy_switches = Some(switches_v2);
        let PricingPolicySnapshot::Active(active) = &mut c2_s2_p2.policy else {
            unreachable!()
        };
        active.policy.catalog_generation = 4;
        active.policy.switch_generation = 6;
        active.policy.effective_version = 8;
        active.policy.policy_version = 5;
        active.policy.source_policy_digest = "commerce-policy-5".to_owned();
        active.policy.content_digest = "effective-policy-8".to_owned();

        vec![c1_s1_p1, c2_s1_p1, c2_s2_p1, c2_s2_p2]
    }

    fn capability(generation: i64, digest: &str) -> RuntimePricingCapability {
        RuntimePricingCapability {
            pricing_schema_version: PRICING_SCHEMA_VERSION,
            capability_generation: generation,
            capability_digest: digest.to_owned(),
        }
    }

    fn manifest() -> RuntimePricingManifest {
        RuntimePricingManifest {
            manifest_generation: 2,
            manifest_digest: "runtime-manifest-2".to_owned(),
            capabilities: vec![
                capability(CAPABILITY_GENERATION, CAPABILITY_DIGEST),
                capability(NEXT_CAPABILITY_GENERATION, NEXT_CAPABILITY_DIGEST),
            ],
        }
    }

    fn request(provider_id: &str, model_id: &str) -> PricingResolutionRequest {
        PricingResolutionRequest {
            account_id: "acct".to_owned(),
            provider_id: provider_id.to_owned(),
            requested_model_id: model_id.to_owned(),
            canonical_model_id: model_id.to_owned(),
        }
    }

    fn rejection_with_manifest(
        bundle: &PricingReadBundle,
        request: &PricingResolutionRequest,
        manifest: &RuntimePricingManifest,
    ) -> PricingResolutionRejection {
        match resolve_pricing(bundle, request, manifest) {
            PricingResolution::Rejected(reason) => reason,
            PricingResolution::Resolved(rule) => panic!("unexpected resolution: {rule:?}"),
        }
    }

    fn rejection(
        bundle: &PricingReadBundle,
        request: &PricingResolutionRequest,
    ) -> PricingResolutionRejection {
        rejection_with_manifest(bundle, request, &manifest())
    }

    #[test]
    fn exact_model_rule_replaces_provider_rule_without_stacking() {
        let bundle = bundle();
        let resolved =
            resolve_pricing(&bundle, &request("anthropic", "claude-opus-4"), &manifest());
        let PricingResolution::Resolved(resolved) = resolved else {
            panic!("expected exact model resolution, got {resolved:?}");
        };
        assert_eq!(resolved.rule.rule_id, "anthropic-claude-opus-4");
        assert_eq!(resolved.rule.payable_multiplier_bp, 6_000);
        assert_eq!(resolved.policy_lineage.catalog.target.version, 3);
        assert_eq!(resolved.policy_lineage.switches.target.version, 5);
        assert_eq!(resolved.admission_lineage.catalog.target.version, 3);
        assert_eq!(resolved.admission_lineage.switches.target.version, 5);
        assert_eq!(resolved.policy_target.version, 7);
    }

    #[test]
    fn provider_rule_is_the_only_fallback() {
        let bundle = bundle();
        let resolved = resolve_pricing(
            &bundle,
            &request("anthropic", "claude-sonnet-4"),
            &manifest(),
        );
        let PricingResolution::Resolved(resolved) = resolved else {
            panic!("expected provider fallback, got {resolved:?}");
        };
        assert_eq!(resolved.rule.rule_id, "anthropic-provider");
        assert_eq!(resolved.rule.payable_multiplier_bp, 8_000);

        let mut exact_only_removed = bundle.clone();
        let PricingPolicySnapshot::Active(active) = &mut exact_only_removed.policy else {
            unreachable!()
        };
        active.policy.rules.retain(|rule| {
            !matches!(
                &rule.scope,
                PolicyRuleScope::Model { provider_id, .. } if provider_id == "openai"
            )
        });
        assert_eq!(
            rejection(&exact_only_removed, &request("openai", "gpt-5.6-sol")),
            PricingResolutionRejection::MissingRule,
            "an exact rule for another provider/model cannot become a fallback"
        );
    }

    #[test]
    fn policy_and_binding_absence_are_distinct() {
        let mut current = bundle();
        current.policy = PricingPolicySnapshot::Unbound;
        assert_eq!(
            rejection(&current, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::NoPolicyBinding
        );

        current.policy = PricingPolicySnapshot::Inactive {
            product_id: "main".to_owned(),
            account_class: AccountClass::B2b,
            binding: AccountPolicyBindingSpec {
                policy_enforcement: PolicyEnforcement::LegacyScalar,
                funding_enforcement: FundingEnforcement::LegacySingle,
                reconciliation_state: ReconciliationState::Pending,
            },
        };
        assert_eq!(
            rejection(&current, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::InactivePolicy
        );
    }

    #[test]
    fn dual_lineage_choreography_keeps_the_common_model_available() {
        let states = choreography_bundles();
        let expected_targets = [
            ((3, 5), (3, 5)),
            ((3, 5), (4, 5)),
            ((3, 5), (4, 6)),
            ((4, 6), (4, 6)),
        ];

        for (state, (expected_policy, expected_admission)) in states.iter().zip(expected_targets) {
            let outcome =
                resolve_pricing(state, &request("anthropic", "claude-sonnet-4"), &manifest());
            let PricingResolution::Resolved(resolved) = outcome else {
                panic!("valid choreography state was rejected: {outcome:?}")
            };
            assert_eq!(
                (
                    resolved.policy_lineage.catalog.target.version,
                    resolved.policy_lineage.switches.target.version,
                ),
                expected_policy
            );
            assert_eq!(
                (
                    resolved.admission_lineage.catalog.target.version,
                    resolved.admission_lineage.switches.target.version,
                ),
                expected_admission
            );
            assert_eq!(resolved.evaluator_schema_version, PRICING_SCHEMA_VERSION);
            assert_eq!(resolved.runtime_manifest_generation, 2);
            assert_eq!(resolved.runtime_manifest_digest, "runtime-manifest-2");
        }

        let c2_s1_p1 = &states[1];
        let admission_switches = c2_s1_p1.admission_switches.as_ref().unwrap();
        let scoped = admission_switches
            .entries
            .iter()
            .find(|entry| {
                entry.provider_id == "anthropic"
                    && !matches!(entry.scope, ProviderSwitchScope::Master)
            })
            .unwrap();
        assert_eq!(scoped.catalog_generation, Some(3));
        assert_eq!(
            c2_s1_p1.admission_catalog.as_ref().unwrap().generation,
            4,
            "the resolver must not require an S1 pin to equal the already-current C2 head"
        );
    }

    #[test]
    fn a_new_model_is_blocked_until_the_policy_advances() {
        let states = choreography_bundles();
        for state in [&states[1], &states[2]] {
            assert_eq!(
                rejection(state, &request("anthropic", "claude-future")),
                PricingResolutionRejection::ModelNotInCatalog {
                    lineage: PricingResolutionLineage::Policy,
                },
                "a provider fallback in P1 must not admit a model absent from pinned C1"
            );
        }

        let outcome = resolve_pricing(
            &states[3],
            &request("anthropic", "claude-future"),
            &manifest(),
        );
        let PricingResolution::Resolved(resolved) = outcome else {
            panic!("C2/S2/P2 did not resolve the new model: {outcome:?}")
        };
        assert_eq!(resolved.rule.rule_id, "anthropic-provider");
        assert_eq!(resolved.policy_lineage.catalog.target.version, 4);
        assert_eq!(resolved.admission_lineage.catalog.target.version, 4);
    }

    #[test]
    fn policy_cannot_advance_past_the_current_admission_switch_lineage() {
        let states = choreography_bundles();
        let mut c2_s1_p2 = states[3].clone();
        c2_s1_p2.admission_switches = states[1].admission_switches.clone();

        for model_id in ["claude-sonnet-4", "claude-future"] {
            assert_eq!(
                rejection(&c2_s1_p2, &request("anthropic", model_id)),
                PricingResolutionRejection::AdmissionScopedSwitchTargetMismatch,
                "P2 must fail closed while the current admission switch still pins C1"
            );
        }
    }

    #[test]
    fn manifest_requires_exact_full_tuple_membership() {
        let states = choreography_bundles();
        let c2_s1_p1 = &states[1];

        let only_c1 = RuntimePricingManifest {
            manifest_generation: 1,
            manifest_digest: "only-c1".to_owned(),
            capabilities: vec![capability(CAPABILITY_GENERATION, CAPABILITY_DIGEST)],
        };
        assert_eq!(
            rejection_with_manifest(c2_s1_p1, &request("anthropic", "claude-sonnet-4"), &only_c1,),
            PricingResolutionRejection::CapabilityNotInManifest {
                lineage: PricingResolutionLineage::Admission,
                dependency: PricingDependencyKind::Catalog,
            }
        );

        let only_c2 = RuntimePricingManifest {
            manifest_generation: 1,
            manifest_digest: "only-c2".to_owned(),
            capabilities: vec![capability(
                NEXT_CAPABILITY_GENERATION,
                NEXT_CAPABILITY_DIGEST,
            )],
        };
        assert_eq!(
            rejection_with_manifest(
                &states[2],
                &request("anthropic", "claude-sonnet-4"),
                &only_c2,
            ),
            PricingResolutionRejection::CapabilityNotInManifest {
                lineage: PricingResolutionLineage::Policy,
                dependency: PricingDependencyKind::Catalog,
            }
        );

        let wrong_digest = RuntimePricingManifest {
            manifest_generation: 3,
            manifest_digest: "wrong-c1-digest".to_owned(),
            capabilities: vec![
                capability(CAPABILITY_GENERATION, "not-capability-17"),
                capability(NEXT_CAPABILITY_GENERATION, NEXT_CAPABILITY_DIGEST),
            ],
        };
        assert_eq!(
            rejection_with_manifest(
                c2_s1_p1,
                &request("anthropic", "claude-sonnet-4"),
                &wrong_digest,
            ),
            PricingResolutionRejection::CapabilityNotInManifest {
                lineage: PricingResolutionLineage::Policy,
                dependency: PricingDependencyKind::Catalog,
            },
            "generation-only matching must never be accepted"
        );

        let wrong_schema = RuntimePricingManifest {
            manifest_generation: 3,
            manifest_digest: "wrong-c1-schema".to_owned(),
            capabilities: vec![
                RuntimePricingCapability {
                    pricing_schema_version: PRICING_SCHEMA_VERSION + 1,
                    capability_generation: CAPABILITY_GENERATION,
                    capability_digest: CAPABILITY_DIGEST.to_owned(),
                },
                capability(NEXT_CAPABILITY_GENERATION, NEXT_CAPABILITY_DIGEST),
            ],
        };
        assert_eq!(
            rejection_with_manifest(
                c2_s1_p1,
                &request("anthropic", "claude-sonnet-4"),
                &wrong_schema,
            ),
            PricingResolutionRejection::CapabilityNotInManifest {
                lineage: PricingResolutionLineage::Policy,
                dependency: PricingDependencyKind::Catalog,
            }
        );
    }

    #[test]
    fn malformed_runtime_manifests_fail_closed() {
        let base = manifest();
        let mut malformed = Vec::new();

        let mut no_generation = base.clone();
        no_generation.manifest_generation = 0;
        malformed.push(no_generation);
        let mut padded_digest = base.clone();
        padded_digest.manifest_digest = " manifest".to_owned();
        malformed.push(padded_digest);
        let mut empty = base.clone();
        empty.capabilities.clear();
        malformed.push(empty);
        let mut duplicate = base.clone();
        duplicate
            .capabilities
            .push(duplicate.capabilities[0].clone());
        malformed.push(duplicate);
        let mut conflicting = base.clone();
        conflicting.capabilities.push(capability(
            CAPABILITY_GENERATION,
            "conflicting-capability-17",
        ));
        malformed.push(conflicting);
        let mut invalid_capability = base;
        invalid_capability.capabilities[0].capability_generation = 0;
        malformed.push(invalid_capability);

        for runtime_manifest in malformed {
            assert_eq!(
                rejection_with_manifest(
                    &bundle(),
                    &request("anthropic", "claude-sonnet-4"),
                    &runtime_manifest,
                ),
                PricingResolutionRejection::InvalidRuntimeManifest
            );
        }
    }

    #[test]
    fn policy_and_admission_catalog_gates_are_independent() {
        let current = bundle();
        assert_eq!(
            rejection(&current, &request("anthropic", "claude-unknown")),
            PricingResolutionRejection::ModelNotInCatalog {
                lineage: PricingResolutionLineage::Policy,
            }
        );

        let mut policy_disabled = current.clone();
        policy_disabled.policy_catalog.as_mut().unwrap().entries[0].enabled = false;
        assert_eq!(
            rejection(&policy_disabled, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::ModelDisabled {
                lineage: PricingResolutionLineage::Policy,
            }
        );

        let mut admission_disabled = current.clone();
        admission_disabled
            .admission_catalog
            .as_mut()
            .unwrap()
            .entries[0]
            .enabled = false;
        assert_eq!(
            rejection(
                &admission_disabled,
                &request("anthropic", "claude-sonnet-4")
            ),
            PricingResolutionRejection::ModelDisabled {
                lineage: PricingResolutionLineage::Admission,
            }
        );

        let mut admission_missing = current;
        admission_missing
            .admission_catalog
            .as_mut()
            .unwrap()
            .entries
            .retain(|entry| entry.canonical_model_id != "claude-sonnet-4");
        assert_eq!(
            rejection(&admission_missing, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::ModelNotInCatalog {
                lineage: PricingResolutionLineage::Admission,
            }
        );
    }

    #[test]
    fn every_missing_dependency_has_a_distinct_reason() {
        let cases = [
            (
                PricingResolutionLineage::Policy,
                PricingDependencyKind::Catalog,
            ),
            (
                PricingResolutionLineage::Policy,
                PricingDependencyKind::Switches,
            ),
            (
                PricingResolutionLineage::Admission,
                PricingDependencyKind::Catalog,
            ),
            (
                PricingResolutionLineage::Admission,
                PricingDependencyKind::Switches,
            ),
        ];

        for (lineage, dependency) in cases {
            let mut current = bundle();
            match (lineage, dependency) {
                (PricingResolutionLineage::Policy, PricingDependencyKind::Catalog) => {
                    current.policy_catalog = None
                }
                (PricingResolutionLineage::Policy, PricingDependencyKind::Switches) => {
                    current.policy_switches = None
                }
                (PricingResolutionLineage::Admission, PricingDependencyKind::Catalog) => {
                    current.admission_catalog = None
                }
                (PricingResolutionLineage::Admission, PricingDependencyKind::Switches) => {
                    current.admission_switches = None
                }
            }
            assert_eq!(
                rejection(&current, &request("anthropic", "claude-sonnet-4")),
                PricingResolutionRejection::MissingDependency {
                    lineage,
                    dependency,
                }
            );
        }
    }

    #[test]
    fn policy_and_admission_switch_gates_are_independent() {
        for lineage in [
            PricingResolutionLineage::Policy,
            PricingResolutionLineage::Admission,
        ] {
            let mut master_disabled = bundle();
            let switches = match lineage {
                PricingResolutionLineage::Policy => {
                    master_disabled.policy_switches.as_mut().unwrap()
                }
                PricingResolutionLineage::Admission => {
                    master_disabled.admission_switches.as_mut().unwrap()
                }
            };
            switches
                .entries
                .iter_mut()
                .find(|entry| {
                    entry.provider_id == "anthropic"
                        && matches!(entry.scope, ProviderSwitchScope::Master)
                })
                .unwrap()
                .enabled = false;
            assert_eq!(
                rejection(&master_disabled, &request("anthropic", "claude-sonnet-4")),
                PricingResolutionRejection::MasterSwitchDisabled { lineage }
            );

            let mut scoped_disabled = bundle();
            let switches = match lineage {
                PricingResolutionLineage::Policy => {
                    scoped_disabled.policy_switches.as_mut().unwrap()
                }
                PricingResolutionLineage::Admission => {
                    scoped_disabled.admission_switches.as_mut().unwrap()
                }
            };
            switches
                .entries
                .iter_mut()
                .find(|entry| {
                    entry.provider_id == "anthropic"
                        && matches!(
                            entry.scope,
                            ProviderSwitchScope::Segment {
                                segment: PolicySegment::B2b,
                                ..
                            }
                        )
                })
                .unwrap()
                .enabled = false;
            assert_eq!(
                rejection(&scoped_disabled, &request("anthropic", "claude-sonnet-4")),
                PricingResolutionRejection::ScopedSwitchDisabled { lineage }
            );

            let mut master_missing = bundle();
            let switches = match lineage {
                PricingResolutionLineage::Policy => {
                    master_missing.policy_switches.as_mut().unwrap()
                }
                PricingResolutionLineage::Admission => {
                    master_missing.admission_switches.as_mut().unwrap()
                }
            };
            switches
                .entries
                .retain(|entry| entry.provider_id != "anthropic");
            assert_eq!(
                rejection(&master_missing, &request("anthropic", "claude-sonnet-4")),
                PricingResolutionRejection::MissingMasterSwitch { lineage }
            );

            let mut scoped_missing = bundle();
            let switches = match lineage {
                PricingResolutionLineage::Policy => {
                    scoped_missing.policy_switches.as_mut().unwrap()
                }
                PricingResolutionLineage::Admission => {
                    scoped_missing.admission_switches.as_mut().unwrap()
                }
            };
            switches.entries.retain(|entry| {
                entry.provider_id != "anthropic"
                    || matches!(entry.scope, ProviderSwitchScope::Master)
            });
            assert_eq!(
                rejection(&scoped_missing, &request("anthropic", "claude-sonnet-4")),
                PricingResolutionRejection::MissingScopedSwitch { lineage }
            );
        }

        let mut policy_target_mismatch = bundle();
        for entry in &mut policy_target_mismatch
            .policy_switches
            .as_mut()
            .unwrap()
            .entries
        {
            if !matches!(entry.scope, ProviderSwitchScope::Master) {
                entry.catalog_generation = Some(2);
            }
        }
        assert_eq!(
            rejection(
                &policy_target_mismatch,
                &request("anthropic", "claude-sonnet-4")
            ),
            PricingResolutionRejection::PolicyScopedSwitchTargetMismatch
        );
    }

    #[test]
    fn fixed_provider_plane_cannot_borrow_another_provider_rule() {
        let bundle = bundle();
        assert_eq!(
            rejection(&bundle, &request("google", "claude-sonnet-4")),
            PricingResolutionRejection::MissingMasterSwitch {
                lineage: PricingResolutionLineage::Policy,
            }
        );
    }

    #[test]
    fn alias_and_canonical_request_share_one_policy_identity() {
        let bundle = bundle();
        let canonical = request("openai", "gpt-5.6-sol");
        let mut alias = canonical.clone();
        alias.requested_model_id = "gpt-5.6".to_owned();

        let PricingResolution::Resolved(canonical_resolved) =
            resolve_pricing(&bundle, &canonical, &manifest())
        else {
            panic!("canonical request did not resolve")
        };
        let PricingResolution::Resolved(alias_resolved) =
            resolve_pricing(&bundle, &alias, &manifest())
        else {
            panic!("alias request did not resolve")
        };
        assert_eq!(canonical_resolved.rule, alias_resolved.rule);
        assert_eq!(
            canonical_resolved.policy_target,
            alias_resolved.policy_target
        );
        assert_eq!(alias_resolved.requested_model_id, "gpt-5.6");
        assert_eq!(alias_resolved.canonical_model_id, "gpt-5.6-sol");
    }

    #[test]
    fn every_dependency_requires_its_own_manifest_member() {
        let cases = [
            (
                PricingResolutionLineage::Policy,
                PricingDependencyKind::Catalog,
            ),
            (
                PricingResolutionLineage::Policy,
                PricingDependencyKind::Switches,
            ),
            (
                PricingResolutionLineage::Admission,
                PricingDependencyKind::Catalog,
            ),
            (
                PricingResolutionLineage::Admission,
                PricingDependencyKind::Switches,
            ),
        ];

        for (index, (lineage, dependency)) in cases.into_iter().enumerate() {
            let generation = 90 + index as i64;
            let digest = format!("unsupported-capability-{generation}");
            let mut current = bundle();
            match (lineage, dependency) {
                (PricingResolutionLineage::Policy, PricingDependencyKind::Catalog) => {
                    let catalog = current.policy_catalog.as_mut().unwrap();
                    catalog.capability_generation = generation;
                    catalog.capability_digest = digest;
                }
                (PricingResolutionLineage::Policy, PricingDependencyKind::Switches) => {
                    let switches = current.policy_switches.as_mut().unwrap();
                    switches.capability_generation = generation;
                    switches.capability_digest = digest;
                }
                (PricingResolutionLineage::Admission, PricingDependencyKind::Catalog) => {
                    let catalog = current.admission_catalog.as_mut().unwrap();
                    catalog.capability_generation = generation;
                    catalog.capability_digest = digest;
                }
                (PricingResolutionLineage::Admission, PricingDependencyKind::Switches) => {
                    let switches = current.admission_switches.as_mut().unwrap();
                    switches.capability_generation = generation;
                    switches.capability_digest = digest;
                }
            }
            assert_eq!(
                rejection(&current, &request("anthropic", "claude-sonnet-4")),
                PricingResolutionRejection::CapabilityNotInManifest {
                    lineage,
                    dependency,
                }
            );
        }
    }

    #[test]
    fn schema_and_target_mismatches_identify_the_exact_lineage() {
        let dependency_cases = [
            (
                PricingResolutionLineage::Policy,
                PricingDependencyKind::Catalog,
            ),
            (
                PricingResolutionLineage::Policy,
                PricingDependencyKind::Switches,
            ),
            (
                PricingResolutionLineage::Admission,
                PricingDependencyKind::Catalog,
            ),
            (
                PricingResolutionLineage::Admission,
                PricingDependencyKind::Switches,
            ),
        ];
        for (lineage, dependency) in dependency_cases {
            let mut current = bundle();
            match (lineage, dependency) {
                (PricingResolutionLineage::Policy, PricingDependencyKind::Catalog) => {
                    current.policy_catalog.as_mut().unwrap().schema_version += 1
                }
                (PricingResolutionLineage::Policy, PricingDependencyKind::Switches) => {
                    current.policy_switches.as_mut().unwrap().schema_version += 1
                }
                (PricingResolutionLineage::Admission, PricingDependencyKind::Catalog) => {
                    current.admission_catalog.as_mut().unwrap().schema_version += 1
                }
                (PricingResolutionLineage::Admission, PricingDependencyKind::Switches) => {
                    current.admission_switches.as_mut().unwrap().schema_version += 1
                }
            }
            assert_eq!(
                rejection(&current, &request("anthropic", "claude-sonnet-4")),
                PricingResolutionRejection::SchemaMismatch {
                    lineage,
                    dependency,
                }
            );
        }

        let mut policy_schema = bundle();
        let PricingPolicySnapshot::Active(active) = &mut policy_schema.policy else {
            unreachable!()
        };
        active.policy.schema_version += 1;
        assert_eq!(
            rejection(&policy_schema, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::PolicySchemaMismatch
        );

        let mut policy_catalog_target = bundle();
        policy_catalog_target
            .policy_catalog
            .as_mut()
            .unwrap()
            .generation += 1;
        assert_eq!(
            rejection(
                &policy_catalog_target,
                &request("anthropic", "claude-sonnet-4")
            ),
            PricingResolutionRejection::CatalogTargetMismatch {
                lineage: PricingResolutionLineage::Policy,
            }
        );

        let mut policy_switch_target = bundle();
        policy_switch_target
            .policy_switches
            .as_mut()
            .unwrap()
            .generation += 1;
        assert_eq!(
            rejection(
                &policy_switch_target,
                &request("anthropic", "claude-sonnet-4")
            ),
            PricingResolutionRejection::PolicySwitchTargetMismatch
        );

        let mut admission_catalog_target = bundle();
        admission_catalog_target
            .admission_catalog
            .as_mut()
            .unwrap()
            .product_id = "other-product".to_owned();
        assert_eq!(
            rejection(
                &admission_catalog_target,
                &request("anthropic", "claude-sonnet-4")
            ),
            PricingResolutionRejection::CatalogTargetMismatch {
                lineage: PricingResolutionLineage::Admission,
            }
        );
    }

    #[test]
    fn malformed_dependencies_and_policy_contracts_fail_closed() {
        for lineage in [
            PricingResolutionLineage::Policy,
            PricingResolutionLineage::Admission,
        ] {
            let mut invalid_catalog = bundle();
            let catalog = match lineage {
                PricingResolutionLineage::Policy => {
                    invalid_catalog.policy_catalog.as_mut().unwrap()
                }
                PricingResolutionLineage::Admission => {
                    invalid_catalog.admission_catalog.as_mut().unwrap()
                }
            };
            catalog.entries.push(catalog.entries[0].clone());
            assert_eq!(
                rejection(&invalid_catalog, &request("anthropic", "claude-sonnet-4")),
                PricingResolutionRejection::InvalidDependency {
                    lineage,
                    dependency: PricingDependencyKind::Catalog,
                }
            );

            let mut invalid_switches = bundle();
            let switches = match lineage {
                PricingResolutionLineage::Policy => {
                    invalid_switches.policy_switches.as_mut().unwrap()
                }
                PricingResolutionLineage::Admission => {
                    invalid_switches.admission_switches.as_mut().unwrap()
                }
            };
            switches.entries.push(switches.entries[0].clone());
            assert_eq!(
                rejection(&invalid_switches, &request("anthropic", "claude-sonnet-4")),
                PricingResolutionRejection::InvalidDependency {
                    lineage,
                    dependency: PricingDependencyKind::Switches,
                }
            );
        }

        let mut mismatched_policy_capabilities = bundle();
        let switches = mismatched_policy_capabilities
            .policy_switches
            .as_mut()
            .unwrap();
        switches.capability_generation = NEXT_CAPABILITY_GENERATION;
        switches.capability_digest = NEXT_CAPABILITY_DIGEST.to_owned();
        assert_eq!(
            rejection(
                &mismatched_policy_capabilities,
                &request("anthropic", "claude-sonnet-4")
            ),
            PricingResolutionRejection::InvalidPolicyContract
        );

        let mut invalid_binding = bundle();
        let PricingPolicySnapshot::Active(active) = &mut invalid_binding.policy else {
            unreachable!()
        };
        active.binding.policy_enforcement = PolicyEnforcement::Strict;
        assert_eq!(
            rejection(&invalid_binding, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::InvalidPolicyContract
        );
    }

    #[test]
    fn resolved_result_preserves_full_manifest_and_dual_lineage_identity() {
        let state = &choreography_bundles()[1];
        let outcome = resolve_pricing(state, &request("anthropic", "claude-sonnet-4"), &manifest());
        let PricingResolution::Resolved(resolved) = outcome else {
            panic!("dual-lineage fixture did not resolve: {outcome:?}")
        };

        assert_eq!(resolved.evaluator_schema_version, PRICING_SCHEMA_VERSION);
        assert_eq!(resolved.runtime_manifest_generation, 2);
        assert_eq!(resolved.runtime_manifest_digest, "runtime-manifest-2");
        assert_eq!(resolved.policy_schema_version, PRICING_SCHEMA_VERSION);
        assert_eq!(
            resolved.policy_lineage.catalog,
            ResolvedPricingDependency {
                target: VersionTarget::new(3, "catalog-3"),
                pricing_schema_version: PRICING_SCHEMA_VERSION,
                capability_generation: CAPABILITY_GENERATION,
                capability_digest: CAPABILITY_DIGEST.to_owned(),
            }
        );
        assert_eq!(
            resolved.policy_lineage.switches,
            ResolvedPricingDependency {
                target: VersionTarget::new(5, "switches-5"),
                pricing_schema_version: PRICING_SCHEMA_VERSION,
                capability_generation: CAPABILITY_GENERATION,
                capability_digest: CAPABILITY_DIGEST.to_owned(),
            }
        );
        assert_eq!(
            resolved.admission_lineage.catalog,
            ResolvedPricingDependency {
                target: VersionTarget::new(4, "catalog-4"),
                pricing_schema_version: PRICING_SCHEMA_VERSION,
                capability_generation: NEXT_CAPABILITY_GENERATION,
                capability_digest: NEXT_CAPABILITY_DIGEST.to_owned(),
            }
        );
        assert_eq!(
            resolved.admission_lineage.switches,
            ResolvedPricingDependency {
                target: VersionTarget::new(5, "switches-5"),
                pricing_schema_version: PRICING_SCHEMA_VERSION,
                capability_generation: CAPABILITY_GENERATION,
                capability_digest: CAPABILITY_DIGEST.to_owned(),
            }
        );
        assert_eq!(
            resolved.policy_target,
            VersionTarget::new(7, "effective-policy-7")
        );
        assert_eq!(resolved.policy_id, "b2b-policy");
        assert_eq!(resolved.source_policy_digest, "commerce-policy-4");
    }

    #[test]
    fn legacy_openkeys_scalar_is_fenced_by_the_same_bundle() {
        let mut current = bundle();
        current.account_multiplier_bp = 7_300;
        for catalog in [
            current.policy_catalog.as_mut().unwrap(),
            current.admission_catalog.as_mut().unwrap(),
        ] {
            catalog.product_id = "openkeys".to_owned();
        }
        for switches in [
            current.policy_switches.as_mut().unwrap(),
            current.admission_switches.as_mut().unwrap(),
        ] {
            for entry in &mut switches.entries {
                if !matches!(entry.scope, ProviderSwitchScope::Master) {
                    entry.scope = ProviderSwitchScope::Product {
                        product_id: "openkeys".to_owned(),
                    };
                }
            }
        }
        let PricingPolicySnapshot::Active(active) = &mut current.policy else {
            unreachable!()
        };
        active.policy.owner_type = PolicyOwnerType::OpenKeys;
        active.policy.owner_id = "acct".to_owned();
        active.policy.account_class = AccountClass::OpenKeys;
        active.policy.product_id = "openkeys".to_owned();
        active.policy.replacement_locked = true;
        active.policy.rules = ["anthropic", "openai"]
            .into_iter()
            .map(|provider| legacy_provider_rule(provider, 7_300))
            .collect();

        let PricingResolution::Resolved(resolved) = resolve_pricing(
            &current,
            &request("anthropic", "claude-sonnet-4"),
            &manifest(),
        ) else {
            panic!("coherent legacy OpenKeys bundle did not resolve")
        };
        assert_eq!(resolved.account_multiplier_bp, 7_300);
        assert_eq!(resolved.rule.payable_multiplier_bp, 7_300);

        current.account_multiplier_bp = 7_400;
        assert_eq!(
            rejection(&current, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::InvalidPolicyContract
        );
    }

    #[test]
    fn request_and_bundle_identity_mismatch_fails_closed() {
        let bundle = bundle();
        let mut mismatched = request("anthropic", "claude-sonnet-4");
        mismatched.account_id = "other-account".to_owned();
        assert_eq!(
            rejection(&bundle, &mismatched),
            PricingResolutionRejection::AccountMismatch
        );

        let mut invalid = request("anthropic", "claude-sonnet-4");
        invalid.provider_id = " anthropic".to_owned();
        assert_eq!(
            rejection(&bundle, &invalid),
            PricingResolutionRejection::InvalidRequest
        );
        assert_eq!(
            PricingResolutionRejection::MissingRule.code(),
            "missing_rule"
        );
    }
}
