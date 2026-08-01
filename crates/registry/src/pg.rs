//! PostgreSQL authority for the engine.
//!
//! All correctness-sensitive mutations are transactions. Request IDs and lease IDs are the
//! idempotency boundary; owner epochs fence stale instances. PostgreSQL is the recovery floor.

use crate::{
    mask_proxy, AccountFundingSnapshot, AccountRow, AnthropicCalibrationRow,
    AnthropicWindowObservation, BillingTotals, CodexCalibrationRow, CodexHomeCalibrationSpend,
    CodexTurnCalibrationAggregate, CodexTurnCalibrationEvent, CodexWindowObservation,
    GeminiCalibrationRow, GeminiWindowObservation, KeyAuth, KeyPolicyUpdate, KeyRow,
    LedgerAttribution, LedgerConsumerLag, LedgerFundingAllocation, LedgerRow, PoolStateRow,
    ProviderCalibrationSubjectSpend, ProviderTurnCalibrationAggregate,
    ProviderTurnCalibrationEvent, SettlementFailure, SettlementHealth, SpendAccountAgg,
    SpendModelAgg, SpendProviderAgg, Sub, SubAdmin, SubHealth, SubRow, UsageDailyAgg,
    UsageDailyProviderAgg, UsageEventInput, UsageKeyAgg, UsageModelAgg, UsageReport,
};
use anyhow::{bail, Context, Result};
use postgres::config::{Host, SslMode};
use postgres::{Client, IsolationLevel, Row, Transaction};
use tokio_postgres_rustls::MakeRustlsConnect;

fn pg_provider_turn_event(row: &Row) -> ProviderTurnCalibrationEvent {
    ProviderTurnCalibrationEvent {
        provider: row.get(0),
        request_id: row.get(1),
        subject_id: row.get(2),
        model_id: row.get(3),
        service_tier: row.get(4),
        inference_geo: row.get(5),
        tariff_schedule_id: row.get(6),
        priced_ts: row.get(7),
        completed_at: row.get(8),
        input_tokens: row.get(9),
        audio_input_tokens: row.get(10),
        cache_read_tokens: row.get(11),
        cached_audio_input_tokens: row.get(12),
        cache_write_5m_tokens: row.get(13),
        cache_write_1h_tokens: row.get(14),
        output_tokens: row.get(15),
        thinking_output_tokens: row.get(16),
        image_output_tokens: row.get(17),
        tool_prompt_tokens: row.get(18),
        search_queries: row.get(19),
        grounded_search_prompts: row.get(20),
        api_input_nanousd: row.get(21),
        api_audio_input_nanousd: row.get(22),
        api_cache_read_nanousd: row.get(23),
        api_cached_audio_input_nanousd: row.get(24),
        api_cache_write_5m_nanousd: row.get(25),
        api_cache_write_1h_nanousd: row.get(26),
        api_output_nanousd: row.get(27),
        api_image_output_nanousd: row.get(28),
        api_search_nanousd: row.get(29),
        api_total_nanousd: row.get(30),
    }
}

fn pg_anthropic_calibration_row(row: &Row) -> AnthropicCalibrationRow {
    AnthropicCalibrationRow {
        subject_id: row.get(0),
        plan: row.get(1),
        window_kind: row.get(2),
        window_duration_mins: row.get(3),
        resets_at: row.get(4),
        anchor_used_fraction_units: row.get(5),
        anchor_resolution_fraction_units: row.get(6),
        anchor_spend_nano: row.get(7),
        used_fraction_units: row.get(8),
        measurement_resolution_fraction_units: row.get(9),
        observed_at: row.get(10),
        observed_fraction_units: row.get(11),
        observed_spend_nano: row.get(12),
        samples: row.get(13),
        unattributed_fraction_units: row.get(14),
        current_capacity_nano: row.get(15),
        current_low_nano: row.get(16),
        current_high_nano: row.get(17),
        current_confidence_bp: row.get(18),
        last_measured_at: row.get(19),
        estimator_version: row.get(20),
        version: row.get(21),
        updated_ts: row.get(22),
    }
}

// Keep the initial version placeholder explicitly typed. PostgreSQL otherwise resolves `$22 + 1`
// through the untyped integer literal as int4 before assignment to the bigint column, and the
// postgres crate correctly refuses to serialize the Rust i64 version into that inferred parameter.
const ANTHROPIC_CALIBRATION_INSERT_SQL: &str = "INSERT INTO anthropic_window_calibrations(\
       subject_id,plan,window_kind,window_duration_mins,resets_at,\
       anchor_used_fraction_units,anchor_resolution_fraction_units,anchor_spend_nano,\
       used_fraction_units,measurement_resolution_fraction_units,observed_at,\
       observed_fraction_units,observed_spend_nano,samples,unattributed_fraction_units,\
       current_capacity_nano,current_low_nano,current_high_nano,\
       current_confidence_bp,last_measured_at,estimator_version,version,updated_ts) \
     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,\
            $18,$19,$20,$21,($22::bigint)+1,$23) \
     ON CONFLICT(subject_id,plan,window_kind) DO NOTHING";

const MIGRATION_0001: &str = include_str!("../migrations_pg/0001_engine_authority.sql");
const MIGRATION_0002: &str = include_str!("../migrations_pg/0002_api_key_policies.sql");
const MIGRATION_0003: &str = include_str!("../migrations_pg/0003_subscription_auth_health.sql");
const MIGRATION_0004: &str = include_str!("../migrations_pg/0004_audit_hardening.sql");
const MIGRATION_0005: &str = include_str!("../migrations_pg/0005_provider_attribution.sql");
const MIGRATION_0006: &str = include_str!("../migrations_pg/0006_multi_discount_expand.sql");
const MIGRATION_0007: &str = include_str!("../migrations_pg/0007_multi_discount_runtime_pins.sql");
const MIGRATION_0008: &str = include_str!("../migrations_pg/0008_catalog_policy_lineage.sql");
const MIGRATION_0009: &str = include_str!("../migrations_pg/0009_pricing_shadow_admission.sql");
const MIGRATION_0010: &str = include_str!("../migrations_pg/0010_codex_window_calibration.sql");
const MIGRATION_0011: &str =
    include_str!("../migrations_pg/0011_codex_calibration_anchor_ready.sql");
const MIGRATION_0012: &str = include_str!("../migrations_pg/0012_codex_home_health.sql");
const MIGRATION_0013: &str = include_str!("../migrations_pg/0013_gemini_window_calibration.sql");
const MIGRATION_0014: &str = include_str!("../migrations_pg/0014_gemini_workload_calibration.sql");
const MIGRATION_0015: &str = include_str!("../migrations_pg/0015_codex_fractional_calibration.sql");
const MIGRATION_0016: &str =
    include_str!("../migrations_pg/0016_multi_discount_strict_enforcement.sql");
const MIGRATION_0017: &str = include_str!("../migrations_pg/0017_policy_runtime_floor.sql");
const MIGRATION_0018: &str = include_str!("../migrations_pg/0018_codex_credit_calibration.sql");
const MIGRATION_0019: &str = include_str!("../migrations_pg/0019_provider_turn_calibration.sql");
const MIGRATION_0020: &str = include_str!("../migrations_pg/0020_anthropic_window_calibration.sql");

/// Highest PostgreSQL schema version understood by this engine build.
pub const CURRENT_SCHEMA_VERSION: i64 = 20;
pub const DEFAULT_APPLICATION_NAME: &str = "claude-api-engine";

const ENGINE_MIGRATIONS: &[(i64, &str)] = &[
    (1, MIGRATION_0001),
    (2, MIGRATION_0002),
    (3, MIGRATION_0003),
    (4, MIGRATION_0004),
    (5, MIGRATION_0005),
    (6, MIGRATION_0006),
    (7, MIGRATION_0007),
    (8, MIGRATION_0008),
    (9, MIGRATION_0009),
    (10, MIGRATION_0010),
    (11, MIGRATION_0011),
    (12, MIGRATION_0012),
    (13, MIGRATION_0013),
    (14, MIGRATION_0014),
    (15, MIGRATION_0015),
    (16, MIGRATION_0016),
    (17, MIGRATION_0017),
    (18, MIGRATION_0018),
    (19, MIGRATION_0019),
    (20, MIGRATION_0020),
];

#[cfg(test)]
pub(crate) const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;

fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn chrono_like(ts: i64) -> String {
    crate::chrono_like(ts)
}

fn account_row(row: &Row) -> AccountRow {
    AccountRow {
        id: row.get(0),
        handle: row.get(1),
        balance_nano: row.get(2),
        spent_nano: row.get(3),
        reserved_nano: row.get(4),
        mult_bp: row.get(5),
        status: row.get(6),
    }
}

fn key_row(row: &Row) -> KeyRow {
    KeyRow {
        key: row.get(0),
        key_id: row.get(1),
        account_id: row.get(2),
        label: row.get(3),
        spent_nano: row.get(4),
        reserved_nano: row.get(5),
        spend_limit_nano: row.get(6),
        expires_ts: row.get(7),
        created_ts: row.get(8),
        last_used_ts: row.get(9),
        status: row.get(10),
    }
}

fn ledger_row(row: &Row) -> Result<LedgerRow> {
    let provider = row.get::<_, Option<String>>(9);
    let official_nano = row.get::<_, Option<i64>>(10);
    let attribution = row
        .get::<_, Option<i64>>(11)
        .map(|attribution_schema_version| {
            Ok::<LedgerAttribution, anyhow::Error>(LedgerAttribution {
                attribution_schema_version,
                snapshot_kind: row.get(12),
                provider_id: provider.clone(),
                product_id: row.get(13),
                account_class: row.get(14),
                requested_model_id: row.get(15),
                canonical_model_id: row.get(16),
                served_model_id: row.get(17),
                served_canonical_model_id: row.get(18),
                billing_invariant_code: row.get(19),
                alias_generation: row.get(20),
                rule_id: row.get(21),
                rule_digest: row.get(22),
                rule_scope: row.get(23),
                pricing_mode: row.get(24),
                rule_origin: row.get(25),
                discount_bps: row.get(26),
                payable_multiplier_bp: row.get(27),
                policy_id: row.get(28),
                policy_version: row.get(29),
                effective_policy_version: row.get(30),
                policy_digest: row.get(31),
                catalog_generation: row.get(32),
                switch_generation: row.get(33),
                tariff_schedule_id: row.get(34),
                tariff_priced_ts: row.get(35),
                official_nano,
                official_cost_json: crate::parse_ledger_json(row.get(36), "official_cost_json")?,
                paid_funded_nano: row.get(37),
                bonus_funded_nano: row.get(38),
                other_funded_nano: row.get(39),
                funding_allocation_json: crate::parse_ledger_json(
                    row.get(40),
                    "funding_allocation_json",
                )?,
                track_eligible: row.get(41),
                retention_eligible: row.get(42),
                commission_eligible: row.get(43),
                snapshot_digest: row.get(44),
                source_policy_digest: row.get(45),
                admission_catalog_generation: row.get(46),
                admission_catalog_digest: row.get(47),
                admission_switch_generation: row.get(48),
                admission_switch_digest: row.get(49),
                runtime_manifest_generation: row.get(50),
                runtime_manifest_digest: row.get(51),
            })
        })
        .transpose()?;
    Ok(LedgerRow {
        id: row.get(0),
        key: row.get(1),
        kind: row.get(2),
        request_id: row.get(3),
        amount_nano: row.get(4),
        reference: row.get(5),
        balance_after_nano: row.get(6),
        ts: row.get(7),
        model: row.get(8),
        provider,
        official_nano,
        attribution,
        funding_allocations: Vec::new(),
    })
}

const POSTGRES_LEDGER_READ_COLUMNS: &str = "
    ledger.id,ledger.key,ledger.kind,ledger.request_id,ledger.amount_nano,ledger.ref,
    ledger.balance_after_nano,ledger.ts,ledger.model,ledger.provider,ledger.official_nano,
    ledger.attribution_schema_version,ledger.snapshot_kind,ledger.product_id,
    ledger.account_class,ledger.requested_model_id,ledger.canonical_model_id,
    ledger.served_model_id,ledger.served_canonical_model_id,ledger.billing_invariant_code,
    ledger.alias_generation,ledger.rule_id,ledger.rule_digest,ledger.rule_scope,
    ledger.pricing_mode,ledger.rule_origin,ledger.discount_bps,ledger.payable_multiplier_bp,
    ledger.policy_id,ledger.policy_version,ledger.effective_policy_version,ledger.policy_digest,
    ledger.catalog_generation,ledger.switch_generation,ledger.tariff_schedule_id,
    ledger.tariff_priced_ts,ledger.official_cost_json::text,ledger.paid_funded_nano,
    ledger.bonus_funded_nano,ledger.other_funded_nano,ledger.funding_allocation_json::text,
    ledger.track_eligible,ledger.retention_eligible,ledger.commission_eligible,
    ledger.snapshot_digest,ledger.source_policy_digest,ledger.admission_catalog_generation,
    ledger.admission_catalog_digest,ledger.admission_switch_generation,
    ledger.admission_switch_digest,ledger.runtime_manifest_generation,
    ledger.runtime_manifest_digest";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owner {
    pub instance_id: String,
    pub epoch: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapacityLease {
    pub lease_id: String,
    pub request_id: String,
    pub subscription_email: String,
    pub lease_until: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    pub canceled_before_delivery: usize,
    pub charged_after_delivery: usize,
    pub processed_outbox: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaintenanceReport {
    pub usage_events: usize,
    pub outbox: usize,
    pub reservations: usize,
    pub pricing_snapshots_cascaded: usize,
    pub pricing_shadow_evaluations_cascaded: usize,
    pub capacity_leases: usize,
    pub engine_instances: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub subscriptions: usize,
    pub accounts: usize,
    pub keys: usize,
    pub ledger_rows: usize,
    pub usage_rows: usize,
    pub pool_rows: usize,
    pub balance_nano: i64,
    pub spent_nano: i64,
    pub reserved_nano: i64,
}

pub struct PgStore {
    client: Client,
}

/// Error class used by the async actor. Logical/invariant failures must never be retried forever;
/// transport and PostgreSQL concurrency failures may be retried within a bounded deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass {
    Transient,
    Fenced,
    Permanent,
}

pub fn classify_failure(error: &anyhow::Error) -> FailureClass {
    let message = format!("{error:#}").to_ascii_lowercase();
    if message.contains("owner lease is stale or fenced") || message.contains("owner was fenced") {
        return FailureClass::Fenced;
    }

    for cause in error.chain() {
        if let Some(pg) = cause.downcast_ref::<postgres::Error>() {
            let Some(db) = pg.as_db_error() else {
                // I/O, TLS and closed-connection failures have no server SQLSTATE.
                return FailureClass::Transient;
            };
            let code = db.code().code();
            if code.starts_with("08")
                || matches!(
                    code,
                    "40001" // serialization_failure
                        | "40P01" // deadlock_detected
                        | "55P03" // lock_not_available
                        | "57014" // query_canceled / statement timeout
                        | "57P01" // admin_shutdown
                        | "57P02" // crash_shutdown
                        | "57P03" // cannot_connect_now
                        | "53300" // too_many_connections
                )
            {
                return FailureClass::Transient;
            }
            return FailureClass::Permanent;
        }
    }
    FailureClass::Permanent
}

/// True only for PostgreSQL's server-side statement or lock timeout SQLSTATEs. Shadow workers use
/// this typed classification to distinguish an exhausted evaluation budget from other read/write
/// failures without parsing error text or retrying the operation.
pub fn is_statement_or_lock_timeout(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<postgres::Error>()
            .and_then(postgres::Error::as_db_error)
            .is_some_and(|db| matches!(db.code().code(), "55P03" | "57014"))
    })
}

fn postgres_policy_funding_evidence(
    tx: &mut Transaction<'_>,
    request_id: &str,
    hold_nano: i64,
    actual_nano: i64,
    lock_buckets: bool,
) -> Result<crate::PolicyFundingEvidence> {
    if actual_nano < 0 || actual_nano > hold_nano {
        bail!("strict settlement actual must be within the reserved hold");
    }
    let lock = if lock_buckets {
        " FOR UPDATE OF allocation,bucket"
    } else {
        ""
    };
    let sql = format!(
        "SELECT allocation.bucket_id,bucket.source_type,allocation.bucket_version,
                allocation.reserved_nano,allocation.allocation_order
           FROM reservation_funding_allocations allocation
           JOIN funding_buckets bucket
             ON bucket.bucket_id=allocation.bucket_id
            AND bucket.account_id=allocation.account_id
          WHERE allocation.request_id=$1
          ORDER BY allocation.allocation_order,allocation.bucket_id{lock}"
    );
    let rows: Vec<(String, String, i64, i64, i64)> = tx
        .query(&sql, &[&request_id])?
        .into_iter()
        .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4)))
        .collect();
    let reserved_total = rows.iter().try_fold(0_i64, |total, row| {
        total
            .checked_add(row.3)
            .context("strict funding reservation total overflow")
    })?;
    if rows.is_empty() || reserved_total != hold_nano {
        bail!("strict settlement funding allocations do not cover the hold");
    }

    let mut charge_remaining = actual_nano;
    let mut paid_funded_nano = 0_i64;
    let mut bonus_funded_nano = 0_i64;
    let mut other_funded_nano = 0_i64;
    let mut allocations = Vec::with_capacity(rows.len());
    for (bucket_id, source_type, bucket_version, reserved_nano, allocation_order) in rows {
        let charged_nano = charge_remaining.min(reserved_nano);
        let released_nano = reserved_nano - charged_nano;
        charge_remaining -= charged_nano;
        let category = match source_type.as_str() {
            "paid" => &mut paid_funded_nano,
            "welcome_track_bonus" => &mut bonus_funded_nano,
            _ => &mut other_funded_nano,
        };
        *category = category
            .checked_add(charged_nano)
            .context("strict funding charge category overflow")?;
        allocations.push(crate::PolicyFundingAllocationEvidence {
            bucket_id,
            source_type,
            bucket_version,
            reserved_nano,
            charged_nano,
            released_nano,
            allocation_order,
        });
    }
    if charge_remaining != 0 {
        bail!("strict settlement funding allocations do not cover the actual charge");
    }
    let allocation_json = serde_json::to_string(&allocations)?;
    Ok(crate::PolicyFundingEvidence {
        allocations,
        paid_funded_nano,
        bonus_funded_nano,
        other_funded_nano,
        allocation_json,
    })
}

fn postgres_write_policy_attribution(
    tx: &mut Transaction<'_>,
    table: &str,
    request_id: &str,
    snapshot: &crate::pricing::PolicyAdmissionSnapshot,
    usage: Option<&UsageEventInput>,
    disposition: &str,
    funding: &crate::PolicyFundingEvidence,
) -> Result<()> {
    let (predicate, priced_ts_assignment) = match table {
        "settlement_outbox" | "usage_events" => ("request_id=$1", "priced_ts=$25,"),
        "ledger" => ("kind='charge' AND request_id=$1", ""),
        _ => bail!("unsupported PostgreSQL policy attribution target"),
    };
    let (rule_scope, _, _) = snapshot.rule_scope.db_parts();
    let served_model = usage
        .map(|value| value.model.as_str())
        .or_else(|| (disposition == "reconcile_full_hold").then(|| snapshot.canonical_model_id()));
    let invariant = if disposition == "reconcile_full_hold" {
        Some("reconciled_full_hold_without_usage")
    } else {
        served_model
            .filter(|model| *model != snapshot.canonical_model_id())
            .map(|_| "served_canonical_model_mismatch")
    };
    let official_cost_json = crate::policy_official_cost_json(snapshot, usage, disposition)?;
    let sql = format!(
        "UPDATE {table} SET
             provider=$2,attribution_schema_version=$3,snapshot_kind='policy_v1',product_id=$4,
             account_class=$5,requested_model_id=$6,canonical_model_id=$7,served_model_id=$8,
             served_canonical_model_id=$8,billing_invariant_code=$9,alias_generation=$10,
             rule_id=$11,rule_digest=$12,rule_scope=$13,pricing_mode=$14,rule_origin=$15,
             discount_bps=$16,payable_multiplier_bp=$17,policy_id=$18,policy_version=$19,
             effective_policy_version=$20,policy_digest=$21,catalog_generation=$22,
             switch_generation=$23,tariff_schedule_id=$24,{priced_ts_assignment}
             tariff_priced_ts=$25,
             official_cost_json=$26::text::jsonb,paid_funded_nano=$27,bonus_funded_nano=$28,
             other_funded_nano=$29,funding_allocation_json=$30::text::jsonb,track_eligible=$31,
             retention_eligible=$32,commission_eligible=$33,snapshot_digest=$34,
             source_policy_digest=$35,admission_catalog_generation=$36,
             admission_catalog_digest=$37,admission_switch_generation=$38,
             admission_switch_digest=$39,runtime_manifest_generation=$40,
             runtime_manifest_digest=$41
           WHERE {predicate}"
    );
    if tx.execute(
        &sql,
        &[
            &request_id,
            &snapshot.provider().as_str(),
            &crate::pricing::POLICY_ADMISSION_SNAPSHOT_SCHEMA_VERSION,
            &snapshot.product_id(),
            &snapshot.account_class().as_str(),
            &snapshot.requested_model_id(),
            &snapshot.canonical_model_id(),
            &served_model,
            &invariant,
            &snapshot.alias_generation(),
            &snapshot.rule_id(),
            &snapshot.rule_digest(),
            &rule_scope,
            &snapshot.pricing_mode().as_str(),
            &snapshot.rule_origin.as_str(),
            &snapshot.discount_bps,
            &snapshot.payable_multiplier_bp(),
            &snapshot.policy_id(),
            &snapshot.policy_version(),
            &snapshot.effective_policy_version(),
            &snapshot.policy_digest(),
            &snapshot.policy_catalog_generation(),
            &snapshot.policy_switch_generation(),
            &snapshot.tariff_schedule_id(),
            &snapshot.tariff_priced_ts(),
            &official_cost_json,
            &funding.paid_funded_nano,
            &funding.bonus_funded_nano,
            &funding.other_funded_nano,
            &funding.allocation_json,
            &snapshot.track_eligible(),
            &snapshot.retention_eligible(),
            &snapshot.commission_eligible(),
            &snapshot.snapshot_digest(),
            &snapshot.source_policy_digest(),
            &snapshot.admission_catalog_generation(),
            &snapshot.admission_catalog_digest(),
            &snapshot.admission_switch_generation(),
            &snapshot.admission_switch_digest(),
            &snapshot.runtime_manifest_generation(),
            &snapshot.runtime_manifest_digest(),
        ],
    )? != 1
    {
        bail!("PostgreSQL policy attribution target row is missing");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn postgres_process_policy_settlement(
    tx: &mut Transaction<'_>,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    disposition: &str,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
    snapshot: &crate::pricing::PolicyAdmissionSnapshot,
    timestamp: i64,
) -> Result<i64> {
    crate::validate_policy_settlement(snapshot, hold_nano, actual_nano, usage, disposition)?;
    let funding = postgres_policy_funding_evidence(tx, request_id, hold_nano, actual_nano, true)?;
    let released_total = hold_nano - actual_nano;
    let balance: i64 = tx
        .query_one(
            "UPDATE accounts
            SET balance_nano=balance_nano+$1,spent_nano=spent_nano+$2,
                reserved_nano=reserved_nano-$3
          WHERE id=$4 AND reserved_nano >= $3
          RETURNING balance_nano",
            &[&released_total, &actual_nano, &hold_nano, &account_id],
        )
        .context("strict PostgreSQL reservation/account aggregate invariant failed")?
        .get(0);
    let key_updated = tx.execute(
        "UPDATE api_keys
            SET spent_nano=spent_nano+$1,reserved_nano=reserved_nano-$2
          WHERE key=$3 AND account_id=$4 AND reserved_nano >= $2",
        &[&actual_nano, &hold_nano, &key, &account_id],
    )?;
    if key_updated != 1 {
        let key_still_exists = tx
            .query_opt("SELECT 1 FROM api_keys WHERE key=$1", &[&key])?
            .is_some();
        if key_still_exists {
            bail!("strict PostgreSQL reservation/key aggregate invariant failed");
        }
    }

    for allocation in &funding.allocations {
        let next_version: i64 = tx
            .query_one(
                "UPDATE funding_buckets
                SET balance_nano=balance_nano+$1,reserved_nano=reserved_nano-$2,
                    spent_nano=spent_nano+$3,version=version+1,updated_ts=$4,
                    status=CASE
                      WHEN status='retired' THEN status
                      WHEN balance_nano+$1>0 THEN 'active'
                      ELSE 'exhausted'
                    END
              WHERE bucket_id=$5 AND account_id=$6 AND reserved_nano >= $2
              RETURNING version",
                &[
                    &allocation.released_nano,
                    &allocation.reserved_nano,
                    &allocation.charged_nano,
                    &timestamp,
                    &allocation.bucket_id,
                    &account_id,
                ],
            )
            .with_context(|| {
                format!(
                    "strict PostgreSQL funding bucket {} invariant failed",
                    allocation.bucket_id
                )
            })?
            .get(0);
        if next_version <= allocation.bucket_version {
            bail!("strict PostgreSQL funding bucket version did not advance");
        }
        if tx.execute(
            "UPDATE reservation_funding_allocations
                SET charged_nano=$1,released_nano=$2
              WHERE request_id=$3 AND account_id=$4 AND bucket_id=$5
                AND charged_nano IS NULL AND released_nano IS NULL",
            &[
                &allocation.charged_nano,
                &allocation.released_nano,
                &request_id,
                &account_id,
                &allocation.bucket_id,
            ],
        )? != 1
        {
            bail!("strict PostgreSQL funding allocation was already terminalized");
        }
    }

    if usage.is_some() || actual_nano > 0 {
        let model = usage
            .map(|value| value.model.as_str())
            .unwrap_or_else(|| snapshot.canonical_model_id());
        let provider = usage
            .map(|value| value.provider.as_str())
            .unwrap_or_else(|| snapshot.provider().as_str());
        let official_nano = usage
            .map(|value| value.real_nano)
            .unwrap_or_else(|| snapshot.official_hold_nano());
        let ledger_id: i64 = tx
            .query_one(
                "INSERT INTO ledger(
                 account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts,model,
                 provider,official_nano)
             VALUES($1,$2,'charge',$3,$4,$5,$6,$7,NULLIF($8,''),$9,$10)
             RETURNING id",
                &[
                    &account_id,
                    &key,
                    &request_id,
                    &actual_nano,
                    &reference,
                    &balance,
                    &timestamp,
                    &model,
                    &provider,
                    &official_nano,
                ],
            )?
            .get(0);
        postgres_write_policy_attribution(
            tx,
            "ledger",
            request_id,
            snapshot,
            usage,
            disposition,
            &funding,
        )?;
        for allocation in funding
            .allocations
            .iter()
            .filter(|allocation| allocation.charged_nano > 0)
        {
            tx.execute(
                "INSERT INTO ledger_funding_allocations(
                     ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
                     direction,amount_nano)
                 VALUES($1,$2,$3,$4,$5,'debit',$6)",
                &[
                    &ledger_id,
                    &account_id,
                    &allocation.bucket_id,
                    &allocation.source_type,
                    &allocation.bucket_version,
                    &allocation.charged_nano,
                ],
            )?;
        }
    }
    if let Some(usage) = usage {
        tx.execute(
            "INSERT INTO usage_events(
                 request_id,account_id,key,model,input_tokens,output_tokens,cache_read_tokens,
                 cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests,real_nano,
                 charge_nano,ref,ts,speed,inference_geo,input_nano,output_nano,cache_read_nano,
                 cache_write_5m_nano,cache_write_1h_nano,web_search_nano,priced_ts,provider)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,
                    $19,$20,$21,$22,$23,$24)",
            &[
                &request_id,
                &account_id,
                &key,
                &usage.model,
                &usage.input_tokens,
                &usage.output_tokens,
                &usage.cache_read_tokens,
                &usage.cache_write_5m_tokens,
                &usage.cache_write_1h_tokens,
                &usage.web_search_requests,
                &usage.real_nano,
                &actual_nano,
                &reference,
                &timestamp,
                &usage.speed,
                &usage.inference_geo,
                &usage.input_nano,
                &usage.output_nano,
                &usage.cache_read_nano,
                &usage.cache_write_5m_nano,
                &usage.cache_write_1h_nano,
                &usage.web_search_nano,
                &usage.priced_ts,
                &usage.provider,
            ],
        )?;
        postgres_write_policy_attribution(
            tx,
            "usage_events",
            request_id,
            snapshot,
            Some(usage),
            disposition,
            &funding,
        )?;
    }
    postgres_write_policy_attribution(
        tx,
        "settlement_outbox",
        request_id,
        snapshot,
        usage,
        disposition,
        &funding,
    )?;
    Ok(balance)
}

impl PgStore {
    pub fn connect(url: &str) -> Result<Self> {
        Self::connect_with_application_name(url, DEFAULT_APPLICATION_NAME)
    }

    pub fn connect_with_application_name(url: &str, application_name: &str) -> Result<Self> {
        let mut config: postgres::Config = url.parse().context("parse engine PostgreSQL URL")?;
        config.application_name(application_name);
        let remote_tcp = config.get_hosts().iter().any(|host| match host {
            Host::Tcp(host) => {
                host != "localhost"
                    && host
                        .parse::<std::net::IpAddr>()
                        .map_or(true, |ip| !ip.is_loopback())
            }
            #[cfg(unix)]
            Host::Unix(_) => false,
        });
        if remote_tcp && config.get_ssl_mode() != SslMode::Require {
            bail!("remote PostgreSQL requires sslmode=require");
        }
        // The forward transport statically links BoringSSL through wreq. Keep PostgreSQL on
        // rustls so a single engine binary never links two libraries that export OpenSSL's ABI.
        let (connector, _certificate_load_errors) = MakeRustlsConnect::with_native_certs()
            .map_err(|errors| anyhow::anyhow!("load native certificates: {errors:?}"))?;
        let mut client = config
            .connect(connector)
            .context("connect engine PostgreSQL")?;
        client
            .batch_execute(
                "SET statement_timeout = '15s'; SET lock_timeout = '5s'; \
                 SET idle_in_transaction_session_timeout = '15s'; SET synchronous_commit = on;",
            )
            .context("configure engine PostgreSQL session")?;
        Ok(Self { client })
    }

    fn apply_migration(&mut self, version: i64, sql: &str) -> Result<()> {
        let mut tx = self.client.transaction()?;
        tx.query_one("SELECT pg_advisory_xact_lock(836214912670::bigint)", &[])?;

        let migrations_table_exists: bool = tx
            .query_one(
                "SELECT to_regclass('public.engine_schema_migrations') IS NOT NULL",
                &[],
            )?
            .get(0);
        let already_applied = if migrations_table_exists {
            tx.query_opt(
                "SELECT 1 FROM engine_schema_migrations WHERE version=$1",
                &[&version],
            )?
            .is_some()
        } else {
            false
        };

        if !already_applied {
            tx.batch_execute(sql)
                .with_context(|| format!("apply engine PostgreSQL migration {version:04}"))?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Apply pending migrations explicitly. Each migration has its own transaction so a DDL
    /// lock acquired by one version cannot be held while a later version waits on another table.
    /// The advisory transaction lock still serializes concurrent migration runners.
    pub fn migrate(&mut self) -> Result<()> {
        for &(version, sql) in ENGINE_MIGRATIONS {
            self.apply_migration(version, sql)?;
        }
        Ok(())
    }

    pub fn schema_version(&mut self) -> Result<i64> {
        Ok(self
            .client
            .query_one(
                "SELECT COALESCE(MAX(version), 0)::bigint FROM engine_schema_migrations",
                &[],
            )?
            .get(0))
    }

    /// Verify the already-installed schema without issuing any DDL. Startup uses this guard;
    /// schema changes belong to the explicit `db migrate-engine` operation.
    pub fn verify_schema(&mut self) -> Result<()> {
        let migrations_table_exists: bool = self
            .client
            .query_one(
                "SELECT to_regclass('public.engine_schema_migrations') IS NOT NULL",
                &[],
            )?
            .get(0);
        if !migrations_table_exists {
            bail!(
                "engine PostgreSQL schema is missing; run `claude-api db migrate-engine` before starting the engine"
            );
        }

        let version = self.schema_version()?;
        if version < CURRENT_SCHEMA_VERSION {
            bail!(
                "engine PostgreSQL schema version {version} is older than required {CURRENT_SCHEMA_VERSION}; run `claude-api db migrate-engine`"
            );
        }
        Ok(())
    }

    pub fn claim_instance(&mut self, instance_id: &str, ttl_secs: i64) -> Result<Owner> {
        let ts = now();
        let epoch: i64 = self
            .client
            .query_one("SELECT nextval('engine_owner_epoch_seq')::bigint", &[])?
            .get(0);
        self.client.execute(
            "INSERT INTO engine_instances(instance_id, owner_epoch, lease_until, started_ts, updated_ts) \
             VALUES($1,$2,$3,$4,$4) ON CONFLICT(instance_id) DO UPDATE SET \
             owner_epoch=EXCLUDED.owner_epoch, lease_until=EXCLUDED.lease_until, \
             started_ts=EXCLUDED.started_ts, updated_ts=EXCLUDED.updated_ts",
            &[&instance_id, &epoch, &(ts + ttl_secs.max(1)), &ts],
        )?;
        Ok(Owner {
            instance_id: instance_id.to_owned(),
            epoch,
        })
    }

    fn strict_pricing_heads_supported(
        tx: &mut Transaction<'_>,
        manifest: &crate::pricing::PricingRuntimeManifestEvidence,
    ) -> Result<bool> {
        manifest.capabilities().iter().try_for_each(|capability| {
            if capability.pricing_schema_version() != crate::pricing::PRICING_SCHEMA_VERSION {
                bail!("runtime manifest contains an unsupported pricing schema version");
            }
            Ok(())
        })?;
        tx.query(
            "SELECT account_id
               FROM account_policy_bindings
              WHERE policy_enforcement='strict' OR funding_enforcement='strict'
              FOR SHARE",
            &[],
        )?;
        tx.query(
            "SELECT head.product_id
               FROM account_policy_bindings binding
               JOIN account_policy_versions policy
                 ON policy.account_id=binding.account_id
                AND policy.effective_version=binding.active_effective_version
               JOIN pricing_catalog_heads head ON head.product_id=policy.product_id
              WHERE binding.policy_enforcement='strict'
                 OR binding.funding_enforcement='strict'
              FOR SHARE OF head",
            &[],
        )?;
        tx.query(
            "SELECT singleton
               FROM provider_switch_head
              WHERE EXISTS(
                  SELECT 1 FROM account_policy_bindings
                   WHERE policy_enforcement='strict' OR funding_enforcement='strict'
              )
              FOR SHARE",
            &[],
        )?;
        let dependencies = tx.query(
            "SELECT DISTINCT pricing_schema_version,capability_generation,capability_digest
               FROM (
                   SELECT catalog.schema_version AS pricing_schema_version,
                          catalog.capability_generation,catalog.capability_digest
                     FROM account_policy_bindings binding
                     JOIN account_policy_versions policy
                       ON policy.account_id=binding.account_id
                      AND policy.effective_version=binding.active_effective_version
                     JOIN pricing_catalog_versions catalog
                       ON catalog.product_id=policy.product_id
                      AND catalog.generation=policy.catalog_generation
                    WHERE binding.policy_enforcement='strict'
                       OR binding.funding_enforcement='strict'
                   UNION
                   SELECT switches.schema_version,switches.capability_generation,
                          switches.capability_digest
                     FROM account_policy_bindings binding
                     JOIN account_policy_versions policy
                       ON policy.account_id=binding.account_id
                      AND policy.effective_version=binding.active_effective_version
                     JOIN provider_switch_versions switches
                       ON switches.generation=policy.switch_generation
                    WHERE binding.policy_enforcement='strict'
                       OR binding.funding_enforcement='strict'
                   UNION
                   SELECT catalog.schema_version,catalog.capability_generation,
                          catalog.capability_digest
                     FROM account_policy_bindings binding
                     JOIN account_policy_versions policy
                       ON policy.account_id=binding.account_id
                      AND policy.effective_version=binding.active_effective_version
                     JOIN pricing_catalog_heads head ON head.product_id=policy.product_id
                     JOIN pricing_catalog_versions catalog
                       ON catalog.product_id=head.product_id
                      AND catalog.generation=head.active_generation
                    WHERE binding.policy_enforcement='strict'
                       OR binding.funding_enforcement='strict'
                   UNION
                   SELECT switches.schema_version,switches.capability_generation,
                          switches.capability_digest
                     FROM provider_switch_head head
                     JOIN provider_switch_versions switches
                       ON switches.generation=head.active_generation
                    WHERE EXISTS(
                        SELECT 1 FROM account_policy_bindings
                         WHERE policy_enforcement='strict' OR funding_enforcement='strict'
                    )
               ) dependency",
            &[],
        )?;
        Ok(dependencies.into_iter().all(|row| {
            let schema_version: i64 = row.get(0);
            let capability_generation: i64 = row.get(1);
            let capability_digest: String = row.get(2);
            manifest.capabilities().iter().any(|capability| {
                capability.pricing_schema_version() == schema_version
                    && capability.capability_generation() == capability_generation
                    && capability.capability_digest() == capability_digest
            })
        }))
    }

    pub fn claim_instance_with_pricing_manifest(
        &mut self,
        instance_id: &str,
        ttl_secs: i64,
        manifest: &crate::pricing::PricingRuntimeManifestEvidence,
    ) -> Result<Owner> {
        if instance_id.trim().is_empty() {
            bail!("engine instance id must not be empty");
        }
        let ts = now();
        let mut tx = self.client.transaction()?;
        if !Self::strict_pricing_heads_supported(&mut tx, manifest)? {
            bail!("runtime pricing manifest does not support every active strict dependency");
        }
        let epoch: i64 = tx
            .query_one("SELECT nextval('engine_owner_epoch_seq')::bigint", &[])?
            .get(0);
        tx.execute(
            "INSERT INTO engine_instances(
                 instance_id,owner_epoch,lease_until,started_ts,updated_ts,pricing_schema_version,
                 pricing_runtime_manifest_generation,pricing_runtime_manifest_digest)
             VALUES($1,$2,$3,$4,$4,$5,$6,$7)
             ON CONFLICT(instance_id) DO UPDATE SET
                 owner_epoch=EXCLUDED.owner_epoch,lease_until=EXCLUDED.lease_until,
                 started_ts=EXCLUDED.started_ts,updated_ts=EXCLUDED.updated_ts,
                 pricing_schema_version=EXCLUDED.pricing_schema_version,
                 pricing_runtime_manifest_generation=EXCLUDED.pricing_runtime_manifest_generation,
                 pricing_runtime_manifest_digest=EXCLUDED.pricing_runtime_manifest_digest",
            &[
                &instance_id,
                &epoch,
                &ts.saturating_add(ttl_secs.max(1)),
                &ts,
                &crate::pricing::PRICING_SCHEMA_VERSION,
                &manifest.manifest_generation(),
                &manifest.manifest_digest(),
            ],
        )?;
        tx.commit()?;
        Ok(Owner {
            instance_id: instance_id.to_owned(),
            epoch,
        })
    }

    pub fn heartbeat_instance(&mut self, owner: &Owner, ttl_secs: i64) -> Result<bool> {
        let ts = now();
        Ok(self.client.execute(
            "UPDATE engine_instances SET lease_until=$3, updated_ts=$4 \
             WHERE instance_id=$1 AND owner_epoch=$2",
            &[
                &owner.instance_id,
                &owner.epoch,
                &(ts + ttl_secs.max(1)),
                &ts,
            ],
        )? == 1)
    }

    pub fn heartbeat_instance_with_pricing_manifest(
        &mut self,
        owner: &Owner,
        ttl_secs: i64,
        manifest: &crate::pricing::PricingRuntimeManifestEvidence,
    ) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        if !Self::strict_pricing_heads_supported(&mut tx, manifest)? {
            tx.rollback()?;
            return Ok(false);
        }
        let changed = tx.execute(
            "UPDATE engine_instances SET lease_until=$3,updated_ts=$4
              WHERE instance_id=$1 AND owner_epoch=$2
                AND pricing_schema_version=$5
                AND pricing_runtime_manifest_generation=$6
                AND pricing_runtime_manifest_digest=$7",
            &[
                &owner.instance_id,
                &owner.epoch,
                &ts.saturating_add(ttl_secs.max(1)),
                &ts,
                &crate::pricing::PRICING_SCHEMA_VERSION,
                &manifest.manifest_generation(),
                &manifest.manifest_digest(),
            ],
        )? == 1;
        tx.commit()?;
        Ok(changed)
    }

    fn assert_owner(tx: &mut Transaction<'_>, owner: &Owner, ts: i64) -> Result<()> {
        let valid = tx.query_opt(
            "SELECT 1 FROM engine_instances WHERE instance_id=$1 AND owner_epoch=$2 AND lease_until >= $3",
            &[&owner.instance_id, &owner.epoch, &ts],
        )?.is_some();
        if !valid {
            bail!("engine owner lease is stale or fenced");
        }
        Ok(())
    }

    /// Recheck the fence after any blocking lock acquisition and hold the owner row until commit.
    /// A concurrent `claim_instance` can then either win before this query (and fence us) or wait
    /// until this transaction has finished, but it cannot replace the epoch between the check and
    /// the money writes.
    fn assert_owner_locked(tx: &mut Transaction<'_>, owner: &Owner, ts: i64) -> Result<()> {
        let valid = tx
            .query_opt(
                "SELECT 1 FROM engine_instances
                  WHERE instance_id=$1 AND owner_epoch=$2 AND lease_until >= $3
                  FOR UPDATE",
                &[&owner.instance_id, &owner.epoch, &ts],
            )?
            .is_some();
        if !valid {
            bail!("engine owner lease is stale or fenced");
        }
        Ok(())
    }

    /// Atomically reserve money for one generated request ID. An exact retry is idempotent.
    pub fn reserve_request(
        &mut self,
        owner: &Owner,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold_nano: i64,
        lease_secs: i64,
    ) -> Result<Option<i64>> {
        let hold = hold_nano.max(0);
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&request_id],
        )?;
        if let Some(row) = tx.query_opt(
            "SELECT account_id, key, hold_nano, balance_after_reserve_nano, owner_instance, owner_epoch, state \
             FROM reservations WHERE request_id=$1",
            &[&request_id],
        )? {
            let exact = row.get::<_, String>(0) == account_id
                && row.get::<_, String>(1) == key
                && row.get::<_, i64>(2) == hold
                && row.get::<_, String>(4) == owner.instance_id
                && row.get::<_, i64>(5) == owner.epoch
                && row.get::<_, String>(6) == "reserved";
            if !exact {
                bail!("reservation request ID belongs to a different or completed operation");
            }
            let balance = row.get(3);
            tx.commit()?;
            return Ok(Some(balance));
        }
        // Овердрафт-буфер: funded-запрос НЕ роняем из-за гонки конкурентных резервов. Пускаем, пока
        // ПОСЛЕ-баланс не ниже пола −OVERDRAFT_NANO (`balance-hold >= -OVERDRAFT` ⇔ `balance >= hold-OVERDRAFT`).
        // Гейт атомарен на строке аккаунта → суммарный баланс НИКОГДА не уходит ниже −$1 даже под
        // конкуренцией (каждый успешный резерв гарантирует post_balance ≥ −$1; за полом любой h>0 отбит).
        // Стоимость: аккаунт может получить максимум $1 в долг (per-account, не per-request) — принятый
        // размен на «ноль ложных 402». Синхронно с `metering::OVERDRAFT_NANO`.
        // $1 per-account floor.
        const OVERDRAFT_NANO: i64 = 1_000_000_000;
        // Гейт `balance-hold >= -OVERDRAFT` пишем как `balance + OVERDRAFT >= hold`: вычитание двух
        // bind-параметров Postgres не типизирует, а сложение с bigint-колонкой выводит тип параметра.
        let Some(row) = tx.query_opt(
            "UPDATE accounts SET balance_nano=balance_nano-$1, reserved_nano=reserved_nano+$1 \
             WHERE id=$2 AND status='active' AND balance_nano + $3 >= $1 RETURNING balance_nano",
            &[&hold, &account_id, &OVERDRAFT_NANO],
        )?
        else {
            tx.rollback()?;
            return Ok(None);
        };
        let balance: i64 = row.get(0);
        let key_updated = tx.execute(
            "UPDATE api_keys SET reserved_nano=reserved_nano+$1 \
             WHERE key=$2 AND account_id=$3 AND status='active' \
               AND (expires_ts IS NULL OR expires_ts>floor(EXTRACT(EPOCH FROM clock_timestamp()))::bigint) \
               AND (spend_limit_nano IS NULL OR spent_nano+reserved_nano+$1<=spend_limit_nano)",
            &[&hold, &key, &account_id],
        )?;
        if key_updated != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO reservations(request_id,account_id,key,hold_nano,balance_after_reserve_nano, \
             owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,'reserved',$9,$9)",
            &[&request_id, &account_id, &key, &hold, &balance, &owner.instance_id,
              &owner.epoch, &(ts + lease_secs.max(1)), &ts],
        )?;
        tx.commit()?;
        Ok(Some(balance))
    }

    /// Atomically reserve the charged legacy hold and persist its immutable pricing identity.
    ///
    /// This method has no production caller in Stage 3B1c.1. The established `reserve_request`
    /// method remains unchanged for all live traffic; an existing reservation without a snapshot
    /// is never backfilled because that would invent atomic attribution after the money commit.
    pub fn reserve_request_with_legacy_snapshot(
        &mut self,
        owner: &Owner,
        key: &str,
        lease_secs: i64,
        snapshot: &crate::pricing::LegacyScalarAdmissionSnapshot,
    ) -> Result<crate::pricing::LegacyScalarReserveOutcome> {
        self.reserve_request_with_legacy_snapshot_guarded(owner, key, lease_secs, snapshot, || true)
    }

    /// Guarded async-handoff primitive. The caller-owned gate is evaluated only for a successful
    /// insert or exact replay, after every fallible write/fence check and immediately before
    /// commit. A rejected gate rolls back this attempt without compensating a committed reserve.
    pub fn reserve_request_with_legacy_snapshot_guarded(
        &mut self,
        owner: &Owner,
        key: &str,
        lease_secs: i64,
        snapshot: &crate::pricing::LegacyScalarAdmissionSnapshot,
        mut commit_gate: impl FnMut() -> bool,
    ) -> Result<crate::pricing::LegacyScalarReserveOutcome> {
        use crate::pricing::{
            LegacyScalarReserveConflict as Conflict, LegacyScalarReserveOutcome as Outcome,
            LegacyScalarReserveReceipt as Receipt, LegacyScalarSnapshotLookup as Lookup,
        };

        snapshot.validate()?;
        if key.trim().is_empty() || lease_secs <= 0 {
            bail!("invalid PostgreSQL legacy snapshot reservation parameters");
        }
        let window_conflict = |trusted_now_ts| -> Result<Option<Conflict>> {
            match snapshot.validate_idempotency_window_at(trusted_now_ts) {
                Ok(()) => Ok(None),
                Err(crate::pricing::LegacyScalarIdempotencyWindowError::Expired) => {
                    Ok(Some(Conflict::ExpiredIdempotencyWindow))
                }
                Err(crate::pricing::LegacyScalarIdempotencyWindowError::AdmissionFromFuture) => {
                    Ok(Some(Conflict::AdmissionTimestampInFuture))
                }
                Err(
                    crate::pricing::LegacyScalarIdempotencyWindowError::InvalidTrustedTimestamp,
                ) => bail!("trusted PostgreSQL reservation clock is invalid"),
            }
        };
        let preflight_ts = now();
        if let Some(conflict) = window_conflict(preflight_ts)? {
            return Ok(Outcome::Conflict(conflict));
        }
        let request_id = snapshot.request_id.as_str();
        let account_id = snapshot.account_id.as_str();
        let hold = snapshot.charged_hold_nano;
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, preflight_ts)?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&request_id],
        )?;
        let fence_ts = now();
        Self::assert_owner_locked(&mut tx, owner, fence_ts)?;
        if let Some(conflict) = window_conflict(fence_ts)? {
            tx.rollback()?;
            return Ok(Outcome::Conflict(conflict));
        }
        if let Some(row) = tx.query_opt(
            "SELECT account_id,key,hold_nano,balance_after_reserve_nano,owner_instance,
                    owner_epoch,state
               FROM reservations
              WHERE request_id=$1
              FOR UPDATE",
            &[&request_id],
        )? {
            let stored_account: String = row.get(0);
            let stored_key: String = row.get(1);
            let stored_hold: i64 = row.get(2);
            let balance: i64 = row.get(3);
            let stored_owner: String = row.get(4);
            let stored_epoch: i64 = row.get(5);
            let state: String = row.get(6);
            let outcome = if stored_account != account_id
                || stored_key != key
                || stored_hold != hold
                || stored_owner != owner.instance_id
                || stored_epoch != owner.epoch
            {
                Outcome::Conflict(Conflict::ReservationIdentity)
            } else if state != "reserved" && state != "delivering" {
                Outcome::Conflict(Conflict::TerminalReservation)
            } else {
                match crate::pricing::postgres::postgres_legacy_scalar_snapshot_lookup(
                    &mut tx, request_id,
                )? {
                    Lookup::Missing => {
                        Outcome::Conflict(Conflict::ExistingReservationWithoutSnapshot)
                    }
                    Lookup::NonLegacy => Outcome::Conflict(Conflict::ExistingNonLegacySnapshot),
                    Lookup::Legacy(stored) if stored.as_ref() == snapshot => {
                        Outcome::Unchanged(Receipt {
                            balance_after_reserve_nano: balance,
                            snapshot: *stored,
                        })
                    }
                    Lookup::Legacy(_) => Outcome::Conflict(Conflict::SnapshotPayload),
                }
            };
            if matches!(&outcome, Outcome::Unchanged(_)) {
                Self::assert_owner_locked(&mut tx, owner, now())?;
                if !commit_gate() {
                    tx.rollback()?;
                    return Ok(Outcome::AbortedBeforeCommit);
                }
            }
            tx.commit()?;
            return Ok(outcome);
        }

        let reservation_ts = now();
        Self::assert_owner_locked(&mut tx, owner, reservation_ts)?;
        if let Some(conflict) = window_conflict(reservation_ts)? {
            tx.rollback()?;
            return Ok(Outcome::Conflict(conflict));
        }
        const OVERDRAFT_NANO: i64 = 1_000_000_000;
        let Some(row) = tx.query_opt(
            "UPDATE accounts SET balance_nano=balance_nano-$1,reserved_nano=reserved_nano+$1
              WHERE id=$2 AND status='active' AND balance_nano+$3 >= $1
              RETURNING balance_nano",
            &[&hold, &account_id, &OVERDRAFT_NANO],
        )?
        else {
            tx.rollback()?;
            return Ok(Outcome::NotReserved);
        };
        let balance: i64 = row.get(0);
        let key_updated = tx.execute(
            "UPDATE api_keys SET reserved_nano=reserved_nano+$1
              WHERE key=$2 AND account_id=$3 AND status='active'
                AND (expires_ts IS NULL OR expires_ts>floor(EXTRACT(EPOCH FROM clock_timestamp()))::bigint)
                AND (spend_limit_nano IS NULL OR spent_nano+reserved_nano+$1<=spend_limit_nano)",
            &[&hold, &key, &account_id],
        )?;
        if key_updated != 1 {
            tx.rollback()?;
            return Ok(Outcome::NotReserved);
        }
        tx.execute(
            "INSERT INTO reservations(request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                 owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,'reserved',$9,$9)",
            &[
                &request_id,
                &account_id,
                &key,
                &hold,
                &balance,
                &owner.instance_id,
                &owner.epoch,
                &(reservation_ts.saturating_add(lease_secs)),
                &reservation_ts,
            ],
        )?;
        if let Err(error) =
            crate::pricing::postgres::postgres_insert_legacy_scalar_admission_snapshot(
                &mut tx, snapshot,
            )
        {
            let _ = tx.rollback();
            return Err(error);
        }
        Self::assert_owner_locked(&mut tx, owner, now())?;
        if !commit_gate() {
            tx.rollback()?;
            return Ok(Outcome::AbortedBeforeCommit);
        }
        tx.commit()?;
        Ok(Outcome::Inserted(Receipt {
            balance_after_reserve_nano: balance,
            snapshot: snapshot.clone(),
        }))
    }

    /// Atomically revalidate the complete strict policy/admission decision, reserve its exact
    /// eligible funding buckets and persist the immutable admission snapshot.
    pub fn reserve_request_with_policy_snapshot(
        &mut self,
        owner: &Owner,
        key: &str,
        lease_secs: i64,
        snapshot: &crate::pricing::PolicyAdmissionSnapshot,
    ) -> Result<crate::pricing::PolicyReserveOutcome> {
        self.reserve_request_with_policy_snapshot_guarded(owner, key, lease_secs, snapshot, || true)
    }

    /// Guarded strict reserve for the async writer. A lost caller cannot leave behind an
    /// unobserved committed hold: the final owner fence and caller gate run immediately before
    /// commit for both inserts and exact active replays.
    pub fn reserve_request_with_policy_snapshot_guarded(
        &mut self,
        owner: &Owner,
        key: &str,
        lease_secs: i64,
        snapshot: &crate::pricing::PolicyAdmissionSnapshot,
        mut commit_gate: impl FnMut() -> bool,
    ) -> Result<crate::pricing::PolicyReserveOutcome> {
        use crate::pricing::{
            PolicyReserveConflict as Conflict, PolicyReserveOutcome as Outcome,
            PolicyReserveReceipt as Receipt, PolicySnapshotLookup as Lookup,
        };

        snapshot.validate()?;
        if key.trim().is_empty() || lease_secs <= 0 {
            bail!("invalid PostgreSQL policy snapshot reservation parameters");
        }
        let window_conflict = |trusted_now_ts| -> Result<Option<Conflict>> {
            match snapshot.validate_idempotency_window_at(trusted_now_ts) {
                Ok(()) => Ok(None),
                Err(crate::pricing::LegacyScalarIdempotencyWindowError::Expired) => {
                    Ok(Some(Conflict::ExpiredIdempotencyWindow))
                }
                Err(crate::pricing::LegacyScalarIdempotencyWindowError::AdmissionFromFuture) => {
                    Ok(Some(Conflict::AdmissionTimestampInFuture))
                }
                Err(
                    crate::pricing::LegacyScalarIdempotencyWindowError::InvalidTrustedTimestamp,
                ) => bail!("trusted PostgreSQL reservation clock is invalid"),
            }
        };
        let preflight_ts = now();
        if let Some(conflict) = window_conflict(preflight_ts)? {
            return Ok(Outcome::Conflict(conflict));
        }

        let request_id = snapshot.request_id();
        let account_id = snapshot.account_id();
        let hold = snapshot.charged_hold_nano();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, preflight_ts)?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&request_id],
        )?;
        let fence_ts = now();
        Self::assert_owner_locked(&mut tx, owner, fence_ts)?;
        if let Some(conflict) = window_conflict(fence_ts)? {
            tx.rollback()?;
            return Ok(Outcome::Conflict(conflict));
        }

        if let Some(row) = tx.query_opt(
            "SELECT account_id,key,hold_nano,balance_after_reserve_nano,owner_instance,
                    owner_epoch,state
               FROM reservations
              WHERE request_id=$1
              FOR UPDATE",
            &[&request_id],
        )? {
            let balance: i64 = row.get(3);
            let outcome = if row.get::<_, String>(0) != account_id
                || row.get::<_, String>(1) != key
                || row.get::<_, i64>(2) != hold
                || row.get::<_, String>(4) != owner.instance_id
                || row.get::<_, i64>(5) != owner.epoch
            {
                Outcome::Conflict(Conflict::ReservationIdentity)
            } else if !matches!(row.get::<_, String>(6).as_str(), "reserved" | "delivering") {
                Outcome::Conflict(Conflict::TerminalReservation)
            } else {
                match crate::pricing::postgres::postgres_policy_snapshot_lookup(
                    &mut tx, request_id, true,
                )? {
                    Lookup::Missing => {
                        Outcome::Conflict(Conflict::ExistingReservationWithoutSnapshot)
                    }
                    Lookup::NonPolicy => Outcome::Conflict(Conflict::ExistingNonPolicySnapshot),
                    Lookup::Policy(stored) if stored.as_ref() == snapshot => {
                        Outcome::Unchanged(Receipt {
                            balance_after_reserve_nano: balance,
                            snapshot: *stored,
                        })
                    }
                    Lookup::Policy(_) => Outcome::Conflict(Conflict::SnapshotPayload),
                }
            };
            if matches!(&outcome, Outcome::Unchanged(_)) {
                Self::assert_owner_locked(&mut tx, owner, now())?;
                if !commit_gate() {
                    tx.rollback()?;
                    return Ok(Outcome::AbortedBeforeCommit);
                }
            }
            tx.commit()?;
            return Ok(outcome);
        }

        let reservation_ts = now();
        Self::assert_owner_locked(&mut tx, owner, reservation_ts)?;
        if let Some(conflict) = window_conflict(reservation_ts)? {
            tx.rollback()?;
            return Ok(Outcome::Conflict(conflict));
        }

        let (rule_scope, rule_provider, rule_model) = snapshot.rule_scope.db_parts();
        let (scoped_type, scoped_product, scoped_segment) = match snapshot.account_class {
            crate::pricing::AccountClass::B2c => ("segment", snapshot.product_id.as_str(), "b2c"),
            crate::pricing::AccountClass::B2b => ("segment", snapshot.product_id.as_str(), "b2b"),
            crate::pricing::AccountClass::OpenKeys | crate::pricing::AccountClass::Service => {
                ("product", snapshot.product_id.as_str(), "")
            }
        };
        let policy_state_matches: bool = tx
            .query_one(
                "SELECT EXISTS(
               SELECT 1
               FROM account_policy_bindings binding
               JOIN account_policy_versions policy
                 ON policy.account_id=binding.account_id
                AND policy.effective_version=binding.active_effective_version
               JOIN account_policy_rules rule
                 ON rule.account_id=policy.account_id
                AND rule.effective_version=policy.effective_version
               JOIN pricing_catalog_versions policy_catalog
                 ON policy_catalog.product_id=policy.product_id
                AND policy_catalog.generation=policy.catalog_generation
               JOIN pricing_catalog_entries policy_model
                 ON policy_model.product_id=policy_catalog.product_id
                AND policy_model.generation=policy_catalog.generation
               JOIN provider_switch_versions policy_switches
                 ON policy_switches.generation=policy.switch_generation
               JOIN provider_switch_entries policy_master
                 ON policy_master.generation=policy_switches.generation
               JOIN provider_switch_entries policy_scoped
                 ON policy_scoped.generation=policy_switches.generation
               JOIN pricing_catalog_heads catalog_head
                 ON catalog_head.product_id=policy.product_id
               JOIN pricing_catalog_versions catalog
                 ON catalog.product_id=catalog_head.product_id
                AND catalog.generation=catalog_head.active_generation
               JOIN pricing_catalog_entries admission_model
                 ON admission_model.product_id=catalog.product_id
                AND admission_model.generation=catalog.generation
               JOIN provider_switch_head switch_head ON switch_head.singleton=1
               JOIN provider_switch_versions switches
                 ON switches.generation=switch_head.active_generation
               JOIN provider_switch_entries admission_master
                 ON admission_master.generation=switches.generation
               JOIN provider_switch_entries admission_scoped
                 ON admission_scoped.generation=switches.generation
               JOIN engine_instances instance
                 ON instance.instance_id=$31 AND instance.owner_epoch=$32
              WHERE binding.account_id=$1
                AND binding.policy_enforcement='strict'
                AND binding.funding_enforcement='strict'
                AND binding.reconciliation_state='verified'
                AND policy.effective_version=$2
                AND policy.policy_id=$3
                AND policy.policy_version=$4
                AND policy.source_policy_digest=$5
                AND policy.content_digest=$6
                AND policy.product_id=$7
                AND policy.account_class=$8
                AND policy.catalog_generation=$9
                AND policy.switch_generation=$10
                AND catalog.generation=$11
                AND catalog.content_digest=$12
                AND switches.generation=$13
                AND switches.content_digest=$14
                AND rule.rule_id=$15
                AND rule.rule_digest=$16
                AND rule.scope_type=$17
                AND rule.provider_id=$18
                AND rule.canonical_model_id IS NOT DISTINCT FROM $19
                AND rule.pricing_mode=$20
                AND rule.rule_origin=$21
                AND rule.discount_bps IS NOT DISTINCT FROM $22
                AND rule.payable_multiplier_bp=$23
                AND rule.track_eligible=$24
                AND rule.retention_eligible=$25
                AND rule.commission_eligible=$26
                AND policy_model.provider_id=$18
                AND policy_model.canonical_model_id=$27
                AND policy_model.enabled
                AND admission_model.provider_id=$18
                AND admission_model.canonical_model_id=$27
                AND admission_model.enabled
                AND policy_master.provider_id=$18
                AND policy_master.scope_type='master'
                AND policy_master.product_id=''
                AND policy_master.segment=''
                AND policy_master.enabled
                AND policy_scoped.provider_id=$18
                AND policy_scoped.scope_type=$28
                AND policy_scoped.product_id=$29
                AND policy_scoped.segment=$30
                AND policy_scoped.catalog_generation=policy_catalog.generation
                AND policy_scoped.enabled
                AND admission_master.provider_id=$18
                AND admission_master.scope_type='master'
                AND admission_master.product_id=''
                AND admission_master.segment=''
                AND admission_master.enabled
                AND admission_scoped.provider_id=$18
                AND admission_scoped.scope_type=$28
                AND admission_scoped.product_id=$29
                AND admission_scoped.segment=$30
                AND admission_scoped.catalog_generation IN (
                    catalog.generation, policy_catalog.generation
                )
                AND admission_scoped.enabled
                AND instance.lease_until >= $33
                AND instance.pricing_schema_version=$34
                AND instance.pricing_runtime_manifest_generation=$35
                AND instance.pricing_runtime_manifest_digest=$36
             )",
                &[
                    &snapshot.account_id,
                    &snapshot.effective_policy_version,
                    &snapshot.policy_id,
                    &snapshot.policy_version,
                    &snapshot.source_policy_digest,
                    &snapshot.policy_digest,
                    &snapshot.product_id,
                    &snapshot.account_class.as_str(),
                    &snapshot.policy_catalog_generation,
                    &snapshot.policy_switch_generation,
                    &snapshot.admission_catalog_generation,
                    &snapshot.admission_catalog_digest,
                    &snapshot.admission_switch_generation,
                    &snapshot.admission_switch_digest,
                    &snapshot.rule_id,
                    &snapshot.rule_digest,
                    &rule_scope,
                    &rule_provider,
                    &rule_model,
                    &snapshot.pricing_mode.as_str(),
                    &snapshot.rule_origin.as_str(),
                    &snapshot.discount_bps,
                    &snapshot.payable_multiplier_bp,
                    &snapshot.track_eligible,
                    &snapshot.retention_eligible,
                    &snapshot.commission_eligible,
                    &snapshot.canonical_model_id,
                    &scoped_type,
                    &scoped_product,
                    &scoped_segment,
                    &owner.instance_id,
                    &owner.epoch,
                    &reservation_ts,
                    &crate::pricing::PRICING_SCHEMA_VERSION,
                    &snapshot.runtime_manifest_generation,
                    &snapshot.runtime_manifest_digest,
                ],
            )?
            .get(0);
        if !policy_state_matches {
            tx.rollback()?;
            return Ok(Outcome::Conflict(Conflict::PolicyStateChanged));
        }

        let eligibility = if snapshot.track_eligible() {
            "track"
        } else {
            "any"
        };
        let buckets: Vec<(String, i64, i64)> = tx
            .query(
                "SELECT bucket_id,version,balance_nano
               FROM funding_buckets
              WHERE account_id=$1 AND status='active' AND balance_nano>0
                AND (($2='track' AND eligibility IN ('track','any'))
                  OR ($2='any' AND eligibility='any'))
              ORDER BY CASE source_type
                         WHEN 'welcome_track_bonus' THEN 0
                         WHEN 'paid' THEN 1
                         ELSE 2
                       END, created_ts, bucket_id
              FOR UPDATE",
                &[&account_id, &eligibility],
            )?
            .into_iter()
            .map(|row| (row.get(0), row.get(1), row.get(2)))
            .collect();
        let eligible_total = buckets.iter().try_fold(0_i64, |total, (_, _, balance)| {
            total
                .checked_add(*balance)
                .context("eligible funding balance overflow")
        })?;
        if eligible_total < hold {
            tx.rollback()?;
            return Ok(Outcome::NotReserved);
        }

        let Some(row) = tx.query_opt(
            "UPDATE accounts
                SET balance_nano=balance_nano-$1,reserved_nano=reserved_nano+$1
              WHERE id=$2 AND status='active' AND balance_nano >= $1
              RETURNING balance_nano",
            &[&hold, &account_id],
        )?
        else {
            tx.rollback()?;
            return Ok(Outcome::NotReserved);
        };
        let balance: i64 = row.get(0);
        if tx.execute(
            "UPDATE api_keys SET reserved_nano=reserved_nano+$1
              WHERE key=$2 AND account_id=$3 AND status='active'
                AND (expires_ts IS NULL OR expires_ts>
                    floor(EXTRACT(EPOCH FROM clock_timestamp()))::bigint)
                AND (spend_limit_nano IS NULL OR
                    spent_nano+reserved_nano+$1<=spend_limit_nano)",
            &[&hold, &key, &account_id],
        )? != 1
        {
            tx.rollback()?;
            return Ok(Outcome::NotReserved);
        }
        tx.execute(
            "INSERT INTO reservations(request_id,account_id,key,hold_nano,
                 balance_after_reserve_nano,owner_instance,owner_epoch,lease_until,state,
                 created_ts,updated_ts)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,'reserved',$9,$9)",
            &[
                &request_id,
                &account_id,
                &key,
                &hold,
                &balance,
                &owner.instance_id,
                &owner.epoch,
                &reservation_ts.saturating_add(lease_secs),
                &reservation_ts,
            ],
        )?;
        crate::pricing::postgres::postgres_insert_policy_admission_snapshot(&mut tx, snapshot)?;

        let mut remaining = hold;
        let mut allocation_order = 1_i64;
        for (bucket_id, version, available) in buckets {
            if remaining == 0 {
                break;
            }
            let reserved = remaining.min(available);
            let next_version = version
                .checked_add(1)
                .context("funding bucket version overflow")?;
            if tx.execute(
                "UPDATE funding_buckets
                    SET balance_nano=balance_nano-$1,reserved_nano=reserved_nano+$1,
                        version=$2,updated_ts=$3,
                        status=CASE WHEN balance_nano-$1=0 THEN 'exhausted' ELSE 'active' END
                  WHERE bucket_id=$4 AND account_id=$5 AND version=$6 AND balance_nano >= $1",
                &[
                    &reserved,
                    &next_version,
                    &reservation_ts,
                    &bucket_id,
                    &account_id,
                    &version,
                ],
            )? != 1
            {
                bail!("strict funding bucket changed during PostgreSQL reserve");
            }
            tx.execute(
                "INSERT INTO reservation_funding_allocations(
                     request_id,account_id,bucket_id,bucket_version,reserved_nano,allocation_order)
                 VALUES($1,$2,$3,$4,$5,$6)",
                &[
                    &request_id,
                    &account_id,
                    &bucket_id,
                    &next_version,
                    &reserved,
                    &allocation_order,
                ],
            )?;
            remaining -= reserved;
            allocation_order += 1;
        }
        if remaining != 0 {
            bail!("strict funding allocation did not cover the reserved hold");
        }
        Self::assert_owner_locked(&mut tx, owner, now())?;
        if !commit_gate() {
            tx.rollback()?;
            return Ok(Outcome::AbortedBeforeCommit);
        }
        tx.commit()?;
        Ok(Outcome::Inserted(Receipt {
            balance_after_reserve_nano: balance,
            snapshot: snapshot.clone(),
        }))
    }

    pub fn policy_admission_snapshot(
        &mut self,
        request_id: &str,
    ) -> Result<Option<crate::pricing::PolicyAdmissionSnapshot>> {
        use crate::pricing::PolicySnapshotLookup as Lookup;

        match crate::pricing::postgres::postgres_policy_snapshot_lookup(
            &mut self.client,
            request_id,
            false,
        )? {
            Lookup::Missing => Ok(None),
            Lookup::Policy(snapshot) => Ok(Some(*snapshot)),
            Lookup::NonPolicy => bail!("pricing admission snapshot is not a policy snapshot"),
        }
    }

    pub fn legacy_scalar_admission_snapshot(
        &mut self,
        request_id: &str,
    ) -> Result<Option<crate::pricing::LegacyScalarAdmissionSnapshot>> {
        use crate::pricing::LegacyScalarSnapshotLookup as Lookup;

        match crate::pricing::postgres::postgres_legacy_scalar_snapshot_lookup(
            &mut self.client,
            request_id,
        )? {
            Lookup::Missing => Ok(None),
            Lookup::Legacy(snapshot) => Ok(Some(*snapshot)),
            Lookup::NonLegacy => {
                bail!("pricing admission snapshot is not a legacy scalar snapshot")
            }
        }
    }

    /// Mark that a successful upstream response is about to be delivered. Recovery charges the hold
    /// for an expired `delivering` reservation rather than making delivered provider usage free.
    pub fn mark_delivering(
        &mut self,
        owner: &Owner,
        request_id: &str,
        lease_secs: i64,
    ) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        let changed = tx.execute(
            "UPDATE reservations SET state='delivering', lease_until=$4, updated_ts=$3 \
             WHERE request_id=$1 AND owner_instance=$2 AND owner_epoch=$5 AND state='reserved'",
            &[
                &request_id,
                &owner.instance_id,
                &ts,
                &(ts + lease_secs.max(1)),
                &owner.epoch,
            ],
        )?;
        let ok = changed == 1 || tx.query_opt(
            "SELECT 1 FROM reservations WHERE request_id=$1 AND owner_instance=$2 AND owner_epoch=$3 \
             AND state IN ('delivering','settlement_pending','settled')",
            &[&request_id, &owner.instance_id, &owner.epoch],
        )?.is_some();
        tx.commit()?;
        Ok(ok)
    }

    /// Renew both durable request and capacity leases for a live response stream.
    pub fn renew_stream_leases(
        &mut self,
        owner: &Owner,
        request_id: Option<&str>,
        capacity_lease_id: Option<&str>,
        lease_secs: i64,
    ) -> Result<bool> {
        let ts = now();
        let lease_until = ts.saturating_add(lease_secs.max(1));
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        let mut valid = true;
        if let Some(request_id) = request_id {
            valid &= tx.execute(
                "UPDATE reservations SET lease_until=$4,updated_ts=$3 \
                 WHERE request_id=$1 AND owner_instance=$2 AND owner_epoch=$5 \
                   AND state IN ('reserved','delivering','settlement_pending')",
                &[
                    &request_id,
                    &owner.instance_id,
                    &ts,
                    &lease_until,
                    &owner.epoch,
                ],
            )? == 1;
        }
        if let Some(lease_id) = capacity_lease_id {
            valid &= tx.execute(
                "UPDATE capacity_leases SET lease_until=$4 \
                 WHERE lease_id=$1 AND owner_instance=$2 AND owner_epoch=$3 AND state='active'",
                &[&lease_id, &owner.instance_id, &owner.epoch, &lease_until],
            )? == 1;
        }
        tx.commit()?;
        Ok(valid)
    }

    fn enqueue_outbox(
        &mut self,
        request_id: &str,
        actual_nano: i64,
        disposition: &str,
        reference: Option<&str>,
        usage: Option<&UsageEventInput>,
    ) -> Result<()> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&request_id],
        )?;
        let reservation = tx
            .query_opt(
                "SELECT hold_nano, state FROM reservations WHERE request_id=$1 FOR UPDATE",
                &[&request_id],
            )?
            .context("settlement reservation does not exist")?;
        let hold: i64 = reservation.get(0);
        let state: String = reservation.get(1);
        let policy_snapshot = match crate::pricing::postgres::postgres_policy_snapshot_lookup(
            &mut tx, request_id, true,
        )? {
            crate::pricing::PolicySnapshotLookup::Policy(snapshot) => Some(*snapshot),
            crate::pricing::PolicySnapshotLookup::Missing
            | crate::pricing::PolicySnapshotLookup::NonPolicy => None,
        };
        let actual = if let Some(snapshot) = policy_snapshot.as_ref() {
            crate::validate_policy_settlement(snapshot, hold, actual_nano, usage, disposition)?;
            actual_nano
        } else {
            actual_nano.max(0)
        };
        let u = usage.cloned().unwrap_or_default();
        let inserted = tx.execute(
            "INSERT INTO settlement_outbox(request_id,actual_nano,disposition,reference,model,input_tokens, \
             output_tokens,cache_read_tokens,cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests, \
             real_nano,speed,inference_geo,input_nano,output_nano,cache_read_nano,cache_write_5m_nano, \
             cache_write_1h_nano,web_search_nano,priced_ts,provider,state,created_ts,updated_ts) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22, \
                    'pending',$23,$23) \
             ON CONFLICT(request_id) DO NOTHING",
            &[&request_id, &actual, &disposition, &reference, &u.model, &u.input_tokens,
              &u.output_tokens, &u.cache_read_tokens, &u.cache_write_5m_tokens,
              &u.cache_write_1h_tokens, &u.web_search_requests, &u.real_nano, &u.speed,
              &u.inference_geo, &u.input_nano, &u.output_nano, &u.cache_read_nano,
              &u.cache_write_5m_nano, &u.cache_write_1h_nano, &u.web_search_nano, &u.priced_ts,
              &u.provider, &ts],
        )?;
        if inserted == 0 {
            let row = tx.query_one(
                "SELECT actual_nano,disposition,reference,model,input_tokens,output_tokens,cache_read_tokens, \
                 cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests,real_nano,speed, \
                 inference_geo,input_nano,output_nano,cache_read_nano,cache_write_5m_nano, \
                 cache_write_1h_nano,web_search_nano,priced_ts \
                 FROM settlement_outbox WHERE request_id=$1",
                &[&request_id],
            )?;
            let exact = row.get::<_, i64>(0) == actual
                && row.get::<_, String>(1) == disposition
                && row.get::<_, Option<String>>(2).as_deref() == reference
                && row.get::<_, String>(3) == u.model
                && row.get::<_, i64>(4) == u.input_tokens
                && row.get::<_, i64>(5) == u.output_tokens
                && row.get::<_, i64>(6) == u.cache_read_tokens
                && row.get::<_, i64>(7) == u.cache_write_5m_tokens
                && row.get::<_, i64>(8) == u.cache_write_1h_tokens
                && row.get::<_, i64>(9) == u.web_search_requests
                && row.get::<_, i64>(10) == u.real_nano
                && row.get::<_, String>(11) == u.speed
                && row.get::<_, String>(12) == u.inference_geo
                && row.get::<_, i64>(13) == u.input_nano
                && row.get::<_, i64>(14) == u.output_nano
                && row.get::<_, i64>(15) == u.cache_read_nano
                && row.get::<_, i64>(16) == u.cache_write_5m_nano
                && row.get::<_, i64>(17) == u.cache_write_1h_nano
                && row.get::<_, i64>(18) == u.web_search_nano
                && row.get::<_, i64>(19) == u.priced_ts;
            if !exact {
                bail!("settlement request ID conflicts with different outbox payload");
            }
        }
        if let Some(snapshot) = policy_snapshot.as_ref() {
            let funding =
                postgres_policy_funding_evidence(&mut tx, request_id, hold, actual, false)?;
            postgres_write_policy_attribution(
                &mut tx,
                "settlement_outbox",
                request_id,
                snapshot,
                usage,
                disposition,
                &funding,
            )?;
        }
        if !matches!(state.as_str(), "settled" | "canceled") {
            tx.execute(
                "UPDATE reservations SET state='settlement_pending', actual_nano=$2, updated_ts=$3 \
                 WHERE request_id=$1",
                &[&request_id, &actual, &ts],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn enqueue_settlement(
        &mut self,
        request_id: &str,
        actual_nano: i64,
        reference: Option<&str>,
        usage: Option<&UsageEventInput>,
    ) -> Result<()> {
        self.enqueue_outbox(request_id, actual_nano, "settle", reference, usage)
    }

    pub fn enqueue_cancel(&mut self, request_id: &str) -> Result<()> {
        self.enqueue_outbox(request_id, 0, "cancel", None, None)
    }

    fn process_outbox_request(&mut self, request_id: &str) -> Result<Option<i64>> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&request_id],
        )?;
        let Some(row) = tx.query_opt(
            "SELECT o.actual_nano,o.disposition,o.reference,o.model,o.input_tokens,o.output_tokens, \
             o.cache_read_tokens,o.cache_write_5m_tokens,o.cache_write_1h_tokens,o.web_search_requests, \
             o.real_nano,o.speed,o.inference_geo,o.input_nano,o.output_nano,o.cache_read_nano, \
             o.cache_write_5m_nano,o.cache_write_1h_nano,o.web_search_nano,o.priced_ts,o.provider, \
             o.state,r.account_id,r.key,r.hold_nano,r.state \
             FROM settlement_outbox o JOIN reservations r USING(request_id) \
             WHERE o.request_id=$1 FOR UPDATE OF o,r",
            &[&request_id],
        )? else {
            tx.rollback()?;
            return Ok(None);
        };
        let provider: String = row.get(20);
        let outbox_state: String = row.get(21);
        let reservation_state: String = row.get(25);
        let account_id: String = row.get(22);
        if outbox_state == "done" || matches!(reservation_state.as_str(), "settled" | "canceled") {
            let balance = tx
                .query_opt(
                    "SELECT balance_nano FROM accounts WHERE id=$1",
                    &[&account_id],
                )?
                .map(|r| r.get(0));
            tx.execute(
                "UPDATE settlement_outbox SET state='done', committed_ts=COALESCE(committed_ts,$2),updated_ts=$2 \
                 WHERE request_id=$1",
                &[&request_id, &ts],
            )?;
            tx.commit()?;
            return Ok(balance);
        }
        let actual: i64 = row.get(0);
        let disposition: String = row.get(1);
        let reference: Option<String> = row.get(2);
        let model: String = row.get(3);
        let account_key: String = row.get(23);
        let hold: i64 = row.get(24);
        let policy_snapshot = match crate::pricing::postgres::postgres_policy_snapshot_lookup(
            &mut tx, request_id, true,
        )? {
            crate::pricing::PolicySnapshotLookup::Policy(snapshot) => Some(*snapshot),
            crate::pricing::PolicySnapshotLookup::Missing
            | crate::pricing::PolicySnapshotLookup::NonPolicy => None,
        };
        let policy_usage =
            (policy_snapshot.is_some() && disposition == "settle").then(|| UsageEventInput {
                model: model.clone(),
                provider: provider.clone(),
                input_tokens: row.get(4),
                output_tokens: row.get(5),
                cache_read_tokens: row.get(6),
                cache_write_5m_tokens: row.get(7),
                cache_write_1h_tokens: row.get(8),
                web_search_requests: row.get(9),
                real_nano: row.get(10),
                speed: row.get(11),
                inference_geo: row.get(12),
                input_nano: row.get(13),
                output_nano: row.get(14),
                cache_read_nano: row.get(15),
                cache_write_5m_nano: row.get(16),
                cache_write_1h_nano: row.get(17),
                web_search_nano: row.get(18),
                priced_ts: row.get(19),
            });
        let balance: i64;
        if let Some(snapshot) = policy_snapshot.as_ref() {
            balance = postgres_process_policy_settlement(
                &mut tx,
                request_id,
                &account_id,
                &account_key,
                hold,
                actual,
                &disposition,
                reference.as_deref(),
                policy_usage.as_ref(),
                snapshot,
                ts,
            )?;
        } else {
            balance = tx.query_one(
            "UPDATE accounts SET balance_nano=balance_nano+$1-$2, spent_nano=spent_nano+$2, \
             reserved_nano=reserved_nano-$1 WHERE id=$3 AND reserved_nano >= $1 RETURNING balance_nano",
            &[&hold, &actual, &account_id],
        ).context("reservation/account aggregate invariant failed")?.get(0);
            let key_updated = tx.execute(
            "UPDATE api_keys SET spent_nano=spent_nano+$1, \
             reserved_nano=CASE WHEN reserved_nano >= $2 THEN reserved_nano-$2 ELSE reserved_nano END \
             WHERE key=$3 AND (reserved_nano >= $2 OR spend_limit_nano IS NULL)",
            &[&actual, &hold, &account_key],
        )?;
            if key_updated != 1 {
                let key_still_exists = tx
                    .query_opt("SELECT 1 FROM api_keys WHERE key=$1", &[&account_key])?
                    .is_some();
                if key_still_exists {
                    bail!("reservation/key aggregate invariant failed");
                }
            }
            if actual > 0 {
                tx.execute(
                "INSERT INTO ledger(account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts,model) \
                 VALUES($1,$2,'charge',$3,$4,$5,$6,$7,NULLIF($8,'')) ON CONFLICT DO NOTHING",
                &[&account_id, &account_key, &request_id, &actual, &reference, &balance, &ts, &model],
            )?;
                if !model.is_empty() {
                    let input_tokens: i64 = row.get(4);
                    let output_tokens: i64 = row.get(5);
                    let cache_read_tokens: i64 = row.get(6);
                    let cache_write_5m_tokens: i64 = row.get(7);
                    let cache_write_1h_tokens: i64 = row.get(8);
                    let web_search_requests: i64 = row.get(9);
                    let real_nano: i64 = row.get(10);
                    let speed: String = row.get(11);
                    let inference_geo: String = row.get(12);
                    let input_nano: i64 = row.get(13);
                    let output_nano: i64 = row.get(14);
                    let cache_read_nano: i64 = row.get(15);
                    let cache_write_5m_nano: i64 = row.get(16);
                    let cache_write_1h_nano: i64 = row.get(17);
                    let web_search_nano: i64 = row.get(18);
                    let priced_ts: i64 = row.get(19);
                    tx.execute(
                    "INSERT INTO usage_events(request_id,account_id,key,model,input_tokens,output_tokens, \
                     cache_read_tokens,cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests, \
                     real_nano,charge_nano,ref,ts,speed,inference_geo,input_nano,output_nano,cache_read_nano, \
                     cache_write_5m_nano,cache_write_1h_nano,web_search_nano,priced_ts,provider) \
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24) \
                     ON CONFLICT(request_id) DO NOTHING",
                    &[&request_id,&account_id,&account_key,&model,&input_tokens,&output_tokens,
                      &cache_read_tokens,&cache_write_5m_tokens,&cache_write_1h_tokens,
                      &web_search_requests,&real_nano,&actual,&reference,&ts,&speed,&inference_geo,
                      &input_nano,&output_nano,&cache_read_nano,&cache_write_5m_nano,
                      &cache_write_1h_nano,&web_search_nano,&priced_ts,&provider],
                )?;
                }
            }
        }
        let final_state = if disposition == "cancel" {
            "canceled"
        } else {
            "settled"
        };
        tx.execute(
            "UPDATE reservations SET state=$2,actual_nano=$3,settled_ts=$4,updated_ts=$4 WHERE request_id=$1",
            &[&request_id, &final_state, &actual, &ts],
        )?;
        tx.execute(
            "UPDATE settlement_outbox SET state='done',attempts=attempts+1,committed_ts=$2,updated_ts=$2, \
             last_error=NULL WHERE request_id=$1",
            &[&request_id, &ts],
        )?;
        tx.commit()?;
        Ok(Some(balance))
    }

    pub fn settle_request(
        &mut self,
        request_id: &str,
        actual_nano: i64,
        reference: Option<&str>,
        usage: Option<&UsageEventInput>,
    ) -> Result<Option<i64>> {
        self.enqueue_settlement(request_id, actual_nano, reference, usage)?;
        self.process_outbox_request(request_id)
    }

    pub fn cancel_request(&mut self, request_id: &str) -> Result<Option<i64>> {
        self.enqueue_cancel(request_id)?;
        self.process_outbox_request(request_id)
    }

    pub fn drain_outbox(&mut self, limit: usize) -> Result<usize> {
        let ids: Vec<String> = self.client.query(
            "SELECT request_id FROM settlement_outbox WHERE state='pending' AND next_attempt_ts <= $1 \
             ORDER BY created_ts LIMIT $2",
            &[&now(), &(limit.clamp(1, 10_000) as i64)],
        )?.into_iter().map(|r| r.get(0)).collect();
        let mut done = 0;
        for id in ids {
            match self.process_outbox_request(&id) {
                Ok(_) => done += 1,
                Err(err) => {
                    let ts = now();
                    let message = format!("{err:#}");
                    let state = if classify_failure(&err) == FailureClass::Permanent {
                        "failed"
                    } else {
                        "pending"
                    };
                    let next_attempt = if state == "failed" { 0 } else { ts + 1 };
                    let _ = self.client.execute(
                        "UPDATE settlement_outbox SET state=$2,attempts=attempts+1,last_error=$3, \
                         next_attempt_ts=$4,updated_ts=$5 WHERE request_id=$1 AND state <> 'done'",
                        &[&id, &state, &message, &next_attempt, &ts],
                    );
                    if state == "failed" {
                        eprintln!("billing outbox request {id} moved to failed: {message}");
                    }
                }
            }
        }
        Ok(done)
    }

    /// Recover only reservations whose exact owner epoch is provably dead/fenced.
    pub fn reconcile_expired(&mut self, limit: usize) -> Result<ReconcileReport> {
        let ts = now();
        let rows = self.client.query(
            "SELECT r.request_id,r.state,r.hold_nano FROM reservations r \
             LEFT JOIN engine_instances i ON i.instance_id=r.owner_instance AND i.owner_epoch=r.owner_epoch \
             WHERE r.state IN ('reserved','delivering','settlement_pending') AND r.lease_until < $1 \
             AND (i.instance_id IS NULL OR i.lease_until < $1) ORDER BY r.created_ts LIMIT $2",
            &[&ts, &(limit.clamp(1, 10_000) as i64)],
        )?;
        let mut report = ReconcileReport::default();
        for row in rows {
            let request_id: String = row.get(0);
            let state: String = row.get(1);
            let hold: i64 = row.get(2);
            match state.as_str() {
                "reserved" => {
                    self.enqueue_outbox(&request_id, 0, "cancel", None, None)?;
                    report.canceled_before_delivery += 1;
                }
                "delivering" => {
                    self.enqueue_outbox(
                        &request_id,
                        hold,
                        "reconcile_full_hold",
                        Some("expired-delivery"),
                        None,
                    )?;
                    report.charged_after_delivery += 1;
                }
                "settlement_pending" => {}
                _ => continue,
            }
        }
        report.processed_outbox = self.drain_outbox(limit)?;
        Ok(report)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn acquire_capacity(
        &mut self,
        owner: &Owner,
        lease_id: &str,
        request_id: &str,
        email: &str,
        lease_secs: i64,
        util_cap: f64,
    ) -> Result<Option<CapacityLease>> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        if let Some(row) = tx.query_opt(
            "SELECT request_id,subscription_email,lease_until,state,owner_instance,owner_epoch \
             FROM capacity_leases WHERE lease_id=$1",
            &[&lease_id],
        )? {
            let exact = row.get::<_, String>(0) == request_id
                && row.get::<_, String>(1) == email
                && row.get::<_, String>(3) == "active"
                && row.get::<_, String>(4) == owner.instance_id
                && row.get::<_, i64>(5) == owner.epoch;
            if !exact {
                bail!("capacity lease ID belongs to another operation");
            }
            let lease = CapacityLease {
                lease_id: lease_id.to_owned(),
                request_id: request_id.to_owned(),
                subscription_email: email.to_owned(),
                lease_until: row.get(2),
            };
            tx.commit()?;
            return Ok(Some(lease));
        }
        let expired = tx.execute(
            "UPDATE capacity_leases SET state='expired',released_ts=$2 \
             WHERE subscription_email=$1 AND state='active' AND lease_until < $2",
            &[&email, &ts],
        )? as i64;
        if expired > 0 {
            tx.execute(
                "UPDATE pool_state SET inflight=GREATEST(0,inflight-$2) WHERE email=$1",
                &[&email, &expired],
            )?;
        }
        let Some(state) = tx.query_opt(
            "SELECT cooling_until,util5,util7,reset5,reset7 FROM pool_state WHERE email=$1 FOR UPDATE",
            &[&email],
        )? else {
            tx.rollback()?;
            return Ok(None);
        };
        let cooling_until: i64 = state.get(0);
        let util5: f64 = state.get(1);
        let util7: f64 = state.get(2);
        let reset5: i64 = state.get(3);
        let reset7: i64 = state.get(4);
        let effective5 = if reset5 > 0 && reset5 <= ts {
            0.0
        } else {
            util5
        };
        let effective7 = if reset7 > 0 && reset7 <= ts {
            0.0
        } else {
            util7
        };
        if cooling_until > ts || effective5 >= util_cap || effective7 >= util_cap {
            tx.rollback()?;
            return Ok(None);
        }
        let lease_until = ts + lease_secs.max(1);
        tx.execute(
            "INSERT INTO capacity_leases(lease_id,request_id,subscription_email,owner_instance,owner_epoch, \
             lease_until,state,created_ts) VALUES($1,$2,$3,$4,$5,$6,'active',$7)",
            &[&lease_id,&request_id,&email,&owner.instance_id,&owner.epoch,&lease_until,&ts],
        )?;
        tx.execute(
            "UPDATE pool_state SET inflight=inflight+1 WHERE email=$1",
            &[&email],
        )?;
        tx.commit()?;
        Ok(Some(CapacityLease {
            lease_id: lease_id.to_owned(),
            request_id: request_id.to_owned(),
            subscription_email: email.to_owned(),
            lease_until,
        }))
    }

    pub fn release_capacity(&mut self, owner: &Owner, lease_id: &str) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        let row = tx.query_opt(
            "UPDATE capacity_leases SET state='released',released_ts=$4 \
             WHERE lease_id=$1 AND owner_instance=$2 AND owner_epoch=$3 AND state='active' \
             RETURNING subscription_email",
            &[&lease_id, &owner.instance_id, &owner.epoch, &ts],
        )?;
        if let Some(row) = row {
            let email: String = row.get(0);
            tx.execute(
                "UPDATE pool_state SET inflight=GREATEST(0,inflight-1) WHERE email=$1",
                &[&email],
            )?;
            tx.commit()?;
            Ok(true)
        } else {
            tx.rollback()?;
            Ok(false)
        }
    }

    pub fn acquire_leader(&mut self, owner: &Owner, name: &str, ttl_secs: i64) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        tx.execute(
            "INSERT INTO leader_leases(name,owner_instance,owner_epoch,lease_until,updated_ts) \
             VALUES($1,$2,$3,$4,$5) ON CONFLICT(name) DO NOTHING",
            &[
                &name,
                &owner.instance_id,
                &owner.epoch,
                &(ts + ttl_secs.max(1)),
                &ts,
            ],
        )?;
        let changed = tx.execute(
            "UPDATE leader_leases SET owner_instance=$2,owner_epoch=$3,lease_until=$4,updated_ts=$5 \
             WHERE name=$1 AND ((owner_instance=$2 AND owner_epoch=$3) OR lease_until < $5)",
            &[&name,&owner.instance_id,&owner.epoch,&(ts + ttl_secs.max(1)),&ts],
        )?;
        tx.commit()?;
        Ok(changed == 1)
    }

    // -- Subscription registry ---------------------------------------------------------------

    pub fn load_active(&mut self, fleet: Option<&str>) -> Result<Vec<Sub>> {
        let rows = self.client.query(
            "SELECT email,token,token_file,proxy,status,fleet,plan FROM subs \
             WHERE status='active' AND ($1::text IS NULL OR fleet=$1) ORDER BY added_ts",
            &[&fleet],
        )?;
        let mut out = Vec::new();
        for row in rows {
            let email: String = row.get(0);
            let token = crate::resolve_token(row.get(1), row.get(2));
            if token.is_empty() {
                continue;
            }
            out.push(Sub {
                email,
                token,
                proxy: row.get(3),
                fleet: row.get(5),
                plan: row.get(6),
            });
        }
        Ok(out)
    }

    pub fn add(&mut self, email: &str, token: &str, proxy: &str, fleet: &str) -> Result<()> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        tx.execute(
            "INSERT INTO subs(email,token,token_file,proxy,status,fleet,added_ts,added) \
             VALUES($1,$2,NULL,$3,'active',$4,$5,$6) ON CONFLICT(email) DO UPDATE SET \
             token=EXCLUDED.token,token_file=NULL,proxy=EXCLUDED.proxy,status='active',fleet=EXCLUDED.fleet, \
             auth_state=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN 'healthy' ELSE subs.auth_state END, \
             auth_fail_streak=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN 0 ELSE subs.auth_fail_streak END, \
             first_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN 0 ELSE subs.first_auth_fail_ts END, \
             last_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN 0 ELSE subs.last_auth_fail_ts END, \
             last_auth_http=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN 0 ELSE subs.last_auth_http END, \
             dead_since_ts=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN 0 ELSE subs.dead_since_ts END, \
             dead_reason=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN '' ELSE subs.dead_reason END, \
             auth_token_fp=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN '' ELSE subs.auth_token_fp END",
            &[&email,&token,&proxy,&fleet,&ts,&chrono_like(ts)],
        )?;
        tx.execute(
            "INSERT INTO pool_state(email) VALUES($1) ON CONFLICT DO NOTHING",
            &[&email],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn add_file(
        &mut self,
        email: &str,
        token_file: &str,
        proxy: &str,
        fleet: &str,
    ) -> Result<()> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        tx.execute(
            "INSERT INTO subs(email,token,token_file,proxy,status,fleet,added_ts,added) \
             VALUES($1,NULL,$2,$3,'active',$4,$5,$6) ON CONFLICT(email) DO UPDATE SET \
             token=NULL,token_file=EXCLUDED.token_file,proxy=EXCLUDED.proxy,status='active',fleet=EXCLUDED.fleet, \
             auth_state=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN 'healthy' ELSE subs.auth_state END, \
             auth_fail_streak=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN 0 ELSE subs.auth_fail_streak END, \
             first_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN 0 ELSE subs.first_auth_fail_ts END, \
             last_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN 0 ELSE subs.last_auth_fail_ts END, \
             last_auth_http=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN 0 ELSE subs.last_auth_http END, \
             dead_since_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN 0 ELSE subs.dead_since_ts END, \
             dead_reason=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN '' ELSE subs.dead_reason END, \
             auth_token_fp=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN '' ELSE subs.auth_token_fp END",
            &[&email,&token_file,&proxy,&fleet,&ts,&chrono_like(ts)],
        )?;
        tx.execute(
            "INSERT INTO pool_state(email) VALUES($1) ON CONFLICT DO NOTHING",
            &[&email],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_sub_status(&mut self, email: &str, status: &str) -> Result<usize> {
        Ok(self.client.execute(
            "UPDATE subs SET status=$1 WHERE email=$2",
            &[&status, &email],
        )? as usize)
    }
    pub fn set_plan(&mut self, email: &str, plan: &str) -> Result<usize> {
        Ok(self
            .client
            .execute("UPDATE subs SET plan=$1 WHERE email=$2", &[&plan, &email])?
            as usize)
    }
    pub fn set_proxy(&mut self, email: &str, proxy: &str) -> Result<usize> {
        Ok(self
            .client
            .execute("UPDATE subs SET proxy=$1 WHERE email=$2", &[&proxy, &email])?
            as usize)
    }
    pub fn set_fleet(&mut self, email: &str, fleet: &str) -> Result<usize> {
        Ok(self
            .client
            .execute("UPDATE subs SET fleet=$1 WHERE email=$2", &[&fleet, &email])?
            as usize)
    }
    pub fn set_proxy_meta(
        &mut self,
        email: &str,
        expire: &str,
        checked_ts: i64,
        ok: bool,
    ) -> Result<usize> {
        Ok(self.client.execute(
            "UPDATE subs SET proxy_expire=$1,proxy_checked_ts=$2,proxy_ok=$3 WHERE email=$4",
            &[&expire, &checked_ts, &ok, &email],
        )? as usize)
    }
    pub fn get_creds(&mut self, email: &str) -> Result<Option<(String, String)>> {
        let Some(row) = self.client.query_opt(
            "SELECT token,token_file,proxy FROM subs WHERE email=$1",
            &[&email],
        )?
        else {
            return Ok(None);
        };
        let token = crate::resolve_token(row.get(0), row.get(1));
        if token.is_empty() {
            Ok(None)
        } else {
            Ok(Some((token, row.get(2))))
        }
    }
    pub fn remove_sub(&mut self, email: &str) -> Result<usize> {
        Ok(self.client.execute(
            "UPDATE subs SET status='deleted' WHERE email=$1 AND status<>'deleted'",
            &[&email],
        )? as usize)
    }
    pub fn clear_subs(&mut self, fleet: Option<&str>) -> Result<usize> {
        Ok(match fleet {
            Some(f) => self.client.execute(
                "UPDATE subs SET status='deleted' WHERE fleet=$1 AND status<>'deleted'",
                &[&f],
            )?,
            None => self.client.execute(
                "UPDATE subs SET status='deleted' WHERE status<>'deleted'",
                &[],
            )?,
        } as usize)
    }
    pub fn list_subs(&mut self) -> Result<Vec<SubRow>> {
        Ok(self.client.query(
            "SELECT email,status,fleet,plan,COALESCE(NULLIF(token,''),NULLIF(token_file,'')),proxy \
             FROM subs ORDER BY added_ts",
            &[],
        )?.into_iter().map(|row| SubRow {
            email: row.get(0), status: row.get(1), fleet: row.get(2), plan: row.get(3),
            has_token: row.get::<_, Option<String>>(4).is_some_and(|s| !s.is_empty()), proxy: row.get(5),
        }).collect())
    }
    pub fn subs_admin(&mut self) -> Result<Vec<SubAdmin>> {
        Ok(self.client.query(
            "SELECT email,status,fleet,COALESCE(NULLIF(token,''),NULLIF(token_file,'')),proxy, \
             proxy_expire,proxy_ok,added_ts,added, \
             COALESCE(auth_state,'healthy'),COALESCE(dead_reason,''),COALESCE(dead_since_ts,0) \
             FROM subs ORDER BY added_ts",
            &[],
        )?.into_iter().map(|row| {
            let proxy: String = row.get(4);
            SubAdmin {
                email: row.get(0), status: row.get(1), fleet: row.get(2),
                has_token: row.get::<_, Option<String>>(3).is_some_and(|s| !s.is_empty()),
                proxy_host: mask_proxy(&proxy), proxy_expire: row.get(5), proxy_ok: row.get(6),
                added_ts: row.get(7), added: row.get(8),
                auth_state: row.get(9), dead_reason: row.get(10), dead_since_ts: row.get(11),
            }
        }).collect())
    }

    /// Load durable auth-health for every subscription (engine seeds in-memory state at startup).
    pub fn load_sub_health(&mut self, fleet: Option<&str>) -> Result<Vec<SubHealth>> {
        Ok(self.client.query(
            "SELECT email,COALESCE(auth_state,'healthy'),COALESCE(auth_fail_streak,0), \
             COALESCE(first_auth_fail_ts,0),COALESCE(last_auth_fail_ts,0),COALESCE(last_auth_http,0), \
             COALESCE(dead_since_ts,0),COALESCE(dead_reason,''),COALESCE(auth_token_fp,'') \
             FROM subs WHERE ($1::text IS NULL OR fleet=$1) ORDER BY added_ts",
            &[&fleet],
        )?.into_iter().map(|row| SubHealth {
            email: row.get(0),
            auth_state: row.get(1),
            auth_fail_streak: row.get::<_, i32>(2) as i64,
            first_auth_fail_ts: row.get(3),
            last_auth_fail_ts: row.get(4),
            last_auth_http: row.get::<_, i32>(5) as i64,
            dead_since_ts: row.get(6),
            dead_reason: row.get(7),
            auth_token_fp: row.get(8),
        }).collect())
    }

    /// Persist one subscription's durable auth-health verdict. Owner-fenced: a stale/fenced engine
    /// (lost the epoch) must not stamp health, exactly like money/pool-state writes.
    pub fn save_sub_health(&mut self, owner: &Owner, h: &SubHealth) -> Result<usize> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        let streak = h.auth_fail_streak as i32;
        let http = h.last_auth_http as i32;
        let first = (h.first_auth_fail_ts != 0).then_some(h.first_auth_fail_ts);
        let last = (h.last_auth_fail_ts != 0).then_some(h.last_auth_fail_ts);
        let http = (http != 0).then_some(http);
        let dead_since = (h.dead_since_ts != 0).then_some(h.dead_since_ts);
        let reason = (!h.dead_reason.is_empty()).then_some(h.dead_reason.as_str());
        let fp = (!h.auth_token_fp.is_empty()).then_some(h.auth_token_fp.as_str());
        let n = tx.execute(
            "UPDATE subs SET auth_state=$1,auth_fail_streak=$2,first_auth_fail_ts=$3, \
             last_auth_fail_ts=$4,last_auth_http=$5,dead_since_ts=$6,dead_reason=$7,auth_token_fp=$8 \
             WHERE email=$9",
            &[&h.auth_state,&streak,&first,&last,&http,&dead_since,&reason,&fp,&h.email],
        )?;
        tx.commit()?;
        Ok(n as usize)
    }

    // -- Accounts, keys, ledger, analytics ---------------------------------------------------

    pub fn account_create(&mut self, id: &str, handle: Option<&str>, mult_bp: i64) -> Result<()> {
        if id.trim().is_empty() || handle.is_some_and(|value| value.trim().is_empty()) {
            bail!("account id and supplied handle must not be empty");
        }
        if !(0..=10_000).contains(&mult_bp) {
            bail!("account multiplier must be within 0..=10000 basis points");
        }
        let ts = now();
        self.client.execute(
            "INSERT INTO accounts(id,handle,mult_bp,status,created_ts,created) VALUES($1,$2,$3,'active',$4,$5)",
            &[&id,&handle,&mult_bp,&ts,&chrono_like(ts)],
        )?;
        Ok(())
    }
    pub fn account_get(&mut self, id: &str) -> Result<Option<AccountRow>> {
        Ok(self.client.query_opt(
            "SELECT id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status FROM accounts WHERE id=$1",
            &[&id],
        )?.map(|r| account_row(&r)))
    }
    pub fn account_funding_snapshot(&mut self, id: &str) -> Result<Option<AccountFundingSnapshot>> {
        let Some(row) = self.client.query_opt(
            "SELECT
                 account.id,account.handle,account.balance_nano,account.spent_nano,
                 account.reserved_nano,account.mult_bp,account.status,
                 binding.account_class,binding.funding_enforcement,binding.reconciliation_state,
                 COUNT(bucket.bucket_id)::bigint,
                 COALESCE(SUM(CASE WHEN bucket.source_type='paid'
                                   THEN bucket.balance_nano ELSE 0 END),0)::bigint,
                 COALESCE(SUM(CASE WHEN bucket.source_type='welcome_track_bonus'
                                   THEN bucket.balance_nano ELSE 0 END),0)::bigint,
                 COALESCE(SUM(CASE WHEN bucket.source_type NOT IN ('paid','welcome_track_bonus')
                                   THEN bucket.balance_nano ELSE 0 END),0)::bigint,
                 COALESCE(SUM(CASE WHEN bucket.source_type='paid'
                                   THEN bucket.reserved_nano ELSE 0 END),0)::bigint,
                 COALESCE(SUM(CASE WHEN bucket.source_type='welcome_track_bonus'
                                   THEN bucket.reserved_nano ELSE 0 END),0)::bigint,
                 COALESCE(SUM(CASE WHEN bucket.source_type NOT IN ('paid','welcome_track_bonus')
                                   THEN bucket.reserved_nano ELSE 0 END),0)::bigint,
                 COALESCE(SUM(CASE WHEN bucket.source_type='paid'
                                   THEN bucket.spent_nano ELSE 0 END),0)::bigint,
                 COALESCE(SUM(CASE WHEN bucket.source_type='welcome_track_bonus'
                                   THEN bucket.spent_nano ELSE 0 END),0)::bigint,
                 COALESCE(SUM(CASE WHEN bucket.source_type NOT IN ('paid','welcome_track_bonus')
                                   THEN bucket.spent_nano ELSE 0 END),0)::bigint
               FROM accounts account
               LEFT JOIN account_policy_bindings binding ON binding.account_id=account.id
               LEFT JOIN funding_buckets bucket ON bucket.account_id=account.id
              WHERE account.id=$1
              GROUP BY account.id,account.handle,account.balance_nano,account.spent_nano,
                       account.reserved_nano,account.mult_bp,account.status,binding.account_class,
                       binding.funding_enforcement,binding.reconciliation_state",
            &[&id],
        )?
        else {
            return Ok(None);
        };
        crate::account_funding_snapshot_from_parts(
            account_row(&row),
            row.get(7),
            row.get(8),
            row.get(9),
            row.get(10),
            row.get(11),
            row.get(12),
            row.get(13),
            row.get(14),
            row.get(15),
            row.get(16),
            row.get(17),
            row.get(18),
            row.get(19),
        )
        .map(Some)
    }
    pub fn account_by_handle(&mut self, handle: &str) -> Result<Option<AccountRow>> {
        Ok(self.client.query_opt(
            "SELECT id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status FROM accounts WHERE handle=$1",
            &[&handle],
        )?.map(|r| account_row(&r)))
    }
    pub fn account_list(&mut self) -> Result<Vec<AccountRow>> {
        Ok(self.client.query(
            "SELECT id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status FROM accounts ORDER BY created_ts",
            &[],
        )?.into_iter().map(|r| account_row(&r)).collect())
    }
    pub fn account_set_status(&mut self, id: &str, status: &str) -> Result<usize> {
        Ok(self
            .client
            .execute("UPDATE accounts SET status=$1 WHERE id=$2", &[&status, &id])?
            as usize)
    }
    pub fn account_set_mult_bp(&mut self, id: &str, mult_bp: i64) -> Result<usize> {
        if !(0..=10_000).contains(&mult_bp) {
            bail!("invalid account multiplier");
        }
        Ok(self.client.execute(
            "UPDATE accounts SET mult_bp=$1 WHERE id=$2",
            &[&mult_bp, &id],
        )? as usize)
    }
    pub fn account_remove(&mut self, id: &str) -> Result<usize> {
        Ok(self.client.execute(
            "UPDATE accounts SET status='deleted' WHERE id=$1 AND status<>'deleted'",
            &[&id],
        )? as usize)
    }

    /// Build the deterministic, read-only Stage 6 funding migration plan from one serializable
    /// PostgreSQL snapshot. This does not change aggregate balances or live billing behavior.
    pub fn funding_reconciliation_plan(
        &mut self,
    ) -> Result<crate::funding::FundingReconciliationPlan> {
        crate::funding::postgres_funding_reconciliation_plan(&mut self.client)
    }

    /// Apply only the exact content-addressed Stage 6 plan observed in the same serializable
    /// transaction. Exception authority is explicit and never promotes an exception to verified.
    pub fn apply_funding_reconciliation(
        &mut self,
        approved_plan_digest: &str,
        allow_exceptions: bool,
    ) -> Result<crate::funding::FundingReconciliationApplyReport> {
        crate::funding::postgres_apply_funding_reconciliation(
            &mut self.client,
            approved_plan_digest,
            allow_exceptions,
        )
    }

    pub fn account_topup(
        &mut self,
        id: &str,
        amount_nano: i64,
        reference: Option<&str>,
    ) -> Result<Option<i64>> {
        if matches!(reference, Some(r) if r.trim().is_empty()) {
            bail!("monetary idempotency reference must not be empty");
        }
        let allocation_amount = amount_nano
            .checked_abs()
            .context("top-up amount cannot be represented as a funding allocation")?;
        let ts = now();
        let kind = if amount_nano >= 0 { "topup" } else { "adjust" };
        let mut tx = self.client.transaction()?;
        if let Some(reference) = reference {
            if let Some(row) = tx.query_opt(
                "SELECT account_id,kind,amount_nano,balance_after_nano FROM ledger \
                 WHERE ref=$1 AND kind IN ('topup','adjust')",
                &[&reference],
            )? {
                let exact = row.get::<_, String>(0) == id
                    && row.get::<_, String>(1) == kind
                    && row.get::<_, i64>(2) == amount_nano;
                if !exact {
                    bail!("idempotency reference belongs to another monetary operation");
                }
                let original = row.get(3);
                tx.commit()?;
                return Ok(original);
            }
        }
        let strict_funding: bool = tx
            .query_one(
                "SELECT EXISTS(
                 SELECT 1 FROM account_policy_bindings
                  WHERE account_id=$1
                    AND policy_enforcement='strict'
                    AND funding_enforcement='strict'
                    AND reconciliation_state='verified'
             )",
                &[&id],
            )?
            .get(0);
        let Some(row) = tx.query_opt(
            "UPDATE accounts SET balance_nano=balance_nano+$1 WHERE id=$2 RETURNING balance_nano",
            &[&amount_nano, &id],
        )?
        else {
            tx.rollback()?;
            return Ok(None);
        };
        let balance: i64 = row.get(0);
        let strict_bucket = if strict_funding {
            let source_ref = reference.unwrap_or("");
            Some(tx.query_one(
                "INSERT INTO funding_buckets(
                     bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                     reserved_nano,spent_nano,version,status,created_ts,updated_ts)
                 VALUES('fund_' || md5(random()::text || clock_timestamp()::text),$1,'paid',$2,
                        'any',$3::bigint,0,0,1,
                        CASE WHEN $3::bigint>0 THEN 'active' ELSE 'exhausted' END,$4,$4)
                 ON CONFLICT(account_id,source_type,source_ref) DO UPDATE SET
                     balance_nano=funding_buckets.balance_nano+EXCLUDED.balance_nano,
                     version=funding_buckets.version+1,updated_ts=EXCLUDED.updated_ts,
                     status=CASE
                       WHEN funding_buckets.status='retired' THEN funding_buckets.status
                       WHEN funding_buckets.balance_nano+EXCLUDED.balance_nano>0 THEN 'active'
                       ELSE 'exhausted'
                     END
                 RETURNING bucket_id,version",
                &[&id, &source_ref, &amount_nano, &ts],
            )?)
        } else {
            None
        };
        let ledger_id: i64 = tx
            .query_one(
                "INSERT INTO ledger(account_id,kind,amount_nano,ref,balance_after_nano,ts)
             VALUES($1,$2,$3,$4,$5,$6) RETURNING id",
                &[&id, &kind, &amount_nano, &reference, &balance, &ts],
            )?
            .get(0);
        if let Some(bucket) = strict_bucket {
            let bucket_id: String = bucket.get(0);
            let bucket_version: i64 = bucket.get(1);
            let direction = if amount_nano >= 0 { "credit" } else { "debit" };
            tx.execute(
                "INSERT INTO ledger_funding_allocations(
                     ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
                     direction,amount_nano)
                 VALUES($1,$2,$3,'paid',$4,$5,$6)",
                &[
                    &ledger_id,
                    &id,
                    &bucket_id,
                    &bucket_version,
                    &direction,
                    &allocation_amount,
                ],
            )?;
        }
        tx.commit()?;
        Ok(Some(balance))
    }

    pub fn key_issue(&mut self, key: &str, account_id: &str, label: Option<&str>) -> Result<()> {
        self.key_issue_with_policy(key, account_id, label, None, None)
    }
    pub fn key_issue_with_policy(
        &mut self,
        key: &str,
        account_id: &str,
        label: Option<&str>,
        spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>,
    ) -> Result<()> {
        self.key_issue_with_policy_ack(key, account_id, label, spend_limit_nano, expires_ts, None)
    }
    pub fn key_issue_with_policy_ack(
        &mut self,
        key: &str,
        account_id: &str,
        label: Option<&str>,
        spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>,
        activation_policy_ack: Option<&crate::KeyActivationPolicyAck>,
    ) -> Result<()> {
        if key.trim().is_empty() || account_id.trim().is_empty() {
            bail!("key and account id must not be empty");
        }
        activation_policy_ack
            .map(crate::KeyActivationPolicyAck::validate)
            .transpose()?;
        let ts = now();
        let mut tx = self.client.transaction()?;
        let policy_state = tx.query_opt(
            "SELECT binding.policy_enforcement,binding.active_effective_version,policy.content_digest
               FROM account_policy_bindings binding
               LEFT JOIN account_policy_versions policy
                 ON policy.account_id=binding.account_id
                AND policy.effective_version=binding.active_effective_version
              WHERE binding.account_id=$1",
            &[&account_id],
        )?;
        let strict = policy_state
            .as_ref()
            .is_some_and(|row| row.get::<_, String>(0) == "strict");
        let ack_matches = activation_policy_ack.is_some_and(|ack| {
            policy_state.as_ref().is_some_and(|row| {
                row.get::<_, Option<i64>>(1) == Some(ack.effective_policy_version)
                    && row.get::<_, Option<String>>(2).as_deref()
                        == Some(ack.policy_digest.as_str())
            })
        });
        if activation_policy_ack.is_some() && !ack_matches {
            bail!("key activation policy ACK does not match the exact active policy");
        }
        if strict && !ack_matches {
            bail!("strict key activation requires the exact active policy ACK");
        }
        let ack_version = activation_policy_ack.map(|ack| ack.effective_policy_version);
        let ack_digest = activation_policy_ack.map(|ack| ack.policy_digest.as_str());
        let ack_ts = activation_policy_ack.map(|_| ts);
        let changed = tx.execute(
            "INSERT INTO api_keys(
                 key,key_id,account_id,label,spend_limit_nano,expires_ts,status,created_ts,created,
                 activation_policy_effective_version,activation_policy_digest,
                 activation_policy_ack_ts) \
             VALUES($1,'key_' || md5(random()::text || clock_timestamp()::text),$2,$3,$4,$5,
                    'active',$6,$7,$8,$9,$10) \
             ON CONFLICT(key) DO UPDATE SET label=EXCLUDED.label, \
             spend_limit_nano=EXCLUDED.spend_limit_nano,expires_ts=EXCLUDED.expires_ts,
             activation_policy_effective_version=COALESCE(
                 EXCLUDED.activation_policy_effective_version,
                 api_keys.activation_policy_effective_version
             ),
             activation_policy_digest=COALESCE(
                 EXCLUDED.activation_policy_digest,
                 api_keys.activation_policy_digest
             ),
             activation_policy_ack_ts=COALESCE(
                 EXCLUDED.activation_policy_ack_ts,
                 api_keys.activation_policy_ack_ts
             ) \
             WHERE api_keys.account_id=EXCLUDED.account_id",
            &[
                &key,
                &account_id,
                &label,
                &spend_limit_nano,
                &expires_ts,
                &ts,
                &chrono_like(ts),
                &ack_version,
                &ack_digest,
                &ack_ts,
            ],
        )?;
        if changed == 0 {
            bail!("key is already owned by another account");
        }
        tx.commit()?;
        Ok(())
    }
    pub fn key_account(&mut self, key: &str) -> Result<Option<KeyAuth>> {
        let Some(row) = self.client.query_opt(
            "SELECT a.id,a.mult_bp,a.balance_nano,k.spent_nano,k.reserved_nano,
                    k.spend_limit_nano,k.expires_ts,(k.status='active' AND a.status='active'),
                    binding.policy_enforcement,binding.funding_enforcement,
                    binding.reconciliation_state,binding.active_effective_version,
                    policy.content_digest,k.activation_policy_effective_version,
                    k.activation_policy_digest,k.activation_policy_ack_ts,
                    COALESCE(
                        k.activation_policy_effective_version=binding.active_effective_version
                        AND k.activation_policy_digest=policy.content_digest
                        AND k.activation_policy_ack_ts IS NOT NULL,
                        false
                    ),
                    CASE WHEN binding.funding_enforcement='strict' THEN (
                        SELECT COALESCE(SUM(bucket.balance_nano),0)::bigint
                          FROM funding_buckets bucket
                         WHERE bucket.account_id=a.id AND bucket.eligibility='any'
                    ) END,
                    CASE WHEN binding.funding_enforcement='strict' THEN (
                        SELECT COALESCE(SUM(bucket.balance_nano),0)::bigint
                          FROM funding_buckets bucket
                         WHERE bucket.account_id=a.id
                           AND bucket.eligibility IN ('track','any')
                    ) END
               FROM api_keys k
               JOIN accounts a ON a.id=k.account_id
               LEFT JOIN account_policy_bindings binding ON binding.account_id=a.id
               LEFT JOIN account_policy_versions policy
                 ON policy.account_id=binding.account_id
                AND policy.effective_version=binding.active_effective_version
              WHERE k.key=$1",
            &[&key],
        )?
        else {
            return Ok(None);
        };
        let policy_enforcement = row
            .get::<_, Option<String>>(8)
            .as_deref()
            .map(crate::pricing::PolicyEnforcement::from_db)
            .transpose()?;
        let funding_enforcement = row
            .get::<_, Option<String>>(9)
            .as_deref()
            .map(crate::pricing::FundingEnforcement::from_db)
            .transpose()?;
        let reconciliation_state = row
            .get::<_, Option<String>>(10)
            .as_deref()
            .map(crate::pricing::ReconciliationState::from_db)
            .transpose()?;
        Ok(Some(KeyAuth {
            account_id: row.get(0),
            mult_bp: row.get(1),
            balance_nano: row.get(2),
            spent_nano: row.get(3),
            reserved_nano: row.get(4),
            spend_limit_nano: row.get(5),
            expires_ts: row.get(6),
            active: row.get(7),
            policy_enforcement,
            funding_enforcement,
            reconciliation_state,
            active_policy_effective_version: row.get(11),
            active_policy_digest: row.get(12),
            activation_policy_effective_version: row.get(13),
            activation_policy_digest: row.get(14),
            activation_policy_ack_ts: row.get(15),
            policy_ack_current: row.get(16),
            paid_available_nano: row.get(17),
            track_available_nano: row.get(18),
        }))
    }
    pub fn key_get(&mut self, key: &str) -> Result<Option<KeyRow>> {
        Ok(self.client.query_opt(
            "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
             k.spend_limit_nano,k.expires_ts,k.created_ts, \
             (SELECT MAX(u.ts) FROM usage_events u WHERE u.account_id=k.account_id AND u.key=k.key), \
             k.status \
             FROM api_keys k WHERE k.key=$1",
            &[&key],
        )?.map(|r| key_row(&r)))
    }
    pub fn key_list(&mut self) -> Result<Vec<KeyRow>> {
        Ok(self
            .client
            .query(
                "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
             k.spend_limit_nano,k.expires_ts,k.created_ts,u.last_used_ts,k.status \
             FROM api_keys k LEFT JOIN ( \
               SELECT key,MAX(ts) AS last_used_ts FROM usage_events GROUP BY key \
             ) u ON u.key=k.key ORDER BY k.created_ts",
                &[],
            )?
            .into_iter()
            .map(|r| key_row(&r))
            .collect())
    }
    pub fn keys_by_account(&mut self, account_id: &str) -> Result<Vec<KeyRow>> {
        Ok(self.client.query(
            "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
             k.spend_limit_nano,k.expires_ts,k.created_ts,u.last_used_ts,k.status \
             FROM api_keys k LEFT JOIN ( \
               SELECT key,MAX(ts) AS last_used_ts FROM usage_events WHERE account_id=$1 GROUP BY key \
             ) u ON u.key=k.key WHERE k.account_id=$1 ORDER BY k.created_ts",
            &[&account_id],
        )?.into_iter().map(|r| key_row(&r)).collect())
    }
    pub fn key_set_status(&mut self, key: &str, status: &str) -> Result<usize> {
        self.key_set_status_with_policy_ack(key, status, None)
    }
    pub fn key_set_status_with_policy_ack(
        &mut self,
        key: &str,
        status: &str,
        activation_policy_ack: Option<&crate::KeyActivationPolicyAck>,
    ) -> Result<usize> {
        self.key_set_status_identity_with_policy_ack("key", key, status, activation_policy_ack)
    }
    pub fn key_set_status_by_id(&mut self, key_id: &str, status: &str) -> Result<usize> {
        self.key_set_status_by_id_with_policy_ack(key_id, status, None)
    }
    pub fn key_set_status_by_id_with_policy_ack(
        &mut self,
        key_id: &str,
        status: &str,
        activation_policy_ack: Option<&crate::KeyActivationPolicyAck>,
    ) -> Result<usize> {
        self.key_set_status_identity_with_policy_ack(
            "key_id",
            key_id,
            status,
            activation_policy_ack,
        )
    }
    fn key_set_status_identity_with_policy_ack(
        &mut self,
        identity_column: &str,
        identity: &str,
        status: &str,
        activation_policy_ack: Option<&crate::KeyActivationPolicyAck>,
    ) -> Result<usize> {
        if !matches!(identity_column, "key" | "key_id") {
            bail!("invalid key status identity column");
        }
        activation_policy_ack
            .map(crate::KeyActivationPolicyAck::validate)
            .transpose()?;
        let mut tx = self.client.transaction()?;
        let query = format!(
            "SELECT binding.policy_enforcement,binding.active_effective_version,
                    policy.content_digest
               FROM api_keys key
               LEFT JOIN account_policy_bindings binding ON binding.account_id=key.account_id
               LEFT JOIN account_policy_versions policy
                 ON policy.account_id=binding.account_id
                AND policy.effective_version=binding.active_effective_version
              WHERE key.{identity_column}=$1
              FOR UPDATE OF key"
        );
        let Some(policy_state) = tx.query_opt(&query, &[&identity])? else {
            tx.rollback()?;
            return Ok(0);
        };
        let ack_matches = activation_policy_ack.is_some_and(|ack| {
            policy_state.get::<_, Option<i64>>(1) == Some(ack.effective_policy_version)
                && policy_state.get::<_, Option<String>>(2).as_deref()
                    == Some(ack.policy_digest.as_str())
        });
        if activation_policy_ack.is_some() && !ack_matches {
            bail!("key activation policy ACK does not match the exact active policy");
        }
        if status == "active"
            && policy_state.get::<_, Option<String>>(0).as_deref() == Some("strict")
            && !ack_matches
        {
            bail!("strict key reactivation requires the exact active policy ACK");
        }
        let update = format!(
            "UPDATE api_keys SET status=$1,
                 activation_policy_effective_version=COALESCE(
                     $2,activation_policy_effective_version
                 ),
                 activation_policy_digest=COALESCE($3,activation_policy_digest),
                 activation_policy_ack_ts=COALESCE($4,activation_policy_ack_ts)
              WHERE {identity_column}=$5"
        );
        let changed = tx.execute(
            &update,
            &[
                &status,
                &activation_policy_ack.map(|ack| ack.effective_policy_version),
                &activation_policy_ack.map(|ack| ack.policy_digest.as_str()),
                &activation_policy_ack.map(|_| now()),
                &identity,
            ],
        )? as usize;
        tx.commit()?;
        Ok(changed)
    }
    pub fn key_set_label_by_id(&mut self, key_id: &str, label: &str) -> Result<usize> {
        Ok(self.client.execute(
            "UPDATE api_keys SET label=$1 WHERE key_id=$2",
            &[&label, &key_id],
        )? as usize)
    }
    pub fn key_set_policy_by_id(
        &mut self,
        account_id: &str,
        key_id: &str,
        spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>,
    ) -> Result<KeyPolicyUpdate> {
        let updated = self.client.execute(
            "UPDATE api_keys SET spend_limit_nano=$3,expires_ts=$4 \
             WHERE key_id=$1 AND account_id=$2 \
               AND ($3::bigint IS NULL OR (reserved_nano<=$3 AND spent_nano<=$3-reserved_nano)) \
               AND ($4::bigint IS NULL OR $4>EXTRACT(EPOCH FROM clock_timestamp())::bigint)",
            &[&key_id, &account_id, &spend_limit_nano, &expires_ts],
        )?;
        if updated == 1 {
            return Ok(KeyPolicyUpdate::Updated);
        }
        let exists: bool = self
            .client
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM api_keys WHERE key_id=$1 AND account_id=$2)",
                &[&key_id, &account_id],
            )?
            .get(0);
        if !exists {
            return Ok(KeyPolicyUpdate::NotFound);
        }
        if expires_ts.is_some_and(|expires| expires <= now()) {
            return Ok(KeyPolicyUpdate::ExpiryNotFuture);
        }
        Ok(KeyPolicyUpdate::LimitBelowUsage)
    }
    pub fn key_remove(&mut self, key: &str) -> Result<usize> {
        Ok(self
            .client
            .execute("DELETE FROM api_keys WHERE key=$1", &[&key])? as usize)
    }
    pub fn key_clear(&mut self) -> Result<usize> {
        Ok(self.client.execute("DELETE FROM api_keys", &[])? as usize)
    }

    fn ledger_page(
        &mut self,
        account_id: &str,
        after_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<LedgerRow>> {
        let predicate = if after_id.is_some() {
            "ledger.account_id=$1 AND ledger.id>$2 ORDER BY ledger.id ASC LIMIT $3"
        } else {
            "ledger.account_id=$1 ORDER BY ledger.id DESC LIMIT $2"
        };
        let sql = format!("SELECT {POSTGRES_LEDGER_READ_COLUMNS} FROM ledger WHERE {predicate}");
        let mut tx = self
            .client
            .build_transaction()
            .isolation_level(IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()?;
        let rows = match after_id {
            Some(after_id) => tx.query(
                &sql,
                &[&account_id, &after_id.max(0), &limit.clamp(1, 1000)],
            )?,
            None => tx.query(&sql, &[&account_id, &limit.clamp(1, 1000)])?,
        };
        let mut entries = rows
            .into_iter()
            .map(|row| ledger_row(&row))
            .collect::<Result<Vec<_>>>()?;
        if let (Some(first_id), Some(last_id)) = (
            entries.iter().map(|entry| entry.id).min(),
            entries.iter().map(|entry| entry.id).max(),
        ) {
            let mut by_ledger =
                std::collections::BTreeMap::<i64, Vec<LedgerFundingAllocation>>::new();
            for row in tx.query(
                "SELECT allocation.ledger_id,allocation.bucket_id,
                        allocation.bucket_source_type,bucket.source_ref,
                        allocation.bucket_version,allocation.direction,
                        allocation.amount_nano,reservation.allocation_order
                   FROM ledger_funding_allocations allocation
                   JOIN ledger ON ledger.id=allocation.ledger_id
                   JOIN funding_buckets bucket
                     ON bucket.bucket_id=allocation.bucket_id
                    AND bucket.account_id=allocation.account_id
                    AND bucket.source_type=allocation.bucket_source_type
                   LEFT JOIN reservation_funding_allocations reservation
                     ON reservation.request_id=ledger.request_id
                    AND reservation.account_id=allocation.account_id
                    AND reservation.bucket_id=allocation.bucket_id
                  WHERE ledger.account_id=$1 AND ledger.id BETWEEN $2 AND $3
                  ORDER BY allocation.ledger_id,
                           reservation.allocation_order NULLS LAST,
                           allocation.bucket_id",
                &[&account_id, &first_id, &last_id],
            )? {
                by_ledger
                    .entry(row.get(0))
                    .or_default()
                    .push(LedgerFundingAllocation {
                        bucket_id: row.get(1),
                        source_type: row.get(2),
                        source_ref: row.get(3),
                        bucket_version: row.get(4),
                        direction: row.get(5),
                        amount_nano: row.get(6),
                        allocation_order: row.get(7),
                    });
            }
            for entry in &mut entries {
                entry.funding_allocations = by_ledger.remove(&entry.id).unwrap_or_default();
            }
        }
        tx.commit()?;
        Ok(entries)
    }

    pub fn ledger_recent(&mut self, account_id: &str, limit: i64) -> Result<Vec<LedgerRow>> {
        self.ledger_page(account_id, None, limit)
    }
    pub fn ledger_after(
        &mut self,
        account_id: &str,
        after_id: i64,
        limit: i64,
    ) -> Result<Vec<LedgerRow>> {
        self.ledger_page(account_id, Some(after_id), limit)
    }
    pub fn ledger_ack(
        &mut self,
        consumer: &str,
        account_id: &str,
        last_ledger_id: i64,
    ) -> Result<usize> {
        if consumer.trim().is_empty() || last_ledger_id < 0 {
            bail!("invalid ledger checkpoint");
        }
        Ok(self.client.execute(
            "INSERT INTO ledger_consumer_checkpoints(consumer,account_id,last_ledger_id,updated_ts) \
             VALUES($1,$2,$3,$4) ON CONFLICT(consumer,account_id) DO UPDATE SET \
             last_ledger_id=GREATEST(ledger_consumer_checkpoints.last_ledger_id,EXCLUDED.last_ledger_id), \
             updated_ts=EXCLUDED.updated_ts",
            &[&consumer,&account_id,&last_ledger_id,&now()],
        )? as usize)
    }
    pub fn ledger_prune(&mut self, older_than_ts: i64) -> Result<usize> {
        Ok(self.client.execute(
            "DELETE FROM ledger WHERE id IN ( \
               SELECT l.id FROM ledger l JOIN ledger_consumer_checkpoints c \
                 ON c.account_id=l.account_id AND c.consumer='pricing' \
               WHERE l.kind='charge' AND l.ts < $1 AND l.id <= c.last_ledger_id \
               ORDER BY l.id LIMIT 5000 \
             )",
            &[&older_than_ts],
        )? as usize)
    }
    pub fn usage_by_model(
        &mut self,
        account_id: &str,
        since_ts: i64,
    ) -> Result<Vec<UsageModelAgg>> {
        self.usage_by_model_between(account_id, since_ts, i64::MAX)
    }
    fn usage_by_model_between(
        &mut self,
        account_id: &str,
        since_ts: i64,
        until_ts: i64,
    ) -> Result<Vec<UsageModelAgg>> {
        Ok(self.client.query(
            "SELECT COALESCE(model,''),COALESCE(NULLIF(provider,''),'anthropic'),COUNT(*)::bigint,COALESCE(SUM(input_tokens),0)::bigint, \
             COALESCE(SUM(output_tokens),0)::bigint,COALESCE(SUM(cache_read_tokens),0)::bigint, \
             COALESCE(SUM(cache_write_5m_tokens),0)::bigint,COALESCE(SUM(cache_write_1h_tokens),0)::bigint, \
             COALESCE(SUM(web_search_requests),0)::bigint,COALESCE(SUM(real_nano),0)::bigint, \
             COALESCE(SUM(charge_nano),0)::bigint,COALESCE(SUM(input_nano),0)::bigint, \
             COALESCE(SUM(output_nano),0)::bigint,COALESCE(SUM(cache_read_nano),0)::bigint, \
             COALESCE(SUM(cache_write_5m_nano),0)::bigint,COALESCE(SUM(cache_write_1h_nano),0)::bigint, \
             COALESCE(SUM(web_search_nano),0)::bigint FROM usage_events \
             WHERE account_id=$1 AND ts >= $2 AND ts < $3 \
             GROUP BY model,COALESCE(NULLIF(provider,''),'anthropic') ORDER BY SUM(real_nano) DESC,model,COALESCE(NULLIF(provider,''),'anthropic')",
            &[&account_id,&since_ts,&until_ts],
        )?.into_iter().map(|r| UsageModelAgg {
            model:r.get(0),provider:r.get(1),requests:r.get(2),input_tokens:r.get(3),output_tokens:r.get(4),
            cache_read_tokens:r.get(5),cache_write_5m_tokens:r.get(6),cache_write_1h_tokens:r.get(7),
            web_search_requests:r.get(8),real_nano:r.get(9),charge_nano:r.get(10),
            input_nano:r.get(11),output_nano:r.get(12),cache_read_nano:r.get(13),
            cache_write_5m_nano:r.get(14),cache_write_1h_nano:r.get(15),web_search_nano:r.get(16),
        }).collect())
    }
    pub fn usage_report(
        &mut self,
        account_id: &str,
        since_ts: i64,
        until_ts: i64,
    ) -> Result<UsageReport> {
        if until_ts <= since_ts {
            return Ok(UsageReport::default());
        }
        let mut transaction = self
            .client
            .build_transaction()
            .isolation_level(IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()?;
        let models = transaction.query(
            "SELECT COALESCE(model,''),COALESCE(NULLIF(provider,''),'anthropic'),COUNT(*)::bigint,COALESCE(SUM(input_tokens),0)::bigint, \
             COALESCE(SUM(output_tokens),0)::bigint,COALESCE(SUM(cache_read_tokens),0)::bigint, \
             COALESCE(SUM(cache_write_5m_tokens),0)::bigint,COALESCE(SUM(cache_write_1h_tokens),0)::bigint, \
             COALESCE(SUM(web_search_requests),0)::bigint,COALESCE(SUM(real_nano),0)::bigint, \
             COALESCE(SUM(charge_nano),0)::bigint,COALESCE(SUM(input_nano),0)::bigint, \
             COALESCE(SUM(output_nano),0)::bigint,COALESCE(SUM(cache_read_nano),0)::bigint, \
             COALESCE(SUM(cache_write_5m_nano),0)::bigint,COALESCE(SUM(cache_write_1h_nano),0)::bigint, \
             COALESCE(SUM(web_search_nano),0)::bigint FROM usage_events \
             WHERE account_id=$1 AND ts >= $2 AND ts < $3 \
             GROUP BY model,COALESCE(NULLIF(provider,''),'anthropic') ORDER BY SUM(real_nano) DESC,model,COALESCE(NULLIF(provider,''),'anthropic')",
            &[&account_id, &since_ts, &until_ts],
        )?.into_iter().map(|r| UsageModelAgg {
            model:r.get(0),provider:r.get(1),requests:r.get(2),input_tokens:r.get(3),output_tokens:r.get(4),
            cache_read_tokens:r.get(5),cache_write_5m_tokens:r.get(6),cache_write_1h_tokens:r.get(7),
            web_search_requests:r.get(8),real_nano:r.get(9),charge_nano:r.get(10),
            input_nano:r.get(11),output_nano:r.get(12),cache_read_nano:r.get(13),
            cache_write_5m_nano:r.get(14),cache_write_1h_nano:r.get(15),web_search_nano:r.get(16),
        }).collect();
        let daily = transaction
            .query(
                "SELECT (ts / 86400) * 86400 AS day_ts, COUNT(*)::bigint, \
             COALESCE(SUM(real_nano),0)::bigint, COALESCE(SUM(charge_nano),0)::bigint \
             FROM usage_events WHERE account_id=$1 AND ts >= $2 AND ts < $3 \
             GROUP BY day_ts ORDER BY day_ts",
                &[&account_id, &since_ts, &until_ts],
            )?
            .into_iter()
            .map(|r| UsageDailyAgg {
                day_ts: r.get(0),
                requests: r.get(1),
                real_nano: r.get(2),
                charge_nano: r.get(3),
            })
            .collect();
        let daily_providers = transaction
            .query(
                "SELECT (ts / 86400) * 86400 AS day_ts, COALESCE(NULLIF(provider,''),'anthropic'), COUNT(*)::bigint, \
             COALESCE(SUM(real_nano),0)::bigint, COALESCE(SUM(charge_nano),0)::bigint \
             FROM usage_events WHERE account_id=$1 AND ts >= $2 AND ts < $3 \
             GROUP BY day_ts, COALESCE(NULLIF(provider,''),'anthropic') ORDER BY day_ts, COALESCE(NULLIF(provider,''),'anthropic')",
                &[&account_id, &since_ts, &until_ts],
            )?
            .into_iter()
            .map(|r| UsageDailyProviderAgg {
                day_ts: r.get(0),
                provider: r.get(1),
                requests: r.get(2),
                real_nano: r.get(3),
                charge_nano: r.get(4),
            })
            .collect();
        let keys = transaction
            .query(
                "SELECT key, COUNT(*)::bigint, COALESCE(SUM(real_nano),0)::bigint, \
             COALESCE(SUM(charge_nano),0)::bigint \
             FROM usage_events WHERE account_id=$1 AND ts >= $2 AND ts < $3 \
             GROUP BY key ORDER BY SUM(real_nano) DESC, key",
                &[&account_id, &since_ts, &until_ts],
            )?
            .into_iter()
            .map(|r| UsageKeyAgg {
                key: r.get(0),
                requests: r.get(1),
                real_nano: r.get(2),
                charge_nano: r.get(3),
            })
            .collect();
        transaction.commit()?;
        Ok(UsageReport {
            models,
            daily,
            daily_providers,
            keys,
        })
    }
    pub fn spend_by_account(&mut self, since_ts: i64, limit: i64) -> Result<Vec<SpendAccountAgg>> {
        self.spend_by_account_range(since_ts, i64::MAX, limit)
    }

    /// То же с явной верхней границей: полуоткрытое окно `since_ts ≤ ts < until_ts` (стыкующиеся
    /// диапазоны не задваивают события). Для произвольного диапазона панели (/spend-stats?from&to).
    pub fn spend_by_account_range(
        &mut self,
        since_ts: i64,
        until_ts: i64,
        limit: i64,
    ) -> Result<Vec<SpendAccountAgg>> {
        Ok(self
            .client
            .query(
                "SELECT u.account_id, COALESCE(a.handle,''), COUNT(*)::bigint, \
                 COALESCE(SUM(u.charge_nano),0)::bigint, COALESCE(SUM(u.real_nano),0)::bigint, \
                 COALESCE(MAX(u.ts),0)::bigint \
                 FROM usage_events u LEFT JOIN accounts a ON a.id=u.account_id \
                 WHERE u.ts>=$1 AND u.ts<$2 GROUP BY u.account_id, a.handle \
                 ORDER BY SUM(u.charge_nano) DESC LIMIT $3",
                &[&since_ts, &until_ts, &limit],
            )?
            .into_iter()
            .map(|r| SpendAccountAgg {
                account_id: r.get(0),
                handle: r.get(1),
                requests: r.get(2),
                charge_nano: r.get(3),
                real_nano: r.get(4),
                last_ts: r.get(5),
            })
            .collect())
    }
    pub fn spend_by_provider(&mut self, since_ts: i64) -> Result<Vec<SpendProviderAgg>> {
        self.spend_by_provider_range(since_ts, i64::MAX)
    }

    /// То же с явной верхней границей окна — см. spend_by_account_range.
    pub fn spend_by_provider_range(
        &mut self,
        since_ts: i64,
        until_ts: i64,
    ) -> Result<Vec<SpendProviderAgg>> {
        Ok(self
            .client
            .query(
                "SELECT COALESCE(NULLIF(provider,''),'anthropic'), COUNT(*)::bigint, \
                 COALESCE(SUM(charge_nano),0)::bigint, COALESCE(SUM(real_nano),0)::bigint \
                 FROM usage_events WHERE ts>=$1 AND ts<$2 GROUP BY 1 ORDER BY SUM(charge_nano) DESC",
                &[&since_ts, &until_ts],
            )?
            .into_iter()
            .map(|r| SpendProviderAgg {
                provider: r.get(0),
                requests: r.get(1),
                charge_nano: r.get(2),
                real_nano: r.get(3),
            })
            .collect())
    }

    /// Top-`limit` моделей по charge за окно — тот же источник, что и spend_by_provider.
    /// `model` — served id из ответа апстрима (по нему посчитан charge), см. lib.rs.
    pub fn spend_by_model(&mut self, since_ts: i64, limit: i64) -> Result<Vec<SpendModelAgg>> {
        self.spend_by_model_range(since_ts, i64::MAX, limit)
    }

    /// То же с явной верхней границей окна — см. spend_by_account_range.
    pub fn spend_by_model_range(
        &mut self,
        since_ts: i64,
        until_ts: i64,
        limit: i64,
    ) -> Result<Vec<SpendModelAgg>> {
        Ok(self
            .client
            .query(
                "SELECT COALESCE(NULLIF(model,''),'(unknown)'), \
                 COALESCE(NULLIF(provider,''),'anthropic'), COUNT(*)::bigint, \
                 COALESCE(SUM(charge_nano),0)::bigint, COALESCE(SUM(real_nano),0)::bigint \
                 FROM usage_events WHERE ts>=$1 AND ts<$2 GROUP BY 1,2 \
                 ORDER BY SUM(charge_nano) DESC, 1, 2 LIMIT $3",
                &[&since_ts, &until_ts, &limit],
            )?
            .into_iter()
            .map(|r| SpendModelAgg {
                model: r.get(0),
                provider: r.get(1),
                requests: r.get(2),
                charge_nano: r.get(3),
                real_nano: r.get(4),
            })
            .collect())
    }

    /// Сводка settlement pipeline для панели: counts по state, failed всего/24ч, backlog
    /// несеттленых старше `backlog_secs`, последние ≤10 failed (last_error урезан до 200
    /// символов — внутри только invariant/SQLSTATE детали, без секретов) и лаг durable
    /// ledger-консьюмера. Read-only, без транзакции: счётчики допускают мелкую гонку.
    pub fn settlement_health(
        &mut self,
        backlog_secs: i64,
        consumer: &str,
    ) -> Result<SettlementHealth> {
        let ts = now();
        let backlog_before = ts - backlog_secs.max(0);
        let failed_since = ts - 86_400;
        let mut health = SettlementHealth::default();
        for row in self.client.query(
            "SELECT state, COUNT(*)::bigint FROM settlement_outbox GROUP BY state",
            &[],
        )? {
            let state: String = row.get(0);
            let count: i64 = row.get(1);
            match state.as_str() {
                "pending" => health.pending = count,
                "processing" => health.processing = count,
                "done" => health.done = count,
                "failed" => health.failed = count,
                _ => {}
            }
        }
        health.failed_24h = self
            .client
            .query_one(
                "SELECT COUNT(*)::bigint FROM settlement_outbox \
                 WHERE state='failed' AND updated_ts>=$1",
                &[&failed_since],
            )?
            .get(0);
        health.pending_with_error = self
            .client
            .query_one(
                "SELECT COUNT(*)::bigint FROM settlement_outbox \
                 WHERE state='pending' AND last_error IS NOT NULL",
                &[],
            )?
            .get(0);
        health.backlog = self
            .client
            .query_one(
                "SELECT COUNT(*)::bigint FROM settlement_outbox \
                 WHERE state IN ('pending','processing') AND created_ts<$1",
                &[&backlog_before],
            )?
            .get(0);
        health.oldest_unsettled_ts = self
            .client
            .query_one(
                "SELECT COALESCE(MIN(created_ts),0)::bigint FROM settlement_outbox \
                 WHERE state IN ('pending','processing')",
                &[],
            )?
            .get(0);
        health.recent_failed = self
            .client
            .query(
                "SELECT request_id, actual_nano, attempts, last_error, updated_ts \
                 FROM settlement_outbox WHERE state='failed' \
                 ORDER BY updated_ts DESC, request_id LIMIT 10",
                &[],
            )?
            .into_iter()
            .map(|r| {
                let raw: Option<String> = r.get(3);
                SettlementFailure {
                    request_id: r.get(0),
                    actual_nano: r.get(1),
                    attempts: r.get(2),
                    last_error: raw.map(|e| e.chars().take(200).collect()),
                    updated_ts: r.get(4),
                }
            })
            .collect();
        let ledger_max_id: i64 = self
            .client
            .query_one("SELECT COALESCE(MAX(id),0)::bigint FROM ledger", &[])?
            .get(0);
        let lag_row = self.client.query_one(
            "SELECT COUNT(*)::bigint, COALESCE(MIN(last_ledger_id),0)::bigint \
             FROM ledger_consumer_checkpoints WHERE consumer=$1",
            &[&consumer],
        )?;
        let unacked_row = self.client.query_one(
            "SELECT COUNT(*)::bigint, COALESCE(MIN(l.ts),0)::bigint FROM ledger l \
             JOIN ledger_consumer_checkpoints c ON c.account_id=l.account_id AND c.consumer=$1 \
             WHERE l.id > c.last_ledger_id",
            &[&consumer],
        )?;
        health.ledger_consumer = LedgerConsumerLag {
            consumer: consumer.to_string(),
            ledger_max_id,
            checkpoints: lag_row.get(0),
            checkpoint_min: lag_row.get(1),
            unacked: unacked_row.get(0),
            oldest_unacked_ts: unacked_row.get(1),
        };
        Ok(health)
    }

    pub fn usage_prune(&mut self, older_than_ts: i64) -> Result<usize> {
        Ok(self.client.execute(
            "DELETE FROM usage_events WHERE id IN ( \
               SELECT id FROM usage_events WHERE ts < $1 ORDER BY id LIMIT 5000 \
             )",
            &[&older_than_ts],
        )? as usize)
    }

    /// Bounded lifecycle cleanup. Financial outcomes remain in ledger; transient request/lease
    /// machinery is removed only after it is terminal and older than the retention cutoff.
    pub fn maintenance_prune(&mut self, older_than_ts: i64) -> Result<MaintenanceReport> {
        crate::pricing::validate_request_lifecycle_prune_cutoff(older_than_ts, now())?;
        let mut tx = self.client.transaction()?;
        let outbox = tx.execute(
            "DELETE FROM settlement_outbox WHERE request_id IN ( \
               SELECT request_id FROM settlement_outbox \
               WHERE state='done' AND committed_ts < $1 \
               ORDER BY committed_ts,request_id LIMIT 5000 \
             )",
            &[&older_than_ts],
        )? as usize;
        let lifecycle_counts = tx.query_one(
            "WITH doomed AS MATERIALIZED ( \
               SELECT r.request_id FROM reservations r \
               WHERE r.state IN ('settled','canceled') AND r.settled_ts < $1 \
                 AND NOT EXISTS (SELECT 1 FROM settlement_outbox o WHERE o.request_id=r.request_id) \
               ORDER BY r.settled_ts,r.request_id LIMIT 5000 FOR UPDATE \
             ), child_counts AS MATERIALIZED ( \
               SELECT \
                 (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots s \
                   WHERE EXISTS (SELECT 1 FROM doomed d WHERE d.request_id=s.request_id)) \
                   AS pricing_snapshots, \
                 (SELECT COUNT(*)::bigint FROM pricing_shadow_admission_evaluations e \
                   WHERE EXISTS (SELECT 1 FROM doomed d WHERE d.request_id=e.request_id)) \
                   AS shadow_evaluations \
             ), deleted AS ( \
               DELETE FROM reservations r USING doomed d \
                WHERE r.request_id=d.request_id \
                RETURNING r.request_id \
             ) \
             SELECT \
               (SELECT COUNT(*)::bigint FROM deleted), \
               child_counts.pricing_snapshots, child_counts.shadow_evaluations \
              FROM child_counts",
            &[&older_than_ts],
        )?;
        let reservations = lifecycle_counts.get::<_, i64>(0) as usize;
        let pricing_snapshots_cascaded = lifecycle_counts.get::<_, i64>(1) as usize;
        let pricing_shadow_evaluations_cascaded = lifecycle_counts.get::<_, i64>(2) as usize;
        let capacity_leases = tx.execute(
            "DELETE FROM capacity_leases WHERE lease_id IN ( \
               SELECT lease_id FROM capacity_leases \
               WHERE state IN ('released','expired') AND released_ts < $1 \
               ORDER BY released_ts LIMIT 5000 \
             )",
            &[&older_than_ts],
        )? as usize;
        let engine_instances = tx.execute(
            "DELETE FROM engine_instances i WHERE lease_until < $1 \
               AND NOT EXISTS (SELECT 1 FROM reservations r WHERE r.owner_instance=i.instance_id \
                               AND r.owner_epoch=i.owner_epoch AND r.state NOT IN ('settled','canceled')) \
               AND NOT EXISTS (SELECT 1 FROM capacity_leases c WHERE c.owner_instance=i.instance_id \
                               AND c.owner_epoch=i.owner_epoch AND c.state='active')",
            &[&older_than_ts],
        )? as usize;
        tx.commit()?;
        Ok(MaintenanceReport {
            outbox,
            reservations,
            pricing_snapshots_cascaded,
            pricing_shadow_evaluations_cascaded,
            capacity_leases,
            engine_instances,
            ..MaintenanceReport::default()
        })
    }
    pub fn billing_totals(&mut self) -> Result<BillingTotals> {
        let row = self.client.query_one(
            "SELECT COALESCE(SUM(balance_nano),0)::bigint,COALESCE(SUM(spent_nano),0)::bigint, \
             COALESCE(SUM(reserved_nano),0)::bigint,COUNT(*) FILTER (WHERE status='active')::bigint FROM accounts",
            &[],
        )?;
        Ok(BillingTotals {
            balance_nano: row.get(0),
            spent_nano: row.get(1),
            reserved_nano: row.get(2),
            active_accounts: row.get(3),
        })
    }

    // -- Durable provider-turn and Claude capacity evidence ----------------------------------

    pub fn record_provider_turn_calibration_event(
        &mut self,
        event: &ProviderTurnCalibrationEvent,
    ) -> Result<ProviderCalibrationSubjectSpend> {
        crate::validate_provider_turn_calibration_event(event)?;
        let mut tx = self.client.transaction()?;
        let inserted = tx.execute(
            "INSERT INTO provider_turn_calibration_events(\
               provider,request_id,subject_id,model_id,service_tier,inference_geo,\
               tariff_schedule_id,priced_ts,completed_at,input_tokens,audio_input_tokens,\
               cache_read_tokens,cached_audio_input_tokens,cache_write_5m_tokens,\
               cache_write_1h_tokens,output_tokens,thinking_output_tokens,image_output_tokens,\
               tool_prompt_tokens,search_queries,grounded_search_prompts,api_input_nanousd,\
               api_audio_input_nanousd,api_cache_read_nanousd,api_cached_audio_input_nanousd,\
               api_cache_write_5m_nanousd,api_cache_write_1h_nanousd,api_output_nanousd,\
               api_image_output_nanousd,api_search_nanousd,api_total_nanousd) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,\
                    $19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31) \
             ON CONFLICT(provider,request_id) DO NOTHING",
            &[
                &event.provider,
                &event.request_id,
                &event.subject_id,
                &event.model_id,
                &event.service_tier,
                &event.inference_geo,
                &event.tariff_schedule_id,
                &event.priced_ts,
                &event.completed_at,
                &event.input_tokens,
                &event.audio_input_tokens,
                &event.cache_read_tokens,
                &event.cached_audio_input_tokens,
                &event.cache_write_5m_tokens,
                &event.cache_write_1h_tokens,
                &event.output_tokens,
                &event.thinking_output_tokens,
                &event.image_output_tokens,
                &event.tool_prompt_tokens,
                &event.search_queries,
                &event.grounded_search_prompts,
                &event.api_input_nanousd,
                &event.api_audio_input_nanousd,
                &event.api_cache_read_nanousd,
                &event.api_cached_audio_input_nanousd,
                &event.api_cache_write_5m_nanousd,
                &event.api_cache_write_1h_nanousd,
                &event.api_output_nanousd,
                &event.api_image_output_nanousd,
                &event.api_search_nanousd,
                &event.api_total_nanousd,
            ],
        )? == 1;
        if inserted {
            tx.execute(
                "INSERT INTO provider_calibration_subject_spend(\
                   provider,subject_id,spent_nano,tracking_started_ts,updated_ts) \
                 VALUES($1,$2,$3,$4,$4) ON CONFLICT(provider,subject_id) DO UPDATE SET \
                   spent_nano=provider_calibration_subject_spend.spent_nano+EXCLUDED.spent_nano, \
                   tracking_started_ts=LEAST(\
                       provider_calibration_subject_spend.tracking_started_ts,\
                       EXCLUDED.tracking_started_ts), \
                   updated_ts=GREATEST(\
                       provider_calibration_subject_spend.updated_ts,EXCLUDED.updated_ts)",
                &[
                    &event.provider,
                    &event.subject_id,
                    &event.api_total_nanousd,
                    &event.completed_at,
                ],
            )?;
        } else {
            let row = tx.query_one(
                &format!(
                    "SELECT {} FROM provider_turn_calibration_events \
                     WHERE provider=$1 AND request_id=$2",
                    crate::PROVIDER_TURN_EVENT_COLUMNS
                ),
                &[&event.provider, &event.request_id],
            )?;
            if pg_provider_turn_event(&row) != *event {
                return Err(crate::ProviderTurnCalibrationReplayConflict.into());
            }
        }
        let row = tx.query_one(
            "SELECT spent_nano,tracking_started_ts,updated_ts \
             FROM provider_calibration_subject_spend WHERE provider=$1 AND subject_id=$2",
            &[&event.provider, &event.subject_id],
        )?;
        let spend = ProviderCalibrationSubjectSpend {
            spent_nano: row.get(0),
            tracking_started_ts: Some(row.get(1)),
            updated_ts: Some(row.get(2)),
            inserted,
        };
        tx.commit()?;
        Ok(spend)
    }

    pub fn provider_calibration_subject_spend(
        &mut self,
        provider: &str,
        subject_id: &str,
    ) -> Result<ProviderCalibrationSubjectSpend> {
        if !matches!(provider, crate::PROVIDER_ANTHROPIC | crate::PROVIDER_GOOGLE)
            || subject_id.is_empty()
        {
            bail!("invalid provider calibration subject");
        }
        Ok(self
            .client
            .query_opt(
                "SELECT spent_nano,tracking_started_ts,updated_ts \
                 FROM provider_calibration_subject_spend WHERE provider=$1 AND subject_id=$2",
                &[&provider, &subject_id],
            )?
            .map(|row| ProviderCalibrationSubjectSpend {
                spent_nano: row.get(0),
                tracking_started_ts: Some(row.get(1)),
                updated_ts: Some(row.get(2)),
                inserted: false,
            })
            .unwrap_or_default())
    }

    pub fn provider_turn_calibration_report(
        &mut self,
        provider: &str,
    ) -> Result<Vec<ProviderTurnCalibrationAggregate>> {
        if !matches!(provider, crate::PROVIDER_ANTHROPIC | crate::PROVIDER_GOOGLE) {
            bail!("invalid provider calibration report provider");
        }
        Ok(self
            .client
            .query(
                "SELECT provider,subject_id,model_id,service_tier,inference_geo,\
                   tariff_schedule_id,COUNT(*)::bigint,MIN(completed_at),MAX(completed_at),\
                   SUM(input_tokens)::bigint,SUM(audio_input_tokens)::bigint,\
                   SUM(cache_read_tokens)::bigint,SUM(cached_audio_input_tokens)::bigint,\
                   SUM(cache_write_5m_tokens)::bigint,SUM(cache_write_1h_tokens)::bigint,\
                   SUM(output_tokens)::bigint,SUM(thinking_output_tokens)::bigint,\
                   SUM(image_output_tokens)::bigint,SUM(tool_prompt_tokens)::bigint,\
                   SUM(search_queries)::bigint,SUM(grounded_search_prompts)::bigint,\
                   SUM(api_input_nanousd)::bigint,SUM(api_audio_input_nanousd)::bigint,\
                   SUM(api_cache_read_nanousd)::bigint,\
                   SUM(api_cached_audio_input_nanousd)::bigint,\
                   SUM(api_cache_write_5m_nanousd)::bigint,\
                   SUM(api_cache_write_1h_nanousd)::bigint,SUM(api_output_nanousd)::bigint,\
                   SUM(api_image_output_nanousd)::bigint,SUM(api_search_nanousd)::bigint,\
                   SUM(api_total_nanousd)::bigint FROM provider_turn_calibration_events \
                 WHERE provider=$1 \
                 GROUP BY provider,subject_id,model_id,service_tier,inference_geo,\
                   tariff_schedule_id \
                 ORDER BY subject_id,model_id,service_tier,inference_geo,tariff_schedule_id",
                &[&provider],
            )?
            .into_iter()
            .map(|row| ProviderTurnCalibrationAggregate {
                provider: row.get(0),
                subject_id: row.get(1),
                model_id: row.get(2),
                service_tier: row.get(3),
                inference_geo: row.get(4),
                tariff_schedule_id: row.get(5),
                turns: row.get(6),
                first_completed_at: row.get(7),
                last_completed_at: row.get(8),
                input_tokens: row.get(9),
                audio_input_tokens: row.get(10),
                cache_read_tokens: row.get(11),
                cached_audio_input_tokens: row.get(12),
                cache_write_5m_tokens: row.get(13),
                cache_write_1h_tokens: row.get(14),
                output_tokens: row.get(15),
                thinking_output_tokens: row.get(16),
                image_output_tokens: row.get(17),
                tool_prompt_tokens: row.get(18),
                search_queries: row.get(19),
                grounded_search_prompts: row.get(20),
                api_input_nanousd: row.get(21),
                api_audio_input_nanousd: row.get(22),
                api_cache_read_nanousd: row.get(23),
                api_cached_audio_input_nanousd: row.get(24),
                api_cache_write_5m_nanousd: row.get(25),
                api_cache_write_1h_nanousd: row.get(26),
                api_output_nanousd: row.get(27),
                api_image_output_nanousd: row.get(28),
                api_search_nanousd: row.get(29),
                api_total_nanousd: row.get(30),
            })
            .collect())
    }

    pub fn load_anthropic_calibration(
        &mut self,
        subject_id: &str,
        plan: &str,
        window_kind: &str,
    ) -> Result<Option<AnthropicCalibrationRow>> {
        let row = self.client.query_opt(
            &format!(
                "SELECT {} FROM anthropic_window_calibrations \
                 WHERE subject_id=$1 AND plan=$2 AND window_kind=$3",
                crate::ANTHROPIC_CALIBRATION_COLUMNS
            ),
            &[&subject_id, &plan, &window_kind],
        )?;
        let row = row.as_ref().map(pg_anthropic_calibration_row);
        if let Some(row) = &row {
            crate::validate_anthropic_calibration_row(row)?;
        }
        Ok(row)
    }

    pub fn list_anthropic_calibrations(&mut self) -> Result<Vec<AnthropicCalibrationRow>> {
        let rows = self
            .client
            .query(
                &format!(
                    "SELECT {} FROM anthropic_window_calibrations \
                     ORDER BY subject_id,plan,window_kind",
                    crate::ANTHROPIC_CALIBRATION_COLUMNS
                ),
                &[],
            )?
            .iter()
            .map(pg_anthropic_calibration_row)
            .collect::<Vec<_>>();
        for row in &rows {
            crate::validate_anthropic_calibration_row(row)?;
        }
        Ok(rows)
    }

    pub fn load_anthropic_window_observations(
        &mut self,
        subject_id: &str,
        plan: &str,
        window_kind: &str,
    ) -> Result<Vec<AnthropicWindowObservation>> {
        let rows = self.client.query(
            "SELECT subject_id,plan,window_kind,window_duration_mins,resets_at,observed_at,\
               used_fraction_units,measurement_resolution_fraction_units,gateway_spend_nano,\
               observation_source,source_request_id FROM anthropic_window_observations \
             WHERE subject_id=$1 AND plan=$2 AND window_kind=$3 ORDER BY observed_at,id",
            &[&subject_id, &plan, &window_kind],
        )?;
        let rows = rows
            .into_iter()
            .map(|row| AnthropicWindowObservation {
                subject_id: row.get(0),
                plan: row.get(1),
                window_kind: row.get(2),
                window_duration_mins: row.get(3),
                resets_at: row.get(4),
                observed_at: row.get(5),
                used_fraction_units: row.get(6),
                measurement_resolution_fraction_units: row.get(7),
                gateway_spend_nano: row.get(8),
                observation_source: row.get(9),
                source_request_id: row.get(10),
            })
            .collect::<Vec<_>>();
        for row in &rows {
            crate::validate_anthropic_window_observation(row)?;
        }
        Ok(rows)
    }

    pub fn save_anthropic_calibration(
        &mut self,
        state: &AnthropicCalibrationRow,
        observation: &AnthropicWindowObservation,
    ) -> Result<Option<i64>> {
        crate::validate_anthropic_calibration_pair(state, observation)?;
        let mut tx = self.client.transaction()?;
        let values: &[&(dyn postgres::types::ToSql + Sync)] = &[
            &state.subject_id,
            &state.plan,
            &state.window_kind,
            &state.window_duration_mins,
            &state.resets_at,
            &state.anchor_used_fraction_units,
            &state.anchor_resolution_fraction_units,
            &state.anchor_spend_nano,
            &state.used_fraction_units,
            &state.measurement_resolution_fraction_units,
            &state.observed_at,
            &state.observed_fraction_units,
            &state.observed_spend_nano,
            &state.samples,
            &state.unattributed_fraction_units,
            &state.current_capacity_nano,
            &state.current_low_nano,
            &state.current_high_nano,
            &state.current_confidence_bp,
            &state.last_measured_at,
            &state.estimator_version,
            &state.version,
            &state.updated_ts,
        ];
        let changed = if state.version == 0 {
            tx.execute(ANTHROPIC_CALIBRATION_INSERT_SQL, values)?
        } else {
            tx.execute(
                "UPDATE anthropic_window_calibrations SET \
                   window_duration_mins=$4,resets_at=$5,anchor_used_fraction_units=$6,\
                   anchor_resolution_fraction_units=$7,anchor_spend_nano=$8,\
                   used_fraction_units=$9,measurement_resolution_fraction_units=$10,\
                   observed_at=$11,observed_fraction_units=$12,observed_spend_nano=$13,\
                   samples=$14,unattributed_fraction_units=$15,current_capacity_nano=$16,\
                   current_low_nano=$17,current_high_nano=$18,current_confidence_bp=$19,\
                   last_measured_at=$20,estimator_version=$21,version=version+1,updated_ts=$23 \
                 WHERE subject_id=$1 AND plan=$2 AND window_kind=$3 AND version=$22",
                values,
            )?
        };
        if changed == 0 {
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO anthropic_window_observations(\
               subject_id,plan,window_kind,window_duration_mins,resets_at,observed_at,\
               used_fraction_units,measurement_resolution_fraction_units,gateway_spend_nano,\
               observation_source,source_request_id) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT DO NOTHING",
            &[
                &observation.subject_id,
                &observation.plan,
                &observation.window_kind,
                &observation.window_duration_mins,
                &observation.resets_at,
                &observation.observed_at,
                &observation.used_fraction_units,
                &observation.measurement_resolution_fraction_units,
                &observation.gateway_spend_nano,
                &observation.observation_source,
                &observation.source_request_id,
            ],
        )?;
        tx.commit()?;
        Ok(Some(state.version.saturating_add(1)))
    }

    // -- Durable OpenAI/Codex capacity evidence ----------------------------------------------

    pub fn credit_codex_home_spend(
        &mut self,
        home_id: &str,
        delta_nano: i64,
        updated_ts: i64,
    ) -> Result<i64> {
        if home_id.is_empty() || delta_nano < 0 || updated_ts <= 0 {
            bail!("invalid Codex home spend credit");
        }
        Ok(self
            .client
            .query_one(
                "INSERT INTO codex_home_spend(home_id,spent_nano,updated_ts) VALUES($1,$2,$3) \
                 ON CONFLICT(home_id) DO UPDATE SET \
                   spent_nano=codex_home_spend.spent_nano+EXCLUDED.spent_nano, \
                   updated_ts=EXCLUDED.updated_ts RETURNING spent_nano",
                &[&home_id, &delta_nano, &updated_ts],
            )?
            .get(0))
    }

    pub fn codex_home_spend(&mut self, home_id: &str) -> Result<i64> {
        Ok(self
            .client
            .query_opt(
                "SELECT spent_nano FROM codex_home_spend WHERE home_id=$1",
                &[&home_id],
            )?
            .map(|row| row.get(0))
            .unwrap_or(0))
    }

    pub fn record_codex_turn_calibration_event(
        &mut self,
        event: &CodexTurnCalibrationEvent,
    ) -> Result<CodexHomeCalibrationSpend> {
        crate::validate_codex_turn_calibration_event(event)?;
        let mut tx = self.client.transaction()?;
        let inserted = tx.execute(
            "INSERT INTO codex_turn_calibration_events(\
               request_id,home_id,model_id,service_tier,provider_reported_tier,\
               api_tariff_schedule_id,credit_schedule_id,completed_at,input_tokens,\
               cached_input_tokens,cache_write_input_tokens,output_tokens,reasoning_output_tokens,\
               api_input_nanousd,api_cached_input_nanousd,api_cache_write_nanousd,\
               api_output_nanousd,api_total_nanousd,chatgpt_input_nanocredits,\
               chatgpt_cached_input_nanocredits,chatgpt_output_nanocredits,\
               chatgpt_total_nanocredits) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,\
                    $19,$20,$21,$22) ON CONFLICT(request_id) DO NOTHING",
            &[
                &event.request_id,
                &event.home_id,
                &event.model_id,
                &event.service_tier,
                &event.provider_reported_tier,
                &event.api_tariff_schedule_id,
                &event.credit_schedule_id,
                &event.completed_at,
                &event.input_tokens,
                &event.cached_input_tokens,
                &event.cache_write_input_tokens,
                &event.output_tokens,
                &event.reasoning_output_tokens,
                &event.api_input_nanousd,
                &event.api_cached_input_nanousd,
                &event.api_cache_write_nanousd,
                &event.api_output_nanousd,
                &event.api_total_nanousd,
                &event.chatgpt_input_nanocredits,
                &event.chatgpt_cached_input_nanocredits,
                &event.chatgpt_output_nanocredits,
                &event.chatgpt_total_nanocredits,
            ],
        )? == 1;
        if inserted {
            tx.execute(
                "INSERT INTO codex_home_spend(\
                   home_id,spent_nano,spent_nanocredits,credit_tracking_started_ts,updated_ts) \
                 VALUES($1,$2,$3,$4,$4) ON CONFLICT(home_id) DO UPDATE SET \
                   spent_nano=codex_home_spend.spent_nano+EXCLUDED.spent_nano, \
                   spent_nanocredits=COALESCE(codex_home_spend.spent_nanocredits,0)\
                       +EXCLUDED.spent_nanocredits, \
                   credit_tracking_started_ts=COALESCE(\
                       codex_home_spend.credit_tracking_started_ts,EXCLUDED.credit_tracking_started_ts), \
                   updated_ts=GREATEST(codex_home_spend.updated_ts,EXCLUDED.updated_ts)",
                &[
                    &event.home_id,
                    &event.api_total_nanousd,
                    &event.chatgpt_total_nanocredits,
                    &event.completed_at,
                ],
            )?;
        } else {
            let row = tx.query_one(
                "SELECT request_id,home_id,model_id,service_tier,provider_reported_tier,\
                   api_tariff_schedule_id,credit_schedule_id,completed_at,input_tokens,\
                   cached_input_tokens,cache_write_input_tokens,output_tokens,\
                   reasoning_output_tokens,api_input_nanousd,api_cached_input_nanousd,\
                   api_cache_write_nanousd,api_output_nanousd,api_total_nanousd,\
                   chatgpt_input_nanocredits,chatgpt_cached_input_nanocredits,\
                   chatgpt_output_nanocredits,chatgpt_total_nanocredits \
                 FROM codex_turn_calibration_events WHERE request_id=$1",
                &[&event.request_id],
            )?;
            let existing = CodexTurnCalibrationEvent {
                request_id: row.get(0),
                home_id: row.get(1),
                model_id: row.get(2),
                service_tier: row.get(3),
                provider_reported_tier: row.get(4),
                api_tariff_schedule_id: row.get(5),
                credit_schedule_id: row.get(6),
                completed_at: row.get(7),
                input_tokens: row.get(8),
                cached_input_tokens: row.get(9),
                cache_write_input_tokens: row.get(10),
                output_tokens: row.get(11),
                reasoning_output_tokens: row.get(12),
                api_input_nanousd: row.get(13),
                api_cached_input_nanousd: row.get(14),
                api_cache_write_nanousd: row.get(15),
                api_output_nanousd: row.get(16),
                api_total_nanousd: row.get(17),
                chatgpt_input_nanocredits: row.get(18),
                chatgpt_cached_input_nanocredits: row.get(19),
                chatgpt_output_nanocredits: row.get(20),
                chatgpt_total_nanocredits: row.get(21),
            };
            if existing != *event {
                return Err(crate::CodexTurnCalibrationReplayConflict.into());
            }
        }
        let row = tx.query_opt(
            "SELECT spent_nano,spent_nanocredits,credit_tracking_started_ts \
             FROM codex_home_spend WHERE home_id=$1",
            &[&event.home_id],
        )?;
        let mut totals = row
            .map(|row| CodexHomeCalibrationSpend {
                spent_nano: row.get(0),
                spent_nanocredits: row.get(1),
                credit_tracking_started_ts: row.get(2),
                inserted: false,
            })
            .unwrap_or_default();
        totals.inserted = inserted;
        tx.commit()?;
        Ok(totals)
    }

    pub fn codex_home_calibration_spend(
        &mut self,
        home_id: &str,
    ) -> Result<CodexHomeCalibrationSpend> {
        Ok(self
            .client
            .query_opt(
                "SELECT spent_nano,spent_nanocredits,credit_tracking_started_ts \
                 FROM codex_home_spend WHERE home_id=$1",
                &[&home_id],
            )?
            .map(|row| CodexHomeCalibrationSpend {
                spent_nano: row.get(0),
                spent_nanocredits: row.get(1),
                credit_tracking_started_ts: row.get(2),
                inserted: false,
            })
            .unwrap_or_default())
    }

    pub fn codex_turn_calibration_report(&mut self) -> Result<Vec<CodexTurnCalibrationAggregate>> {
        Ok(self
            .client
            .query(
                "SELECT home_id,model_id,service_tier,provider_reported_tier,\
                   api_tariff_schedule_id,credit_schedule_id,COUNT(*)::bigint,\
                   MIN(completed_at),MAX(completed_at),SUM(input_tokens)::bigint,\
                   SUM(cached_input_tokens)::bigint,SUM(cache_write_input_tokens)::bigint,\
                   SUM(output_tokens)::bigint,SUM(reasoning_output_tokens)::bigint,\
                   SUM(api_input_nanousd)::bigint,SUM(api_cached_input_nanousd)::bigint,\
                   SUM(api_cache_write_nanousd)::bigint,SUM(api_output_nanousd)::bigint,\
                   SUM(api_total_nanousd)::bigint,SUM(chatgpt_input_nanocredits)::bigint,\
                   SUM(chatgpt_cached_input_nanocredits)::bigint,\
                   SUM(chatgpt_output_nanocredits)::bigint,SUM(chatgpt_total_nanocredits)::bigint \
                 FROM codex_turn_calibration_events \
                 GROUP BY home_id,model_id,service_tier,provider_reported_tier,\
                   api_tariff_schedule_id,credit_schedule_id \
                 ORDER BY home_id,model_id,service_tier,provider_reported_tier",
                &[],
            )?
            .into_iter()
            .map(|row| CodexTurnCalibrationAggregate {
                home_id: row.get(0),
                model_id: row.get(1),
                service_tier: row.get(2),
                provider_reported_tier: row.get(3),
                api_tariff_schedule_id: row.get(4),
                credit_schedule_id: row.get(5),
                turns: row.get(6),
                first_completed_at: row.get(7),
                last_completed_at: row.get(8),
                input_tokens: row.get(9),
                cached_input_tokens: row.get(10),
                cache_write_input_tokens: row.get(11),
                output_tokens: row.get(12),
                reasoning_output_tokens: row.get(13),
                api_input_nanousd: row.get(14),
                api_cached_input_nanousd: row.get(15),
                api_cache_write_nanousd: row.get(16),
                api_output_nanousd: row.get(17),
                api_total_nanousd: row.get(18),
                chatgpt_input_nanocredits: row.get(19),
                chatgpt_cached_input_nanocredits: row.get(20),
                chatgpt_output_nanocredits: row.get(21),
                chatgpt_total_nanocredits: row.get(22),
            })
            .collect())
    }

    // -- Durable Gemini capacity evidence ----------------------------------------------------

    pub fn credit_gemini_profile_spend(
        &mut self,
        profile_id: &str,
        delta_nano: i64,
        updated_ts: i64,
    ) -> Result<i64> {
        if profile_id.is_empty() || delta_nano < 0 || updated_ts <= 0 {
            bail!("invalid Gemini profile spend credit");
        }
        Ok(self
            .client
            .query_one(
                "INSERT INTO gemini_profile_spend(profile_id,spent_nano,updated_ts) \
                 VALUES($1,$2,$3) ON CONFLICT(profile_id) DO UPDATE SET \
                   spent_nano=gemini_profile_spend.spent_nano+EXCLUDED.spent_nano, \
                   updated_ts=EXCLUDED.updated_ts RETURNING spent_nano",
                &[&profile_id, &delta_nano, &updated_ts],
            )?
            .get(0))
    }

    pub fn gemini_profile_spend(&mut self, profile_id: &str) -> Result<i64> {
        Ok(self
            .client
            .query_opt(
                "SELECT spent_nano FROM gemini_profile_spend WHERE profile_id=$1",
                &[&profile_id],
            )?
            .map(|row| row.get(0))
            .unwrap_or(0))
    }

    pub fn save_codex_home_health(
        &mut self,
        home_id: &str,
        row: &crate::CodexHomeHealthRow,
        updated_ts: i64,
    ) -> Result<()> {
        self.client.execute(
            "INSERT INTO codex_home_health(\
               home_id,account_state,auth_fail_streak,first_auth_fail_ts,cooling_until,updated_ts) \
             VALUES($1,$2,$3,$4,$5,$6) \
             ON CONFLICT(home_id) DO UPDATE SET \
               account_state=EXCLUDED.account_state, \
               auth_fail_streak=EXCLUDED.auth_fail_streak, \
               first_auth_fail_ts=EXCLUDED.first_auth_fail_ts, \
               cooling_until=EXCLUDED.cooling_until, \
               updated_ts=EXCLUDED.updated_ts",
            &[
                &home_id,
                &row.account_state,
                &row.auth_fail_streak,
                &row.first_auth_fail_ts,
                &row.cooling_until,
                &updated_ts,
            ],
        )?;
        Ok(())
    }

    /// A home with no stored verdict starts healthy: absence of evidence is not evidence of fault.
    pub fn load_codex_home_health(&mut self, home_id: &str) -> Result<crate::CodexHomeHealthRow> {
        Ok(self
            .client
            .query_opt(
                "SELECT account_state,auth_fail_streak,first_auth_fail_ts,cooling_until \
                 FROM codex_home_health WHERE home_id=$1",
                &[&home_id],
            )?
            .map(|row| crate::CodexHomeHealthRow {
                account_state: row.get(0),
                auth_fail_streak: row.get(1),
                first_auth_fail_ts: row.get(2),
                cooling_until: row.get(3),
            })
            .unwrap_or_default())
    }

    pub fn load_codex_calibration(
        &mut self,
        home_id: &str,
        window_duration_mins: i64,
    ) -> Result<Option<CodexCalibrationRow>> {
        Ok(self
            .client
            .query_opt(
                "SELECT home_id,window_duration_mins,resets_at,anchor_used_percent,\
                   anchor_spend_nano,used_percent,observed_at,sum_used_sq,sum_used_spend_nano,\
                   observed_points,samples,current_capacity_nano,current_low_nano,current_high_nano,\
                   current_confidence_bp,last_capacity_nano,last_low_nano,last_high_nano,\
                   last_confidence_bp,last_measured_at,estimator_version,version,updated_ts,\
                   anchor_ready,\
                   COALESCE(anchor_used_fraction_units,anchor_used_percent*1000000),\
                   COALESCE(used_fraction_units,used_percent*1000000),\
                   COALESCE(observed_fraction_units,observed_points*1000000),\
                   COALESCE(observed_spend_nano,0),anchor_spend_nanocredits,\
                   observed_spend_nanocredits,current_capacity_nanocredits,\
                   current_low_nanocredits,current_high_nanocredits,last_capacity_nanocredits,\
                   last_low_nanocredits,last_high_nanocredits,credit_samples,\
                   credit_estimator_version,unattributed_fraction_units \
                 FROM codex_window_calibrations WHERE home_id=$1 AND window_duration_mins=$2",
                &[&home_id, &window_duration_mins],
            )?
            .map(|row| CodexCalibrationRow {
                home_id: row.get(0),
                window_duration_mins: row.get(1),
                resets_at: row.get(2),
                anchor_used_percent: row.get(3),
                anchor_spend_nano: row.get(4),
                used_percent: row.get(5),
                observed_at: row.get(6),
                sum_used_sq: row.get(7),
                sum_used_spend_nano: row.get(8),
                observed_points: row.get(9),
                samples: row.get(10),
                current_capacity_nano: row.get(11),
                current_low_nano: row.get(12),
                current_high_nano: row.get(13),
                current_confidence_bp: row.get(14),
                last_capacity_nano: row.get(15),
                last_low_nano: row.get(16),
                last_high_nano: row.get(17),
                last_confidence_bp: row.get(18),
                last_measured_at: row.get(19),
                estimator_version: row.get(20),
                version: row.get(21),
                updated_ts: row.get(22),
                anchor_ready: row.get(23),
                anchor_used_fraction_units: row.get(24),
                used_fraction_units: row.get(25),
                observed_fraction_units: row.get(26),
                observed_spend_nano: row.get(27),
                anchor_spend_nanocredits: row.get(28),
                observed_spend_nanocredits: row.get(29),
                current_capacity_nanocredits: row.get(30),
                current_low_nanocredits: row.get(31),
                current_high_nanocredits: row.get(32),
                last_capacity_nanocredits: row.get(33),
                last_low_nanocredits: row.get(34),
                last_high_nanocredits: row.get(35),
                credit_samples: row.get(36),
                credit_estimator_version: row.get(37),
                unattributed_fraction_units: row.get(38),
            }))
    }

    /// Load immutable raw evidence for a one-time estimator-version rebuild.
    pub fn load_codex_window_observations(
        &mut self,
        home_id: &str,
        window_duration_mins: i64,
    ) -> Result<Vec<CodexWindowObservation>> {
        Ok(self
            .client
            .query(
                "SELECT home_id,window_duration_mins,resets_at,observed_at,used_percent,\
                   COALESCE(used_fraction_units,used_percent*1000000),gateway_spend_nano,\
                   gateway_spend_nanocredits \
                 FROM codex_window_observations \
                 WHERE home_id=$1 AND window_duration_mins=$2 ORDER BY observed_at,id",
                &[&home_id, &window_duration_mins],
            )?
            .into_iter()
            .map(|row| CodexWindowObservation {
                home_id: row.get(0),
                window_duration_mins: row.get(1),
                resets_at: row.get(2),
                observed_at: row.get(3),
                used_percent: row.get(4),
                used_fraction_units: row.get(5),
                gateway_spend_nano: row.get(6),
                gateway_spend_nanocredits: row.get(7),
            })
            .collect())
    }

    /// Save calibration evidence with optimistic concurrency. A conflict returns `None` and rolls
    /// back the raw observation together with the stale derived row.
    pub fn save_codex_calibration(
        &mut self,
        state: &CodexCalibrationRow,
        observation: &CodexWindowObservation,
    ) -> Result<Option<i64>> {
        let mut tx = self.client.transaction()?;
        let version = if state.version == 0 {
            tx.query_opt(
                "INSERT INTO codex_window_calibrations( \
                   home_id,window_duration_mins,resets_at,anchor_used_percent,anchor_spend_nano,\
                   used_percent,observed_at,sum_used_sq,sum_used_spend_nano,observed_points,samples,\
                   current_capacity_nano,current_low_nano,current_high_nano,current_confidence_bp,\
                   last_capacity_nano,last_low_nano,last_high_nano,last_confidence_bp,last_measured_at,\
                   estimator_version,updated_ts,version,anchor_ready,anchor_used_fraction_units,\
                   used_fraction_units,observed_fraction_units,observed_spend_nano,\
                   anchor_spend_nanocredits,observed_spend_nanocredits,\
                   current_capacity_nanocredits,current_low_nanocredits,current_high_nanocredits,\
                   last_capacity_nanocredits,last_low_nanocredits,last_high_nanocredits,\
                   credit_samples,credit_estimator_version,unattributed_fraction_units \
                 ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,\
                          $19,$20,$21,$22,1,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,\
                          $34,$35,$36,$37,$38) \
                 ON CONFLICT(home_id,window_duration_mins) DO NOTHING RETURNING version",
                &[&state.home_id,&state.window_duration_mins,&state.resets_at,
                  &state.anchor_used_percent,&state.anchor_spend_nano,&state.used_percent,
                  &state.observed_at,&state.sum_used_sq,&state.sum_used_spend_nano,
                  &state.observed_points,&state.samples,&state.current_capacity_nano,
                  &state.current_low_nano,&state.current_high_nano,&state.current_confidence_bp,
                  &state.last_capacity_nano,&state.last_low_nano,&state.last_high_nano,
                  &state.last_confidence_bp,&state.last_measured_at,&state.estimator_version,
                  &state.updated_ts,&state.anchor_ready,&state.anchor_used_fraction_units,
                  &state.used_fraction_units,&state.observed_fraction_units,
                  &state.observed_spend_nano,&state.anchor_spend_nanocredits,
                  &state.observed_spend_nanocredits,&state.current_capacity_nanocredits,
                  &state.current_low_nanocredits,&state.current_high_nanocredits,
                  &state.last_capacity_nanocredits,&state.last_low_nanocredits,
                  &state.last_high_nanocredits,&state.credit_samples,
                  &state.credit_estimator_version,&state.unattributed_fraction_units],
            )?
        } else {
            tx.query_opt(
                "UPDATE codex_window_calibrations SET \
                   resets_at=$3,anchor_used_percent=$4,anchor_spend_nano=$5,used_percent=$6,\
                   observed_at=$7,sum_used_sq=$8,sum_used_spend_nano=$9,observed_points=$10,\
                   samples=$11,current_capacity_nano=$12,current_low_nano=$13,current_high_nano=$14,\
                   current_confidence_bp=$15,last_capacity_nano=$16,last_low_nano=$17,\
                   last_high_nano=$18,last_confidence_bp=$19,last_measured_at=$20,\
                   estimator_version=$21,updated_ts=$22,version=version+1,anchor_ready=$24,\
                   anchor_used_fraction_units=$25,used_fraction_units=$26,\
                   observed_fraction_units=$27,observed_spend_nano=$28,\
                   anchor_spend_nanocredits=$29,observed_spend_nanocredits=$30,\
                   current_capacity_nanocredits=$31,current_low_nanocredits=$32,\
                   current_high_nanocredits=$33,last_capacity_nanocredits=$34,\
                   last_low_nanocredits=$35,last_high_nanocredits=$36,credit_samples=$37,\
                   credit_estimator_version=$38,unattributed_fraction_units=$39 \
                 WHERE home_id=$1 AND window_duration_mins=$2 AND version=$23 RETURNING version",
                &[&state.home_id,&state.window_duration_mins,&state.resets_at,
                  &state.anchor_used_percent,&state.anchor_spend_nano,&state.used_percent,
                  &state.observed_at,&state.sum_used_sq,&state.sum_used_spend_nano,
                  &state.observed_points,&state.samples,&state.current_capacity_nano,
                  &state.current_low_nano,&state.current_high_nano,&state.current_confidence_bp,
                  &state.last_capacity_nano,&state.last_low_nano,&state.last_high_nano,
                  &state.last_confidence_bp,&state.last_measured_at,&state.estimator_version,
                  &state.updated_ts,&state.version,&state.anchor_ready,
                  &state.anchor_used_fraction_units,&state.used_fraction_units,
                  &state.observed_fraction_units,&state.observed_spend_nano,
                  &state.anchor_spend_nanocredits,&state.observed_spend_nanocredits,
                  &state.current_capacity_nanocredits,&state.current_low_nanocredits,
                  &state.current_high_nanocredits,&state.last_capacity_nanocredits,
                  &state.last_low_nanocredits,&state.last_high_nanocredits,
                  &state.credit_samples,&state.credit_estimator_version,
                  &state.unattributed_fraction_units],
            )?
        };
        let Some(version) = version.map(|row| row.get::<_, i64>(0)) else {
            return Ok(None);
        };
        tx.execute(
            "INSERT INTO codex_window_observations( \
               home_id,window_duration_mins,resets_at,observed_at,used_percent,\
               used_fraction_units,gateway_spend_nano,gateway_spend_nanocredits \
             ) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING",
            &[
                &observation.home_id,
                &observation.window_duration_mins,
                &observation.resets_at,
                &observation.observed_at,
                &observation.used_percent,
                &observation.used_fraction_units,
                &observation.gateway_spend_nano,
                &observation.gateway_spend_nanocredits,
            ],
        )?;
        tx.commit()?;
        Ok(Some(version))
    }

    pub fn load_gemini_calibration(
        &mut self,
        profile_id: &str,
        bucket_id: &str,
    ) -> Result<Option<GeminiCalibrationRow>> {
        let row = self.client.query_opt(
            "SELECT profile_id,bucket_id,window_kind,window_duration_mins,resets_at,\
               anchor_used_fraction_units,anchor_spend_nano,anchor_ready,used_fraction_units,\
               observed_at,sum_used_sq::text,sum_used_spend_nano::text,\
               observed_fraction_units,observed_spend_nano,samples,current_capacity_nano,current_low_nano,\
               current_high_nano,current_confidence_bp,last_measured_at,estimator_version,\
               version,updated_ts FROM gemini_window_calibrations \
             WHERE profile_id=$1 AND bucket_id=$2",
            &[&profile_id, &bucket_id],
        )?;
        let row = row.map(|row| GeminiCalibrationRow {
            profile_id: row.get(0),
            bucket_id: row.get(1),
            window_kind: row.get(2),
            window_duration_mins: row.get(3),
            resets_at: row.get(4),
            anchor_used_fraction_units: row.get(5),
            anchor_spend_nano: row.get(6),
            anchor_ready: row.get(7),
            used_fraction_units: row.get(8),
            observed_at: row.get(9),
            sum_used_sq: row.get(10),
            sum_used_spend_nano: row.get(11),
            observed_fraction_units: row.get(12),
            observed_spend_nano: row.get(13),
            samples: row.get(14),
            current_capacity_nano: row.get(15),
            current_low_nano: row.get(16),
            current_high_nano: row.get(17),
            current_confidence_bp: row.get(18),
            last_measured_at: row.get(19),
            estimator_version: row.get(20),
            version: row.get(21),
            updated_ts: row.get(22),
        });
        if let Some(row) = &row {
            crate::validate_gemini_calibration_row(row)?;
        }
        Ok(row)
    }

    pub fn load_gemini_window_observations(
        &mut self,
        profile_id: &str,
        bucket_id: &str,
    ) -> Result<Vec<GeminiWindowObservation>> {
        Ok(self
            .client
            .query(
                "SELECT profile_id,bucket_id,window_kind,window_duration_mins,resets_at,\
                   observed_at,used_fraction_units,gateway_spend_nano \
                 FROM gemini_window_observations WHERE profile_id=$1 AND bucket_id=$2 \
                 ORDER BY observed_at,id",
                &[&profile_id, &bucket_id],
            )?
            .into_iter()
            .map(|row| GeminiWindowObservation {
                profile_id: row.get(0),
                bucket_id: row.get(1),
                window_kind: row.get(2),
                window_duration_mins: row.get(3),
                resets_at: row.get(4),
                observed_at: row.get(5),
                used_fraction_units: row.get(6),
                gateway_spend_nano: row.get(7),
            })
            .collect())
    }

    /// Save Gemini calibration evidence with optimistic concurrency. A conflict rolls the raw
    /// observation back together with the stale derived row.
    pub fn save_gemini_calibration(
        &mut self,
        state: &GeminiCalibrationRow,
        observation: &GeminiWindowObservation,
    ) -> Result<Option<i64>> {
        crate::validate_gemini_calibration_pair(state, observation)?;
        let mut tx = self.client.transaction()?;
        let version = if state.version == 0 {
            tx.query_opt(
                "INSERT INTO gemini_window_calibrations( \
                   profile_id,bucket_id,window_kind,window_duration_mins,resets_at,\
                   anchor_used_fraction_units,anchor_spend_nano,anchor_ready,used_fraction_units,\
                   observed_at,sum_used_sq,sum_used_spend_nano,observed_fraction_units,\
                   observed_spend_nano,samples,current_capacity_nano,current_low_nano,current_high_nano,\
                   current_confidence_bp,last_measured_at,estimator_version,updated_ts,version \
                 ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,CAST(CAST($11 AS text) AS numeric),\
                          CAST(CAST($12 AS text) AS numeric),$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,1) \
                 ON CONFLICT(profile_id,bucket_id) DO NOTHING RETURNING version",
                &[
                    &state.profile_id,
                    &state.bucket_id,
                    &state.window_kind,
                    &state.window_duration_mins,
                    &state.resets_at,
                    &state.anchor_used_fraction_units,
                    &state.anchor_spend_nano,
                    &state.anchor_ready,
                    &state.used_fraction_units,
                    &state.observed_at,
                    &state.sum_used_sq,
                    &state.sum_used_spend_nano,
                    &state.observed_fraction_units,
                    &state.observed_spend_nano,
                    &state.samples,
                    &state.current_capacity_nano,
                    &state.current_low_nano,
                    &state.current_high_nano,
                    &state.current_confidence_bp,
                    &state.last_measured_at,
                    &state.estimator_version,
                    &state.updated_ts,
                ],
            )?
        } else {
            tx.query_opt(
                "UPDATE gemini_window_calibrations SET window_kind=$3,window_duration_mins=$4,\
                   resets_at=$5,anchor_used_fraction_units=$6,anchor_spend_nano=$7,\
                   anchor_ready=$8,used_fraction_units=$9,observed_at=$10,\
                   sum_used_sq=CAST(CAST($11 AS text) AS numeric),\
                   sum_used_spend_nano=CAST(CAST($12 AS text) AS numeric),\
                   observed_fraction_units=$13,observed_spend_nano=$14,samples=$15,\
                   current_capacity_nano=$16,current_low_nano=$17,current_high_nano=$18,\
                   current_confidence_bp=$19,last_measured_at=$20,estimator_version=$21,\
                   updated_ts=$22,version=version+1 \
                 WHERE profile_id=$1 AND bucket_id=$2 AND version=$23 RETURNING version",
                &[
                    &state.profile_id,
                    &state.bucket_id,
                    &state.window_kind,
                    &state.window_duration_mins,
                    &state.resets_at,
                    &state.anchor_used_fraction_units,
                    &state.anchor_spend_nano,
                    &state.anchor_ready,
                    &state.used_fraction_units,
                    &state.observed_at,
                    &state.sum_used_sq,
                    &state.sum_used_spend_nano,
                    &state.observed_fraction_units,
                    &state.observed_spend_nano,
                    &state.samples,
                    &state.current_capacity_nano,
                    &state.current_low_nano,
                    &state.current_high_nano,
                    &state.current_confidence_bp,
                    &state.last_measured_at,
                    &state.estimator_version,
                    &state.updated_ts,
                    &state.version,
                ],
            )?
        };
        let Some(version) = version.map(|row| row.get::<_, i64>(0)) else {
            return Ok(None);
        };
        tx.execute(
            "INSERT INTO gemini_window_observations( \
               profile_id,bucket_id,window_kind,window_duration_mins,resets_at,observed_at,\
               used_fraction_units,gateway_spend_nano \
             ) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING",
            &[
                &observation.profile_id,
                &observation.bucket_id,
                &observation.window_kind,
                &observation.window_duration_mins,
                &observation.resets_at,
                &observation.observed_at,
                &observation.used_fraction_units,
                &observation.gateway_spend_nano,
            ],
        )?;
        tx.commit()?;
        Ok(Some(version))
    }

    // -- Fenced pool-state persistence --------------------------------------------------------

    pub fn load_pool_state(&mut self) -> Result<Vec<PoolStateRow>> {
        Ok(self.client.query(
            "SELECT email,cooling_until,cap5h,cap7d,spent_total,util5,util7,reset5,reset7,calib_n,version \
             FROM pool_state",
            &[],
        )?.into_iter().map(|r| PoolStateRow {
            email:r.get(0),cooling_until:r.get(1),cap5h_usd:r.get(2),cap7d_usd:r.get(3),
            spent_total_usd:r.get(4),spent_delta_usd:0.0,util5h:r.get(5),util7d:r.get(6),reset5h:r.get(7),
            reset7d:r.get(8),calib_n:r.get(9),version:r.get(10),
        }).collect())
    }

    pub fn save_pool_state(
        &mut self,
        owner: &Owner,
        rows: &[PoolStateRow],
    ) -> Result<Vec<(String, i64)>> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        let mut versions = Vec::with_capacity(rows.len());
        for row in rows {
            tx.execute(
                "INSERT INTO pool_state(email) VALUES($1) ON CONFLICT DO NOTHING",
                &[&row.email],
            )?;
            let updated = tx.query_opt(
                "UPDATE pool_state SET cooling_until=$2,cap5h=$3,cap7d=$4,spent_total=spent_total+$5,util5=$6,util7=$7, \
                 reset5=$8,reset7=$9,calib_n=$10,version=version+1,writer_instance=$11,writer_epoch=$12,updated_ts=$13 \
                 WHERE email=$1 AND version=$14 RETURNING version",
                &[&row.email,&row.cooling_until,&row.cap5h_usd,&row.cap7d_usd,&row.spent_delta_usd,
                  &row.util5h,&row.util7d,&row.reset5h,&row.reset7d,&row.calib_n,
                  &owner.instance_id,&owner.epoch,&ts,&row.version],
            )?;
            let Some(updated) = updated else {
                bail!(
                    "pool-state CAS conflict for {} at version {}",
                    row.email,
                    row.version
                );
            };
            versions.push((row.email.clone(), updated.get(0)));
        }
        tx.commit()?;
        Ok(versions)
    }

    pub fn pool_inflight(&mut self, email: &str) -> Result<Option<i64>> {
        Ok(self
            .client
            .query_opt("SELECT inflight FROM pool_state WHERE email=$1", &[&email])?
            .map(|r| r.get(0)))
    }

    /// One-time, repeatable copy from a fully drained SQLite authority. Anonymous aggregate holds
    /// cannot be safely attributed, so a non-zero `reserved_nano` aborts the migration.
    pub fn import_sqlite(&mut self, sqlite_path: &str) -> Result<ImportReport> {
        let sqlite = crate::open(sqlite_path)?;
        let policy_state_rows: i64 = sqlite.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM pricing_catalog_versions)
               + (SELECT COUNT(*) FROM provider_switch_versions)
               + (SELECT COUNT(*) FROM account_policy_versions)
               + (SELECT COUNT(*) FROM account_policy_bindings)
               + (SELECT COUNT(*) FROM funding_buckets)
               + (SELECT COUNT(*) FROM pricing_admission_snapshots)
               + (SELECT COUNT(*) FROM pricing_shadow_admission_evaluations)
               + (SELECT COUNT(*) FROM reservation_funding_allocations)
               + (SELECT COUNT(*) FROM ledger_funding_allocations)",
            [],
            |row| row.get(0),
        )?;
        if policy_state_rows != 0 {
            bail!(
                "SQLite contains policy/funding state unsupported by the legacy importer; \
                 use the policy-aware migration path"
            );
        }
        let attribution_predicate = crate::SQLITE_ATTRIBUTION_COLUMNS
            .iter()
            .map(|(name, _)| format!("\"{name}\" IS NOT NULL"))
            .collect::<Vec<_>>()
            .join(" OR ");
        for table in ["billing_settlement_outbox", "usage_events", "ledger"] {
            let predicate = if table == "ledger" {
                format!("({attribution_predicate}) OR official_nano IS NOT NULL")
            } else {
                attribution_predicate.clone()
            };
            let rows: i64 = sqlite.query_row(
                &format!("SELECT COUNT(*) FROM \"{table}\" WHERE {predicate}"),
                [],
                |row| row.get(0),
            )?;
            if rows != 0 {
                bail!(
                    "SQLite contains policy attribution unsupported by the legacy importer; \
                     use the policy-aware migration path"
                );
            }
        }
        let unresolved_request_lifecycle: i64 = sqlite.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM billing_reservations
                    WHERE state NOT IN ('settled','canceled'))
               + (SELECT COUNT(*) FROM billing_settlement_outbox
                    WHERE state <> 'done')",
            [],
            |row| row.get(0),
        )?;
        if unresolved_request_lifecycle != 0 {
            bail!("SQLite contains unresolved request lifecycle rows; drain before migration");
        }
        let source_totals = crate::billing_totals(&sqlite)?;
        if source_totals.reserved_nano != 0 {
            bail!(
                "SQLite contains {} anonymous reserved nanodollars; drain/reconcile before migration",
                source_totals.reserved_nano
            );
        }

        let mut tx = self.client.transaction()?;
        tx.query_one("SELECT pg_advisory_xact_lock(836214912671::bigint)", &[])?;
        let target_policy_state_rows: i64 = tx
            .query_one(
                "SELECT (
                     (SELECT COUNT(*) FROM pricing_catalog_versions)
                   + (SELECT COUNT(*) FROM provider_switch_versions)
                   + (SELECT COUNT(*) FROM account_policy_versions)
                   + (SELECT COUNT(*) FROM account_policy_bindings)
                   + (SELECT COUNT(*) FROM funding_buckets)
                   + (SELECT COUNT(*) FROM pricing_admission_snapshots)
                   + (SELECT COUNT(*) FROM pricing_shadow_admission_evaluations)
                   + (SELECT COUNT(*) FROM reservation_funding_allocations)
                   + (SELECT COUNT(*) FROM ledger_funding_allocations)
                 )::bigint",
                &[],
            )?
            .get(0);
        if target_policy_state_rows != 0 {
            bail!(
                "PostgreSQL already contains policy/funding authority; \
                 refusing the legacy SQLite import"
            );
        }
        let active_runtime: i64 = tx.query_one(
            "SELECT (SELECT COUNT(*) FROM reservations WHERE state NOT IN ('settled','canceled')) + \
             (SELECT COUNT(*) FROM capacity_leases WHERE state='active')",
            &[],
        )?.get(0);
        if active_runtime != 0 {
            bail!("PostgreSQL already has active runtime leases; refusing SQLite import");
        }

        // Runtime-only rows never come from SQLite and must not survive a re-run of the import.
        tx.execute("DELETE FROM settlement_outbox", &[])?;
        tx.execute("DELETE FROM reservations", &[])?;
        tx.execute("DELETE FROM capacity_leases", &[])?;
        tx.execute("DELETE FROM leader_leases", &[])?;
        tx.execute("DELETE FROM engine_instances", &[])?;
        tx.execute("DELETE FROM usage_events", &[])?;
        tx.execute("DELETE FROM ledger", &[])?;
        tx.execute("DELETE FROM api_keys", &[])?;
        tx.execute("DELETE FROM accounts", &[])?;
        tx.execute("DELETE FROM pool_state", &[])?;
        tx.execute("DELETE FROM subs", &[])?;

        let mut report = ImportReport::default();

        {
            let mut stmt = sqlite.prepare(
                "SELECT email,token,token_file,COALESCE(proxy,''),COALESCE(plan,''),COALESCE(status,'active'), \
                 COALESCE(fleet,'prod'),COALESCE(added_ts,0),COALESCE(added,''),COALESCE(proxy_expire,''), \
                 proxy_checked_ts,proxy_ok FROM subs ORDER BY email",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, Option<i64>>(10)?,
                    r.get::<_, Option<i64>>(11)?,
                ))
            })?;
            for row in rows {
                let (
                    email,
                    token,
                    token_file,
                    proxy,
                    plan,
                    status,
                    fleet,
                    added_ts,
                    added,
                    expire,
                    checked,
                    ok,
                ) = row?;
                let proxy_ok = ok.map(|n| n != 0);
                tx.execute(
                    "INSERT INTO subs(email,token,token_file,proxy,plan,status,fleet,added_ts,added, \
                     proxy_expire,proxy_checked_ts,proxy_ok) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
                    &[&email,&token,&token_file,&proxy,&plan,&status,&fleet,&added_ts,&added,&expire,&checked,&proxy_ok],
                )?;
                report.subscriptions += 1;
            }
        }
        {
            let mut stmt = sqlite.prepare(
                "SELECT id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,COALESCE(status,'active'), \
                 COALESCE(created_ts,0),COALESCE(created,'') FROM accounts ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, String>(8)?,
                ))
            })?;
            for row in rows {
                let (id, handle, balance, spent, reserved, mult, status, created_ts, created) =
                    row?;
                tx.execute(
                    "INSERT INTO accounts(id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status,created_ts,created) \
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
                    &[&id,&handle,&balance,&spent,&reserved,&mult,&status,&created_ts,&created],
                )?;
                report.accounts += 1;
                report.balance_nano = report
                    .balance_nano
                    .checked_add(balance)
                    .context("balance sum overflow")?;
                report.spent_nano = report
                    .spent_nano
                    .checked_add(spent)
                    .context("spent sum overflow")?;
                report.reserved_nano = report
                    .reserved_nano
                    .checked_add(reserved)
                    .context("reserved sum overflow")?;
            }
        }
        {
            let mut stmt = sqlite.prepare(
                "SELECT key,key_id,account_id,label,spent_nano,reserved_nano,spend_limit_nano,expires_ts, \
                 COALESCE(status,'active'),COALESCE(created_ts,0),COALESCE(created,'') \
                 FROM api_keys ORDER BY key",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, i64>(9)?,
                    r.get::<_, String>(10)?,
                ))
            })?;
            for row in rows {
                let (
                    key,
                    key_id,
                    account_id,
                    label,
                    spent,
                    reserved,
                    spend_limit,
                    expires,
                    status,
                    created_ts,
                    created,
                ) = row?;
                let account_id =
                    account_id.context("legacy key has no account_id after SQLite migration")?;
                tx.execute(
                    "INSERT INTO api_keys(key,key_id,account_id,label,spent_nano,reserved_nano, \
                     spend_limit_nano,expires_ts,status,created_ts,created) \
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
                    &[
                        &key,
                        &key_id,
                        &account_id,
                        &label,
                        &spent,
                        &reserved,
                        &spend_limit,
                        &expires,
                        &status,
                        &created_ts,
                        &created,
                    ],
                )?;
                report.keys += 1;
            }
        }
        {
            let mut stmt = sqlite.prepare(
                "SELECT id,account_id,key,kind,request_id,amount_nano,ref,balance_after_nano, \
                 COALESCE(ts,0),model,provider \
                 FROM ledger ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, Option<String>>(9)?,
                    r.get::<_, Option<String>>(10)?,
                ))
            })?;
            for row in rows {
                let (
                    id,
                    account_id,
                    key,
                    kind,
                    request_id,
                    amount,
                    reference,
                    balance,
                    ts,
                    model,
                    provider,
                ) = row?;
                tx.execute(
                    "INSERT INTO ledger(
                         id,account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,
                         ts,model,provider
                     ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
                    &[
                        &id,
                        &account_id,
                        &key,
                        &kind,
                        &request_id,
                        &amount,
                        &reference,
                        &balance,
                        &ts,
                        &model,
                        &provider,
                    ],
                )?;
                report.ledger_rows += 1;
            }
            tx.query_one(
                "SELECT setval(pg_get_serial_sequence('ledger','id'),GREATEST(COALESCE(MAX(id),0),1), \
                 COALESCE(MAX(id),0) > 0) FROM ledger",
                &[],
            )?;
        }
        {
            let mut stmt = sqlite.prepare(
                "SELECT id,request_id,account_id,key,model,input_tokens,output_tokens, \
                 cache_read_tokens,cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests, \
                 real_nano,charge_nano,ref,ts,speed,inference_geo,input_nano,output_nano, \
                 cache_read_nano,cache_write_5m_nano,cache_write_1h_nano,web_search_nano, \
                 priced_ts,COALESCE(NULLIF(provider,''),'anthropic') \
                 FROM usage_events ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, i64>(9)?,
                    r.get::<_, i64>(10)?,
                    r.get::<_, i64>(11)?,
                    r.get::<_, i64>(12)?,
                    r.get::<_, Option<String>>(13)?,
                    r.get::<_, i64>(14)?,
                    r.get::<_, String>(15)?,
                    r.get::<_, String>(16)?,
                    r.get::<_, i64>(17)?,
                    r.get::<_, i64>(18)?,
                    r.get::<_, i64>(19)?,
                    r.get::<_, i64>(20)?,
                    r.get::<_, i64>(21)?,
                    r.get::<_, i64>(22)?,
                    r.get::<_, i64>(23)?,
                    r.get::<_, String>(24)?,
                ))
            })?;
            for row in rows {
                let (
                    id,
                    request_id,
                    account_id,
                    key,
                    model,
                    input,
                    output,
                    cache_read,
                    cache5,
                    cache1,
                    web,
                    real,
                    charge,
                    reference,
                    ts,
                    speed,
                    inference_geo,
                    input_nano,
                    output_nano,
                    cache_read_nano,
                    cache_write_5m_nano,
                    cache_write_1h_nano,
                    web_search_nano,
                    priced_ts,
                    provider,
                ) = row?;
                tx.execute(
                    "INSERT INTO usage_events(
                         id,request_id,account_id,key,model,input_tokens,output_tokens,
                         cache_read_tokens,cache_write_5m_tokens,cache_write_1h_tokens,
                         web_search_requests,real_nano,charge_nano,ref,ts,speed,inference_geo,
                         input_nano,output_nano,cache_read_nano,cache_write_5m_nano,
                         cache_write_1h_nano,web_search_nano,priced_ts,provider
                     ) VALUES(
                         $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,
                         $18,$19,$20,$21,$22,$23,$24,$25
                     )",
                    &[
                        &id,
                        &request_id,
                        &account_id,
                        &key,
                        &model,
                        &input,
                        &output,
                        &cache_read,
                        &cache5,
                        &cache1,
                        &web,
                        &real,
                        &charge,
                        &reference,
                        &ts,
                        &speed,
                        &inference_geo,
                        &input_nano,
                        &output_nano,
                        &cache_read_nano,
                        &cache_write_5m_nano,
                        &cache_write_1h_nano,
                        &web_search_nano,
                        &priced_ts,
                        &provider,
                    ],
                )?;
                report.usage_rows += 1;
            }
            tx.query_one(
                "SELECT setval(pg_get_serial_sequence('usage_events','id'),GREATEST(COALESCE(MAX(id),0),1), \
                 COALESCE(MAX(id),0) > 0) FROM usage_events",
                &[],
            )?;
        }
        {
            let mut stmt = sqlite.prepare(
                "SELECT email,COALESCE(cooling_until,0),COALESCE(cap5h,0),COALESCE(cap7d,0), \
                 COALESCE(spent_total,0),COALESCE(util5,0),COALESCE(util7,0),COALESCE(reset5,0), \
                 COALESCE(reset7,0),COALESCE(calib_n,0),COALESCE(updated_ts,0) FROM pool_state ORDER BY email",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, f64>(3)?,
                    r.get::<_, f64>(4)?,
                    r.get::<_, f64>(5)?,
                    r.get::<_, f64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, i64>(9)?,
                    r.get::<_, i64>(10)?,
                ))
            })?;
            for row in rows {
                let (
                    email,
                    cooling,
                    cap5,
                    cap7,
                    spent,
                    util5,
                    util7,
                    reset5,
                    reset7,
                    calib,
                    updated,
                ) = row?;
                tx.execute(
                    "INSERT INTO pool_state(email,cooling_until,cap5h,cap7d,spent_total,util5,util7,reset5,reset7, \
                     calib_n,updated_ts) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
                    &[&email,&cooling,&cap5,&cap7,&spent,&util5,&util7,&reset5,&reset7,&calib,&updated],
                )?;
                report.pool_rows += 1;
            }
        }
        // Every subscription needs a capacity row even if old SQLite had never persisted its live state.
        tx.execute(
            "INSERT INTO pool_state(email) SELECT email FROM subs ON CONFLICT(email) DO NOTHING",
            &[],
        )?;

        let target = tx.query_one(
            "SELECT COUNT(*)::bigint,COALESCE(SUM(balance_nano),0)::bigint, \
             COALESCE(SUM(spent_nano),0)::bigint,COALESCE(SUM(reserved_nano),0)::bigint FROM accounts",
            &[],
        )?;
        let target_accounts: i64 = target.get(0);
        let target_balance: i64 = target.get(1);
        let target_spent: i64 = target.get(2);
        let target_reserved: i64 = target.get(3);
        if target_accounts as usize != report.accounts
            || target_balance != report.balance_nano
            || target_spent != report.spent_nano
            || target_reserved != report.reserved_nano
            || report.balance_nano != source_totals.balance_nano
            || report.spent_nano != source_totals.spent_nano
        {
            bail!("SQLite/PostgreSQL monetary reconciliation mismatch; import rolled back");
        }
        tx.commit()?;
        Ok(report)
    }
}

impl PgStore {
    pub fn stage8_engine_evidence(
        &mut self,
        request: &crate::stage8::Stage8EngineEvidenceRequest,
    ) -> Result<crate::stage8::Stage8EngineEvidenceReport> {
        crate::stage8::postgres_stage8_engine_evidence(&mut self.client, request)
    }

    pub fn pricing_shadow_admission_evaluation(
        &mut self,
        request_id: &str,
    ) -> Result<Option<crate::pricing::PricingShadowAdmissionEvaluation>> {
        crate::pricing::postgres::postgres_pricing_shadow_admission_evaluation(
            &mut self.client,
            request_id,
        )
    }

    pub fn insert_pricing_shadow_admission_evaluation(
        &mut self,
        input: &crate::pricing::PricingShadowAdmissionEvaluationInput,
    ) -> Result<crate::pricing::PricingShadowEvaluationWrite> {
        crate::pricing::postgres::postgres_insert_pricing_shadow_admission_evaluation(
            &mut self.client,
            input,
        )
    }

    pub fn insert_pricing_shadow_admission_evaluation_with_timeout(
        &mut self,
        input: &crate::pricing::PricingShadowAdmissionEvaluationInput,
        timeout_ms: u64,
    ) -> Result<crate::pricing::PricingShadowEvaluationWrite> {
        crate::pricing::postgres::postgres_insert_pricing_shadow_admission_evaluation_with_timeout(
            &mut self.client,
            input,
            timeout_ms,
        )
    }

    pub fn pricing_read_bundle(
        &mut self,
        account_id: &str,
    ) -> Result<crate::pricing::PricingReadBundle> {
        crate::pricing::postgres::postgres_pricing_read_bundle(&mut self.client, account_id)
    }

    pub fn pricing_read_bundle_with_timeout(
        &mut self,
        account_id: &str,
        timeout_ms: u64,
    ) -> Result<crate::pricing::PricingReadBundle> {
        crate::pricing::postgres::postgres_pricing_read_bundle_with_timeout(
            &mut self.client,
            account_id,
            timeout_ms,
        )
    }

    pub fn pricing_catalog_by_generation(
        &mut self,
        product_id: &str,
        generation: i64,
    ) -> Result<Option<crate::pricing::PricingCatalogSpec>> {
        crate::pricing::postgres::postgres_pricing_catalog_by_generation(
            &mut self.client,
            product_id,
            generation,
        )
    }

    pub fn active_pricing_catalog(
        &mut self,
        product_id: &str,
    ) -> Result<Option<crate::pricing::PricingCatalogSpec>> {
        crate::pricing::postgres::postgres_active_pricing_catalog(&mut self.client, product_id)
    }

    pub fn prepare_pricing_catalog(
        &mut self,
        spec: &crate::pricing::PricingCatalogSpec,
    ) -> Result<crate::pricing::PricingMutation> {
        crate::pricing::postgres::postgres_prepare_pricing_catalog(&mut self.client, spec)
    }

    pub fn activate_pricing_catalog(
        &mut self,
        product_id: &str,
        target: &crate::pricing::VersionTarget,
        expectation: &crate::pricing::ActiveExpectation,
    ) -> Result<crate::pricing::PricingMutation> {
        crate::pricing::postgres::postgres_activate_pricing_catalog(
            &mut self.client,
            product_id,
            target,
            expectation,
        )
    }

    pub fn provider_switches_by_generation(
        &mut self,
        generation: i64,
    ) -> Result<Option<crate::pricing::ProviderSwitchSpec>> {
        crate::pricing::postgres::postgres_provider_switches_by_generation(
            &mut self.client,
            generation,
        )
    }

    pub fn active_provider_switches(
        &mut self,
    ) -> Result<Option<crate::pricing::ProviderSwitchSpec>> {
        crate::pricing::postgres::postgres_active_provider_switches(&mut self.client)
    }

    pub fn prepare_provider_switches(
        &mut self,
        spec: &crate::pricing::ProviderSwitchSpec,
    ) -> Result<crate::pricing::PricingMutation> {
        crate::pricing::postgres::postgres_prepare_provider_switches(&mut self.client, spec)
    }

    pub fn activate_provider_switches(
        &mut self,
        target: &crate::pricing::VersionTarget,
        expectation: &crate::pricing::ActiveExpectation,
    ) -> Result<crate::pricing::PricingMutation> {
        crate::pricing::postgres::postgres_activate_provider_switches(
            &mut self.client,
            target,
            expectation,
        )
    }

    pub fn account_policy_by_version(
        &mut self,
        account_id: &str,
        effective_version: i64,
    ) -> Result<Option<crate::pricing::AccountPolicySpec>> {
        crate::pricing::postgres::postgres_account_policy_by_version(
            &mut self.client,
            account_id,
            effective_version,
        )
    }

    pub fn active_account_policy(
        &mut self,
        account_id: &str,
    ) -> Result<Option<crate::pricing::ActiveAccountPolicy>> {
        crate::pricing::postgres::postgres_active_account_policy(&mut self.client, account_id)
    }

    pub fn prepare_account_policy(
        &mut self,
        spec: &crate::pricing::AccountPolicySpec,
    ) -> Result<crate::pricing::PricingMutation> {
        crate::pricing::postgres::postgres_prepare_account_policy(&mut self.client, spec)
    }

    pub fn activate_account_policy(
        &mut self,
        activation: &crate::pricing::AccountPolicyActivationSpec,
        expectation: &crate::pricing::PolicyActiveExpectation,
    ) -> Result<crate::pricing::PricingMutation> {
        crate::pricing::postgres::postgres_activate_account_policy(
            &mut self.client,
            activation,
            expectation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    fn assert_postgres_batch_rejected(client: &mut Client, sql: &str, expected_message: &str) {
        let error = client
            .batch_execute(sql)
            .expect_err("PostgreSQL batch unexpectedly committed");
        let message = error
            .as_db_error()
            .map(|error| error.message())
            .unwrap_or("non-database PostgreSQL error");
        assert!(
            message.contains(expected_message),
            "unexpected PostgreSQL error: {message}"
        );
        let _ = client.batch_execute("ROLLBACK");
    }

    fn legacy_snapshot(
        request_id: &str,
        account_id: &str,
        official_hold_nano: i64,
        charged_hold_nano: i64,
    ) -> crate::pricing::LegacyScalarAdmissionSnapshot {
        legacy_snapshot_at(
            request_id,
            account_id,
            official_hold_nano,
            charged_hold_nano,
            now(),
        )
    }

    fn legacy_snapshot_at(
        request_id: &str,
        account_id: &str,
        official_hold_nano: i64,
        charged_hold_nano: i64,
        admission_ts: i64,
    ) -> crate::pricing::LegacyScalarAdmissionSnapshot {
        crate::pricing::LegacyScalarAdmissionSnapshot::new(
            crate::pricing::LegacyScalarAdmissionSnapshotInput {
                request_id: request_id.into(),
                account_id: account_id.into(),
                provider: crate::pricing::SnapshotProvider::Anthropic,
                requested_model_id: "claude-sonnet-5".into(),
                canonical_model_id: "claude-sonnet-5".into(),
                alias_generation: 1,
                tariff_schedule_id: "anthropic/standard/sonnet-current/v1".into(),
                tariff_priced_ts: admission_ts,
                admission_ts,
                payable_multiplier_bp: 2_000,
                official_hold_nano,
                charged_hold_nano,
                premium_modifiers: crate::pricing::LegacyPremiumModifiers::AnthropicV1 {
                    speed: crate::pricing::SnapshotAnthropicSpeed::Standard,
                    inference_geo: crate::pricing::SnapshotAnthropicInferenceGeo::Global,
                    inference_geo_basis_points: 10_000,
                },
            },
        )
        .unwrap()
    }

    fn openai_legacy_snapshot(
        request_id: &str,
        account_id: &str,
        official_hold_nano: i64,
        charged_hold_nano: i64,
    ) -> crate::pricing::LegacyScalarAdmissionSnapshot {
        let admission_ts = now();
        crate::pricing::LegacyScalarAdmissionSnapshot::new(
            crate::pricing::LegacyScalarAdmissionSnapshotInput {
                request_id: request_id.into(),
                account_id: account_id.into(),
                provider: crate::pricing::SnapshotProvider::OpenAi,
                requested_model_id: "gpt-5.6".into(),
                canonical_model_id: "gpt-5.6-sol".into(),
                alias_generation: 1,
                tariff_schedule_id: "openai/gpt-5.6-sol/epoch-0/v1".into(),
                tariff_priced_ts: admission_ts,
                admission_ts,
                payable_multiplier_bp: 2_000,
                official_hold_nano,
                charged_hold_nano,
                premium_modifiers: crate::pricing::LegacyPremiumModifiers::OpenAiV1 {
                    service_tier: crate::pricing::SnapshotOpenAiServiceTier::Fast,
                    service_tier_multiplier_basis_points: 25_000,
                    context_tier: crate::pricing::SnapshotOpenAiContextTier::Long,
                    input_multiplier_basis_points: 20_000,
                    output_multiplier_basis_points: 15_000,
                },
            },
        )
        .unwrap()
    }

    /// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
    /// pg::tests::postgres_legacy_snapshot_contract_matrix`
    #[test]
    fn postgres_legacy_snapshot_contract_matrix() {
        use crate::pricing::{
            LegacyScalarReserveConflict as Conflict, LegacyScalarReserveOutcome as O,
        };

        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping PostgreSQL legacy snapshot contract: \
                 CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let mut pg = PgStore::connect(&url).unwrap();
        pg.client
            .batch_execute("SET statement_timeout=0; SET lock_timeout=0")
            .unwrap();
        pg.client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        pg.client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .unwrap();
        pg.migrate().unwrap();
        pg.client
            .batch_execute(
                "TRUNCATE account_policy_bindings,account_policy_rules,account_policy_versions,
                 provider_switch_head,provider_switch_entries,provider_switch_versions,
                 pricing_catalog_heads,pricing_catalog_entries,pricing_catalog_versions,
                 settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances,
                 usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE",
            )
            .unwrap();

        let owner = pg.claim_instance("snapshot-engine", 600).unwrap();
        pg.account_create("snapshot-account", None, 2_000).unwrap();
        pg.account_topup("snapshot-account", 1_000, None).unwrap();
        pg.key_issue("snapshot-key", "snapshot-account", None)
            .unwrap();

        let current = now();
        let money_before_window_checks: (i64, i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano, \
                            (SELECT COUNT(*)::bigint FROM ledger) \
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id \
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2), row.get(3))
        };
        let expired = legacy_snapshot_at(
            "expired-window-request",
            "snapshot-account",
            500,
            100,
            current - 2 * crate::pricing::LEGACY_SCALAR_REPLAY_MAX_AGE_SECS,
        );
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &expired)
                .unwrap(),
            O::Conflict(Conflict::ExpiredIdempotencyWindow)
        );
        let future = legacy_snapshot_at(
            "future-window-request",
            "snapshot-account",
            500,
            100,
            current + 2 * crate::pricing::LEGACY_SCALAR_REPLAY_MAX_AGE_SECS,
        );
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &future)
                .unwrap(),
            O::Conflict(Conflict::AdmissionTimestampInFuture)
        );
        let money_after_window_checks: (i64, i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano, \
                            (SELECT COUNT(*)::bigint FROM ledger) \
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id \
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2), row.get(3))
        };
        assert_eq!(money_after_window_checks, money_before_window_checks);
        let rejected_window_rows = pg
            .client
            .query_one(
                "SELECT \
                    (SELECT COUNT(*)::bigint FROM reservations \
                      WHERE request_id IN ('expired-window-request','future-window-request')), \
                    (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots \
                      WHERE request_id IN ('expired-window-request','future-window-request'))",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                rejected_window_rows.get::<_, i64>(0),
                rejected_window_rows.get::<_, i64>(1),
            ),
            (0, 0)
        );

        let aborted_snapshot =
            legacy_snapshot("aborted-before-commit", "snapshot-account", 500, 100);
        let mut insert_gate_calls = 0;
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot_guarded(
                &owner,
                "snapshot-key",
                60,
                &aborted_snapshot,
                || {
                    insert_gate_calls += 1;
                    false
                },
            )
            .unwrap(),
            O::AbortedBeforeCommit
        );
        assert_eq!(insert_gate_calls, 1);
        let aborted_counts = pg
            .client
            .query_one(
                "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano, \
                        (SELECT COUNT(*)::bigint FROM reservations \
                          WHERE request_id='aborted-before-commit'), \
                        (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots \
                          WHERE request_id='aborted-before-commit') \
                   FROM accounts a JOIN api_keys k ON k.account_id=a.id \
                  WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                aborted_counts.get::<_, i64>(0),
                aborted_counts.get::<_, i64>(1),
                aborted_counts.get::<_, i64>(2),
                aborted_counts.get::<_, i64>(3),
                aborted_counts.get::<_, i64>(4),
            ),
            (
                money_before_window_checks.0,
                money_before_window_checks.1,
                money_before_window_checks.2,
                0,
                0,
            )
        );

        let snapshot = legacy_snapshot("snapshot-request", "snapshot-account", 500, 100);

        let inserted = pg
            .reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &snapshot)
            .unwrap();
        let O::Inserted(inserted) = inserted else {
            panic!("first PostgreSQL snapshot reservation was not inserted");
        };
        assert_eq!(inserted.balance_after_reserve_nano, 900);
        assert_eq!(inserted.snapshot, snapshot);
        assert_eq!(
            pg.legacy_scalar_admission_snapshot("snapshot-request")
                .unwrap()
                .unwrap(),
            snapshot
        );
        let mut replay_gate_calls = 0;
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot_guarded(
                &owner,
                "snapshot-key",
                60,
                &snapshot,
                || {
                    replay_gate_calls += 1;
                    false
                },
            )
            .unwrap(),
            O::AbortedBeforeCommit
        );
        assert_eq!(replay_gate_calls, 1);
        let replay_abort_counts = pg
            .client
            .query_one(
                "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano, \
                        (SELECT COUNT(*)::bigint FROM reservations \
                          WHERE request_id='snapshot-request'), \
                        (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots \
                          WHERE request_id='snapshot-request') \
                   FROM accounts a JOIN api_keys k ON k.account_id=a.id \
                  WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                replay_abort_counts.get::<_, i64>(0),
                replay_abort_counts.get::<_, i64>(1),
                replay_abort_counts.get::<_, i64>(2),
                replay_abort_counts.get::<_, i64>(3),
                replay_abort_counts.get::<_, i64>(4),
            ),
            (900, 100, 100, 1, 1)
        );
        let reserved_lease: i64 = pg
            .client
            .query_one(
                "SELECT lease_until FROM reservations WHERE request_id='snapshot-request'",
                &[],
            )
            .unwrap()
            .get(0);
        assert!(matches!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 9_999, &snapshot)
                .unwrap(),
            O::Unchanged(_)
        ));
        assert_eq!(
            pg.client
                .query_one(
                    "SELECT lease_until FROM reservations WHERE request_id='snapshot-request'",
                    &[],
                )
                .unwrap()
                .get::<_, i64>(0),
            reserved_lease
        );
        assert!(pg.mark_delivering(&owner, "snapshot-request", 60).unwrap());
        let delivering_lease: i64 = pg
            .client
            .query_one(
                "SELECT lease_until FROM reservations WHERE request_id='snapshot-request'",
                &[],
            )
            .unwrap()
            .get(0);
        assert!(matches!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 9_999, &snapshot)
                .unwrap(),
            O::Unchanged(_)
        ));
        assert_eq!(
            pg.client
                .query_one(
                    "SELECT lease_until FROM reservations WHERE request_id='snapshot-request'",
                    &[],
                )
                .unwrap()
                .get::<_, i64>(0),
            delivering_lease
        );

        let different = legacy_snapshot("snapshot-request", "snapshot-account", 501, 100);
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &different)
                .unwrap(),
            O::Conflict(Conflict::SnapshotPayload)
        );
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "different-key", 60, &snapshot)
                .unwrap(),
            O::Conflict(Conflict::ReservationIdentity)
        );

        assert_eq!(
            pg.reserve_request(
                &owner,
                "legacy-only",
                "snapshot-account",
                "snapshot-key",
                50,
                60
            )
            .unwrap(),
            Some(850)
        );
        let legacy_only = legacy_snapshot("legacy-only", "snapshot-account", 250, 50);
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &legacy_only)
                .unwrap(),
            O::Conflict(Conflict::ExistingReservationWithoutSnapshot)
        );
        assert!(pg
            .legacy_scalar_admission_snapshot("legacy-only")
            .unwrap()
            .is_none());

        pg.client
            .batch_execute(
                "DROP TRIGGER IF EXISTS reject_test_legacy_snapshot
                     ON pricing_admission_snapshots;
                 DROP FUNCTION IF EXISTS reject_test_legacy_snapshot();
                 CREATE FUNCTION reject_test_legacy_snapshot()
                 RETURNS trigger LANGUAGE plpgsql AS $$
                 BEGIN
                     IF NEW.request_id = 'rollback-request' THEN
                         RAISE EXCEPTION 'injected snapshot failure';
                     END IF;
                     RETURN NEW;
                 END;
                 $$;
                 CREATE TRIGGER reject_test_legacy_snapshot
                 BEFORE INSERT ON pricing_admission_snapshots
                 FOR EACH ROW EXECUTE FUNCTION reject_test_legacy_snapshot();",
            )
            .unwrap();
        let before: (i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2))
        };
        let rollback = legacy_snapshot("rollback-request", "snapshot-account", 500, 100);
        assert!(pg
            .reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &rollback)
            .is_err());
        let after: (i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2))
        };
        assert_eq!(after, before);
        let rollback_counts = pg
            .client
            .query_one(
                "SELECT
                     (SELECT COUNT(*) FROM reservations WHERE request_id='rollback-request'),
                     (SELECT COUNT(*) FROM pricing_admission_snapshots
                       WHERE request_id='rollback-request')",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                rollback_counts.get::<_, i64>(0),
                rollback_counts.get::<_, i64>(1),
            ),
            (0, 0)
        );
        pg.client
            .batch_execute(
                "DROP TRIGGER reject_test_legacy_snapshot ON pricing_admission_snapshots;
                 DROP FUNCTION reject_test_legacy_snapshot();",
            )
            .unwrap();

        pg.account_create("disabled-account", None, 2_000).unwrap();
        pg.account_topup("disabled-account", 1_000, None).unwrap();
        pg.key_issue("disabled-key", "disabled-account", None)
            .unwrap();
        pg.key_set_status("disabled-key", "disabled").unwrap();
        let disabled = legacy_snapshot("disabled-request", "disabled-account", 500, 100);
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "disabled-key", 60, &disabled)
                .unwrap(),
            O::NotReserved
        );
        let disabled_counts = pg
            .client
            .query_one(
                "SELECT
                     (SELECT COUNT(*) FROM reservations WHERE request_id='disabled-request'),
                     (SELECT COUNT(*) FROM pricing_admission_snapshots
                       WHERE request_id='disabled-request')",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                disabled_counts.get::<_, i64>(0),
                disabled_counts.get::<_, i64>(1),
            ),
            (0, 0)
        );

        pg.account_create("openai-snapshot-account", None, 2_000)
            .unwrap();
        pg.account_topup("openai-snapshot-account", 1_000, None)
            .unwrap();
        pg.key_issue("openai-snapshot-key", "openai-snapshot-account", None)
            .unwrap();
        let openai_snapshot = openai_legacy_snapshot(
            "openai-snapshot-request",
            "openai-snapshot-account",
            500,
            100,
        );
        assert!(matches!(
            pg.reserve_request_with_legacy_snapshot(
                &owner,
                "openai-snapshot-key",
                60,
                &openai_snapshot
            )
            .unwrap(),
            O::Inserted(_)
        ));
        assert_eq!(
            pg.legacy_scalar_admission_snapshot("openai-snapshot-request")
                .unwrap()
                .unwrap(),
            openai_snapshot
        );
        assert!(pg
            .legacy_scalar_admission_snapshot("invalid\0request")
            .is_err());

        let concurrent_snapshot =
            legacy_snapshot("concurrent-snapshot-request", "snapshot-account", 125, 25);
        let concurrent_money_before: (i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2))
        };
        let barrier = Arc::new(Barrier::new(3));
        let spawn_reserve = |barrier: Arc<Barrier>| {
            let worker_url = url.clone();
            let worker_owner = owner.clone();
            let worker_snapshot = concurrent_snapshot.clone();
            std::thread::spawn(move || {
                let mut worker = PgStore::connect(&worker_url).unwrap();
                worker
                    .client
                    .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
                    .unwrap();
                barrier.wait();
                worker
                    .reserve_request_with_legacy_snapshot(
                        &worker_owner,
                        "snapshot-key",
                        60,
                        &worker_snapshot,
                    )
                    .unwrap()
            })
        };
        let first = spawn_reserve(barrier.clone());
        let second = spawn_reserve(barrier.clone());
        barrier.wait();
        let outcomes = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, O::Inserted(_)))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, O::Unchanged(_)))
                .count(),
            1
        );
        let concurrent_money_after: (i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2))
        };
        assert_eq!(
            concurrent_money_after,
            (
                concurrent_money_before.0 - 25,
                concurrent_money_before.1 + 25,
                concurrent_money_before.2 + 25,
            )
        );

        pg.cancel_request("snapshot-request").unwrap();
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &snapshot)
                .unwrap(),
            O::Conflict(Conflict::TerminalReservation)
        );
        assert_eq!(
            pg.legacy_scalar_admission_snapshot("snapshot-request")
                .unwrap()
                .unwrap(),
            snapshot
        );

        let counts = pg
            .client
            .query_one(
                "SELECT
                     (SELECT COUNT(*) FROM reservations WHERE request_id='snapshot-request'),
                     (SELECT COUNT(*) FROM pricing_admission_snapshots
                       WHERE request_id='snapshot-request')",
                &[],
            )
            .unwrap();
        assert_eq!((counts.get::<_, i64>(0), counts.get::<_, i64>(1)), (1, 1));

        // Deterministically fence an old writer while it is waiting for this request's advisory
        // lock. The locked recheck after the wait must reject it without touching customer money.
        let fence_snapshot = legacy_snapshot("fence-race-request", "snapshot-account", 500, 100);
        let money_before_fence: (i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2))
        };
        let mut blocker = PgStore::connect(&url).unwrap();
        blocker
            .client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .unwrap();
        blocker
            .client
            .query_one(
                "SELECT pg_advisory_lock(hashtextextended($1, 0))",
                &[&fence_snapshot.request_id.as_str()],
            )
            .unwrap();

        let worker_url = url.clone();
        let worker_owner = owner.clone();
        let worker_snapshot = fence_snapshot.clone();
        let worker = std::thread::spawn(
            move || -> anyhow::Result<crate::pricing::LegacyScalarReserveOutcome> {
                let mut worker = PgStore::connect(&worker_url)?;
                worker
                    .client
                    .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")?;
                worker.reserve_request_with_legacy_snapshot(
                    &worker_owner,
                    "snapshot-key",
                    60,
                    &worker_snapshot,
                )
            },
        );

        let wait_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let waiting: i64 = pg
                .client
                .query_one(
                    "SELECT COUNT(*)::bigint FROM pg_locks
                      WHERE locktype='advisory' AND NOT granted",
                    &[],
                )
                .unwrap()
                .get(0);
            if waiting > 0 {
                break;
            }
            assert!(
                std::time::Instant::now() < wait_deadline,
                "snapshot writer did not reach the advisory-lock wait"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let replacement_owner = pg.claim_instance("snapshot-engine", 600).unwrap();
        assert!(replacement_owner.epoch > owner.epoch);
        let unlocked: bool = blocker
            .client
            .query_one(
                "SELECT pg_advisory_unlock(hashtextextended($1, 0))",
                &[&fence_snapshot.request_id.as_str()],
            )
            .unwrap()
            .get(0);
        assert!(unlocked);
        let fenced_error = worker
            .join()
            .expect("snapshot fence worker panicked")
            .unwrap_err();
        assert!(fenced_error
            .to_string()
            .contains("engine owner lease is stale or fenced"));

        let money_after_fence: (i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2))
        };
        assert_eq!(money_after_fence, money_before_fence);
        let fence_counts = pg
            .client
            .query_one(
                "SELECT
                     (SELECT COUNT(*) FROM reservations WHERE request_id='fence-race-request'),
                     (SELECT COUNT(*) FROM pricing_admission_snapshots
                       WHERE request_id='fence-race-request')",
                &[],
            )
            .unwrap();
        assert_eq!(
            (fence_counts.get::<_, i64>(0), fence_counts.get::<_, i64>(1),),
            (0, 0)
        );
        pg.client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }

    fn shadow_pg_catalog(generation: i64, digest: &str) -> crate::pricing::PricingCatalogSpec {
        crate::pricing::PricingCatalogSpec {
            product_id: "main".into(),
            generation,
            schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
            capability_generation: 17,
            capability_digest: "capability-17".into(),
            content_digest: digest.into(),
            entries: vec![crate::pricing::PricingCatalogEntrySpec {
                provider_id: "anthropic".into(),
                canonical_model_id: "claude-sonnet-5".into(),
                enabled: true,
            }],
        }
    }

    fn shadow_pg_switches(
        generation: i64,
        catalog_generation: i64,
        digest: &str,
    ) -> crate::pricing::ProviderSwitchSpec {
        crate::pricing::ProviderSwitchSpec {
            generation,
            schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
            capability_generation: 17,
            capability_digest: "capability-17".into(),
            content_digest: digest.into(),
            entries: vec![
                crate::pricing::ProviderSwitchEntrySpec {
                    provider_id: "anthropic".into(),
                    scope: crate::pricing::ProviderSwitchScope::Master,
                    catalog_generation: None,
                    enabled: true,
                },
                crate::pricing::ProviderSwitchEntrySpec {
                    provider_id: "anthropic".into(),
                    scope: crate::pricing::ProviderSwitchScope::Segment {
                        product_id: "main".into(),
                        segment: crate::pricing::PolicySegment::B2b,
                    },
                    catalog_generation: Some(catalog_generation),
                    enabled: true,
                },
            ],
        }
    }

    fn shadow_pg_rule() -> crate::pricing::AccountPolicyRuleSpec {
        crate::pricing::AccountPolicyRuleSpec {
            rule_id: "anthropic-discount".into(),
            rule_digest: "anthropic-discount-digest".into(),
            scope: crate::pricing::PolicyRuleScope::Provider {
                provider_id: "anthropic".into(),
            },
            pricing_mode: crate::pricing::PricingMode::Discount,
            rule_origin: crate::pricing::RuleOrigin::Managed,
            discount_bps: Some(1_000),
            payable_multiplier_bp: 9_000,
            track_eligible: false,
            retention_eligible: false,
            commission_eligible: false,
        }
    }

    fn shadow_pg_policy() -> crate::pricing::AccountPolicySpec {
        crate::pricing::AccountPolicySpec {
            account_id: "shadow-pg-account".into(),
            effective_version: 1,
            policy_id: "b2b:shadow-pg-account".into(),
            policy_version: 1,
            source_policy_digest: "source-1".into(),
            owner_type: crate::pricing::PolicyOwnerType::B2bClient,
            owner_id: "shadow-pg-account".into(),
            account_class: crate::pricing::AccountClass::B2b,
            product_id: "main".into(),
            schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
            catalog_generation: 1,
            switch_generation: 1,
            content_digest: "shadow-policy-1".into(),
            replacement_locked: false,
            rules: vec![shadow_pg_rule()],
        }
    }

    fn shadow_pg_dependency(version: i64, digest: &str) -> crate::pricing::PricingShadowDependency {
        crate::pricing::PricingShadowDependency {
            target: crate::pricing::VersionTarget::new(version, digest),
            pricing_schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
            capability_generation: 17,
            capability_digest: "capability-17".into(),
        }
    }

    fn shadow_pg_manifest() -> crate::pricing::PricingRuntimeManifestEvidence {
        crate::pricing::PricingRuntimeManifestEvidence::new(
            1,
            vec![crate::pricing::PricingRuntimeCapabilityEvidence::new(
                crate::pricing::PRICING_SCHEMA_VERSION,
                17,
                "capability-17",
            )
            .unwrap()],
        )
        .unwrap()
    }

    fn shadow_pg_resolved(
        actual: &crate::pricing::ShadowActualSnapshotRef,
    ) -> crate::pricing::PricingShadowEvaluationOutcome {
        crate::pricing::PricingShadowEvaluationOutcome::Resolved(Box::new(
            crate::pricing::PricingShadowResolved::new(
                actual,
                crate::pricing::PricingShadowResolvedInput {
                    observed_multiplier_bp: 2_000,
                    product_id: "main".into(),
                    account_class: crate::pricing::AccountClass::B2b,
                    policy: crate::pricing::PricingShadowPolicyIdentity {
                        target: crate::pricing::VersionTarget::new(1, "shadow-policy-1"),
                        policy_id: "b2b:shadow-pg-account".into(),
                        policy_version: 1,
                        source_policy_digest: "source-1".into(),
                        schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
                    },
                    policy_lineage: crate::pricing::PricingShadowLineage {
                        catalog: shadow_pg_dependency(1, "shadow-catalog-1"),
                        switches: shadow_pg_dependency(1, "shadow-switches-1"),
                    },
                    admission_lineage: crate::pricing::PricingShadowLineage {
                        catalog: shadow_pg_dependency(2, "shadow-catalog-2"),
                        switches: shadow_pg_dependency(2, "shadow-switches-2"),
                    },
                    rule: shadow_pg_rule(),
                },
            )
            .unwrap(),
        ))
    }

    fn shadow_pg_input(
        snapshot: &crate::pricing::LegacyScalarAdmissionSnapshot,
        outcome: crate::pricing::PricingShadowEvaluationOutcome,
        enqueued_ts: i64,
        evaluated_ts: i64,
        diagnostic: serde_json::Value,
    ) -> crate::pricing::PricingShadowAdmissionEvaluationInput {
        crate::pricing::PricingShadowAdmissionEvaluationInput::new(
            crate::pricing::ShadowActualSnapshotRef::from_snapshot(snapshot).unwrap(),
            crate::pricing::PRICING_SCHEMA_VERSION,
            shadow_pg_manifest(),
            enqueued_ts,
            evaluated_ts,
            outcome,
            crate::pricing::ShadowDiagnosticContext::new(diagnostic).unwrap(),
        )
        .unwrap()
    }

    fn stage8_pg_manifest() -> crate::pricing::PricingRuntimeManifestEvidence {
        crate::pricing::PricingRuntimeManifestEvidence::new(
            1,
            vec![crate::pricing::PricingRuntimeCapabilityEvidence::new(
                crate::pricing::PRICING_SCHEMA_VERSION,
                1,
                "stage8-capability-1",
            )
            .unwrap()],
        )
        .unwrap()
    }

    fn stage8_pg_catalog(product_id: &str) -> crate::pricing::PricingCatalogSpec {
        crate::pricing::PricingCatalogSpec {
            product_id: product_id.into(),
            generation: 1,
            schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
            capability_generation: 1,
            capability_digest: "stage8-capability-1".into(),
            content_digest: format!("stage8-{product_id}-catalog-1"),
            entries: vec![
                crate::pricing::PricingCatalogEntrySpec {
                    provider_id: "anthropic".into(),
                    canonical_model_id: "claude-sonnet-5".into(),
                    enabled: true,
                },
                crate::pricing::PricingCatalogEntrySpec {
                    provider_id: "openai".into(),
                    canonical_model_id: "gpt-5.6-sol".into(),
                    enabled: true,
                },
            ],
        }
    }

    fn stage8_pg_switches() -> crate::pricing::ProviderSwitchSpec {
        use crate::pricing::{PolicySegment, ProviderSwitchEntrySpec, ProviderSwitchScope};

        let mut entries = Vec::new();
        for provider_id in ["anthropic", "openai"] {
            entries.push(ProviderSwitchEntrySpec {
                provider_id: provider_id.into(),
                scope: ProviderSwitchScope::Master,
                catalog_generation: None,
                enabled: true,
            });
            for product_id in ["main", "openkeys"] {
                entries.push(ProviderSwitchEntrySpec {
                    provider_id: provider_id.into(),
                    scope: ProviderSwitchScope::Product {
                        product_id: product_id.into(),
                    },
                    catalog_generation: Some(1),
                    enabled: true,
                });
            }
            for segment in [PolicySegment::B2c, PolicySegment::B2b] {
                entries.push(ProviderSwitchEntrySpec {
                    provider_id: provider_id.into(),
                    scope: ProviderSwitchScope::Segment {
                        product_id: "main".into(),
                        segment,
                    },
                    catalog_generation: Some(1),
                    enabled: true,
                });
            }
        }
        crate::pricing::ProviderSwitchSpec {
            generation: 1,
            schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
            capability_generation: 1,
            capability_digest: "stage8-capability-1".into(),
            content_digest: "stage8-switches-1".into(),
            entries,
        }
    }

    fn stage8_pg_rule(
        provider_id: &str,
        payable_multiplier_bp: i64,
    ) -> crate::pricing::AccountPolicyRuleSpec {
        crate::pricing::AccountPolicyRuleSpec {
            rule_id: format!("stage8-{provider_id}-discount"),
            rule_digest: format!("stage8-{provider_id}-discount-digest"),
            scope: crate::pricing::PolicyRuleScope::Provider {
                provider_id: provider_id.into(),
            },
            pricing_mode: crate::pricing::PricingMode::Discount,
            rule_origin: crate::pricing::RuleOrigin::Managed,
            discount_bps: Some(10_000 - payable_multiplier_bp),
            payable_multiplier_bp,
            track_eligible: false,
            retention_eligible: false,
            commission_eligible: false,
        }
    }

    fn stage8_pg_policy() -> crate::pricing::AccountPolicySpec {
        crate::pricing::AccountPolicySpec {
            account_id: "stage8-account".into(),
            effective_version: 1,
            policy_id: "b2b:stage8-account".into(),
            policy_version: 1,
            source_policy_digest: "stage8-source-policy-1".into(),
            owner_type: crate::pricing::PolicyOwnerType::B2bClient,
            owner_id: "stage8-account".into(),
            account_class: crate::pricing::AccountClass::B2b,
            product_id: "main".into(),
            schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
            catalog_generation: 1,
            switch_generation: 1,
            content_digest: "stage8-policy-1".into(),
            replacement_locked: false,
            rules: vec![
                stage8_pg_rule("anthropic", 2_000),
                stage8_pg_rule("openai", 3_000),
            ],
        }
    }

    fn stage8_pg_snapshot(
        request_id: &str,
        provider_id: &str,
        admission_ts: i64,
        charged_hold_nano: i64,
    ) -> crate::pricing::LegacyScalarAdmissionSnapshot {
        let (provider, requested_model_id, canonical_model_id, tariff_schedule_id, premium) =
            match provider_id {
                "anthropic" => (
                    crate::pricing::SnapshotProvider::Anthropic,
                    "claude-sonnet-5",
                    "claude-sonnet-5",
                    "anthropic/standard/sonnet-current/v1",
                    crate::pricing::LegacyPremiumModifiers::AnthropicV1 {
                        speed: crate::pricing::SnapshotAnthropicSpeed::Standard,
                        inference_geo: crate::pricing::SnapshotAnthropicInferenceGeo::Global,
                        inference_geo_basis_points: 10_000,
                    },
                ),
                "openai" => (
                    crate::pricing::SnapshotProvider::OpenAi,
                    "gpt-5.6",
                    "gpt-5.6-sol",
                    "openai/gpt-5.6-sol/epoch-0/v1",
                    crate::pricing::LegacyPremiumModifiers::OpenAiV1 {
                        service_tier: crate::pricing::SnapshotOpenAiServiceTier::Standard,
                        service_tier_multiplier_basis_points: 10_000,
                        context_tier: crate::pricing::SnapshotOpenAiContextTier::Standard,
                        input_multiplier_basis_points: 10_000,
                        output_multiplier_basis_points: 10_000,
                    },
                ),
                other => panic!("unsupported Stage 8 test provider {other}"),
            };
        crate::pricing::LegacyScalarAdmissionSnapshot::new(
            crate::pricing::LegacyScalarAdmissionSnapshotInput {
                request_id: request_id.into(),
                account_id: "stage8-account".into(),
                provider,
                requested_model_id: requested_model_id.into(),
                canonical_model_id: canonical_model_id.into(),
                alias_generation: 1,
                tariff_schedule_id: tariff_schedule_id.into(),
                tariff_priced_ts: admission_ts,
                admission_ts,
                payable_multiplier_bp: 2_000,
                official_hold_nano: 500_000_000,
                charged_hold_nano,
                premium_modifiers: premium,
            },
        )
        .unwrap()
    }

    fn stage8_pg_shadow_input(
        snapshot: &crate::pricing::LegacyScalarAdmissionSnapshot,
        provider_id: &str,
    ) -> crate::pricing::PricingShadowAdmissionEvaluationInput {
        let actual = crate::pricing::ShadowActualSnapshotRef::from_snapshot(snapshot).unwrap();
        let dependency_catalog = crate::pricing::PricingShadowDependency {
            target: crate::pricing::VersionTarget::new(1, "stage8-main-catalog-1"),
            pricing_schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
            capability_generation: 1,
            capability_digest: "stage8-capability-1".into(),
        };
        let dependency_switches = crate::pricing::PricingShadowDependency {
            target: crate::pricing::VersionTarget::new(1, "stage8-switches-1"),
            pricing_schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
            capability_generation: 1,
            capability_digest: "stage8-capability-1".into(),
        };
        let outcome = crate::pricing::PricingShadowEvaluationOutcome::Resolved(Box::new(
            crate::pricing::PricingShadowResolved::new(
                &actual,
                crate::pricing::PricingShadowResolvedInput {
                    observed_multiplier_bp: 2_000,
                    product_id: "main".into(),
                    account_class: crate::pricing::AccountClass::B2b,
                    policy: crate::pricing::PricingShadowPolicyIdentity {
                        target: crate::pricing::VersionTarget::new(1, "stage8-policy-1"),
                        policy_id: "b2b:stage8-account".into(),
                        policy_version: 1,
                        source_policy_digest: "stage8-source-policy-1".into(),
                        schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
                    },
                    policy_lineage: crate::pricing::PricingShadowLineage {
                        catalog: dependency_catalog.clone(),
                        switches: dependency_switches.clone(),
                    },
                    admission_lineage: crate::pricing::PricingShadowLineage {
                        catalog: dependency_catalog,
                        switches: dependency_switches,
                    },
                    rule: stage8_pg_rule(
                        provider_id,
                        if provider_id == "anthropic" {
                            2_000
                        } else {
                            3_000
                        },
                    ),
                },
            )
            .unwrap(),
        ));
        crate::pricing::PricingShadowAdmissionEvaluationInput::new(
            actual,
            crate::pricing::PRICING_SCHEMA_VERSION,
            stage8_pg_manifest(),
            snapshot.admission_ts() + 1,
            snapshot.admission_ts() + 2,
            outcome,
            crate::pricing::ShadowDiagnosticContext::new(serde_json::json!({})).unwrap(),
        )
        .unwrap()
    }

    /// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
    /// pg::tests::postgres_stage8_engine_evidence_contract`
    #[test]
    fn postgres_stage8_engine_evidence_contract() {
        use crate::pricing::{
            AccountPolicyActivationSpec, AccountPolicyBindingSpec, ActiveExpectation,
            FundingEnforcement, LegacyScalarReserveOutcome, PolicyActiveExpectation,
            PolicyEnforcement, PricingMutation, ReconciliationState,
        };

        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping PostgreSQL Stage 8 evidence contract: \
                 CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let mut pg = PgStore::connect(&url).unwrap();
        pg.client
            .batch_execute("SET statement_timeout=0; SET lock_timeout=0")
            .unwrap();
        pg.client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        pg.client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .unwrap();
        pg.migrate().unwrap();
        pg.client
            .batch_execute(
                "TRUNCATE account_policy_bindings,account_policy_rules,account_policy_versions,
                 provider_switch_head,provider_switch_entries,provider_switch_versions,
                 pricing_catalog_heads,pricing_catalog_entries,pricing_catalog_versions,
                 settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances,
                 usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE",
            )
            .unwrap();

        let owner = pg.claim_instance("stage8-engine", 600).unwrap();
        pg.account_create("stage8-account", None, 2_000).unwrap();
        pg.account_topup("stage8-account", 190_000_000, None)
            .unwrap();
        pg.key_issue("stage8-key", "stage8-account", None).unwrap();

        for catalog in [stage8_pg_catalog("main"), stage8_pg_catalog("openkeys")] {
            assert_eq!(
                pg.prepare_pricing_catalog(&catalog).unwrap(),
                PricingMutation::Stored
            );
            assert_eq!(
                pg.activate_pricing_catalog(
                    &catalog.product_id,
                    &catalog.target(),
                    &ActiveExpectation::Absent,
                )
                .unwrap(),
                PricingMutation::Applied
            );
        }
        let switches = stage8_pg_switches();
        assert_eq!(
            pg.prepare_provider_switches(&switches).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            pg.activate_provider_switches(&switches.target(), &ActiveExpectation::Absent)
                .unwrap(),
            PricingMutation::Applied
        );
        let policy = stage8_pg_policy();
        assert_eq!(
            pg.prepare_account_policy(&policy).unwrap(),
            PricingMutation::Stored
        );
        let binding = AccountPolicyBindingSpec {
            policy_enforcement: PolicyEnforcement::Shadow,
            funding_enforcement: FundingEnforcement::Shadow,
            reconciliation_state: ReconciliationState::Verified,
        };
        let activation = AccountPolicyActivationSpec {
            account_id: policy.account_id.clone(),
            effective_version: policy.effective_version,
            content_digest: policy.content_digest.clone(),
            binding,
        };
        assert_eq!(
            pg.activate_account_policy(&activation, &PolicyActiveExpectation::Unbound)
                .unwrap(),
            PricingMutation::Applied
        );

        let window_end_ts = now() - 2;
        let window_start_ts = window_end_ts - 100;
        let authority_ts = window_start_ts - 10;
        for statement in [
            "UPDATE accounts SET created_ts=$1 WHERE id='stage8-account'",
            "UPDATE pricing_catalog_versions SET created_ts=$1",
            "UPDATE pricing_catalog_heads SET updated_ts=$1",
            "UPDATE provider_switch_versions SET created_ts=$1",
            "UPDATE provider_switch_head SET updated_ts=$1",
            "UPDATE account_policy_versions SET created_ts=$1",
            "UPDATE account_policy_bindings SET updated_ts=$1",
        ] {
            pg.client.execute(statement, &[&authority_ts]).unwrap();
        }

        let snapshots = [
            stage8_pg_snapshot(
                "stage8-anthropic-request",
                "anthropic",
                window_start_ts + 10,
                100_000_000,
            ),
            stage8_pg_snapshot(
                "stage8-openai-request",
                "openai",
                window_start_ts + 20,
                90_000_000,
            ),
        ];
        for (snapshot, provider_id) in snapshots.iter().zip(["anthropic", "openai"]) {
            assert!(matches!(
                pg.reserve_request_with_legacy_snapshot(&owner, "stage8-key", 600, snapshot)
                    .unwrap(),
                LegacyScalarReserveOutcome::Inserted(_)
            ));
            assert!(matches!(
                pg.insert_pricing_shadow_admission_evaluation(&stage8_pg_shadow_input(
                    snapshot,
                    provider_id,
                ))
                .unwrap(),
                crate::pricing::PricingShadowEvaluationWrite::Inserted(_)
            ));
        }
        let totals = pg
            .client
            .query_one(
                "SELECT balance_nano,reserved_nano,spent_nano FROM accounts \
                 WHERE id='stage8-account'",
                &[],
            )
            .unwrap();
        let balance_nano: i64 = totals.get(0);
        let reserved_nano: i64 = totals.get(1);
        let spent_nano: i64 = totals.get(2);
        pg.client
            .execute(
                "INSERT INTO funding_buckets( \
                   bucket_id,account_id,source_type,source_ref,eligibility,balance_nano, \
                   reserved_nano,spent_nano,version,status,created_ts,updated_ts \
                 ) VALUES( \
                   'stage8-paid','stage8-account','paid','fixture','any',$1,$2,$3,1,'active',$4,$4)",
                &[&balance_nano, &reserved_nano, &spent_nano, &authority_ts],
            )
            .unwrap();

        let durable_before: (i64, i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots), \
                            (SELECT COUNT(*)::bigint FROM pricing_shadow_admission_evaluations), \
                            (SELECT balance_nano FROM accounts WHERE id='stage8-account'), \
                            (SELECT reserved_nano FROM accounts WHERE id='stage8-account')",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2), row.get(3))
        };
        let request = crate::stage8::Stage8EngineEvidenceRequest {
            window_start_ts,
            window_end_ts,
            min_samples_per_provider: 1,
            financial_sample_size: 2,
            gemini_client_admissions: 0,
            runtime_manifest: stage8_pg_manifest(),
        };
        let report = pg.stage8_engine_evidence(&request).unwrap();
        assert!(report.passed, "unexpected blockers: {:?}", report.blockers);
        assert_eq!(report.counts.active_accounts, 1);
        assert_eq!(report.counts.reconciled_accounts, 1);
        assert_eq!(report.counts.scalar_parity_rows, 1);
        assert_eq!(report.counts.policy_divergence_rows, 1);
        assert_eq!(report.financial_samples.len(), 2);
        assert!(report.evidence_digest.starts_with("sha256:v1:"));
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("stage8-account"));
        assert!(!serialized.contains("stage8-anthropic-request"));
        assert!(!serialized.contains("stage8-openai-request"));

        let mut blocked_request = request.clone();
        blocked_request.gemini_client_admissions = 1;
        let blocked = pg.stage8_engine_evidence(&blocked_request).unwrap();
        assert!(!blocked.passed);
        assert!(blocked
            .blockers
            .iter()
            .any(|blocker| blocker.code == "gemini_client_admissions_nonzero"));
        let durable_after: (i64, i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots), \
                            (SELECT COUNT(*)::bigint FROM pricing_shadow_admission_evaluations), \
                            (SELECT balance_nano FROM accounts WHERE id='stage8-account'), \
                            (SELECT reserved_nano FROM accounts WHERE id='stage8-account')",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2), row.get(3))
        };
        assert_eq!(durable_after, durable_before);

        pg.client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }

    /// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
    /// pg::tests::postgres_typed_shadow_evaluation_contract`
    #[test]
    fn postgres_typed_shadow_evaluation_contract() {
        use crate::pricing::{
            LegacyScalarReserveOutcome, PricingMutation, PricingShadowEvaluationConflict,
            PricingShadowEvaluationOutcome, PricingShadowEvaluationWrite as Write,
            PricingShadowReadErrorCode, PricingShadowRejectionCode, ShadowActualSnapshotRef,
        };
        use serde_json::json;

        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping PostgreSQL typed shadow contract: \
                 CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let mut pg = PgStore::connect(&url).unwrap();
        pg.client
            .batch_execute("SET statement_timeout=0; SET lock_timeout=0")
            .unwrap();
        pg.client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        pg.client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .unwrap();
        pg.migrate().unwrap();
        pg.client
            .batch_execute(
                "TRUNCATE account_policy_bindings,account_policy_rules,account_policy_versions,
                 provider_switch_head,provider_switch_entries,provider_switch_versions,
                 pricing_catalog_heads,pricing_catalog_entries,pricing_catalog_versions,
                 settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances,
                 usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE",
            )
            .unwrap();

        let owner = pg.claim_instance("shadow-pg-engine", 600).unwrap();
        pg.account_create("shadow-pg-account", None, 2_000).unwrap();
        pg.account_topup("shadow-pg-account", 2_000_000_000, None)
            .unwrap();
        pg.key_issue("shadow-pg-key", "shadow-pg-account", None)
            .unwrap();
        for catalog in [
            shadow_pg_catalog(1, "shadow-catalog-1"),
            shadow_pg_catalog(2, "shadow-catalog-2"),
        ] {
            assert_eq!(
                pg.prepare_pricing_catalog(&catalog).unwrap(),
                PricingMutation::Stored
            );
        }
        for switches in [
            shadow_pg_switches(1, 1, "shadow-switches-1"),
            shadow_pg_switches(2, 2, "shadow-switches-2"),
        ] {
            assert_eq!(
                pg.prepare_provider_switches(&switches).unwrap(),
                PricingMutation::Stored
            );
        }
        assert_eq!(
            pg.prepare_account_policy(&shadow_pg_policy()).unwrap(),
            PricingMutation::Stored
        );

        let snapshot = legacy_snapshot(
            "shadow-pg-request",
            "shadow-pg-account",
            500_000_000,
            100_000_000,
        );
        assert!(matches!(
            pg.reserve_request_with_legacy_snapshot(&owner, "shadow-pg-key", 60, &snapshot,)
                .unwrap(),
            LegacyScalarReserveOutcome::Inserted(_)
        ));
        let actual = ShadowActualSnapshotRef::from_snapshot(&snapshot).unwrap();
        let first_enqueued_ts = snapshot.admission_ts() + 1;
        let first_evaluated_ts = first_enqueued_ts + 1;
        let input = shadow_pg_input(
            &snapshot,
            shadow_pg_resolved(&actual),
            first_enqueued_ts,
            first_evaluated_ts,
            json!({"writer": "concurrent"}),
        );

        // The live worker uses transaction-local limits on both its read-only snapshot and its
        // immutable insert. Exercise them against real PostgreSQL locks: this proves set_config is
        // accepted inside REPEATABLE READ READ ONLY and that neither timeout leaks to the session.
        let timed_bundle = pg
            .pricing_read_bundle_with_timeout("shadow-pg-account", 250)
            .unwrap();
        assert_eq!(timed_bundle.account_id, "shadow-pg-account");
        assert_eq!(
            pg.client
                .query_one("SHOW statement_timeout", &[])
                .unwrap()
                .get::<_, String>(0),
            "15s"
        );

        let mut read_blocker = PgStore::connect(&url).unwrap();
        read_blocker
            .client
            .batch_execute("BEGIN; LOCK TABLE accounts IN ACCESS EXCLUSIVE MODE")
            .unwrap();
        let read_started = std::time::Instant::now();
        let read_timeout = pg
            .pricing_read_bundle_with_timeout("shadow-pg-account", 50)
            .unwrap_err();
        assert!(is_statement_or_lock_timeout(&read_timeout));
        assert!(
            read_started.elapsed() < std::time::Duration::from_secs(2),
            "timed shadow read exceeded its bounded lock wait"
        );
        read_blocker.client.batch_execute("ROLLBACK").unwrap();
        assert_eq!(
            pg.pricing_read_bundle_with_timeout("shadow-pg-account", 250)
                .unwrap()
                .account_multiplier_bp,
            2_000
        );

        let timed_snapshot = legacy_snapshot(
            "shadow-pg-timeout-request",
            "shadow-pg-account",
            500_000_000,
            100_000_000,
        );
        assert!(matches!(
            pg.reserve_request_with_legacy_snapshot(&owner, "shadow-pg-key", 60, &timed_snapshot,)
                .unwrap(),
            LegacyScalarReserveOutcome::Inserted(_)
        ));
        let timed_input = shadow_pg_input(
            &timed_snapshot,
            PricingShadowEvaluationOutcome::ReadError {
                reason: PricingShadowReadErrorCode::PricingReadFailed,
            },
            timed_snapshot.admission_ts() + 1,
            timed_snapshot.admission_ts() + 2,
            json!({}),
        );
        let mut write_blocker = PgStore::connect(&url).unwrap();
        write_blocker.client.batch_execute("BEGIN").unwrap();
        write_blocker
            .client
            .query_one(
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                &[&"multi-discount:shadow-evaluation:shadow-pg-timeout-request"],
            )
            .unwrap();
        let write_started = std::time::Instant::now();
        let write_timeout = pg
            .insert_pricing_shadow_admission_evaluation_with_timeout(&timed_input, 50)
            .unwrap_err();
        assert!(is_statement_or_lock_timeout(&write_timeout));
        assert!(
            write_started.elapsed() < std::time::Duration::from_secs(2),
            "timed shadow insert exceeded its bounded lock wait"
        );
        assert!(pg
            .pricing_shadow_admission_evaluation("shadow-pg-timeout-request")
            .unwrap()
            .is_none());
        write_blocker.client.batch_execute("ROLLBACK").unwrap();
        assert!(matches!(
            pg.insert_pricing_shadow_admission_evaluation_with_timeout(&timed_input, 250)
                .unwrap(),
            Write::Inserted(_)
        ));
        assert_eq!(
            pg.client
                .query_one("SHOW lock_timeout", &[])
                .unwrap()
                .get::<_, String>(0),
            "5s"
        );

        let money_before: (i64, i64, i64, String) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,r.hold_nano,r.state
                       FROM accounts a JOIN reservations r ON r.account_id=a.id
                      WHERE r.request_id='shadow-pg-request'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2), row.get(3))
        };

        let barrier = Arc::new(Barrier::new(2));
        let writers = [input.clone(), input.clone()].map(|input| {
            let url = url.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut writer = PgStore::connect(&url).unwrap();
                barrier.wait();
                writer
                    .insert_pricing_shadow_admission_evaluation(&input)
                    .unwrap()
            })
        });
        let outcomes = writers.map(|writer| writer.join().unwrap());
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, Write::Inserted(_)))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, Write::Unchanged(_)))
                .count(),
            1
        );
        let stored = pg
            .pricing_shadow_admission_evaluation("shadow-pg-request")
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.evaluation_digest(),
            input.to_evaluation().unwrap().evaluation_digest()
        );

        let replay = shadow_pg_input(
            &snapshot,
            shadow_pg_resolved(&actual),
            first_enqueued_ts + 8,
            first_evaluated_ts + 17,
            json!({"writer": "lost-ack-replay"}),
        );
        let Write::Unchanged(first) = pg
            .insert_pricing_shadow_admission_evaluation(&replay)
            .unwrap()
        else {
            panic!("PostgreSQL exact shadow replay was not unchanged");
        };
        assert_eq!(first.enqueued_ts(), first_enqueued_ts);
        assert_eq!(
            first.diagnostic_context().value(),
            &json!({"writer": "concurrent"})
        );

        let conflict = shadow_pg_input(
            &snapshot,
            PricingShadowEvaluationOutcome::Rejected {
                reason: PricingShadowRejectionCode::MissingRule,
                observed_multiplier_bp: 2_000,
            },
            first_enqueued_ts,
            first_evaluated_ts,
            json!({}),
        );
        assert_eq!(
            pg.insert_pricing_shadow_admission_evaluation(&conflict)
                .unwrap(),
            Write::Conflict(PricingShadowEvaluationConflict::ExistingSemanticResult)
        );
        assert_eq!(
            pg.client
                .query_one(
                    "SELECT COUNT(*)::bigint FROM pricing_shadow_admission_evaluations
                      WHERE request_id='shadow-pg-request'",
                    &[],
                )
                .unwrap()
                .get::<_, i64>(0),
            1
        );

        let money_after_shadow: (i64, i64, i64, String) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,r.hold_nano,r.state
                       FROM accounts a JOIN reservations r ON r.account_id=a.id
                      WHERE r.request_id='shadow-pg-request'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2), row.get(3))
        };
        assert_eq!(money_after_shadow, money_before);

        for (request_id, outcome) in [
            (
                "shadow-pg-rejected",
                PricingShadowEvaluationOutcome::Rejected {
                    reason: PricingShadowRejectionCode::NoPolicyBinding,
                    observed_multiplier_bp: 2_000,
                },
            ),
            (
                "shadow-pg-read-error",
                PricingShadowEvaluationOutcome::ReadError {
                    reason: PricingShadowReadErrorCode::PricingReadFailed,
                },
            ),
        ] {
            let snapshot =
                legacy_snapshot(request_id, "shadow-pg-account", 500_000_000, 100_000_000);
            assert!(matches!(
                pg.reserve_request_with_legacy_snapshot(&owner, "shadow-pg-key", 60, &snapshot,)
                    .unwrap(),
                LegacyScalarReserveOutcome::Inserted(_)
            ));
            let diagnostic = if request_id == "shadow-pg-read-error" {
                let empty = serde_json::to_string(&json!({"payload": ""})).unwrap();
                let boundary = json!({"payload": "x".repeat(4_096 - empty.len())});
                assert_eq!(serde_json::to_string(&boundary).unwrap().len(), 4_096);
                boundary
            } else {
                json!({})
            };
            let input = shadow_pg_input(
                &snapshot,
                outcome.clone(),
                snapshot.admission_ts() + 1,
                snapshot.admission_ts() + 2,
                diagnostic,
            );
            assert!(matches!(
                pg.insert_pricing_shadow_admission_evaluation(&input)
                    .unwrap(),
                Write::Inserted(_)
            ));
            assert_eq!(
                pg.pricing_shadow_admission_evaluation(request_id)
                    .unwrap()
                    .unwrap()
                    .outcome(),
                &outcome
            );
        }

        pg.settle_request(
            "shadow-pg-read-error",
            10,
            Some("shadow-retention-settle"),
            None,
        )
        .unwrap();
        assert!(pg.maintenance_prune(now()).is_err());
        let rows_after_unsafe_prune = pg
            .client
            .query_one(
                "SELECT \
                    (SELECT COUNT(*)::bigint FROM reservations \
                      WHERE request_id='shadow-pg-read-error'), \
                    (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots \
                      WHERE request_id='shadow-pg-read-error'), \
                    (SELECT COUNT(*)::bigint FROM pricing_shadow_admission_evaluations \
                      WHERE request_id='shadow-pg-read-error')",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                rows_after_unsafe_prune.get::<_, i64>(0),
                rows_after_unsafe_prune.get::<_, i64>(1),
                rows_after_unsafe_prune.get::<_, i64>(2),
            ),
            (1, 1, 1)
        );
        pg.client
            .batch_execute(
                "UPDATE reservations SET settled_ts=100 \
                   WHERE request_id='shadow-pg-read-error'; \
                 UPDATE settlement_outbox SET committed_ts=100,state='done' \
                   WHERE request_id='shadow-pg-read-error';",
            )
            .unwrap();
        let ledger_before_retention: i64 = pg
            .client
            .query_one("SELECT COUNT(*)::bigint FROM ledger", &[])
            .unwrap()
            .get(0);
        let retention = pg.maintenance_prune(200).unwrap();
        assert_eq!(retention.outbox, 1);
        assert_eq!(retention.reservations, 1);
        assert_eq!(retention.pricing_snapshots_cascaded, 1);
        assert_eq!(retention.pricing_shadow_evaluations_cascaded, 1);
        let retained_counts = pg
            .client
            .query_one(
                "SELECT \
                    (SELECT COUNT(*)::bigint FROM reservations \
                      WHERE request_id='shadow-pg-read-error'), \
                    (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots \
                      WHERE request_id='shadow-pg-read-error'), \
                    (SELECT COUNT(*)::bigint FROM pricing_shadow_admission_evaluations \
                      WHERE request_id='shadow-pg-read-error'), \
                    (SELECT COUNT(*)::bigint FROM ledger)",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                retained_counts.get::<_, i64>(0),
                retained_counts.get::<_, i64>(1),
                retained_counts.get::<_, i64>(2),
                retained_counts.get::<_, i64>(3),
            ),
            (0, 0, 0, ledger_before_retention)
        );

        pg.client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }

    #[test]
    fn engine_migration_plan_is_contiguous() {
        let versions: Vec<_> = ENGINE_MIGRATIONS
            .iter()
            .map(|(version, _)| *version)
            .collect();
        assert_eq!(versions, (1..=CURRENT_SCHEMA_VERSION).collect::<Vec<_>>());
    }

    #[test]
    fn anthropic_initial_calibration_version_is_bound_as_bigint() {
        assert!(
            ANTHROPIC_CALIBRATION_INSERT_SQL.contains("($22::bigint)+1"),
            "an untyped `$22 + 1` makes PostgreSQL infer int4 and reject the Rust i64 version",
        );
    }

    /// Run with an isolated database, for example:
    /// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry pg::tests::stage2_fault_matrix`
    #[test]
    fn stage2_fault_matrix() {
        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!("skipping PostgreSQL fault matrix: CLAUDE_API_TEST_DATABASE_URL is unset");
            return;
        };
        // Keep the destructive-test lock on a dedicated session: this matrix intentionally drops
        // and recreates its working PgStore while exercising crash recovery.
        let mut lock_holder = PgStore::connect(&url).unwrap();
        lock_holder
            .client
            .batch_execute("SET statement_timeout=0; SET lock_timeout=0")
            .unwrap();
        lock_holder
            .client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        let mut pg = PgStore::connect(&url).unwrap();
        pg.client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .unwrap();
        pg.migrate().unwrap();
        assert_eq!(pg.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
        pg.migrate().unwrap();
        assert_eq!(pg.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
        let runtime_pin_constraints: i64 = pg
            .client
            .query_one(
                "SELECT COUNT(*)::bigint FROM pg_constraint
                 WHERE conname IN (
                     'provider_switch_versions_capability_identity',
                     'provider_switch_versions_ack_identity',
                     'provider_switch_entries_catalog_fk',
                     'provider_switch_entries_catalog_scope',
                     'account_policy_versions_switch_fk',
                     'account_policy_versions_ack_identity',
                     'pricing_catalog_versions_capability_generation',
                     'pricing_catalog_versions_ack_identity',
                     'account_policy_versions_source_identity',
                     'account_policy_versions_class_identity',
                     'account_policy_versions_lineage_identity',
                     'account_policy_bindings_active_class_fk'
                 )",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(runtime_pin_constraints, 12);
        pg.client
            .batch_execute(
                "TRUNCATE account_policy_bindings,account_policy_rules,account_policy_versions, \
             provider_switch_head,provider_switch_entries,provider_switch_versions, \
             pricing_catalog_heads,pricing_catalog_entries,pricing_catalog_versions, \
             anthropic_window_observations,anthropic_window_calibrations, \
             provider_turn_calibration_events,provider_calibration_subject_spend, \
             gemini_window_observations,gemini_window_calibrations,gemini_profile_spend, \
             codex_turn_calibration_events,codex_window_observations,\
             codex_window_calibrations,codex_home_spend, \
             codex_home_health, \
             settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances, \
             usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE",
            )
            .unwrap();

        let anthropic_state = AnthropicCalibrationRow {
            subject_id: "stage2-anthropic-subject".into(),
            plan: "max20".into(),
            window_kind: "5h".into(),
            window_duration_mins: 300,
            resets_at: 2_000_000_000,
            anchor_used_fraction_units: 10_000_000,
            anchor_resolution_fraction_units: 100_000,
            anchor_spend_nano: 0,
            used_fraction_units: 10_000_000,
            measurement_resolution_fraction_units: 100_000,
            observed_at: 100,
            observed_fraction_units: 0,
            observed_spend_nano: 0,
            samples: 0,
            unattributed_fraction_units: 0,
            current_capacity_nano: None,
            current_low_nano: None,
            current_high_nano: None,
            current_confidence_bp: 0,
            last_measured_at: None,
            estimator_version: 4,
            version: 0,
            updated_ts: 100,
        };
        let anthropic_observation = AnthropicWindowObservation {
            subject_id: anthropic_state.subject_id.clone(),
            plan: anthropic_state.plan.clone(),
            window_kind: anthropic_state.window_kind.clone(),
            window_duration_mins: anthropic_state.window_duration_mins,
            resets_at: anthropic_state.resets_at,
            observed_at: anthropic_state.observed_at,
            used_fraction_units: anthropic_state.used_fraction_units,
            measurement_resolution_fraction_units: anthropic_state
                .measurement_resolution_fraction_units,
            gateway_spend_nano: 0,
            observation_source: "stage2-test".into(),
            source_request_id: None,
        };
        assert_eq!(
            pg.save_anthropic_calibration(&anthropic_state, &anthropic_observation)
                .unwrap(),
            Some(1),
        );
        assert_eq!(
            pg.load_anthropic_calibration("stage2-anthropic-subject", "max20", "5h")
                .unwrap()
                .unwrap()
                .version,
            1,
        );

        assert_eq!(
            pg.credit_codex_home_spend("stage2-codex-home", 40_000_000_000, 100)
                .unwrap(),
            40_000_000_000
        );
        assert_eq!(
            pg.credit_codex_home_spend("stage2-codex-home", 60_000_000_000, 101)
                .unwrap(),
            100_000_000_000
        );
        let state = CodexCalibrationRow {
            home_id: "stage2-codex-home".into(),
            window_duration_mins: 300,
            resets_at: 2_000_000_000,
            anchor_used_percent: 10,
            anchor_used_fraction_units: 10_000_000,
            anchor_spend_nano: 100_000_000_000,
            used_percent: 10,
            used_fraction_units: 10_000_000,
            observed_at: 101,
            sum_used_sq: 0,
            sum_used_spend_nano: 0,
            observed_points: 0,
            observed_fraction_units: 0,
            observed_spend_nano: 0,
            anchor_spend_nanocredits: None,
            observed_spend_nanocredits: None,
            current_capacity_nanocredits: None,
            current_low_nanocredits: None,
            current_high_nanocredits: None,
            last_capacity_nanocredits: None,
            last_low_nanocredits: None,
            last_high_nanocredits: None,
            credit_samples: None,
            credit_estimator_version: None,
            unattributed_fraction_units: None,
            samples: 0,
            current_capacity_nano: None,
            current_low_nano: None,
            current_high_nano: None,
            current_confidence_bp: 0,
            last_capacity_nano: None,
            last_low_nano: None,
            last_high_nano: None,
            last_confidence_bp: 0,
            last_measured_at: None,
            anchor_ready: false,
            estimator_version: 1,
            version: 0,
            updated_ts: 101,
        };
        let observation = CodexWindowObservation {
            home_id: state.home_id.clone(),
            window_duration_mins: state.window_duration_mins,
            resets_at: state.resets_at,
            observed_at: state.observed_at,
            used_percent: state.used_percent,
            used_fraction_units: state.used_fraction_units,
            gateway_spend_nano: state.anchor_spend_nano,
            gateway_spend_nanocredits: None,
        };
        assert_eq!(
            pg.save_codex_calibration(&state, &observation).unwrap(),
            Some(1)
        );
        assert_eq!(
            pg.save_codex_calibration(&state, &observation).unwrap(),
            None
        );
        assert_eq!(
            pg.load_codex_calibration("stage2-codex-home", 300)
                .unwrap()
                .unwrap()
                .version,
            1
        );
        assert_eq!(
            pg.load_codex_window_observations("stage2-codex-home", 300)
                .unwrap(),
            vec![observation.clone()]
        );
        pg.client
            .batch_execute(
                "DELETE FROM codex_window_observations WHERE home_id='stage2-codex-home'; \
                 DELETE FROM codex_window_calibrations WHERE home_id='stage2-codex-home'; \
                 DELETE FROM codex_home_spend WHERE home_id='stage2-codex-home';",
            )
            .unwrap();

        assert_eq!(
            pg.credit_gemini_profile_spend("stage2-gemini-profile", 19_404_000, 102)
                .unwrap(),
            19_404_000
        );
        assert_eq!(
            pg.credit_gemini_profile_spend("stage2-gemini-profile", 1, 103)
                .unwrap(),
            19_404_001
        );
        let gemini_state = GeminiCalibrationRow {
            profile_id: "stage2-gemini-profile".into(),
            bucket_id: "gemini-5h".into(),
            window_kind: "5h".into(),
            window_duration_mins: 300,
            resets_at: 2_000_000_000,
            anchor_used_fraction_units: 1_970,
            anchor_spend_nano: 0,
            anchor_ready: false,
            used_fraction_units: 1_970,
            observed_at: 103,
            sum_used_sq: i128::MAX.to_string(),
            sum_used_spend_nano: "0".into(),
            observed_fraction_units: 0,
            observed_spend_nano: 12_345,
            samples: 0,
            current_capacity_nano: None,
            current_low_nano: None,
            current_high_nano: None,
            current_confidence_bp: 0,
            last_measured_at: None,
            estimator_version: 1,
            version: 0,
            updated_ts: 103,
        };
        let gemini_observation = GeminiWindowObservation {
            profile_id: gemini_state.profile_id.clone(),
            bucket_id: gemini_state.bucket_id.clone(),
            window_kind: gemini_state.window_kind.clone(),
            window_duration_mins: gemini_state.window_duration_mins,
            resets_at: gemini_state.resets_at,
            observed_at: gemini_state.observed_at,
            used_fraction_units: gemini_state.used_fraction_units,
            gateway_spend_nano: 19_404_001,
        };
        assert_eq!(
            pg.save_gemini_calibration(&gemini_state, &gemini_observation)
                .unwrap(),
            Some(1)
        );
        assert_eq!(
            pg.save_gemini_calibration(&gemini_state, &gemini_observation)
                .unwrap(),
            None
        );
        let restored_gemini = pg
            .load_gemini_calibration("stage2-gemini-profile", "gemini-5h")
            .unwrap()
            .unwrap();
        assert_eq!(restored_gemini.version, 1);
        assert_eq!(restored_gemini.sum_used_sq, i128::MAX.to_string());
        assert_eq!(restored_gemini.observed_spend_nano, 12_345);
        assert_eq!(
            pg.load_gemini_window_observations("stage2-gemini-profile", "gemini-5h")
                .unwrap(),
            vec![gemini_observation]
        );
        pg.client
            .batch_execute(
                "DELETE FROM gemini_window_observations \
                   WHERE profile_id='stage2-gemini-profile'; \
                 DELETE FROM gemini_window_calibrations \
                   WHERE profile_id='stage2-gemini-profile'; \
                 DELETE FROM gemini_profile_spend \
                   WHERE profile_id='stage2-gemini-profile';",
            )
            .unwrap();

        let trigger_count: i64 = pg
            .client
            .query_one(
                "SELECT COUNT(*)::bigint FROM pg_trigger \
                 WHERE tgname IN ('pricing_snapshot_reservation_account', \
                                  'pricing_snapshot_immutable_update', \
                                  'pricing_shadow_admission_evaluation_rule_identity', \
                                  'pricing_shadow_admission_evaluation_immutable_update', \
                                  'ledger_funding_allocation_account') \
                   AND NOT tgisinternal",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(trigger_count, 5);
        let seeded_policy_rows: i64 = pg
            .client
            .query_one(
                "SELECT (SELECT COUNT(*) FROM pricing_catalog_versions) \
                      + (SELECT COUNT(*) FROM provider_switch_versions) \
                      + (SELECT COUNT(*) FROM account_policy_versions) \
                      + (SELECT COUNT(*) FROM funding_buckets) \
                      + (SELECT COUNT(*) FROM pricing_admission_snapshots) \
                      + (SELECT COUNT(*) FROM pricing_shadow_admission_evaluations)",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(seeded_policy_rows, 0);

        pg.client
            .batch_execute(
                "INSERT INTO accounts(id,mult_bp,status,created_ts,created) \
                   VALUES('schema-a',2000,'active',1,''),('schema-b',3000,'active',1,''); \
                 INSERT INTO reservations(
                     request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                     owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts
                 ) VALUES(
                     'schema-request','schema-a','schema-key',100,0,
                     'schema-engine',1,100,'reserved',1,1
                 );",
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                "INSERT INTO pricing_admission_snapshots(
                     request_id,account_id,snapshot_kind,schema_version,provider_id,
                     requested_model_id,canonical_model_id,alias_generation,pricing_mode,
                     rule_origin,payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,
                     admission_ts,official_hold_nano,charged_hold_nano,premium_modifiers,
                     snapshot_digest
                 ) VALUES(
                     'schema-request','schema-b','legacy_scalar',1,'anthropic',
                     'claude-test','claude-test',1,'legacy_scalar','legacy',2000,
                     'legacy-tariff',1,1,100,20,'{}'::jsonb,'snapshot'
                 )",
                &[],
            )
            .is_err());
        pg.client
            .execute(
                "INSERT INTO pricing_admission_snapshots(
                     request_id,account_id,snapshot_kind,schema_version,provider_id,
                     requested_model_id,canonical_model_id,alias_generation,pricing_mode,
                     rule_origin,payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,
                     admission_ts,official_hold_nano,charged_hold_nano,premium_modifiers,
                     snapshot_digest
                 ) VALUES(
                     'schema-request','schema-a','legacy_scalar',1,'anthropic',
                     'claude-test','claude-test',1,'legacy_scalar','legacy',2000,
                     'legacy-tariff',1,1,100,20,'{}'::jsonb,'snapshot'
                 )",
                &[],
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                "UPDATE pricing_admission_snapshots
                 SET charged_hold_nano=21 WHERE request_id='schema-request'",
                &[],
            )
            .is_err());
        assert!(pg
            .client
            .execute(
                "INSERT INTO pricing_shadow_admission_evaluations(
                     request_id,account_id,actual_snapshot_kind,actual_snapshot_digest,
                     provider_id,requested_model_id,canonical_model_id,
                     alias_generation,evaluator_schema_version,runtime_manifest_generation,
                     runtime_manifest_digest,enqueued_ts,evaluated_ts,outcome,reason_code,
                     authorized_multiplier_bp,observed_multiplier_bp,official_hold_nano,
                     legacy_hold_nano,
                     comparison_result,diagnostic_context,evaluation_digest
                 ) VALUES(
                     'schema-request','schema-b','legacy_scalar','snapshot',
                     'anthropic','claude-test','claude-test',
                     1,1,1,'runtime-manifest',1,2,'rejected','no_policy_binding',
                     2000,2000,100,20,'not_comparable','{}'::jsonb,'shadow-rejected'
                 )",
                &[],
            )
            .is_err());
        pg.client
            .execute(
                "INSERT INTO pricing_shadow_admission_evaluations(
                     request_id,account_id,actual_snapshot_kind,actual_snapshot_digest,
                     provider_id,requested_model_id,canonical_model_id,
                     alias_generation,evaluator_schema_version,runtime_manifest_generation,
                     runtime_manifest_digest,enqueued_ts,evaluated_ts,outcome,reason_code,
                     authorized_multiplier_bp,observed_multiplier_bp,official_hold_nano,
                     legacy_hold_nano,
                     comparison_result,diagnostic_context,evaluation_digest
                 ) VALUES(
                     'schema-request','schema-a','legacy_scalar','snapshot',
                     'anthropic','claude-test','claude-test',
                     1,1,1,'runtime-manifest',1,2,'rejected','no_policy_binding',
                     2000,2000,100,20,'not_comparable','{}'::jsonb,'shadow-rejected'
                 )",
                &[],
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                "UPDATE pricing_shadow_admission_evaluations
                 SET reason_code='different_reason' WHERE request_id='schema-request'",
                &[],
            )
            .is_err());
        pg.client
            .batch_execute(
                "INSERT INTO funding_buckets(
                     bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                     reserved_nano,spent_nano,version,status,created_ts,updated_ts
                 ) VALUES
                     ('schema-paid-a','schema-a','paid','primary','any',1000,0,0,1,'active',1,1),
                     ('schema-paid-b','schema-b','paid','primary','any',1000,0,0,1,'active',1,1);
                 INSERT INTO ledger(
                     account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts
                 ) VALUES('schema-b','schema-key','charge','schema-ledger-request',10,'schema-charge',990,1);",
            )
            .unwrap();
        let ledger_id: i64 = pg
            .client
            .query_one(
                "SELECT id FROM ledger WHERE request_id='schema-ledger-request'",
                &[],
            )
            .unwrap()
            .get(0);
        assert!(pg
            .client
            .execute(
                "INSERT INTO ledger_funding_allocations(
                     ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
                     direction,amount_nano
                 ) VALUES($1,'schema-a','schema-paid-a','paid',1,'debit',10)",
                &[&ledger_id],
            )
            .is_err());
        pg.client
            .execute(
                "INSERT INTO ledger_funding_allocations(
                     ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
                     direction,amount_nano
                 ) VALUES($1,'schema-b','schema-paid-b','paid',1,'debit',10)",
                &[&ledger_id],
            )
            .unwrap();
        pg.client
            .batch_execute(
                "INSERT INTO pricing_catalog_versions(
                     product_id,generation,schema_version,capability_generation,capability_digest,
                     content_digest,created_ts
                 ) VALUES('schema-main',1,1,1,'capability','catalog',1);
                 INSERT INTO provider_switch_versions(
                     generation,schema_version,capability_generation,capability_digest,
                     content_digest,created_ts
                 ) VALUES(1,1,1,'capability','switch',1);
                 INSERT INTO account_policy_versions(
                     account_id,effective_version,policy_id,policy_version,source_policy_digest,
                     owner_type,owner_id,account_class,product_id,schema_version,
                     catalog_generation,switch_generation,
                     content_digest,replacement_locked,created_ts
                 ) VALUES(
                     'schema-a',1,'schema-policy',1,'source-policy','global_b2c','global','b2c',
                     'schema-main',1,1,1,'policy',false,1
                 );",
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                "INSERT INTO account_policy_bindings(
                     account_id,product_id,account_class,active_effective_version,
                     policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
                 ) VALUES(
                     'schema-a','schema-main','b2b',1,
                     'shadow','legacy_single','pending',1
                 )",
                &[],
            )
            .is_err());
        pg.client
            .execute(
                "INSERT INTO account_policy_bindings(
                     account_id,product_id,account_class,active_effective_version,
                     policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
                 ) VALUES(
                     'schema-a','schema-main','b2c',1,
                     'shadow','legacy_single','pending',1
                 )",
                &[],
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                "INSERT INTO account_policy_rules(
                     account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                     canonical_model_id,pricing_mode,rule_origin,discount_bps,
                     payable_multiplier_bp,track_eligible,retention_eligible,commission_eligible
                 ) VALUES(
                     'schema-a',1,'missing-discount','rule','model','anthropic','claude-test',
                     'discount','managed',NULL,5000,false,false,false
                 )",
                &[],
            )
            .is_err());
        pg.client
            .batch_execute(
                "INSERT INTO pricing_catalog_entries(
                     product_id,generation,provider_id,canonical_model_id,enabled
                 ) VALUES('schema-main',1,'anthropic','claude-test',true);
                 INSERT INTO provider_switch_entries(
                     generation,provider_id,scope_type,product_id,segment,catalog_generation,enabled
                 ) VALUES(1,'anthropic','segment','schema-main','b2c',1,true);
                 INSERT INTO account_policy_rules(
                     account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                     canonical_model_id,pricing_mode,rule_origin,discount_bps,
                     payable_multiplier_bp,track_eligible,retention_eligible,commission_eligible
                 ) VALUES(
                     'schema-a',1,'managed-rule','managed-rule-digest','provider','anthropic',NULL,
                     'discount','managed',6000,4000,false,false,false
                 );
                 INSERT INTO reservations(
                     request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                     owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts
                 ) VALUES
                     (
                         'schema-policy-request','schema-a','schema-key',100,0,
                         'schema-engine',1,100,'reserved',1,1
                     ),
                     (
                         'schema-shadow-request','schema-a','schema-key',100,0,
                         'schema-engine',1,100,'reserved',1,1
                     );
                 INSERT INTO pricing_admission_snapshots(
                     request_id,account_id,snapshot_kind,schema_version,provider_id,product_id,
                     account_class,requested_model_id,canonical_model_id,alias_generation,
                     rule_id,rule_digest,rule_scope,pricing_mode,rule_origin,discount_bps,
                     payable_multiplier_bp,policy_id,policy_version,effective_policy_version,
                     policy_digest,catalog_generation,switch_generation,tariff_schedule_id,
                     tariff_priced_ts,admission_ts,official_hold_nano,charged_hold_nano,
                     track_eligible,retention_eligible,commission_eligible,premium_modifiers,
                     snapshot_digest
                 ) VALUES(
                     'schema-policy-request','schema-a','policy_v1',1,'anthropic','schema-main',
                     'b2c','claude-test','claude-test',1,'managed-rule','managed-rule-digest',
                     'provider','discount','managed',6000,4000,'schema-policy',1,1,'policy',1,1,
                     'tariff',1,1,100,40,false,false,false,'{}'::jsonb,'policy-snapshot'
                 );
                 INSERT INTO pricing_admission_snapshots(
                     request_id,account_id,snapshot_kind,schema_version,provider_id,
                     requested_model_id,canonical_model_id,alias_generation,pricing_mode,
                     rule_origin,payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,
                     admission_ts,official_hold_nano,charged_hold_nano,premium_modifiers,
                     snapshot_digest
                 ) VALUES(
                     'schema-shadow-request','schema-a','legacy_scalar',1,'anthropic',
                     'claude-test','claude-test',1,'legacy_scalar','legacy',2000,
                     'legacy-tariff',1,1,100,20,'{}'::jsonb,'actual-snapshot'
                 );",
            )
            .unwrap();
        let resolved_shadow_sql = "INSERT INTO pricing_shadow_admission_evaluations(
                 request_id,account_id,actual_snapshot_kind,actual_snapshot_digest,provider_id,
                 requested_model_id,canonical_model_id,alias_generation,evaluator_schema_version,
                 runtime_manifest_generation,runtime_manifest_digest,enqueued_ts,evaluated_ts,
                 outcome,authorized_multiplier_bp,observed_multiplier_bp,official_hold_nano,
                 legacy_hold_nano,product_id,account_class,effective_policy_version,policy_id,
                 policy_version,source_policy_digest,policy_digest,policy_schema_version,
                 policy_catalog_generation,policy_catalog_schema_version,
                 policy_catalog_capability_generation,policy_catalog_capability_digest,
                 policy_catalog_digest,policy_switch_generation,policy_switch_schema_version,
                 policy_switch_capability_generation,policy_switch_capability_digest,
                 policy_switch_digest,admission_catalog_generation,admission_catalog_schema_version,
                 admission_catalog_capability_generation,admission_catalog_capability_digest,
                 admission_catalog_digest,admission_switch_generation,admission_switch_schema_version,
                 admission_switch_capability_generation,admission_switch_capability_digest,
                 admission_switch_digest,rule_id,rule_digest,rule_scope,pricing_mode,rule_origin,
                 discount_bps,payable_multiplier_bp,track_eligible,retention_eligible,
                 commission_eligible,policy_hold_nano,comparison_result,diagnostic_context,
                 evaluation_digest
             ) VALUES(
                 'schema-shadow-request','schema-a','legacy_scalar',$1,$2,
                 'claude-test','claude-test',1,1,1,'runtime-manifest',1,2,
                 'resolved',$3,2000,$4,$5,'schema-main','b2c',1,'schema-policy',1,
                 'source-policy','policy',1,1,
                 CASE WHEN $11='policy_catalog_schema_version' THEN NULL ELSE 1 END,
                 CASE WHEN $11='policy_catalog_capability_generation' THEN NULL ELSE 1 END,
                 CASE WHEN $11='policy_catalog_capability_digest' THEN NULL ELSE $6 END,
                 'catalog',1,
                 CASE WHEN $11='policy_switch_schema_version' THEN NULL ELSE 1 END,
                 CASE WHEN $11='policy_switch_capability_generation' THEN NULL ELSE 1 END,
                 CASE WHEN $11='policy_switch_capability_digest' THEN NULL ELSE $6 END,
                 'switch',1,
                 CASE WHEN $11='admission_catalog_schema_version' THEN NULL ELSE 1 END,
                 CASE WHEN $11='admission_catalog_capability_generation' THEN NULL ELSE 1 END,
                 CASE WHEN $11='admission_catalog_capability_digest' THEN NULL ELSE $6 END,
                 'catalog',1,
                 CASE WHEN $11='admission_switch_schema_version' THEN NULL ELSE 1 END,
                 CASE WHEN $11='admission_switch_capability_generation' THEN NULL ELSE 1 END,
                 CASE WHEN $11='admission_switch_capability_digest' THEN NULL ELSE $6 END,
                 'switch','managed-rule','managed-rule-digest','provider',
                 'discount','managed',$7,$8,false,false,false,$9,'different','{}'::jsonb,$10
             )";
        let mut assert_shadow_rejected =
            |actual_digest: &str,
             provider: &str,
             authorized_multiplier_bp: i64,
             official_hold_nano: i64,
             legacy_hold_nano: i64,
             capability_digest: &str,
             discount_bps: i64,
             payable_multiplier_bp: i64,
             evaluation_digest: &str| {
                assert!(pg
                    .client
                    .execute(
                        resolved_shadow_sql,
                        &[
                            &actual_digest,
                            &provider,
                            &authorized_multiplier_bp,
                            &official_hold_nano,
                            &legacy_hold_nano,
                            &capability_digest,
                            &discount_bps,
                            &payable_multiplier_bp,
                            &40_i64,
                            &evaluation_digest,
                            &"",
                        ],
                    )
                    .is_err());
            };
        assert_shadow_rejected(
            "wrong-actual-snapshot",
            "anthropic",
            2000,
            100,
            20,
            "capability",
            6000,
            4000,
            "wrong-actual-digest",
        );
        assert_shadow_rejected(
            "actual-snapshot",
            "openai",
            2000,
            100,
            20,
            "capability",
            6000,
            4000,
            "wrong-actual-provider",
        );
        assert_shadow_rejected(
            "actual-snapshot",
            "anthropic",
            2001,
            100,
            20,
            "capability",
            6000,
            4000,
            "wrong-actual-multiplier",
        );
        assert_shadow_rejected(
            "actual-snapshot",
            "anthropic",
            2000,
            101,
            20,
            "capability",
            6000,
            4000,
            "wrong-official-hold",
        );
        assert_shadow_rejected(
            "actual-snapshot",
            "anthropic",
            2000,
            100,
            21,
            "capability",
            6000,
            4000,
            "wrong-legacy-hold",
        );
        assert_shadow_rejected(
            "actual-snapshot",
            "anthropic",
            2000,
            100,
            20,
            "wrong-capability",
            6000,
            4000,
            "wrong-capability",
        );
        assert_shadow_rejected(
            "actual-snapshot",
            "anthropic",
            2000,
            100,
            20,
            "capability",
            5000,
            5000,
            "wrong-rule-economics",
        );
        for null_field in [
            "policy_catalog_schema_version",
            "policy_catalog_capability_generation",
            "policy_catalog_capability_digest",
            "policy_switch_schema_version",
            "policy_switch_capability_generation",
            "policy_switch_capability_digest",
            "admission_catalog_schema_version",
            "admission_catalog_capability_generation",
            "admission_catalog_capability_digest",
            "admission_switch_schema_version",
            "admission_switch_capability_generation",
            "admission_switch_capability_digest",
        ] {
            assert!(pg
                .client
                .execute(
                    resolved_shadow_sql,
                    &[
                        &"actual-snapshot",
                        &"anthropic",
                        &2000_i64,
                        &100_i64,
                        &20_i64,
                        &"capability",
                        &6000_i64,
                        &4000_i64,
                        &40_i64,
                        &null_field,
                        &null_field,
                    ],
                )
                .is_err());
        }
        pg.client
            .execute(
                resolved_shadow_sql,
                &[
                    &"actual-snapshot",
                    &"anthropic",
                    &2000_i64,
                    &100_i64,
                    &20_i64,
                    &"capability",
                    &6000_i64,
                    &4000_i64,
                    &40_i64,
                    &"shadow-resolved",
                    &"",
                ],
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                resolved_shadow_sql,
                &[
                    &"actual-snapshot",
                    &"anthropic",
                    &2000_i64,
                    &100_i64,
                    &20_i64,
                    &"capability",
                    &6000_i64,
                    &4000_i64,
                    &40_i64,
                    &"shadow-resolved",
                    &"",
                ],
            )
            .is_err());
        pg.client
            .batch_execute(
                "INSERT INTO reservations(
                     request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                     owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts
                 ) VALUES
                     (
                         'schema-shadow-read-error','schema-a','schema-key',100,0,
                         'schema-engine',1,100,'reserved',1,1
                     ),
                     (
                         'schema-shadow-rejected','schema-a','schema-key',100,0,
                         'schema-engine',1,100,'reserved',1,1
                     );
                 INSERT INTO pricing_admission_snapshots(
                     request_id,account_id,snapshot_kind,schema_version,provider_id,
                     requested_model_id,canonical_model_id,alias_generation,pricing_mode,
                     rule_origin,payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,
                     admission_ts,official_hold_nano,charged_hold_nano,premium_modifiers,
                     snapshot_digest
                 ) VALUES
                     (
                         'schema-shadow-read-error','schema-a','legacy_scalar',1,'anthropic',
                         'claude-test','claude-test',1,'legacy_scalar','legacy',2000,
                         'legacy-tariff',1,1,100,20,'{}'::jsonb,'failure-actual'
                     ),
                     (
                         'schema-shadow-rejected','schema-a','legacy_scalar',1,'anthropic',
                         'claude-test','claude-test',1,'legacy_scalar','legacy',2000,
                         'legacy-tariff',1,1,100,20,'{}'::jsonb,'failure-actual'
                     );",
            )
            .unwrap();
        let failure_shadow_sql = "INSERT INTO pricing_shadow_admission_evaluations(
                 request_id,account_id,actual_snapshot_kind,actual_snapshot_digest,provider_id,
                 requested_model_id,canonical_model_id,alias_generation,evaluator_schema_version,
                 runtime_manifest_generation,runtime_manifest_digest,enqueued_ts,evaluated_ts,
                 outcome,reason_code,authorized_multiplier_bp,observed_multiplier_bp,
                 official_hold_nano,legacy_hold_nano,comparison_result,diagnostic_context,
                 evaluation_digest
             ) VALUES(
                 $1,'schema-a','legacy_scalar','failure-actual','anthropic',
                 'claude-test','claude-test',1,1,1,'runtime-manifest',1,2,
                 $2,'authority_read',2000,$3,100,20,'not_comparable','{}'::jsonb,$4
             )";
        assert!(pg
            .client
            .execute(
                failure_shadow_sql,
                &[
                    &"schema-shadow-read-error",
                    &"rejected",
                    &Option::<i64>::None,
                    &"missing-rejected-observation",
                ],
            )
            .is_err());
        pg.client
            .execute(
                failure_shadow_sql,
                &[
                    &"schema-shadow-read-error",
                    &"read_error",
                    &Option::<i64>::None,
                    &"read-error",
                ],
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                failure_shadow_sql,
                &[
                    &"schema-shadow-rejected",
                    &"read_error",
                    &Some(2000_i64),
                    &"unexpected-read-observation",
                ],
            )
            .is_err());
        pg.client
            .execute(
                failure_shadow_sql,
                &[
                    &"schema-shadow-rejected",
                    &"rejected",
                    &Some(2000_i64),
                    &"rejected",
                ],
            )
            .unwrap();
        pg.client.batch_execute(
            "TRUNCATE settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances, \
             usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE; \
             DELETE FROM provider_switch_entries; \
             DELETE FROM provider_switch_head; \
             DELETE FROM provider_switch_versions; \
             DELETE FROM pricing_catalog_entries; \
             DELETE FROM pricing_catalog_heads; \
             DELETE FROM pricing_catalog_versions;",
        ).unwrap();

        // Exercise the real one-time SQLite importer before the transactional fault matrix.
        let sqlite_path = std::env::temp_dir().join(format!(
            "claude-stage2-import-{}-{}.db",
            std::process::id(),
            now()
        ));
        let sqlite_path_s = sqlite_path.to_string_lossy().into_owned();
        {
            let sqlite = crate::open(&sqlite_path_s).unwrap();
            crate::add(&sqlite, "import-sub", "import-token", "", "prod").unwrap();
            crate::account_create(&sqlite, "import-acct", Some("import-handle"), 2000).unwrap();
            crate::key_issue(&sqlite, "import-key", "import-acct", Some("imported")).unwrap();
            crate::account_topup(&sqlite, "import-acct", 5_000, Some("import-seed")).unwrap();
            crate::account_reserve(&sqlite, "import-acct", 1_000).unwrap();
            crate::account_settle(
                &sqlite,
                "import-acct",
                "import-key",
                1_000,
                200,
                Some("import-charge"),
                Some(&UsageEventInput {
                    model: "gpt-import-test".into(),
                    provider: crate::PROVIDER_OPENAI.into(),
                    input_tokens: 11,
                    output_tokens: 12,
                    cache_read_tokens: 13,
                    cache_write_5m_tokens: 14,
                    cache_write_1h_tokens: 15,
                    web_search_requests: 16,
                    real_nano: 180,
                    speed: "fast".into(),
                    inference_geo: "us-east".into(),
                    input_nano: 21,
                    output_nano: 22,
                    cache_read_nano: 23,
                    cache_write_5m_nano: 24,
                    cache_write_1h_nano: 25,
                    web_search_nano: 65,
                    priced_ts: 123_456,
                }),
            )
            .unwrap();
            crate::save_pool_state(
                &sqlite,
                &[PoolStateRow {
                    email: "import-sub".into(),
                    cooling_until: 123,
                    version: 0,
                    ..Default::default()
                }],
            )
            .unwrap();
        }
        let imported = pg.import_sqlite(&sqlite_path_s).unwrap();
        assert_eq!(
            (imported.subscriptions, imported.accounts, imported.keys),
            (1, 1, 1)
        );
        assert_eq!(
            (
                imported.balance_nano,
                imported.spent_nano,
                imported.reserved_nano
            ),
            (4_800, 200, 0)
        );
        let imported_usage = pg
            .client
            .query_one(
                "SELECT request_id,account_id,key,model,provider,
                        input_tokens,output_tokens,cache_read_tokens,
                        cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests,
                        real_nano,charge_nano,ref,speed,inference_geo,
                        input_nano,output_nano,cache_read_nano,cache_write_5m_nano,
                        cache_write_1h_nano,web_search_nano,priced_ts
                 FROM usage_events",
                &[],
            )
            .unwrap();
        assert_eq!(imported_usage.get::<_, Option<String>>(0), None);
        assert_eq!(imported_usage.get::<_, String>(1), "import-acct");
        assert_eq!(
            imported_usage.get::<_, Option<String>>(2).as_deref(),
            Some("import-key")
        );
        assert_eq!(
            (
                imported_usage.get::<_, Option<String>>(3).as_deref(),
                imported_usage.get::<_, String>(4).as_str()
            ),
            (Some("gpt-import-test"), crate::PROVIDER_OPENAI)
        );
        assert_eq!(
            (
                imported_usage.get::<_, i64>(5),
                imported_usage.get::<_, i64>(6),
                imported_usage.get::<_, i64>(7),
                imported_usage.get::<_, i64>(8),
                imported_usage.get::<_, i64>(9),
                imported_usage.get::<_, i64>(10)
            ),
            (11, 12, 13, 14, 15, 16)
        );
        assert_eq!(
            (
                imported_usage.get::<_, i64>(11),
                imported_usage.get::<_, i64>(12),
                imported_usage.get::<_, Option<String>>(13).as_deref(),
                imported_usage.get::<_, String>(14).as_str(),
                imported_usage.get::<_, String>(15).as_str()
            ),
            (180, 200, Some("import-charge"), "fast", "us-east")
        );
        assert_eq!(
            (
                imported_usage.get::<_, i64>(16),
                imported_usage.get::<_, i64>(17),
                imported_usage.get::<_, i64>(18),
                imported_usage.get::<_, i64>(19),
                imported_usage.get::<_, i64>(20),
                imported_usage.get::<_, i64>(21),
                imported_usage.get::<_, i64>(22)
            ),
            (21, 22, 23, 24, 25, 65, 123_456)
        );
        pg.client
            .execute(
                "INSERT INTO funding_buckets(
                     bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                     reserved_nano,spent_nano,version,status,created_ts,updated_ts
                 ) VALUES(
                     'target-policy-bucket','import-acct','paid','primary','any',
                     4800,0,200,1,'active',1,1
                 )",
                &[],
            )
            .unwrap();
        assert!(
            pg.import_sqlite(&sqlite_path_s).is_err(),
            "materialized PostgreSQL policy/funding authority must block the legacy importer"
        );
        pg.client
            .execute(
                "DELETE FROM funding_buckets WHERE bucket_id='target-policy-bucket'",
                &[],
            )
            .unwrap();
        {
            let sqlite = crate::open(&sqlite_path_s).unwrap();
            sqlite
                .execute(
                    "INSERT INTO funding_buckets(
                         bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                         reserved_nano,spent_nano,version,status,created_ts,updated_ts
                     ) VALUES(
                         'import-policy-bucket','import-acct','paid','primary','any',
                         4800,0,200,1,'active',1,1
                     )",
                    [],
                )
                .unwrap();
        }
        assert!(
            pg.import_sqlite(&sqlite_path_s).is_err(),
            "policy/funding state must require the policy-aware migration path"
        );
        let preserved_account = pg
            .client
            .query_one(
                "SELECT balance_nano,spent_nano,reserved_nano FROM accounts WHERE id='import-acct'",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                preserved_account.get::<_, i64>(0),
                preserved_account.get::<_, i64>(1),
                preserved_account.get::<_, i64>(2)
            ),
            (4_800, 200, 0),
            "a failed policy-aware preflight must not delete PostgreSQL authority"
        );
        {
            let sqlite = crate::open(&sqlite_path_s).unwrap();
            sqlite
                .execute(
                    "DELETE FROM funding_buckets WHERE bucket_id='import-policy-bucket'",
                    [],
                )
                .unwrap();
            sqlite
                .execute(
                    "UPDATE ledger SET official_nano=180 WHERE ref='import-charge'",
                    [],
                )
                .unwrap();
        }
        assert!(
            pg.import_sqlite(&sqlite_path_s).is_err(),
            "new official-cost attribution must require the policy-aware migration path"
        );
        {
            let sqlite = crate::open(&sqlite_path_s).unwrap();
            sqlite
                .execute(
                    "UPDATE ledger SET official_nano=NULL WHERE ref='import-charge'",
                    [],
                )
                .unwrap();
            crate::account_reserve(&sqlite, "import-acct", 100).unwrap();
        }
        assert!(
            pg.import_sqlite(&sqlite_path_s).is_err(),
            "anonymous SQLite hold must block cutover"
        );
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{sqlite_path_s}{suffix}"));
        }
        pg.client.batch_execute(
            "TRUNCATE settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances, \
             usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE",
        ).unwrap();

        pg.add("sub@test", "token", "", "prod").unwrap();
        pg.account_create("acct", Some("handle"), 2000).unwrap();
        pg.key_issue("key", "acct", Some("primary")).unwrap();
        assert_eq!(
            pg.account_topup("acct", 1_000, Some("seed")).unwrap(),
            Some(1_000)
        );
        assert_eq!(
            pg.account_topup("acct", 1_000, Some("seed")).unwrap(),
            Some(1_000)
        );
        assert!(pg.account_topup("acct", 999, Some("seed")).is_err());

        let owner = pg.claim_instance("engine-a", 60).unwrap();
        pg.account_create("policy-acct", None, 10_000).unwrap();
        pg.account_topup("policy-acct", 1_000, Some("policy-seed"))
            .unwrap();
        pg.key_issue_with_policy(
            "limited-key",
            "policy-acct",
            Some("limited"),
            Some(700),
            Some(now() + 60),
        )
        .unwrap();
        assert_eq!(
            pg.reserve_request(&owner, "policy-1", "policy-acct", "limited-key", 500, 60)
                .unwrap(),
            Some(500)
        );
        assert_eq!(
            pg.reserve_request(&owner, "policy-1", "policy-acct", "limited-key", 500, 60)
                .unwrap(),
            Some(500)
        );
        assert_eq!(
            pg.key_get("limited-key").unwrap().unwrap().reserved_nano,
            500
        );
        let limited_key_id = pg.key_get("limited-key").unwrap().unwrap().key_id;
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct", &limited_key_id, Some(499), None)
                .unwrap(),
            KeyPolicyUpdate::LimitBelowUsage,
        );
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct", &limited_key_id, Some(700), Some(now() + 120))
                .unwrap(),
            KeyPolicyUpdate::Updated,
        );
        assert_eq!(
            pg.reserve_request(&owner, "policy-2", "policy-acct", "limited-key", 300, 60)
                .unwrap(),
            None
        );
        assert_eq!(pg.cancel_request("policy-1").unwrap(), Some(1_000));
        assert_eq!(pg.cancel_request("policy-1").unwrap(), Some(1_000));
        assert_eq!(
            pg.reserve_request(&owner, "policy-3", "policy-acct", "limited-key", 700, 60)
                .unwrap(),
            Some(300)
        );
        assert_eq!(
            pg.settle_request("policy-3", 650, None, None).unwrap(),
            Some(350)
        );
        let limited = pg.key_get("limited-key").unwrap().unwrap();
        assert_eq!(
            (
                limited.spent_nano,
                limited.reserved_nano,
                limited.spend_limit_nano
            ),
            (650, 0, Some(700))
        );
        assert_eq!(
            pg.reserve_request(
                &owner,
                "policy-boundary",
                "policy-acct",
                "limited-key",
                50,
                60
            )
            .unwrap(),
            Some(300)
        );
        assert_eq!(
            pg.settle_request("policy-boundary", 50, None, None)
                .unwrap(),
            Some(300)
        );
        assert_eq!(
            pg.reserve_request(&owner, "policy-over", "policy-acct", "limited-key", 1, 60)
                .unwrap(),
            None
        );
        pg.key_issue_with_policy("expired-key", "policy-acct", None, None, Some(now()))
            .unwrap();
        assert_eq!(
            pg.reserve_request(
                &owner,
                "policy-expired",
                "policy-acct",
                "expired-key",
                1,
                60
            )
            .unwrap(),
            None
        );
        let expired_key_id = pg.key_get("expired-key").unwrap().unwrap().key_id;
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct", &expired_key_id, None, Some(now() + 60))
                .unwrap(),
            KeyPolicyUpdate::Updated,
        );
        assert!(pg
            .reserve_request(
                &owner,
                "policy-extended",
                "policy-acct",
                "expired-key",
                1,
                60
            )
            .unwrap()
            .is_some());
        pg.cancel_request("policy-extended").unwrap();
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct", &expired_key_id, None, None)
                .unwrap(),
            KeyPolicyUpdate::Updated,
        );
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct", "key_missing", None, None)
                .unwrap(),
            KeyPolicyUpdate::NotFound,
        );
        pg.key_issue_with_policy("disabled-key", "policy-acct", None, None, None)
            .unwrap();
        pg.key_set_status("disabled-key", "disabled").unwrap();
        assert_eq!(
            pg.reserve_request(
                &owner,
                "policy-disabled",
                "policy-acct",
                "disabled-key",
                1,
                60
            )
            .unwrap(),
            None
        );

        pg.account_create("concurrent-policy-acct", None, 10_000)
            .unwrap();
        pg.account_topup(
            "concurrent-policy-acct",
            1_000,
            Some("concurrent-policy-seed"),
        )
        .unwrap();
        pg.key_issue_with_policy(
            "concurrent-limited-key",
            "concurrent-policy-acct",
            None,
            Some(700),
            None,
        )
        .unwrap();
        let policy_barrier = Arc::new(Barrier::new(3));
        let mut policy_joins = Vec::new();
        for n in 0..2 {
            let url = url.clone();
            let owner = owner.clone();
            let barrier = Arc::clone(&policy_barrier);
            policy_joins.push(std::thread::spawn(move || {
                let mut connection = PgStore::connect(&url).unwrap();
                let request_id = format!("concurrent-policy-{n}");
                barrier.wait();
                let result = connection
                    .reserve_request(
                        &owner,
                        &request_id,
                        "concurrent-policy-acct",
                        "concurrent-limited-key",
                        400,
                        60,
                    )
                    .unwrap();
                (request_id, result)
            }));
        }
        policy_barrier.wait();
        let policy_results: Vec<_> = policy_joins
            .into_iter()
            .map(|join| join.join().unwrap())
            .collect();
        assert_eq!(
            policy_results
                .iter()
                .filter(|(_, result)| result.is_some())
                .count(),
            1,
            "concurrent reservations must not jointly cross a key cap"
        );
        for (request_id, result) in policy_results {
            if result.is_some() {
                pg.cancel_request(&request_id).unwrap();
            }
        }
        assert_eq!(
            pg.key_get("concurrent-limited-key")
                .unwrap()
                .unwrap()
                .reserved_nano,
            0
        );

        // A reserve racing a stricter policy replacement must serialize on the key row. The two
        // incompatible operations can never both succeed.
        pg.account_create("policy-update-race-acct", None, 10_000)
            .unwrap();
        pg.account_topup(
            "policy-update-race-acct",
            1_000,
            Some("policy-update-race-seed"),
        )
        .unwrap();
        pg.key_issue_with_policy(
            "policy-update-race-key",
            "policy-update-race-acct",
            None,
            Some(1_000),
            None,
        )
        .unwrap();
        let race_key_id = pg
            .key_get("policy-update-race-key")
            .unwrap()
            .unwrap()
            .key_id;
        let race_barrier = Arc::new(Barrier::new(3));
        let reserve_url = url.clone();
        let reserve_owner = owner.clone();
        let reserve_barrier = Arc::clone(&race_barrier);
        let reserve_join = std::thread::spawn(move || {
            let mut connection = PgStore::connect(&reserve_url).unwrap();
            reserve_barrier.wait();
            connection
                .reserve_request(
                    &reserve_owner,
                    "policy-update-race-request",
                    "policy-update-race-acct",
                    "policy-update-race-key",
                    400,
                    60,
                )
                .unwrap()
                .is_some()
        });
        let update_url = url.clone();
        let update_barrier = Arc::clone(&race_barrier);
        let update_join = std::thread::spawn(move || {
            let mut connection = PgStore::connect(&update_url).unwrap();
            update_barrier.wait();
            connection
                .key_set_policy_by_id("policy-update-race-acct", &race_key_id, Some(300), None)
                .unwrap()
                == KeyPolicyUpdate::Updated
        });
        race_barrier.wait();
        let reserve_won = reserve_join.join().unwrap();
        let update_won = update_join.join().unwrap();
        assert_ne!(
            reserve_won, update_won,
            "exactly one incompatible racing operation must succeed"
        );
        let raced_key = pg.key_get("policy-update-race-key").unwrap().unwrap();
        if let Some(limit) = raced_key.spend_limit_nano {
            assert!(raced_key.spent_nano + raced_key.reserved_nano <= limit);
        }
        assert_eq!(
            pg.account_get("policy-update-race-acct")
                .unwrap()
                .unwrap()
                .reserved_nano,
            raced_key.reserved_nano,
        );
        if reserve_won {
            pg.cancel_request("policy-update-race-request").unwrap();
        }

        assert_eq!(
            pg.reserve_request(&owner, "req-1", "acct", "key", 600, 60)
                .unwrap(),
            Some(400)
        );
        assert_eq!(
            pg.reserve_request(&owner, "req-1", "acct", "key", 600, 60)
                .unwrap(),
            Some(400)
        );
        assert!(pg.mark_delivering(&owner, "req-1", 60).unwrap());
        let usage = UsageEventInput {
            model: "claude-test".into(),
            input_tokens: 10,
            output_tokens: 20,
            real_nano: 200,
            ..Default::default()
        };
        assert_eq!(
            pg.settle_request("req-1", 250, Some("anthropic-1"), Some(&usage))
                .unwrap(),
            Some(750)
        );
        assert_eq!(
            pg.settle_request("req-1", 250, Some("anthropic-1"), Some(&usage))
                .unwrap(),
            Some(750)
        );
        assert!(pg
            .settle_request("req-1", 251, Some("anthropic-1"), Some(&usage))
            .is_err());
        let charge_count: i64 = pg
            .client
            .query_one(
                "SELECT COUNT(*)::bigint FROM ledger WHERE kind='charge' AND request_id='req-1'",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(charge_count, 1, "exact retry must not double-charge");

        assert_eq!(
            pg.reserve_request(&owner, "req-2", "acct", "key", 300, 60)
                .unwrap(),
            Some(450)
        );
        assert_eq!(pg.cancel_request("req-2").unwrap(), Some(750));
        assert_eq!(pg.cancel_request("req-2").unwrap(), Some(750));

        // Crash boundary: enqueue commits but settlement application has not run. A fresh connection
        // drains the durable row exactly once.
        assert_eq!(
            pg.reserve_request(&owner, "req-3", "acct", "key", 400, 60)
                .unwrap(),
            Some(350)
        );
        assert!(pg.mark_delivering(&owner, "req-3", 60).unwrap());
        pg.enqueue_settlement("req-3", 100, Some("anthropic-3"), None)
            .unwrap();
        drop(pg);
        let mut pg = PgStore::connect(&url).unwrap();
        assert_eq!(pg.drain_outbox(100).unwrap(), 1);
        assert_eq!(pg.drain_outbox(100).unwrap(), 0);
        let account = pg.account_get("acct").unwrap().unwrap();
        assert_eq!(
            (
                account.balance_nano,
                account.spent_nano,
                account.reserved_nano
            ),
            (650, 350, 0)
        );

        // Овердрафт-буфер ($1): funded-запрос НЕ роняем из-за гонки — баланс может уйти в лёгкий минус
        // до пола −$1 (−1e9 nano), но НИКОГДА ниже; за полом любой положительный hold отбит. (`owner`
        // ещё валиден — фенсинг ниже.)
        pg.account_create("od-acct", None, 10_000).unwrap();
        pg.key_issue("od-key", "od-acct", None).unwrap();
        pg.account_topup("od-acct", 1_000, Some("od-seed")).unwrap();
        // hold ≫ баланса, но в пределах balance+$1 → овердрафт пускает; баланс → −$0.999999.
        assert_eq!(
            pg.reserve_request(&owner, "od-1", "od-acct", "od-key", 1_000_000_000, 60)
                .unwrap(),
            Some(-999_999_000)
        );
        // добираем РОВНО до пола −$1 (граница включительно)
        assert_eq!(
            pg.reserve_request(&owner, "od-2", "od-acct", "od-key", 1_000, 60)
                .unwrap(),
            Some(-1_000_000_000)
        );
        // на полу −$1 любой положительный hold отбит (защита от бесконечного долга)
        assert_eq!(
            pg.reserve_request(&owner, "od-3", "od-acct", "od-key", 1, 60)
                .unwrap(),
            None
        );
        // на свежем аккаунте hold СВЕРХ balance+$1 → отказ (за буфером), обычный в пределах — ок
        pg.account_create("od-acct2", None, 10_000).unwrap();
        pg.key_issue("od-key2", "od-acct2", None).unwrap();
        pg.account_topup("od-acct2", 1_000, Some("od-seed2"))
            .unwrap(); // balance = 1000 nano
        assert_eq!(
            pg.reserve_request(&owner, "od-4", "od-acct2", "od-key2", 1_000_002_000, 60)
                .unwrap(),
            None
        );
        assert_eq!(
            pg.reserve_request(&owner, "od-5", "od-acct2", "od-key2", 1_000, 60)
                .unwrap(),
            Some(0)
        );
        // Снимаем наши holds → reserved_nano аккаунтов обратно в 0 (глобальный billing_totals ниже ждёт 0).
        pg.cancel_request("od-1").unwrap();
        pg.cancel_request("od-2").unwrap();
        pg.cancel_request("od-5").unwrap();

        // A later epoch with the same instance identity fences the stale writer.
        let owner2 = pg.claim_instance("engine-a", 60).unwrap();
        assert!(owner2.epoch > owner.epoch);
        assert!(pg
            .reserve_request(&owner, "stale", "acct", "key", 1, 60)
            .is_err());
        assert_eq!(
            pg.reserve_request(&owner2, "req-4", "acct", "key", 100, 60)
                .unwrap(),
            Some(550)
        );
        pg.cancel_request("req-4").unwrap();

        // Recovery distinguishes a request never delivered (refund) from a delivered response whose
        // exact usage was lost (conservatively charge the already approved hold).
        let dead = pg.claim_instance("dead-engine", 60).unwrap();
        pg.reserve_request(&dead, "req-5", "acct", "key", 100, 1)
            .unwrap();
        pg.reserve_request(&dead, "req-6", "acct", "key", 100, 1)
            .unwrap();
        pg.mark_delivering(&dead, "req-6", 1).unwrap();
        pg.client
            .execute(
                "UPDATE engine_instances SET lease_until=0 WHERE instance_id='dead-engine'",
                &[],
            )
            .unwrap();
        pg.client
            .execute(
                "UPDATE reservations SET lease_until=0 WHERE request_id IN ('req-5','req-6')",
                &[],
            )
            .unwrap();
        let recovered = pg.reconcile_expired(100).unwrap();
        assert_eq!(recovered.canceled_before_delivery, 1);
        assert_eq!(recovered.charged_after_delivery, 1);
        assert_eq!(pg.account_get("acct").unwrap().unwrap().reserved_nano, 0);

        // Pool state is versioned CAS and fenced by owner epoch.
        let mut state = pg.load_pool_state().unwrap();
        assert_eq!(state.len(), 1);
        let stale_state = state.clone();
        let versions = pg.save_pool_state(&owner2, &state).unwrap();
        assert_eq!(versions[0].1, 1);
        assert!(pg.save_pool_state(&owner2, &stale_state).is_err());
        state[0].version = versions[0].1;
        assert!(pg.save_pool_state(&owner2, &state).is_ok());

        // Atomic capacity transaction: every concurrent contender receives a tracked lease.
        let barrier = Arc::new(Barrier::new(9));
        let mut joins = Vec::new();
        for n in 0..8 {
            let url = url.clone();
            let owner = owner2.clone();
            let barrier = Arc::clone(&barrier);
            joins.push(std::thread::spawn(move || {
                let mut c = PgStore::connect(&url).unwrap();
                barrier.wait();
                c.acquire_capacity(
                    &owner,
                    &format!("lease-{n}"),
                    &format!("capacity-{n}"),
                    "sub@test",
                    60,
                    0.95,
                )
                .unwrap()
            }));
        }
        barrier.wait();
        let leases: Vec<_> = joins
            .into_iter()
            .filter_map(|j| j.join().unwrap())
            .collect();
        assert_eq!(
            leases.len(),
            8,
            "capacity tracking must not reject concurrency"
        );
        assert_eq!(pg.pool_inflight("sub@test").unwrap(), Some(8));
        for lease in &leases {
            assert!(pg.release_capacity(&owner2, &lease.lease_id).unwrap());
        }
        for lease in &leases {
            assert!(!pg.release_capacity(&owner2, &lease.lease_id).unwrap());
        }
        assert_eq!(pg.pool_inflight("sub@test").unwrap(), Some(0));

        // One PostgreSQL lease-epoch leader at a time; there is no Redlock path.
        let peer = pg.claim_instance("engine-b", 60).unwrap();
        assert!(pg.acquire_leader(&owner2, "poller", 60).unwrap());
        assert!(!pg.acquire_leader(&peer, "poller", 60).unwrap());

        let totals = pg.billing_totals().unwrap();
        assert_eq!(totals.reserved_nano, 0);
        let aggregate: i64 = pg.client.query_one(
            "SELECT COALESCE(SUM(hold_nano),0)::bigint FROM reservations WHERE state NOT IN ('settled','canceled')",
            &[],
        ).unwrap().get(0);
        assert_eq!(aggregate, 0);

        // Cross-authority conservation: commerce-originated topups/adjustments are the only
        // funding source, while the engine may retain them as balance, completed spend, or an
        // in-flight hold. Pin this per account so opposing errors cannot cancel in a global sum.
        const DIVERGENCE_SQL: &str = "\
            WITH funding AS ( \
              SELECT account_id, COALESCE(SUM(amount_nano),0)::bigint AS funded_nano \
              FROM ledger WHERE kind IN ('topup','adjust') GROUP BY account_id \
            ) \
            SELECT COALESCE(MAX(ABS( \
              a.balance_nano + a.spent_nano + a.reserved_nano \
              - COALESCE(f.funded_nano,0) \
            )),0)::bigint \
            FROM accounts a LEFT JOIN funding f ON f.account_id=a.id";
        let divergence: i64 = pg.client.query_one(DIVERGENCE_SQL, &[]).unwrap().get(0);
        assert_eq!(
            divergence, 0,
            "every account must conserve all durable funding"
        );

        let hold_mismatches: i64 = pg
            .client
            .query_one(
                "WITH holds AS ( \
                   SELECT account_id,COALESCE(SUM(hold_nano),0)::bigint AS held_nano \
                   FROM reservations WHERE state NOT IN ('settled','canceled') GROUP BY account_id \
                 ) \
                 SELECT COUNT(*)::bigint FROM accounts a LEFT JOIN holds h ON h.account_id=a.id \
                 WHERE a.reserved_nano <> COALESCE(h.held_nano,0)",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(
            hold_mismatches, 0,
            "reserved aggregates must equal their source holds"
        );

        // Prove the production gauge's equation is sensitive rather than a zero-valued tautology.
        pg.client.batch_execute("BEGIN").unwrap();
        pg.client
            .execute(
                "UPDATE accounts SET balance_nano=balance_nano+17 WHERE id='acct'",
                &[],
            )
            .unwrap();
        let corrupted: i64 = pg.client.query_one(DIVERGENCE_SQL, &[]).unwrap().get(0);
        assert_eq!(corrupted, 17);
        pg.client.batch_execute("ROLLBACK").unwrap();
        let restored: i64 = pg.client.query_one(DIVERGENCE_SQL, &[]).unwrap().get(0);
        assert_eq!(restored, 0);
        lock_holder
            .client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }

    /// PostgreSQL contract of the panel health read. Skipped without a live database:
    /// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
    /// pg::tests::postgres_settlement_health_contract`
    #[test]
    fn postgres_settlement_health_contract() {
        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping PostgreSQL settlement health contract: \
                 CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let mut pg = PgStore::connect(&url).unwrap();
        pg.client
            .batch_execute("SET statement_timeout=0; SET lock_timeout=0")
            .unwrap();
        pg.client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        pg.client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .unwrap();
        pg.migrate().unwrap();
        pg.client
            .batch_execute(
                "TRUNCATE settlement_outbox,reservations,capacity_leases,leader_leases,
                 engine_instances,usage_events,ledger,api_keys,accounts,pool_state,subs
                 RESTART IDENTITY CASCADE",
            )
            .unwrap();

        // Пустая БД: нули и отсутствие лага.
        let empty = pg.settlement_health(300, "pricing").unwrap();
        assert_eq!(empty.pending + empty.done + empty.failed + empty.backlog, 0);
        assert_eq!(empty.ledger_consumer.checkpoints, 0);

        pg.account_create("health-account", None, 2_000).unwrap();
        pg.account_topup("health-account", 1_000_000, None).unwrap();
        let ts = now();
        let mut seed =
            |request_id: &str, state: &str, error: Option<&str>, created: i64, updated: i64| {
                // outbox ссылается на reservations(request_id) — сеем обе строки согласованно.
                pg.client
                    .execute(
                        "INSERT INTO reservations(request_id,account_id,key,hold_nano,state, \
                     balance_after_reserve_nano,owner_instance,owner_epoch,lease_until, \
                     created_ts,updated_ts) \
                     VALUES($1,'health-account','k',1000,'settled',0,'health-test',1,$2,$3,$4)",
                        &[&request_id, &created, &created, &updated],
                    )
                    .unwrap();
                pg.client
                    .execute(
                        "INSERT INTO settlement_outbox(request_id,actual_nano,disposition,state, \
                     attempts,next_attempt_ts,last_error,created_ts,updated_ts) \
                     VALUES($1,1000,'settle',$2,3,0,$3,$4,$5)",
                        &[&request_id, &state, &error, &created, &updated],
                    )
                    .unwrap();
            };
        seed("r-done", "done", None, ts - 100, ts - 90);
        seed(
            "r-pending-old",
            "pending",
            Some("transient"),
            ts - 3600,
            ts - 60,
        );
        seed(
            "r-failed",
            "failed",
            Some(&"x".repeat(500)),
            ts - 7200,
            ts - 30,
        );

        let h = pg.settlement_health(300, "pricing").unwrap();
        assert_eq!((h.pending, h.done, h.failed), (1, 1, 1));
        assert_eq!(h.failed_24h, 1);
        assert_eq!(h.pending_with_error, 1);
        assert_eq!(h.backlog, 1);
        assert_eq!(h.oldest_unsettled_ts, ts - 3600);
        assert_eq!(h.recent_failed.len(), 1);
        assert_eq!(
            h.recent_failed[0]
                .last_error
                .as_deref()
                .unwrap()
                .chars()
                .count(),
            200,
            "last_error урезан до 200 символов, как и в SQLite-twin"
        );

        // Watermark ниже max(ledger.id) → виден лаг и возраст старейшей неподтверждённой строки.
        pg.ledger_ack("pricing", "health-account", 0).unwrap();
        let h = pg.settlement_health(300, "pricing").unwrap();
        let lag = &h.ledger_consumer;
        assert_eq!(lag.checkpoints, 1);
        assert_eq!(lag.checkpoint_min, 0);
        assert!(lag.ledger_max_id > 0);
        assert_eq!(lag.unacked, 1, "topup-строка выше watermark'а");
        assert!(lag.oldest_unacked_ts > 0);

        pg.client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }

    /// Real PostgreSQL parity for the Stage 6 content-addressed planner/apply contract. Skipped
    /// unless the dedicated destructive test database is supplied.
    #[test]
    fn postgres_funding_reconciliation_contract() {
        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping PostgreSQL funding reconciliation contract: \
                 CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let mut pg = PgStore::connect(&url).unwrap();
        pg.client
            .batch_execute("SET statement_timeout=0; SET lock_timeout=0")
            .unwrap();
        pg.client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        pg.client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .unwrap();
        pg.migrate().unwrap();
        pg.client
            .batch_execute("TRUNCATE accounts RESTART IDENTITY CASCADE")
            .unwrap();

        pg.account_create("funding-pg", None, 10_000).unwrap();
        pg.client
            .execute(
                "INSERT INTO account_policy_bindings( \
                   account_id,product_id,account_class,active_effective_version,policy_enforcement, \
                   funding_enforcement,reconciliation_state,updated_ts \
                 ) VALUES('funding-pg','main','b2c',NULL,'shadow','legacy_single','pending',1)",
                &[],
            )
            .unwrap();
        pg.account_topup(
            "funding-pg",
            crate::funding::WELCOME_TRACK_BONUS_NANO,
            Some("signup-bonus:pg-user"),
        )
        .unwrap();
        pg.account_topup("funding-pg", 10_000_000_000, Some("cryptomus:pg-paid"))
            .unwrap();
        let balance: i64 = pg
            .client
            .query_one(
                "UPDATE accounts SET balance_nano=balance_nano-5000000000, \
                 spent_nano=spent_nano+5000000000 WHERE id='funding-pg' RETURNING balance_nano",
                &[],
            )
            .unwrap()
            .get(0);
        pg.client
            .execute(
                "INSERT INTO ledger(account_id,kind,amount_nano,balance_after_nano,ts) \
                 VALUES('funding-pg','charge',5000000000,$1,1)",
                &[&balance],
            )
            .unwrap();

        let plan = pg.funding_reconciliation_plan().unwrap();
        assert_eq!((plan.ready_accounts, plan.exception_accounts), (1, 0));
        let applied = pg
            .apply_funding_reconciliation(&plan.plan_digest, false)
            .unwrap();
        assert_eq!(applied.inserted_buckets, 2);
        let totals = pg
            .client
            .query_one(
                "SELECT SUM(balance_nano)::bigint, \
                        SUM(balance_nano) FILTER (WHERE source_type='paid')::bigint \
                 FROM funding_buckets WHERE account_id='funding-pg'",
                &[],
            )
            .unwrap();
        assert_eq!(totals.get::<_, i64>(0), 9_000_000_000);
        assert_eq!(totals.get::<_, i64>(1), 9_000_000_000);
        let replay = pg.funding_reconciliation_plan().unwrap();
        assert_eq!(replay.replay_accounts, 1);

        pg.account_create("funding-promo", None, 10_000).unwrap();
        pg.client
            .execute(
                "INSERT INTO account_policy_bindings( \
                   account_id,product_id,account_class,active_effective_version,policy_enforcement, \
                   funding_enforcement,reconciliation_state,updated_ts \
                 ) VALUES('funding-promo','main','b2c',NULL,'shadow','legacy_single','pending',1)",
                &[],
            )
            .unwrap();
        pg.account_topup("funding-promo", 2_000_000_000, Some("promo:pg-legacy"))
            .unwrap();
        let exception = pg.funding_reconciliation_plan().unwrap();
        assert_eq!(exception.exception_accounts, 1);
        assert!(pg
            .apply_funding_reconciliation(&exception.plan_digest, false)
            .is_err());
        let exception_applied = pg
            .apply_funding_reconciliation(&exception.plan_digest, true)
            .unwrap();
        assert_eq!(exception_applied.exception_accounts, 1);
        let restricted_balance: i64 = pg
            .client
            .query_one(
                "SELECT balance_nano FROM funding_buckets WHERE account_id='funding-promo' \
                 AND source_type='legacy_restricted'",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(restricted_balance, 2_000_000_000);

        pg.account_create("funding-drift", None, 10_000).unwrap();
        pg.client
            .execute(
                "INSERT INTO account_policy_bindings( \
                   account_id,product_id,account_class,active_effective_version,policy_enforcement, \
                   funding_enforcement,reconciliation_state,updated_ts \
                 ) VALUES('funding-drift','main','b2c',NULL,'shadow','legacy_single','pending',1)",
                &[],
            )
            .unwrap();
        pg.account_topup("funding-drift", 10, Some("platega:before-plan"))
            .unwrap();
        let approved = pg.funding_reconciliation_plan().unwrap();
        pg.account_topup("funding-drift", 1, Some("platega:after-plan"))
            .unwrap();
        assert!(pg
            .apply_funding_reconciliation(&approved.plan_digest, true)
            .is_err());
        let bucket_count: i64 = pg
            .client
            .query_one(
                "SELECT COUNT(*)::bigint FROM funding_buckets WHERE account_id='funding-drift'",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(bucket_count, 0);

        pg.client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }

    /// Real PostgreSQL proof that the additive Control API readers preserve the same funding and
    /// immutable ledger evidence as the SQLite path.
    #[test]
    fn postgres_account_funding_and_ledger_read_contract() {
        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping PostgreSQL funding/ledger read contract: \
                 CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let mut pg = PgStore::connect(&url).unwrap();
        pg.client
            .batch_execute("SET statement_timeout=0; SET lock_timeout=0")
            .unwrap();
        pg.client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        pg.client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .unwrap();
        pg.migrate().unwrap();
        pg.client
            .batch_execute(
                "TRUNCATE accounts RESTART IDENTITY CASCADE;
                 INSERT INTO accounts(
                     id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status,
                     created_ts,created
                 ) VALUES('read-account','read-user',900,300,40,5000,'active',1,'');
                 INSERT INTO account_policy_bindings(
                     account_id,product_id,account_class,active_effective_version,
                     policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
                 ) VALUES('read-account','main','b2c',NULL,'shadow','shadow','verified',1);
                 INSERT INTO funding_buckets(
                     bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                     reserved_nano,spent_nano,version,status,created_ts,updated_ts
                 ) VALUES
                     ('read-paid','read-account','paid','payment:read','any',700,40,0,2,
                      'active',1,2),
                     ('read-bonus','read-account','welcome_track_bonus','welcome','track',200,0,
                      300,2,'active',1,2);
                 INSERT INTO ledger(
                     account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts,model,
                     provider,official_nano,attribution_schema_version,snapshot_kind,product_id,
                     account_class,requested_model_id,canonical_model_id,served_model_id,
                     served_canonical_model_id,alias_generation,rule_id,rule_digest,rule_scope,
                     pricing_mode,rule_origin,payable_multiplier_bp,policy_id,policy_version,
                     effective_policy_version,policy_digest,catalog_generation,switch_generation,
                     tariff_schedule_id,tariff_priced_ts,official_cost_json,paid_funded_nano,
                     bonus_funded_nano,other_funded_nano,funding_allocation_json,track_eligible,
                     retention_eligible,commission_eligible,snapshot_digest,source_policy_digest,
                     admission_catalog_generation,admission_catalog_digest,
                     admission_switch_generation,admission_switch_digest,
                     runtime_manifest_generation,runtime_manifest_digest
                 ) VALUES(
                     'read-account','read-key','charge','read-request',300,'provider:read',900,2,
                     'claude-read','anthropic',600,1,'policy_v1','main','b2c','claude-read',
                     'claude-read','claude-read','claude-read',1,'read-rule','read-rule-digest',
                     'provider','track','managed',5000,'read-policy',1,1,'read-policy-digest',1,1,
                     'read-tariff',2,
                     '{\"schema_version\":1,\"provider\":\"anthropic\",\"official_nano\":600}'::jsonb,
                     0,300,0,
                     '[{\"bucket_id\":\"read-bonus\",\"source_type\":\"welcome_track_bonus\",\"bucket_version\":1,\"reserved_nano\":300,\"charged_nano\":300,\"released_nano\":0,\"allocation_order\":1}]'::jsonb,
                     true,true,true,'read-snapshot','read-source-policy',1,'read-catalog',1,
                     'read-switch',1,'read-runtime'
                 );
                 INSERT INTO ledger_funding_allocations(
                     ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
                     direction,amount_nano
                 ) SELECT id,'read-account','read-bonus','welcome_track_bonus',1,'debit',300
                     FROM ledger WHERE request_id='read-request';",
            )
            .unwrap();

        let snapshot = pg
            .account_funding_snapshot("read-account")
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.account.balance_nano, 900);
        assert_eq!(
            (
                snapshot.funding.account_class,
                snapshot.funding.funding_enforcement,
                snapshot.funding.reconciliation_state,
                snapshot.funding.bucket_count,
                snapshot.funding.paid_balance_nano,
                snapshot.funding.bonus_balance_nano,
                snapshot.funding.unattributed_balance_nano,
                snapshot.funding.paid_reserved_nano,
                snapshot.funding.unattributed_reserved_nano,
                snapshot.funding.bonus_spent_nano,
                snapshot.funding.unattributed_spent_nano,
            ),
            (
                Some(crate::pricing::AccountClass::B2c),
                Some(crate::pricing::FundingEnforcement::Shadow),
                Some(crate::pricing::ReconciliationState::Verified),
                2,
                700,
                200,
                0,
                40,
                0,
                300,
                0,
            )
        );
        let recent = pg.ledger_recent("read-account", 10).unwrap();
        let after = pg.ledger_after("read-account", 0, 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(after.len(), 1);
        assert_eq!(recent[0].id, after[0].id);
        assert_eq!(recent[0].request_id.as_deref(), Some("read-request"));
        let attribution = recent[0].attribution.as_ref().unwrap();
        assert_eq!(attribution.snapshot_kind.as_deref(), Some("policy_v1"));
        assert_eq!(
            (
                attribution.source_policy_digest.as_deref(),
                attribution.admission_catalog_digest.as_deref(),
                attribution.runtime_manifest_digest.as_deref(),
                attribution.bonus_funded_nano,
            ),
            (
                Some("read-source-policy"),
                Some("read-catalog"),
                Some("read-runtime"),
                Some(300),
            )
        );
        assert_eq!(
            recent[0].funding_allocations,
            vec![LedgerFundingAllocation {
                bucket_id: "read-bonus".into(),
                source_type: "welcome_track_bonus".into(),
                source_ref: "welcome".into(),
                bucket_version: 1,
                direction: "debit".into(),
                amount_nano: 300,
                allocation_order: None,
            }]
        );

        pg.client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }

    /// Real PostgreSQL proof that the Stage 9 expansion fences scalar money writers and unsafe
    /// key/cutover transitions before the policy-aware runtime is activated.
    #[test]
    fn postgres_stage9_strict_enforcement_guards() {
        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping PostgreSQL Stage 9 strict guards: \
                 CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let mut pg = PgStore::connect(&url).unwrap();
        pg.client
            .batch_execute("SET statement_timeout=0; SET lock_timeout=0")
            .unwrap();
        pg.client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        pg.client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .unwrap();
        pg.migrate().unwrap();
        pg.client
            .batch_execute(
                "TRUNCATE engine_instances RESTART IDENTITY CASCADE;
                 ALTER SEQUENCE engine_owner_epoch_seq RESTART WITH 2;
                 INSERT INTO engine_instances(
                     instance_id,owner_epoch,lease_until,started_ts,updated_ts
                 ) VALUES('stage9-guard-engine',1,9999999999,1,1);
                 TRUNCATE accounts RESTART IDENTITY CASCADE;
                 TRUNCATE pricing_catalog_versions,provider_switch_versions
                 RESTART IDENTITY CASCADE;
                 INSERT INTO accounts(
                     id,balance_nano,reserved_nano,mult_bp,status,created_ts,created
                 ) VALUES
                     ('stage9-strict',1000,0,2000,'active',1,''),
                     ('stage9-cutover',1000,0,2000,'active',1,'');
                 INSERT INTO pricing_catalog_versions(
                     product_id,generation,schema_version,capability_generation,
                     capability_digest,content_digest,created_ts
                 ) VALUES('stage9-product',1,1,1,'stage9-capability','stage9-catalog-v1',1);
                 INSERT INTO pricing_catalog_entries(
                     product_id,generation,provider_id,canonical_model_id,enabled
                 ) VALUES('stage9-product',1,'anthropic','claude-stage9',true);
                 INSERT INTO pricing_catalog_heads(product_id,active_generation,updated_ts)
                 VALUES('stage9-product',1,1);
                 INSERT INTO provider_switch_versions(
                     generation,schema_version,capability_generation,capability_digest,
                     content_digest,created_ts
                 ) VALUES(1,1,1,'stage9-capability','stage9-switch-v1',1);
                 INSERT INTO provider_switch_entries(
                     generation,provider_id,scope_type,product_id,segment,catalog_generation,enabled
                 ) VALUES
                     (1,'anthropic','master','','',NULL,true),
                     (1,'anthropic','segment','stage9-product','b2c',1,true);
                 INSERT INTO provider_switch_head(singleton,active_generation,updated_ts)
                 VALUES(1,1,1);
                 INSERT INTO account_policy_versions(
                     account_id,effective_version,policy_id,policy_version,source_policy_digest,
                     owner_type,owner_id,account_class,product_id,schema_version,
                     catalog_generation,switch_generation,content_digest,replacement_locked,created_ts
                 ) VALUES
                     (
                         'stage9-strict',1,'stage9-policy',1,'stage9-source-strict-v1',
                         'global_b2c','global','b2c','stage9-product',1,1,1,
                         'stage9-policy-strict-v1',false,1
                     ),
                     (
                         'stage9-cutover',1,'stage9-policy',1,'stage9-source-cutover-v1',
                         'global_b2c','global','b2c','stage9-product',1,1,1,
                         'stage9-policy-cutover-v1',false,1
                     ),
                     (
                         'stage9-cutover',2,'stage9-policy',2,'stage9-source-cutover-v2',
                         'global_b2c','global','b2c','stage9-product',1,1,1,
                         'stage9-policy-cutover-v2',false,2
                     );
                 INSERT INTO account_policy_rules(
                     account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                     canonical_model_id,pricing_mode,rule_origin,discount_bps,
                     payable_multiplier_bp,track_eligible,retention_eligible,commission_eligible
                 ) VALUES
                     (
                         'stage9-strict',1,'stage9-rule','stage9-rule-strict-v1','provider',
                         'anthropic',NULL,'track','managed',NULL,10000,true,true,false
                     ),
                     (
                         'stage9-cutover',1,'stage9-rule','stage9-rule-cutover-v1','provider',
                         'anthropic',NULL,'track','managed',NULL,10000,true,true,false
                     ),
                     (
                         'stage9-cutover',2,'stage9-rule','stage9-rule-cutover-v2','provider',
                         'anthropic',NULL,'track','managed',NULL,10000,true,true,false
                     );
                 INSERT INTO funding_buckets(
                     bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                     reserved_nano,spent_nano,version,status,created_ts,updated_ts
                 ) VALUES
                     (
                         'stage9-strict-paid','stage9-strict','paid','primary','any',
                         1000,0,0,1,'active',1,1
                     ),
                     (
                         'stage9-cutover-paid','stage9-cutover','paid','primary','any',
                         1000,0,0,1,'active',1,1
                     );
                 INSERT INTO api_keys(
                     key,key_id,account_id,status,created_ts,created,
                     activation_policy_effective_version,activation_policy_digest,
                     activation_policy_ack_ts
                 ) VALUES
                     (
                         'stage9-strict-key','key_stage9_strict','stage9-strict','active',1,'',
                         1,'stage9-policy-strict-v1',1
                     ),
                     (
                         'stage9-cutover-key','key_stage9_cutover','stage9-cutover','active',1,'',
                         NULL,NULL,NULL
                     );
                 INSERT INTO account_policy_bindings(
                     account_id,product_id,account_class,active_effective_version,
                     policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
                 ) VALUES (
                         'stage9-cutover','stage9-product','b2c',1,
                         'shadow','legacy_single','verified',1
                     );",
            )
            .unwrap();

        let runtime_floor_error = pg
            .client
            .execute(
                "INSERT INTO account_policy_bindings(
                     account_id,product_id,account_class,active_effective_version,
                     policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
                 ) VALUES(
                     'stage9-strict','stage9-product','b2c',1,
                     'strict','strict','verified',1
                 )",
                &[],
            )
            .expect_err("a live policy-incapable engine unexpectedly allowed strict cutover");
        assert_eq!(
            runtime_floor_error.as_db_error().unwrap().message(),
            "strict pricing activation requires policy-incapable engine instances to drain"
        );
        pg.client
            .execute(
                "UPDATE engine_instances
                 SET pricing_schema_version=1,
                     pricing_runtime_manifest_generation=1,
                     pricing_runtime_manifest_digest='stage9-runtime-v1'
                 WHERE instance_id='stage9-guard-engine'",
                &[],
            )
            .unwrap();
        pg.client
            .execute(
                "INSERT INTO account_policy_bindings(
                     account_id,product_id,account_class,active_effective_version,
                     policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
                 ) VALUES(
                     'stage9-strict','stage9-product','b2c',1,
                     'strict','strict','verified',1
                 )",
                &[],
            )
            .unwrap();

        // Deferred parity allows one atomic policy-aware transaction to move both aggregates.
        pg.client
            .batch_execute(
                "BEGIN;
                 UPDATE accounts SET balance_nano=balance_nano+25
                 WHERE id='stage9-strict';
                 UPDATE funding_buckets
                 SET balance_nano=balance_nano+25,version=version+1
                 WHERE bucket_id='stage9-strict-paid';
                 COMMIT;",
            )
            .unwrap();
        let parity: (i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,b.balance_nano
                     FROM accounts a
                     JOIN funding_buckets b ON b.account_id=a.id
                     WHERE a.id='stage9-strict'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1))
        };
        assert_eq!(parity, (1025, 1025));

        assert_postgres_batch_rejected(
            &mut pg.client,
            "BEGIN;
             UPDATE funding_buckets SET balance_nano=balance_nano+1
             WHERE bucket_id='stage9-strict-paid';
             COMMIT;",
            "strict funding buckets do not match account aggregates",
        );
        let bucket_balance: i64 = pg
            .client
            .query_one(
                "SELECT balance_nano FROM funding_buckets
                 WHERE bucket_id='stage9-strict-paid'",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(bucket_balance, 1025);

        // The runtime top-up now dual-writes the strict paid source and aggregate atomically;
        // exact replay remains one monetary operation. A compensating adjustment uses its own
        // paid-source evidence and restores the original total without hiding either ledger row.
        assert_eq!(
            pg.account_topup("stage9-strict", 1, Some("stage9-strict-topup"))
                .unwrap(),
            Some(1026)
        );
        assert_eq!(
            pg.account_topup("stage9-strict", 1, Some("stage9-strict-topup"))
                .unwrap(),
            Some(1026)
        );
        assert_eq!(
            pg.account_topup("stage9-strict", -1, Some("stage9-strict-adjust"))
                .unwrap(),
            Some(1025)
        );
        let strict_topup_state: (i64, i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT
                         (SELECT balance_nano FROM accounts WHERE id='stage9-strict'),
                         (SELECT COALESCE(SUM(balance_nano),0)::bigint FROM funding_buckets
                          WHERE account_id='stage9-strict'),
                         (SELECT COUNT(*)::bigint FROM ledger
                          WHERE ref IN ('stage9-strict-topup','stage9-strict-adjust')),
                         (SELECT COUNT(*)::bigint FROM ledger_funding_allocations allocation
                          JOIN ledger ON ledger.id=allocation.ledger_id
                          WHERE ledger.ref IN ('stage9-strict-topup','stage9-strict-adjust'))",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2), row.get(3))
        };
        assert_eq!(strict_topup_state, (1025, 1025, 2, 2));

        let owner = Owner {
            instance_id: "stage9-guard-engine".to_owned(),
            epoch: 1,
        };
        let scalar_snapshot = legacy_snapshot("stage9-scalar-reserve", "stage9-strict", 100, 20);
        assert!(
            pg.reserve_request_with_legacy_snapshot(
                &owner,
                "stage9-strict-key",
                60,
                &scalar_snapshot,
            )
            .is_err()
        );
        let scalar_reserve_state: (i64, i64, i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT
                         (SELECT balance_nano FROM accounts WHERE id='stage9-strict'),
                         (SELECT reserved_nano FROM accounts WHERE id='stage9-strict'),
                         (SELECT reserved_nano FROM api_keys WHERE key='stage9-strict-key'),
                         (SELECT COUNT(*)::bigint FROM reservations
                          WHERE request_id='stage9-scalar-reserve'),
                         (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots
                          WHERE request_id='stage9-scalar-reserve')",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4))
        };
        assert_eq!(scalar_reserve_state, (1025, 0, 0, 0, 0));

        let strict_admission_ts = now();
        let strict_snapshot = crate::pricing::PolicyAdmissionSnapshot::new(
            crate::pricing::PolicyAdmissionSnapshotInput {
                request_id: "stage9-runtime-reserve".into(),
                account_id: "stage9-strict".into(),
                provider: crate::pricing::SnapshotProvider::Anthropic,
                product_id: "stage9-product".into(),
                account_class: crate::pricing::AccountClass::B2c,
                requested_model_id: "claude-stage9".into(),
                canonical_model_id: "claude-stage9".into(),
                alias_generation: 1,
                rule_id: "stage9-rule".into(),
                rule_digest: "stage9-rule-strict-v1".into(),
                rule_scope: crate::pricing::PolicyRuleScope::Provider {
                    provider_id: "anthropic".into(),
                },
                pricing_mode: crate::pricing::PricingMode::Track,
                rule_origin: crate::pricing::RuleOrigin::Managed,
                discount_bps: None,
                payable_multiplier_bp: 10_000,
                policy_id: "stage9-policy".into(),
                policy_version: 1,
                effective_policy_version: 1,
                source_policy_digest: "stage9-source-strict-v1".into(),
                policy_digest: "stage9-policy-strict-v1".into(),
                policy_catalog_generation: 1,
                policy_switch_generation: 1,
                admission_catalog_generation: 1,
                admission_catalog_digest: "stage9-catalog-v1".into(),
                admission_switch_generation: 1,
                admission_switch_digest: "stage9-switch-v1".into(),
                runtime_manifest_generation: 1,
                runtime_manifest_digest: "stage9-runtime-v1".into(),
                tariff_schedule_id: "stage9-tariff-v1".into(),
                tariff_priced_ts: strict_admission_ts,
                admission_ts: strict_admission_ts,
                official_hold_nano: 100,
                charged_hold_nano: 100,
                track_eligible: true,
                retention_eligible: true,
                commission_eligible: false,
                premium_modifiers: crate::pricing::LegacyPremiumModifiers::AnthropicV1 {
                    speed: crate::pricing::SnapshotAnthropicSpeed::Standard,
                    inference_geo: crate::pricing::SnapshotAnthropicInferenceGeo::Global,
                    inference_geo_basis_points: 10_000,
                },
            },
        )
        .unwrap();
        assert!(matches!(
            pg.reserve_request_with_policy_snapshot(
                &owner,
                "stage9-strict-key",
                60,
                &strict_snapshot,
            )
            .unwrap(),
            crate::pricing::PolicyReserveOutcome::Inserted(_)
        ));
        assert_eq!(
            pg.cancel_request("stage9-runtime-reserve").unwrap(),
            Some(1025)
        );
        let runtime_reserve_state: (String, i64, i64, String, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT reservation.state,allocation.charged_nano,allocation.released_nano,
                        outbox.snapshot_kind,outbox.runtime_manifest_generation
                   FROM reservations reservation
                   JOIN reservation_funding_allocations allocation USING(request_id)
                   JOIN settlement_outbox outbox USING(request_id)
                  WHERE reservation.request_id='stage9-runtime-reserve'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4))
        };
        assert_eq!(
            runtime_reserve_state,
            ("canceled".into(), 0, 100, "policy_v1".into(), 1)
        );

        // Seed a valid strict reservation atomically, then prove the scalar settlement path cannot
        // move aggregate money without terminalizing the exact source-bucket allocation.
        pg.client
            .batch_execute(
                "BEGIN;
                 UPDATE accounts
                 SET balance_nano=balance_nano-100,reserved_nano=reserved_nano+100
                 WHERE id='stage9-strict';
                 UPDATE api_keys SET reserved_nano=reserved_nano+100
                 WHERE key='stage9-strict-key';
                 UPDATE funding_buckets
                 SET balance_nano=balance_nano-100,reserved_nano=reserved_nano+100,
                     version=version+1
                 WHERE bucket_id='stage9-strict-paid';
                 INSERT INTO reservations(
                     request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                     owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts
                 ) VALUES(
                     'stage9-strict-request','stage9-strict','stage9-strict-key',100,925,
                     'stage9-guard-engine',1,9999999999,'reserved',1,1
                 );
                 INSERT INTO pricing_admission_snapshots(
                     request_id,account_id,snapshot_kind,schema_version,provider_id,product_id,
                     account_class,requested_model_id,canonical_model_id,alias_generation,
                     rule_id,rule_digest,rule_scope,pricing_mode,rule_origin,discount_bps,
                     payable_multiplier_bp,policy_id,policy_version,effective_policy_version,
                     policy_digest,catalog_generation,switch_generation,tariff_schedule_id,
                     tariff_priced_ts,admission_ts,official_hold_nano,charged_hold_nano,
                     track_eligible,retention_eligible,commission_eligible,premium_modifiers,
                     snapshot_digest,source_policy_digest,admission_catalog_generation,
                     admission_catalog_digest,admission_switch_generation,
                     admission_switch_digest,runtime_manifest_generation,runtime_manifest_digest
                 ) VALUES(
                     'stage9-strict-request','stage9-strict','policy_v1',1,'anthropic',
                     'stage9-product','b2c','claude-stage9','claude-stage9',1,
                     'stage9-rule','stage9-rule-strict-v1','provider','track','managed',NULL,
                     10000,'stage9-policy',1,1,'stage9-policy-strict-v1',1,1,
                     'stage9-tariff-v1',1,1,100,100,true,true,false,'{}'::jsonb,
                     'stage9-snapshot-v1','stage9-source-strict-v1',1,'stage9-catalog-v1',
                     1,'stage9-switch-v1',1,'stage9-runtime-v1'
                 );
                 INSERT INTO reservation_funding_allocations(
                     request_id,account_id,bucket_id,bucket_version,reserved_nano,
                     charged_nano,released_nano,allocation_order
                 )
                 SELECT
                     'stage9-strict-request','stage9-strict',bucket_id,version,100,NULL,NULL,1
                 FROM funding_buckets WHERE bucket_id='stage9-strict-paid';
                 COMMIT;",
            )
            .unwrap();
        assert!(pg
            .settle_request(
                "stage9-strict-request",
                60,
                Some("stage9-scalar-settle"),
                None
            )
            .is_err());
        let scalar_settlement_state: (i64, i64, i64, i64, i64, String, Option<i64>, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT
                         a.balance_nano,a.reserved_nano,
                         b.balance_nano,b.reserved_nano,
                         k.reserved_nano,r.state,r.actual_nano,
                         (SELECT COUNT(*)::bigint FROM ledger
                          WHERE request_id='stage9-strict-request')
                     FROM accounts a
                     JOIN funding_buckets b ON b.account_id=a.id
                     JOIN api_keys k ON k.account_id=a.id
                     JOIN reservations r ON r.account_id=a.id
                     WHERE a.id='stage9-strict'
                       AND b.bucket_id='stage9-strict-paid'
                       AND k.key='stage9-strict-key'
                       AND r.request_id='stage9-strict-request'",
                    &[],
                )
                .unwrap();
            (
                row.get(0),
                row.get(1),
                row.get(2),
                row.get(3),
                row.get(4),
                row.get(5),
                row.get(6),
                row.get(7),
            )
        };
        assert_eq!(
            scalar_settlement_state,
            (925, 100, 925, 100, 100, "reserved".into(), None, 0,)
        );
        pg.client
            .batch_execute(
                "BEGIN;
                 UPDATE accounts
                 SET balance_nano=balance_nano+40,reserved_nano=reserved_nano-100,
                     spent_nano=spent_nano+60
                 WHERE id='stage9-strict';
                 UPDATE api_keys
                 SET reserved_nano=reserved_nano-100,spent_nano=spent_nano+60
                 WHERE key='stage9-strict-key';
                 UPDATE funding_buckets
                 SET balance_nano=balance_nano+40,reserved_nano=reserved_nano-100,
                     spent_nano=spent_nano+60,version=version+1
                 WHERE bucket_id='stage9-strict-paid';
                 UPDATE reservation_funding_allocations
                 SET charged_nano=60,released_nano=40
                 WHERE request_id='stage9-strict-request';
                 UPDATE reservations
                 SET state='settled',actual_nano=60,settled_ts=2,updated_ts=2
                 WHERE request_id='stage9-strict-request';
                 UPDATE settlement_outbox
                 SET state='done',committed_ts=2,updated_ts=2,
                     source_policy_digest='stage9-source-strict-v1',
                     admission_catalog_generation=1,
                     admission_catalog_digest='stage9-catalog-v1',
                     admission_switch_generation=1,
                     admission_switch_digest='stage9-switch-v1',
                     runtime_manifest_generation=1,
                     runtime_manifest_digest='stage9-runtime-v1'
                 WHERE request_id='stage9-strict-request';
                 INSERT INTO ledger(
                     account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts,
                     source_policy_digest,admission_catalog_generation,
                     admission_catalog_digest,admission_switch_generation,
                     admission_switch_digest,runtime_manifest_generation,
                     runtime_manifest_digest
                 ) VALUES(
                     'stage9-strict','stage9-strict-key','charge','stage9-strict-request',60,
                     'stage9-policy-settle',965,2,'stage9-source-strict-v1',1,
                     'stage9-catalog-v1',1,'stage9-switch-v1',1,'stage9-runtime-v1'
                 );
                 COMMIT;",
            )
            .unwrap();
        let policy_settlement_state: (i64, i64, i64, i64, i64, i64, i64, String) = {
            let row = pg
                .client
                .query_one(
                    "SELECT
                         a.balance_nano,a.reserved_nano,
                         b.balance_nano,b.reserved_nano,
                         allocation.charged_nano,allocation.released_nano,
                         (SELECT COUNT(*)::bigint FROM ledger
                          WHERE request_id='stage9-strict-request'),
                         reservation.state
                     FROM accounts a
                     JOIN funding_buckets b ON b.account_id=a.id
                     JOIN reservations reservation ON reservation.account_id=a.id
                     JOIN reservation_funding_allocations allocation
                       ON allocation.request_id=reservation.request_id
                     WHERE a.id='stage9-strict'
                       AND b.bucket_id='stage9-strict-paid'
                       AND reservation.request_id='stage9-strict-request'",
                    &[],
                )
                .unwrap();
            (
                row.get(0),
                row.get(1),
                row.get(2),
                row.get(3),
                row.get(4),
                row.get(5),
                row.get(6),
                row.get(7),
            )
        };
        assert_eq!(
            policy_settlement_state,
            (965, 0, 965, 0, 60, 40, 1, "settled".into())
        );

        let incapable_claim = pg
            .claim_instance("stage9-policy-incapable-rollback", 600)
            .expect_err("a policy-incapable engine unexpectedly claimed an epoch after strict");
        assert!(
            format!("{incapable_claim:#}")
                .contains("strict pricing requires a policy-capable engine runtime manifest"),
            "unexpected incapable claim error: {incapable_claim:#}"
        );
        let compatible_manifest = crate::pricing::PricingRuntimeManifestEvidence::new(
            1,
            vec![crate::pricing::PricingRuntimeCapabilityEvidence::new(
                crate::pricing::PRICING_SCHEMA_VERSION,
                1,
                "stage9-capability",
            )
            .unwrap()],
        )
        .unwrap();
        let capable_owner = pg
            .claim_instance_with_pricing_manifest(
                "stage9-policy-capable-runtime",
                600,
                &compatible_manifest,
            )
            .unwrap();
        assert!(pg
            .heartbeat_instance_with_pricing_manifest(&capable_owner, 600, &compatible_manifest,)
            .unwrap());
        let unsupported_manifest = crate::pricing::PricingRuntimeManifestEvidence::new(
            2,
            vec![crate::pricing::PricingRuntimeCapabilityEvidence::new(
                crate::pricing::PRICING_SCHEMA_VERSION,
                2,
                "unsupported-stage9-capability",
            )
            .unwrap()],
        )
        .unwrap();
        let unsupported_claim = pg
            .claim_instance_with_pricing_manifest(
                "stage9-policy-unsupported-runtime",
                600,
                &unsupported_manifest,
            )
            .expect_err("unsupported pricing runtime unexpectedly claimed an owner epoch");
        assert!(format!("{unsupported_claim:#}")
            .contains("does not support every active strict dependency"));
        assert!(!pg
            .heartbeat_instance_with_pricing_manifest(&capable_owner, 600, &unsupported_manifest,)
            .unwrap());

        // First strict cutover fails with an unstamped active key.
        let cutover_error = pg
            .client
            .execute(
                "UPDATE account_policy_bindings
                 SET policy_enforcement='strict',funding_enforcement='strict',updated_ts=2
                 WHERE account_id='stage9-cutover'",
                &[],
            )
            .expect_err("unstamped key unexpectedly allowed strict cutover");
        assert_eq!(
            cutover_error.as_db_error().unwrap().message(),
            "strict binding activation requires every active key to carry the exact policy ACK"
        );
        pg.client
            .execute(
                "UPDATE api_keys
                 SET activation_policy_effective_version=1,
                     activation_policy_digest='stage9-policy-cutover-v1',
                     activation_policy_ack_ts=1
                 WHERE key='stage9-cutover-key'",
                &[],
            )
            .unwrap();

        // Even with keys stamped, an active legacy reservation must drain before strict cutover.
        pg.client
            .batch_execute(
                "INSERT INTO reservations(
                     request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                     owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts
                 ) VALUES(
                     'stage9-cutover-legacy','stage9-cutover','stage9-cutover-key',10,990,
                     'stage9-guard-engine',1,9999999999,'reserved',1,1
                 );
                 INSERT INTO pricing_admission_snapshots(
                     request_id,account_id,snapshot_kind,schema_version,provider_id,
                     requested_model_id,canonical_model_id,alias_generation,pricing_mode,
                     rule_origin,payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,
                     admission_ts,official_hold_nano,charged_hold_nano,premium_modifiers,
                     snapshot_digest
                 ) VALUES(
                     'stage9-cutover-legacy','stage9-cutover','legacy_scalar',1,'anthropic',
                     'claude-stage9','claude-stage9',1,'legacy_scalar','legacy',2000,
                     'stage9-tariff-v1',1,1,50,10,'{}'::jsonb,'stage9-legacy-snapshot'
                 );",
            )
            .unwrap();
        let legacy_error = pg
            .client
            .execute(
                "UPDATE account_policy_bindings
                 SET policy_enforcement='strict',funding_enforcement='strict',updated_ts=3
                 WHERE account_id='stage9-cutover'",
                &[],
            )
            .expect_err("legacy reservation unexpectedly allowed strict cutover");
        assert_eq!(
            legacy_error.as_db_error().unwrap().message(),
            "strict binding activation requires legacy reservations to drain"
        );
        pg.client
            .execute(
                "DELETE FROM reservations WHERE request_id='stage9-cutover-legacy'",
                &[],
            )
            .unwrap();

        // A dormant policy snapshot is not enough: cutover also verifies its exact allocation.
        pg.client
            .batch_execute(
                "INSERT INTO reservations(
                     request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                     owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts
                 ) VALUES(
                     'stage9-cutover-incomplete','stage9-cutover','stage9-cutover-key',10,990,
                     'stage9-guard-engine',1,9999999999,'reserved',1,1
                 );
                 INSERT INTO pricing_admission_snapshots(
                     request_id,account_id,snapshot_kind,schema_version,provider_id,product_id,
                     account_class,requested_model_id,canonical_model_id,alias_generation,
                     rule_id,rule_digest,rule_scope,pricing_mode,rule_origin,discount_bps,
                     payable_multiplier_bp,policy_id,policy_version,effective_policy_version,
                     policy_digest,catalog_generation,switch_generation,tariff_schedule_id,
                     tariff_priced_ts,admission_ts,official_hold_nano,charged_hold_nano,
                     track_eligible,retention_eligible,commission_eligible,premium_modifiers,
                     snapshot_digest,source_policy_digest,admission_catalog_generation,
                     admission_catalog_digest,admission_switch_generation,
                     admission_switch_digest,runtime_manifest_generation,runtime_manifest_digest
                 ) VALUES(
                     'stage9-cutover-incomplete','stage9-cutover','policy_v1',1,'anthropic',
                     'stage9-product','b2c','claude-stage9','claude-stage9',1,
                     'stage9-rule','stage9-rule-cutover-v1','provider','track','managed',NULL,
                     10000,'stage9-policy',1,1,'stage9-policy-cutover-v1',1,1,
                     'stage9-tariff-v1',1,1,50,10,true,true,false,'{}'::jsonb,
                     'stage9-incomplete-snapshot','stage9-source-cutover-v1',1,
                     'stage9-catalog-v1',1,'stage9-switch-v1',1,'stage9-runtime-v1'
                 );",
            )
            .unwrap();
        let incomplete_error = pg
            .client
            .execute(
                "UPDATE account_policy_bindings
                 SET policy_enforcement='strict',funding_enforcement='strict',updated_ts=4
                 WHERE account_id='stage9-cutover'",
                &[],
            )
            .expect_err("incomplete policy allocation unexpectedly allowed strict cutover");
        assert_eq!(
            incomplete_error.as_db_error().unwrap().message(),
            "strict reservation funding allocation is incomplete or ineligible"
        );
        pg.client
            .execute(
                "DELETE FROM reservations WHERE request_id='stage9-cutover-incomplete'",
                &[],
            )
            .unwrap();
        assert_eq!(
            pg.client
                .execute(
                    "UPDATE account_policy_bindings
                     SET policy_enforcement='strict',funding_enforcement='strict',updated_ts=5
                     WHERE account_id='stage9-cutover'",
                    &[],
                )
                .unwrap(),
            1
        );

        // New issue/reactivation requires the exact active policy ACK.
        assert!(pg
            .key_issue("stage9-unstamped-key", "stage9-cutover", None)
            .is_err());
        let unstamped_count: i64 = pg
            .client
            .query_one(
                "SELECT COUNT(*)::bigint FROM api_keys WHERE key='stage9-unstamped-key'",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(unstamped_count, 0);
        pg.client
            .execute(
                "INSERT INTO api_keys(key,key_id,account_id,status,created_ts,created)
                 VALUES(
                     'stage9-reactivate-key','key_stage9_reactivate','stage9-cutover',
                     'inactive',1,''
                 )",
                &[],
            )
            .unwrap();
        let wrong_ack = pg
            .client
            .execute(
                "UPDATE api_keys
                 SET status='active',activation_policy_effective_version=1,
                     activation_policy_digest='wrong-policy-digest',activation_policy_ack_ts=1
                 WHERE key='stage9-reactivate-key'",
                &[],
            )
            .expect_err("wrong policy ACK unexpectedly activated a strict key");
        assert_eq!(
            wrong_ack.as_db_error().unwrap().message(),
            "strict key activation requires the exact active policy ACK"
        );
        assert_eq!(
            pg.client
                .execute(
                    "UPDATE api_keys
                     SET status='active',activation_policy_effective_version=1,
                         activation_policy_digest='stage9-policy-cutover-v1',
                         activation_policy_ack_ts=1
                     WHERE key='stage9-reactivate-key'",
                    &[],
                )
                .unwrap(),
            1
        );

        // Strict-to-strict policy replacement leaves already-active keys usable, while their next
        // activation must acknowledge the new exact policy.
        assert_eq!(
            pg.client
                .execute(
                    "UPDATE account_policy_bindings
                     SET active_effective_version=2,updated_ts=6
                     WHERE account_id='stage9-cutover'",
                    &[],
                )
                .unwrap(),
            1
        );
        pg.client
            .execute(
                "UPDATE api_keys SET status='inactive' WHERE key='stage9-reactivate-key'",
                &[],
            )
            .unwrap();
        let stale_ack = pg
            .client
            .execute(
                "UPDATE api_keys SET status='active' WHERE key='stage9-reactivate-key'",
                &[],
            )
            .expect_err("stale policy ACK unexpectedly reactivated a strict key");
        assert_eq!(
            stale_ack.as_db_error().unwrap().message(),
            "strict key activation requires the exact active policy ACK"
        );
        assert_eq!(
            pg.client
                .execute(
                    "UPDATE api_keys
                     SET status='active',activation_policy_effective_version=2,
                         activation_policy_digest='stage9-policy-cutover-v2',
                         activation_policy_ack_ts=2
                     WHERE key='stage9-reactivate-key'",
                    &[],
                )
                .unwrap(),
            1
        );

        pg.client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }
}
