use super::*;
use crate::{PROVIDER_ANTHROPIC, PROVIDER_GOOGLE};
use std::sync::{Arc, Barrier};




















#[test]
fn engine_migration_plan_is_contiguous() {
    let versions: Vec<_> = ENGINE_MIGRATIONS
        .iter()
        .map(|(version, _)| *version)
        .collect();
    assert_eq!(versions, (1..=CURRENT_SCHEMA_VERSION).collect::<Vec<_>>());
}

#[test]
fn execution_group_migration_preserves_old_reservation_writers() {
    let normalized = MIGRATION_0021
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(normalized.contains("ADD COLUMN IF NOT EXISTS group_id text"));
    assert!(normalized.contains("ADD COLUMN IF NOT EXISTS attempt integer NOT NULL DEFAULT 1"));
    assert!(!normalized.contains("group_id text NOT NULL"));
    assert!(!normalized.contains("DEFAULT request_id"));
    assert!(normalized.contains("CREATE TABLE IF NOT EXISTS execution_group_winner"));
}

#[test]
fn gemini_exact_migration_is_additive_and_plan_scoped() {
    let normalized = MIGRATION_0022
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(normalized.contains("CREATE TABLE IF NOT EXISTS gemini_exact_window_calibrations"));
    assert!(normalized.contains("PRIMARY KEY (profile_id, plan, bucket_id)"));
    assert!(normalized.contains("measurement_resolution_fraction_units bigint NOT NULL"));
    assert!(normalized.contains("unattributed_fraction_units bigint NOT NULL DEFAULT 0"));
    assert!(normalized.contains("observation_source text NOT NULL"));
    assert!(!normalized.contains("ALTER TABLE gemini_window_calibrations"));
    assert!(!normalized.contains("DROP TABLE"));
}

#[test]
fn pricing_release_funding_v2_migration_is_additive_and_one_head() {
    let normalized = MIGRATION_0023
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    for table in [
        "pricing_release_policy_versions",
        "pricing_release_policy_rules",
        "pricing_release_versions",
        "pricing_release_recovery_links",
        "pricing_release_assignments",
        "account_funding_generations_v2",
        "account_funding_head_v2",
        "funding_lots_v2",
        "pricing_stage8_evidence_v2",
        "pricing_release_head_v2",
        "pricing_release_activations_v2",
        "pricing_request_snapshots_v2",
        "pricing_request_funding_allocations_v2",
        "funding_ledger_allocations_v2",
    ] {
        assert!(
            normalized.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")),
            "missing pricing/funding v2 table {table}",
        );
    }
    assert!(!normalized.contains(" DROP TABLE "));
    assert!(!normalized.contains(" TRUNCATE "));
    assert!(!normalized.contains(" pricing_mode "));
    assert!(!normalized.contains(" track "));
    assert!(!normalized.contains(" tier "));
    assert!(!normalized.contains(" retention "));
    assert!(normalized.contains("scope_type IN ('global', 'provider', 'model')"));
    assert!(normalized.contains("source_type IN ('paid', 'welcome_bonus')"));
    assert!(normalized.contains("billing_mode = 'meter_only'"));
    assert!(normalized.contains("charged_hold_nano = floor("));
    assert!(
        normalized.contains("released_total <> GREATEST(reservation_hold - reservation_actual, 0)")
    );
    assert!(normalized.contains("pricing_release_head_step_v2"));
    assert!(normalized.contains("pricing_release_head_audit_v2"));
    assert!(normalized.contains("initial pricing v2 release head version must be 1"));
    assert!(normalized.contains(
        "FOREACH table_name IN ARRAY ARRAY['settlement_outbox', 'usage_events', 'ledger']"
    ));
    assert!(normalized.contains("ADD COLUMN IF NOT EXISTS release_schema_version bigint"));
    assert!(!normalized.contains("ADD COLUMN IF NOT EXISTS release_schema_version bigint NOT NULL"));
}

#[test]
fn pre_cutover_funding_snapshot_migration_is_additive_and_release_independent() {
    let normalized = MIGRATION_0024
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    for table in [
        "funding_reservation_snapshots_v2",
        "funding_reservation_allocations_v2",
    ] {
        assert!(
            normalized.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")),
            "missing pre-cutover funding v2 table {table}",
        );
    }
    assert!(!normalized.contains(" DROP TABLE "));
    assert!(!normalized.contains(" TRUNCATE "));
    assert!(!normalized.contains(" pricing_mode "));
    assert!(!normalized.contains(" track "));
    assert!(!normalized.contains(" tier "));
    assert!(!normalized.contains(" retention "));
    assert!(normalized.contains("lot_source_type IN ('paid', 'welcome_bonus')"));
    assert!(normalized.contains("lot_source_type = 'paid'"));
    assert!(normalized.contains("account funding v2 head cannot be deleted"));
    assert!(normalized.contains("account funding v2 head must advance one version and generation"));
    assert!(normalized
        .contains("active normalized reservation lacks one compatible funding v2 snapshot"));
    assert!(!normalized.contains("pricing_release_head_v2"));
    assert!(!normalized.contains("pricing_release_activations_v2"));
}

#[test]
fn pricing_release_runtime_epoch_fence_is_additive_and_head_gated() {
    let normalized = MIGRATION_0025
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    assert!(normalized.contains("ADD COLUMN IF NOT EXISTS pricing_release_claim_epoch bigint"));
    assert!(!normalized.contains("pricing_release_claim_epoch bigint NOT NULL"));
    assert!(normalized.contains("engine_instances_release_v2_epoch_shape"));
    assert!(normalized.contains("engine_instances_release_v2_epoch_fence"));
    assert!(normalized.contains("SELECT 1 FROM pricing_release_head_v2 WHERE singleton = 1"));
    assert!(normalized.contains("NEW.pricing_release_claim_epoch IS DISTINCT FROM NEW.owner_epoch"));
    assert!(!normalized.contains(" DROP TABLE "));
    assert!(!normalized.contains(" TRUNCATE "));
}

#[test]
fn pricing_release_zero_drain_migration_preserves_observation_and_adds_dormant_extensions() {
    let normalized = MIGRATION_0026
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    assert!(normalized.contains("CREATE TABLE pricing_release_assignment_extensions_v2"));
    assert!(normalized.contains("pricing_release_assignment_extension_v2_pair"));
    assert!(normalized.contains("DEFERRABLE INITIALLY DEFERRED"));
    assert!(
        normalized.contains("pricing assignment extension requires the exact current release head")
    );
    assert!(normalized.contains("pricing assignment extension pair is incomplete"));
    assert!(normalized.contains("must cover a prepared recovery release"));
    assert!(normalized.contains("DROP CONSTRAINT pricing_stage8_evidence_v2_check1"));
    assert!(!normalized.contains("DROP CONSTRAINT pricing_stage8_evidence_v2_check;"));
    assert!(normalized.contains("pricing_stage8_evidence_v2_passed_check"));
    assert!(normalized.contains("passed AND blocker_count = 0"));
    assert!(!normalized.contains("passed AND blocker_count = 0 AND legacy_inflight_count = 0"));
    assert!(!normalized.contains(" DROP TABLE "));
    assert!(!normalized.contains(" TRUNCATE "));
}

#[test]
fn kimi_calibration_migration_is_additive_and_keeps_served_model_identity() {
    // Strip `--` comment lines first: the header prose deliberately names the 0019 authority
    // to explain why this migration stands beside it, and that mention must not be mistaken
    // for a statement touching it.
    let ddl = MIGRATION_0027
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let normalized = ddl.split_whitespace().collect::<Vec<_>>().join(" ");

    for table in [
        "kimi_turn_calibration_events",
        "kimi_calibration_subject_spend",
        "kimi_window_observations",
        "kimi_window_calibrations",
    ] {
        assert!(
            normalized.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")),
            "missing KIMI calibration table {table}",
        );
    }

    // Expand-only: nothing is dropped, truncated or altered.
    assert!(!normalized.contains(" DROP TABLE "));
    assert!(!normalized.contains(" TRUNCATE "));
    assert!(!normalized.contains(" DROP CONSTRAINT "));
    assert!(!normalized.contains(" ALTER TABLE "));

    // The shared 0019 authority must be left completely untouched: its provider CHECK is a
    // closed set, and its row carries a single model id, which cannot express a served model
    // that differs from the requested one.
    assert!(!normalized.contains("provider_turn_calibration_events"));
    assert!(!normalized.contains("provider_calibration_subject_spend"));

    // Billing follows the served model, so both identities are immutable columns.
    assert!(normalized.contains("requested_model text NOT NULL"));
    assert!(normalized.contains("served_model text NOT NULL"));

    // Paid plan and the exact native window duration are part of the durable identity.
    assert!(normalized.contains("PRIMARY KEY (subject_id, plan, window_duration_secs)"));

    // Raw provider integers are the authority; the derived fraction sits beside them.
    assert!(normalized.contains("native_used_units bigint NOT NULL"));
    assert!(
        normalized.contains("native_limit_units bigint NOT NULL CHECK (native_limit_units > 0)")
    );
    assert!(normalized.contains("measurement_resolution_fraction_units bigint NOT NULL"));

    // KIMI serves quota only from a separate endpoint, so no request id is ever invented.
    assert!(normalized.contains("observation_source IN ('poll')"));
    assert!(!normalized.contains("source_request_id"));

    // Disjoint legs must sum exactly to the recorded total.
    assert!(normalized.contains(
        "api_total_nanousd = api_input_nanousd + api_cache_read_nanousd \
         + api_cache_write_nanousd + api_output_nanousd"
    ));
    assert!(normalized.contains("CHECK (reasoning_output_tokens <= output_tokens)"));

    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (27)"));
}

#[test]
fn pricing_ledger_release_v2_attribution_migration_only_widens_ledger_shape() {
    let ddl = MIGRATION_0028
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let normalized = ddl.split_whitespace().collect::<Vec<_>>().join(" ");

    // The legacy ranges constraint is swapped for a superset: policy_v1 and legacy_scalar
    // keep the exact same expression, release_v2 is only added to the closed kind set.
    assert!(normalized.contains("DROP CONSTRAINT ledger_multi_discount_ranges"));
    assert!(normalized.contains("ADD CONSTRAINT ledger_multi_discount_ranges"));
    assert!(normalized.contains("snapshot_kind IN ('policy_v1', 'legacy_scalar', 'release_v2')"));
    assert!(normalized.contains("VALIDATE CONSTRAINT ledger_multi_discount_ranges"));

    // The dedicated release_v2 shape is a separate additive constraint, valid for every
    // pre-existing row because none of them carries snapshot_kind='release_v2'.
    assert!(normalized.contains("ADD CONSTRAINT ledger_release_v2_attribution_shape"));
    assert!(normalized.contains("snapshot_kind IS DISTINCT FROM 'release_v2'"));
    assert!(normalized.contains("attribution_schema_version >= 2"));
    assert!(normalized.contains("account_class IN ('b2c', 'b2b', 'openkeys', 'service')"));
    assert!(normalized.contains("commission_eligible IS NULL"));
    assert!(normalized.contains("VALIDATE CONSTRAINT ledger_release_v2_attribution_shape"));

    // Expand-only: no data is rewritten and nothing else is dropped.
    assert!(!normalized.contains(" DROP TABLE "));
    assert!(!normalized.contains(" TRUNCATE "));
    assert!(!normalized.contains(" UPDATE "));
    assert!(!normalized.contains(" DELETE "));

    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (28)"));
}

#[test]
fn glm_calibration_migration_is_additive_and_keeps_dual_ledger_identity() {
    // Strip `--` comment lines first: the header prose deliberately names the 0019 and
    // 0027 authorities to explain why this migration stands beside them, and those mentions
    // must not be mistaken for statements touching them.
    let ddl = MIGRATION_0029
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let normalized = ddl.split_whitespace().collect::<Vec<_>>().join(" ");

    for table in [
        "glm_turn_calibration_events",
        "glm_calibration_subject_spend",
        "glm_window_observations",
        "glm_window_calibrations",
    ] {
        assert!(
            normalized.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")),
            "missing GLM calibration table {table}",
        );
    }

    // Expand-only: nothing is dropped, truncated or altered.
    assert!(!normalized.contains(" DROP TABLE "));
    assert!(!normalized.contains(" TRUNCATE "));
    assert!(!normalized.contains(" DROP CONSTRAINT "));
    assert!(!normalized.contains(" ALTER TABLE "));

    // The shared 0019 authority and the KIMI 0027 authority must both be left completely
    // untouched: neither durable identity can carry a served model distinct from the
    // requested one plus a paid plan plus a per-turn native ledger.
    assert!(!normalized.contains("provider_turn_calibration_events"));
    assert!(!normalized.contains("provider_calibration_subject_spend"));
    assert!(!normalized.contains("kimi_turn_calibration_events"));
    assert!(!normalized.contains("kimi_calibration_subject_spend"));
    assert!(!normalized.contains("kimi_window_observations"));
    assert!(!normalized.contains("kimi_window_calibrations"));

    // Billing follows the served model, so both identities are immutable columns.
    assert!(normalized.contains("requested_model text NOT NULL"));
    assert!(normalized.contains("served_model text NOT NULL"));

    // Paid plan and the exact native window duration are part of the durable identity.
    assert!(normalized.contains("PRIMARY KEY (subject_id, plan, window_duration_secs)"));

    // Two effective-dated schedules price every turn: the API rate card and the native
    // credit multipliers.
    assert!(normalized.contains("api_tariff_schedule_id text NOT NULL"));
    assert!(normalized.contains("credit_schedule_id text NOT NULL"));

    // Only GLM-5.2 takes a reasoning effort, so the column stays nullable.
    assert!(normalized.contains("reasoning_effort text CHECK (reasoning_effort IS NULL"));
    assert!(!normalized.contains("reasoning_effort text NOT NULL"));

    // Dual ledger: API nanoUSD and native microcredits legs each sum to their own total.
    assert!(normalized.contains("spent_api_nanousd bigint NOT NULL"));
    assert!(normalized.contains("spent_native_microcredits bigint NOT NULL"));
    assert!(normalized.contains(
        "api_total_nanousd = api_fresh_input_nanousd + api_cached_input_nanousd \
         + api_output_nanousd"
    ));
    assert!(normalized.contains(
        "native_total_microcredits = native_fresh_input_microcredits \
         + native_cached_input_microcredits + native_output_microcredits"
    ));
    assert!(normalized.contains("CHECK (reasoning_tokens <= output_tokens)"));

    // Raw quota counters have unproven units: unknown stays NULL, never 0, and the derived
    // fraction pair exists only once the units are proven.
    assert!(normalized.contains(
        "native_used_units bigint CHECK (native_used_units IS NULL OR native_used_units >= 0)"
    ));
    assert!(
        normalized.contains("native_remaining_units bigint CHECK (native_remaining_units IS NULL")
    );
    assert!(normalized.contains(
        "used_fraction_units bigint CHECK (used_fraction_units IS NULL OR used_fraction_units BETWEEN 0 AND 100000000)"
    ));
    assert!(normalized.contains(
        "(used_fraction_units IS NULL) = (measurement_resolution_fraction_units IS NULL)"
    ));

    // Quota arrives by poll and on responses; a response names its request, a poll invents
    // none, and the dedup key treats NULL raw counters as equal.
    assert!(normalized.contains("observation_source IN ('poll', 'response')"));
    assert!(normalized.contains("source_request_id"));
    assert!(normalized.contains("UNIQUE NULLS NOT DISTINCT"));

    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (29)"));
}

#[test]
fn glm_calibration_migration_is_registered_at_the_current_schema_version() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 46);
    let registered = ENGINE_MIGRATIONS
        .iter()
        .find(|(version, _)| *version == 29)
        .map(|(_, sql)| *sql);
    assert_eq!(registered, Some(MIGRATION_0029));
}

#[test]
fn account_discount_contract_migration_closes_the_runtime_provider_set() {
    let normalized = MIGRATION_0046
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(normalized
        .contains("CHECK (provider_id IN ('anthropic', 'openai', 'google', 'kimi', 'glm'))"));
    assert!(!normalized.contains("'zhipu'"));
    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (46)"));
}

/// A migration records its own version — `apply_migration` runs the SQL but never writes the
/// bookkeeping row. Forgetting that line still creates the tables, so every local suite without a
/// database stays green while `schema_version()` silently sticks at the previous number and the
/// engine then refuses to start against the new schema. Assert the line exists for every
/// registered migration instead of finding out from a red deploy.
#[test]
fn every_migration_registers_its_own_schema_version() {
    for &(version, sql) in ENGINE_MIGRATIONS {
        // Both spellings are in use (`engine_schema_migrations` and the schema-qualified
        // `public.engine_schema_migrations`), and whitespace varies, so normalize before
        // matching: the invariant is the value inserted, not the formatting.
        let needle = format!("VALUES({version})");
        let registers = sql
            .split("engine_schema_migrations(version)")
            .skip(1)
            .any(|tail| {
                tail.chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
                    .starts_with(&needle)
            });
        assert!(
            registers,
            "migration {version:04} does not register its own version: \
             `schema_version()` would stay at {} after it applies",
            version - 1
        );
    }
}

/// The disable store is only reachable once its migration is actually in the applied set: the
/// pools read it on every roster load, so a registered-but-missing table would fail closed for a
/// whole fleet rather than for one member.
#[test]
fn pool_member_disable_migration_is_registered_at_the_current_schema_version() {
    let registered = ENGINE_MIGRATIONS
        .iter()
        .find(|(version, _)| *version == 32)
        .map(|(_, sql)| *sql);
    assert_eq!(registered, Some(MIGRATION_0032));
    assert_eq!(
        ENGINE_MIGRATIONS.last().map(|(version, _)| *version),
        Some(CURRENT_SCHEMA_VERSION)
    );
}

/// Anthropic must never become addressable here. Claude subscriptions already carry
/// `active|paused|disabled`, and a second switch for the same subscription is exactly the
/// two-sources-of-truth bug this closed set exists to prevent.
#[test]
fn pool_member_disable_ddl_excludes_anthropic_and_covers_every_roster_fleet() {
    let ddl = MIGRATION_0032
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    for provider in crate::ROSTER_BACKED_PROVIDERS {
        assert!(
            ddl.contains(&format!("'{provider}'")),
            "roster-backed provider {provider} is missing from the DDL CHECK"
        );
    }
    assert!(!ddl.contains("'anthropic'"));
}

/// Real PostgreSQL proof that the operator switch is durable and idempotent in both directions.
/// Skipped unless the destructive test database is supplied:
/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::pool_member_disable_postgres_roundtrip`
#[test]
fn pool_member_disable_postgres_roundtrip() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping pool member disable round-trip: test URL is unset");
        return;
    };
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
    lock_holder
        .client
        .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
        .unwrap();

    let mut pg = PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    pg.client
        .batch_execute("TRUNCATE pool_member_disables RESTART IDENTITY CASCADE")
        .unwrap();

    assert!(pg
        .pool_member_disabled(crate::PROVIDER_GOOGLE)
        .unwrap()
        .is_empty());

    pg.pool_member_set_disabled(
        crate::PROVIDER_GOOGLE,
        "gemini_oauth_000002",
        true,
        false,
        "operator",
        "refresh token revoked by Google",
    )
    .unwrap();
    // Disabling twice must not fail and must not create a second row.
    pg.pool_member_set_disabled(
        crate::PROVIDER_GOOGLE,
        "gemini_oauth_000002",
        true,
        false,
        "operator",
        "still revoked",
    )
    .unwrap();

    let disabled = pg.pool_member_disabled(crate::PROVIDER_GOOGLE).unwrap();
    assert_eq!(disabled.len(), 1);
    assert!(disabled.contains("gemini_oauth_000002"));

    // Fleets are isolated: a Gemini disable must never hide a Codex home.
    assert!(pg
        .pool_member_disabled(crate::PROVIDER_OPENAI)
        .unwrap()
        .is_empty());

    // Re-enabling is idempotent too.
    pg.pool_member_set_disabled(crate::PROVIDER_GOOGLE, "gemini_oauth_000002", false, false, "", "")
        .unwrap();
    pg.pool_member_set_disabled(crate::PROVIDER_GOOGLE, "gemini_oauth_000002", false, false, "", "")
        .unwrap();
    assert!(pg
        .pool_member_disabled(crate::PROVIDER_GOOGLE)
        .unwrap()
        .is_empty());

    // Claude can never be addressed through this store.
    assert!(pg
        .pool_member_set_disabled(crate::PROVIDER_ANTHROPIC, "someone@example.com", true, false, "", "")
        .is_err());
    assert!(pg.pool_member_disabled(crate::PROVIDER_ANTHROPIC).is_err());
    assert!(pg
        .pool_member_set_disabled(crate::PROVIDER_GOOGLE, "", true, false, "", "")
        .is_err());

    // Hiding is a presentation choice layered on top of a disable, never a way to take a serving
    // profile out of the operator's view while it keeps receiving traffic.
    assert!(pg
        .pool_member_set_disabled(crate::PROVIDER_GOOGLE, "gemini_oauth_000003", false, true, "", "")
        .is_err());
    pg.pool_member_set_disabled(
        crate::PROVIDER_GOOGLE,
        "gemini_oauth_000003",
        true,
        true,
        "operator",
        "dead credential",
    )
    .unwrap();
    let disables = pg.pool_member_disables(crate::PROVIDER_GOOGLE).unwrap();
    assert_eq!(disables.get("gemini_oauth_000003"), Some(&true));
    // Routability does not care about the presentation axis.
    assert!(pg
        .pool_member_disabled(crate::PROVIDER_GOOGLE)
        .unwrap()
        .contains("gemini_oauth_000003"));
    // Re-enabling drops the hidden flag with the row: a member back in rotation is visible again.
    pg.pool_member_set_disabled(crate::PROVIDER_GOOGLE, "gemini_oauth_000003", false, false, "", "")
        .unwrap();
    assert!(pg
        .pool_member_disables(crate::PROVIDER_GOOGLE)
        .unwrap()
        .is_empty());
}

/// Real PostgreSQL proof for immutable turn replay, cumulative spend, observation history and
/// estimator-state CAS. Skipped unless the dedicated destructive test database is supplied:
/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::kimi_calibration_postgres_matrix`
#[test]
fn kimi_calibration_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping KIMI PostgreSQL calibration matrix: test URL is unset");
        return;
    };
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
    lock_holder
        .client
        .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
        .unwrap();

    let mut pg = PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    pg.client
        .batch_execute(
            "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
             kimi_calibration_subject_spend,kimi_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();

    let event = KimiTurnCalibrationEvent {
        request_id: "kimi-pg-replay".into(),
        subject_id: "kimi-pg-subject".into(),
        plan: "Moderato".into(),
        requested_model: "kimi-for-coding".into(),
        served_model: "kimi-k2.7-code".into(),
        context_mode: "256k".into(),
        reasoning_effort: "high".into(),
        tariff_schedule_id: "moonshot/kimi/2026-08-03/v1".into(),
        priced_ts: 190,
        completed_at: 200,
        input_tokens: 100,
        cache_read_tokens: 20,
        cache_write_tokens: 0,
        output_tokens: 10,
        reasoning_output_tokens: 4,
        api_input_nanousd: 600,
        api_cache_read_nanousd: 100,
        api_cache_write_nanousd: 0,
        api_output_nanousd: 300,
        api_total_nanousd: 1_000,
    };

    // Two ambiguous replies may race from active and candidate blue-green generations. The
    // immutable key must pick one insert while the loser observes an exact replay, not a
    // unique-violation and never a second spend advance.
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let url = url.clone();
        let event = event.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            let mut pg = PgStore::connect(&url).unwrap();
            barrier.wait();
            pg.record_kimi_turn(&event).unwrap()
        }));
    }
    let mut insert_outcomes = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    insert_outcomes.sort_unstable();
    assert_eq!(insert_outcomes, vec![false, true]);
    assert_eq!(pg.kimi_subject_spend(&event.subject_id).unwrap(), 1_000);

    let mut conflict = event.clone();
    conflict.requested_model = "k3".into();
    let error = pg.record_kimi_turn(&conflict).unwrap_err().to_string();
    assert!(
        error.contains("replay conflict"),
        "unexpected error: {error}"
    );
    assert_eq!(pg.kimi_subject_spend(&event.subject_id).unwrap(), 1_000);

    // Out-of-order finalizers still retain the earliest tracking start and latest update.
    let older = KimiTurnCalibrationEvent {
        request_id: "kimi-pg-older".into(),
        priced_ts: 90,
        completed_at: 100,
        api_input_nanousd: 300,
        api_cache_read_nanousd: 50,
        api_output_nanousd: 150,
        api_total_nanousd: 500,
        ..event.clone()
    };
    assert!(pg.record_kimi_turn(&older).unwrap());
    assert_eq!(pg.kimi_subject_spend(&event.subject_id).unwrap(), 1_500);
    let spend_times = pg
        .client
        .query_one(
            "SELECT tracking_started_ts,updated_ts FROM kimi_calibration_subject_spend \
             WHERE subject_id=$1",
            &[&event.subject_id],
        )
        .unwrap();
    assert_eq!(
        (spend_times.get::<_, i64>(0), spend_times.get::<_, i64>(1)),
        (100, 200)
    );

    let fraction = crate::kimi_fraction_from_native(100, 1_000).unwrap();
    let observation = KimiWindowObservation {
        subject_id: event.subject_id.clone(),
        plan: event.plan.clone(),
        window_duration_secs: crate::KIMI_ROLLING_WINDOW_SECS,
        window_name: Some("rolling".into()),
        resets_at: 10_000,
        observed_at: 300,
        native_used_units: 100,
        native_limit_units: 1_000,
        used_fraction_units: fraction.used_fraction_units,
        measurement_resolution_fraction_units: fraction.measurement_resolution_fraction_units,
        cumulative_api_spend_nano: 1_500,
    };
    let state = KimiCalibrationRow {
        subject_id: observation.subject_id.clone(),
        plan: observation.plan.clone(),
        window_duration_secs: observation.window_duration_secs,
        window_name: observation.window_name.clone(),
        resets_at: observation.resets_at,
        anchor_used_fraction_units: observation.used_fraction_units,
        anchor_resolution_fraction_units: observation.measurement_resolution_fraction_units,
        anchor_spend_nano: observation.cumulative_api_spend_nano,
        used_fraction_units: observation.used_fraction_units,
        measurement_resolution_fraction_units: observation.measurement_resolution_fraction_units,
        observed_at: observation.observed_at,
        native_limit_units: observation.native_limit_units,
        native_used_units: observation.native_used_units,
        observed_fraction_units: 0,
        observed_spend_nano: 0,
        samples: 0,
        unattributed_fraction_units: 0,
        current_capacity_nano: None,
        current_low_nano: None,
        current_high_nano: None,
        current_confidence_bp: 0,
        last_measured_at: None,
        estimator_version: 1,
        version: 0,
        updated_ts: observation.observed_at,
    };
    assert_eq!(
        pg.save_kimi_calibration(&state, &observation).unwrap(),
        Some(1)
    );

    let mut second_observation = observation.clone();
    let second_fraction = crate::kimi_fraction_from_native(101, 1_000).unwrap();
    second_observation.observed_at = 301;
    second_observation.native_used_units = 101;
    second_observation.used_fraction_units = second_fraction.used_fraction_units;
    second_observation.measurement_resolution_fraction_units =
        second_fraction.measurement_resolution_fraction_units;
    let mut second_state = pg
        .load_kimi_calibration(&state.subject_id, &state.plan, state.window_duration_secs)
        .unwrap()
        .unwrap();
    second_state.observed_at = second_observation.observed_at;
    second_state.native_used_units = second_observation.native_used_units;
    second_state.used_fraction_units = second_observation.used_fraction_units;
    second_state.measurement_resolution_fraction_units =
        second_observation.measurement_resolution_fraction_units;
    second_state.updated_ts = second_observation.observed_at;
    assert_eq!(
        pg.save_kimi_calibration(&second_state, &second_observation)
            .unwrap(),
        Some(2)
    );

    // A stale writer loses the CAS and rolls its observation back. Raw history remains exact,
    // oldest-first and contains only the two winning transitions.
    assert_eq!(
        pg.save_kimi_calibration(&state, &observation).unwrap(),
        None
    );
    let history = pg
        .load_kimi_window_observations(&state.subject_id, &state.plan, state.window_duration_secs)
        .unwrap();
    assert_eq!(
        history
            .iter()
            .map(|row| row.observed_at)
            .collect::<Vec<_>>(),
        vec![300, 301]
    );
    let stored = pg
        .load_kimi_calibration(&state.subject_id, &state.plan, state.window_duration_secs)
        .unwrap()
        .unwrap();
    assert_eq!(stored.version, 2);
    assert_eq!(stored.native_used_units, 101);

    pg.client
        .batch_execute(
            "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
             kimi_calibration_subject_spend,kimi_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    lock_holder
        .client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}

/// Real PostgreSQL proof for immutable turn replay, cumulative dual-ledger spend,
/// observation history and estimator-state CAS. Skipped unless the dedicated destructive
/// test database is supplied:
/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::glm_calibration_postgres_matrix`
#[test]
fn glm_calibration_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping GLM PostgreSQL calibration matrix: test URL is unset");
        return;
    };
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
    lock_holder
        .client
        .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
        .unwrap();

    let mut pg = PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    pg.client
        .batch_execute(
            "TRUNCATE glm_window_calibrations,glm_window_observations,\
             glm_calibration_subject_spend,glm_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();

    let event = GlmTurnCalibrationEvent {
        request_id: "glm-pg-replay".into(),
        subject_id: "glm-pg-subject".into(),
        plan: "Pro".into(),
        requested_model: "glm-5".into(),
        served_model: "glm-5.2".into(),
        context_mode: "1m".into(),
        reasoning_effort: Some("high".into()),
        api_tariff_schedule_id: "zhipu/z.ai-open-platform/2026-08-03/v1".into(),
        credit_schedule_id: "zhipu/glm-coding-plan-credits/2026-07-30/v1".into(),
        priced_ts: 190,
        completed_at: 200,
        fresh_input_tokens: 100,
        cached_input_tokens: 20,
        cache_write_tokens: 0,
        output_tokens: 10,
        reasoning_tokens: 4,
        api_fresh_input_nanousd: 600,
        api_cached_input_nanousd: 100,
        api_output_nanousd: 300,
        api_total_nanousd: 1_000,
        native_fresh_input_microcredits: 400,
        native_cached_input_microcredits: 30,
        native_output_microcredits: 70,
        native_total_microcredits: 500,
        off_peak: false,
    };

    // Two ambiguous replies may race from active and candidate blue-green generations. The
    // immutable key must pick one insert while the loser observes an exact replay, not a
    // unique-violation and never a second spend advance on either ledger.
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let url = url.clone();
        let event = event.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            let mut pg = PgStore::connect(&url).unwrap();
            barrier.wait();
            pg.record_glm_turn(&event).unwrap()
        }));
    }
    let mut insert_outcomes = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    insert_outcomes.sort_unstable();
    assert_eq!(insert_outcomes, vec![false, true]);
    assert_eq!(
        pg.glm_subject_spend(&event.subject_id).unwrap(),
        GlmSubjectSpend {
            spent_api_nanousd: 1_000,
            spent_native_microcredits: 500,
        }
    );

    let mut conflict = event.clone();
    conflict.requested_model = "glm-5.1".into();
    let error = pg.record_glm_turn(&conflict).unwrap_err().to_string();
    assert!(
        error.contains("replay conflict"),
        "unexpected error: {error}"
    );
    assert_eq!(
        pg.glm_subject_spend(&event.subject_id).unwrap(),
        GlmSubjectSpend {
            spent_api_nanousd: 1_000,
            spent_native_microcredits: 500,
        }
    );

    // Out-of-order finalizers still retain the earliest tracking start and latest update.
    let older = GlmTurnCalibrationEvent {
        request_id: "glm-pg-older".into(),
        priced_ts: 90,
        completed_at: 100,
        api_fresh_input_nanousd: 300,
        api_cached_input_nanousd: 50,
        api_output_nanousd: 150,
        api_total_nanousd: 500,
        native_fresh_input_microcredits: 100,
        native_cached_input_microcredits: 20,
        native_output_microcredits: 30,
        native_total_microcredits: 150,
        ..event.clone()
    };
    assert!(pg.record_glm_turn(&older).unwrap());
    assert_eq!(
        pg.glm_subject_spend(&event.subject_id).unwrap(),
        GlmSubjectSpend {
            spent_api_nanousd: 1_500,
            spent_native_microcredits: 650,
        }
    );
    let spend_times = pg
        .client
        .query_one(
            "SELECT tracking_started_ts,updated_ts FROM glm_calibration_subject_spend \
             WHERE subject_id=$1",
            &[&event.subject_id],
        )
        .unwrap();
    assert_eq!(
        (spend_times.get::<_, i64>(0), spend_times.get::<_, i64>(1)),
        (100, 200)
    );

    let fraction = crate::glm_fraction_from_native(100, 1_000).unwrap();
    let observation = GlmWindowObservation {
        subject_id: event.subject_id.clone(),
        plan: event.plan.clone(),
        window_duration_secs: crate::GLM_5H_WINDOW_SECS,
        reset_at: Some(10_000),
        observed_at: 300,
        native_used_units: Some(100),
        native_limit_units: Some(1_000),
        native_remaining_units: Some(900),
        percentage_raw: Some(10),
        used_fraction_units: Some(fraction.used_fraction_units),
        measurement_resolution_fraction_units: Some(fraction.measurement_resolution_fraction_units),
        cumulative_api_nanousd: 1_500,
        cumulative_native_microcredits: 650,
        observation_source: "poll".into(),
        source_request_id: None,
    };
    let state = GlmCalibrationRow {
        subject_id: observation.subject_id.clone(),
        plan: observation.plan.clone(),
        window_duration_secs: observation.window_duration_secs,
        reset_at: observation.reset_at,
        anchor_used_fraction_units: observation.used_fraction_units,
        anchor_resolution_fraction_units: observation.measurement_resolution_fraction_units,
        anchor_spend_api_nanousd: observation.cumulative_api_nanousd,
        anchor_spend_native_microcredits: observation.cumulative_native_microcredits,
        used_fraction_units: observation.used_fraction_units,
        measurement_resolution_fraction_units: observation.measurement_resolution_fraction_units,
        observed_at: observation.observed_at,
        // Pro publishes 12 000 credits per 5-hour window.
        native_limit_microcredits: Some(12_000_000_000),
        native_used_microcredits: Some(650),
        observed_fraction_units: 0,
        observed_spend_api_nanousd: 0,
        observed_spend_native_microcredits: 0,
        samples: 0,
        unattributed_fraction_units: 0,
        current_capacity_nanousd: None,
        current_low_nanousd: None,
        current_high_nanousd: None,
        current_confidence_bp: 0,
        last_measured_at: None,
        estimator_version: 1,
        version: 0,
        updated_ts: observation.observed_at,
    };
    assert_eq!(
        pg.save_glm_calibration(&state, &observation).unwrap(),
        Some(1)
    );

    let mut second_observation = observation.clone();
    let second_fraction = crate::glm_fraction_from_native(101, 1_000).unwrap();
    second_observation.observed_at = 301;
    second_observation.native_used_units = Some(101);
    second_observation.native_remaining_units = Some(899);
    second_observation.used_fraction_units = Some(second_fraction.used_fraction_units);
    second_observation.measurement_resolution_fraction_units =
        Some(second_fraction.measurement_resolution_fraction_units);
    let mut second_state = pg
        .load_glm_calibration(&state.subject_id, &state.plan, state.window_duration_secs)
        .unwrap()
        .unwrap();
    second_state.observed_at = second_observation.observed_at;
    second_state.native_used_microcredits = Some(700);
    second_state.used_fraction_units = second_observation.used_fraction_units;
    second_state.measurement_resolution_fraction_units =
        second_observation.measurement_resolution_fraction_units;
    second_state.updated_ts = second_observation.observed_at;
    assert_eq!(
        pg.save_glm_calibration(&second_state, &second_observation)
            .unwrap(),
        Some(2)
    );

    // A stale writer loses the CAS and rolls its observation back. Raw history remains
    // exact, oldest-first and contains only the two winning transitions.
    assert_eq!(pg.save_glm_calibration(&state, &observation).unwrap(), None);
    let history = pg
        .load_glm_window_observations(&state.subject_id, &state.plan, state.window_duration_secs)
        .unwrap();
    assert_eq!(
        history
            .iter()
            .map(|row| row.observed_at)
            .collect::<Vec<_>>(),
        vec![300, 301]
    );
    let stored = pg
        .load_glm_calibration(&state.subject_id, &state.plan, state.window_duration_secs)
        .unwrap()
        .unwrap();
    assert_eq!(stored.version, 2);
    assert_eq!(stored.native_used_microcredits, Some(700));
    assert_eq!(stored.native_remaining_units(), Some(11_999_999_300));

    pg.client
        .batch_execute(
            "TRUNCATE glm_window_calibrations,glm_window_observations,\
             glm_calibration_subject_spend,glm_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    lock_holder
        .client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}

#[test]
fn anthropic_initial_calibration_version_is_bound_as_bigint() {
    assert!(
        ANTHROPIC_CALIBRATION_INSERT_SQL.contains("($22::bigint)+1"),
        "an untyped `$22 + 1` makes PostgreSQL infer int4 and reject the Rust i64 version",
    );
}





/// Run with an isolated database, for example:
/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::pre_cutover_funding_snapshot_postgres_matrix`





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
    pg.client
        .batch_execute(
            "TRUNCATE anthropic_window_observations,anthropic_window_calibrations, \
         provider_turn_calibration_events,provider_calibration_subject_spend, \
         gemini_exact_window_observations,gemini_exact_window_calibrations, \
         gemini_window_observations,gemini_window_calibrations,gemini_profile_spend, \
         codex_turn_calibration_events,codex_window_observations,\
         codex_window_calibrations,codex_home_spend, \
         codex_home_health, \
         execution_group_winner,settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances, \
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
        observation_source: "poll".into(),
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
    pg.client.batch_execute(
        "TRUNCATE execution_group_winner,settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances, \
         usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE;",
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
                charge_basis_nano: 180,
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
        "TRUNCATE execution_group_winner,settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances, \
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

    // The hot authorization statement returns account/key state and the entire bounded pricing
    // override set from one PostgreSQL snapshot. A change is visible on the next call without a
    // TTL cache, and absent overrides preserve the account default.
    let auth = pg.key_account("key").unwrap().unwrap();
    assert!(auth.active);
    assert!(auth.provider_mult_bp.is_empty());
    assert_eq!(auth.mult_for(PROVIDER_OPENAI), 2_000);
    pg.set_account_provider_discount("acct", PROVIDER_OPENAI, 2_500, now())
        .unwrap();
    pg.set_account_provider_discount("acct", PROVIDER_GOOGLE, 10_000, now())
        .unwrap();
    let auth = pg.key_account("key").unwrap().unwrap();
    assert_eq!(
        auth.provider_mult_bp,
        vec![
            (PROVIDER_GOOGLE.to_string(), 10_000),
            (PROVIDER_OPENAI.to_string(), 2_500),
        ]
    );
    assert_eq!(auth.mult_for(PROVIDER_OPENAI), 2_500);
    assert_eq!(auth.mult_for(PROVIDER_ANTHROPIC), 2_000);
    assert!(pg
        .clear_account_provider_discount("acct", PROVIDER_OPENAI)
        .unwrap());
    assert!(pg
        .clear_account_provider_discount("acct", PROVIDER_GOOGLE)
        .unwrap());
    assert!(pg
        .key_account("key")
        .unwrap()
        .unwrap()
        .provider_mult_bp
        .is_empty());

    pg.account_set_status("acct", "disabled").unwrap();
    assert!(!pg.key_account("key").unwrap().unwrap().active);
    pg.account_set_status("acct", "active").unwrap();
    pg.key_set_status("key", "disabled").unwrap();
    assert!(!pg.key_account("key").unwrap().unwrap().active);
    pg.key_set_status("key", "active").unwrap();
    assert!(pg.key_account("key").unwrap().unwrap().active);

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
        charge_basis_nano: 200,
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

    const EXECUTION_GROUP: &str = "428f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";
    const ZERO_EXECUTION_GROUP: &str = "528f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";
    pg.account_create("group-acct", None, 10_000).unwrap();
    pg.account_topup("group-acct", 500, Some("group-seed"))
        .unwrap();
    pg.key_issue("group-key", "group-acct", None).unwrap();
    let group_first = crate::ExecutionAttempt::grouped(EXECUTION_GROUP, 1).unwrap();
    let group_second = crate::ExecutionAttempt::grouped(EXECUTION_GROUP, 2).unwrap();
    assert_eq!(
        pg.reserve_request_for_execution(
            &owner,
            "group-request-1",
            "group-acct",
            "group-key",
            200,
            60,
            &group_first,
        )
        .unwrap(),
        Some(300),
    );
    assert_eq!(
        pg.reserve_request_for_execution(
            &owner,
            "group-request-1",
            "group-acct",
            "group-key",
            200,
            60,
            &group_first,
        )
        .unwrap(),
        Some(300),
    );
    assert!(pg
        .reserve_request_for_execution(
            &owner,
            "group-request-1",
            "group-acct",
            "group-key",
            200,
            60,
            &group_second,
        )
        .is_err());
    assert_eq!(
        pg.reserve_request_for_execution(
            &owner,
            "group-request-2",
            "group-acct",
            "group-key",
            200,
            60,
            &group_second,
        )
        .unwrap(),
        Some(100),
    );
    assert_eq!(
        pg.settle_request("group-request-2", 120, Some("group:second"), None)
            .unwrap(),
        Some(180),
    );
    assert_eq!(
        pg.settle_request("group-request-1", 100, Some("group:first"), None)
            .unwrap(),
        Some(380),
    );
    assert_eq!(
        pg.settle_request("group-request-1", 100, Some("group:first"), None)
            .unwrap(),
        Some(380),
    );
    let group_state = pg
        .client
        .query_one(
            "SELECT account.balance_nano,account.spent_nano,account.reserved_nano,
                    winner.winner_request_id,
                    loser.state,loser.actual_nano,outbox.actual_nano,
                    (SELECT COUNT(*)::bigint FROM ledger
                      WHERE kind='charge'
                        AND request_id IN ('group-request-1','group-request-2'))
               FROM accounts account
               JOIN reservations loser ON loser.account_id=account.id
               JOIN settlement_outbox outbox USING(request_id)
               JOIN execution_group_winner winner ON winner.group_id=$1
              WHERE account.id='group-acct' AND loser.request_id='group-request-1'",
            &[&EXECUTION_GROUP],
        )
        .unwrap();
    assert_eq!(group_state.get::<_, i64>(0), 380);
    assert_eq!(group_state.get::<_, i64>(1), 120);
    assert_eq!(group_state.get::<_, i64>(2), 0);
    assert_eq!(group_state.get::<_, String>(3), "group-request-2");
    assert_eq!(group_state.get::<_, String>(4), "canceled");
    assert_eq!(group_state.get::<_, i64>(5), 0);
    assert_eq!(group_state.get::<_, i64>(6), 100);
    assert_eq!(group_state.get::<_, i64>(7), 1);

    let zero = crate::ExecutionAttempt::grouped(ZERO_EXECUTION_GROUP, 1).unwrap();
    let positive = crate::ExecutionAttempt::grouped(ZERO_EXECUTION_GROUP, 2).unwrap();
    assert_eq!(
        pg.reserve_request_for_execution(
            &owner,
            "group-zero",
            "group-acct",
            "group-key",
            100,
            60,
            &zero,
        )
        .unwrap(),
        Some(280),
    );
    assert_eq!(
        pg.settle_request("group-zero", 0, None, None).unwrap(),
        Some(380),
    );
    assert_eq!(
        pg.client
            .query_one(
                "SELECT COUNT(*)::bigint FROM execution_group_winner WHERE group_id=$1",
                &[&ZERO_EXECUTION_GROUP],
            )
            .unwrap()
            .get::<_, i64>(0),
        0,
    );
    assert_eq!(
        pg.reserve_request_for_execution(
            &owner,
            "group-positive",
            "group-acct",
            "group-key",
            100,
            60,
            &positive,
        )
        .unwrap(),
        Some(280),
    );
    assert_eq!(
        pg.settle_request("group-positive", 50, Some("group:positive"), None)
            .unwrap(),
        Some(330),
    );

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
    let recovered = pg.reconcile_expired(100, false).unwrap();
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
            "TRUNCATE execution_group_winner,settlement_outbox,reservations,capacity_leases,leader_leases,
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




/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::tariff_overrides_postgres_matrix`
#[test]
fn tariff_overrides_postgres_matrix() {
    use crate::pricing::{
        resolve_tariff_override, TariffOverrideInsert, TariffOverrideInsertOutcome as O,
        TariffOverrideRejection as R,
    };

    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!(
            "skipping tariff override PostgreSQL matrix: CLAUDE_API_TEST_DATABASE_URL is unset"
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
        .batch_execute("TRUNCATE pricing_tariff_overrides")
        .unwrap();

    let family = "google/gemini/gemini-2.5-pro";
    let gemini_payload = |cached_input: i128| {
        serde_json::json!({
            "input": "1250",
            "audio_input": "1250",
            "cached_input": cached_input.to_string(),
            "cached_audio_input": "125",
            "output": "10000",
            "image_output": "0",
            "long_context_threshold": 200000,
            "long_input": "2500",
            "long_audio_input": "2500",
            "long_cached_input": "250",
            "long_cached_audio_input": "250",
            "long_output": "15000",
            "search": {"kind": "per_grounded_prompt", "nano": "35000000"}
        })
    };
    // The flat-price Gemini shape carries u64::MAX as the long-context threshold: the digest and
    // the typed read must survive the jsonb numeric round trip at the exact u64 boundary.
    let mut gemini_payload_max_threshold = gemini_payload(300);
    gemini_payload_max_threshold["long_context_threshold"] =
        serde_json::json!(u64::MAX);
    let insert = |version: i64, effective_from: i64, payload: serde_json::Value| {
        TariffOverrideInsert {
            tariff_family: family.to_owned(),
            version,
            effective_from,
            payload,
            created_by: "matrix-operator".to_owned(),
            reason: "postgres matrix".to_owned(),
        }
    };
    let row_count = |pg: &mut PgStore| -> i64 {
        pg.client
            .query_one("SELECT COUNT(*)::bigint FROM pricing_tariff_overrides", &[])
            .unwrap()
            .get(0)
    };

    // Seed v2: effective_from = 0 is allowed only for the first override of a family.
    let seeded = pg
        .insert_tariff_override(&insert(2, 0, gemini_payload(125)))
        .unwrap();
    let O::Inserted(seed_receipt) = seeded else {
        panic!("seed insert returned {seeded:?}");
    };
    assert_eq!(seed_receipt.tariff_family, family);
    assert_eq!(seed_receipt.version, 2);
    assert_eq!(seed_receipt.effective_from, 0);
    assert!(seed_receipt.created_ts > 0);
    assert!(seed_receipt.payload_digest.starts_with("sha256:v2:"));
    assert_eq!(row_count(&mut pg), 1);

    // A non-seed row may not reach into the past beyond the skew grace.
    let past_seed = pg
        .insert_tariff_override(&insert(3, 0, gemini_payload(125)))
        .unwrap();
    assert!(
        matches!(past_seed, O::Rejected(R::Invalid { .. })),
        "v3 with effective_from=0 must be rejected, got {past_seed:?}"
    );
    assert_eq!(row_count(&mut pg), 1);

    // Exact replay of the seed (same family+version+payload+effective_from) is Unchanged even
    // with a different operator attribution.
    let mut replay_insert = insert(2, 0, gemini_payload(125));
    replay_insert.created_by = "other-operator".to_owned();
    let replay = pg.insert_tariff_override(&replay_insert).unwrap();
    let O::Unchanged(replay_receipt) = replay else {
        panic!("exact replay returned {replay:?}");
    };
    assert_eq!(replay_receipt, seed_receipt);
    assert_eq!(row_count(&mut pg), 1);

    // Same key, different payload: typed conflict, nothing written.
    let conflict = pg
        .insert_tariff_override(&insert(2, 0, gemini_payload(126)))
        .unwrap();
    assert_eq!(
        conflict,
        O::Rejected(R::Conflict {
            existing_digest: seed_receipt.payload_digest.clone(),
            existing_effective_from: 0,
        })
    );
    assert_eq!(row_count(&mut pg), 1);

    // Sequence enforcement: v4 while the head is v2 is a typed sequence violation.
    let gap = pg
        .insert_tariff_override(&insert(4, now(), gemini_payload(125)))
        .unwrap();
    assert_eq!(
        gap,
        O::Rejected(R::SequenceViolation { expected_next: 3 })
    );
    assert_eq!(row_count(&mut pg), 1);

    let t0 = now();
    // v3 effective now: allowed.
    let v3 = pg
        .insert_tariff_override(&insert(3, t0, gemini_payload(200)))
        .unwrap();
    let O::Inserted(v3_receipt) = v3 else {
        panic!("v3 insert returned {v3:?}");
    };
    // v4 with a past effective_from (beyond the grace) is rejected...
    let past_v4 = pg
        .insert_tariff_override(&insert(4, t0 - 3_600, gemini_payload(300)))
        .unwrap();
    assert!(
        matches!(past_v4, O::Rejected(R::Invalid { .. })),
        "past v4 must be rejected, got {past_v4:?}"
    );
    // ...while a future effective_from is a scheduled republication and is allowed.
    let v4 = pg
        .insert_tariff_override(&insert(4, t0 + 3_600, gemini_payload_max_threshold.clone()))
        .unwrap();
    let O::Inserted(v4_receipt) = v4 else {
        panic!("v4 insert returned {v4:?}");
    };
    assert_eq!(
        v4_receipt.payload["long_context_threshold"],
        serde_json::json!(u64::MAX)
    );
    assert_eq!(row_count(&mut pg), 3);

    // Unknown families and malformed payloads never reach the database.
    let mut unknown = insert(2, 0, gemini_payload(125));
    unknown.tariff_family = "unknown/provider/model".to_owned();
    let rejected = pg.insert_tariff_override(&unknown).unwrap();
    assert!(
        matches!(rejected, O::Rejected(R::Invalid { .. })),
        "unknown family must be rejected, got {rejected:?}"
    );
    let mut malformed = insert(5, t0, gemini_payload(400));
    malformed.payload["input"] = serde_json::json!(1250.5);
    let rejected = pg.insert_tariff_override(&malformed).unwrap();
    assert!(
        matches!(rejected, O::Rejected(R::Invalid { .. })),
        "float payload must be rejected, got {rejected:?}"
    );
    assert_eq!(row_count(&mut pg), 3);

    // The read side verifies every digest and serves resolution across effective_from boundaries.
    let rows = pg.list_tariff_overrides().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], seed_receipt);
    assert_eq!(rows[1], v3_receipt);
    // rows[2] carries u64::MAX as long_context_threshold: digest verification inside the list
    // read proves the jsonb numeric round trip stayed exact at the u64 boundary.
    assert_eq!(rows[2], v4_receipt);
    assert_eq!(
        resolve_tariff_override(&rows, family, 0).map(|row| row.version),
        Some(2)
    );
    assert_eq!(
        resolve_tariff_override(&rows, family, t0 - 1).map(|row| row.version),
        Some(2)
    );
    assert_eq!(
        resolve_tariff_override(&rows, family, t0).map(|row| row.version),
        Some(3)
    );
    assert_eq!(
        resolve_tariff_override(&rows, family, t0 + 3_600).map(|row| row.version),
        Some(4)
    );
    assert_eq!(resolve_tariff_override(&rows, "google/gemini/gemini-2.5-flash", i64::MAX), None);

    // A row whose stored digest does not match its payload fails the read closed. The writer
    // cannot produce such a row, so it is injected by raw SQL with a well-formed wrong digest.
    pg.client
        .batch_execute(
            "INSERT INTO pricing_tariff_overrides(
                 tariff_family,version,effective_from,payload,payload_digest,created_ts,
                 created_by,reason
             ) VALUES(
                 'tampered/family',2,0,'{}'::jsonb,
                 'sha256:v2:0000000000000000000000000000000000000000000000000000000000000000',
                 1,'matrix-operator','tampered digest'
             )",
        )
        .unwrap();
    let tampered = pg.list_tariff_overrides().expect_err("tampered row must fail closed");
    assert!(
        format!("{tampered:#}").contains("digest verification"),
        "unexpected error: {tampered:#}"
    );

    // The append-only trigger rejects UPDATE and DELETE on any row, including the tampered one.
    let updated = pg
        .client
        .batch_execute(
            "UPDATE pricing_tariff_overrides SET reason='corrected' WHERE tariff_family='tampered/family'",
        )
        .expect_err("UPDATE must be rejected");
    assert!(
        updated
            .as_db_error()
            .is_some_and(|error| error.message().contains("append-only")),
        "unexpected UPDATE error: {updated}"
    );
    let deleted = pg
        .client
        .batch_execute("DELETE FROM pricing_tariff_overrides WHERE tariff_family='tampered/family'")
        .expect_err("DELETE must be rejected");
    assert!(
        deleted
            .as_db_error()
            .is_some_and(|error| error.message().contains("append-only")),
        "unexpected DELETE error: {deleted}"
    );

    // Leave the shared throwaway database clean for the next run.
    pg.client
        .batch_execute("TRUNCATE pricing_tariff_overrides")
        .unwrap();
    assert_eq!(row_count(&mut pg), 0);

    pg.client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}
