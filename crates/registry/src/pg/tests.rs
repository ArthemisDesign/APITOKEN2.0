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
fn request_observability_views_migration_is_expand_only() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 62);
    assert_eq!(
        ENGINE_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == 61)
            .map(|(_, sql)| *sql),
        Some(MIGRATION_0061),
    );
    let normalized = MIGRATION_0061
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(normalized.contains("CREATE VIEW request_fact_usage_daily"));
    assert!(normalized.contains("CREATE VIEW request_fact_tool_usage_daily"));
    assert!(normalized.contains("LEFT JOIN usage_events u ON u.request_id = f.billing_request_id"));
    assert!(normalized.contains("f.account_id"));
    assert!(normalized.contains("f.key_id"));
    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (61)"));
    for forbidden in ["DROP TABLE", "DROP COLUMN", "TRUNCATE", "logical_request_id"] {
        assert!(!normalized.contains(forbidden), "0061 exposes or removes {forbidden}");
    }
}

#[test]
fn request_usage_grafana_rollups_migration_is_expand_only() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 62);
    assert_eq!(
        ENGINE_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == 62)
            .map(|(_, sql)| *sql),
        Some(MIGRATION_0062),
    );
    let normalized = MIGRATION_0062
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for view in [
        "request_fact_usage_top_customer_model_daily",
        "request_fact_usage_top_client_daily",
        "request_fact_usage_top_model_daily",
        "request_fact_usage_top_tool_daily",
    ] {
        assert!(normalized.contains(&format!("CREATE VIEW {view}")));
    }
    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (62)"));
    for forbidden in ["DROP TABLE", "DROP COLUMN", "TRUNCATE", "logical_request_id"] {
        assert!(!normalized.contains(forbidden), "0062 exposes or removes {forbidden}");
    }
}

#[test]
fn gemini_batch_streaming_lifecycle_migration_is_expand_only() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 62);
    assert_eq!(
        ENGINE_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == 60)
            .map(|(_, sql)| *sql),
        Some(MIGRATION_0060),
    );
    let normalized = MIGRATION_0060
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for table in [
        "gemini_batch_admissions",
        "gemini_batch_admission_items",
        "gemini_batch_admission_item_files",
        "gemini_batch_output_builds",
    ] {
        assert!(normalized.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")));
    }
    for column in [
        "received_bytes bigint",
        "terminal_items_ts bigint",
        "output_state text",
        "tombstone_expiration_ts bigint",
    ] {
        assert!(normalized.contains(column), "0060 is missing {column}");
    }
    assert!(normalized.contains("next_item_index bigint NOT NULL DEFAULT 0 CHECK (next_item_index BETWEEN 0 AND 100000)"));
    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (60)"));
    for forbidden in ["DROP TABLE", "DROP COLUMN", "TRUNCATE"] {
        assert!(!normalized.contains(forbidden), "0060 contains {forbidden}");
    }
}

#[test]
fn gemini_batch_ultra_profile_slots_migration_is_expand_only_and_rollback_safe() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 62);
    assert_eq!(
        ENGINE_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == 59)
            .map(|(_, sql)| *sql),
        Some(MIGRATION_0059),
    );
    let normalized = MIGRATION_0059
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(normalized.contains("CREATE TABLE IF NOT EXISTS gemini_batch_profile_leases_extra"));
    assert!(normalized.contains("slot_number smallint NOT NULL CHECK (slot_number BETWEEN 3 AND 20)"));
    assert!(normalized.contains("CREATE TABLE IF NOT EXISTS gemini_batch_profile_dispatch_state"));
    assert!(normalized.contains("next_dispatch_not_before_ms bigint"));
    assert!(normalized.contains("updated_ts_ms bigint"));
    assert!(normalized.contains("pg_advisory_xact_lock(hashtextextended(OLD.profile_id, 552966749))"));
    assert!(normalized.contains("AFTER DELETE ON gemini_batch_profile_leases"));
    assert!(normalized.contains("AFTER DELETE ON gemini_batch_profile_leases_slot2"));
    assert!(normalized.contains("ON CONFLICT (profile_id) DO NOTHING"));
    assert!(normalized.contains("VALUES (59)"));
    for forbidden in ["ALTER TABLE", "DROP TABLE", "DROP COLUMN", "TRUNCATE"] {
        assert!(!normalized.contains(forbidden), "0059 contains {forbidden}");
    }
}

#[test]
fn gemini_batch_second_profile_slot_migration_is_expand_only_and_rollback_safe() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 62);
    assert_eq!(
        ENGINE_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == 58)
            .map(|(_, sql)| *sql),
        Some(MIGRATION_0058),
    );
    let normalized = MIGRATION_0058
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(normalized.contains("CREATE TABLE IF NOT EXISTS gemini_batch_profile_leases_slot2"));
    assert!(normalized.contains("CREATE TRIGGER gemini_batch_profile_slot2_promote"));
    assert!(normalized.contains("AFTER DELETE ON gemini_batch_profile_leases"));
    assert!(normalized.contains("ON CONFLICT (profile_id) DO NOTHING"));
    assert!(normalized.contains("VALUES (58)"));
    for forbidden in ["ALTER TABLE", "DROP TABLE", "DROP COLUMN", "TRUNCATE"] {
        assert!(!normalized.contains(forbidden), "0058 contains {forbidden}");
    }
}

#[test]
fn gemini_batch_chunked_file_shape_migration_replaces_only_the_narrow_check() {
    assert_eq!(
        ENGINE_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == 57)
            .map(|(_, sql)| *sql),
        Some(MIGRATION_0057),
    );
    let normalized = MIGRATION_0057.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(normalized.contains("ADD COLUMN IF NOT EXISTS storage_kind text"));
    assert!(normalized.contains("gemini_batch_files_state_storage_shape"));
    assert!(normalized.contains("storage_kind = 'chunked'"));
    assert!(normalized.contains("storage_kind = 'inline_legacy'"));
    assert!(normalized.contains("VALUES (57)"));
    assert!(!normalized.contains("DROP TABLE"));
    assert!(!normalized.contains("DROP COLUMN"));
}

#[test]
fn gemini_batch_authority_correction_migration_is_expand_only() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 62);
    assert_eq!(
        ENGINE_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == 56)
            .map(|(_, sql)| *sql),
        Some(MIGRATION_0056),
    );
    let normalized = MIGRATION_0056
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(normalized.contains("CREATE TABLE IF NOT EXISTS gemini_batch_file_chunks"));
    assert!(normalized.contains("ADD COLUMN IF NOT EXISTS key_id text"));
    assert!(normalized.contains("ADD COLUMN IF NOT EXISTS creator_key_id text"));
    assert!(normalized.contains("ALTER COLUMN result_expiration_ts DROP NOT NULL"));
    assert!(normalized.contains("calibration_api_total_nanousd bigint"));
    assert!(normalized.contains("num_nonnulls("));
    assert!(normalized.contains("VALUES (56)"));
    for forbidden in ["DROP TABLE", "DROP COLUMN", "TRUNCATE", "CREATE TRIGGER"] {
        assert!(!normalized.contains(forbidden), "0056 contains {forbidden}");
    }
}

#[test]
fn gemini_batch_foundation_migration_is_dormant_expand_only() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 62);
    assert_eq!(
        ENGINE_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == 55)
            .map(|(_, sql)| *sql),
        Some(MIGRATION_0055),
    );

    let executable = MIGRATION_0055
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let normalized = executable.split_whitespace().collect::<Vec<_>>().join(" ");

    for table in [
        "gemini_batch_jobs",
        "gemini_batch_items",
        "gemini_batch_item_files",
        "gemini_batch_blobs",
        "gemini_batch_files",
        "gemini_batch_settlement_outbox",
        "gemini_batch_profile_leases",
    ] {
        assert!(
            normalized.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")),
            "migration 0055 is missing {table}",
        );
    }
    for forbidden in [
        "ALTER TABLE",
        "DROP TABLE",
        "DROP COLUMN",
        "TRUNCATE",
        "CREATE TRIGGER",
        "UPDATE accounts",
        "UPDATE api_keys",
        "UPDATE reservations",
        "UPDATE ledger",
        "UPDATE usage_events",
    ] {
        assert!(
            !normalized.contains(forbidden),
            "dormant migration 0055 contains forbidden legacy/runtime mutation: {forbidden}",
        );
    }
    assert_eq!(
        normalized.matches("INSERT INTO").count(),
        1,
        "0055 may insert only its engine_schema_migrations bookkeeping row",
    );
    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (55)"));
    assert!(!normalized.contains("request_payload json"));
    assert!(!normalized.contains("result_payload json"));
    assert!(!normalized.contains("metadata json"));
    for forbidden_counter in [
        "request_count",
        "successful_request_count",
        "failed_request_count",
        "pending_request_count",
    ] {
        assert!(
            !normalized.contains(forbidden_counter),
            "batchStats must remain derived-on-read, found {forbidden_counter}",
        );
    }
    assert!(normalized.contains("ON gemini_batch_items(job_id, state, item_index)"));
    assert!(normalized.contains("ON gemini_batch_item_files(file_id, job_id, item_index)"));
    assert!(normalized.contains("WHERE idempotency_digest IS NOT NULL"));
}

/// Real PostgreSQL proof that migration 0055 applies, constrains the dormant authority, remains
/// replay-safe, and leaves legacy money writers usable. Skipped unless an isolated destructive test
/// database is supplied through `CLAUDE_API_TEST_DATABASE_URL`.
#[test]
fn gemini_batch_foundation_migration_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping Gemini batch foundation matrix: test URL is unset");
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

    let mut pg = PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    assert_eq!(pg.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);

    let tables: Vec<String> = pg
        .client
        .query(
            "SELECT tablename FROM pg_tables \
             WHERE schemaname='public' AND tablename LIKE 'gemini_batch_%' \
             ORDER BY tablename",
            &[],
        )
        .unwrap()
        .into_iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(
        tables,
        vec![
            "gemini_batch_admission_item_files",
            "gemini_batch_admission_items",
            "gemini_batch_admissions",
            "gemini_batch_blobs",
            "gemini_batch_file_chunks",
            "gemini_batch_files",
            "gemini_batch_item_files",
            "gemini_batch_items",
            "gemini_batch_jobs",
            "gemini_batch_output_builds",
            "gemini_batch_profile_dispatch_state",
            "gemini_batch_profile_leases",
            "gemini_batch_profile_leases_extra",
            "gemini_batch_profile_leases_slot2",
            "gemini_batch_settlement_outbox",
        ],
    );

    let account = "gemini-batch-migration-account";
    let raw_key = "gemini-batch-migration-key";
    let key_id = "gemini-batch-migration-key-id";
    let job = "gemini-batch-migration-job";
    // A host slot may retain rows after a killed candidate process. Clean children before their
    // restrictive owner so rerunning the matrix proves replay instead of tripping on its own residue.
    pg.client
        .batch_execute(&format!(
            "DELETE FROM gemini_batch_profile_leases_extra WHERE job_id='{job}'; \
             DELETE FROM gemini_batch_profile_leases_slot2 WHERE job_id='{job}'; \
             DELETE FROM gemini_batch_profile_leases WHERE job_id='{job}'; \
             DELETE FROM gemini_batch_settlement_outbox WHERE job_id='{job}'; \
             DELETE FROM gemini_batch_blobs WHERE job_id='{job}'; \
             DELETE FROM gemini_batch_item_files WHERE job_id='{job}'; \
             DELETE FROM gemini_batch_items WHERE job_id='{job}'; \
             DELETE FROM gemini_batch_jobs WHERE job_id='{job}'; \
             DELETE FROM api_keys WHERE key='{raw_key}'; \
             DELETE FROM accounts WHERE id='{account}';"
        ))
        .unwrap();
    pg.client
        .execute(
            "INSERT INTO accounts(id,balance_nano,spent_nano,mult_bp,status,created_ts,created) \
             VALUES($1,1000000,0,5000,'active',1,'migration-matrix')",
            &[&account],
        )
        .unwrap();
    pg.client
        .execute(
            "INSERT INTO api_keys(key,key_id,account_id,created_ts,created) \
             VALUES($1,$2,$3,1,'migration-matrix')",
            &[&raw_key, &key_id, &account],
        )
        .unwrap();
    pg.client
        .execute(
            "INSERT INTO gemini_batch_jobs( \
                 job_id,account_id,creator_key_id,public_model,display_name, \
                 canonical_request_digest,input_kind,schema_version,encryption_policy_version, \
                 create_ts,update_ts,deadline_ts \
             ) VALUES($1,$2,$3,'gemini-2.5-flash','matrix',decode(repeat('11',32),'hex'), \
                 'inline',1,1,10,10,20)",
            &[&job, &account, &key_id],
        )
        .unwrap();
    pg.client
        .execute(
            "INSERT INTO gemini_batch_items( \
                 job_id,item_index,request_id,logical_request_id,execution_group_id,request_digest, \
                 hold_nano,payable_multiplier_bp,priced_ts,tariff_family,tariff_version, \
                 tariff_schedule_id,state,created_ts,updated_ts \
             ) VALUES($1,0,'gemini-batch-item-request','gemini-batch-item-logical', \
                 'gemini-batch-item-group',decode(repeat('22',32),'hex'),100,5000,10, \
                 'google/gemini/gemini-2.5-flash',1, \
                 'google/gemini/gemini-2.5-flash/v1','queued',10,10)",
            &[&job],
        )
        .unwrap();

    let invalid_half_fence = pg.client.execute(
        "UPDATE gemini_batch_items SET worker_instance='worker' \
         WHERE job_id=$1 AND item_index=0",
        &[&job],
    );
    assert_eq!(
        invalid_half_fence
            .expect_err("half-populated claim fence must fail")
            .as_db_error()
            .map(|error| error.code().code()),
        Some("23514"),
    );
    let invalid_terminal = pg.client.execute(
        "UPDATE gemini_batch_items SET terminal_ts=11,terminal_class='success' \
         WHERE job_id=$1 AND item_index=0",
        &[&job],
    );
    assert_eq!(
        invalid_terminal
            .expect_err("terminal evidence on queued item must fail")
            .as_db_error()
            .map(|error| error.code().code()),
        Some("23514"),
    );

    pg.client
        .execute("DELETE FROM api_keys WHERE key=$1", &[&raw_key])
        .unwrap();
    let creator_key_id: String = pg
        .client
        .query_one(
            "SELECT creator_key_id FROM gemini_batch_jobs WHERE job_id=$1",
            &[&job],
        )
        .unwrap()
        .get(0);
    assert_eq!(creator_key_id, key_id);
    let account_delete = pg
        .client
        .execute("DELETE FROM accounts WHERE id=$1", &[&account]);
    let account_delete_code = account_delete
        .expect_err("accepted batch must restrict account deletion")
        .as_db_error()
        .map(|error| error.code().code().to_owned());
    assert!(
        matches!(account_delete_code.as_deref(), Some("23001" | "23503")),
        "restrictive account delete returned unexpected SQLSTATE: {account_delete_code:?}",
    );

    // The immutable contiguous migration registry proves replay. Re-running the entire current
    // migration plan over a synthetic dormant job now also asks later reporting-view DDL to rebind
    // unrelated base relations, which is not a production upgrade path. Verify the current schema
    // marker and row preservation directly.
    assert_eq!(pg.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    let item_count: i64 = pg
        .client
        .query_one(
            "SELECT COUNT(*)::bigint FROM gemini_batch_items WHERE job_id=$1",
            &[&job],
        )
        .unwrap()
        .get(0);
    assert_eq!(item_count, 1, "migration replay must preserve dormant rows");

    pg.client
        .execute("DELETE FROM gemini_batch_items WHERE job_id=$1", &[&job])
        .unwrap();
    pg.client
        .execute("DELETE FROM gemini_batch_jobs WHERE job_id=$1", &[&job])
        .unwrap();
    pg.client
        .execute("DELETE FROM accounts WHERE id=$1", &[&account])
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
    assert_eq!(CURRENT_SCHEMA_VERSION, 62);
    let registered = ENGINE_MIGRATIONS
        .iter()
        .find(|(version, _)| *version == 29)
        .map(|(_, sql)| *sql);
    assert_eq!(registered, Some(MIGRATION_0029));
}

#[test]
fn tripo3d_calibration_migration_is_additive_and_keeps_dual_ledger_identity() {
    // Strip `--` comment lines first: the header prose deliberately names the 0019, 0027 and
    // 0029 authorities to explain why this migration stands beside them, and those mentions
    // must not be mistaken for statements touching them.
    let ddl = MIGRATION_0049
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let normalized = ddl.split_whitespace().collect::<Vec<_>>().join(" ");

    for table in [
        "tripo3d_turn_calibration_events",
        "tripo3d_calibration_subject_spend",
        "tripo3d_balance_observations",
        "tripo3d_calibration_state",
    ] {
        assert!(
            normalized.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")),
            "missing Tripo3D calibration table {table}",
        );
    }

    // Expand-only: nothing is dropped, truncated or altered.
    assert!(!normalized.contains(" DROP TABLE "));
    assert!(!normalized.contains(" TRUNCATE "));
    assert!(!normalized.contains(" DROP CONSTRAINT "));
    assert!(!normalized.contains(" ALTER TABLE "));

    // The 0019 shared authority, the KIMI 0027 authority and the GLM 0029 authority must all
    // be left completely untouched: none of their durable identities can carry a per-task
    // native credit total on a windowless prepaid balance track.
    assert!(!normalized.contains("provider_turn_calibration_events"));
    assert!(!normalized.contains("provider_calibration_subject_spend"));
    assert!(!normalized.contains("kimi_turn_calibration_events"));
    assert!(!normalized.contains("kimi_calibration_subject_spend"));
    assert!(!normalized.contains("kimi_window_observations"));
    assert!(!normalized.contains("kimi_window_calibrations"));
    assert!(!normalized.contains("glm_turn_calibration_events"));
    assert!(!normalized.contains("glm_calibration_subject_spend"));
    assert!(!normalized.contains("glm_window_observations"));
    assert!(!normalized.contains("glm_window_calibrations"));

    // Requested and resolved model versions are separate columns, nullable because
    // version-independent task kinds exist.
    assert!(normalized.contains("requested_model_version text CHECK"));
    assert!(normalized.contains("resolved_model_version text CHECK"));
    assert!(!normalized.contains("requested_model_version text NOT NULL"));

    // The upstream task id is audit metadata, never the money identity: request_id is the PK.
    assert!(normalized.contains("PRIMARY KEY (request_id)"));
    assert!(normalized.contains("upstream_task_id text NOT NULL"));

    // Dual ledger at the published fixed rate: the API nanoUSD leg is the exact fixed-rate
    // image of the native millicredit leg, which also makes a partial zero impossible. A zero
    // pair stays legal for the documented free tasks.
    assert!(normalized
        .contains("native_total_millicredits bigint NOT NULL CHECK (native_total_millicredits >= 0)"));
    assert!(normalized.contains("api_total_nanousd bigint NOT NULL CHECK (api_total_nanousd >= 0)"));
    assert!(normalized.contains("CHECK (api_total_nanousd = native_total_millicredits * 10000)"));
    assert!(normalized.contains("spent_api_nanousd bigint NOT NULL"));
    assert!(normalized.contains("spent_native_millicredits bigint NOT NULL"));

    // Raw balance floats are preserved verbatim as text; the parsed fixed-point halves stay
    // NULL until the unit is proven — unknown stays NULL, never 0.
    assert!(normalized.contains("balance_raw text NOT NULL"));
    assert!(normalized.contains("frozen_raw text NOT NULL"));
    assert!(normalized.contains(
        "balance_micro_units bigint CHECK (balance_micro_units IS NULL OR balance_micro_units >= 0)"
    ));
    assert!(normalized.contains(
        "frozen_micro_units bigint CHECK (frozen_micro_units IS NULL OR frozen_micro_units >= 0)"
    ));

    // Balance arrives by poll and in the wake of responses; a response names its request, a
    // poll invents none, and the dedup key treats NULL parsed halves as equal.
    assert!(normalized.contains("observation_source IN ('poll', 'response')"));
    assert!(normalized.contains("source_request_id"));
    assert!(normalized.contains("UNIQUE NULLS NOT DISTINCT"));

    // No window anywhere: prepaid balance never resets, so there is no duration to key on and
    // the state keys on subject + declared top-up cohort.
    assert!(!normalized.contains("window_duration_secs"));
    assert!(!normalized.contains("reset_at"));
    assert!(normalized.contains("PRIMARY KEY (subject_id, cohort)"));

    // The cold/measured split: cold publishes nothing; measured requires proven balance
    // halves, movement on both spend ledgers and a capacity with a proven low.
    assert!(normalized.contains("latest_balance_micro_units IS NOT NULL"));
    assert!(normalized.contains("current_capacity_nanousd IS NOT NULL"));

    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (49)"));
}

#[test]
fn tripo3d_calibration_migration_is_registered_at_the_current_schema_version() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 62);
    let registered = ENGINE_MIGRATIONS
        .iter()
        .find(|(version, _)| *version == 49)
        .map(|(_, sql)| *sql);
    // Compare by content, not by identity: two `&str` constants over the same source are not
    // guaranteed to share an address.
    assert_eq!(registered, Some(MIGRATION_0049));
    assert_eq!(
        ENGINE_MIGRATIONS.last().map(|(version, _)| *version),
        Some(CURRENT_SCHEMA_VERSION)
    );
}

#[test]
fn suno_calibration_migration_is_additive_and_keeps_dual_ledger_identity() {
    // Strip `--` comment lines first: the header prose deliberately names the 0019, 0027,
    // 0029 and 0049 authorities to explain why this migration stands beside them, and those
    // mentions must not be mistaken for statements touching them.
    let ddl = MIGRATION_0050
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let normalized = ddl.split_whitespace().collect::<Vec<_>>().join(" ");

    for table in [
        "suno_turn_calibration_events",
        "suno_calibration_subject_spend",
        "suno_window_observations",
        "suno_window_calibrations",
    ] {
        assert!(
            normalized.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")),
            "missing Suno calibration table {table}",
        );
    }

    // Expand-only: nothing is dropped, truncated or altered.
    assert!(!normalized.contains(" DROP TABLE "));
    assert!(!normalized.contains(" TRUNCATE "));
    assert!(!normalized.contains(" DROP CONSTRAINT "));
    assert!(!normalized.contains(" ALTER TABLE "));

    // The shared 0019 authority, the KIMI 0027 authority, the GLM 0029 authority and the
    // Tripo3D 0049 authority must all be left completely untouched: none of their durable
    // identities can carry a schedule-derived per-turn native credit total on a monthly
    // subscription window.
    assert!(!normalized.contains("provider_turn_calibration_events"));
    assert!(!normalized.contains("provider_calibration_subject_spend"));
    assert!(!normalized.contains("kimi_turn_calibration_events"));
    assert!(!normalized.contains("kimi_calibration_subject_spend"));
    assert!(!normalized.contains("kimi_window_observations"));
    assert!(!normalized.contains("kimi_window_calibrations"));
    assert!(!normalized.contains("glm_turn_calibration_events"));
    assert!(!normalized.contains("glm_calibration_subject_spend"));
    assert!(!normalized.contains("glm_window_observations"));
    assert!(!normalized.contains("glm_window_calibrations"));
    assert!(!normalized.contains("tripo3d_turn_calibration_events"));
    assert!(!normalized.contains("tripo3d_calibration_subject_spend"));
    assert!(!normalized.contains("tripo3d_balance_observations"));
    assert!(!normalized.contains("tripo3d_calibration_state"));

    // Only the paid plans are admitted; Free is excluded by design.
    assert!(normalized.contains("plan text NOT NULL CHECK (plan IN ('Pro', 'Premier'))"));

    // The served model stays nullable until the live matrix pins the wire id spellings; the
    // requested model is always known.
    assert!(normalized.contains("requested_model text NOT NULL"));
    assert!(normalized.contains("served_model text CHECK (served_model IS NULL"));
    assert!(!normalized.contains("served_model text NOT NULL"));

    // The upstream clip id is audit metadata, never the money identity: request_id is the PK.
    assert!(normalized.contains("PRIMARY KEY (request_id)"));
    assert!(normalized.contains("upstream_clip_id text NOT NULL"));

    // Dual ledger at the reviewed derived fixed rate: the API nanoUSD leg is the exact
    // fixed-rate image of the native millicredit leg, which also makes a partial zero
    // impossible. A zero pair stays legal for a refunded failed generation.
    assert!(normalized
        .contains("native_total_millicredits bigint NOT NULL CHECK (native_total_millicredits >= 0)"));
    assert!(normalized.contains("api_total_nanousd bigint NOT NULL CHECK (api_total_nanousd >= 0)"));
    assert!(normalized.contains("CHECK (api_total_nanousd = native_total_millicredits * 4000)"));
    assert!(normalized.contains("spent_api_nanousd bigint NOT NULL"));
    assert!(normalized.contains("spent_native_millicredits bigint NOT NULL"));

    // A schedule-derived native leg is flagged as such, never presented as provider truth.
    assert!(normalized.contains("native_schedule_derived boolean NOT NULL"));

    // The window dimension is present in full: paid plan and the exact native window duration
    // are part of the durable identity, with the reset anchor stored only when `period`
    // supplies it.
    assert!(normalized.contains("PRIMARY KEY (subject_id, plan, window_duration_secs)"));
    assert!(normalized.contains("reset_at bigint CHECK (reset_at IS NULL OR reset_at > 0)"));

    // Raw quota counters are verbatim and nullable: unknown stays NULL, never 0. The raw
    // `period` text is preserved. The derived fraction pair exists only when the field
    // semantics allow it.
    assert!(normalized.contains(
        "native_used_units bigint CHECK (native_used_units IS NULL OR native_used_units >= 0)"
    ));
    assert!(normalized.contains("period_raw text CHECK (period_raw IS NULL OR period_raw <> '')"));
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

    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (50)"));
}

#[test]
fn suno_calibration_migration_is_registered_at_the_current_schema_version() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 62);
    let registered = ENGINE_MIGRATIONS
        .iter()
        .find(|(version, _)| *version == 50)
        .map(|(_, sql)| *sql);
    // Compare by content, not by identity: two `&str` constants over the same source are not
    // guaranteed to share an address.
    assert_eq!(registered, Some(MIGRATION_0050));
    assert_eq!(
        ENGINE_MIGRATIONS.last().map(|(version, _)| *version),
        Some(CURRENT_SCHEMA_VERSION)
    );
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

#[test]
fn settlement_floor_accounting_migration_is_expand_only_and_auditable() {
    let normalized = MIGRATION_0047
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for column in [
        "ADD COLUMN IF NOT EXISTS uncollected_nano bigint NOT NULL DEFAULT 0",
        "ADD COLUMN IF NOT EXISTS collected_nano bigint",
        "ADD COLUMN IF NOT EXISTS provider text",
        "ADD COLUMN IF NOT EXISTS payable_multiplier_bp bigint",
        "ADD COLUMN IF NOT EXISTS charge_basis_nano bigint",
    ] {
        assert!(normalized.contains(column), "missing expansion: {column}");
    }
    assert!(normalized.contains("actual_nano = collected_nano + uncollected_nano"));
    assert!(normalized.contains("uncollected_nano <= charge_nano"));
    assert!(normalized.contains("VALIDATE CONSTRAINT reservations_settlement_collection_shape"));
    assert!(!normalized.contains(" DROP "));
    assert!(!normalized.contains(" DELETE "));
    assert!(!normalized.contains(" UPDATE "));
    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (47)"));
}

#[test]
fn settlement_floor_terminal_fence_migration_blocks_mixed_version_writers() {
    let normalized = MIGRATION_0048
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(normalized.contains("CREATE TRIGGER accounts_settlement_floor_fence"));
    assert!(normalized.contains("CREATE TRIGGER reservations_priced_terminal_collection_fence"));
    assert!(normalized.contains("NEW.spent_nano > OLD.spent_nano"));
    assert!(normalized.contains("OLD.balance_nano < -1000000000"));
    assert!(normalized.contains("ELSE -1000000000"));
    assert!(normalized.contains("CHECK ((provider IS NULL) = (payable_multiplier_bp IS NULL))"));
    assert!(normalized.contains("reservations_priced_terminal_collection_evidence"));
    assert!(normalized.contains("state NOT IN ('settled', 'canceled')"));
    assert!(normalized.contains("collected_nano IS NOT NULL AND uncollected_nano IS NOT NULL"));
    assert_eq!(normalized.matches("USING ERRCODE = '40001'").count(), 2);
    assert!(normalized.contains("VALIDATE CONSTRAINT reservations_scalar_pricing_pair"));
    assert!(normalized
        .contains("VALIDATE CONSTRAINT reservations_priced_terminal_collection_evidence"));
    assert!(!normalized.contains(" DROP TABLE "));
    assert!(!normalized.contains(" TRUNCATE "));
    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (48)"));
}

#[test]
fn tripo3d_pricing_provider_migration_widens_both_closed_sets() {
    // Strip `--` comment lines first: the header names the 0046/0047 constraints to explain the
    // widening, and those mentions must not be mistaken for the statements themselves.
    let ddl = MIGRATION_0051
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let normalized = ddl.split_whitespace().collect::<Vec<_>>().join(" ");

    // Both closed provider sets widen with `tripo3d`, keeping every existing id.
    assert!(normalized.contains(
        "CHECK (provider_id IN ('anthropic', 'openai', 'google', 'kimi', 'glm', 'tripo3d'))"
    ));
    assert!(normalized.contains(
        "(provider IS NULL OR provider IN ('anthropic', 'openai', 'google', 'kimi', 'glm', 'tripo3d'))"
    ));
    // The multiplier bound survives the re-add unchanged.
    assert!(normalized
        .contains("(payable_multiplier_bp IS NULL OR payable_multiplier_bp BETWEEN 0 AND 10000)"));
    // Expand-only widening: constraints are replaced (the predicate only grows), no table or
    // data is touched, and both re-added constraints are validated.
    assert!(!normalized.contains(" DROP TABLE "));
    assert!(!normalized.contains(" TRUNCATE "));
    assert!(!normalized.contains(" DELETE "));
    assert!(!normalized.contains(" UPDATE "));
    assert!(normalized.contains("VALIDATE CONSTRAINT account_provider_discounts_provider_id_check"));
    assert!(normalized.contains("VALIDATE CONSTRAINT reservations_scalar_pricing_shape"));
    // No other provider's authority is mentioned by a statement.
    assert!(!normalized.contains("suno"));

    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (51)"));
}

#[test]
fn tripo3d_pricing_provider_migration_is_registered_at_the_current_schema_version() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 62);
    let registered = ENGINE_MIGRATIONS
        .iter()
        .find(|(version, _)| *version == 51)
        .map(|(_, sql)| *sql);
    // Compare by content, not by identity: two `&str` constants over the same source are not
    // guaranteed to share an address.
    assert_eq!(registered, Some(MIGRATION_0051));
    assert_eq!(
        ENGINE_MIGRATIONS.last().map(|(version, _)| *version),
        Some(CURRENT_SCHEMA_VERSION)
    );
}

#[test]
fn suno_pricing_provider_migration_widens_both_closed_sets() {
    // Strip `--` comment lines first: the header names the 0046/0047/0051 constraints to explain
    // the widening, and those mentions must not be mistaken for the statements themselves.
    let ddl = MIGRATION_0052
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let normalized = ddl.split_whitespace().collect::<Vec<_>>().join(" ");

    // Both closed provider sets widen with `suno`, keeping every existing id.
    assert!(normalized.contains(
        "CHECK (provider_id IN ('anthropic', 'openai', 'google', 'kimi', 'glm', 'tripo3d', 'suno'))"
    ));
    assert!(normalized.contains(
        "(provider IS NULL OR provider IN ('anthropic', 'openai', 'google', 'kimi', 'glm', 'tripo3d', 'suno'))"
    ));
    // The multiplier bound survives the re-add unchanged.
    assert!(normalized
        .contains("(payable_multiplier_bp IS NULL OR payable_multiplier_bp BETWEEN 0 AND 10000)"));
    // Expand-only widening: constraints are replaced (the predicate only grows), no table or
    // data is touched, and both re-added constraints are validated.
    assert!(!normalized.contains(" DROP TABLE "));
    assert!(!normalized.contains(" TRUNCATE "));
    assert!(!normalized.contains(" DELETE "));
    assert!(!normalized.contains(" UPDATE "));
    assert!(normalized.contains("VALIDATE CONSTRAINT account_provider_discounts_provider_id_check"));
    assert!(normalized.contains("VALIDATE CONSTRAINT reservations_scalar_pricing_shape"));

    assert!(normalized.contains("INSERT INTO engine_schema_migrations(version) VALUES (52)"));
}

#[test]
fn suno_pricing_provider_migration_is_registered_at_the_current_schema_version() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 62);
    let registered = ENGINE_MIGRATIONS
        .iter()
        .find(|(version, _)| *version == 52)
        .map(|(_, sql)| *sql);
    // Compare by content, not by identity: two `&str` constants over the same source are not
    // guaranteed to share an address.
    assert_eq!(registered, Some(MIGRATION_0052));
    assert_eq!(
        ENGINE_MIGRATIONS.last().map(|(version, _)| *version),
        Some(CURRENT_SCHEMA_VERSION)
    );
}

/// Real PostgreSQL proof for the dormant request-fact storage shape and replay behavior.
/// Skipped unless an isolated destructive test database is supplied:
/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::request_facts_migration_postgres_matrix`
#[test]
fn request_facts_migration_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping request-facts migration matrix: test URL is unset");
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

    let mut pg = PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    assert_eq!(pg.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);

    let table_exists: bool = pg
        .client
        .query_one(
            "SELECT to_regclass('public.request_facts') IS NOT NULL",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(table_exists, "migration 0053 must create request_facts");

    let check_definitions: Vec<String> = pg
        .client
        .query(
            "SELECT pg_get_constraintdef(oid) \
               FROM pg_constraint \
              WHERE conrelid = 'public.request_facts'::regclass \
                AND contype = 'c'",
            &[],
        )
        .unwrap()
        .into_iter()
        .map(|row| row.get(0))
        .collect();
    let checks = check_definitions.join(" ");
    for (vocabulary, values) in [
        (
            "client_kind",
            &[
                "claude_code",
                "opencode",
                "codex_cli",
                "cursor",
                "sdk",
                "custom",
                "unknown",
            ][..],
        ),
        ("client_source", &["explicit", "heuristic", "unknown"][..]),
        (
            "tool_choice_mode",
            &["auto", "required", "none", "named", "unknown"][..],
        ),
        (
            "provider_terminal_class",
            &[
                "success",
                "client_error",
                "quota",
                "auth",
                "timeout",
                "transport",
                "upstream_error",
                "protocol_error",
                "unknown",
            ][..],
        ),
        (
            "billing_outcome",
            &[
                "winner",
                "loser",
                "zero_metered",
                "canceled",
                "reconciled",
                "not_applicable",
                "unknown",
            ][..],
        ),
    ] {
        let definition = check_definitions
            .iter()
            .find(|definition| definition.contains(vocabulary))
            .unwrap_or_else(|| panic!("request_facts lacks the {vocabulary} CHECK: {checks}"));
        for value in values {
            assert!(
                definition.contains(&format!("'{value}'::text")),
                "request_facts {vocabulary} CHECK is missing {value}: {definition}"
            );
        }
    }

    let indexes: Vec<(String, String)> = pg
        .client
        .query(
            "SELECT indexname,indexdef \
               FROM pg_indexes \
              WHERE schemaname = 'public' AND tablename = 'request_facts' \
              ORDER BY indexname",
            &[],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    for (expected, shape) in [
        (
            "request_facts_logical_attempt_idx",
            "(logical_request_id, attempt)",
        ),
        (
            "request_facts_account_admitted_idx",
            "(account_id, admitted_at DESC, fact_id)",
        ),
        ("request_facts_admitted_idx", "(admitted_at)"),
    ] {
        let definition = indexes
            .iter()
            .find(|(name, _)| name == expected)
            .map(|(_, definition)| definition)
            .unwrap_or_else(|| panic!("missing request_facts index {expected}: {indexes:?}"));
        assert!(
            definition.ends_with(shape),
            "request_facts index {expected} has the wrong shape: {definition}"
        );
    }
    let billing_unique = indexes
        .iter()
        .find(|(name, _)| name == "request_facts_billing_request_id_key")
        .map(|(_, definition)| definition)
        .expect("billing_request_id UNIQUE must create its constraint index");
    assert!(billing_unique.contains("CREATE UNIQUE INDEX"));
    assert!(billing_unique.ends_with("(billing_request_id)"));

    pg.client
        .execute(
            "DELETE FROM request_facts \
              WHERE logical_request_id IN ($1,$2) OR billing_request_id=$3",
            &[
                &"request-facts-migration-matrix",
                &"request-facts-migration-invalid-client",
                &"request-facts-migration-billing",
            ],
        )
        .unwrap();
    pg.client
        .execute(
            "INSERT INTO request_facts( \
                 logical_request_id,billing_request_id,account_id,key_id,client_kind,client_source, \
                 provider_plane,route_class,request_class,tool_choice_mode, \
                 provider_terminal_class,billing_outcome,admitted_at \
             ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
            &[
                &"request-facts-migration-matrix",
                &"request-facts-migration-billing",
                &"request-facts-migration-account",
                &"request-facts-migration-key",
                &"opencode",
                &"explicit",
                &"anthropic",
                &"native",
                &"messages",
                &"auto",
                &"success",
                &"winner",
                &1_i64,
            ],
        )
        .unwrap();

    let bad_client = pg.client.execute(
        "INSERT INTO request_facts( \
             logical_request_id,account_id,key_id,client_kind,client_source, \
             provider_plane,route_class,request_class,admitted_at \
         ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        &[
            &"request-facts-migration-invalid-client",
            &"request-facts-migration-account",
            &"request-facts-migration-key",
            &"fabricated-client",
            &"unknown",
            &"anthropic",
            &"native",
            &"messages",
            &1_i64,
        ],
    );
    let bad_client = bad_client.expect_err("out-of-vocabulary client_kind must fail its CHECK");
    assert_eq!(
        bad_client.as_db_error().map(|error| error.code().code()),
        Some("23514")
    );

    // Migration replay is proved by the contiguous immutable migration registry above. Re-running
    // `migrate()` after inserting a deliberately non-UUID legacy fixture is not a valid replay
    // proof once later analytics views bind the base table: PostgreSQL validates the full relation
    // graph even though schema version 61 is already registered. Verify version and row preservation
    // without asking the migration runner to re-plan old DDL over a synthetic invalid identity.
    assert_eq!(pg.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    let row_count: i64 = pg
        .client
        .query_one(
            "SELECT COUNT(*)::bigint FROM request_facts WHERE logical_request_id=$1",
            &[&"request-facts-migration-matrix"],
        )
        .unwrap()
        .get(0);
    assert_eq!(row_count, 1, "exact migration replay must preserve the row");

    pg.client
        .execute(
            "DELETE FROM request_facts \
              WHERE logical_request_id IN ($1,$2) OR billing_request_id=$3",
            &[
                &"request-facts-migration-matrix",
                &"request-facts-migration-invalid-client",
                &"request-facts-migration-billing",
            ],
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

/// Real PostgreSQL proof for migration 0054's crash-safe terminal envelope and its correction of
/// unknown request evidence. Skipped unless an isolated destructive test database is supplied:
/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::request_fact_terminal_envelope_migration_postgres_matrix`
#[test]
fn request_fact_terminal_envelope_migration_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping request-fact terminal-envelope matrix: test URL is unset");
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

    let mut pg = PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    assert_eq!(pg.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    assert_eq!(CURRENT_SCHEMA_VERSION, 62);
    assert_eq!(
        ENGINE_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == 54)
            .map(|(_, sql)| *sql),
        Some(MIGRATION_0054),
    );

    let check_violation = |result: std::result::Result<u64, postgres::Error>, case: &str| {
        let error = result.unwrap_err();
        assert_eq!(
            error.as_db_error().map(|error| error.code().code()),
            Some("23514"),
            "{case} did not fail a CHECK: {error}",
        );
    };

    let fact_columns: Vec<(String, String, Option<String>)> = pg
        .client
        .query(
            "SELECT column_name,is_nullable,column_default \
               FROM information_schema.columns \
              WHERE table_schema='public' AND table_name='request_facts' \
                AND column_name = ANY($1) \
              ORDER BY column_name",
            &[&&[
                "delivery_state",
                "stream_flag",
                "tool_classes",
                "tool_results_in_input",
                "tool_calls_in_output",
                "structured_output_flag",
                "reasoning_flag",
                "input_modalities",
                "output_modalities",
            ][..]],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect();
    assert_eq!(fact_columns.len(), 9);
    for evidence in [
        "delivery_state",
        "tool_classes",
        "tool_results_in_input",
        "tool_calls_in_output",
        "structured_output_flag",
        "reasoning_flag",
        "input_modalities",
        "output_modalities",
    ] {
        let (_, nullable, default) = fact_columns
            .iter()
            .find(|(name, _, _)| name == evidence)
            .unwrap_or_else(|| panic!("request_facts lacks {evidence}: {fact_columns:?}"));
        assert_eq!(nullable, "YES", "{evidence} must preserve unknown as NULL");
        assert_eq!(default, &None, "{evidence} must not fabricate a default");
    }
    let (_, stream_nullable, stream_default) = fact_columns
        .iter()
        .find(|(name, _, _)| name == "stream_flag")
        .unwrap();
    assert_eq!(stream_nullable, "NO", "0054 must not alter stream_flag");
    assert_eq!(stream_default.as_deref(), Some("false"));

    let envelope_columns = [
        ("request_fact_terminal_schema_version", "integer"),
        ("request_fact_terminal_at", "bigint"),
        ("request_fact_http_status_code", "integer"),
        ("request_fact_provider_terminal_class", "text"),
        ("request_fact_delivery_state", "text"),
        ("request_fact_downstream_disconnect", "boolean"),
        ("request_fact_upstream_request_id", "text"),
        ("request_fact_first_public_byte_at", "bigint"),
        ("request_fact_internal_attempt_count", "integer"),
        ("request_fact_failure_class", "text"),
        ("request_fact_tool_calls_in_output", "boolean"),
    ];
    let outbox_columns: Vec<(String, String, String, Option<String>)> = pg
        .client
        .query(
            "SELECT column_name,data_type,is_nullable,column_default \
               FROM information_schema.columns \
              WHERE table_schema='public' AND table_name='settlement_outbox' \
                AND column_name LIKE 'request_fact_%' \
              ORDER BY ordinal_position",
            &[],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3)))
        .collect();
    assert_eq!(
        outbox_columns
            .iter()
            .map(|(name, data_type, _, _)| (name.as_str(), data_type.as_str()))
            .collect::<Vec<_>>(),
        envelope_columns,
        "0054 must add exactly the eleven typed terminal-envelope columns",
    );
    for (name, _, nullable, default) in &outbox_columns {
        assert_eq!(nullable, "YES", "old writers require nullable {name}");
        assert_eq!(default, &None, "old writers require no default for {name}");
    }

    let constraints: Vec<(String, bool, String)> = pg
        .client
        .query(
            "SELECT conname,convalidated,pg_get_constraintdef(oid) \
               FROM pg_constraint \
              WHERE conrelid IN ('request_facts'::regclass,'settlement_outbox'::regclass) \
                AND conname = ANY($1) \
              ORDER BY conname",
            &[&&[
                "request_facts_delivery_state_valid",
                "settlement_outbox_fact_schema_version_positive",
                "settlement_outbox_request_fact_http_status_code_range",
                "settlement_outbox_fact_provider_terminal_class_valid",
                "settlement_outbox_request_fact_delivery_state_valid",
                "settlement_outbox_fact_attempt_count_nonnegative",
                "settlement_outbox_request_fact_terminal_envelope_shape",
            ][..]],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect();
    assert_eq!(constraints.len(), 7, "missing 0054 CHECKs: {constraints:?}");
    assert!(
        constraints.iter().all(|(_, validated, _)| *validated),
        "every 0054 CHECK must finish validated: {constraints:?}",
    );
    for value in [
        "not_started",
        "started",
        "completed",
        "interrupted",
        "unknown",
    ] {
        assert!(
            constraints
                .iter()
                .filter(|(name, _, _)| name.contains("delivery_state"))
                .all(|(_, _, definition)| definition.contains(&format!("'{value}'::text"))),
            "both delivery-state constraints must admit {value}: {constraints:?}",
        );
    }
    let provider_check = constraints
        .iter()
        .find(|(name, _, _)| name.ends_with("provider_terminal_class_valid"))
        .unwrap();
    for value in [
        "success",
        "client_error",
        "quota",
        "auth",
        "timeout",
        "transport",
        "upstream_error",
        "protocol_error",
        "unknown",
    ] {
        assert!(provider_check.2.contains(&format!("'{value}'::text")));
    }

    let related_indexes: Vec<String> = pg
        .client
        .query(
            "SELECT indexname FROM pg_indexes \
              WHERE schemaname='public' \
                AND ( \
                    (tablename='settlement_outbox' AND indexdef LIKE '%request_fact_%') \
                    OR (tablename='request_facts' AND ( \
                        indexdef LIKE '%delivery_state%' \
                        OR indexdef LIKE '%terminal_at%' \
                        OR indexdef LIKE '%http_status_code%' \
                        OR indexdef LIKE '%provider_terminal_class%' \
                        OR indexdef LIKE '%billing_outcome%' \
                        OR indexdef LIKE '%downstream_disconnect%' \
                        OR indexdef LIKE '%upstream_request_id%' \
                        OR indexdef LIKE '%first_public_byte_at%' \
                        OR indexdef LIKE '%internal_attempt_count%' \
                        OR indexdef LIKE '%failure_class%' \
                        OR indexdef LIKE '%tool_calls_in_output%' \
                    )) \
                )",
            &[],
        )
        .unwrap()
        .into_iter()
        .map(|row| row.get(0))
        .collect();
    assert!(
        related_indexes.is_empty(),
        "0054 must add no request-fact-related index: {related_indexes:?}",
    );

    let prefix = "request-fact-terminal-envelope-matrix";
    pg.client
        .execute(
            "DELETE FROM request_facts WHERE logical_request_id LIKE $1",
            &[&format!("{prefix}%")],
        )
        .unwrap();
    pg.client
        .execute(
            "DELETE FROM settlement_outbox WHERE request_id LIKE $1",
            &[&format!("{prefix}%")],
        )
        .unwrap();
    pg.client
        .execute(
            "DELETE FROM reservations WHERE request_id LIKE $1",
            &[&format!("{prefix}%")],
        )
        .unwrap();
    pg.client
        .execute(
            "DELETE FROM api_keys WHERE key=$1",
            &[&format!("{prefix}-key")],
        )
        .unwrap();
    pg.client
        .execute(
            "DELETE FROM accounts WHERE id=$1",
            &[&format!("{prefix}-account")],
        )
        .unwrap();

    pg.client
        .execute(
            "INSERT INTO request_facts( \
                 logical_request_id,account_id,key_id,client_kind,client_source,provider_plane, \
                 route_class,request_class,admitted_at,delivery_state \
             ) VALUES($1,$2,$3,'unknown','unknown','anthropic','native','messages',1,$4)",
            &[
                &format!("{prefix}-fact-valid"),
                &format!("{prefix}-account"),
                &format!("{prefix}-key-id"),
                &"interrupted",
            ],
        )
        .unwrap();
    let unknown_row = pg
        .client
        .query_one(
            "INSERT INTO request_facts( \
                 logical_request_id,account_id,key_id,client_kind,client_source,provider_plane, \
                 route_class,request_class,admitted_at \
             ) VALUES($1,$2,$3,'unknown','unknown','anthropic','native','messages',1) \
             RETURNING tool_classes,tool_results_in_input,tool_calls_in_output, \
                       structured_output_flag,reasoning_flag,input_modalities,output_modalities",
            &[
                &format!("{prefix}-fact-unknown"),
                &format!("{prefix}-account"),
                &format!("{prefix}-key-id"),
            ],
        )
        .unwrap();
    let unknown_evidence: (
        Option<i32>,
        Option<bool>,
        Option<bool>,
        Option<bool>,
        Option<bool>,
        Option<i32>,
        Option<i32>,
    ) = (
        unknown_row.get(0),
        unknown_row.get(1),
        unknown_row.get(2),
        unknown_row.get(3),
        unknown_row.get(4),
        unknown_row.get(5),
        unknown_row.get(6),
    );
    assert_eq!(
        unknown_evidence,
        (None, None, None, None, None, None, None),
        "omitted dormant evidence must remain unknown",
    );
    let bad_fact_delivery = pg.client.execute(
        "INSERT INTO request_facts( \
             logical_request_id,account_id,key_id,client_kind,client_source,provider_plane, \
             route_class,request_class,admitted_at,delivery_state \
         ) VALUES($1,$2,$3,'unknown','unknown','anthropic','native','messages',1,'fabricated')",
        &[
            &format!("{prefix}-fact-bad-delivery"),
            &format!("{prefix}-account"),
            &format!("{prefix}-key-id"),
        ],
    );
    check_violation(
        bad_fact_delivery,
        "request_facts delivery vocabulary accepted fabricated",
    );

    pg.client
        .execute(
            "INSERT INTO accounts(id,balance_nano,spent_nano,mult_bp,status,created_ts,created) \
             VALUES($1,0,0,10000,'active',1,'')",
            &[&format!("{prefix}-account")],
        )
        .unwrap();
    pg.client
        .execute(
            "INSERT INTO api_keys(key,key_id,account_id,created_ts,created) \
             VALUES($1,$2,$3,1,'')",
            &[
                &format!("{prefix}-key"),
                &format!("{prefix}-key-id"),
                &format!("{prefix}-account"),
            ],
        )
        .unwrap();
    for suffix in [
        "old-writer",
        "valid",
        "bad-version",
        "bad-http-low",
        "bad-http-high",
        "bad-provider",
        "bad-delivery",
        "bad-attempts",
        "bad-shape",
    ] {
        pg.client
            .execute(
                "INSERT INTO reservations( \
                     request_id,account_id,key,hold_nano,balance_after_reserve_nano,owner_instance, \
                     owner_epoch,lease_until,state,created_ts,updated_ts \
                 ) VALUES($1,$2,$3,0,0,'terminal-envelope-matrix',1,100,'reserved',1,1)",
                &[
                    &format!("{prefix}-{suffix}"),
                    &format!("{prefix}-account"),
                    &format!("{prefix}-key"),
                ],
            )
            .unwrap();
    }

    // A pre-0054 writer omits every new column and must still insert a completely NULL envelope.
    pg.client
        .execute(
            "INSERT INTO settlement_outbox(request_id,actual_nano,disposition,created_ts,updated_ts) \
             VALUES($1,0,'cancel',1,1)",
            &[&format!("{prefix}-old-writer")],
        )
        .unwrap();
    let null_count: i64 = pg
        .client
        .query_one(
            "SELECT num_nulls( \
                 request_fact_terminal_schema_version,request_fact_terminal_at, \
                 request_fact_http_status_code,request_fact_provider_terminal_class, \
                 request_fact_delivery_state,request_fact_downstream_disconnect, \
                 request_fact_upstream_request_id,request_fact_first_public_byte_at, \
                 request_fact_internal_attempt_count,request_fact_failure_class, \
                 request_fact_tool_calls_in_output \
             )::bigint FROM settlement_outbox WHERE request_id=$1",
            &[&format!("{prefix}-old-writer")],
        )
        .unwrap()
        .get(0);
    assert_eq!(null_count, 11);

    pg.client
        .execute(
            "INSERT INTO settlement_outbox( \
                 request_id,actual_nano,disposition,created_ts,updated_ts, \
                 request_fact_terminal_schema_version,request_fact_terminal_at, \
                 request_fact_http_status_code,request_fact_provider_terminal_class, \
                 request_fact_delivery_state,request_fact_downstream_disconnect, \
                 request_fact_upstream_request_id,request_fact_first_public_byte_at, \
                 request_fact_internal_attempt_count,request_fact_failure_class, \
                 request_fact_tool_calls_in_output \
             ) VALUES($1,0,'cancel',1,1,1,2,499,'transport','interrupted',true,$2,3,0,$3,true)",
            &[
                &format!("{prefix}-valid"),
                &"bounded-upstream-id",
                &"downstream_closed",
            ],
        )
        .unwrap();

    let cases = [
        (
            "bad-version",
            "request_fact_terminal_schema_version,request_fact_terminal_at,request_fact_provider_terminal_class,request_fact_delivery_state",
            "0,2,'success','completed'",
        ),
        (
            "bad-http-low",
            "request_fact_terminal_schema_version,request_fact_terminal_at,request_fact_http_status_code,request_fact_provider_terminal_class,request_fact_delivery_state",
            "1,2,99,'client_error','not_started'",
        ),
        (
            "bad-http-high",
            "request_fact_terminal_schema_version,request_fact_terminal_at,request_fact_http_status_code,request_fact_provider_terminal_class,request_fact_delivery_state",
            "1,2,600,'client_error','not_started'",
        ),
        (
            "bad-provider",
            "request_fact_terminal_schema_version,request_fact_terminal_at,request_fact_provider_terminal_class,request_fact_delivery_state",
            "1,2,'fabricated','unknown'",
        ),
        (
            "bad-delivery",
            "request_fact_terminal_schema_version,request_fact_terminal_at,request_fact_provider_terminal_class,request_fact_delivery_state",
            "1,2,'success','fabricated'",
        ),
        (
            "bad-attempts",
            "request_fact_terminal_schema_version,request_fact_terminal_at,request_fact_provider_terminal_class,request_fact_delivery_state,request_fact_internal_attempt_count",
            "1,2,'success','completed',-1",
        ),
        (
            "bad-shape",
            "request_fact_http_status_code",
            "500",
        ),
    ];
    for (suffix, columns, values) in cases {
        let sql = format!(
            "INSERT INTO settlement_outbox( \
                 request_id,actual_nano,disposition,created_ts,updated_ts,{columns} \
             ) VALUES($1,0,'cancel',1,1,{values})",
        );
        let result = pg.client.execute(&sql, &[&format!("{prefix}-{suffix}")]);
        check_violation(
            result,
            &format!("invalid envelope case {suffix} was accepted"),
        );
    }

    pg.migrate().unwrap();
    assert_eq!(pg.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    let replayed_row = pg
        .client
        .query_one(
            "SELECT request_fact_terminal_schema_version,request_fact_terminal_at, \
                    request_fact_http_status_code,request_fact_provider_terminal_class, \
                    request_fact_delivery_state,request_fact_downstream_disconnect, \
                    request_fact_upstream_request_id,request_fact_first_public_byte_at, \
                    request_fact_internal_attempt_count,request_fact_failure_class, \
                    request_fact_tool_calls_in_output \
               FROM settlement_outbox WHERE request_id=$1",
            &[&format!("{prefix}-valid")],
        )
        .unwrap();
    let replayed: (
        i32,
        i64,
        i32,
        String,
        String,
        bool,
        String,
        i64,
        i32,
        String,
        bool,
    ) = (
        replayed_row.get(0),
        replayed_row.get(1),
        replayed_row.get(2),
        replayed_row.get(3),
        replayed_row.get(4),
        replayed_row.get(5),
        replayed_row.get(6),
        replayed_row.get(7),
        replayed_row.get(8),
        replayed_row.get(9),
        replayed_row.get(10),
    );
    assert_eq!(
        replayed,
        (
            1,
            2,
            499,
            "transport".to_owned(),
            "interrupted".to_owned(),
            true,
            "bounded-upstream-id".to_owned(),
            3,
            0,
            "downstream_closed".to_owned(),
            true,
        ),
        "migration replay must preserve the full durable terminal envelope",
    );

    pg.client
        .execute(
            "DELETE FROM request_facts WHERE logical_request_id LIKE $1",
            &[&format!("{prefix}%")],
        )
        .unwrap();
    pg.client
        .execute(
            "DELETE FROM settlement_outbox WHERE request_id LIKE $1",
            &[&format!("{prefix}%")],
        )
        .unwrap();
    pg.client
        .execute(
            "DELETE FROM reservations WHERE request_id LIKE $1",
            &[&format!("{prefix}%")],
        )
        .unwrap();
    pg.client
        .execute(
            "DELETE FROM api_keys WHERE key=$1",
            &[&format!("{prefix}-key")],
        )
        .unwrap();
    pg.client
        .execute(
            "DELETE FROM accounts WHERE id=$1",
            &[&format!("{prefix}-account")],
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

/// Real PostgreSQL proof that migration 0048 rejects an old settlement transaction before it can
/// cross the shared floor, and rejects terminal priced rows without immutable collection evidence.
/// Skipped unless an isolated destructive test database is supplied:
/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::settlement_floor_terminal_fence_postgres_matrix`
#[test]
fn settlement_floor_terminal_fence_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping settlement-floor terminal fence matrix: test URL is unset");
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

    let mut pg = PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    assert_eq!(pg.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);

    let mut tx = pg.client.transaction().unwrap();
    tx.batch_execute(
        "INSERT INTO accounts(id,balance_nano,spent_nano,mult_bp,status,created_ts,created)
           VALUES('terminal-fence-account',-900000000,0,10000,'active',1,''),
                 ('terminal-fence-debt-account',-1500000000,0,10000,'active',1,'');
         INSERT INTO api_keys(key,key_id,account_id,created_ts,created)
           VALUES('terminal-fence-key','terminal_fence_key_id','terminal-fence-account',1,'');
         INSERT INTO reservations(
             request_id,account_id,key,hold_nano,balance_after_reserve_nano,
             owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts
         ) VALUES(
             'terminal-fence-legacy','terminal-fence-account','terminal-fence-key',0,-900000000,
             'terminal-fence-owner',1,100,'settled',1,1
         ),(
             'terminal-fence-priced','terminal-fence-account','terminal-fence-key',0,-900000000,
             'terminal-fence-owner',1,100,'reserved',1,1
         );
         UPDATE reservations
            SET provider='anthropic',payable_multiplier_bp=10000
          WHERE request_id='terminal-fence-priced';",
    )
    .unwrap();

    tx.batch_execute("SAVEPOINT floor_cross").unwrap();
    let floor_error = tx
        .execute(
            "UPDATE accounts
                SET balance_nano=-1000000001,spent_nano=spent_nano+100000001
              WHERE id='terminal-fence-account'",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        floor_error
            .as_db_error()
            .and_then(|error| error.constraint()),
        Some("accounts_settlement_floor_fence")
    );
    assert_eq!(
        floor_error.as_db_error().map(|error| error.code().code()),
        Some("40001")
    );
    assert_eq!(
        classify_failure(&anyhow::Error::new(floor_error)),
        FailureClass::Transient,
        "the deployed outbox actor must leave a fenced row pending"
    );
    tx.batch_execute("ROLLBACK TO SAVEPOINT floor_cross")
        .unwrap();
    let unchanged = tx
        .query_one(
            "SELECT balance_nano,spent_nano FROM accounts WHERE id='terminal-fence-account'",
            &[],
        )
        .unwrap();
    assert_eq!(unchanged.get::<_, i64>(0), -900_000_000);
    assert_eq!(unchanged.get::<_, i64>(1), 0);

    // Existing adjustment debt remains legal but settlement may not make it any deeper.
    assert_eq!(
        tx.execute(
            "UPDATE accounts SET spent_nano=spent_nano+1
              WHERE id='terminal-fence-debt-account'",
            &[],
        )
        .unwrap(),
        1
    );
    tx.batch_execute("SAVEPOINT debt_cross").unwrap();
    let debt_error = tx
        .execute(
            "UPDATE accounts SET balance_nano=balance_nano-1,spent_nano=spent_nano+1
              WHERE id='terminal-fence-debt-account'",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        debt_error
            .as_db_error()
            .and_then(|error| error.constraint()),
        Some("accounts_settlement_floor_fence")
    );
    tx.batch_execute("ROLLBACK TO SAVEPOINT debt_cross")
        .unwrap();

    tx.batch_execute("SAVEPOINT terminal_without_evidence")
        .unwrap();
    let evidence_error = tx
        .execute(
            "UPDATE reservations
                SET state='settled',actual_nano=10,settled_ts=2,updated_ts=2
              WHERE request_id='terminal-fence-priced'",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        evidence_error
            .as_db_error()
            .and_then(|error| error.constraint()),
        Some("reservations_priced_terminal_collection_evidence")
    );
    assert_eq!(
        evidence_error
            .as_db_error()
            .map(|error| error.code().code()),
        Some("40001")
    );
    assert_eq!(
        classify_failure(&anyhow::Error::new(evidence_error)),
        FailureClass::Transient,
        "the deployed outbox actor must leave a fenced row pending"
    );
    tx.batch_execute("ROLLBACK TO SAVEPOINT terminal_without_evidence")
        .unwrap();
    assert_eq!(
        tx.query_one(
            "SELECT state FROM reservations WHERE request_id='terminal-fence-priced'",
            &[],
        )
        .unwrap()
        .get::<_, String>(0),
        "reserved"
    );

    tx.batch_execute("SAVEPOINT incomplete_pricing_pair")
        .unwrap();
    let pricing_pair_error = tx
        .execute(
            "UPDATE reservations SET provider=NULL
              WHERE request_id='terminal-fence-priced'",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        pricing_pair_error
            .as_db_error()
            .and_then(|error| error.constraint()),
        Some("reservations_scalar_pricing_pair")
    );
    tx.batch_execute("ROLLBACK TO SAVEPOINT incomplete_pricing_pair")
        .unwrap();

    // The legacy both-null terminal shape stays readable, while the new priced shape closes only
    // with evidence whose sum is already protected by migration 0047.
    let legacy = tx
        .query_one(
            "SELECT state,provider,payable_multiplier_bp,collected_nano,uncollected_nano
               FROM reservations WHERE request_id='terminal-fence-legacy'",
            &[],
        )
        .unwrap();
    assert_eq!(legacy.get::<_, String>(0), "settled");
    assert_eq!(legacy.get::<_, Option<String>>(1), None);
    assert_eq!(legacy.get::<_, Option<i64>>(2), None);
    assert_eq!(legacy.get::<_, Option<i64>>(3), None);
    assert_eq!(legacy.get::<_, Option<i64>>(4), None);
    assert_eq!(
        tx.execute(
            "UPDATE reservations
                SET state='settled',actual_nano=10,collected_nano=7,uncollected_nano=3,
                    settled_ts=2,updated_ts=2
              WHERE request_id='terminal-fence-priced'",
            &[],
        )
        .unwrap(),
        1
    );

    tx.rollback().unwrap();
    lock_holder
        .client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
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
    pg.pool_member_set_disabled(
        crate::PROVIDER_GOOGLE,
        "gemini_oauth_000002",
        false,
        false,
        "",
        "",
    )
    .unwrap();
    pg.pool_member_set_disabled(
        crate::PROVIDER_GOOGLE,
        "gemini_oauth_000002",
        false,
        false,
        "",
        "",
    )
    .unwrap();
    assert!(pg
        .pool_member_disabled(crate::PROVIDER_GOOGLE)
        .unwrap()
        .is_empty());

    // Claude can never be addressed through this store.
    assert!(pg
        .pool_member_set_disabled(
            crate::PROVIDER_ANTHROPIC,
            "someone@example.com",
            true,
            false,
            "",
            ""
        )
        .is_err());
    assert!(pg.pool_member_disabled(crate::PROVIDER_ANTHROPIC).is_err());
    assert!(pg
        .pool_member_set_disabled(crate::PROVIDER_GOOGLE, "", true, false, "", "")
        .is_err());

    // Hiding is a presentation choice layered on top of a disable, never a way to take a serving
    // profile out of the operator's view while it keeps receiving traffic.
    assert!(pg
        .pool_member_set_disabled(
            crate::PROVIDER_GOOGLE,
            "gemini_oauth_000003",
            false,
            true,
            "",
            ""
        )
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
    pg.pool_member_set_disabled(
        crate::PROVIDER_GOOGLE,
        "gemini_oauth_000003",
        false,
        false,
        "",
        "",
    )
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

/// Real PostgreSQL proof for immutable turn replay, cumulative dual-ledger spend, balance
/// observation history and estimator-state CAS on the windowless Tripo3D track. Skipped unless
/// the dedicated destructive test database is supplied:
/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::tripo3d_calibration_postgres_matrix`
#[test]
fn tripo3d_calibration_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping Tripo3D PostgreSQL calibration matrix: test URL is unset");
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
            "TRUNCATE tripo3d_calibration_state,tripo3d_balance_observations,\
             tripo3d_calibration_subject_spend,tripo3d_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();

    let event = Tripo3dTurnCalibrationEvent {
        request_id: "tripo3d-pg-replay".into(),
        subject_id: "tripo3d-pg-subject".into(),
        cohort: "tripo3d-api-50".into(),
        task_type: "image_to_model".into(),
        requested_model_version: Some("v2.5-20250123".into()),
        resolved_model_version: Some("v2.5-20250123".into()),
        tariff_schedule_id: "tripo3d/openapi-billing/2026-08-12".into(),
        priced_ts: 190,
        completed_at: 200,
        upstream_task_id: "task_pg_1".into(),
        native_total_millicredits: 20_000,
        api_total_nanousd: 200_000_000,
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
            pg.record_tripo3d_turn(&event).unwrap()
        }));
    }
    let mut insert_outcomes = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    insert_outcomes.sort_unstable();
    assert_eq!(insert_outcomes, vec![false, true]);
    assert_eq!(
        pg.tripo3d_subject_spend(&event.subject_id).unwrap(),
        Tripo3dSubjectSpend {
            spent_api_nanousd: 200_000_000,
            spent_native_millicredits: 20_000,
        }
    );

    let mut conflict = event.clone();
    conflict.task_type = "text_to_model".into();
    let error = pg.record_tripo3d_turn(&conflict).unwrap_err().to_string();
    assert!(
        error.contains("replay conflict"),
        "unexpected error: {error}"
    );
    assert_eq!(
        pg.tripo3d_subject_spend(&event.subject_id).unwrap(),
        Tripo3dSubjectSpend {
            spent_api_nanousd: 200_000_000,
            spent_native_millicredits: 20_000,
        }
    );

    // A documented free task settles at the legal zero pair; out-of-order finalizers still
    // retain the earliest tracking start and latest update.
    let free = Tripo3dTurnCalibrationEvent {
        request_id: "tripo3d-pg-free".into(),
        task_type: "animate_prerigcheck".into(),
        requested_model_version: Some("v2.0-20250506".into()),
        resolved_model_version: Some("v2.0-20250506".into()),
        priced_ts: 90,
        completed_at: 100,
        upstream_task_id: "task_pg_0".into(),
        native_total_millicredits: 0,
        api_total_nanousd: 0,
        ..event.clone()
    };
    assert!(pg.record_tripo3d_turn(&free).unwrap());
    assert_eq!(
        pg.tripo3d_subject_spend(&event.subject_id).unwrap(),
        Tripo3dSubjectSpend {
            spent_api_nanousd: 200_000_000,
            spent_native_millicredits: 20_000,
        }
    );
    let spend_times = pg
        .client
        .query_one(
            "SELECT tracking_started_ts,updated_ts FROM tripo3d_calibration_subject_spend \
             WHERE subject_id=$1",
            &[&event.subject_id],
        )
        .unwrap();
    assert_eq!(
        (spend_times.get::<_, i64>(0), spend_times.get::<_, i64>(1)),
        (100, 200)
    );

    let observation = Tripo3dBalanceObservation {
        subject_id: event.subject_id.clone(),
        cohort: event.cohort.clone(),
        observed_at: 300,
        balance_raw: "4980.0".into(),
        frozen_raw: "0.0".into(),
        // Unproven units: the parsed halves stay NULL, never 0.
        balance_micro_units: None,
        frozen_micro_units: None,
        cumulative_api_nanousd: 200_000_000,
        cumulative_native_millicredits: 20_000,
        observation_source: "poll".into(),
        source_request_id: None,
    };
    let state = Tripo3dCalibrationRow {
        subject_id: observation.subject_id.clone(),
        cohort: observation.cohort.clone(),
        anchor_balance_micro_units: None,
        anchor_frozen_micro_units: None,
        anchor_spend_api_nanousd: observation.cumulative_api_nanousd,
        anchor_spend_native_millicredits: observation.cumulative_native_millicredits,
        latest_balance_raw: observation.balance_raw.clone(),
        latest_frozen_raw: observation.frozen_raw.clone(),
        latest_balance_micro_units: None,
        latest_frozen_micro_units: None,
        observed_at: observation.observed_at,
        observed_spend_api_nanousd: 0,
        observed_spend_native_millicredits: 0,
        samples: 0,
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
        pg.save_tripo3d_calibration(&state, &observation).unwrap(),
        Some(1)
    );

    let mut second_observation = observation.clone();
    second_observation.observed_at = 301;
    second_observation.balance_raw = "4960.0".into();
    let mut second_state = pg
        .load_tripo3d_calibration(&state.subject_id, &state.cohort)
        .unwrap()
        .unwrap();
    second_state.observed_at = second_observation.observed_at;
    second_state.latest_balance_raw = second_observation.balance_raw.clone();
    second_state.updated_ts = second_observation.observed_at;
    assert_eq!(
        pg.save_tripo3d_calibration(&second_state, &second_observation)
            .unwrap(),
        Some(2)
    );

    // A stale writer loses the CAS and rolls its observation back. Raw history remains exact,
    // oldest-first and contains only the two winning transitions.
    assert_eq!(
        pg.save_tripo3d_calibration(&state, &observation).unwrap(),
        None
    );
    let history = pg
        .load_tripo3d_balance_observations(&state.subject_id, &state.cohort)
        .unwrap();
    assert_eq!(
        history
            .iter()
            .map(|row| row.observed_at)
            .collect::<Vec<_>>(),
        vec![300, 301]
    );
    let stored = pg
        .load_tripo3d_calibration(&state.subject_id, &state.cohort)
        .unwrap()
        .unwrap();
    assert_eq!(stored.version, 2);
    assert_eq!(stored.latest_balance_raw, "4960.0");
    assert_eq!(stored.native_remaining_micro_units(), None);

    pg.client
        .batch_execute(
            "TRUNCATE tripo3d_calibration_state,tripo3d_balance_observations,\
             tripo3d_calibration_subject_spend,tripo3d_turn_calibration_events \
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

/// Real PostgreSQL proof for immutable turn replay, cumulative dual-ledger spend, quota
/// observation history and estimator-state CAS on the monthly-window Suno track. Skipped
/// unless the dedicated destructive test database is supplied:
/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::suno_calibration_postgres_matrix`
#[test]
fn suno_calibration_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping Suno PostgreSQL calibration matrix: test URL is unset");
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
            "TRUNCATE suno_window_calibrations,suno_window_observations,\
             suno_calibration_subject_spend,suno_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();

    let event = SunoTurnCalibrationEvent {
        request_id: "suno-pg-replay".into(),
        subject_id: "suno-pg-subject".into(),
        plan: "Pro".into(),
        requested_model: "v5.5".into(),
        served_model: None,
        tariff_schedule_id: "suno/derived-subscription/2026-08-12".into(),
        priced_ts: 190,
        completed_at: 200,
        upstream_clip_id: "clip_pg_1".into(),
        native_total_millicredits: 5_000,
        api_total_nanousd: 20_000_000,
        native_schedule_derived: true,
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
            pg.record_suno_turn(&event).unwrap()
        }));
    }
    let mut insert_outcomes = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    insert_outcomes.sort_unstable();
    assert_eq!(insert_outcomes, vec![false, true]);
    assert_eq!(
        pg.suno_subject_spend(&event.subject_id).unwrap(),
        SunoSubjectSpend {
            spent_api_nanousd: 20_000_000,
            spent_native_millicredits: 5_000,
        }
    );

    // Re-grading the same turn from schedule-derived to provider-reported under the same
    // request id is a semantic conflict, never a silent update.
    let mut conflict = event.clone();
    conflict.native_schedule_derived = false;
    let error = pg.record_suno_turn(&conflict).unwrap_err().to_string();
    assert!(
        error.contains("replay conflict"),
        "unexpected error: {error}"
    );
    assert_eq!(
        pg.suno_subject_spend(&event.subject_id).unwrap(),
        SunoSubjectSpend {
            spent_api_nanousd: 20_000_000,
            spent_native_millicredits: 5_000,
        }
    );

    // A finalized-but-failed generation with zero credit movement settles at the legal zero
    // pair; out-of-order finalizers still retain the earliest tracking start and latest
    // update.
    let refunded = SunoTurnCalibrationEvent {
        request_id: "suno-pg-refunded".into(),
        priced_ts: 90,
        completed_at: 100,
        upstream_clip_id: "clip_pg_0".into(),
        native_total_millicredits: 0,
        api_total_nanousd: 0,
        ..event.clone()
    };
    assert!(pg.record_suno_turn(&refunded).unwrap());
    assert_eq!(
        pg.suno_subject_spend(&event.subject_id).unwrap(),
        SunoSubjectSpend {
            spent_api_nanousd: 20_000_000,
            spent_native_millicredits: 5_000,
        }
    );
    let spend_times = pg
        .client
        .query_one(
            "SELECT tracking_started_ts,updated_ts FROM suno_calibration_subject_spend \
             WHERE subject_id=$1",
            &[&event.subject_id],
        )
        .unwrap();
    assert_eq!(
        (spend_times.get::<_, i64>(0), spend_times.get::<_, i64>(1)),
        (100, 200)
    );

    let observation = SunoWindowObservation {
        subject_id: event.subject_id.clone(),
        plan: event.plan.clone(),
        window_duration_secs: 2_592_000,
        reset_at: None,
        observed_at: 300,
        // Unproven field semantics: the raw counters stay NULL, never 0.
        native_limit_units: None,
        native_used_units: None,
        native_remaining_units: None,
        period_raw: Some("monthly".into()),
        used_fraction_units: None,
        measurement_resolution_fraction_units: None,
        cumulative_api_nanousd: 20_000_000,
        cumulative_native_millicredits: 5_000,
        observation_source: "poll".into(),
        source_request_id: None,
    };
    let state = SunoCalibrationRow {
        subject_id: observation.subject_id.clone(),
        plan: observation.plan.clone(),
        window_duration_secs: observation.window_duration_secs,
        reset_at: None,
        anchor_used_fraction_units: None,
        anchor_resolution_fraction_units: None,
        anchor_spend_api_nanousd: observation.cumulative_api_nanousd,
        anchor_spend_native_millicredits: observation.cumulative_native_millicredits,
        used_fraction_units: None,
        measurement_resolution_fraction_units: None,
        observed_at: observation.observed_at,
        native_limit_millicredits: None,
        native_used_millicredits: None,
        observed_fraction_units: 0,
        observed_spend_api_nanousd: 0,
        observed_spend_native_millicredits: 0,
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
        pg.save_suno_calibration(&state, &observation).unwrap(),
        Some(1)
    );

    let mut second_observation = observation.clone();
    second_observation.observed_at = 301;
    let mut second_state = pg
        .load_suno_calibration(
            &state.subject_id,
            &state.plan,
            state.window_duration_secs,
        )
        .unwrap()
        .unwrap();
    second_state.observed_at = second_observation.observed_at;
    second_state.updated_ts = second_observation.observed_at;
    assert_eq!(
        pg.save_suno_calibration(&second_state, &second_observation)
            .unwrap(),
        Some(2)
    );

    // A stale writer loses the CAS and rolls its observation back. Raw history remains exact,
    // oldest-first and contains only the two winning transitions.
    assert_eq!(
        pg.save_suno_calibration(&state, &observation).unwrap(),
        None
    );
    let history = pg
        .load_suno_window_observations(&state.subject_id, &state.plan, state.window_duration_secs)
        .unwrap();
    assert_eq!(
        history
            .iter()
            .map(|row| row.observed_at)
            .collect::<Vec<_>>(),
        vec![300, 301]
    );
    let stored = pg
        .load_suno_calibration(
            &state.subject_id,
            &state.plan,
            state.window_duration_secs,
        )
        .unwrap()
        .unwrap();
    assert_eq!(stored.version, 2);
    assert_eq!(stored.native_remaining_units(), None);

    pg.client
        .batch_execute(
            "TRUNCATE suno_window_calibrations,suno_window_observations,\
             suno_calibration_subject_spend,suno_turn_calibration_events \
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

fn request_fact_admission(
    logical: &str,
    billing: &str,
    group: Option<&str>,
    attempt: i32,
    account_id: &str,
    key_id: &str,
    admitted_at: i64,
) -> crate::request_facts::RequestFactAdmission {
    crate::request_facts::RequestFactAdmission {
        logical_request_id: logical.into(),
        billing_request_id: billing.into(),
        execution_group_id: group.map(str::to_owned),
        attempt,
        account_id: account_id.into(),
        key_id: key_id.into(),
        client_kind: crate::request_facts::ClientKind::OpenCode,
        client_source: crate::request_facts::ClientSource::Explicit,
        client_version: Some("1.0".into()),
        provider_plane: "anthropic".into(),
        route_class: "direct".into(),
        request_class: "messages".into(),
        requested_model: Some("claude-test".into()),
        executable_model: Some("claude-test".into()),
        stream_flag: true,
        tools_declared_count: Some(1),
        tool_classes: Some(crate::request_facts::TOOL_CLASS_CUSTOM_FUNCTION),
        tool_choice_mode: Some(crate::request_facts::ToolChoiceMode::Auto),
        parallel_tools_requested: Some(false),
        tool_results_in_input: Some(false),
        structured_output_flag: None,
        reasoning_flag: Some(true),
        service_tier: Some("standard".into()),
        input_modalities: Some(crate::request_facts::MODALITY_TEXT),
        output_modalities: Some(crate::request_facts::MODALITY_TEXT),
        admitted_at,
    }
}

fn request_fact_terminal(
    terminal_at: i64,
    delivery_state: crate::request_facts::DeliveryState,
) -> crate::request_facts::RequestFactTerminalEvidence {
    crate::request_facts::RequestFactTerminalEvidence {
        terminal_at,
        http_status_code: Some(200),
        provider_terminal_class: crate::request_facts::ProviderTerminalClass::Success,
        delivery_state,
        downstream_disconnect: Some(false),
        upstream_request_id: Some("upstream-safe-id".into()),
        first_public_byte_at: Some(terminal_at),
        internal_attempt_count: Some(1),
        failure_class: None,
        tool_calls_in_output: Some(false),
    }
}

fn lock_request_fact_matrix(url: &str) -> (PgStore, PgStore) {
    let mut lock_holder = PgStore::connect(url).unwrap();
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
    let mut pg = PgStore::connect(url).unwrap();
    pg.migrate().unwrap();
    pg.client
        .batch_execute(
            "TRUNCATE request_facts,execution_group_winner,settlement_outbox,reservations, \
             capacity_leases,leader_leases,engine_instances,usage_events,ledger,api_keys,accounts \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    (lock_holder, pg)
}

fn unlock_request_fact_matrix(lock_holder: &mut PgStore) {
    lock_holder
        .client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}

/// Dormant PostgreSQL lifecycle proof: admission exact replay, same-transaction delivery,
/// crash-safe terminalization, and old-caller compatibility.
#[test]
fn request_fact_lifecycle_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping request-fact lifecycle matrix: test URL is unset");
        return;
    };
    let (mut lock_holder, mut pg) = lock_request_fact_matrix(&url);
    pg.account_create("rf-account", None, 10_000).unwrap();
    pg.account_topup("rf-account", 10_000, Some("rf-seed"))
        .unwrap();
    pg.key_issue("rf-key", "rf-account", None).unwrap();
    let key_id = pg.key_get("rf-key").unwrap().unwrap().key_id;
    let owner = pg.claim_instance("rf-owner", 600).unwrap();
    let admitted_at = now();
    const LOGICAL: &str = "11111111-1111-4111-8111-111111111111";
    const BILLING: &str = "22222222-2222-4222-8222-222222222222";
    const CONFLICT_BILLING: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
    let orphan_fact = request_fact_admission(
        "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        CONFLICT_BILLING,
        None,
        1,
        "rf-account",
        &key_id,
        admitted_at,
    );
    {
        let mut tx = pg.client.transaction().unwrap();
        super::request_facts::insert_or_validate_admission(&mut tx, &orphan_fact).unwrap();
        tx.commit().unwrap();
    }
    let mut conflicting_orphan = orphan_fact;
    conflicting_orphan.route_class = "conflict".into();
    assert!(pg
        .reserve_request_for_execution_with_fact(
            &owner,
            CONFLICT_BILLING,
            "rf-account",
            "rf-key",
            100,
            600,
            &crate::ExecutionAttempt::direct(),
            &conflicting_orphan,
        )
        .is_err());
    let rollback_row = pg
        .client
        .query_one(
            "SELECT account.balance_nano,account.reserved_nano,key.reserved_nano,                     EXISTS(SELECT 1 FROM reservations WHERE request_id=$1)                FROM accounts account JOIN api_keys key ON key.account_id=account.id               WHERE account.id='rf-account' AND key.key='rf-key'",
            &[&CONFLICT_BILLING],
        )
        .unwrap();
    assert_eq!(rollback_row.get::<_, i64>(0), 10_000);
    assert_eq!(rollback_row.get::<_, i64>(1), 0);
    assert_eq!(rollback_row.get::<_, i64>(2), 0);
    assert!(!rollback_row.get::<_, bool>(3));

    let fact = request_fact_admission(
        LOGICAL,
        BILLING,
        None,
        1,
        "rf-account",
        &key_id,
        admitted_at,
    );
    assert!(pg
        .reserve_request_for_execution_with_fact(
            &owner,
            BILLING,
            "rf-account",
            "rf-key",
            100,
            600,
            &crate::ExecutionAttempt::direct(),
            &fact,
        )
        .unwrap()
        .is_some());
    assert!(pg
        .reserve_request_for_execution_with_fact(
            &owner,
            BILLING,
            "rf-account",
            "rf-key",
            100,
            600,
            &crate::ExecutionAttempt::direct(),
            &fact,
        )
        .unwrap()
        .is_some());
    let mut conflict = fact.clone();
    conflict.route_class = "conflict".into();
    assert!(pg
        .reserve_request_for_execution_with_fact(
            &owner,
            BILLING,
            "rf-account",
            "rf-key",
            100,
            600,
            &crate::ExecutionAttempt::direct(),
            &conflict,
        )
        .is_err());
    let aggregate_row = pg
        .client
        .query_one(
            "SELECT account.balance_nano,account.reserved_nano,key.reserved_nano \
               FROM accounts account JOIN api_keys key ON key.account_id=account.id \
              WHERE account.id='rf-account' AND key.key='rf-key'",
            &[],
        )
        .unwrap();
    let aggregates: (i64, i64, i64) = (
        aggregate_row.get(0),
        aggregate_row.get(1),
        aggregate_row.get(2),
    );
    assert_eq!(aggregates, (9_900, 100, 100));

    // Legacy reservation remains valid and deliberately has no analytics fact.
    assert!(pg
        .reserve_request(&owner, "legacy-rf", "rf-account", "rf-key", 10, 600)
        .unwrap()
        .is_some());
    let legacy_fact_count: i64 = pg
        .client
        .query_one(
            "SELECT COUNT(*)::bigint FROM request_facts WHERE billing_request_id='legacy-rf'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(legacy_fact_count, 0);

    pg.client
        .execute(
            "UPDATE request_facts SET admitted_at=$2 WHERE billing_request_id=$1",
            &[&BILLING, &(now() + 100)],
        )
        .unwrap();
    assert!(pg
        .mark_delivering_with_request_fact(&owner, BILLING, 600)
        .is_err());
    let rolled_back_delivery: (String, Option<i64>) = {
        let row = pg
            .client
            .query_one(
                "SELECT reservation.state,fact.delivery_started_at                    FROM reservations reservation JOIN request_facts fact                      ON fact.billing_request_id=reservation.request_id                   WHERE reservation.request_id=$1",
                &[&BILLING],
            )
            .unwrap();
        (row.get(0), row.get(1))
    };
    assert_eq!(rolled_back_delivery, ("reserved".into(), None));
    pg.client
        .execute(
            "UPDATE request_facts SET admitted_at=$2 WHERE billing_request_id=$1",
            &[&BILLING, &admitted_at],
        )
        .unwrap();

    assert!(pg
        .mark_delivering_with_request_fact(&owner, BILLING, 600)
        .unwrap());
    let first_delivery: Option<i64> = pg
        .client
        .query_one(
            "SELECT delivery_started_at FROM request_facts WHERE billing_request_id=$1",
            &[&BILLING],
        )
        .unwrap()
        .get(0);
    assert!(first_delivery.is_some());
    assert!(pg
        .mark_delivering_with_request_fact(&owner, BILLING, 600)
        .unwrap());
    let second_delivery: Option<i64> = pg
        .client
        .query_one(
            "SELECT delivery_started_at FROM request_facts WHERE billing_request_id=$1",
            &[&BILLING],
        )
        .unwrap()
        .get(0);
    assert_eq!(first_delivery, second_delivery);

    let terminal_at = now().max(admitted_at);
    let terminal =
        request_fact_terminal(terminal_at, crate::request_facts::DeliveryState::Completed);
    pg.enqueue_settlement_with_request_fact(BILLING, 50, None, None, &terminal)
        .unwrap();
    let pending_row = pg
        .client
        .query_one(
            "SELECT outbox.state,fact.terminal_at,outbox.request_fact_terminal_schema_version \
               FROM settlement_outbox outbox JOIN request_facts fact \
                 ON fact.billing_request_id=outbox.request_id WHERE outbox.request_id=$1",
            &[&BILLING],
        )
        .unwrap();
    let pending: (String, Option<i64>, Option<i32>) =
        (pending_row.get(0), pending_row.get(1), pending_row.get(2));
    assert_eq!(pending, ("pending".into(), None, Some(1)));
    drop(pg);

    // A new connection has no in-memory evidence. Drain recovers entirely from the durable outbox.
    let mut recovered = PgStore::connect(&url).unwrap();
    assert_eq!(recovered.drain_outbox(100).unwrap(), 1);
    let terminal_db_row = recovered
        .client
        .query_one(
            "SELECT terminal_at,provider_terminal_class,delivery_state,billing_outcome, \
                    tool_calls_in_output FROM request_facts WHERE billing_request_id=$1",
            &[&BILLING],
        )
        .unwrap();
    let terminal_row: (i64, String, String, String, Option<bool>) = (
        terminal_db_row.get(0),
        terminal_db_row.get(1),
        terminal_db_row.get(2),
        terminal_db_row.get(3),
        terminal_db_row.get(4),
    );
    assert_eq!(
        terminal_row,
        (
            terminal_at,
            "success".into(),
            "completed".into(),
            "winner".into(),
            Some(false),
        )
    );
    assert_eq!(recovered.drain_outbox(100).unwrap(), 0);
    assert_eq!(
        recovered
            .settle_request_with_request_fact(BILLING, 50, None, None, &terminal)
            .unwrap(),
        Some(9_940),
    );
    let mut conflicting_terminal = terminal;
    conflicting_terminal.http_status_code = Some(201);
    assert!(recovered
        .settle_request_with_request_fact(BILLING, 50, None, None, &conflicting_terminal)
        .is_err());
    let legacy_terminal =
        request_fact_terminal(now(), crate::request_facts::DeliveryState::NotStarted);
    recovered
        .cancel_request_with_request_fact("legacy-rf", &legacy_terminal)
        .unwrap();
    let legacy_fact_count: i64 = recovered
        .client
        .query_one(
            "SELECT COUNT(*)::bigint FROM request_facts WHERE billing_request_id='legacy-rf'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(legacy_fact_count, 0);
    unlock_request_fact_matrix(&mut lock_holder);
}

/// Outcome, reconciliation, terminal batch, and prune proof for the dormant write-only surface.
#[test]
fn request_fact_outcomes_batch_and_prune_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping request-fact outcome/batch/prune matrix: test URL is unset");
        return;
    };
    let (mut lock_holder, mut pg) = lock_request_fact_matrix(&url);
    pg.account_create("rf2-account", None, 10_000).unwrap();
    pg.account_topup("rf2-account", 100_000, Some("rf2-seed"))
        .unwrap();
    pg.key_issue("rf2-key", "rf2-account", None).unwrap();
    let key_id = pg.key_get("rf2-key").unwrap().unwrap().key_id;
    let owner = pg.claim_instance("rf2-owner", 600).unwrap();
    let admitted_at = now();
    let terminal =
        request_fact_terminal(admitted_at, crate::request_facts::DeliveryState::NotStarted);
    let cases = [
        (
            "33333333-3333-4333-8333-333333333331",
            "33333333-3333-4333-8333-333333333332",
            0_i64,
            "zero_metered",
        ),
        (
            "33333333-3333-4333-8333-333333333333",
            "33333333-3333-4333-8333-333333333334",
            0_i64,
            "canceled",
        ),
    ];
    for (logical, billing, actual, outcome) in cases {
        let fact = request_fact_admission(
            logical,
            billing,
            None,
            1,
            "rf2-account",
            &key_id,
            admitted_at,
        );
        pg.reserve_request_for_execution_with_fact(
            &owner,
            billing,
            "rf2-account",
            "rf2-key",
            10,
            600,
            &crate::ExecutionAttempt::direct(),
            &fact,
        )
        .unwrap();
        if outcome == "canceled" {
            pg.cancel_request_with_request_fact(billing, &terminal)
                .unwrap();
        } else {
            pg.settle_request_with_request_fact(billing, actual, None, None, &terminal)
                .unwrap();
        }
        let stored: String = pg
            .client
            .query_one(
                "SELECT billing_outcome FROM request_facts WHERE billing_request_id=$1",
                &[&billing],
            )
            .unwrap()
            .get(0);
        assert_eq!(stored, outcome);
    }

    const GROUP: &str = "44444444-4444-4444-8444-444444444444";
    let first_id = "55555555-5555-4555-8555-555555555551";
    let second_id = "55555555-5555-4555-8555-555555555552";
    for (attempt, billing) in [(1, first_id), (2, second_id)] {
        let execution = crate::ExecutionAttempt::grouped(GROUP, attempt).unwrap();
        let fact = request_fact_admission(
            "66666666-6666-4666-8666-666666666666",
            billing,
            Some(GROUP),
            attempt,
            "rf2-account",
            &key_id,
            admitted_at,
        );
        pg.reserve_request_for_execution_with_fact(
            &owner,
            billing,
            "rf2-account",
            "rf2-key",
            10,
            600,
            &execution,
            &fact,
        )
        .unwrap();
    }
    pg.settle_request_with_request_fact(first_id, 5, None, None, &terminal)
        .unwrap();
    pg.settle_request_with_request_fact(second_id, 5, None, None, &terminal)
        .unwrap();
    let outcomes: Vec<String> = pg
        .client
        .query(
            "SELECT billing_outcome FROM request_facts WHERE billing_request_id IN ($1,$2) \
             ORDER BY billing_request_id",
            &[&first_id, &second_id],
        )
        .unwrap()
        .into_iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(outcomes, vec!["winner", "loser"]);

    // Reconciler synthesizes honest unknown evidence only when the optional fact exists.
    let reserved_id = "77777777-7777-4777-8777-777777777771";
    let delivering_id = "77777777-7777-4777-8777-777777777772";
    for (logical, billing) in [
        ("88888888-8888-4888-8888-888888888881", reserved_id),
        ("88888888-8888-4888-8888-888888888882", delivering_id),
    ] {
        let fact = request_fact_admission(
            logical,
            billing,
            None,
            1,
            "rf2-account",
            &key_id,
            admitted_at,
        );
        pg.reserve_request_for_execution_with_fact(
            &owner,
            billing,
            "rf2-account",
            "rf2-key",
            10,
            1,
            &crate::ExecutionAttempt::direct(),
            &fact,
        )
        .unwrap();
    }
    pg.mark_delivering_with_request_fact(&owner, delivering_id, 1)
        .unwrap();
    pg.client
        .execute(
            "UPDATE reservations SET lease_until=0 WHERE request_id IN ($1,$2)",
            &[&reserved_id, &delivering_id],
        )
        .unwrap();
    pg.client
        .execute(
            "UPDATE engine_instances SET lease_until=0 WHERE instance_id=$1",
            &[&owner.instance_id],
        )
        .unwrap();
    let report = pg.reconcile_expired(100, false).unwrap();
    assert_eq!(report.canceled_before_delivery, 1);
    assert_eq!(report.charged_after_delivery, 1);
    let reconciled: Vec<(String, String)> = pg
        .client
        .query(
            "SELECT billing_request_id,delivery_state FROM request_facts \
             WHERE billing_request_id IN ($1,$2) ORDER BY billing_request_id",
            &[&reserved_id, &delivering_id],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    assert_eq!(
        reconciled,
        vec![
            (reserved_id.into(), "not_started".into()),
            (delivering_id.into(), "interrupted".into()),
        ]
    );
    let reconcile_outcome: String = pg
        .client
        .query_one(
            "SELECT billing_outcome FROM request_facts WHERE billing_request_id=$1",
            &[&delivering_id],
        )
        .unwrap()
        .get(0);
    assert_eq!(reconcile_outcome, "reconciled");

    let make_terminal = |logical: &str, billing: Option<&str>, admitted: i64| {
        let base = request_fact_admission(
            logical,
            billing.unwrap_or("99999999-9999-4999-8999-999999999999"),
            None,
            1,
            "rf2-account",
            &key_id,
            admitted,
        );
        crate::request_facts::TerminalRequestFact {
            logical_request_id: base.logical_request_id,
            billing_request_id: billing.map(str::to_owned),
            execution_group_id: None,
            attempt: 1,
            account_id: base.account_id,
            key_id: base.key_id,
            client_kind: base.client_kind,
            client_source: base.client_source,
            client_version: base.client_version,
            provider_plane: base.provider_plane,
            route_class: base.route_class,
            request_class: base.request_class,
            requested_model: base.requested_model,
            executable_model: base.executable_model,
            stream_flag: base.stream_flag,
            tools_declared_count: base.tools_declared_count,
            tool_classes: base.tool_classes,
            tool_choice_mode: base.tool_choice_mode,
            parallel_tools_requested: base.parallel_tools_requested,
            tool_results_in_input: base.tool_results_in_input,
            structured_output_flag: base.structured_output_flag,
            reasoning_flag: base.reasoning_flag,
            service_tier: base.service_tier,
            input_modalities: base.input_modalities,
            output_modalities: base.output_modalities,
            admitted_at: admitted,
            terminal: request_fact_terminal(
                admitted,
                crate::request_facts::DeliveryState::NotStarted,
            ),
        }
    };
    let batch_billing = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1";
    let batch = vec![
        make_terminal(
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2",
            Some(batch_billing),
            admitted_at,
        ),
        make_terminal("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3", None, admitted_at),
    ];
    assert_eq!(pg.insert_terminal_request_facts(&batch).unwrap(), 2);
    assert_eq!(pg.insert_terminal_request_facts(&batch).unwrap(), 1);

    let window = crate::request_facts::RequestFactReadWindow {
        from: admitted_at,
        to: admitted_at + 1,
    };
    let summary = pg.request_facts_summary(window, None).unwrap();
    assert!(summary.totals.persisted >= 8);
    assert_eq!(summary.totals.nonterminal, 0);
    assert!(summary
        .routes
        .groups
        .iter()
        .any(|group| group.values == vec![Some("anthropic".into()), Some("direct".into()), Some("messages".into())]));
    let first_page = pg.request_facts_page(window, None, None, 2).unwrap();
    assert_eq!(first_page.rows.len(), 2);
    assert!(first_page.next.is_some());
    assert!(first_page.rows[0].admitted_at >= first_page.rows[1].admitted_at);
    let second_page = pg
        .request_facts_page(window, None, first_page.next, 2)
        .unwrap();
    assert!(second_page
        .rows
        .iter()
        .all(|row| !first_page.rows.iter().any(|first| first.fact_id == row.fact_id)));
    let account_summary = pg
        .request_facts_summary(window, Some("rf2-account"))
        .unwrap();
    assert!(account_summary.totals.persisted >= 6);
    let logical = pg
        .request_facts_logical("66666666-6666-4666-8666-666666666666")
        .unwrap();
    assert_eq!(logical.rows.len(), 2);
    assert_eq!(logical.rows[0].attempt, 1);
    assert_eq!(logical.rows[1].attempt, 2);
    assert!(!logical.truncated);
    assert!(pg.request_facts_logical("not-a-uuid").is_err());
    assert!(pg
        .request_facts_page(window, None, None, crate::request_facts::MAX_REQUEST_FACT_READ_LIMIT + 1)
        .is_err());
    let oversized = vec![batch[0].clone(); crate::request_facts::MAX_REQUEST_FACT_BATCH + 1];
    assert!(pg.insert_terminal_request_facts(&oversized).is_err());

    // Prune facts first, before their corresponding old lifecycle rows, while retaining young data.
    let cutoff = now() - crate::pricing::PRICING_REQUEST_LIFECYCLE_MIN_RETENTION_SECS;
    let old_billing = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb3";
    let old_admission = request_fact_admission(
        "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb4",
        old_billing,
        None,
        1,
        "rf2-account",
        &key_id,
        cutoff - 1,
    );
    let prune_owner = pg.claim_instance("rf2-prune-owner", 600).unwrap();
    pg.reserve_request_for_execution_with_fact(
        &prune_owner,
        old_billing,
        "rf2-account",
        "rf2-key",
        10,
        600,
        &crate::ExecutionAttempt::direct(),
        &old_admission,
    )
    .unwrap();
    let old_terminal =
        request_fact_terminal(cutoff - 1, crate::request_facts::DeliveryState::NotStarted);
    pg.settle_request_with_request_fact(old_billing, 1, None, None, &old_terminal)
        .unwrap();
    pg.client
        .execute(
            "UPDATE settlement_outbox SET committed_ts=$2,updated_ts=$2 WHERE request_id=$1",
            &[&old_billing, &(cutoff - 1)],
        )
        .unwrap();
    pg.client
        .execute(
            "UPDATE reservations SET settled_ts=$2,updated_ts=$2 WHERE request_id=$1",
            &[&old_billing, &(cutoff - 1)],
        )
        .unwrap();
    let old_fact = make_terminal("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb1", None, cutoff - 1);
    let young_fact = make_terminal("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb2", None, cutoff + 1);
    pg.insert_terminal_request_facts(&[old_fact, young_fact])
        .unwrap();
    let before_illegal: i64 = pg
        .client
        .query_one("SELECT COUNT(*)::bigint FROM request_facts", &[])
        .unwrap()
        .get(0);
    assert!(pg.maintenance_prune(now()).is_err());
    let after_illegal: i64 = pg
        .client
        .query_one("SELECT COUNT(*)::bigint FROM request_facts", &[])
        .unwrap()
        .get(0);
    assert_eq!(before_illegal, after_illegal);
    let report = pg.maintenance_prune(cutoff).unwrap();
    assert_eq!(report.request_facts, 2);
    assert_eq!(report.outbox, 1);
    assert_eq!(report.reservations, 1);
    let related_remaining: i64 = pg
        .client
        .query_one(
            "SELECT (SELECT COUNT(*) FROM request_facts WHERE billing_request_id=$1) +                     (SELECT COUNT(*) FROM settlement_outbox WHERE request_id=$1) +                     (SELECT COUNT(*) FROM reservations WHERE request_id=$1)",
            &[&old_billing],
        )
        .unwrap()
        .get(0);
    assert_eq!(related_remaining, 0);
    let remaining: i64 = pg
        .client
        .query_one(
            "SELECT COUNT(*)::bigint FROM request_facts WHERE logical_request_id=$1",
            &[&"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb2"],
        )
        .unwrap()
        .get(0);
    assert_eq!(remaining, 1);
    unlock_request_fact_matrix(&mut lock_holder);
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
    const RAW_SECRET_KEY: &str = "sk-pool-postgres-secret-never-use-as-key-id";
    const NONSECRET_KEY_ID: &str = "key_postgres_nonsecret_identity_91bd";
    pg.key_issue_with_policy(
        RAW_SECRET_KEY,
        "acct",
        Some("primary"),
        Some(1_500),
        Some(now() + 3_600),
    )
    .unwrap();
    pg.client
        .execute(
            "UPDATE api_keys SET key_id=$1 WHERE key=$2",
            &[&NONSECRET_KEY_ID, &RAW_SECRET_KEY],
        )
        .unwrap();
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
    let auth = pg.key_account(RAW_SECRET_KEY).unwrap().unwrap();
    assert_eq!(auth.account_id, "acct");
    assert_eq!(auth.key_id, NONSECRET_KEY_ID);
    assert_eq!(auth.mult_bp, 2_000);
    assert_eq!(auth.balance_nano, 1_000);
    assert_eq!(auth.spent_nano, 0);
    assert_eq!(auth.reserved_nano, 0);
    assert_eq!(auth.spend_limit_nano, Some(1_500));
    assert!(auth.expires_ts.is_some_and(|expires| expires > now()));
    assert!(auth.active);
    assert!(auth.provider_mult_bp.is_empty());
    assert_eq!(auth.mult_for(PROVIDER_OPENAI), 2_000);
    pg.set_account_provider_discount("acct", PROVIDER_OPENAI, 2_500, now())
        .unwrap();
    pg.set_account_provider_discount("acct", PROVIDER_GOOGLE, 10_000, now())
        .unwrap();
    let auth = pg.key_account(RAW_SECRET_KEY).unwrap().unwrap();
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
        .key_account(RAW_SECRET_KEY)
        .unwrap()
        .unwrap()
        .provider_mult_bp
        .is_empty());

    pg.account_set_status("acct", "disabled").unwrap();
    assert!(!pg.key_account(RAW_SECRET_KEY).unwrap().unwrap().active);
    pg.account_set_status("acct", "active").unwrap();
    pg.key_set_status(RAW_SECRET_KEY, "disabled").unwrap();
    assert!(!pg.key_account(RAW_SECRET_KEY).unwrap().unwrap().active);
    pg.key_set_status(RAW_SECRET_KEY, "active").unwrap();
    assert!(pg.key_account(RAW_SECRET_KEY).unwrap().unwrap().active);

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
        pg.reserve_request(&owner, "req-1", "acct", RAW_SECRET_KEY, 600, 60)
            .unwrap(),
        Some(400)
    );
    assert_eq!(
        pg.reserve_request(&owner, "req-1", "acct", RAW_SECRET_KEY, 600, 60)
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
    pg.client
        .execute(
            "UPDATE ledger SET uncollected_nano=17 WHERE kind='charge' AND request_id='req-1'",
            &[],
        )
        .unwrap();
    let charge = pg
        .ledger_after("acct", 0, 100)
        .unwrap()
        .into_iter()
        .find(|entry| entry.request_id.as_deref() == Some("req-1"))
        .unwrap();
    assert_eq!(charge.amount_nano, 250, "billed amount stays unchanged");
    assert_eq!(charge.uncollected_nano, 17);

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
        pg.reserve_request(&owner, "req-2", "acct", RAW_SECRET_KEY, 300, 60)
            .unwrap(),
        Some(450)
    );
    assert_eq!(pg.cancel_request("req-2").unwrap(), Some(750));
    assert_eq!(pg.cancel_request("req-2").unwrap(), Some(750));

    // Crash boundary: enqueue commits but settlement application has not run. A fresh connection
    // drains the durable row exactly once.
    assert_eq!(
        pg.reserve_request(&owner, "req-3", "acct", RAW_SECRET_KEY, 400, 60)
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

    // Three in-flight requests share one account-wide settlement floor. They deliberately finish
    // on independent PostgreSQL sessions so the test covers the row-lock serialization, not merely
    // the arithmetic of a sequential happy path.
    pg.account_create("floor-pg-acct", None, 5_000).unwrap();
    pg.account_topup("floor-pg-acct", 700_000_000, Some("floor-pg-seed"))
        .unwrap();
    pg.key_issue_with_policy(
        "floor-pg-key",
        "floor-pg-acct",
        None,
        Some(1_850_000_000),
        None,
    )
    .unwrap();
    let floor_pricing = crate::ReservationPricing::new(crate::PROVIDER_OPENAI, 5_000).unwrap();
    for (request_id, hold, expected_balance) in [
        ("floor-pg-1", 200_000_000, 500_000_000),
        ("floor-pg-2", 200_000_000, 300_000_000),
        ("floor-pg-3", 300_000_000, 0),
    ] {
        assert_eq!(
            pg.reserve_priced_request_for_execution(
                &owner,
                request_id,
                "floor-pg-acct",
                "floor-pg-key",
                hold,
                60,
                &crate::ExecutionAttempt::direct(),
                &floor_pricing,
            )
            .unwrap(),
            Some(expected_balance),
        );
    }
    // An edit after admission affects only the next request; every in-flight charge keeps its
    // reserve-time multiplier and remains reconcilable against the matching official basis.
    pg.set_account_provider_discount("floor-pg-acct", crate::PROVIDER_OPENAI, 10_000, now())
        .unwrap();

    let settlement_barrier = Arc::new(Barrier::new(4));
    let mut settlement_joins = Vec::new();
    for (request_id, actual) in [
        ("floor-pg-1", 550_000_000),
        ("floor-pg-2", 600_000_000),
        ("floor-pg-3", 700_000_000),
    ] {
        let settlement_url = url.clone();
        let barrier = Arc::clone(&settlement_barrier);
        settlement_joins.push(std::thread::spawn(move || {
            let mut connection = PgStore::connect(&settlement_url).unwrap();
            let usage = UsageEventInput {
                model: "gpt-floor-test".into(),
                provider: crate::PROVIDER_OPENAI.into(),
                real_nano: actual * 2,
                charge_basis_nano: actual * 2,
                ..Default::default()
            };
            let reference = format!("{request_id}:provider");
            barrier.wait();
            connection
                .settle_request(request_id, actual, Some(&reference), Some(&usage))
                .unwrap()
        }));
    }
    settlement_barrier.wait();
    for result in settlement_joins {
        assert!(
            result
                .join()
                .unwrap()
                .is_some_and(|balance| balance >= -1_000_000_000),
            "every terminal update must observe the shared account floor",
        );
    }

    let floor_account = pg
        .client
        .query_one(
            "SELECT balance_nano,spent_nano,reserved_nano,uncollected_nano \
             FROM accounts WHERE id='floor-pg-acct'",
            &[],
        )
        .unwrap();
    let floor_balance: i64 = floor_account.get(0);
    let floor_spent: i64 = floor_account.get(1);
    let floor_reserved: i64 = floor_account.get(2);
    let floor_uncollected: i64 = floor_account.get(3);
    assert_eq!(
        (
            floor_balance,
            floor_spent,
            floor_reserved,
            floor_uncollected
        ),
        (-1_000_000_000, 1_850_000_000, 0, 150_000_000),
    );
    assert_eq!(
        floor_balance + floor_spent + floor_reserved - floor_uncollected,
        700_000_000,
    );
    let floor_key = pg.key_get("floor-pg-key").unwrap().unwrap();
    assert_eq!(floor_key.spent_nano, 1_850_000_000);
    assert_eq!(floor_key.reserved_nano, 0);
    assert_eq!(floor_key.spend_limit_nano, Some(1_850_000_000));
    assert_eq!(
        pg.reserve_request(
            &owner,
            "floor-pg-over-limit",
            "floor-pg-acct",
            "floor-pg-key",
            1,
            60,
        )
        .unwrap(),
        None,
        "the full billed amount, including shortfall, must consume the key spend limit",
    );
    let floor_evidence = pg
        .client
        .query_one(
            "SELECT COALESCE(SUM(reservation.actual_nano),0)::bigint, \
                    COALESCE(SUM(reservation.collected_nano),0)::bigint, \
                    COALESCE(SUM(reservation.uncollected_nano),0)::bigint, \
                    COUNT(DISTINCT reservation.provider)::bigint, \
                    MIN(reservation.payable_multiplier_bp),MAX(reservation.payable_multiplier_bp), \
                    (SELECT COALESCE(SUM(amount_nano),0)::bigint FROM ledger \
                      WHERE kind='charge' AND request_id LIKE 'floor-pg-%'), \
                    (SELECT COALESCE(SUM(uncollected_nano),0)::bigint FROM ledger \
                      WHERE kind='charge' AND request_id LIKE 'floor-pg-%'), \
                    (SELECT COALESCE(SUM(official_nano),0)::bigint FROM ledger \
                      WHERE kind='charge' AND request_id LIKE 'floor-pg-%'), \
                    (SELECT COALESCE(SUM(charge_nano),0)::bigint FROM usage_events \
                      WHERE request_id LIKE 'floor-pg-%'), \
                    (SELECT COALESCE(SUM(uncollected_nano),0)::bigint FROM usage_events \
                      WHERE request_id LIKE 'floor-pg-%'), \
                    (SELECT COALESCE(SUM(charge_basis_nano),0)::bigint FROM usage_events \
                      WHERE request_id LIKE 'floor-pg-%') \
             FROM reservations reservation WHERE reservation.request_id LIKE 'floor-pg-%'",
            &[],
        )
        .unwrap();
    assert_eq!(floor_evidence.get::<_, i64>(0), 1_850_000_000);
    assert_eq!(floor_evidence.get::<_, i64>(1), 1_700_000_000);
    assert_eq!(floor_evidence.get::<_, i64>(2), 150_000_000);
    assert_eq!(floor_evidence.get::<_, i64>(3), 1);
    assert_eq!(floor_evidence.get::<_, Option<i64>>(4), Some(5_000));
    assert_eq!(floor_evidence.get::<_, Option<i64>>(5), Some(5_000));
    assert_eq!(floor_evidence.get::<_, i64>(6), 1_850_000_000);
    assert_eq!(floor_evidence.get::<_, i64>(7), 150_000_000);
    assert_eq!(floor_evidence.get::<_, i64>(8), 3_700_000_000);
    assert_eq!(floor_evidence.get::<_, i64>(9), 1_850_000_000);
    assert_eq!(floor_evidence.get::<_, i64>(10), 150_000_000);
    assert_eq!(floor_evidence.get::<_, i64>(11), 3_700_000_000);
    let replay_usage = UsageEventInput {
        model: "gpt-floor-test".into(),
        provider: crate::PROVIDER_OPENAI.into(),
        real_nano: 1_400_000_000,
        charge_basis_nano: 1_400_000_000,
        ..Default::default()
    };
    assert_eq!(
        pg.settle_request(
            "floor-pg-3",
            700_000_000,
            Some("floor-pg-3:provider"),
            Some(&replay_usage),
        )
        .unwrap(),
        Some(-1_000_000_000),
    );
    assert_eq!(
        pg.account_get("floor-pg-acct")
            .unwrap()
            .unwrap()
            .uncollected_nano,
        150_000_000,
    );

    // A negative adjustment may record debt below the ordinary floor while a hold is in flight.
    // Settlement must not worsen that debt, but it must still consume the hold already removed
    // from the balance rather than misclassifying the whole request as pool-funded loss.
    pg.account_create("debt-pg-acct", None, 5_000).unwrap();
    pg.account_topup("debt-pg-acct", 1_000_000_000, Some("debt-pg-seed"))
        .unwrap();
    pg.key_issue("debt-pg-key", "debt-pg-acct", None).unwrap();
    assert_eq!(
        pg.reserve_priced_request_for_execution(
            &owner,
            "debt-pg-request",
            "debt-pg-acct",
            "debt-pg-key",
            500_000_000,
            60,
            &crate::ExecutionAttempt::direct(),
            &floor_pricing,
        )
        .unwrap(),
        Some(500_000_000),
    );
    assert_eq!(
        pg.account_topup("debt-pg-acct", -2_000_000_000, Some("debt-pg-adjustment"),)
            .unwrap(),
        Some(-1_500_000_000),
    );
    let debt_usage = UsageEventInput {
        model: "gpt-debt-test".into(),
        provider: crate::PROVIDER_OPENAI.into(),
        real_nano: 1_400_000_000,
        charge_basis_nano: 1_400_000_000,
        ..Default::default()
    };
    assert_eq!(
        pg.settle_request(
            "debt-pg-request",
            700_000_000,
            Some("debt-pg-provider-ref"),
            Some(&debt_usage),
        )
        .unwrap(),
        Some(-1_500_000_000),
    );
    let debt_evidence = pg
        .client
        .query_one(
            "SELECT a.balance_nano,a.spent_nano,a.reserved_nano,a.uncollected_nano, \
                    r.collected_nano,r.uncollected_nano,l.amount_nano,l.uncollected_nano \
             FROM accounts a JOIN reservations r ON r.account_id=a.id \
             JOIN ledger l ON l.account_id=a.id AND l.request_id=r.request_id \
             WHERE a.id='debt-pg-acct'",
            &[],
        )
        .unwrap();
    assert_eq!(
        (0..8)
            .map(|index| debt_evidence.get::<_, i64>(index))
            .collect::<Vec<_>>(),
        vec![
            -1_500_000_000,
            700_000_000,
            0,
            200_000_000,
            500_000_000,
            200_000_000,
            700_000_000,
            200_000_000,
        ],
    );

    // Zero-multiplier requests carry no customer debit, but their authoritative provider usage is
    // still durable. The settlement writes no charge ledger row and exact replay stays idempotent.
    pg.account_create("meter-only-pg-acct", None, 0).unwrap();
    pg.key_issue("meter-only-pg-key", "meter-only-pg-acct", None)
        .unwrap();
    let meter_only_pricing = crate::ReservationPricing::new(crate::PROVIDER_OPENAI, 0).unwrap();
    assert_eq!(
        pg.reserve_priced_request_for_execution(
            &owner,
            "meter-only-pg-request",
            "meter-only-pg-acct",
            "meter-only-pg-key",
            0,
            60,
            &crate::ExecutionAttempt::direct(),
            &meter_only_pricing,
        )
        .unwrap(),
        Some(0),
    );
    let meter_only_usage = UsageEventInput {
        model: "gpt-meter-only".into(),
        provider: crate::PROVIDER_OPENAI.into(),
        input_tokens: 7,
        output_tokens: 11,
        real_nano: 123,
        charge_basis_nano: 123,
        ..Default::default()
    };
    assert_eq!(
        pg.settle_request(
            "meter-only-pg-request",
            0,
            Some("meter-only-pg-ref"),
            Some(&meter_only_usage),
        )
        .unwrap(),
        Some(0),
    );
    let meter_only_evidence = pg
        .client
        .query_one(
            "SELECT account.balance_nano,account.spent_nano,account.reserved_nano,
                    account.uncollected_nano,
                    usage.real_nano,usage.charge_nano,usage.provider,
                    usage.payable_multiplier_bp,usage.charge_basis_nano,usage.uncollected_nano,
                    (SELECT COUNT(*)::bigint FROM ledger
                      WHERE kind='charge' AND request_id='meter-only-pg-request')
               FROM accounts account
               JOIN usage_events usage ON usage.account_id=account.id
              WHERE account.id='meter-only-pg-acct'
                AND usage.request_id='meter-only-pg-request'",
            &[],
        )
        .unwrap();
    assert_eq!(meter_only_evidence.get::<_, i64>(0), 0);
    assert_eq!(meter_only_evidence.get::<_, i64>(1), 0);
    assert_eq!(meter_only_evidence.get::<_, i64>(2), 0);
    assert_eq!(meter_only_evidence.get::<_, i64>(3), 0);
    assert_eq!(meter_only_evidence.get::<_, i64>(4), 123);
    assert_eq!(meter_only_evidence.get::<_, i64>(5), 0);
    assert_eq!(
        meter_only_evidence.get::<_, String>(6),
        crate::PROVIDER_OPENAI
    );
    assert_eq!(meter_only_evidence.get::<_, Option<i64>>(7), Some(0));
    assert_eq!(meter_only_evidence.get::<_, Option<i64>>(8), Some(123));
    assert_eq!(meter_only_evidence.get::<_, i64>(9), 0);
    assert_eq!(meter_only_evidence.get::<_, i64>(10), 0);
    assert_eq!(
        pg.settle_request(
            "meter-only-pg-request",
            0,
            Some("meter-only-pg-ref"),
            Some(&meter_only_usage),
        )
        .unwrap(),
        Some(0),
    );
    assert_eq!(
        pg.client
            .query_one(
                "SELECT COUNT(*)::bigint FROM usage_events
                  WHERE request_id='meter-only-pg-request'",
                &[],
            )
            .unwrap()
            .get::<_, i64>(0),
        1,
    );

    // A damaged per-key hold must not be hidden by the account aggregate. The account UPDATE runs
    // first in the transaction, so observing the original tuple afterwards proves full rollback.
    pg.account_create("key-fence-pg-acct", None, 10_000)
        .unwrap();
    pg.account_topup("key-fence-pg-acct", 1_000, Some("key-fence-pg-seed"))
        .unwrap();
    pg.key_issue("key-fence-pg-key", "key-fence-pg-acct", None)
        .unwrap();
    assert_eq!(
        pg.reserve_request(
            &owner,
            "key-fence-pg-request",
            "key-fence-pg-acct",
            "key-fence-pg-key",
            400,
            60,
        )
        .unwrap(),
        Some(600),
    );
    pg.client
        .execute(
            "UPDATE api_keys SET reserved_nano=0 WHERE key='key-fence-pg-key'",
            &[],
        )
        .unwrap();
    let key_fence_error = pg
        .settle_request("key-fence-pg-request", 300, Some("key-fence-pg-ref"), None)
        .unwrap_err()
        .to_string();
    assert!(
        key_fence_error.contains("reservation/key aggregate invariant failed"),
        "unexpected PostgreSQL key-fence error: {key_fence_error}",
    );
    let key_fence_account = pg
        .client
        .query_one(
            "SELECT balance_nano,spent_nano,reserved_nano,uncollected_nano
               FROM accounts WHERE id='key-fence-pg-acct'",
            &[],
        )
        .unwrap();
    assert_eq!(
        (
            key_fence_account.get::<_, i64>(0),
            key_fence_account.get::<_, i64>(1),
            key_fence_account.get::<_, i64>(2),
            key_fence_account.get::<_, i64>(3),
        ),
        (600, 0, 400, 0),
    );
    let key_fence_reservation = pg
        .client
        .query_one(
            "SELECT state,actual_nano,collected_nano,uncollected_nano
               FROM reservations WHERE request_id='key-fence-pg-request'",
            &[],
        )
        .unwrap();
    assert_eq!(
        (
            key_fence_reservation.get::<_, String>(0),
            key_fence_reservation.get::<_, Option<i64>>(1),
            key_fence_reservation.get::<_, Option<i64>>(2),
            key_fence_reservation.get::<_, Option<i64>>(3),
        ),
        ("settlement_pending".into(), Some(300), None, None),
        "only the durable intent may survive a failed settlement transaction",
    );
    pg.client
        .execute(
            "UPDATE api_keys SET reserved_nano=400 WHERE key='key-fence-pg-key'",
            &[],
        )
        .unwrap();
    assert_eq!(
        pg.settle_request("key-fence-pg-request", 300, Some("key-fence-pg-ref"), None,)
            .unwrap(),
        Some(700),
    );

    // Unique request evidence is part of the money transaction. A pre-existing ledger or usage
    // row must fail the settlement and roll back account/key/winner updates, never be ignored.
    pg.account_create("evidence-pg-acct", None, 5_000).unwrap();
    pg.account_topup("evidence-pg-acct", 1_000, Some("evidence-pg-seed"))
        .unwrap();
    pg.key_issue("evidence-pg-key", "evidence-pg-acct", None)
        .unwrap();
    let evidence_pricing = crate::ReservationPricing::new(crate::PROVIDER_OPENAI, 5_000).unwrap();
    assert_eq!(
        pg.reserve_priced_request_for_execution(
            &owner,
            "evidence-pg-request",
            "evidence-pg-acct",
            "evidence-pg-key",
            400,
            60,
            &crate::ExecutionAttempt::direct(),
            &evidence_pricing,
        )
        .unwrap(),
        Some(600),
    );
    let evidence_usage = UsageEventInput {
        model: "gpt-evidence".into(),
        provider: crate::PROVIDER_OPENAI.into(),
        real_nano: 600,
        charge_basis_nano: 600,
        ..Default::default()
    };
    pg.client
        .execute(
            "INSERT INTO ledger(
                 account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts,
                 model,provider,official_nano,payable_multiplier_bp,uncollected_nano
             ) VALUES(
                 'evidence-pg-acct','evidence-pg-key','charge','evidence-pg-request',1,
                 'foreign-ledger-row',999,1,'foreign','openai',2,5000,0
             )",
            &[],
        )
        .unwrap();
    assert!(pg
        .settle_request(
            "evidence-pg-request",
            300,
            Some("evidence-pg-ref"),
            Some(&evidence_usage),
        )
        .is_err());
    let evidence_account = pg
        .client
        .query_one(
            "SELECT balance_nano,spent_nano,reserved_nano,uncollected_nano
               FROM accounts WHERE id='evidence-pg-acct'",
            &[],
        )
        .unwrap();
    assert_eq!(
        (
            evidence_account.get::<_, i64>(0),
            evidence_account.get::<_, i64>(1),
            evidence_account.get::<_, i64>(2),
            evidence_account.get::<_, i64>(3),
        ),
        (600, 0, 400, 0),
    );
    pg.client
        .execute(
            "DELETE FROM ledger WHERE kind='charge' AND request_id='evidence-pg-request'",
            &[],
        )
        .unwrap();
    pg.client
        .execute(
            "INSERT INTO usage_events(request_id,account_id,key,model,provider,ts)
             VALUES('evidence-pg-request','evidence-pg-acct','evidence-pg-key','foreign','openai',1)",
            &[],
        )
        .unwrap();
    assert!(pg
        .settle_request(
            "evidence-pg-request",
            300,
            Some("evidence-pg-ref"),
            Some(&evidence_usage),
        )
        .is_err());
    let evidence_account = pg
        .client
        .query_one(
            "SELECT balance_nano,spent_nano,reserved_nano,uncollected_nano
               FROM accounts WHERE id='evidence-pg-acct'",
            &[],
        )
        .unwrap();
    assert_eq!(
        (
            evidence_account.get::<_, i64>(0),
            evidence_account.get::<_, i64>(1),
            evidence_account.get::<_, i64>(2),
            evidence_account.get::<_, i64>(3),
        ),
        (600, 0, 400, 0),
    );
    assert_eq!(
        pg.client
            .query_one(
                "SELECT COUNT(*)::bigint FROM ledger
                  WHERE kind='charge' AND request_id='evidence-pg-request'",
                &[],
            )
            .unwrap()
            .get::<_, i64>(0),
        0,
        "the ledger insert preceding the usage conflict must roll back",
    );
    pg.client
        .execute(
            "DELETE FROM usage_events WHERE request_id='evidence-pg-request'",
            &[],
        )
        .unwrap();
    assert_eq!(
        pg.settle_request(
            "evidence-pg-request",
            300,
            Some("evidence-pg-ref"),
            Some(&evidence_usage),
        )
        .unwrap(),
        Some(700),
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
        .reserve_request(&owner, "stale", "acct", RAW_SECRET_KEY, 1, 60)
        .is_err());
    assert_eq!(
        pg.reserve_request(&owner2, "req-4", "acct", RAW_SECRET_KEY, 100, 60)
            .unwrap(),
        Some(550)
    );
    pg.cancel_request("req-4").unwrap();

    // Recovery distinguishes a request never delivered (refund) from a delivered response whose
    // exact usage was lost (conservatively charge the already approved hold).
    let dead = pg.claim_instance("dead-engine", 60).unwrap();
    pg.reserve_request(&dead, "req-5", "acct", RAW_SECRET_KEY, 100, 1)
        .unwrap();
    pg.reserve_request(&dead, "req-6", "acct", RAW_SECRET_KEY, 100, 1)
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
          a.balance_nano + a.spent_nano + a.reserved_nano - a.uncollected_nano \
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
    gemini_payload_max_threshold["long_context_threshold"] = serde_json::json!(u64::MAX);
    let insert =
        |version: i64, effective_from: i64, payload: serde_json::Value| TariffOverrideInsert {
            tariff_family: family.to_owned(),
            version,
            effective_from,
            payload,
            created_by: "matrix-operator".to_owned(),
            reason: "postgres matrix".to_owned(),
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
    assert_eq!(gap, O::Rejected(R::SequenceViolation { expected_next: 3 }));
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
    assert_eq!(
        resolve_tariff_override(&rows, "google/gemini/gemini-2.5-flash", i64::MAX),
        None
    );

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
    let tampered = pg
        .list_tariff_overrides()
        .expect_err("tampered row must fail closed");
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
