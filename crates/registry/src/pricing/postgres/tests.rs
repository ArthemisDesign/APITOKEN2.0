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
        eprintln!("skipping PostgreSQL pricing contract: CLAUDE_API_TEST_DATABASE_URL is unset");
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
                rule_digest: format!("contract-openkeys-{provider_id}-digest-{effective_version}"),
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
        postgres_account_policy_by_version(&mut client, "pricing-pg-contract-openkeys", 1).unwrap(),
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
        postgres_prepare_pricing_catalog(&mut client, &catalog("history", 1, "history-1")).unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        postgres_prepare_pricing_catalog(&mut client, &catalog("history", 3, "history-3")).unwrap(),
        PricingMutation::Stored
    );
    assert!(matches!(
        postgres_prepare_pricing_catalog(&mut client, &catalog("history", 2, "history-2")).unwrap(),
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
        postgres_prepare_account_policy(&mut client, &b2b_policy(2, 2, "b2b-policy-2")).unwrap(),
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
            binding: openkeys_binding.clone(),
        }
    );

    let mut openkeys_successor = openkeys_v1.clone();
    openkeys_successor.effective_version = 2;
    openkeys_successor.policy_version = 2;
    openkeys_successor.source_policy_digest = "contract-openkeys-managed-source-2".to_owned();
    openkeys_successor.content_digest = "contract-openkeys-managed-policy-2".to_owned();
    openkeys_successor.replacement_locked = false;
    openkeys_successor.rules = ["anthropic", "openai"]
        .into_iter()
        .map(|provider_id| AccountPolicyRuleSpec {
            rule_id: format!("contract-openkeys-{provider_id}-managed-2"),
            rule_digest: format!("contract-openkeys-{provider_id}-managed-digest-2"),
            scope: PolicyRuleScope::Provider {
                provider_id: provider_id.to_owned(),
            },
            pricing_mode: PricingMode::Discount,
            rule_origin: RuleOrigin::Managed,
            discount_bps: Some(0),
            payable_multiplier_bp: 10_000,
            track_eligible: false,
            retention_eligible: false,
            commission_eligible: false,
        })
        .collect();
    let openkeys_transition = LockedOpenKeysPolicyTransitionSpec {
        policy: openkeys_successor.clone(),
        expected_active: ActivePolicyTarget {
            target: openkeys_v1.target(),
            binding: openkeys_binding,
        },
    };
    let mut invalid_openkeys_transition = openkeys_transition.clone();
    invalid_openkeys_transition.policy.rules[0].discount_bps = Some(100);
    invalid_openkeys_transition.policy.rules[0].payable_multiplier_bp = 9_900;
    assert!(matches!(
        postgres_locked_openkeys_policy_transition(&mut client, &invalid_openkeys_transition)
            .unwrap(),
        PricingMutation::Rejected(PricingRejection::Invalid { .. })
    ));

    client
        .batch_execute(
            "CREATE FUNCTION pricing_contract_reject_locked_openkeys_child()
             RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 IF NEW.account_id = 'pricing-pg-contract-openkeys'
                    AND NEW.effective_version = 2
                    AND NEW.provider_id = 'openai' THEN
                     RAISE EXCEPTION 'injected locked OpenKeys child failure';
                 END IF;
                 RETURN NEW;
             END;
             $$;
             CREATE TRIGGER pricing_contract_reject_locked_openkeys_child
             BEFORE INSERT ON account_policy_rules
             FOR EACH ROW EXECUTE FUNCTION pricing_contract_reject_locked_openkeys_child();",
        )
        .unwrap();
    assert!(postgres_locked_openkeys_policy_transition(&mut client, &openkeys_transition).is_err());
    assert_eq!(
        postgres_account_policy_by_version(&mut client, "pricing-pg-contract-openkeys", 2).unwrap(),
        None
    );
    assert_eq!(
        postgres_active_account_policy(&mut client, "pricing-pg-contract-openkeys")
            .unwrap()
            .expect("legacy OpenKeys policy remains active")
            .policy,
        normalize_policy(&openkeys_v1)
    );
    client
        .batch_execute(
            "DROP TRIGGER pricing_contract_reject_locked_openkeys_child
                 ON account_policy_rules;
             DROP FUNCTION pricing_contract_reject_locked_openkeys_child();",
        )
        .unwrap();

    assert_eq!(
        postgres_locked_openkeys_policy_transition(&mut client, &openkeys_transition).unwrap(),
        PricingMutation::Applied
    );
    assert_eq!(
        postgres_locked_openkeys_policy_transition(&mut client, &openkeys_transition).unwrap(),
        PricingMutation::Unchanged
    );
    assert_eq!(
        postgres_active_account_policy(&mut client, "pricing-pg-contract-openkeys").unwrap(),
        Some(ActiveAccountPolicy {
            policy: normalize_policy(&openkeys_successor),
            binding: crate::pricing::locked_openkeys_transition_binding(),
        })
    );

    let mut forbidden_third = openkeys_successor;
    forbidden_third.effective_version = 3;
    forbidden_third.policy_version = 3;
    forbidden_third.content_digest = "contract-openkeys-managed-policy-3".to_owned();
    // The transition consumed the replacement lock: the engine-validated canonical managed
    // 1:1 successor can now advance through the generic CAS lane in later generations.
    assert_eq!(
        postgres_prepare_account_policy(&mut client, &forbidden_third).unwrap(),
        PricingMutation::Stored
    );
    assert_eq!(
        postgres_activate_account_policy(
            &mut client,
            &activation(
                &forbidden_third,
                crate::pricing::locked_openkeys_transition_binding(),
            ),
            &PolicyActiveExpectation::Exact(ActivePolicyTarget {
                target: VersionTarget::new(
                    2,
                    "contract-openkeys-managed-policy-2".to_owned(),
                ),
                binding: crate::pricing::locked_openkeys_transition_binding(),
            }),
        )
        .unwrap(),
        PricingMutation::Applied
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
        assert_eq!(
            postgres_latest_pricing_release_policy_v2(&mut client, &policy.policy_id).unwrap(),
            Some(policy.clone())
        );
    }
    assert_eq!(
        postgres_latest_pricing_release_policy_v2(&mut client, "pricing-v2-producer-missing")
            .unwrap(),
        None
    );
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
    assert_eq!(
        postgres_latest_pricing_release_policy_v2(&mut client, &newest_policy.policy_id).unwrap(),
        Some(newest_policy.clone())
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
        postgres_prepare_pricing_release_recovery_link_v2(&mut client, &missing_recovery).unwrap(),
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
