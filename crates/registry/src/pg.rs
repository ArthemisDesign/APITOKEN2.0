//! PostgreSQL authority for the engine.
//!
//! All correctness-sensitive mutations are transactions. Request IDs and lease IDs are the
//! idempotency boundary; owner epochs fence stale instances. PostgreSQL is the recovery floor.

use crate::{
    mask_proxy, AccountRow, AnthropicCalibrationRow,
    AnthropicWindowObservation, BillingTotals, ClaudeLifecycleProfile, CodexCalibrationRow,
    CodexHomeCalibrationSpend, CodexTurnCalibrationAggregate, CodexTurnCalibrationEvent,
    CodexWindowObservation, GeminiCalibrationRow, GeminiExactCalibrationRow,
    GeminiExactWindowObservation, GeminiWindowObservation, GlmCalibrationRow, GlmSubjectSpend,
    GlmTurnCalibrationEvent, GlmWindowObservation, KeyAuth, KeyPolicyUpdate, KeyRow,
    KimiCalibrationRow, KimiTurnCalibrationEvent, KimiWindowObservation,
    LedgerConsumerLag, LedgerRow, PoolStateRow,
    ProviderCalibrationSubjectSpend, ProviderTurnCalibrationAggregate,
    ProviderTurnCalibrationEvent, SettlementFailure, SettlementHealth, SpendAccountAgg,
    SpendModelAgg, SpendProviderAgg, Sub, SubAdmin, SubHealth, SubRow, UsageDailyAgg,
    UsageDailyProviderAgg, UsageEventInput, UsageKeyAgg, UsageModelAgg, UsageReport,
    ACCOUNT_OVERDRAFT_NANO, PROVIDER_OPENAI,
};
use anyhow::{bail, Context, Result};
use postgres::config::{Host, SslMode};
use postgres::{Client, IsolationLevel, Row, Transaction};
use std::collections::HashSet;
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

fn pg_gemini_exact_calibration_row(row: &Row) -> GeminiExactCalibrationRow {
    GeminiExactCalibrationRow {
        profile_id: row.get(0),
        plan: row.get(1),
        bucket_id: row.get(2),
        window_kind: row.get(3),
        window_duration_mins: row.get(4),
        resets_at: row.get(5),
        anchor_used_fraction_units: row.get(6),
        anchor_resolution_fraction_units: row.get(7),
        anchor_spend_nano: row.get(8),
        used_fraction_units: row.get(9),
        measurement_resolution_fraction_units: row.get(10),
        observed_at: row.get(11),
        observed_fraction_units: row.get(12),
        observed_spend_nano: row.get(13),
        samples: row.get(14),
        unattributed_fraction_units: row.get(15),
        current_capacity_nano: row.get(16),
        current_low_nano: row.get(17),
        current_high_nano: row.get(18),
        current_confidence_bp: row.get(19),
        last_measured_at: row.get(20),
        estimator_version: row.get(21),
        version: row.get(22),
        updated_ts: row.get(23),
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

// As above, type the version placeholder explicitly so postgres accepts the Rust i64 parameter.
const GEMINI_EXACT_CALIBRATION_INSERT_SQL: &str = "INSERT INTO gemini_exact_window_calibrations(\
       profile_id,plan,bucket_id,window_kind,window_duration_mins,resets_at,\
       anchor_used_fraction_units,anchor_resolution_fraction_units,anchor_spend_nano,\
       used_fraction_units,measurement_resolution_fraction_units,observed_at,\
       observed_fraction_units,observed_spend_nano,samples,unattributed_fraction_units,\
       current_capacity_nano,current_low_nano,current_high_nano,current_confidence_bp,\
       last_measured_at,estimator_version,version,updated_ts) \
     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,\
            $20,$21,$22,($23::bigint)+1,$24) \
     ON CONFLICT(profile_id,plan,bucket_id) DO NOTHING";

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
const MIGRATION_0021: &str = include_str!("../migrations_pg/0021_execution_group_fencing.sql");
const MIGRATION_0022: &str =
    include_str!("../migrations_pg/0022_gemini_exact_window_calibration.sql");
const MIGRATION_0023: &str = include_str!("../migrations_pg/0023_pricing_release_funding_v2.sql");
const MIGRATION_0024: &str =
    include_str!("../migrations_pg/0024_pre_cutover_funding_snapshots_v2.sql");
const MIGRATION_0025: &str =
    include_str!("../migrations_pg/0025_pricing_release_runtime_epoch_fence.sql");
const MIGRATION_0026: &str =
    include_str!("../migrations_pg/0026_pricing_release_zero_drain_extensions.sql");
const MIGRATION_0027: &str = include_str!("../migrations_pg/0027_kimi_window_calibration.sql");
const MIGRATION_0028: &str =
    include_str!("../migrations_pg/0028_pricing_ledger_release_v2_attribution.sql");
const MIGRATION_0029: &str = include_str!("../migrations_pg/0029_glm_window_calibration.sql");
const MIGRATION_0030: &str =
    include_str!("../migrations_pg/0030_pricing_release_policy_override_extensions.sql");
const MIGRATION_0031: &str =
    include_str!("../migrations_pg/0031_pricing_request_snapshots_extension_lineage.sql");
const MIGRATION_0032: &str = include_str!("../migrations_pg/0032_pool_member_disables.sql");
const MIGRATION_0033: &str = include_str!("../migrations_pg/0033_pool_member_hidden.sql");
const MIGRATION_0034: &str =
    include_str!("../migrations_pg/0034_pricing_release_b2c_to_b2b_extensions.sql");
const MIGRATION_0035: &str =
    include_str!("../migrations_pg/0035_pricing_release_successor_activation.sql");
const MIGRATION_0036: &str =
    include_str!("../migrations_pg/0036_pricing_tariff_overrides.sql");
const MIGRATION_0037: &str =
    include_str!("../migrations_pg/0037_pricing_tariff_override_family_dots.sql");
const MIGRATION_0038: &str =
    include_str!("../migrations_pg/0038_reservation_measured_cost.sql");
const MIGRATION_0039: &str =
    include_str!("../migrations_pg/0039_pricing_release_opt_out.sql");
const MIGRATION_0040: &str =
    include_str!("../migrations_pg/0040_service_meter_only_strict.sql");
const MIGRATION_0041: &str = include_str!("../migrations_pg/0041_strict_funding_lots_v2.sql");
const MIGRATION_0042: &str =
    include_str!("../migrations_pg/0042_strict_funding_active_generation.sql");
const MIGRATION_0043: &str =
    include_str!("../migrations_pg/0043_account_provider_discounts.sql");
const MIGRATION_0044: &str =
    include_str!("../migrations_pg/0044_retire_pricing_design_triggers.sql");
const MIGRATION_0045: &str =
    include_str!("../migrations_pg/0045_retire_policy_runtime_floor.sql");
const MIGRATION_0046: &str =
    include_str!("../migrations_pg/0046_account_discount_contract.sql");

/// Highest PostgreSQL schema version understood by this engine build.
pub const CURRENT_SCHEMA_VERSION: i64 = 46;
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
    (21, MIGRATION_0021),
    (22, MIGRATION_0022),
    (23, MIGRATION_0023),
    (24, MIGRATION_0024),
    (25, MIGRATION_0025),
    (26, MIGRATION_0026),
    (27, MIGRATION_0027),
    (28, MIGRATION_0028),
    (29, MIGRATION_0029),
    (30, MIGRATION_0030),
    (31, MIGRATION_0031),
    (32, MIGRATION_0032),
    (33, MIGRATION_0033),
    (34, MIGRATION_0034),
    (35, MIGRATION_0035),
    (36, MIGRATION_0036),
    (37, MIGRATION_0037),
    (38, MIGRATION_0038),
    (39, MIGRATION_0039),
    (40, MIGRATION_0040),
    (41, MIGRATION_0041),
    (42, MIGRATION_0042),
    (43, MIGRATION_0043),
    (44, MIGRATION_0044),
    (45, MIGRATION_0045),
    (46, MIGRATION_0046),
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
    let provider = crate::resolve_ledger_provider(
        row.get::<_, Option<String>>(9),
        row.get::<_, Option<String>>(11),
    )?;
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
        official_nano: row.get(10),
    })
}

const POSTGRES_LEDGER_READ_COLUMNS: &str = "
    ledger.id,ledger.key,ledger.kind,ledger.request_id,ledger.amount_nano,ledger.ref,
    ledger.balance_after_nano,ledger.ts,ledger.model,ledger.provider,ledger.official_nano,
    CASE
      WHEN ledger.kind<>'charge' THEN NULL
      WHEN ledger.request_id IS NOT NULL THEN (
        SELECT CASE
          WHEN COUNT(*) > 0
           AND COUNT(NULLIF(candidate.provider,''))=COUNT(*)
           AND COUNT(DISTINCT NULLIF(candidate.provider,''))=1
          THEN MIN(NULLIF(candidate.provider,''))
        END
          FROM usage_events candidate
         WHERE candidate.account_id=ledger.account_id
           AND candidate.request_id=ledger.request_id
      )
      ELSE (
        SELECT CASE
          WHEN COUNT(*) > 0
           AND COUNT(NULLIF(candidate.provider,''))=COUNT(*)
           AND COUNT(DISTINCT NULLIF(candidate.provider,''))=1
          THEN MIN(NULLIF(candidate.provider,''))
        END
          FROM usage_events candidate
         WHERE candidate.account_id=ledger.account_id
           AND candidate.request_id IS NULL
           AND candidate.key IS NOT DISTINCT FROM ledger.key
           AND candidate.charge_nano=ledger.amount_nano
           AND candidate.ref IS NOT DISTINCT FROM ledger.ref
           AND candidate.model IS NOT DISTINCT FROM ledger.model
           AND ABS(candidate.ts-ledger.ts)<=1
      )
    END";

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

/// Existing service credential selected for one authenticated GPT Image 2 public smoke.
///
/// The raw API key deliberately has no public field and this type implements neither `Debug` nor
/// serialization. Server composition may borrow it only long enough to call our own public origin.
pub struct OpenAiImageSmokeCredential {
    key: String,
    pub key_id: String,
    pub account_id: String,
    pub balance_nano: i64,
    pub spent_nano: i64,
    pub reserved_nano: i64,
    pub key_spent_nano: i64,
    pub key_reserved_nano: i64,
}

impl OpenAiImageSmokeCredential {
    pub fn authorization_key(&self) -> &str {
        &self.key
    }
}

/// Secret-free, authoritative terminal evidence for one public image request.
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct OpenAiImageSettlementEvidence {
    pub request_id: String,
    pub account_id: String,
    pub key_id: String,
    pub reservation_state: String,
    pub reservation_hold_nano: i64,
    pub reservation_actual_nano: Option<i64>,
    pub outbox_state: String,
    pub outbox_disposition: String,
    pub model: String,
    pub provider: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub real_nano: i64,
    pub charge_nano: i64,
    pub input_nano: i64,
    pub output_nano: i64,
    pub cache_read_nano: i64,
    pub priced_ts: i64,
    pub balance_nano: i64,
    pub spent_nano: i64,
    pub reserved_nano: i64,
    pub key_spent_nano: i64,
    pub key_reserved_nano: i64,
}

/// Identifier-free read-only progress snapshot for one fenced GPT Image 2 settlement.
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct OpenAiImageSettlementDiagnostic {
    pub schema_version: u32,
    pub status: &'static str,
    pub reservation_present: bool,
    pub reservation_state: Option<String>,
    pub reservation_hold_nano: Option<i64>,
    pub reservation_actual_nano: Option<i64>,
    pub snapshot_present: bool,
    pub release_generation: Option<i64>,
    pub release_billing_mode: Option<String>,
    pub snapshot_openai: bool,
    pub snapshot_canonical_model: bool,
    pub snapshot_tariff: bool,
    pub snapshot_requested_model: bool,
    pub snapshot_generation_controls: bool,
    pub official_hold_nano: Option<i64>,
    pub charged_hold_nano: Option<i64>,
    pub outbox_present: bool,
    pub outbox_state: Option<String>,
    pub outbox_disposition: Option<String>,
    pub outbox_attempts: Option<i64>,
    pub outbox_has_error: bool,
    pub usage_present: bool,
    pub usage_openai: bool,
    pub usage_model: bool,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub real_nano: Option<i64>,
    pub charge_nano: Option<i64>,
    pub input_nano: Option<i64>,
    pub output_nano: Option<i64>,
    pub cache_read_nano: Option<i64>,
    pub priced_ts: Option<i64>,
    pub account_present: bool,
    pub key_present: bool,
}

pub struct PgStore {
    pub(crate) client: Client,
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
        self.reserve_request_for_execution(
            owner,
            request_id,
            account_id,
            key,
            hold_nano,
            lease_secs,
            &crate::ExecutionAttempt::direct(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn reserve_request_for_execution(
        &mut self,
        owner: &Owner,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold_nano: i64,
        lease_secs: i64,
        execution: &crate::ExecutionAttempt,
    ) -> Result<Option<i64>> {
        let hold = hold_nano.max(0);
        let preflight_ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, preflight_ts)?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&request_id],
        )?;
        Self::assert_owner_locked(&mut tx, owner, now())?;
        if let Some(row) = tx.query_opt(
            "SELECT account_id,key,hold_nano,balance_after_reserve_nano,owner_instance,owner_epoch, \
                    state,group_id,attempt \
             FROM reservations WHERE request_id=$1",
            &[&request_id],
        )? {
            let exact = row.get::<_, String>(0) == account_id
                && row.get::<_, String>(1) == key
                && row.get::<_, i64>(2) == hold
                && row.get::<_, String>(4) == owner.instance_id
                && row.get::<_, i64>(5) == owner.epoch
                && row.get::<_, String>(6) == "reserved"
                && row.get::<_, Option<String>>(7).as_deref() == execution.group_id()
                && row.get::<_, i32>(8) == execution.attempt();
            if !exact {
                bail!("reservation request ID belongs to a different or completed operation");
            }
            let balance = row.get(3);
            Self::assert_owner_locked(&mut tx, owner, now())?;
            tx.commit()?;
            return Ok(Some(balance));
        }
        // Овердрафт-буфер: funded-запрос НЕ роняем из-за гонки конкурентных резервов. Пускаем, пока
        // ПОСЛЕ-баланс не ниже пола −OVERDRAFT_NANO (`balance-hold >= -OVERDRAFT` ⇔ `balance >= hold-OVERDRAFT`).
        // Гейт атомарен на строке аккаунта → суммарный баланс НИКОГДА не уходит ниже −$1 даже под
        // конкуренцией (каждый успешный резерв гарантирует post_balance ≥ −$1; за полом любой h>0 отбит).
        // Стоимость: аккаунт может получить максимум $1 в долг (per-account, не per-request) — принятый
        // размен на «ноль ложных 402». Синхронно с `metering::OVERDRAFT_NANO`.
        // Write the gate as `balance >= hold-overdraft` with explicit bigint casts. Besides keeping
        // parameter inference stable, this avoids overflowing a very large stored balance merely
        // to decide whether a small hold fits.
        let reservation_ts = now();
        Self::assert_owner_locked(&mut tx, owner, reservation_ts)?;
        let Some(row) = tx.query_opt(
            "UPDATE accounts SET balance_nano=balance_nano-$1, reserved_nano=reserved_nano+$1 \
             WHERE id=$2 AND status='active' \
               AND balance_nano >= $1::bigint - $3::bigint RETURNING balance_nano",
            &[&hold, &account_id, &ACCOUNT_OVERDRAFT_NANO],
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
             owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts,group_id,attempt) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,'reserved',$9,$9,$10,$11)",
            &[&request_id, &account_id, &key, &hold, &balance, &owner.instance_id,
              &owner.epoch, &(reservation_ts.saturating_add(lease_secs.max(1))), &reservation_ts,
              &execution.group_id(),
              &execution.attempt()],
        )?;
        Self::assert_owner_locked(&mut tx, owner, now())?;
        tx.commit()?;
        Ok(Some(balance))
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

    /// Record what the in-flight turn has cost so far, so a death that prevents settlement no longer
    /// forces the reconciler to charge the ceiling.
    ///
    /// Monotonic by construction: a checkpoint never lowers an earlier one. Provider usage arrives
    /// as a cumulative snapshot, but frames can be reordered under retry and an annotation-only
    /// frame can parse as zero — either would otherwise erase a real measurement and hand the
    /// customer's turn back to the full-hold fallback. Fenced on the owning epoch like every other
    /// mutation, so a superseded generation cannot rewrite a live reservation.
    pub fn checkpoint_measured(
        &mut self,
        owner: &Owner,
        request_id: &str,
        measured_nano: i64,
    ) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        let changed = tx.execute(
            "UPDATE reservations \
                SET measured_nano=GREATEST(COALESCE(measured_nano,0),$4), updated_ts=$3 \
              WHERE request_id=$1 AND owner_instance=$2 AND owner_epoch=$5 \
                AND state IN ('reserved','delivering')",
            &[
                &request_id,
                &owner.instance_id,
                &ts,
                &measured_nano.max(0),
                &owner.epoch,
            ],
        )?;
        tx.commit()?;
        Ok(changed == 1)
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
        let account_id: String = tx
            .query_opt(
                "SELECT account_id FROM reservations WHERE request_id=$1",
                &[&request_id],
            )?
            .context("settlement reservation does not exist")?
            .get(0);
        let reservation = tx
            .query_opt(
                "SELECT account_id,hold_nano,state,actual_nano
                   FROM reservations WHERE request_id=$1 FOR UPDATE",
                &[&request_id],
            )?
            .context("settlement reservation does not exist")?;
        if reservation.get::<_, String>(0) != account_id {
            bail!("settlement reservation account changed while acquiring funding lock");
        }
        let state: String = reservation.get(2);
        let terminal_actual = reservation.get::<_, Option<i64>>(3);
        if matches!(state.as_str(), "settled" | "canceled") {
            let terminal_actual =
                terminal_actual.context("terminal reservation lacks actual amount")?;
            let _ = terminal_actual;
        }
        let actual = actual_nano.max(0);
        let u = usage.cloned().unwrap_or_default();
        let (
            release_schema_version,
            release_generation,
            release_digest,
            release_billing_mode,
            release_funding_generation,
            release_snapshot_digest,
        ): (
            Option<i64>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
        ) = (None, None, None, None, None, None);
        let inserted = tx.execute(
            "INSERT INTO settlement_outbox(request_id,actual_nano,disposition,reference,model,input_tokens, \
             output_tokens,cache_read_tokens,cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests, \
             real_nano,speed,inference_geo,input_nano,output_nano,cache_read_nano,cache_write_5m_nano, \
             cache_write_1h_nano,web_search_nano,priced_ts,provider,release_schema_version,
             release_generation,release_digest,release_billing_mode,release_funding_generation,
             release_snapshot_digest,state,created_ts,updated_ts) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22, \
                    $23,$24,$25,$26,$27,$28,'pending',$29,$29) \
             ON CONFLICT(request_id) DO NOTHING",
            &[&request_id, &actual, &disposition, &reference, &u.model, &u.input_tokens,
              &u.output_tokens, &u.cache_read_tokens, &u.cache_write_5m_tokens,
              &u.cache_write_1h_tokens, &u.web_search_requests, &u.real_nano, &u.speed,
              &u.inference_geo, &u.input_nano, &u.output_nano, &u.cache_read_nano,
              &u.cache_write_5m_nano, &u.cache_write_1h_nano, &u.web_search_nano, &u.priced_ts,
              &u.provider, &release_schema_version, &release_generation, &release_digest,
              &release_billing_mode, &release_funding_generation, &release_snapshot_digest, &ts],
        )?;
        if inserted == 0 {
            let row = tx.query_one(
                "SELECT actual_nano,disposition,reference,model,input_tokens,output_tokens,cache_read_tokens, \
                 cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests,real_nano,speed, \
                 inference_geo,input_nano,output_nano,cache_read_nano,cache_write_5m_nano, \
                 cache_write_1h_nano,web_search_nano,priced_ts,provider,
                 release_schema_version,release_generation,release_digest,release_billing_mode,
                 release_funding_generation,release_snapshot_digest \
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
                && row.get::<_, i64>(19) == u.priced_ts
                && row.get::<_, String>(20) == u.provider
                && row.get::<_, Option<i64>>(21) == release_schema_version
                && row.get::<_, Option<i64>>(22) == release_generation
                && row.get::<_, Option<String>>(23) == release_digest
                && row.get::<_, Option<String>>(24) == release_billing_mode
                && row.get::<_, Option<i64>>(25) == release_funding_generation
                && row.get::<_, Option<String>>(26) == release_snapshot_digest;
            if !exact {
                bail!("settlement request ID conflicts with different outbox payload");
            }
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

    /// Select the key for the active engine account whose unique handle is `crm-parsing`.
    /// The smoke may only spend a free account: the default multiplier and every provider
    /// override on it must be zero, so the request is metered but never charged. Ambiguous
    /// active keys are a hard error: the smoke never creates a credential or borrows an
    /// unrelated workload.
    pub fn openai_image_smoke_credential(&mut self) -> Result<OpenAiImageSmokeCredential> {
        let mut candidates = Vec::new();
        for row in self.client.query(
            "SELECT account.id,key.key,key.key_id,account.balance_nano,account.spent_nano,
                    account.reserved_nano,key.spent_nano,key.reserved_nano
               FROM accounts account
               JOIN api_keys key ON key.account_id=account.id
              WHERE account.handle='crm-parsing' AND account.status='active'
                AND account.mult_bp=0 AND key.status='active'
                AND (key.expires_ts IS NULL OR key.expires_ts>$1)
                AND NOT EXISTS (
                    SELECT 1 FROM account_provider_discounts discount
                     WHERE discount.account_id=account.id AND discount.mult_bp<>0
                )
              ORDER BY key.key_id",
            &[&now()],
        )? {
            candidates.push(OpenAiImageSmokeCredential {
                account_id: row.get(0),
                key: row.get(1),
                key_id: row.get(2),
                balance_nano: row.get(3),
                spent_nano: row.get(4),
                reserved_nano: row.get(5),
                key_spent_nano: row.get(6),
                key_reserved_nano: row.get(7),
            });
        }
        match candidates.len() {
            1 => Ok(candidates.pop().expect("one candidate")),
            0 => bail!("no active free service key can run the GPT Image 2 smoke"),
            count => {
                bail!("GPT Image 2 smoke service credential is ambiguous ({count} candidates)")
            }
        }
    }

    /// Read and revalidate one terminal image settlement in a coherent snapshot.
    pub fn openai_image_settlement_evidence(
        &mut self,
        request_id: &str,
    ) -> Result<Option<OpenAiImageSettlementEvidence>> {
        let mut tx = self
            .client
            .build_transaction()
            .isolation_level(IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()?;
        let Some(row) = tx.query_opt(
            "SELECT reservation.account_id,key.key_id,reservation.state,reservation.hold_nano,
                    reservation.actual_nano,outbox.state,outbox.disposition,
                    usage.model,usage.provider,usage.input_tokens,usage.output_tokens,
                    usage.cache_read_tokens,usage.real_nano,usage.charge_nano,usage.input_nano,
                    usage.output_nano,usage.cache_read_nano,usage.priced_ts,
                    account.balance_nano,account.spent_nano,account.reserved_nano,
                    key.spent_nano,key.reserved_nano
               FROM reservations reservation
               JOIN settlement_outbox outbox USING(request_id)
               JOIN usage_events usage USING(request_id)
               JOIN accounts account ON account.id=reservation.account_id
               JOIN api_keys key ON key.key=reservation.key AND key.account_id=reservation.account_id
              WHERE reservation.request_id=$1",
            &[&request_id],
        )? else {
            tx.commit()?;
            return Ok(None);
        };
        let evidence = OpenAiImageSettlementEvidence {
            request_id: request_id.to_owned(),
            account_id: row.get(0),
            key_id: row.get(1),
            reservation_state: row.get(2),
            reservation_hold_nano: row.get(3),
            reservation_actual_nano: row.get(4),
            outbox_state: row.get(5),
            outbox_disposition: row.get(6),
            model: row.get(7),
            provider: row.get(8),
            input_tokens: row.get(9),
            output_tokens: row.get(10),
            cache_read_tokens: row.get(11),
            real_nano: row.get(12),
            charge_nano: row.get(13),
            input_nano: row.get(14),
            output_nano: row.get(15),
            cache_read_nano: row.get(16),
            priced_ts: row.get(17),
            balance_nano: row.get(18),
            spent_nano: row.get(19),
            reserved_nano: row.get(20),
            key_spent_nano: row.get(21),
            key_reserved_nano: row.get(22),
        };
        tx.commit()?;
        Ok(Some(evidence))
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
        let Some(account_row) = tx.query_opt(
            "SELECT reservation.account_id
               FROM settlement_outbox outbox
               JOIN reservations reservation USING(request_id)
              WHERE outbox.request_id=$1",
            &[&request_id],
        )?
        else {
            tx.rollback()?;
            return Ok(None);
        };
        let locked_account_id: String = account_row.get(0);
        let Some(row) = tx.query_opt(
            "SELECT o.actual_nano,o.disposition,o.reference,o.model,o.input_tokens,o.output_tokens, \
             o.cache_read_tokens,o.cache_write_5m_tokens,o.cache_write_1h_tokens,o.web_search_requests, \
             o.real_nano,o.speed,o.inference_geo,o.input_nano,o.output_nano,o.cache_read_nano, \
             o.cache_write_5m_nano,o.cache_write_1h_nano,o.web_search_nano,o.priced_ts,o.provider, \
             o.state,r.account_id,r.key,r.hold_nano,r.state,COALESCE(r.group_id,r.request_id), \
             r.attempt,r.actual_nano \
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
        if account_id != locked_account_id {
            bail!("settlement reservation account changed while acquiring funding lock");
        }
        let actual: i64 = row.get(0);
        let effective_group_id: String = row.get(26);
        let execution_attempt: i32 = row.get(27);
        if outbox_state == "done" || matches!(reservation_state.as_str(), "settled" | "canceled") {
            let winner = if actual > 0 {
                tx.query_opt(
                    "SELECT winner_request_id FROM execution_group_winner WHERE group_id=$1",
                    &[&effective_group_id],
                )?
                .map(|winner| winner.get::<_, String>(0))
            } else {
                None
            };
            let expected_actual = if winner.as_deref().is_some_and(|winner| winner != request_id) {
                0
            } else {
                actual
            };
            if row.get::<_, Option<i64>>(28) != Some(expected_actual) {
                bail!("stored settlement differs from durable execution-group winner");
            }
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
        let disposition: String = row.get(1);
        let reference: Option<String> = row.get(2);
        let model: String = row.get(3);
        let account_key: String = row.get(23);
        let hold: i64 = row.get(24);
        let mut losing_attempt: Option<String> = None;
        let effective_actual = if actual > 0 {
            tx.execute(
                "INSERT INTO execution_group_winner(group_id,winner_request_id,decided_at)
                 VALUES($1,$2,$3) ON CONFLICT(group_id) DO NOTHING",
                &[&effective_group_id, &request_id, &ts],
            )?;
            let winner: String = tx
                .query_one(
                    "SELECT winner_request_id FROM execution_group_winner WHERE group_id=$1",
                    &[&effective_group_id],
                )?
                .get(0);
            if winner == request_id {
                actual
            } else {
                losing_attempt = Some(winner);
                0
            }
        } else {
            0
        };
        let effective_disposition = if losing_attempt.is_some() {
            "cancel"
        } else {
            disposition.as_str()
        };
        // Release-format snapshots never reached production (zero rows were ever written) and
        // One settlement for every reservation. A reservation opened by an older binary in a
        // retired snapshot format settles here too: the money is the hold and the actual on the
        // reservation row, which this path has always owned. Only the extra attribution those
        // formats carried is dropped — never the customer's money.
        let balance: i64;
        {
            balance = tx.query_one(
            "UPDATE accounts SET balance_nano=balance_nano+$1-$2, spent_nano=spent_nano+$2, \
             reserved_nano=reserved_nano-$1 WHERE id=$3 AND reserved_nano >= $1 RETURNING balance_nano",
            &[&hold, &effective_actual, &account_id],
        ).context("reservation/account aggregate invariant failed")?.get(0);
            let key_updated = tx.execute(
            "UPDATE api_keys SET spent_nano=spent_nano+$1, \
             reserved_nano=CASE WHEN reserved_nano >= $2 THEN reserved_nano-$2 ELSE reserved_nano END \
             WHERE key=$3 AND (reserved_nano >= $2 OR spend_limit_nano IS NULL)",
            &[&effective_actual, &hold, &account_key],
        )?;
            if key_updated != 1 {
                let key_still_exists = tx
                    .query_opt("SELECT 1 FROM api_keys WHERE key=$1", &[&account_key])?
                    .is_some();
                if key_still_exists {
                    bail!("reservation/key aggregate invariant failed");
                }
            }
            if effective_actual > 0 {
                tx.execute(
                "INSERT INTO ledger(account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts,model) \
                 VALUES($1,$2,'charge',$3,$4,$5,$6,$7,NULLIF($8,'')) ON CONFLICT DO NOTHING",
                &[&account_id, &account_key, &request_id, &effective_actual, &reference, &balance, &ts, &model],
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
                      &web_search_requests,&real_nano,&effective_actual,&reference,&ts,&speed,&inference_geo,
                      &input_nano,&output_nano,&cache_read_nano,&cache_write_5m_nano,
                      &cache_write_1h_nano,&web_search_nano,&priced_ts,&provider],
                )?;
                }
            }
        }
        let final_state = if effective_disposition == "cancel" {
            "canceled"
        } else {
            "settled"
        };
        tx.execute(
            "UPDATE reservations SET state=$2,actual_nano=$3,settled_ts=$4,updated_ts=$4 WHERE request_id=$1",
            &[&request_id, &final_state, &effective_actual, &ts],
        )?;
        tx.execute(
            "UPDATE settlement_outbox SET state='done',attempts=attempts+1,committed_ts=$2,updated_ts=$2, \
             last_error=NULL WHERE request_id=$1",
            &[&request_id, &ts],
        )?;
        tx.commit()?;
        if let Some(winner_request_id) = losing_attempt {
            crate::record_execution_group_loser(
                &effective_group_id,
                &winner_request_id,
                request_id,
                execution_attempt,
            );
        }
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
                        elog::error("registry", format!("billing outbox request {id} moved to failed: {message}"));
                    }
                }
            }
        }
        Ok(done)
    }

    /// Recover only reservations whose exact owner epoch is provably dead/fenced.
    ///
    /// `charge_hold_on_unknown_usage` is the fleet-wide fallback switch, off by default: a turn that
    /// died before reporting any usage settles at nothing rather than at the admission ceiling.
    pub fn reconcile_expired(
        &mut self,
        limit: usize,
        charge_hold_on_unknown_usage: bool,
    ) -> Result<ReconcileReport> {
        let ts = now();
        let rows = self.client.query(
            "SELECT r.request_id,r.state,r.hold_nano,r.measured_nano FROM reservations r \
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
            let measured: Option<i64> = row.get(3);
            match state.as_str() {
                "reserved" => {
                    self.enqueue_outbox(&request_id, 0, "cancel", None, None)?;
                    report.canceled_before_delivery += 1;
                }
                "delivering" => {
                    // The owner died mid-answer. If it managed to checkpoint what the turn had
                    // measured, that is what the customer owes — never more. With nothing measured
                    // the turn is not billed at all: the hold is an admission device, not a price,
                    // and no customer can reach this branch on purpose — it needs our own process
                    // to die. The operator switch restores the old conservative fallback.
                    // The disposition names the recovery path, not the amount: it drives model
                    // attribution and the billing invariant downstream, so every post-delivery
                    // recovery keeps the one label and only the sum varies.
                    let charge = match measured {
                        Some(measured) => measured.clamp(0, hold),
                        None if charge_hold_on_unknown_usage => hold,
                        None => 0,
                    };
                    let reason = "reconcile_full_hold";
                    self.enqueue_outbox(
                        &request_id,
                        charge,
                        reason,
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
    /// Pull a roster-backed pool member out of rotation, or put it back. Presence of the row is
    /// the disabled state, so both directions are idempotent and no partial write can leave a
    /// third state behind.
    pub fn pool_member_set_disabled(
        &mut self,
        provider: &str,
        member_id: &str,
        disabled: bool,
        hidden: bool,
        actor: &str,
        reason: &str,
    ) -> Result<()> {
        crate::require_roster_backed_provider(provider)?;
        if member_id.is_empty() {
            anyhow::bail!("pool member id must not be empty");
        }
        // Hiding a member that still receives traffic would take live capacity out of the
        // operator's view while it keeps serving. The row only exists for a disabled member, so
        // the storage shape already prevents it; reject explicitly rather than silently coercing.
        if hidden && !disabled {
            anyhow::bail!("a pool member can only be hidden while it is disabled");
        }
        if disabled {
            self.client.execute(
                "INSERT INTO pool_member_disables
                     (provider, member_id, reason, actor, updated_ts, hidden)
                 VALUES ($1,$2,$3,$4,$5,$6)
                 ON CONFLICT (provider, member_id) DO UPDATE
                     SET reason=EXCLUDED.reason,
                         actor=EXCLUDED.actor,
                         updated_ts=EXCLUDED.updated_ts,
                         hidden=EXCLUDED.hidden",
                &[&provider, &member_id, &reason, &actor, &now(), &hidden],
            )?;
        } else {
            // Re-enabling drops the hidden flag with the row: a member back in rotation is always
            // visible again.
            self.client.execute(
                "DELETE FROM pool_member_disables WHERE provider=$1 AND member_id=$2",
                &[&provider, &member_id],
            )?;
        }
        Ok(())
    }

    /// Every disabled member of one fleet, mapped to whether the operator also hid it. One read
    /// serves both axes so routability and presentation can never disagree about a member.
    pub fn pool_member_disables(
        &mut self,
        provider: &str,
    ) -> Result<std::collections::HashMap<String, bool>> {
        crate::require_roster_backed_provider(provider)?;
        Ok(self
            .client
            .query(
                "SELECT member_id, hidden FROM pool_member_disables WHERE provider=$1",
                &[&provider],
            )?
            .into_iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, bool>(1)))
            .collect())
    }

    /// Just the routability axis, for callers that never present anything.
    pub fn pool_member_disabled(&mut self, provider: &str) -> Result<HashSet<String>> {
        Ok(self.pool_member_disables(provider)?.into_keys().collect())
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
    pub fn load_claude_lifecycle(&mut self) -> Result<Vec<ClaudeLifecycleProfile>> {
        Ok(self.client.query(
            "SELECT email,status,fleet,COALESCE(NULLIF(token,''),NULLIF(token_file,'')),proxy, \
             plan,added_ts,COALESCE(auth_state,'healthy') FROM subs ORDER BY added_ts,email",
            &[],
        )?.into_iter().map(|row| ClaudeLifecycleProfile {
            email: row.get(0), status: row.get(1), fleet: row.get(2),
            has_token: row.get::<_, Option<String>>(3).is_some_and(|value| !value.is_empty()),
            proxy: row.get(4), plan: row.get(5), added_ts: row.get(6), auth_state: row.get(7),
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
        let mut transaction = self.client.transaction()?;
        transaction.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&crate::pricing::ACCOUNT_WRITE_LOCK],
        )?;
        transaction.execute(
            "INSERT INTO accounts(id,handle,mult_bp,status,created_ts,created) VALUES($1,$2,$3,'active',$4,$5)",
            &[&id,&handle,&mult_bp,&ts,&chrono_like(ts)],
        )?;
        transaction.commit()?;
        Ok(())
    }
    pub fn account_get(&mut self, id: &str) -> Result<Option<AccountRow>> {
        Ok(self.client.query_opt(
            "SELECT id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status FROM accounts WHERE id=$1",
            &[&id],
        )?.map(|r| account_row(&r)))
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
        let mut transaction = self.client.transaction()?;
        transaction.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&crate::pricing::ACCOUNT_WRITE_LOCK],
        )?;
        let affected = transaction
            .execute("UPDATE accounts SET status=$1 WHERE id=$2", &[&status, &id])?
            as usize;
        transaction.commit()?;
        Ok(affected)
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
        let mut transaction = self.client.transaction()?;
        transaction.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&crate::pricing::ACCOUNT_WRITE_LOCK],
        )?;
        let affected = transaction.execute(
            "UPDATE accounts SET status='deleted' WHERE id=$1 AND status<>'deleted'",
            &[&id],
        )? as usize;
        transaction.commit()?;
        Ok(affected)
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
        let Some(row) = tx.query_opt(
            "UPDATE accounts SET balance_nano=balance_nano+$1 WHERE id=$2 RETURNING balance_nano",
            &[&amount_nano, &id],
        )?
        else {
            tx.rollback()?;
            return Ok(None);
        };
        let balance: i64 = row.get(0);
        tx.execute(
            "INSERT INTO ledger(account_id,kind,amount_nano,ref,balance_after_nano,ts)
             VALUES($1,$2,$3,$4,$5,$6)",
            &[&id, &kind, &amount_nano, &reference, &balance, &ts],
        )?;
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
        if key.trim().is_empty() || account_id.trim().is_empty() {
            bail!("key and account id must not be empty");
        }
        let ts = now();
        let changed = self.client.execute(
            "INSERT INTO api_keys(
                 key,key_id,account_id,label,spend_limit_nano,expires_ts,status,created_ts,created) \
             VALUES($1,'key_' || md5(random()::text || clock_timestamp()::text),$2,$3,$4,$5,
                    'active',$6,$7) \
             ON CONFLICT(key) DO UPDATE SET label=EXCLUDED.label, \
             spend_limit_nano=EXCLUDED.spend_limit_nano,expires_ts=EXCLUDED.expires_ts \
             WHERE api_keys.account_id=EXCLUDED.account_id",
            &[
                &key,
                &account_id,
                &label,
                &spend_limit_nano,
                &expires_ts,
                &ts,
                &chrono_like(ts),
            ],
        )?;
        if changed == 0 {
            bail!("key is already owned by another account");
        }
        Ok(())
    }
    /// Read every per-provider discount of one account (control-plane listing).
    pub fn account_provider_discounts(&mut self, account_id: &str) -> Result<Vec<(String, i64)>> {
        Ok(self
            .client
            .query(
                "SELECT provider_id,mult_bp FROM account_provider_discounts
                  WHERE account_id=$1 ORDER BY provider_id",
                &[&account_id],
            )?
            .into_iter()
            .map(|row| (row.get(0), row.get(1)))
            .collect())
    }

    /// Set (or replace) one provider discount. Effective on the next authorization: there is no
    /// version to activate and no snapshot that can disagree with it.
    pub fn set_account_provider_discount(
        &mut self,
        account_id: &str,
        provider_id: &str,
        mult_bp: i64,
        ts: i64,
    ) -> Result<()> {
        crate::ensure_valid_provider_discount(provider_id, mult_bp)?;
        let changed = self.client.execute(
            "INSERT INTO account_provider_discounts(account_id,provider_id,mult_bp,updated_ts)
             SELECT $1,$2,$3,$4 WHERE EXISTS(SELECT 1 FROM accounts WHERE id=$1)
             ON CONFLICT(account_id,provider_id) DO UPDATE SET
                 mult_bp=EXCLUDED.mult_bp,updated_ts=EXCLUDED.updated_ts",
            &[&account_id, &provider_id, &mult_bp, &ts],
        )?;
        if changed == 0 {
            anyhow::bail!("unknown account");
        }
        Ok(())
    }

    /// Remove one provider discount; the account falls back to its default multiplier.
    pub fn clear_account_provider_discount(
        &mut self,
        account_id: &str,
        provider_id: &str,
    ) -> Result<bool> {
        Ok(self.client.execute(
            "DELETE FROM account_provider_discounts WHERE account_id=$1 AND provider_id=$2",
            &[&account_id, &provider_id],
        )? > 0)
    }

    pub fn key_account(&mut self, key: &str) -> Result<Option<KeyAuth>> {
        // Account state and pricing must be one statement: besides removing a network round trip
        // from every authorization, this prevents a concurrent override edit from being paired
        // with account/key fields read from a different PostgreSQL snapshot.
        let rows = self.client.query(
            "SELECT a.id,a.mult_bp,a.balance_nano,k.spent_nano,k.reserved_nano,
                    k.spend_limit_nano,k.expires_ts,(k.status='active' AND a.status='active'),
                    d.provider_id,d.mult_bp
               FROM api_keys k
               JOIN accounts a ON a.id=k.account_id
               LEFT JOIN account_provider_discounts d ON d.account_id=a.id
              WHERE k.key=$1 ORDER BY d.provider_id NULLS LAST",
            &[&key],
        )?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        let mut provider_mult_bp = Vec::with_capacity(crate::DISCOUNT_PROVIDER_IDS.len());
        for discount in &rows {
            match (
                discount.get::<_, Option<String>>(8),
                discount.get::<_, Option<i64>>(9),
            ) {
                (Some(provider), Some(multiplier)) => provider_mult_bp.push((provider, multiplier)),
                (None, None) => {}
                _ => bail!("provider discount row is structurally invalid"),
            }
        }
        Ok(Some(KeyAuth {
            account_id: row.get(0),
            mult_bp: row.get(1),
            provider_mult_bp,
            balance_nano: row.get(2),
            spent_nano: row.get(3),
            reserved_nano: row.get(4),
            spend_limit_nano: row.get(5),
            expires_ts: row.get(6),
            active: row.get(7),
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
        self.key_set_status_identity("key", key, status)
    }
    pub fn key_set_status_by_id(&mut self, key_id: &str, status: &str) -> Result<usize> {
        self.key_set_status_identity("key_id", key_id, status)
    }
    fn key_set_status_identity(
        &mut self,
        identity_column: &str,
        identity: &str,
        status: &str,
    ) -> Result<usize> {
        if !matches!(identity_column, "key" | "key_id") {
            bail!("invalid key status identity column");
        }
        Ok(self.client.execute(
            &format!("UPDATE api_keys SET status=$1 WHERE {identity_column}=$2"),
            &[&status, &identity],
        )? as usize)
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
        let entries = rows
            .into_iter()
            .map(|row| ledger_row(&row))
            .collect::<Result<Vec<_>>>()?;
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
             ), deleted AS ( \
               DELETE FROM reservations r USING doomed d \
                WHERE r.request_id=d.request_id \
                RETURNING r.request_id \
             ) \
             SELECT COUNT(*)::bigint FROM deleted",
            &[&older_than_ts],
        )?;
        let reservations = lifecycle_counts.get::<_, i64>(0) as usize;
        tx.execute(
            "DELETE FROM execution_group_winner winner
              WHERE NOT EXISTS (
                SELECT 1 FROM reservations reservation
                 WHERE COALESCE(reservation.group_id,reservation.request_id)=winner.group_id
              )",
            &[],
        )?;
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

    pub fn recent_provider_turn_calibration_events(
        &mut self,
        provider: &str,
        limit: usize,
    ) -> Result<Vec<ProviderTurnCalibrationEvent>> {
        if !matches!(provider, crate::PROVIDER_ANTHROPIC | crate::PROVIDER_GOOGLE) {
            bail!("invalid provider calibration event provider");
        }
        if !(1..=crate::MAX_RECENT_PROVIDER_TURN_CALIBRATION_EVENTS).contains(&limit) {
            bail!("invalid provider calibration event limit");
        }
        let limit = i64::try_from(limit).context("provider calibration event limit overflow")?;
        Ok(self
            .client
            .query(
                &format!(
                    "SELECT {} FROM provider_turn_calibration_events \
                     WHERE provider=$1 ORDER BY completed_at DESC,request_id DESC LIMIT $2",
                    crate::PROVIDER_TURN_EVENT_COLUMNS
                ),
                &[&provider, &limit],
            )?
            .iter()
            .map(pg_provider_turn_event)
            .collect())
    }

    fn kimi_calibration_row(row: &Row) -> KimiCalibrationRow {
        KimiCalibrationRow {
            subject_id: row.get(0),
            plan: row.get(1),
            window_duration_secs: row.get(2),
            window_name: row.get(3),
            resets_at: row.get(4),
            anchor_used_fraction_units: row.get(5),
            anchor_resolution_fraction_units: row.get(6),
            anchor_spend_nano: row.get(7),
            used_fraction_units: row.get(8),
            measurement_resolution_fraction_units: row.get(9),
            observed_at: row.get(10),
            native_limit_units: row.get(11),
            native_used_units: row.get(12),
            observed_fraction_units: row.get(13),
            observed_spend_nano: row.get(14),
            samples: row.get(15),
            unattributed_fraction_units: row.get(16),
            current_capacity_nano: row.get(17),
            current_low_nano: row.get(18),
            current_high_nano: row.get(19),
            current_confidence_bp: row.get(20),
            last_measured_at: row.get(21),
            estimator_version: row.get(22),
            version: row.get(23),
            updated_ts: row.get(24),
        }
    }

    pub fn load_kimi_calibration(
        &mut self,
        subject_id: &str,
        plan: &str,
        window_duration_secs: i64,
    ) -> Result<Option<KimiCalibrationRow>> {
        let row = self.client.query_opt(
            &format!(
                "SELECT {} FROM kimi_window_calibrations \
                 WHERE subject_id=$1 AND plan=$2 AND window_duration_secs=$3",
                crate::KIMI_CALIBRATION_COLUMNS
            ),
            &[&subject_id, &plan, &window_duration_secs],
        )?;
        let row = row.as_ref().map(Self::kimi_calibration_row);
        if let Some(row) = &row {
            crate::validate_kimi_calibration_row(row)?;
        }
        Ok(row)
    }

    pub fn list_kimi_calibrations(&mut self) -> Result<Vec<KimiCalibrationRow>> {
        let rows = self.client.query(
            &format!(
                "SELECT {} FROM kimi_window_calibrations \
                 ORDER BY plan, window_duration_secs, subject_id",
                crate::KIMI_CALIBRATION_COLUMNS
            ),
            &[],
        )?;
        let rows: Vec<KimiCalibrationRow> = rows.iter().map(Self::kimi_calibration_row).collect();
        for row in &rows {
            crate::validate_kimi_calibration_row(row)?;
        }
        Ok(rows)
    }

    /// Most recent immutable KIMI turn events, newest first, bounded by `limit`.
    ///
    /// This is the attribution surface of the admin-only calibration runner: its paid turns are
    /// found here by exact internal request id. The read is bounded so it stays cheap no matter
    /// how long the fleet runs, and every row is revalidated on the way out — a corrupted row
    /// fails the read closed instead of feeding the runner invented numbers.
    pub fn list_kimi_recent_turns(&mut self, limit: i64) -> Result<Vec<KimiTurnCalibrationEvent>> {
        let rows = self.client.query(
            "SELECT request_id,subject_id,plan,requested_model,served_model,context_mode,\
             reasoning_effort,tariff_schedule_id,priced_ts,completed_at,input_tokens,\
             cache_read_tokens,cache_write_tokens,output_tokens,reasoning_output_tokens,\
             api_input_nanousd,api_cache_read_nanousd,api_cache_write_nanousd,\
             api_output_nanousd,api_total_nanousd \
             FROM kimi_turn_calibration_events ORDER BY completed_at DESC, request_id LIMIT $1",
            &[&limit],
        )?;
        rows.iter()
            .map(|row| {
                let event = KimiTurnCalibrationEvent {
                    request_id: row.get(0),
                    subject_id: row.get(1),
                    plan: row.get(2),
                    requested_model: row.get(3),
                    served_model: row.get(4),
                    context_mode: row.get(5),
                    reasoning_effort: row.get(6),
                    tariff_schedule_id: row.get(7),
                    priced_ts: row.get(8),
                    completed_at: row.get(9),
                    input_tokens: row.get(10),
                    cache_read_tokens: row.get(11),
                    cache_write_tokens: row.get(12),
                    output_tokens: row.get(13),
                    reasoning_output_tokens: row.get(14),
                    api_input_nanousd: row.get(15),
                    api_cache_read_nanousd: row.get(16),
                    api_cache_write_nanousd: row.get(17),
                    api_output_nanousd: row.get(18),
                    api_total_nanousd: row.get(19),
                };
                event.validate()?;
                Ok(event)
            })
            .collect()
    }

    /// Immutable observation history for one window, oldest first.
    ///
    /// This is what an estimator-version change rebuilds from: a stored derived value is never
    /// authority, so the raw rows must remain readable in order.
    pub fn load_kimi_window_observations(
        &mut self,
        subject_id: &str,
        plan: &str,
        window_duration_secs: i64,
    ) -> Result<Vec<KimiWindowObservation>> {
        let rows = self.client.query(
            "SELECT subject_id,plan,window_duration_secs,window_name,resets_at,observed_at,\
             native_used_units,native_limit_units,used_fraction_units,\
             measurement_resolution_fraction_units,cumulative_api_spend_nano \
             FROM kimi_window_observations \
             WHERE subject_id=$1 AND plan=$2 AND window_duration_secs=$3 \
             ORDER BY observed_at, id",
            &[&subject_id, &plan, &window_duration_secs],
        )?;
        Ok(rows
            .iter()
            .map(|row| KimiWindowObservation {
                subject_id: row.get(0),
                plan: row.get(1),
                window_duration_secs: row.get(2),
                window_name: row.get(3),
                resets_at: row.get(4),
                observed_at: row.get(5),
                native_used_units: row.get(6),
                native_limit_units: row.get(7),
                used_fraction_units: row.get(8),
                measurement_resolution_fraction_units: row.get(9),
                cumulative_api_spend_nano: row.get(10),
            })
            .collect())
    }

    /// Persist one priced turn and advance the subject's cumulative spend in the same
    /// transaction.
    ///
    /// The pairing is the whole point: a quota observation read afterwards must never see a
    /// window total that its own traffic has not yet been added to, or the estimator would
    /// attribute our spend to somebody else's movement.
    ///
    /// Returns `Ok(true)` for a fresh insert and `Ok(false)` when the exact same payload was
    /// already stored — the internal request id survives every pre-byte retry, so replay must be
    /// a no-op. A *different* payload under that id is an error, never an update: overwriting
    /// would silently rewrite priced history.
    pub fn record_kimi_turn(&mut self, event: &KimiTurnCalibrationEvent) -> Result<bool> {
        event.validate()?;
        let mut tx = self.client.transaction()?;
        let inserted = tx.execute(
            "INSERT INTO kimi_turn_calibration_events(request_id,subject_id,plan,requested_model,\
             served_model,context_mode,reasoning_effort,tariff_schedule_id,priced_ts,completed_at,\
             input_tokens,cache_read_tokens,cache_write_tokens,output_tokens,\
             reasoning_output_tokens,api_input_nanousd,api_cache_read_nanousd,\
             api_cache_write_nanousd,api_output_nanousd,api_total_nanousd) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20) \
             ON CONFLICT(request_id) DO NOTHING",
            &[
                &event.request_id,
                &event.subject_id,
                &event.plan,
                &event.requested_model,
                &event.served_model,
                &event.context_mode,
                &event.reasoning_effort,
                &event.tariff_schedule_id,
                &event.priced_ts,
                &event.completed_at,
                &event.input_tokens,
                &event.cache_read_tokens,
                &event.cache_write_tokens,
                &event.output_tokens,
                &event.reasoning_output_tokens,
                &event.api_input_nanousd,
                &event.api_cache_read_nanousd,
                &event.api_cache_write_nanousd,
                &event.api_output_nanousd,
                &event.api_total_nanousd,
            ],
        )? == 1;
        if !inserted {
            // `ON CONFLICT` serializes two simultaneous ambiguous replies on the immutable key.
            // Once the winner commits, the loser reads its row and can distinguish an exact
            // replay from a semantic conflict without ever advancing spend twice.
            let row = tx.query_one(
                "SELECT subject_id,plan,requested_model,served_model,context_mode,reasoning_effort,\
                 tariff_schedule_id,priced_ts,completed_at,input_tokens,cache_read_tokens,\
                 cache_write_tokens,output_tokens,reasoning_output_tokens,api_input_nanousd,\
                 api_cache_read_nanousd,api_cache_write_nanousd,api_output_nanousd,api_total_nanousd \
                 FROM kimi_turn_calibration_events WHERE request_id=$1",
                &[&event.request_id],
            )?;
            let stored = KimiTurnCalibrationEvent {
                request_id: event.request_id.clone(),
                subject_id: row.get(0),
                plan: row.get(1),
                requested_model: row.get(2),
                served_model: row.get(3),
                context_mode: row.get(4),
                reasoning_effort: row.get(5),
                tariff_schedule_id: row.get(6),
                priced_ts: row.get(7),
                completed_at: row.get(8),
                input_tokens: row.get(9),
                cache_read_tokens: row.get(10),
                cache_write_tokens: row.get(11),
                output_tokens: row.get(12),
                reasoning_output_tokens: row.get(13),
                api_input_nanousd: row.get(14),
                api_cache_read_nanousd: row.get(15),
                api_cache_write_nanousd: row.get(16),
                api_output_nanousd: row.get(17),
                api_total_nanousd: row.get(18),
            };
            if !event.is_exact_replay_of(&stored) {
                return Err(crate::KimiTurnReplayConflict.into());
            }
            tx.commit()?;
            return Ok(false);
        }

        tx.execute(
            "INSERT INTO kimi_calibration_subject_spend(subject_id,spent_nano,\
             tracking_started_ts,updated_ts) VALUES($1,$2,$3,$3) \
             ON CONFLICT(subject_id) DO UPDATE SET \
             spent_nano=kimi_calibration_subject_spend.spent_nano+EXCLUDED.spent_nano, \
             tracking_started_ts=LEAST(\
                 kimi_calibration_subject_spend.tracking_started_ts,EXCLUDED.tracking_started_ts), \
             updated_ts=GREATEST(\
                 kimi_calibration_subject_spend.updated_ts,EXCLUDED.updated_ts)",
            &[
                &event.subject_id,
                &event.api_total_nanousd,
                &event.completed_at,
            ],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Cumulative official API replacement cost for a subject, or zero when nothing is tracked.
    pub fn kimi_subject_spend(&mut self, subject_id: &str) -> Result<i64> {
        let row = self.client.query_opt(
            "SELECT spent_nano FROM kimi_calibration_subject_spend WHERE subject_id=$1",
            &[&subject_id],
        )?;
        Ok(row.map(|row| row.get::<_, i64>(0)).unwrap_or(0))
    }

    /// Store an immutable observation and the estimator state it produced, under a CAS on
    /// `version`.
    ///
    /// The observation row is inserted first and is idempotent by its own unique constraint, so a
    /// duplicate poll adds no sample. Returns the new version, or `None` when the CAS lost — a
    /// lost CAS means another writer advanced the same window and this caller must re-read rather
    /// than overwrite.
    pub fn save_kimi_calibration(
        &mut self,
        state: &KimiCalibrationRow,
        observation: &KimiWindowObservation,
    ) -> Result<Option<i64>> {
        crate::validate_kimi_calibration_pair(state, observation)?;
        let mut tx = self.client.transaction()?;
        tx.execute(
            "INSERT INTO kimi_window_observations(subject_id,plan,window_duration_secs,\
             window_name,resets_at,observed_at,native_used_units,native_limit_units,\
             used_fraction_units,measurement_resolution_fraction_units,\
             cumulative_api_spend_nano,observation_source,estimator_version) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'poll',$12) \
             ON CONFLICT DO NOTHING",
            &[
                &observation.subject_id,
                &observation.plan,
                &observation.window_duration_secs,
                &observation.window_name,
                &observation.resets_at,
                &observation.observed_at,
                &observation.native_used_units,
                &observation.native_limit_units,
                &observation.used_fraction_units,
                &observation.measurement_resolution_fraction_units,
                &observation.cumulative_api_spend_nano,
                &state.estimator_version,
            ],
        )?;
        let next_version = state
            .version
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("KIMI calibration version overflow"))?;
        let updated = tx.execute(
            "INSERT INTO kimi_window_calibrations(subject_id,plan,window_duration_secs,\
             window_name,resets_at,anchor_used_fraction_units,anchor_resolution_fraction_units,\
             anchor_spend_nano,used_fraction_units,measurement_resolution_fraction_units,\
             observed_at,native_limit_units,native_used_units,observed_fraction_units,\
             observed_spend_nano,samples,unattributed_fraction_units,current_capacity_nano,\
             current_low_nano,current_high_nano,current_confidence_bp,last_measured_at,\
             estimator_version,version,updated_ts) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,\
             $22,$23,$24,$25) \
             ON CONFLICT(subject_id,plan,window_duration_secs) DO UPDATE SET \
             window_name=EXCLUDED.window_name,resets_at=EXCLUDED.resets_at,\
             anchor_used_fraction_units=EXCLUDED.anchor_used_fraction_units,\
             anchor_resolution_fraction_units=EXCLUDED.anchor_resolution_fraction_units,\
             anchor_spend_nano=EXCLUDED.anchor_spend_nano,\
             used_fraction_units=EXCLUDED.used_fraction_units,\
             measurement_resolution_fraction_units=\
             EXCLUDED.measurement_resolution_fraction_units,\
             observed_at=EXCLUDED.observed_at,native_limit_units=EXCLUDED.native_limit_units,\
             native_used_units=EXCLUDED.native_used_units,\
             observed_fraction_units=EXCLUDED.observed_fraction_units,\
             observed_spend_nano=EXCLUDED.observed_spend_nano,samples=EXCLUDED.samples,\
             unattributed_fraction_units=EXCLUDED.unattributed_fraction_units,\
             current_capacity_nano=EXCLUDED.current_capacity_nano,\
             current_low_nano=EXCLUDED.current_low_nano,\
             current_high_nano=EXCLUDED.current_high_nano,\
             current_confidence_bp=EXCLUDED.current_confidence_bp,\
             last_measured_at=EXCLUDED.last_measured_at,\
             estimator_version=EXCLUDED.estimator_version,version=EXCLUDED.version,\
             updated_ts=EXCLUDED.updated_ts \
             WHERE kimi_window_calibrations.version=$26",
            &[
                &state.subject_id,
                &state.plan,
                &state.window_duration_secs,
                &state.window_name,
                &state.resets_at,
                &state.anchor_used_fraction_units,
                &state.anchor_resolution_fraction_units,
                &state.anchor_spend_nano,
                &state.used_fraction_units,
                &state.measurement_resolution_fraction_units,
                &state.observed_at,
                &state.native_limit_units,
                &state.native_used_units,
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
                &next_version,
                &state.updated_ts,
                &state.version,
            ],
        )?;
        if updated == 0 {
            // Another writer advanced this window first. Rolling back keeps the observation and
            // the state consistent; the caller re-reads and folds again.
            tx.rollback()?;
            return Ok(None);
        }
        tx.commit()?;
        Ok(Some(next_version))
    }

    fn glm_calibration_row(row: &Row) -> GlmCalibrationRow {
        GlmCalibrationRow {
            subject_id: row.get(0),
            plan: row.get(1),
            window_duration_secs: row.get(2),
            reset_at: row.get(3),
            anchor_used_fraction_units: row.get(4),
            anchor_resolution_fraction_units: row.get(5),
            anchor_spend_api_nanousd: row.get(6),
            anchor_spend_native_microcredits: row.get(7),
            used_fraction_units: row.get(8),
            measurement_resolution_fraction_units: row.get(9),
            observed_at: row.get(10),
            native_limit_microcredits: row.get(11),
            native_used_microcredits: row.get(12),
            observed_fraction_units: row.get(13),
            observed_spend_api_nanousd: row.get(14),
            observed_spend_native_microcredits: row.get(15),
            samples: row.get(16),
            unattributed_fraction_units: row.get(17),
            current_capacity_nanousd: row.get(18),
            current_low_nanousd: row.get(19),
            current_high_nanousd: row.get(20),
            current_confidence_bp: row.get(21),
            last_measured_at: row.get(22),
            estimator_version: row.get(23),
            version: row.get(24),
            updated_ts: row.get(25),
        }
    }

    pub fn load_glm_calibration(
        &mut self,
        subject_id: &str,
        plan: &str,
        window_duration_secs: i64,
    ) -> Result<Option<GlmCalibrationRow>> {
        let row = self.client.query_opt(
            &format!(
                "SELECT {} FROM glm_window_calibrations \
                 WHERE subject_id=$1 AND plan=$2 AND window_duration_secs=$3",
                crate::GLM_CALIBRATION_COLUMNS
            ),
            &[&subject_id, &plan, &window_duration_secs],
        )?;
        let row = row.as_ref().map(Self::glm_calibration_row);
        if let Some(row) = &row {
            crate::validate_glm_calibration_row(row)?;
        }
        Ok(row)
    }

    pub fn list_glm_calibrations(&mut self) -> Result<Vec<GlmCalibrationRow>> {
        let rows = self.client.query(
            &format!(
                "SELECT {} FROM glm_window_calibrations \
                 ORDER BY plan, window_duration_secs, subject_id",
                crate::GLM_CALIBRATION_COLUMNS
            ),
            &[],
        )?;
        let rows: Vec<GlmCalibrationRow> = rows.iter().map(Self::glm_calibration_row).collect();
        for row in &rows {
            crate::validate_glm_calibration_row(row)?;
        }
        Ok(rows)
    }

    /// Immutable observation history for one window, oldest first.
    ///
    /// This is what an estimator-version change rebuilds from: a stored derived value is never
    /// authority, so the raw rows must remain readable in order.
    pub fn load_glm_window_observations(
        &mut self,
        subject_id: &str,
        plan: &str,
        window_duration_secs: i64,
    ) -> Result<Vec<GlmWindowObservation>> {
        let rows = self.client.query(
            "SELECT subject_id,plan,window_duration_secs,reset_at,observed_at,\
             native_used_units,native_limit_units,native_remaining_units,percentage_raw,\
             used_fraction_units,measurement_resolution_fraction_units,cumulative_api_nanousd,\
             cumulative_native_microcredits,observation_source,source_request_id \
             FROM glm_window_observations \
             WHERE subject_id=$1 AND plan=$2 AND window_duration_secs=$3 \
             ORDER BY observed_at, id",
            &[&subject_id, &plan, &window_duration_secs],
        )?;
        Ok(rows
            .iter()
            .map(|row| GlmWindowObservation {
                subject_id: row.get(0),
                plan: row.get(1),
                window_duration_secs: row.get(2),
                reset_at: row.get(3),
                observed_at: row.get(4),
                native_used_units: row.get(5),
                native_limit_units: row.get(6),
                native_remaining_units: row.get(7),
                percentage_raw: row.get(8),
                used_fraction_units: row.get(9),
                measurement_resolution_fraction_units: row.get(10),
                cumulative_api_nanousd: row.get(11),
                cumulative_native_microcredits: row.get(12),
                observation_source: row.get(13),
                source_request_id: row.get(14),
            })
            .collect())
    }

    /// Persist one priced turn and advance the subject's cumulative dual ledgers in the same
    /// transaction.
    ///
    /// The pairing is the whole point: a quota observation read afterwards must never see a
    /// window total that its own traffic has not yet been added to, or the estimator would
    /// attribute our spend to somebody else's movement. API nanoUSD and native microcredits
    /// advance together here, but remain two independent exact sums — one is never derived
    /// from the other.
    ///
    /// Returns `Ok(true)` for a fresh insert and `Ok(false)` when the exact same payload was
    /// already stored — the internal request id survives every pre-byte retry, so replay must
    /// be a no-op. A *different* payload under that id is an error, never an update:
    /// overwriting would silently rewrite priced history.
    pub fn record_glm_turn(&mut self, event: &GlmTurnCalibrationEvent) -> Result<bool> {
        event.validate()?;
        let mut tx = self.client.transaction()?;
        let inserted = tx.execute(
            "INSERT INTO glm_turn_calibration_events(request_id,subject_id,plan,requested_model,\
             served_model,context_mode,reasoning_effort,api_tariff_schedule_id,\
             credit_schedule_id,priced_ts,completed_at,fresh_input_tokens,cached_input_tokens,\
             cache_write_tokens,output_tokens,reasoning_tokens,api_fresh_input_nanousd,\
             api_cached_input_nanousd,api_output_nanousd,api_total_nanousd,\
             native_fresh_input_microcredits,native_cached_input_microcredits,\
             native_output_microcredits,native_total_microcredits,off_peak) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,\
             $22,$23,$24,$25) \
             ON CONFLICT(request_id) DO NOTHING",
            &[
                &event.request_id,
                &event.subject_id,
                &event.plan,
                &event.requested_model,
                &event.served_model,
                &event.context_mode,
                &event.reasoning_effort,
                &event.api_tariff_schedule_id,
                &event.credit_schedule_id,
                &event.priced_ts,
                &event.completed_at,
                &event.fresh_input_tokens,
                &event.cached_input_tokens,
                &event.cache_write_tokens,
                &event.output_tokens,
                &event.reasoning_tokens,
                &event.api_fresh_input_nanousd,
                &event.api_cached_input_nanousd,
                &event.api_output_nanousd,
                &event.api_total_nanousd,
                &event.native_fresh_input_microcredits,
                &event.native_cached_input_microcredits,
                &event.native_output_microcredits,
                &event.native_total_microcredits,
                &event.off_peak,
            ],
        )? == 1;
        if !inserted {
            // `ON CONFLICT` serializes two simultaneous ambiguous replies on the immutable key.
            // Once the winner commits, the loser reads its row and can distinguish an exact
            // replay from a semantic conflict without ever advancing spend twice.
            let row = tx.query_one(
                "SELECT subject_id,plan,requested_model,served_model,context_mode,\
                 reasoning_effort,api_tariff_schedule_id,credit_schedule_id,priced_ts,\
                 completed_at,fresh_input_tokens,cached_input_tokens,cache_write_tokens,\
                 output_tokens,reasoning_tokens,api_fresh_input_nanousd,api_cached_input_nanousd,\
                 api_output_nanousd,api_total_nanousd,native_fresh_input_microcredits,\
                 native_cached_input_microcredits,native_output_microcredits,\
                 native_total_microcredits,off_peak \
                 FROM glm_turn_calibration_events WHERE request_id=$1",
                &[&event.request_id],
            )?;
            let stored = GlmTurnCalibrationEvent {
                request_id: event.request_id.clone(),
                subject_id: row.get(0),
                plan: row.get(1),
                requested_model: row.get(2),
                served_model: row.get(3),
                context_mode: row.get(4),
                reasoning_effort: row.get(5),
                api_tariff_schedule_id: row.get(6),
                credit_schedule_id: row.get(7),
                priced_ts: row.get(8),
                completed_at: row.get(9),
                fresh_input_tokens: row.get(10),
                cached_input_tokens: row.get(11),
                cache_write_tokens: row.get(12),
                output_tokens: row.get(13),
                reasoning_tokens: row.get(14),
                api_fresh_input_nanousd: row.get(15),
                api_cached_input_nanousd: row.get(16),
                api_output_nanousd: row.get(17),
                api_total_nanousd: row.get(18),
                native_fresh_input_microcredits: row.get(19),
                native_cached_input_microcredits: row.get(20),
                native_output_microcredits: row.get(21),
                native_total_microcredits: row.get(22),
                off_peak: row.get(23),
            };
            if !event.is_exact_replay_of(&stored) {
                return Err(crate::GlmTurnReplayConflict.into());
            }
            tx.commit()?;
            return Ok(false);
        }

        tx.execute(
            "INSERT INTO glm_calibration_subject_spend(subject_id,spent_api_nanousd,\
             spent_native_microcredits,tracking_started_ts,updated_ts) VALUES($1,$2,$3,$4,$4) \
             ON CONFLICT(subject_id) DO UPDATE SET \
             spent_api_nanousd=glm_calibration_subject_spend.spent_api_nanousd\
                 +EXCLUDED.spent_api_nanousd, \
             spent_native_microcredits=glm_calibration_subject_spend.spent_native_microcredits\
                 +EXCLUDED.spent_native_microcredits, \
             tracking_started_ts=LEAST(\
                 glm_calibration_subject_spend.tracking_started_ts,EXCLUDED.tracking_started_ts), \
             updated_ts=GREATEST(\
                 glm_calibration_subject_spend.updated_ts,EXCLUDED.updated_ts)",
            &[
                &event.subject_id,
                &event.api_total_nanousd,
                &event.native_total_microcredits,
                &event.completed_at,
            ],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Cumulative dual ledgers for a subject, or zeroes when nothing is tracked.
    pub fn glm_subject_spend(&mut self, subject_id: &str) -> Result<GlmSubjectSpend> {
        let row = self.client.query_opt(
            "SELECT spent_api_nanousd,spent_native_microcredits \
             FROM glm_calibration_subject_spend WHERE subject_id=$1",
            &[&subject_id],
        )?;
        Ok(row
            .map(|row| GlmSubjectSpend {
                spent_api_nanousd: row.get(0),
                spent_native_microcredits: row.get(1),
            })
            .unwrap_or_default())
    }

    /// Store an immutable observation and the estimator state it produced, under a CAS on
    /// `version`.
    ///
    /// The observation row is inserted first and is idempotent by its own unique constraint,
    /// so a duplicate poll adds no sample. Returns the new version, or `None` when the CAS
    /// lost — a lost CAS means another writer advanced the same window and this caller must
    /// re-read rather than overwrite.
    pub fn save_glm_calibration(
        &mut self,
        state: &GlmCalibrationRow,
        observation: &GlmWindowObservation,
    ) -> Result<Option<i64>> {
        crate::validate_glm_calibration_pair(state, observation)?;
        let mut tx = self.client.transaction()?;
        tx.execute(
            "INSERT INTO glm_window_observations(subject_id,plan,window_duration_secs,reset_at,\
             observed_at,native_used_units,native_limit_units,native_remaining_units,\
             percentage_raw,used_fraction_units,measurement_resolution_fraction_units,\
             cumulative_api_nanousd,cumulative_native_microcredits,observation_source,\
             source_request_id,estimator_version) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16) \
             ON CONFLICT DO NOTHING",
            &[
                &observation.subject_id,
                &observation.plan,
                &observation.window_duration_secs,
                &observation.reset_at,
                &observation.observed_at,
                &observation.native_used_units,
                &observation.native_limit_units,
                &observation.native_remaining_units,
                &observation.percentage_raw,
                &observation.used_fraction_units,
                &observation.measurement_resolution_fraction_units,
                &observation.cumulative_api_nanousd,
                &observation.cumulative_native_microcredits,
                &observation.observation_source,
                &observation.source_request_id,
                &state.estimator_version,
            ],
        )?;
        let next_version = state
            .version
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("GLM calibration version overflow"))?;
        let updated = tx.execute(
            "INSERT INTO glm_window_calibrations(subject_id,plan,window_duration_secs,reset_at,\
             anchor_used_fraction_units,anchor_resolution_fraction_units,anchor_spend_api_nanousd,\
             anchor_spend_native_microcredits,used_fraction_units,\
             measurement_resolution_fraction_units,observed_at,native_limit_microcredits,\
             native_used_microcredits,observed_fraction_units,observed_spend_api_nanousd,\
             observed_spend_native_microcredits,samples,unattributed_fraction_units,\
             current_capacity_nanousd,current_low_nanousd,current_high_nanousd,\
             current_confidence_bp,last_measured_at,estimator_version,version,updated_ts) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,\
             $22,$23,$24,$25,$26) \
             ON CONFLICT(subject_id,plan,window_duration_secs) DO UPDATE SET \
             reset_at=EXCLUDED.reset_at,\
             anchor_used_fraction_units=EXCLUDED.anchor_used_fraction_units,\
             anchor_resolution_fraction_units=EXCLUDED.anchor_resolution_fraction_units,\
             anchor_spend_api_nanousd=EXCLUDED.anchor_spend_api_nanousd,\
             anchor_spend_native_microcredits=EXCLUDED.anchor_spend_native_microcredits,\
             used_fraction_units=EXCLUDED.used_fraction_units,\
             measurement_resolution_fraction_units=\
             EXCLUDED.measurement_resolution_fraction_units,\
             observed_at=EXCLUDED.observed_at,\
             native_limit_microcredits=EXCLUDED.native_limit_microcredits,\
             native_used_microcredits=EXCLUDED.native_used_microcredits,\
             observed_fraction_units=EXCLUDED.observed_fraction_units,\
             observed_spend_api_nanousd=EXCLUDED.observed_spend_api_nanousd,\
             observed_spend_native_microcredits=EXCLUDED.observed_spend_native_microcredits,\
             samples=EXCLUDED.samples,\
             unattributed_fraction_units=EXCLUDED.unattributed_fraction_units,\
             current_capacity_nanousd=EXCLUDED.current_capacity_nanousd,\
             current_low_nanousd=EXCLUDED.current_low_nanousd,\
             current_high_nanousd=EXCLUDED.current_high_nanousd,\
             current_confidence_bp=EXCLUDED.current_confidence_bp,\
             last_measured_at=EXCLUDED.last_measured_at,\
             estimator_version=EXCLUDED.estimator_version,version=EXCLUDED.version,\
             updated_ts=EXCLUDED.updated_ts \
             WHERE glm_window_calibrations.version=$27",
            &[
                &state.subject_id,
                &state.plan,
                &state.window_duration_secs,
                &state.reset_at,
                &state.anchor_used_fraction_units,
                &state.anchor_resolution_fraction_units,
                &state.anchor_spend_api_nanousd,
                &state.anchor_spend_native_microcredits,
                &state.used_fraction_units,
                &state.measurement_resolution_fraction_units,
                &state.observed_at,
                &state.native_limit_microcredits,
                &state.native_used_microcredits,
                &state.observed_fraction_units,
                &state.observed_spend_api_nanousd,
                &state.observed_spend_native_microcredits,
                &state.samples,
                &state.unattributed_fraction_units,
                &state.current_capacity_nanousd,
                &state.current_low_nanousd,
                &state.current_high_nanousd,
                &state.current_confidence_bp,
                &state.last_measured_at,
                &state.estimator_version,
                &next_version,
                &state.updated_ts,
                &state.version,
            ],
        )?;
        if updated == 0 {
            // Another writer advanced this window first. Rolling back keeps the observation and
            // the state consistent; the caller re-reads and folds again.
            tx.rollback()?;
            return Ok(None);
        }
        tx.commit()?;
        Ok(Some(next_version))
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

    pub fn load_gemini_exact_calibration(
        &mut self,
        profile_id: &str,
        plan: &str,
        bucket_id: &str,
    ) -> Result<Option<GeminiExactCalibrationRow>> {
        let row = self.client.query_opt(
            &format!(
                "SELECT {} FROM gemini_exact_window_calibrations \
                 WHERE profile_id=$1 AND plan=$2 AND bucket_id=$3",
                crate::GEMINI_EXACT_CALIBRATION_COLUMNS
            ),
            &[&profile_id, &plan, &bucket_id],
        )?;
        let row = row.as_ref().map(pg_gemini_exact_calibration_row);
        if let Some(row) = &row {
            crate::validate_gemini_exact_calibration_row(row)?;
        }
        Ok(row)
    }

    pub fn list_gemini_exact_calibrations(&mut self) -> Result<Vec<GeminiExactCalibrationRow>> {
        let rows = self
            .client
            .query(
                &format!(
                    "SELECT {} FROM gemini_exact_window_calibrations \
                     ORDER BY profile_id,plan,bucket_id",
                    crate::GEMINI_EXACT_CALIBRATION_COLUMNS
                ),
                &[],
            )?
            .iter()
            .map(pg_gemini_exact_calibration_row)
            .collect::<Vec<_>>();
        for row in &rows {
            crate::validate_gemini_exact_calibration_row(row)?;
        }
        Ok(rows)
    }

    pub fn load_gemini_exact_window_observations(
        &mut self,
        profile_id: &str,
        plan: &str,
        bucket_id: &str,
    ) -> Result<Vec<GeminiExactWindowObservation>> {
        let rows = self.client.query(
            "SELECT profile_id,plan,bucket_id,window_kind,window_duration_mins,resets_at,\
               observed_at,used_fraction_units,measurement_resolution_fraction_units,\
               gateway_spend_nano,observation_source,source_request_id \
             FROM gemini_exact_window_observations \
             WHERE profile_id=$1 AND plan=$2 AND bucket_id=$3 ORDER BY observed_at,id",
            &[&profile_id, &plan, &bucket_id],
        )?;
        let rows = rows
            .into_iter()
            .map(|row| GeminiExactWindowObservation {
                profile_id: row.get(0),
                plan: row.get(1),
                bucket_id: row.get(2),
                window_kind: row.get(3),
                window_duration_mins: row.get(4),
                resets_at: row.get(5),
                observed_at: row.get(6),
                used_fraction_units: row.get(7),
                measurement_resolution_fraction_units: row.get(8),
                gateway_spend_nano: row.get(9),
                observation_source: row.get(10),
                source_request_id: row.get(11),
            })
            .collect::<Vec<_>>();
        for row in &rows {
            crate::validate_gemini_exact_window_observation(row)?;
        }
        Ok(rows)
    }

    pub fn save_gemini_exact_calibration(
        &mut self,
        state: &GeminiExactCalibrationRow,
        observation: &GeminiExactWindowObservation,
    ) -> Result<Option<i64>> {
        crate::validate_gemini_exact_calibration_pair(state, observation)?;
        let mut tx = self.client.transaction()?;
        let values: &[&(dyn postgres::types::ToSql + Sync)] = &[
            &state.profile_id,
            &state.plan,
            &state.bucket_id,
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
            tx.execute(GEMINI_EXACT_CALIBRATION_INSERT_SQL, values)?
        } else {
            tx.execute(
                "UPDATE gemini_exact_window_calibrations SET \
                   window_kind=$4,window_duration_mins=$5,resets_at=$6,\
                   anchor_used_fraction_units=$7,anchor_resolution_fraction_units=$8,\
                   anchor_spend_nano=$9,used_fraction_units=$10,\
                   measurement_resolution_fraction_units=$11,observed_at=$12,\
                   observed_fraction_units=$13,observed_spend_nano=$14,samples=$15,\
                   unattributed_fraction_units=$16,current_capacity_nano=$17,\
                   current_low_nano=$18,current_high_nano=$19,current_confidence_bp=$20,\
                   last_measured_at=$21,estimator_version=$22,version=version+1,updated_ts=$24 \
                 WHERE profile_id=$1 AND plan=$2 AND bucket_id=$3 AND version=$23",
                values,
            )?
        };
        if changed == 0 {
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO gemini_exact_window_observations(\
               profile_id,plan,bucket_id,window_kind,window_duration_mins,resets_at,observed_at,\
               used_fraction_units,measurement_resolution_fraction_units,gateway_spend_nano,\
               observation_source,source_request_id) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) ON CONFLICT DO NOTHING",
            &[
                &observation.profile_id,
                &observation.plan,
                &observation.bucket_id,
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
        // The SQLite mirror no longer installs the retired pricing tables, so there is no policy
        // state left to refuse: an importable snapshot carries accounts, keys, ledger and usage.
        let policy_state_rows: i64 = 0;
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
        // Retired pricing tables may still hold immutable history; nothing reads them, so their
        // presence is not a reason to refuse an import.
        let target_policy_state_rows: i64 = 0;
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





















    pub fn list_tariff_overrides(&mut self) -> Result<Vec<crate::pricing::TariffOverride>> {
        crate::pricing::postgres_list_tariff_overrides(&mut self.client)
    }

    pub fn insert_tariff_override(
        &mut self,
        insert: &crate::pricing::TariffOverrideInsert,
    ) -> Result<crate::pricing::TariffOverrideInsertOutcome> {
        crate::pricing::postgres_insert_tariff_override(&mut self.client, insert)
    }














    /// Inspect each durable stage independently without returning request, account, or key identity.
    pub fn openai_image_settlement_diagnostic(
        &mut self,
        request_id: &str,
    ) -> Result<OpenAiImageSettlementDiagnostic> {
        let mut tx = self
            .client
            .build_transaction()
            .isolation_level(IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()?;
        let row = tx.query_one(
            r#"SELECT reservation.state,reservation.hold_nano,reservation.actual_nano,
                    snapshot.release_generation,snapshot.billing_mode,snapshot.provider_id,
                    snapshot.canonical_model_id,snapshot.tariff_schedule_id,
                    snapshot.official_hold_nano,snapshot.charged_hold_nano,
                    snapshot.official_cost_json->>'requested_model_id',
                    snapshot.official_cost_json->'premium_modifiers' =
                      '{"kind":"openai_image_v1","operation":"generation","background":"opaque","quality":"low","size":"auto","reference_count":0}'::jsonb,
                    outbox.state,outbox.disposition,outbox.attempts,
                    outbox.last_error IS NOT NULL,
                    usage.provider,usage.model,usage.input_tokens,usage.output_tokens,
                    usage.cache_read_tokens,usage.real_nano,usage.charge_nano,usage.input_nano,
                    usage.output_nano,usage.cache_read_nano,usage.priced_ts,
                    account.id IS NOT NULL,key.key_id IS NOT NULL
               FROM (SELECT $1::text AS request_id) wanted
               LEFT JOIN reservations reservation USING(request_id)
               LEFT JOIN pricing_request_snapshots_v2 snapshot USING(request_id)
               LEFT JOIN settlement_outbox outbox USING(request_id)
               LEFT JOIN usage_events usage USING(request_id)
               LEFT JOIN accounts account ON account.id=reservation.account_id
               LEFT JOIN api_keys key ON key.key=reservation.key
                                     AND key.account_id=reservation.account_id"#,
            &[&request_id],
        )?;
        let reservation_state: Option<String> = row.get(0);
        let release_generation: Option<i64> = row.get(3);
        let release_billing_mode: Option<String> = row.get(4);
        let snapshot_provider: Option<String> = row.get(5);
        let snapshot_model: Option<String> = row.get(6);
        let snapshot_tariff: Option<String> = row.get(7);
        let snapshot_requested_model: Option<String> = row.get(10);
        let snapshot_generation_controls: Option<bool> = row.get(11);
        let outbox_state: Option<String> = row.get(12);
        let outbox_disposition: Option<String> = row.get(13);
        let usage_provider: Option<String> = row.get(16);
        let usage_model: Option<String> = row.get(17);
        let snapshot_present = release_generation.is_some();
        let reservation_present = reservation_state.is_some();
        let outbox_present = outbox_state.is_some();
        let usage_present = usage_provider.is_some();
        let account_present: bool = row.get(27);
        let key_present: bool = row.get(28);
        let status = if !reservation_present {
            "reservation_missing"
        } else if !snapshot_present {
            "snapshot_missing"
        } else if !outbox_present {
            "outbox_missing"
        } else if outbox_state.as_deref() == Some("failed") {
            "outbox_failed"
        } else if outbox_state.as_deref() != Some("done") {
            "outbox_pending"
        } else if !usage_present {
            "usage_missing"
        } else if reservation_state.as_deref() != Some("settled") {
            "reservation_nonterminal"
        } else if !account_present || !key_present {
            "principal_missing"
        } else {
            "terminal_evidence_present"
        };
        let diagnostic = OpenAiImageSettlementDiagnostic {
            schema_version: 1,
            status,
            reservation_present,
            reservation_state,
            reservation_hold_nano: row.get(1),
            reservation_actual_nano: row.get(2),
            snapshot_present,
            release_generation,
            release_billing_mode,
            snapshot_openai: snapshot_provider.as_deref() == Some(PROVIDER_OPENAI),
            snapshot_canonical_model: snapshot_model.as_deref() == Some("gpt-image-2-2026-04-21"),
            snapshot_tariff: snapshot_tariff.as_deref() == Some("openai/gpt-image-2/2026-04-21/v1"),
            snapshot_requested_model: snapshot_requested_model.as_deref() == Some("gpt-image-2"),
            snapshot_generation_controls: snapshot_generation_controls.unwrap_or(false),
            official_hold_nano: row.get(8),
            charged_hold_nano: row.get(9),
            outbox_present,
            outbox_state,
            outbox_disposition,
            outbox_attempts: row.get(14),
            outbox_has_error: row.get::<_, Option<bool>>(15).unwrap_or(false),
            usage_present,
            usage_openai: usage_provider.as_deref() == Some(PROVIDER_OPENAI),
            usage_model: usage_model.as_deref() == Some("gpt-image-2"),
            input_tokens: row.get(18),
            output_tokens: row.get(19),
            cache_read_tokens: row.get(20),
            real_nano: row.get(21),
            charge_nano: row.get(22),
            input_nano: row.get(23),
            output_nano: row.get(24),
            cache_read_nano: row.get(25),
            priced_ts: row.get(26),
            account_present,
            key_present,
        };
        tx.commit()?;
        Ok(diagnostic)
    }
}

#[cfg(test)]
mod tests;
