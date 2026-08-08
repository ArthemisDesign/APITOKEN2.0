use super::*;

fn db() -> Connection {
    open(":memory:").unwrap()
}

/// The roster-backed plane is a closed set. Anthropic is the one that matters: Claude
/// subscriptions already carry `active|paused|disabled`, so letting them through here would give
/// a single subscription two switches that can disagree.
#[test]
fn only_roster_backed_fleets_accept_an_operator_disable() {
    for provider in ROSTER_BACKED_PROVIDERS {
        require_roster_backed_provider(provider).unwrap();
    }
    assert!(require_roster_backed_provider(PROVIDER_ANTHROPIC).is_err());
    assert!(require_roster_backed_provider("").is_err());
    assert!(require_roster_backed_provider("gemini").is_err());
    assert!(require_roster_backed_provider("codex").is_err());
}

#[test]
fn pricing_policy_schema_is_idempotent_and_preserves_legacy_money() {
    let c = db();
    account_create(&c, "legacy", None, 3750).unwrap();
    account_topup(&c, "legacy", 4_000_000_000, Some("legacy-seed")).unwrap();
    account_reserve(&c, "legacy", 125_000_000).unwrap();
    let before: (i64, i64, i64, i64) = c
        .query_row(
            "SELECT balance_nano,spent_nano,reserved_nano,mult_bp \
             FROM accounts WHERE id='legacy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();

    migrate_pricing_policy_schema(&c).unwrap();
    migrate_pricing_policy_schema(&c).unwrap();

    let after: (i64, i64, i64, i64) = c
        .query_row(
            "SELECT balance_nano,spent_nano,reserved_nano,mult_bp \
             FROM accounts WHERE id='legacy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(after, before);
    for table in [
        "pricing_catalog_versions",
        "provider_switch_versions",
        "account_policy_versions",
        "account_policy_bindings",
        "funding_buckets",
        "pricing_admission_snapshots",
        "pricing_shadow_admission_evaluations",
        "execution_group_winner",
    ] {
        let count: i64 = c
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "{table} must stay empty during schema expansion");
    }
    for (table, column) in [
        ("ledger", "request_id"),
        ("usage_events", "request_id"),
        ("billing_reservations", "group_id"),
        ("billing_reservations", "attempt"),
        ("ledger", "provider"),
        ("billing_settlement_outbox", "snapshot_digest"),
        ("usage_events", "funding_allocation_json"),
        ("pricing_catalog_versions", "capability_generation"),
        ("provider_switch_versions", "capability_generation"),
        ("provider_switch_versions", "capability_digest"),
        ("provider_switch_entries", "catalog_generation"),
        ("account_policy_versions", "switch_generation"),
        ("account_policy_versions", "source_policy_digest"),
        ("account_policy_versions", "account_class"),
        ("pricing_admission_snapshots", "source_policy_digest"),
        (
            "pricing_admission_snapshots",
            "admission_catalog_generation",
        ),
        ("pricing_admission_snapshots", "admission_catalog_digest"),
        ("pricing_admission_snapshots", "admission_switch_generation"),
        ("pricing_admission_snapshots", "admission_switch_digest"),
        ("pricing_admission_snapshots", "runtime_manifest_generation"),
        ("pricing_admission_snapshots", "runtime_manifest_digest"),
        ("reservation_funding_allocations", "allocation_order"),
        ("api_keys", "activation_policy_effective_version"),
        ("api_keys", "activation_policy_digest"),
        ("api_keys", "activation_policy_ack_ts"),
        ("billing_settlement_outbox", "source_policy_digest"),
        ("billing_settlement_outbox", "runtime_manifest_digest"),
        ("usage_events", "source_policy_digest"),
        ("usage_events", "runtime_manifest_digest"),
        ("ledger", "source_policy_digest"),
        ("ledger", "runtime_manifest_digest"),
    ] {
        let present: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
                rusqlite::params![table, column],
                |row| row.get(0),
            )
            .unwrap();
        assert!(present, "missing SQLite parity column {table}.{column}");
    }
}

#[test]
fn pricing_policy_schema_upgrades_old_sqlite_runtime_pins_without_orphans() {
    let c = db();
    c.execute_batch(
        "DROP TRIGGER IF EXISTS pricing_catalog_versions_runtime_pins_delete;
         DROP TRIGGER IF EXISTS pricing_catalog_versions_runtime_pins_update;
         DROP TABLE account_policy_bindings;
         DROP TABLE account_policy_rules;
         DROP TABLE account_policy_versions;
         DROP TABLE provider_switch_head;
         DROP TABLE provider_switch_entries;
         DROP TABLE provider_switch_versions;
         DROP TABLE pricing_catalog_heads;
         DROP TABLE pricing_catalog_entries;
         DROP TABLE pricing_catalog_versions;

         CREATE TABLE pricing_catalog_versions (
             product_id TEXT NOT NULL CHECK (product_id <> ''),
             generation INTEGER NOT NULL CHECK (generation > 0),
             schema_version INTEGER NOT NULL CHECK (schema_version > 0),
             capability_digest TEXT NOT NULL CHECK (capability_digest <> ''),
             content_digest TEXT NOT NULL CHECK (content_digest <> ''),
             created_ts INTEGER NOT NULL,
             PRIMARY KEY (product_id, generation)
         );
         CREATE TABLE pricing_catalog_entries (
             product_id TEXT NOT NULL,
             generation INTEGER NOT NULL,
             provider_id TEXT NOT NULL CHECK (provider_id <> ''),
             canonical_model_id TEXT NOT NULL CHECK (canonical_model_id <> ''),
             enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
             PRIMARY KEY (product_id, generation, provider_id, canonical_model_id),
             FOREIGN KEY (product_id, generation)
                 REFERENCES pricing_catalog_versions(product_id, generation)
                 ON DELETE CASCADE
         );
         CREATE TABLE pricing_catalog_heads (
             product_id TEXT PRIMARY KEY CHECK (product_id <> ''),
             active_generation INTEGER NOT NULL CHECK (active_generation > 0),
             updated_ts INTEGER NOT NULL,
             FOREIGN KEY (product_id, active_generation)
                 REFERENCES pricing_catalog_versions(product_id, generation)
                 ON DELETE RESTRICT
         );
         CREATE TABLE provider_switch_versions (
             generation INTEGER PRIMARY KEY CHECK (generation > 0),
             schema_version INTEGER NOT NULL CHECK (schema_version > 0),
             content_digest TEXT NOT NULL CHECK (content_digest <> ''),
             created_ts INTEGER NOT NULL
         );
         CREATE TABLE provider_switch_entries (
             generation INTEGER NOT NULL
                 REFERENCES provider_switch_versions(generation) ON DELETE CASCADE,
             provider_id TEXT NOT NULL CHECK (provider_id <> ''),
             scope_type TEXT NOT NULL
                 CHECK (scope_type IN ('master', 'product', 'segment')),
             product_id TEXT NOT NULL DEFAULT '',
             segment TEXT NOT NULL DEFAULT '',
             enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
             PRIMARY KEY (generation, provider_id, scope_type, product_id, segment),
             CHECK (
                 (scope_type = 'master' AND product_id = '' AND segment = '')
                 OR (scope_type = 'product' AND product_id <> '' AND segment = '')
                 OR (
                     scope_type = 'segment'
                     AND product_id <> ''
                     AND segment IN ('b2c', 'b2b')
                 )
             )
         );
         CREATE TABLE account_policy_versions (
             account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
             effective_version INTEGER NOT NULL CHECK (effective_version > 0),
             policy_id TEXT NOT NULL CHECK (policy_id <> ''),
             policy_version INTEGER NOT NULL CHECK (policy_version > 0),
             owner_type TEXT NOT NULL
                 CHECK (owner_type IN ('global_b2c', 'b2b_client', 'openkeys', 'service')),
             owner_id TEXT NOT NULL CHECK (owner_id <> ''),
             product_id TEXT NOT NULL CHECK (product_id <> ''),
             schema_version INTEGER NOT NULL CHECK (schema_version > 0),
             catalog_generation INTEGER NOT NULL CHECK (catalog_generation > 0),
             content_digest TEXT NOT NULL CHECK (content_digest <> ''),
             replacement_locked INTEGER NOT NULL CHECK (replacement_locked IN (0, 1)),
             created_ts INTEGER NOT NULL,
             PRIMARY KEY (account_id, effective_version),
             UNIQUE (account_id, effective_version, product_id),
             UNIQUE (
                 account_id, effective_version, policy_id, policy_version,
                 product_id, catalog_generation, content_digest
             ),
             FOREIGN KEY (product_id, catalog_generation)
                 REFERENCES pricing_catalog_versions(product_id, generation)
                 ON DELETE RESTRICT
         );
         CREATE TABLE account_policy_bindings (
             account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
             product_id TEXT NOT NULL CHECK (product_id <> ''),
             account_class TEXT NOT NULL
                 CHECK (account_class IN ('b2c', 'b2b', 'openkeys', 'service')),
             active_effective_version INTEGER,
             policy_enforcement TEXT NOT NULL
                 CHECK (policy_enforcement IN ('legacy_scalar', 'shadow', 'strict')),
             funding_enforcement TEXT NOT NULL
                 CHECK (funding_enforcement IN ('legacy_single', 'shadow', 'strict')),
             reconciliation_state TEXT NOT NULL
                 CHECK (reconciliation_state IN ('pending', 'verified', 'exception')),
             updated_ts INTEGER NOT NULL,
             FOREIGN KEY (account_id, active_effective_version, product_id)
                 REFERENCES account_policy_versions(
                     account_id,
                     effective_version,
                     product_id
                 )
                 ON DELETE RESTRICT,
             CHECK (policy_enforcement <> 'strict' OR active_effective_version IS NOT NULL),
             CHECK (
                 funding_enforcement <> 'strict'
                 OR reconciliation_state = 'verified'
             )
         );
         ALTER TABLE account_policy_versions
             ADD COLUMN switch_generation INTEGER;
         CREATE TRIGGER account_policy_versions_runtime_pins_insert
         BEFORE INSERT ON account_policy_versions
         FOR EACH ROW
         WHEN NEW.switch_generation IS NULL
           OR NEW.switch_generation <= 0
           OR NOT EXISTS (
               SELECT 1 FROM provider_switch_versions
               WHERE generation = NEW.switch_generation
           )
         BEGIN
             SELECT RAISE(ABORT, 'invalid account policy switch pin');
         END;
         CREATE TRIGGER account_policy_versions_runtime_pins_update
         BEFORE UPDATE ON account_policy_versions
         FOR EACH ROW
         WHEN NEW.switch_generation IS NULL
           OR NEW.switch_generation <= 0
           OR NOT EXISTS (
               SELECT 1 FROM provider_switch_versions
               WHERE generation = NEW.switch_generation
           )
         BEGIN
             SELECT RAISE(ABORT, 'invalid account policy switch pin');
         END;",
    )
    .unwrap();

    migrate_pricing_policy_schema(&c).unwrap();
    let foreign_key_violations: i64 = c
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(foreign_key_violations, 0);
    account_create(&c, "upgraded-policy-account", None, 2000).unwrap();
    c.execute_batch(
        "INSERT INTO pricing_catalog_versions(
             product_id,generation,schema_version,capability_generation,capability_digest,
             content_digest,created_ts
         ) VALUES
             ('switch-catalog',1,1,1,'capability','switch-catalog-digest',1),
             ('policy-catalog',1,1,1,'capability','policy-catalog-digest',1);
         INSERT INTO provider_switch_versions(
             generation,schema_version,capability_generation,capability_digest,
             content_digest,created_ts
         ) VALUES
             (1,1,1,'capability','catalog-switch-digest',1),
             (2,1,1,'capability','policy-switch-digest',1);
         INSERT INTO provider_switch_entries(
             generation,provider_id,scope_type,product_id,segment,catalog_generation,enabled
         ) VALUES(1,'anthropic','product','switch-catalog','',1,1);
         INSERT INTO account_policy_versions(
             account_id,effective_version,policy_id,policy_version,source_policy_digest,
             owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
             switch_generation,content_digest,replacement_locked,created_ts
         ) VALUES(
             'upgraded-policy-account',1,'b2c:global',1,'source-policy-digest',
             'global_b2c','global','b2c','policy-catalog',1,1,2,'policy-digest',0,1
         );",
    )
    .unwrap();

    assert!(c
        .execute(
            "INSERT INTO pricing_catalog_versions(
                 product_id,generation,schema_version,capability_digest,
                 content_digest,created_ts
             ) VALUES('missing-capability-generation',1,1,'capability','catalog',1)",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO account_policy_versions(
                 account_id,effective_version,policy_id,policy_version,owner_type,owner_id,
                 product_id,schema_version,catalog_generation,switch_generation,
                 content_digest,replacement_locked,created_ts
             ) VALUES(
                 'upgraded-policy-account',2,'b2c:global',2,'global_b2c','global',
                 'policy-catalog',1,1,2,'missing-lineage',0,1
             )",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "UPDATE account_policy_versions SET source_policy_digest=NULL
             WHERE account_id='upgraded-policy-account' AND effective_version=1",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO account_policy_bindings(
                 account_id,product_id,account_class,active_effective_version,
                 policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
             ) VALUES(
                 'upgraded-policy-account','policy-catalog','b2b',1,
                 'shadow','legacy_single','pending',1
             )",
            [],
        )
        .is_err());
    c.execute(
        "INSERT INTO account_policy_bindings(
             account_id,product_id,account_class,active_effective_version,
             policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
         ) VALUES(
             'upgraded-policy-account','policy-catalog','b2c',1,
             'shadow','legacy_single','pending',1
         )",
        [],
    )
    .unwrap();
    assert!(c
        .execute(
            "UPDATE account_policy_versions
             SET owner_type='b2b_client', account_class='b2b'
             WHERE account_id='upgraded-policy-account' AND effective_version=1",
            [],
        )
        .is_err());

    assert!(c
        .execute(
            "DELETE FROM pricing_catalog_versions
             WHERE product_id='switch-catalog' AND generation=1",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "UPDATE pricing_catalog_versions SET generation=2
             WHERE product_id='switch-catalog' AND generation=1",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "DELETE FROM provider_switch_versions WHERE generation=2",
            [],
        )
        .is_err());
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM provider_switch_versions WHERE generation=2",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    assert_eq!(
        c.query_row(
            "SELECT switch_generation FROM account_policy_versions
             WHERE account_id='upgraded-policy-account' AND effective_version=1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        2
    );
    assert!(c
        .execute(
            "UPDATE provider_switch_versions SET generation=3 WHERE generation=2",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO provider_switch_versions(
                 generation,schema_version,content_digest,created_ts
             ) VALUES(3,1,'missing-capability-pins',1)",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO provider_switch_entries(
                 generation,provider_id,scope_type,product_id,segment,
                 catalog_generation,enabled
             ) VALUES(1,'openai','product','missing-catalog','',1,1)",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO account_policy_versions(
                 account_id,effective_version,policy_id,policy_version,source_policy_digest,
                 owner_type,owner_id,account_class,product_id,schema_version,
                 catalog_generation,content_digest,replacement_locked,created_ts
             ) VALUES(
                 'upgraded-policy-account',2,'b2c:global',2,'source-policy-digest-2',
                 'global_b2c','global','b2c','policy-catalog',1,1,
                 'missing-switch-pin',0,1
             )",
            [],
        )
        .is_err());
}

#[test]
fn pricing_policy_schema_rejects_invalid_rules_switches_and_buckets() {
    let c = db();
    account_create(&c, "policy-account", None, 2000).unwrap();
    c.execute_batch(
        "INSERT INTO pricing_catalog_versions(
             product_id,generation,schema_version,capability_generation,capability_digest,
             content_digest,created_ts
         ) VALUES('main',1,1,1,'capability-digest','catalog-digest',1);
         INSERT INTO pricing_catalog_entries(
             product_id,generation,provider_id,canonical_model_id,enabled
         ) VALUES('main',1,'anthropic','claude-test',1);
         INSERT INTO provider_switch_versions(
             generation,schema_version,capability_generation,capability_digest,
             content_digest,created_ts
         ) VALUES(1,1,1,'capability-digest','switch-digest',1);
         INSERT INTO account_policy_versions(
             account_id,effective_version,policy_id,policy_version,source_policy_digest,
             owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
             switch_generation,content_digest,replacement_locked,created_ts
         ) VALUES(
             'policy-account',1,'b2c:global',1,'source-policy-digest','global_b2c','global',
             'b2c','main',1,1,1,'policy-digest',0,1
         );
         INSERT INTO account_policy_rules(
             account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
             canonical_model_id,pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
             track_eligible,retention_eligible,commission_eligible
         ) VALUES(
             'policy-account',1,'anthropic-provider','rule-digest','provider','anthropic',NULL,
             'discount','managed',6000,4000,0,0,0
         );",
    )
    .unwrap();

    assert!(c
        .execute(
            "INSERT INTO account_policy_rules(
                 account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                 canonical_model_id,pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
                 track_eligible,retention_eligible,commission_eligible
             ) VALUES(
                 'policy-account',1,'duplicate','duplicate-digest','provider','anthropic',NULL,
                 'discount','managed',5000,5000,0,0,0
             )",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO account_policy_rules(
                 account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                 canonical_model_id,pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
                 track_eligible,retention_eligible,commission_eligible
             ) VALUES(
                 'policy-account',1,'bad-step','bad-step-digest','model','anthropic','claude-test',
                 'discount','managed',5050,4950,0,0,0
             )",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO account_policy_rules(
                 account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                 canonical_model_id,pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
                 track_eligible,retention_eligible,commission_eligible
             ) VALUES(
                 'policy-account',1,'missing-discount','missing-discount-digest','model',
                 'anthropic','claude-test','discount','managed',NULL,5000,0,0,0
             )",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO account_policy_bindings(
                 account_id,product_id,account_class,active_effective_version,
                 policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
             ) VALUES(
                 'policy-account','openkeys','b2c',1,
                 'shadow','legacy_single','pending',1
             )",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO provider_switch_entries(
                 generation,provider_id,scope_type,product_id,segment,catalog_generation,enabled
             ) VALUES(1,'anthropic','segment','main','consumer',1,1)",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO provider_switch_entries(
                 generation,provider_id,scope_type,product_id,segment,catalog_generation,enabled
             ) VALUES(1,'anthropic','master','','',1,1)",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO funding_buckets(
                 bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES(
                 'welcome','policy-account','welcome_track_bonus','signup','any',
                 4000000000,0,0,1,'active',1,1
             )",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO usage_events(
                 account_id,real_nano,charge_nano,ts,official_cost_json
             ) VALUES('policy-account',10,5,1,'not-json')",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO usage_events(
                 account_id,real_nano,charge_nano,ts,paid_funded_nano
             ) VALUES('policy-account',10,5,1,5)",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO ledger(
                 account_id,kind,amount_nano,ts,official_nano
             ) VALUES('policy-account','charge',5,1,-1)",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO usage_events(
                 account_id,real_nano,charge_nano,ts,priced_ts,tariff_priced_ts
             ) VALUES('policy-account',10,5,1,10,11)",
            [],
        )
        .is_err());
}

#[test]
fn pricing_shadow_admission_requires_exact_actual_capability_and_rule_identity() {
    let c = db();
    account_create(&c, "shadow-account", None, 2000).unwrap();
    c.execute_batch(
        "INSERT INTO pricing_catalog_versions(
             product_id,generation,schema_version,capability_generation,capability_digest,
             content_digest,created_ts
         ) VALUES('main',1,1,1,'capability-digest','catalog-digest',1);
         INSERT INTO pricing_catalog_entries(
             product_id,generation,provider_id,canonical_model_id,enabled
         ) VALUES('main',1,'anthropic','claude-test',1);
         INSERT INTO provider_switch_versions(
             generation,schema_version,capability_generation,capability_digest,
             content_digest,created_ts
         ) VALUES(1,1,1,'capability-digest','switch-digest',1);
         INSERT INTO account_policy_versions(
             account_id,effective_version,policy_id,policy_version,source_policy_digest,
             owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
             switch_generation,content_digest,replacement_locked,created_ts
         ) VALUES(
             'shadow-account',1,'b2c:global',1,'source-policy','global_b2c','global','b2c',
             'main',1,1,1,'policy-digest',0,1
         );
         INSERT INTO account_policy_rules(
             account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
             canonical_model_id,pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
             track_eligible,retention_eligible,commission_eligible
         ) VALUES(
             'shadow-account',1,'anthropic-provider','rule-digest','provider','anthropic',NULL,
             'discount','managed',6000,4000,0,0,0
         );
         INSERT INTO billing_reservations(
             request_id,account_id,key,hold_nano,state,balance_after_reserve_nano,
             lease_until,created_ts,updated_ts
         ) VALUES('shadow-request','shadow-account','key',100,'reserved',0,100,1,1);
         INSERT INTO pricing_admission_snapshots(
             request_id,account_id,snapshot_kind,schema_version,provider_id,
             requested_model_id,canonical_model_id,alias_generation,pricing_mode,rule_origin,
             payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,admission_ts,
             official_hold_nano,charged_hold_nano,premium_modifiers,snapshot_digest
         ) VALUES(
             'shadow-request','shadow-account','legacy_scalar',1,'anthropic',
             'claude-test','claude-test',1,'legacy_scalar','legacy',2000,
             'legacy-tariff',1,1,100,20,'{}','actual-digest'
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
             'shadow-request','shadow-account','legacy_scalar',?1,?2,
             'claude-test','claude-test',1,1,1,'runtime-manifest',1,2,
             'resolved',?3,2000,?4,?5,'main','b2c',1,'b2c:global',1,
             'source-policy','policy-digest',1,1,
             CASE WHEN ?11='policy_catalog_schema_version' THEN NULL ELSE 1 END,
             CASE WHEN ?11='policy_catalog_capability_generation' THEN NULL ELSE 1 END,
             CASE WHEN ?11='policy_catalog_capability_digest' THEN NULL ELSE ?6 END,
             'catalog-digest',1,
             CASE WHEN ?11='policy_switch_schema_version' THEN NULL ELSE 1 END,
             CASE WHEN ?11='policy_switch_capability_generation' THEN NULL ELSE 1 END,
             CASE WHEN ?11='policy_switch_capability_digest' THEN NULL ELSE ?6 END,
             'switch-digest',1,
             CASE WHEN ?11='admission_catalog_schema_version' THEN NULL ELSE 1 END,
             CASE WHEN ?11='admission_catalog_capability_generation' THEN NULL ELSE 1 END,
             CASE WHEN ?11='admission_catalog_capability_digest' THEN NULL ELSE ?6 END,
             'catalog-digest',1,
             CASE WHEN ?11='admission_switch_schema_version' THEN NULL ELSE 1 END,
             CASE WHEN ?11='admission_switch_capability_generation' THEN NULL ELSE 1 END,
             CASE WHEN ?11='admission_switch_capability_digest' THEN NULL ELSE ?6 END,
             'switch-digest','anthropic-provider','rule-digest','provider',
             'discount','managed',?7,?8,0,0,0,?9,'different','{}',?10
         )";
    let assert_rejected = |actual_digest: &str,
                           provider: &str,
                           authorized_multiplier_bp: i64,
                           official_hold_nano: i64,
                           legacy_hold_nano: i64,
                           capability_digest: &str,
                           discount_bps: i64,
                           payable_multiplier_bp: i64,
                           evaluation_digest: &str| {
        assert!(c
            .execute(
                resolved_shadow_sql,
                rusqlite::params![
                    actual_digest,
                    provider,
                    authorized_multiplier_bp,
                    official_hold_nano,
                    legacy_hold_nano,
                    capability_digest,
                    discount_bps,
                    payable_multiplier_bp,
                    40_i64,
                    evaluation_digest,
                    ""
                ],
            )
            .is_err());
    };
    assert_rejected(
        "wrong-actual-digest",
        "anthropic",
        2000,
        100,
        20,
        "capability-digest",
        6000,
        4000,
        "wrong-actual-digest",
    );
    assert_rejected(
        "actual-digest",
        "openai",
        2000,
        100,
        20,
        "capability-digest",
        6000,
        4000,
        "wrong-actual-provider",
    );
    assert_rejected(
        "actual-digest",
        "anthropic",
        2001,
        100,
        20,
        "capability-digest",
        6000,
        4000,
        "wrong-actual-multiplier",
    );
    assert_rejected(
        "actual-digest",
        "anthropic",
        2000,
        101,
        20,
        "capability-digest",
        6000,
        4000,
        "wrong-official-hold",
    );
    assert_rejected(
        "actual-digest",
        "anthropic",
        2000,
        100,
        21,
        "capability-digest",
        6000,
        4000,
        "wrong-legacy-hold",
    );
    assert_rejected(
        "actual-digest",
        "anthropic",
        2000,
        100,
        20,
        "wrong-capability",
        6000,
        4000,
        "wrong-capability",
    );
    assert_rejected(
        "actual-digest",
        "anthropic",
        2000,
        100,
        20,
        "capability-digest",
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
        assert!(c
            .execute(
                resolved_shadow_sql,
                rusqlite::params![
                    "actual-digest",
                    "anthropic",
                    2000_i64,
                    100_i64,
                    20_i64,
                    "capability-digest",
                    6000_i64,
                    4000_i64,
                    40_i64,
                    null_field,
                    null_field
                ],
            )
            .is_err());
    }

    let valid = rusqlite::params![
        "actual-digest",
        "anthropic",
        2000_i64,
        100_i64,
        20_i64,
        "capability-digest",
        6000_i64,
        4000_i64,
        40_i64,
        "shadow-evaluation",
        ""
    ];
    c.execute(resolved_shadow_sql, valid).unwrap();
    assert!(c
        .execute(
            resolved_shadow_sql,
            rusqlite::params![
                "actual-digest",
                "anthropic",
                2000_i64,
                100_i64,
                20_i64,
                "capability-digest",
                6000_i64,
                4000_i64,
                40_i64,
                "shadow-evaluation",
                ""
            ],
        )
        .is_err());
    assert!(c
        .execute(
            "UPDATE pricing_shadow_admission_evaluations
             SET evaluation_digest='replacement' WHERE request_id='shadow-request'",
            [],
        )
        .is_err());

    for request_id in ["shadow-read-error", "shadow-rejected"] {
        c.execute(
            "INSERT INTO billing_reservations(
                 request_id,account_id,key,hold_nano,state,balance_after_reserve_nano,
                 lease_until,created_ts,updated_ts
             ) VALUES(?1,'shadow-account','key',100,'reserved',0,100,1,1)",
            [request_id],
        )
        .unwrap();
        c.execute(
            "INSERT INTO pricing_admission_snapshots(
                 request_id,account_id,snapshot_kind,schema_version,provider_id,
                 requested_model_id,canonical_model_id,alias_generation,pricing_mode,rule_origin,
                 payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,admission_ts,
                 official_hold_nano,charged_hold_nano,premium_modifiers,snapshot_digest
             ) VALUES(
                 ?1,'shadow-account','legacy_scalar',1,'anthropic','claude-test','claude-test',1,
                 'legacy_scalar','legacy',2000,'legacy-tariff',1,1,100,20,'{}','failure-actual'
             )",
            [request_id],
        )
        .unwrap();
    }
    let failure_shadow_sql = "INSERT INTO pricing_shadow_admission_evaluations(
             request_id,account_id,actual_snapshot_kind,actual_snapshot_digest,provider_id,
             requested_model_id,canonical_model_id,alias_generation,evaluator_schema_version,
             runtime_manifest_generation,runtime_manifest_digest,enqueued_ts,evaluated_ts,
             outcome,reason_code,authorized_multiplier_bp,observed_multiplier_bp,
             official_hold_nano,legacy_hold_nano,comparison_result,diagnostic_context,
             evaluation_digest
         ) VALUES(
             ?1,'shadow-account','legacy_scalar','failure-actual','anthropic',
             'claude-test','claude-test',1,1,1,'runtime-manifest',1,2,
             ?2,'authority_read',2000,?3,100,20,'not_comparable','{}',?4
         )";
    assert!(c
        .execute(
            failure_shadow_sql,
            rusqlite::params![
                "shadow-read-error",
                "rejected",
                Option::<i64>::None,
                "missing-rejected-observation"
            ],
        )
        .is_err());
    c.execute(
        failure_shadow_sql,
        rusqlite::params![
            "shadow-read-error",
            "read_error",
            Option::<i64>::None,
            "read-error"
        ],
    )
    .unwrap();
    assert!(c
        .execute(
            failure_shadow_sql,
            rusqlite::params![
                "shadow-rejected",
                "read_error",
                Some(2000_i64),
                "unexpected-read-observation"
            ],
        )
        .is_err());
    c.execute(
        failure_shadow_sql,
        rusqlite::params!["shadow-rejected", "rejected", Some(2000_i64), "rejected"],
    )
    .unwrap();
}

#[test]
fn pricing_snapshots_and_funding_allocations_are_account_scoped() {
    let c = db();
    let foreign_keys: bool = c
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert!(foreign_keys);
    account_create(&c, "account-a", None, 2000).unwrap();
    account_create(&c, "account-b", None, 3000).unwrap();
    c.execute(
        "INSERT INTO billing_reservations(
             request_id,account_id,key,hold_nano,state,balance_after_reserve_nano,
             lease_until,created_ts,updated_ts
         ) VALUES('request-a','account-a','key-a',100,'reserved',0,100,1,1)",
        [],
    )
    .unwrap();

    let legacy_snapshot_sql = "INSERT INTO pricing_admission_snapshots(
             request_id,account_id,snapshot_kind,schema_version,provider_id,
             requested_model_id,canonical_model_id,alias_generation,pricing_mode,rule_origin,
             payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,admission_ts,
             official_hold_nano,charged_hold_nano,premium_modifiers,snapshot_digest
         ) VALUES(?1,?2,'legacy_scalar',1,'anthropic','claude-test','claude-test',1,
             'legacy_scalar','legacy',2000,'legacy-tariff',1,1,100,20,'{}','snapshot')";
    assert!(c
        .execute(
            legacy_snapshot_sql,
            rusqlite::params!["request-a", "account-b"],
        )
        .is_err());
    c.execute(
        legacy_snapshot_sql,
        rusqlite::params!["request-a", "account-a"],
    )
    .unwrap();
    assert!(c
        .execute(
            "UPDATE pricing_admission_snapshots
             SET charged_hold_nano=21 WHERE request_id='request-a'",
            [],
        )
        .is_err());
    let rejected_shadow_sql = "INSERT INTO pricing_shadow_admission_evaluations(
             request_id,account_id,actual_snapshot_kind,actual_snapshot_digest,
             provider_id,requested_model_id,canonical_model_id,
             alias_generation,evaluator_schema_version,runtime_manifest_generation,
             runtime_manifest_digest,enqueued_ts,evaluated_ts,outcome,reason_code,
             authorized_multiplier_bp,observed_multiplier_bp,official_hold_nano,legacy_hold_nano,
             comparison_result,diagnostic_context,evaluation_digest
         ) VALUES(?1,?2,'legacy_scalar','snapshot','anthropic','claude-test','claude-test',1,1,1,
             'runtime-manifest',1,2,'rejected','no_policy_binding',2000,2000,100,20,
             'not_comparable','{}','shadow-rejected')";
    assert!(c
        .execute(
            rejected_shadow_sql,
            rusqlite::params!["request-a", "account-b"],
        )
        .is_err());
    c.execute(
        rejected_shadow_sql,
        rusqlite::params!["request-a", "account-a"],
    )
    .unwrap();
    assert!(c
        .execute(
            "UPDATE pricing_shadow_admission_evaluations
             SET reason_code='different_reason' WHERE request_id='request-a'",
            [],
        )
        .is_err());

    c.execute_batch(
        "INSERT INTO funding_buckets(
             bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
             reserved_nano,spent_nano,version,status,created_ts,updated_ts
         ) VALUES
             ('paid-a','account-a','paid','primary','any',1000,0,0,1,'active',1,1),
             ('paid-b','account-b','paid','primary','any',1000,0,0,1,'active',1,1);
         INSERT INTO ledger(
             account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts
         ) VALUES('account-b','key-b','charge','ledger-request',10,'charge-ref',990,1);",
    )
    .unwrap();
    let ledger_id = c.last_insert_rowid();
    assert!(c
        .execute(
            "INSERT INTO ledger_funding_allocations(
                 ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
                 direction,amount_nano
             ) VALUES(?1,'account-a','paid-a','paid',1,'debit',10)",
            rusqlite::params![ledger_id],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO ledger_funding_allocations(
                 ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
                 direction,amount_nano
             ) VALUES(?1,'account-b','paid-a','paid',1,'debit',10)",
            rusqlite::params![ledger_id],
        )
        .is_err());
    c.execute(
        "INSERT INTO ledger_funding_allocations(
             ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
             direction,amount_nano
         ) VALUES(?1,'account-b','paid-b','paid',1,'debit',10)",
        rusqlite::params![ledger_id],
    )
    .unwrap();
}

/// Персист состояния пула: save→load переносит cooling/калибровку (upsert по email).
#[test]
fn pool_state_save_load_roundtrip() {
    let c = db();
    let rows = vec![PoolStateRow {
        email: "a@x.io".into(),
        cooling_until: 123456,
        cap5h_usd: 50.0,
        cap7d_usd: 1500.0,
        spent_total_usd: 12.5,
        util5h: 0.3,
        util7d: 0.1,
        reset5h: 999,
        reset7d: 888,
        calib_n: 4,
        version: 0,
        spent_delta_usd: 0.0,
    }];
    save_pool_state(&c, &rows).unwrap();
    // повторный save (upsert) не дублирует и обновляет
    let mut r2 = rows.clone();
    r2[0].cooling_until = 222222;
    save_pool_state(&c, &r2).unwrap();
    let got = load_pool_state(&c).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].email, "a@x.io");
    assert_eq!(got[0].cooling_until, 222222);
    assert!((got[0].cap5h_usd - 50.0).abs() < 1e-9);
    assert_eq!(got[0].calib_n, 4);
}

#[test]
fn anthropic_calibration_schema_is_exact_plan_scoped_and_prior_free() {
    let c = db();
    let tables: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ( \
               'provider_turn_calibration_events','provider_calibration_subject_spend', \
               'anthropic_window_calibrations','anthropic_window_observations')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 4);

    let columns = c
        .prepare("SELECT name FROM pragma_table_info('anthropic_window_calibrations')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(!columns.iter().any(|name| {
        let name = name.to_ascii_lowercase();
        name.contains("prior") || name.contains("ema") || name.contains("nominal")
    }));
    for name in [
        "plan",
        "window_kind",
        "anchor_used_fraction_units",
        "anchor_resolution_fraction_units",
        "observed_fraction_units",
        "observed_spend_nano",
        "unattributed_fraction_units",
        "current_capacity_nano",
        "current_low_nano",
        "current_high_nano",
        "current_confidence_bp",
    ] {
        assert!(columns.contains(&name.to_owned()), "missing {name}");
    }

    let turn_columns = c
        .prepare("SELECT name FROM pragma_table_info('provider_turn_calibration_events')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for name in [
        "subject_id",
        "model_id",
        "service_tier",
        "inference_geo",
        "cache_read_tokens",
        "cache_write_5m_tokens",
        "cache_write_1h_tokens",
        "thinking_output_tokens",
        "search_queries",
        "api_total_nanousd",
    ] {
        assert!(turn_columns.contains(&name.to_owned()), "missing {name}");
    }

    let insert_state = "INSERT INTO anthropic_window_calibrations( \
        subject_id,plan,window_kind,window_duration_mins,resets_at, \
        anchor_used_fraction_units,anchor_resolution_fraction_units,anchor_spend_nano, \
        used_fraction_units,measurement_resolution_fraction_units,observed_at,updated_ts) \
        VALUES(?1,?2,?3,?4,2000000000,?5,?6,0,?5,?6,100,100)";
    c.execute(
        insert_state,
        rusqlite::params!["sub-a", "max20", "5h", 300, 12_345_000, 1_000],
    )
    .unwrap();
    c.execute(
        insert_state,
        rusqlite::params!["sub-a", "max20", "7d", 10_080, 40_000_000, 1_000_000],
    )
    .unwrap();
    // Correcting the durable plan creates a separate cold identity instead of mutating the
    // Max cohort's evidence.
    c.execute(
        insert_state,
        rusqlite::params!["sub-a", "pro", "5h", 300, 12_345_000, 1_000],
    )
    .unwrap();
    assert!(c
        .execute(
            insert_state,
            rusqlite::params!["sub-b", "max20", "7d", 300, 1, 1],
        )
        .is_err());

    let insert_observation = "INSERT INTO anthropic_window_observations( \
        subject_id,plan,window_kind,window_duration_mins,resets_at,observed_at, \
        used_fraction_units,measurement_resolution_fraction_units,gateway_spend_nano, \
        observation_source,source_request_id) \
        VALUES(?1,'max20','5h',300,2000000000,?2,?3,?4,?5,?6,?7)";
    c.execute(
        insert_observation,
        rusqlite::params![
            "sub-a",
            101,
            12_345_000,
            1_000,
            42_000_000,
            "response",
            "cal-request-a"
        ],
    )
    .unwrap();
    assert!(c
        .execute(
            insert_observation,
            rusqlite::params![
                "sub-a",
                102,
                13_000_000,
                1_000_000,
                43_000_000,
                "response",
                "cal-request-a"
            ],
        )
        .is_err());
    c.execute(
        insert_observation,
        rusqlite::params![
            "sub-a",
            103,
            13_000_000,
            1_000_000,
            43_000_000,
            "poll",
            Option::<String>::None
        ],
    )
    .unwrap();
    assert!(c
        .execute(
            insert_observation,
            rusqlite::params!["sub-b", 104, 1, 1, 0, "response", Option::<String>::None],
        )
        .is_err());
}

#[test]
fn codex_calibration_schema_has_no_capacity_prior() {
    let c = db();
    let tables: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ( \
               'codex_home_spend','codex_window_calibrations','codex_window_observations', \
               'codex_turn_calibration_events')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 4);

    let columns = c
        .prepare("SELECT name FROM pragma_table_info('codex_window_calibrations')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(!columns.iter().any(|name| name.contains("prior")));
    assert!(columns.contains(&"window_duration_mins".to_owned()));
    assert!(columns.contains(&"resets_at".to_owned()));
    assert!(columns.contains(&"anchor_ready".to_owned()));
    for name in [
        "anchor_spend_nanocredits",
        "observed_spend_nanocredits",
        "current_capacity_nanocredits",
        "credit_samples",
        "unattributed_fraction_units",
    ] {
        assert!(columns.contains(&name.to_owned()), "missing {name}");
    }

    let turn_columns = c
        .prepare("SELECT name FROM pragma_table_info('codex_turn_calibration_events')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for name in [
        "home_id",
        "model_id",
        "service_tier",
        "cached_input_tokens",
        "cache_write_input_tokens",
        "reasoning_output_tokens",
        "api_total_nanousd",
        "chatgpt_total_nanocredits",
    ] {
        assert!(turn_columns.contains(&name.to_owned()), "missing {name}");
    }
}

#[test]
fn gemini_calibration_schema_has_exact_two_window_contract_and_no_prior() {
    let c = db();
    let tables: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ( \
               'gemini_profile_spend','gemini_window_calibrations',\
               'gemini_window_observations')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 3);

    let columns = c
        .prepare("SELECT name FROM pragma_table_info('gemini_window_calibrations')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(!columns.iter().any(|name| name.contains("prior")));
    assert!(columns.contains(&"bucket_id".to_owned()));
    assert!(columns.contains(&"window_kind".to_owned()));
    assert!(columns.contains(&"anchor_used_fraction_units".to_owned()));
    assert!(columns.contains(&"sum_used_sq".to_owned()));
    assert!(columns.contains(&"observed_spend_nano".to_owned()));

    let insert = "INSERT INTO gemini_window_calibrations( \
        profile_id,bucket_id,window_kind,window_duration_mins,resets_at, \
        anchor_used_fraction_units,anchor_spend_nano,used_fraction_units,observed_at, \
        updated_ts) VALUES(?1,?2,?3,?4,200,?5,0,?5,100,100)";
    c.execute(
        insert,
        rusqlite::params!["profile-a", "gemini-5h", "5h", 300, 12_345],
    )
    .unwrap();
    c.execute(
        insert,
        rusqlite::params!["profile-a", "gemini-weekly", "weekly", 10_080, 67_890],
    )
    .unwrap();
    assert!(c
        .execute(
            insert,
            rusqlite::params!["profile-b", "gemini-daily", "5h", 300, 1],
        )
        .is_err());
    assert!(c
        .execute(
            insert,
            rusqlite::params!["profile-b", "gemini-5h", "5h", 300, 100_000_001],
        )
        .is_err());
}

#[test]
fn gemini_exact_calibration_schema_is_plan_scoped_and_replayable() {
    let c = db();
    let tables: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ( \
               'gemini_exact_window_calibrations','gemini_exact_window_observations')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 2);

    let columns = c
        .prepare("SELECT name FROM pragma_table_info('gemini_exact_window_calibrations')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(!columns.iter().any(|name| {
        let name = name.to_ascii_lowercase();
        name.contains("prior") || name.contains("ema") || name.contains("nominal")
    }));
    for name in [
        "plan",
        "bucket_id",
        "anchor_resolution_fraction_units",
        "measurement_resolution_fraction_units",
        "observed_fraction_units",
        "observed_spend_nano",
        "unattributed_fraction_units",
        "current_capacity_nano",
        "current_low_nano",
        "current_high_nano",
    ] {
        assert!(columns.contains(&name.to_owned()), "missing {name}");
    }

    let insert_state = "INSERT INTO gemini_exact_window_calibrations( \
        profile_id,plan,bucket_id,window_kind,window_duration_mins,resets_at, \
        anchor_used_fraction_units,anchor_resolution_fraction_units,anchor_spend_nano, \
        used_fraction_units,measurement_resolution_fraction_units,observed_at,updated_ts) \
        VALUES(?1,?2,?3,?4,?5,2000000000,?6,?7,0,?6,?7,100,100)";
    c.execute(
        insert_state,
        rusqlite::params!["profile-a", "google_ai_pro", "gemini-5h", "5h", 300, 10, 1],
    )
    .unwrap();
    c.execute(
        insert_state,
        rusqlite::params![
            "profile-a",
            "google_ai_ultra",
            "gemini-5h",
            "5h",
            300,
            10,
            1
        ],
    )
    .unwrap();
    c.execute(
        insert_state,
        rusqlite::params![
            "profile-a",
            "google_ai_pro",
            "gemini-weekly",
            "weekly",
            10_080,
            20,
            1_000_000
        ],
    )
    .unwrap();
    assert!(c
        .execute(
            insert_state,
            rusqlite::params![
                "profile-b",
                "google_ai_pro",
                "gemini-weekly",
                "weekly",
                300,
                1,
                1
            ],
        )
        .is_err());

    let insert_observation = "INSERT INTO gemini_exact_window_observations( \
        profile_id,plan,bucket_id,window_kind,window_duration_mins,resets_at,observed_at, \
        used_fraction_units,measurement_resolution_fraction_units,gateway_spend_nano, \
        observation_source,source_request_id) \
        VALUES('profile-a','google_ai_pro','gemini-5h','5h',300,2000000000,?1, \
            ?2,?3,?4,?5,?6)";
    c.execute(
        insert_observation,
        rusqlite::params![101, 10, 1, 1_000, "poll", Option::<String>::None],
    )
    .unwrap();
    c.execute(
        insert_observation,
        rusqlite::params![102, 20, 10, 2_000, "response", "gemini-cal-a"],
    )
    .unwrap();
    assert!(c
        .execute(
            insert_observation,
            rusqlite::params![103, 30, 10, 3_000, "response", "gemini-cal-a"],
        )
        .is_err());
    assert!(c
        .execute(
            insert_observation,
            rusqlite::params![104, 40, 10, 4_000, "response", Option::<String>::None],
        )
        .is_err());
}

#[test]
fn gemini_spend_and_calibration_are_exact_durable_and_cas_versioned() {
    let c = db();
    assert_eq!(gemini_profile_spend(&c, "profile-a").unwrap(), 0);
    assert_eq!(
        credit_gemini_profile_spend(&c, "profile-a", 19_404_000, 100).unwrap(),
        19_404_000
    );
    assert_eq!(
        credit_gemini_profile_spend(&c, "profile-a", 1, 101).unwrap(),
        19_404_001
    );

    let mut state = GeminiCalibrationRow {
        profile_id: "profile-a".to_string(),
        bucket_id: "gemini-5h".to_string(),
        window_kind: "5h".to_string(),
        window_duration_mins: 300,
        resets_at: 2_000_000_000,
        anchor_used_fraction_units: 1_970,
        anchor_spend_nano: 0,
        anchor_ready: false,
        used_fraction_units: 1_970,
        observed_at: 100,
        sum_used_sq: "170141183460469231731687303715884105727".to_string(),
        sum_used_spend_nano: "0".to_string(),
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
        updated_ts: 100,
    };
    let observation = GeminiWindowObservation {
        profile_id: state.profile_id.clone(),
        bucket_id: state.bucket_id.clone(),
        window_kind: state.window_kind.clone(),
        window_duration_mins: state.window_duration_mins,
        resets_at: state.resets_at,
        observed_at: state.observed_at,
        used_fraction_units: state.used_fraction_units,
        gateway_spend_nano: 19_404_001,
    };
    assert_eq!(
        save_gemini_calibration(&c, &state, &observation).unwrap(),
        Some(1)
    );
    assert_eq!(
        save_gemini_calibration(&c, &state, &observation).unwrap(),
        None
    );
    state = load_gemini_calibration(&c, "profile-a", "gemini-5h")
        .unwrap()
        .unwrap();
    assert_eq!(state.version, 1);
    assert_eq!(state.sum_used_sq, i128::MAX.to_string());
    assert_eq!(state.observed_spend_nano, 12_345);
    assert_eq!(
        load_gemini_window_observations(&c, "profile-a", "gemini-5h").unwrap(),
        vec![observation]
    );

    let mismatched = GeminiWindowObservation {
        profile_id: "profile-b".to_string(),
        observed_at: 101,
        ..load_gemini_window_observations(&c, "profile-a", "gemini-5h")
            .unwrap()
            .pop()
            .unwrap()
    };
    assert!(save_gemini_calibration(&c, &state, &mismatched).is_err());

    state.sum_used_sq = "01".to_string();
    assert!(save_gemini_calibration(
        &c,
        &state,
        &GeminiWindowObservation {
            observed_at: 101,
            ..load_gemini_window_observations(&c, "profile-a", "gemini-5h")
                .unwrap()
                .pop()
                .unwrap()
        }
    )
    .is_err());
}

#[test]
fn codex_home_health_defaults_to_healthy_and_round_trips() {
    let c = db();
    // Absence of evidence is not evidence of a fault: an unknown home starts routable.
    assert_eq!(
        load_codex_home_health(&c, "home-new").unwrap(),
        CodexHomeHealthRow::default()
    );

    let dead = CodexHomeHealthRow {
        account_state: "dead".to_string(),
        auth_fail_streak: 2,
        first_auth_fail_ts: 1_000,
        cooling_until: 1_900,
    };
    save_codex_home_health(&c, "home-a", &dead, 2_000).unwrap();
    // The verdict a restart used to discard now survives it, which is the whole point: a
    // corroborated dead subscription must not be re-admitted by every blue-green handoff.
    assert_eq!(load_codex_home_health(&c, "home-a").unwrap(), dead);

    let repaired = CodexHomeHealthRow::default();
    save_codex_home_health(&c, "home-a", &repaired, 2_100).unwrap();
    assert_eq!(load_codex_home_health(&c, "home-a").unwrap(), repaired);
    // Homes are independent: one dead subscription never taints its neighbours.
    assert_eq!(
        load_codex_home_health(&c, "home-b").unwrap(),
        CodexHomeHealthRow::default()
    );
}

#[test]
fn codex_spend_and_calibration_are_durable_and_cas_versioned() {
    let c = db();
    assert_eq!(
        codex_home_calibration_spend(&c, "home-a").unwrap(),
        CodexHomeCalibrationSpend::default()
    );
    let event = |request_id: &str,
                 api_total_nanousd: i64,
                 chatgpt_total_nanocredits: i64,
                 completed_at: i64| {
        CodexTurnCalibrationEvent {
            request_id: request_id.into(),
            home_id: "home-a".into(),
            model_id: "gpt-5.6-terra".into(),
            service_tier: "standard".into(),
            provider_reported_tier: Some("default".into()),
            api_tariff_schedule_id: "openai/test/v1".into(),
            credit_schedule_id: "chatgpt/test/v1".into(),
            completed_at,
            input_tokens: 1,
            cached_input_tokens: 0,
            cache_write_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            api_input_nanousd: api_total_nanousd,
            api_cached_input_nanousd: 0,
            api_cache_write_nanousd: 0,
            api_output_nanousd: 0,
            api_total_nanousd,
            chatgpt_input_nanocredits: chatgpt_total_nanocredits,
            chatgpt_cached_input_nanocredits: 0,
            chatgpt_output_nanocredits: 0,
            chatgpt_total_nanocredits,
        }
    };
    let first = event("request-1", 40_000_000_000, 4_000_000_000, 100);
    let totals = record_codex_turn_calibration_event(&c, &first).unwrap();
    assert!(totals.inserted);
    assert_eq!(totals.spent_nano, 40_000_000_000);
    assert_eq!(totals.spent_nanocredits, Some(4_000_000_000));
    assert_eq!(totals.credit_tracking_started_ts, Some(100));
    assert!(
        !record_codex_turn_calibration_event(&c, &first)
            .unwrap()
            .inserted
    );
    let mut conflict = first.clone();
    conflict.api_input_nanousd += 1;
    conflict.api_total_nanousd += 1;
    let conflict_error = record_codex_turn_calibration_event(&c, &conflict).unwrap_err();
    assert!(is_codex_turn_calibration_replay_conflict(&conflict_error));
    let totals = record_codex_turn_calibration_event(
        &c,
        &event("request-2", 60_000_000_000, 6_000_000_000, 101),
    )
    .unwrap();
    assert_eq!(totals.spent_nano, 100_000_000_000);
    assert_eq!(totals.spent_nanocredits, Some(10_000_000_000));
    assert_eq!(
        totals.credit_tracking_started_ts,
        Some(100),
        "tracking start stays pinned to the first immutable event"
    );
    let report = codex_turn_calibration_report(&c).unwrap();
    assert_eq!(report.len(), 1);
    assert_eq!(report[0].turns, 2);
    assert_eq!(report[0].api_total_nanousd, 100_000_000_000);
    assert_eq!(report[0].chatgpt_total_nanocredits, 10_000_000_000);

    let mut state = CodexCalibrationRow {
        home_id: "home-a".into(),
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
        anchor_spend_nanocredits: Some(10_000_000_000),
        observed_spend_nanocredits: Some(0),
        current_capacity_nanocredits: None,
        current_low_nanocredits: None,
        current_high_nanocredits: None,
        last_capacity_nanocredits: None,
        last_low_nanocredits: None,
        last_high_nanocredits: None,
        credit_samples: Some(0),
        credit_estimator_version: Some(1),
        unattributed_fraction_units: Some(0),
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
        home_id: "home-a".into(),
        window_duration_mins: 300,
        resets_at: 2_000_000_000,
        observed_at: 101,
        used_percent: 10,
        used_fraction_units: 10_000_000,
        gateway_spend_nano: 100_000_000_000,
        gateway_spend_nanocredits: Some(10_000_000_000),
    };
    assert_eq!(
        save_codex_calibration(&c, &state, &observation).unwrap(),
        Some(1)
    );
    assert_eq!(
        save_codex_calibration(&c, &state, &observation).unwrap(),
        None,
        "a second absent-row derivation must lose CAS"
    );

    state = load_codex_calibration(&c, "home-a", 300).unwrap().unwrap();
    assert_eq!(state.version, 1);
    assert!(!state.anchor_ready);
    state.used_percent = 11;
    state.used_fraction_units = 11_000_000;
    state.observed_at = 102;
    state.updated_ts = 102;
    let mut second = observation.clone();
    second.used_percent = 11;
    second.used_fraction_units = 11_000_000;
    second.observed_at = 102;
    assert_eq!(
        save_codex_calibration(&c, &state, &second).unwrap(),
        Some(2)
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM codex_window_observations WHERE home_id='home-a'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        2
    );
    let observations = load_codex_window_observations(&c, "home-a", 300).unwrap();
    assert_eq!(
        observations
            .iter()
            .map(|row| row.observed_at)
            .collect::<Vec<_>>(),
        vec![101, 102]
    );
}

// хелпер: аккаунт с балансом + ключ под ним (ref=None — админ-сид, не платёж, без дедупа)
fn acct_with_key(c: &Connection, acct: &str, key: &str, usd_nano: i64, mult: i64) {
    account_create(c, acct, None, mult).unwrap();
    account_topup(c, acct, usd_nano, None).unwrap();
    key_issue(c, key, acct, None).unwrap();
}

fn legacy_snapshot(
    request_id: &str,
    account_id: &str,
    official_hold_nano: i64,
    charged_hold_nano: i64,
) -> pricing::LegacyScalarAdmissionSnapshot {
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
) -> pricing::LegacyScalarAdmissionSnapshot {
    pricing::LegacyScalarAdmissionSnapshot::new(pricing::LegacyScalarAdmissionSnapshotInput {
        request_id: request_id.into(),
        account_id: account_id.into(),
        provider: pricing::SnapshotProvider::Anthropic,
        requested_model_id: "claude-sonnet-5".into(),
        canonical_model_id: "claude-sonnet-5".into(),
        alias_generation: 1,
        tariff_schedule_id: "anthropic/standard/sonnet-current/v1".into(),
        tariff_priced_ts: admission_ts,
        admission_ts,
        payable_multiplier_bp: 2_000,
        official_hold_nano,
        charged_hold_nano,
        premium_modifiers: pricing::LegacyPremiumModifiers::AnthropicV1 {
            speed: pricing::SnapshotAnthropicSpeed::Standard,
            inference_geo: pricing::SnapshotAnthropicInferenceGeo::Global,
            inference_geo_basis_points: 10_000,
        },
    })
    .unwrap()
}

fn openai_legacy_snapshot(
    request_id: &str,
    account_id: &str,
    official_hold_nano: i64,
    charged_hold_nano: i64,
) -> pricing::LegacyScalarAdmissionSnapshot {
    let admission_ts = now();
    pricing::LegacyScalarAdmissionSnapshot::new(pricing::LegacyScalarAdmissionSnapshotInput {
        request_id: request_id.into(),
        account_id: account_id.into(),
        provider: pricing::SnapshotProvider::OpenAi,
        requested_model_id: "gpt-5.6".into(),
        canonical_model_id: "gpt-5.6-sol".into(),
        alias_generation: 1,
        tariff_schedule_id: "openai/gpt-5.6-sol/epoch-0/v1".into(),
        tariff_priced_ts: admission_ts,
        admission_ts,
        payable_multiplier_bp: 2_000,
        official_hold_nano,
        charged_hold_nano,
        premium_modifiers: pricing::LegacyPremiumModifiers::OpenAiV1 {
            service_tier: pricing::SnapshotOpenAiServiceTier::Fast,
            service_tier_multiplier_basis_points: 25_000,
            context_tier: pricing::SnapshotOpenAiContextTier::Long,
            input_multiplier_basis_points: 20_000,
            output_multiplier_basis_points: 15_000,
        },
    })
    .unwrap()
}

#[test]
fn authoritative_database_uses_full_synchronous_durability() {
    let c = db();
    let synchronous: i64 = c.query_row("PRAGMA synchronous", [], |r| r.get(0)).unwrap();
    assert_eq!(synchronous, 2); // SQLite FULL
}

#[test]
fn open_fails_closed_when_legacy_topup_references_are_duplicated() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "registry-duplicate-ref-{}-{unique}.db",
        std::process::id()
    ));
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE ledger(id INTEGER PRIMARY KEY AUTOINCREMENT, account_id TEXT NOT NULL, \
             key TEXT, kind TEXT NOT NULL, amount_nano INTEGER NOT NULL, ref TEXT, \
             balance_after_nano INTEGER, ts INTEGER, model TEXT); \
             INSERT INTO ledger(account_id,kind,amount_nano,ref) VALUES('a','topup',1,'dup'); \
             INSERT INTO ledger(account_id,kind,amount_nano,ref) VALUES('a','topup',1,'dup');",
        )
        .unwrap();
    }
    assert!(open(path.to_str().unwrap()).is_err());
    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(format!("{}-wal", path.display()));
    let _ = fs::remove_file(format!("{}-shm", path.display()));
}

#[test]
fn legacy_keys_with_same_suffix_migrate_to_distinct_accounts() {
    let c = db();
    c.execute(
        "INSERT INTO api_keys(key,key_id,balance_nano,spent_nano,mult_bp,status,reserved_nano) \
         VALUES(?1,?2,?3,?4,?5,'active',0)",
        rusqlite::params!["sk-user-a-123456789abc", "legacy_a", 100, 10, 2000],
    )
    .unwrap();
    c.execute(
        "INSERT INTO api_keys(key,key_id,balance_nano,spent_nano,mult_bp,status,reserved_nano) \
         VALUES(?1,?2,?3,?4,?5,'active',0)",
        rusqlite::params!["sk-user-b-123456789abc", "legacy_b", 200, 20, 3000],
    )
    .unwrap();
    migrate_legacy_keys(&c).unwrap();
    let a = key_get(&c, "sk-user-a-123456789abc")
        .unwrap()
        .unwrap()
        .account_id
        .unwrap();
    let b = key_get(&c, "sk-user-b-123456789abc")
        .unwrap()
        .unwrap()
        .account_id
        .unwrap();
    assert_ne!(a, b);
    assert_eq!(account_get(&c, &a).unwrap().unwrap().balance_nano, 100);
    assert_eq!(account_get(&c, &b).unwrap().unwrap().balance_nano, 200);
}

/// Агрегаты трат (для /metrics): суммы по аккаунтам + число активных.
#[test]
fn billing_totals_aggregates_across_accounts() {
    let c = db();
    acct_with_key(&c, "acct_1", "sk-1", 5_000_000_000, 10000); // $5
    acct_with_key(&c, "acct_2", "sk-2", 3_000_000_000, 10000); // $3
    account_reserve(&c, "acct_1", 1_000_000_000).unwrap();
    account_settle(&c, "acct_1", "sk-1", 1_000_000_000, 400_000_000, None, None).unwrap(); // spent $0.4
    account_reserve(&c, "acct_2", 500_000_000).unwrap(); // висящий резерв $0.5
    account_set_status(&c, "acct_2", "disabled").unwrap();
    let t = billing_totals(&c).unwrap();
    assert_eq!(t.balance_nano, 4_600_000_000 + 2_500_000_000); // $4.6 + $2.5
    assert_eq!(t.spent_nano, 400_000_000);
    assert_eq!(t.reserved_nano, 500_000_000);
    assert_eq!(t.active_accounts, 1);
}

/// Без per-request identity старт не может доказать, что резерв осиротел: fail-closed оставляет hold.
#[test]
fn reconcile_does_not_refund_unowned_aggregate_reservations() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000_000_000, 2000);
    account_reserve(&c, "a", 600_000_000).unwrap();
    assert_eq!(reconcile_reservations(&c).unwrap(), 0);
    let acc = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(acc.balance_nano, 400_000_000);
    assert_eq!(acc.reserved_nano, 600_000_000);
}

/// reserve атомарно гейтит по балансу аккаунта; settle сводит пару к −actual; per-key spent + ledger.
#[test]
fn reserve_gates_and_settle_nets_to_actual() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000_000_000, 2000); // $1.00
    assert_eq!(
        account_reserve(&c, "a", 600_000_000).unwrap(),
        Some(400_000_000)
    );
    assert_eq!(account_reserve(&c, "a", 600_000_000).unwrap(), None); // $0.40 < $0.60 → отказ
    assert_eq!(
        account_settle(&c, "a", "k", 600_000_000, 100_000_000, Some("req1"), None).unwrap(),
        Some(900_000_000)
    );
    let acc = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(acc.balance_nano, 900_000_000);
    assert_eq!(acc.spent_nano, 100_000_000);
    // per-key атрибуция: spent по ключу тоже $0.10
    assert_eq!(key_get(&c, "k").unwrap().unwrap().spent_nano, 100_000_000);
    // ledger: строка topup ($1) + строка charge ($0.10)
    let cnt: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM ledger WHERE account_id='a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cnt, 2);
}

/// Exact provider usage is never silently clamped to the estimate held before delivery.
#[test]
fn settle_records_exact_actual_above_hold() {
    let c = db();
    acct_with_key(&c, "a", "k", 100, 2000);
    assert_eq!(account_reserve(&c, "a", 100).unwrap(), Some(0));
    assert_eq!(
        account_settle(&c, "a", "k", 100, 150, Some("req"), None).unwrap(),
        Some(-50)
    );
    let acc = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(acc.balance_nano, -50);
    assert_eq!(acc.spent_nano, 150);
    assert_eq!(acc.reserved_nano, 0);
    assert_eq!(key_get(&c, "k").unwrap().unwrap().spent_nano, 150);
    assert_eq!(ledger_recent(&c, "a", 10).unwrap()[0].amount_nano, 150);
}

#[test]
fn sqlite_request_lifecycle_is_exactly_once() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000, 2000);
    assert_eq!(
        sqlite_reserve_request(&c, "req", "a", "k", 400, 60).unwrap(),
        Some(600)
    );
    assert_eq!(
        sqlite_reserve_request(&c, "req", "a", "k", 400, 60).unwrap(),
        Some(600)
    );
    assert!(sqlite_mark_delivering(&c, "req", 60).unwrap());
    assert_eq!(
        sqlite_settle_request(&c, "req", "a", "k", 400, 150, Some("provider:req"), None).unwrap(),
        Some(850),
    );
    assert_eq!(
        sqlite_settle_request(&c, "req", "a", "k", 400, 150, Some("provider:req"), None).unwrap(),
        Some(850),
    );
    let account = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(
        (
            account.balance_nano,
            account.spent_nano,
            account.reserved_nano
        ),
        (850, 150, 0)
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM ledger WHERE kind='charge'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1,
    );
    assert!(pricing::sqlite_legacy_scalar_admission_snapshot(&c, "req")
        .unwrap()
        .is_none());
}

#[test]
fn sqlite_execution_group_charges_only_the_first_nonzero_settlement() {
    const GROUP: &str = "018f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";
    const ZERO_GROUP: &str = "128f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";

    let c = db();
    acct_with_key(&c, "group-account", "group-key", 1_000, 2_000);
    let first = ExecutionAttempt::grouped(GROUP, 1).unwrap();
    let second = ExecutionAttempt::grouped(GROUP, 2).unwrap();
    assert_eq!(
        sqlite_reserve_request_for_execution(
            &c,
            "group-request-1",
            "group-account",
            "group-key",
            400,
            60,
            &first,
        )
        .unwrap(),
        Some(600),
    );
    assert_eq!(
        sqlite_reserve_request_for_execution(
            &c,
            "group-request-1",
            "group-account",
            "group-key",
            400,
            60,
            &first,
        )
        .unwrap(),
        Some(600),
    );
    assert!(sqlite_reserve_request_for_execution(
        &c,
        "group-request-1",
        "group-account",
        "group-key",
        400,
        60,
        &second,
    )
    .is_err());
    assert!(
        sqlite_reserve_request(&c, "group-request-1", "group-account", "group-key", 400, 60,)
            .is_err()
    );
    assert_eq!(
        sqlite_reserve_request_for_execution(
            &c,
            "group-request-2",
            "group-account",
            "group-key",
            300,
            60,
            &second,
        )
        .unwrap(),
        Some(300),
    );

    // Settlement order, not attempt number, chooses the durable winner.
    assert_eq!(
        sqlite_settle_request(
            &c,
            "group-request-2",
            "group-account",
            "group-key",
            300,
            200,
            Some("provider:second"),
            None,
        )
        .unwrap(),
        Some(400),
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            "group-request-1",
            "group-account",
            "group-key",
            400,
            150,
            Some("provider:first"),
            None,
        )
        .unwrap(),
        Some(800),
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            "group-request-1",
            "group-account",
            "group-key",
            400,
            150,
            Some("provider:first"),
            None,
        )
        .unwrap(),
        Some(800),
    );
    let account = account_get(&c, "group-account").unwrap().unwrap();
    assert_eq!(
        (
            account.balance_nano,
            account.spent_nano,
            account.reserved_nano
        ),
        (800, 200, 0),
    );
    assert_eq!(
        c.query_row(
            "SELECT
               (SELECT winner_request_id FROM execution_group_winner WHERE group_id=?1),
               (SELECT COUNT(*) FROM ledger WHERE kind='charge'
                 AND ref IN ('provider:first','provider:second')),
               (SELECT actual_nano FROM billing_reservations
                 WHERE request_id='group-request-1'),
               (SELECT state FROM billing_reservations
                 WHERE request_id='group-request-1'),
               (SELECT actual_nano FROM billing_settlement_outbox
                 WHERE request_id='group-request-1')",
            rusqlite::params![GROUP],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            )),
        )
        .unwrap(),
        ("group-request-2".into(), 1, 0, "canceled".into(), 150),
    );

    // A zero settlement does not consume the group winner slot.
    let zero = ExecutionAttempt::grouped(ZERO_GROUP, 1).unwrap();
    let positive = ExecutionAttempt::grouped(ZERO_GROUP, 2).unwrap();
    assert_eq!(
        sqlite_reserve_request_for_execution(
            &c,
            "zero-request",
            "group-account",
            "group-key",
            100,
            60,
            &zero,
        )
        .unwrap(),
        Some(700),
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            "zero-request",
            "group-account",
            "group-key",
            100,
            0,
            None,
            None,
        )
        .unwrap(),
        Some(800),
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM execution_group_winner WHERE group_id=?1",
            rusqlite::params![ZERO_GROUP],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0,
    );
    assert_eq!(
        sqlite_reserve_request_for_execution(
            &c,
            "positive-request",
            "group-account",
            "group-key",
            100,
            60,
            &positive,
        )
        .unwrap(),
        Some(700),
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            "positive-request",
            "group-account",
            "group-key",
            100,
            50,
            Some("provider:positive"),
            None,
        )
        .unwrap(),
        Some(750),
    );
    assert_eq!(
        c.query_row(
            "SELECT winner_request_id FROM execution_group_winner WHERE group_id=?1",
            rusqlite::params![ZERO_GROUP],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "positive-request",
    );
}

#[test]
fn sqlite_execution_group_winner_is_pruned_only_after_all_group_replays_expire() {
    const GROUP: &str = "228f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";

    let c = db();
    acct_with_key(
        &c,
        "retained-group-account",
        "retained-group-key",
        1_000,
        2_000,
    );
    for (request_id, attempt) in [("retained-winner", 1), ("retained-loser", 2)] {
        assert!(sqlite_reserve_request_for_execution(
            &c,
            request_id,
            "retained-group-account",
            "retained-group-key",
            100,
            60,
            &ExecutionAttempt::grouped(GROUP, attempt).unwrap(),
        )
        .unwrap()
        .is_some());
    }
    assert_eq!(
        sqlite_settle_request(
            &c,
            "retained-winner",
            "retained-group-account",
            "retained-group-key",
            100,
            50,
            Some("retained:winner"),
            None,
        )
        .unwrap(),
        Some(850),
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            "retained-loser",
            "retained-group-account",
            "retained-group-key",
            100,
            60,
            Some("retained:loser"),
            None,
        )
        .unwrap(),
        Some(950),
    );
    c.execute(
        "UPDATE billing_reservations SET settled_ts=1 WHERE request_id='retained-winner'",
        [],
    )
    .unwrap();
    c.execute(
        "UPDATE billing_settlement_outbox SET committed_ts=1 WHERE request_id='retained-winner'",
        [],
    )
    .unwrap();
    let first_prune = sqlite_maintenance_prune(&c, 2).unwrap();
    assert_eq!((first_prune.outbox, first_prune.reservations), (1, 1));
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM execution_group_winner WHERE group_id=?1",
            rusqlite::params![GROUP],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            "retained-loser",
            "retained-group-account",
            "retained-group-key",
            100,
            60,
            Some("retained:loser"),
            None,
        )
        .unwrap(),
        Some(950),
    );

    c.execute(
        "UPDATE billing_reservations SET settled_ts=1 WHERE request_id='retained-loser'",
        [],
    )
    .unwrap();
    c.execute(
        "UPDATE billing_settlement_outbox SET committed_ts=1 WHERE request_id='retained-loser'",
        [],
    )
    .unwrap();
    let second_prune = sqlite_maintenance_prune(&c, 2).unwrap();
    assert_eq!((second_prune.outbox, second_prune.reservations), (1, 1));
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM execution_group_winner WHERE group_id=?1",
            rusqlite::params![GROUP],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0,
    );
}

#[test]
fn sqlite_legacy_snapshot_reserve_is_atomic_and_exactly_idempotent() {
    use pricing::{LegacyScalarReserveConflict as Conflict, LegacyScalarReserveOutcome as O};

    let c = db();
    acct_with_key(&c, "snapshot-account", "snapshot-key", 1_000, 2_000);
    let snapshot = legacy_snapshot("snapshot-request", "snapshot-account", 500, 100);

    let inserted =
        sqlite_reserve_request_with_legacy_snapshot(&c, "snapshot-key", 60, &snapshot).unwrap();
    let O::Inserted(inserted) = inserted else {
        panic!("first exact snapshot reservation was not inserted");
    };
    assert_eq!(inserted.balance_after_reserve_nano, 900);
    assert_eq!(inserted.snapshot, snapshot);
    assert_eq!(
        pricing::sqlite_legacy_scalar_admission_snapshot(&c, "snapshot-request")
            .unwrap()
            .unwrap(),
        snapshot
    );
    let original_lease: i64 = c
        .query_row(
            "SELECT lease_until FROM billing_reservations WHERE request_id='snapshot-request'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let replay =
        sqlite_reserve_request_with_legacy_snapshot(&c, "snapshot-key", 9_999, &snapshot).unwrap();
    let O::Unchanged(replay) = replay else {
        panic!("exact snapshot replay was not idempotent");
    };
    assert_eq!(replay.balance_after_reserve_nano, 900);
    assert_eq!(replay.snapshot, snapshot);
    assert_eq!(
        c.query_row(
            "SELECT lease_until FROM billing_reservations WHERE request_id='snapshot-request'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        original_lease
    );
    assert!(sqlite_mark_delivering(&c, "snapshot-request", 60).unwrap());
    let delivering_lease: i64 = c
        .query_row(
            "SELECT lease_until FROM billing_reservations WHERE request_id='snapshot-request'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(matches!(
        sqlite_reserve_request_with_legacy_snapshot(&c, "snapshot-key", 60, &snapshot).unwrap(),
        O::Unchanged(_)
    ));
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*),MIN(hold_nano),MAX(hold_nano) FROM billing_reservations
             WHERE request_id='snapshot-request'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?
            )),
        )
        .unwrap(),
        (1, 100, 100)
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM pricing_admission_snapshots
             WHERE request_id='snapshot-request'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    assert_eq!(
        c.query_row(
            "SELECT lease_until FROM billing_reservations WHERE request_id='snapshot-request'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        delivering_lease
    );
    let account = account_get(&c, "snapshot-account").unwrap().unwrap();
    let key = key_get(&c, "snapshot-key").unwrap().unwrap();
    assert_eq!((account.balance_nano, account.reserved_nano), (900, 100));
    assert_eq!(key.reserved_nano, 100);

    let different = legacy_snapshot("snapshot-request", "snapshot-account", 501, 100);
    assert_eq!(
        sqlite_reserve_request_with_legacy_snapshot(&c, "snapshot-key", 60, &different).unwrap(),
        O::Conflict(Conflict::SnapshotPayload)
    );
    assert_eq!(
        sqlite_reserve_request_with_legacy_snapshot(&c, "different-key", 60, &snapshot).unwrap(),
        O::Conflict(Conflict::ReservationIdentity)
    );

    assert_eq!(
        sqlite_reserve_request(
            &c,
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
        sqlite_reserve_request_with_legacy_snapshot(&c, "snapshot-key", 60, &legacy_only).unwrap(),
        O::Conflict(Conflict::ExistingReservationWithoutSnapshot)
    );
    assert!(
        pricing::sqlite_legacy_scalar_admission_snapshot(&c, "legacy-only")
            .unwrap()
            .is_none()
    );

    sqlite_settle_request(
        &c,
        "snapshot-request",
        "snapshot-account",
        "snapshot-key",
        100,
        10,
        Some("snapshot-settle"),
        None,
    )
    .unwrap();
    assert_eq!(
        sqlite_reserve_request_with_legacy_snapshot(&c, "snapshot-key", 60, &snapshot).unwrap(),
        O::Conflict(Conflict::TerminalReservation)
    );
}

#[test]
fn sqlite_guarded_legacy_snapshot_aborts_before_commit_without_compensation() {
    use pricing::LegacyScalarReserveOutcome as O;

    let c = db();
    acct_with_key(&c, "guarded-account", "guarded-key", 1_000, 2_000);
    let snapshot = legacy_snapshot("guarded-request", "guarded-account", 500, 100);
    let mut insert_gate_calls = 0;
    assert_eq!(
        sqlite_reserve_request_with_legacy_snapshot_guarded(
            &c,
            "guarded-key",
            60,
            &snapshot,
            || {
                insert_gate_calls += 1;
                false
            },
        )
        .unwrap(),
        O::AbortedBeforeCommit
    );
    assert_eq!(insert_gate_calls, 1);
    assert_eq!(
        c.query_row(
            "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano, \
                    (SELECT COUNT(*) FROM billing_reservations), \
                    (SELECT COUNT(*) FROM pricing_admission_snapshots) \
               FROM accounts a JOIN api_keys k ON k.account_id=a.id \
              WHERE a.id='guarded-account' AND k.key='guarded-key'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            )),
        )
        .unwrap(),
        (1_000, 0, 0, 0, 0)
    );

    assert!(matches!(
        sqlite_reserve_request_with_legacy_snapshot(&c, "guarded-key", 60, &snapshot).unwrap(),
        O::Inserted(_)
    ));
    let mut replay_gate_calls = 0;
    assert_eq!(
        sqlite_reserve_request_with_legacy_snapshot_guarded(
            &c,
            "guarded-key",
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
    assert_eq!(
        c.query_row(
            "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano, \
                    (SELECT COUNT(*) FROM billing_reservations), \
                    (SELECT COUNT(*) FROM pricing_admission_snapshots) \
               FROM accounts a JOIN api_keys k ON k.account_id=a.id \
              WHERE a.id='guarded-account' AND k.key='guarded-key'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            )),
        )
        .unwrap(),
        (900, 100, 100, 1, 1)
    );
}

#[test]
fn sqlite_legacy_snapshot_failure_never_leaves_money_or_orphans() {
    use pricing::LegacyScalarReserveOutcome as O;

    let rejected = db();
    acct_with_key(&rejected, "poor-account", "poor-key", 50, 2_000);
    let too_large = legacy_snapshot("poor-request", "poor-account", 500, 100);
    assert_eq!(
        sqlite_reserve_request_with_legacy_snapshot(&rejected, "poor-key", 60, &too_large).unwrap(),
        O::NotReserved
    );
    assert_eq!(
        rejected
            .query_row(
                "SELECT balance_nano,reserved_nano FROM accounts WHERE id='poor-account'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap(),
        (50, 0)
    );
    assert_eq!(
        rejected
            .query_row(
                "SELECT
                     (SELECT COUNT(*) FROM billing_reservations WHERE request_id='poor-request'),
                     (SELECT COUNT(*) FROM pricing_admission_snapshots WHERE request_id='poor-request')",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap(),
        (0, 0)
    );

    let failing = db();
    acct_with_key(&failing, "rollback-account", "rollback-key", 1_000, 2_000);
    failing
        .execute_batch(
            "CREATE TRIGGER reject_test_legacy_snapshot
             BEFORE INSERT ON pricing_admission_snapshots
             BEGIN
                 SELECT RAISE(ABORT, 'injected snapshot failure');
             END;",
        )
        .unwrap();
    let snapshot = legacy_snapshot("rollback-request", "rollback-account", 500, 100);
    assert!(
        sqlite_reserve_request_with_legacy_snapshot(&failing, "rollback-key", 60, &snapshot)
            .is_err()
    );
    assert_eq!(
        failing
            .query_row(
                "SELECT balance_nano,reserved_nano FROM accounts WHERE id='rollback-account'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap(),
        (1_000, 0)
    );
    assert_eq!(
        failing
            .query_row(
                "SELECT reserved_nano FROM api_keys WHERE key='rollback-key'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        failing
            .query_row(
                "SELECT
                     (SELECT COUNT(*) FROM billing_reservations WHERE request_id='rollback-request'),
                     (SELECT COUNT(*) FROM pricing_admission_snapshots WHERE request_id='rollback-request')",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap(),
        (0, 0)
    );
}

#[test]
fn sqlite_legacy_snapshot_rejects_outside_replay_window_before_money() {
    use pricing::{LegacyScalarReserveConflict as Conflict, LegacyScalarReserveOutcome as O};

    let c = db();
    acct_with_key(&c, "window-account", "window-key", 1_000, 2_000);
    let baseline_ledger = c
        .query_row("SELECT COUNT(*) FROM ledger", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();
    let current = now();
    let expired = legacy_snapshot_at(
        "expired-window-request",
        "window-account",
        500,
        100,
        current - 2 * pricing::LEGACY_SCALAR_REPLAY_MAX_AGE_SECS,
    );
    assert_eq!(
        sqlite_reserve_request_with_legacy_snapshot(&c, "window-key", 60, &expired).unwrap(),
        O::Conflict(Conflict::ExpiredIdempotencyWindow)
    );

    let future = legacy_snapshot_at(
        "future-window-request",
        "window-account",
        500,
        100,
        current + 2 * pricing::LEGACY_SCALAR_REPLAY_MAX_AGE_SECS,
    );
    assert_eq!(
        sqlite_reserve_request_with_legacy_snapshot(&c, "window-key", 60, &future).unwrap(),
        O::Conflict(Conflict::AdmissionTimestampInFuture)
    );

    assert_eq!(
        c.query_row(
            "SELECT balance_nano,reserved_nano FROM accounts WHERE id='window-account'",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap(),
        (1_000, 0)
    );
    assert_eq!(
        c.query_row(
            "SELECT reserved_nano FROM api_keys WHERE key='window-key'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
    assert_eq!(
        c.query_row(
            "SELECT
               (SELECT COUNT(*) FROM billing_reservations),
               (SELECT COUNT(*) FROM pricing_admission_snapshots),
               (SELECT COUNT(*) FROM ledger)",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            )),
        )
        .unwrap(),
        (0, 0, baseline_ledger)
    );
}

#[test]
fn sqlite_maintenance_reports_pricing_rows_removed_by_terminal_cascade() {
    use pricing::{LegacyScalarReserveOutcome as O, PricingShadowEvaluationWrite as W};

    let c = db();
    acct_with_key(&c, "retention-account", "retention-key", 1_000, 2_000);
    let snapshot = legacy_snapshot("retention-request", "retention-account", 500, 100);
    assert!(matches!(
        sqlite_reserve_request_with_legacy_snapshot(&c, "retention-key", 60, &snapshot).unwrap(),
        O::Inserted(_)
    ));

    let actual = pricing::ShadowActualSnapshotRef::from_snapshot(&snapshot).unwrap();
    let manifest = pricing::PricingRuntimeManifestEvidence::new(
        1,
        vec![pricing::PricingRuntimeCapabilityEvidence::new(
            pricing::PRICING_SCHEMA_VERSION,
            1,
            "retention-capability-digest",
        )
        .unwrap()],
    )
    .unwrap();
    let evaluation = pricing::PricingShadowAdmissionEvaluationInput::new(
        actual,
        pricing::PRICING_SCHEMA_VERSION,
        manifest,
        snapshot.admission_ts(),
        snapshot.admission_ts(),
        pricing::PricingShadowEvaluationOutcome::ReadError {
            reason: pricing::PricingShadowReadErrorCode::PricingReadFailed,
        },
        pricing::ShadowDiagnosticContext::empty(),
    )
    .unwrap();
    assert!(matches!(
        pricing::sqlite_insert_pricing_shadow_admission_evaluation(&c, &evaluation).unwrap(),
        W::Inserted(_)
    ));

    sqlite_settle_request(
        &c,
        "retention-request",
        "retention-account",
        "retention-key",
        100,
        10,
        Some("retention-settle"),
        None,
    )
    .unwrap();
    assert!(sqlite_maintenance_prune(&c, now()).is_err());
    assert_eq!(
        c.query_row(
            "SELECT \
               (SELECT COUNT(*) FROM billing_reservations \
                 WHERE request_id='retention-request'), \
               (SELECT COUNT(*) FROM pricing_admission_snapshots \
                 WHERE request_id='retention-request'), \
               (SELECT COUNT(*) FROM pricing_shadow_admission_evaluations \
                 WHERE request_id='retention-request')",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?
            )),
        )
        .unwrap(),
        (1, 1, 1)
    );
    c.execute(
        "UPDATE billing_reservations SET settled_ts=100 WHERE request_id='retention-request'",
        [],
    )
    .unwrap();
    c.execute(
        "UPDATE billing_settlement_outbox SET committed_ts=100,state='done' \
         WHERE request_id='retention-request'",
        [],
    )
    .unwrap();
    let ledger_before = c
        .query_row("SELECT COUNT(*) FROM ledger", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();

    let report = sqlite_maintenance_prune(&c, 200).unwrap();
    assert_eq!(report.outbox, 1);
    assert_eq!(report.reservations, 1);
    assert_eq!(report.pricing_snapshots_cascaded, 1);
    assert_eq!(report.pricing_shadow_evaluations_cascaded, 1);
    assert_eq!(
        c.query_row(
            "SELECT
               (SELECT COUNT(*) FROM billing_reservations),
               (SELECT COUNT(*) FROM pricing_admission_snapshots),
               (SELECT COUNT(*) FROM pricing_shadow_admission_evaluations),
               (SELECT COUNT(*) FROM execution_group_winner),
               (SELECT COUNT(*) FROM ledger)",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            )),
        )
        .unwrap(),
        (0, 0, 0, 0, ledger_before)
    );
}

#[test]
fn sqlite_openai_legacy_snapshot_roundtrips_typed_modifiers() {
    use pricing::LegacyScalarReserveOutcome as O;

    let c = db();
    acct_with_key(
        &c,
        "openai-snapshot-account",
        "openai-snapshot-key",
        1_000,
        2_000,
    );
    let snapshot = openai_legacy_snapshot(
        "openai-snapshot-request",
        "openai-snapshot-account",
        500,
        100,
    );
    assert!(matches!(
        sqlite_reserve_request_with_legacy_snapshot(&c, "openai-snapshot-key", 60, &snapshot)
            .unwrap(),
        O::Inserted(_)
    ));
    assert_eq!(
        pricing::sqlite_legacy_scalar_admission_snapshot(&c, "openai-snapshot-request")
            .unwrap()
            .unwrap(),
        snapshot
    );
    assert!(pricing::sqlite_legacy_scalar_admission_snapshot(&c, "invalid\0request").is_err());
}

#[test]
fn sqlite_pending_settlement_survives_until_recovery() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000, 2000);
    sqlite_reserve_request(&c, "req", "a", "k", 500, 60).unwrap();
    sqlite_mark_delivering(&c, "req", 60).unwrap();
    // Simulate a process crash after durable intent commit but before the balance transaction.
    assert_eq!(
        sqlite_enqueue_settlement(
            &c,
            "req",
            "a",
            "k",
            500,
            175,
            Some("provider:req"),
            None,
            "settle",
        )
        .unwrap(),
        None,
    );
    let before = account_get(&c, "a").unwrap().unwrap();
    assert_eq!((before.balance_nano, before.reserved_nano), (500, 500));
    let report = sqlite_reconcile_expired(&c, 100, false).unwrap();
    assert_eq!(report.processed_outbox, 1);
    let after = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(
        (after.balance_nano, after.spent_nano, after.reserved_nano),
        (825, 175, 0)
    );
}

#[test]
fn sqlite_expired_reservations_follow_delivery_state() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000, 2000);
    sqlite_reserve_request(&c, "pre", "a", "k", 200, 60).unwrap();
    sqlite_reserve_request(&c, "delivered", "a", "k", 300, 60).unwrap();
    sqlite_mark_delivering(&c, "delivered", 60).unwrap();
    c.execute("UPDATE billing_reservations SET lease_until=0", [])
        .unwrap();
    // Default policy: a turn that died before reporting any usage is released, not billed at the
    // admission ceiling. The pre-delivery reservation is cancelled either way.
    let report = sqlite_reconcile_expired(&c, 100, false).unwrap();
    assert_eq!(report.canceled_before_delivery, 1);
    assert_eq!(report.charged_after_delivery, 1);
    let account = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(
        (
            account.balance_nano,
            account.spent_nano,
            account.reserved_nano
        ),
        (1_000, 0, 0)
    );
}

/// The conservative fallback is a switch, not a deletion: an operator facing a provider that stops
/// reporting usage must be able to restore full-hold recovery without a deploy.
#[test]
fn sqlite_expired_delivery_bills_the_hold_only_when_the_fallback_is_re_armed() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000, 2000);
    sqlite_reserve_request(&c, "delivered", "a", "k", 300, 60).unwrap();
    sqlite_mark_delivering(&c, "delivered", 60).unwrap();
    c.execute("UPDATE billing_reservations SET lease_until=0", [])
        .unwrap();
    let report = sqlite_reconcile_expired(&c, 100, true).unwrap();
    assert_eq!(report.charged_after_delivery, 1);
    let account = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(
        (
            account.balance_nano,
            account.spent_nano,
            account.reserved_nano
        ),
        (700, 300, 0)
    );
}

#[test]
fn token_source_switch_resets_only_stale_health() {
    let c = db();
    add(&c, "sub@example.com", "token-a", "", "prod").unwrap();
    let dead = SubHealth {
        email: "sub@example.com".into(),
        auth_state: "dead".into(),
        auth_fail_streak: 3,
        first_auth_fail_ts: 1,
        last_auth_fail_ts: 2,
        last_auth_http: 401,
        dead_since_ts: 2,
        dead_reason: "authentication_error".into(),
        auth_token_fp: "old-fingerprint".into(),
    };
    save_sub_health(&c, &dead).unwrap();

    add(&c, "sub@example.com", "token-a", "proxy", "prod").unwrap();
    assert_eq!(load_sub_health(&c, None).unwrap()[0].auth_state, "dead");

    add(&c, "sub@example.com", "token-b", "proxy", "prod").unwrap();
    let changed = &load_sub_health(&c, None).unwrap()[0];
    assert_eq!(
        (changed.auth_state.as_str(), changed.auth_fail_streak),
        ("healthy", 0)
    );
    assert!(changed.auth_token_fp.is_empty());
    let sources: (Option<String>, Option<String>) = c
        .query_row(
            "SELECT token,token_file FROM subs WHERE email='sub@example.com'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(sources, (Some("token-b".into()), None));

    add_file(&c, "sub@example.com", "/tmp/token", "proxy", "prod").unwrap();
    let sources: (Option<String>, Option<String>) = c
        .query_row(
            "SELECT token,token_file FROM subs WHERE email='sub@example.com'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(sources, (None, Some("/tmp/token".into())));
}

/// Двойной settle (перекрытие деплоя: reconcile уже вернул резерв, затем settle старого инстанса)
/// НЕ переначисляет и НЕ уводит reserved в минус — кламп MIN(hold,reserved)/MAX(0,…).
#[test]
fn double_settle_no_overcredit() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000_000_000, 10000);
    account_reserve(&c, "a", 400_000_000).unwrap();
    // Эмулируем внешний/исторический возврат hold до прихода старого settle.
    c.execute(
        "UPDATE accounts SET balance_nano=balance_nano+reserved_nano, reserved_nano=0 WHERE id='a'",
        [],
    )
    .unwrap();
    assert_eq!(
        account_get(&c, "a").unwrap().unwrap().balance_nano,
        1_000_000_000
    );
    // теперь прилетает settle СТАРОГО инстанса на тот же hold (actual $0.1)
    account_settle(&c, "a", "k", 400_000_000, 100_000_000, None, None).unwrap();
    let acc = account_get(&c, "a").unwrap().unwrap();
    // без клампа было бы: +$0.4 (второй раз!) − $0.1 = $1.3 (over-credit) и reserved=−$0.4.
    // с клампом: MIN(0.4, reserved=0)=0 → баланс += 0 − $0.1 = $0.9; reserved MAX(0,−0.4)=0.
    assert_eq!(
        acc.balance_nano, 900_000_000,
        "нет over-credit: списан только actual"
    );
    assert_eq!(acc.reserved_nano, 0, "reserved не ушёл в минус");
}

/// release (settle с actual=0) возвращает резерв полностью, ledger-charge НЕ пишется.
#[test]
fn reserve_release_refunds_fully() {
    let c = db();
    acct_with_key(&c, "a", "k", 500_000_000, 2000);
    account_reserve(&c, "a", 200_000_000).unwrap();
    account_settle(&c, "a", "k", 200_000_000, 0, None, None).unwrap();
    assert_eq!(
        account_get(&c, "a").unwrap().unwrap().balance_nano,
        500_000_000
    );
    let charges: i64 = c
        .query_row("SELECT COUNT(*) FROM ledger WHERE kind='charge'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(charges, 0);
}

/// usage_events: запись по корзинам и агрегат по модели (суммы + real/charge nano + requests).
#[test]
fn usage_events_aggregate_by_model() {
    let c = db();
    acct_with_key(&c, "a", "k", 100_000_000_000, 4000);
    let opus = UsageEventInput {
        model: "claude-opus-4-8".into(),
        input_tokens: 1000,
        output_tokens: 500,
        cache_read_tokens: 200,
        cache_write_5m_tokens: 100,
        cache_write_1h_tokens: 50,
        web_search_requests: 2,
        real_nano: 20_000_000,
        charge_basis_nano: 20_000_000,
        ..Default::default()
    };
    usage_event_add(&c, "a", Some("k"), &opus, 8_000_000, Some("req1")).unwrap();
    usage_event_add(&c, "a", Some("k"), &opus, 8_000_000, Some("req2")).unwrap();
    let sonnet = UsageEventInput {
        model: "claude-sonnet-5".into(),
        input_tokens: 300,
        output_tokens: 100,
        real_nano: 5_000_000,
        charge_basis_nano: 5_000_000,
        ..Default::default()
    };
    usage_event_add(&c, "a", Some("k"), &sonnet, 2_000_000, Some("req3")).unwrap();

    let aggs = usage_by_model(&c, "a", 0).unwrap();
    assert_eq!(aggs.len(), 2);
    // сортировка по SUM(real_nano) DESC → opus первый (2×20M > 5M)
    let o = &aggs[0];
    assert_eq!(o.model, "claude-opus-4-8");
    assert_eq!(o.requests, 2);
    assert_eq!(o.input_tokens, 2000); // 2×1000
    assert_eq!(o.output_tokens, 1000);
    assert_eq!(o.cache_read_tokens, 400);
    assert_eq!(o.cache_write_5m_tokens, 200);
    assert_eq!(o.cache_write_1h_tokens, 100);
    assert_eq!(o.web_search_requests, 4);
    assert_eq!(o.real_nano, 40_000_000);
    assert_eq!(o.charge_nano, 16_000_000);
    assert_eq!(aggs[1].model, "claude-sonnet-5");
    assert_eq!(aggs[1].requests, 1);
    // окно отсекает по ts: since в будущем → пусто
    assert!(usage_by_model(&c, "a", now() + 10_000).unwrap().is_empty());
    // prune всего → таблица пуста
    assert!(usage_prune(&c, now() + 10_000).unwrap() >= 3);
    assert!(usage_by_model(&c, "a", 0).unwrap().is_empty());
}

#[test]
fn usage_report_uses_one_exact_window_for_daily_and_key_totals() {
    let c = db();
    acct_with_key(&c, "a", "k", 100_000_000_000, 4000);
    let day_one = 20_000 * 86_400;
    let day_two = day_one + 86_400;
    let first = UsageEventInput {
        model: "claude-opus-4-8".into(),
        input_tokens: 10,
        real_nano: 20_000_000,
        charge_basis_nano: 20_000_000,
        input_nano: 20_000_000,
        ..Default::default()
    };
    let second = UsageEventInput {
        model: "claude-opus-4-8".into(),
        provider: PROVIDER_OPENAI.into(),
        output_tokens: 10,
        real_nano: 30_000_000,
        charge_basis_nano: 30_000_000,
        output_nano: 30_000_000,
        ..Default::default()
    };
    let third = UsageEventInput {
        model: "claude-sonnet-5".into(),
        cache_read_tokens: 10,
        real_nano: 5_000_000,
        charge_basis_nano: 5_000_000,
        cache_read_nano: 5_000_000,
        ..Default::default()
    };
    usage_event_add(&c, "a", Some("k"), &first, 8_000_000, Some("r1")).unwrap();
    usage_event_add(&c, "a", Some("k"), &second, 12_000_000, Some("r2")).unwrap();
    usage_event_add(&c, "a", Some("k-other"), &third, 2_000_000, Some("r3")).unwrap();
    c.execute(
        "UPDATE usage_events SET ts=CASE ref \
         WHEN 'r1' THEN ?1 WHEN 'r2' THEN ?2 ELSE ?3 END",
        rusqlite::params![day_one + 100, day_one + 200, day_two + 10],
    )
    .unwrap();

    let report = usage_report(&c, "a", day_one + 150, day_two + 100).unwrap();
    assert_eq!(report.models.len(), 2);
    assert_eq!(
        report.daily,
        vec![
            UsageDailyAgg {
                day_ts: day_one,
                requests: 1,
                real_nano: 30_000_000,
                charge_nano: 12_000_000,
            },
            UsageDailyAgg {
                day_ts: day_two,
                requests: 1,
                real_nano: 5_000_000,
                charge_nano: 2_000_000,
            },
        ]
    );
    assert_eq!(
        report.daily_providers,
        vec![
            UsageDailyProviderAgg {
                day_ts: day_one,
                provider: PROVIDER_OPENAI.into(),
                requests: 1,
                real_nano: 30_000_000,
                charge_nano: 12_000_000,
            },
            UsageDailyProviderAgg {
                day_ts: day_two,
                provider: PROVIDER_ANTHROPIC.into(),
                requests: 1,
                real_nano: 5_000_000,
                charge_nano: 2_000_000,
            },
        ]
    );
    assert_eq!(
        report.keys,
        vec![
            UsageKeyAgg {
                key: Some("k".into()),
                requests: 1,
                real_nano: 30_000_000,
                charge_nano: 12_000_000,
            },
            UsageKeyAgg {
                key: Some("k-other".into()),
                requests: 1,
                real_nano: 5_000_000,
                charge_nano: 2_000_000,
            },
        ]
    );
    assert_eq!(
        report.daily.iter().map(|row| row.real_nano).sum::<i64>(),
        report.models.iter().map(|row| row.real_nano).sum::<i64>(),
    );
    assert_eq!(
        report.keys.iter().map(|row| row.charge_nano).sum::<i64>(),
        report.models.iter().map(|row| row.charge_nano).sum::<i64>(),
    );
    assert_eq!(
        usage_report(&c, "a", day_two, day_two).unwrap(),
        UsageReport::default()
    );
}

/// Оба апстрима сеттлятся в одни и те же денежные таблицы, поэтому «кто заработал» должно
/// читаться из явной колонки, а не угадываться по имени модели.
#[test]
fn spend_is_attributed_to_the_serving_provider() {
    let c = db();
    acct_with_key(&c, "a", "k", 100_000_000_000, 4000);
    let claude = UsageEventInput {
        model: "claude-opus-5".into(),
        provider: PROVIDER_ANTHROPIC.into(),
        real_nano: 20_000_000,
        charge_basis_nano: 20_000_000,
        ..Default::default()
    };
    let codex = UsageEventInput {
        model: "gpt-5.6".into(),
        provider: PROVIDER_OPENAI.into(),
        real_nano: 5_000_000,
        charge_basis_nano: 5_000_000,
        ..Default::default()
    };
    usage_event_add(&c, "a", Some("k"), &claude, 8_000_000, Some("req1")).unwrap();
    usage_event_add(&c, "a", Some("k"), &codex, 2_000_000, Some("req2")).unwrap();
    usage_event_add(&c, "a", Some("k"), &codex, 3_000_000, Some("req3")).unwrap();

    let rows = spend_by_provider(&c, 0).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].provider, PROVIDER_ANTHROPIC);
    assert_eq!(rows[0].requests, 1);
    assert_eq!(rows[0].charge_nano, 8_000_000);
    assert_eq!(rows[1].provider, PROVIDER_OPENAI);
    assert_eq!(rows[1].requests, 2);
    assert_eq!(rows[1].charge_nano, 5_000_000);
    assert_eq!(rows[1].real_nano, 10_000_000);
    // Окно отсекает по ts, как и остальные агрегаты панели.
    assert!(spend_by_provider(&c, now() + 10_000).unwrap().is_empty());
}

/// Строка, записанная релизом без атрибуции, должна читаться как Claude, а не выпадать из
/// разбивки: blue-green оставляет предыдущий слот пишущим во время промоушена.
#[test]
fn usage_written_before_attribution_reads_as_the_claude_fleet() {
    let c = db();
    acct_with_key(&c, "a", "k", 10_000_000_000, 4000);
    let legacy = UsageEventInput {
        model: "claude-opus-5".into(),
        real_nano: 1_000_000,
        charge_basis_nano: 1_000_000,
        ..Default::default()
    };
    usage_event_add(&c, "a", Some("k"), &legacy, 1_000_000, Some("req1")).unwrap();
    c.execute("UPDATE usage_events SET provider=''", [])
        .unwrap();
    let rows = spend_by_provider(&c, 0).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provider, PROVIDER_ANTHROPIC);

    // The queued settlement payload is JSON: a row serialized by the previous release carries
    // every field except this one, and must still decode instead of poisoning the outbox.
    let mut payload: serde_json::Value = serde_json::to_value(&legacy).unwrap();
    payload.as_object_mut().unwrap().remove("provider");
    let decoded: UsageEventInput = serde_json::from_value(payload).unwrap();
    assert_eq!(decoded.provider, PROVIDER_ANTHROPIC);
    assert_eq!(decoded.model, "claude-opus-5");
}

/// Разбивка расхода по моделям: top-N по charge, группировка по (model, provider) — один
/// model ID, обслуженный разными апстримами, не смешивается в одну строку.
#[test]
fn spend_is_broken_down_by_served_model() {
    let c = db();
    acct_with_key(&c, "a", "k", 100_000_000_000, 4000);
    let opus = UsageEventInput {
        model: "claude-opus-5".into(),
        real_nano: 20_000_000,
        charge_basis_nano: 20_000_000,
        ..Default::default()
    };
    let gpt = UsageEventInput {
        model: "gpt-5.6".into(),
        provider: PROVIDER_OPENAI.into(),
        real_nano: 5_000_000,
        charge_basis_nano: 5_000_000,
        ..Default::default()
    };
    usage_event_add(&c, "a", Some("k"), &opus, 8_000_000, Some("req1")).unwrap();
    usage_event_add(&c, "a", Some("k"), &gpt, 2_000_000, Some("req2")).unwrap();
    usage_event_add(&c, "a", Some("k"), &gpt, 3_000_000, Some("req3")).unwrap();

    let rows = spend_by_model(&c, 0, 20).unwrap();
    assert_eq!(rows.len(), 2);
    // сортировка по SUM(charge_nano) DESC → opus первый (8M > 2+3M)
    assert_eq!(rows[0].model, "claude-opus-5");
    assert_eq!(rows[0].provider, PROVIDER_ANTHROPIC);
    assert_eq!(rows[0].requests, 1);
    assert_eq!(rows[0].charge_nano, 8_000_000);
    assert_eq!(rows[0].real_nano, 20_000_000);
    assert_eq!(rows[1].model, "gpt-5.6");
    assert_eq!(rows[1].provider, PROVIDER_OPENAI);
    assert_eq!(rows[1].requests, 2);
    assert_eq!(rows[1].charge_nano, 5_000_000);
    // limit обрезает выдачу, окно — по ts, как у остальных spend-агрегатов
    assert_eq!(spend_by_model(&c, 0, 1).unwrap().len(), 1);
    assert!(spend_by_model(&c, now() + 10_000, 20).unwrap().is_empty());
}

/// Верхняя граница range-вариантов spend-агрегатов: полуоткрытое окно [since, until) —
/// событие ровно на `until` не попадает (стыкующиеся диапазоны не задваиваются), а open-ended
/// обёртки эквивалентны until=i64::MAX.
#[test]
fn spend_range_honors_upper_bound() {
    let c = db();
    acct_with_key(&c, "a", "k", 100_000_000_000, 2000);
    let usage = UsageEventInput {
        model: "claude-opus-5".into(),
        real_nano: 10_000_000,
        charge_basis_nano: 10_000_000,
        ..Default::default()
    };
    for (i, ts) in [1_000i64, 2_000, 3_000].iter().enumerate() {
        usage_event_add(
            &c,
            "a",
            Some("k"),
            &usage,
            1_000_000,
            Some(&format!("req{i}")),
        )
        .unwrap();
        c.execute(
            "UPDATE usage_events SET ts=?1 WHERE ref=?2",
            rusqlite::params![ts, format!("req{i}")],
        )
        .unwrap();
    }
    // [1000, 3000): события 1000 и 2000 внутри, 3000 — ровно на границе, исключено.
    let accounts = spend_by_account_range(&c, 1_000, 3_000, 50).unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].requests, 2);
    assert_eq!(accounts[0].charge_nano, 2_000_000);
    assert_eq!(accounts[0].real_nano, 20_000_000);
    assert_eq!(accounts[0].last_ts, 2_000);
    let providers = spend_by_provider_range(&c, 1_000, 3_000).unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].requests, 2);
    let models = spend_by_model_range(&c, 1_000, 3_000, 20).unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].requests, 2);
    // Нижняя граница включительна, пустой хвост за последним событием — пуст.
    assert_eq!(
        spend_by_account_range(&c, 2_000, 3_000, 50).unwrap()[0].requests,
        1
    );
    assert!(spend_by_provider_range(&c, 3_001, 9_999)
        .unwrap()
        .is_empty());
    // Open-ended обёртки видят всё, как раньше.
    assert_eq!(spend_by_account(&c, 0, 50).unwrap()[0].requests, 3);
    assert_eq!(spend_by_provider(&c, 0).unwrap()[0].requests, 3);
    assert_eq!(spend_by_model(&c, 0, 20).unwrap()[0].requests, 3);
}

/// Сводка settlement pipeline: counts по state, failed за 24ч, backlog старых несеттленых,
/// ≤10 failed с урезанным до 200 символов last_error и лаг pricing-консьюмера ledger'а.
#[test]
fn settlement_health_reports_outbox_and_consumer_lag() {
    let c = db();
    // Пустая БД: везде нули, oldest_* = 0, consumer lag без watermark'ов.
    let empty = settlement_health(&c, 300, "pricing").unwrap();
    assert_eq!(empty.pending + empty.done + empty.failed + empty.backlog, 0);
    assert_eq!(empty.oldest_unsettled_ts, 0);
    assert!(empty.recent_failed.is_empty());
    assert_eq!(empty.ledger_consumer.ledger_max_id, 0);
    assert_eq!(empty.ledger_consumer.checkpoints, 0);

    acct_with_key(&c, "a", "k", 100_000_000_000, 2000);
    let ts = now();
    let seed_outbox = |request_id: &str,
                       state: &str,
                       attempts: i64,
                       error: Option<&str>,
                       created: i64,
                       updated: i64| {
        c.execute(
            "INSERT INTO billing_settlement_outbox(request_id,actual_nano,state,attempts, \
             next_attempt_ts,last_error,created_ts,updated_ts) \
             VALUES(?1,1000,?2,?3,0,?4,?5,?6)",
            rusqlite::params![request_id, state, attempts, error, created, updated],
        )
        .unwrap();
    };
    seed_outbox("r-done", "done", 1, None, ts - 100, ts - 90);
    seed_outbox("r-pending-fresh", "pending", 0, None, ts - 10, ts - 10);
    seed_outbox(
        "r-pending-old",
        "pending",
        3,
        Some("transient pg error"),
        ts - 3600,
        ts - 60,
    );
    seed_outbox(
        "r-failed-new",
        "failed",
        5,
        Some(&"x".repeat(500)),
        ts - 7200,
        ts - 30,
    );
    seed_outbox(
        "r-failed-old",
        "failed",
        5,
        Some("invariant violated"),
        ts - 200_000,
        ts - 100_000,
    );

    let h = settlement_health(&c, 300, "pricing").unwrap();
    assert_eq!(h.pending, 2);
    assert_eq!(h.processing, 0);
    assert_eq!(h.done, 1);
    assert_eq!(h.failed, 2);
    assert_eq!(h.failed_24h, 1, "старый failed за пределами 24ч-окна");
    assert_eq!(h.pending_with_error, 1);
    assert_eq!(h.backlog, 1, "только r-pending-old старше 300с");
    assert_eq!(h.oldest_unsettled_ts, ts - 3600);
    assert_eq!(h.recent_failed.len(), 2);
    assert_eq!(
        h.recent_failed[0].request_id, "r-failed-new",
        "свежий failed первым"
    );
    assert_eq!(
        h.recent_failed[0]
            .last_error
            .as_deref()
            .unwrap()
            .chars()
            .count(),
        200,
        "last_error урезан до 200 символов"
    );
    assert_eq!(
        h.recent_failed[1].last_error.as_deref(),
        Some("invariant violated")
    );

    // Лаг консьюмера: первая topup-строка подтверждена (ack), вторая — ещё нет.
    let first: i64 = c
        .query_row("SELECT MIN(id) FROM ledger", [], |r| r.get(0))
        .unwrap();
    ledger_ack(&c, "pricing", "a", first).unwrap();
    account_topup(&c, "a", 1_000_000, None).unwrap();
    let h = settlement_health(&c, 300, "pricing").unwrap();
    let lag = &h.ledger_consumer;
    assert_eq!(lag.consumer, "pricing");
    assert!(lag.ledger_max_id > first);
    assert_eq!(lag.checkpoints, 1);
    assert_eq!(lag.checkpoint_min, first);
    assert_eq!(lag.unacked, 1, "вторая topup-строка выше watermark'а");
    assert!(lag.oldest_unacked_ts > 0);
    // Consumer без watermark'ов не считается отставшим (та же семантика, что у ledger_prune).
    let h = settlement_health(&c, 300, "unknown").unwrap();
    assert_eq!(h.ledger_consumer.checkpoints, 0);
    assert_eq!(h.ledger_consumer.unacked, 0);
    assert_eq!(h.ledger_consumer.oldest_unacked_ts, 0);
}

/// settle пишет usage_event В ТОЙ ЖЕ операции (один коммит); при actual=0 usage НЕ пишется.
#[test]
fn settle_writes_usage_event_in_same_tx() {
    let c = db();
    acct_with_key(&c, "a", "k", 10_000_000_000, 4000);
    account_reserve(&c, "a", 1_000_000_000).unwrap();
    let u = UsageEventInput {
        model: "claude-opus-4-8".into(),
        input_tokens: 100,
        output_tokens: 50,
        real_nano: 5_000_000,
        charge_basis_nano: 5_000_000,
        ..Default::default()
    };
    account_settle(
        &c,
        "a",
        "k",
        1_000_000_000,
        400_000_000,
        Some("req1"),
        Some(&u),
    )
    .unwrap();
    let charges: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM ledger WHERE kind='charge' AND account_id='a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(charges, 1, "charge записан");
    // charge-строка несёт модель (для точного per-model графика); topup/adjust — NULL.
    assert_eq!(
        ledger_recent(&c, "a", 10).unwrap()[0].model.as_deref(),
        Some("claude-opus-4-8"),
        "модель проставлена в ledger-charge"
    );
    let agg = usage_by_model(&c, "a", 0).unwrap();
    assert_eq!(agg.len(), 1);
    assert_eq!(agg[0].model, "claude-opus-4-8");
    assert_eq!(agg[0].input_tokens, 100);
    assert_eq!(agg[0].charge_nano, 400_000_000);
    // actual=0 (release/refund) → usage НЕ добавляется (charge не было)
    account_reserve(&c, "a", 500_000_000).unwrap();
    account_settle(&c, "a", "k", 500_000_000, 0, None, Some(&u)).unwrap();
    assert_eq!(
        usage_by_model(&c, "a", 0).unwrap()[0].requests,
        1,
        "usage не прибавился при actual=0"
    );
}

/// group-commit: reserve/settle в ОДНОЙ транзакции видят эффекты предыдущих (атомарность
/// `charge≤hold≤balance` сохранена), результаты в порядке ops, settle пишет usage.
#[test]
fn hot_batch_sequential_and_atomic() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000_000_000, 4000);
    // 3 резерва по 400M в одной пачке: 3-й видит списания первых двух → отказ (None).
    let ops = vec![
        HotOp::Reserve {
            account_id: "a",
            key: "k",
            hold: 400_000_000,
        },
        HotOp::Reserve {
            account_id: "a",
            key: "k",
            hold: 400_000_000,
        },
        HotOp::Reserve {
            account_id: "a",
            key: "k",
            hold: 400_000_000,
        },
    ];
    let r = apply_hot_batch(&c, &ops).unwrap();
    assert_eq!(r[0], Some(600_000_000));
    assert_eq!(r[1], Some(200_000_000));
    assert_eq!(
        r[2], None,
        "3-й резерв видит эффекты предыдущих в той же tx → отказ"
    );
    let acc = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(acc.balance_nano, 200_000_000);
    assert_eq!(acc.reserved_nano, 800_000_000);
    // settle в пачке: возвращает hold − actual, пишет usage; release (actual=0) возвращает hold.
    let u = UsageEventInput {
        model: "claude-opus-4-8".into(),
        input_tokens: 10,
        real_nano: 1000,
        charge_basis_nano: 1000,
        ..Default::default()
    };
    let ops2 = vec![
        HotOp::Settle {
            account_id: "a",
            key: "k",
            hold: 400_000_000,
            actual: 100_000_000,
            reference: Some("r1"),
            usage: Some(&u),
        },
        HotOp::Settle {
            account_id: "a",
            key: "k",
            hold: 400_000_000,
            actual: 0,
            reference: None,
            usage: None,
        },
    ];
    apply_hot_batch(&c, &ops2).unwrap();
    let acc = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(acc.balance_nano, 900_000_000); // 200 +300(settle1) +400(settle2)
    assert_eq!(acc.reserved_nano, 0);
    assert_eq!(acc.spent_nano, 100_000_000);
    assert_eq!(
        usage_by_model(&c, "a", 0).unwrap().len(),
        1,
        "usage записан из батча"
    );
}

/// заблокированный аккаунт не резервируется; резолв ключа отражает активность обоих.
#[test]
fn reserve_rejects_disabled_account() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000_000_000, 2000);
    assert!(key_account(&c, "k").unwrap().unwrap().active);
    account_set_status(&c, "a", "disabled").unwrap();
    assert_eq!(account_reserve(&c, "a", 1).unwrap(), None);
    assert!(!key_account(&c, "k").unwrap().unwrap().active); // аккаунт неактивен → ключ тоже
}

/// Идемпотентный topup: повтор вебхука с тем же payment-ref НЕ начисляет дважды.
#[test]
fn topup_is_idempotent_by_ref() {
    let c = db();
    account_create(&c, "a", None, 2000).unwrap();
    // первый вебхук: +$10, ref=tx_ABC
    assert_eq!(
        account_topup(&c, "a", 10_000_000_000, Some("tx_ABC")).unwrap(),
        Some(10_000_000_000)
    );
    // ПОВТОР того же вебхука (ретрай) — баланс НЕ должен вырасти
    assert_eq!(
        account_topup(&c, "a", 10_000_000_000, Some("tx_ABC")).unwrap(),
        Some(10_000_000_000)
    );
    assert_eq!(
        account_get(&c, "a").unwrap().unwrap().balance_nano,
        10_000_000_000
    ); // ровно $10
       // ДРУГОЙ ref начисляет нормально
    assert_eq!(
        account_topup(&c, "a", 5_000_000_000, Some("tx_XYZ")).unwrap(),
        Some(15_000_000_000)
    );
    // без ref (админ-коррекция) — не дедупится, всегда применяется
    account_topup(&c, "a", 1_000_000_000, None).unwrap();
    account_topup(&c, "a", 1_000_000_000, None).unwrap();
    assert_eq!(
        account_get(&c, "a").unwrap().unwrap().balance_nano,
        17_000_000_000
    );
    // в ledger ровно один topup на каждый уникальный ref (+ 2 без ref)
    let topups: i64 = c
        .query_row("SELECT COUNT(*) FROM ledger WHERE kind='topup'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(topups, 4); // tx_ABC, tx_XYZ, и 2 без ref
                           // Поздний точный replay возвращает сохранённый исходный результат, не текущий баланс.
    assert_eq!(
        account_topup(&c, "a", 10_000_000_000, Some("tx_ABC")).unwrap(),
        Some(10_000_000_000)
    );
}

/// A duplicate monetary reference succeeds only for the exact original operation.
#[test]
fn monetary_reference_rejects_parameter_mismatch_and_deduplicates_adjustments() {
    let c = db();
    account_create(&c, "a", None, 2000).unwrap();
    account_create(&c, "b", None, 2000).unwrap();
    assert_eq!(
        account_topup(&c, "a", 100, Some("payment:1")).unwrap(),
        Some(100)
    );
    assert!(account_topup(&c, "a", 200, Some("payment:1")).is_err());
    assert!(account_topup(&c, "b", 100, Some("payment:1")).is_err());
    assert_eq!(account_get(&c, "a").unwrap().unwrap().balance_nano, 100);
    assert_eq!(account_get(&c, "b").unwrap().unwrap().balance_nano, 0);
    assert_eq!(
        account_topup(&c, "a", -25, Some("adjust:1")).unwrap(),
        Some(75)
    );
    assert_eq!(
        account_topup(&c, "a", -25, Some("adjust:1")).unwrap(),
        Some(75)
    );
    assert!(account_topup(&c, "a", -30, Some("adjust:1")).is_err());
    assert_eq!(account_get(&c, "a").unwrap().unwrap().balance_nano, 75);
    assert!(account_topup(&c, "a", 1, Some("   ")).is_err());
}

/// Без consumer acknowledgement watermark charge-строки нельзя безопасно удалять.
#[test]
fn ledger_prune_is_disabled_without_consumer_watermarks() {
    let c = db();
    acct_with_key(&c, "a", "k", 5_000_000_000, 10000);
    account_reserve(&c, "a", 1_000_000_000).unwrap();
    account_settle(&c, "a", "k", 1_000_000_000, 400_000_000, Some("old"), None).unwrap();
    c.execute("UPDATE ledger SET ts = 1000", []).unwrap();
    assert_eq!(ledger_prune(&c, 2000).unwrap(), 0);
    let rows = ledger_after(&c, "a", 0, 10).unwrap();
    assert_eq!(
        rows.len(),
        2,
        "topup and unacknowledged charge remain cursor-visible"
    );
    assert!(rows.iter().any(|row| row.kind == "charge"));
}

/// N ключей под ОДНИМ аккаунтом тратят из ОБЩЕГО баланса (ключевая модель).
#[test]
fn multiple_keys_share_one_account_balance() {
    let c = db();
    account_create(&c, "team", Some("tg:123"), 2000).unwrap();
    account_topup(&c, "team", 1_000_000_000, None).unwrap(); // $1 на команду
    key_issue(&c, "k-alice", "team", Some("alice")).unwrap();
    key_issue(&c, "k-bob", "team", Some("bob")).unwrap();
    // оба ключа резолвятся в тот же аккаунт
    assert_eq!(
        key_account(&c, "k-alice").unwrap().unwrap().account_id,
        "team"
    );
    assert_eq!(
        key_account(&c, "k-bob").unwrap().unwrap().account_id,
        "team"
    );
    // alice тратит $0.30, bob $0.20 — из общего баланса
    account_reserve(&c, "team", 300_000_000).unwrap();
    account_settle(&c, "team", "k-alice", 300_000_000, 300_000_000, None, None).unwrap();
    account_reserve(&c, "team", 200_000_000).unwrap();
    account_settle(&c, "team", "k-bob", 200_000_000, 200_000_000, None, None).unwrap();
    assert_eq!(
        account_get(&c, "team").unwrap().unwrap().balance_nano,
        500_000_000
    ); // $0.50 осталось
       // атрибуция по ключам раздельная
    assert_eq!(
        key_get(&c, "k-alice").unwrap().unwrap().spent_nano,
        300_000_000
    );
    assert_eq!(
        key_get(&c, "k-bob").unwrap().unwrap().spent_nano,
        200_000_000
    );
    // вход по handle
    assert_eq!(account_by_handle(&c, "tg:123").unwrap().unwrap().id, "team");
}

/// Control-plane management uses a stable public ID and never needs to persist the raw key.
#[test]
fn key_can_be_disabled_by_non_secret_id() {
    let c = db();
    account_create(&c, "acct", None, 2000).unwrap();
    key_issue(&c, "sk-pool-super-secret", "acct", Some("prod")).unwrap();
    let issued = key_get(&c, "sk-pool-super-secret").unwrap().unwrap();
    assert!(issued.key_id.starts_with("key_"));
    assert_eq!(
        key_set_status_by_id(&c, &issued.key_id, "disabled").unwrap(),
        1
    );
    assert_eq!(
        key_set_label_by_id(&c, &issued.key_id, "renamed").unwrap(),
        1
    );
    let updated = key_get(&c, "sk-pool-super-secret").unwrap().unwrap();
    assert_eq!(updated.status, "disabled");
    assert_eq!(updated.label.as_deref(), Some("renamed"));
    assert_eq!(key_set_label_by_id(&c, "key_missing", "unused").unwrap(), 0);
}

#[test]
fn per_key_policy_gates_reservations_and_releases_allowance() {
    let c = db();
    account_create(&c, "acct", None, 10_000).unwrap();
    account_topup(&c, "acct", 1_000, None).unwrap();
    key_issue_with_policy(
        &c,
        "limited",
        "acct",
        Some("limited"),
        Some(700),
        Some(now() + 60),
    )
    .unwrap();

    assert_eq!(
        account_reserve_for_key(&c, "acct", "limited", 500).unwrap(),
        Some(500)
    );
    assert_eq!(
        account_reserve_for_key(&c, "acct", "limited", 300).unwrap(),
        None
    );
    let account = account_get(&c, "acct").unwrap().unwrap();
    assert_eq!((account.balance_nano, account.reserved_nano), (500, 500));

    account_settle(&c, "acct", "limited", 500, 400, None, None).unwrap();
    let key = key_get(&c, "limited").unwrap().unwrap();
    assert_eq!(
        (key.spent_nano, key.reserved_nano, key.spend_limit_nano),
        (400, 0, Some(700))
    );

    assert_eq!(
        account_reserve_for_key(&c, "acct", "limited", 300).unwrap(),
        Some(300)
    );
    account_settle(&c, "acct", "limited", 300, 0, None, None).unwrap();
    assert_eq!(key_get(&c, "limited").unwrap().unwrap().reserved_nano, 0);

    key_issue_with_policy(&c, "expired", "acct", None, None, Some(now())).unwrap();
    assert_eq!(
        account_reserve_for_key(&c, "acct", "expired", 1).unwrap(),
        None
    );
    assert_eq!(account_get(&c, "acct").unwrap().unwrap().reserved_nano, 0);
    let expired_auth = key_account(&c, "expired").unwrap().unwrap();
    assert!(expired_auth.active);
    assert!(
        !expired_auth.active_at(now()),
        "expiry is exclusive at the exact second"
    );

    key_set_status(&c, "limited", "disabled").unwrap();
    assert_eq!(
        account_reserve_for_key(&c, "acct", "limited", 1).unwrap(),
        None
    );
    assert!(!key_account(&c, "limited")
        .unwrap()
        .unwrap()
        .active_at(now()));
}

#[test]
fn key_policy_can_be_replaced_without_undercutting_live_usage() {
    let c = db();
    account_create(&c, "acct", None, 10_000).unwrap();
    account_topup(&c, "acct", 2_000, None).unwrap();
    key_issue_with_policy(&c, "mutable", "acct", None, Some(1_000), Some(now() + 60)).unwrap();
    let key_id = key_get(&c, "mutable").unwrap().unwrap().key_id;

    assert_eq!(
        account_reserve_for_key(&c, "acct", "mutable", 600).unwrap(),
        Some(1_400)
    );
    assert_eq!(
        key_set_policy_by_id(&c, "acct", &key_id, Some(599), None).unwrap(),
        KeyPolicyUpdate::LimitBelowUsage,
    );
    assert_eq!(
        key_get(&c, "mutable").unwrap().unwrap().spend_limit_nano,
        Some(1_000)
    );
    assert_eq!(
        key_set_policy_by_id(&c, "acct", &key_id, Some(600), None).unwrap(),
        KeyPolicyUpdate::Updated,
    );
    account_settle(&c, "acct", "mutable", 600, 500, None, None).unwrap();
    assert_eq!(
        key_set_policy_by_id(&c, "acct", &key_id, Some(499), None).unwrap(),
        KeyPolicyUpdate::LimitBelowUsage,
    );

    let future = now() + 3_600;
    assert_eq!(
        key_set_policy_by_id(&c, "acct", &key_id, None, Some(future)).unwrap(),
        KeyPolicyUpdate::Updated,
    );
    let updated = key_get(&c, "mutable").unwrap().unwrap();
    assert_eq!(
        (updated.spend_limit_nano, updated.expires_ts),
        (None, Some(future))
    );
    assert_eq!(
        key_set_policy_by_id(&c, "acct", &key_id, None, None).unwrap(),
        KeyPolicyUpdate::Updated,
    );
    key_set_status_by_id(&c, &key_id, "disabled").unwrap();
    assert_eq!(
        key_set_policy_by_id(&c, "acct", &key_id, None, Some(now() + 7_200)).unwrap(),
        KeyPolicyUpdate::Updated,
    );
    assert!(!key_account(&c, "mutable")
        .unwrap()
        .unwrap()
        .active_at(now()));
    assert_eq!(
        key_set_policy_by_id(&c, "other-account", &key_id, None, None).unwrap(),
        KeyPolicyUpdate::NotFound,
    );
    assert_eq!(
        key_set_policy_by_id(&c, "acct", "key_missing", None, None).unwrap(),
        KeyPolicyUpdate::NotFound,
    );
}

#[test]
fn ledger_cursor_is_oldest_first_and_multiplier_is_mutable() {
    let c = db();
    acct_with_key(&c, "acct", "key", 2_000_000_000, 4000);
    account_reserve(&c, "acct", 100_000_000).unwrap();
    account_settle(
        &c,
        "acct",
        "key",
        100_000_000,
        50_000_000,
        Some("request"),
        None,
    )
    .unwrap();
    let first = ledger_after(&c, "acct", 0, 1).unwrap();
    assert_eq!(first.len(), 1);
    let rest = ledger_after(&c, "acct", first[0].id, 10).unwrap();
    assert_eq!(rest.len(), 1);
    assert!(rest[0].id > first[0].id);
    assert!(rest[0].attribution.is_none());
    assert!(rest[0].funding_allocations.is_empty());
    let funding = account_funding_snapshot(&c, "acct")
        .unwrap()
        .unwrap()
        .funding;
    assert_eq!(funding.bucket_count, 0);
    assert_eq!(funding.unattributed_balance_nano, 1_950_000_000);
    assert_eq!(funding.unattributed_spent_nano, 50_000_000);
    assert_eq!(account_set_mult_bp(&c, "acct", 3500).unwrap(), 1);
    assert_eq!(account_get(&c, "acct").unwrap().unwrap().mult_bp, 3500);
}

#[test]
fn ledger_reads_recover_exact_provider_from_matching_usage() {
    let c = db();
    account_create(&c, "provider-account", None, 10_000).unwrap();
    c.execute_batch(
        "INSERT INTO ledger(
             account_id,kind,request_id,amount_nano,balance_after_nano,ts,model,provider
         ) VALUES(
             'provider-account','charge','provider-request',250,9750,100,'gpt-test',NULL
         );
         INSERT INTO usage_events(
             request_id,account_id,model,real_nano,charge_nano,ts,provider
         ) VALUES(
             'provider-request','provider-account','gpt-test',500,250,100,'openai'
         );",
    )
    .unwrap();

    let rows = ledger_after(&c, "provider-account", 0, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provider.as_deref(), Some(PROVIDER_OPENAI));

    c.execute(
        "UPDATE ledger SET provider='anthropic' WHERE request_id='provider-request'",
        [],
    )
    .unwrap();
    assert!(ledger_after(&c, "provider-account", 0, 10).is_err());
}

#[test]
fn ledger_reads_recover_legacy_provider_only_from_strict_settlement_fingerprint() {
    let c = db();
    account_create(&c, "legacy-provider-account", None, 10_000).unwrap();
    account_create(&c, "other-provider-account", None, 10_000).unwrap();
    c.execute_batch(
        "INSERT INTO ledger(
             account_id,key,kind,amount_nano,ref,balance_after_nano,ts,model,provider
         ) VALUES
             ('legacy-provider-account','legacy-key','charge',250,'legacy:exact',9750,100,
              'gpt-legacy',NULL),
             ('legacy-provider-account','legacy-key','charge',125,'legacy:claude',9625,200,
              'claude-legacy',NULL),
             ('legacy-provider-account','legacy-key','charge',75,'legacy:conflict',9550,300,
              'ambiguous-model',NULL),
             ('legacy-provider-account','legacy-key','charge',60,'legacy:model-only',9490,400,
              'gpt-5',NULL);

         INSERT INTO usage_events(
             account_id,key,model,real_nano,charge_nano,ref,ts,provider
         ) VALUES
             ('legacy-provider-account','legacy-key','gpt-legacy',500,250,'legacy:exact',101,
              'openai');
         INSERT INTO usage_events(
             account_id,key,model,real_nano,charge_nano,ref,ts
         ) VALUES
             ('legacy-provider-account','legacy-key','claude-legacy',250,125,'legacy:claude',200);
         INSERT INTO usage_events(
             account_id,key,model,real_nano,charge_nano,ref,ts,provider
         ) VALUES
             ('legacy-provider-account','legacy-key','ambiguous-model',150,75,
              'legacy:conflict',300,'openai'),
             ('legacy-provider-account','legacy-key','ambiguous-model',150,75,
              'legacy:conflict',300,'google'),
             ('legacy-provider-account','wrong-key','gpt-5',120,60,'legacy:model-only',400,
              'openai'),
             ('legacy-provider-account','legacy-key','gpt-5',122,61,'legacy:model-only',400,
              'openai'),
             ('legacy-provider-account','legacy-key','gpt-5',120,60,'wrong-ref',400,'openai'),
             ('legacy-provider-account','legacy-key','wrong-model',120,60,
              'legacy:model-only',400,'openai'),
             ('legacy-provider-account','legacy-key','gpt-5',120,60,'legacy:model-only',402,
              'openai'),
             ('other-provider-account','legacy-key','gpt-5',120,60,'legacy:model-only',400,
              'openai');
         INSERT INTO usage_events(
             request_id,account_id,key,model,real_nano,charge_nano,ref,ts,provider
         ) VALUES(
             'unrelated-request','legacy-provider-account','legacy-key','gpt-5',120,60,
             'legacy:model-only',400,'openai'
         );",
    )
    .unwrap();

    let rows = ledger_after(&c, "legacy-provider-account", 0, 10).unwrap();
    assert_eq!(rows.len(), 4);
    let provider_for = |reference: &str| {
        rows.iter()
            .find(|row| row.reference.as_deref() == Some(reference))
            .unwrap()
            .provider
            .as_deref()
    };
    assert_eq!(provider_for("legacy:exact"), Some(PROVIDER_OPENAI));
    assert_eq!(provider_for("legacy:claude"), Some(PROVIDER_ANTHROPIC));
    assert_eq!(provider_for("legacy:conflict"), None);
    assert_eq!(provider_for("legacy:model-only"), None);
}

#[test]
fn strict_policy_reserve_settlement_and_topup_preserve_funding_identity() {
    use crate::pricing::{
        AccountClass, FundingEnforcement, LegacyPremiumModifiers, PolicyAdmissionSnapshot,
        PolicyAdmissionSnapshotInput, PolicyEnforcement, PolicyReserveOutcome, PolicyRuleScope,
        PricingMode, ReconciliationState, RuleOrigin, SnapshotAnthropicInferenceGeo,
        SnapshotAnthropicSpeed, SnapshotProvider,
    };

    let c = db();
    account_create(&c, "strict-account", None, 5_000).unwrap();
    account_topup(&c, "strict-account", 1_000, Some("strict-seed")).unwrap();
    c.execute_batch(
        "INSERT INTO pricing_catalog_versions(
             product_id,generation,schema_version,capability_generation,capability_digest,
             content_digest,created_ts
         ) VALUES('main',1,1,1,'capability','catalog-digest',1);
         INSERT INTO pricing_catalog_entries(
             product_id,generation,provider_id,canonical_model_id,enabled
         ) VALUES('main',1,'anthropic','claude-test',1);
         INSERT INTO pricing_catalog_heads(product_id,active_generation,updated_ts)
         VALUES('main',1,1);
         INSERT INTO provider_switch_versions(
             generation,schema_version,capability_generation,capability_digest,
             content_digest,created_ts
         ) VALUES(1,1,1,'capability','switch-digest',1);
         INSERT INTO provider_switch_entries(
             generation,provider_id,scope_type,product_id,segment,catalog_generation,enabled
         ) VALUES
             (1,'anthropic','master','','',NULL,1),
             (1,'anthropic','segment','main','b2c',1,1);
         INSERT INTO provider_switch_head(singleton,active_generation,updated_ts)
         VALUES(1,1,1);
         INSERT INTO account_policy_versions(
             account_id,effective_version,policy_id,policy_version,source_policy_digest,
             owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
             switch_generation,content_digest,replacement_locked,created_ts
         ) VALUES(
             'strict-account',1,'b2c:global',1,'source-policy','global_b2c','global','b2c',
             'main',1,1,1,'policy-digest',0,1
         );
         INSERT INTO account_policy_rules(
             account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
             canonical_model_id,pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
             track_eligible,retention_eligible,commission_eligible
         ) VALUES
             ('strict-account',1,'track-provider','track-digest','provider','anthropic',NULL,
              'track','managed',NULL,5000,1,1,1),
             ('strict-account',1,'static-model','static-digest','model','anthropic',
              'claude-test','discount','managed',0,10000,0,0,0);
         INSERT INTO account_policy_bindings(
             account_id,product_id,account_class,active_effective_version,
             policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
         ) VALUES('strict-account','main','b2c',1,'shadow','shadow','verified',1);
         INSERT INTO funding_buckets(
             bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
             reserved_nano,spent_nano,version,status,created_ts,updated_ts
         ) VALUES
             ('strict-bonus','strict-account','welcome_track_bonus','welcome','track',400,
              0,0,1,'active',1,1),
             ('strict-paid','strict-account','paid','seed','any',600,
              0,0,1,'active',2,2);",
    )
    .unwrap();
    let ack = KeyActivationPolicyAck {
        effective_policy_version: 1,
        policy_digest: "policy-digest".into(),
    };
    key_issue_with_policy_ack(
        &c,
        "strict-key",
        "strict-account",
        None,
        None,
        None,
        Some(&ack),
    )
    .unwrap();
    c.execute(
        "UPDATE account_policy_bindings
            SET policy_enforcement='strict',funding_enforcement='strict'
          WHERE account_id='strict-account'",
        [],
    )
    .unwrap();
    let auth = key_account(&c, "strict-key").unwrap().unwrap();
    assert!(auth.active_at(now()));
    assert_eq!(auth.policy_enforcement, Some(PolicyEnforcement::Strict));
    assert_eq!(auth.funding_enforcement, Some(FundingEnforcement::Strict));
    assert_eq!(
        auth.reconciliation_state,
        Some(ReconciliationState::Verified)
    );
    assert_eq!(
        (auth.paid_available_nano, auth.track_available_nano),
        (Some(600), Some(1_000))
    );

    let admission_ts = now();
    let track_snapshot = PolicyAdmissionSnapshot::new(PolicyAdmissionSnapshotInput {
        request_id: "strict-track-request".into(),
        account_id: "strict-account".into(),
        provider: SnapshotProvider::Anthropic,
        product_id: "main".into(),
        account_class: AccountClass::B2c,
        requested_model_id: "claude-test".into(),
        canonical_model_id: "claude-test".into(),
        alias_generation: 1,
        rule_id: "track-provider".into(),
        rule_digest: "track-digest".into(),
        rule_scope: PolicyRuleScope::Provider {
            provider_id: "anthropic".into(),
        },
        pricing_mode: PricingMode::Track,
        rule_origin: RuleOrigin::Managed,
        discount_bps: None,
        payable_multiplier_bp: 5_000,
        policy_id: "b2c:global".into(),
        policy_version: 1,
        effective_policy_version: 1,
        source_policy_digest: "source-policy".into(),
        policy_digest: "policy-digest".into(),
        policy_catalog_generation: 1,
        policy_switch_generation: 1,
        admission_catalog_generation: 1,
        admission_catalog_digest: "catalog-digest".into(),
        admission_switch_generation: 1,
        admission_switch_digest: "switch-digest".into(),
        runtime_manifest_generation: 1,
        runtime_manifest_digest: "runtime-manifest".into(),
        tariff_schedule_id: "anthropic/claude-test/v1".into(),
        tariff_priced_ts: admission_ts,
        admission_ts,
        official_hold_nano: 1_000,
        charged_hold_nano: 500,
        track_eligible: true,
        retention_eligible: true,
        commission_eligible: true,
        premium_modifiers: LegacyPremiumModifiers::AnthropicV1 {
            speed: SnapshotAnthropicSpeed::Standard,
            inference_geo: SnapshotAnthropicInferenceGeo::Global,
            inference_geo_basis_points: 10_000,
        },
    })
    .unwrap();
    assert!(matches!(
        sqlite_reserve_request_with_policy_snapshot(&c, "strict-key", 60, &track_snapshot).unwrap(),
        PolicyReserveOutcome::Inserted(_)
    ));
    assert_eq!(
        c.query_row(
            "SELECT group_concat(bucket_id || ':' || reserved_nano, ',')
               FROM (
                   SELECT bucket_id,reserved_nano
                     FROM reservation_funding_allocations
                    WHERE request_id='strict-track-request'
                    ORDER BY allocation_order
               )",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "strict-bonus:400,strict-paid:100"
    );
    let usage = UsageEventInput {
        model: "claude-test".into(),
        provider: PROVIDER_ANTHROPIC.into(),
        input_tokens: 1,
        real_nano: 600,
        charge_basis_nano: 600,
        input_nano: 600,
        priced_ts: admission_ts,
        speed: "standard".into(),
        inference_geo: "global".into(),
        ..Default::default()
    };
    assert_eq!(
        sqlite_settle_request(
            &c,
            "strict-track-request",
            "strict-account",
            "strict-key",
            500,
            300,
            Some("strict-provider-ref"),
            Some(&usage),
        )
        .unwrap(),
        Some(700)
    );
    let account = account_get(&c, "strict-account").unwrap().unwrap();
    assert_eq!(
        (
            account.balance_nano,
            account.spent_nano,
            account.reserved_nano
        ),
        (700, 300, 0)
    );
    let buckets: Vec<(String, i64, i64, i64)> = {
        let mut statement = c
            .prepare(
                "SELECT bucket_id,balance_nano,reserved_nano,spent_nano
               FROM funding_buckets
              WHERE bucket_id IN ('strict-bonus','strict-paid') ORDER BY bucket_id",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    assert_eq!(
        buckets,
        vec![
            ("strict-bonus".into(), 100, 0, 300),
            ("strict-paid".into(), 600, 0, 0)
        ]
    );
    assert_eq!(
        c.query_row(
            "SELECT allocation.bucket_id || ':' || allocation.amount_nano
               FROM ledger_funding_allocations allocation
               JOIN ledger ON ledger.id=allocation.ledger_id
              WHERE ledger.request_id='strict-track-request'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "strict-bonus:300"
    );
    let attribution: (String, i64, i64, i64, String) = c
        .query_row(
            "SELECT snapshot_kind,paid_funded_nano,bonus_funded_nano,
                runtime_manifest_generation,runtime_manifest_digest
           FROM ledger WHERE request_id='strict-track-request'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        attribution,
        ("policy_v1".into(), 0, 300, 1, "runtime-manifest".into())
    );
    let snapshot = account_funding_snapshot(&c, "strict-account")
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.account.balance_nano, 700);
    assert_eq!(
        (
            snapshot.funding.account_class,
            snapshot.funding.funding_enforcement,
            snapshot.funding.reconciliation_state,
            snapshot.funding.bucket_count,
        ),
        (
            Some(AccountClass::B2c),
            Some(FundingEnforcement::Strict),
            Some(ReconciliationState::Verified),
            2,
        )
    );
    assert_eq!(
        (
            snapshot.funding.paid_balance_nano,
            snapshot.funding.bonus_balance_nano,
            snapshot.funding.other_balance_nano,
            snapshot.funding.unattributed_balance_nano,
            snapshot.funding.bonus_spent_nano,
            snapshot.funding.unattributed_spent_nano,
        ),
        (600, 100, 0, 0, 300, 0)
    );
    let charge = ledger_recent(&c, "strict-account", 1).unwrap().remove(0);
    assert_eq!(charge.request_id.as_deref(), Some("strict-track-request"));
    assert_eq!(charge.provider.as_deref(), Some(PROVIDER_ANTHROPIC));
    let charge_attribution = charge.attribution.unwrap();
    assert_eq!(
        charge_attribution.snapshot_kind.as_deref(),
        Some("policy_v1")
    );
    assert_eq!(
        (
            charge_attribution.source_policy_digest.as_deref(),
            charge_attribution.admission_catalog_digest.as_deref(),
            charge_attribution.runtime_manifest_digest.as_deref(),
            charge_attribution.bonus_funded_nano,
        ),
        (
            Some("source-policy"),
            Some("catalog-digest"),
            Some("runtime-manifest"),
            Some(300),
        )
    );
    assert_eq!(
        charge.funding_allocations,
        vec![LedgerFundingAllocation {
            bucket_id: "strict-bonus".into(),
            source_type: "welcome_track_bonus".into(),
            source_ref: "welcome".into(),
            bucket_version: 2,
            direction: "debit".into(),
            amount_nano: 300,
            allocation_order: Some(1),
        }]
    );

    assert_eq!(
        account_topup(&c, "strict-account", 200, Some("strict-topup")).unwrap(),
        Some(900)
    );
    assert_eq!(
        account_topup(&c, "strict-account", 200, Some("strict-topup")).unwrap(),
        Some(900)
    );
    let parity: (i64, i64) = c
        .query_row(
            "SELECT account.balance_nano,COALESCE(SUM(bucket.balance_nano),0)
           FROM accounts account
           LEFT JOIN funding_buckets bucket ON bucket.account_id=account.id
          WHERE account.id='strict-account' GROUP BY account.id",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(parity, (900, 900));
    let topup = ledger_recent(&c, "strict-account", 1).unwrap().remove(0);
    assert!(topup.attribution.is_none());
    assert_eq!(topup.request_id, None);
    assert_eq!(topup.funding_allocations.len(), 1);
    assert_eq!(topup.funding_allocations[0].source_ref, "strict-topup");
    assert_eq!(topup.funding_allocations[0].direction, "credit");

    const EXECUTION_GROUP: &str = "328f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";
    let grouped_winner = PolicyAdmissionSnapshot::new(PolicyAdmissionSnapshotInput {
        request_id: "strict-group-winner".into(),
        official_hold_nano: 200,
        charged_hold_nano: 100,
        ..track_snapshot.as_input()
    })
    .unwrap();
    let grouped_loser = PolicyAdmissionSnapshot::new(PolicyAdmissionSnapshotInput {
        request_id: "strict-group-loser".into(),
        official_hold_nano: 200,
        charged_hold_nano: 100,
        ..track_snapshot.as_input()
    })
    .unwrap();
    assert!(matches!(
        sqlite_reserve_request_with_policy_snapshot_for_execution(
            &c,
            "strict-key",
            60,
            &grouped_winner,
            &ExecutionAttempt::grouped(EXECUTION_GROUP, 1).unwrap(),
        )
        .unwrap(),
        PolicyReserveOutcome::Inserted(_)
    ));
    assert!(matches!(
        sqlite_reserve_request_with_policy_snapshot_for_execution(
            &c,
            "strict-key",
            60,
            &grouped_loser,
            &ExecutionAttempt::grouped(EXECUTION_GROUP, 2).unwrap(),
        )
        .unwrap(),
        PolicyReserveOutcome::Inserted(_)
    ));
    let mut winner_usage = usage.clone();
    winner_usage.real_nano = 120;
    winner_usage.input_nano = 120;
    assert_eq!(
        sqlite_settle_request(
            &c,
            "strict-group-winner",
            "strict-account",
            "strict-key",
            100,
            60,
            Some("strict-group:winner"),
            Some(&winner_usage),
        )
        .unwrap(),
        Some(740),
    );
    let mut loser_usage = usage.clone();
    loser_usage.real_nano = 140;
    loser_usage.input_nano = 140;
    assert_eq!(
        sqlite_settle_request(
            &c,
            "strict-group-loser",
            "strict-account",
            "strict-key",
            100,
            70,
            Some("strict-group:loser"),
            Some(&loser_usage),
        )
        .unwrap(),
        Some(840),
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            "strict-group-loser",
            "strict-account",
            "strict-key",
            100,
            70,
            Some("strict-group:loser"),
            Some(&loser_usage),
        )
        .unwrap(),
        Some(840),
    );
    let strict_group_state: (i64, i64, i64, String, i64, i64, i64, i64, i64) = c
        .query_row(
            "SELECT account.balance_nano,account.spent_nano,account.reserved_nano,
                    reservation.state,reservation.actual_nano,
                    allocation.charged_nano,allocation.released_nano,
                    outbox.actual_nano,
                    (SELECT COUNT(*) FROM ledger
                      WHERE request_id='strict-group-loser')
               FROM accounts account
               JOIN billing_reservations reservation ON reservation.account_id=account.id
               JOIN reservation_funding_allocations allocation
                 ON allocation.request_id=reservation.request_id
               JOIN billing_settlement_outbox outbox USING(request_id)
              WHERE account.id='strict-account'
                AND reservation.request_id='strict-group-loser'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        strict_group_state,
        (840, 360, 0, "canceled".into(), 0, 0, 100, 70, 0),
    );
    let loser_official_cost: String = c
        .query_row(
            "SELECT official_cost_json FROM billing_settlement_outbox
              WHERE request_id='strict-group-loser'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let loser_official_cost =
        serde_json::from_str::<serde_json::Value>(&loser_official_cost).unwrap();
    assert_eq!(loser_official_cost["official_nano"], 140);
    assert!(loser_official_cost.get("disposition").is_none());

    let static_snapshot = PolicyAdmissionSnapshot::new(PolicyAdmissionSnapshotInput {
        request_id: "strict-static-request".into(),
        rule_id: "static-model".into(),
        rule_digest: "static-digest".into(),
        rule_scope: PolicyRuleScope::Model {
            provider_id: "anthropic".into(),
            canonical_model_id: "claude-test".into(),
        },
        pricing_mode: PricingMode::Discount,
        discount_bps: Some(0),
        payable_multiplier_bp: 10_000,
        official_hold_nano: 850,
        charged_hold_nano: 850,
        track_eligible: false,
        retention_eligible: false,
        commission_eligible: false,
        ..track_snapshot.as_input()
    })
    .unwrap();
    assert_eq!(
        sqlite_reserve_request_with_policy_snapshot(&c, "strict-key", 60, &static_snapshot)
            .unwrap(),
        PolicyReserveOutcome::NotReserved
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM billing_reservations
              WHERE request_id='strict-static-request'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
}

#[test]
fn service_meter_only_strict_sqlite_lane_meters_without_a_charge_row() {
    use crate::pricing::{
        AccountClass, LegacyPremiumModifiers, PolicyAdmissionSnapshot,
        PolicyAdmissionSnapshotInput, PolicyReserveOutcome, PolicyRuleScope, PricingMode,
        RuleOrigin, SnapshotAnthropicInferenceGeo, SnapshotAnthropicSpeed, SnapshotProvider,
    };

    let c = db();
    account_create(&c, "svc-meter", None, 5_000).unwrap();
    account_create(&c, "svc-b2c", None, 5_000).unwrap();
    c.execute_batch(
        "INSERT INTO pricing_catalog_versions(
             product_id,generation,schema_version,capability_generation,capability_digest,
             content_digest,created_ts
         ) VALUES('main',1,1,1,'capability','catalog-digest',1);
         INSERT INTO pricing_catalog_entries(
             product_id,generation,provider_id,canonical_model_id,enabled
         ) VALUES('main',1,'anthropic','claude-test',1);
         INSERT INTO pricing_catalog_heads(product_id,active_generation,updated_ts)
         VALUES('main',1,1);
         INSERT INTO provider_switch_versions(
             generation,schema_version,capability_generation,capability_digest,
             content_digest,created_ts
         ) VALUES(1,1,1,'capability','switch-digest',1);
         INSERT INTO provider_switch_entries(
             generation,provider_id,scope_type,product_id,segment,catalog_generation,enabled
         ) VALUES
             (1,'anthropic','master','','',NULL,1),
             (1,'anthropic','product','main','',1,1),
             (1,'anthropic','segment','main','b2c',1,1);
         INSERT INTO provider_switch_head(singleton,active_generation,updated_ts)
         VALUES(1,1,1);
         INSERT INTO account_policy_versions(
             account_id,effective_version,policy_id,policy_version,source_policy_digest,
             owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
             switch_generation,content_digest,replacement_locked,created_ts
         ) VALUES
             ('svc-meter',1,'svc-policy',1,'svc-source','service','svc-meter','service',
              'main',1,1,1,'svc-digest',0,1),
             ('svc-b2c',1,'b2c-policy',1,'b2c-source','global_b2c','global','b2c',
              'main',1,1,1,'b2c-digest',0,1);
         INSERT INTO account_policy_rules(
             account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
             canonical_model_id,pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
             track_eligible,retention_eligible,commission_eligible
         ) VALUES
             ('svc-meter',1,'svc-rule','svc-rule-digest','provider','anthropic',NULL,
              'discount','managed',10000,0,0,0,0),
             ('svc-b2c',1,'b2c-rule','b2c-rule-digest','provider','anthropic',NULL,
              'track','managed',NULL,10000,1,1,0);
         INSERT INTO account_policy_bindings(
             account_id,product_id,account_class,active_effective_version,
             policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
         ) VALUES
             ('svc-meter','main','service',1,'strict','strict','verified',1),
             ('svc-b2c','main','b2c',1,'strict','strict','verified',1);",
    )
    .unwrap();
    // The SQLite trigger mirrors the PostgreSQL service binding: a payable-0 managed rule under
    // a customer-class policy cannot be inserted even below the engine writer.
    assert!(c
        .execute(
            "INSERT INTO account_policy_rules(
                 account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                 canonical_model_id,pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
                 track_eligible,retention_eligible,commission_eligible
             ) VALUES(
                 'svc-b2c',1,'b2c-sneak','b2c-sneak-digest','provider','openai',NULL,
                 'discount','managed',10000,0,0,0,0
             )",
            [],
        )
        .is_err());
    for (key, account, digest) in [
        ("svc-key", "svc-meter", "svc-digest"),
        ("b2c-key", "svc-b2c", "b2c-digest"),
    ] {
        key_issue_with_policy_ack(
            &c,
            key,
            account,
            None,
            None,
            None,
            Some(&KeyActivationPolicyAck {
                effective_policy_version: 1,
                policy_digest: digest.into(),
            }),
        )
        .unwrap();
    }
    // A negative balance must not reject the meter-only lane; it never moves.
    c.execute("UPDATE accounts SET balance_nano=-50 WHERE id='svc-meter'", [])
        .unwrap();
    c.execute("UPDATE accounts SET balance_nano=0 WHERE id='svc-b2c'", [])
        .unwrap();

    // Snapshot validation: payable-0 builds only for the service class; a customer class keeps
    // the exact typed rejection, and a non-zero charged hold under a payable-0 rule is invalid.
    let admission_ts = now();
    let snapshot = |request_id: &str, account_class: AccountClass, charged: i64| {
        PolicyAdmissionSnapshot::new(PolicyAdmissionSnapshotInput {
            request_id: request_id.into(),
            account_id: "svc-meter".into(),
            provider: SnapshotProvider::Anthropic,
            product_id: "main".into(),
            account_class,
            requested_model_id: "claude-test".into(),
            canonical_model_id: "claude-test".into(),
            alias_generation: 1,
            rule_id: "svc-rule".into(),
            rule_digest: "svc-rule-digest".into(),
            rule_scope: PolicyRuleScope::Provider {
                provider_id: "anthropic".into(),
            },
            pricing_mode: PricingMode::Discount,
            rule_origin: RuleOrigin::Managed,
            discount_bps: Some(10_000),
            payable_multiplier_bp: 0,
            policy_id: "svc-policy".into(),
            policy_version: 1,
            effective_policy_version: 1,
            source_policy_digest: "svc-source".into(),
            policy_digest: "svc-digest".into(),
            policy_catalog_generation: 1,
            policy_switch_generation: 1,
            admission_catalog_generation: 1,
            admission_catalog_digest: "catalog-digest".into(),
            admission_switch_generation: 1,
            admission_switch_digest: "switch-digest".into(),
            runtime_manifest_generation: 1,
            runtime_manifest_digest: "runtime-manifest".into(),
            tariff_schedule_id: "anthropic/claude-test/v1".into(),
            tariff_priced_ts: admission_ts,
            admission_ts,
            official_hold_nano: 100,
            charged_hold_nano: charged,
            track_eligible: false,
            retention_eligible: false,
            commission_eligible: false,
            premium_modifiers: LegacyPremiumModifiers::AnthropicV1 {
                speed: SnapshotAnthropicSpeed::Standard,
                inference_geo: SnapshotAnthropicInferenceGeo::Global,
                inference_geo_basis_points: 10_000,
            },
        })
    };
    assert!(snapshot("svc-rejected-b2c", AccountClass::B2c, 0).is_err());
    assert!(snapshot("svc-rejected-b2b", AccountClass::B2b, 0).is_err());
    assert!(snapshot("svc-rejected-openkeys", AccountClass::OpenKeys, 0).is_err());
    assert!(snapshot("svc-rejected-sneak", AccountClass::Service, 1).is_err());
    let meter_snapshot = snapshot("svc-request", AccountClass::Service, 0).unwrap();
    assert!(meter_snapshot.is_service_meter_only());

    // Reserve: zero hold admitted at a negative balance, no funding allocation, no money moved.
    assert!(matches!(
        sqlite_reserve_request_with_policy_snapshot(&c, "svc-key", 60, &meter_snapshot).unwrap(),
        PolicyReserveOutcome::Inserted(_)
    ));
    let reserve_state: (i64, i64, i64, i64) = c
        .query_row(
            "SELECT
                 (SELECT hold_nano FROM billing_reservations WHERE request_id='svc-request'),
                 (SELECT balance_nano FROM accounts WHERE id='svc-meter'),
                 (SELECT reserved_nano FROM accounts WHERE id='svc-meter'),
                 (SELECT COUNT(*) FROM reservation_funding_allocations
                   WHERE request_id='svc-request')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(reserve_state, (0, -50, 0, 0));

    // Settlement: full official usage metered, charge exactly zero, NO ledger charge row, the
    // usage event carries the immutable service/payable-0 attribution.
    let usage = UsageEventInput {
        model: "claude-test".into(),
        provider: PROVIDER_ANTHROPIC.into(),
        input_tokens: 1,
        real_nano: 40,
        charge_basis_nano: 40,
        input_nano: 40,
        priced_ts: admission_ts,
        speed: "standard".into(),
        inference_geo: "global".into(),
        ..Default::default()
    };
    assert_eq!(
        sqlite_settle_request(
            &c,
            "svc-request",
            "svc-meter",
            "svc-key",
            0,
            0,
            Some("svc-settle"),
            Some(&usage),
        )
        .unwrap(),
        Some(-50)
    );
    let settled_state: (i64, i64, i64, i64, i64, i64, String) = c
        .query_row(
            "SELECT
                 (SELECT balance_nano FROM accounts WHERE id='svc-meter'),
                 (SELECT spent_nano FROM accounts WHERE id='svc-meter'),
                 (SELECT COUNT(*) FROM ledger
                   WHERE account_id='svc-meter' AND kind='charge'),
                 (SELECT COUNT(*) FROM usage_events WHERE request_id='svc-request'),
                 (SELECT charge_nano FROM usage_events WHERE request_id='svc-request'),
                 (SELECT discount_bps FROM usage_events WHERE request_id='svc-request'),
                 (SELECT account_class FROM usage_events WHERE request_id='svc-request')",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        settled_state,
        (0 - 50, 0, 0, 1, 0, 10_000, "service".to_string())
    );

    // A payable-0 rule cannot sneak a positive charge: settlement recomputes from the pinned
    // multiplier and rejects before any money mutation.
    let sneak_snapshot = snapshot("svc-sneak-request", AccountClass::Service, 0).unwrap();
    assert!(matches!(
        sqlite_reserve_request_with_policy_snapshot(&c, "svc-key", 60, &sneak_snapshot).unwrap(),
        PolicyReserveOutcome::Inserted(_)
    ));
    assert!(sqlite_settle_request(
        &c,
        "svc-sneak-request",
        "svc-meter",
        "svc-key",
        0,
        50,
        Some("svc-sneak"),
        Some(&usage),
    )
    .is_err());
    let sneak_charges: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM ledger WHERE account_id='svc-meter' AND kind='charge'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(sneak_charges, 0);

    // Customer classes are byte-identical: a zero-balance b2c strict reserve is still NotReserved.
    let b2c_snapshot = PolicyAdmissionSnapshot::new(PolicyAdmissionSnapshotInput {
        request_id: "svc-b2c-request".into(),
        account_id: "svc-b2c".into(),
        provider: SnapshotProvider::Anthropic,
        product_id: "main".into(),
        account_class: AccountClass::B2c,
        requested_model_id: "claude-test".into(),
        canonical_model_id: "claude-test".into(),
        alias_generation: 1,
        rule_id: "b2c-rule".into(),
        rule_digest: "b2c-rule-digest".into(),
        rule_scope: PolicyRuleScope::Provider {
            provider_id: "anthropic".into(),
        },
        pricing_mode: PricingMode::Track,
        rule_origin: RuleOrigin::Managed,
        discount_bps: None,
        payable_multiplier_bp: 10_000,
        policy_id: "b2c-policy".into(),
        policy_version: 1,
        effective_policy_version: 1,
        source_policy_digest: "b2c-source".into(),
        policy_digest: "b2c-digest".into(),
        policy_catalog_generation: 1,
        policy_switch_generation: 1,
        admission_catalog_generation: 1,
        admission_catalog_digest: "catalog-digest".into(),
        admission_switch_generation: 1,
        admission_switch_digest: "switch-digest".into(),
        runtime_manifest_generation: 1,
        runtime_manifest_digest: "runtime-manifest".into(),
        tariff_schedule_id: "anthropic/claude-test/v1".into(),
        tariff_priced_ts: admission_ts,
        admission_ts,
        official_hold_nano: 100,
        charged_hold_nano: 100,
        track_eligible: true,
        retention_eligible: true,
        commission_eligible: false,
        premium_modifiers: LegacyPremiumModifiers::AnthropicV1 {
            speed: SnapshotAnthropicSpeed::Standard,
            inference_geo: SnapshotAnthropicInferenceGeo::Global,
            inference_geo_basis_points: 10_000,
        },
    })
    .unwrap();
    assert!(matches!(
        sqlite_reserve_request_with_policy_snapshot(&c, "b2c-key", 60, &b2c_snapshot).unwrap(),
        PolicyReserveOutcome::NotReserved
    ));
}
