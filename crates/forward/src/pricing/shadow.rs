//! Pure construction of dormant pricing shadow work and evaluation inputs.
//!
//! The module has no database, clock, queue, configuration or runtime caller. A future bounded
//! worker may supply one coherent registry read bundle, but this code cannot enqueue, persist,
//! change admission or participate in charging by itself.

use super::{
    resolve_pricing, PricingDependencyKind, PricingResolution, PricingResolutionLineage,
    PricingResolutionRejection, PricingResolutionRequest, ResolvedPricingDependency,
    ResolvedPricingLineage, ResolvedPricingRule, RuntimePricingManifest,
};
use anyhow::{bail, Context, Result};
use registry::pricing::{
    LegacyScalarAdmissionSnapshot, PricingReadBundle, PricingRuntimeManifestEvidence,
    PricingShadowAdmissionEvaluationInput, PricingShadowDependency, PricingShadowEvaluationOutcome,
    PricingShadowLineage, PricingShadowPolicyIdentity, PricingShadowReadErrorCode,
    PricingShadowRejectionCode, PricingShadowResolved, PricingShadowResolvedInput,
    ShadowActualSnapshotRef, ShadowDiagnosticContext, ShadowEligibilityError,
    PRICING_SCHEMA_VERSION,
};

/// Bounded immutable data needed to evaluate one future pricing shadow request.
///
/// All request/account/provider/model fields come from a validated actual admission snapshot.
/// The full canonical manifest member set is pinned so insert-time dependency membership can be
/// proved even if runtime configuration changes while an item is waiting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingShadowWorkItem {
    actual: ShadowActualSnapshotRef,
    runtime_manifest: PricingRuntimeManifestEvidence,
    enqueued_ts: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingShadowWorkItemError {
    InvalidActualSnapshot,
    InvalidEnqueueTimestamp,
    EnqueuedBeforeAdmission,
    InvalidActualAmount,
    /// Compatibility-only mapping for the retired first-rollout funding-cap rejection.
    BalanceCappedActual,
}

impl PricingShadowWorkItemError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidActualSnapshot => "invalid_actual_snapshot",
            Self::InvalidEnqueueTimestamp => "invalid_enqueue_timestamp",
            Self::EnqueuedBeforeAdmission => "enqueued_before_admission",
            Self::InvalidActualAmount => "invalid_actual_amount",
            Self::BalanceCappedActual => "balance_capped_actual",
        }
    }
}

impl std::fmt::Display for PricingShadowWorkItemError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for PricingShadowWorkItemError {}

impl PricingShadowWorkItem {
    pub fn new(
        snapshot: &LegacyScalarAdmissionSnapshot,
        runtime_manifest: PricingRuntimeManifestEvidence,
        enqueued_ts: i64,
    ) -> std::result::Result<Self, PricingShadowWorkItemError> {
        let actual = ShadowActualSnapshotRef::from_snapshot(snapshot)
            .map_err(|_| PricingShadowWorkItemError::InvalidActualSnapshot)?;
        actual
            .validate_shadow_eligibility(enqueued_ts)
            .map_err(|error| match error {
                ShadowEligibilityError::InvalidActualSnapshot => {
                    PricingShadowWorkItemError::InvalidActualSnapshot
                }
                ShadowEligibilityError::InvalidEnqueueTimestamp => {
                    PricingShadowWorkItemError::InvalidEnqueueTimestamp
                }
                ShadowEligibilityError::EnqueuedBeforeAdmission => {
                    PricingShadowWorkItemError::EnqueuedBeforeAdmission
                }
                ShadowEligibilityError::InvalidActualAmount => {
                    PricingShadowWorkItemError::InvalidActualAmount
                }
                ShadowEligibilityError::BalanceCappedActual => {
                    PricingShadowWorkItemError::BalanceCappedActual
                }
            })?;
        Ok(Self {
            actual,
            runtime_manifest,
            enqueued_ts,
        })
    }

    pub fn request_id(&self) -> &str {
        self.actual.request_id()
    }

    pub fn account_id(&self) -> &str {
        self.actual.account_id()
    }

    pub const fn provider(&self) -> registry::pricing::SnapshotProvider {
        self.actual.provider()
    }

    pub const fn enqueued_ts(&self) -> i64 {
        self.enqueued_ts
    }
}

/// Result of the future bounded policy-read step, supplied explicitly to the pure builder.
#[derive(Clone, Copy, Debug)]
pub enum PricingShadowEvaluationSource<'a> {
    Bundle(&'a PricingReadBundle),
    ReadFailure(PricingShadowReadFailure),
}

/// Failures a future bounded reader may report without fabricating a policy rejection.
///
/// Invalid actual snapshots are rejected while constructing the work item. Errors converting a
/// resolved amount remain outer builder errors until registry exposes a distinguishable typed
/// arithmetic error; neither case may be mislabeled by a caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingShadowReadFailure {
    PricingReadFailed,
    EvaluationTimeout,
    EvaluationCancelled,
}

/// Build one validated immutable registry input without reading or writing external state.
///
/// Timestamps are explicit inputs: this function never reads a clock. A successful bundle is
/// resolved exactly once. A typed read failure is persisted as a read-error outcome and is never
/// confused with a policy rejection.
pub fn build_pricing_shadow_evaluation(
    work: PricingShadowWorkItem,
    source: PricingShadowEvaluationSource<'_>,
    evaluated_ts: i64,
    diagnostic_context: ShadowDiagnosticContext,
) -> Result<PricingShadowAdmissionEvaluationInput> {
    let outcome = match source {
        PricingShadowEvaluationSource::Bundle(bundle) => {
            let manifest = RuntimePricingManifest::from_evidence(&work.runtime_manifest);
            let request = PricingResolutionRequest {
                account_id: work.actual.account_id().to_owned(),
                provider_id: work.actual.provider().as_str().to_owned(),
                requested_model_id: work.actual.requested_model_id().to_owned(),
                canonical_model_id: work.actual.canonical_model_id().to_owned(),
            };
            let resolution = resolve_pricing(bundle, &request, &manifest);
            // Resolve every supplied bundle exactly once, but never serialize another account's
            // scalar beside this actual snapshot. The shadow schema intentionally has no
            // observed-account column, so such a row would be false durable attribution even
            // with an AccountMismatch reason.
            if bundle.account_id != work.actual.account_id() {
                bail!("pricing shadow read bundle differs from the actual snapshot account");
            }
            if !(0..=10_000).contains(&bundle.account_multiplier_bp) {
                bail!("pricing shadow read bundle contains an invalid legacy scalar");
            }
            shadow_outcome(
                &work.actual,
                &work.runtime_manifest,
                bundle.account_multiplier_bp,
                resolution,
            )?
        }
        PricingShadowEvaluationSource::ReadFailure(reason) => {
            PricingShadowEvaluationOutcome::ReadError {
                reason: match reason {
                    PricingShadowReadFailure::PricingReadFailed => {
                        PricingShadowReadErrorCode::PricingReadFailed
                    }
                    PricingShadowReadFailure::EvaluationTimeout => {
                        PricingShadowReadErrorCode::EvaluationTimeout
                    }
                    PricingShadowReadFailure::EvaluationCancelled => {
                        PricingShadowReadErrorCode::EvaluationCancelled
                    }
                },
            }
        }
    };

    PricingShadowAdmissionEvaluationInput::new(
        work.actual,
        PRICING_SCHEMA_VERSION,
        work.runtime_manifest,
        work.enqueued_ts,
        evaluated_ts,
        outcome,
        diagnostic_context,
    )
    .context("construct canonical pricing shadow evaluation input")
}

fn shadow_outcome(
    actual: &ShadowActualSnapshotRef,
    runtime_manifest: &PricingRuntimeManifestEvidence,
    observed_multiplier_bp: i64,
    resolution: PricingResolution,
) -> Result<PricingShadowEvaluationOutcome> {
    let resolved = match resolution {
        PricingResolution::Rejected(reason) => {
            return Ok(PricingShadowEvaluationOutcome::Rejected {
                reason: shadow_rejection_code(reason),
                observed_multiplier_bp,
            });
        }
        PricingResolution::Resolved(resolved) => resolved,
    };

    validate_resolved_identity(actual, runtime_manifest, observed_multiplier_bp, &resolved)?;

    let ResolvedPricingRule {
        product_id,
        account_class,
        account_multiplier_bp,
        provider_id: _,
        requested_model_id: _,
        canonical_model_id: _,
        evaluator_schema_version: _,
        runtime_manifest_generation: _,
        runtime_manifest_digest: _,
        policy_schema_version,
        policy_lineage,
        admission_lineage,
        policy_target,
        policy_id,
        policy_version,
        source_policy_digest,
        binding: _,
        rule,
    } = resolved;

    let resolved = PricingShadowResolved::new(
        actual,
        PricingShadowResolvedInput {
            observed_multiplier_bp: account_multiplier_bp,
            product_id,
            account_class,
            policy: PricingShadowPolicyIdentity {
                target: policy_target,
                policy_id,
                policy_version,
                source_policy_digest,
                schema_version: policy_schema_version,
            },
            policy_lineage: shadow_lineage(policy_lineage),
            admission_lineage: shadow_lineage(admission_lineage),
            rule,
        },
    )
    .context("convert resolved pricing rule into canonical shadow evidence")?;

    Ok(PricingShadowEvaluationOutcome::Resolved(Box::new(resolved)))
}

fn validate_resolved_identity(
    actual: &ShadowActualSnapshotRef,
    runtime_manifest: &PricingRuntimeManifestEvidence,
    observed_multiplier_bp: i64,
    resolved: &ResolvedPricingRule,
) -> Result<()> {
    if resolved.evaluator_schema_version != PRICING_SCHEMA_VERSION {
        bail!("shadow resolver returned an unexpected evaluator schema");
    }
    if resolved.runtime_manifest_generation != runtime_manifest.manifest_generation()
        || resolved.runtime_manifest_digest != runtime_manifest.manifest_digest()
    {
        bail!("shadow resolver manifest identity differs from canonical registry evidence");
    }
    if resolved.provider_id != actual.provider().as_str()
        || resolved.requested_model_id != actual.requested_model_id()
        || resolved.canonical_model_id != actual.canonical_model_id()
    {
        bail!("shadow resolver provider or model identity differs from the actual snapshot");
    }
    if resolved.account_multiplier_bp != observed_multiplier_bp {
        bail!("shadow resolver scalar differs from the coherent pricing read bundle");
    }
    Ok(())
}

fn shadow_lineage(lineage: ResolvedPricingLineage) -> PricingShadowLineage {
    PricingShadowLineage {
        catalog: shadow_dependency(lineage.catalog),
        switches: shadow_dependency(lineage.switches),
    }
}

fn shadow_dependency(dependency: ResolvedPricingDependency) -> PricingShadowDependency {
    PricingShadowDependency {
        target: dependency.target,
        pricing_schema_version: dependency.pricing_schema_version,
        capability_generation: dependency.capability_generation,
        capability_digest: dependency.capability_digest,
    }
}

const fn shadow_rejection_code(reason: PricingResolutionRejection) -> PricingShadowRejectionCode {
    use PricingDependencyKind::{Catalog, Switches};
    use PricingResolutionLineage::{Admission, Policy};
    use PricingResolutionRejection as Rejection;
    use PricingShadowRejectionCode as Shadow;

    match reason {
        Rejection::InvalidRequest => Shadow::InvalidRequest,
        Rejection::InvalidRuntimeManifest => Shadow::InvalidRuntimeManifest,
        Rejection::AccountMismatch => Shadow::AccountMismatch,
        Rejection::NoPolicyBinding => Shadow::NoPolicyBinding,
        Rejection::InactivePolicy => Shadow::InactivePolicy,
        Rejection::MissingDependency {
            lineage: Policy,
            dependency: Catalog,
        } => Shadow::MissingPolicyCatalog,
        Rejection::MissingDependency {
            lineage: Policy,
            dependency: Switches,
        } => Shadow::MissingPolicySwitches,
        Rejection::MissingDependency {
            lineage: Admission,
            dependency: Catalog,
        } => Shadow::MissingAdmissionCatalog,
        Rejection::MissingDependency {
            lineage: Admission,
            dependency: Switches,
        } => Shadow::MissingAdmissionSwitches,
        Rejection::PolicySchemaMismatch => Shadow::PolicySchemaMismatch,
        Rejection::SchemaMismatch {
            lineage: Policy,
            dependency: Catalog,
        } => Shadow::PolicyCatalogSchemaMismatch,
        Rejection::SchemaMismatch {
            lineage: Policy,
            dependency: Switches,
        } => Shadow::PolicySwitchSchemaMismatch,
        Rejection::SchemaMismatch {
            lineage: Admission,
            dependency: Catalog,
        } => Shadow::AdmissionCatalogSchemaMismatch,
        Rejection::SchemaMismatch {
            lineage: Admission,
            dependency: Switches,
        } => Shadow::AdmissionSwitchSchemaMismatch,
        Rejection::CatalogTargetMismatch { lineage: Policy } => Shadow::PolicyCatalogTargetMismatch,
        Rejection::CatalogTargetMismatch { lineage: Admission } => {
            Shadow::AdmissionCatalogTargetMismatch
        }
        Rejection::PolicySwitchTargetMismatch => Shadow::PolicySwitchTargetMismatch,
        Rejection::CapabilityNotInManifest {
            lineage: Policy,
            dependency: Catalog,
        } => Shadow::UnsupportedPolicyCatalogCapability,
        Rejection::CapabilityNotInManifest {
            lineage: Policy,
            dependency: Switches,
        } => Shadow::UnsupportedPolicySwitchCapability,
        Rejection::CapabilityNotInManifest {
            lineage: Admission,
            dependency: Catalog,
        } => Shadow::UnsupportedAdmissionCatalogCapability,
        Rejection::CapabilityNotInManifest {
            lineage: Admission,
            dependency: Switches,
        } => Shadow::UnsupportedAdmissionSwitchCapability,
        Rejection::InvalidDependency {
            lineage: Policy,
            dependency: Catalog,
        } => Shadow::InvalidPolicyCatalog,
        Rejection::InvalidDependency {
            lineage: Policy,
            dependency: Switches,
        } => Shadow::InvalidPolicySwitches,
        Rejection::InvalidDependency {
            lineage: Admission,
            dependency: Catalog,
        } => Shadow::InvalidAdmissionCatalog,
        Rejection::InvalidDependency {
            lineage: Admission,
            dependency: Switches,
        } => Shadow::InvalidAdmissionSwitches,
        Rejection::InvalidPolicyContract => Shadow::InvalidPolicyContract,
        Rejection::ModelNotInCatalog { lineage: Policy } => Shadow::PolicyModelNotInCatalog,
        Rejection::ModelNotInCatalog { lineage: Admission } => Shadow::AdmissionModelNotInCatalog,
        Rejection::ModelDisabled { lineage: Policy } => Shadow::PolicyModelDisabled,
        Rejection::ModelDisabled { lineage: Admission } => Shadow::AdmissionModelDisabled,
        Rejection::MissingMasterSwitch { lineage: Policy } => Shadow::MissingPolicyMasterSwitch,
        Rejection::MissingMasterSwitch { lineage: Admission } => {
            Shadow::MissingAdmissionMasterSwitch
        }
        Rejection::MasterSwitchDisabled { lineage: Policy } => Shadow::PolicyMasterSwitchDisabled,
        Rejection::MasterSwitchDisabled { lineage: Admission } => {
            Shadow::AdmissionMasterSwitchDisabled
        }
        Rejection::MissingScopedSwitch { lineage: Policy } => Shadow::MissingPolicyScopedSwitch,
        Rejection::MissingScopedSwitch { lineage: Admission } => {
            Shadow::MissingAdmissionScopedSwitch
        }
        Rejection::PolicyScopedSwitchTargetMismatch => Shadow::PolicyScopedSwitchTargetMismatch,
        Rejection::AdmissionScopedSwitchTargetMismatch => {
            Shadow::AdmissionScopedSwitchTargetMismatch
        }
        Rejection::ScopedSwitchDisabled { lineage: Policy } => Shadow::PolicyScopedSwitchDisabled,
        Rejection::ScopedSwitchDisabled { lineage: Admission } => {
            Shadow::AdmissionScopedSwitchDisabled
        }
        Rejection::MissingRule => Shadow::MissingRule,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use registry::pricing::{
        AccountClass, AccountPolicyBindingSpec, AccountPolicyRuleSpec, AccountPolicySpec,
        ActiveAccountPolicy, FundingEnforcement, LegacyPremiumModifiers,
        LegacyScalarAdmissionSnapshotInput, PolicyEnforcement, PolicyOwnerType, PolicyRuleScope,
        PolicySegment, PricingCatalogEntrySpec, PricingCatalogSpec, PricingMode,
        PricingPolicySnapshot, PricingRuntimeCapabilityEvidence, PricingShadowComparison,
        ProviderSwitchEntrySpec, ProviderSwitchScope, ProviderSwitchSpec, ReconciliationState,
        RuleOrigin, SnapshotAnthropicInferenceGeo, SnapshotAnthropicSpeed,
        SnapshotGeminiContextRate, SnapshotGeminiSearchBilling, SnapshotProvider,
    };

    const CAPABILITY_GENERATION: i64 = 17;
    const CAPABILITY_DIGEST: &str = "capability-17";
    const NEXT_CAPABILITY_GENERATION: i64 = 18;
    const NEXT_CAPABILITY_DIGEST: &str = "capability-18";
    const ADMISSION_TS: i64 = 200;
    const ENQUEUED_TS: i64 = 201;
    const EVALUATED_TS: i64 = 202;

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

    fn evidence() -> PricingRuntimeManifestEvidence {
        PricingRuntimeManifestEvidence::new(
            2,
            vec![
                PricingRuntimeCapabilityEvidence::new(
                    PRICING_SCHEMA_VERSION,
                    NEXT_CAPABILITY_GENERATION,
                    NEXT_CAPABILITY_DIGEST,
                )
                .unwrap(),
                PricingRuntimeCapabilityEvidence::new(
                    PRICING_SCHEMA_VERSION,
                    CAPABILITY_GENERATION,
                    CAPABILITY_DIGEST,
                )
                .unwrap(),
            ],
        )
        .unwrap()
    }

    fn snapshot(
        requested_model_id: &str,
        canonical_model_id: &str,
        multiplier: i64,
        official_hold_nano: i64,
        charged_hold_nano: i64,
    ) -> LegacyScalarAdmissionSnapshot {
        LegacyScalarAdmissionSnapshot::new(LegacyScalarAdmissionSnapshotInput {
            request_id: "shadow-request".into(),
            account_id: "acct".into(),
            provider: SnapshotProvider::Anthropic,
            requested_model_id: requested_model_id.into(),
            canonical_model_id: canonical_model_id.into(),
            alias_generation: 11,
            tariff_schedule_id: "anthropic-tariff-v1".into(),
            tariff_priced_ts: ADMISSION_TS - 1,
            admission_ts: ADMISSION_TS,
            payable_multiplier_bp: multiplier,
            official_hold_nano,
            charged_hold_nano,
            premium_modifiers: LegacyPremiumModifiers::AnthropicV1 {
                speed: SnapshotAnthropicSpeed::Standard,
                inference_geo: SnapshotAnthropicInferenceGeo::Global,
                inference_geo_basis_points: 10_000,
            },
        })
        .unwrap()
    }

    fn catalog(
        generation: i64,
        capability_generation: i64,
        capability_digest: &str,
    ) -> PricingCatalogSpec {
        PricingCatalogSpec {
            product_id: "main".into(),
            generation,
            schema_version: PRICING_SCHEMA_VERSION,
            capability_generation,
            capability_digest: capability_digest.into(),
            content_digest: format!("catalog-{generation}"),
            entries: vec![
                PricingCatalogEntrySpec {
                    provider_id: "anthropic".into(),
                    canonical_model_id: "claude-sonnet-4".into(),
                    enabled: true,
                },
                PricingCatalogEntrySpec {
                    provider_id: "anthropic".into(),
                    canonical_model_id: "claude-opus-4".into(),
                    enabled: true,
                },
            ],
        }
    }

    fn switches() -> ProviderSwitchSpec {
        ProviderSwitchSpec {
            generation: 5,
            schema_version: PRICING_SCHEMA_VERSION,
            capability_generation: CAPABILITY_GENERATION,
            capability_digest: CAPABILITY_DIGEST.into(),
            content_digest: "switches-5".into(),
            entries: vec![
                ProviderSwitchEntrySpec {
                    provider_id: "anthropic".into(),
                    scope: ProviderSwitchScope::Master,
                    catalog_generation: None,
                    enabled: true,
                },
                ProviderSwitchEntrySpec {
                    provider_id: "anthropic".into(),
                    scope: ProviderSwitchScope::Segment {
                        product_id: "main".into(),
                        segment: PolicySegment::B2b,
                    },
                    catalog_generation: Some(3),
                    enabled: true,
                },
            ],
        }
    }

    fn bundle() -> PricingReadBundle {
        let catalog = catalog(3, CAPABILITY_GENERATION, CAPABILITY_DIGEST);
        let switches = switches();
        PricingReadBundle {
            account_id: "acct".into(),
            account_multiplier_bp: 8_000,
            policy: PricingPolicySnapshot::Active(ActiveAccountPolicy {
                policy: AccountPolicySpec {
                    account_id: "acct".into(),
                    effective_version: 7,
                    policy_id: "b2b-policy".into(),
                    policy_version: 4,
                    source_policy_digest: "commerce-policy-4".into(),
                    owner_type: PolicyOwnerType::B2bClient,
                    owner_id: "client".into(),
                    account_class: AccountClass::B2b,
                    product_id: "main".into(),
                    schema_version: PRICING_SCHEMA_VERSION,
                    catalog_generation: 3,
                    switch_generation: 5,
                    content_digest: "effective-policy-7".into(),
                    replacement_locked: false,
                    rules: vec![
                        provider_rule("anthropic", 8_000),
                        model_rule("anthropic", "claude-opus-4", 6_000),
                    ],
                },
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

    fn work_for(snapshot: &LegacyScalarAdmissionSnapshot) -> PricingShadowWorkItem {
        PricingShadowWorkItem::new(snapshot, evidence(), ENQUEUED_TS).unwrap()
    }

    fn google_snapshot() -> LegacyScalarAdmissionSnapshot {
        LegacyScalarAdmissionSnapshot::new(LegacyScalarAdmissionSnapshotInput {
            request_id: "google-shadow-request".into(),
            account_id: "acct".into(),
            provider: SnapshotProvider::Google,
            requested_model_id: "gemini-3-flash-preview".into(),
            canonical_model_id: "gemini-3-flash-preview".into(),
            alias_generation: 1,
            tariff_schedule_id: "google/gemini-developer-api/2026-08-02".into(),
            tariff_priced_ts: ADMISSION_TS - 1,
            admission_ts: ADMISSION_TS,
            payable_multiplier_bp: 8_000,
            official_hold_nano: 1_000,
            charged_hold_nano: 800,
            premium_modifiers: LegacyPremiumModifiers::GeminiV1 {
                context_rate: SnapshotGeminiContextRate::ConservativeMaximum,
                search_billing: SnapshotGeminiSearchBilling::PerQuery,
                grounding_enabled: true,
                search_reserve_units: 32,
            },
        })
        .unwrap()
    }

    fn google_bundle() -> PricingReadBundle {
        let mut state = bundle();
        let google_catalog_entries = vec![PricingCatalogEntrySpec {
            provider_id: "google".into(),
            canonical_model_id: "gemini-3-flash-preview".into(),
            enabled: true,
        }];
        for catalog in [
            state.policy_catalog.as_mut().unwrap(),
            state.admission_catalog.as_mut().unwrap(),
        ] {
            catalog.entries = google_catalog_entries.clone();
        }
        let google_switch_entries = vec![
            ProviderSwitchEntrySpec {
                provider_id: "google".into(),
                scope: ProviderSwitchScope::Master,
                catalog_generation: None,
                enabled: true,
            },
            ProviderSwitchEntrySpec {
                provider_id: "google".into(),
                scope: ProviderSwitchScope::Segment {
                    product_id: "main".into(),
                    segment: PolicySegment::B2b,
                },
                catalog_generation: Some(3),
                enabled: true,
            },
        ];
        for switches in [
            state.policy_switches.as_mut().unwrap(),
            state.admission_switches.as_mut().unwrap(),
        ] {
            switches.entries = google_switch_entries.clone();
        }
        let PricingPolicySnapshot::Active(active) = &mut state.policy else {
            panic!("fixture must carry an active policy")
        };
        active.policy.rules = vec![provider_rule("google", 6_000)];
        state
    }

    #[test]
    fn work_item_rejects_early_timestamps_and_accepts_exact_funding_cap() {
        let valid = snapshot("claude-sonnet-4", "claude-sonnet-4", 8_000, 1_000, 800);
        assert!(PricingShadowWorkItem::new(&valid, evidence(), ADMISSION_TS - 1).is_err());
        assert!(PricingShadowWorkItem::new(&valid, evidence(), 0).is_err());

        let capped = snapshot("claude-sonnet-4", "claude-sonnet-4", 8_000, 1_000, 799);
        assert!(PricingShadowWorkItem::new(&capped, evidence(), ENQUEUED_TS).is_ok());

        let overstated = snapshot("claude-sonnet-4", "claude-sonnet-4", 8_000, 1_000, 801);
        assert!(PricingShadowWorkItem::new(&overstated, evidence(), ENQUEUED_TS).is_err());

        let half_up = snapshot("claude-sonnet-4", "claude-sonnet-4", 5_000, 3, 2);
        assert!(PricingShadowWorkItem::new(&half_up, evidence(), ENQUEUED_TS).is_ok());

        let work = work_for(&valid);
        assert_eq!(work.request_id(), "shadow-request");
        assert_eq!(work.account_id(), "acct");
        assert_eq!(work.enqueued_ts(), ENQUEUED_TS);
    }

    #[test]
    fn resolver_manifest_is_derived_from_canonical_registry_evidence() {
        let evidence = evidence();
        let manifest = RuntimePricingManifest::from_evidence(&evidence);
        assert_eq!(manifest.manifest_generation, evidence.manifest_generation());
        assert_eq!(manifest.manifest_digest, evidence.manifest_digest());
        assert_eq!(manifest.capabilities.len(), 2);
        assert_eq!(
            manifest.capabilities[0].capability_generation,
            CAPABILITY_GENERATION
        );
        assert_eq!(
            manifest.capabilities[1].capability_generation,
            NEXT_CAPABILITY_GENERATION
        );
    }

    #[test]
    fn builder_preserves_actual_alias_manifest_policy_and_both_lineages() {
        let snapshot = snapshot("claude-latest", "claude-sonnet-4", 8_000, 1_000, 800);
        let expected_manifest = evidence();
        let built = build_pricing_shadow_evaluation(
            PricingShadowWorkItem::new(&snapshot, expected_manifest.clone(), ENQUEUED_TS).unwrap(),
            PricingShadowEvaluationSource::Bundle(&bundle()),
            EVALUATED_TS,
            ShadowDiagnosticContext::empty(),
        )
        .unwrap();

        assert_eq!(built.actual().provider(), SnapshotProvider::Anthropic);
        assert_eq!(built.actual().requested_model_id(), "claude-latest");
        assert_eq!(built.actual().canonical_model_id(), "claude-sonnet-4");
        assert_eq!(
            built.runtime_manifest().manifest_digest(),
            expected_manifest.manifest_digest()
        );
        let PricingShadowEvaluationOutcome::Resolved(resolved) = built.outcome() else {
            panic!("expected resolved shadow outcome")
        };
        assert_eq!(resolved.observed_multiplier_bp, 8_000);
        assert_eq!(resolved.policy_hold_nano(), 800);
        assert_eq!(resolved.comparison(), PricingShadowComparison::Equal);
        assert_eq!(resolved.policy.target.version, 7);
        assert_eq!(resolved.policy_lineage.catalog.target.version, 3);
        assert_eq!(resolved.policy_lineage.switches.target.version, 5);
        assert_eq!(resolved.admission_lineage.catalog.target.version, 3);
        assert_eq!(resolved.admission_lineage.switches.target.version, 5);
        assert_eq!(resolved.rule.rule_id, "anthropic-provider");
    }

    #[test]
    fn google_actual_snapshot_resolves_through_the_same_target_shadow_contract() {
        let snapshot = google_snapshot();
        let built = build_pricing_shadow_evaluation(
            work_for(&snapshot),
            PricingShadowEvaluationSource::Bundle(&google_bundle()),
            EVALUATED_TS,
            ShadowDiagnosticContext::empty(),
        )
        .unwrap();

        assert_eq!(built.actual().provider(), SnapshotProvider::Google);
        let PricingShadowEvaluationOutcome::Resolved(resolved) = built.outcome() else {
            panic!("expected resolved Google shadow outcome")
        };
        assert_eq!(resolved.rule.rule_id, "google-provider");
        assert_eq!(resolved.policy_hold_nano(), 600);
        assert_eq!(resolved.comparison(), PricingShadowComparison::Different);
    }

    #[test]
    fn builder_preserves_dual_lineage_and_derives_different_exact_model_hold() {
        let snapshot = snapshot("claude-opus-latest", "claude-opus-4", 8_000, 1_000, 800);
        let mut state = bundle();
        state.admission_catalog = Some(catalog(
            4,
            NEXT_CAPABILITY_GENERATION,
            NEXT_CAPABILITY_DIGEST,
        ));
        let built = build_pricing_shadow_evaluation(
            work_for(&snapshot),
            PricingShadowEvaluationSource::Bundle(&state),
            EVALUATED_TS,
            ShadowDiagnosticContext::empty(),
        )
        .unwrap();

        let PricingShadowEvaluationOutcome::Resolved(resolved) = built.outcome() else {
            panic!("expected resolved shadow outcome")
        };
        assert_eq!(resolved.policy_lineage.catalog.target.version, 3);
        assert_eq!(resolved.admission_lineage.catalog.target.version, 4);
        assert_eq!(resolved.admission_lineage.switches.target.version, 5);
        assert_eq!(resolved.rule.rule_id, "anthropic-claude-opus-4");
        assert_eq!(resolved.policy_hold_nano(), 600);
        assert_eq!(resolved.comparison(), PricingShadowComparison::Different);
    }

    #[test]
    fn builder_distinguishes_policy_rejection_from_typed_read_errors() {
        let snapshot = snapshot("claude-sonnet-4", "claude-sonnet-4", 8_000, 1_000, 800);
        let mut unbound = bundle();
        unbound.policy = PricingPolicySnapshot::Unbound;
        unbound.policy_catalog = None;
        unbound.policy_switches = None;
        unbound.admission_catalog = None;
        unbound.admission_switches = None;
        let rejected = build_pricing_shadow_evaluation(
            work_for(&snapshot),
            PricingShadowEvaluationSource::Bundle(&unbound),
            EVALUATED_TS,
            ShadowDiagnosticContext::empty(),
        )
        .unwrap();
        assert!(matches!(
            rejected.outcome(),
            PricingShadowEvaluationOutcome::Rejected {
                reason: PricingShadowRejectionCode::NoPolicyBinding,
                observed_multiplier_bp: 8_000,
            }
        ));

        let mut malformed_policy = bundle();
        let PricingPolicySnapshot::Active(active) = &mut malformed_policy.policy else {
            unreachable!()
        };
        active.policy.account_id = "different-policy-account".into();
        let rejected = build_pricing_shadow_evaluation(
            work_for(&snapshot),
            PricingShadowEvaluationSource::Bundle(&malformed_policy),
            EVALUATED_TS,
            ShadowDiagnosticContext::empty(),
        )
        .unwrap();
        assert!(matches!(
            rejected.outcome(),
            PricingShadowEvaluationOutcome::Rejected {
                reason: PricingShadowRejectionCode::AccountMismatch,
                observed_multiplier_bp: 8_000,
            }
        ));

        for (failure, reason) in [
            (
                PricingShadowReadFailure::PricingReadFailed,
                PricingShadowReadErrorCode::PricingReadFailed,
            ),
            (
                PricingShadowReadFailure::EvaluationTimeout,
                PricingShadowReadErrorCode::EvaluationTimeout,
            ),
            (
                PricingShadowReadFailure::EvaluationCancelled,
                PricingShadowReadErrorCode::EvaluationCancelled,
            ),
        ] {
            let failed = build_pricing_shadow_evaluation(
                work_for(&snapshot),
                PricingShadowEvaluationSource::ReadFailure(failure),
                EVALUATED_TS,
                ShadowDiagnosticContext::empty(),
            )
            .unwrap();
            assert!(matches!(
                failed.outcome(),
                PricingShadowEvaluationOutcome::ReadError { reason: stored } if *stored == reason
            ));
        }
    }

    #[test]
    fn builder_fails_closed_on_timestamp_or_resolver_identity_drift() {
        let snapshot = snapshot("claude-sonnet-4", "claude-sonnet-4", 8_000, 1_000, 800);
        assert!(build_pricing_shadow_evaluation(
            work_for(&snapshot),
            PricingShadowEvaluationSource::Bundle(&bundle()),
            ENQUEUED_TS - 1,
            ShadowDiagnosticContext::empty(),
        )
        .is_err());

        let mut wrong_account = bundle();
        wrong_account.account_id = "different-account".into();
        assert!(build_pricing_shadow_evaluation(
            work_for(&snapshot),
            PricingShadowEvaluationSource::Bundle(&wrong_account),
            EVALUATED_TS,
            ShadowDiagnosticContext::empty(),
        )
        .is_err());

        let actual = ShadowActualSnapshotRef::from_snapshot(&snapshot).unwrap();
        let evidence = evidence();
        let manifest = RuntimePricingManifest::from_evidence(&evidence);
        let resolution = resolve_pricing(
            &bundle(),
            &PricingResolutionRequest {
                account_id: actual.account_id().into(),
                provider_id: actual.provider().as_str().into(),
                requested_model_id: actual.requested_model_id().into(),
                canonical_model_id: actual.canonical_model_id().into(),
            },
            &manifest,
        );
        let PricingResolution::Resolved(resolved) = resolution else {
            panic!("fixture did not resolve")
        };

        let mut changed = resolved.clone();
        changed.runtime_manifest_generation += 1;
        assert!(shadow_outcome(
            &actual,
            &evidence,
            8_000,
            PricingResolution::Resolved(changed)
        )
        .is_err());

        let mut changed = resolved.clone();
        changed.runtime_manifest_digest.push_str("-changed");
        assert!(shadow_outcome(
            &actual,
            &evidence,
            8_000,
            PricingResolution::Resolved(changed)
        )
        .is_err());

        for changed in [
            ResolvedPricingRule {
                provider_id: "openai".into(),
                ..resolved.clone()
            },
            ResolvedPricingRule {
                requested_model_id: "different-alias".into(),
                ..resolved.clone()
            },
            ResolvedPricingRule {
                canonical_model_id: "different-model".into(),
                ..resolved.clone()
            },
        ] {
            assert!(shadow_outcome(
                &actual,
                &evidence,
                8_000,
                PricingResolution::Resolved(changed)
            )
            .is_err());
        }
    }

    #[test]
    fn all_resolver_rejections_have_exact_shadow_codes() {
        use PricingDependencyKind::{Catalog, Switches};
        use PricingResolutionLineage::{Admission, Policy};
        use PricingResolutionRejection as Rejection;

        let reasons = vec![
            Rejection::InvalidRequest,
            Rejection::InvalidRuntimeManifest,
            Rejection::AccountMismatch,
            Rejection::NoPolicyBinding,
            Rejection::InactivePolicy,
            Rejection::MissingDependency {
                lineage: Policy,
                dependency: Catalog,
            },
            Rejection::MissingDependency {
                lineage: Policy,
                dependency: Switches,
            },
            Rejection::MissingDependency {
                lineage: Admission,
                dependency: Catalog,
            },
            Rejection::MissingDependency {
                lineage: Admission,
                dependency: Switches,
            },
            Rejection::PolicySchemaMismatch,
            Rejection::SchemaMismatch {
                lineage: Policy,
                dependency: Catalog,
            },
            Rejection::SchemaMismatch {
                lineage: Policy,
                dependency: Switches,
            },
            Rejection::SchemaMismatch {
                lineage: Admission,
                dependency: Catalog,
            },
            Rejection::SchemaMismatch {
                lineage: Admission,
                dependency: Switches,
            },
            Rejection::CatalogTargetMismatch { lineage: Policy },
            Rejection::CatalogTargetMismatch { lineage: Admission },
            Rejection::PolicySwitchTargetMismatch,
            Rejection::CapabilityNotInManifest {
                lineage: Policy,
                dependency: Catalog,
            },
            Rejection::CapabilityNotInManifest {
                lineage: Policy,
                dependency: Switches,
            },
            Rejection::CapabilityNotInManifest {
                lineage: Admission,
                dependency: Catalog,
            },
            Rejection::CapabilityNotInManifest {
                lineage: Admission,
                dependency: Switches,
            },
            Rejection::InvalidDependency {
                lineage: Policy,
                dependency: Catalog,
            },
            Rejection::InvalidDependency {
                lineage: Policy,
                dependency: Switches,
            },
            Rejection::InvalidDependency {
                lineage: Admission,
                dependency: Catalog,
            },
            Rejection::InvalidDependency {
                lineage: Admission,
                dependency: Switches,
            },
            Rejection::InvalidPolicyContract,
            Rejection::ModelNotInCatalog { lineage: Policy },
            Rejection::ModelNotInCatalog { lineage: Admission },
            Rejection::ModelDisabled { lineage: Policy },
            Rejection::ModelDisabled { lineage: Admission },
            Rejection::MissingMasterSwitch { lineage: Policy },
            Rejection::MissingMasterSwitch { lineage: Admission },
            Rejection::MasterSwitchDisabled { lineage: Policy },
            Rejection::MasterSwitchDisabled { lineage: Admission },
            Rejection::MissingScopedSwitch { lineage: Policy },
            Rejection::MissingScopedSwitch { lineage: Admission },
            Rejection::PolicyScopedSwitchTargetMismatch,
            Rejection::AdmissionScopedSwitchTargetMismatch,
            Rejection::ScopedSwitchDisabled { lineage: Policy },
            Rejection::ScopedSwitchDisabled { lineage: Admission },
            Rejection::MissingRule,
        ];

        assert_eq!(reasons.len(), 41);
        for reason in reasons {
            assert_eq!(shadow_rejection_code(reason).as_str(), reason.code());
        }
    }
}
