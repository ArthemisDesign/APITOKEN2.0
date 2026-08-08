use super::*;
use std::sync::{Arc, Barrier};

#[test]
fn legacy_pricing_path_closed_unit_predicate() {
    // Dual-path closure predicate: closed only while a head exists AND the account has not
    // opted out. Every other combination keeps the writer open.
    assert!(!legacy_pricing_path_closed(false, false));
    assert!(!legacy_pricing_path_closed(false, true));
    assert!(legacy_pricing_path_closed(true, false));
    assert!(!legacy_pricing_path_closed(true, true));
}

fn release_settlement_snapshot(
    billing_mode: crate::pricing::BillingModeV2,
    provider_id: &str,
) -> crate::pricing::PricingRequestSnapshotV2 {
    let balance = billing_mode == crate::pricing::BillingModeV2::Balance;
    crate::pricing::PricingRequestSnapshotV2 {
        request_id: "release-settlement-request".into(),
        account_id: "release-settlement-account".into(),
        release_schema_version: 2,
        release_generation: 1,
        release_digest: "release-digest".into(),
        assignment_digest: "assignment-digest".into(),
        account_class: if balance {
            crate::pricing::AccountClass::B2c
        } else {
            crate::pricing::AccountClass::Service
        },
        policy_id: "release-policy".into(),
        policy_version: 1,
        policy_digest: "release-policy-digest".into(),
        billing_mode,
        funding_generation: balance.then_some(1),
        provider_id: provider_id.into(),
        canonical_model_id: "provider-model".into(),
        rule: balance.then(|| crate::pricing::PricingReleasePolicyRuleV2 {
            rule_id: "global-rule".into(),
            rule_digest: "global-rule-digest".into(),
            scope: crate::pricing::PricingReleaseRuleScopeV2::Global,
            discount_bps: 5_000,
            payable_multiplier_bp: 5_000,
        }),
        tariff_schedule_id: "provider-tariff".into(),
        tariff_priced_ts: 1,
        official_hold_nano: 8_000_000,
        charged_hold_nano: if balance { 4_000_000 } else { 0 },
        official_cost_json: serde_json::json!({}),
        snapshot_digest: "snapshot-digest".into(),
        created_ts: 1,
    }
}

#[test]
fn release_settlement_preserves_provider_adapter_customer_cap() {
    let snapshot = release_settlement_snapshot(
        crate::pricing::BillingModeV2::Balance,
        crate::PROVIDER_OPENAI,
    );
    let usage = UsageEventInput {
        model: "provider-model".into(),
        provider: crate::PROVIDER_OPENAI.into(),
        real_nano: 30_500_000,
        charge_basis_nano: 30_500_000,
        output_tokens: 1_000,
        ..UsageEventInput::default()
    };
    assert_eq!(
        pricing_release_settlement_actual_v2(
            &snapshot,
            4_000_000,
            3_500_000,
            "settle",
            Some(&usage),
        )
        .unwrap(),
        3_500_000
    );
}

#[test]
fn release_settlement_fails_closed_on_lineage_and_meter_only_debit() {
    let balance = release_settlement_snapshot(
        crate::pricing::BillingModeV2::Balance,
        crate::PROVIDER_OPENAI,
    );
    let wrong_provider = UsageEventInput {
        provider: crate::PROVIDER_GOOGLE.into(),
        real_nano: 1,
        charge_basis_nano: 1,
        ..UsageEventInput::default()
    };
    assert!(pricing_release_settlement_actual_v2(
        &balance,
        100,
        1,
        "settle",
        Some(&wrong_provider),
    )
    .is_err());

    let service = release_settlement_snapshot(
        crate::pricing::BillingModeV2::MeterOnly,
        crate::PROVIDER_GOOGLE,
    );
    assert!(pricing_release_settlement_actual_v2(&service, 0, 1, "settle", None).is_err());
    assert_eq!(
        pricing_release_settlement_actual_v2(&service, 0, 0, "settle", None).unwrap(),
        0
    );
}

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
             execution_group_winner,settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances,
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

    let aborted_snapshot = legacy_snapshot("aborted-before-commit", "snapshot-account", 500, 100);
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
    let mut entries = vec![
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
    ];
    if product_id == "main" {
        entries.push(crate::pricing::PricingCatalogEntrySpec {
            provider_id: "google".into(),
            canonical_model_id: "gemini-3-flash-preview".into(),
            enabled: true,
        });
    }
    crate::pricing::PricingCatalogSpec {
        product_id: product_id.into(),
        generation: 1,
        schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
        capability_generation: 1,
        capability_digest: "stage8-capability-1".into(),
        content_digest: format!("stage8-{product_id}-catalog-1"),
        entries,
    }
}

fn stage8_pg_switches() -> crate::pricing::ProviderSwitchSpec {
    use crate::pricing::{PolicySegment, ProviderSwitchEntrySpec, ProviderSwitchScope};

    let mut entries = Vec::new();
    for provider_id in ["anthropic", "openai", "google"] {
        entries.push(ProviderSwitchEntrySpec {
            provider_id: provider_id.into(),
            scope: ProviderSwitchScope::Master,
            catalog_generation: None,
            enabled: true,
        });
        let products: &[&str] = if provider_id == "google" {
            &["main"]
        } else {
            &["main", "openkeys"]
        };
        for product_id in products {
            entries.push(ProviderSwitchEntrySpec {
                provider_id: provider_id.into(),
                scope: ProviderSwitchScope::Product {
                    product_id: (*product_id).into(),
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
             execution_group_winner,settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances,
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
        let snapshot = legacy_snapshot(request_id, "shadow-pg-account", 500_000_000, 100_000_000);
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
    assert_eq!(CURRENT_SCHEMA_VERSION, 40);
    let registered = ENGINE_MIGRATIONS
        .iter()
        .find(|(version, _)| *version == 29)
        .map(|(_, sql)| *sql);
    assert_eq!(registered, Some(MIGRATION_0029));
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
/// pg::tests::pricing_release_funding_v2_postgres_matrix`
#[test]
fn pricing_release_funding_v2_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping pricing/funding v2 PostgreSQL matrix: test URL is unset");
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
    pg.client.batch_execute("BEGIN").unwrap();
    // Other destructive matrices may leave a strict binding committed. Remove the
    // account-owned policy graph inside this rollback-only fixture before inserting a
    // legacy runtime row, so the advisory lock also provides order-independent isolation.
    pg.client
        .batch_execute(
            "TRUNCATE accounts RESTART IDENTITY CASCADE;
             TRUNCATE engine_instances RESTART IDENTITY CASCADE;

             INSERT INTO accounts(
                 id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status,created_ts,created
             ) VALUES
                 ('v2-matrix-b2c','v2-matrix-b2c',900,0,100,5000,'active',100,'matrix'),
                 ('v2-matrix-service','v2-matrix-service',0,0,0,10000,'active',100,'matrix');

             INSERT INTO pricing_release_policy_versions(
                 policy_id,policy_version,owner_type,owner_id,account_class,product_id,
                 billing_mode,schema_version,capability_generation,capability_digest,
                 catalog_generation,catalog_digest,switch_generation,switch_digest,
                 content_digest,created_ts
             ) VALUES
                 ('v2-policy-b2c',1,'global_b2c','global','b2c','main','balance',2,
                  1,'capability-v2',1,'catalog-main-v2',1,'switch-v2','policy-b2c-v2',100),
                 ('v2-policy-service',1,'service','service-domain','service',NULL,
                  'meter_only',2,1,'capability-v2',NULL,NULL,NULL,NULL,
                  'policy-service-v2',100);
             INSERT INTO pricing_release_policy_rules(
                 policy_id,policy_version,rule_id,rule_digest,scope_type,provider_id,
                 canonical_model_id,discount_bps,payable_multiplier_bp
             ) VALUES(
                 'v2-policy-b2c',1,'global-50','global-50-digest','global',NULL,NULL,5000,5000
             );

             INSERT INTO pricing_release_versions(
                 generation,release_kind,schema_version,capability_generation,
                 capability_digest,main_catalog_generation,main_catalog_digest,
                 openkeys_catalog_generation,openkeys_catalog_digest,switch_generation,
                 switch_digest,inventory_digest,policy_manifest_digest,
                 assignment_manifest_digest,funding_manifest_digest,
                 minimum_runtime_schema_version,content_digest,created_ts
             ) VALUES
                 (101,'target',2,1,'capability-v2',1,'catalog-main-v2',1,
                  'catalog-openkeys-v2',1,'switch-v2','inventory-v2','policies-v2',
                  'assignments-v2','funding-v2',2,'release-target-v2',100),
                 (102,'recovery',2,1,'capability-v2',1,'catalog-main-v2',1,
                  'catalog-openkeys-v2',1,'switch-v2','inventory-v2','policies-recovery-v2',
                  'assignments-recovery-v2','funding-v2',2,'release-recovery-v2',100);
             INSERT INTO pricing_release_recovery_links(
                 target_generation,target_digest,recovery_generation,recovery_digest,
                 link_digest,created_ts
             ) VALUES(101,'release-target-v2',102,'release-recovery-v2','recovery-link-v2',100);

             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES('v2-matrix-b2c',1,2,'source-v2','normalization-v2',900,100,0,0,100,100);
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES(
                 'v2-paid-lot','v2-matrix-b2c',1,'paid','payment:v2',900,100,0,0,'active',100,100
             );
             INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts
             ) VALUES('v2-matrix-b2c',1,1,100);

             INSERT INTO pricing_release_assignments(
                 release_generation,account_id,account_class,policy_id,policy_version,
                 policy_digest,billing_mode,funding_generation,purpose,responsible,
                 assignment_digest
             ) VALUES
                 (101,'v2-matrix-b2c','b2c','v2-policy-b2c',1,'policy-b2c-v2',
                  'balance',1,NULL,NULL,'assignment-b2c-v2'),
                 (101,'v2-matrix-service','service','v2-policy-service',1,
                  'policy-service-v2','meter_only',NULL,'internal-domain','owner-team',
                  'assignment-service-v2');

             INSERT INTO pricing_stage8_evidence_v2(
                 evidence_digest,target_generation,target_digest,recovery_generation,
                 recovery_digest,inventory_digest,funding_digest,shadow_digest,
                 runtime_floor_digest,legacy_inflight_count,blocker_count,passed,
                 observed_ts,valid_until_ts
             ) VALUES(
                 'evidence-v2',101,'release-target-v2',102,'release-recovery-v2',
                 'inventory-v2','funding-v2','shadow-v2','runtime-v2',2,0,true,100,1000
             );

             INSERT INTO engine_instances(
                 instance_id,owner_epoch,lease_until,started_ts,updated_ts
             ) VALUES('v2-epoch-fence',1,1000,100,100);
             ",
        )
        .unwrap();

    pg.client
        .batch_execute("SAVEPOINT missing_activation_audit")
        .unwrap();
    let missing_audit = pg
        .client
        .batch_execute(
            "INSERT INTO pricing_release_head_v2(
                 singleton,active_generation,active_digest,head_version,updated_ts
             ) VALUES(1,101,'release-target-v2',1,500);
             SET CONSTRAINTS pricing_release_head_audit_v2 IMMEDIATE;",
        )
        .expect_err("head without activation audit must fail");
    assert!(missing_audit
        .as_db_error()
        .is_some_and(|error| error.message().contains("lacks matching activation audit")));
    pg.client
        .batch_execute(
            "ROLLBACK TO SAVEPOINT missing_activation_audit;
             RELEASE SAVEPOINT missing_activation_audit;",
        )
        .unwrap();

    pg.client
        .batch_execute(
            "INSERT INTO pricing_release_activations_v2(
                 activation_kind,from_generation,from_digest,to_generation,to_digest,
                 expected_head_version,resulting_head_version,evidence_digest,operator_id,
                 reason,activated_ts
             ) VALUES(
                 'cutover',NULL,NULL,101,'release-target-v2',0,1,'evidence-v2',
                 'matrix-operator','matrix cutover',500
             );
             INSERT INTO pricing_release_head_v2(
                 singleton,active_generation,active_digest,head_version,updated_ts
             ) VALUES(1,101,'release-target-v2',1,500);

             INSERT INTO reservations(
                 request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                 owner_instance,owner_epoch,lease_until,state,actual_nano,created_ts,updated_ts
             ) VALUES
                 ('v2-request-b2c','v2-matrix-b2c','v2-key-b2c',100,900,
                  'matrix-owner',1,1000,'reserved',NULL,500,500),
                 ('v2-request-service','v2-matrix-service','v2-key-service',0,0,
                  'matrix-owner',1,1000,'reserved',NULL,500,500);
             INSERT INTO pricing_request_snapshots_v2(
                 request_id,account_id,release_schema_version,release_generation,
                 release_digest,assignment_digest,account_class,policy_id,policy_version,
                 policy_digest,billing_mode,funding_generation,provider_id,canonical_model_id,
                 rule_id,rule_digest,rule_scope,discount_bps,payable_multiplier_bp,
                 tariff_schedule_id,tariff_priced_ts,official_hold_nano,charged_hold_nano,
                 official_cost_json,snapshot_digest,created_ts
             ) VALUES
                 ('v2-request-b2c','v2-matrix-b2c',2,101,'release-target-v2',
                  'assignment-b2c-v2','b2c','v2-policy-b2c',1,'policy-b2c-v2','balance',1,
                  'google','gemini-2.5-pro','global-50','global-50-digest','global',5000,5000,
                  'tariff-v2',500,200,100,'{}'::jsonb,'snapshot-b2c-v2',500),
                 ('v2-request-service','v2-matrix-service',2,101,'release-target-v2',
                  'assignment-service-v2','service','v2-policy-service',1,
                  'policy-service-v2','meter_only',NULL,'google','gemini-2.5-pro',
                  NULL,NULL,NULL,NULL,NULL,'tariff-v2',500,500,0,'{}'::jsonb,
                  'snapshot-service-v2',500);
             INSERT INTO pricing_request_funding_allocations_v2(
                 request_id,account_id,funding_generation,allocation_order,lot_id,
                 lot_source_type,lot_version,reserved_nano,charged_nano,released_nano
             ) VALUES(
                 'v2-request-b2c','v2-matrix-b2c',1,1,'v2-paid-lot','paid',0,100,NULL,NULL
             );
             INSERT INTO usage_events(
                 request_id,account_id,key,model,real_nano,charge_nano,provider,ts,
                 release_schema_version,release_generation,release_digest,
                 release_billing_mode,release_funding_generation,release_snapshot_digest
             ) VALUES(
                 'v2-request-service','v2-matrix-service','v2-key-service','gemini-2.5-pro',
                 500,0,'google',500,2,101,'release-target-v2','meter_only',NULL,
                 'snapshot-service-v2'
             );
             SET CONSTRAINTS ALL IMMEDIATE;",
        )
        .unwrap();

    pg.client
        .batch_execute(
            "INSERT INTO accounts(
                 id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status,created_ts,created
             ) VALUES(
                 'v2-post-cutover-b2c','v2-post-cutover-b2c',0,0,0,5000,'active',501,'matrix'
             );
             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES(
                 'v2-post-cutover-b2c',1,2,'post-cutover-source-v2',
                 'post-cutover-normalization-v2',0,0,0,0,501,501
             );
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES(
                 'v2-post-cutover-paid','v2-post-cutover-b2c',1,'paid','provision:v2',
                 0,0,0,0,'exhausted',501,501
             );
             INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts
             ) VALUES('v2-post-cutover-b2c',1,1,501);
             SET CONSTRAINTS ALL IMMEDIATE;",
        )
        .unwrap();

    pg.client
        .batch_execute("SAVEPOINT incomplete_assignment_extension")
        .unwrap();
    let incomplete_extension = pg
        .client
        .batch_execute(
            "INSERT INTO pricing_release_assignment_extensions_v2(
                 release_generation,account_id,account_class,policy_id,policy_version,
                 policy_digest,billing_mode,funding_generation,purpose,responsible,
                 assignment_digest,provisioning_head_generation,provisioning_head_digest,
                 provisioning_head_version,paired_recovery_generation,paired_recovery_digest,
                 extension_group_digest,extension_digest,created_ts
             ) VALUES(
                 101,'v2-post-cutover-b2c','b2c','v2-policy-b2c',1,'policy-b2c-v2',
                 'balance',1,NULL,NULL,'post-cutover-target-assignment',101,
                 'release-target-v2',1,102,'release-recovery-v2','post-cutover-group',
                 'post-cutover-target-extension',501
             );
             SET CONSTRAINTS pricing_release_assignment_extension_v2_pair IMMEDIATE;",
        )
        .expect_err("an active-only post-cutover extension must not strand recovery");
    assert!(incomplete_extension
        .as_db_error()
        .is_some_and(|error| error.message().contains("extension pair is incomplete")));
    pg.client
        .batch_execute(
            "ROLLBACK TO SAVEPOINT incomplete_assignment_extension;
             RELEASE SAVEPOINT incomplete_assignment_extension;
             SET CONSTRAINTS ALL DEFERRED;",
        )
        .unwrap();

    pg.client
        .batch_execute(
            "INSERT INTO pricing_release_assignment_extensions_v2(
                 release_generation,account_id,account_class,policy_id,policy_version,
                 policy_digest,billing_mode,funding_generation,purpose,responsible,
                 assignment_digest,provisioning_head_generation,provisioning_head_digest,
                 provisioning_head_version,paired_recovery_generation,paired_recovery_digest,
                 extension_group_digest,extension_digest,created_ts
             ) VALUES
                 (101,'v2-post-cutover-b2c','b2c','v2-policy-b2c',1,'policy-b2c-v2',
                  'balance',1,NULL,NULL,'post-cutover-target-assignment',101,
                  'release-target-v2',1,102,'release-recovery-v2','post-cutover-group',
                  'post-cutover-target-extension',501),
                 (102,'v2-post-cutover-b2c','b2c','v2-policy-b2c',1,'policy-b2c-v2',
                  'balance',1,NULL,NULL,'post-cutover-recovery-assignment',101,
                  'release-target-v2',1,102,'release-recovery-v2','post-cutover-group',
                  'post-cutover-recovery-extension',501);
             SET CONSTRAINTS ALL IMMEDIATE;",
        )
        .unwrap();

    pg.client
        .batch_execute("SAVEPOINT immutable_assignment_extension")
        .unwrap();
    let immutable_extension = pg
        .client
        .batch_execute(
            "UPDATE pricing_release_assignment_extensions_v2
                SET assignment_digest='changed'
              WHERE release_generation=101 AND account_id='v2-post-cutover-b2c';",
        )
        .expect_err("post-cutover assignment extensions must be immutable");
    assert!(immutable_extension
        .as_db_error()
        .is_some_and(|error| error.message().contains("immutable pricing v2 authority")));
    pg.client
        .batch_execute(
            "ROLLBACK TO SAVEPOINT immutable_assignment_extension;
             RELEASE SAVEPOINT immutable_assignment_extension;",
        )
        .unwrap();

    pg.client
        .batch_execute("SAVEPOINT stale_runtime_claim")
        .unwrap();
    let stale_runtime_claim = pg
        .client
        .batch_execute(
            "UPDATE engine_instances
                SET owner_epoch=2,lease_until=1100,started_ts=500,updated_ts=500
              WHERE instance_id='v2-epoch-fence';",
        )
        .expect_err("an active release accepted a runtime without an epoch-bound v2 claim");
    assert!(stale_runtime_claim
        .as_db_error()
        .is_some_and(|error| { error.message().contains("owner-epoch-bound runtime claim") }));
    pg.client
        .batch_execute(
            "ROLLBACK TO SAVEPOINT stale_runtime_claim;
             RELEASE SAVEPOINT stale_runtime_claim;",
        )
        .unwrap();

    assert_eq!(
        pg.client
            .execute(
                "UPDATE engine_instances
                    SET owner_epoch=2,lease_until=1100,started_ts=500,updated_ts=500,
                        pricing_release_schema_version=2,funding_schema_version=2,
                        pricing_release_runtime_digest='runtime-v2-epoch-fence',
                        pricing_release_claim_epoch=2
                  WHERE instance_id='v2-epoch-fence'",
                &[],
            )
            .unwrap(),
        1
    );
    assert_eq!(
        pg.client
            .execute(
                "UPDATE engine_instances SET lease_until=1200,updated_ts=501
                  WHERE instance_id='v2-epoch-fence' AND owner_epoch=2",
                &[],
            )
            .unwrap(),
        1
    );

    pg.client
        .batch_execute("SAVEPOINT inherited_runtime_claim")
        .unwrap();
    let inherited_runtime_claim = pg
        .client
        .batch_execute(
            "UPDATE engine_instances
                SET owner_epoch=3,lease_until=1300,started_ts=502,updated_ts=502
              WHERE instance_id='v2-epoch-fence';",
        )
        .expect_err("a new owner epoch inherited the previous runtime's v2 claim");
    assert!(inherited_runtime_claim
        .as_db_error()
        .is_some_and(|error| { error.message().contains("owner-epoch-bound runtime claim") }));
    pg.client
        .batch_execute(
            "ROLLBACK TO SAVEPOINT inherited_runtime_claim;
             RELEASE SAVEPOINT inherited_runtime_claim;",
        )
        .unwrap();

    pg.client.batch_execute("SAVEPOINT service_rule").unwrap();
    let service_rule = pg
        .client
        .batch_execute(
            "INSERT INTO pricing_release_policy_rules(
                 policy_id,policy_version,rule_id,rule_digest,scope_type,provider_id,
                 canonical_model_id,discount_bps,payable_multiplier_bp
             ) VALUES(
                 'v2-policy-service',1,'invalid-service-rule','invalid-service-rule',
                 'global',NULL,NULL,10000,0
             );",
        )
        .expect_err("service policy must not accept discount rules");
    assert!(service_rule.as_db_error().is_some_and(|error| {
        error
            .message()
            .contains("forbidden for meter-only service policies")
    }));
    pg.client
        .batch_execute("ROLLBACK TO SAVEPOINT service_rule; RELEASE SAVEPOINT service_rule")
        .unwrap();

    pg.client.batch_execute("SAVEPOINT service_charge").unwrap();
    let service_charge = pg
        .client
        .batch_execute(
            "UPDATE usage_events SET charge_nano=1
             WHERE request_id='v2-request-service';",
        )
        .expect_err("meter-only usage must not carry a customer charge");
    assert!(service_charge
        .as_db_error()
        .is_some_and(|error| { error.message().contains("cannot carry customer charge") }));
    pg.client
        .batch_execute("ROLLBACK TO SAVEPOINT service_charge; RELEASE SAVEPOINT service_charge")
        .unwrap();

    pg.client
        .batch_execute("SAVEPOINT immutable_policy")
        .unwrap();
    let immutable = pg
        .client
        .batch_execute(
            "UPDATE pricing_release_policy_versions
             SET owner_id='changed' WHERE policy_id='v2-policy-b2c';",
        )
        .expect_err("prepared pricing policy must be immutable");
    assert!(immutable
        .as_db_error()
        .is_some_and(|error| { error.message().contains("immutable pricing v2 authority") }));
    pg.client
        .batch_execute("ROLLBACK TO SAVEPOINT immutable_policy; RELEASE SAVEPOINT immutable_policy")
        .unwrap();

    pg.client.batch_execute("ROLLBACK").unwrap();
    lock_holder
        .client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}

/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::pricing_release_runtime_v2_postgres_matrix`
#[test]
fn pricing_release_runtime_v2_postgres_matrix() {
    use crate::pricing::{
        AccountClass, BillingModeV2, PricingMutation, PricingRejection,
        PricingReleaseActivationKindV2, PricingReleaseAssignmentExtensionMemberV2,
        PricingReleaseAssignmentExtensionV2, PricingReleaseAssignmentV2, PricingReleaseKindV2,
        PricingReleasePolicyRuleV2, PricingReleasePolicyV2, PricingReleaseRecoveryLinkV2,
        PricingReleaseReserveOutcomeV2, PricingReleaseRuleScopeV2, PricingReleaseV2,
    };

    const TARGET_GENERATION: i64 = 91_001;
    const RECOVERY_GENERATION: i64 = 91_002;
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping pricing release runtime v2 PostgreSQL matrix: test URL is unset");
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
            "TRUNCATE pricing_release_policy_versions,pricing_release_versions,
             account_policy_bindings,account_policy_rules,account_policy_versions,
             provider_switch_head,provider_switch_entries,provider_switch_versions,
             pricing_catalog_heads,pricing_catalog_entries,pricing_catalog_versions,
             execution_group_winner,settlement_outbox,reservations,capacity_leases,
             leader_leases,engine_instances,usage_events,ledger,api_keys,accounts,pool_state,
             subs RESTART IDENTITY CASCADE",
        )
        .unwrap();

    let owner = pg
        .claim_instance_with_pricing_manifest("release-runtime-v2", 600, &stage8_pg_manifest())
        .unwrap();
    pg.account_create("release-runtime-b2c", None, 5_000)
        .unwrap();
    pg.account_topup("release-runtime-b2c", 2_000, Some("runtime-seed"))
        .unwrap();
    pg.key_issue("release-runtime-b2c-key", "release-runtime-b2c", None)
        .unwrap();
    pg.account_create("release-runtime-service", Some("crm-parsing"), 10_000)
        .unwrap();
    pg.account_create("release-runtime-openkeys", None, 10_000)
        .unwrap();
    pg.account_topup(
        "release-runtime-openkeys",
        3_000,
        Some("openkeys-runtime-seed"),
    )
    .unwrap();
    pg.key_issue(
        "release-runtime-service-key",
        "release-runtime-service",
        None,
    )
    .unwrap();
    pg.account_create("release-runtime-b2c-convert", None, 5_000)
        .unwrap();
    pg.account_topup(
        "release-runtime-b2c-convert",
        1_500,
        Some("convert-runtime-seed"),
    )
    .unwrap();
    pg.account_create("release-runtime-b2b", None, 10_000)
        .unwrap();
    pg.account_topup("release-runtime-b2b", 4_000, Some("b2b-runtime-seed"))
        .unwrap();
    pg.account_create("release-runtime-b2c-funding", None, 5_000)
        .unwrap();
    pg.account_topup(
        "release-runtime-b2c-funding",
        900,
        Some("funding-runtime-seed"),
    )
    .unwrap();

    for catalog in [stage8_pg_catalog("main"), stage8_pg_catalog("openkeys")] {
        assert_eq!(
            pg.prepare_pricing_catalog(&catalog).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            pg.activate_pricing_catalog(
                &catalog.product_id,
                &catalog.target(),
                &crate::pricing::ActiveExpectation::Absent,
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
        pg.activate_provider_switches(
            &switches.target(),
            &crate::pricing::ActiveExpectation::Absent,
        )
        .unwrap(),
        PricingMutation::Applied
    );

    pg.client
        .batch_execute(
            "BEGIN;
             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES(
                 'release-runtime-b2c',1,2,'runtime-source','runtime-normalization',
                 2000,0,0,1,100,100
             );
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES(
                 'release-runtime-paid','release-runtime-b2c',1,'paid','runtime-seed',
                 2000,0,0,1,'active',100,100
             );
             INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts
             ) VALUES('release-runtime-b2c',1,1,100);
             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES(
                 'release-runtime-openkeys',1,2,'runtime-source','runtime-normalization',
                 3000,0,0,1,100,100
             );
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES(
                 'release-runtime-openkeys-paid','release-runtime-openkeys',1,
                 'paid','runtime-seed',3000,0,0,1,'active',100,100
             );
             INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts
             ) VALUES('release-runtime-openkeys',1,1,100);
             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES(
                 'release-runtime-b2c-convert',1,2,'runtime-source','runtime-normalization',
                 1500,0,0,1,100,100
             );
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES(
                 'release-runtime-convert-paid','release-runtime-b2c-convert',1,
                 'paid','convert-runtime-seed',1500,0,0,1,'active',100,100
             );
             INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts
             ) VALUES('release-runtime-b2c-convert',1,1,100);
             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES(
                 'release-runtime-b2b',1,2,'runtime-source','runtime-normalization',
                 4000,0,0,1,100,100
             );
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES(
                 'release-runtime-b2b-paid','release-runtime-b2b',1,
                 'paid','b2b-runtime-seed',4000,0,0,1,'active',100,100
             );
             INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts
             ) VALUES('release-runtime-b2b',1,1,100);
             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES(
                 'release-runtime-b2c-funding',1,2,'runtime-source','runtime-normalization',
                 900,0,0,1,100,100
             );
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES(
                 'release-runtime-funding-paid','release-runtime-b2c-funding',1,
                 'paid','funding-runtime-seed',900,0,0,1,'active',100,100
             );
             INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts
             ) VALUES('release-runtime-b2c-funding',1,1,100);
             SET CONSTRAINTS ALL IMMEDIATE;
             COMMIT;",
        )
        .unwrap();

    let b2c_policy = PricingReleasePolicyV2 {
        policy_id: "release-runtime-b2c-policy".into(),
        policy_version: 1,
        owner_type: crate::pricing::PolicyOwnerType::GlobalB2c,
        owner_id: "global".into(),
        account_class: crate::pricing::AccountClass::B2c,
        product_id: Some("main".into()),
        billing_mode: BillingModeV2::Balance,
        schema_version: 2,
        capability_generation: 1,
        capability_digest: "stage8-capability-1".into(),
        catalog_generation: Some(1),
        catalog_digest: Some("stage8-main-catalog-1".into()),
        switch_generation: Some(1),
        switch_digest: Some("stage8-switches-1".into()),
        content_digest: "release-runtime-b2c-policy-digest".into(),
        rules: vec![PricingReleasePolicyRuleV2 {
            rule_id: "release-runtime-global-50".into(),
            rule_digest: "release-runtime-global-50-digest".into(),
            scope: PricingReleaseRuleScopeV2::Global,
            discount_bps: 5_000,
            payable_multiplier_bp: 5_000,
        }],
    };
    let service_policy = PricingReleasePolicyV2 {
        policy_id: "release-runtime-service-policy".into(),
        policy_version: 1,
        owner_type: crate::pricing::PolicyOwnerType::Service,
        owner_id: "service-inventory-opaque-id".into(),
        account_class: crate::pricing::AccountClass::Service,
        product_id: None,
        billing_mode: BillingModeV2::MeterOnly,
        schema_version: 2,
        capability_generation: 1,
        capability_digest: "stage8-capability-1".into(),
        catalog_generation: None,
        catalog_digest: None,
        switch_generation: None,
        switch_digest: None,
        content_digest: "release-runtime-service-policy-digest".into(),
        rules: Vec::new(),
    };
    let openkeys_policy = PricingReleasePolicyV2 {
        policy_id: "release-runtime-openkeys-policy".into(),
        policy_version: 1,
        owner_type: crate::pricing::PolicyOwnerType::OpenKeys,
        owner_id: "openkeys".into(),
        account_class: crate::pricing::AccountClass::OpenKeys,
        product_id: Some("openkeys".into()),
        billing_mode: BillingModeV2::Balance,
        schema_version: 2,
        capability_generation: 1,
        capability_digest: "stage8-capability-1".into(),
        catalog_generation: Some(1),
        catalog_digest: Some("stage8-openkeys-catalog-1".into()),
        switch_generation: Some(1),
        switch_digest: Some("stage8-switches-1".into()),
        content_digest: "release-runtime-openkeys-policy-digest".into(),
        rules: vec![PricingReleasePolicyRuleV2 {
            rule_id: "release-runtime-openkeys-one-to-one".into(),
            rule_digest: "release-runtime-openkeys-one-to-one-digest".into(),
            scope: PricingReleaseRuleScopeV2::Global,
            discount_bps: 0,
            payable_multiplier_bp: 10_000,
        }],
    };
    let b2b_policy = PricingReleasePolicyV2 {
        policy_id: "release-runtime-b2b-policy".into(),
        policy_version: 1,
        owner_type: crate::pricing::PolicyOwnerType::B2bClient,
        owner_id: "release-runtime-b2b".into(),
        account_class: crate::pricing::AccountClass::B2b,
        product_id: Some("main".into()),
        billing_mode: BillingModeV2::Balance,
        schema_version: 2,
        capability_generation: 1,
        capability_digest: "stage8-capability-1".into(),
        catalog_generation: Some(1),
        catalog_digest: Some("stage8-main-catalog-1".into()),
        switch_generation: Some(1),
        switch_digest: Some("stage8-switches-1".into()),
        content_digest: "release-runtime-b2b-policy-digest".into(),
        rules: vec![PricingReleasePolicyRuleV2 {
            rule_id: "release-runtime-b2b-google".into(),
            rule_digest: "release-runtime-b2b-google-digest".into(),
            scope: PricingReleaseRuleScopeV2::Provider {
                provider_id: crate::PROVIDER_GOOGLE.into(),
            },
            discount_bps: 2_000,
            payable_multiplier_bp: 8_000,
        }],
    };
    let b2b_convert_policy = PricingReleasePolicyV2 {
        policy_id: "release-runtime-b2b-convert-policy".into(),
        owner_id: "release-runtime-b2c-convert".into(),
        content_digest: "release-runtime-b2b-convert-policy-digest".into(),
        rules: vec![PricingReleasePolicyRuleV2 {
            rule_id: "release-runtime-b2b-convert-google".into(),
            rule_digest: "release-runtime-b2b-convert-google-digest".into(),
            scope: PricingReleaseRuleScopeV2::Provider {
                provider_id: crate::PROVIDER_GOOGLE.into(),
            },
            discount_bps: 2_500,
            payable_multiplier_bp: 7_500,
        }],
        ..b2b_policy.clone()
    };
    for policy in [
        &b2c_policy,
        &service_policy,
        &openkeys_policy,
        &b2b_policy,
        &b2b_convert_policy,
    ] {
        assert_eq!(
            pg.prepare_pricing_release_policy_v2(policy).unwrap(),
            PricingMutation::Stored
        );
    }

    let assignments = vec![
        PricingReleaseAssignmentV2 {
            account_id: "release-runtime-b2c".into(),
            account_class: crate::pricing::AccountClass::B2c,
            policy_id: b2c_policy.policy_id.clone(),
            policy_version: 1,
            policy_digest: b2c_policy.content_digest.clone(),
            billing_mode: BillingModeV2::Balance,
            funding_generation: Some(1),
            purpose: None,
            responsible: None,
            assignment_digest: "release-runtime-b2c-assignment".into(),
        },
        PricingReleaseAssignmentV2 {
            account_id: "release-runtime-service".into(),
            account_class: crate::pricing::AccountClass::Service,
            policy_id: service_policy.policy_id.clone(),
            policy_version: 1,
            policy_digest: service_policy.content_digest.clone(),
            billing_mode: BillingModeV2::MeterOnly,
            funding_generation: None,
            purpose: Some("internal-runtime".into()),
            responsible: Some("runtime-team".into()),
            assignment_digest: "release-runtime-service-assignment".into(),
        },
        PricingReleaseAssignmentV2 {
            account_id: "release-runtime-openkeys".into(),
            account_class: crate::pricing::AccountClass::OpenKeys,
            policy_id: openkeys_policy.policy_id.clone(),
            policy_version: 1,
            policy_digest: openkeys_policy.content_digest.clone(),
            billing_mode: BillingModeV2::Balance,
            funding_generation: Some(1),
            purpose: None,
            responsible: None,
            assignment_digest: "release-runtime-openkeys-assignment".into(),
        },
        PricingReleaseAssignmentV2 {
            account_id: "release-runtime-b2c-convert".into(),
            account_class: crate::pricing::AccountClass::B2c,
            policy_id: b2c_policy.policy_id.clone(),
            policy_version: 1,
            policy_digest: b2c_policy.content_digest.clone(),
            billing_mode: BillingModeV2::Balance,
            funding_generation: Some(1),
            purpose: None,
            responsible: None,
            assignment_digest: "release-runtime-b2c-convert-assignment".into(),
        },
        PricingReleaseAssignmentV2 {
            account_id: "release-runtime-b2b".into(),
            account_class: crate::pricing::AccountClass::B2b,
            policy_id: b2b_policy.policy_id.clone(),
            policy_version: 1,
            policy_digest: b2b_policy.content_digest.clone(),
            billing_mode: BillingModeV2::Balance,
            funding_generation: Some(1),
            purpose: None,
            responsible: None,
            assignment_digest: "release-runtime-b2b-assignment".into(),
        },
        PricingReleaseAssignmentV2 {
            account_id: "release-runtime-b2c-funding".into(),
            account_class: crate::pricing::AccountClass::B2c,
            policy_id: b2c_policy.policy_id.clone(),
            policy_version: 1,
            policy_digest: b2c_policy.content_digest.clone(),
            billing_mode: BillingModeV2::Balance,
            funding_generation: Some(1),
            purpose: None,
            responsible: None,
            assignment_digest: "release-runtime-b2c-funding-assignment".into(),
        },
    ];
    let release = |generation, release_kind, digest: &str| PricingReleaseV2 {
        generation,
        release_kind,
        schema_version: 2,
        capability_generation: 1,
        capability_digest: "stage8-capability-1".into(),
        main_catalog_generation: 1,
        main_catalog_digest: "stage8-main-catalog-1".into(),
        openkeys_catalog_generation: 1,
        openkeys_catalog_digest: "stage8-openkeys-catalog-1".into(),
        switch_generation: 1,
        switch_digest: "stage8-switches-1".into(),
        inventory_digest: "release-runtime-inventory".into(),
        policy_manifest_digest: format!("release-runtime-policies-{generation}"),
        assignment_manifest_digest: format!("release-runtime-assignments-{generation}"),
        funding_manifest_digest: "release-runtime-funding".into(),
        minimum_runtime_schema_version: 2,
        content_digest: digest.into(),
        assignments: assignments.clone(),
    };
    let target = release(
        TARGET_GENERATION,
        PricingReleaseKindV2::Target,
        "release-runtime-target",
    );
    let recovery = release(
        RECOVERY_GENERATION,
        PricingReleaseKindV2::Recovery,
        "release-runtime-recovery",
    );
    assert_eq!(
        pg.prepare_pricing_release_v2(&target).unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        pg.prepare_pricing_release_v2(&recovery).unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        pg.prepare_pricing_release_recovery_link_v2(&PricingReleaseRecoveryLinkV2 {
            target_generation: TARGET_GENERATION,
            target_digest: target.content_digest.clone(),
            recovery_generation: RECOVERY_GENERATION,
            recovery_digest: recovery.content_digest.clone(),
            link_digest: "release-runtime-recovery-link".into(),
        })
        .unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(pg.pricing_release_provisioning_context_v2().unwrap(), None);

    let activated_ts = now();
    pg.client
        .execute(
            "INSERT INTO pricing_stage8_evidence_v2(
                 evidence_digest,target_generation,target_digest,recovery_generation,
                 recovery_digest,inventory_digest,funding_digest,shadow_digest,
                 runtime_floor_digest,legacy_inflight_count,blocker_count,passed,
                 observed_ts,valid_until_ts
             ) VALUES(
                 'release-runtime-evidence',$1,$2,$3,$4,'release-runtime-inventory',
                 'release-runtime-funding','release-runtime-shadow','release-runtime-floor',
                 0,0,true,$5,$6
             )",
            &[
                &TARGET_GENERATION,
                &target.content_digest,
                &RECOVERY_GENERATION,
                &recovery.content_digest,
                &activated_ts,
                &activated_ts.saturating_add(600),
            ],
        )
        .unwrap();
    pg.client
        .batch_execute(&format!(
            "BEGIN;
             INSERT INTO pricing_release_activations_v2(
                 activation_kind,from_generation,from_digest,to_generation,to_digest,
                 expected_head_version,resulting_head_version,evidence_digest,operator_id,
                 reason,activated_ts
             ) VALUES(
                 'cutover',NULL,NULL,{TARGET_GENERATION},'release-runtime-target',0,1,
                 'release-runtime-evidence','runtime-test','runtime matrix',{activated_ts}
             );
             INSERT INTO pricing_release_head_v2(
                 singleton,active_generation,active_digest,head_version,updated_ts
             ) VALUES(1,{TARGET_GENERATION},'release-runtime-target',1,{activated_ts});
             SET CONSTRAINTS ALL IMMEDIATE;
             COMMIT;"
        ))
        .unwrap();
    let cutover_context = pg
        .pricing_release_provisioning_context_v2()
        .unwrap()
        .expect("cutover head has a coherent provisioning context");
    assert_eq!(cutover_context.head.active_generation, TARGET_GENERATION);
    assert_eq!(
        cutover_context.activation.activation_kind,
        PricingReleaseActivationKindV2::Cutover
    );
    assert_eq!(
        cutover_context.active_release.release_kind,
        PricingReleaseKindV2::Target
    );
    let cutover_recovery = cutover_context
        .paired_recovery
        .expect("active target exposes its evidence-paired recovery");
    assert_eq!(cutover_recovery.release.generation, RECOVERY_GENERATION);
    assert_eq!(
        cutover_recovery.recovery_link.link_digest,
        "release-runtime-recovery-link"
    );

    let image_smoke_credential = pg.openai_image_smoke_credential().unwrap();
    assert_eq!(image_smoke_credential.account_id, "release-runtime-service");
    assert_eq!(
        image_smoke_credential.authorization_key(),
        "release-runtime-service-key"
    );
    assert_eq!(image_smoke_credential.purpose, "internal-runtime");
    assert_eq!(image_smoke_credential.responsible, "runtime-team");
    pg.key_issue(
        "release-runtime-service-second-key",
        "release-runtime-service",
        None,
    )
    .unwrap();
    let ambiguous_error = match pg.openai_image_smoke_credential() {
        Ok(_) => panic!("multiple active service keys must fail closed"),
        Err(error) => error,
    };
    assert!(ambiguous_error
        .to_string()
        .contains("credential is ambiguous (2 candidates)"));
    assert_eq!(
        pg.key_set_status("release-runtime-service-second-key", "inactive")
            .unwrap(),
        1
    );
    assert_eq!(
        pg.openai_image_smoke_credential().unwrap().account_id,
        "release-runtime-service"
    );

    pg.account_create("release-runtime-post-cutover", None, 5_000)
        .unwrap();
    pg.account_topup(
        "release-runtime-post-cutover",
        1_200,
        Some("post-cutover-runtime-seed"),
    )
    .unwrap();
    pg.client
        .batch_execute(
            "BEGIN;
             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES(
                 'release-runtime-post-cutover',1,2,'post-cutover-runtime-source',
                 'post-cutover-runtime-normalization',1200,0,0,1,100,100
             );
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES(
                 'release-runtime-post-cutover-paid','release-runtime-post-cutover',1,
                 'paid','post-cutover-runtime-seed',1200,0,0,1,'active',100,100
             );
             INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts
             ) VALUES('release-runtime-post-cutover',1,1,100);
             SET CONSTRAINTS ALL IMMEDIATE;
             COMMIT;",
        )
        .unwrap();

    let post_cutover_assignment = |assignment_digest: &str| PricingReleaseAssignmentV2 {
        account_id: "release-runtime-post-cutover".into(),
        account_class: AccountClass::B2c,
        policy_id: b2c_policy.policy_id.clone(),
        policy_version: b2c_policy.policy_version,
        policy_digest: b2c_policy.content_digest.clone(),
        billing_mode: BillingModeV2::Balance,
        funding_generation: Some(1),
        purpose: None,
        responsible: None,
        assignment_digest: assignment_digest.into(),
    };
    let extension = PricingReleaseAssignmentExtensionV2 {
        provisioning_head_generation: TARGET_GENERATION,
        provisioning_head_digest: target.content_digest.clone(),
        provisioning_head_version: 1,
        paired_recovery_generation: Some(RECOVERY_GENERATION),
        paired_recovery_digest: Some(recovery.content_digest.clone()),
        extension_group_digest: "release-runtime-post-cutover-extension-group".into(),
        members: vec![
            PricingReleaseAssignmentExtensionMemberV2 {
                release_generation: TARGET_GENERATION,
                assignment: post_cutover_assignment(
                    "release-runtime-post-cutover-target-assignment",
                ),
                extension_digest: "release-runtime-post-cutover-target-extension".into(),
            },
            PricingReleaseAssignmentExtensionMemberV2 {
                release_generation: RECOVERY_GENERATION,
                assignment: post_cutover_assignment(
                    "release-runtime-post-cutover-recovery-assignment",
                ),
                extension_digest: "release-runtime-post-cutover-recovery-extension".into(),
            },
        ],
    };
    let mut unpaired_extension = extension.clone();
    unpaired_extension.paired_recovery_generation = None;
    unpaired_extension.paired_recovery_digest = None;
    unpaired_extension.members.truncate(1);
    unpaired_extension.extension_group_digest =
        "release-runtime-post-cutover-unpaired-extension-group".into();
    assert!(matches!(
        pg.prepare_pricing_release_assignment_extension_v2(&unpaired_extension)
            .unwrap(),
        PricingMutation::Rejected(PricingRejection::MissingDependency { dependency })
            if dependency == "recovery_link"
    ));
    let mut stale_funding_extension = extension.clone();
    for member in &mut stale_funding_extension.members {
        member.assignment.funding_generation = Some(2);
    }
    assert!(matches!(
        pg.prepare_pricing_release_assignment_extension_v2(&stale_funding_extension)
            .unwrap(),
        PricingMutation::Rejected(PricingRejection::MissingDependency { dependency })
            if dependency == "funding_head"
    ));
    assert_eq!(
        pg.prepare_pricing_release_assignment_extension_v2(&extension)
            .unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        pg.prepare_pricing_release_assignment_extension_v2(&extension)
            .unwrap(),
        PricingMutation::Unchanged
    );
    assert_eq!(
        pg.pricing_release_assignment_extension_v2(
            extension.provisioning_head_version,
            "release-runtime-post-cutover",
        )
        .unwrap(),
        Some(extension.clone())
    );
    let post_cutover_resolution = pg
        .pricing_release_resolution_v2(
            "release-runtime-post-cutover",
            crate::PROVIDER_GOOGLE,
            "gemini-3-flash-preview",
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        post_cutover_resolution.assignment.assignment_digest,
        "release-runtime-post-cutover-target-assignment"
    );
    assert_eq!(post_cutover_resolution.payable_multiplier_bp(), Some(5_000));

    let mut stale_extension = extension.clone();
    stale_extension.provisioning_head_version = 2;
    stale_extension.extension_group_digest =
        "release-runtime-post-cutover-stale-extension-group".into();
    assert!(matches!(
        pg.prepare_pricing_release_assignment_extension_v2(&stale_extension)
            .unwrap(),
        PricingMutation::Rejected(PricingRejection::Stale { .. })
    ));

    let mut conflicting_replay = extension.clone();
    conflicting_replay.extension_group_digest =
        "release-runtime-post-cutover-conflicting-extension-group".into();
    assert_eq!(
        pg.prepare_pricing_release_assignment_extension_v2(&conflicting_replay)
            .unwrap(),
        PricingMutation::Rejected(PricingRejection::VersionConflict)
    );

    let resolution = pg
        .pricing_release_resolution_v2(
            "release-runtime-b2c",
            crate::PROVIDER_GOOGLE,
            "gemini-3-flash-preview",
        )
        .unwrap()
        .unwrap();
    assert_eq!(resolution.payable_multiplier_bp(), Some(5_000));
    let admission_ts = now();
    let legacy_snapshot = crate::pricing::LegacyScalarAdmissionSnapshot::new(
        crate::pricing::LegacyScalarAdmissionSnapshotInput {
            request_id: "release-runtime-b2c-request".into(),
            account_id: "release-runtime-b2c".into(),
            provider: crate::pricing::SnapshotProvider::Google,
            requested_model_id: "gemini-3-flash-preview".into(),
            canonical_model_id: "gemini-3-flash-preview".into(),
            alias_generation: 1,
            tariff_schedule_id: "google/runtime-test/v1".into(),
            tariff_priced_ts: admission_ts,
            admission_ts,
            payable_multiplier_bp: 5_000,
            official_hold_nano: 200,
            charged_hold_nano: 100,
            premium_modifiers: crate::pricing::LegacyPremiumModifiers::GeminiV1 {
                context_rate: crate::pricing::SnapshotGeminiContextRate::ConservativeMaximum,
                search_billing: crate::pricing::SnapshotGeminiSearchBilling::PerQuery,
                grounding_enabled: false,
                search_reserve_units: 0,
            },
        },
    )
    .unwrap();
    let quote =
        crate::pricing::PricingReleaseQuoteV2::from_legacy_snapshot(&legacy_snapshot).unwrap();
    let receipt = match pg
        .reserve_request_with_pricing_release_v2(
            &owner,
            "release-runtime-b2c-key",
            600,
            &resolution,
            &quote,
        )
        .unwrap()
    {
        PricingReleaseReserveOutcomeV2::Inserted(receipt) => receipt,
        other => panic!("unexpected release-v2 reserve outcome: {other:?}"),
    };
    assert_eq!(receipt.snapshot.charged_hold_nano, 100);
    assert!(matches!(
        pg.reserve_request_with_pricing_release_v2(
            &owner,
            "release-runtime-b2c-key",
            600,
            &resolution,
            &quote,
        )
        .unwrap(),
        PricingReleaseReserveOutcomeV2::Unchanged(_)
    ));

    let usage = UsageEventInput {
        model: "gemini-3-flash-preview".into(),
        provider: crate::PROVIDER_GOOGLE.into(),
        input_tokens: 8,
        output_tokens: 4,
        real_nano: 80,
        charge_basis_nano: 80,
        input_nano: 40,
        output_nano: 40,
        priced_ts: admission_ts,
        ..UsageEventInput::default()
    };
    assert_eq!(
        pg.settle_request(
            "release-runtime-b2c-request",
            40,
            Some("runtime-settle"),
            Some(&usage),
        )
        .unwrap(),
        Some(1_960)
    );
    assert_eq!(
        pg.settle_request(
            "release-runtime-b2c-request",
            40,
            Some("runtime-settle"),
            Some(&usage),
        )
        .unwrap(),
        Some(1_960)
    );
    let b2c = pg
        .client
        .query_one(
            "SELECT account.balance_nano,account.reserved_nano,account.spent_nano,
                    event.charge_nano,event.real_nano,allocation.charged_nano,
                    allocation.released_nano
               FROM accounts account
               JOIN usage_events event ON event.account_id=account.id
               JOIN pricing_request_funding_allocations_v2 allocation
                 ON allocation.request_id=event.request_id
              WHERE account.id='release-runtime-b2c'",
            &[],
        )
        .unwrap();
    assert_eq!(
        (
            b2c.get::<_, i64>(0),
            b2c.get::<_, i64>(1),
            b2c.get::<_, i64>(2),
            b2c.get::<_, i64>(3),
            b2c.get::<_, i64>(4),
            b2c.get::<_, Option<i64>>(5),
            b2c.get::<_, Option<i64>>(6),
        ),
        (1_960, 0, 40, 40, 80, Some(40), Some(60))
    );

    let service_resolution = pg
        .pricing_release_resolution_v2(
            "release-runtime-service",
            crate::PROVIDER_GOOGLE,
            "runtime-only-future-model",
        )
        .unwrap()
        .unwrap();
    assert_eq!(service_resolution.billing_mode(), BillingModeV2::MeterOnly);
    assert_eq!(service_resolution.payable_multiplier_bp(), None);
    let service_snapshot = crate::pricing::LegacyScalarAdmissionSnapshot::new(
        crate::pricing::LegacyScalarAdmissionSnapshotInput {
            request_id: "release-runtime-service-request".into(),
            account_id: "release-runtime-service".into(),
            provider: crate::pricing::SnapshotProvider::Google,
            requested_model_id: "runtime-only-future-model".into(),
            canonical_model_id: "runtime-only-future-model".into(),
            alias_generation: 1,
            tariff_schedule_id: "google/runtime-service/v1".into(),
            tariff_priced_ts: admission_ts,
            admission_ts,
            payable_multiplier_bp: 10_000,
            official_hold_nano: 500,
            charged_hold_nano: 500,
            premium_modifiers: crate::pricing::LegacyPremiumModifiers::GeminiV1 {
                context_rate: crate::pricing::SnapshotGeminiContextRate::ConservativeMaximum,
                search_billing: crate::pricing::SnapshotGeminiSearchBilling::PerQuery,
                grounding_enabled: false,
                search_reserve_units: 0,
            },
        },
    )
    .unwrap();
    let service_quote =
        crate::pricing::PricingReleaseQuoteV2::from_legacy_snapshot(&service_snapshot).unwrap();
    let service_receipt = match pg
        .reserve_request_with_pricing_release_v2(
            &owner,
            "release-runtime-service-key",
            600,
            &service_resolution,
            &service_quote,
        )
        .unwrap()
    {
        PricingReleaseReserveOutcomeV2::Inserted(receipt) => receipt,
        other => panic!("unexpected service release-v2 reserve outcome: {other:?}"),
    };
    assert_eq!(service_receipt.snapshot.charged_hold_nano, 0);
    assert_eq!(service_receipt.balance_after_reserve_nano, None);
    let pending_service = pg
        .openai_image_settlement_diagnostic("release-runtime-service-request")
        .unwrap();
    assert_eq!(pending_service.status, "outbox_missing");
    assert!(pending_service.reservation_present);
    assert!(pending_service.snapshot_present);
    assert!(!pending_service.outbox_present);
    assert!(!pending_service.usage_present);
    let service_usage = UsageEventInput {
        model: "runtime-only-future-model".into(),
        provider: crate::PROVIDER_GOOGLE.into(),
        input_tokens: 10,
        real_nano: 500,
        charge_basis_nano: 500,
        input_nano: 500,
        priced_ts: admission_ts,
        ..UsageEventInput::default()
    };
    assert_eq!(
        pg.settle_request(
            "release-runtime-service-request",
            0,
            None,
            Some(&service_usage),
        )
        .unwrap(),
        Some(0)
    );
    let service = pg
        .client
        .query_one(
            "SELECT account.balance_nano,account.reserved_nano,account.spent_nano,
                    event.charge_nano,event.real_nano,
                    (SELECT count(*)::bigint FROM ledger
                      WHERE account_id='release-runtime-service')
               FROM accounts account
               JOIN usage_events event ON event.account_id=account.id
              WHERE account.id='release-runtime-service'",
            &[],
        )
        .unwrap();
    assert_eq!(
        (
            service.get::<_, i64>(0),
            service.get::<_, i64>(1),
            service.get::<_, i64>(2),
            service.get::<_, i64>(3),
            service.get::<_, i64>(4),
            service.get::<_, i64>(5),
        ),
        (0, 0, 0, 0, 500, 0)
    );
    let terminal_service = pg
        .openai_image_settlement_diagnostic("release-runtime-service-request")
        .unwrap();
    assert_eq!(terminal_service.status, "terminal_evidence_present");
    assert_eq!(
        terminal_service.reservation_state.as_deref(),
        Some("settled")
    );
    assert_eq!(terminal_service.outbox_state.as_deref(), Some("done"));
    assert!(terminal_service.usage_present);
    assert_eq!(terminal_service.real_nano, Some(500));
    assert_eq!(terminal_service.charge_nano, Some(0));
    assert!(terminal_service.account_present);
    assert!(terminal_service.key_present);

    let legacy_error = pg
        .reserve_request(
            &owner,
            "release-runtime-new-legacy-request",
            "release-runtime-b2c",
            "release-runtime-b2c-key",
            1,
            600,
        )
        .expect_err("active release accepted a new legacy-format reserve");
    assert!(legacy_error
        .downcast_ref::<crate::pricing::LegacyPricingPathClosedV2>()
        .is_some());

    let b2c_policy_override = PricingReleasePolicyV2 {
        policy_version: 2,
        content_digest: "release-runtime-b2c-policy-v2-digest".into(),
        rules: vec![
            PricingReleasePolicyRuleV2 {
                rule_id: "release-runtime-override-google".into(),
                rule_digest: "release-runtime-override-google-digest".into(),
                scope: PricingReleaseRuleScopeV2::Provider {
                    provider_id: crate::PROVIDER_GOOGLE.into(),
                },
                discount_bps: 6_000,
                payable_multiplier_bp: 4_000,
            },
            PricingReleasePolicyRuleV2 {
                rule_id: "release-runtime-global-50".into(),
                rule_digest: "release-runtime-global-50-digest".into(),
                scope: PricingReleaseRuleScopeV2::Global,
                discount_bps: 5_000,
                payable_multiplier_bp: 5_000,
            },
        ],
        ..b2c_policy.clone()
    };
    assert_eq!(
        pg.prepare_pricing_release_policy_v2(&b2c_policy_override)
            .unwrap(),
        PricingMutation::Stored
    );
    let override_assignment = |assignment_digest: &str| PricingReleaseAssignmentV2 {
        account_id: "release-runtime-b2c".into(),
        account_class: AccountClass::B2c,
        policy_id: b2c_policy_override.policy_id.clone(),
        policy_version: b2c_policy_override.policy_version,
        policy_digest: b2c_policy_override.content_digest.clone(),
        billing_mode: BillingModeV2::Balance,
        funding_generation: Some(1),
        purpose: None,
        responsible: None,
        assignment_digest: assignment_digest.into(),
    };
    let override_extension = PricingReleaseAssignmentExtensionV2 {
        provisioning_head_generation: TARGET_GENERATION,
        provisioning_head_digest: target.content_digest.clone(),
        provisioning_head_version: 1,
        paired_recovery_generation: Some(RECOVERY_GENERATION),
        paired_recovery_digest: Some(recovery.content_digest.clone()),
        extension_group_digest: "release-runtime-override-extension-group".into(),
        members: vec![
            PricingReleaseAssignmentExtensionMemberV2 {
                release_generation: TARGET_GENERATION,
                assignment: override_assignment("release-runtime-override-target-assignment"),
                extension_digest: "release-runtime-override-target-extension".into(),
            },
            PricingReleaseAssignmentExtensionMemberV2 {
                release_generation: RECOVERY_GENERATION,
                assignment: override_assignment("release-runtime-override-recovery-assignment"),
                extension_digest: "release-runtime-override-recovery-extension".into(),
            },
        ],
    };
    let mut downgrade_extension = override_extension.clone();
    for member in &mut downgrade_extension.members {
        member.assignment.policy_version = b2c_policy.policy_version;
        member.assignment.policy_digest = b2c_policy.content_digest.clone();
    }
    downgrade_extension.extension_group_digest = "release-runtime-override-downgrade-group".into();
    assert!(matches!(
        pg.prepare_pricing_release_assignment_extension_v2(&downgrade_extension)
            .unwrap(),
        PricingMutation::Rejected(PricingRejection::MissingDependency { dependency })
            if dependency.starts_with("assignment:")
    ));
    assert_eq!(
        pg.prepare_pricing_release_assignment_extension_v2(&override_extension)
            .unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        pg.prepare_pricing_release_assignment_extension_v2(&override_extension)
            .unwrap(),
        PricingMutation::Unchanged
    );
    let overridden = pg
        .pricing_release_resolution_v2(
            "release-runtime-b2c",
            crate::PROVIDER_GOOGLE,
            "gemini-3-flash-preview",
        )
        .unwrap()
        .unwrap();
    assert_eq!(overridden.assignment.policy_version, 2);
    assert_eq!(overridden.payable_multiplier_bp(), Some(4_000));
    let override_snapshot = crate::pricing::LegacyScalarAdmissionSnapshot::new(
        crate::pricing::LegacyScalarAdmissionSnapshotInput {
            request_id: "release-runtime-b2c-request-override".into(),
            account_id: "release-runtime-b2c".into(),
            provider: crate::pricing::SnapshotProvider::Google,
            requested_model_id: "gemini-3-flash-preview".into(),
            canonical_model_id: "gemini-3-flash-preview".into(),
            alias_generation: 1,
            tariff_schedule_id: "google/runtime-test/v1".into(),
            tariff_priced_ts: admission_ts,
            admission_ts,
            payable_multiplier_bp: 4_000,
            official_hold_nano: 200,
            charged_hold_nano: 80,
            premium_modifiers: crate::pricing::LegacyPremiumModifiers::GeminiV1 {
                context_rate: crate::pricing::SnapshotGeminiContextRate::ConservativeMaximum,
                search_billing: crate::pricing::SnapshotGeminiSearchBilling::PerQuery,
                grounding_enabled: false,
                search_reserve_units: 0,
            },
        },
    )
    .unwrap();
    let override_quote =
        crate::pricing::PricingReleaseQuoteV2::from_legacy_snapshot(&override_snapshot).unwrap();
    let override_receipt = match pg
        .reserve_request_with_pricing_release_v2(
            &owner,
            "release-runtime-b2c-key",
            600,
            &overridden,
            &override_quote,
        )
        .unwrap()
    {
        PricingReleaseReserveOutcomeV2::Inserted(receipt) => receipt,
        other => panic!("unexpected override reserve outcome: {other:?}"),
    };
    assert_eq!(override_receipt.snapshot.charged_hold_nano, 80);
    assert_eq!(override_receipt.snapshot.policy_version, 2);

    // B2C -> B2B class-changing conversion: the only class transition an assignment
    // extension may perform on a base-covered account. The extension starts a new B2B
    // policy lineage, so the strictly-newer-version rule does not apply (the extension
    // policy_version 1 equals the base policy_version 1 under a different policy id).
    // Every rejected attempt below must leave no row behind, so the coherent pair at the
    // end can still be stored.
    let convert_assignment = |assignment_digest: &str| PricingReleaseAssignmentV2 {
        account_id: "release-runtime-b2c-convert".into(),
        account_class: AccountClass::B2b,
        policy_id: b2b_convert_policy.policy_id.clone(),
        policy_version: b2b_convert_policy.policy_version,
        policy_digest: b2b_convert_policy.content_digest.clone(),
        billing_mode: BillingModeV2::Balance,
        funding_generation: Some(1),
        purpose: None,
        responsible: None,
        assignment_digest: assignment_digest.into(),
    };
    let convert_extension = PricingReleaseAssignmentExtensionV2 {
        provisioning_head_generation: TARGET_GENERATION,
        provisioning_head_digest: target.content_digest.clone(),
        provisioning_head_version: 1,
        paired_recovery_generation: Some(RECOVERY_GENERATION),
        paired_recovery_digest: Some(recovery.content_digest.clone()),
        extension_group_digest: "release-runtime-convert-extension-group".into(),
        members: vec![
            PricingReleaseAssignmentExtensionMemberV2 {
                release_generation: TARGET_GENERATION,
                assignment: convert_assignment("release-runtime-convert-target-assignment"),
                extension_digest: "release-runtime-convert-target-extension".into(),
            },
            PricingReleaseAssignmentExtensionMemberV2 {
                release_generation: RECOVERY_GENERATION,
                assignment: convert_assignment("release-runtime-convert-recovery-assignment"),
                extension_digest: "release-runtime-convert-recovery-extension".into(),
            },
        ],
    };

    // A base B2B account cannot convert back to B2C.
    let b2b_back_assignment = |assignment_digest: &str| PricingReleaseAssignmentV2 {
        account_id: "release-runtime-b2b".into(),
        account_class: AccountClass::B2c,
        policy_id: b2c_policy.policy_id.clone(),
        policy_version: b2c_policy.policy_version,
        policy_digest: b2c_policy.content_digest.clone(),
        billing_mode: BillingModeV2::Balance,
        funding_generation: Some(1),
        purpose: None,
        responsible: None,
        assignment_digest: assignment_digest.into(),
    };
    let mut b2b_back_extension = convert_extension.clone();
    b2b_back_extension.extension_group_digest = "release-runtime-b2b-back-extension-group".into();
    b2b_back_extension.members = vec![
        PricingReleaseAssignmentExtensionMemberV2 {
            release_generation: TARGET_GENERATION,
            assignment: b2b_back_assignment("release-runtime-b2b-back-target-assignment"),
            extension_digest: "release-runtime-b2b-back-target-extension".into(),
        },
        PricingReleaseAssignmentExtensionMemberV2 {
            release_generation: RECOVERY_GENERATION,
            assignment: b2b_back_assignment("release-runtime-b2b-back-recovery-assignment"),
            extension_digest: "release-runtime-b2b-back-recovery-extension".into(),
        },
    ];
    assert!(matches!(
        pg.prepare_pricing_release_assignment_extension_v2(&b2b_back_extension)
            .unwrap(),
        PricingMutation::Rejected(PricingRejection::MissingDependency { dependency })
            if dependency.starts_with("assignment:")
    ));

    // A base B2C account cannot convert to OpenKeys.
    let mut openkeys_extension = convert_extension.clone();
    openkeys_extension.extension_group_digest = "release-runtime-openkeys-convert-group".into();
    for (member, suffix) in openkeys_extension
        .members
        .iter_mut()
        .zip(["target", "recovery"])
    {
        member.assignment.account_class = AccountClass::OpenKeys;
        member.assignment.policy_id = openkeys_policy.policy_id.clone();
        member.assignment.policy_version = openkeys_policy.policy_version;
        member.assignment.policy_digest = openkeys_policy.content_digest.clone();
        member.assignment.assignment_digest =
            format!("release-runtime-openkeys-convert-{suffix}-assignment");
        member.extension_digest = format!("release-runtime-openkeys-convert-{suffix}-extension");
    }
    assert!(matches!(
        pg.prepare_pricing_release_assignment_extension_v2(&openkeys_extension)
            .unwrap(),
        PricingMutation::Rejected(PricingRejection::MissingDependency { dependency })
            if dependency.starts_with("assignment:")
    ));

    // A base B2C account cannot convert to service.
    let mut service_extension = convert_extension.clone();
    service_extension.extension_group_digest = "release-runtime-service-convert-group".into();
    for (member, suffix) in service_extension
        .members
        .iter_mut()
        .zip(["target", "recovery"])
    {
        member.assignment.account_class = AccountClass::Service;
        member.assignment.policy_id = service_policy.policy_id.clone();
        member.assignment.policy_version = service_policy.policy_version;
        member.assignment.policy_digest = service_policy.content_digest.clone();
        member.assignment.billing_mode = BillingModeV2::MeterOnly;
        member.assignment.funding_generation = None;
        member.assignment.purpose = Some("converted-service".into());
        member.assignment.responsible = Some("runtime-team".into());
        member.assignment.assignment_digest =
            format!("release-runtime-service-convert-{suffix}-assignment");
        member.extension_digest = format!("release-runtime-service-convert-{suffix}-extension");
    }
    assert!(matches!(
        pg.prepare_pricing_release_assignment_extension_v2(&service_extension)
            .unwrap(),
        PricingMutation::Rejected(PricingRejection::MissingDependency { dependency })
            if dependency.starts_with("assignment:")
    ));

    // A billing-mode mismatch cannot even pass structural validation: a customer class
    // with meter_only is rejected before any dependency check.
    let mut metered_extension = convert_extension.clone();
    metered_extension.extension_group_digest = "release-runtime-metered-convert-group".into();
    for (member, suffix) in metered_extension
        .members
        .iter_mut()
        .zip(["target", "recovery"])
    {
        member.assignment.billing_mode = BillingModeV2::MeterOnly;
        member.assignment.funding_generation = None;
        member.assignment.assignment_digest =
            format!("release-runtime-metered-convert-{suffix}-assignment");
        member.extension_digest = format!("release-runtime-metered-convert-{suffix}-extension");
    }
    assert!(matches!(
        pg.prepare_pricing_release_assignment_extension_v2(&metered_extension)
            .unwrap(),
        PricingMutation::Rejected(PricingRejection::Invalid { .. })
    ));

    // Purpose/responsible metadata must stay identical to the base (null for balance
    // classes), so a non-null purpose is rejected before any dependency check.
    let mut purpose_extension = convert_extension.clone();
    purpose_extension.extension_group_digest = "release-runtime-purpose-convert-group".into();
    for (member, suffix) in purpose_extension
        .members
        .iter_mut()
        .zip(["target", "recovery"])
    {
        member.assignment.purpose = Some("ops".into());
        member.assignment.assignment_digest =
            format!("release-runtime-purpose-convert-{suffix}-assignment");
        member.extension_digest = format!("release-runtime-purpose-convert-{suffix}-extension");
    }
    assert!(matches!(
        pg.prepare_pricing_release_assignment_extension_v2(&purpose_extension)
            .unwrap(),
        PricingMutation::Rejected(PricingRejection::Invalid { .. })
    ));

    // A funding generation different from the base's stays rejected even when it is the
    // exact active funding head: the class change keeps the base funding generation.
    pg.client
        .batch_execute(
            "BEGIN;
             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES(
                 'release-runtime-b2c-funding',2,2,'runtime-reseed-source',
                 'runtime-reseed-normalization',900,0,0,1,200,200
             );
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES(
                 'release-runtime-funding-paid-2','release-runtime-b2c-funding',2,
                 'paid','funding-runtime-reseed',900,0,0,1,'active',200,200
             );
             UPDATE account_funding_head_v2
                SET active_generation=2,head_version=2,updated_ts=200
              WHERE account_id='release-runtime-b2c-funding';
             SET CONSTRAINTS ALL IMMEDIATE;
             COMMIT;",
        )
        .unwrap();
    let funding_convert_assignment = |assignment_digest: &str, funding_generation| {
        PricingReleaseAssignmentV2 {
            account_id: "release-runtime-b2c-funding".into(),
            funding_generation,
            assignment_digest: assignment_digest.into(),
            ..convert_assignment(assignment_digest)
        }
    };
    let funding_convert_extension = |group: &str, funding_generation| {
        PricingReleaseAssignmentExtensionV2 {
            extension_group_digest: group.into(),
            members: vec![
                PricingReleaseAssignmentExtensionMemberV2 {
                    release_generation: TARGET_GENERATION,
                    assignment: funding_convert_assignment(
                        &format!("release-runtime-funding-{group}-target-assignment"),
                        funding_generation,
                    ),
                    extension_digest: format!(
                        "release-runtime-funding-{group}-target-extension"
                    ),
                },
                PricingReleaseAssignmentExtensionMemberV2 {
                    release_generation: RECOVERY_GENERATION,
                    assignment: funding_convert_assignment(
                        &format!("release-runtime-funding-{group}-recovery-assignment"),
                        funding_generation,
                    ),
                    extension_digest: format!(
                        "release-runtime-funding-{group}-recovery-extension"
                    ),
                },
            ],
            ..convert_extension.clone()
        }
    };
    // The base generation is no longer the active funding head.
    assert!(matches!(
        pg.prepare_pricing_release_assignment_extension_v2(&funding_convert_extension(
            "stale-generation",
            Some(1),
        ))
        .unwrap(),
        PricingMutation::Rejected(PricingRejection::MissingDependency { dependency })
            if dependency == "funding_head"
    ));
    // The active funding head passes the head gate but differs from the base's.
    assert!(matches!(
        pg.prepare_pricing_release_assignment_extension_v2(&funding_convert_extension(
            "advanced-generation",
            Some(2),
        ))
        .unwrap(),
        PricingMutation::Rejected(PricingRejection::MissingDependency { dependency })
            if dependency.starts_with("assignment:")
    ));

    // The coherent b2c-base -> b2b-extension active/recovery pair is accepted, replays
    // exactly, reads back, and the resolver prefers the converted B2B lineage.
    assert_eq!(
        pg.prepare_pricing_release_assignment_extension_v2(&convert_extension)
            .unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        pg.prepare_pricing_release_assignment_extension_v2(&convert_extension)
            .unwrap(),
        PricingMutation::Unchanged
    );
    assert_eq!(
        pg.pricing_release_assignment_extension_v2(1, "release-runtime-b2c-convert")
            .unwrap(),
        Some(convert_extension.clone())
    );
    let converted = pg
        .pricing_release_resolution_v2(
            "release-runtime-b2c-convert",
            crate::PROVIDER_GOOGLE,
            "gemini-3-flash-preview",
        )
        .unwrap()
        .unwrap();
    assert_eq!(converted.assignment.account_class, AccountClass::B2b);
    assert_eq!(
        converted.assignment.policy_id,
        "release-runtime-b2b-convert-policy"
    );
    assert_eq!(converted.assignment.policy_version, 1);
    assert_eq!(converted.payable_multiplier_bp(), Some(7_500));

    let openkeys_google = pg
        .pricing_release_resolution_v2(
            "release-runtime-openkeys",
            crate::PROVIDER_GOOGLE,
            "gemini-3-flash-preview",
        )
        .unwrap()
        .unwrap();
    assert_eq!(openkeys_google.payable_multiplier_bp(), Some(10_000));
    assert_eq!(
        openkeys_google.assignment.policy_id,
        "release-runtime-openkeys-policy"
    );
    let openkeys_anthropic = pg
        .pricing_release_resolution_v2("release-runtime-openkeys", "anthropic", "claude-sonnet-5")
        .unwrap()
        .unwrap();
    assert_eq!(openkeys_anthropic.payable_multiplier_bp(), Some(10_000));

    let recovery_activated_ts = activated_ts.saturating_add(1);
    pg.client
        .batch_execute(&format!(
            "BEGIN;
             INSERT INTO pricing_release_activations_v2(
                 activation_kind,from_generation,from_digest,to_generation,to_digest,
                 expected_head_version,resulting_head_version,evidence_digest,operator_id,
                 reason,activated_ts
             ) VALUES(
                 'recovery',{TARGET_GENERATION},'release-runtime-target',
                 {RECOVERY_GENERATION},'release-runtime-recovery',1,2,
                 'release-runtime-evidence','runtime-test','runtime recovery',
                 {recovery_activated_ts}
             );
             UPDATE pricing_release_head_v2
                SET active_generation={RECOVERY_GENERATION},
                    active_digest='release-runtime-recovery',head_version=2,
                    updated_ts={recovery_activated_ts}
              WHERE singleton=1;
             SET CONSTRAINTS ALL IMMEDIATE;
             COMMIT;"
        ))
        .unwrap();
    let recovery_context = pg
        .pricing_release_provisioning_context_v2()
        .unwrap()
        .expect("recovery head has a coherent provisioning context");
    assert_eq!(recovery_context.head.active_generation, RECOVERY_GENERATION);
    assert_eq!(
        recovery_context.activation.activation_kind,
        PricingReleaseActivationKindV2::Recovery
    );
    assert_eq!(
        recovery_context.active_release.release_kind,
        PricingReleaseKindV2::Recovery
    );
    assert!(recovery_context.paired_recovery.is_none());

    pg.client
        .batch_execute(
            "TRUNCATE pricing_release_policy_versions,pricing_release_versions,
             account_policy_bindings,account_policy_rules,account_policy_versions,
             provider_switch_head,provider_switch_entries,provider_switch_versions,
             pricing_catalog_heads,pricing_catalog_entries,pricing_catalog_versions,
             execution_group_winner,settlement_outbox,reservations,capacity_leases,
             leader_leases,engine_instances,usage_events,ledger,api_keys,accounts,pool_state,
             subs RESTART IDENTITY CASCADE",
        )
        .unwrap();
    pg.client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}

/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::pricing_release_ledger_attribution_v2_postgres_matrix`
#[test]
fn pricing_release_ledger_attribution_v2_postgres_matrix() {
    use crate::pricing::{
        AccountClass, BillingModeV2, PricingMutation, PricingReleaseAssignmentV2,
        PricingReleaseKindV2, PricingReleasePolicyRuleV2, PricingReleasePolicyV2,
        PricingReleaseRecoveryLinkV2, PricingReleaseReserveOutcomeV2, PricingReleaseRuleScopeV2,
        PricingReleaseV2,
    };

    const TARGET_GENERATION: i64 = 93_001;
    const RECOVERY_GENERATION: i64 = 93_002;
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping release-v2 ledger attribution matrix: test URL is unset");
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
            "TRUNCATE pricing_release_policy_versions,pricing_release_versions,
             account_policy_bindings,account_policy_rules,account_policy_versions,
             provider_switch_head,provider_switch_entries,provider_switch_versions,
             pricing_catalog_heads,pricing_catalog_entries,pricing_catalog_versions,
             execution_group_winner,settlement_outbox,reservations,capacity_leases,
             leader_leases,engine_instances,usage_events,ledger,api_keys,accounts,pool_state,
             subs RESTART IDENTITY CASCADE",
        )
        .unwrap();

    let owner = pg
        .claim_instance_with_pricing_manifest("v2-ledger-engine", 600, &stage8_pg_manifest())
        .unwrap();
    pg.account_create("v2-ledger-b2c", None, 5_000).unwrap();
    pg.account_topup("v2-ledger-b2c", 10_060, Some("v2-ledger-seed"))
        .unwrap();
    pg.key_issue("v2-ledger-b2c-key", "v2-ledger-b2c", None)
        .unwrap();
    pg.account_create("v2-ledger-service", None, 10_000)
        .unwrap();
    pg.key_issue("v2-ledger-service-key", "v2-ledger-service", None)
        .unwrap();

    for catalog in [stage8_pg_catalog("main"), stage8_pg_catalog("openkeys")] {
        assert_eq!(
            pg.prepare_pricing_catalog(&catalog).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            pg.activate_pricing_catalog(
                &catalog.product_id,
                &catalog.target(),
                &crate::pricing::ActiveExpectation::Absent,
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
        pg.activate_provider_switches(
            &switches.target(),
            &crate::pricing::ActiveExpectation::Absent,
        )
        .unwrap(),
        PricingMutation::Applied
    );

    pg.client
        .batch_execute(
            "BEGIN;
             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES(
                 'v2-ledger-b2c',1,2,'v2-ledger-source','v2-ledger-normalization',
                 10060,0,0,1,100,100
             );
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES
                 ('v2-ledger-bonus','v2-ledger-b2c',1,'welcome_bonus',
                  'signup-bonus:v2-ledger',60,0,0,1,'active',100,100),
                 ('v2-ledger-paid','v2-ledger-b2c',1,'paid','v2-ledger-seed',
                  10000,0,0,1,'active',100,100);
             INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts
             ) VALUES('v2-ledger-b2c',1,1,100);
             SET CONSTRAINTS ALL IMMEDIATE;
             COMMIT;",
        )
        .unwrap();

    let b2c_policy = PricingReleasePolicyV2 {
        policy_id: "v2-ledger-b2c-policy".into(),
        policy_version: 1,
        owner_type: crate::pricing::PolicyOwnerType::GlobalB2c,
        owner_id: "global".into(),
        account_class: AccountClass::B2c,
        product_id: Some("main".into()),
        billing_mode: BillingModeV2::Balance,
        schema_version: 2,
        capability_generation: 1,
        capability_digest: "stage8-capability-1".into(),
        catalog_generation: Some(1),
        catalog_digest: Some("stage8-main-catalog-1".into()),
        switch_generation: Some(1),
        switch_digest: Some("stage8-switches-1".into()),
        content_digest: "v2-ledger-b2c-policy-digest".into(),
        rules: vec![PricingReleasePolicyRuleV2 {
            rule_id: "v2-ledger-global-50".into(),
            rule_digest: "v2-ledger-global-50-digest".into(),
            scope: PricingReleaseRuleScopeV2::Global,
            discount_bps: 5_000,
            payable_multiplier_bp: 5_000,
        }],
    };
    let service_policy = PricingReleasePolicyV2 {
        policy_id: "v2-ledger-service-policy".into(),
        policy_version: 1,
        owner_type: crate::pricing::PolicyOwnerType::Service,
        owner_id: "v2-ledger-service".into(),
        account_class: AccountClass::Service,
        product_id: None,
        billing_mode: BillingModeV2::MeterOnly,
        schema_version: 2,
        capability_generation: 1,
        capability_digest: "stage8-capability-1".into(),
        catalog_generation: None,
        catalog_digest: None,
        switch_generation: None,
        switch_digest: None,
        content_digest: "v2-ledger-service-policy-digest".into(),
        rules: Vec::new(),
    };
    for policy in [&b2c_policy, &service_policy] {
        assert_eq!(
            pg.prepare_pricing_release_policy_v2(policy).unwrap(),
            PricingMutation::Stored
        );
    }

    let assignments = vec![
        PricingReleaseAssignmentV2 {
            account_id: "v2-ledger-b2c".into(),
            account_class: AccountClass::B2c,
            policy_id: b2c_policy.policy_id.clone(),
            policy_version: 1,
            policy_digest: b2c_policy.content_digest.clone(),
            billing_mode: BillingModeV2::Balance,
            funding_generation: Some(1),
            purpose: None,
            responsible: None,
            assignment_digest: "v2-ledger-b2c-assignment".into(),
        },
        PricingReleaseAssignmentV2 {
            account_id: "v2-ledger-service".into(),
            account_class: AccountClass::Service,
            policy_id: service_policy.policy_id.clone(),
            policy_version: 1,
            policy_digest: service_policy.content_digest.clone(),
            billing_mode: BillingModeV2::MeterOnly,
            funding_generation: None,
            purpose: Some("internal-ledger-test".into()),
            responsible: Some("runtime-team".into()),
            assignment_digest: "v2-ledger-service-assignment".into(),
        },
    ];
    let release = |generation, release_kind, digest: &str| PricingReleaseV2 {
        generation,
        release_kind,
        schema_version: 2,
        capability_generation: 1,
        capability_digest: "stage8-capability-1".into(),
        main_catalog_generation: 1,
        main_catalog_digest: "stage8-main-catalog-1".into(),
        openkeys_catalog_generation: 1,
        openkeys_catalog_digest: "stage8-openkeys-catalog-1".into(),
        switch_generation: 1,
        switch_digest: "stage8-switches-1".into(),
        inventory_digest: "v2-ledger-inventory".into(),
        policy_manifest_digest: format!("v2-ledger-policies-{generation}"),
        assignment_manifest_digest: format!("v2-ledger-assignments-{generation}"),
        funding_manifest_digest: "v2-ledger-funding".into(),
        minimum_runtime_schema_version: 2,
        content_digest: digest.into(),
        assignments: assignments.clone(),
    };
    let target = release(
        TARGET_GENERATION,
        PricingReleaseKindV2::Target,
        "v2-ledger-target",
    );
    let recovery = release(
        RECOVERY_GENERATION,
        PricingReleaseKindV2::Recovery,
        "v2-ledger-recovery",
    );
    assert_eq!(
        pg.prepare_pricing_release_v2(&target).unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        pg.prepare_pricing_release_v2(&recovery).unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        pg.prepare_pricing_release_recovery_link_v2(&PricingReleaseRecoveryLinkV2 {
            target_generation: TARGET_GENERATION,
            target_digest: target.content_digest.clone(),
            recovery_generation: RECOVERY_GENERATION,
            recovery_digest: recovery.content_digest.clone(),
            link_digest: "v2-ledger-recovery-link".into(),
        })
        .unwrap(),
        PricingMutation::Stored
    );

    let activated_ts = now();
    pg.client
        .execute(
            "INSERT INTO pricing_stage8_evidence_v2(
                 evidence_digest,target_generation,target_digest,recovery_generation,
                 recovery_digest,inventory_digest,funding_digest,shadow_digest,
                 runtime_floor_digest,legacy_inflight_count,blocker_count,passed,
                 observed_ts,valid_until_ts
             ) VALUES(
                 'v2-ledger-evidence',$1,$2,$3,$4,'v2-ledger-inventory',
                 'v2-ledger-funding','v2-ledger-shadow','v2-ledger-floor',
                 0,0,true,$5,$6
             )",
            &[
                &TARGET_GENERATION,
                &target.content_digest,
                &RECOVERY_GENERATION,
                &recovery.content_digest,
                &activated_ts,
                &activated_ts.saturating_add(600),
            ],
        )
        .unwrap();
    pg.client
        .batch_execute(&format!(
            "BEGIN;
             INSERT INTO pricing_release_activations_v2(
                 activation_kind,from_generation,from_digest,to_generation,to_digest,
                 expected_head_version,resulting_head_version,evidence_digest,operator_id,
                 reason,activated_ts
             ) VALUES(
                 'cutover',NULL,NULL,{TARGET_GENERATION},'v2-ledger-target',0,1,
                 'v2-ledger-evidence','ledger-test','ledger attribution matrix',{activated_ts}
             );
             INSERT INTO pricing_release_head_v2(
                 singleton,active_generation,active_digest,head_version,updated_ts
             ) VALUES(1,{TARGET_GENERATION},'v2-ledger-target',1,{activated_ts});
             SET CONSTRAINTS ALL IMMEDIATE;
             COMMIT;"
        ))
        .unwrap();

    let resolution = pg
        .pricing_release_resolution_v2(
            "v2-ledger-b2c",
            crate::PROVIDER_GOOGLE,
            "gemini-3-flash-preview",
        )
        .unwrap()
        .unwrap();
    assert_eq!(resolution.payable_multiplier_bp(), Some(5_000));
    let admission_ts = now();
    let legacy_snapshot = crate::pricing::LegacyScalarAdmissionSnapshot::new(
        crate::pricing::LegacyScalarAdmissionSnapshotInput {
            request_id: "v2-ledger-b2c-request".into(),
            account_id: "v2-ledger-b2c".into(),
            provider: crate::pricing::SnapshotProvider::Google,
            requested_model_id: "gemini-3-flash-preview".into(),
            canonical_model_id: "gemini-3-flash-preview".into(),
            alias_generation: 1,
            tariff_schedule_id: "google/v2-ledger/v1".into(),
            tariff_priced_ts: admission_ts,
            admission_ts,
            payable_multiplier_bp: 5_000,
            official_hold_nano: 200,
            charged_hold_nano: 100,
            premium_modifiers: crate::pricing::LegacyPremiumModifiers::GeminiV1 {
                context_rate: crate::pricing::SnapshotGeminiContextRate::ConservativeMaximum,
                search_billing: crate::pricing::SnapshotGeminiSearchBilling::PerQuery,
                grounding_enabled: false,
                search_reserve_units: 0,
            },
        },
    )
    .unwrap();
    let quote =
        crate::pricing::PricingReleaseQuoteV2::from_legacy_snapshot(&legacy_snapshot).unwrap();
    let receipt = match pg
        .reserve_request_with_pricing_release_v2(
            &owner,
            "v2-ledger-b2c-key",
            600,
            &resolution,
            &quote,
        )
        .unwrap()
    {
        PricingReleaseReserveOutcomeV2::Inserted(receipt) => receipt,
        other => panic!("unexpected release-v2 reserve outcome: {other:?}"),
    };
    assert_eq!(receipt.snapshot.charged_hold_nano, 100);
    let reserve_allocations: Vec<(String, i64, i64)> = pg
        .client
        .query(
            "SELECT lot_source_type,allocation_order,reserved_nano
               FROM pricing_request_funding_allocations_v2
              WHERE request_id='v2-ledger-b2c-request' ORDER BY allocation_order",
            &[],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect();
    assert_eq!(
        reserve_allocations,
        vec![
            ("welcome_bonus".to_string(), 1, 60),
            ("paid".to_string(), 2, 40),
        ]
    );

    let usage = UsageEventInput {
        model: "gemini-3-flash-preview".into(),
        provider: crate::PROVIDER_GOOGLE.into(),
        input_tokens: 8,
        output_tokens: 4,
        real_nano: 160,
        charge_basis_nano: 160,
        input_nano: 80,
        output_nano: 80,
        priced_ts: admission_ts,
        ..UsageEventInput::default()
    };
    assert_eq!(
        pg.settle_request(
            "v2-ledger-b2c-request",
            80,
            Some("v2-ledger-settle"),
            Some(&usage),
        )
        .unwrap(),
        Some(9_980)
    );

    let entries = pg.ledger_after("v2-ledger-b2c", 0, 10).unwrap();
    assert_eq!(entries.len(), 2);
    let charge = entries
        .iter()
        .find(|entry| entry.kind == "charge")
        .expect("release-v2 settlement wrote a charge ledger row");
    assert_eq!(charge.request_id.as_deref(), Some("v2-ledger-b2c-request"));
    assert_eq!(charge.amount_nano, 80);
    assert_eq!(charge.provider.as_deref(), Some("google"));
    assert_eq!(charge.official_nano, Some(160));
    let attribution = charge
        .attribution
        .as_ref()
        .expect("release-v2 charge carries immutable attribution");
    assert_eq!(attribution.attribution_schema_version, 2);
    assert_eq!(attribution.snapshot_kind.as_deref(), Some("release_v2"));
    assert_eq!(attribution.account_class.as_deref(), Some("b2c"));
    assert_eq!(attribution.provider_id.as_deref(), Some("google"));
    assert_eq!(
        attribution.requested_model_id.as_deref(),
        Some("gemini-3-flash-preview")
    );
    assert_eq!(
        attribution.canonical_model_id.as_deref(),
        Some("gemini-3-flash-preview")
    );
    assert_eq!(
        attribution.served_model_id.as_deref(),
        Some("gemini-3-flash-preview")
    );
    assert_eq!(
        attribution.served_canonical_model_id.as_deref(),
        Some("gemini-3-flash-preview")
    );
    assert_eq!(attribution.rule_id.as_deref(), Some("v2-ledger-global-50"));
    assert_eq!(
        attribution.rule_digest.as_deref(),
        Some("v2-ledger-global-50-digest")
    );
    assert_eq!(attribution.rule_scope.as_deref(), Some("global"));
    assert_eq!(attribution.discount_bps, Some(5_000));
    assert_eq!(attribution.payable_multiplier_bp, Some(5_000));
    assert_eq!(
        attribution.policy_id.as_deref(),
        Some("v2-ledger-b2c-policy")
    );
    assert_eq!(attribution.policy_version, Some(1));
    assert_eq!(
        attribution.policy_digest.as_deref(),
        Some("v2-ledger-b2c-policy-digest")
    );
    assert_eq!(
        attribution.tariff_schedule_id.as_deref(),
        Some("google/v2-ledger/v1")
    );
    assert_eq!(attribution.tariff_priced_ts, Some(admission_ts));
    assert_eq!(attribution.official_nano, Some(160));
    assert!(attribution
        .official_cost_json
        .as_ref()
        .is_some_and(serde_json::Value::is_object));
    assert_eq!(
        (
            attribution.paid_funded_nano,
            attribution.bonus_funded_nano,
            attribution.other_funded_nano,
        ),
        (Some(20), Some(60), Some(0))
    );
    assert_eq!(
        attribution.paid_funded_nano.unwrap()
            + attribution.bonus_funded_nano.unwrap()
            + attribution.other_funded_nano.unwrap(),
        charge.amount_nano
    );
    let funding_evidence = attribution
        .funding_allocation_json
        .as_ref()
        .and_then(serde_json::Value::as_array)
        .expect("release-v2 charge carries v2 funding allocation evidence");
    assert_eq!(funding_evidence.len(), 2);
    assert_eq!(funding_evidence[0]["allocation_order"], 1);
    assert_eq!(funding_evidence[0]["lot_id"], "v2-ledger-bonus");
    assert_eq!(funding_evidence[0]["lot_source_type"], "welcome_bonus");
    assert_eq!(funding_evidence[0]["direction"], "debit");
    assert_eq!(funding_evidence[0]["amount_nano"], 60);
    assert!(funding_evidence[0]["lot_version"].as_i64().unwrap() > 0);
    assert_eq!(funding_evidence[1]["allocation_order"], 2);
    assert_eq!(funding_evidence[1]["lot_id"], "v2-ledger-paid");
    assert_eq!(funding_evidence[1]["lot_source_type"], "paid");
    assert_eq!(funding_evidence[1]["direction"], "debit");
    assert_eq!(funding_evidence[1]["amount_nano"], 20);
    assert_eq!(attribution.pricing_mode, None);
    assert_eq!(attribution.rule_origin, None);
    assert_eq!(attribution.track_eligible, None);
    assert_eq!(attribution.retention_eligible, None);
    assert_eq!(attribution.commission_eligible, None);
    assert_eq!(
        attribution.snapshot_digest.as_deref(),
        Some(receipt.snapshot.snapshot_digest.as_str())
    );
    assert_eq!(attribution.release_schema_version, Some(2));
    assert_eq!(attribution.release_generation, Some(TARGET_GENERATION));
    assert_eq!(
        attribution.release_digest.as_deref(),
        Some("v2-ledger-target")
    );
    assert_eq!(attribution.release_billing_mode.as_deref(), Some("balance"));
    assert_eq!(attribution.release_funding_generation, Some(1));

    let durable_allocations: Vec<(String, String, i64)> = pg
        .client
        .query(
            "SELECT lot_id,lot_source_type,amount_nano
               FROM funding_ledger_allocations_v2
              WHERE ledger_id=$1 ORDER BY allocation_order",
            &[&charge.id],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect();
    assert_eq!(
        durable_allocations,
        vec![
            (
                "v2-ledger-bonus".to_string(),
                "welcome_bonus".to_string(),
                60
            ),
            ("v2-ledger-paid".to_string(), "paid".to_string(), 20),
        ]
    );

    assert_eq!(
        pg.settle_request(
            "v2-ledger-b2c-request",
            80,
            Some("v2-ledger-settle"),
            Some(&usage),
        )
        .unwrap(),
        Some(9_980)
    );
    let charge_count: i64 = pg
        .client
        .query_one(
            "SELECT count(*)::bigint FROM ledger
              WHERE account_id='v2-ledger-b2c' AND kind='charge'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(charge_count, 1);

    let service_resolution = pg
        .pricing_release_resolution_v2(
            "v2-ledger-service",
            crate::PROVIDER_GOOGLE,
            "v2-ledger-meter-only-model",
        )
        .unwrap()
        .unwrap();
    assert_eq!(service_resolution.billing_mode(), BillingModeV2::MeterOnly);
    let service_snapshot = crate::pricing::LegacyScalarAdmissionSnapshot::new(
        crate::pricing::LegacyScalarAdmissionSnapshotInput {
            request_id: "v2-ledger-service-request".into(),
            account_id: "v2-ledger-service".into(),
            provider: crate::pricing::SnapshotProvider::Google,
            requested_model_id: "v2-ledger-meter-only-model".into(),
            canonical_model_id: "v2-ledger-meter-only-model".into(),
            alias_generation: 1,
            tariff_schedule_id: "google/v2-ledger-service/v1".into(),
            tariff_priced_ts: admission_ts,
            admission_ts,
            payable_multiplier_bp: 10_000,
            official_hold_nano: 500,
            charged_hold_nano: 500,
            premium_modifiers: crate::pricing::LegacyPremiumModifiers::GeminiV1 {
                context_rate: crate::pricing::SnapshotGeminiContextRate::ConservativeMaximum,
                search_billing: crate::pricing::SnapshotGeminiSearchBilling::PerQuery,
                grounding_enabled: false,
                search_reserve_units: 0,
            },
        },
    )
    .unwrap();
    let service_quote =
        crate::pricing::PricingReleaseQuoteV2::from_legacy_snapshot(&service_snapshot).unwrap();
    let service_receipt = match pg
        .reserve_request_with_pricing_release_v2(
            &owner,
            "v2-ledger-service-key",
            600,
            &service_resolution,
            &service_quote,
        )
        .unwrap()
    {
        PricingReleaseReserveOutcomeV2::Inserted(receipt) => receipt,
        other => panic!("unexpected service release-v2 reserve outcome: {other:?}"),
    };
    assert_eq!(service_receipt.snapshot.charged_hold_nano, 0);
    let service_usage = UsageEventInput {
        model: "v2-ledger-meter-only-model".into(),
        provider: crate::PROVIDER_GOOGLE.into(),
        input_tokens: 10,
        real_nano: 500,
        charge_basis_nano: 500,
        input_nano: 500,
        priced_ts: admission_ts,
        ..UsageEventInput::default()
    };
    assert_eq!(
        pg.settle_request("v2-ledger-service-request", 0, None, Some(&service_usage),)
            .unwrap(),
        Some(0)
    );
    assert_eq!(
        pg.settle_request("v2-ledger-service-request", 0, None, Some(&service_usage),)
            .unwrap(),
        Some(0)
    );
    let service_ledger_count: i64 = pg
        .client
        .query_one(
            "SELECT count(*)::bigint FROM ledger WHERE account_id='v2-ledger-service'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(service_ledger_count, 0);

    pg.client
        .batch_execute(
            "TRUNCATE pricing_release_policy_versions,pricing_release_versions,
             account_policy_bindings,account_policy_rules,account_policy_versions,
             provider_switch_head,provider_switch_entries,provider_switch_versions,
             pricing_catalog_heads,pricing_catalog_entries,pricing_catalog_versions,
             execution_group_winner,settlement_outbox,reservations,capacity_leases,
             leader_leases,engine_instances,usage_events,ledger,api_keys,accounts,pool_state,
             subs RESTART IDENTITY CASCADE",
        )
        .unwrap();
    pg.client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}

/// Dual path of the release-v2 retirement: the opt-out marker branch of the resolver, the
/// account-aware legacy-path closure gate, the guarded one-way opt-out writer and the mixed
/// in-flight drain of a release-v2 reservation across the opt-out.
///
/// The drain account is opted out at the FIXTURE level (direct marker UPDATE): the guarded
/// writer requires a `strict/strict/verified` binding, and migration 0016's strict triggers
/// (a) forbid the strict binding cutover while a non-policy reservation is in flight and
/// (b) forbid settling a release-format reservation once the binding is strict, so a
/// production account can only ever reach the guarded writer fully drained. The drain
/// scenario therefore pairs a non-strict binding with a fixture opt-out and proves that
/// settlement is dispatched by the immutable reserve-time snapshot, never by the marker.
///
/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::pricing_release_opt_out_dual_path_postgres_matrix`
#[test]
fn pricing_release_opt_out_dual_path_postgres_matrix() {
    use crate::pricing::{
        AccountClass, BillingModeV2, PricingMutation, PricingRejection,
        PricingReleaseAssignmentV2, PricingReleaseKindV2, PricingReleaseOptOutOutcomeV2,
        PricingReleaseOptOutV2, PricingReleasePolicyRuleV2, PricingReleasePolicyV2,
        PricingReleaseRecoveryLinkV2, PricingReleaseReserveOutcomeV2, PricingReleaseRuleScopeV2,
        PricingReleaseV2,
    };

    const TARGET_GENERATION: i64 = 94_001;
    const RECOVERY_GENERATION: i64 = 94_002;
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping pricing release opt-out dual-path matrix: test URL is unset");
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
            "TRUNCATE pricing_release_policy_versions,pricing_release_versions,
             account_policy_bindings,account_policy_rules,account_policy_versions,
             provider_switch_head,provider_switch_entries,provider_switch_versions,
             pricing_catalog_heads,pricing_catalog_entries,pricing_catalog_versions,
             execution_group_winner,settlement_outbox,reservations,capacity_leases,
             leader_leases,engine_instances,usage_events,ledger,api_keys,accounts,pool_state,
             subs RESTART IDENTITY CASCADE",
        )
        .unwrap();

    let manifest = stage8_pg_manifest();
    let owner = pg
        .claim_instance_with_pricing_manifest("dual-path-engine", 600, &manifest)
        .unwrap();
    pg.account_create("dual-drain", None, 5_000).unwrap();
    pg.account_topup("dual-drain", 10_060, Some("dual-drain-seed"))
        .unwrap();
    pg.key_issue("dual-drain-key", "dual-drain", None).unwrap();
    pg.account_create("dual-ctl", None, 10_000).unwrap();
    pg.account_topup("dual-ctl", 2_000, Some("dual-ctl-seed"))
        .unwrap();
    pg.key_issue("dual-ctl-key", "dual-ctl", None).unwrap();
    pg.account_create("dual-legacy", None, 10_000).unwrap();
    pg.key_issue("dual-legacy-key", "dual-legacy", None).unwrap();

    for catalog in [stage8_pg_catalog("main"), stage8_pg_catalog("openkeys")] {
        assert_eq!(
            pg.prepare_pricing_catalog(&catalog).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            pg.activate_pricing_catalog(
                &catalog.product_id,
                &catalog.target(),
                &crate::pricing::ActiveExpectation::Absent,
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
        pg.activate_provider_switches(
            &switches.target(),
            &crate::pricing::ActiveExpectation::Absent,
        )
        .unwrap(),
        PricingMutation::Applied
    );

    pg.client
        .batch_execute(
            "BEGIN;
             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES(
                 'dual-drain',1,2,'dual-drain-source','dual-drain-normalization',
                 10060,0,0,1,100,100
             );
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES
                 ('dual-drain-bonus','dual-drain',1,'welcome_bonus',
                  'signup-bonus:dual-drain',60,0,0,1,'active',100,100),
                 ('dual-drain-paid','dual-drain',1,'paid','dual-drain-seed',
                  10000,0,0,1,'active',100,100);
             INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts
             ) VALUES('dual-drain',1,1,100);
             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES(
                 'dual-ctl',1,2,'dual-ctl-source','dual-ctl-normalization',
                 2000,0,0,1,100,100
             );
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES(
                 'dual-ctl-paid','dual-ctl',1,'paid','dual-ctl-seed',
                 2000,0,0,1,'active',100,100
             );
             INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts
             ) VALUES('dual-ctl',1,1,100);
             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES(
                 'dual-legacy',1,2,'dual-legacy-source','dual-legacy-normalization',
                 0,0,0,1,100,100
             );
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES(
                 'dual-legacy-paid','dual-legacy',1,'paid','dual-legacy-anchor',
                 0,0,0,1,'active',100,100
             );
             INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts
             ) VALUES('dual-legacy',1,1,100);
             SET CONSTRAINTS ALL IMMEDIATE;
             COMMIT;",
        )
        .unwrap();

    let drain_policy = PricingReleasePolicyV2 {
        policy_id: "dual-drain-policy".into(),
        policy_version: 1,
        owner_type: crate::pricing::PolicyOwnerType::GlobalB2c,
        owner_id: "global".into(),
        account_class: AccountClass::B2c,
        product_id: Some("main".into()),
        billing_mode: BillingModeV2::Balance,
        schema_version: 2,
        capability_generation: 1,
        capability_digest: "stage8-capability-1".into(),
        catalog_generation: Some(1),
        catalog_digest: Some("stage8-main-catalog-1".into()),
        switch_generation: Some(1),
        switch_digest: Some("stage8-switches-1".into()),
        content_digest: "dual-drain-policy-digest".into(),
        rules: vec![PricingReleasePolicyRuleV2 {
            rule_id: "dual-drain-global-50".into(),
            rule_digest: "dual-drain-global-50-digest".into(),
            scope: PricingReleaseRuleScopeV2::Global,
            discount_bps: 5_000,
            payable_multiplier_bp: 5_000,
        }],
    };
    let ctl_policy = PricingReleasePolicyV2 {
        policy_id: "dual-ctl-policy".into(),
        content_digest: "dual-ctl-policy-digest".into(),
        rules: vec![PricingReleasePolicyRuleV2 {
            rule_id: "dual-ctl-global".into(),
            rule_digest: "dual-ctl-global-digest".into(),
            scope: PricingReleaseRuleScopeV2::Global,
            discount_bps: 5_000,
            payable_multiplier_bp: 5_000,
        }],
        ..drain_policy.clone()
    };
    let legacy_policy = PricingReleasePolicyV2 {
        policy_id: "dual-legacy-policy".into(),
        content_digest: "dual-legacy-policy-digest".into(),
        rules: vec![PricingReleasePolicyRuleV2 {
            rule_id: "dual-legacy-global".into(),
            rule_digest: "dual-legacy-global-digest".into(),
            scope: PricingReleaseRuleScopeV2::Global,
            discount_bps: 5_000,
            payable_multiplier_bp: 5_000,
        }],
        ..drain_policy.clone()
    };
    for policy in [&drain_policy, &ctl_policy, &legacy_policy] {
        assert_eq!(
            pg.prepare_pricing_release_policy_v2(policy).unwrap(),
            PricingMutation::Stored
        );
    }

    let assignments = vec![
        PricingReleaseAssignmentV2 {
            account_id: "dual-drain".into(),
            account_class: AccountClass::B2c,
            policy_id: drain_policy.policy_id.clone(),
            policy_version: 1,
            policy_digest: drain_policy.content_digest.clone(),
            billing_mode: BillingModeV2::Balance,
            funding_generation: Some(1),
            purpose: None,
            responsible: None,
            assignment_digest: "dual-drain-assignment".into(),
        },
        PricingReleaseAssignmentV2 {
            account_id: "dual-ctl".into(),
            account_class: AccountClass::B2c,
            policy_id: ctl_policy.policy_id.clone(),
            policy_version: 1,
            policy_digest: ctl_policy.content_digest.clone(),
            billing_mode: BillingModeV2::Balance,
            funding_generation: Some(1),
            purpose: None,
            responsible: None,
            assignment_digest: "dual-ctl-assignment".into(),
        },
        PricingReleaseAssignmentV2 {
            account_id: "dual-legacy".into(),
            account_class: AccountClass::B2c,
            policy_id: legacy_policy.policy_id.clone(),
            policy_version: 1,
            policy_digest: legacy_policy.content_digest.clone(),
            billing_mode: BillingModeV2::Balance,
            funding_generation: Some(1),
            purpose: None,
            responsible: None,
            assignment_digest: "dual-legacy-assignment".into(),
        },
    ];
    let release = |generation, release_kind, digest: &str| PricingReleaseV2 {
        generation,
        release_kind,
        schema_version: 2,
        capability_generation: 1,
        capability_digest: "stage8-capability-1".into(),
        main_catalog_generation: 1,
        main_catalog_digest: "stage8-main-catalog-1".into(),
        openkeys_catalog_generation: 1,
        openkeys_catalog_digest: "stage8-openkeys-catalog-1".into(),
        switch_generation: 1,
        switch_digest: "stage8-switches-1".into(),
        inventory_digest: "dual-path-inventory".into(),
        policy_manifest_digest: format!("dual-path-policies-{generation}"),
        assignment_manifest_digest: format!("dual-path-assignments-{generation}"),
        funding_manifest_digest: "dual-path-funding".into(),
        minimum_runtime_schema_version: 2,
        content_digest: digest.into(),
        assignments: assignments.clone(),
    };
    let target = release(
        TARGET_GENERATION,
        PricingReleaseKindV2::Target,
        "dual-path-target",
    );
    let recovery = release(
        RECOVERY_GENERATION,
        PricingReleaseKindV2::Recovery,
        "dual-path-recovery",
    );
    assert_eq!(
        pg.prepare_pricing_release_v2(&target).unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        pg.prepare_pricing_release_v2(&recovery).unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        pg.prepare_pricing_release_recovery_link_v2(&PricingReleaseRecoveryLinkV2 {
            target_generation: TARGET_GENERATION,
            target_digest: target.content_digest.clone(),
            recovery_generation: RECOVERY_GENERATION,
            recovery_digest: recovery.content_digest.clone(),
            link_digest: "dual-path-recovery-link".into(),
        })
        .unwrap(),
        PricingMutation::Stored
    );

    let activated_ts = now();
    pg.client
        .execute(
            "INSERT INTO pricing_stage8_evidence_v2(
                 evidence_digest,target_generation,target_digest,recovery_generation,
                 recovery_digest,inventory_digest,funding_digest,shadow_digest,
                 runtime_floor_digest,legacy_inflight_count,blocker_count,passed,
                 observed_ts,valid_until_ts
             ) VALUES(
                 'dual-path-evidence',$1,$2,$3,$4,'dual-path-inventory',
                 'dual-path-funding','dual-path-shadow','dual-path-floor',
                 0,0,true,$5,$6
             )",
            &[
                &TARGET_GENERATION,
                &target.content_digest,
                &RECOVERY_GENERATION,
                &recovery.content_digest,
                &activated_ts,
                &activated_ts.saturating_add(600),
            ],
        )
        .unwrap();
    pg.client
        .batch_execute(&format!(
            "BEGIN;
             INSERT INTO pricing_release_activations_v2(
                 activation_kind,from_generation,from_digest,to_generation,to_digest,
                 expected_head_version,resulting_head_version,evidence_digest,operator_id,
                 reason,activated_ts
             ) VALUES(
                 'cutover',NULL,NULL,{TARGET_GENERATION},'dual-path-target',0,1,
                 'dual-path-evidence','dual-path-test','dual path matrix',{activated_ts}
             );
             INSERT INTO pricing_release_head_v2(
                 singleton,active_generation,active_digest,head_version,updated_ts
             ) VALUES(1,{TARGET_GENERATION},'dual-path-target',1,{activated_ts});
             SET CONSTRAINTS ALL IMMEDIATE;
             COMMIT;"
        ))
        .unwrap();

    // Strict fixtures (migration-0016 compliant: ACK'd keys BEFORE the binding insert, no
    // in-flight reservations, policy-capable engine manifest already claimed above).
    pg.client
        .batch_execute(
            "BEGIN;
             INSERT INTO accounts(
                 id,balance_nano,reserved_nano,mult_bp,status,created_ts,created
             ) VALUES
                 ('dual-strict',1000,0,10000,'active',1,''),
                 ('dual-stale',500,0,10000,'active',1,'');
             INSERT INTO account_policy_versions(
                 account_id,effective_version,policy_id,policy_version,source_policy_digest,
                 owner_type,owner_id,account_class,product_id,schema_version,
                 catalog_generation,switch_generation,content_digest,replacement_locked,created_ts
             ) VALUES
                 ('dual-strict',1,'dual-strict-policy',1,'dual-strict-source-v1',
                  'global_b2c','global','b2c','main',1,1,1,'dual-strict-policy-v1',false,1),
                 ('dual-stale',1,'dual-stale-policy',1,'dual-stale-source-v1',
                  'global_b2c','global','b2c','main',1,1,1,'dual-stale-policy-v1',false,1),
                 ('dual-stale',2,'dual-stale-policy',2,'dual-stale-source-v2',
                  'global_b2c','global','b2c','main',1,1,1,'dual-stale-policy-v2',false,2);
             INSERT INTO account_policy_rules(
                 account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                 canonical_model_id,pricing_mode,rule_origin,discount_bps,
                 payable_multiplier_bp,track_eligible,retention_eligible,commission_eligible
             ) VALUES
                 ('dual-strict',1,'dual-strict-rule','dual-strict-rule-v1','provider',
                  'anthropic',NULL,'track','managed',NULL,10000,true,true,false),
                 ('dual-stale',1,'dual-stale-rule','dual-stale-rule-v1','provider',
                  'anthropic',NULL,'track','managed',NULL,10000,true,true,false);
             INSERT INTO funding_buckets(
                 bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES
                 ('dual-strict-paid','dual-strict','paid','primary','any',
                  1000,0,0,1,'active',1,1),
                 ('dual-stale-paid','dual-stale','paid','primary','any',
                  500,0,0,1,'active',1,1);
             INSERT INTO api_keys(
                 key,key_id,account_id,status,created_ts,created,
                 activation_policy_effective_version,activation_policy_digest,
                 activation_policy_ack_ts
             ) VALUES
                 ('dual-strict-key','key_dual_strict','dual-strict','active',1,'',
                  1,'dual-strict-policy-v1',1),
                 ('dual-stale-key','key_dual_stale','dual-stale','active',1,'',
                  1,'dual-stale-policy-v1',1);
             INSERT INTO account_policy_bindings(
                 account_id,product_id,account_class,active_effective_version,
                 policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
             ) VALUES
                 ('dual-strict','main','b2c',1,'strict','strict','verified',1),
                 ('dual-stale','main','b2c',1,'strict','strict','verified',1);
             UPDATE api_keys
                SET activation_policy_effective_version=2,
                    activation_policy_digest='dual-stale-policy-v2'
              WHERE key='dual-stale-key';
             SET CONSTRAINTS ALL IMMEDIATE;
             COMMIT;",
        )
        .unwrap();

    // A. Baseline: both assigned accounts resolve through the head; the strict account without
    // an assignment fails closed exactly like before the dual path.
    let drain_resolution = pg
        .pricing_release_resolution_v2("dual-drain", crate::PROVIDER_GOOGLE, "gemini-3-flash-preview")
        .unwrap()
        .expect("dual-drain resolves through the active release head");
    assert_eq!(drain_resolution.payable_multiplier_bp(), Some(5_000));
    assert!(pg
        .pricing_release_resolution_v2("dual-ctl", crate::PROVIDER_GOOGLE, "gemini-3-flash-preview")
        .unwrap()
        .is_some());
    assert!(pg
        .pricing_release_resolution_v2("dual-strict", "anthropic", "claude-sonnet-5")
        .is_err());

    // B. Non-opted accounts keep the closed errors on every non-release reserve writer.
    let closed_error = pg
        .reserve_request(&owner, "dual-ctl-plain", "dual-ctl", "dual-ctl-key", 100, 600)
        .expect_err("plain reserve of a non-opted account must stay closed");
    assert!(closed_error
        .downcast_ref::<crate::pricing::LegacyPricingPathClosedV2>()
        .is_some());
    assert!(matches!(
        pg.reserve_request_with_legacy_snapshot(
            &owner,
            "dual-ctl-key",
            600,
            &legacy_snapshot("dual-ctl-scalar", "dual-ctl", 100, 100),
        )
        .unwrap(),
        crate::pricing::LegacyScalarReserveOutcome::Conflict(
            crate::pricing::LegacyScalarReserveConflict::ActivePricingRelease
        )
    ));

    let strict_admission_ts = now();
    let strict_snapshot = crate::pricing::PolicyAdmissionSnapshot::new(
        crate::pricing::PolicyAdmissionSnapshotInput {
            request_id: "dual-strict-request".into(),
            account_id: "dual-strict".into(),
            provider: crate::pricing::SnapshotProvider::Anthropic,
            product_id: "main".into(),
            account_class: AccountClass::B2c,
            requested_model_id: "claude-sonnet-5".into(),
            canonical_model_id: "claude-sonnet-5".into(),
            alias_generation: 1,
            rule_id: "dual-strict-rule".into(),
            rule_digest: "dual-strict-rule-v1".into(),
            rule_scope: crate::pricing::PolicyRuleScope::Provider {
                provider_id: "anthropic".into(),
            },
            pricing_mode: crate::pricing::PricingMode::Track,
            rule_origin: crate::pricing::RuleOrigin::Managed,
            discount_bps: None,
            payable_multiplier_bp: 10_000,
            policy_id: "dual-strict-policy".into(),
            policy_version: 1,
            effective_policy_version: 1,
            source_policy_digest: "dual-strict-source-v1".into(),
            policy_digest: "dual-strict-policy-v1".into(),
            policy_catalog_generation: 1,
            policy_switch_generation: 1,
            admission_catalog_generation: 1,
            admission_catalog_digest: "stage8-main-catalog-1".into(),
            admission_switch_generation: 1,
            admission_switch_digest: "stage8-switches-1".into(),
            runtime_manifest_generation: manifest.manifest_generation(),
            runtime_manifest_digest: manifest.manifest_digest().into(),
            tariff_schedule_id: "dual-strict-tariff-v1".into(),
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
        pg.reserve_request_with_policy_snapshot(&owner, "dual-strict-key", 600, &strict_snapshot)
            .unwrap(),
        crate::pricing::PolicyReserveOutcome::Conflict(
            crate::pricing::PolicyReserveConflict::ActivePricingRelease
        )
    ));

    // C. The guarded writer fails closed: no strict path, stale ACK, unknown account, garbage.
    let opt_out = |account_id: &str| PricingReleaseOptOutV2 {
        account_id: account_id.into(),
        created_by: Some("dual-path-operator".into()),
        reason: Some("dual path matrix".into()),
    };
    assert!(matches!(
        pg.pricing_release_opt_out_v2(&opt_out("dual-legacy")).unwrap(),
        PricingReleaseOptOutOutcomeV2::Rejected(PricingRejection::MissingDependency {
            dependency
        }) if dependency == "active_strict_policy_binding"
    ));
    assert!(matches!(
        pg.pricing_release_opt_out_v2(&opt_out("dual-stale")).unwrap(),
        PricingReleaseOptOutOutcomeV2::Rejected(PricingRejection::MissingDependency {
            dependency
        }) if dependency == "active_strict_policy_binding"
    ));
    assert!(matches!(
        pg.pricing_release_opt_out_v2(&opt_out("dual-missing")).unwrap(),
        PricingReleaseOptOutOutcomeV2::Rejected(PricingRejection::MissingDependency {
            dependency
        }) if dependency == "account"
    ));
    assert!(matches!(
        pg.pricing_release_opt_out_v2(&PricingReleaseOptOutV2 {
            account_id: " ".into(),
            created_by: None,
            reason: None,
        })
        .unwrap(),
        PricingReleaseOptOutOutcomeV2::Rejected(PricingRejection::Invalid { .. })
    ));
    let marker_absent: i64 = pg
        .client
        .query_one(
            "SELECT count(*)::bigint FROM accounts
              WHERE pricing_release_opt_out_ts IS NOT NULL",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(marker_absent, 0);

    // D. The guarded writer sets the marker idempotently and the resolver falls through.
    let applied_ts = match pg.pricing_release_opt_out_v2(&opt_out("dual-strict")).unwrap() {
        PricingReleaseOptOutOutcomeV2::Applied {
            pricing_release_opt_out_ts,
        } => pricing_release_opt_out_ts,
        other => panic!("unexpected opt-out outcome: {other:?}"),
    };
    let stored_ts: Option<i64> = pg
        .client
        .query_one(
            "SELECT pricing_release_opt_out_ts FROM accounts WHERE id='dual-strict'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(stored_ts, Some(applied_ts));
    for replay in [
        opt_out("dual-strict"),
        PricingReleaseOptOutV2 {
            account_id: "dual-strict".into(),
            created_by: Some("another-operator".into()),
            reason: Some("repeated call with different attribution".into()),
        },
    ] {
        assert_eq!(
            pg.pricing_release_opt_out_v2(&replay).unwrap(),
            PricingReleaseOptOutOutcomeV2::Unchanged {
                pricing_release_opt_out_ts: applied_ts
            }
        );
    }
    assert_eq!(
        pg.client
            .query_one(
                "SELECT pricing_release_opt_out_ts FROM accounts WHERE id='dual-strict'",
                &[],
            )
            .unwrap()
            .get::<_, Option<i64>>(0),
        Some(applied_ts)
    );
    // Dual path: with the marker set the resolver answers "no release" instead of the
    // coverage error, while the head keeps serving everyone else.
    assert!(pg
        .pricing_release_resolution_v2("dual-strict", "anthropic", "claude-sonnet-5")
        .unwrap()
        .is_none());
    assert!(pg
        .pricing_release_resolution_v2("dual-ctl", crate::PROVIDER_GOOGLE, "gemini-3-flash-preview")
        .unwrap()
        .is_some());

    // E. Post-opt-out strict reserve settles through the policy settlement path.
    assert!(matches!(
        pg.reserve_request_with_policy_snapshot(&owner, "dual-strict-key", 600, &strict_snapshot)
            .unwrap(),
        crate::pricing::PolicyReserveOutcome::Inserted(_)
    ));
    let strict_reserve_allocation: (String, i64) = {
        let row = pg
            .client
            .query_one(
                "SELECT bucket_id,reserved_nano FROM reservation_funding_allocations
                  WHERE request_id='dual-strict-request'",
                &[],
            )
            .unwrap();
        (row.get(0), row.get(1))
    };
    assert_eq!(
        strict_reserve_allocation,
        ("dual-strict-paid".to_string(), 100)
    );
    let strict_usage = UsageEventInput {
        model: "claude-sonnet-5".into(),
        provider: "anthropic".into(),
        input_tokens: 8,
        output_tokens: 4,
        real_nano: 40,
        charge_basis_nano: 40,
        input_nano: 20,
        output_nano: 20,
        priced_ts: strict_admission_ts,
        ..UsageEventInput::default()
    };
    assert_eq!(
        pg.settle_request(
            "dual-strict-request",
            40,
            Some("dual-strict-settle"),
            Some(&strict_usage),
        )
        .unwrap(),
        Some(960)
    );
    assert_eq!(
        pg.settle_request(
            "dual-strict-request",
            40,
            Some("dual-strict-settle"),
            Some(&strict_usage),
        )
        .unwrap(),
        Some(960)
    );
    let strict_state: (i64, i64, i64, i64, i64, i64, String, String, i64) = {
        let row = pg
            .client
            .query_one(
                "SELECT
                     (SELECT balance_nano FROM accounts WHERE id='dual-strict'),
                     (SELECT reserved_nano FROM accounts WHERE id='dual-strict'),
                     (SELECT spent_nano FROM accounts WHERE id='dual-strict'),
                     (SELECT balance_nano FROM funding_buckets
                       WHERE bucket_id='dual-strict-paid'),
                     (SELECT charged_nano FROM reservation_funding_allocations
                       WHERE request_id='dual-strict-request'),
                     (SELECT released_nano FROM reservation_funding_allocations
                       WHERE request_id='dual-strict-request'),
                     (SELECT state FROM reservations WHERE request_id='dual-strict-request'),
                     (SELECT snapshot_kind FROM settlement_outbox
                       WHERE request_id='dual-strict-request'),
                     (SELECT count(*)::bigint FROM ledger
                       WHERE account_id='dual-strict' AND kind='charge')",
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
            row.get(8),
        )
    };
    assert_eq!(
        strict_state,
        (960, 0, 40, 960, 40, 60, "settled".to_string(), "policy_v1".to_string(), 1)
    );

    // F. Mixed in-flight drain: a release-v2 reservation created BEFORE the opt-out settles
    // exactly once with the correct paid/bonus split AFTER the opt-out.
    let drain_admission_ts = now();
    let drain_legacy_snapshot = crate::pricing::LegacyScalarAdmissionSnapshot::new(
        crate::pricing::LegacyScalarAdmissionSnapshotInput {
            request_id: "dual-drain-release-request".into(),
            account_id: "dual-drain".into(),
            provider: crate::pricing::SnapshotProvider::Google,
            requested_model_id: "gemini-3-flash-preview".into(),
            canonical_model_id: "gemini-3-flash-preview".into(),
            alias_generation: 1,
            tariff_schedule_id: "google/dual-drain/v1".into(),
            tariff_priced_ts: drain_admission_ts,
            admission_ts: drain_admission_ts,
            payable_multiplier_bp: 5_000,
            official_hold_nano: 200,
            charged_hold_nano: 100,
            premium_modifiers: crate::pricing::LegacyPremiumModifiers::GeminiV1 {
                context_rate: crate::pricing::SnapshotGeminiContextRate::ConservativeMaximum,
                search_billing: crate::pricing::SnapshotGeminiSearchBilling::PerQuery,
                grounding_enabled: false,
                search_reserve_units: 0,
            },
        },
    )
    .unwrap();
    let drain_quote =
        crate::pricing::PricingReleaseQuoteV2::from_legacy_snapshot(&drain_legacy_snapshot)
            .unwrap();
    assert!(matches!(
        pg.reserve_request_with_pricing_release_v2(
            &owner,
            "dual-drain-key",
            600,
            &drain_resolution,
            &drain_quote,
        )
        .unwrap(),
        PricingReleaseReserveOutcomeV2::Inserted(_)
    ));
    let drain_reserve_allocations: Vec<(String, i64, i64)> = pg
        .client
        .query(
            "SELECT lot_source_type,allocation_order,reserved_nano
               FROM pricing_request_funding_allocations_v2
              WHERE request_id='dual-drain-release-request' ORDER BY allocation_order",
            &[],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect();
    assert_eq!(
        drain_reserve_allocations,
        vec![
            ("welcome_bonus".to_string(), 1, 60),
            ("paid".to_string(), 2, 40),
        ]
    );

    let fixture_opt_out_ts = now();
    pg.client
        .execute(
            "UPDATE accounts SET pricing_release_opt_out_ts=$1 WHERE id='dual-drain'",
            &[&fixture_opt_out_ts],
        )
        .unwrap();
    // The head still exists and still serves the non-opted account; the drain account falls
    // through to the direct paths from here on.
    assert!(pg
        .pricing_release_resolution_v2("dual-drain", crate::PROVIDER_GOOGLE, "gemini-3-flash-preview")
        .unwrap()
        .is_none());
    assert!(pg
        .pricing_release_resolution_v2("dual-ctl", crate::PROVIDER_GOOGLE, "gemini-3-flash-preview")
        .unwrap()
        .is_some());

    let drain_usage = UsageEventInput {
        model: "gemini-3-flash-preview".into(),
        provider: crate::PROVIDER_GOOGLE.into(),
        input_tokens: 8,
        output_tokens: 4,
        real_nano: 80,
        charge_basis_nano: 80,
        input_nano: 40,
        output_nano: 40,
        priced_ts: drain_admission_ts,
        ..UsageEventInput::default()
    };
    assert_eq!(
        pg.settle_request(
            "dual-drain-release-request",
            40,
            Some("dual-drain-settle"),
            Some(&drain_usage),
        )
        .unwrap(),
        Some(10_020)
    );
    assert_eq!(
        pg.settle_request(
            "dual-drain-release-request",
            40,
            Some("dual-drain-settle"),
            Some(&drain_usage),
        )
        .unwrap(),
        Some(10_020)
    );
    let drain_charges = pg.ledger_after("dual-drain", 0, 10).unwrap();
    let charge = drain_charges
        .iter()
        .find(|entry| entry.kind == "charge")
        .expect("drain settlement wrote exactly one charge");
    assert_eq!(charge.amount_nano, 40);
    assert_eq!(
        charge.request_id.as_deref(),
        Some("dual-drain-release-request")
    );
    let attribution = charge
        .attribution
        .as_ref()
        .expect("drain charge keeps its immutable release-v2 attribution");
    assert_eq!(attribution.snapshot_kind.as_deref(), Some("release_v2"));
    assert_eq!(
        (
            attribution.paid_funded_nano,
            attribution.bonus_funded_nano,
            attribution.other_funded_nano,
        ),
        (Some(0), Some(40), Some(0))
    );
    let durable_allocations: Vec<(String, String, i64)> = pg
        .client
        .query(
            "SELECT lot_id,lot_source_type,amount_nano
               FROM funding_ledger_allocations_v2
              WHERE ledger_id=$1 ORDER BY allocation_order",
            &[&charge.id],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect();
    assert_eq!(
        durable_allocations,
        vec![("dual-drain-bonus".to_string(), "welcome_bonus".to_string(), 40)]
    );
    let drain_state: (i64, i64, i64, String, i64) = {
        let row = pg
            .client
            .query_one(
                "SELECT
                     (SELECT balance_nano FROM accounts WHERE id='dual-drain'),
                     (SELECT reserved_nano FROM accounts WHERE id='dual-drain'),
                     (SELECT spent_nano FROM accounts WHERE id='dual-drain'),
                     (SELECT state FROM reservations
                       WHERE request_id='dual-drain-release-request'),
                     (SELECT count(*)::bigint FROM ledger
                       WHERE account_id='dual-drain' AND kind='charge')",
                &[],
            )
            .unwrap();
        (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4))
    };
    assert_eq!(drain_state, (10_020, 0, 40, "settled".to_string(), 1));

    // After the opt-out every non-release reserve writer serves the drain account again.
    assert_eq!(
        pg.reserve_request(
            &owner,
            "dual-drain-plain-request",
            "dual-drain",
            "dual-drain-key",
            50,
            600,
        )
        .unwrap(),
        Some(9_970)
    );
    assert_eq!(
        pg.cancel_request("dual-drain-plain-request").unwrap(),
        Some(10_020)
    );
    assert!(matches!(
        pg.reserve_request_with_legacy_snapshot(
            &owner,
            "dual-drain-key",
            600,
            &legacy_snapshot("dual-drain-scalar-request", "dual-drain", 100, 100),
        )
        .unwrap(),
        crate::pricing::LegacyScalarReserveOutcome::Inserted(_)
    ));
    assert_eq!(
        pg.cancel_request("dual-drain-scalar-request").unwrap(),
        Some(10_020)
    );

    // G. The head still closes the non-opted account on every writer.
    let still_closed = pg
        .reserve_request(&owner, "dual-ctl-plain-2", "dual-ctl", "dual-ctl-key", 100, 600)
        .expect_err("plain reserve of a non-opted account must remain closed");
    assert!(still_closed
        .downcast_ref::<crate::pricing::LegacyPricingPathClosedV2>()
        .is_some());
    assert!(matches!(
        pg.reserve_request_with_legacy_snapshot(
            &owner,
            "dual-ctl-key",
            600,
            &legacy_snapshot("dual-ctl-scalar-2", "dual-ctl", 100, 100),
        )
        .unwrap(),
        crate::pricing::LegacyScalarReserveOutcome::Conflict(
            crate::pricing::LegacyScalarReserveConflict::ActivePricingRelease
        )
    ));

    pg.client
        .batch_execute(
            "TRUNCATE pricing_release_policy_versions,pricing_release_versions,
             account_policy_bindings,account_policy_rules,account_policy_versions,
             provider_switch_head,provider_switch_entries,provider_switch_versions,
             pricing_catalog_heads,pricing_catalog_entries,pricing_catalog_versions,
             execution_group_winner,settlement_outbox,reservations,capacity_leases,
             leader_leases,engine_instances,usage_events,ledger,api_keys,accounts,pool_state,
             subs RESTART IDENTITY CASCADE",
        )
        .unwrap();
    pg.client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}

/// Run with an isolated database, for example:
/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::pre_cutover_funding_snapshot_postgres_matrix`
#[test]
fn pre_cutover_funding_snapshot_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping pre-cutover funding v2 PostgreSQL matrix: test URL is unset");
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
    assert_eq!(pg.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    pg.migrate().unwrap();
    assert_eq!(pg.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    pg.client
        .batch_execute(
            "BEGIN;
             INSERT INTO accounts(
                 id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status,created_ts,created
             ) VALUES
                 ('pre-cutover-v2','pre-cutover-v2',900,0,100,5000,'active',100,'matrix'),
                 ('pre-cutover-legacy','pre-cutover-legacy',1000,0,0,5000,
                  'active',100,'matrix');
             INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts
             ) VALUES(
                 'pre-cutover-v2',1,2,'pre-cutover-source','pre-cutover-normalization',
                 900,100,0,1,100,100
             );
             INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES
                 ('pre-cutover-bonus','pre-cutover-v2',1,'welcome_bonus',
                  'signup-bonus:user',0,60,0,1,'exhausted',100,100),
                 ('pre-cutover-paid','pre-cutover-v2',1,'paid','normalized-paid',
                  900,40,0,1,'active',100,100);
             INSERT INTO reservations(
                 request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                 owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts
             ) VALUES
                 ('pre-cutover-request','pre-cutover-v2','pre-cutover-key',100,900,
                  'pre-cutover-owner',1,1000,'reserved',100,100),
                 ('legacy-request','pre-cutover-legacy','legacy-key',0,1000,
                  'pre-cutover-owner',1,1000,'reserved',100,100);
             INSERT INTO funding_reservation_snapshots_v2(
                 request_id,account_id,funding_schema_version,funding_generation,
                 funding_head_version,hold_nano,snapshot_digest,created_ts
             ) VALUES(
                 'pre-cutover-request','pre-cutover-v2',2,1,1,100,
                 'pre-cutover-snapshot-digest',100
             );
             INSERT INTO funding_reservation_allocations_v2(
                 request_id,account_id,funding_generation,allocation_order,lot_id,
                 lot_source_type,lot_version,reserved_nano,charged_nano,released_nano
             ) VALUES
                 ('pre-cutover-request','pre-cutover-v2',1,1,'pre-cutover-bonus',
                  'welcome_bonus',1,60,NULL,NULL),
                 ('pre-cutover-request','pre-cutover-v2',1,2,'pre-cutover-paid',
                  'paid',1,40,NULL,NULL);
             INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts
             ) VALUES('pre-cutover-v2',1,1,100);
             SET CONSTRAINTS ALL IMMEDIATE;
             SET CONSTRAINTS ALL DEFERRED;",
        )
        .unwrap();

    pg.client
        .batch_execute("SAVEPOINT missing_normalized_snapshot")
        .unwrap();
    let missing_snapshot = pg
        .client
        .batch_execute(
            "INSERT INTO reservations(
                 request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                 owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts
             ) VALUES(
                 'missing-pre-cutover-snapshot','pre-cutover-v2','missing-key',0,900,
                 'pre-cutover-owner',1,1000,'reserved',100,100
             );
             SET CONSTRAINTS ALL IMMEDIATE;",
        )
        .expect_err("normalized reservation without one funding snapshot must fail");
    assert!(missing_snapshot.as_db_error().is_some_and(|error| {
        error
            .message()
            .contains("lacks one compatible funding v2 snapshot")
    }));
    pg.client
        .batch_execute(
            "ROLLBACK TO SAVEPOINT missing_normalized_snapshot;
             RELEASE SAVEPOINT missing_normalized_snapshot;
             SET CONSTRAINTS ALL DEFERRED;",
        )
        .unwrap();

    pg.client
        .batch_execute("SAVEPOINT delete_active_snapshot")
        .unwrap();
    let deleted_snapshot = pg
        .client
        .batch_execute(
            "DELETE FROM funding_reservation_snapshots_v2
              WHERE request_id='pre-cutover-request';
             SET CONSTRAINTS ALL IMMEDIATE;",
        )
        .expect_err("active normalized reservation must retain its funding snapshot");
    assert!(deleted_snapshot.as_db_error().is_some_and(|error| {
        error
            .message()
            .contains("lacks one compatible funding v2 snapshot")
    }));
    pg.client
        .batch_execute(
            "ROLLBACK TO SAVEPOINT delete_active_snapshot;
             RELEASE SAVEPOINT delete_active_snapshot;
             SET CONSTRAINTS ALL DEFERRED;",
        )
        .unwrap();

    pg.client
        .batch_execute("SAVEPOINT delete_funding_head")
        .unwrap();
    let deleted_head = pg
        .client
        .batch_execute("DELETE FROM account_funding_head_v2 WHERE account_id='pre-cutover-v2';")
        .expect_err("a normalized account cannot return to legacy funding writers");
    assert!(deleted_head.as_db_error().is_some_and(|error| {
        error
            .message()
            .contains("funding v2 head cannot be deleted")
    }));
    pg.client
        .batch_execute(
            "ROLLBACK TO SAVEPOINT delete_funding_head;
             RELEASE SAVEPOINT delete_funding_head;",
        )
        .unwrap();

    pg.client
        .batch_execute("SAVEPOINT paid_before_bonus")
        .unwrap();
    let wrong_order = pg
        .client
        .batch_execute(
            "INSERT INTO reservations(
                 request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                 owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts
             ) VALUES(
                 'wrong-order-request','pre-cutover-v2','wrong-order-key',10,900,
                 'pre-cutover-owner',1,1000,'reserved',100,100
             );
             INSERT INTO funding_reservation_snapshots_v2(
                 request_id,account_id,funding_schema_version,funding_generation,
                 funding_head_version,hold_nano,snapshot_digest,created_ts
             ) VALUES(
                 'wrong-order-request','pre-cutover-v2',2,1,1,10,
                 'wrong-order-snapshot-digest',100
             );
             INSERT INTO funding_reservation_allocations_v2(
                 request_id,account_id,funding_generation,allocation_order,lot_id,
                 lot_source_type,lot_version,reserved_nano,charged_nano,released_nano
             ) VALUES
                 ('wrong-order-request','pre-cutover-v2',1,1,'pre-cutover-paid',
                  'paid',1,5,NULL,NULL),
                 ('wrong-order-request','pre-cutover-v2',1,2,'pre-cutover-bonus',
                  'welcome_bonus',1,5,NULL,NULL);
             SET CONSTRAINTS ALL IMMEDIATE;",
        )
        .expect_err("pre-cutover funding allocation must remain bonus-first");
    assert!(wrong_order
        .as_db_error()
        .is_some_and(|error| { error.message().contains("do not cover hold bonus-first") }));
    pg.client
        .batch_execute(
            "ROLLBACK TO SAVEPOINT paid_before_bonus;
             RELEASE SAVEPOINT paid_before_bonus;
             SET CONSTRAINTS ALL DEFERRED;",
        )
        .unwrap();

    pg.client
        .batch_execute(
            "UPDATE accounts
                SET balance_nano=880,reserved_nano=0,spent_nano=120
              WHERE id='pre-cutover-v2';
             UPDATE account_funding_generations_v2
                SET balance_nano=880,reserved_nano=0,spent_nano=120,version=2,updated_ts=200
              WHERE account_id='pre-cutover-v2' AND generation=1;
             UPDATE funding_lots_v2
                SET reserved_nano=0,spent_nano=60,version=2,updated_ts=200
              WHERE lot_id='pre-cutover-bonus';
             UPDATE funding_lots_v2
                SET balance_nano=880,reserved_nano=0,spent_nano=60,version=2,updated_ts=200
              WHERE lot_id='pre-cutover-paid';
             UPDATE funding_reservation_allocations_v2
                SET charged_nano=60,released_nano=0
              WHERE request_id='pre-cutover-request' AND allocation_order=1;
             UPDATE funding_reservation_allocations_v2
                SET charged_nano=60,released_nano=0
              WHERE request_id='pre-cutover-request' AND allocation_order=2;
             UPDATE reservations
                SET state='settled',actual_nano=120,settled_ts=200,updated_ts=200
              WHERE request_id='pre-cutover-request';
             SET CONSTRAINTS ALL IMMEDIATE;",
        )
        .unwrap();

    let terminal = pg
        .client
        .query_one(
            "SELECT account.balance_nano,account.reserved_nano,account.spent_nano,
                    generation.balance_nano,generation.reserved_nano,generation.spent_nano,
                    sum(allocation.charged_nano)::bigint
               FROM accounts account
               JOIN account_funding_generations_v2 generation
                 ON generation.account_id=account.id AND generation.generation=1
               JOIN funding_reservation_allocations_v2 allocation
                 ON allocation.request_id='pre-cutover-request'
              WHERE account.id='pre-cutover-v2'
              GROUP BY account.balance_nano,account.reserved_nano,account.spent_nano,
                       generation.balance_nano,generation.reserved_nano,
                       generation.spent_nano",
            &[],
        )
        .unwrap();
    assert_eq!(
        (
            terminal.get::<_, i64>(0),
            terminal.get::<_, i64>(1),
            terminal.get::<_, i64>(2),
            terminal.get::<_, i64>(3),
            terminal.get::<_, i64>(4),
            terminal.get::<_, i64>(5),
            terminal.get::<_, i64>(6),
        ),
        (880, 0, 120, 880, 0, 120, 120),
    );

    pg.client.batch_execute("ROLLBACK").unwrap();
    lock_holder
        .client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}

fn seed_active_funding_v2(pg: &mut PgStore, account_id: &str, lots: &[(&str, &str, &str, i64)]) {
    let account = pg
        .client
        .query_one(
            "SELECT balance_nano,reserved_nano,spent_nano FROM accounts WHERE id=$1",
            &[&account_id],
        )
        .unwrap();
    let balance_nano: i64 = account.get(0);
    let reserved_nano: i64 = account.get(1);
    let spent_nano: i64 = account.get(2);
    assert_eq!(
        reserved_nano, 0,
        "test normalization requires no active hold"
    );
    assert_eq!(spent_nano, 0, "test normalization starts before usage");
    let lot_balance = lots.iter().try_fold(0_i64, |total, lot| {
        total.checked_add(lot.3).ok_or("test lot balance overflow")
    });
    assert_eq!(lot_balance.unwrap(), balance_nano);

    let timestamp = now();
    let source_state_digest = format!("writer-source:{account_id}");
    let normalization_digest = format!("writer-normalization:{account_id}");
    let mut tx = pg.client.transaction().unwrap();
    tx.execute(
        "INSERT INTO account_funding_generations_v2(
             account_id,generation,schema_version,source_state_digest,
             normalization_digest,balance_nano,reserved_nano,spent_nano,version,
             normalized_ts,updated_ts)
         VALUES($1,1,2,$2,$3,$4,0,0,1,$5,$5)",
        &[
            &account_id,
            &source_state_digest,
            &normalization_digest,
            &balance_nano,
            &timestamp,
        ],
    )
    .unwrap();
    for &(lot_id, source_type, source_ref, lot_balance) in lots {
        let status = if lot_balance > 0 {
            "active"
        } else {
            "exhausted"
        };
        tx.execute(
            "INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts)
             VALUES($1,$2,1,$3,$4,$5,0,0,1,$6,$7,$7)",
            &[
                &lot_id,
                &account_id,
                &source_type,
                &source_ref,
                &lot_balance,
                &status,
                &timestamp,
            ],
        )
        .unwrap();
    }
    tx.execute(
        "INSERT INTO account_funding_head_v2(
             account_id,active_generation,head_version,updated_ts)
         VALUES($1,1,1,$2)",
        &[&account_id, &timestamp],
    )
    .unwrap();
    tx.commit().unwrap();
}

fn funding_v2_money_state(pg: &mut PgStore, account_id: &str, request_id: &str) -> String {
    pg.client
        .query_one(
            "SELECT jsonb_build_object(
                 'account',(
                     SELECT jsonb_build_array(balance_nano,reserved_nano,spent_nano)
                     FROM accounts WHERE id=$1
                 ),
                 'generation',(
                     SELECT jsonb_build_array(balance_nano,reserved_nano,spent_nano,version)
                     FROM account_funding_generations_v2
                     WHERE account_id=$1 AND generation=1
                 ),
                 'lots',(
                     SELECT COALESCE(jsonb_agg(jsonb_build_array(
                         lot_id,source_type,balance_nano,reserved_nano,spent_nano,version,status
                     ) ORDER BY lot_id),'[]'::jsonb)
                     FROM funding_lots_v2
                     WHERE account_id=$1 AND funding_generation=1
                 ),
                 'snapshot',(
                     SELECT jsonb_build_array(
                         funding_generation,funding_head_version,hold_nano,snapshot_digest
                     )
                     FROM funding_reservation_snapshots_v2 WHERE request_id=$2
                 ),
                 'allocations',(
                     SELECT COALESCE(jsonb_agg(jsonb_build_array(
                         allocation_order,lot_id,lot_source_type,lot_version,reserved_nano,
                         charged_nano,released_nano
                     ) ORDER BY allocation_order),'[]'::jsonb)
                     FROM funding_reservation_allocations_v2 WHERE request_id=$2
                 ),
                 'ledger_allocations',(
                     SELECT COALESCE(jsonb_agg(jsonb_build_array(
                         allocation.allocation_order,allocation.lot_id,
                         allocation.lot_source_type,allocation.lot_version,
                         allocation.direction,allocation.amount_nano
                     ) ORDER BY allocation.allocation_order),'[]'::jsonb)
                     FROM funding_ledger_allocations_v2 allocation
                     JOIN ledger ON ledger.id=allocation.ledger_id
                     WHERE ledger.request_id=$2
                 )
             )::text",
            &[&account_id, &request_id],
        )
        .unwrap()
        .get(0)
}

fn wait_for_postgres_lock(client: &mut Client, application_name: &str) {
    for _ in 0..100 {
        let waiting: bool = client
            .query_one(
                "SELECT EXISTS(
                     SELECT 1 FROM pg_stat_activity
                     WHERE application_name=$1 AND wait_event_type='Lock'
                 )",
                &[&application_name],
            )
            .unwrap()
            .get(0);
        if waiting {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("PostgreSQL writer {application_name} did not reach the expected lock wait");
}

/// Real PostgreSQL proof for the pre-cutover Stage 6 dual writers. The matrix covers exact
/// replay, bonus-first allocation, cancellation, paid overrun, top-up classifications,
/// durable outbox recovery, and both account-lock ordering races.
///
/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::pre_cutover_funding_v2_writer_postgres_matrix`
#[test]
fn pre_cutover_funding_v2_writer_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping funding v2 writer PostgreSQL matrix: test URL is unset");
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
            "TRUNCATE execution_group_winner,settlement_outbox,reservations,capacity_leases,
             leader_leases,engine_instances,usage_events,ledger,api_keys,accounts,pool_state,
             subs RESTART IDENTITY CASCADE",
        )
        .unwrap();
    let owner = pg.claim_instance("funding-v2-writer", 600).unwrap();

    const BILLION: i64 = 1_000_000_000;
    pg.account_create("funding-v2-main", None, 5_000).unwrap();
    assert_eq!(
        pg.account_topup(
            "funding-v2-main",
            5 * BILLION,
            Some("signup-bonus:writer-seed"),
        )
        .unwrap(),
        Some(5 * BILLION)
    );
    assert_eq!(
        pg.account_topup("funding-v2-main", 10 * BILLION, Some("platega:writer-seed"),)
            .unwrap(),
        Some(15 * BILLION)
    );
    pg.key_issue("funding-v2-main-key", "funding-v2-main", None)
        .unwrap();
    seed_active_funding_v2(
        &mut pg,
        "funding-v2-main",
        &[
            (
                "funding-v2-main-bonus",
                "welcome_bonus",
                "signup-bonus:writer-seed",
                5 * BILLION,
            ),
            (
                "funding-v2-main-paid",
                "paid",
                "platega:writer-seed",
                10 * BILLION,
            ),
        ],
    );

    assert_eq!(
        pg.reserve_request(
            &owner,
            "funding-v2-main-settle",
            "funding-v2-main",
            "funding-v2-main-key",
            7 * BILLION,
            60,
        )
        .unwrap(),
        Some(8 * BILLION)
    );
    let allocation_order: String = pg
        .client
        .query_one(
            "SELECT string_agg(
                 lot_source_type || ':' || reserved_nano::text,',' ORDER BY allocation_order
             ) FROM funding_reservation_allocations_v2 WHERE request_id=$1",
            &[&"funding-v2-main-settle"],
        )
        .unwrap()
        .get(0);
    assert_eq!(allocation_order, "welcome_bonus:5000000000,paid:2000000000");
    let reserved_state =
        funding_v2_money_state(&mut pg, "funding-v2-main", "funding-v2-main-settle");
    assert_eq!(
        pg.reserve_request(
            &owner,
            "funding-v2-main-settle",
            "funding-v2-main",
            "funding-v2-main-key",
            7 * BILLION,
            60,
        )
        .unwrap(),
        Some(8 * BILLION)
    );
    assert_eq!(
        funding_v2_money_state(&mut pg, "funding-v2-main", "funding-v2-main-settle"),
        reserved_state,
        "exact reserve replay must not repeat any funding mutation"
    );

    assert_eq!(
        pg.settle_request(
            "funding-v2-main-settle",
            6 * BILLION,
            Some("funding-v2-main-charge"),
            None,
        )
        .unwrap(),
        Some(9 * BILLION)
    );
    let terminal_allocations: String = pg
        .client
        .query_one(
            "SELECT string_agg(
                 lot_source_type || ':' || charged_nano::text || ':' || released_nano::text,
                 ',' ORDER BY allocation_order
             ) FROM funding_reservation_allocations_v2 WHERE request_id=$1",
            &[&"funding-v2-main-settle"],
        )
        .unwrap()
        .get(0);
    assert_eq!(
        terminal_allocations,
        "welcome_bonus:5000000000:0,paid:1000000000:1000000000"
    );
    let settlement_ledger: String = pg
        .client
        .query_one(
            "SELECT string_agg(
                 allocation.lot_source_type || ':' || allocation.amount_nano::text,
                 ',' ORDER BY allocation.allocation_order
             )
             FROM funding_ledger_allocations_v2 allocation
             JOIN ledger ON ledger.id=allocation.ledger_id
             WHERE ledger.request_id=$1",
            &[&"funding-v2-main-settle"],
        )
        .unwrap()
        .get(0);
    assert_eq!(
        settlement_ledger,
        "welcome_bonus:5000000000,paid:1000000000"
    );
    let terminal_state =
        funding_v2_money_state(&mut pg, "funding-v2-main", "funding-v2-main-settle");
    assert_eq!(
        pg.settle_request(
            "funding-v2-main-settle",
            6 * BILLION,
            Some("funding-v2-main-charge"),
            None,
        )
        .unwrap(),
        Some(9 * BILLION)
    );
    assert_eq!(
        funding_v2_money_state(&mut pg, "funding-v2-main", "funding-v2-main-settle"),
        terminal_state,
        "terminal replay must validate immutable evidence without repeating money writes"
    );

    assert_eq!(
        pg.reserve_request(
            &owner,
            "funding-v2-main-cancel",
            "funding-v2-main",
            "funding-v2-main-key",
            2 * BILLION,
            60,
        )
        .unwrap(),
        Some(7 * BILLION)
    );
    assert_eq!(
        pg.cancel_request("funding-v2-main-cancel").unwrap(),
        Some(9 * BILLION)
    );
    let canceled: (String, i64, i64, i64) = {
        let row = pg
            .client
            .query_one(
                "SELECT reservation.state,allocation.charged_nano,
                        allocation.released_nano,
                        (SELECT COUNT(*)::bigint FROM ledger
                         WHERE request_id=reservation.request_id)
                 FROM reservations reservation
                 JOIN funding_reservation_allocations_v2 allocation USING(request_id)
                 WHERE reservation.request_id=$1",
                &[&"funding-v2-main-cancel"],
            )
            .unwrap();
        (row.get(0), row.get(1), row.get(2), row.get(3))
    };
    assert_eq!(canceled, ("canceled".into(), 0, 2 * BILLION, 0));

    assert_eq!(
        pg.account_topup(
            "funding-v2-main",
            3 * BILLION,
            Some("platega:writer-post-head"),
        )
        .unwrap(),
        Some(12 * BILLION)
    );
    assert_eq!(
        pg.account_topup(
            "funding-v2-main",
            3 * BILLION,
            Some("platega:writer-post-head"),
        )
        .unwrap(),
        Some(12 * BILLION),
        "exact top-up replay must not append a second funding lot mutation"
    );
    assert_eq!(
        pg.account_topup(
            "funding-v2-main",
            5 * BILLION,
            Some("signup-bonus:writer-post-head"),
        )
        .unwrap(),
        Some(17 * BILLION)
    );
    assert_eq!(
        pg.account_topup(
            "funding-v2-main",
            -BILLION,
            Some("manual-adjust:writer-post-head"),
        )
        .unwrap(),
        Some(16 * BILLION)
    );
    let topup_allocations: String = pg
        .client
        .query_one(
            "SELECT string_agg(
                 lot.source_type || ':' || allocation.direction || ':' ||
                 allocation.amount_nano::text,',' ORDER BY ledger.ref
             )
             FROM funding_ledger_allocations_v2 allocation
             JOIN ledger ON ledger.id=allocation.ledger_id
             JOIN funding_lots_v2 lot ON lot.lot_id=allocation.lot_id
             WHERE ledger.ref IN(
                 'platega:writer-post-head',
                 'signup-bonus:writer-post-head',
                 'manual-adjust:writer-post-head'
             )",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(
        topup_allocations,
        "paid:debit:1000000000,paid:credit:3000000000,welcome_bonus:credit:5000000000"
    );
    let aggregate: (i64, i64, i64) = {
        let row = pg
            .client
            .query_one(
                "SELECT account.balance_nano,generation.balance_nano,
                        sum(lot.balance_nano)::bigint
                 FROM accounts account
                 JOIN account_funding_generations_v2 generation
                   ON generation.account_id=account.id AND generation.generation=1
                 JOIN funding_lots_v2 lot ON lot.account_id=account.id
                 WHERE account.id=$1
                 GROUP BY account.balance_nano,generation.balance_nano",
                &[&"funding-v2-main"],
            )
            .unwrap();
        (row.get(0), row.get(1), row.get(2))
    };
    assert_eq!(aggregate, (16 * BILLION, 16 * BILLION, 16 * BILLION));

    assert_eq!(
        pg.reserve_request(
            &owner,
            "funding-v2-outbox-recovery",
            "funding-v2-main",
            "funding-v2-main-key",
            4 * BILLION,
            60,
        )
        .unwrap(),
        Some(12 * BILLION)
    );
    pg.enqueue_settlement(
        "funding-v2-outbox-recovery",
        3 * BILLION,
        Some("funding-v2-outbox-recovery"),
        None,
    )
    .unwrap();
    let mut recovery = PgStore::connect(&url).unwrap();
    assert_eq!(recovery.drain_outbox(10).unwrap(), 1);
    let recovered: (String, String, i64, i64) = {
        let row = pg
            .client
            .query_one(
                "SELECT reservation.state,outbox.state,account.balance_nano,
                        generation.balance_nano
                 FROM reservations reservation
                 JOIN settlement_outbox outbox USING(request_id)
                 JOIN accounts account ON account.id=reservation.account_id
                 JOIN account_funding_generations_v2 generation
                   ON generation.account_id=account.id AND generation.generation=1
                 WHERE reservation.request_id=$1",
                &[&"funding-v2-outbox-recovery"],
            )
            .unwrap();
        (row.get(0), row.get(1), row.get(2), row.get(3))
    };
    assert_eq!(
        recovered,
        ("settled".into(), "done".into(), 13 * BILLION, 13 * BILLION)
    );
    let generation_advance_ts = now();
    pg.client.batch_execute("BEGIN").unwrap();
    pg.client
        .execute(
            "INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts)
             SELECT id,2,2,'writer-source:generation-2','writer-normalization:generation-2',
                    balance_nano,reserved_nano,spent_nano,1,$1,$1
             FROM accounts WHERE id='funding-v2-main'",
            &[&generation_advance_ts],
        )
        .unwrap();
    pg.client
        .execute(
            "INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts)
             SELECT
                 'funding-v2-main-generation-2-' || source_type,
                 account_id,2,source_type,'generation-2-' || source_type,
                 sum(balance_nano)::bigint,sum(reserved_nano)::bigint,sum(spent_nano)::bigint,
                 1,CASE WHEN sum(balance_nano)>0 THEN 'active' ELSE 'exhausted' END,$1,$1
             FROM funding_lots_v2
             WHERE account_id='funding-v2-main' AND funding_generation=1
             GROUP BY account_id,source_type",
            &[&generation_advance_ts],
        )
        .unwrap();
    pg.client
        .execute(
            "UPDATE account_funding_head_v2
             SET active_generation=2,head_version=2,updated_ts=$1
             WHERE account_id='funding-v2-main'",
            &[&generation_advance_ts],
        )
        .unwrap();
    pg.client.batch_execute("COMMIT").unwrap();
    let post_advance_state =
        funding_v2_money_state(&mut pg, "funding-v2-main", "funding-v2-main-settle");
    assert_eq!(
        pg.settle_request(
            "funding-v2-main-settle",
            6 * BILLION,
            Some("funding-v2-main-charge"),
            None,
        )
        .unwrap(),
        Some(13 * BILLION)
    );
    assert_eq!(
        funding_v2_money_state(&mut pg, "funding-v2-main", "funding-v2-main-settle"),
        post_advance_state,
        "terminal replay must remain valid after the active funding generation advances"
    );

    pg.account_create("funding-v2-overrun", None, 5_000)
        .unwrap();
    pg.account_topup(
        "funding-v2-overrun",
        2 * BILLION,
        Some("signup-bonus:overrun-seed"),
    )
    .unwrap();
    pg.key_issue("funding-v2-overrun-key", "funding-v2-overrun", None)
        .unwrap();
    seed_active_funding_v2(
        &mut pg,
        "funding-v2-overrun",
        &[
            (
                "funding-v2-overrun-bonus",
                "welcome_bonus",
                "signup-bonus:overrun-seed",
                2 * BILLION,
            ),
            (
                "funding-v2-overrun-paid",
                "paid",
                "normalized-paid-anchor",
                0,
            ),
        ],
    );
    assert_eq!(
        pg.reserve_request(
            &owner,
            "funding-v2-paid-overrun",
            "funding-v2-overrun",
            "funding-v2-overrun-key",
            BILLION,
            60,
        )
        .unwrap(),
        Some(BILLION)
    );
    let overrun_reserve: String = pg
        .client
        .query_one(
            "SELECT string_agg(
                 lot_source_type || ':' || reserved_nano::text,',' ORDER BY allocation_order
             ) FROM funding_reservation_allocations_v2 WHERE request_id=$1",
            &[&"funding-v2-paid-overrun"],
        )
        .unwrap()
        .get(0);
    assert_eq!(overrun_reserve, "welcome_bonus:1000000000,paid:0");
    assert_eq!(
        pg.settle_request(
            "funding-v2-paid-overrun",
            BILLION + 500_000_000,
            Some("funding-v2-paid-overrun"),
            None,
        )
        .unwrap(),
        Some(500_000_000)
    );
    let overrun_terminal: String = pg
        .client
        .query_one(
            "SELECT string_agg(
                 lot_source_type || ':' || reserved_nano::text || ':' ||
                 charged_nano::text || ':' || released_nano::text,
                 ',' ORDER BY allocation_order
             ) FROM funding_reservation_allocations_v2 WHERE request_id=$1",
            &[&"funding-v2-paid-overrun"],
        )
        .unwrap()
        .get(0);
    assert_eq!(
        overrun_terminal,
        "welcome_bonus:1000000000:1000000000:0,paid:0:500000000:0"
    );

    pg.account_create("funding-v2-wait", None, 5_000).unwrap();
    pg.account_topup("funding-v2-wait", 2 * BILLION, Some("platega:wait-seed"))
        .unwrap();
    pg.key_issue("funding-v2-wait-key", "funding-v2-wait", None)
        .unwrap();
    let mut normalizer = PgStore::connect(&url).unwrap();
    let mut normalization = normalizer.client.transaction().unwrap();
    let funding_lock = "funding-v2-account:funding-v2-wait";
    normalization
        .query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&funding_lock],
        )
        .unwrap();
    let normalization_ts = now();
    normalization
        .execute(
            "INSERT INTO account_funding_generations_v2(
                 account_id,generation,schema_version,source_state_digest,
                 normalization_digest,balance_nano,reserved_nano,spent_nano,version,
                 normalized_ts,updated_ts)
             VALUES('funding-v2-wait',1,2,'wait-source','wait-normalization',
                    $1,0,0,1,$2,$2)",
            &[&(2 * BILLION), &normalization_ts],
        )
        .unwrap();
    normalization
        .execute(
            "INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts)
             VALUES('funding-v2-wait-paid','funding-v2-wait',1,'paid','platega:wait-seed',
                    $1,0,0,1,'active',$2,$2)",
            &[&(2 * BILLION), &normalization_ts],
        )
        .unwrap();
    normalization
        .execute(
            "INSERT INTO account_funding_head_v2(
                 account_id,active_generation,head_version,updated_ts)
             VALUES('funding-v2-wait',1,1,$1)",
            &[&normalization_ts],
        )
        .unwrap();

    let (reserve_tx, reserve_rx) = std::sync::mpsc::channel();
    let reserve_url = url.clone();
    let reserve_owner = owner.clone();
    let reserve_thread = std::thread::spawn(move || {
        let mut writer =
            PgStore::connect_with_application_name(&reserve_url, "funding-v2-normalization-writer")
                .unwrap();
        reserve_tx
            .send(
                writer
                    .reserve_request(
                        &reserve_owner,
                        "funding-v2-after-normalization",
                        "funding-v2-wait",
                        "funding-v2-wait-key",
                        BILLION,
                        60,
                    )
                    .map_err(|error| format!("{error:#}")),
            )
            .unwrap();
    });
    wait_for_postgres_lock(&mut pg.client, "funding-v2-normalization-writer");
    normalization.commit().unwrap();
    assert_eq!(
        reserve_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap(),
        Some(BILLION)
    );
    reserve_thread.join().unwrap();
    let snapshot_count: i64 = pg
        .client
        .query_one(
            "SELECT COUNT(*)::bigint FROM funding_reservation_snapshots_v2
             WHERE request_id='funding-v2-after-normalization'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(
        snapshot_count, 1,
        "writer waiting behind normalization must reread and use the new funding head"
    );
    assert_eq!(
        pg.cancel_request("funding-v2-after-normalization").unwrap(),
        Some(2 * BILLION)
    );

    assert_eq!(
        pg.reserve_request(
            &owner,
            "funding-v2-settlement-lock-order",
            "funding-v2-wait",
            "funding-v2-wait-key",
            BILLION,
            60,
        )
        .unwrap(),
        Some(BILLION)
    );
    pg.enqueue_settlement(
        "funding-v2-settlement-lock-order",
        BILLION,
        Some("funding-v2-settlement-lock-order"),
        None,
    )
    .unwrap();
    let mut account_locker = PgStore::connect(&url).unwrap();
    let mut account_lock = account_locker.client.transaction().unwrap();
    account_lock
        .query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&funding_lock],
        )
        .unwrap();
    let (settlement_tx, settlement_rx) = std::sync::mpsc::channel();
    let settlement_url = url.clone();
    let settlement_thread = std::thread::spawn(move || {
        let mut writer =
            PgStore::connect_with_application_name(&settlement_url, "funding-v2-settlement-writer")
                .unwrap();
        settlement_tx
            .send(
                writer
                    .process_outbox_request("funding-v2-settlement-lock-order")
                    .map_err(|error| format!("{error:#}")),
            )
            .unwrap();
    });
    wait_for_postgres_lock(&mut pg.client, "funding-v2-settlement-writer");
    let mut observer = PgStore::connect(&url).unwrap();
    let mut observer_tx = observer.client.transaction().unwrap();
    observer_tx
        .query_one(
            "SELECT request_id FROM reservations
             WHERE request_id='funding-v2-settlement-lock-order' FOR UPDATE NOWAIT",
            &[],
        )
        .expect("settlement must not lock the reservation before the account funding lock");
    observer_tx.rollback().unwrap();
    account_lock.commit().unwrap();
    assert_eq!(
        settlement_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap(),
        Some(BILLION)
    );
    settlement_thread.join().unwrap();
    let lock_order_state: (String, String) = {
        let row = pg
            .client
            .query_one(
                "SELECT reservation.state,outbox.state
                 FROM reservations reservation
                 JOIN settlement_outbox outbox USING(request_id)
                 WHERE reservation.request_id='funding-v2-settlement-lock-order'",
                &[],
            )
            .unwrap();
        (row.get(0), row.get(1))
    };
    assert_eq!(lock_order_state, ("settled".into(), "done".into()));

    lock_holder
        .client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
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
        "TRUNCATE execution_group_winner,settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances, \
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

/// PostgreSQL contract of the shadow lineage rebind (B2C→B2B conversion). Skipped without a
/// live database:
/// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
/// pg::tests::postgres_shadow_lineage_rebind_contract`
#[test]
fn postgres_shadow_lineage_rebind_contract() {
    use crate::pricing::{
        AccountPolicyActivationSpec, AccountPolicyBindingSpec, ActiveExpectation,
        ActivePolicyTarget, FundingEnforcement, PolicyActiveExpectation, PolicyEnforcement,
        PricingMutation, PricingRejection, ReconciliationState,
    };

    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!(
            "skipping PostgreSQL shadow lineage rebind contract: \
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
             execution_group_winner,settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances,
             usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE",
        )
        .unwrap();

    pg.account_create("rebind-pg-account", None, 2_000).unwrap();
    let catalog = shadow_pg_catalog(1, "rebind-catalog-1");
    assert_eq!(
        pg.prepare_pricing_catalog(&catalog).unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        pg.activate_pricing_catalog("main", &catalog.target(), &ActiveExpectation::Absent)
            .unwrap(),
        PricingMutation::Applied
    );
    // The switches must pin both segments: the account starts b2c and rebinds to b2b.
    let mut switches = shadow_pg_switches(1, 1, "rebind-switches-1");
    switches.entries.push(crate::pricing::ProviderSwitchEntrySpec {
        provider_id: "anthropic".into(),
        scope: crate::pricing::ProviderSwitchScope::Segment {
            product_id: "main".into(),
            segment: crate::pricing::PolicySegment::B2c,
        },
        catalog_generation: Some(1),
        enabled: true,
    });
    assert_eq!(
        pg.prepare_provider_switches(&switches).unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        pg.activate_provider_switches(&switches.target(), &ActiveExpectation::Absent).unwrap(),
        PricingMutation::Applied
    );

    let shadow_binding = || AccountPolicyBindingSpec {
        policy_enforcement: PolicyEnforcement::Shadow,
        funding_enforcement: FundingEnforcement::Shadow,
        reconciliation_state: ReconciliationState::Pending,
    };
    let activate = |version: i64, digest: &str, binding: AccountPolicyBindingSpec| {
        AccountPolicyActivationSpec {
            account_id: "rebind-pg-account".into(),
            effective_version: version,
            content_digest: digest.into(),
            binding,
        }
    };

    // v1: the account starts on the shared global-b2c lineage, as every B2C signup does.
    let b2c_v1 = crate::pricing::AccountPolicySpec {
        account_id: "rebind-pg-account".into(),
        effective_version: 1,
        policy_id: "global-b2c".into(),
        policy_version: 1,
        source_policy_digest: "b2c-source-1".into(),
        owner_type: crate::pricing::PolicyOwnerType::GlobalB2c,
        owner_id: "global".into(),
        account_class: crate::pricing::AccountClass::B2c,
        product_id: "main".into(),
        schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
        catalog_generation: 1,
        switch_generation: 1,
        content_digest: "rebind-b2c-1".into(),
        replacement_locked: false,
        rules: vec![crate::pricing::AccountPolicyRuleSpec {
            rule_id: "anthropic-track".into(),
            rule_digest: "anthropic-track-digest".into(),
            scope: crate::pricing::PolicyRuleScope::Provider {
                provider_id: "anthropic".into(),
            },
            pricing_mode: crate::pricing::PricingMode::Track,
            rule_origin: crate::pricing::RuleOrigin::Managed,
            discount_bps: None,
            payable_multiplier_bp: 10_000,
            track_eligible: true,
            retention_eligible: true,
            commission_eligible: true,
        }],
    };
    assert_eq!(
        pg.prepare_account_policy(&b2c_v1).unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        pg.activate_account_policy(
            &activate(1, "rebind-b2c-1", shadow_binding()),
            &PolicyActiveExpectation::Unbound,
        )
        .unwrap(),
        PricingMutation::Applied
    );

    // B2C→B2B conversion: new policy_id/owner/class lineage, policy_version restarts at 1,
    // effective_version stays monotonic.
    let mut b2b_v2 = shadow_pg_policy();
    b2b_v2.account_id = "rebind-pg-account".into();
    b2b_v2.effective_version = 2;
    b2b_v2.policy_id = "b2b:rebind-pg-account".into();
    b2b_v2.policy_version = 1;
    b2b_v2.owner_id = "rebind-pg-account".into();
    b2b_v2.source_policy_digest = "b2b-source-1".into();
    b2b_v2.content_digest = "rebind-b2b-2".into();
    assert_eq!(
        pg.prepare_account_policy(&b2b_v2).unwrap(),
        PricingMutation::Stored
    );
    // Idempotent re-prepare of the pending rebind stays unchanged.
    assert_eq!(
        pg.prepare_account_policy(&b2b_v2).unwrap(),
        PricingMutation::Unchanged
    );
    // The CAS pins the exact OLD lineage target and moves the binding row to class b2b.
    assert_eq!(
        pg.activate_account_policy(
            &activate(2, "rebind-b2b-2", shadow_binding()),
            &PolicyActiveExpectation::Unbound,
        )
        .unwrap(),
        PricingMutation::Rejected(PricingRejection::PolicyCasMismatch {
            actual: crate::pricing::PolicyBindingState::Active(ActivePolicyTarget {
                target: b2c_v1.target(),
                binding: shadow_binding(),
            }),
        })
    );
    assert_eq!(
        pg.activate_account_policy(
            &activate(2, "rebind-b2b-2", shadow_binding()),
            &PolicyActiveExpectation::Exact(ActivePolicyTarget {
                target: b2c_v1.target(),
                binding: shadow_binding(),
            }),
        )
        .unwrap(),
        PricingMutation::Applied
    );
    let binding_row = pg
        .client
        .query_one(
            "SELECT account_class, active_effective_version
               FROM account_policy_bindings WHERE account_id='rebind-pg-account'",
            &[],
        )
        .unwrap();
    assert_eq!(binding_row.get::<_, String>(0), "b2b");
    assert_eq!(binding_row.get::<_, i64>(1), 2);

    // The new lineage advances normally, then strict enforcement makes the identity
    // immutable again.
    let mut b2b_v3 = b2b_v2.clone();
    b2b_v3.effective_version = 3;
    b2b_v3.policy_version = 2;
    b2b_v3.source_policy_digest = "b2b-source-2".into();
    b2b_v3.content_digest = "rebind-b2b-3".into();
    assert_eq!(
        pg.prepare_account_policy(&b2b_v3).unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        pg.activate_account_policy(
            &activate(
                3,
                "rebind-b2b-3",
                AccountPolicyBindingSpec {
                    policy_enforcement: PolicyEnforcement::Strict,
                    funding_enforcement: FundingEnforcement::Strict,
                    reconciliation_state: ReconciliationState::Verified,
                },
            ),
            &PolicyActiveExpectation::Exact(ActivePolicyTarget {
                target: b2b_v2.target(),
                binding: shadow_binding(),
            }),
        )
        .unwrap(),
        PricingMutation::Applied
    );
    let mut b2c_v4 = b2c_v1.clone();
    b2c_v4.effective_version = 4;
    b2c_v4.policy_version = 2;
    b2c_v4.source_policy_digest = "b2c-source-2".into();
    b2c_v4.content_digest = "rebind-b2c-4".into();
    assert_eq!(
        pg.prepare_account_policy(&b2c_v4).unwrap(),
        PricingMutation::Rejected(PricingRejection::VersionConflict)
    );

    pg.client
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
                 FROM ledger WHERE request_id='read-request';
             INSERT INTO ledger(
                 account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts,model,
                 provider
             ) VALUES(
                 'read-account','read-key','charge','read-provider-recovery',25,
                 'provider:recovery',875,3,'gpt-read',NULL
             );
             INSERT INTO usage_events(
                 request_id,account_id,key,model,real_nano,charge_nano,ts,provider
             ) VALUES(
                 'read-provider-recovery','read-account','read-key','gpt-read',50,25,3,
                 'openai'
             );
             INSERT INTO ledger(
                 account_id,key,kind,amount_nano,ref,balance_after_nano,ts,model,provider
             ) VALUES
                 ('read-account','read-key','charge',30,'legacy:read',845,4,
                  'gpt-legacy',NULL),
                 ('read-account','read-key','charge',20,'legacy:claude',825,5,
                  'claude-legacy',NULL),
                 ('read-account','read-key','charge',15,'legacy:conflict',810,6,
                  'ambiguous-model',NULL),
                 ('read-account','read-key','charge',10,'legacy:model-only',800,7,
                  'gpt-5',NULL);
             INSERT INTO usage_events(
                 account_id,key,model,real_nano,charge_nano,ref,ts,provider
             ) VALUES
                 ('read-account','read-key','gpt-legacy',60,30,'legacy:read',5,'openai');
             INSERT INTO usage_events(
                 account_id,key,model,real_nano,charge_nano,ref,ts
             ) VALUES
                 ('read-account','read-key','claude-legacy',40,20,'legacy:claude',5);
             INSERT INTO usage_events(
                 account_id,key,model,real_nano,charge_nano,ref,ts,provider
             ) VALUES
                 ('read-account','read-key','ambiguous-model',30,15,'legacy:conflict',6,
                  'openai'),
                 ('read-account','read-key','ambiguous-model',30,15,'legacy:conflict',6,
                  'google'),
                 ('read-account','wrong-key','gpt-5',20,10,'legacy:model-only',7,'openai');
             INSERT INTO usage_events(
                 request_id,account_id,key,model,real_nano,charge_nano,ref,ts,provider
             ) VALUES(
                 'unrelated-read-request','read-account','read-key','gpt-5',20,10,
                 'legacy:model-only',7,'openai'
             );",
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
    assert_eq!(recent.len(), 6);
    assert_eq!(after.len(), 6);
    let attributed = recent
        .iter()
        .find(|row| row.request_id.as_deref() == Some("read-request"))
        .unwrap();
    let recovered = recent
        .iter()
        .find(|row| row.request_id.as_deref() == Some("read-provider-recovery"))
        .unwrap();
    assert_eq!(recovered.provider.as_deref(), Some(crate::PROVIDER_OPENAI));
    let legacy_provider_for = |reference: &str| {
        recent
            .iter()
            .find(|row| row.reference.as_deref() == Some(reference))
            .unwrap()
            .provider
            .as_deref()
    };
    assert_eq!(
        legacy_provider_for("legacy:read"),
        Some(crate::PROVIDER_OPENAI)
    );
    assert_eq!(
        legacy_provider_for("legacy:claude"),
        Some(crate::PROVIDER_ANTHROPIC)
    );
    assert_eq!(legacy_provider_for("legacy:conflict"), None);
    assert_eq!(legacy_provider_for("legacy:model-only"), None);
    assert_eq!(attributed.request_id.as_deref(), Some("read-request"));
    let attribution = attributed.attribution.as_ref().unwrap();
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
        attributed.funding_allocations,
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
    // A previously committed strict binding rejects a policy-incapable runtime before this
    // matrix can exercise the transition itself. Clear the account graph first so execution
    // order across serialized destructive tests cannot change the fixture's starting state.
    pg.client
        .batch_execute(
            "TRUNCATE accounts RESTART IDENTITY CASCADE;
             TRUNCATE engine_instances RESTART IDENTITY CASCADE;
             ALTER SEQUENCE engine_owner_epoch_seq RESTART WITH 2;
             INSERT INTO engine_instances(
                 instance_id,owner_epoch,lease_until,started_ts,updated_ts
             ) VALUES('stage9-guard-engine',1,9999999999,1,1);
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
    assert!(pg
        .reserve_request_with_legacy_snapshot(&owner, "stage9-strict-key", 60, &scalar_snapshot,)
        .is_err());
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
        pg.reserve_request_with_policy_snapshot(&owner, "stage9-strict-key", 60, &strict_snapshot,)
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
