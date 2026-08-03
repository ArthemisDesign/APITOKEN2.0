//! PostgreSQL persistence and dormant Stage 3B snapshot read for the pricing contract.
//!
//! Every mutation is serialized by a namespaced transaction advisory lock. Row locks alone are
//! insufficient because an absent head or binding has no row to lock. Immutable versions and their
//! children are always written in one transaction; an active pointer moves only through an
//! explicit compare-and-set.

use super::{
    expectation_matches, invalid, missing, normalize_catalog, normalize_policy, normalize_switches,
    policy_expectation_matches, require_id, validate_account_policy,
    validate_account_policy_activation, validate_account_policy_binding,
    validate_account_policy_shape, validate_active_expectation,
    validate_legacy_snapshot_request_id, validate_policy_active_expectation,
    validate_pricing_catalog, validate_provider_switches, validate_version_target, AccountClass,
    AccountPolicyActivationSpec, AccountPolicyBindingSpec, AccountPolicyRuleSpec,
    AccountPolicySpec, ActiveAccountPolicy, ActiveExpectation, ActivePolicyTarget,
    FundingEnforcement, LegacyPremiumModifiers, LegacyScalarAdmissionSnapshot,
    LegacyScalarAdmissionSnapshotInput, LegacyScalarSnapshotLookup, PolicyActiveExpectation,
    PolicyAdmissionSnapshot, PolicyAdmissionSnapshotInput, PolicyBindingState, PolicyEnforcement,
    PolicyOwnerType, PolicyRuleScope, PolicySnapshotLookup, PricingCatalogEntrySpec,
    PricingCatalogSpec, PricingMode, PricingMutation, PricingPolicySnapshot, PricingReadBundle,
    PricingRejection, PricingShadowAdmissionEvaluation, PricingShadowAdmissionEvaluationInput,
    PricingShadowEvaluationWrite, PricingShadowStorageRow, ProviderSwitchEntrySpec,
    ProviderSwitchScope, ProviderSwitchSpec, ReconciliationState, RuleOrigin,
    ShadowActualSnapshotRef, SnapshotProvider, VersionTarget,
};
use anyhow::{bail, Context, Result};
use postgres::{Client, GenericClient, IsolationLevel, Transaction};
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn stale(actual: Option<VersionTarget>) -> PricingMutation {
    PricingMutation::Rejected(PricingRejection::Stale { actual })
}

fn version_conflict() -> PricingMutation {
    PricingMutation::Rejected(PricingRejection::VersionConflict)
}

fn cas_mismatch(actual: Option<VersionTarget>) -> PricingMutation {
    PricingMutation::Rejected(PricingRejection::CasMismatch { actual })
}

fn policy_cas_mismatch(actual: PolicyBindingState) -> PricingMutation {
    PricingMutation::Rejected(PricingRejection::PolicyCasMismatch { actual })
}

fn locked() -> PricingMutation {
    PricingMutation::Rejected(PricingRejection::Locked)
}

fn commit_mutation(
    transaction: Transaction<'_>,
    mutation: PricingMutation,
    operation: &str,
) -> Result<PricingMutation> {
    transaction
        .commit()
        .with_context(|| format!("commit PostgreSQL {operation}"))?;
    Ok(mutation)
}

fn advisory_lock(transaction: &mut Transaction<'_>, key: &str) -> Result<()> {
    transaction
        .query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&key],
        )
        .with_context(|| format!("lock PostgreSQL pricing mutation namespace {key:?}"))?;
    Ok(())
}

fn catalog_lock_key(product_id: &str) -> String {
    format!("multi-discount:catalog:{product_id}")
}

fn policy_lock_key(account_id: &str) -> String {
    format!("multi-discount:policy:{account_id}")
}

fn shadow_evaluation_advisory_lock(
    transaction: &mut Transaction<'_>,
    request_id: &str,
) -> Result<()> {
    let key = format!("multi-discount:shadow-evaluation:{request_id}");
    transaction
        .query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&key],
        )
        // Request IDs must not enter a future shadow error storm through error context.
        .context("lock PostgreSQL shadow evaluation mutation")?;
    Ok(())
}

const SWITCH_LOCK_KEY: &str = "multi-discount:switches";
pub(crate) const PRICING_RELEASE_CONTROL_LOCK_V2: &str = "pricing-release-v2:control-plane";

fn postgres_legacy_scalar_snapshot_lookup_inner<C: GenericClient>(
    client: &mut C,
    request_id: &str,
    key_share: bool,
) -> Result<LegacyScalarSnapshotLookup> {
    validate_legacy_snapshot_request_id(request_id)?;
    let sql = if key_share {
        "SELECT snapshot_kind,schema_version,account_id,provider_id,requested_model_id,
                canonical_model_id,alias_generation,pricing_mode,rule_origin,
                tariff_schedule_id,tariff_priced_ts,admission_ts,payable_multiplier_bp,
                official_hold_nano,charged_hold_nano,premium_modifiers::text,snapshot_digest
           FROM pricing_admission_snapshots
          WHERE request_id=$1
          FOR KEY SHARE"
    } else {
        "SELECT snapshot_kind,schema_version,account_id,provider_id,requested_model_id,
                canonical_model_id,alias_generation,pricing_mode,rule_origin,
                tariff_schedule_id,tariff_priced_ts,admission_ts,payable_multiplier_bp,
                official_hold_nano,charged_hold_nano,premium_modifiers::text,snapshot_digest
           FROM pricing_admission_snapshots
          WHERE request_id=$1"
    };
    let Some(row) = client
        .query_opt(sql, &[&request_id])
        .context("read PostgreSQL pricing admission snapshot")?
    else {
        return Ok(LegacyScalarSnapshotLookup::Missing);
    };

    let snapshot_kind: String = row.get(0);
    if snapshot_kind != "legacy_scalar" {
        return Ok(LegacyScalarSnapshotLookup::NonLegacy);
    }
    let pricing_mode: String = row.get(7);
    let rule_origin: String = row.get(8);
    if pricing_mode != "legacy_scalar" || rule_origin != "legacy" {
        bail!("stored legacy scalar snapshot has an invalid fixed shape");
    }
    let provider_id: String = row.get(3);
    let premium_modifiers_json: String = row.get(15);
    let snapshot = LegacyScalarAdmissionSnapshot::from_stored(
        row.get(1),
        LegacyScalarAdmissionSnapshotInput {
            request_id: request_id.to_owned(),
            account_id: row.get(2),
            provider: SnapshotProvider::from_db(&provider_id)?,
            requested_model_id: row.get(4),
            canonical_model_id: row.get(5),
            alias_generation: row.get(6),
            tariff_schedule_id: row.get(9),
            tariff_priced_ts: row.get(10),
            admission_ts: row.get(11),
            payable_multiplier_bp: row.get(12),
            official_hold_nano: row.get(13),
            charged_hold_nano: row.get(14),
            premium_modifiers: LegacyPremiumModifiers::from_json(&premium_modifiers_json)?,
        },
        row.get(16),
    )?;
    Ok(LegacyScalarSnapshotLookup::Legacy(Box::new(snapshot)))
}

pub(crate) fn postgres_legacy_scalar_snapshot_lookup<C: GenericClient>(
    client: &mut C,
    request_id: &str,
) -> Result<LegacyScalarSnapshotLookup> {
    postgres_legacy_scalar_snapshot_lookup_inner(client, request_id, false)
}

pub(crate) fn postgres_insert_legacy_scalar_admission_snapshot<C: GenericClient>(
    client: &mut C,
    snapshot: &LegacyScalarAdmissionSnapshot,
) -> Result<()> {
    snapshot.validate()?;
    let provider_id = snapshot.provider.as_str();
    let premium_modifiers = snapshot.premium_modifiers_json()?;
    let snapshot_digest = snapshot.snapshot_digest().as_str();
    let inserted = client
        .execute(
            "INSERT INTO pricing_admission_snapshots(
                 request_id,account_id,snapshot_kind,schema_version,provider_id,
                 requested_model_id,canonical_model_id,alias_generation,pricing_mode,rule_origin,
                 payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,admission_ts,
                 official_hold_nano,charged_hold_nano,premium_modifiers,snapshot_digest
             ) VALUES(
                 $1,$2,'legacy_scalar',$3,$4,$5,$6,$7,'legacy_scalar','legacy',
                 $8,$9,$10,$11,$12,$13,$14::text::jsonb,$15
             )",
            &[
                &snapshot.request_id,
                &snapshot.account_id,
                &snapshot.schema_version,
                &provider_id,
                &snapshot.requested_model_id,
                &snapshot.canonical_model_id,
                &snapshot.alias_generation,
                &snapshot.payable_multiplier_bp,
                &snapshot.tariff_schedule_id,
                &snapshot.tariff_priced_ts,
                &snapshot.admission_ts,
                &snapshot.official_hold_nano,
                &snapshot.charged_hold_nano,
                &premium_modifiers,
                &snapshot_digest,
            ],
        )
        .context("insert PostgreSQL legacy scalar admission snapshot")?;
    if inserted != 1 {
        bail!("PostgreSQL legacy scalar admission snapshot insert changed no row");
    }
    Ok(())
}

pub(crate) fn postgres_policy_snapshot_lookup<C: GenericClient>(
    client: &mut C,
    request_id: &str,
    key_share: bool,
) -> Result<PolicySnapshotLookup> {
    validate_legacy_snapshot_request_id(request_id)?;
    let lock = if key_share { " FOR KEY SHARE" } else { "" };
    let sql = format!(
        "SELECT snapshot_kind,schema_version,account_id,provider_id,product_id,account_class,
                requested_model_id,canonical_model_id,alias_generation,rule_id,rule_digest,
                rule_scope,pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
                policy_id,policy_version,effective_policy_version,source_policy_digest,
                policy_digest,catalog_generation,switch_generation,
                admission_catalog_generation,admission_catalog_digest,
                admission_switch_generation,admission_switch_digest,
                runtime_manifest_generation,runtime_manifest_digest,tariff_schedule_id,
                tariff_priced_ts,admission_ts,official_hold_nano,charged_hold_nano,
                track_eligible,retention_eligible,commission_eligible,premium_modifiers::text,
                snapshot_digest
           FROM pricing_admission_snapshots WHERE request_id=$1{lock}"
    );
    let Some(row) = client.query_opt(&sql, &[&request_id])? else {
        return Ok(PolicySnapshotLookup::Missing);
    };
    let kind: String = row.get(0);
    if kind != "policy_v1" {
        return Ok(PolicySnapshotLookup::NonPolicy);
    }
    let provider_id: String = row.get(3);
    let canonical_model_id: String = row.get(7);
    let rule_scope: String = row.get(11);
    let input = PolicyAdmissionSnapshotInput {
        request_id: request_id.to_owned(),
        account_id: row.get(2),
        provider: SnapshotProvider::from_db(&provider_id)?,
        product_id: row.get(4),
        account_class: AccountClass::from_db(&row.get::<_, String>(5))?,
        requested_model_id: row.get(6),
        canonical_model_id: canonical_model_id.clone(),
        alias_generation: row.get(8),
        rule_id: row.get(9),
        rule_digest: row.get(10),
        rule_scope: PolicyRuleScope::from_db(
            &rule_scope,
            provider_id,
            (rule_scope == "model").then_some(canonical_model_id),
        )?,
        pricing_mode: PricingMode::from_db(&row.get::<_, String>(12))?,
        rule_origin: RuleOrigin::from_db(&row.get::<_, String>(13))?,
        discount_bps: row.get(14),
        payable_multiplier_bp: row.get(15),
        policy_id: row.get(16),
        policy_version: row.get(17),
        effective_policy_version: row.get(18),
        source_policy_digest: row.get(19),
        policy_digest: row.get(20),
        policy_catalog_generation: row.get(21),
        policy_switch_generation: row.get(22),
        admission_catalog_generation: row.get(23),
        admission_catalog_digest: row.get(24),
        admission_switch_generation: row.get(25),
        admission_switch_digest: row.get(26),
        runtime_manifest_generation: row.get(27),
        runtime_manifest_digest: row.get(28),
        tariff_schedule_id: row.get(29),
        tariff_priced_ts: row.get(30),
        admission_ts: row.get(31),
        official_hold_nano: row.get(32),
        charged_hold_nano: row.get(33),
        track_eligible: row.get(34),
        retention_eligible: row.get(35),
        commission_eligible: row.get(36),
        premium_modifiers: LegacyPremiumModifiers::from_json(&row.get::<_, String>(37))?,
    };
    Ok(PolicySnapshotLookup::Policy(Box::new(
        PolicyAdmissionSnapshot::from_stored(row.get(1), input, row.get(38))?,
    )))
}

pub(crate) fn postgres_insert_policy_admission_snapshot<C: GenericClient>(
    client: &mut C,
    snapshot: &PolicyAdmissionSnapshot,
) -> Result<()> {
    snapshot.validate()?;
    let premium_modifiers = snapshot.premium_modifiers_json()?;
    let (rule_scope, _, _) = snapshot.rule_scope.db_parts();
    let provider_id = snapshot.provider.as_str();
    let account_class = snapshot.account_class.as_str();
    let pricing_mode = snapshot.pricing_mode.as_str();
    let rule_origin = snapshot.rule_origin.as_str();
    let inserted = client
        .execute(
            "INSERT INTO pricing_admission_snapshots(
             request_id,account_id,snapshot_kind,schema_version,provider_id,product_id,
             account_class,requested_model_id,canonical_model_id,alias_generation,rule_id,
             rule_digest,rule_scope,pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
             policy_id,policy_version,effective_policy_version,source_policy_digest,policy_digest,
             catalog_generation,switch_generation,admission_catalog_generation,
             admission_catalog_digest,admission_switch_generation,admission_switch_digest,
             runtime_manifest_generation,runtime_manifest_digest,tariff_schedule_id,
             tariff_priced_ts,admission_ts,official_hold_nano,charged_hold_nano,track_eligible,
             retention_eligible,commission_eligible,premium_modifiers,snapshot_digest
         ) VALUES(
             $1,$2,'policy_v1',$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,
             $19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,
             $38::text::jsonb,$39
         )",
            &[
                &snapshot.request_id,
                &snapshot.account_id,
                &snapshot.schema_version,
                &provider_id,
                &snapshot.product_id,
                &account_class,
                &snapshot.requested_model_id,
                &snapshot.canonical_model_id,
                &snapshot.alias_generation,
                &snapshot.rule_id,
                &snapshot.rule_digest,
                &rule_scope,
                &pricing_mode,
                &rule_origin,
                &snapshot.discount_bps,
                &snapshot.payable_multiplier_bp,
                &snapshot.policy_id,
                &snapshot.policy_version,
                &snapshot.effective_policy_version,
                &snapshot.source_policy_digest,
                &snapshot.policy_digest,
                &snapshot.policy_catalog_generation,
                &snapshot.policy_switch_generation,
                &snapshot.admission_catalog_generation,
                &snapshot.admission_catalog_digest,
                &snapshot.admission_switch_generation,
                &snapshot.admission_switch_digest,
                &snapshot.runtime_manifest_generation,
                &snapshot.runtime_manifest_digest,
                &snapshot.tariff_schedule_id,
                &snapshot.tariff_priced_ts,
                &snapshot.admission_ts,
                &snapshot.official_hold_nano,
                &snapshot.charged_hold_nano,
                &snapshot.track_eligible,
                &snapshot.retention_eligible,
                &snapshot.commission_eligible,
                &premium_modifiers,
                &snapshot.snapshot_digest(),
            ],
        )
        .context("insert PostgreSQL policy admission snapshot")?;
    if inserted != 1 {
        bail!("PostgreSQL policy admission snapshot insert changed no row");
    }
    Ok(())
}

fn postgres_shadow_storage_row<C: GenericClient>(
    client: &mut C,
    request_id: &str,
    key_share: bool,
) -> Result<Option<PricingShadowStorageRow>> {
    validate_legacy_snapshot_request_id(request_id)?;
    let sql = if key_share {
        "SELECT e.*, e.diagnostic_context::text AS diagnostic_context_text
           FROM pricing_shadow_admission_evaluations AS e
          WHERE e.request_id=$1
          FOR KEY SHARE OF e"
    } else {
        "SELECT e.*, e.diagnostic_context::text AS diagnostic_context_text
           FROM pricing_shadow_admission_evaluations AS e
          WHERE e.request_id=$1"
    };
    let Some(row) = client
        .query_opt(sql, &[&request_id])
        .context("read PostgreSQL pricing shadow admission evaluation")?
    else {
        return Ok(None);
    };
    Ok(Some(PricingShadowStorageRow {
        request_id: row.get("request_id"),
        account_id: row.get("account_id"),
        actual_snapshot_kind: row.get("actual_snapshot_kind"),
        actual_snapshot_digest: row.get("actual_snapshot_digest"),
        provider_id: row.get("provider_id"),
        requested_model_id: row.get("requested_model_id"),
        canonical_model_id: row.get("canonical_model_id"),
        alias_generation: row.get("alias_generation"),
        evaluator_schema_version: row.get("evaluator_schema_version"),
        runtime_manifest_generation: row.get("runtime_manifest_generation"),
        runtime_manifest_digest: row.get("runtime_manifest_digest"),
        enqueued_ts: row.get("enqueued_ts"),
        evaluated_ts: row.get("evaluated_ts"),
        outcome: row.get("outcome"),
        reason_code: row.get("reason_code"),
        authorized_multiplier_bp: row.get("authorized_multiplier_bp"),
        observed_multiplier_bp: row.get("observed_multiplier_bp"),
        official_hold_nano: row.get("official_hold_nano"),
        legacy_hold_nano: row.get("legacy_hold_nano"),
        product_id: row.get("product_id"),
        account_class: row.get("account_class"),
        effective_policy_version: row.get("effective_policy_version"),
        policy_id: row.get("policy_id"),
        policy_version: row.get("policy_version"),
        source_policy_digest: row.get("source_policy_digest"),
        policy_digest: row.get("policy_digest"),
        policy_schema_version: row.get("policy_schema_version"),
        policy_catalog_generation: row.get("policy_catalog_generation"),
        policy_catalog_schema_version: row.get("policy_catalog_schema_version"),
        policy_catalog_capability_generation: row.get("policy_catalog_capability_generation"),
        policy_catalog_capability_digest: row.get("policy_catalog_capability_digest"),
        policy_catalog_digest: row.get("policy_catalog_digest"),
        policy_switch_generation: row.get("policy_switch_generation"),
        policy_switch_schema_version: row.get("policy_switch_schema_version"),
        policy_switch_capability_generation: row.get("policy_switch_capability_generation"),
        policy_switch_capability_digest: row.get("policy_switch_capability_digest"),
        policy_switch_digest: row.get("policy_switch_digest"),
        admission_catalog_generation: row.get("admission_catalog_generation"),
        admission_catalog_schema_version: row.get("admission_catalog_schema_version"),
        admission_catalog_capability_generation: row.get("admission_catalog_capability_generation"),
        admission_catalog_capability_digest: row.get("admission_catalog_capability_digest"),
        admission_catalog_digest: row.get("admission_catalog_digest"),
        admission_switch_generation: row.get("admission_switch_generation"),
        admission_switch_schema_version: row.get("admission_switch_schema_version"),
        admission_switch_capability_generation: row.get("admission_switch_capability_generation"),
        admission_switch_capability_digest: row.get("admission_switch_capability_digest"),
        admission_switch_digest: row.get("admission_switch_digest"),
        rule_id: row.get("rule_id"),
        rule_digest: row.get("rule_digest"),
        rule_scope: row.get("rule_scope"),
        pricing_mode: row.get("pricing_mode"),
        rule_origin: row.get("rule_origin"),
        discount_bps: row.get("discount_bps"),
        payable_multiplier_bp: row.get("payable_multiplier_bp"),
        track_eligible: row.get("track_eligible"),
        retention_eligible: row.get("retention_eligible"),
        commission_eligible: row.get("commission_eligible"),
        policy_hold_nano: row.get("policy_hold_nano"),
        comparison_result: row.get("comparison_result"),
        diagnostic_context: row.get("diagnostic_context_text"),
        evaluation_digest: row.get("evaluation_digest"),
    }))
}

pub(crate) fn postgres_shadow_evaluation_in_transaction<C: GenericClient>(
    client: &mut C,
    request_id: &str,
    key_share: bool,
) -> Result<Option<PricingShadowAdmissionEvaluation>> {
    // Maintenance deletes the actual parent before cascading to the shadow child. Follow the same
    // parent -> child lock order so a replay cannot deadlock with retention cleanup.
    let locked_actual = if key_share {
        Some(postgres_legacy_scalar_snapshot_lookup_inner(
            client, request_id, true,
        )?)
    } else {
        None
    };
    let Some(row) = postgres_shadow_storage_row(client, request_id, key_share)? else {
        return Ok(None);
    };
    let actual_lookup = match locked_actual {
        Some(actual) => actual,
        None => postgres_legacy_scalar_snapshot_lookup_inner(client, request_id, false)?,
    };
    let actual = match actual_lookup {
        LegacyScalarSnapshotLookup::Legacy(snapshot) => *snapshot,
        LegacyScalarSnapshotLookup::Missing => {
            bail!("stored shadow evaluation is missing its actual snapshot")
        }
        LegacyScalarSnapshotLookup::NonLegacy => {
            bail!("stored shadow evaluation references a non-legacy actual snapshot")
        }
    };
    Ok(Some(PricingShadowAdmissionEvaluation::from_storage(
        &actual, row,
    )?))
}

pub(crate) fn postgres_pricing_shadow_admission_evaluation(
    client: &mut Client,
    request_id: &str,
) -> Result<Option<PricingShadowAdmissionEvaluation>> {
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .context("begin PostgreSQL shadow evaluation read transaction")?;
    let evaluation =
        postgres_shadow_evaluation_in_transaction(&mut transaction, request_id, false)?;
    transaction
        .commit()
        .context("commit PostgreSQL shadow evaluation read transaction")?;
    Ok(evaluation)
}

pub(crate) fn postgres_insert_pricing_shadow_admission_evaluation(
    client: &mut Client,
    input: &PricingShadowAdmissionEvaluationInput,
) -> Result<PricingShadowEvaluationWrite> {
    postgres_insert_pricing_shadow_admission_evaluation_inner(client, input, None)
}

pub(crate) fn postgres_insert_pricing_shadow_admission_evaluation_with_timeout(
    client: &mut Client,
    input: &PricingShadowAdmissionEvaluationInput,
    timeout_ms: u64,
) -> Result<PricingShadowEvaluationWrite> {
    postgres_insert_pricing_shadow_admission_evaluation_inner(client, input, Some(timeout_ms))
}

fn postgres_insert_pricing_shadow_admission_evaluation_inner(
    client: &mut Client,
    input: &PricingShadowAdmissionEvaluationInput,
    timeout_ms: Option<u64>,
) -> Result<PricingShadowEvaluationWrite> {
    let candidate = input.to_evaluation()?;
    let row = candidate.storage_row()?;
    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL pricing shadow evaluation transaction")?;
    if let Some(timeout_ms) = timeout_ms {
        set_shadow_transaction_timeout(&mut transaction, timeout_ms)?;
    }
    shadow_evaluation_advisory_lock(&mut transaction, candidate.actual().request_id())?;

    if let Some(existing) = postgres_shadow_evaluation_in_transaction(
        &mut transaction,
        candidate.actual().request_id(),
        true,
    )? {
        let outcome = candidate.classify_existing(existing)?;
        transaction
            .commit()
            .context("commit PostgreSQL shadow evaluation replay transaction")?;
        return Ok(outcome);
    }

    let actual = match postgres_legacy_scalar_snapshot_lookup_inner(
        &mut transaction,
        candidate.actual().request_id(),
        true,
    )? {
        LegacyScalarSnapshotLookup::Legacy(snapshot) => *snapshot,
        LegacyScalarSnapshotLookup::Missing => {
            bail!("shadow evaluation actual snapshot does not exist")
        }
        LegacyScalarSnapshotLookup::NonLegacy => {
            bail!("shadow evaluation actual snapshot is not legacy scalar")
        }
    };
    if ShadowActualSnapshotRef::from_snapshot(&actual)? != *candidate.actual() {
        bail!("shadow evaluation input does not match the stored actual snapshot");
    }

    let inserted = transaction
        .execute(
            "INSERT INTO pricing_shadow_admission_evaluations(
                 request_id,account_id,actual_snapshot_kind,actual_snapshot_digest,provider_id,
                 requested_model_id,canonical_model_id,alias_generation,evaluator_schema_version,
                 runtime_manifest_generation,runtime_manifest_digest,enqueued_ts,evaluated_ts,
                 outcome,reason_code,authorized_multiplier_bp,observed_multiplier_bp,
                 official_hold_nano,legacy_hold_nano,product_id,account_class,
                 effective_policy_version,policy_id,policy_version,source_policy_digest,
                 policy_digest,policy_schema_version,policy_catalog_generation,
                 policy_catalog_schema_version,policy_catalog_capability_generation,
                 policy_catalog_capability_digest,policy_catalog_digest,policy_switch_generation,
                 policy_switch_schema_version,policy_switch_capability_generation,
                 policy_switch_capability_digest,policy_switch_digest,admission_catalog_generation,
                 admission_catalog_schema_version,admission_catalog_capability_generation,
                 admission_catalog_capability_digest,admission_catalog_digest,
                 admission_switch_generation,admission_switch_schema_version,
                 admission_switch_capability_generation,admission_switch_capability_digest,
                 admission_switch_digest,rule_id,rule_digest,rule_scope,pricing_mode,rule_origin,
                 discount_bps,payable_multiplier_bp,track_eligible,retention_eligible,
                 commission_eligible,policy_hold_nano,comparison_result,diagnostic_context,
                 evaluation_digest
             ) VALUES(
                 $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,
                 $21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,$38,
                 $39,$40,$41,$42,$43,$44,$45,$46,$47,$48,$49,$50,$51,$52,$53,$54,$55,$56,
                 $57,$58,$59,$60::text::jsonb,$61
             )
             ON CONFLICT (request_id) DO NOTHING",
            &[
                &row.request_id,
                &row.account_id,
                &row.actual_snapshot_kind,
                &row.actual_snapshot_digest,
                &row.provider_id,
                &row.requested_model_id,
                &row.canonical_model_id,
                &row.alias_generation,
                &row.evaluator_schema_version,
                &row.runtime_manifest_generation,
                &row.runtime_manifest_digest,
                &row.enqueued_ts,
                &row.evaluated_ts,
                &row.outcome,
                &row.reason_code,
                &row.authorized_multiplier_bp,
                &row.observed_multiplier_bp,
                &row.official_hold_nano,
                &row.legacy_hold_nano,
                &row.product_id,
                &row.account_class,
                &row.effective_policy_version,
                &row.policy_id,
                &row.policy_version,
                &row.source_policy_digest,
                &row.policy_digest,
                &row.policy_schema_version,
                &row.policy_catalog_generation,
                &row.policy_catalog_schema_version,
                &row.policy_catalog_capability_generation,
                &row.policy_catalog_capability_digest,
                &row.policy_catalog_digest,
                &row.policy_switch_generation,
                &row.policy_switch_schema_version,
                &row.policy_switch_capability_generation,
                &row.policy_switch_capability_digest,
                &row.policy_switch_digest,
                &row.admission_catalog_generation,
                &row.admission_catalog_schema_version,
                &row.admission_catalog_capability_generation,
                &row.admission_catalog_capability_digest,
                &row.admission_catalog_digest,
                &row.admission_switch_generation,
                &row.admission_switch_schema_version,
                &row.admission_switch_capability_generation,
                &row.admission_switch_capability_digest,
                &row.admission_switch_digest,
                &row.rule_id,
                &row.rule_digest,
                &row.rule_scope,
                &row.pricing_mode,
                &row.rule_origin,
                &row.discount_bps,
                &row.payable_multiplier_bp,
                &row.track_eligible,
                &row.retention_eligible,
                &row.commission_eligible,
                &row.policy_hold_nano,
                &row.comparison_result,
                &row.diagnostic_context,
                &row.evaluation_digest,
            ],
        )
        .context("insert PostgreSQL pricing shadow admission evaluation")?;

    if inserted == 1 {
        transaction
            .commit()
            .context("commit PostgreSQL pricing shadow admission evaluation")?;
        return Ok(PricingShadowEvaluationWrite::Inserted(Box::new(candidate)));
    }
    if inserted != 0 {
        bail!("PostgreSQL shadow evaluation insert changed an unexpected row count");
    }

    let existing = postgres_shadow_evaluation_in_transaction(
        &mut transaction,
        candidate.actual().request_id(),
        true,
    )?
    .context("shadow evaluation conflict row disappeared before classification")?;
    let outcome = candidate.classify_existing(existing)?;
    transaction
        .commit()
        .context("commit PostgreSQL shadow evaluation conflict transaction")?;
    Ok(outcome)
}

fn set_shadow_transaction_timeout(
    transaction: &mut postgres::Transaction<'_>,
    timeout_ms: u64,
) -> Result<()> {
    if !(1..=15_000).contains(&timeout_ms) {
        bail!("pricing shadow database timeout must be in 1..=15000 milliseconds");
    }
    let timeout = format!("{timeout_ms}ms");
    transaction
        .query_one(
            "SELECT set_config('statement_timeout',$1,true),set_config('lock_timeout',$1,true)",
            &[&timeout],
        )
        .context("configure bounded PostgreSQL pricing shadow transaction")?;
    Ok(())
}

fn catalog_by_generation<C: GenericClient>(
    client: &mut C,
    product_id: &str,
    generation: i64,
    key_share: bool,
) -> Result<Option<PricingCatalogSpec>> {
    let sql = if key_share {
        "SELECT product_id,generation,schema_version,capability_generation,
                capability_digest,content_digest
         FROM pricing_catalog_versions
         WHERE product_id=$1 AND generation=$2
         FOR KEY SHARE"
    } else {
        "SELECT product_id,generation,schema_version,capability_generation,
                capability_digest,content_digest
         FROM pricing_catalog_versions
         WHERE product_id=$1 AND generation=$2"
    };
    let Some(header) = client.query_opt(sql, &[&product_id, &generation])? else {
        return Ok(None);
    };
    let mut spec = PricingCatalogSpec {
        product_id: header.get(0),
        generation: header.get(1),
        schema_version: header.get(2),
        capability_generation: header.get(3),
        capability_digest: header.get(4),
        content_digest: header.get(5),
        entries: Vec::new(),
    };
    spec.entries = client
        .query(
            "SELECT provider_id,canonical_model_id,enabled
             FROM pricing_catalog_entries
             WHERE product_id=$1 AND generation=$2
             ORDER BY provider_id,canonical_model_id",
            &[&product_id, &generation],
        )?
        .into_iter()
        .map(|row| PricingCatalogEntrySpec {
            provider_id: row.get(0),
            canonical_model_id: row.get(1),
            enabled: row.get(2),
        })
        .collect();
    spec = normalize_catalog(&spec);
    validate_pricing_catalog(&spec).context("validate stored PostgreSQL pricing catalog")?;
    Ok(Some(spec))
}

pub(crate) fn postgres_pricing_catalog_by_generation(
    client: &mut Client,
    product_id: &str,
    generation: i64,
) -> Result<Option<PricingCatalogSpec>> {
    require_id("product id", product_id)?;
    if generation <= 0 {
        bail!("catalog generation must be positive");
    }
    catalog_by_generation(client, product_id, generation, false)
}

fn catalog_head_locked(
    transaction: &mut Transaction<'_>,
    product_id: &str,
) -> Result<Option<VersionTarget>> {
    let Some(row) = transaction.query_opt(
        "SELECT h.active_generation,v.content_digest
         FROM pricing_catalog_heads h
         LEFT JOIN pricing_catalog_versions v
           ON v.product_id=h.product_id AND v.generation=h.active_generation
         WHERE h.product_id=$1
         FOR UPDATE OF h",
        &[&product_id],
    )?
    else {
        return Ok(None);
    };
    let version: i64 = row.get(0);
    let digest: Option<String> = row.get(1);
    Ok(Some(VersionTarget::new(
        version,
        digest.context("pricing catalog head references a missing version")?,
    )))
}

fn catalog_head_shared(
    transaction: &mut Transaction<'_>,
    product_id: &str,
) -> Result<Option<VersionTarget>> {
    let Some(row) = transaction.query_opt(
        "SELECT h.active_generation,v.content_digest
         FROM pricing_catalog_heads h
         LEFT JOIN pricing_catalog_versions v
           ON v.product_id=h.product_id AND v.generation=h.active_generation
         WHERE h.product_id=$1
         FOR SHARE OF h",
        &[&product_id],
    )?
    else {
        return Ok(None);
    };
    let version: i64 = row.get(0);
    let digest: Option<String> = row.get(1);
    Ok(Some(VersionTarget::new(
        version,
        digest.context("pricing catalog head references a missing version")?,
    )))
}

fn active_pricing_catalog<C: GenericClient>(
    client: &mut C,
    product_id: &str,
) -> Result<Option<PricingCatalogSpec>> {
    require_id("product id", product_id)?;
    let Some(row) = client.query_opt(
        "SELECT h.active_generation,v.content_digest
         FROM pricing_catalog_heads h
         LEFT JOIN pricing_catalog_versions v
           ON v.product_id=h.product_id AND v.generation=h.active_generation
         WHERE h.product_id=$1",
        &[&product_id],
    )?
    else {
        return Ok(None);
    };
    let generation: i64 = row.get(0);
    let digest: Option<String> = row.get(1);
    digest.context("pricing catalog head references a missing version")?;
    catalog_by_generation(client, product_id, generation, false)?
        .map(Some)
        .context("pricing catalog head references a missing version")
}

pub(crate) fn postgres_active_pricing_catalog(
    client: &mut Client,
    product_id: &str,
) -> Result<Option<PricingCatalogSpec>> {
    require_id("product id", product_id)?;
    active_pricing_catalog(client, product_id)
}

fn switches_by_generation<C: GenericClient>(
    client: &mut C,
    generation: i64,
    key_share: bool,
) -> Result<Option<ProviderSwitchSpec>> {
    let sql = if key_share {
        "SELECT generation,schema_version,capability_generation,capability_digest,content_digest
         FROM provider_switch_versions
         WHERE generation=$1
         FOR KEY SHARE"
    } else {
        "SELECT generation,schema_version,capability_generation,capability_digest,content_digest
         FROM provider_switch_versions
         WHERE generation=$1"
    };
    let Some(header) = client.query_opt(sql, &[&generation])? else {
        return Ok(None);
    };
    let mut spec = ProviderSwitchSpec {
        generation: header.get(0),
        schema_version: header.get(1),
        capability_generation: header.get(2),
        capability_digest: header.get(3),
        content_digest: header.get(4),
        entries: Vec::new(),
    };
    let rows = client.query(
        "SELECT provider_id,scope_type,product_id,segment,catalog_generation,enabled
         FROM provider_switch_entries
         WHERE generation=$1
         ORDER BY provider_id,scope_type,product_id,segment",
        &[&generation],
    )?;
    spec.entries = rows
        .into_iter()
        .map(|row| {
            let provider_id: String = row.get(0);
            let scope_type: String = row.get(1);
            let product_id: String = row.get(2);
            let segment: String = row.get(3);
            Ok(ProviderSwitchEntrySpec {
                provider_id,
                scope: ProviderSwitchScope::from_db(&scope_type, product_id, segment)?,
                catalog_generation: row.get(4),
                enabled: row.get(5),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    spec = normalize_switches(&spec);
    validate_provider_switches(&spec).context("validate stored PostgreSQL provider switches")?;
    Ok(Some(spec))
}

pub(crate) fn postgres_provider_switches_by_generation(
    client: &mut Client,
    generation: i64,
) -> Result<Option<ProviderSwitchSpec>> {
    if generation <= 0 {
        bail!("provider switch generation must be positive");
    }
    switches_by_generation(client, generation, false)
}

fn switch_head_locked(transaction: &mut Transaction<'_>) -> Result<Option<VersionTarget>> {
    let Some(row) = transaction.query_opt(
        "SELECT h.active_generation,v.content_digest
         FROM provider_switch_head h
         LEFT JOIN provider_switch_versions v ON v.generation=h.active_generation
         WHERE h.singleton=1
         FOR UPDATE OF h",
        &[],
    )?
    else {
        return Ok(None);
    };
    let version: i64 = row.get(0);
    let digest: Option<String> = row.get(1);
    Ok(Some(VersionTarget::new(
        version,
        digest.context("provider switch head references a missing version")?,
    )))
}

fn switch_head_shared(transaction: &mut Transaction<'_>) -> Result<Option<VersionTarget>> {
    let Some(row) = transaction.query_opt(
        "SELECT h.active_generation,v.content_digest
         FROM provider_switch_head h
         LEFT JOIN provider_switch_versions v ON v.generation=h.active_generation
         WHERE h.singleton=1
         FOR SHARE OF h",
        &[],
    )?
    else {
        return Ok(None);
    };
    let version: i64 = row.get(0);
    let digest: Option<String> = row.get(1);
    Ok(Some(VersionTarget::new(
        version,
        digest.context("provider switch head references a missing version")?,
    )))
}

fn active_provider_switches<C: GenericClient>(
    client: &mut C,
) -> Result<Option<ProviderSwitchSpec>> {
    let Some(row) = client.query_opt(
        "SELECT h.active_generation,v.content_digest
         FROM provider_switch_head h
         LEFT JOIN provider_switch_versions v ON v.generation=h.active_generation
         WHERE h.singleton=1",
        &[],
    )?
    else {
        return Ok(None);
    };
    let generation: i64 = row.get(0);
    let digest: Option<String> = row.get(1);
    digest.context("provider switch head references a missing version")?;
    switches_by_generation(client, generation, false)?
        .map(Some)
        .context("provider switch head references a missing version")
}

pub(crate) fn postgres_active_provider_switches(
    client: &mut Client,
) -> Result<Option<ProviderSwitchSpec>> {
    active_provider_switches(client)
}

fn policy_by_version<C: GenericClient>(
    client: &mut C,
    account_id: &str,
    effective_version: i64,
    key_share: bool,
) -> Result<Option<AccountPolicySpec>> {
    let sql = if key_share {
        "SELECT account_id,effective_version,policy_id,policy_version,source_policy_digest,
                owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
                switch_generation,content_digest,replacement_locked
         FROM account_policy_versions
         WHERE account_id=$1 AND effective_version=$2
         FOR KEY SHARE"
    } else {
        "SELECT account_id,effective_version,policy_id,policy_version,source_policy_digest,
                owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
                switch_generation,content_digest,replacement_locked
         FROM account_policy_versions
         WHERE account_id=$1 AND effective_version=$2"
    };
    let Some(header) = client.query_opt(sql, &[&account_id, &effective_version])? else {
        return Ok(None);
    };
    let owner_type: String = header.get(5);
    let account_class: String = header.get(7);
    let rows = client.query(
        "SELECT rule_id,rule_digest,scope_type,provider_id,canonical_model_id,
                pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
                track_eligible,retention_eligible,commission_eligible
         FROM account_policy_rules
         WHERE account_id=$1 AND effective_version=$2
         ORDER BY provider_id,scope_type,canonical_model_id,rule_id",
        &[&account_id, &effective_version],
    )?;
    let rules = rows
        .into_iter()
        .map(|row| {
            let scope_type: String = row.get(2);
            let provider_id: String = row.get(3);
            let canonical_model_id: Option<String> = row.get(4);
            let pricing_mode: String = row.get(5);
            let rule_origin: String = row.get(6);
            Ok(AccountPolicyRuleSpec {
                rule_id: row.get(0),
                rule_digest: row.get(1),
                scope: PolicyRuleScope::from_db(&scope_type, provider_id, canonical_model_id)?,
                pricing_mode: PricingMode::from_db(&pricing_mode)?,
                rule_origin: RuleOrigin::from_db(&rule_origin)?,
                discount_bps: row.get(7),
                payable_multiplier_bp: row.get(8),
                track_eligible: row.get(9),
                retention_eligible: row.get(10),
                commission_eligible: row.get(11),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(normalize_policy(&AccountPolicySpec {
        account_id: header.get(0),
        effective_version: header.get(1),
        policy_id: header.get(2),
        policy_version: header.get(3),
        source_policy_digest: header.get(4),
        owner_type: PolicyOwnerType::from_db(&owner_type)?,
        owner_id: header.get(6),
        account_class: AccountClass::from_db(&account_class)?,
        product_id: header.get(8),
        schema_version: header.get(9),
        catalog_generation: header.get(10),
        switch_generation: header.get(11),
        content_digest: header.get(12),
        replacement_locked: header.get(13),
        rules,
    })))
}

pub(crate) fn postgres_account_policy_by_version(
    client: &mut Client,
    account_id: &str,
    effective_version: i64,
) -> Result<Option<AccountPolicySpec>> {
    require_id("account id", account_id)?;
    if effective_version <= 0 {
        bail!("effective policy version must be positive");
    }
    policy_by_version(client, account_id, effective_version, false)
}

#[derive(Clone, Debug)]
struct StoredPolicyBinding {
    product_id: String,
    account_class: AccountClass,
    active_target: Option<VersionTarget>,
    binding: AccountPolicyBindingSpec,
}

impl StoredPolicyBinding {
    fn active_policy_target(&self) -> Option<ActivePolicyTarget> {
        self.active_target.clone().map(|target| ActivePolicyTarget {
            target,
            binding: self.binding.clone(),
        })
    }

    fn state(&self) -> PolicyBindingState {
        match self.active_policy_target() {
            Some(active) => PolicyBindingState::Active(active),
            None => PolicyBindingState::Inactive(self.binding.clone()),
        }
    }
}

fn policy_binding_from_row(row: postgres::Row) -> Result<StoredPolicyBinding> {
    let account_class: String = row.get(1);
    let active_version: Option<i64> = row.get(2);
    let active_digest: Option<String> = row.get(3);
    let policy_enforcement: String = row.get(4);
    let funding_enforcement: String = row.get(5);
    let reconciliation_state: String = row.get(6);
    let active_target = match (active_version, active_digest) {
        (None, None) => None,
        (Some(version), Some(digest)) => Some(VersionTarget::new(version, digest)),
        _ => bail!("account policy binding references a missing effective version"),
    };
    let binding = AccountPolicyBindingSpec {
        policy_enforcement: PolicyEnforcement::from_db(&policy_enforcement)?,
        funding_enforcement: FundingEnforcement::from_db(&funding_enforcement)?,
        reconciliation_state: ReconciliationState::from_db(&reconciliation_state)?,
    };
    validate_account_policy_binding(&binding)
        .context("validate stored PostgreSQL account policy binding")?;
    Ok(StoredPolicyBinding {
        product_id: row.get(0),
        account_class: AccountClass::from_db(&account_class)?,
        active_target,
        binding,
    })
}

fn policy_binding_locked(
    transaction: &mut Transaction<'_>,
    account_id: &str,
) -> Result<Option<StoredPolicyBinding>> {
    transaction
        .query_opt(
            "SELECT b.product_id,b.account_class,b.active_effective_version,v.content_digest,
                    b.policy_enforcement,b.funding_enforcement,b.reconciliation_state
             FROM account_policy_bindings b
             LEFT JOIN account_policy_versions v
               ON v.account_id=b.account_id
              AND v.effective_version=b.active_effective_version
              AND v.product_id=b.product_id
             WHERE b.account_id=$1
             FOR UPDATE OF b",
            &[&account_id],
        )?
        .map(policy_binding_from_row)
        .transpose()
}

fn policy_binding<C: GenericClient>(
    client: &mut C,
    account_id: &str,
) -> Result<Option<StoredPolicyBinding>> {
    client
        .query_opt(
            "SELECT b.product_id,b.account_class,b.active_effective_version,v.content_digest,
                    b.policy_enforcement,b.funding_enforcement,b.reconciliation_state
             FROM account_policy_bindings b
             LEFT JOIN account_policy_versions v
               ON v.account_id=b.account_id
              AND v.effective_version=b.active_effective_version
              AND v.product_id=b.product_id
             WHERE b.account_id=$1",
            &[&account_id],
        )?
        .map(policy_binding_from_row)
        .transpose()
}

pub(crate) fn postgres_active_account_policy(
    client: &mut Client,
    account_id: &str,
) -> Result<Option<ActiveAccountPolicy>> {
    require_id("account id", account_id)?;
    let Some(stored_binding) = policy_binding(client, account_id)? else {
        return Ok(None);
    };
    let Some(target) = &stored_binding.active_target else {
        return Ok(None);
    };
    let policy = policy_by_version(client, account_id, target.version, false)?
        .context("account policy binding references a missing effective version")?;
    if policy.content_digest != target.content_digest
        || policy.product_id != stored_binding.product_id
        || policy.account_class != stored_binding.account_class
    {
        bail!("account policy binding identity does not match its active policy");
    }
    Ok(Some(ActiveAccountPolicy {
        policy,
        binding: stored_binding.binding,
    }))
}

/// Read the account scalar, exact policy dependencies and independently moving admission heads
/// from one PostgreSQL snapshot.
///
/// `REPEATABLE READ` gives the multi-statement decoder one coherent historical view without
/// locking activation writers. The transaction is explicitly read-only: this dormant Stage 3B
/// primitive cannot move a head, change a binding, or affect billing state. An active policy must
/// resolve both immutable dependency versions; a missing pinned row is an integrity error rather
/// than an incomplete active bundle.
pub(crate) fn postgres_pricing_read_bundle(
    client: &mut Client,
    account_id: &str,
) -> Result<PricingReadBundle> {
    postgres_pricing_read_bundle_inner(client, account_id, None)
}

pub(crate) fn postgres_pricing_read_bundle_with_timeout(
    client: &mut Client,
    account_id: &str,
    timeout_ms: u64,
) -> Result<PricingReadBundle> {
    postgres_pricing_read_bundle_inner(client, account_id, Some(timeout_ms))
}

fn postgres_pricing_read_bundle_inner(
    client: &mut Client,
    account_id: &str,
    timeout_ms: Option<u64>,
) -> Result<PricingReadBundle> {
    require_id("account id", account_id)?;
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .context("begin PostgreSQL pricing read snapshot")?;
    if let Some(timeout_ms) = timeout_ms {
        set_shadow_transaction_timeout(&mut transaction, timeout_ms)?;
    }
    let account_multiplier_bp = transaction
        .query_opt("SELECT mult_bp FROM accounts WHERE id=$1", &[&account_id])?
        .map(|row| row.get::<_, i64>(0))
        .context("PostgreSQL pricing bundle account does not exist")?;

    let Some(stored_binding) = policy_binding(&mut transaction, account_id)? else {
        let bundle = PricingReadBundle {
            account_id: account_id.to_owned(),
            account_multiplier_bp,
            policy: PricingPolicySnapshot::Unbound,
            policy_catalog: None,
            policy_switches: None,
            admission_catalog: None,
            admission_switches: None,
        };
        transaction
            .commit()
            .context("commit PostgreSQL pricing read snapshot")?;
        return Ok(bundle);
    };

    let product_id = stored_binding.product_id.clone();
    let (policy, policy_catalog, policy_switches) = match stored_binding.active_target.as_ref() {
        None => (
            PricingPolicySnapshot::Inactive {
                product_id: product_id.clone(),
                account_class: stored_binding.account_class,
                binding: stored_binding.binding,
            },
            None,
            None,
        ),
        Some(target) => {
            let policy = policy_by_version(&mut transaction, account_id, target.version, false)?
                .context("account policy binding references a missing effective version")?;
            if policy.content_digest != target.content_digest
                || policy.product_id != product_id
                || policy.account_class != stored_binding.account_class
            {
                bail!("account policy binding identity does not match its active policy");
            }
            let policy_catalog = catalog_by_generation(
                &mut transaction,
                &policy.product_id,
                policy.catalog_generation,
                false,
            )?
            .context("active account policy references a missing pricing catalog generation")?;
            let policy_switches =
                switches_by_generation(&mut transaction, policy.switch_generation, false)?
                    .context(
                        "active account policy references a missing provider switch generation",
                    )?;
            (
                PricingPolicySnapshot::Active(ActiveAccountPolicy {
                    policy,
                    binding: stored_binding.binding,
                }),
                Some(policy_catalog),
                Some(policy_switches),
            )
        }
    };
    let admission_catalog = active_pricing_catalog(&mut transaction, &product_id)?;
    let admission_switches = active_provider_switches(&mut transaction)?;
    let bundle = PricingReadBundle {
        account_id: account_id.to_owned(),
        account_multiplier_bp,
        policy,
        policy_catalog,
        policy_switches,
        admission_catalog,
        admission_switches,
    };
    transaction
        .commit()
        .context("commit PostgreSQL pricing read snapshot")?;
    Ok(bundle)
}

fn newest_catalog_target(
    transaction: &mut Transaction<'_>,
    product_id: &str,
) -> Result<Option<VersionTarget>> {
    Ok(transaction
        .query_opt(
            "SELECT generation,content_digest
             FROM pricing_catalog_versions
             WHERE product_id=$1
             ORDER BY generation DESC
             LIMIT 1",
            &[&product_id],
        )?
        .map(|row| VersionTarget::new(row.get(0), row.get::<_, String>(1))))
}

fn insert_catalog(
    transaction: &mut Transaction<'_>,
    spec: &PricingCatalogSpec,
    created_ts: i64,
) -> Result<bool> {
    let inserted = transaction.execute(
        "INSERT INTO pricing_catalog_versions(
             product_id,generation,schema_version,capability_generation,
             capability_digest,content_digest,created_ts
         ) VALUES($1,$2,$3,$4,$5,$6,$7)
         ON CONFLICT(product_id,generation) DO NOTHING",
        &[
            &spec.product_id,
            &spec.generation,
            &spec.schema_version,
            &spec.capability_generation,
            &spec.capability_digest,
            &spec.content_digest,
            &created_ts,
        ],
    )? == 1;
    if !inserted {
        return Ok(false);
    }
    for entry in &spec.entries {
        transaction.execute(
            "INSERT INTO pricing_catalog_entries(
                 product_id,generation,provider_id,canonical_model_id,enabled
             ) VALUES($1,$2,$3,$4,$5)",
            &[
                &spec.product_id,
                &spec.generation,
                &entry.provider_id,
                &entry.canonical_model_id,
                &entry.enabled,
            ],
        )?;
    }
    Ok(true)
}

pub(crate) fn postgres_prepare_pricing_catalog(
    client: &mut Client,
    incoming: &PricingCatalogSpec,
) -> Result<PricingMutation> {
    let incoming = normalize_catalog(incoming);
    if let Err(error) = validate_pricing_catalog(&incoming) {
        return Ok(invalid(error));
    }

    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL pricing catalog prepare")?;
    advisory_lock(&mut transaction, &catalog_lock_key(&incoming.product_id))?;

    if let Some(existing) = catalog_by_generation(
        &mut transaction,
        &incoming.product_id,
        incoming.generation,
        true,
    )? {
        let outcome = if existing == incoming {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "pricing catalog prepare");
    }

    if let Some(newest) = newest_catalog_target(&mut transaction, &incoming.product_id)? {
        if incoming.generation < newest.version {
            return commit_mutation(transaction, stale(Some(newest)), "pricing catalog prepare");
        }
    }

    if !insert_catalog(&mut transaction, &incoming, now())? {
        let existing = catalog_by_generation(
            &mut transaction,
            &incoming.product_id,
            incoming.generation,
            true,
        )?
        .context("catalog insert conflict did not expose the conflicting version")?;
        let outcome = if existing == incoming {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "pricing catalog prepare");
    }
    commit_mutation(
        transaction,
        PricingMutation::Stored,
        "pricing catalog prepare",
    )
}

pub(crate) fn postgres_activate_pricing_catalog(
    client: &mut Client,
    product_id: &str,
    target: &VersionTarget,
    expectation: &ActiveExpectation,
) -> Result<PricingMutation> {
    if let Err(error) = require_id("product id", product_id)
        .and_then(|_| validate_version_target(target))
        .and_then(|_| validate_active_expectation(expectation))
    {
        return Ok(invalid(error));
    }

    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL pricing catalog activation")?;
    advisory_lock(&mut transaction, &catalog_lock_key(product_id))?;
    let actual = catalog_head_locked(&mut transaction, product_id)?;
    let Some(stored) = catalog_by_generation(&mut transaction, product_id, target.version, true)?
    else {
        return commit_mutation(
            transaction,
            missing(format!(
                "pricing catalog {product_id:?} generation {}",
                target.version
            )),
            "pricing catalog activation",
        );
    };
    if stored.content_digest != target.content_digest {
        return commit_mutation(
            transaction,
            version_conflict(),
            "pricing catalog activation",
        );
    }
    if actual.as_ref() == Some(target) {
        return commit_mutation(
            transaction,
            PricingMutation::Unchanged,
            "pricing catalog activation",
        );
    }
    if actual
        .as_ref()
        .is_some_and(|current| target.version < current.version)
    {
        return commit_mutation(transaction, stale(actual), "pricing catalog activation");
    }
    if !expectation_matches(expectation, actual.as_ref()) {
        return commit_mutation(
            transaction,
            cas_mismatch(actual),
            "pricing catalog activation",
        );
    }

    let updated_ts = now();
    let affected = match expectation {
        ActiveExpectation::Absent => transaction.execute(
            "INSERT INTO pricing_catalog_heads(product_id,active_generation,updated_ts)
             VALUES($1,$2,$3)
             ON CONFLICT(product_id) DO NOTHING",
            &[&product_id, &target.version, &updated_ts],
        )?,
        ActiveExpectation::Exact(expected) => transaction.execute(
            "UPDATE pricing_catalog_heads h
             SET active_generation=$2,updated_ts=$3
             WHERE h.product_id=$1
               AND h.active_generation=$4
               AND EXISTS (
                   SELECT 1
                   FROM pricing_catalog_versions v
                   WHERE v.product_id=h.product_id
                     AND v.generation=h.active_generation
                     AND v.content_digest=$5
               )",
            &[
                &product_id,
                &target.version,
                &updated_ts,
                &expected.version,
                &expected.content_digest,
            ],
        )?,
    };
    if affected != 1 {
        transaction
            .rollback()
            .context("rollback lost PostgreSQL pricing catalog CAS")?;
        return Ok(cas_mismatch(actual));
    }
    commit_mutation(
        transaction,
        PricingMutation::Applied,
        "pricing catalog activation",
    )
}

enum DependencyCheck<T> {
    Ready(T),
    Rejected(PricingMutation),
}

#[derive(Clone, Debug)]
struct SwitchDependencies {
    catalogs: Vec<(String, VersionTarget)>,
}

fn switch_dependencies(
    transaction: &mut Transaction<'_>,
    switches: &ProviderSwitchSpec,
) -> Result<DependencyCheck<SwitchDependencies>> {
    let mut targets = BTreeSet::new();
    for entry in &switches.entries {
        let (product_id, generation) = match (&entry.scope, entry.catalog_generation) {
            (
                ProviderSwitchScope::Product { product_id }
                | ProviderSwitchScope::Segment { product_id, .. },
                Some(generation),
            ) => (product_id, generation),
            _ => continue,
        };
        targets.insert((product_id.clone(), generation));
    }
    let mut catalogs = Vec::with_capacity(targets.len());
    for (product_id, generation) in targets {
        let Some(catalog) = catalog_by_generation(transaction, &product_id, generation, true)?
        else {
            return Ok(DependencyCheck::Rejected(missing(format!(
                "pricing catalog {product_id:?} generation {generation}"
            ))));
        };
        if catalog.capability_generation != switches.capability_generation
            || catalog.capability_digest != switches.capability_digest
        {
            return Ok(DependencyCheck::Rejected(invalid(anyhow::anyhow!(
                "provider switches and catalog {product_id:?} generation {generation} \
                 have different capability pins"
            ))));
        }
        if let Some(entry) = switches.entries.iter().find(|entry| {
            matches!(
                &entry.scope,
                ProviderSwitchScope::Product {
                    product_id: scoped_product
                } | ProviderSwitchScope::Segment {
                    product_id: scoped_product,
                    ..
                } if scoped_product == &product_id
            ) && entry.catalog_generation == Some(generation)
                && !catalog
                    .entries
                    .iter()
                    .any(|catalog_entry| catalog_entry.provider_id == entry.provider_id)
        }) {
            return Ok(DependencyCheck::Rejected(invalid(anyhow::anyhow!(
                "provider switch {:?} references provider {:?} absent from catalog {:?} \
                 generation {}",
                switches.content_digest,
                entry.provider_id,
                product_id,
                generation
            ))));
        }
        catalogs.push((product_id, catalog.target()));
    }
    Ok(DependencyCheck::Ready(SwitchDependencies { catalogs }))
}

fn active_switch_dependencies(
    transaction: &mut Transaction<'_>,
    dependencies: &SwitchDependencies,
) -> Result<Option<PricingMutation>> {
    for (product_id, expected) in &dependencies.catalogs {
        let actual = catalog_head_shared(transaction, product_id)?;
        if actual.as_ref() != Some(expected) {
            return Ok(Some(missing(format!(
                "active pricing catalog {product_id:?} target {expected:?}"
            ))));
        }
    }
    Ok(None)
}

fn newest_switch_target(transaction: &mut Transaction<'_>) -> Result<Option<VersionTarget>> {
    Ok(transaction
        .query_opt(
            "SELECT generation,content_digest
             FROM provider_switch_versions
             ORDER BY generation DESC
             LIMIT 1",
            &[],
        )?
        .map(|row| VersionTarget::new(row.get(0), row.get::<_, String>(1))))
}

fn insert_switches(
    transaction: &mut Transaction<'_>,
    spec: &ProviderSwitchSpec,
    created_ts: i64,
) -> Result<bool> {
    let inserted = transaction.execute(
        "INSERT INTO provider_switch_versions(
             generation,schema_version,capability_generation,capability_digest,
             content_digest,created_ts
         ) VALUES($1,$2,$3,$4,$5,$6)
         ON CONFLICT(generation) DO NOTHING",
        &[
            &spec.generation,
            &spec.schema_version,
            &spec.capability_generation,
            &spec.capability_digest,
            &spec.content_digest,
            &created_ts,
        ],
    )? == 1;
    if !inserted {
        return Ok(false);
    }
    for entry in &spec.entries {
        let (scope_type, product_id, segment) = entry.scope.db_parts();
        transaction.execute(
            "INSERT INTO provider_switch_entries(
                 generation,provider_id,scope_type,product_id,segment,catalog_generation,enabled
             ) VALUES($1,$2,$3,$4,$5,$6,$7)",
            &[
                &spec.generation,
                &entry.provider_id,
                &scope_type,
                &product_id,
                &segment,
                &entry.catalog_generation,
                &entry.enabled,
            ],
        )?;
    }
    Ok(true)
}

pub(crate) fn postgres_prepare_provider_switches(
    client: &mut Client,
    incoming: &ProviderSwitchSpec,
) -> Result<PricingMutation> {
    let incoming = normalize_switches(incoming);
    if let Err(error) = validate_provider_switches(&incoming) {
        return Ok(invalid(error));
    }

    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL provider switch prepare")?;
    advisory_lock(&mut transaction, SWITCH_LOCK_KEY)?;

    if let Some(existing) = switches_by_generation(&mut transaction, incoming.generation, true)? {
        let outcome = if existing == incoming {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "provider switch prepare");
    }
    if let Some(newest) = newest_switch_target(&mut transaction)? {
        if incoming.generation < newest.version {
            return commit_mutation(transaction, stale(Some(newest)), "provider switch prepare");
        }
    }
    match switch_dependencies(&mut transaction, &incoming)? {
        DependencyCheck::Ready(_) => {}
        DependencyCheck::Rejected(outcome) => {
            return commit_mutation(transaction, outcome, "provider switch prepare");
        }
    }
    if !insert_switches(&mut transaction, &incoming, now())? {
        let existing = switches_by_generation(&mut transaction, incoming.generation, true)?
            .context("switch insert conflict did not expose the conflicting version")?;
        let outcome = if existing == incoming {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "provider switch prepare");
    }
    commit_mutation(
        transaction,
        PricingMutation::Stored,
        "provider switch prepare",
    )
}

pub(crate) fn postgres_activate_provider_switches(
    client: &mut Client,
    target: &VersionTarget,
    expectation: &ActiveExpectation,
) -> Result<PricingMutation> {
    if let Err(error) =
        validate_version_target(target).and_then(|_| validate_active_expectation(expectation))
    {
        return Ok(invalid(error));
    }

    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL provider switch activation")?;
    advisory_lock(&mut transaction, SWITCH_LOCK_KEY)?;
    let actual = switch_head_locked(&mut transaction)?;
    let Some(stored) = switches_by_generation(&mut transaction, target.version, true)? else {
        return commit_mutation(
            transaction,
            missing(format!("provider switch generation {}", target.version)),
            "provider switch activation",
        );
    };
    if stored.content_digest != target.content_digest {
        return commit_mutation(
            transaction,
            version_conflict(),
            "provider switch activation",
        );
    }
    if actual.as_ref() == Some(target) {
        return commit_mutation(
            transaction,
            PricingMutation::Unchanged,
            "provider switch activation",
        );
    }
    if actual
        .as_ref()
        .is_some_and(|current| target.version < current.version)
    {
        return commit_mutation(transaction, stale(actual), "provider switch activation");
    }
    if !expectation_matches(expectation, actual.as_ref()) {
        return commit_mutation(
            transaction,
            cas_mismatch(actual),
            "provider switch activation",
        );
    }
    let dependencies = match switch_dependencies(&mut transaction, &stored)? {
        DependencyCheck::Ready(dependencies) => dependencies,
        DependencyCheck::Rejected(outcome) => {
            return commit_mutation(transaction, outcome, "provider switch activation");
        }
    };
    if let Some(outcome) = active_switch_dependencies(&mut transaction, &dependencies)? {
        return commit_mutation(transaction, outcome, "provider switch activation");
    }

    let updated_ts = now();
    let affected = match expectation {
        ActiveExpectation::Absent => transaction.execute(
            "INSERT INTO provider_switch_head(singleton,active_generation,updated_ts)
             VALUES(1,$1,$2)
             ON CONFLICT(singleton) DO NOTHING",
            &[&target.version, &updated_ts],
        )?,
        ActiveExpectation::Exact(expected) => transaction.execute(
            "UPDATE provider_switch_head h
             SET active_generation=$1,updated_ts=$2
             WHERE h.singleton=1
               AND h.active_generation=$3
               AND EXISTS (
                   SELECT 1
                   FROM provider_switch_versions v
                   WHERE v.generation=h.active_generation
                     AND v.content_digest=$4
               )",
            &[
                &target.version,
                &updated_ts,
                &expected.version,
                &expected.content_digest,
            ],
        )?,
    };
    if affected != 1 {
        transaction
            .rollback()
            .context("rollback lost PostgreSQL provider switch CAS")?;
        return Ok(cas_mismatch(actual));
    }
    commit_mutation(
        transaction,
        PricingMutation::Applied,
        "provider switch activation",
    )
}

#[derive(Clone, Debug)]
struct PolicyLineage {
    target: VersionTarget,
    policy_id: String,
    policy_version: i64,
    source_policy_digest: String,
    owner_type: PolicyOwnerType,
    owner_id: String,
    account_class: AccountClass,
    product_id: String,
}

fn policy_lineage_from_row(row: postgres::Row) -> Result<PolicyLineage> {
    let owner_type: String = row.get(5);
    let account_class: String = row.get(7);
    Ok(PolicyLineage {
        target: VersionTarget::new(row.get(0), row.get::<_, String>(1)),
        policy_id: row.get(2),
        policy_version: row.get(3),
        source_policy_digest: row.get(4),
        owner_type: PolicyOwnerType::from_db(&owner_type)?,
        owner_id: row.get(6),
        account_class: AccountClass::from_db(&account_class)?,
        product_id: row.get(8),
    })
}

fn newest_policy_lineage(
    transaction: &mut Transaction<'_>,
    account_id: &str,
) -> Result<Option<PolicyLineage>> {
    transaction
        .query_opt(
            "SELECT effective_version,content_digest,policy_id,policy_version,
                    source_policy_digest,owner_type,owner_id,account_class,product_id
             FROM account_policy_versions
             WHERE account_id=$1
             ORDER BY effective_version DESC
             LIMIT 1",
            &[&account_id],
        )?
        .map(policy_lineage_from_row)
        .transpose()
}

fn locked_policy_lineage(
    transaction: &mut Transaction<'_>,
    account_id: &str,
) -> Result<Option<PolicyLineage>> {
    transaction
        .query_opt(
            "SELECT effective_version,content_digest,policy_id,policy_version,
                    source_policy_digest,owner_type,owner_id,account_class,product_id
             FROM account_policy_versions
             WHERE account_id=$1 AND replacement_locked
             ORDER BY effective_version
             LIMIT 1
             FOR KEY SHARE",
            &[&account_id],
        )?
        .map(policy_lineage_from_row)
        .transpose()
}

fn policy_identity_matches(lineage: &PolicyLineage, incoming: &AccountPolicySpec) -> bool {
    lineage.policy_id == incoming.policy_id
        && lineage.owner_type == incoming.owner_type
        && lineage.owner_id == incoming.owner_id
        && lineage.account_class == incoming.account_class
        && lineage.product_id == incoming.product_id
}

fn binding_identity_matches(binding: &StoredPolicyBinding, policy: &AccountPolicySpec) -> bool {
    binding.product_id == policy.product_id && binding.account_class == policy.account_class
}

#[derive(Clone, Debug)]
struct PolicyDependencies {
    catalog: PricingCatalogSpec,
    switches: ProviderSwitchSpec,
}

fn policy_dependencies(
    transaction: &mut Transaction<'_>,
    policy: &AccountPolicySpec,
) -> Result<DependencyCheck<PolicyDependencies>> {
    let Some(catalog) = catalog_by_generation(
        transaction,
        &policy.product_id,
        policy.catalog_generation,
        true,
    )?
    else {
        return Ok(DependencyCheck::Rejected(missing(format!(
            "pricing catalog {:?} generation {}",
            policy.product_id, policy.catalog_generation
        ))));
    };
    let Some(switches) = switches_by_generation(transaction, policy.switch_generation, true)?
    else {
        return Ok(DependencyCheck::Rejected(missing(format!(
            "provider switch generation {}",
            policy.switch_generation
        ))));
    };
    let Some(account) = transaction.query_opt(
        "SELECT mult_bp
         FROM accounts
         WHERE id=$1
         FOR SHARE",
        &[&policy.account_id],
    )?
    else {
        return Ok(DependencyCheck::Rejected(missing(format!(
            "engine account {:?}",
            policy.account_id
        ))));
    };
    let legacy_multiplier_bp: i64 = account.get(0);
    if let Err(error) =
        validate_account_policy(policy, &catalog, &switches, Some(legacy_multiplier_bp))
    {
        return Ok(DependencyCheck::Rejected(invalid(error)));
    }
    Ok(DependencyCheck::Ready(PolicyDependencies {
        catalog,
        switches,
    }))
}

fn active_policy_dependencies(
    transaction: &mut Transaction<'_>,
    dependencies: &PolicyDependencies,
) -> Result<Option<PricingMutation>> {
    // Keep the same lock order as switch activation: switch head first, then catalog heads.
    // Otherwise policy activation could deadlock with switch activation while each held one head.
    let expected_switch = dependencies.switches.target();
    let actual_switch = switch_head_shared(transaction)?;
    if actual_switch.as_ref() != Some(&expected_switch) {
        return Ok(Some(missing(format!(
            "active provider switch target {expected_switch:?}"
        ))));
    }

    let expected_catalog = dependencies.catalog.target();
    let actual_catalog = catalog_head_shared(transaction, &dependencies.catalog.product_id)?;
    if actual_catalog.as_ref() != Some(&expected_catalog) {
        return Ok(Some(missing(format!(
            "active pricing catalog {:?} target {expected_catalog:?}",
            dependencies.catalog.product_id
        ))));
    }
    Ok(None)
}

fn insert_policy(
    transaction: &mut Transaction<'_>,
    spec: &AccountPolicySpec,
    created_ts: i64,
) -> Result<bool> {
    let inserted = transaction.execute(
        "INSERT INTO account_policy_versions(
             account_id,effective_version,policy_id,policy_version,source_policy_digest,
             owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
             switch_generation,content_digest,replacement_locked,created_ts
         ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
         ON CONFLICT(account_id,effective_version) DO NOTHING",
        &[
            &spec.account_id,
            &spec.effective_version,
            &spec.policy_id,
            &spec.policy_version,
            &spec.source_policy_digest,
            &spec.owner_type.as_str(),
            &spec.owner_id,
            &spec.account_class.as_str(),
            &spec.product_id,
            &spec.schema_version,
            &spec.catalog_generation,
            &spec.switch_generation,
            &spec.content_digest,
            &spec.replacement_locked,
            &created_ts,
        ],
    )? == 1;
    if !inserted {
        return Ok(false);
    }
    for rule in &spec.rules {
        let (scope_type, provider_id, canonical_model_id) = rule.scope.db_parts();
        transaction.execute(
            "INSERT INTO account_policy_rules(
                 account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                 canonical_model_id,pricing_mode,rule_origin,discount_bps,
                 payable_multiplier_bp,track_eligible,retention_eligible,commission_eligible
             ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
            &[
                &spec.account_id,
                &spec.effective_version,
                &rule.rule_id,
                &rule.rule_digest,
                &scope_type,
                &provider_id,
                &canonical_model_id,
                &rule.pricing_mode.as_str(),
                &rule.rule_origin.as_str(),
                &rule.discount_bps,
                &rule.payable_multiplier_bp,
                &rule.track_eligible,
                &rule.retention_eligible,
                &rule.commission_eligible,
            ],
        )?;
    }
    Ok(true)
}

pub(crate) fn postgres_prepare_account_policy(
    client: &mut Client,
    incoming: &AccountPolicySpec,
) -> Result<PricingMutation> {
    let incoming = normalize_policy(incoming);
    if let Err(error) = validate_account_policy_shape(&incoming) {
        return Ok(invalid(error));
    }
    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL account policy prepare")?;
    advisory_lock(&mut transaction, &policy_lock_key(&incoming.account_id))?;
    let current_binding = policy_binding_locked(&mut transaction, &incoming.account_id)?;

    if let Some(binding) = &current_binding {
        if !binding_identity_matches(binding, &incoming) {
            return commit_mutation(transaction, version_conflict(), "account policy prepare");
        }
    }
    if let Some(existing) = policy_by_version(
        &mut transaction,
        &incoming.account_id,
        incoming.effective_version,
        true,
    )? {
        let outcome = if existing == incoming {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "account policy prepare");
    }

    let newest = newest_policy_lineage(&mut transaction, &incoming.account_id)?;
    if let Some(newest) = &newest {
        if incoming.effective_version < newest.target.version
            || incoming.policy_version < newest.policy_version
        {
            return commit_mutation(
                transaction,
                stale(Some(newest.target.clone())),
                "account policy prepare",
            );
        }
        if !policy_identity_matches(newest, &incoming)
            || (incoming.policy_version == newest.policy_version
                && incoming.source_policy_digest != newest.source_policy_digest)
        {
            return commit_mutation(transaction, version_conflict(), "account policy prepare");
        }
    }
    if locked_policy_lineage(&mut transaction, &incoming.account_id)?.is_some() {
        return commit_mutation(transaction, locked(), "account policy prepare");
    }
    match policy_dependencies(&mut transaction, &incoming)? {
        DependencyCheck::Ready(_) => {}
        DependencyCheck::Rejected(outcome) => {
            return commit_mutation(transaction, outcome, "account policy prepare");
        }
    }
    if !insert_policy(&mut transaction, &incoming, now())? {
        let existing = policy_by_version(
            &mut transaction,
            &incoming.account_id,
            incoming.effective_version,
            true,
        )?
        .context("policy insert conflict did not expose the conflicting version")?;
        let outcome = if existing == incoming {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "account policy prepare");
    }
    commit_mutation(
        transaction,
        PricingMutation::Stored,
        "account policy prepare",
    )
}

pub(crate) fn postgres_activate_account_policy(
    client: &mut Client,
    activation: &AccountPolicyActivationSpec,
    expectation: &PolicyActiveExpectation,
) -> Result<PricingMutation> {
    if let Err(error) = validate_account_policy_activation(activation)
        .and_then(|_| validate_policy_active_expectation(expectation))
    {
        return Ok(invalid(error));
    }
    let target = VersionTarget::new(
        activation.effective_version,
        activation.content_digest.clone(),
    );

    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL account policy activation")?;
    advisory_lock(&mut transaction, &policy_lock_key(&activation.account_id))?;
    let current_binding = policy_binding_locked(&mut transaction, &activation.account_id)?;
    let actual_state = current_binding
        .as_ref()
        .map(StoredPolicyBinding::state)
        .unwrap_or(PolicyBindingState::Unbound);

    let Some(policy) = policy_by_version(
        &mut transaction,
        &activation.account_id,
        activation.effective_version,
        true,
    )?
    else {
        return commit_mutation(
            transaction,
            missing(format!(
                "account policy {:?} effective version {}",
                activation.account_id, activation.effective_version
            )),
            "account policy activation",
        );
    };
    if policy.content_digest != activation.content_digest {
        return commit_mutation(transaction, version_conflict(), "account policy activation");
    }
    if let Some(binding) = &current_binding {
        if !binding_identity_matches(binding, &policy) {
            return commit_mutation(transaction, version_conflict(), "account policy activation");
        }
    }

    let desired = ActivePolicyTarget {
        target: target.clone(),
        binding: activation.binding.clone(),
    };
    if actual_state == PolicyBindingState::Active(desired) {
        return commit_mutation(
            transaction,
            PricingMutation::Unchanged,
            "account policy activation",
        );
    }
    if current_binding
        .as_ref()
        .and_then(|binding| binding.active_target.as_ref())
        .is_some_and(|current| target.version < current.version)
    {
        let actual = current_binding
            .as_ref()
            .and_then(|binding| binding.active_target.clone());
        return commit_mutation(transaction, stale(actual), "account policy activation");
    }
    if !policy_expectation_matches(expectation, &actual_state) {
        return commit_mutation(
            transaction,
            policy_cas_mismatch(actual_state),
            "account policy activation",
        );
    }
    if let Some(locked_history) = locked_policy_lineage(&mut transaction, &activation.account_id)? {
        if locked_history.target != target {
            return commit_mutation(transaction, locked(), "account policy activation");
        }
    }
    let dependencies = match policy_dependencies(&mut transaction, &policy)? {
        DependencyCheck::Ready(dependencies) => dependencies,
        DependencyCheck::Rejected(outcome) => {
            return commit_mutation(transaction, outcome, "account policy activation");
        }
    };
    if let Some(outcome) = active_policy_dependencies(&mut transaction, &dependencies)? {
        return commit_mutation(transaction, outcome, "account policy activation");
    }

    let updated_ts = now();
    let affected = match expectation {
        PolicyActiveExpectation::Unbound => transaction.execute(
            "INSERT INTO account_policy_bindings(
                 account_id,product_id,account_class,active_effective_version,
                 policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
             ) VALUES($1,$2,$3,$4,$5,$6,$7,$8)
             ON CONFLICT(account_id) DO NOTHING",
            &[
                &activation.account_id,
                &policy.product_id,
                &policy.account_class.as_str(),
                &activation.effective_version,
                &activation.binding.policy_enforcement.as_str(),
                &activation.binding.funding_enforcement.as_str(),
                &activation.binding.reconciliation_state.as_str(),
                &updated_ts,
            ],
        )?,
        PolicyActiveExpectation::Inactive(expected) => transaction.execute(
            "UPDATE account_policy_bindings
             SET active_effective_version=$2,
                 policy_enforcement=$3,
                 funding_enforcement=$4,
                 reconciliation_state=$5,
                 updated_ts=$6
             WHERE account_id=$1
               AND active_effective_version IS NULL
               AND policy_enforcement=$7
               AND funding_enforcement=$8
               AND reconciliation_state=$9
               AND product_id=$10
               AND account_class=$11",
            &[
                &activation.account_id,
                &activation.effective_version,
                &activation.binding.policy_enforcement.as_str(),
                &activation.binding.funding_enforcement.as_str(),
                &activation.binding.reconciliation_state.as_str(),
                &updated_ts,
                &expected.policy_enforcement.as_str(),
                &expected.funding_enforcement.as_str(),
                &expected.reconciliation_state.as_str(),
                &policy.product_id,
                &policy.account_class.as_str(),
            ],
        )?,
        PolicyActiveExpectation::Exact(expected) => transaction.execute(
            "UPDATE account_policy_bindings b
             SET active_effective_version=$2,
                 policy_enforcement=$3,
                 funding_enforcement=$4,
                 reconciliation_state=$5,
                 updated_ts=$6
             WHERE b.account_id=$1
               AND b.active_effective_version=$7
               AND b.policy_enforcement=$8
               AND b.funding_enforcement=$9
               AND b.reconciliation_state=$10
               AND EXISTS (
                   SELECT 1
                   FROM account_policy_versions v
                   WHERE v.account_id=b.account_id
                     AND v.effective_version=b.active_effective_version
                     AND v.product_id=b.product_id
                     AND v.content_digest=$11
               )
               AND b.product_id=$12
               AND b.account_class=$13",
            &[
                &activation.account_id,
                &activation.effective_version,
                &activation.binding.policy_enforcement.as_str(),
                &activation.binding.funding_enforcement.as_str(),
                &activation.binding.reconciliation_state.as_str(),
                &updated_ts,
                &expected.target.version,
                &expected.binding.policy_enforcement.as_str(),
                &expected.binding.funding_enforcement.as_str(),
                &expected.binding.reconciliation_state.as_str(),
                &expected.target.content_digest,
                &policy.product_id,
                &policy.account_class.as_str(),
            ],
        )?,
    };
    if affected != 1 {
        transaction
            .rollback()
            .context("rollback lost PostgreSQL account policy CAS")?;
        return Ok(policy_cas_mismatch(actual_state));
    }
    commit_mutation(
        transaction,
        PricingMutation::Applied,
        "account policy activation",
    )
}

fn release_policy_v2_in_transaction<C: GenericClient>(
    client: &mut C,
    policy_id: &str,
    policy_version: i64,
) -> Result<Option<super::PricingReleasePolicyV2>> {
    let Some(row) = client
        .query_opt(
            "SELECT policy_id,policy_version,owner_type,owner_id,account_class,product_id,
                    billing_mode,schema_version,capability_generation,capability_digest,
                    catalog_generation,catalog_digest,switch_generation,switch_digest,
                    content_digest
               FROM pricing_release_policy_versions
              WHERE policy_id=$1 AND policy_version=$2",
            &[&policy_id, &policy_version],
        )
        .context("read PostgreSQL pricing release policy v2")?
    else {
        return Ok(None);
    };
    let rules = client
        .query(
            "SELECT rule_id,rule_digest,scope_type,provider_id,canonical_model_id,
                    discount_bps,payable_multiplier_bp
               FROM pricing_release_policy_rules
              WHERE policy_id=$1 AND policy_version=$2
              ORDER BY scope_type,provider_id,canonical_model_id,rule_id",
            &[&policy_id, &policy_version],
        )?
        .into_iter()
        .map(|rule| {
            Ok(super::PricingReleasePolicyRuleV2 {
                rule_id: rule.get(0),
                rule_digest: rule.get(1),
                scope: super::PricingReleaseRuleScopeV2::from_db(
                    &rule.get::<_, String>(2),
                    rule.get(3),
                    rule.get(4),
                )?,
                discount_bps: rule.get(5),
                payable_multiplier_bp: rule.get(6),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(
        super::release_v2::normalize_pricing_release_policy_v2(&super::PricingReleasePolicyV2 {
            policy_id: row.get(0),
            policy_version: row.get(1),
            owner_type: super::PolicyOwnerType::from_db(&row.get::<_, String>(2))?,
            owner_id: row.get(3),
            account_class: super::AccountClass::from_db(&row.get::<_, String>(4))?,
            product_id: row.get(5),
            billing_mode: super::BillingModeV2::from_db(&row.get::<_, String>(6))?,
            schema_version: row.get(7),
            capability_generation: row.get(8),
            capability_digest: row.get(9),
            catalog_generation: row.get(10),
            catalog_digest: row.get(11),
            switch_generation: row.get(12),
            switch_digest: row.get(13),
            content_digest: row.get(14),
            rules,
        }),
    ))
}

pub(crate) fn postgres_pricing_release_policy_v2(
    client: &mut Client,
    policy_id: &str,
    policy_version: i64,
) -> Result<Option<super::PricingReleasePolicyV2>> {
    super::require_id("pricing release policy id", policy_id)?;
    if policy_version <= 0 {
        bail!("pricing release policy version must be positive");
    }
    release_policy_v2_in_transaction(client, policy_id, policy_version)
}

pub(crate) fn postgres_prepare_pricing_release_policy_v2(
    client: &mut Client,
    policy: &super::PricingReleasePolicyV2,
) -> Result<PricingMutation> {
    if let Err(error) = super::validate_pricing_release_policy_v2(policy) {
        return Ok(invalid(error));
    }
    let policy = super::release_v2::normalize_pricing_release_policy_v2(policy);
    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL pricing release policy v2 prepare")?;
    advisory_lock(
        &mut transaction,
        &format!("pricing-release-v2:policy:{}", policy.policy_id),
    )?;
    if let Some(existing) = release_policy_v2_in_transaction(
        &mut transaction,
        &policy.policy_id,
        policy.policy_version,
    )? {
        let outcome = if existing == policy {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "pricing release policy v2 replay");
    }
    if let Some(newest) = transaction.query_opt(
        "SELECT policy_version,content_digest
           FROM pricing_release_policy_versions
          WHERE policy_id=$1
          ORDER BY policy_version DESC
          LIMIT 1",
        &[&policy.policy_id],
    )? {
        let newest = VersionTarget::new(newest.get(0), newest.get::<_, String>(1));
        if policy.policy_version < newest.version {
            return commit_mutation(
                transaction,
                stale(Some(newest)),
                "pricing release policy v2 stale prepare",
            );
        }
    }

    if let (
        Some(product_id),
        Some(catalog_generation),
        Some(catalog_digest),
        Some(switch_generation),
        Some(switch_digest),
    ) = (
        policy.product_id.as_deref(),
        policy.catalog_generation,
        policy.catalog_digest.as_deref(),
        policy.switch_generation,
        policy.switch_digest.as_deref(),
    ) {
        let catalog_ready: bool = transaction
            .query_one(
                "SELECT EXISTS(
                 SELECT 1 FROM pricing_catalog_versions
                  WHERE product_id=$1 AND generation=$2 AND content_digest=$3
                    AND capability_generation=$4 AND capability_digest=$5
             )",
                &[
                    &product_id,
                    &catalog_generation,
                    &catalog_digest,
                    &policy.capability_generation,
                    &policy.capability_digest,
                ],
            )?
            .get(0);
        if !catalog_ready {
            return commit_mutation(
                transaction,
                missing("catalog"),
                "pricing release policy v2 missing catalog",
            );
        }
        let switches_ready: bool = transaction
            .query_one(
                "SELECT EXISTS(
                 SELECT 1 FROM provider_switch_versions
                  WHERE generation=$1 AND content_digest=$2
                    AND capability_generation=$3 AND capability_digest=$4
             )",
                &[
                    &switch_generation,
                    &switch_digest,
                    &policy.capability_generation,
                    &policy.capability_digest,
                ],
            )?
            .get(0);
        if !switches_ready {
            return commit_mutation(
                transaction,
                missing("provider_switches"),
                "pricing release policy v2 missing switches",
            );
        }
        for rule in &policy.rules {
            let (_, provider_id, canonical_model_id) = rule.scope.db_parts();
            let Some(provider_id) = provider_id else {
                continue;
            };
            let rule_dependency_ready: bool = transaction
                .query_one(
                    "SELECT EXISTS(
                     SELECT 1 FROM pricing_catalog_entries
                      WHERE product_id=$1 AND generation=$2 AND provider_id=$3
                        AND ($4::text IS NULL OR canonical_model_id=$4)
                 )",
                    &[
                        &product_id,
                        &catalog_generation,
                        &provider_id,
                        &canonical_model_id,
                    ],
                )?
                .get(0);
            if !rule_dependency_ready {
                return commit_mutation(
                    transaction,
                    missing(&format!("policy_rule:{}", rule.rule_id)),
                    "pricing release policy v2 missing rule dependency",
                );
            }
        }
    }

    let created_ts = now();
    transaction.execute(
        "INSERT INTO pricing_release_policy_versions(
             policy_id,policy_version,owner_type,owner_id,account_class,product_id,billing_mode,
             schema_version,capability_generation,capability_digest,catalog_generation,
             catalog_digest,switch_generation,switch_digest,content_digest,created_ts
         ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
        &[
            &policy.policy_id,
            &policy.policy_version,
            &policy.owner_type.as_str(),
            &policy.owner_id,
            &policy.account_class.as_str(),
            &policy.product_id,
            &policy.billing_mode.as_str(),
            &policy.schema_version,
            &policy.capability_generation,
            &policy.capability_digest,
            &policy.catalog_generation,
            &policy.catalog_digest,
            &policy.switch_generation,
            &policy.switch_digest,
            &policy.content_digest,
            &created_ts,
        ],
    )?;
    for rule in &policy.rules {
        let (scope_type, provider_id, canonical_model_id) = rule.scope.db_parts();
        transaction.execute(
            "INSERT INTO pricing_release_policy_rules(
                 policy_id,policy_version,rule_id,rule_digest,scope_type,provider_id,
                 canonical_model_id,discount_bps,payable_multiplier_bp
             ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            &[
                &policy.policy_id,
                &policy.policy_version,
                &rule.rule_id,
                &rule.rule_digest,
                &scope_type,
                &provider_id,
                &canonical_model_id,
                &rule.discount_bps,
                &rule.payable_multiplier_bp,
            ],
        )?;
    }
    commit_mutation(
        transaction,
        PricingMutation::Stored,
        "pricing release policy v2 prepare",
    )
}

pub(crate) fn pricing_release_v2_in_transaction<C: GenericClient>(
    client: &mut C,
    generation: i64,
) -> Result<Option<super::PricingReleaseV2>> {
    let Some(row) = client.query_opt(
        "SELECT generation,release_kind,schema_version,capability_generation,
                capability_digest,main_catalog_generation,main_catalog_digest,
                openkeys_catalog_generation,openkeys_catalog_digest,switch_generation,
                switch_digest,inventory_digest,policy_manifest_digest,assignment_manifest_digest,
                funding_manifest_digest,minimum_runtime_schema_version,content_digest
           FROM pricing_release_versions WHERE generation=$1",
        &[&generation],
    )?
    else {
        return Ok(None);
    };
    let assignments = client
        .query(
            "SELECT account_id,account_class,policy_id,policy_version,policy_digest,billing_mode,
                funding_generation,purpose,responsible,assignment_digest
           FROM pricing_release_assignments
          WHERE release_generation=$1 ORDER BY account_id",
            &[&generation],
        )?
        .into_iter()
        .map(|assignment| {
            Ok(super::PricingReleaseAssignmentV2 {
                account_id: assignment.get(0),
                account_class: super::AccountClass::from_db(&assignment.get::<_, String>(1))?,
                policy_id: assignment.get(2),
                policy_version: assignment.get(3),
                policy_digest: assignment.get(4),
                billing_mode: super::BillingModeV2::from_db(&assignment.get::<_, String>(5))?,
                funding_generation: assignment.get(6),
                purpose: assignment.get(7),
                responsible: assignment.get(8),
                assignment_digest: assignment.get(9),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(super::PricingReleaseV2 {
        generation: row.get(0),
        release_kind: super::PricingReleaseKindV2::from_db(&row.get::<_, String>(1))?,
        schema_version: row.get(2),
        capability_generation: row.get(3),
        capability_digest: row.get(4),
        main_catalog_generation: row.get(5),
        main_catalog_digest: row.get(6),
        openkeys_catalog_generation: row.get(7),
        openkeys_catalog_digest: row.get(8),
        switch_generation: row.get(9),
        switch_digest: row.get(10),
        inventory_digest: row.get(11),
        policy_manifest_digest: row.get(12),
        assignment_manifest_digest: row.get(13),
        funding_manifest_digest: row.get(14),
        minimum_runtime_schema_version: row.get(15),
        content_digest: row.get(16),
        assignments,
    }))
}

pub(crate) fn postgres_pricing_release_v2(
    client: &mut Client,
    generation: i64,
) -> Result<Option<super::PricingReleaseV2>> {
    if generation <= 0 {
        bail!("pricing release generation must be positive");
    }
    pricing_release_v2_in_transaction(client, generation)
}

pub(crate) fn postgres_prepare_pricing_release_v2(
    client: &mut Client,
    release: &super::PricingReleaseV2,
) -> Result<PricingMutation> {
    if let Err(error) = super::validate_pricing_release_v2(release) {
        return Ok(invalid(error));
    }
    let release = super::release_v2::normalize_pricing_release_v2(release);
    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL pricing release v2 prepare")?;
    advisory_lock(&mut transaction, PRICING_RELEASE_CONTROL_LOCK_V2)?;
    if let Some(existing) = pricing_release_v2_in_transaction(&mut transaction, release.generation)?
    {
        let outcome = if existing == release {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "pricing release v2 replay");
    }
    if let Some(newest) = transaction.query_opt(
        "SELECT generation,content_digest
           FROM pricing_release_versions
          ORDER BY generation DESC
          LIMIT 1",
        &[],
    )? {
        let newest = VersionTarget::new(newest.get(0), newest.get::<_, String>(1));
        if release.generation < newest.version {
            return commit_mutation(
                transaction,
                stale(Some(newest)),
                "pricing release v2 stale prepare",
            );
        }
    }

    for (product_id, generation, digest) in [
        (
            "main",
            release.main_catalog_generation,
            release.main_catalog_digest.as_str(),
        ),
        (
            "openkeys",
            release.openkeys_catalog_generation,
            release.openkeys_catalog_digest.as_str(),
        ),
    ] {
        let exists: bool = transaction
            .query_one(
                "SELECT EXISTS(
                 SELECT 1 FROM pricing_catalog_versions
                  WHERE product_id=$1 AND generation=$2 AND content_digest=$3
                    AND capability_generation=$4 AND capability_digest=$5
             )",
                &[
                    &product_id,
                    &generation,
                    &digest,
                    &release.capability_generation,
                    &release.capability_digest,
                ],
            )?
            .get(0);
        if !exists {
            return commit_mutation(
                transaction,
                missing(&format!("catalog:{product_id}")),
                "pricing release v2 missing catalog",
            );
        }
    }
    let switches_exist: bool = transaction
        .query_one(
            "SELECT EXISTS(
             SELECT 1 FROM provider_switch_versions
              WHERE generation=$1 AND content_digest=$2
                AND capability_generation=$3 AND capability_digest=$4
         )",
            &[
                &release.switch_generation,
                &release.switch_digest,
                &release.capability_generation,
                &release.capability_digest,
            ],
        )?
        .get(0);
    if !switches_exist {
        return commit_mutation(
            transaction,
            missing("provider_switches"),
            "pricing release v2 missing switches",
        );
    }

    let inventory_count: i64 = transaction
        .query_one("SELECT count(*)::bigint FROM accounts", &[])?
        .get(0);
    if inventory_count != release.assignments.len() as i64 {
        return commit_mutation(
            transaction,
            invalid(anyhow::anyhow!(
                "release assignments do not cover the exact engine account inventory"
            )),
            "pricing release v2 incomplete inventory",
        );
    }
    for assignment in &release.assignments {
        let assignment_ready: bool = transaction
            .query_one(
                "SELECT EXISTS(
                 SELECT 1
                   FROM accounts account
                   JOIN pricing_release_policy_versions policy
                     ON policy.policy_id=$2 AND policy.policy_version=$3
                    AND policy.content_digest=$4
                  WHERE account.id=$1
                    AND policy.account_class=$5 AND policy.billing_mode=$6
                    AND (
                        ($6='meter_only' AND $7::bigint IS NULL)
                        OR ($6='balance' AND EXISTS(
                            SELECT 1 FROM account_funding_generations_v2 generation
                             WHERE generation.account_id=account.id
                               AND generation.generation=$7
                        ))
                    )
             )",
                &[
                    &assignment.account_id,
                    &assignment.policy_id,
                    &assignment.policy_version,
                    &assignment.policy_digest,
                    &assignment.account_class.as_str(),
                    &assignment.billing_mode.as_str(),
                    &assignment.funding_generation,
                ],
            )?
            .get(0);
        if !assignment_ready {
            return commit_mutation(
                transaction,
                missing(&format!("assignment:{}", assignment.account_id)),
                "pricing release v2 invalid assignment dependency",
            );
        }
    }

    let created_ts = now();
    transaction.execute(
        "INSERT INTO pricing_release_versions(
             generation,release_kind,schema_version,capability_generation,capability_digest,
             main_catalog_generation,main_catalog_digest,openkeys_catalog_generation,
             openkeys_catalog_digest,switch_generation,switch_digest,inventory_digest,
             policy_manifest_digest,assignment_manifest_digest,funding_manifest_digest,
             minimum_runtime_schema_version,content_digest,created_ts
         ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)",
        &[
            &release.generation,
            &release.release_kind.as_str(),
            &release.schema_version,
            &release.capability_generation,
            &release.capability_digest,
            &release.main_catalog_generation,
            &release.main_catalog_digest,
            &release.openkeys_catalog_generation,
            &release.openkeys_catalog_digest,
            &release.switch_generation,
            &release.switch_digest,
            &release.inventory_digest,
            &release.policy_manifest_digest,
            &release.assignment_manifest_digest,
            &release.funding_manifest_digest,
            &release.minimum_runtime_schema_version,
            &release.content_digest,
            &created_ts,
        ],
    )?;
    for assignment in &release.assignments {
        transaction.execute(
            "INSERT INTO pricing_release_assignments(
                 release_generation,account_id,account_class,policy_id,policy_version,
                 policy_digest,billing_mode,funding_generation,purpose,responsible,
                 assignment_digest
             ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
            &[
                &release.generation,
                &assignment.account_id,
                &assignment.account_class.as_str(),
                &assignment.policy_id,
                &assignment.policy_version,
                &assignment.policy_digest,
                &assignment.billing_mode.as_str(),
                &assignment.funding_generation,
                &assignment.purpose,
                &assignment.responsible,
                &assignment.assignment_digest,
            ],
        )?;
    }
    commit_mutation(
        transaction,
        PricingMutation::Stored,
        "pricing release v2 prepare",
    )
}

pub(crate) fn postgres_prepare_pricing_release_recovery_link_v2(
    client: &mut Client,
    link: &super::PricingReleaseRecoveryLinkV2,
) -> Result<PricingMutation> {
    if let Err(error) = super::validate_pricing_release_recovery_link_v2(link) {
        return Ok(invalid(error));
    }
    let mut transaction = client.transaction()?;
    advisory_lock(&mut transaction, PRICING_RELEASE_CONTROL_LOCK_V2)?;
    if let Some(row) = transaction.query_opt(
        "SELECT target_digest,recovery_digest,link_digest
           FROM pricing_release_recovery_links
          WHERE target_generation=$1 AND recovery_generation=$2",
        &[&link.target_generation, &link.recovery_generation],
    )? {
        let exact = row.get::<_, String>(0) == link.target_digest
            && row.get::<_, String>(1) == link.recovery_digest
            && row.get::<_, String>(2) == link.link_digest;
        return commit_mutation(
            transaction,
            if exact {
                PricingMutation::Unchanged
            } else {
                version_conflict()
            },
            "pricing release recovery link v2 replay",
        );
    }
    for (label, generation, digest, expected_kind) in [
        (
            "target_release",
            link.target_generation,
            link.target_digest.as_str(),
            "target",
        ),
        (
            "recovery_release",
            link.recovery_generation,
            link.recovery_digest.as_str(),
            "recovery",
        ),
    ] {
        let Some(release) = transaction.query_opt(
            "SELECT release_kind,content_digest
               FROM pricing_release_versions
              WHERE generation=$1",
            &[&generation],
        )?
        else {
            return commit_mutation(
                transaction,
                missing(label),
                "pricing release recovery link v2 missing release",
            );
        };
        if release.get::<_, String>(1) != digest {
            return commit_mutation(
                transaction,
                version_conflict(),
                "pricing release recovery link v2 digest conflict",
            );
        }
        if release.get::<_, String>(0) != expected_kind {
            return commit_mutation(
                transaction,
                invalid(anyhow::anyhow!(
                    "pricing recovery link {label} has the wrong release kind"
                )),
                "pricing release recovery link v2 kind mismatch",
            );
        }
    }
    if transaction
        .query_opt(
            "SELECT target_generation,recovery_generation
               FROM pricing_release_recovery_links
              WHERE link_digest=$1",
            &[&link.link_digest],
        )?
        .is_some()
    {
        return commit_mutation(
            transaction,
            version_conflict(),
            "pricing release recovery link v2 digest conflict",
        );
    }
    transaction.execute(
        "INSERT INTO pricing_release_recovery_links(
             target_generation,target_digest,recovery_generation,recovery_digest,
             link_digest,created_ts
         ) VALUES($1,$2,$3,$4,$5,$6)",
        &[
            &link.target_generation,
            &link.target_digest,
            &link.recovery_generation,
            &link.recovery_digest,
            &link.link_digest,
            &now(),
        ],
    )?;
    commit_mutation(
        transaction,
        PricingMutation::Stored,
        "pricing release recovery link v2 prepare",
    )
}

pub(crate) fn postgres_pricing_release_recovery_link_v2(
    client: &mut Client,
    target_generation: i64,
    recovery_generation: i64,
) -> Result<Option<super::PricingReleaseRecoveryLinkV2>> {
    if target_generation <= 0 || recovery_generation <= target_generation {
        bail!("invalid pricing release recovery link generations");
    }
    Ok(client
        .query_opt(
            "SELECT target_generation,target_digest,recovery_generation,recovery_digest,link_digest
           FROM pricing_release_recovery_links
          WHERE target_generation=$1 AND recovery_generation=$2",
            &[&target_generation, &recovery_generation],
        )?
        .map(|row| super::PricingReleaseRecoveryLinkV2 {
            target_generation: row.get(0),
            target_digest: row.get(1),
            recovery_generation: row.get(2),
            recovery_digest: row.get(3),
            link_digest: row.get(4),
        }))
}

fn pricing_release_assignment_extension_v2_in_transaction<C: GenericClient>(
    client: &mut C,
    provisioning_head_version: i64,
    account_id: &str,
) -> Result<Option<super::PricingReleaseAssignmentExtensionV2>> {
    let rows = client.query(
        "SELECT release_generation,account_class,policy_id,policy_version,policy_digest,
                billing_mode,funding_generation,purpose,responsible,assignment_digest,
                provisioning_head_generation,provisioning_head_digest,provisioning_head_version,
                paired_recovery_generation,paired_recovery_digest,extension_group_digest,
                extension_digest
           FROM pricing_release_assignment_extensions_v2
          WHERE provisioning_head_version=$1 AND account_id=$2
          ORDER BY release_generation",
        &[&provisioning_head_version, &account_id],
    )?;
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    let provisioning_head_generation = first.get(10);
    let provisioning_head_digest = first.get(11);
    let stored_head_version = first.get(12);
    let paired_recovery_generation = first.get(13);
    let paired_recovery_digest = first.get(14);
    let extension_group_digest = first.get(15);
    let members = rows
        .into_iter()
        .map(|row| {
            Ok(super::PricingReleaseAssignmentExtensionMemberV2 {
                release_generation: row.get(0),
                assignment: super::PricingReleaseAssignmentV2 {
                    account_id: account_id.to_owned(),
                    account_class: super::AccountClass::from_db(&row.get::<_, String>(1))?,
                    policy_id: row.get(2),
                    policy_version: row.get(3),
                    policy_digest: row.get(4),
                    billing_mode: super::BillingModeV2::from_db(&row.get::<_, String>(5))?,
                    funding_generation: row.get(6),
                    purpose: row.get(7),
                    responsible: row.get(8),
                    assignment_digest: row.get(9),
                },
                extension_digest: row.get(16),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let extension = super::PricingReleaseAssignmentExtensionV2 {
        provisioning_head_generation,
        provisioning_head_digest,
        provisioning_head_version: stored_head_version,
        paired_recovery_generation,
        paired_recovery_digest,
        extension_group_digest,
        members,
    };
    super::validate_pricing_release_assignment_extension_v2(&extension)
        .context("validate stored pricing assignment extension")?;
    Ok(Some(extension))
}

pub(crate) fn postgres_pricing_release_assignment_extension_v2(
    client: &mut Client,
    provisioning_head_version: i64,
    account_id: &str,
) -> Result<Option<super::PricingReleaseAssignmentExtensionV2>> {
    if provisioning_head_version <= 0 {
        bail!("pricing assignment extension head version must be positive");
    }
    require_id("pricing assignment extension account id", account_id)?;
    pricing_release_assignment_extension_v2_in_transaction(
        client,
        provisioning_head_version,
        account_id,
    )
}

pub(crate) fn postgres_prepare_pricing_release_assignment_extension_v2(
    client: &mut Client,
    extension: &super::PricingReleaseAssignmentExtensionV2,
) -> Result<PricingMutation> {
    if let Err(error) = super::validate_pricing_release_assignment_extension_v2(extension) {
        return Ok(invalid(error));
    }
    let extension = super::release_v2::normalize_pricing_release_assignment_extension_v2(extension);
    let account_id = &extension.members[0].assignment.account_id;
    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL pricing assignment extension prepare")?;
    advisory_lock(&mut transaction, PRICING_RELEASE_CONTROL_LOCK_V2)?;

    if let Some(existing) = pricing_release_assignment_extension_v2_in_transaction(
        &mut transaction,
        extension.provisioning_head_version,
        account_id,
    )? {
        let outcome = if existing == extension {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "pricing assignment extension replay");
    }

    let current_head = transaction.query_opt(
        "SELECT active_generation,active_digest,head_version
           FROM pricing_release_head_v2 WHERE singleton=1",
        &[],
    )?;
    let exact_head = current_head.as_ref().is_some_and(|row| {
        row.get::<_, i64>(0) == extension.provisioning_head_generation
            && row.get::<_, String>(1) == extension.provisioning_head_digest
            && row.get::<_, i64>(2) == extension.provisioning_head_version
    });
    if !exact_head {
        let actual =
            current_head.map(|row| VersionTarget::new(row.get(0), row.get::<_, String>(1)));
        return commit_mutation(
            transaction,
            stale(actual),
            "pricing assignment extension stale head",
        );
    }

    let provisioning_context =
        pricing_release_provisioning_context_v2_in_transaction(&mut transaction)?
            .context("active pricing release lacks a coherent provisioning context")?;
    let expected_recovery = provisioning_context
        .paired_recovery
        .as_ref()
        .map(|paired| {
            (
                paired.recovery_link.recovery_generation,
                paired.recovery_link.recovery_digest.as_str(),
            )
        });
    let supplied_recovery = extension
        .paired_recovery_generation
        .zip(extension.paired_recovery_digest.as_deref());
    if supplied_recovery != expected_recovery {
        return commit_mutation(
            transaction,
            missing("recovery_link"),
            "pricing assignment extension activation pair mismatch",
        );
    }

    let assignment = &extension.members[0].assignment;
    if assignment.billing_mode == super::BillingModeV2::Balance {
        crate::funding_v2::lock_funding_account_v2(&mut transaction, account_id)?;
        let funding_head = crate::funding_v2::active_funding_head_v2(&mut transaction, account_id)?;
        if funding_head.as_ref().map(|head| head.generation) != assignment.funding_generation {
            return commit_mutation(
                transaction,
                missing("funding_head"),
                "pricing assignment extension stale funding head",
            );
        }
    }

    if let Some(recovery_generation) = extension.paired_recovery_generation {
        let link_exists: bool = transaction
            .query_one(
                "SELECT EXISTS(
                 SELECT 1 FROM pricing_release_recovery_links
                  WHERE target_generation=$1 AND target_digest=$2
                    AND recovery_generation=$3 AND recovery_digest=$4
             )",
                &[
                    &extension.provisioning_head_generation,
                    &extension.provisioning_head_digest,
                    &recovery_generation,
                    &extension.paired_recovery_digest,
                ],
            )?
            .get(0);
        if !link_exists {
            return commit_mutation(
                transaction,
                missing("recovery_link"),
                "pricing assignment extension missing recovery link",
            );
        }
    }

    for member in &extension.members {
        let assignment = &member.assignment;
        let ready: bool = transaction
            .query_one(
                "SELECT EXISTS(
                 SELECT 1
                   FROM accounts account
                   JOIN pricing_release_versions release ON release.generation=$2
                   JOIN pricing_release_policy_versions policy
                     ON policy.policy_id=$3 AND policy.policy_version=$4
                    AND policy.content_digest=$5
                  WHERE account.id=$1
                    AND policy.account_class=$6 AND policy.billing_mode=$7
                    AND NOT EXISTS(
                        SELECT 1 FROM pricing_release_assignments base
                         WHERE base.release_generation=$2 AND base.account_id=$1
                    )
                    AND (
                        ($7='meter_only' AND $8::bigint IS NULL)
                        OR ($7='balance' AND EXISTS(
                            SELECT 1 FROM account_funding_generations_v2 generation
                             WHERE generation.account_id=$1 AND generation.generation=$8
                        ))
                    )
             )",
                &[
                    &assignment.account_id,
                    &member.release_generation,
                    &assignment.policy_id,
                    &assignment.policy_version,
                    &assignment.policy_digest,
                    &assignment.account_class.as_str(),
                    &assignment.billing_mode.as_str(),
                    &assignment.funding_generation,
                ],
            )?
            .get(0);
        if !ready {
            return commit_mutation(
                transaction,
                missing(&format!("assignment:{}", member.release_generation)),
                "pricing assignment extension missing dependency",
            );
        }
    }

    let created_ts = now();
    for member in &extension.members {
        let assignment = &member.assignment;
        transaction.execute(
            "INSERT INTO pricing_release_assignment_extensions_v2(
                 release_generation,account_id,account_class,policy_id,policy_version,
                 policy_digest,billing_mode,funding_generation,purpose,responsible,
                 assignment_digest,provisioning_head_generation,provisioning_head_digest,
                 provisioning_head_version,paired_recovery_generation,paired_recovery_digest,
                 extension_group_digest,extension_digest,created_ts
             ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)",
            &[
                &member.release_generation,
                &assignment.account_id,
                &assignment.account_class.as_str(),
                &assignment.policy_id,
                &assignment.policy_version,
                &assignment.policy_digest,
                &assignment.billing_mode.as_str(),
                &assignment.funding_generation,
                &assignment.purpose,
                &assignment.responsible,
                &assignment.assignment_digest,
                &extension.provisioning_head_generation,
                &extension.provisioning_head_digest,
                &extension.provisioning_head_version,
                &extension.paired_recovery_generation,
                &extension.paired_recovery_digest,
                &extension.extension_group_digest,
                &member.extension_digest,
                &created_ts,
            ],
        )?;
    }
    commit_mutation(
        transaction,
        PricingMutation::Stored,
        "pricing assignment extension prepare",
    )
}

pub(crate) fn postgres_pricing_release_head_v2(
    client: &mut Client,
) -> Result<Option<super::PricingReleaseHeadV2>> {
    Ok(client
        .query_opt(
            "SELECT active_generation,active_digest,head_version,updated_ts
           FROM pricing_release_head_v2 WHERE singleton=1",
            &[],
        )?
        .map(|row| super::PricingReleaseHeadV2 {
            active_generation: row.get(0),
            active_digest: row.get(1),
            head_version: row.get(2),
            updated_ts: row.get(3),
        }))
}

fn pricing_release_provisioning_release_v2_in_transaction<C: GenericClient>(
    client: &mut C,
    generation: i64,
) -> Result<Option<super::PricingReleaseProvisioningReleaseV2>> {
    let Some(row) = client.query_opt(
        "SELECT generation,release_kind,schema_version,capability_generation, \
                capability_digest,main_catalog_generation,main_catalog_digest, \
                openkeys_catalog_generation,openkeys_catalog_digest,switch_generation, \
                switch_digest,inventory_digest,funding_manifest_digest, \
                minimum_runtime_schema_version,content_digest \
           FROM pricing_release_versions WHERE generation=$1",
        &[&generation],
    )? else {
        return Ok(None);
    };
    Ok(Some(super::PricingReleaseProvisioningReleaseV2 {
        generation: row.get(0),
        release_kind: super::PricingReleaseKindV2::from_db(&row.get::<_, String>(1))?,
        schema_version: row.get(2),
        capability_generation: row.get(3),
        capability_digest: row.get(4),
        main_catalog_generation: row.get(5),
        main_catalog_digest: row.get(6),
        openkeys_catalog_generation: row.get(7),
        openkeys_catalog_digest: row.get(8),
        switch_generation: row.get(9),
        switch_digest: row.get(10),
        inventory_digest: row.get(11),
        funding_manifest_digest: row.get(12),
        minimum_runtime_schema_version: row.get(13),
        content_digest: row.get(14),
    }))
}

fn same_pricing_release_provisioning_lineage_v2(
    target: &super::PricingReleaseProvisioningReleaseV2,
    recovery: &super::PricingReleaseProvisioningReleaseV2,
) -> bool {
    target.schema_version == recovery.schema_version
        && target.capability_generation == recovery.capability_generation
        && target.capability_digest == recovery.capability_digest
        && target.main_catalog_generation == recovery.main_catalog_generation
        && target.main_catalog_digest == recovery.main_catalog_digest
        && target.openkeys_catalog_generation == recovery.openkeys_catalog_generation
        && target.openkeys_catalog_digest == recovery.openkeys_catalog_digest
        && target.switch_generation == recovery.switch_generation
        && target.switch_digest == recovery.switch_digest
        && target.inventory_digest == recovery.inventory_digest
        && target.funding_manifest_digest == recovery.funding_manifest_digest
        && target.minimum_runtime_schema_version == recovery.minimum_runtime_schema_version
}

fn pricing_release_provisioning_context_v2_in_transaction<C: GenericClient>(
    client: &mut C,
) -> Result<Option<super::PricingReleaseProvisioningContextV2>> {
    let Some(head_row) = client.query_opt(
        "SELECT active_generation,active_digest,head_version,updated_ts \
           FROM pricing_release_head_v2 WHERE singleton=1",
        &[],
    )? else {
        return Ok(None);
    };
    let head = super::PricingReleaseHeadV2 {
        active_generation: head_row.get(0),
        active_digest: head_row.get(1),
        head_version: head_row.get(2),
        updated_ts: head_row.get(3),
    };
    let audit = client
        .query_opt(
            "SELECT activation.id,activation.activation_kind,activation.from_generation, \
                    activation.from_digest,activation.to_generation,activation.to_digest, \
                    activation.expected_head_version,activation.evidence_digest, \
                    activation.activated_ts,evidence.target_generation,evidence.target_digest, \
                    evidence.recovery_generation,evidence.recovery_digest,evidence.passed, \
                    evidence.blocker_count \
               FROM pricing_release_activations_v2 activation \
               JOIN pricing_stage8_evidence_v2 evidence \
                 ON evidence.evidence_digest=activation.evidence_digest \
              WHERE activation.resulting_head_version=$1",
            &[&head.head_version],
        )?
        .context("active pricing release head lacks activation evidence")?;
    let activation_kind =
        super::PricingReleaseActivationKindV2::from_db(&audit.get::<_, String>(1))?;
    let from_generation: Option<i64> = audit.get(2);
    let from_digest: Option<String> = audit.get(3);
    let to_generation: i64 = audit.get(4);
    let to_digest: String = audit.get(5);
    let expected_head_version: i64 = audit.get(6);
    let evidence_digest: String = audit.get(7);
    let activated_ts: i64 = audit.get(8);
    let target_generation: i64 = audit.get(9);
    let target_digest: String = audit.get(10);
    let recovery_generation: i64 = audit.get(11);
    let recovery_digest: String = audit.get(12);
    let evidence_passed: bool = audit.get(13);
    let blocker_count: i64 = audit.get(14);

    let head_matches_audit = to_generation == head.active_generation
        && to_digest == head.active_digest
        && expected_head_version.checked_add(1) == Some(head.head_version)
        && activated_ts == head.updated_ts;
    let activation_matches_evidence = match activation_kind {
        super::PricingReleaseActivationKindV2::Cutover => {
            from_generation.is_none()
                && from_digest.is_none()
                && expected_head_version == 0
                && to_generation == target_generation
                && to_digest == target_digest
        }
        super::PricingReleaseActivationKindV2::Recovery => {
            from_generation == Some(target_generation)
                && from_digest.as_deref() == Some(target_digest.as_str())
                && expected_head_version > 0
                && to_generation == recovery_generation
                && to_digest == recovery_digest
        }
    };
    if !head_matches_audit
        || !activation_matches_evidence
        || !evidence_passed
        || blocker_count != 0
        || target_generation <= 0
        || recovery_generation <= target_generation
    {
        bail!("active pricing release head, activation audit, or Stage 8 evidence disagree");
    }

    let target = pricing_release_provisioning_release_v2_in_transaction(
        client,
        target_generation,
    )?
    .context("pricing provisioning target release is missing")?;
    let recovery = pricing_release_provisioning_release_v2_in_transaction(
        client,
        recovery_generation,
    )?
    .context("pricing provisioning recovery release is missing")?;
    if target.release_kind != super::PricingReleaseKindV2::Target
        || target.content_digest != target_digest
        || recovery.release_kind != super::PricingReleaseKindV2::Recovery
        || recovery.content_digest != recovery_digest
        || !same_pricing_release_provisioning_lineage_v2(&target, &recovery)
    {
        bail!("pricing provisioning releases disagree with activation evidence");
    }
    let same_base_funding_assignments: bool = client
        .query_one(
            "SELECT NOT EXISTS( \
                 (SELECT account_id,billing_mode,funding_generation \
                    FROM pricing_release_assignments WHERE release_generation=$1 \
                  EXCEPT \
                  SELECT account_id,billing_mode,funding_generation \
                    FROM pricing_release_assignments WHERE release_generation=$2) \
                 UNION ALL \
                 (SELECT account_id,billing_mode,funding_generation \
                    FROM pricing_release_assignments WHERE release_generation=$2 \
                  EXCEPT \
                  SELECT account_id,billing_mode,funding_generation \
                    FROM pricing_release_assignments WHERE release_generation=$1) \
             )",
            &[&target_generation, &recovery_generation],
        )?
        .get(0);
    if !same_base_funding_assignments {
        bail!("pricing provisioning target/recovery funding assignments disagree");
    }
    let recovery_link = client
        .query_opt(
            "SELECT target_generation,target_digest,recovery_generation,recovery_digest, \
                    link_digest \
               FROM pricing_release_recovery_links \
              WHERE target_generation=$1 AND target_digest=$2 \
                AND recovery_generation=$3 AND recovery_digest=$4",
            &[
                &target_generation,
                &target_digest,
                &recovery_generation,
                &recovery_digest,
            ],
        )?
        .map(|row| super::PricingReleaseRecoveryLinkV2 {
            target_generation: row.get(0),
            target_digest: row.get(1),
            recovery_generation: row.get(2),
            recovery_digest: row.get(3),
            link_digest: row.get(4),
        })
        .context("pricing provisioning activation pair lacks its recovery link")?;

    let (active_release, paired_recovery) = match activation_kind {
        super::PricingReleaseActivationKindV2::Cutover => (
            target,
            Some(super::PricingReleaseProvisioningRecoveryV2 {
                release: recovery,
                recovery_link,
            }),
        ),
        super::PricingReleaseActivationKindV2::Recovery => (recovery, None),
    };
    Ok(Some(super::PricingReleaseProvisioningContextV2 {
        head,
        activation: super::PricingReleaseProvisioningActivationV2 {
            activation_id: audit.get(0),
            activation_kind,
            evidence_digest,
            activated_ts,
        },
        active_release,
        paired_recovery,
    }))
}

pub(crate) fn postgres_pricing_release_provisioning_context_v2(
    client: &mut Client,
) -> Result<Option<super::PricingReleaseProvisioningContextV2>> {
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .context("begin PostgreSQL pricing provisioning context v2")?;
    transaction.batch_execute("SET LOCAL statement_timeout='30s'; SET LOCAL lock_timeout='5s'")?;
    let context = pricing_release_provisioning_context_v2_in_transaction(&mut transaction)?;
    transaction.commit()?;
    Ok(context)
}

fn reject_pricing_release_activation_v2(
    transaction: Transaction<'_>,
    rejection: super::PricingReleaseActivationRejectionV2,
) -> Result<super::PricingReleaseActivationOutcomeV2> {
    transaction
        .rollback()
        .context("rollback rejected PostgreSQL pricing release activation")?;
    Ok(super::PricingReleaseActivationOutcomeV2::Rejected(
        rejection,
    ))
}

fn locked_pricing_release_head_v2(
    transaction: &mut Transaction<'_>,
) -> Result<Option<super::PricingReleaseHeadV2>> {
    Ok(transaction
        .query_opt(
            "SELECT active_generation,active_digest,head_version,updated_ts \
             FROM pricing_release_head_v2 WHERE singleton=1 FOR UPDATE",
            &[],
        )?
        .map(|row| super::PricingReleaseHeadV2 {
            active_generation: row.get(0),
            active_digest: row.get(1),
            head_version: row.get(2),
            updated_ts: row.get(3),
        }))
}

fn pricing_release_activation_receipt_v2(
    transaction: &mut Transaction<'_>,
    head: &super::PricingReleaseHeadV2,
) -> Result<Option<super::PricingReleaseActivationReceiptV2>> {
    let Some(row) = transaction.query_opt(
        "SELECT id,activation_kind,from_generation,from_digest,expected_head_version, \
                to_generation,to_digest,evidence_digest,operator_id,reason,activated_ts \
         FROM pricing_release_activations_v2 WHERE resulting_head_version=$1",
        &[&head.head_version],
    )?
    else {
        return Ok(None);
    };
    if row.get::<_, i64>(5) != head.active_generation
        || row.get::<_, String>(6) != head.active_digest
    {
        bail!("pricing release activation audit disagrees with the active head");
    }
    Ok(Some(super::PricingReleaseActivationReceiptV2 {
        activation_id: row.get(0),
        activation_kind: super::PricingReleaseActivationKindV2::from_db(&row.get::<_, String>(1))?,
        from_generation: row.get(2),
        from_digest: row.get(3),
        expected_head_version: row.get(4),
        head: head.clone(),
        evidence_digest: row.get(7),
        operator_id: row.get(8),
        reason: row.get(9),
        activated_ts: row.get(10),
    }))
}

fn activation_request_matches_receipt_v2(
    request: &super::PricingReleaseActivationRequestV2,
    receipt: &super::PricingReleaseActivationReceiptV2,
) -> bool {
    let expected = match &request.expectation {
        super::PricingReleaseHeadExpectationV2::Absent => (None, None, 0),
        super::PricingReleaseHeadExpectationV2::Exact(head) => (
            Some(head.active_generation),
            Some(head.active_digest.as_str()),
            head.head_version,
        ),
    };
    receipt.activation_kind == request.activation_kind
        && receipt.from_generation == expected.0
        && receipt.from_digest.as_deref() == expected.1
        && receipt.expected_head_version == expected.2
        && receipt.evidence_digest == request.evidence.evidence_digest
        && receipt.operator_id == request.operator_id
        && receipt.reason == request.reason
}

fn release_head_expectation_matches_v2(
    expectation: &super::PricingReleaseHeadExpectationV2,
    actual: &Option<super::PricingReleaseHeadV2>,
) -> bool {
    match (expectation, actual) {
        (super::PricingReleaseHeadExpectationV2::Absent, None) => true,
        (super::PricingReleaseHeadExpectationV2::Exact(expected), Some(actual)) => {
            expected == actual
        }
        _ => false,
    }
}

fn pricing_release_activation_evidence_matches_v2(
    row: &postgres::Row,
    evidence: &super::PricingReleaseActivationEvidenceV2,
) -> bool {
    row.get::<_, i64>(0) == evidence.target_generation
        && row.get::<_, String>(1) == evidence.target_digest
        && row.get::<_, i64>(2) == evidence.recovery_generation
        && row.get::<_, String>(3) == evidence.recovery_digest
        && row.get::<_, String>(4) == evidence.engine_inventory_digest
        && row.get::<_, String>(5) == evidence.funding_digest
        && row.get::<_, String>(6) == evidence.shadow_digest
        && row.get::<_, String>(7) == evidence.runtime_floor_digest
        && row.get::<_, i64>(8) == evidence.legacy_inflight_count
        && row.get::<_, i64>(9) == 0
        && row.get::<_, bool>(10)
        && row.get::<_, i64>(11) == evidence.observed_ts
        && row.get::<_, i64>(12) == evidence.valid_until_ts
}

/// Atomically activate the first target release or advance that exact target to its recovery.
///
/// This is deliberately one short SERIALIZABLE transaction. It takes only the release control
/// lock, rechecks engine authority, appends evidence/audit and mutates the singleton head. It never
/// updates accounts, funding lots, reservations, ledger rows or any other per-account state.
pub(crate) fn postgres_activate_pricing_release_v2(
    client: &mut Client,
    request: &super::PricingReleaseActivationRequestV2,
    runtime_manifest: &super::PricingRuntimeManifestEvidence,
) -> Result<super::PricingReleaseActivationOutcomeV2> {
    if let Err(error) = super::validate_pricing_release_activation_v2(request) {
        return Ok(super::PricingReleaseActivationOutcomeV2::Rejected(
            super::PricingReleaseActivationRejectionV2::Invalid {
                reason: error.to_string(),
            },
        ));
    }
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .context("begin PostgreSQL pricing release activation")?;
    transaction.batch_execute("SET LOCAL statement_timeout='30s'; SET LOCAL lock_timeout='5s'")?;
    advisory_lock(&mut transaction, PRICING_RELEASE_CONTROL_LOCK_V2)?;
    let now_ts: i64 = transaction
        .query_one(
            "SELECT floor(extract(epoch FROM transaction_timestamp()))::bigint",
            &[],
        )?
        .get(0);
    let current_head = locked_pricing_release_head_v2(&mut transaction)?;
    let evidence = &request.evidence;
    let (to_generation, to_digest) = match request.activation_kind {
        super::PricingReleaseActivationKindV2::Cutover => {
            (evidence.target_generation, evidence.target_digest.as_str())
        }
        super::PricingReleaseActivationKindV2::Recovery => (
            evidence.recovery_generation,
            evidence.recovery_digest.as_str(),
        ),
    };

    if current_head.as_ref().is_some_and(|head| {
        head.active_generation == to_generation && head.active_digest == to_digest
    }) {
        let receipt = pricing_release_activation_receipt_v2(
            &mut transaction,
            current_head
                .as_ref()
                .expect("checked current pricing release head"),
        )?;
        if let Some(receipt) =
            receipt.filter(|receipt| activation_request_matches_receipt_v2(request, receipt))
        {
            transaction
                .rollback()
                .context("rollback unchanged PostgreSQL pricing release activation")?;
            return Ok(super::PricingReleaseActivationOutcomeV2::Unchanged(receipt));
        }
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::CasMismatch {
                actual: current_head,
            },
        );
    }
    if !release_head_expectation_matches_v2(&request.expectation, &current_head) {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::CasMismatch {
                actual: current_head,
            },
        );
    }
    if now_ts < evidence.observed_ts || now_ts > evidence.valid_until_ts {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::EvidenceStale {
                now_ts,
                observed_ts: evidence.observed_ts,
                valid_until_ts: evidence.valid_until_ts,
            },
        );
    }

    let Some(target) =
        pricing_release_v2_in_transaction(&mut transaction, evidence.target_generation)?
    else {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::MissingDependency {
                dependency: format!("target release {}", evidence.target_generation),
            },
        );
    };
    let Some(recovery) =
        pricing_release_v2_in_transaction(&mut transaction, evidence.recovery_generation)?
    else {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::MissingDependency {
                dependency: format!("recovery release {}", evidence.recovery_generation),
            },
        );
    };
    if target.content_digest != evidence.target_digest
        || target.release_kind != super::PricingReleaseKindV2::Target
        || recovery.content_digest != evidence.recovery_digest
        || recovery.release_kind != super::PricingReleaseKindV2::Recovery
    {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::ReleaseLineageDrift {
                reason: "target or recovery immutable identity differs from activation evidence"
                    .to_owned(),
            },
        );
    }
    let recovery_link_matches: bool = transaction
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM pricing_release_recovery_links \
             WHERE target_generation=$1 AND target_digest=$2 \
               AND recovery_generation=$3 AND recovery_digest=$4)",
            &[
                &evidence.target_generation,
                &evidence.target_digest,
                &evidence.recovery_generation,
                &evidence.recovery_digest,
            ],
        )?
        .get(0);
    let same_runtime_lineage = target.schema_version == recovery.schema_version
        && target.capability_generation == recovery.capability_generation
        && target.capability_digest == recovery.capability_digest
        && target.main_catalog_generation == recovery.main_catalog_generation
        && target.main_catalog_digest == recovery.main_catalog_digest
        && target.openkeys_catalog_generation == recovery.openkeys_catalog_generation
        && target.openkeys_catalog_digest == recovery.openkeys_catalog_digest
        && target.switch_generation == recovery.switch_generation
        && target.switch_digest == recovery.switch_digest
        && target.inventory_digest == recovery.inventory_digest
        && target.funding_manifest_digest == recovery.funding_manifest_digest
        && target.minimum_runtime_schema_version == recovery.minimum_runtime_schema_version;
    let same_funding_assignments = target
        .assignments
        .iter()
        .map(|assignment| {
            (
                assignment.account_id.as_str(),
                assignment.billing_mode,
                assignment.funding_generation,
            )
        })
        .eq(recovery.assignments.iter().map(|assignment| {
            (
                assignment.account_id.as_str(),
                assignment.billing_mode,
                assignment.funding_generation,
            )
        }));
    if !recovery_link_matches || !same_runtime_lineage || !same_funding_assignments {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::ReleaseLineageDrift {
                reason: "target and recovery are not one prepared runtime/funding lineage"
                    .to_owned(),
            },
        );
    }
    let active_heads_match: bool = transaction
        .query_one(
            "SELECT \
               EXISTS(SELECT 1 FROM pricing_catalog_heads head \
                 JOIN pricing_catalog_versions version \
                   ON version.product_id=head.product_id \
                  AND version.generation=head.active_generation \
                 WHERE head.product_id='main' AND head.active_generation=$1 \
                   AND version.content_digest=$2) \
               AND EXISTS(SELECT 1 FROM pricing_catalog_heads head \
                 JOIN pricing_catalog_versions version \
                   ON version.product_id=head.product_id \
                  AND version.generation=head.active_generation \
                 WHERE head.product_id='openkeys' AND head.active_generation=$3 \
                   AND version.content_digest=$4) \
               AND EXISTS(SELECT 1 FROM provider_switch_head head \
                 JOIN provider_switch_versions version \
                   ON version.generation=head.active_generation \
                 WHERE head.singleton=1 AND head.active_generation=$5 \
                   AND version.content_digest=$6)",
            &[
                &target.main_catalog_generation,
                &target.main_catalog_digest,
                &target.openkeys_catalog_generation,
                &target.openkeys_catalog_digest,
                &target.switch_generation,
                &target.switch_digest,
            ],
        )?
        .get(0);
    if !active_heads_match {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::ReleaseLineageDrift {
                reason: "active catalog or provider-switch head moved after release preparation"
                    .to_owned(),
            },
        );
    }

    // Commerce has already verified the canonical source report and its 120-second age bound. The
    // exact source capture closes the otherwise ambiguous gap before combined evidence persisted.
    let authority_cutoff = evidence.engine_captured_ts;
    let authority_changed_rows: i64 = transaction
        .query_one(
            "SELECT COALESCE(SUM(changed),0)::bigint FROM( \
               SELECT COUNT(*)::bigint changed FROM pricing_catalog_heads WHERE updated_ts >= $1 \
               UNION ALL SELECT COUNT(*)::bigint FROM provider_switch_head WHERE updated_ts >= $1 \
               UNION ALL SELECT COUNT(*)::bigint FROM account_policy_bindings binding \
                 JOIN pricing_release_assignments assignment \
                   ON assignment.release_generation=$2 AND assignment.account_id=binding.account_id \
                 WHERE binding.updated_ts >= $1 \
               UNION ALL SELECT COUNT(*)::bigint FROM account_policy_versions policy \
                 JOIN pricing_release_assignments assignment \
                   ON assignment.release_generation=$2 AND assignment.account_id=policy.account_id \
                 WHERE policy.created_ts >= $1 \
               UNION ALL SELECT COUNT(*)::bigint FROM pricing_catalog_versions WHERE created_ts >= $1 \
               UNION ALL SELECT COUNT(*)::bigint FROM provider_switch_versions WHERE created_ts >= $1 \
             ) changes",
            &[&authority_cutoff, &evidence.target_generation],
        )?
        .get(0);
    if authority_changed_rows != 0 {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::AuthorityDrift {
                changed_rows: authority_changed_rows,
            },
        );
    }

    let inventory_digest = match request.activation_kind {
        super::PricingReleaseActivationKindV2::Cutover => {
            crate::stage8::engine_inventory_digest(&mut transaction)?
        }
        super::PricingReleaseActivationKindV2::Recovery => {
            crate::stage8::release_base_inventory_digest_v2(
                &mut transaction,
                evidence.target_generation,
            )?
        }
    };
    if target.inventory_digest != evidence.engine_inventory_digest
        || recovery.inventory_digest != evidence.engine_inventory_digest
        || inventory_digest != evidence.engine_inventory_digest
    {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::InventoryDrift {
                expected_digest: evidence.engine_inventory_digest.clone(),
                actual_digest: inventory_digest,
            },
        );
    }
    let coverage_drift: i64 = transaction
        .query_one(
            "WITH covered AS( \
               SELECT account.id, \
                 EXISTS(SELECT 1 FROM pricing_release_assignments assignment \
                   WHERE assignment.release_generation=$1 AND assignment.account_id=account.id) \
                 OR ($4 AND EXISTS(SELECT 1 FROM pricing_release_assignment_extensions_v2 extension \
                   WHERE extension.release_generation=$1 AND extension.account_id=account.id \
                     AND extension.provisioning_head_generation=$1 \
                     AND extension.provisioning_head_digest=$2 \
                     AND extension.provisioning_head_version=$3)) target_covered, \
                 EXISTS(SELECT 1 FROM pricing_release_assignments assignment \
                   WHERE assignment.release_generation=$5 AND assignment.account_id=account.id) \
                 OR ($4 AND EXISTS(SELECT 1 FROM pricing_release_assignment_extensions_v2 extension \
                   WHERE extension.release_generation=$5 AND extension.account_id=account.id \
                     AND extension.provisioning_head_generation=$1 \
                     AND extension.provisioning_head_digest=$2 \
                     AND extension.provisioning_head_version=$3 \
                     AND extension.paired_recovery_generation=$5 \
                     AND extension.paired_recovery_digest=$6)) recovery_covered \
               FROM accounts account \
             ) SELECT COUNT(*)::bigint FROM covered \
               WHERE NOT target_covered OR NOT recovery_covered",
            &[
                &evidence.target_generation,
                &evidence.target_digest,
                &current_head.as_ref().map(|head| head.head_version).unwrap_or(0),
                &matches!(
                    request.activation_kind,
                    super::PricingReleaseActivationKindV2::Recovery
                ),
                &evidence.recovery_generation,
                &evidence.recovery_digest,
            ],
        )?
        .get(0);
    if coverage_drift != 0 {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::InventoryDrift {
                expected_digest: evidence.engine_inventory_digest.clone(),
                actual_digest: format!("uncovered_accounts:{coverage_drift}"),
            },
        );
    }

    let funding_digest = crate::stage8::funding_manifest_digest(&mut transaction, Some(&target))?;
    if target.funding_manifest_digest != evidence.funding_digest
        || recovery.funding_manifest_digest != evidence.funding_digest
        || funding_digest != evidence.funding_digest
    {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::FundingDrift {
                expected_digest: evidence.funding_digest.clone(),
                actual_digest: funding_digest,
            },
        );
    }
    let funding_invariant_drift: i64 = transaction
        .query_one(
            "WITH assignment AS( \
               SELECT account_id,billing_mode,funding_generation \
                 FROM pricing_release_assignments WHERE release_generation IN($1,$2) \
               UNION \
               SELECT account_id,billing_mode,funding_generation \
                 FROM pricing_release_assignment_extensions_v2 \
                 WHERE release_generation IN($1,$2) \
                   AND provisioning_head_generation=$1 \
                   AND provisioning_head_digest=$3 \
                   AND provisioning_head_version=$4 \
             ), balance_assignment AS( \
               SELECT DISTINCT account_id,funding_generation FROM assignment \
               WHERE billing_mode='balance' \
             ) \
             SELECT COUNT(*)::bigint FROM balance_assignment assignment \
             JOIN accounts account ON account.id=assignment.account_id \
             LEFT JOIN account_funding_head_v2 head ON head.account_id=assignment.account_id \
             LEFT JOIN account_funding_generations_v2 generation \
               ON generation.account_id=assignment.account_id \
              AND generation.generation=assignment.funding_generation \
             LEFT JOIN LATERAL( \
               SELECT COUNT(*)::bigint lot_count, \
                      COUNT(*) FILTER(WHERE lot.source_type='paid')::bigint paid_count, \
                      COALESCE(SUM(lot.balance_nano::numeric),0) balance_nano, \
                      COALESCE(SUM(lot.reserved_nano::numeric),0) reserved_nano, \
                      COALESCE(SUM(lot.spent_nano::numeric),0) spent_nano \
               FROM funding_lots_v2 lot WHERE lot.account_id=assignment.account_id \
                 AND lot.funding_generation=assignment.funding_generation \
             ) lots ON true \
             WHERE head.active_generation IS DISTINCT FROM assignment.funding_generation \
                OR generation.account_id IS NULL OR lots.lot_count=0 OR lots.paid_count=0 \
                OR generation.balance_nano::numeric<>account.balance_nano::numeric \
                OR generation.reserved_nano::numeric<>account.reserved_nano::numeric \
                OR generation.spent_nano::numeric<>account.spent_nano::numeric \
                OR lots.balance_nano<>generation.balance_nano::numeric \
                OR lots.reserved_nano<>generation.reserved_nano::numeric \
                OR lots.spent_nano<>generation.spent_nano::numeric",
            &[
                &evidence.target_generation,
                &evidence.recovery_generation,
                &evidence.target_digest,
                &current_head
                    .as_ref()
                    .map(|head| head.head_version)
                    .unwrap_or(0),
            ],
        )?
        .get(0);
    if funding_invariant_drift != 0 {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::FundingInvariantDrift {
                account_count: funding_invariant_drift,
            },
        );
    }

    let runtime_floor = crate::stage8::runtime_floor_check_v2(
        &mut transaction,
        now_ts,
        target
            .minimum_runtime_schema_version
            .max(recovery.minimum_runtime_schema_version),
        runtime_manifest,
    )?;
    if runtime_floor.live_instances == 0
        || runtime_floor.live_instances != runtime_floor.compatible_instances
    {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::RuntimeIncompatible {
                live_instances: runtime_floor.live_instances,
                compatible_instances: runtime_floor.compatible_instances,
            },
        );
    }
    if runtime_floor.digest != evidence.runtime_floor_digest {
        return reject_pricing_release_activation_v2(
            transaction,
            super::PricingReleaseActivationRejectionV2::RuntimeFloorDrift {
                expected_digest: evidence.runtime_floor_digest.clone(),
                actual_digest: runtime_floor.digest,
            },
        );
    }

    if let Some(stored) = transaction.query_opt(
        "SELECT target_generation,target_digest,recovery_generation,recovery_digest, \
                inventory_digest,funding_digest,shadow_digest,runtime_floor_digest, \
                legacy_inflight_count,blocker_count,passed,observed_ts,valid_until_ts \
         FROM pricing_stage8_evidence_v2 WHERE evidence_digest=$1",
        &[&evidence.evidence_digest],
    )? {
        if !pricing_release_activation_evidence_matches_v2(&stored, evidence) {
            return reject_pricing_release_activation_v2(
                transaction,
                super::PricingReleaseActivationRejectionV2::EvidenceConflict {
                    evidence_digest: evidence.evidence_digest.clone(),
                },
            );
        }
    } else {
        transaction.execute(
            "INSERT INTO pricing_stage8_evidence_v2( \
               evidence_digest,target_generation,target_digest,recovery_generation, \
               recovery_digest,inventory_digest,funding_digest,shadow_digest, \
               runtime_floor_digest,legacy_inflight_count,blocker_count,passed, \
               observed_ts,valid_until_ts \
             ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,0,true,$11,$12)",
            &[
                &evidence.evidence_digest,
                &evidence.target_generation,
                &evidence.target_digest,
                &evidence.recovery_generation,
                &evidence.recovery_digest,
                &evidence.engine_inventory_digest,
                &evidence.funding_digest,
                &evidence.shadow_digest,
                &evidence.runtime_floor_digest,
                &evidence.legacy_inflight_count,
                &evidence.observed_ts,
                &evidence.valid_until_ts,
            ],
        )?;
    }

    let (from_generation, from_digest, expected_head_version) = match &request.expectation {
        super::PricingReleaseHeadExpectationV2::Absent => (None, None, 0),
        super::PricingReleaseHeadExpectationV2::Exact(head) => (
            Some(head.active_generation),
            Some(head.active_digest.as_str()),
            head.head_version,
        ),
    };
    let resulting_head_version = expected_head_version + 1;
    let activation_id: i64 = transaction
        .query_one(
            "INSERT INTO pricing_release_activations_v2( \
               activation_kind,from_generation,from_digest,to_generation,to_digest, \
               expected_head_version,resulting_head_version,evidence_digest,operator_id, \
               reason,activated_ts \
             ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING id",
            &[
                &request.activation_kind.as_str(),
                &from_generation,
                &from_digest,
                &to_generation,
                &to_digest,
                &expected_head_version,
                &resulting_head_version,
                &evidence.evidence_digest,
                &request.operator_id,
                &request.reason,
                &now_ts,
            ],
        )?
        .get(0);
    let changed = match &request.expectation {
        super::PricingReleaseHeadExpectationV2::Absent => transaction.execute(
            "INSERT INTO pricing_release_head_v2( \
               singleton,active_generation,active_digest,head_version,updated_ts \
             ) VALUES(1,$1,$2,$3,$4)",
            &[&to_generation, &to_digest, &resulting_head_version, &now_ts],
        )?,
        super::PricingReleaseHeadExpectationV2::Exact(expected) => transaction.execute(
            "UPDATE pricing_release_head_v2 SET active_generation=$1,active_digest=$2, \
                    head_version=$3,updated_ts=$4 \
             WHERE singleton=1 AND active_generation=$5 AND active_digest=$6 \
               AND head_version=$7 AND updated_ts=$8",
            &[
                &to_generation,
                &to_digest,
                &resulting_head_version,
                &now_ts,
                &expected.active_generation,
                &expected.active_digest,
                &expected.head_version,
                &expected.updated_ts,
            ],
        )?,
    };
    if changed != 1 {
        bail!("pricing release head CAS changed an unexpected row count");
    }
    transaction.batch_execute("SET CONSTRAINTS pricing_release_head_audit_v2 IMMEDIATE")?;
    let receipt = super::PricingReleaseActivationReceiptV2 {
        activation_id,
        activation_kind: request.activation_kind,
        from_generation,
        from_digest: from_digest.map(str::to_owned),
        expected_head_version,
        head: super::PricingReleaseHeadV2 {
            active_generation: to_generation,
            active_digest: to_digest.to_owned(),
            head_version: resulting_head_version,
            updated_ts: now_ts,
        },
        evidence_digest: evidence.evidence_digest.clone(),
        operator_id: request.operator_id.clone(),
        reason: request.reason.clone(),
        activated_ts: now_ts,
    };
    transaction
        .commit()
        .context("commit PostgreSQL pricing release activation")?;
    Ok(super::PricingReleaseActivationOutcomeV2::Applied(receipt))
}

pub(crate) fn pricing_release_resolution_v2_in_transaction<C: GenericClient>(
    client: &mut C,
    account_id: &str,
    provider_id: &str,
    canonical_model_id: &str,
) -> Result<Option<super::PricingReleaseResolutionV2>> {
    for (label, value) in [
        ("pricing release account id", account_id),
        ("pricing release provider id", provider_id),
        ("pricing release canonical model id", canonical_model_id),
    ] {
        super::require_id(label, value)?;
    }
    if !matches!(provider_id, "anthropic" | "openai" | "google") {
        bail!("pricing release provider is outside the fixed runtime plane");
    }

    let Some(row) = client.query_opt(
        "SELECT head.active_generation,head.active_digest,head.head_version,head.updated_ts,
                release.schema_version,
                assignment.account_class,assignment.policy_id,assignment.policy_version,
                assignment.policy_digest,assignment.billing_mode,assignment.funding_generation,
                assignment.purpose,assignment.responsible,assignment.assignment_digest,
                release.capability_generation,release.capability_digest,
                release.main_catalog_generation,release.main_catalog_digest,
                release.openkeys_catalog_generation,release.openkeys_catalog_digest,
                release.switch_generation,release.switch_digest
           FROM pricing_release_head_v2 head
           JOIN pricing_release_versions release
             ON release.generation=head.active_generation
            AND release.content_digest=head.active_digest
           JOIN LATERAL (
               SELECT account_id,account_class,policy_id,policy_version,policy_digest,
                      billing_mode,funding_generation,purpose,responsible,assignment_digest
                 FROM pricing_release_assignments
                WHERE release_generation=release.generation AND account_id=$1
               UNION ALL
               SELECT account_id,account_class,policy_id,policy_version,policy_digest,
                      billing_mode,funding_generation,purpose,responsible,assignment_digest
                 FROM pricing_release_assignment_extensions_v2
                WHERE release_generation=release.generation AND account_id=$1
           ) assignment ON TRUE
          WHERE head.singleton=1",
        &[&account_id],
    )?
    else {
        let head_exists: bool = client
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM pricing_release_head_v2 WHERE singleton=1)",
                &[],
            )?
            .get(0);
        if head_exists {
            bail!("active pricing release does not cover the requested account");
        }
        return Ok(None);
    };

    let head = super::PricingReleaseHeadV2 {
        active_generation: row.get(0),
        active_digest: row.get(1),
        head_version: row.get(2),
        updated_ts: row.get(3),
    };
    let release_schema_version: i64 = row.get(4);
    let assignment = super::PricingReleaseAssignmentV2 {
        account_id: account_id.to_owned(),
        account_class: super::AccountClass::from_db(&row.get::<_, String>(5))?,
        policy_id: row.get(6),
        policy_version: row.get(7),
        policy_digest: row.get(8),
        billing_mode: super::BillingModeV2::from_db(&row.get::<_, String>(9))?,
        funding_generation: row.get(10),
        purpose: row.get(11),
        responsible: row.get(12),
        assignment_digest: row.get(13),
    };
    let release_capability_generation: i64 = row.get(14);
    let release_capability_digest: String = row.get(15);
    let main_catalog_generation: i64 = row.get(16);
    let main_catalog_digest: String = row.get(17);
    let openkeys_catalog_generation: i64 = row.get(18);
    let openkeys_catalog_digest: String = row.get(19);
    let release_switch_generation: i64 = row.get(20);
    let release_switch_digest: String = row.get(21);

    let policy =
        release_policy_v2_in_transaction(client, &assignment.policy_id, assignment.policy_version)?
            .context("active pricing release assignment policy is missing")?;
    if policy.content_digest != assignment.policy_digest
        || policy.account_class != assignment.account_class
        || policy.billing_mode != assignment.billing_mode
        || policy.capability_generation != release_capability_generation
        || policy.capability_digest != release_capability_digest
        || policy
            .switch_generation
            .is_some_and(|generation| generation != release_switch_generation)
        || policy
            .switch_digest
            .as_deref()
            .is_some_and(|digest| digest != release_switch_digest)
    {
        bail!("active pricing release assignment/policy identity is inconsistent");
    }

    let master_enabled = client
        .query_opt(
            "SELECT entry.enabled
               FROM provider_switch_versions switches
               JOIN provider_switch_entries entry ON entry.generation=switches.generation
              WHERE switches.generation=$1 AND switches.content_digest=$2
                AND switches.capability_generation=$3 AND switches.capability_digest=$4
                AND entry.provider_id=$5 AND entry.scope_type='master'
                AND entry.product_id='' AND entry.segment=''",
            &[
                &release_switch_generation,
                &release_switch_digest,
                &release_capability_generation,
                &release_capability_digest,
                &provider_id,
            ],
        )?
        .context("active pricing release lacks the provider master switch")?
        .get::<_, bool>(0);
    if !master_enabled {
        bail!("active pricing release provider master switch is disabled");
    }

    let rule = if assignment.billing_mode == super::BillingModeV2::MeterOnly {
        if assignment.account_class != super::AccountClass::Service
            || !policy.rules.is_empty()
            || policy.product_id.is_some()
        {
            bail!("active meter-only release policy is not a canonical service policy");
        }
        None
    } else {
        let product_id = policy
            .product_id
            .as_deref()
            .context("active balance release policy lacks a product id")?;
        let catalog_generation = policy
            .catalog_generation
            .context("active balance release policy lacks a catalog generation")?;
        let catalog_digest = policy
            .catalog_digest
            .as_deref()
            .context("active balance release policy lacks a catalog digest")?;
        let (release_catalog_generation, release_catalog_digest) = match product_id {
            "main" => (main_catalog_generation, main_catalog_digest.as_str()),
            "openkeys" => (
                openkeys_catalog_generation,
                openkeys_catalog_digest.as_str(),
            ),
            _ => bail!("active pricing release policy references an unsupported product"),
        };
        if catalog_generation != release_catalog_generation
            || catalog_digest != release_catalog_digest
        {
            bail!("active pricing release policy catalog differs from the release pin");
        }
        let model_enabled: bool = client
            .query_opt(
                "SELECT entry.enabled
                   FROM pricing_catalog_versions catalog
                   JOIN pricing_catalog_entries entry
                     ON entry.product_id=catalog.product_id
                    AND entry.generation=catalog.generation
                  WHERE catalog.product_id=$1 AND catalog.generation=$2
                    AND catalog.content_digest=$3
                    AND catalog.capability_generation=$4 AND catalog.capability_digest=$5
                    AND entry.provider_id=$6 AND entry.canonical_model_id=$7",
                &[
                    &product_id,
                    &catalog_generation,
                    &catalog_digest,
                    &release_capability_generation,
                    &release_capability_digest,
                    &provider_id,
                    &canonical_model_id,
                ],
            )?
            .context("active pricing release model is absent from the pinned catalog")?
            .get(0);
        if !model_enabled {
            bail!("active pricing release model is disabled in the pinned catalog");
        }

        let (scope_type, segment) = match assignment.account_class {
            super::AccountClass::B2c => ("segment", "b2c"),
            super::AccountClass::B2b => ("segment", "b2b"),
            super::AccountClass::OpenKeys => ("product", ""),
            super::AccountClass::Service => {
                bail!("service assignment cannot use balance billing")
            }
        };
        let scoped_switch = client
            .query_opt(
                "SELECT enabled,catalog_generation
                   FROM provider_switch_entries
                  WHERE generation=$1 AND provider_id=$2 AND scope_type=$3
                    AND product_id=$4 AND segment=$5",
                &[
                    &release_switch_generation,
                    &provider_id,
                    &scope_type,
                    &product_id,
                    &segment,
                ],
            )?
            .context("active pricing release lacks the required scoped provider switch")?;
        if !scoped_switch.get::<_, bool>(0)
            || scoped_switch.get::<_, Option<i64>>(1) != Some(catalog_generation)
        {
            bail!("active pricing release scoped provider switch is disabled or stale");
        }

        let selected = policy
            .rules
            .iter()
            .find(|rule| {
                matches!(
                    &rule.scope,
                    super::PricingReleaseRuleScopeV2::Model {
                        provider_id: rule_provider,
                        canonical_model_id: rule_model,
                    } if rule_provider == provider_id && rule_model == canonical_model_id
                )
            })
            .or_else(|| {
                policy.rules.iter().find(|rule| {
                    matches!(
                        &rule.scope,
                        super::PricingReleaseRuleScopeV2::Provider {
                            provider_id: rule_provider,
                        } if rule_provider == provider_id
                    )
                })
            })
            .or_else(|| {
                policy
                    .rules
                    .iter()
                    .find(|rule| matches!(&rule.scope, super::PricingReleaseRuleScopeV2::Global))
            })
            .cloned()
            .context("active pricing release has no applicable model/provider/global rule")?;
        Some(selected)
    };

    Ok(Some(super::PricingReleaseResolutionV2 {
        release_digest: head.active_digest.clone(),
        head,
        release_schema_version,
        assignment,
        policy,
        rule,
    }))
}

pub(crate) fn postgres_pricing_release_resolution_v2(
    client: &mut Client,
    account_id: &str,
    provider_id: &str,
    canonical_model_id: &str,
) -> Result<Option<super::PricingReleaseResolutionV2>> {
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .context("begin PostgreSQL pricing release resolution v2")?;
    let resolved = pricing_release_resolution_v2_in_transaction(
        &mut transaction,
        account_id,
        provider_id,
        canonical_model_id,
    )?;
    transaction.commit()?;
    Ok(resolved)
}

pub(crate) fn pricing_request_snapshot_v2_in_transaction<C: GenericClient>(
    client: &mut C,
    request_id: &str,
) -> Result<Option<super::PricingRequestSnapshotV2>> {
    super::require_id("pricing release request id", request_id)?;
    let Some(row) = client.query_opt(
        "SELECT account_id,release_schema_version,release_generation,release_digest,
                assignment_digest,account_class,policy_id,policy_version,policy_digest,
                billing_mode,funding_generation,provider_id,canonical_model_id,rule_id,
                rule_digest,rule_scope,discount_bps,payable_multiplier_bp,tariff_schedule_id,
                tariff_priced_ts,official_hold_nano,charged_hold_nano,official_cost_json::text,
                snapshot_digest,created_ts
           FROM pricing_request_snapshots_v2 WHERE request_id=$1",
        &[&request_id],
    )?
    else {
        return Ok(None);
    };
    let provider_id: String = row.get(11);
    let canonical_model_id: String = row.get(12);
    let rule_id: Option<String> = row.get(13);
    let rule_digest: Option<String> = row.get(14);
    let rule_scope: Option<String> = row.get(15);
    let discount_bps: Option<i64> = row.get(16);
    let payable_multiplier_bp: Option<i64> = row.get(17);
    let rule = match (
        rule_id,
        rule_digest,
        rule_scope.as_deref(),
        discount_bps,
        payable_multiplier_bp,
    ) {
        (None, None, None, None, None) => None,
        (Some(rule_id), Some(rule_digest), Some(scope), Some(discount_bps), Some(multiplier)) => {
            let scope = match scope {
                "global" => super::PricingReleaseRuleScopeV2::Global,
                "provider" => super::PricingReleaseRuleScopeV2::Provider {
                    provider_id: provider_id.clone(),
                },
                "model" => super::PricingReleaseRuleScopeV2::Model {
                    provider_id: provider_id.clone(),
                    canonical_model_id: canonical_model_id.clone(),
                },
                _ => bail!("stored pricing release request rule scope is invalid"),
            };
            Some(super::PricingReleasePolicyRuleV2 {
                rule_id,
                rule_digest,
                scope,
                discount_bps,
                payable_multiplier_bp: multiplier,
            })
        }
        _ => bail!("stored pricing release request rule identity is partial"),
    };
    let snapshot = super::PricingRequestSnapshotV2 {
        request_id: request_id.to_owned(),
        account_id: row.get(0),
        release_schema_version: row.get(1),
        release_generation: row.get(2),
        release_digest: row.get(3),
        assignment_digest: row.get(4),
        account_class: super::AccountClass::from_db(&row.get::<_, String>(5))?,
        policy_id: row.get(6),
        policy_version: row.get(7),
        policy_digest: row.get(8),
        billing_mode: super::BillingModeV2::from_db(&row.get::<_, String>(9))?,
        funding_generation: row.get(10),
        provider_id,
        canonical_model_id,
        rule,
        tariff_schedule_id: row.get(18),
        tariff_priced_ts: row.get(19),
        official_hold_nano: row.get(20),
        charged_hold_nano: row.get(21),
        official_cost_json: serde_json::from_str(&row.get::<_, String>(22))
            .context("decode stored pricing release request official cost")?,
        snapshot_digest: row.get(23),
        created_ts: row.get(24),
    };
    if snapshot.snapshot_digest != super::release_v2::pricing_request_snapshot_digest_v2(&snapshot)?
    {
        bail!("stored pricing release request snapshot digest mismatch");
    }
    Ok(Some(snapshot))
}

pub(crate) fn insert_pricing_request_snapshot_v2<C: GenericClient>(
    client: &mut C,
    snapshot: &super::PricingRequestSnapshotV2,
) -> Result<()> {
    if snapshot.snapshot_digest != super::release_v2::pricing_request_snapshot_digest_v2(snapshot)?
    {
        bail!("pricing release request snapshot digest is invalid");
    }
    let (rule_id, rule_digest, rule_scope, discount_bps, payable_multiplier_bp) =
        if let Some(rule) = snapshot.rule.as_ref() {
            let (scope, _, _) = rule.scope.db_parts();
            (
                Some(rule.rule_id.as_str()),
                Some(rule.rule_digest.as_str()),
                Some(scope),
                Some(rule.discount_bps),
                Some(rule.payable_multiplier_bp),
            )
        } else {
            (None, None, None, None, None)
        };
    let official_cost_json = serde_json::to_string(&snapshot.official_cost_json)
        .context("encode pricing release request official cost")?;
    client.execute(
        "INSERT INTO pricing_request_snapshots_v2(
             request_id,account_id,release_schema_version,release_generation,release_digest,
             assignment_digest,account_class,policy_id,policy_version,policy_digest,billing_mode,
             funding_generation,provider_id,canonical_model_id,rule_id,rule_digest,rule_scope,
             discount_bps,payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,
             official_hold_nano,charged_hold_nano,official_cost_json,snapshot_digest,created_ts)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,
                $21,$22,$23,$24::text::jsonb,$25,$26)",
        &[
            &snapshot.request_id,
            &snapshot.account_id,
            &snapshot.release_schema_version,
            &snapshot.release_generation,
            &snapshot.release_digest,
            &snapshot.assignment_digest,
            &snapshot.account_class.as_str(),
            &snapshot.policy_id,
            &snapshot.policy_version,
            &snapshot.policy_digest,
            &snapshot.billing_mode.as_str(),
            &snapshot.funding_generation,
            &snapshot.provider_id,
            &snapshot.canonical_model_id,
            &rule_id,
            &rule_digest,
            &rule_scope,
            &discount_bps,
            &payable_multiplier_bp,
            &snapshot.tariff_schedule_id,
            &snapshot.tariff_priced_ts,
            &snapshot.official_hold_nano,
            &snapshot.charged_hold_nano,
            &official_cost_json,
            &snapshot.snapshot_digest,
            &snapshot.created_ts,
        ],
    )?;
    Ok(())
}

pub(crate) fn postgres_pricing_release_inventory_v2(
    client: &mut Client,
    after_account_id: Option<&str>,
    limit: i64,
) -> Result<super::PricingReleaseInventoryPageV2> {
    if !(1..=500).contains(&limit) {
        bail!("pricing release inventory limit must be within 1..=500");
    }
    if let Some(after_account_id) = after_account_id {
        super::require_id("pricing release inventory cursor", after_account_id)?;
    }
    let fetch_limit = limit + 1;
    let mut accounts = client
        .query(
            "SELECT account.id,account.status,account.mult_bp,account.balance_nano,
                account.reserved_nano,account.spent_nano,head.active_generation,head.head_version
           FROM accounts account
           LEFT JOIN account_funding_head_v2 head ON head.account_id=account.id
          WHERE ($1::text IS NULL OR account.id>$1)
          ORDER BY account.id LIMIT $2",
            &[&after_account_id, &fetch_limit],
        )?
        .into_iter()
        .map(|row| super::PricingReleaseInventoryAccountV2 {
            account_id: row.get(0),
            status: row.get(1),
            multiplier_bp: row.get(2),
            balance_nano: row.get(3),
            reserved_nano: row.get(4),
            spent_nano: row.get(5),
            funding_generation: row.get(6),
            funding_head_version: row.get(7),
        })
        .collect::<Vec<_>>();
    let next_after_account_id = (accounts.len() > limit as usize).then(|| {
        accounts.truncate(limit as usize);
        accounts
            .last()
            .expect("non-empty paginated inventory")
            .account_id
            .clone()
    });
    Ok(super::PricingReleaseInventoryPageV2 {
        accounts,
        next_after_account_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pg::{PgStore, POSTGRES_DESTRUCTIVE_TEST_LOCK};
    use crate::pricing::{PolicySegment, PRICING_SCHEMA_VERSION};
    use std::sync::{Arc, Barrier};
    use tokio_postgres_rustls::MakeRustlsConnect;

    fn connect_client(url: &str) -> Client {
        let config: postgres::Config = url.parse().expect("parse PostgreSQL contract URL");
        let (connector, _certificate_errors) =
            MakeRustlsConnect::with_native_certs().expect("load PostgreSQL root certificates");
        config
            .connect(connector)
            .expect("connect PostgreSQL pricing contract client")
    }

    fn test_client() -> Option<(String, Client)> {
        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping PostgreSQL pricing contract: CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return None;
        };

        let mut client = connect_client(&url);
        client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .expect("serialize destructive PostgreSQL registry tests");

        let mut store = PgStore::connect(&url).expect("connect PostgreSQL migration client");
        store
            .migrate()
            .expect("migrate isolated PostgreSQL contract database");
        drop(store);

        Some((url, client))
    }

    fn catalog(product_id: &str, generation: i64, digest: &str) -> PricingCatalogSpec {
        PricingCatalogSpec {
            product_id: product_id.to_owned(),
            generation,
            schema_version: PRICING_SCHEMA_VERSION,
            capability_generation: 17,
            capability_digest: "capability-17".to_owned(),
            content_digest: digest.to_owned(),
            entries: vec![
                PricingCatalogEntrySpec {
                    provider_id: "openai".to_owned(),
                    canonical_model_id: "gpt-5".to_owned(),
                    enabled: true,
                },
                PricingCatalogEntrySpec {
                    provider_id: "anthropic".to_owned(),
                    canonical_model_id: "claude-sonnet-4".to_owned(),
                    enabled: true,
                },
            ],
        }
    }

    fn switches(generation: i64, digest: &str) -> ProviderSwitchSpec {
        switches_for_catalog(generation, 1, digest)
    }

    fn switches_for_catalog(
        generation: i64,
        catalog_generation: i64,
        digest: &str,
    ) -> ProviderSwitchSpec {
        let scoped = [
            (
                "anthropic",
                ProviderSwitchScope::Segment {
                    product_id: "main".to_owned(),
                    segment: PolicySegment::B2b,
                },
            ),
            (
                "openai",
                ProviderSwitchScope::Segment {
                    product_id: "main".to_owned(),
                    segment: PolicySegment::B2b,
                },
            ),
            (
                "anthropic",
                ProviderSwitchScope::Product {
                    product_id: "openkeys".to_owned(),
                },
            ),
            (
                "openai",
                ProviderSwitchScope::Product {
                    product_id: "openkeys".to_owned(),
                },
            ),
        ];
        let mut entries = vec![
            ProviderSwitchEntrySpec {
                provider_id: "anthropic".to_owned(),
                scope: ProviderSwitchScope::Master,
                catalog_generation: None,
                enabled: true,
            },
            ProviderSwitchEntrySpec {
                provider_id: "openai".to_owned(),
                scope: ProviderSwitchScope::Master,
                catalog_generation: None,
                enabled: true,
            },
        ];
        entries.extend(
            scoped
                .into_iter()
                .map(|(provider_id, scope)| ProviderSwitchEntrySpec {
                    provider_id: provider_id.to_owned(),
                    scope,
                    catalog_generation: Some(catalog_generation),
                    enabled: true,
                }),
        );
        ProviderSwitchSpec {
            generation,
            schema_version: PRICING_SCHEMA_VERSION,
            capability_generation: 17,
            capability_digest: "capability-17".to_owned(),
            content_digest: digest.to_owned(),
            entries,
        }
    }

    fn main_b2b_switches_for_catalog(
        generation: i64,
        catalog_generation: i64,
        digest: &str,
    ) -> ProviderSwitchSpec {
        let mut spec = switches_for_catalog(generation, catalog_generation, digest);
        spec.entries.retain(|entry| {
            matches!(entry.scope, ProviderSwitchScope::Master)
                || matches!(
                    &entry.scope,
                    ProviderSwitchScope::Segment {
                        product_id,
                        segment: PolicySegment::B2b,
                    } if product_id == "main"
                )
        });
        spec
    }

    fn b2b_policy(effective_version: i64, policy_version: i64, digest: &str) -> AccountPolicySpec {
        b2b_policy_for_lineage(
            "pricing-pg-contract-b2b",
            effective_version,
            policy_version,
            1,
            1,
            digest,
        )
    }

    fn b2b_policy_for_lineage(
        account_id: &str,
        effective_version: i64,
        policy_version: i64,
        catalog_generation: i64,
        switch_generation: i64,
        digest: &str,
    ) -> AccountPolicySpec {
        AccountPolicySpec {
            account_id: account_id.to_owned(),
            effective_version,
            policy_id: "contract-b2b-policy".to_owned(),
            policy_version,
            source_policy_digest: format!("contract-b2b-source-{policy_version}"),
            owner_type: PolicyOwnerType::B2bClient,
            owner_id: "contract-client".to_owned(),
            account_class: AccountClass::B2b,
            product_id: "main".to_owned(),
            schema_version: PRICING_SCHEMA_VERSION,
            catalog_generation,
            switch_generation,
            content_digest: digest.to_owned(),
            replacement_locked: false,
            rules: vec![AccountPolicyRuleSpec {
                rule_id: format!("contract-b2b-rule-{policy_version}"),
                rule_digest: format!("contract-b2b-rule-digest-{policy_version}"),
                scope: PolicyRuleScope::Provider {
                    provider_id: "anthropic".to_owned(),
                },
                pricing_mode: PricingMode::Discount,
                rule_origin: RuleOrigin::Managed,
                discount_bps: Some(2_000),
                payable_multiplier_bp: 8_000,
                track_eligible: false,
                retention_eligible: false,
                commission_eligible: false,
            }],
        }
    }

    fn openkeys_policy(account_id: &str, effective_version: i64) -> AccountPolicySpec {
        AccountPolicySpec {
            account_id: account_id.to_owned(),
            effective_version,
            policy_id: format!("contract-openkeys-policy-{account_id}"),
            policy_version: effective_version,
            source_policy_digest: format!("contract-openkeys-source-{effective_version}"),
            owner_type: PolicyOwnerType::OpenKeys,
            owner_id: account_id.to_owned(),
            account_class: AccountClass::OpenKeys,
            product_id: "openkeys".to_owned(),
            schema_version: PRICING_SCHEMA_VERSION,
            catalog_generation: 1,
            switch_generation: 1,
            content_digest: format!("contract-openkeys-policy-{effective_version}"),
            replacement_locked: true,
            rules: ["anthropic", "openai"]
                .into_iter()
                .map(|provider_id| AccountPolicyRuleSpec {
                    rule_id: format!("contract-openkeys-{provider_id}-{effective_version}"),
                    rule_digest: format!(
                        "contract-openkeys-{provider_id}-digest-{effective_version}"
                    ),
                    scope: PolicyRuleScope::Provider {
                        provider_id: provider_id.to_owned(),
                    },
                    pricing_mode: PricingMode::Discount,
                    rule_origin: RuleOrigin::Legacy,
                    discount_bps: None,
                    payable_multiplier_bp: 7_300,
                    track_eligible: false,
                    retention_eligible: false,
                    commission_eligible: false,
                })
                .collect(),
        }
    }

    fn binding(
        policy_enforcement: PolicyEnforcement,
        funding_enforcement: FundingEnforcement,
        reconciliation_state: ReconciliationState,
    ) -> AccountPolicyBindingSpec {
        AccountPolicyBindingSpec {
            policy_enforcement,
            funding_enforcement,
            reconciliation_state,
        }
    }

    fn activation(
        policy: &AccountPolicySpec,
        binding: AccountPolicyBindingSpec,
    ) -> AccountPolicyActivationSpec {
        AccountPolicyActivationSpec {
            account_id: policy.account_id.clone(),
            effective_version: policy.effective_version,
            content_digest: policy.content_digest.clone(),
            binding,
        }
    }

    fn is_missing(mutation: &PricingMutation) -> bool {
        matches!(
            mutation,
            PricingMutation::Rejected(PricingRejection::MissingDependency { .. })
        )
    }

    fn assert_active_bundle_lineages(
        client: &mut Client,
        account_id: &str,
        expected_policy: &AccountPolicySpec,
        expected_binding: &AccountPolicyBindingSpec,
        expected_policy_catalog: &PricingCatalogSpec,
        expected_policy_switches: &ProviderSwitchSpec,
        expected_admission_catalog: &PricingCatalogSpec,
        expected_admission_switches: &ProviderSwitchSpec,
    ) {
        let bundle = postgres_pricing_read_bundle(client, account_id)
            .expect("read PostgreSQL dual-lineage pricing bundle");
        assert_eq!(bundle.account_id, account_id);
        assert_eq!(bundle.account_multiplier_bp, 8_000);
        assert_eq!(
            bundle.policy,
            PricingPolicySnapshot::Active(ActiveAccountPolicy {
                policy: normalize_policy(expected_policy),
                binding: expected_binding.clone(),
            })
        );
        assert_eq!(
            bundle.policy_catalog,
            Some(normalize_catalog(expected_policy_catalog))
        );
        assert_eq!(
            bundle.policy_switches,
            Some(normalize_switches(expected_policy_switches))
        );
        assert_eq!(
            bundle.admission_catalog,
            Some(normalize_catalog(expected_admission_catalog))
        );
        assert_eq!(
            bundle.admission_switches,
            Some(normalize_switches(expected_admission_switches))
        );
    }

    fn run_postgres_dual_lineage_rollout_matrix(client: &mut Client) {
        const ACCOUNT_ID: &str = "pricing-pg-contract-dual-lineage";

        client
            .batch_execute(
                "TRUNCATE
                     account_policy_bindings,
                     account_policy_rules,
                     account_policy_versions,
                     provider_switch_head,
                     provider_switch_entries,
                     provider_switch_versions,
                     pricing_catalog_heads,
                     pricing_catalog_entries,
                     pricing_catalog_versions
                 CASCADE;
                 INSERT INTO accounts(id,mult_bp,status,created_ts,created)
                 VALUES('pricing-pg-contract-dual-lineage',8000,'active',1,'')
                 ON CONFLICT(id) DO UPDATE SET mult_bp=EXCLUDED.mult_bp,status='active';",
            )
            .expect("reset PostgreSQL dual-lineage fixtures");

        let catalog_v1 = catalog("main", 1, "dual-lineage-catalog-1");
        let catalog_v2 = catalog("main", 2, "dual-lineage-catalog-2");
        let switches_v1 = main_b2b_switches_for_catalog(1, 1, "dual-lineage-switches-1");
        let switches_v2 = main_b2b_switches_for_catalog(2, 2, "dual-lineage-switches-2");
        let policy_v1 = b2b_policy_for_lineage(ACCOUNT_ID, 1, 1, 1, 1, "dual-lineage-policy-1");
        let policy_v2 = b2b_policy_for_lineage(ACCOUNT_ID, 2, 2, 2, 2, "dual-lineage-policy-2");
        let binding = binding(
            PolicyEnforcement::Shadow,
            FundingEnforcement::Shadow,
            ReconciliationState::Pending,
        );

        for catalog in [&catalog_v1, &catalog_v2] {
            assert_eq!(
                postgres_prepare_pricing_catalog(client, catalog).unwrap(),
                PricingMutation::Stored
            );
        }
        for switches in [&switches_v1, &switches_v2] {
            assert_eq!(
                postgres_prepare_provider_switches(client, switches).unwrap(),
                PricingMutation::Stored
            );
        }
        for policy in [&policy_v1, &policy_v2] {
            assert_eq!(
                postgres_prepare_account_policy(client, policy).unwrap(),
                PricingMutation::Stored
            );
        }

        assert_eq!(
            postgres_activate_pricing_catalog(
                client,
                "main",
                &catalog_v1.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            postgres_activate_provider_switches(
                client,
                &switches_v1.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            postgres_pricing_read_bundle(client, ACCOUNT_ID).unwrap(),
            PricingReadBundle {
                account_id: ACCOUNT_ID.to_owned(),
                account_multiplier_bp: 8_000,
                policy: PricingPolicySnapshot::Unbound,
                policy_catalog: None,
                policy_switches: None,
                admission_catalog: None,
                admission_switches: None,
            },
            "an unbound account has no product context even when global heads exist"
        );
        assert_eq!(
            postgres_activate_account_policy(
                client,
                &activation(&policy_v1, binding.clone()),
                &PolicyActiveExpectation::Unbound,
            )
            .unwrap(),
            PricingMutation::Applied
        );

        // C1/S1/P1: both lineages initially agree.
        assert_active_bundle_lineages(
            client,
            ACCOUNT_ID,
            &policy_v1,
            &binding,
            &catalog_v1,
            &switches_v1,
            &catalog_v1,
            &switches_v1,
        );

        assert_eq!(
            postgres_activate_pricing_catalog(
                client,
                "main",
                &catalog_v2.target(),
                &ActiveExpectation::Exact(catalog_v1.target()),
            )
            .unwrap(),
            PricingMutation::Applied
        );
        // C2/S1/P1: admission sees C2 while policy resolution remains pinned to C1/S1.
        assert_active_bundle_lineages(
            client,
            ACCOUNT_ID,
            &policy_v1,
            &binding,
            &catalog_v1,
            &switches_v1,
            &catalog_v2,
            &switches_v1,
        );

        assert_eq!(
            postgres_activate_provider_switches(
                client,
                &switches_v2.target(),
                &ActiveExpectation::Exact(switches_v1.target()),
            )
            .unwrap(),
            PricingMutation::Applied
        );
        // C2/S2/P1: admission has advanced fully; the old policy still resolves C1/S1.
        assert_active_bundle_lineages(
            client,
            ACCOUNT_ID,
            &policy_v1,
            &binding,
            &catalog_v1,
            &switches_v1,
            &catalog_v2,
            &switches_v2,
        );

        assert_eq!(
            postgres_activate_account_policy(
                client,
                &activation(&policy_v2, binding.clone()),
                &PolicyActiveExpectation::Exact(ActivePolicyTarget {
                    target: policy_v1.target(),
                    binding: binding.clone(),
                }),
            )
            .unwrap(),
            PricingMutation::Applied
        );
        // C2/S2/P2: policy and admission lineages converge again.
        assert_active_bundle_lineages(
            client,
            ACCOUNT_ID,
            &policy_v2,
            &binding,
            &catalog_v2,
            &switches_v2,
            &catalog_v2,
            &switches_v2,
        );

        client
            .batch_execute(
                "TRUNCATE
                     account_policy_bindings,
                     account_policy_rules,
                     account_policy_versions,
                     provider_switch_head,
                     provider_switch_entries,
                     provider_switch_versions,
                     pricing_catalog_heads,
                     pricing_catalog_entries,
                     pricing_catalog_versions
                 CASCADE;
                 DELETE FROM accounts WHERE id='pricing-pg-contract-dual-lineage';",
            )
            .expect("clean PostgreSQL dual-lineage fixtures");
    }

    /// Run against an isolated database:
    /// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
    /// pricing::postgres::tests::postgres_pricing_contract_matrix -- --nocapture`
    #[test]
    fn postgres_pricing_contract_matrix() {
        let Some((url, mut client)) = test_client() else {
            return;
        };
        client
            .batch_execute(
                "SET statement_timeout='15s';
                 SET lock_timeout='5s';
                 TRUNCATE
                     account_policy_bindings,
                     account_policy_rules,
                     account_policy_versions,
                     provider_switch_head,
                     provider_switch_entries,
                     provider_switch_versions,
                     pricing_catalog_heads,
                     pricing_catalog_entries,
                     pricing_catalog_versions
                 CASCADE;
                 INSERT INTO accounts(id,mult_bp,status,created_ts,created) VALUES
                     ('pricing-pg-contract-b2b',8000,'active',1,''),
                     ('pricing-pg-contract-openkeys',7300,'active',1,''),
                     ('pricing-pg-contract-openkeys-mismatch',7400,'active',1,'')
                 ON CONFLICT(id) DO UPDATE SET mult_bp=EXCLUDED.mult_bp,status='active';",
            )
            .expect("reset isolated PostgreSQL pricing fixtures");

        client.batch_execute("BEGIN").unwrap();
        client
            .query_one(
                "SELECT mult_bp FROM accounts
                 WHERE id='pricing-pg-contract-openkeys'
                 FOR SHARE",
                &[],
            )
            .unwrap();
        let mut scalar_writer = connect_client(&url);
        scalar_writer
            .batch_execute("SET lock_timeout='200ms'")
            .unwrap();
        assert!(scalar_writer
            .execute(
                "UPDATE accounts SET mult_bp=7400
                 WHERE id='pricing-pg-contract-openkeys'",
                &[],
            )
            .is_err());
        client.batch_execute("ROLLBACK").unwrap();
        assert_eq!(
            scalar_writer
                .execute(
                    "UPDATE accounts SET mult_bp=7300
                     WHERE id='pricing-pg-contract-openkeys'",
                    &[],
                )
                .unwrap(),
            1
        );

        client
            .batch_execute(
                "DROP TRIGGER IF EXISTS pricing_contract_reject_child
                     ON pricing_catalog_entries;
                 DROP FUNCTION IF EXISTS pricing_contract_reject_child();
                 CREATE FUNCTION pricing_contract_reject_child()
                 RETURNS trigger LANGUAGE plpgsql AS $$
                 BEGIN
                     IF NEW.provider_id = 'openai' THEN
                         RAISE EXCEPTION 'injected pricing child failure';
                     END IF;
                     RETURN NEW;
                 END;
                 $$;
                 CREATE TRIGGER pricing_contract_reject_child
                 BEFORE INSERT ON pricing_catalog_entries
                 FOR EACH ROW EXECUTE FUNCTION pricing_contract_reject_child();",
            )
            .unwrap();
        assert!(postgres_prepare_pricing_catalog(
            &mut client,
            &catalog("rollback", 1, "rollback-catalog")
        )
        .is_err());
        let rollback_counts: (i64, i64) = client
            .query_one(
                "SELECT
                     (SELECT COUNT(*)::bigint FROM pricing_catalog_versions
                       WHERE product_id='rollback'),
                     (SELECT COUNT(*)::bigint FROM pricing_catalog_entries
                       WHERE product_id='rollback')",
                &[],
            )
            .map(|row| (row.get(0), row.get(1)))
            .unwrap();
        assert_eq!(rollback_counts, (0, 0));
        client
            .batch_execute(
                "DROP TRIGGER pricing_contract_reject_child ON pricing_catalog_entries;
                 DROP FUNCTION pricing_contract_reject_child();",
            )
            .unwrap();

        let main_catalog = catalog("main", 1, "main-catalog-1");
        let openkeys_catalog = catalog("openkeys", 1, "openkeys-catalog-1");
        let switch_v1 = switches(1, "switches-1");
        let b2b_v1 = b2b_policy(1, 1, "b2b-policy-1");
        let b2b_v3 = b2b_policy(3, 3, "b2b-policy-3");
        let openkeys_v1 = openkeys_policy("pricing-pg-contract-openkeys", 1);

        assert_eq!(
            postgres_pricing_read_bundle(&mut client, "pricing-pg-contract-openkeys-mismatch",)
                .unwrap(),
            PricingReadBundle {
                account_id: "pricing-pg-contract-openkeys-mismatch".to_owned(),
                account_multiplier_bp: 7_400,
                policy: PricingPolicySnapshot::Unbound,
                policy_catalog: None,
                policy_switches: None,
                admission_catalog: None,
                admission_switches: None,
            }
        );

        let mut malformed_policy = b2b_v1.clone();
        malformed_policy.effective_version = 0;
        assert!(matches!(
            postgres_prepare_account_policy(&mut client, &malformed_policy).unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));

        let mut wrong_product_policy = b2b_v1.clone();
        wrong_product_policy.product_id = "other-product".to_owned();
        assert!(matches!(
            postgres_prepare_account_policy(&mut client, &wrong_product_policy).unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));

        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &main_catalog).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &openkeys_catalog).unwrap(),
            PricingMutation::Stored
        );

        client
            .batch_execute(
                "CREATE FUNCTION pricing_contract_reject_switch_child()
                 RETURNS trigger LANGUAGE plpgsql AS $$
                 BEGIN
                     IF NEW.provider_id = 'openai' THEN
                         RAISE EXCEPTION 'injected switch child failure';
                     END IF;
                     RETURN NEW;
                 END;
                 $$;
                 CREATE TRIGGER pricing_contract_reject_switch_child
                 BEFORE INSERT ON provider_switch_entries
                 FOR EACH ROW EXECUTE FUNCTION pricing_contract_reject_switch_child();",
            )
            .unwrap();
        assert!(postgres_prepare_provider_switches(&mut client, &switch_v1).is_err());
        let switch_rollback_counts: (i64, i64) = client
            .query_one(
                "SELECT
                     (SELECT COUNT(*)::bigint FROM provider_switch_versions
                       WHERE generation=1),
                     (SELECT COUNT(*)::bigint FROM provider_switch_entries
                       WHERE generation=1)",
                &[],
            )
            .map(|row| (row.get(0), row.get(1)))
            .unwrap();
        assert_eq!(switch_rollback_counts, (0, 0));
        client
            .batch_execute(
                "DROP TRIGGER pricing_contract_reject_switch_child
                     ON provider_switch_entries;
                 DROP FUNCTION pricing_contract_reject_switch_child();",
            )
            .unwrap();
        assert_eq!(
            postgres_prepare_provider_switches(&mut client, &switch_v1).unwrap(),
            PricingMutation::Stored
        );

        client
            .batch_execute(
                "CREATE FUNCTION pricing_contract_reject_policy_child()
                 RETURNS trigger LANGUAGE plpgsql AS $$
                 BEGIN
                     IF NEW.provider_id = 'openai' THEN
                         RAISE EXCEPTION 'injected policy child failure';
                     END IF;
                     RETURN NEW;
                 END;
                 $$;
                 CREATE TRIGGER pricing_contract_reject_policy_child
                 BEFORE INSERT ON account_policy_rules
                 FOR EACH ROW EXECUTE FUNCTION pricing_contract_reject_policy_child();",
            )
            .unwrap();
        assert!(postgres_prepare_account_policy(&mut client, &openkeys_v1).is_err());
        let policy_rollback_counts: (i64, i64) = client
            .query_one(
                "SELECT
                     (SELECT COUNT(*)::bigint FROM account_policy_versions
                       WHERE account_id='pricing-pg-contract-openkeys'),
                     (SELECT COUNT(*)::bigint FROM account_policy_rules
                       WHERE account_id='pricing-pg-contract-openkeys')",
                &[],
            )
            .map(|row| (row.get(0), row.get(1)))
            .unwrap();
        assert_eq!(policy_rollback_counts, (0, 0));
        client
            .batch_execute(
                "DROP TRIGGER pricing_contract_reject_policy_child
                     ON account_policy_rules;
                 DROP FUNCTION pricing_contract_reject_policy_child();",
            )
            .unwrap();
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &b2b_v1).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &openkeys_v1).unwrap(),
            PricingMutation::Stored
        );

        // Prepare persists complete immutable lineage but never creates or moves a head/binding.
        let head_and_binding_count: i64 = client
            .query_one(
                "SELECT
                     (SELECT COUNT(*) FROM pricing_catalog_heads)
                   + (SELECT COUNT(*) FROM provider_switch_head)
                   + (SELECT COUNT(*) FROM account_policy_bindings)",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(head_and_binding_count, 0);
        assert_eq!(
            postgres_pricing_catalog_by_generation(&mut client, "main", 1).unwrap(),
            Some(normalize_catalog(&main_catalog))
        );
        assert_eq!(
            postgres_provider_switches_by_generation(&mut client, 1).unwrap(),
            Some(normalize_switches(&switch_v1))
        );
        assert_eq!(
            postgres_account_policy_by_version(&mut client, "pricing-pg-contract-openkeys", 1)
                .unwrap(),
            Some(normalize_policy(&openkeys_v1))
        );
        let registry_timestamp: i64 = client
            .query_one(
                "SELECT created_ts FROM account_policy_versions
                 WHERE account_id='pricing-pg-contract-openkeys' AND effective_version=1",
                &[],
            )
            .unwrap()
            .get(0);
        assert!(registry_timestamp > 0);

        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &main_catalog).unwrap(),
            PricingMutation::Unchanged
        );
        assert_eq!(
            postgres_prepare_provider_switches(&mut client, &switch_v1).unwrap(),
            PricingMutation::Unchanged
        );
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &b2b_v1).unwrap(),
            PricingMutation::Unchanged
        );

        let race_v1 = catalog("race", 1, "race-catalog-1");
        let race_v2 = catalog("race", 2, "race-catalog-2");
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &race_v1).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &race_v2).unwrap(),
            PricingMutation::Stored
        );
        let barrier = Arc::new(Barrier::new(2));
        let racers = [race_v1.target(), race_v2.target()].map(|target| {
            let url = url.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut racer = connect_client(&url);
                barrier.wait();
                postgres_activate_pricing_catalog(
                    &mut racer,
                    "race",
                    &target,
                    &ActiveExpectation::Absent,
                )
                .unwrap()
            })
        });
        let race_outcomes = racers.map(|racer| racer.join().expect("pricing CAS racer"));
        assert_eq!(
            race_outcomes
                .iter()
                .filter(|outcome| **outcome == PricingMutation::Applied)
                .count(),
            1
        );
        assert!(race_outcomes.iter().any(|outcome| matches!(
            outcome,
            PricingMutation::Rejected(
                PricingRejection::CasMismatch { .. } | PricingRejection::Stale { .. }
            )
        )));

        let mut catalog_conflict = main_catalog.clone();
        catalog_conflict.content_digest = "main-catalog-conflict".to_owned();
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &catalog_conflict).unwrap(),
            version_conflict()
        );
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &catalog("history", 1, "history-1"))
                .unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &catalog("history", 3, "history-3"))
                .unwrap(),
            PricingMutation::Stored
        );
        assert!(matches!(
            postgres_prepare_pricing_catalog(&mut client, &catalog("history", 2, "history-2"))
                .unwrap(),
            PricingMutation::Rejected(PricingRejection::Stale { .. })
        ));

        let switch_v3 = switches(3, "switches-3");
        assert_eq!(
            postgres_prepare_provider_switches(&mut client, &switch_v3).unwrap(),
            PricingMutation::Stored
        );
        assert!(matches!(
            postgres_prepare_provider_switches(&mut client, &switches(2, "switches-2")).unwrap(),
            PricingMutation::Rejected(PricingRejection::Stale { .. })
        ));
        let mut switch_conflict = switch_v1.clone();
        switch_conflict.content_digest = "switches-conflict".to_owned();
        assert_eq!(
            postgres_prepare_provider_switches(&mut client, &switch_conflict).unwrap(),
            version_conflict()
        );

        let mut b2b_conflict = b2b_v1.clone();
        b2b_conflict.content_digest = "b2b-policy-conflict".to_owned();
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &b2b_conflict).unwrap(),
            version_conflict()
        );
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &b2b_v3).unwrap(),
            PricingMutation::Stored
        );
        assert!(matches!(
            postgres_prepare_account_policy(&mut client, &b2b_policy(2, 2, "b2b-policy-2"))
                .unwrap(),
            PricingMutation::Rejected(PricingRejection::Stale { .. })
        ));

        let openkeys_v2 = openkeys_policy("pricing-pg-contract-openkeys", 2);
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &openkeys_v2).unwrap(),
            locked()
        );
        let mismatched_openkeys = openkeys_policy("pricing-pg-contract-openkeys-mismatch", 1);
        assert!(matches!(
            postgres_prepare_account_policy(&mut client, &mismatched_openkeys).unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));

        let inactive = binding(
            PolicyEnforcement::LegacyScalar,
            FundingEnforcement::LegacySingle,
            ReconciliationState::Pending,
        );
        client
            .execute(
                "INSERT INTO account_policy_bindings(
                     account_id,product_id,account_class,active_effective_version,
                     policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
                 ) VALUES($1,'main','b2b',NULL,$2,$3,$4,1)",
                &[
                    &b2b_v1.account_id,
                    &inactive.policy_enforcement.as_str(),
                    &inactive.funding_enforcement.as_str(),
                    &inactive.reconciliation_state.as_str(),
                ],
            )
            .unwrap();
        assert_eq!(
            postgres_active_account_policy(&mut client, &b2b_v1.account_id).unwrap(),
            None
        );
        assert_eq!(
            postgres_pricing_read_bundle(&mut client, &b2b_v1.account_id).unwrap(),
            PricingReadBundle {
                account_id: b2b_v1.account_id.clone(),
                account_multiplier_bp: 8_000,
                policy: PricingPolicySnapshot::Inactive {
                    product_id: "main".to_owned(),
                    account_class: AccountClass::B2b,
                    binding: inactive.clone(),
                },
                policy_catalog: None,
                policy_switches: None,
                admission_catalog: None,
                admission_switches: None,
            }
        );

        let active_v1_binding = binding(
            PolicyEnforcement::Shadow,
            FundingEnforcement::Shadow,
            ReconciliationState::Pending,
        );
        let activate_v1 = activation(&b2b_v1, active_v1_binding.clone());
        let strict = activation(
            &b2b_v1,
            binding(
                PolicyEnforcement::Strict,
                FundingEnforcement::Strict,
                ReconciliationState::Verified,
            ),
        );
        // Strict is now a valid dormant binding, but it cannot activate before the exact
        // catalog and switch dependencies are active.
        assert!(is_missing(
            &postgres_activate_account_policy(
                &mut client,
                &strict,
                &PolicyActiveExpectation::Inactive(inactive.clone())
            )
            .unwrap()
        ));
        assert!(is_missing(
            &postgres_activate_account_policy(
                &mut client,
                &activate_v1,
                &PolicyActiveExpectation::Inactive(inactive.clone())
            )
            .unwrap()
        ));
        let still_null: Option<i64> = client
            .query_one(
                "SELECT active_effective_version FROM account_policy_bindings
                 WHERE account_id=$1",
                &[&b2b_v1.account_id],
            )
            .unwrap()
            .get(0);
        assert_eq!(still_null, None);

        // A prepared switch is not activatable until every exact pinned catalog is active.
        assert!(is_missing(
            &postgres_activate_provider_switches(
                &mut client,
                &switch_v1.target(),
                &ActiveExpectation::Absent
            )
            .unwrap()
        ));
        assert_eq!(
            postgres_active_provider_switches(&mut client).unwrap(),
            None
        );

        assert_eq!(
            postgres_activate_pricing_catalog(
                &mut client,
                "main",
                &main_catalog.target(),
                &ActiveExpectation::Absent
            )
            .unwrap(),
            PricingMutation::Applied
        );
        // Lost ACK is idempotent even when the retry carries an obsolete expectation.
        assert_eq!(
            postgres_activate_pricing_catalog(
                &mut client,
                "main",
                &main_catalog.target(),
                &ActiveExpectation::Exact(VersionTarget::new(99, "obsolete"))
            )
            .unwrap(),
            PricingMutation::Unchanged
        );
        let catalog_only = postgres_pricing_read_bundle(&mut client, &b2b_v1.account_id).unwrap();
        assert_eq!(catalog_only.policy_catalog, None);
        assert_eq!(catalog_only.policy_switches, None);
        assert_eq!(
            catalog_only.admission_catalog,
            Some(normalize_catalog(&main_catalog))
        );
        assert_eq!(catalog_only.admission_switches, None);
        assert!(matches!(
            catalog_only.policy,
            PricingPolicySnapshot::Inactive { .. }
        ));
        assert!(is_missing(
            &postgres_activate_provider_switches(
                &mut client,
                &switch_v1.target(),
                &ActiveExpectation::Absent
            )
            .unwrap()
        ));
        assert_eq!(
            postgres_activate_pricing_catalog(
                &mut client,
                "openkeys",
                &openkeys_catalog.target(),
                &ActiveExpectation::Absent
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            postgres_activate_provider_switches(
                &mut client,
                &switch_v1.target(),
                &ActiveExpectation::Absent
            )
            .unwrap(),
            PricingMutation::Applied
        );
        let active_heads = postgres_pricing_read_bundle(&mut client, &b2b_v1.account_id).unwrap();
        assert_eq!(active_heads.policy_catalog, None);
        assert_eq!(active_heads.policy_switches, None);
        assert_eq!(
            active_heads.admission_catalog,
            Some(normalize_catalog(&main_catalog))
        );
        assert_eq!(
            active_heads.admission_switches,
            Some(normalize_switches(&switch_v1))
        );
        assert!(matches!(
            active_heads.policy,
            PricingPolicySnapshot::Inactive { .. }
        ));
        assert_eq!(
            postgres_activate_provider_switches(
                &mut client,
                &switch_v1.target(),
                &ActiveExpectation::Exact(VersionTarget::new(99, "obsolete"))
            )
            .unwrap(),
            PricingMutation::Unchanged
        );
        assert!(matches!(
            postgres_activate_provider_switches(
                &mut client,
                &switch_v3.target(),
                &ActiveExpectation::Absent
            )
            .unwrap(),
            PricingMutation::Rejected(PricingRejection::CasMismatch { .. })
        ));

        assert_eq!(
            postgres_activate_account_policy(
                &mut client,
                &activate_v1,
                &PolicyActiveExpectation::Unbound
            )
            .unwrap(),
            policy_cas_mismatch(PolicyBindingState::Inactive(inactive.clone()))
        );
        assert_eq!(
            postgres_activate_account_policy(
                &mut client,
                &activate_v1,
                &PolicyActiveExpectation::Inactive(inactive)
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            postgres_pricing_read_bundle(&mut client, &b2b_v1.account_id).unwrap(),
            PricingReadBundle {
                account_id: b2b_v1.account_id.clone(),
                account_multiplier_bp: 8_000,
                policy: PricingPolicySnapshot::Active(ActiveAccountPolicy {
                    policy: normalize_policy(&b2b_v1),
                    binding: active_v1_binding.clone(),
                }),
                policy_catalog: Some(normalize_catalog(&main_catalog)),
                policy_switches: Some(normalize_switches(&switch_v1)),
                admission_catalog: Some(normalize_catalog(&main_catalog)),
                admission_switches: Some(normalize_switches(&switch_v1)),
            }
        );
        assert_eq!(
            postgres_activate_account_policy(
                &mut client,
                &activate_v1,
                &PolicyActiveExpectation::Unbound
            )
            .unwrap(),
            PricingMutation::Unchanged
        );

        let active_v3_binding = binding(
            PolicyEnforcement::Shadow,
            FundingEnforcement::Shadow,
            ReconciliationState::Verified,
        );
        let activate_v3 = activation(&b2b_v3, active_v3_binding.clone());
        let wrong_expected = PolicyActiveExpectation::Exact(ActivePolicyTarget {
            target: b2b_v1.target(),
            binding: binding(
                PolicyEnforcement::LegacyScalar,
                FundingEnforcement::LegacySingle,
                ReconciliationState::Pending,
            ),
        });
        assert!(matches!(
            postgres_activate_account_policy(&mut client, &activate_v3, &wrong_expected).unwrap(),
            PricingMutation::Rejected(PricingRejection::PolicyCasMismatch { .. })
        ));
        let expected_v1 = PolicyActiveExpectation::Exact(ActivePolicyTarget {
            target: b2b_v1.target(),
            binding: active_v1_binding,
        });
        assert_eq!(
            postgres_activate_account_policy(&mut client, &activate_v3, &expected_v1).unwrap(),
            PricingMutation::Applied
        );
        assert!(matches!(
            postgres_activate_account_policy(
                &mut client,
                &activate_v1,
                &PolicyActiveExpectation::Exact(ActivePolicyTarget {
                    target: b2b_v3.target(),
                    binding: active_v3_binding.clone(),
                })
            )
            .unwrap(),
            PricingMutation::Rejected(PricingRejection::Stale { .. })
        ));
        assert_eq!(
            postgres_active_account_policy(&mut client, &b2b_v1.account_id)
                .unwrap()
                .expect("active B2B policy"),
            ActiveAccountPolicy {
                policy: normalize_policy(&b2b_v3),
                binding: active_v3_binding.clone(),
            }
        );

        let openkeys_binding = binding(
            PolicyEnforcement::Shadow,
            FundingEnforcement::Shadow,
            ReconciliationState::Verified,
        );
        let activate_openkeys = activation(&openkeys_v1, openkeys_binding.clone());
        assert_eq!(
            postgres_activate_account_policy(
                &mut client,
                &activate_openkeys,
                &PolicyActiveExpectation::Unbound
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            postgres_active_account_policy(&mut client, &openkeys_v1.account_id)
                .unwrap()
                .expect("active OpenKeys policy"),
            ActiveAccountPolicy {
                policy: normalize_policy(&openkeys_v1),
                binding: openkeys_binding,
            }
        );

        let b2b_v4 = b2b_policy(4, 4, "b2b-policy-4");
        let b2b_v5 = b2b_policy(5, 5, "b2b-policy-5");
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &b2b_v4).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &b2b_v5).unwrap(),
            PricingMutation::Stored
        );
        let policy_race_barrier = Arc::new(Barrier::new(2));
        let expected_v3 = PolicyActiveExpectation::Exact(ActivePolicyTarget {
            target: b2b_v3.target(),
            binding: active_v3_binding.clone(),
        });
        let policy_racers = [b2b_v4, b2b_v5].map(|policy| {
            let url = url.clone();
            let barrier = Arc::clone(&policy_race_barrier);
            let expected = expected_v3.clone();
            let desired_binding = active_v3_binding.clone();
            std::thread::spawn(move || {
                let mut racer = connect_client(&url);
                let activation = activation(&policy, desired_binding);
                barrier.wait();
                postgres_activate_account_policy(&mut racer, &activation, &expected).unwrap()
            })
        });
        let policy_race_outcomes = policy_racers.map(|racer| racer.join().unwrap());
        assert_eq!(
            policy_race_outcomes
                .iter()
                .filter(|outcome| **outcome == PricingMutation::Applied)
                .count(),
            1
        );
        assert!(policy_race_outcomes.iter().any(|outcome| matches!(
            outcome,
            PricingMutation::Rejected(PricingRejection::PolicyCasMismatch { .. })
                | PricingMutation::Rejected(PricingRejection::Stale { .. })
        )));

        let switch_v4 = switches(4, "switches-4");
        assert_eq!(
            postgres_prepare_provider_switches(&mut client, &switch_v4).unwrap(),
            PricingMutation::Stored
        );
        let switch_race_barrier = Arc::new(Barrier::new(2));
        let switch_racers = [switch_v3.target(), switch_v4.target()].map(|target| {
            let url = url.clone();
            let barrier = Arc::clone(&switch_race_barrier);
            let expected = ActiveExpectation::Exact(switch_v1.target());
            std::thread::spawn(move || {
                let mut racer = connect_client(&url);
                barrier.wait();
                postgres_activate_provider_switches(&mut racer, &target, &expected).unwrap()
            })
        });
        let switch_race_outcomes = switch_racers.map(|racer| racer.join().unwrap());
        assert_eq!(
            switch_race_outcomes
                .iter()
                .filter(|outcome| **outcome == PricingMutation::Applied)
                .count(),
            1
        );
        assert!(switch_race_outcomes.iter().any(|outcome| matches!(
            outcome,
            PricingMutation::Rejected(PricingRejection::CasMismatch { .. })
                | PricingMutation::Rejected(PricingRejection::Stale { .. })
        )));
        let torn = postgres_pricing_read_bundle(&mut client, &b2b_v1.account_id).unwrap();
        let active_policy = match torn.policy {
            PricingPolicySnapshot::Active(active) => active,
            other => panic!("expected active PostgreSQL pricing policy, got {other:?}"),
        };
        assert_eq!(active_policy.policy.catalog_generation, 1);
        assert_eq!(active_policy.policy.switch_generation, 1);
        assert_eq!(torn.policy_catalog, Some(normalize_catalog(&main_catalog)));
        assert_eq!(torn.policy_switches, Some(normalize_switches(&switch_v1)));
        assert_eq!(
            torn.admission_catalog,
            Some(normalize_catalog(&main_catalog))
        );
        assert_ne!(
            torn.admission_switches
                .expect("current PostgreSQL switch head")
                .generation,
            active_policy.policy.switch_generation
        );

        client
            .batch_execute(
                "TRUNCATE
                     account_policy_bindings,
                     account_policy_rules,
                     account_policy_versions,
                     provider_switch_head,
                     provider_switch_entries,
                     provider_switch_versions,
                     pricing_catalog_heads,
                     pricing_catalog_entries,
                     pricing_catalog_versions
                 CASCADE;",
            )
            .expect("clean PostgreSQL pricing contract fixtures");
        client
            .execute(
                "DELETE FROM accounts WHERE id LIKE 'pricing-pg-contract-%'",
                &[],
            )
            .expect("clean PostgreSQL pricing contract accounts");

        run_postgres_dual_lineage_rollout_matrix(&mut client);

        client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .expect("unlock PostgreSQL pricing contract fixture");
    }

    #[test]
    fn postgres_pricing_release_v2_producer_matrix() {
        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!("skipping pricing release v2 producer matrix: test URL is unset");
            return;
        };
        let mut lock_holder = connect_client(&url);
        lock_holder
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        let mut migrator = PgStore::connect(&url).unwrap();
        migrator.migrate().unwrap();
        drop(migrator);
        let mut client = connect_client(&url);
        client
            .batch_execute(
                "TRUNCATE
                     pricing_release_policy_versions,
                     pricing_release_versions,
                     account_funding_generations_v2,
                     provider_switch_versions,
                     pricing_catalog_versions,
                     accounts
                 CASCADE",
            )
            .unwrap();

        let main = PricingCatalogSpec {
            product_id: "main".into(),
            generation: 1,
            schema_version: PRICING_SCHEMA_VERSION,
            capability_generation: 1,
            capability_digest: "release-v2-capability".into(),
            content_digest: "release-v2-main-catalog".into(),
            entries: vec![PricingCatalogEntrySpec {
                provider_id: "anthropic".into(),
                canonical_model_id: "claude-release-v2".into(),
                enabled: true,
            }],
        };
        let openkeys = PricingCatalogSpec {
            product_id: "openkeys".into(),
            content_digest: "release-v2-openkeys-catalog".into(),
            ..main.clone()
        };
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &main).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &openkeys).unwrap(),
            PricingMutation::Stored
        );
        let switches = ProviderSwitchSpec {
            generation: 1,
            schema_version: PRICING_SCHEMA_VERSION,
            capability_generation: 1,
            capability_digest: "release-v2-capability".into(),
            content_digest: "release-v2-switches".into(),
            entries: vec![
                ProviderSwitchEntrySpec {
                    provider_id: "anthropic".into(),
                    scope: ProviderSwitchScope::Master,
                    catalog_generation: None,
                    enabled: true,
                },
                ProviderSwitchEntrySpec {
                    provider_id: "anthropic".into(),
                    scope: ProviderSwitchScope::Product {
                        product_id: "main".into(),
                    },
                    catalog_generation: Some(1),
                    enabled: true,
                },
                ProviderSwitchEntrySpec {
                    provider_id: "anthropic".into(),
                    scope: ProviderSwitchScope::Product {
                        product_id: "openkeys".into(),
                    },
                    catalog_generation: Some(1),
                    enabled: true,
                },
            ],
        };
        assert_eq!(
            postgres_prepare_provider_switches(&mut client, &switches).unwrap(),
            PricingMutation::Stored
        );

        client
            .batch_execute(
                "BEGIN;
                 INSERT INTO accounts(
                     id,handle,balance_nano,reserved_nano,spent_nano,mult_bp,status,created_ts,created
                 ) VALUES
                     ('pricing-v2-producer-b2c','pricing-v2-producer-b2c',100,0,0,5000,
                      'disabled',100,'producer'),
                     ('pricing-v2-producer-service','pricing-v2-producer-service',0,0,0,10000,
                      'active',100,'producer');
                 INSERT INTO account_funding_generations_v2(
                     account_id,generation,schema_version,source_state_digest,
                     normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                     normalized_ts,updated_ts
                 ) VALUES(
                     'pricing-v2-producer-b2c',1,2,'source','normalization',100,0,0,0,100,100
                 );
                 INSERT INTO funding_lots_v2(
                     lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                     reserved_nano,spent_nano,version,status,created_ts,updated_ts
                 ) VALUES(
                     'pricing-v2-producer-paid','pricing-v2-producer-b2c',1,'paid','legacy',
                     100,0,0,0,'active',100,100
                 );
                 INSERT INTO account_funding_head_v2(
                     account_id,active_generation,head_version,updated_ts
                 ) VALUES('pricing-v2-producer-b2c',1,1,100);
                 COMMIT;",
            )
            .unwrap();

        let b2c_policy = crate::pricing::PricingReleasePolicyV2 {
            policy_id: "pricing-v2-producer-b2c".into(),
            policy_version: 1,
            owner_type: PolicyOwnerType::GlobalB2c,
            owner_id: "global".into(),
            account_class: AccountClass::B2c,
            product_id: Some("main".into()),
            billing_mode: crate::pricing::BillingModeV2::Balance,
            schema_version: 2,
            capability_generation: 1,
            capability_digest: "release-v2-capability".into(),
            catalog_generation: Some(1),
            catalog_digest: Some("release-v2-main-catalog".into()),
            switch_generation: Some(1),
            switch_digest: Some("release-v2-switches".into()),
            content_digest: "release-v2-b2c-policy".into(),
            rules: vec![crate::pricing::PricingReleasePolicyRuleV2 {
                rule_id: "global-50".into(),
                rule_digest: "global-50-digest".into(),
                scope: crate::pricing::PricingReleaseRuleScopeV2::Global,
                discount_bps: 5_000,
                payable_multiplier_bp: 5_000,
            }],
        };
        let service_policy = crate::pricing::PricingReleasePolicyV2 {
            policy_id: "pricing-v2-producer-service".into(),
            policy_version: 1,
            owner_type: PolicyOwnerType::Service,
            owner_id: "internal-domain".into(),
            account_class: AccountClass::Service,
            product_id: None,
            billing_mode: crate::pricing::BillingModeV2::MeterOnly,
            schema_version: 2,
            capability_generation: 1,
            capability_digest: "release-v2-capability".into(),
            catalog_generation: None,
            catalog_digest: None,
            switch_generation: None,
            switch_digest: None,
            content_digest: "release-v2-service-policy".into(),
            rules: Vec::new(),
        };
        for policy in [&b2c_policy, &service_policy] {
            assert_eq!(
                postgres_prepare_pricing_release_policy_v2(&mut client, policy).unwrap(),
                PricingMutation::Stored
            );
            assert_eq!(
                postgres_prepare_pricing_release_policy_v2(&mut client, policy).unwrap(),
                PricingMutation::Unchanged
            );
            assert_eq!(
                postgres_pricing_release_policy_v2(
                    &mut client,
                    &policy.policy_id,
                    policy.policy_version,
                )
                .unwrap(),
                Some(policy.clone())
            );
        }
        let mut malformed_policy = b2c_policy.clone();
        malformed_policy.policy_id = "pricing-v2-producer-malformed".into();
        malformed_policy.rules[0].discount_bps = 4_999;
        assert!(matches!(
            postgres_prepare_pricing_release_policy_v2(&mut client, &malformed_policy).unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));
        let mut newest_policy = b2c_policy.clone();
        newest_policy.policy_id = "pricing-v2-producer-monotonic".into();
        newest_policy.policy_version = 2;
        newest_policy.content_digest = "release-v2-policy-monotonic-2".into();
        assert_eq!(
            postgres_prepare_pricing_release_policy_v2(&mut client, &newest_policy).unwrap(),
            PricingMutation::Stored
        );
        let stale_policy = crate::pricing::PricingReleasePolicyV2 {
            policy_version: 1,
            content_digest: "release-v2-policy-monotonic-1".into(),
            ..newest_policy
        };
        assert!(matches!(
            postgres_prepare_pricing_release_policy_v2(&mut client, &stale_policy).unwrap(),
            PricingMutation::Rejected(PricingRejection::Stale { .. })
        ));

        let assignments = vec![
            crate::pricing::PricingReleaseAssignmentV2 {
                account_id: "pricing-v2-producer-b2c".into(),
                account_class: AccountClass::B2c,
                policy_id: b2c_policy.policy_id.clone(),
                policy_version: 1,
                policy_digest: b2c_policy.content_digest.clone(),
                billing_mode: crate::pricing::BillingModeV2::Balance,
                funding_generation: Some(1),
                purpose: None,
                responsible: None,
                assignment_digest: "release-v2-assignment-b2c".into(),
            },
            crate::pricing::PricingReleaseAssignmentV2 {
                account_id: "pricing-v2-producer-service".into(),
                account_class: AccountClass::Service,
                policy_id: service_policy.policy_id.clone(),
                policy_version: 1,
                policy_digest: service_policy.content_digest.clone(),
                billing_mode: crate::pricing::BillingModeV2::MeterOnly,
                funding_generation: None,
                purpose: Some("internal-domain".into()),
                responsible: Some("owner-team".into()),
                assignment_digest: "release-v2-assignment-service".into(),
            },
        ];
        let release = crate::pricing::PricingReleaseV2 {
            generation: 1,
            release_kind: crate::pricing::PricingReleaseKindV2::Target,
            schema_version: 2,
            capability_generation: 1,
            capability_digest: "release-v2-capability".into(),
            main_catalog_generation: 1,
            main_catalog_digest: "release-v2-main-catalog".into(),
            openkeys_catalog_generation: 1,
            openkeys_catalog_digest: "release-v2-openkeys-catalog".into(),
            switch_generation: 1,
            switch_digest: "release-v2-switches".into(),
            inventory_digest: "release-v2-inventory".into(),
            policy_manifest_digest: "release-v2-policy-manifest".into(),
            assignment_manifest_digest: "release-v2-assignment-manifest".into(),
            funding_manifest_digest: "release-v2-funding-manifest".into(),
            minimum_runtime_schema_version: 2,
            content_digest: "release-v2-target".into(),
            assignments: assignments.clone(),
        };
        assert_eq!(
            postgres_prepare_pricing_release_v2(&mut client, &release).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            postgres_prepare_pricing_release_v2(&mut client, &release).unwrap(),
            PricingMutation::Unchanged
        );
        assert_eq!(
            postgres_pricing_release_v2(&mut client, 1).unwrap(),
            Some(release.clone())
        );
        assert_eq!(postgres_pricing_release_head_v2(&mut client).unwrap(), None);

        let page = postgres_pricing_release_inventory_v2(&mut client, None, 1).unwrap();
        assert_eq!(page.accounts.len(), 1);
        assert_eq!(page.accounts[0].status, "disabled");
        assert!(page.next_after_account_id.is_some());
        let second = postgres_pricing_release_inventory_v2(
            &mut client,
            page.next_after_account_id.as_deref(),
            1,
        )
        .unwrap();
        assert_eq!(second.accounts.len(), 1);
        assert!(second.next_after_account_id.is_none());

        let recovery = crate::pricing::PricingReleaseV2 {
            generation: 2,
            release_kind: crate::pricing::PricingReleaseKindV2::Recovery,
            policy_manifest_digest: "release-v2-recovery-policy-manifest".into(),
            assignment_manifest_digest: "release-v2-recovery-assignment-manifest".into(),
            content_digest: "release-v2-recovery".into(),
            assignments,
            ..release.clone()
        };
        assert_eq!(
            postgres_prepare_pricing_release_v2(&mut client, &recovery).unwrap(),
            PricingMutation::Stored
        );
        let newer_target = crate::pricing::PricingReleaseV2 {
            generation: 4,
            release_kind: crate::pricing::PricingReleaseKindV2::Target,
            content_digest: "release-v2-newer-target".into(),
            ..recovery.clone()
        };
        assert_eq!(
            postgres_prepare_pricing_release_v2(&mut client, &newer_target).unwrap(),
            PricingMutation::Stored
        );
        let stale_release = crate::pricing::PricingReleaseV2 {
            generation: 3,
            content_digest: "release-v2-stale-target".into(),
            ..newer_target.clone()
        };
        assert!(matches!(
            postgres_prepare_pricing_release_v2(&mut client, &stale_release).unwrap(),
            PricingMutation::Rejected(PricingRejection::Stale { .. })
        ));
        let missing_recovery = crate::pricing::PricingReleaseRecoveryLinkV2 {
            target_generation: 1,
            target_digest: release.content_digest.clone(),
            recovery_generation: 5,
            recovery_digest: "release-v2-missing-recovery".into(),
            link_digest: "release-v2-missing-recovery-link".into(),
        };
        assert!(matches!(
            postgres_prepare_pricing_release_recovery_link_v2(&mut client, &missing_recovery)
                .unwrap(),
            PricingMutation::Rejected(PricingRejection::MissingDependency { .. })
        ));
        let wrong_recovery_kind = crate::pricing::PricingReleaseRecoveryLinkV2 {
            target_generation: 1,
            target_digest: release.content_digest.clone(),
            recovery_generation: 4,
            recovery_digest: newer_target.content_digest.clone(),
            link_digest: "release-v2-wrong-recovery-kind".into(),
        };
        assert!(matches!(
            postgres_prepare_pricing_release_recovery_link_v2(&mut client, &wrong_recovery_kind)
                .unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));
        let link = crate::pricing::PricingReleaseRecoveryLinkV2 {
            target_generation: 1,
            target_digest: release.content_digest.clone(),
            recovery_generation: 2,
            recovery_digest: recovery.content_digest.clone(),
            link_digest: "release-v2-recovery-link".into(),
        };
        assert_eq!(
            postgres_prepare_pricing_release_recovery_link_v2(&mut client, &link).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            postgres_prepare_pricing_release_recovery_link_v2(&mut client, &link).unwrap(),
            PricingMutation::Unchanged
        );
        assert_eq!(
            postgres_pricing_release_recovery_link_v2(&mut client, 1, 2).unwrap(),
            Some(link)
        );

        client
            .execute(
                "INSERT INTO accounts(id,mult_bp,status,created_ts,created)
                 VALUES('pricing-v2-producer-race',5000,'active',100,'producer')",
                &[],
            )
            .unwrap();
        let incomplete = crate::pricing::PricingReleaseV2 {
            generation: 5,
            release_kind: crate::pricing::PricingReleaseKindV2::Target,
            content_digest: "release-v2-incomplete".into(),
            ..release
        };
        assert!(matches!(
            postgres_prepare_pricing_release_v2(&mut client, &incomplete).unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));
        assert_eq!(postgres_pricing_release_head_v2(&mut client).unwrap(), None);

        client
            .batch_execute(
                "TRUNCATE
                     pricing_release_policy_versions,
                     pricing_release_versions,
                     account_funding_generations_v2,
                     provider_switch_versions,
                     pricing_catalog_versions,
                     accounts
                 CASCADE",
            )
            .unwrap();
        lock_holder
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }
}
