//! Canonical immutable identity for one strict policy-priced reservation.

use super::snapshots::{
    require_bounded_id, CanonicalDigestEncoder, LegacyPremiumModifiers,
    LegacyScalarIdempotencyWindowError, SnapshotProvider, LEGACY_SCALAR_REPLAY_MAX_AGE_SECS,
};
use super::{AccountClass, PolicyRuleScope, PricingMode, RuleOrigin};
use anyhow::{bail, Context, Result};

pub const POLICY_ADMISSION_SNAPSHOT_SCHEMA_VERSION: i64 = 1;
const ID_MAX_BYTES: usize = 512;
const TARIFF_ID_MAX_BYTES: usize = 256;
const POLICY_SNAPSHOT_DIGEST_DOMAIN: &[u8] = b"claude-api/pricing/policy-admission-snapshot/v1\0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyAdmissionSnapshotInput {
    pub request_id: String,
    pub account_id: String,
    pub provider: SnapshotProvider,
    pub product_id: String,
    pub account_class: AccountClass,
    pub requested_model_id: String,
    pub canonical_model_id: String,
    pub alias_generation: i64,
    pub rule_id: String,
    pub rule_digest: String,
    pub rule_scope: PolicyRuleScope,
    pub pricing_mode: PricingMode,
    pub rule_origin: RuleOrigin,
    pub discount_bps: Option<i64>,
    pub payable_multiplier_bp: i64,
    pub policy_id: String,
    pub policy_version: i64,
    pub effective_policy_version: i64,
    pub source_policy_digest: String,
    pub policy_digest: String,
    pub policy_catalog_generation: i64,
    pub policy_switch_generation: i64,
    pub admission_catalog_generation: i64,
    pub admission_catalog_digest: String,
    pub admission_switch_generation: i64,
    pub admission_switch_digest: String,
    pub runtime_manifest_generation: i64,
    pub runtime_manifest_digest: String,
    pub tariff_schedule_id: String,
    pub tariff_priced_ts: i64,
    pub admission_ts: i64,
    pub official_hold_nano: i64,
    pub charged_hold_nano: i64,
    pub track_eligible: bool,
    pub retention_eligible: bool,
    pub commission_eligible: bool,
    pub premium_modifiers: LegacyPremiumModifiers,
}

/// Every pricing, catalog, tariff and funding-eligibility decision frozen before strict reserve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyAdmissionSnapshot {
    pub(crate) schema_version: i64,
    pub(crate) request_id: String,
    pub(crate) account_id: String,
    pub(crate) provider: SnapshotProvider,
    pub(crate) product_id: String,
    pub(crate) account_class: AccountClass,
    pub(crate) requested_model_id: String,
    pub(crate) canonical_model_id: String,
    pub(crate) alias_generation: i64,
    pub(crate) rule_id: String,
    pub(crate) rule_digest: String,
    pub(crate) rule_scope: PolicyRuleScope,
    pub(crate) pricing_mode: PricingMode,
    pub(crate) rule_origin: RuleOrigin,
    pub(crate) discount_bps: Option<i64>,
    pub(crate) payable_multiplier_bp: i64,
    pub(crate) policy_id: String,
    pub(crate) policy_version: i64,
    pub(crate) effective_policy_version: i64,
    pub(crate) source_policy_digest: String,
    pub(crate) policy_digest: String,
    pub(crate) policy_catalog_generation: i64,
    pub(crate) policy_switch_generation: i64,
    pub(crate) admission_catalog_generation: i64,
    pub(crate) admission_catalog_digest: String,
    pub(crate) admission_switch_generation: i64,
    pub(crate) admission_switch_digest: String,
    pub(crate) runtime_manifest_generation: i64,
    pub(crate) runtime_manifest_digest: String,
    pub(crate) tariff_schedule_id: String,
    pub(crate) tariff_priced_ts: i64,
    pub(crate) admission_ts: i64,
    pub(crate) official_hold_nano: i64,
    pub(crate) charged_hold_nano: i64,
    pub(crate) track_eligible: bool,
    pub(crate) retention_eligible: bool,
    pub(crate) commission_eligible: bool,
    pub(crate) premium_modifiers: LegacyPremiumModifiers,
    snapshot_digest: String,
}

impl PolicyAdmissionSnapshot {
    pub fn new(input: PolicyAdmissionSnapshotInput) -> Result<Self> {
        validate_input(&input)?;
        let mut snapshot = Self {
            schema_version: POLICY_ADMISSION_SNAPSHOT_SCHEMA_VERSION,
            request_id: input.request_id,
            account_id: input.account_id,
            provider: input.provider,
            product_id: input.product_id,
            account_class: input.account_class,
            requested_model_id: input.requested_model_id,
            canonical_model_id: input.canonical_model_id,
            alias_generation: input.alias_generation,
            rule_id: input.rule_id,
            rule_digest: input.rule_digest,
            rule_scope: input.rule_scope,
            pricing_mode: input.pricing_mode,
            rule_origin: input.rule_origin,
            discount_bps: input.discount_bps,
            payable_multiplier_bp: input.payable_multiplier_bp,
            policy_id: input.policy_id,
            policy_version: input.policy_version,
            effective_policy_version: input.effective_policy_version,
            source_policy_digest: input.source_policy_digest,
            policy_digest: input.policy_digest,
            policy_catalog_generation: input.policy_catalog_generation,
            policy_switch_generation: input.policy_switch_generation,
            admission_catalog_generation: input.admission_catalog_generation,
            admission_catalog_digest: input.admission_catalog_digest,
            admission_switch_generation: input.admission_switch_generation,
            admission_switch_digest: input.admission_switch_digest,
            runtime_manifest_generation: input.runtime_manifest_generation,
            runtime_manifest_digest: input.runtime_manifest_digest,
            tariff_schedule_id: input.tariff_schedule_id,
            tariff_priced_ts: input.tariff_priced_ts,
            admission_ts: input.admission_ts,
            official_hold_nano: input.official_hold_nano,
            charged_hold_nano: input.charged_hold_nano,
            track_eligible: input.track_eligible,
            retention_eligible: input.retention_eligible,
            commission_eligible: input.commission_eligible,
            premium_modifiers: input.premium_modifiers,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.compute_digest();
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != POLICY_ADMISSION_SNAPSHOT_SCHEMA_VERSION {
            bail!("unsupported policy admission snapshot schema version");
        }
        validate_input(&self.as_input())?;
        if self.snapshot_digest != self.compute_digest() {
            bail!("policy admission snapshot digest does not match its typed payload");
        }
        Ok(())
    }

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

    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub const fn provider(&self) -> SnapshotProvider {
        self.provider
    }
    pub fn product_id(&self) -> &str {
        &self.product_id
    }
    pub const fn account_class(&self) -> AccountClass {
        self.account_class
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
    pub fn rule_id(&self) -> &str {
        &self.rule_id
    }
    pub fn rule_digest(&self) -> &str {
        &self.rule_digest
    }
    pub const fn rule_scope(&self) -> &PolicyRuleScope {
        &self.rule_scope
    }
    pub const fn pricing_mode(&self) -> PricingMode {
        self.pricing_mode
    }
    pub const fn payable_multiplier_bp(&self) -> i64 {
        self.payable_multiplier_bp
    }
    pub fn policy_id(&self) -> &str {
        &self.policy_id
    }
    pub const fn policy_version(&self) -> i64 {
        self.policy_version
    }
    pub const fn effective_policy_version(&self) -> i64 {
        self.effective_policy_version
    }
    pub fn source_policy_digest(&self) -> &str {
        &self.source_policy_digest
    }
    pub fn policy_digest(&self) -> &str {
        &self.policy_digest
    }
    pub const fn policy_catalog_generation(&self) -> i64 {
        self.policy_catalog_generation
    }
    pub const fn policy_switch_generation(&self) -> i64 {
        self.policy_switch_generation
    }
    pub const fn admission_catalog_generation(&self) -> i64 {
        self.admission_catalog_generation
    }
    pub fn admission_catalog_digest(&self) -> &str {
        &self.admission_catalog_digest
    }
    pub const fn admission_switch_generation(&self) -> i64 {
        self.admission_switch_generation
    }
    pub fn admission_switch_digest(&self) -> &str {
        &self.admission_switch_digest
    }
    pub const fn runtime_manifest_generation(&self) -> i64 {
        self.runtime_manifest_generation
    }
    pub fn runtime_manifest_digest(&self) -> &str {
        &self.runtime_manifest_digest
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
    pub const fn official_hold_nano(&self) -> i64 {
        self.official_hold_nano
    }
    pub const fn charged_hold_nano(&self) -> i64 {
        self.charged_hold_nano
    }
    pub const fn track_eligible(&self) -> bool {
        self.track_eligible
    }
    pub const fn retention_eligible(&self) -> bool {
        self.retention_eligible
    }
    pub const fn commission_eligible(&self) -> bool {
        self.commission_eligible
    }
    pub fn premium_modifiers(&self) -> &LegacyPremiumModifiers {
        &self.premium_modifiers
    }
    pub fn snapshot_digest(&self) -> &str {
        &self.snapshot_digest
    }

    /// Apply the pinned policy multiplier with the same exact integer half-up rule used for the
    /// admission hold. Settlement callers use this instead of reconstructing money math.
    pub fn charge_for_official_nano(&self, official_nano: i64) -> Result<i64> {
        if official_nano < 0 {
            bail!("official policy-priced amount must be non-negative");
        }
        let numerator = i128::from(official_nano)
            .checked_mul(i128::from(self.payable_multiplier_bp))
            .context("policy charge multiplication overflow")?;
        let charged = numerator
            .checked_add(5_000)
            .context("policy charge rounding overflow")?
            / 10_000;
        i64::try_from(charged).context("policy charge does not fit i64")
    }

    pub(crate) fn premium_modifiers_json(&self) -> Result<String> {
        self.premium_modifiers.to_canonical_json()
    }

    pub(crate) fn from_stored(
        schema_version: i64,
        input: PolicyAdmissionSnapshotInput,
        snapshot_digest: String,
    ) -> Result<Self> {
        let expected = Self::new(input)?;
        if schema_version != POLICY_ADMISSION_SNAPSHOT_SCHEMA_VERSION
            || snapshot_digest != expected.snapshot_digest
        {
            bail!("stored policy admission snapshot failed canonical digest verification");
        }
        Ok(expected)
    }

    pub(crate) fn as_input(&self) -> PolicyAdmissionSnapshotInput {
        PolicyAdmissionSnapshotInput {
            request_id: self.request_id.clone(),
            account_id: self.account_id.clone(),
            provider: self.provider,
            product_id: self.product_id.clone(),
            account_class: self.account_class,
            requested_model_id: self.requested_model_id.clone(),
            canonical_model_id: self.canonical_model_id.clone(),
            alias_generation: self.alias_generation,
            rule_id: self.rule_id.clone(),
            rule_digest: self.rule_digest.clone(),
            rule_scope: self.rule_scope.clone(),
            pricing_mode: self.pricing_mode,
            rule_origin: self.rule_origin,
            discount_bps: self.discount_bps,
            payable_multiplier_bp: self.payable_multiplier_bp,
            policy_id: self.policy_id.clone(),
            policy_version: self.policy_version,
            effective_policy_version: self.effective_policy_version,
            source_policy_digest: self.source_policy_digest.clone(),
            policy_digest: self.policy_digest.clone(),
            policy_catalog_generation: self.policy_catalog_generation,
            policy_switch_generation: self.policy_switch_generation,
            admission_catalog_generation: self.admission_catalog_generation,
            admission_catalog_digest: self.admission_catalog_digest.clone(),
            admission_switch_generation: self.admission_switch_generation,
            admission_switch_digest: self.admission_switch_digest.clone(),
            runtime_manifest_generation: self.runtime_manifest_generation,
            runtime_manifest_digest: self.runtime_manifest_digest.clone(),
            tariff_schedule_id: self.tariff_schedule_id.clone(),
            tariff_priced_ts: self.tariff_priced_ts,
            admission_ts: self.admission_ts,
            official_hold_nano: self.official_hold_nano,
            charged_hold_nano: self.charged_hold_nano,
            track_eligible: self.track_eligible,
            retention_eligible: self.retention_eligible,
            commission_eligible: self.commission_eligible,
            premium_modifiers: self.premium_modifiers.clone(),
        }
    }

    fn compute_digest(&self) -> String {
        let mut e = CanonicalDigestEncoder::new(POLICY_SNAPSHOT_DIGEST_DOMAIN);
        e.i64(1, self.schema_version);
        e.string(2, &self.request_id);
        e.string(3, &self.account_id);
        e.string(4, "policy_v1");
        e.string(5, self.provider.as_str());
        e.string(6, &self.product_id);
        e.string(7, self.account_class.as_str());
        e.string(8, &self.requested_model_id);
        e.string(9, &self.canonical_model_id);
        e.i64(10, self.alias_generation);
        e.string(11, &self.rule_id);
        e.string(12, &self.rule_digest);
        let (scope, provider, model) = self.rule_scope.db_parts();
        e.string(13, scope);
        e.string(14, provider);
        e.string(15, model.unwrap_or(""));
        e.string(16, self.pricing_mode.as_str());
        e.string(17, self.rule_origin.as_str());
        e.i64(18, self.discount_bps.unwrap_or(-1));
        e.i64(19, self.payable_multiplier_bp);
        e.string(20, &self.policy_id);
        e.i64(21, self.policy_version);
        e.i64(22, self.effective_policy_version);
        e.string(23, &self.source_policy_digest);
        e.string(24, &self.policy_digest);
        e.i64(25, self.policy_catalog_generation);
        e.i64(26, self.policy_switch_generation);
        e.i64(27, self.admission_catalog_generation);
        e.string(28, &self.admission_catalog_digest);
        e.i64(29, self.admission_switch_generation);
        e.string(30, &self.admission_switch_digest);
        e.i64(31, self.runtime_manifest_generation);
        e.string(32, &self.runtime_manifest_digest);
        e.string(33, &self.tariff_schedule_id);
        e.i64(34, self.tariff_priced_ts);
        e.i64(35, self.admission_ts);
        e.i64(36, self.official_hold_nano);
        e.i64(37, self.charged_hold_nano);
        e.i64(38, i64::from(self.track_eligible));
        e.i64(39, i64::from(self.retention_eligible));
        e.i64(40, i64::from(self.commission_eligible));
        self.premium_modifiers.feed_digest(&mut e);
        e.finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyReserveConflict {
    ActivePricingRelease,
    ReservationIdentity,
    ExistingReservationWithoutSnapshot,
    ExistingNonPolicySnapshot,
    SnapshotPayload,
    TerminalReservation,
    ExpiredIdempotencyWindow,
    AdmissionTimestampInFuture,
    PolicyStateChanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyReserveReceipt {
    pub balance_after_reserve_nano: i64,
    pub snapshot: PolicyAdmissionSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyReserveOutcome {
    NotReserved,
    Inserted(PolicyReserveReceipt),
    Unchanged(PolicyReserveReceipt),
    AbortedBeforeCommit,
    Conflict(PolicyReserveConflict),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PolicySnapshotLookup {
    Missing,
    NonPolicy,
    Policy(Box<PolicyAdmissionSnapshot>),
}

fn validate_input(input: &PolicyAdmissionSnapshotInput) -> Result<()> {
    for (label, value) in [
        ("request id", input.request_id.as_str()),
        ("account id", input.account_id.as_str()),
        ("product id", input.product_id.as_str()),
        ("requested model id", input.requested_model_id.as_str()),
        ("canonical model id", input.canonical_model_id.as_str()),
        ("rule id", input.rule_id.as_str()),
        ("rule digest", input.rule_digest.as_str()),
        ("policy id", input.policy_id.as_str()),
        ("source policy digest", input.source_policy_digest.as_str()),
        ("policy digest", input.policy_digest.as_str()),
        (
            "admission catalog digest",
            input.admission_catalog_digest.as_str(),
        ),
        (
            "admission switch digest",
            input.admission_switch_digest.as_str(),
        ),
        (
            "runtime manifest digest",
            input.runtime_manifest_digest.as_str(),
        ),
    ] {
        require_bounded_id(label, value, ID_MAX_BYTES)?;
    }
    require_bounded_id(
        "tariff schedule id",
        &input.tariff_schedule_id,
        TARIFF_ID_MAX_BYTES,
    )?;
    if input.alias_generation <= 0
        || input.policy_version <= 0
        || input.effective_policy_version <= 0
        || input.policy_catalog_generation <= 0
        || input.policy_switch_generation <= 0
        || input.admission_catalog_generation <= 0
        || input.admission_switch_generation <= 0
        || input.runtime_manifest_generation <= 0
    {
        bail!("policy snapshot versions and generations must be positive");
    }
    if input.rule_scope.provider_id() != input.provider.as_str() {
        bail!("policy snapshot rule provider does not match the fixed provider plane");
    }
    if input
        .rule_scope
        .canonical_model_id()
        .is_some_and(|model| model != input.canonical_model_id)
    {
        bail!("policy snapshot model rule does not match the canonical model");
    }
    match (input.pricing_mode, input.rule_origin, input.discount_bps) {
        (PricingMode::Track, RuleOrigin::Managed, None)
            if input.track_eligible && input.retention_eligible => {}
        (PricingMode::Discount, RuleOrigin::Managed, Some(discount))
            if (0..=9_500).contains(&discount)
                && discount % 100 == 0
                && input.payable_multiplier_bp == 10_000 - discount
                && !input.track_eligible
                && !input.retention_eligible
                && !input.commission_eligible => {}
        (PricingMode::Discount, RuleOrigin::Legacy, None)
            if (1..=10_000).contains(&input.payable_multiplier_bp)
                && !input.track_eligible
                && !input.retention_eligible
                && !input.commission_eligible => {}
        _ => bail!("invalid policy snapshot rule contract"),
    }
    if input.commission_eligible && !input.track_eligible {
        bail!("policy snapshot commission eligibility requires track funding");
    }
    if input.tariff_priced_ts <= 0
        || input.admission_ts <= 0
        || input.tariff_priced_ts > input.admission_ts
    {
        bail!("invalid policy snapshot timestamps");
    }
    if input.official_hold_nano <= 0 || input.charged_hold_nano <= 0 {
        bail!("strict policy snapshot holds must be positive");
    }
    let numerator = i128::from(input.official_hold_nano)
        .checked_mul(i128::from(input.payable_multiplier_bp))
        .context("policy snapshot hold multiplication overflow")?;
    let expected = numerator
        .checked_add(5_000)
        .context("policy snapshot hold rounding overflow")?
        / 10_000;
    if expected != i128::from(input.charged_hold_nano) {
        bail!("policy snapshot charged hold does not match its exact multiplier");
    }
    input
        .premium_modifiers
        .validate_for_provider(input.provider)?;
    input.premium_modifiers.to_canonical_json()?;
    Ok(())
}
