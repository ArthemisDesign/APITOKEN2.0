//! Typed producer contract for the immutable pricing-release/funding-v2 authority.
//!
//! This module deliberately exposes prepare/read data only. A prepared policy or release does not
//! create the singleton release head and therefore cannot change live admission or customer money.

use super::{require_id, AccountClass, PolicyOwnerType};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PRICING_RELEASE_SCHEMA_VERSION: i64 = 2;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingModeV2 {
    Balance,
    MeterOnly,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingReleaseKindV2 {
    Target,
    Recovery,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "scope", deny_unknown_fields)]
pub enum PricingReleaseRuleScopeV2 {
    Global,
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
pub struct PricingReleasePolicyRuleV2 {
    pub rule_id: String,
    pub rule_digest: String,
    pub scope: PricingReleaseRuleScopeV2,
    pub discount_bps: i64,
    pub payable_multiplier_bp: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleasePolicyV2 {
    pub policy_id: String,
    pub policy_version: i64,
    pub owner_type: PolicyOwnerType,
    pub owner_id: String,
    pub account_class: AccountClass,
    pub product_id: Option<String>,
    pub billing_mode: BillingModeV2,
    pub schema_version: i64,
    pub capability_generation: i64,
    pub capability_digest: String,
    pub catalog_generation: Option<i64>,
    pub catalog_digest: Option<String>,
    pub switch_generation: Option<i64>,
    pub switch_digest: Option<String>,
    pub content_digest: String,
    pub rules: Vec<PricingReleasePolicyRuleV2>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseAssignmentV2 {
    pub account_id: String,
    pub account_class: AccountClass,
    pub policy_id: String,
    pub policy_version: i64,
    pub policy_digest: String,
    pub billing_mode: BillingModeV2,
    pub funding_generation: Option<i64>,
    pub purpose: Option<String>,
    pub responsible: Option<String>,
    pub assignment_digest: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseV2 {
    pub generation: i64,
    pub release_kind: PricingReleaseKindV2,
    pub schema_version: i64,
    pub capability_generation: i64,
    pub capability_digest: String,
    pub main_catalog_generation: i64,
    pub main_catalog_digest: String,
    pub openkeys_catalog_generation: i64,
    pub openkeys_catalog_digest: String,
    pub switch_generation: i64,
    pub switch_digest: String,
    pub inventory_digest: String,
    pub policy_manifest_digest: String,
    pub assignment_manifest_digest: String,
    pub funding_manifest_digest: String,
    pub minimum_runtime_schema_version: i64,
    pub content_digest: String,
    pub assignments: Vec<PricingReleaseAssignmentV2>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseRecoveryLinkV2 {
    pub target_generation: i64,
    pub target_digest: String,
    pub recovery_generation: i64,
    pub recovery_digest: String,
    pub link_digest: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseHeadV2 {
    pub active_generation: i64,
    pub active_digest: String,
    pub head_version: i64,
    pub updated_ts: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseInventoryAccountV2 {
    pub account_id: String,
    pub status: String,
    pub multiplier_bp: i64,
    pub balance_nano: i64,
    pub reserved_nano: i64,
    pub spent_nano: i64,
    pub funding_generation: Option<i64>,
    pub funding_head_version: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseInventoryPageV2 {
    pub accounts: Vec<PricingReleaseInventoryAccountV2>,
    pub next_after_account_id: Option<String>,
}

impl BillingModeV2 {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Balance => "balance",
            Self::MeterOnly => "meter_only",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self> {
        match value {
            "balance" => Ok(Self::Balance),
            "meter_only" => Ok(Self::MeterOnly),
            _ => bail!("unknown pricing release billing mode {value:?}"),
        }
    }
}

impl PricingReleaseKindV2 {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Target => "target",
            Self::Recovery => "recovery",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self> {
        match value {
            "target" => Ok(Self::Target),
            "recovery" => Ok(Self::Recovery),
            _ => bail!("unknown pricing release kind {value:?}"),
        }
    }
}

impl PricingReleaseRuleScopeV2 {
    pub(crate) fn db_parts(&self) -> (&'static str, Option<&str>, Option<&str>) {
        match self {
            Self::Global => ("global", None, None),
            Self::Provider { provider_id } => ("provider", Some(provider_id), None),
            Self::Model {
                provider_id,
                canonical_model_id,
            } => ("model", Some(provider_id), Some(canonical_model_id)),
        }
    }

    pub(crate) fn from_db(
        scope: &str,
        provider_id: Option<String>,
        canonical_model_id: Option<String>,
    ) -> Result<Self> {
        match (scope, provider_id, canonical_model_id) {
            ("global", None, None) => Ok(Self::Global),
            ("provider", Some(provider_id), None) => Ok(Self::Provider { provider_id }),
            ("model", Some(provider_id), Some(canonical_model_id)) => Ok(Self::Model {
                provider_id,
                canonical_model_id,
            }),
            _ => bail!("invalid stored pricing release rule scope"),
        }
    }

    fn identity(&self) -> (&'static str, &str, &str) {
        match self {
            Self::Global => ("global", "", ""),
            Self::Provider { provider_id } => ("provider", provider_id, ""),
            Self::Model {
                provider_id,
                canonical_model_id,
            } => ("model", provider_id, canonical_model_id),
        }
    }
}

pub(crate) fn normalize_pricing_release_policy_v2(
    policy: &PricingReleasePolicyV2,
) -> PricingReleasePolicyV2 {
    let mut normalized = policy.clone();
    normalized.rules.sort_by(|left, right| {
        (left.scope.identity(), &left.rule_id).cmp(&(right.scope.identity(), &right.rule_id))
    });
    normalized
}

pub(crate) fn normalize_pricing_release_v2(release: &PricingReleaseV2) -> PricingReleaseV2 {
    let mut normalized = release.clone();
    normalized
        .assignments
        .sort_by(|left, right| left.account_id.cmp(&right.account_id));
    normalized
}

fn expected_account_class(owner_type: PolicyOwnerType) -> AccountClass {
    match owner_type {
        PolicyOwnerType::GlobalB2c => AccountClass::B2c,
        PolicyOwnerType::B2bClient => AccountClass::B2b,
        PolicyOwnerType::OpenKeys => AccountClass::OpenKeys,
        PolicyOwnerType::Service => AccountClass::Service,
    }
}

pub fn validate_pricing_release_policy_v2(policy: &PricingReleasePolicyV2) -> Result<()> {
    require_id("pricing release policy id", &policy.policy_id)?;
    require_id("pricing release policy owner id", &policy.owner_id)?;
    require_id(
        "pricing release policy capability digest",
        &policy.capability_digest,
    )?;
    require_id(
        "pricing release policy content digest",
        &policy.content_digest,
    )?;
    if policy.policy_version <= 0 || policy.capability_generation <= 0 {
        bail!("pricing release policy versions and capability generation must be positive");
    }
    if policy.schema_version != PRICING_RELEASE_SCHEMA_VERSION {
        bail!("unsupported pricing release policy schema version");
    }
    if policy.account_class != expected_account_class(policy.owner_type) {
        bail!("pricing release policy owner and account class do not match");
    }

    let service = policy.account_class == AccountClass::Service;
    if service {
        if policy.billing_mode != BillingModeV2::MeterOnly
            || policy.product_id.is_some()
            || policy.catalog_generation.is_some()
            || policy.catalog_digest.is_some()
            || policy.switch_generation.is_some()
            || policy.switch_digest.is_some()
            || !policy.rules.is_empty()
        {
            bail!("service policy must be catalog-free meter_only without pricing rules");
        }
        return Ok(());
    }
    if policy.billing_mode != BillingModeV2::Balance {
        bail!("customer pricing release policy must use balance billing");
    }
    require_id(
        "pricing release policy product id",
        policy.product_id.as_deref().unwrap_or(""),
    )?;
    if policy.catalog_generation.is_none_or(|value| value <= 0)
        || policy.switch_generation.is_none_or(|value| value <= 0)
    {
        bail!("customer pricing release policy requires positive catalog and switch generations");
    }
    require_id(
        "pricing release policy catalog digest",
        policy.catalog_digest.as_deref().unwrap_or(""),
    )?;
    require_id(
        "pricing release policy switch digest",
        policy.switch_digest.as_deref().unwrap_or(""),
    )?;
    if policy.rules.is_empty() {
        bail!("customer pricing release policy must contain at least one rule");
    }

    let mut ids = BTreeSet::new();
    let mut scopes = BTreeSet::new();
    let mut global_count = 0_usize;
    for rule in &policy.rules {
        require_id("pricing release rule id", &rule.rule_id)?;
        require_id("pricing release rule digest", &rule.rule_digest)?;
        if !ids.insert(rule.rule_id.as_str()) || !scopes.insert(rule.scope.identity()) {
            bail!("pricing release policy contains a duplicate rule id or scope");
        }
        let (scope, provider_id, canonical_model_id) = rule.scope.db_parts();
        if let Some(provider_id) = provider_id {
            require_id("pricing release rule provider id", provider_id)?;
        }
        if let Some(canonical_model_id) = canonical_model_id {
            require_id(
                "pricing release rule canonical model id",
                canonical_model_id,
            )?;
        }
        global_count += usize::from(scope == "global");
        if !(0..=10_000).contains(&rule.discount_bps)
            || rule.payable_multiplier_bp != 10_000 - rule.discount_bps
        {
            bail!("pricing release rule has inconsistent basis points");
        }
        if policy.account_class == AccountClass::OpenKeys && rule.discount_bps != 0 {
            bail!("OpenKeys pricing release policy must remain exactly 1:1");
        }
        if policy.account_class == AccountClass::B2c
            && scope == "global"
            && rule.discount_bps != 5_000
        {
            bail!("B2C global pricing release discount must be exactly 5000 basis points");
        }
    }
    match policy.account_class {
        AccountClass::B2c if global_count != 1 => {
            bail!("B2C pricing release policy requires exactly one global rule")
        }
        AccountClass::B2b if global_count != 0 => {
            bail!("B2B pricing release policy cannot inherit a global B2C rule")
        }
        AccountClass::OpenKeys if global_count != 1 => {
            bail!("OpenKeys pricing release policy requires one canonical global 1:1 rule")
        }
        _ => {}
    }
    Ok(())
}

fn validate_assignment(assignment: &PricingReleaseAssignmentV2) -> Result<()> {
    require_id(
        "pricing release assignment account id",
        &assignment.account_id,
    )?;
    require_id(
        "pricing release assignment policy id",
        &assignment.policy_id,
    )?;
    require_id(
        "pricing release assignment policy digest",
        &assignment.policy_digest,
    )?;
    require_id(
        "pricing release assignment digest",
        &assignment.assignment_digest,
    )?;
    if assignment.policy_version <= 0 {
        bail!("pricing release assignment policy version must be positive");
    }
    if assignment.account_class == AccountClass::Service {
        if assignment.billing_mode != BillingModeV2::MeterOnly
            || assignment.funding_generation.is_some()
        {
            bail!("service assignment must use meter_only without funding generation");
        }
        require_id(
            "service assignment purpose",
            assignment.purpose.as_deref().unwrap_or(""),
        )?;
        require_id(
            "service assignment responsible",
            assignment.responsible.as_deref().unwrap_or(""),
        )?;
    } else if assignment.billing_mode != BillingModeV2::Balance
        || assignment
            .funding_generation
            .is_none_or(|generation| generation <= 0)
        || assignment.purpose.is_some()
        || assignment.responsible.is_some()
    {
        bail!("customer assignment requires balance billing and one funding generation");
    }
    Ok(())
}

pub fn validate_pricing_release_v2(release: &PricingReleaseV2) -> Result<()> {
    if release.generation <= 0
        || release.capability_generation <= 0
        || release.main_catalog_generation <= 0
        || release.openkeys_catalog_generation <= 0
        || release.switch_generation <= 0
    {
        bail!("pricing release generations must be positive");
    }
    if release.schema_version != PRICING_RELEASE_SCHEMA_VERSION
        || release.minimum_runtime_schema_version < PRICING_RELEASE_SCHEMA_VERSION
    {
        bail!("pricing release requires schema/runtime version 2 or newer");
    }
    for (label, value) in [
        (
            "pricing release capability digest",
            release.capability_digest.as_str(),
        ),
        (
            "pricing release main catalog digest",
            release.main_catalog_digest.as_str(),
        ),
        (
            "pricing release OpenKeys catalog digest",
            release.openkeys_catalog_digest.as_str(),
        ),
        (
            "pricing release switch digest",
            release.switch_digest.as_str(),
        ),
        (
            "pricing release inventory digest",
            release.inventory_digest.as_str(),
        ),
        (
            "pricing release policy manifest digest",
            release.policy_manifest_digest.as_str(),
        ),
        (
            "pricing release assignment manifest digest",
            release.assignment_manifest_digest.as_str(),
        ),
        (
            "pricing release funding manifest digest",
            release.funding_manifest_digest.as_str(),
        ),
        (
            "pricing release content digest",
            release.content_digest.as_str(),
        ),
    ] {
        require_id(label, value)?;
    }
    if release.assignments.is_empty() {
        bail!("pricing release must assign the full active account inventory");
    }
    let mut accounts = BTreeSet::new();
    for assignment in &release.assignments {
        validate_assignment(assignment)?;
        if !accounts.insert(assignment.account_id.as_str()) {
            bail!("pricing release contains a duplicate account assignment");
        }
    }
    Ok(())
}

pub fn validate_pricing_release_recovery_link_v2(
    link: &PricingReleaseRecoveryLinkV2,
) -> Result<()> {
    if link.target_generation <= 0 || link.recovery_generation <= link.target_generation {
        bail!("pricing recovery generation must be newer than its positive target generation");
    }
    require_id("pricing recovery target digest", &link.target_digest)?;
    require_id("pricing recovery release digest", &link.recovery_digest)?;
    require_id("pricing recovery link digest", &link.link_digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b2c_policy() -> PricingReleasePolicyV2 {
        PricingReleasePolicyV2 {
            policy_id: "b2c-global".into(),
            policy_version: 1,
            owner_type: PolicyOwnerType::GlobalB2c,
            owner_id: "global".into(),
            account_class: AccountClass::B2c,
            product_id: Some("main".into()),
            billing_mode: BillingModeV2::Balance,
            schema_version: 2,
            capability_generation: 1,
            capability_digest: "capability".into(),
            catalog_generation: Some(1),
            catalog_digest: Some("main-catalog".into()),
            switch_generation: Some(1),
            switch_digest: Some("switches".into()),
            content_digest: "b2c-policy".into(),
            rules: vec![PricingReleasePolicyRuleV2 {
                rule_id: "global-50".into(),
                rule_digest: "global-50-digest".into(),
                scope: PricingReleaseRuleScopeV2::Global,
                discount_bps: 5_000,
                payable_multiplier_bp: 5_000,
            }],
        }
    }

    #[test]
    fn policy_validation_enforces_precedence_classes_without_track_semantics() {
        validate_pricing_release_policy_v2(&b2c_policy()).unwrap();

        let mut b2b = b2c_policy();
        b2b.policy_id = "b2b".into();
        b2b.owner_type = PolicyOwnerType::B2bClient;
        b2b.owner_id = "client".into();
        b2b.account_class = AccountClass::B2b;
        assert!(validate_pricing_release_policy_v2(&b2b)
            .unwrap_err()
            .to_string()
            .contains("cannot inherit"));

        let mut openkeys = b2c_policy();
        openkeys.policy_id = "openkeys".into();
        openkeys.owner_type = PolicyOwnerType::OpenKeys;
        openkeys.owner_id = "openkeys".into();
        openkeys.account_class = AccountClass::OpenKeys;
        openkeys.product_id = Some("openkeys".into());
        openkeys.rules[0].discount_bps = 1;
        openkeys.rules[0].payable_multiplier_bp = 9_999;
        assert!(validate_pricing_release_policy_v2(&openkeys)
            .unwrap_err()
            .to_string()
            .contains("exactly 1:1"));
    }

    #[test]
    fn service_policy_is_meter_only_and_rule_free() {
        let policy = PricingReleasePolicyV2 {
            policy_id: "service".into(),
            policy_version: 1,
            owner_type: PolicyOwnerType::Service,
            owner_id: "worker".into(),
            account_class: AccountClass::Service,
            product_id: None,
            billing_mode: BillingModeV2::MeterOnly,
            schema_version: 2,
            capability_generation: 1,
            capability_digest: "capability".into(),
            catalog_generation: None,
            catalog_digest: None,
            switch_generation: None,
            switch_digest: None,
            content_digest: "service-policy".into(),
            rules: Vec::new(),
        };
        validate_pricing_release_policy_v2(&policy).unwrap();
    }

    #[test]
    fn rule_scope_json_is_strict_and_pins_the_consumer_shape() {
        let rule = PricingReleasePolicyRuleV2 {
            rule_id: "gemini-provider".into(),
            rule_digest: "gemini-provider-digest".into(),
            scope: PricingReleaseRuleScopeV2::Provider {
                provider_id: "google".into(),
            },
            discount_bps: 6_000,
            payable_multiplier_bp: 4_000,
        };
        assert_eq!(
            serde_json::to_value(&rule).unwrap(),
            serde_json::json!({
                "rule_id": "gemini-provider",
                "rule_digest": "gemini-provider-digest",
                "scope": {"scope": "provider", "provider_id": "google"},
                "discount_bps": 6_000,
                "payable_multiplier_bp": 4_000,
            })
        );
        assert!(
            serde_json::from_value::<PricingReleasePolicyRuleV2>(serde_json::json!({
                "rule_id": "gemini-provider",
                "rule_digest": "gemini-provider-digest",
                "scope": {"scope": "provider", "provider_id": "google", "unknown": true},
                "discount_bps": 6_000,
                "payable_multiplier_bp": 4_000,
            }))
            .is_err()
        );
    }
}
