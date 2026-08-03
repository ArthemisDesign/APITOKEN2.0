//! Typed producer contract for the immutable pricing-release/funding-v2 authority.
//!
//! This module deliberately exposes prepare/read data only. A prepared policy or release does not
//! create the singleton release head and therefore cannot change live admission or customer money.

use super::{require_id, AccountClass, PolicyOwnerType};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;

pub const PRICING_RELEASE_SCHEMA_VERSION: i64 = 2;
pub const FUNDING_SCHEMA_VERSION_V2: i64 = 2;
pub const PRICING_RELEASE_RUNTIME_DIGEST_V2: &str =
    "sha256:v2:afd0bae49dbd4ab9ac1a1eace5a147052800fb3ad3545f59e11d27732ba69137";

const REQUEST_SNAPSHOT_DIGEST_DOMAIN_V2: &[u8] = b"apitoken:pricing-release-request-snapshot:v2\0";

/// A new legacy PostgreSQL reserve reached its money mutation after the global release head
/// became visible. Runtime callers must resolve that head and retry through release-v2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyPricingPathClosedV2;

impl fmt::Display for LegacyPricingPathClosedV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("legacy pricing reserve is closed by the active release head")
    }
}

impl std::error::Error for LegacyPricingPathClosedV2 {}

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

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingReleaseActivationKindV2 {
    Cutover,
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
pub struct PricingReleaseAssignmentExtensionMemberV2 {
    pub release_generation: i64,
    pub assignment: PricingReleaseAssignmentV2,
    pub extension_digest: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseAssignmentExtensionV2 {
    pub provisioning_head_generation: i64,
    pub provisioning_head_digest: String,
    pub provisioning_head_version: i64,
    pub paired_recovery_generation: Option<i64>,
    pub paired_recovery_digest: Option<String>,
    pub extension_group_digest: String,
    pub members: Vec<PricingReleaseAssignmentExtensionMemberV2>,
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

/// Immutable release lineage needed to provision an account after the global cutover.
///
/// Assignment manifests are intentionally omitted: a provisioning caller only needs the exact
/// capability/catalog/switch lineage for a new policy and the immutable release identity for its
/// assignment extension. Existing accounts remain represented by the release's full inventory.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseProvisioningReleaseV2 {
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
    pub funding_manifest_digest: String,
    pub minimum_runtime_schema_version: i64,
    pub content_digest: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseProvisioningActivationV2 {
    pub activation_id: i64,
    pub activation_kind: PricingReleaseActivationKindV2,
    pub evidence_digest: String,
    pub activated_ts: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseProvisioningRecoveryV2 {
    pub release: PricingReleaseProvisioningReleaseV2,
    pub recovery_link: PricingReleaseRecoveryLinkV2,
}

/// One coherent post-cutover provisioning snapshot.
///
/// An active target carries the exact recovery selected by its activation evidence. An active
/// recovery has no further confirmed pair, so `paired_recovery` is absent.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseProvisioningContextV2 {
    pub head: PricingReleaseHeadV2,
    pub activation: PricingReleaseProvisioningActivationV2,
    pub active_release: PricingReleaseProvisioningReleaseV2,
    pub paired_recovery: Option<PricingReleaseProvisioningRecoveryV2>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingReleaseHeadExpectationV2 {
    Absent,
    Exact(PricingReleaseHeadV2),
}

/// Fresh combined Stage 8 identity supplied by the protected commerce control plane.
///
/// The combined digest is an audit link to commerce authority; it is not trusted as engine
/// authority. Activation recomputes the three mutable engine subdigests inside the head CAS
/// transaction and persists this exact identity only when every comparison succeeds.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseActivationEvidenceV2 {
    pub evidence_digest: String,
    pub target_generation: i64,
    pub target_digest: String,
    pub recovery_generation: i64,
    pub recovery_digest: String,
    pub engine_inventory_digest: String,
    pub funding_digest: String,
    pub shadow_digest: String,
    pub runtime_floor_digest: String,
    pub legacy_inflight_count: i64,
    pub engine_captured_ts: i64,
    pub observed_ts: i64,
    pub valid_until_ts: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseActivationRequestV2 {
    pub activation_kind: PricingReleaseActivationKindV2,
    pub expectation: PricingReleaseHeadExpectationV2,
    pub evidence: PricingReleaseActivationEvidenceV2,
    pub operator_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingReleaseActivationReceiptV2 {
    pub activation_id: i64,
    pub activation_kind: PricingReleaseActivationKindV2,
    pub from_generation: Option<i64>,
    pub from_digest: Option<String>,
    pub expected_head_version: i64,
    pub head: PricingReleaseHeadV2,
    pub evidence_digest: String,
    pub operator_id: String,
    pub reason: String,
    pub activated_ts: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingReleaseActivationRejectionV2 {
    Invalid {
        reason: String,
    },
    MissingDependency {
        dependency: String,
    },
    CasMismatch {
        actual: Option<PricingReleaseHeadV2>,
    },
    EvidenceStale {
        now_ts: i64,
        observed_ts: i64,
        valid_until_ts: i64,
    },
    EvidenceConflict {
        evidence_digest: String,
    },
    ReleaseLineageDrift {
        reason: String,
    },
    InventoryDrift {
        expected_digest: String,
        actual_digest: String,
    },
    FundingDrift {
        expected_digest: String,
        actual_digest: String,
    },
    FundingInvariantDrift {
        account_count: i64,
    },
    RuntimeFloorDrift {
        expected_digest: String,
        actual_digest: String,
    },
    RuntimeIncompatible {
        live_instances: i64,
        compatible_instances: i64,
    },
    AuthorityDrift {
        changed_rows: i64,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingReleaseActivationOutcomeV2 {
    Applied(PricingReleaseActivationReceiptV2),
    Unchanged(PricingReleaseActivationReceiptV2),
    Rejected(PricingReleaseActivationRejectionV2),
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

/// One exact immutable global-head decision for a provider/model admission.
///
/// Readers build this value in one repeatable-read transaction. The reserve writer re-resolves
/// the same identity under its request/account locks before persisting money, so a concurrent
/// target -> recovery transition is a typed stale decision rather than a torn snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingReleaseResolutionV2 {
    pub head: PricingReleaseHeadV2,
    pub release_schema_version: i64,
    pub release_digest: String,
    pub assignment: PricingReleaseAssignmentV2,
    pub policy: PricingReleasePolicyV2,
    pub rule: Option<PricingReleasePolicyRuleV2>,
}

impl PricingReleaseResolutionV2 {
    pub fn billing_mode(&self) -> BillingModeV2 {
        self.assignment.billing_mode
    }

    pub fn payable_multiplier_bp(&self) -> Option<i64> {
        self.rule.as_ref().map(|rule| rule.payable_multiplier_bp)
    }
}

/// Provider-owned official reserve quote consumed by the release-v2 writer.
///
/// The public constructor accepts only an already validated legacy-provider snapshot. That keeps
/// canonical model/tariff/modifier provenance in the existing Anthropic/OpenAI/Google builders;
/// the release writer owns only immutable policy resolution and customer charging.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingReleaseQuoteV2 {
    request_id: String,
    account_id: String,
    provider_id: String,
    canonical_model_id: String,
    tariff_schedule_id: String,
    tariff_priced_ts: i64,
    official_hold_nano: i64,
    official_cost_json: serde_json::Value,
    admission_ts: i64,
}

impl PricingReleaseQuoteV2 {
    pub fn from_legacy_snapshot(snapshot: &super::LegacyScalarAdmissionSnapshot) -> Result<Self> {
        snapshot.validate()?;
        let official_cost_json = serde_json::json!({
            "alias_generation": snapshot.alias_generation(),
            "premium_modifiers": snapshot.premium_modifiers(),
            "requested_model_id": snapshot.requested_model_id(),
        });
        let quote = Self {
            request_id: snapshot.request_id().to_owned(),
            account_id: snapshot.account_id().to_owned(),
            provider_id: snapshot.provider().as_str().to_owned(),
            canonical_model_id: snapshot.canonical_model_id().to_owned(),
            tariff_schedule_id: snapshot.tariff_schedule_id().to_owned(),
            tariff_priced_ts: snapshot.tariff_priced_ts(),
            official_hold_nano: snapshot.official_hold_nano(),
            official_cost_json,
            admission_ts: snapshot.admission_ts(),
        };
        quote.validate()?;
        Ok(quote)
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn canonical_model_id(&self) -> &str {
        &self.canonical_model_id
    }

    pub fn tariff_schedule_id(&self) -> &str {
        &self.tariff_schedule_id
    }

    pub fn tariff_priced_ts(&self) -> i64 {
        self.tariff_priced_ts
    }

    pub fn official_hold_nano(&self) -> i64 {
        self.official_hold_nano
    }

    pub fn official_cost_json(&self) -> &serde_json::Value {
        &self.official_cost_json
    }

    pub fn admission_ts(&self) -> i64 {
        self.admission_ts
    }

    pub fn validate(&self) -> Result<()> {
        for (label, value) in [
            ("release quote request id", self.request_id.as_str()),
            ("release quote account id", self.account_id.as_str()),
            ("release quote provider id", self.provider_id.as_str()),
            (
                "release quote canonical model id",
                self.canonical_model_id.as_str(),
            ),
            (
                "release quote tariff schedule id",
                self.tariff_schedule_id.as_str(),
            ),
        ] {
            require_id(label, value)?;
        }
        if !matches!(self.provider_id.as_str(), "anthropic" | "openai" | "google") {
            bail!("release quote provider is outside the fixed runtime plane");
        }
        if self.tariff_priced_ts <= 0 || self.admission_ts <= 0 || self.official_hold_nano < 0 {
            bail!("release quote timestamps/official hold are invalid");
        }
        if !self.official_cost_json.is_object() {
            bail!("release quote official cost must be a JSON object");
        }
        let encoded = serde_json::to_vec(&self.official_cost_json)
            .context("encode release quote official cost")?;
        if encoded.len() > 16 * 1024 {
            bail!("release quote official cost exceeds the storage bound");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingRequestSnapshotV2 {
    pub request_id: String,
    pub account_id: String,
    pub release_schema_version: i64,
    pub release_generation: i64,
    pub release_digest: String,
    pub assignment_digest: String,
    pub account_class: AccountClass,
    pub policy_id: String,
    pub policy_version: i64,
    pub policy_digest: String,
    pub billing_mode: BillingModeV2,
    pub funding_generation: Option<i64>,
    pub provider_id: String,
    pub canonical_model_id: String,
    pub rule: Option<PricingReleasePolicyRuleV2>,
    pub tariff_schedule_id: String,
    pub tariff_priced_ts: i64,
    pub official_hold_nano: i64,
    pub charged_hold_nano: i64,
    pub official_cost_json: serde_json::Value,
    pub snapshot_digest: String,
    pub created_ts: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingReleaseReserveReceiptV2 {
    pub balance_after_reserve_nano: Option<i64>,
    pub snapshot: PricingRequestSnapshotV2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingReleaseReserveConflictV2 {
    ActiveReleaseChanged,
    ReservationIdentity,
    ExistingReservationWithoutReleaseSnapshot,
    SnapshotPayload,
    TerminalReservation,
    ExpiredIdempotencyWindow,
    AdmissionTimestampInFuture,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PricingReleaseReserveOutcomeV2 {
    NoActiveRelease,
    Inserted(PricingReleaseReserveReceiptV2),
    Unchanged(PricingReleaseReserveReceiptV2),
    NotReserved,
    Conflict(PricingReleaseReserveConflictV2),
    AbortedBeforeCommit,
}

fn digest_bytes(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn digest_string(hasher: &mut Sha256, value: &str) {
    digest_bytes(hasher, value.as_bytes());
}

fn digest_optional_string(hasher: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            hasher.update([1]);
            digest_string(hasher, value);
        }
        None => hasher.update([0]),
    }
}

fn digest_optional_i64(hasher: &mut Sha256, value: Option<i64>) {
    match value {
        Some(value) => {
            hasher.update([1]);
            hasher.update(value.to_be_bytes());
        }
        None => hasher.update([0]),
    }
}

fn finish_digest(hasher: Sha256) -> String {
    let bytes = hasher.finalize();
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("sha256:v2:{hex}")
}

pub(crate) fn pricing_request_snapshot_digest_v2(
    snapshot: &PricingRequestSnapshotV2,
) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(REQUEST_SNAPSHOT_DIGEST_DOMAIN_V2);
    digest_string(&mut hasher, &snapshot.request_id);
    digest_string(&mut hasher, &snapshot.account_id);
    hasher.update(snapshot.release_schema_version.to_be_bytes());
    hasher.update(snapshot.release_generation.to_be_bytes());
    digest_string(&mut hasher, &snapshot.release_digest);
    digest_string(&mut hasher, &snapshot.assignment_digest);
    digest_string(&mut hasher, snapshot.account_class.as_str());
    digest_string(&mut hasher, &snapshot.policy_id);
    hasher.update(snapshot.policy_version.to_be_bytes());
    digest_string(&mut hasher, &snapshot.policy_digest);
    digest_string(&mut hasher, snapshot.billing_mode.as_str());
    digest_optional_i64(&mut hasher, snapshot.funding_generation);
    digest_string(&mut hasher, &snapshot.provider_id);
    digest_string(&mut hasher, &snapshot.canonical_model_id);
    if let Some(rule) = snapshot.rule.as_ref() {
        let (scope, provider_id, canonical_model_id) = rule.scope.db_parts();
        hasher.update([1]);
        digest_string(&mut hasher, &rule.rule_id);
        digest_string(&mut hasher, &rule.rule_digest);
        digest_string(&mut hasher, scope);
        digest_optional_string(&mut hasher, provider_id);
        digest_optional_string(&mut hasher, canonical_model_id);
        hasher.update(rule.discount_bps.to_be_bytes());
        hasher.update(rule.payable_multiplier_bp.to_be_bytes());
    } else {
        hasher.update([0]);
    }
    digest_string(&mut hasher, &snapshot.tariff_schedule_id);
    hasher.update(snapshot.tariff_priced_ts.to_be_bytes());
    hasher.update(snapshot.official_hold_nano.to_be_bytes());
    hasher.update(snapshot.charged_hold_nano.to_be_bytes());
    let official_cost = serde_json::to_vec(&snapshot.official_cost_json)
        .context("encode release request official cost for digest")?;
    digest_bytes(&mut hasher, &official_cost);
    hasher.update(snapshot.created_ts.to_be_bytes());
    Ok(finish_digest(hasher))
}

pub(crate) fn build_pricing_request_snapshot_v2(
    resolution: &PricingReleaseResolutionV2,
    quote: &PricingReleaseQuoteV2,
    created_ts: i64,
) -> Result<PricingRequestSnapshotV2> {
    quote.validate()?;
    if created_ts <= 0
        || resolution.release_schema_version < PRICING_RELEASE_SCHEMA_VERSION
        || resolution.assignment.account_id != quote.account_id
        || resolution.assignment.policy_id != resolution.policy.policy_id
        || resolution.assignment.policy_version != resolution.policy.policy_version
        || resolution.assignment.policy_digest != resolution.policy.content_digest
        || resolution.assignment.account_class != resolution.policy.account_class
        || resolution.assignment.billing_mode != resolution.policy.billing_mode
    {
        bail!("release request resolution identity is inconsistent");
    }
    let charged_hold_nano = match resolution.assignment.billing_mode {
        BillingModeV2::Balance => {
            let multiplier = resolution
                .rule
                .as_ref()
                .context("balance release resolution lacks a pricing rule")?
                .payable_multiplier_bp;
            i64::try_from(
                i128::from(quote.official_hold_nano)
                    .checked_mul(i128::from(multiplier))
                    .context("release request hold multiplication overflow")?
                    / 10_000,
            )
            .context("release request charged hold is outside i64")?
        }
        BillingModeV2::MeterOnly => {
            if resolution.rule.is_some() {
                bail!("meter-only release resolution cannot carry a pricing rule");
            }
            0
        }
    };
    let mut snapshot = PricingRequestSnapshotV2 {
        request_id: quote.request_id.clone(),
        account_id: quote.account_id.clone(),
        release_schema_version: resolution.release_schema_version,
        release_generation: resolution.head.active_generation,
        release_digest: resolution.release_digest.clone(),
        assignment_digest: resolution.assignment.assignment_digest.clone(),
        account_class: resolution.assignment.account_class,
        policy_id: resolution.policy.policy_id.clone(),
        policy_version: resolution.policy.policy_version,
        policy_digest: resolution.policy.content_digest.clone(),
        billing_mode: resolution.assignment.billing_mode,
        funding_generation: resolution.assignment.funding_generation,
        provider_id: quote.provider_id.clone(),
        canonical_model_id: quote.canonical_model_id.clone(),
        rule: resolution.rule.clone(),
        tariff_schedule_id: quote.tariff_schedule_id.clone(),
        tariff_priced_ts: quote.tariff_priced_ts,
        official_hold_nano: quote.official_hold_nano,
        charged_hold_nano,
        official_cost_json: quote.official_cost_json.clone(),
        snapshot_digest: String::new(),
        created_ts,
    };
    snapshot.snapshot_digest = pricing_request_snapshot_digest_v2(&snapshot)?;
    Ok(snapshot)
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

impl PricingReleaseActivationKindV2 {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cutover => "cutover",
            Self::Recovery => "recovery",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self> {
        match value {
            "cutover" => Ok(Self::Cutover),
            "recovery" => Ok(Self::Recovery),
            _ => bail!("unknown pricing release activation kind {value:?}"),
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

pub(crate) fn normalize_pricing_release_assignment_extension_v2(
    extension: &PricingReleaseAssignmentExtensionV2,
) -> PricingReleaseAssignmentExtensionV2 {
    let mut normalized = extension.clone();
    normalized
        .members
        .sort_by_key(|member| member.release_generation);
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

pub fn validate_pricing_release_assignment_extension_v2(
    extension: &PricingReleaseAssignmentExtensionV2,
) -> Result<()> {
    if extension.provisioning_head_generation <= 0 || extension.provisioning_head_version <= 0 {
        bail!("pricing assignment extension head identity must be positive");
    }
    require_id(
        "pricing assignment extension head digest",
        &extension.provisioning_head_digest,
    )?;
    require_id(
        "pricing assignment extension group digest",
        &extension.extension_group_digest,
    )?;
    let paired = match (
        extension.paired_recovery_generation,
        extension.paired_recovery_digest.as_deref(),
    ) {
        (None, None) => None,
        (Some(generation), Some(digest)) if generation > extension.provisioning_head_generation => {
            require_id("pricing assignment extension recovery digest", digest)?;
            Some(generation)
        }
        _ => bail!("pricing assignment extension has an invalid recovery identity"),
    };
    let expected_members = usize::from(paired.is_some()) + 1;
    if extension.members.len() != expected_members {
        bail!("pricing assignment extension must contain the exact active/recovery pair");
    }

    let mut release_generations = BTreeSet::new();
    let mut extension_digests = BTreeSet::new();
    let mut first: Option<&PricingReleaseAssignmentV2> = None;
    for member in &extension.members {
        validate_assignment(&member.assignment)?;
        require_id(
            "pricing assignment extension member digest",
            &member.extension_digest,
        )?;
        if !release_generations.insert(member.release_generation)
            || !extension_digests.insert(member.extension_digest.as_str())
        {
            bail!("pricing assignment extension contains duplicate member identity");
        }
        if member.release_generation != extension.provisioning_head_generation
            && Some(member.release_generation) != paired
        {
            bail!("pricing assignment extension member is outside the active/recovery pair");
        }
        if let Some(first) = first {
            if member.assignment.account_id != first.account_id
                || member.assignment.account_class != first.account_class
                || member.assignment.policy_id != first.policy_id
                || member.assignment.policy_version != first.policy_version
                || member.assignment.policy_digest != first.policy_digest
                || member.assignment.billing_mode != first.billing_mode
                || member.assignment.funding_generation != first.funding_generation
                || member.assignment.purpose != first.purpose
                || member.assignment.responsible != first.responsible
            {
                bail!("pricing assignment extension pair has inconsistent assignment semantics");
            }
        } else {
            first = Some(&member.assignment);
        }
    }
    if !release_generations.contains(&extension.provisioning_head_generation)
        || paired.is_some_and(|generation| !release_generations.contains(&generation))
    {
        bail!("pricing assignment extension does not cover the exact active/recovery pair");
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
        bail!("pricing release must assign the full engine account inventory");
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

pub fn validate_pricing_release_activation_v2(
    request: &PricingReleaseActivationRequestV2,
) -> Result<()> {
    let evidence = &request.evidence;
    for (label, value) in [
        (
            "pricing activation evidence digest",
            evidence.evidence_digest.as_str(),
        ),
        (
            "pricing activation target digest",
            evidence.target_digest.as_str(),
        ),
        (
            "pricing activation recovery digest",
            evidence.recovery_digest.as_str(),
        ),
        (
            "pricing activation inventory digest",
            evidence.engine_inventory_digest.as_str(),
        ),
        (
            "pricing activation funding digest",
            evidence.funding_digest.as_str(),
        ),
        (
            "pricing activation shadow digest",
            evidence.shadow_digest.as_str(),
        ),
        (
            "pricing activation runtime floor digest",
            evidence.runtime_floor_digest.as_str(),
        ),
        ("pricing activation operator", request.operator_id.as_str()),
        ("pricing activation reason", request.reason.as_str()),
    ] {
        require_id(label, value)?;
    }
    for (label, value) in [
        (
            "pricing activation evidence digest",
            evidence.evidence_digest.as_str(),
        ),
        (
            "pricing activation inventory digest",
            evidence.engine_inventory_digest.as_str(),
        ),
        (
            "pricing activation funding digest",
            evidence.funding_digest.as_str(),
        ),
        (
            "pricing activation shadow digest",
            evidence.shadow_digest.as_str(),
        ),
        (
            "pricing activation runtime floor digest",
            evidence.runtime_floor_digest.as_str(),
        ),
    ] {
        let Some(hex) = value.strip_prefix("sha256:v2:") else {
            bail!("{label} must be a canonical sha256:v2 digest");
        };
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("{label} must be a canonical sha256:v2 digest");
        }
    }
    if request.operator_id.len() > 200 || request.reason.len() > 2_000 {
        bail!("pricing activation operator or reason exceeds its storage bound");
    }
    if request
        .operator_id
        .chars()
        .chain(request.reason.chars())
        .any(char::is_control)
    {
        bail!("pricing activation operator and reason cannot contain control characters");
    }
    if evidence.target_generation <= 0
        || evidence.recovery_generation <= evidence.target_generation
        || evidence.legacy_inflight_count < 0
        || evidence.engine_captured_ts <= 0
        || evidence.observed_ts <= 0
        || evidence.engine_captured_ts > evidence.observed_ts.saturating_add(5)
        || evidence
            .observed_ts
            .saturating_sub(evidence.engine_captured_ts)
            > 120
        || evidence.valid_until_ts <= evidence.observed_ts
        || evidence.valid_until_ts - evidence.observed_ts > 300
    {
        bail!("pricing activation evidence has invalid generations, counts, or TTL");
    }
    match (&request.activation_kind, &request.expectation) {
        (PricingReleaseActivationKindV2::Cutover, PricingReleaseHeadExpectationV2::Absent) => {}
        (
            PricingReleaseActivationKindV2::Recovery,
            PricingReleaseHeadExpectationV2::Exact(head),
        ) if head.active_generation == evidence.target_generation
            && head.active_digest == evidence.target_digest
            && head.head_version > 0
            && head.head_version < i64::MAX
            && head.updated_ts > 0 => {}
        (PricingReleaseActivationKindV2::Cutover, _) => {
            bail!("pricing cutover requires an absent release head")
        }
        (PricingReleaseActivationKindV2::Recovery, _) => {
            bail!("pricing recovery requires the exact target release head")
        }
    }
    Ok(())
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

    #[test]
    fn assignment_extension_requires_one_semantic_active_recovery_pair() {
        let assignment = |digest: &str| PricingReleaseAssignmentV2 {
            account_id: "post-cutover-account".into(),
            account_class: AccountClass::B2c,
            policy_id: "b2c-global".into(),
            policy_version: 1,
            policy_digest: "b2c-policy".into(),
            billing_mode: BillingModeV2::Balance,
            funding_generation: Some(1),
            purpose: None,
            responsible: None,
            assignment_digest: digest.into(),
        };
        let mut extension = PricingReleaseAssignmentExtensionV2 {
            provisioning_head_generation: 101,
            provisioning_head_digest: "target-release".into(),
            provisioning_head_version: 1,
            paired_recovery_generation: Some(102),
            paired_recovery_digest: Some("recovery-release".into()),
            extension_group_digest: "extension-group".into(),
            members: vec![
                PricingReleaseAssignmentExtensionMemberV2 {
                    release_generation: 101,
                    assignment: assignment("target-assignment"),
                    extension_digest: "target-extension".into(),
                },
                PricingReleaseAssignmentExtensionMemberV2 {
                    release_generation: 102,
                    assignment: assignment("recovery-assignment"),
                    extension_digest: "recovery-extension".into(),
                },
            ],
        };
        validate_pricing_release_assignment_extension_v2(&extension).unwrap();

        extension.members[1].assignment.policy_digest = "different-policy".into();
        assert!(validate_pricing_release_assignment_extension_v2(&extension)
            .unwrap_err()
            .to_string()
            .contains("inconsistent assignment semantics"));
        extension.members[1].assignment.policy_digest = "b2c-policy".into();
        extension.members.pop();
        assert!(validate_pricing_release_assignment_extension_v2(&extension)
            .unwrap_err()
            .to_string()
            .contains("exact active/recovery pair"));
    }

    #[test]
    fn activation_contract_requires_bounded_evidence_and_exact_cas_shape() {
        let digest = |byte: char| format!("sha256:v2:{}", byte.to_string().repeat(64));
        let request = PricingReleaseActivationRequestV2 {
            activation_kind: PricingReleaseActivationKindV2::Cutover,
            expectation: PricingReleaseHeadExpectationV2::Absent,
            evidence: PricingReleaseActivationEvidenceV2 {
                evidence_digest: digest('a'),
                target_generation: 10,
                target_digest: "target-release".into(),
                recovery_generation: 11,
                recovery_digest: "recovery-release".into(),
                engine_inventory_digest: digest('b'),
                funding_digest: digest('c'),
                shadow_digest: digest('d'),
                runtime_floor_digest: digest('e'),
                legacy_inflight_count: 7,
                engine_captured_ts: 1_000,
                observed_ts: 1_100,
                valid_until_ts: 1_400,
            },
            operator_id: "pricing-worker".into(),
            reason: "activate exact target".into(),
        };
        validate_pricing_release_activation_v2(&request).unwrap();

        let mut recovery = request.clone();
        recovery.activation_kind = PricingReleaseActivationKindV2::Recovery;
        recovery.expectation = PricingReleaseHeadExpectationV2::Exact(PricingReleaseHeadV2 {
            active_generation: 10,
            active_digest: "target-release".into(),
            head_version: 1,
            updated_ts: 1_200,
        });
        validate_pricing_release_activation_v2(&recovery).unwrap();

        let mut malformed = request.clone();
        malformed.evidence.runtime_floor_digest = "not-canonical".into();
        assert!(validate_pricing_release_activation_v2(&malformed).is_err());
        malformed = request.clone();
        malformed.evidence.valid_until_ts += 1;
        assert!(validate_pricing_release_activation_v2(&malformed).is_err());
        malformed = request;
        malformed.activation_kind = PricingReleaseActivationKindV2::Recovery;
        assert!(validate_pricing_release_activation_v2(&malformed).is_err());
    }
}
