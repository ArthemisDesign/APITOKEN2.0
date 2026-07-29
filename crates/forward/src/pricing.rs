//! Dormant, side-effect-free pricing-policy resolver.
//!
//! This module deliberately has no runtime caller yet. It consumes one transactionally read
//! registry bundle plus identities fixed by the provider runtime, and either returns one exact
//! rule or a typed fail-closed reason. It does not read a database, inspect HTTP input, calculate
//! token costs, reserve money, emit telemetry, or change admission.

use registry::pricing::{
    validate_account_policy, AccountClass, AccountPolicyBindingSpec, AccountPolicyRuleSpec,
    PolicyRuleScope, PricingPolicySnapshot, PricingReadBundle, ProviderSwitchScope, VersionTarget,
    PRICING_SCHEMA_VERSION,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePricingCapability {
    pub pricing_schema_version: i64,
    pub capability_generation: i64,
    pub capability_digest: String,
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
    pub runtime: RuntimePricingCapability,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPricingRule {
    pub product_id: String,
    pub account_class: AccountClass,
    pub account_multiplier_bp: i64,
    pub provider_id: String,
    pub requested_model_id: String,
    pub canonical_model_id: String,
    pub capability_generation: i64,
    pub capability_digest: String,
    pub catalog_target: VersionTarget,
    pub switch_target: VersionTarget,
    pub policy_target: VersionTarget,
    pub policy_id: String,
    pub policy_version: i64,
    pub source_policy_digest: String,
    pub binding: AccountPolicyBindingSpec,
    pub rule: AccountPolicyRuleSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingResolutionRejection {
    InvalidRequest,
    AccountMismatch,
    NoPolicyBinding,
    InactivePolicy,
    MissingActiveCatalog,
    MissingActiveSwitches,
    SchemaMismatch,
    CatalogTargetMismatch,
    SwitchTargetMismatch,
    CapabilityMismatch,
    InvalidStoredContract,
    ModelNotInCatalog,
    ModelDisabled,
    MissingMasterSwitch,
    MasterSwitchDisabled,
    MissingScopedSwitch,
    ScopedSwitchTargetMismatch,
    ScopedSwitchDisabled,
    MissingRule,
}

impl PricingResolutionRejection {
    /// Stable, low-cardinality telemetry value. It intentionally contains no account/model IDs.
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::AccountMismatch => "account_mismatch",
            Self::NoPolicyBinding => "no_policy_binding",
            Self::InactivePolicy => "inactive_policy",
            Self::MissingActiveCatalog => "missing_active_catalog",
            Self::MissingActiveSwitches => "missing_active_switches",
            Self::SchemaMismatch => "schema_mismatch",
            Self::CatalogTargetMismatch => "catalog_target_mismatch",
            Self::SwitchTargetMismatch => "switch_target_mismatch",
            Self::CapabilityMismatch => "capability_mismatch",
            Self::InvalidStoredContract => "invalid_stored_contract",
            Self::ModelNotInCatalog => "model_not_in_catalog",
            Self::ModelDisabled => "model_disabled",
            Self::MissingMasterSwitch => "missing_master_switch",
            Self::MasterSwitchDisabled => "master_switch_disabled",
            Self::MissingScopedSwitch => "missing_scoped_switch",
            Self::ScopedSwitchTargetMismatch => "scoped_switch_target_mismatch",
            Self::ScopedSwitchDisabled => "scoped_switch_disabled",
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

/// Resolve an immutable pricing rule from one coherent registry snapshot.
///
/// The function is intentionally stricter than the future shadow caller: every malformed or torn
/// dependency becomes a typed rejection. A later runtime integration may observe this result, but
/// must not turn it into live admission or charging behavior without a separate rollout.
pub fn resolve_pricing(
    bundle: &PricingReadBundle,
    request: &PricingResolutionRequest,
) -> PricingResolution {
    if !valid_id(&request.account_id)
        || !valid_id(&request.provider_id)
        || !valid_id(&request.requested_model_id)
        || !valid_id(&request.canonical_model_id)
        || !valid_id(&request.runtime.capability_digest)
        || request.runtime.pricing_schema_version <= 0
        || request.runtime.capability_generation <= 0
    {
        return rejected(PricingResolutionRejection::InvalidRequest);
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

    let Some(catalog) = bundle.catalog.as_ref() else {
        return rejected(PricingResolutionRejection::MissingActiveCatalog);
    };
    let Some(switches) = bundle.switches.as_ref() else {
        return rejected(PricingResolutionRejection::MissingActiveSwitches);
    };

    if request.runtime.pricing_schema_version != PRICING_SCHEMA_VERSION
        || active.policy.schema_version != request.runtime.pricing_schema_version
        || catalog.schema_version != request.runtime.pricing_schema_version
        || switches.schema_version != request.runtime.pricing_schema_version
    {
        return rejected(PricingResolutionRejection::SchemaMismatch);
    }
    if catalog.product_id != active.policy.product_id
        || catalog.generation != active.policy.catalog_generation
    {
        return rejected(PricingResolutionRejection::CatalogTargetMismatch);
    }
    if switches.generation != active.policy.switch_generation {
        return rejected(PricingResolutionRejection::SwitchTargetMismatch);
    }
    if catalog.capability_generation != request.runtime.capability_generation
        || switches.capability_generation != request.runtime.capability_generation
        || catalog.capability_digest != request.runtime.capability_digest
        || switches.capability_digest != request.runtime.capability_digest
    {
        return rejected(PricingResolutionRejection::CapabilityMismatch);
    }

    let Some(master) = switches.entries.iter().find(|entry| {
        entry.provider_id == request.provider_id
            && matches!(entry.scope, ProviderSwitchScope::Master)
    }) else {
        return rejected(PricingResolutionRejection::MissingMasterSwitch);
    };
    if !master.enabled {
        return rejected(PricingResolutionRejection::MasterSwitchDisabled);
    }

    let required_scope = required_scope(active.policy.account_class, &active.policy.product_id);
    let Some(scoped) = switches
        .entries
        .iter()
        .find(|entry| entry.provider_id == request.provider_id && entry.scope == required_scope)
    else {
        return rejected(PricingResolutionRejection::MissingScopedSwitch);
    };
    if scoped.catalog_generation != Some(catalog.generation) {
        return rejected(PricingResolutionRejection::ScopedSwitchTargetMismatch);
    }
    if !scoped.enabled {
        return rejected(PricingResolutionRejection::ScopedSwitchDisabled);
    }

    let Some(model) = catalog.entries.iter().find(|entry| {
        entry.provider_id == request.provider_id
            && entry.canonical_model_id == request.canonical_model_id
    }) else {
        return rejected(PricingResolutionRejection::ModelNotInCatalog);
    };
    if !model.enabled {
        return rejected(PricingResolutionRejection::ModelDisabled);
    }

    if validate_account_policy(
        &active.policy,
        catalog,
        switches,
        Some(bundle.account_multiplier_bp),
    )
    .is_err()
    {
        return rejected(PricingResolutionRejection::InvalidStoredContract);
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
        capability_generation: request.runtime.capability_generation,
        capability_digest: request.runtime.capability_digest.clone(),
        catalog_target: catalog.target(),
        switch_target: switches.target(),
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
            catalog: Some(catalog),
            switches: Some(switches),
        }
    }

    fn request(provider_id: &str, model_id: &str) -> PricingResolutionRequest {
        PricingResolutionRequest {
            account_id: "acct".to_owned(),
            provider_id: provider_id.to_owned(),
            requested_model_id: model_id.to_owned(),
            canonical_model_id: model_id.to_owned(),
            runtime: RuntimePricingCapability {
                pricing_schema_version: PRICING_SCHEMA_VERSION,
                capability_generation: CAPABILITY_GENERATION,
                capability_digest: CAPABILITY_DIGEST.to_owned(),
            },
        }
    }

    fn rejection(
        bundle: &PricingReadBundle,
        request: &PricingResolutionRequest,
    ) -> PricingResolutionRejection {
        match resolve_pricing(bundle, request) {
            PricingResolution::Rejected(reason) => reason,
            PricingResolution::Resolved(rule) => panic!("unexpected resolution: {rule:?}"),
        }
    }

    #[test]
    fn exact_model_rule_replaces_provider_rule_without_stacking() {
        let bundle = bundle();
        let resolved = resolve_pricing(&bundle, &request("anthropic", "claude-opus-4"));
        let PricingResolution::Resolved(resolved) = resolved else {
            panic!("expected exact model resolution, got {resolved:?}");
        };
        assert_eq!(resolved.rule.rule_id, "anthropic-claude-opus-4");
        assert_eq!(resolved.rule.payable_multiplier_bp, 6_000);
        assert_eq!(resolved.catalog_target.version, 3);
        assert_eq!(resolved.switch_target.version, 5);
        assert_eq!(resolved.policy_target.version, 7);
    }

    #[test]
    fn provider_rule_is_the_only_fallback() {
        let bundle = bundle();
        let resolved = resolve_pricing(&bundle, &request("anthropic", "claude-sonnet-4"));
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
    fn torn_heads_fail_closed_at_each_choreography_step() {
        let initial = bundle();
        assert!(matches!(
            resolve_pricing(&initial, &request("anthropic", "claude-sonnet-4")),
            PricingResolution::Resolved(_)
        ));

        let mut catalog_advanced = initial.clone();
        catalog_advanced.catalog.as_mut().unwrap().generation = 4;
        catalog_advanced.catalog.as_mut().unwrap().content_digest = "catalog-4".to_owned();
        assert_eq!(
            rejection(&catalog_advanced, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::CatalogTargetMismatch
        );

        let mut switches_only_advanced = initial.clone();
        switches_only_advanced.switches.as_mut().unwrap().generation = 6;
        assert_eq!(
            rejection(
                &switches_only_advanced,
                &request("anthropic", "claude-sonnet-4")
            ),
            PricingResolutionRejection::SwitchTargetMismatch
        );

        let mut switches_advanced = catalog_advanced.clone();
        switches_advanced.switches.as_mut().unwrap().generation = 6;
        switches_advanced.switches.as_mut().unwrap().content_digest = "switches-6".to_owned();
        for entry in &mut switches_advanced.switches.as_mut().unwrap().entries {
            if !matches!(entry.scope, ProviderSwitchScope::Master) {
                entry.catalog_generation = Some(4);
            }
        }
        assert_eq!(
            rejection(&switches_advanced, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::CatalogTargetMismatch
        );

        let mut policy_advanced = switches_advanced;
        let PricingPolicySnapshot::Active(active) = &mut policy_advanced.policy else {
            unreachable!()
        };
        active.policy.catalog_generation = 4;
        active.policy.switch_generation = 6;
        active.policy.effective_version = 8;
        active.policy.content_digest = "effective-policy-8".to_owned();
        assert!(matches!(
            resolve_pricing(&policy_advanced, &request("anthropic", "claude-sonnet-4")),
            PricingResolution::Resolved(_)
        ));
    }

    #[test]
    fn capability_and_schema_drift_fail_closed() {
        let bundle = bundle();
        let mut capability_drift = request("anthropic", "claude-sonnet-4");
        capability_drift.runtime.capability_generation += 1;
        assert_eq!(
            rejection(&bundle, &capability_drift),
            PricingResolutionRejection::CapabilityMismatch
        );

        let mut schema_drift = request("anthropic", "claude-sonnet-4");
        schema_drift.runtime.pricing_schema_version += 1;
        assert_eq!(
            rejection(&bundle, &schema_drift),
            PricingResolutionRejection::SchemaMismatch
        );
    }

    #[test]
    fn catalog_presence_and_enablement_are_distinct() {
        let current = bundle();
        assert_eq!(
            rejection(&current, &request("anthropic", "claude-unknown")),
            PricingResolutionRejection::ModelNotInCatalog
        );

        let mut disabled = current;
        disabled.catalog.as_mut().unwrap().entries[0].enabled = false;
        assert_eq!(
            rejection(&disabled, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::ModelDisabled
        );

        let mut no_active_catalog = bundle();
        no_active_catalog.catalog = None;
        assert_eq!(
            rejection(&no_active_catalog, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::MissingActiveCatalog
        );

        let mut no_active_switches = bundle();
        no_active_switches.switches = None;
        assert_eq!(
            rejection(
                &no_active_switches,
                &request("anthropic", "claude-sonnet-4")
            ),
            PricingResolutionRejection::MissingActiveSwitches
        );
    }

    #[test]
    fn master_and_segment_switches_fail_closed_independently() {
        let mut master_disabled = bundle();
        master_disabled
            .switches
            .as_mut()
            .unwrap()
            .entries
            .iter_mut()
            .find(|entry| {
                entry.provider_id == "anthropic"
                    && matches!(entry.scope, ProviderSwitchScope::Master)
            })
            .unwrap()
            .enabled = false;
        master_disabled.catalog.as_mut().unwrap().entries[0].enabled = false;
        assert_eq!(
            rejection(&master_disabled, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::MasterSwitchDisabled
        );

        let mut segment_disabled = bundle();
        segment_disabled
            .switches
            .as_mut()
            .unwrap()
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
            rejection(&segment_disabled, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::ScopedSwitchDisabled
        );

        let mut master_missing = bundle();
        master_missing
            .switches
            .as_mut()
            .unwrap()
            .entries
            .retain(|entry| {
                !(entry.provider_id == "anthropic"
                    && matches!(entry.scope, ProviderSwitchScope::Master))
            });
        assert_eq!(
            rejection(&master_missing, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::MissingMasterSwitch
        );

        let mut segment_missing = bundle();
        segment_missing
            .switches
            .as_mut()
            .unwrap()
            .entries
            .retain(|entry| {
                !(entry.provider_id == "anthropic"
                    && matches!(
                        entry.scope,
                        ProviderSwitchScope::Segment {
                            segment: PolicySegment::B2b,
                            ..
                        }
                    ))
            });
        assert_eq!(
            rejection(&segment_missing, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::MissingScopedSwitch
        );

        let mut segment_torn = bundle();
        segment_torn
            .switches
            .as_mut()
            .unwrap()
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
            .catalog_generation = Some(2);
        assert_eq!(
            rejection(&segment_torn, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::ScopedSwitchTargetMismatch
        );
    }

    #[test]
    fn fixed_provider_plane_cannot_borrow_another_provider_rule() {
        let bundle = bundle();
        assert_eq!(
            rejection(&bundle, &request("google", "claude-sonnet-4")),
            PricingResolutionRejection::MissingMasterSwitch
        );
    }

    #[test]
    fn alias_and_canonical_request_share_one_policy_identity() {
        let bundle = bundle();
        let canonical = request("openai", "gpt-5.6-sol");
        let mut alias = canonical.clone();
        alias.requested_model_id = "gpt-5.6".to_owned();

        let PricingResolution::Resolved(canonical_resolved) = resolve_pricing(&bundle, &canonical)
        else {
            panic!("canonical request did not resolve")
        };
        let PricingResolution::Resolved(alias_resolved) = resolve_pricing(&bundle, &alias) else {
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
    fn legacy_openkeys_scalar_is_fenced_by_the_same_bundle() {
        let mut current = bundle();
        current.account_multiplier_bp = 7_300;
        current.catalog.as_mut().unwrap().product_id = "openkeys".to_owned();
        for entry in &mut current.switches.as_mut().unwrap().entries {
            if !matches!(entry.scope, ProviderSwitchScope::Master) {
                entry.scope = ProviderSwitchScope::Product {
                    product_id: "openkeys".to_owned(),
                };
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

        let PricingResolution::Resolved(resolved) =
            resolve_pricing(&current, &request("anthropic", "claude-sonnet-4"))
        else {
            panic!("coherent legacy OpenKeys bundle did not resolve")
        };
        assert_eq!(resolved.account_multiplier_bp, 7_300);
        assert_eq!(resolved.rule.payable_multiplier_bp, 7_300);

        current.account_multiplier_bp = 7_400;
        assert_eq!(
            rejection(&current, &request("anthropic", "claude-sonnet-4")),
            PricingResolutionRejection::InvalidStoredContract
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
