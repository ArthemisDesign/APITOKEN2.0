//! # registry — реестр подписок (пункт 1)
//!
//! Источник истины пула: engine-owned PostgreSQL. SQLite remains a migration/rollback source. Для форвардинг-прокси подписке нужны
//! только OAuth-токен + прокси (+ статус/флот). Токен берётся из колонки `token` (inline)
//! либо из файла `token_file`. Совместим с исторической subscriptions.db (мягкая миграция).
//!
//! **Границы крейта:** только хранение/чтение подписок. НИКАКОЙ HTTP/логики пула.
//! Ниже по стеку зависеть не от кого.

pub mod authority;
pub mod funding;
pub mod pg;
pub mod pricing;
pub mod stage8;

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use std::fs;

/// Рантайм-запись подписки с УЖЕ разрешённым токеном (inline или из файла).
#[derive(Clone, Debug)]
pub struct Sub {
    pub email: String,
    pub token: String, // OAuth Bearer подписки (секрет)
    pub proxy: String, // http://user:pass@ip:port ("" = без прокси)
    pub fleet: String,
    pub plan: String, // pro|max5|max20 (детект) — для per-sub прайора ёмкости в pool ("" = неизвестно)
}

const COLS: &[(&str, &str)] = &[
    ("email", "TEXT PRIMARY KEY"),
    ("token", "TEXT"),
    ("token_file", "TEXT"),
    ("proxy", "TEXT"),
    ("plan", "TEXT"),
    ("status", "TEXT"),
    ("fleet", "TEXT"),
    ("added_ts", "INTEGER"),
    ("added", "TEXT"),
    // Метаданные прокси (заполняет authbot — владелец жизненного цикла; движок лишь читает/показывает):
    ("proxy_expire", "TEXT"), // дата истечения прокси из IPRoyal (ISO), "" = неизвестно
    ("proxy_checked_ts", "INTEGER"), // ts последней health-проверки прокси (fingerprint-free)
    ("proxy_ok", "INTEGER"),  // 1=жив / 0=мёртв на последней проверке (NULL=не проверялся)
    // Durable auth-health (движок пишет из коррелированных probe; переживает рестарт). Зеркало
    // engine PostgreSQL migration 0003. Токен-fingerprint даёт авто-ревайв при замене токена.
    ("auth_state", "TEXT"), // 'healthy' | 'suspect' | 'dead'
    ("auth_fail_streak", "INTEGER"),
    ("first_auth_fail_ts", "INTEGER"),
    ("last_auth_fail_ts", "INTEGER"),
    ("last_auth_http", "INTEGER"),
    ("dead_since_ts", "INTEGER"),
    ("dead_reason", "TEXT"),
    ("auth_token_fp", "TEXT"),
];

const PRICING_POLICY_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS pricing_catalog_versions (
    product_id TEXT NOT NULL CHECK (product_id <> ''),
    generation INTEGER NOT NULL CHECK (generation > 0),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    capability_generation INTEGER NOT NULL CHECK (capability_generation > 0),
    capability_digest TEXT NOT NULL CHECK (capability_digest <> ''),
    content_digest TEXT NOT NULL CHECK (content_digest <> ''),
    created_ts INTEGER NOT NULL,
    PRIMARY KEY (product_id, generation),
    UNIQUE (
        product_id,
        generation,
        schema_version,
        capability_generation,
        capability_digest,
        content_digest
    )
);
CREATE TABLE IF NOT EXISTS pricing_catalog_entries (
    product_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    provider_id TEXT NOT NULL CHECK (provider_id <> ''),
    canonical_model_id TEXT NOT NULL CHECK (canonical_model_id <> ''),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    PRIMARY KEY (product_id, generation, provider_id, canonical_model_id),
    FOREIGN KEY (product_id, generation)
        REFERENCES pricing_catalog_versions(product_id, generation) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS pricing_catalog_entries_enabled
    ON pricing_catalog_entries(product_id, generation, provider_id)
    WHERE enabled = 1;
CREATE TABLE IF NOT EXISTS pricing_catalog_heads (
    product_id TEXT PRIMARY KEY CHECK (product_id <> ''),
    active_generation INTEGER NOT NULL CHECK (active_generation > 0),
    updated_ts INTEGER NOT NULL,
    FOREIGN KEY (product_id, active_generation)
        REFERENCES pricing_catalog_versions(product_id, generation) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS provider_switch_versions (
    generation INTEGER PRIMARY KEY CHECK (generation > 0),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    capability_generation INTEGER NOT NULL CHECK (capability_generation > 0),
    capability_digest TEXT NOT NULL CHECK (capability_digest <> ''),
    content_digest TEXT NOT NULL CHECK (content_digest <> ''),
    created_ts INTEGER NOT NULL,
    UNIQUE (
        generation,
        schema_version,
        capability_generation,
        capability_digest,
        content_digest
    )
);
CREATE TABLE IF NOT EXISTS provider_switch_entries (
    generation INTEGER NOT NULL REFERENCES provider_switch_versions(generation) ON DELETE CASCADE,
    provider_id TEXT NOT NULL CHECK (provider_id <> ''),
    scope_type TEXT NOT NULL CHECK (scope_type IN ('master', 'product', 'segment')),
    product_id TEXT NOT NULL DEFAULT '',
    segment TEXT NOT NULL DEFAULT '',
    catalog_generation INTEGER,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    PRIMARY KEY (generation, provider_id, scope_type, product_id, segment),
    FOREIGN KEY (product_id, catalog_generation)
        REFERENCES pricing_catalog_versions(product_id, generation) ON DELETE RESTRICT,
    CHECK (
        (
            scope_type = 'master'
            AND product_id = ''
            AND segment = ''
            AND catalog_generation IS NULL
        )
        OR (
            scope_type = 'product'
            AND product_id <> ''
            AND segment = ''
            AND catalog_generation IS NOT NULL
            AND catalog_generation > 0
        )
        OR (
            scope_type = 'segment'
            AND product_id <> ''
            AND segment IN ('b2c', 'b2b')
            AND catalog_generation IS NOT NULL
            AND catalog_generation > 0
        )
    )
);
CREATE TABLE IF NOT EXISTS provider_switch_head (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    active_generation INTEGER NOT NULL
        REFERENCES provider_switch_versions(generation) ON DELETE RESTRICT,
    updated_ts INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS account_policy_versions (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    effective_version INTEGER NOT NULL CHECK (effective_version > 0),
    policy_id TEXT NOT NULL CHECK (policy_id <> ''),
    policy_version INTEGER NOT NULL CHECK (policy_version > 0),
    source_policy_digest TEXT NOT NULL CHECK (source_policy_digest <> ''),
    owner_type TEXT NOT NULL
        CHECK (owner_type IN ('global_b2c', 'b2b_client', 'openkeys', 'service')),
    owner_id TEXT NOT NULL CHECK (owner_id <> ''),
    account_class TEXT NOT NULL CHECK (account_class IN ('b2c', 'b2b', 'openkeys', 'service')),
    product_id TEXT NOT NULL CHECK (product_id <> ''),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    catalog_generation INTEGER NOT NULL CHECK (catalog_generation > 0),
    switch_generation INTEGER NOT NULL CHECK (switch_generation > 0),
    content_digest TEXT NOT NULL CHECK (content_digest <> ''),
    replacement_locked INTEGER NOT NULL CHECK (replacement_locked IN (0, 1)),
    created_ts INTEGER NOT NULL,
    PRIMARY KEY (account_id, effective_version),
    UNIQUE (account_id, effective_version, product_id),
    UNIQUE (account_id, effective_version, product_id, account_class),
    UNIQUE (
        account_id,
        effective_version,
        policy_id,
        policy_version,
        product_id,
        catalog_generation,
        content_digest
    ),
    UNIQUE (
        account_id,
        effective_version,
        policy_id,
        policy_version,
        source_policy_digest,
        owner_type,
        owner_id,
        product_id,
        account_class,
        catalog_generation,
        switch_generation,
        schema_version,
        content_digest
    ),
    FOREIGN KEY (product_id, catalog_generation)
        REFERENCES pricing_catalog_versions(product_id, generation) ON DELETE RESTRICT,
    FOREIGN KEY (switch_generation)
        REFERENCES provider_switch_versions(generation) ON DELETE RESTRICT,
    CHECK (
        (owner_type = 'global_b2c' AND account_class = 'b2c')
        OR (owner_type = 'b2b_client' AND account_class = 'b2b')
        OR (owner_type = 'openkeys' AND account_class = 'openkeys')
        OR (owner_type = 'service' AND account_class = 'service')
    )
);
CREATE INDEX IF NOT EXISTS account_policy_versions_policy
    ON account_policy_versions(policy_id, policy_version);
CREATE TABLE IF NOT EXISTS account_policy_rules (
    account_id TEXT NOT NULL,
    effective_version INTEGER NOT NULL,
    rule_id TEXT NOT NULL CHECK (rule_id <> ''),
    rule_digest TEXT NOT NULL CHECK (rule_digest <> ''),
    scope_type TEXT NOT NULL CHECK (scope_type IN ('provider', 'model')),
    provider_id TEXT NOT NULL CHECK (provider_id <> ''),
    canonical_model_id TEXT,
    pricing_mode TEXT NOT NULL CHECK (pricing_mode IN ('track', 'discount')),
    rule_origin TEXT NOT NULL CHECK (rule_origin IN ('managed', 'legacy')),
    discount_bps INTEGER,
    payable_multiplier_bp INTEGER NOT NULL CHECK (payable_multiplier_bp BETWEEN 0 AND 10000),
    track_eligible INTEGER NOT NULL CHECK (track_eligible IN (0, 1)),
    retention_eligible INTEGER NOT NULL CHECK (retention_eligible IN (0, 1)),
    commission_eligible INTEGER NOT NULL CHECK (commission_eligible IN (0, 1)),
    PRIMARY KEY (account_id, effective_version, rule_id),
    UNIQUE (account_id, effective_version, rule_id, rule_digest),
    FOREIGN KEY (account_id, effective_version)
        REFERENCES account_policy_versions(account_id, effective_version) ON DELETE CASCADE,
    CHECK (
        (scope_type = 'provider' AND canonical_model_id IS NULL)
        OR (
            scope_type = 'model'
            AND canonical_model_id IS NOT NULL
            AND canonical_model_id <> ''
        )
    ),
    CHECK (
        (
            pricing_mode = 'track'
            AND rule_origin = 'managed'
            AND discount_bps IS NULL
        )
        OR (
            pricing_mode = 'discount'
            AND rule_origin = 'managed'
            AND discount_bps IS NOT NULL
            AND discount_bps BETWEEN 0 AND 9500
            AND discount_bps % 100 = 0
            AND payable_multiplier_bp = 10000 - discount_bps
        )
        OR (
            pricing_mode = 'discount'
            AND rule_origin = 'legacy'
            AND discount_bps IS NULL
            AND payable_multiplier_bp BETWEEN 1 AND 10000
        )
    ),
    CHECK (
        (
            pricing_mode = 'track'
            AND track_eligible = 1
            AND retention_eligible = 1
        )
        OR (
            pricing_mode = 'discount'
            AND track_eligible = 0
            AND retention_eligible = 0
            AND commission_eligible = 0
        )
    ),
    CHECK (commission_eligible = 0 OR pricing_mode = 'track')
);
CREATE UNIQUE INDEX IF NOT EXISTS account_policy_rules_provider_scope
    ON account_policy_rules(account_id, effective_version, provider_id)
    WHERE scope_type = 'provider';
CREATE UNIQUE INDEX IF NOT EXISTS account_policy_rules_model_scope
    ON account_policy_rules(account_id, effective_version, provider_id, canonical_model_id)
    WHERE scope_type = 'model';
CREATE TABLE IF NOT EXISTS account_policy_bindings (
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    product_id TEXT NOT NULL CHECK (product_id <> ''),
    account_class TEXT NOT NULL CHECK (account_class IN ('b2c', 'b2b', 'openkeys', 'service')),
    active_effective_version INTEGER,
    policy_enforcement TEXT NOT NULL
        CHECK (policy_enforcement IN ('legacy_scalar', 'shadow', 'strict')),
    funding_enforcement TEXT NOT NULL
        CHECK (funding_enforcement IN ('legacy_single', 'shadow', 'strict')),
    reconciliation_state TEXT NOT NULL
        CHECK (reconciliation_state IN ('pending', 'verified', 'exception')),
    updated_ts INTEGER NOT NULL,
    FOREIGN KEY (account_id, active_effective_version, product_id)
        REFERENCES account_policy_versions(account_id, effective_version, product_id)
        ON DELETE RESTRICT,
    FOREIGN KEY (account_id, active_effective_version, product_id, account_class)
        REFERENCES account_policy_versions(
            account_id,
            effective_version,
            product_id,
            account_class
        )
        ON DELETE RESTRICT,
    CHECK (policy_enforcement <> 'strict' OR active_effective_version IS NOT NULL),
    CHECK (funding_enforcement <> 'strict' OR reconciliation_state = 'verified')
);
CREATE INDEX IF NOT EXISTS account_policy_bindings_enforcement
    ON account_policy_bindings(policy_enforcement, funding_enforcement, reconciliation_state);

CREATE TABLE IF NOT EXISTS funding_buckets (
    bucket_id TEXT PRIMARY KEY CHECK (bucket_id <> ''),
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    source_type TEXT NOT NULL CHECK (source_type <> ''),
    source_ref TEXT NOT NULL DEFAULT '',
    eligibility TEXT NOT NULL CHECK (eligibility IN ('any', 'track', 'none')),
    balance_nano INTEGER NOT NULL,
    reserved_nano INTEGER NOT NULL CHECK (reserved_nano >= 0),
    spent_nano INTEGER NOT NULL CHECK (spent_nano >= 0),
    version INTEGER NOT NULL CHECK (version > 0),
    status TEXT NOT NULL CHECK (status IN ('active', 'exhausted', 'retired')),
    created_ts INTEGER NOT NULL,
    updated_ts INTEGER NOT NULL,
    UNIQUE (account_id, source_type, source_ref),
    UNIQUE (bucket_id, account_id),
    UNIQUE (bucket_id, account_id, source_type),
    CHECK (source_type = 'paid' OR balance_nano >= 0),
    CHECK (source_type <> 'paid' OR eligibility = 'any'),
    CHECK (source_type <> 'welcome_track_bonus' OR eligibility = 'track'),
    CHECK (source_type <> 'legacy_restricted' OR eligibility = 'none')
);
CREATE INDEX IF NOT EXISTS funding_buckets_account_status
    ON funding_buckets(account_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS funding_buckets_one_welcome
    ON funding_buckets(account_id)
    WHERE source_type = 'welcome_track_bonus';

CREATE TABLE IF NOT EXISTS pricing_admission_snapshots (
    request_id TEXT PRIMARY KEY REFERENCES billing_reservations(request_id) ON DELETE CASCADE,
    account_id TEXT NOT NULL,
    snapshot_kind TEXT NOT NULL CHECK (snapshot_kind IN ('policy_v1', 'legacy_scalar')),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    provider_id TEXT NOT NULL CHECK (provider_id <> ''),
    product_id TEXT,
    account_class TEXT CHECK (account_class IN ('b2c', 'b2b', 'openkeys', 'service')),
    requested_model_id TEXT NOT NULL CHECK (requested_model_id <> ''),
    canonical_model_id TEXT NOT NULL CHECK (canonical_model_id <> ''),
    alias_generation INTEGER NOT NULL CHECK (alias_generation > 0),
    rule_id TEXT,
    rule_digest TEXT,
    rule_scope TEXT CHECK (rule_scope IN ('provider', 'model')),
    pricing_mode TEXT NOT NULL CHECK (pricing_mode IN ('track', 'discount', 'legacy_scalar')),
    rule_origin TEXT NOT NULL CHECK (rule_origin IN ('managed', 'legacy')),
    discount_bps INTEGER,
    payable_multiplier_bp INTEGER NOT NULL CHECK (payable_multiplier_bp BETWEEN 0 AND 10000),
    policy_id TEXT,
    policy_version INTEGER CHECK (policy_version > 0),
    effective_policy_version INTEGER CHECK (effective_policy_version > 0),
    policy_digest TEXT,
    catalog_generation INTEGER CHECK (catalog_generation > 0),
    switch_generation INTEGER CHECK (switch_generation > 0),
    tariff_schedule_id TEXT NOT NULL CHECK (tariff_schedule_id <> ''),
    tariff_priced_ts INTEGER NOT NULL CHECK (tariff_priced_ts > 0),
    admission_ts INTEGER NOT NULL CHECK (admission_ts > 0),
    official_hold_nano INTEGER NOT NULL CHECK (official_hold_nano >= 0),
    charged_hold_nano INTEGER NOT NULL CHECK (charged_hold_nano >= 0),
    track_eligible INTEGER CHECK (track_eligible IN (0, 1)),
    retention_eligible INTEGER CHECK (retention_eligible IN (0, 1)),
    commission_eligible INTEGER CHECK (commission_eligible IN (0, 1)),
    premium_modifiers TEXT NOT NULL
        CHECK (json_valid(premium_modifiers) AND json_type(premium_modifiers) = 'object'),
    snapshot_digest TEXT NOT NULL CHECK (snapshot_digest <> ''),
    UNIQUE (request_id, account_id),
    FOREIGN KEY (
        account_id,
        effective_policy_version,
        policy_id,
        policy_version,
        product_id,
        catalog_generation,
        policy_digest
    )
        REFERENCES account_policy_versions(
            account_id,
            effective_version,
            policy_id,
            policy_version,
            product_id,
            catalog_generation,
            content_digest
        ) ON DELETE RESTRICT,
    FOREIGN KEY (account_id, effective_policy_version, rule_id, rule_digest)
        REFERENCES account_policy_rules(
            account_id,
            effective_version,
            rule_id,
            rule_digest
        ) ON DELETE RESTRICT,
    FOREIGN KEY (product_id, catalog_generation, provider_id, canonical_model_id)
        REFERENCES pricing_catalog_entries(
            product_id,
            generation,
            provider_id,
            canonical_model_id
        ) ON DELETE RESTRICT,
    FOREIGN KEY (switch_generation)
        REFERENCES provider_switch_versions(generation) ON DELETE RESTRICT,
    CHECK (
        (
            snapshot_kind = 'policy_v1'
            AND product_id IS NOT NULL
            AND product_id <> ''
            AND account_class IS NOT NULL
            AND rule_id IS NOT NULL
            AND rule_id <> ''
            AND rule_digest IS NOT NULL
            AND rule_digest <> ''
            AND rule_scope IS NOT NULL
            AND policy_id IS NOT NULL
            AND policy_id <> ''
            AND policy_version IS NOT NULL
            AND effective_policy_version IS NOT NULL
            AND policy_digest IS NOT NULL
            AND policy_digest <> ''
            AND catalog_generation IS NOT NULL
            AND switch_generation IS NOT NULL
            AND track_eligible IS NOT NULL
            AND retention_eligible IS NOT NULL
            AND commission_eligible IS NOT NULL
            AND (
                (
                    pricing_mode = 'track'
                    AND rule_origin = 'managed'
                    AND discount_bps IS NULL
                    AND track_eligible = 1
                    AND retention_eligible = 1
                )
                OR (
                    pricing_mode = 'discount'
                    AND rule_origin = 'managed'
                    AND discount_bps IS NOT NULL
                    AND discount_bps BETWEEN 0 AND 9500
                    AND discount_bps % 100 = 0
                    AND payable_multiplier_bp = 10000 - discount_bps
                    AND track_eligible = 0
                    AND retention_eligible = 0
                    AND commission_eligible = 0
                )
                OR (
                    pricing_mode = 'discount'
                    AND rule_origin = 'legacy'
                    AND discount_bps IS NULL
                    AND payable_multiplier_bp BETWEEN 1 AND 10000
                    AND track_eligible = 0
                    AND retention_eligible = 0
                    AND commission_eligible = 0
                )
            )
        )
        OR (
            snapshot_kind = 'legacy_scalar'
            AND product_id IS NULL
            AND account_class IS NULL
            AND rule_id IS NULL
            AND rule_digest IS NULL
            AND rule_scope IS NULL
            AND pricing_mode = 'legacy_scalar'
            AND rule_origin = 'legacy'
            AND discount_bps IS NULL
            AND policy_id IS NULL
            AND policy_version IS NULL
            AND effective_policy_version IS NULL
            AND policy_digest IS NULL
            AND catalog_generation IS NULL
            AND switch_generation IS NULL
            AND track_eligible IS NULL
            AND retention_eligible IS NULL
            AND commission_eligible IS NULL
        )
    ),
    CHECK (commission_eligible IS NOT 1 OR pricing_mode = 'track')
);
CREATE INDEX IF NOT EXISTS pricing_admission_snapshots_account
    ON pricing_admission_snapshots(account_id, admission_ts);
CREATE TRIGGER IF NOT EXISTS pricing_snapshot_reservation_account
BEFORE INSERT ON pricing_admission_snapshots
FOR EACH ROW
WHEN NOT EXISTS (
    SELECT 1
    FROM billing_reservations
    WHERE request_id = NEW.request_id
      AND account_id = NEW.account_id
)
BEGIN
    SELECT RAISE(ABORT, 'pricing snapshot account does not match reservation');
END;
CREATE TRIGGER IF NOT EXISTS pricing_snapshot_immutable_update
BEFORE UPDATE ON pricing_admission_snapshots
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'pricing admission snapshots are immutable');
END;

CREATE UNIQUE INDEX IF NOT EXISTS pricing_admission_snapshots_shadow_actual_identity
    ON pricing_admission_snapshots(
        request_id,account_id,snapshot_kind,provider_id,requested_model_id,canonical_model_id,
        alias_generation,payable_multiplier_bp,official_hold_nano,charged_hold_nano,snapshot_digest
    );
CREATE TABLE IF NOT EXISTS pricing_shadow_admission_evaluations (
    request_id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    actual_snapshot_kind TEXT NOT NULL CHECK (actual_snapshot_kind = 'legacy_scalar'),
    actual_snapshot_digest TEXT NOT NULL CHECK (actual_snapshot_digest <> ''),
    provider_id TEXT NOT NULL CHECK (provider_id <> ''),
    requested_model_id TEXT NOT NULL CHECK (requested_model_id <> ''),
    canonical_model_id TEXT NOT NULL CHECK (canonical_model_id <> ''),
    alias_generation INTEGER NOT NULL CHECK (alias_generation > 0),
    evaluator_schema_version INTEGER NOT NULL CHECK (evaluator_schema_version > 0),
    runtime_manifest_generation INTEGER NOT NULL CHECK (runtime_manifest_generation > 0),
    runtime_manifest_digest TEXT NOT NULL CHECK (runtime_manifest_digest <> ''),
    enqueued_ts INTEGER NOT NULL CHECK (enqueued_ts > 0),
    evaluated_ts INTEGER NOT NULL CHECK (evaluated_ts > 0),
    outcome TEXT NOT NULL CHECK (outcome IN ('resolved', 'rejected', 'read_error')),
    reason_code TEXT,
    authorized_multiplier_bp INTEGER NOT NULL
        CHECK (authorized_multiplier_bp BETWEEN 0 AND 10000),
    observed_multiplier_bp INTEGER CHECK (observed_multiplier_bp BETWEEN 0 AND 10000),
    official_hold_nano INTEGER NOT NULL CHECK (official_hold_nano >= 0),
    legacy_hold_nano INTEGER NOT NULL CHECK (legacy_hold_nano >= 0),
    product_id TEXT,
    account_class TEXT CHECK (account_class IN ('b2c', 'b2b', 'openkeys', 'service')),
    effective_policy_version INTEGER CHECK (effective_policy_version > 0),
    policy_id TEXT,
    policy_version INTEGER CHECK (policy_version > 0),
    source_policy_digest TEXT,
    policy_digest TEXT,
    policy_schema_version INTEGER CHECK (policy_schema_version > 0),
    policy_catalog_generation INTEGER CHECK (policy_catalog_generation > 0),
    policy_catalog_schema_version INTEGER CHECK (policy_catalog_schema_version > 0),
    policy_catalog_capability_generation INTEGER
        CHECK (policy_catalog_capability_generation > 0),
    policy_catalog_capability_digest TEXT,
    policy_catalog_digest TEXT,
    policy_switch_generation INTEGER CHECK (policy_switch_generation > 0),
    policy_switch_schema_version INTEGER CHECK (policy_switch_schema_version > 0),
    policy_switch_capability_generation INTEGER
        CHECK (policy_switch_capability_generation > 0),
    policy_switch_capability_digest TEXT,
    policy_switch_digest TEXT,
    admission_catalog_generation INTEGER CHECK (admission_catalog_generation > 0),
    admission_catalog_schema_version INTEGER CHECK (admission_catalog_schema_version > 0),
    admission_catalog_capability_generation INTEGER
        CHECK (admission_catalog_capability_generation > 0),
    admission_catalog_capability_digest TEXT,
    admission_catalog_digest TEXT,
    admission_switch_generation INTEGER CHECK (admission_switch_generation > 0),
    admission_switch_schema_version INTEGER CHECK (admission_switch_schema_version > 0),
    admission_switch_capability_generation INTEGER
        CHECK (admission_switch_capability_generation > 0),
    admission_switch_capability_digest TEXT,
    admission_switch_digest TEXT,
    rule_id TEXT,
    rule_digest TEXT,
    rule_scope TEXT CHECK (rule_scope IN ('provider', 'model')),
    pricing_mode TEXT CHECK (pricing_mode IN ('track', 'discount')),
    rule_origin TEXT CHECK (rule_origin IN ('managed', 'legacy')),
    discount_bps INTEGER,
    payable_multiplier_bp INTEGER CHECK (payable_multiplier_bp BETWEEN 0 AND 10000),
    track_eligible INTEGER CHECK (track_eligible IN (0, 1)),
    retention_eligible INTEGER CHECK (retention_eligible IN (0, 1)),
    commission_eligible INTEGER CHECK (commission_eligible IN (0, 1)),
    policy_hold_nano INTEGER CHECK (policy_hold_nano >= 0),
    comparison_result TEXT NOT NULL
        CHECK (comparison_result IN ('equal', 'different', 'not_comparable')),
    -- Best-effort, non-authoritative diagnostics. Immutable identity belongs in typed columns.
    diagnostic_context TEXT NOT NULL
        CHECK (json_valid(diagnostic_context) AND json_type(diagnostic_context) = 'object'),
    evaluation_digest TEXT NOT NULL CHECK (evaluation_digest <> ''),
    UNIQUE (request_id, account_id),
    FOREIGN KEY (
        request_id,account_id,actual_snapshot_kind,provider_id,requested_model_id,
        canonical_model_id,alias_generation,authorized_multiplier_bp,official_hold_nano,
        legacy_hold_nano,actual_snapshot_digest
    ) REFERENCES pricing_admission_snapshots(
        request_id,account_id,snapshot_kind,provider_id,requested_model_id,canonical_model_id,
        alias_generation,payable_multiplier_bp,official_hold_nano,charged_hold_nano,snapshot_digest
    ) ON DELETE CASCADE,
    FOREIGN KEY (
        account_id,
        effective_policy_version,
        policy_id,
        policy_version,
        source_policy_digest,
        product_id,
        account_class,
        policy_schema_version,
        policy_catalog_generation,
        policy_switch_generation,
        policy_digest
    ) REFERENCES account_policy_versions(
        account_id,
        effective_version,
        policy_id,
        policy_version,
        source_policy_digest,
        product_id,
        account_class,
        schema_version,
        catalog_generation,
        switch_generation,
        content_digest
    ) ON DELETE RESTRICT,
    FOREIGN KEY (account_id, effective_policy_version, rule_id, rule_digest)
        REFERENCES account_policy_rules(account_id, effective_version, rule_id, rule_digest)
        ON DELETE RESTRICT,
    FOREIGN KEY (
        product_id,
        policy_catalog_generation,
        provider_id,
        canonical_model_id
    ) REFERENCES pricing_catalog_entries(
        product_id,
        generation,
        provider_id,
        canonical_model_id
    ) ON DELETE RESTRICT,
    FOREIGN KEY (
        product_id,
        policy_catalog_generation,
        policy_catalog_schema_version,
        policy_catalog_capability_generation,
        policy_catalog_capability_digest,
        policy_catalog_digest
    ) REFERENCES pricing_catalog_versions(
        product_id,
        generation,
        schema_version,
        capability_generation,
        capability_digest,
        content_digest
    ) ON DELETE RESTRICT,
    FOREIGN KEY (
        policy_switch_generation,
        policy_switch_schema_version,
        policy_switch_capability_generation,
        policy_switch_capability_digest,
        policy_switch_digest
    ) REFERENCES provider_switch_versions(
        generation,
        schema_version,
        capability_generation,
        capability_digest,
        content_digest
    ) ON DELETE RESTRICT,
    FOREIGN KEY (
        product_id,
        admission_catalog_generation,
        provider_id,
        canonical_model_id
    ) REFERENCES pricing_catalog_entries(
        product_id,
        generation,
        provider_id,
        canonical_model_id
    ) ON DELETE RESTRICT,
    FOREIGN KEY (
        product_id,
        admission_catalog_generation,
        admission_catalog_schema_version,
        admission_catalog_capability_generation,
        admission_catalog_capability_digest,
        admission_catalog_digest
    ) REFERENCES pricing_catalog_versions(
        product_id,
        generation,
        schema_version,
        capability_generation,
        capability_digest,
        content_digest
    ) ON DELETE RESTRICT,
    FOREIGN KEY (
        admission_switch_generation,
        admission_switch_schema_version,
        admission_switch_capability_generation,
        admission_switch_capability_digest,
        admission_switch_digest
    ) REFERENCES provider_switch_versions(
        generation,
        schema_version,
        capability_generation,
        capability_digest,
        content_digest
    ) ON DELETE RESTRICT,
    CHECK (evaluated_ts >= enqueued_ts),
    CHECK (
        (
            outcome = 'resolved'
            AND reason_code IS NULL
            AND observed_multiplier_bp IS NOT NULL
            AND product_id IS NOT NULL AND product_id <> ''
            AND account_class IS NOT NULL
            AND effective_policy_version IS NOT NULL
            AND policy_id IS NOT NULL AND policy_id <> ''
            AND policy_version IS NOT NULL
            AND source_policy_digest IS NOT NULL AND source_policy_digest <> ''
            AND policy_digest IS NOT NULL AND policy_digest <> ''
            AND policy_schema_version IS NOT NULL
            AND policy_schema_version = evaluator_schema_version
            AND policy_catalog_generation IS NOT NULL
            AND policy_catalog_schema_version IS NOT NULL
            AND policy_catalog_schema_version = policy_schema_version
            AND policy_catalog_capability_generation IS NOT NULL
            AND policy_catalog_capability_digest IS NOT NULL
            AND policy_catalog_capability_digest <> ''
            AND policy_catalog_digest IS NOT NULL AND policy_catalog_digest <> ''
            AND policy_switch_generation IS NOT NULL
            AND policy_switch_schema_version IS NOT NULL
            AND policy_switch_schema_version = policy_schema_version
            AND policy_switch_capability_generation IS NOT NULL
            AND policy_switch_capability_generation = policy_catalog_capability_generation
            AND policy_switch_capability_digest IS NOT NULL
            AND policy_switch_capability_digest <> ''
            AND policy_switch_capability_digest = policy_catalog_capability_digest
            AND policy_switch_digest IS NOT NULL AND policy_switch_digest <> ''
            AND admission_catalog_generation IS NOT NULL
            AND admission_catalog_schema_version IS NOT NULL
            AND admission_catalog_schema_version = evaluator_schema_version
            AND admission_catalog_capability_generation IS NOT NULL
            AND admission_catalog_capability_digest IS NOT NULL
            AND admission_catalog_capability_digest <> ''
            AND admission_catalog_digest IS NOT NULL AND admission_catalog_digest <> ''
            AND admission_switch_generation IS NOT NULL
            AND admission_switch_schema_version IS NOT NULL
            AND admission_switch_schema_version = evaluator_schema_version
            AND admission_switch_capability_generation IS NOT NULL
            AND admission_switch_capability_digest IS NOT NULL
            AND admission_switch_capability_digest <> ''
            AND admission_switch_digest IS NOT NULL AND admission_switch_digest <> ''
            AND rule_id IS NOT NULL AND rule_id <> ''
            AND rule_digest IS NOT NULL AND rule_digest <> ''
            AND rule_scope IS NOT NULL
            AND pricing_mode IS NOT NULL
            AND rule_origin IS NOT NULL
            AND payable_multiplier_bp IS NOT NULL
            AND track_eligible IS NOT NULL
            AND retention_eligible IS NOT NULL
            AND commission_eligible IS NOT NULL
            AND policy_hold_nano IS NOT NULL
            AND (
                (comparison_result = 'equal' AND policy_hold_nano = legacy_hold_nano)
                OR (comparison_result = 'different' AND policy_hold_nano <> legacy_hold_nano)
            )
            AND (
                (
                    pricing_mode = 'track'
                    AND rule_origin = 'managed'
                    AND discount_bps IS NULL
                    AND track_eligible = 1
                    AND retention_eligible = 1
                )
                OR (
                    pricing_mode = 'discount'
                    AND rule_origin = 'managed'
                    AND discount_bps IS NOT NULL
                    AND discount_bps BETWEEN 0 AND 9500
                    AND discount_bps % 100 = 0
                    AND payable_multiplier_bp = 10000 - discount_bps
                    AND track_eligible = 0
                    AND retention_eligible = 0
                    AND commission_eligible = 0
                )
                OR (
                    pricing_mode = 'discount'
                    AND rule_origin = 'legacy'
                    AND discount_bps IS NULL
                    AND payable_multiplier_bp BETWEEN 1 AND 10000
                    AND track_eligible = 0
                    AND retention_eligible = 0
                    AND commission_eligible = 0
                )
            )
        )
        OR (
            outcome IN ('rejected', 'read_error')
            AND reason_code IS NOT NULL AND reason_code <> ''
            AND (
                (outcome = 'rejected' AND observed_multiplier_bp IS NOT NULL)
                OR (outcome = 'read_error' AND observed_multiplier_bp IS NULL)
            )
            AND product_id IS NULL
            AND account_class IS NULL
            AND effective_policy_version IS NULL
            AND policy_id IS NULL
            AND policy_version IS NULL
            AND source_policy_digest IS NULL
            AND policy_digest IS NULL
            AND policy_schema_version IS NULL
            AND policy_catalog_generation IS NULL
            AND policy_catalog_schema_version IS NULL
            AND policy_catalog_capability_generation IS NULL
            AND policy_catalog_capability_digest IS NULL
            AND policy_catalog_digest IS NULL
            AND policy_switch_generation IS NULL
            AND policy_switch_schema_version IS NULL
            AND policy_switch_capability_generation IS NULL
            AND policy_switch_capability_digest IS NULL
            AND policy_switch_digest IS NULL
            AND admission_catalog_generation IS NULL
            AND admission_catalog_schema_version IS NULL
            AND admission_catalog_capability_generation IS NULL
            AND admission_catalog_capability_digest IS NULL
            AND admission_catalog_digest IS NULL
            AND admission_switch_generation IS NULL
            AND admission_switch_schema_version IS NULL
            AND admission_switch_capability_generation IS NULL
            AND admission_switch_capability_digest IS NULL
            AND admission_switch_digest IS NULL
            AND rule_id IS NULL
            AND rule_digest IS NULL
            AND rule_scope IS NULL
            AND pricing_mode IS NULL
            AND rule_origin IS NULL
            AND discount_bps IS NULL
            AND payable_multiplier_bp IS NULL
            AND track_eligible IS NULL
            AND retention_eligible IS NULL
            AND commission_eligible IS NULL
            AND policy_hold_nano IS NULL
            AND comparison_result = 'not_comparable'
        )
    ),
    CHECK (commission_eligible IS NOT 1 OR pricing_mode = 'track')
);
CREATE INDEX IF NOT EXISTS pricing_shadow_admission_evaluations_time
    ON pricing_shadow_admission_evaluations(evaluated_ts, outcome);
CREATE INDEX IF NOT EXISTS pricing_shadow_admission_evaluations_account
    ON pricing_shadow_admission_evaluations(account_id, evaluated_ts);
CREATE TRIGGER IF NOT EXISTS pricing_shadow_admission_evaluations_rule_identity
BEFORE INSERT ON pricing_shadow_admission_evaluations
FOR EACH ROW
WHEN NEW.outcome = 'resolved' AND NOT EXISTS (
    SELECT 1
    FROM account_policy_rules AS rule
    WHERE rule.account_id = NEW.account_id
      AND rule.effective_version = NEW.effective_policy_version
      AND rule.rule_id = NEW.rule_id
      AND rule.rule_digest = NEW.rule_digest
      AND rule.scope_type = NEW.rule_scope
      AND rule.provider_id = NEW.provider_id
      AND rule.canonical_model_id IS
          CASE WHEN NEW.rule_scope = 'model' THEN NEW.canonical_model_id ELSE NULL END
      AND rule.pricing_mode = NEW.pricing_mode
      AND rule.rule_origin = NEW.rule_origin
      AND rule.discount_bps IS NEW.discount_bps
      AND rule.payable_multiplier_bp = NEW.payable_multiplier_bp
      AND rule.track_eligible = NEW.track_eligible
      AND rule.retention_eligible = NEW.retention_eligible
      AND rule.commission_eligible = NEW.commission_eligible
)
BEGIN
    SELECT RAISE(ABORT, 'pricing shadow admission rule identity does not match immutable policy rule');
END;
CREATE TRIGGER IF NOT EXISTS pricing_shadow_admission_evaluations_immutable_update
BEFORE UPDATE ON pricing_shadow_admission_evaluations
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'pricing shadow admission evaluations are immutable');
END;
CREATE TABLE IF NOT EXISTS reservation_funding_allocations (
    request_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    bucket_id TEXT NOT NULL,
    bucket_version INTEGER NOT NULL CHECK (bucket_version > 0),
    reserved_nano INTEGER NOT NULL CHECK (reserved_nano >= 0),
    charged_nano INTEGER CHECK (charged_nano IS NULL OR charged_nano >= 0),
    released_nano INTEGER CHECK (released_nano IS NULL OR released_nano >= 0),
    allocation_order INTEGER CHECK (allocation_order IS NULL OR allocation_order > 0),
    PRIMARY KEY (request_id, bucket_id),
    FOREIGN KEY (request_id, account_id)
        REFERENCES pricing_admission_snapshots(request_id, account_id) ON DELETE CASCADE,
    FOREIGN KEY (bucket_id, account_id)
        REFERENCES funding_buckets(bucket_id, account_id) ON DELETE RESTRICT,
    CHECK (released_nano IS NULL OR released_nano <= reserved_nano)
);
CREATE INDEX IF NOT EXISTS reservation_funding_allocations_bucket
    ON reservation_funding_allocations(bucket_id, request_id);
CREATE TABLE IF NOT EXISTS ledger_funding_allocations (
    ledger_id INTEGER NOT NULL REFERENCES ledger(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    bucket_id TEXT NOT NULL,
    bucket_source_type TEXT NOT NULL CHECK (bucket_source_type <> ''),
    bucket_version INTEGER NOT NULL CHECK (bucket_version > 0),
    direction TEXT NOT NULL CHECK (direction IN ('debit', 'credit')),
    amount_nano INTEGER NOT NULL CHECK (amount_nano >= 0),
    PRIMARY KEY (ledger_id, bucket_id),
    FOREIGN KEY (bucket_id, account_id, bucket_source_type)
        REFERENCES funding_buckets(bucket_id, account_id, source_type) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS ledger_funding_allocations_bucket
    ON ledger_funding_allocations(bucket_id, ledger_id);
CREATE TRIGGER IF NOT EXISTS ledger_funding_allocation_account
BEFORE INSERT ON ledger_funding_allocations
FOR EACH ROW
WHEN NOT EXISTS (
    SELECT 1 FROM ledger
    WHERE id = NEW.ledger_id
      AND account_id = NEW.account_id
)
BEGIN
    SELECT RAISE(ABORT, 'funding allocation account does not match ledger');
END;
CREATE TRIGGER IF NOT EXISTS ledger_funding_allocation_account_update
BEFORE UPDATE ON ledger_funding_allocations
FOR EACH ROW
WHEN NOT EXISTS (
    SELECT 1 FROM ledger
    WHERE id = NEW.ledger_id
      AND account_id = NEW.account_id
)
BEGIN
    SELECT RAISE(ABORT, 'funding allocation account does not match ledger');
END;
"#;

const SQLITE_ATTRIBUTION_COLUMNS: &[(&str, &str)] = &[
    ("attribution_schema_version", "INTEGER"),
    ("snapshot_kind", "TEXT"),
    ("product_id", "TEXT"),
    ("account_class", "TEXT"),
    ("requested_model_id", "TEXT"),
    ("canonical_model_id", "TEXT"),
    ("served_model_id", "TEXT"),
    ("served_canonical_model_id", "TEXT"),
    ("billing_invariant_code", "TEXT"),
    ("alias_generation", "INTEGER"),
    ("rule_id", "TEXT"),
    ("rule_digest", "TEXT"),
    ("rule_scope", "TEXT"),
    ("pricing_mode", "TEXT"),
    ("rule_origin", "TEXT"),
    ("discount_bps", "INTEGER"),
    ("payable_multiplier_bp", "INTEGER"),
    ("policy_id", "TEXT"),
    ("policy_version", "INTEGER"),
    ("effective_policy_version", "INTEGER"),
    ("policy_digest", "TEXT"),
    ("catalog_generation", "INTEGER"),
    ("switch_generation", "INTEGER"),
    ("tariff_schedule_id", "TEXT"),
    ("tariff_priced_ts", "INTEGER"),
    ("official_cost_json", "TEXT"),
    ("paid_funded_nano", "INTEGER"),
    ("bonus_funded_nano", "INTEGER"),
    ("other_funded_nano", "INTEGER"),
    ("funding_allocation_json", "TEXT"),
    ("track_eligible", "INTEGER"),
    ("retention_eligible", "INTEGER"),
    ("commission_eligible", "INTEGER"),
    ("snapshot_digest", "TEXT"),
    ("source_policy_digest", "TEXT"),
    ("admission_catalog_generation", "INTEGER"),
    ("admission_catalog_digest", "TEXT"),
    ("admission_switch_generation", "INTEGER"),
    ("admission_switch_digest", "TEXT"),
    ("runtime_manifest_generation", "INTEGER"),
    ("runtime_manifest_digest", "TEXT"),
];

pub fn open(path: &str) -> Result<Connection> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            let _ = fs::create_dir_all(dir);
        }
    }
    let c = Connection::open(path).with_context(|| format!("открыть БД {path}"))?;
    // AUDIT(C38): this database is authoritative for balances and ledger entries. In WAL mode,
    // synchronous=FULL makes an acknowledged commit durable across OS crashes and power loss.
    // Performance-sensitive nonfinancial state should move to a separate database if needed.
    c.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; \
                     PRAGMA busy_timeout=5000; PRAGMA wal_autocheckpoint=1000;",
    )?;
    c.execute(
        "CREATE TABLE IF NOT EXISTS subs(email TEXT PRIMARY KEY, token TEXT, token_file TEXT, \
         proxy TEXT, plan TEXT DEFAULT '', status TEXT DEFAULT 'active', fleet TEXT DEFAULT 'prod', \
         added_ts INTEGER, added TEXT)",
        [],
    )?;
    // мягкая миграция: доливаем недостающие колонки в существующую (историческую) таблицу
    for (name, ty) in COLS {
        let _ = c.execute(&format!("ALTER TABLE subs ADD COLUMN {name} {ty}"), []);
    }
    // Биллинг: ключи клиентов с балансом в нанодолларах (1 USD = 1e9 нано). i64 INTEGER
    // вмещает до ~$9.2 млрд — с запасом. balance может уйти в минус (тогда ключ блокируется).
    c.execute(
        "CREATE TABLE IF NOT EXISTS api_keys(key TEXT PRIMARY KEY, balance_nano INTEGER NOT NULL DEFAULT 0, \
         spent_nano INTEGER NOT NULL DEFAULT 0, mult_bp INTEGER NOT NULL DEFAULT 900, \
         status TEXT NOT NULL DEFAULT 'active', created_ts INTEGER, created TEXT, \
         reserved_nano INTEGER NOT NULL DEFAULT 0)",
        [],
    )?;
    // Мягкая миграция: колонка учёта незакрытых резервов (леджер крах-безопасности).
    let _ = c.execute(
        "ALTER TABLE api_keys ADD COLUMN reserved_nano INTEGER NOT NULL DEFAULT 0",
        [],
    );

    // АККАУНТЫ клиентов: ЕДИНЫЙ баланс на профиль; ключи (api_keys) — доступы к нему (1:N).
    // Баланс/резерв/наценка живут ЗДЕСЬ, не на ключе. Ключ теперь несёт account_id + label +
    // per-key spent (атрибуция расхода по ключу без разделения баланса).
    c.execute(
        "CREATE TABLE IF NOT EXISTS accounts(id TEXT PRIMARY KEY, handle TEXT, \
         balance_nano INTEGER NOT NULL DEFAULT 0, spent_nano INTEGER NOT NULL DEFAULT 0, \
         reserved_nano INTEGER NOT NULL DEFAULT 0, mult_bp INTEGER NOT NULL DEFAULT 2000, \
         status TEXT NOT NULL DEFAULT 'active', created_ts INTEGER, created TEXT)",
        [],
    )?;
    // handle (внешняя идентичность: TG id / email) уникален, когда задан.
    let _ = c.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS accounts_handle ON accounts(handle) WHERE handle IS NOT NULL", []);
    let _ = c.execute("ALTER TABLE api_keys ADD COLUMN account_id TEXT", []);
    let _ = c.execute("ALTER TABLE api_keys ADD COLUMN label TEXT", []);
    // Stable public identifier for control-plane key management. The usable `key` remains secret;
    // dashboards and the commercial backend can revoke by `key_id` without persisting that secret.
    let _ = c.execute("ALTER TABLE api_keys ADD COLUMN key_id TEXT", []);
    let _ = c.execute(
        "ALTER TABLE api_keys ADD COLUMN spend_limit_nano INTEGER",
        [],
    );
    let _ = c.execute("ALTER TABLE api_keys ADD COLUMN expires_ts INTEGER", []);
    let _ = c.execute(
        "UPDATE api_keys SET key_id = 'key_' || lower(hex(randomblob(16))) WHERE key_id IS NULL",
        [],
    );
    let _ = c.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS api_keys_key_id ON api_keys(key_id) WHERE key_id IS NOT NULL", []);
    let _ = c.execute(
        "CREATE INDEX IF NOT EXISTS api_keys_account ON api_keys(account_id)",
        [],
    );

    // ЛЕДЖЕР: append-only история движений баланса (пополнения/списания/возвраты) — для точного
    // учёта, споров и дашбордов. Текущий баланс = accounts.balance_nano; ledger — журнал КАК он менялся.
    c.execute(
        "CREATE TABLE IF NOT EXISTS ledger(id INTEGER PRIMARY KEY AUTOINCREMENT, account_id TEXT NOT NULL, \
         key TEXT, kind TEXT NOT NULL, amount_nano INTEGER NOT NULL, ref TEXT, \
         balance_after_nano INTEGER, ts INTEGER, model TEXT)",
        [],
    )?;
    // Атрибуция charge-строк к Claude-модели (для точного per-model дневного графика). Модель известна
    // в момент settle (тот же запрос, что и usage_event). topup/adjust модели не имеют → NULL. Идемпотентно.
    let _ = c.execute("ALTER TABLE ledger ADD COLUMN model TEXT", []);
    let _ = c.execute(
        "CREATE INDEX IF NOT EXISTS ledger_acct ON ledger(account_id, id)",
        [],
    );
    // AUDIT(C2): correctness-critical idempotency indexes must fail closed. A legacy database with
    // duplicate references must not open for billing traffic without explicit operator repair.
    c.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS ledger_topup_ref ON ledger(ref) \
         WHERE kind='topup' AND ref IS NOT NULL",
        [],
    )
    .context("create required unique top-up reference index")?;
    // AUDIT(C40): negative adjustments are retryable monetary mutations too, so their supplied
    // references share the same global idempotency namespace as top-ups.
    c.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS ledger_money_ref ON ledger(ref) \
         WHERE kind IN ('topup','adjust') AND ref IS NOT NULL",
        [],
    )
    .context("create required unique monetary reference index")?;
    c.execute(
        "CREATE TABLE IF NOT EXISTS ledger_consumer_checkpoints(consumer TEXT NOT NULL, \
         account_id TEXT NOT NULL, last_ledger_id INTEGER NOT NULL, updated_ts INTEGER NOT NULL, \
         PRIMARY KEY(consumer,account_id))",
        [],
    )?;
    // AUDIT-TODO(C2): move schema upgrades into versioned transactions with explicit duplicate repair.
    // Retained for the future checkpoint-aware pruning path; current timestamp-only prune is disabled.
    let _ = c.execute(
        "CREATE INDEX IF NOT EXISTS ledger_charge_ts ON ledger(ts) WHERE kind='charge'",
        [],
    );

    // Миграция старой модели (key=кошелёк): ключам без account_id заводим аккаунт и переносим баланс.
    migrate_legacy_keys(&c)?;
    // Персист волатильного состояния пула (переживание рестарта): cooling (бан на дни не должен
    // забываться при деплое) + калибровка ёмкости (дорого переучивать) + spent/util/reset.
    c.execute(
        "CREATE TABLE IF NOT EXISTS pool_state(email TEXT PRIMARY KEY, cooling_until INTEGER, \
         cap5h REAL, cap7d REAL, spent_total REAL, util5 REAL, util7 REAL, \
         reset5 INTEGER, reset7 INTEGER, calib_n INTEGER, updated_ts INTEGER)",
        [],
    )?;
    // OpenAI/Codex calibration is based exclusively on durable, real gateway spend paired with
    // provider-reported window duration/reset snapshots. These tables intentionally contain no
    // configured capacity prior or fixed 5-hour/7-day slots.
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS codex_home_spend( \
           home_id TEXT PRIMARY KEY, \
           spent_nano INTEGER NOT NULL DEFAULT 0 CHECK(spent_nano >= 0), \
           spent_nanocredits INTEGER CHECK(spent_nanocredits IS NULL OR spent_nanocredits >= 0), \
           credit_tracking_started_ts INTEGER \
             CHECK(credit_tracking_started_ts IS NULL OR credit_tracking_started_ts > 0), \
           updated_ts INTEGER NOT NULL); \
         CREATE TABLE IF NOT EXISTS codex_home_health( \
           home_id TEXT PRIMARY KEY, \
           account_state TEXT NOT NULL DEFAULT 'healthy' \
             CHECK(account_state IN ('healthy','suspect','dead')), \
           auth_fail_streak INTEGER NOT NULL DEFAULT 0 CHECK(auth_fail_streak >= 0), \
           first_auth_fail_ts INTEGER NOT NULL DEFAULT 0 CHECK(first_auth_fail_ts >= 0), \
           cooling_until INTEGER NOT NULL DEFAULT 0 CHECK(cooling_until >= 0), \
           updated_ts INTEGER NOT NULL); \
         CREATE TABLE IF NOT EXISTS codex_window_calibrations( \
           home_id TEXT NOT NULL, \
           window_duration_mins INTEGER NOT NULL CHECK(window_duration_mins > 0), \
           resets_at INTEGER NOT NULL CHECK(resets_at > 0), \
           anchor_used_percent INTEGER NOT NULL CHECK(anchor_used_percent BETWEEN 0 AND 100), \
           anchor_spend_nano INTEGER NOT NULL CHECK(anchor_spend_nano >= 0), \
           used_percent INTEGER NOT NULL CHECK(used_percent BETWEEN 0 AND 100), \
           observed_at INTEGER NOT NULL CHECK(observed_at > 0), \
           sum_used_sq INTEGER NOT NULL DEFAULT 0 CHECK(sum_used_sq >= 0), \
           sum_used_spend_nano INTEGER NOT NULL DEFAULT 0 CHECK(sum_used_spend_nano >= 0), \
           observed_points INTEGER NOT NULL DEFAULT 0 CHECK(observed_points >= 0), \
           samples INTEGER NOT NULL DEFAULT 0 CHECK(samples >= 0), \
           current_capacity_nano INTEGER CHECK(current_capacity_nano IS NULL OR current_capacity_nano >= 0), \
           current_low_nano INTEGER CHECK(current_low_nano IS NULL OR current_low_nano >= 0), \
           current_high_nano INTEGER CHECK(current_high_nano IS NULL OR current_high_nano >= 0), \
           current_confidence_bp INTEGER NOT NULL DEFAULT 0 CHECK(current_confidence_bp BETWEEN 0 AND 10000), \
           last_capacity_nano INTEGER CHECK(last_capacity_nano IS NULL OR last_capacity_nano >= 0), \
           last_low_nano INTEGER CHECK(last_low_nano IS NULL OR last_low_nano >= 0), \
           last_high_nano INTEGER CHECK(last_high_nano IS NULL OR last_high_nano >= 0), \
           last_confidence_bp INTEGER NOT NULL DEFAULT 0 CHECK(last_confidence_bp BETWEEN 0 AND 10000), \
           last_measured_at INTEGER CHECK(last_measured_at IS NULL OR last_measured_at > 0), \
           anchor_ready INTEGER NOT NULL DEFAULT 0 CHECK(anchor_ready IN (0,1)), \
           anchor_used_fraction_units INTEGER CHECK(anchor_used_fraction_units BETWEEN 0 AND 100000000), \
           used_fraction_units INTEGER CHECK(used_fraction_units BETWEEN 0 AND 100000000), \
           observed_fraction_units INTEGER CHECK(observed_fraction_units >= 0), \
           observed_spend_nano INTEGER CHECK(observed_spend_nano >= 0), \
           anchor_spend_nanocredits INTEGER \
             CHECK(anchor_spend_nanocredits IS NULL OR anchor_spend_nanocredits >= 0), \
           observed_spend_nanocredits INTEGER \
             CHECK(observed_spend_nanocredits IS NULL OR observed_spend_nanocredits >= 0), \
           current_capacity_nanocredits INTEGER \
             CHECK(current_capacity_nanocredits IS NULL OR current_capacity_nanocredits >= 0), \
           current_low_nanocredits INTEGER \
             CHECK(current_low_nanocredits IS NULL OR current_low_nanocredits >= 0), \
           current_high_nanocredits INTEGER \
             CHECK(current_high_nanocredits IS NULL OR current_high_nanocredits >= 0), \
           last_capacity_nanocredits INTEGER \
             CHECK(last_capacity_nanocredits IS NULL OR last_capacity_nanocredits >= 0), \
           last_low_nanocredits INTEGER \
             CHECK(last_low_nanocredits IS NULL OR last_low_nanocredits >= 0), \
           last_high_nanocredits INTEGER \
             CHECK(last_high_nanocredits IS NULL OR last_high_nanocredits >= 0), \
           credit_samples INTEGER CHECK(credit_samples IS NULL OR credit_samples >= 0), \
           credit_estimator_version INTEGER \
             CHECK(credit_estimator_version IS NULL OR credit_estimator_version > 0), \
           unattributed_fraction_units INTEGER \
             CHECK(unattributed_fraction_units IS NULL OR unattributed_fraction_units >= 0), \
           estimator_version INTEGER NOT NULL DEFAULT 1 CHECK(estimator_version > 0), \
           version INTEGER NOT NULL DEFAULT 0 CHECK(version >= 0), \
           updated_ts INTEGER NOT NULL, \
           PRIMARY KEY(home_id,window_duration_mins), \
           CHECK(current_low_nano IS NULL OR current_capacity_nano IS NOT NULL), \
           CHECK(current_high_nano IS NULL OR current_capacity_nano IS NOT NULL), \
           CHECK(last_low_nano IS NULL OR last_capacity_nano IS NOT NULL), \
           CHECK(last_high_nano IS NULL OR last_capacity_nano IS NOT NULL), \
           CHECK((current_low_nanocredits IS NULL AND current_high_nanocredits IS NULL) \
             OR current_capacity_nanocredits IS NOT NULL), \
           CHECK((last_low_nanocredits IS NULL AND last_high_nanocredits IS NULL) \
             OR last_capacity_nanocredits IS NOT NULL)); \
         CREATE TABLE IF NOT EXISTS codex_window_observations( \
           id INTEGER PRIMARY KEY AUTOINCREMENT, \
           home_id TEXT NOT NULL, \
           window_duration_mins INTEGER NOT NULL CHECK(window_duration_mins > 0), \
           resets_at INTEGER NOT NULL CHECK(resets_at > 0), \
           observed_at INTEGER NOT NULL CHECK(observed_at > 0), \
           used_percent INTEGER NOT NULL CHECK(used_percent BETWEEN 0 AND 100), \
           used_fraction_units INTEGER CHECK(used_fraction_units BETWEEN 0 AND 100000000), \
           gateway_spend_nano INTEGER NOT NULL CHECK(gateway_spend_nano >= 0), \
           gateway_spend_nanocredits INTEGER \
             CHECK(gateway_spend_nanocredits IS NULL OR gateway_spend_nanocredits >= 0), \
           UNIQUE(home_id,window_duration_mins,resets_at,observed_at,used_percent,gateway_spend_nano)); \
         CREATE INDEX IF NOT EXISTS codex_window_observations_window \
           ON codex_window_observations(home_id,window_duration_mins,resets_at,observed_at); \
         CREATE TABLE IF NOT EXISTS codex_turn_calibration_events( \
           request_id TEXT PRIMARY KEY, \
           home_id TEXT NOT NULL CHECK(home_id <> ''), \
           model_id TEXT NOT NULL CHECK(model_id <> ''), \
           service_tier TEXT NOT NULL CHECK(service_tier IN ('standard','fast')), \
           provider_reported_tier TEXT, \
           api_tariff_schedule_id TEXT NOT NULL CHECK(api_tariff_schedule_id <> ''), \
           credit_schedule_id TEXT NOT NULL CHECK(credit_schedule_id <> ''), \
           completed_at INTEGER NOT NULL CHECK(completed_at > 0), \
           input_tokens INTEGER NOT NULL CHECK(input_tokens >= 0), \
           cached_input_tokens INTEGER NOT NULL CHECK(cached_input_tokens >= 0), \
           cache_write_input_tokens INTEGER NOT NULL CHECK(cache_write_input_tokens >= 0), \
           output_tokens INTEGER NOT NULL CHECK(output_tokens >= 0), \
           reasoning_output_tokens INTEGER NOT NULL CHECK(reasoning_output_tokens >= 0), \
           api_input_nanousd INTEGER NOT NULL CHECK(api_input_nanousd >= 0), \
           api_cached_input_nanousd INTEGER NOT NULL CHECK(api_cached_input_nanousd >= 0), \
           api_cache_write_nanousd INTEGER NOT NULL CHECK(api_cache_write_nanousd >= 0), \
           api_output_nanousd INTEGER NOT NULL CHECK(api_output_nanousd >= 0), \
           api_total_nanousd INTEGER NOT NULL CHECK(api_total_nanousd >= 0), \
           chatgpt_input_nanocredits INTEGER NOT NULL CHECK(chatgpt_input_nanocredits >= 0), \
           chatgpt_cached_input_nanocredits INTEGER NOT NULL \
             CHECK(chatgpt_cached_input_nanocredits >= 0), \
           chatgpt_output_nanocredits INTEGER NOT NULL CHECK(chatgpt_output_nanocredits >= 0), \
           chatgpt_total_nanocredits INTEGER NOT NULL CHECK(chatgpt_total_nanocredits >= 0), \
           CHECK(cached_input_tokens + cache_write_input_tokens <= input_tokens), \
           CHECK(reasoning_output_tokens <= output_tokens), \
           CHECK(input_tokens > 0 OR output_tokens > 0), \
           CHECK(api_total_nanousd = api_input_nanousd + api_cached_input_nanousd \
             + api_cache_write_nanousd + api_output_nanousd), \
           CHECK(chatgpt_total_nanocredits = chatgpt_input_nanocredits \
             + chatgpt_cached_input_nanocredits + chatgpt_output_nanocredits)); \
         CREATE INDEX IF NOT EXISTS codex_turn_calibration_events_home_time \
           ON codex_turn_calibration_events(home_id,completed_at DESC); \
         CREATE INDEX IF NOT EXISTS codex_turn_calibration_events_model_time \
           ON codex_turn_calibration_events(model_id,completed_at DESC); \
         CREATE INDEX IF NOT EXISTS codex_turn_calibration_events_time \
           ON codex_turn_calibration_events(completed_at DESC);",
    )?;
    // Expand-only compatibility for SQLite databases created before estimator v3. The ignored
    // duplicate-column error is expected on every later open and on freshly created databases.
    let _ = c.execute(
        "ALTER TABLE codex_window_calibrations ADD COLUMN anchor_ready INTEGER NOT NULL DEFAULT 0 CHECK(anchor_ready IN (0,1))",
        [],
    );
    // SQLite parity for PostgreSQL migration 0015. Nullable columns preserve compatibility with
    // legacy databases and binaries; the v6 estimator reconstructs a missing fixed-point value
    // from the immutable whole-percent projection before writing both representations.
    for statement in [
        "ALTER TABLE codex_window_calibrations ADD COLUMN anchor_used_fraction_units INTEGER CHECK(anchor_used_fraction_units BETWEEN 0 AND 100000000)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN used_fraction_units INTEGER CHECK(used_fraction_units BETWEEN 0 AND 100000000)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN observed_fraction_units INTEGER CHECK(observed_fraction_units >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN observed_spend_nano INTEGER CHECK(observed_spend_nano >= 0)",
        "ALTER TABLE codex_window_observations ADD COLUMN used_fraction_units INTEGER CHECK(used_fraction_units BETWEEN 0 AND 100000000)",
        "ALTER TABLE codex_home_spend ADD COLUMN spent_nanocredits INTEGER CHECK(spent_nanocredits IS NULL OR spent_nanocredits >= 0)",
        "ALTER TABLE codex_home_spend ADD COLUMN credit_tracking_started_ts INTEGER CHECK(credit_tracking_started_ts IS NULL OR credit_tracking_started_ts > 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN anchor_spend_nanocredits INTEGER CHECK(anchor_spend_nanocredits IS NULL OR anchor_spend_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN observed_spend_nanocredits INTEGER CHECK(observed_spend_nanocredits IS NULL OR observed_spend_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN current_capacity_nanocredits INTEGER CHECK(current_capacity_nanocredits IS NULL OR current_capacity_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN current_low_nanocredits INTEGER CHECK(current_low_nanocredits IS NULL OR current_low_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN current_high_nanocredits INTEGER CHECK(current_high_nanocredits IS NULL OR current_high_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN last_capacity_nanocredits INTEGER CHECK(last_capacity_nanocredits IS NULL OR last_capacity_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN last_low_nanocredits INTEGER CHECK(last_low_nanocredits IS NULL OR last_low_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN last_high_nanocredits INTEGER CHECK(last_high_nanocredits IS NULL OR last_high_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN credit_samples INTEGER CHECK(credit_samples IS NULL OR credit_samples >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN credit_estimator_version INTEGER CHECK(credit_estimator_version IS NULL OR credit_estimator_version > 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN unattributed_fraction_units INTEGER CHECK(unattributed_fraction_units IS NULL OR unattributed_fraction_units >= 0)",
        "ALTER TABLE codex_window_observations ADD COLUMN gateway_spend_nanocredits INTEGER CHECK(gateway_spend_nanocredits IS NULL OR gateway_spend_nanocredits >= 0)",
    ] {
        let _ = c.execute(statement, []);
    }
    // Native Gemini calibration uses the two explicit Antigravity quota-summary windows. Keep
    // SQLite schema parity for importer/tests even though PostgreSQL remains production authority.
    // Large WLS accumulators are canonical decimal text because SQLite has no exact i128 integer
    // type; registry validates them before estimator arithmetic.
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS gemini_profile_spend( \
           profile_id TEXT PRIMARY KEY, \
           spent_nano INTEGER NOT NULL DEFAULT 0 CHECK(spent_nano >= 0), \
           updated_ts INTEGER NOT NULL CHECK(updated_ts > 0)); \
         CREATE TABLE IF NOT EXISTS gemini_window_calibrations( \
           profile_id TEXT NOT NULL, \
           bucket_id TEXT NOT NULL, \
           window_kind TEXT NOT NULL CHECK(window_kind IN ('5h','weekly')), \
           window_duration_mins INTEGER NOT NULL CHECK(window_duration_mins > 0), \
           resets_at INTEGER NOT NULL CHECK(resets_at > 0), \
           anchor_used_fraction_units INTEGER NOT NULL \
             CHECK(anchor_used_fraction_units BETWEEN 0 AND 100000000), \
           anchor_spend_nano INTEGER NOT NULL CHECK(anchor_spend_nano >= 0), \
           anchor_ready INTEGER NOT NULL DEFAULT 0 CHECK(anchor_ready IN (0,1)), \
           used_fraction_units INTEGER NOT NULL \
             CHECK(used_fraction_units BETWEEN 0 AND 100000000), \
           observed_at INTEGER NOT NULL CHECK(observed_at > 0), \
           sum_used_sq TEXT NOT NULL DEFAULT '0', \
           sum_used_spend_nano TEXT NOT NULL DEFAULT '0', \
           observed_fraction_units INTEGER NOT NULL DEFAULT 0 \
             CHECK(observed_fraction_units >= 0), \
           observed_spend_nano INTEGER NOT NULL DEFAULT 0 \
             CHECK(observed_spend_nano >= 0), \
           samples INTEGER NOT NULL DEFAULT 0 CHECK(samples >= 0), \
           current_capacity_nano INTEGER \
             CHECK(current_capacity_nano IS NULL OR current_capacity_nano >= 0), \
           current_low_nano INTEGER \
             CHECK(current_low_nano IS NULL OR current_low_nano >= 0), \
           current_high_nano INTEGER \
             CHECK(current_high_nano IS NULL OR current_high_nano >= 0), \
           current_confidence_bp INTEGER NOT NULL DEFAULT 0 \
             CHECK(current_confidence_bp BETWEEN 0 AND 10000), \
           last_measured_at INTEGER CHECK(last_measured_at IS NULL OR last_measured_at > 0), \
           estimator_version INTEGER NOT NULL DEFAULT 1 CHECK(estimator_version > 0), \
           version INTEGER NOT NULL DEFAULT 0 CHECK(version >= 0), \
           updated_ts INTEGER NOT NULL CHECK(updated_ts > 0), \
           PRIMARY KEY(profile_id,bucket_id), \
           CHECK((bucket_id='gemini-5h' AND window_kind='5h' AND window_duration_mins=300) \
             OR (bucket_id='gemini-weekly' AND window_kind='weekly' \
               AND window_duration_mins=10080)), \
           CHECK(current_low_nano IS NULL OR current_capacity_nano IS NOT NULL), \
           CHECK(current_high_nano IS NULL OR current_capacity_nano IS NOT NULL)); \
         CREATE TABLE IF NOT EXISTS gemini_window_observations( \
           id INTEGER PRIMARY KEY AUTOINCREMENT, \
           profile_id TEXT NOT NULL, \
           bucket_id TEXT NOT NULL, \
           window_kind TEXT NOT NULL CHECK(window_kind IN ('5h','weekly')), \
           window_duration_mins INTEGER NOT NULL CHECK(window_duration_mins > 0), \
           resets_at INTEGER NOT NULL CHECK(resets_at > 0), \
           observed_at INTEGER NOT NULL CHECK(observed_at > 0), \
           used_fraction_units INTEGER NOT NULL \
             CHECK(used_fraction_units BETWEEN 0 AND 100000000), \
           gateway_spend_nano INTEGER NOT NULL CHECK(gateway_spend_nano >= 0), \
           CHECK((bucket_id='gemini-5h' AND window_kind='5h' AND window_duration_mins=300) \
             OR (bucket_id='gemini-weekly' AND window_kind='weekly' \
               AND window_duration_mins=10080)), \
           UNIQUE(profile_id,bucket_id,resets_at,observed_at,used_fraction_units,gateway_spend_nano)); \
         CREATE INDEX IF NOT EXISTS gemini_window_observations_window \
           ON gemini_window_observations(profile_id,bucket_id,resets_at,observed_at);",
    )?;
    // Expand-only compatibility for SQLite authorities opened before estimator v2. Production
    // PostgreSQL receives the same column through engine migration 0014.
    let _ = c.execute(
        "ALTER TABLE gemini_window_calibrations ADD COLUMN observed_spend_nano INTEGER NOT NULL DEFAULT 0 CHECK(observed_spend_nano >= 0)",
        [],
    );
    // Разбивка расхода по токенам/моделям для клиентских дашбордов (per-request). НЕ money-БД:
    // авторитет денег — accounts.balance_nano + ledger. Эта таблица — аналитика (что реально
    // потрачено по корзинам токенов и моделям), пишется рядом с charge, обрезается по ретенции.
    c.execute(
        "CREATE TABLE IF NOT EXISTS usage_events(id INTEGER PRIMARY KEY AUTOINCREMENT, \
         account_id TEXT NOT NULL, key TEXT, model TEXT, \
         input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0, \
         cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_write_5m_tokens INTEGER NOT NULL DEFAULT 0, \
         cache_write_1h_tokens INTEGER NOT NULL DEFAULT 0, web_search_requests INTEGER NOT NULL DEFAULT 0, \
         real_nano INTEGER NOT NULL DEFAULT 0, charge_nano INTEGER NOT NULL DEFAULT 0, ref TEXT, ts INTEGER)",
        [],
    )?;
    for (name, ty) in [
        ("speed", "TEXT NOT NULL DEFAULT 'standard'"),
        ("inference_geo", "TEXT NOT NULL DEFAULT ''"),
        ("input_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("output_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("cache_read_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("cache_write_5m_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("cache_write_1h_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("web_search_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("priced_ts", "INTEGER NOT NULL DEFAULT 0"),
        ("provider", "TEXT NOT NULL DEFAULT 'anthropic'"),
    ] {
        let _ = c.execute(
            &format!("ALTER TABLE usage_events ADD COLUMN {name} {ty}"),
            [],
        );
    }
    // Индекс под агрегацию по окну (account_id + время) и под фоновую обрезку по ts.
    let _ = c.execute(
        "CREATE INDEX IF NOT EXISTS usage_events_acct_ts ON usage_events(account_id, ts)",
        [],
    );
    // SQLite money durability mirrors the PostgreSQL request lifecycle: every hold has an exact
    // request identity and lease, while settlement intent is committed to an outbox before the
    // balance mutation. Recovery can therefore distinguish pre-delivery cancellation from a
    // delivered request and can retry the exact settlement after process/database failures.
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS billing_reservations( \
           request_id TEXT PRIMARY KEY, account_id TEXT NOT NULL, key TEXT NOT NULL, \
           hold_nano INTEGER NOT NULL, state TEXT NOT NULL, \
           balance_after_reserve_nano INTEGER NOT NULL, actual_nano INTEGER, \
           balance_after_settle_nano INTEGER, reference TEXT, lease_until INTEGER NOT NULL, \
           created_ts INTEGER NOT NULL, updated_ts INTEGER NOT NULL, settled_ts INTEGER); \
         CREATE INDEX IF NOT EXISTS billing_reservations_lease \
           ON billing_reservations(state,lease_until); \
         CREATE TABLE IF NOT EXISTS billing_settlement_outbox( \
           request_id TEXT PRIMARY KEY, actual_nano INTEGER NOT NULL, reference TEXT, \
           usage_json TEXT, state TEXT NOT NULL DEFAULT 'pending', attempts INTEGER NOT NULL DEFAULT 0, \
           next_attempt_ts INTEGER NOT NULL DEFAULT 0, last_error TEXT, \
           created_ts INTEGER NOT NULL, updated_ts INTEGER NOT NULL, committed_ts INTEGER); \
         CREATE INDEX IF NOT EXISTS billing_outbox_pending \
           ON billing_settlement_outbox(state,next_attempt_ts,created_ts);"
    )?;
    migrate_pricing_policy_schema(&c)?;
    Ok(c)
}

fn ensure_sqlite_column(
    conn: &Connection,
    table: &str,
    name: &str,
    column_type: &str,
) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
        rusqlite::params![table, name],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute_batch(&format!(
            "ALTER TABLE \"{table}\" ADD COLUMN \"{name}\" {column_type}"
        ))
        .with_context(|| format!("add SQLite policy column {table}.{name}"))?;
    }
    Ok(())
}

fn migrate_pricing_policy_schema(conn: &Connection) -> Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .context("begin SQLite multi-discount schema transaction")?;
    install_pricing_policy_schema(&tx)?;
    tx.commit()
        .context("commit SQLite multi-discount schema transaction")
}

fn install_pricing_policy_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(PRICING_POLICY_SCHEMA_SQL)
        .context("install SQLite multi-discount foundation schema")?;

    ensure_sqlite_column(
        conn,
        "pricing_catalog_versions",
        "capability_generation",
        "INTEGER",
    )?;
    ensure_sqlite_column(
        conn,
        "provider_switch_versions",
        "capability_generation",
        "INTEGER",
    )?;
    ensure_sqlite_column(
        conn,
        "provider_switch_versions",
        "capability_digest",
        "TEXT",
    )?;
    ensure_sqlite_column(
        conn,
        "provider_switch_entries",
        "catalog_generation",
        "INTEGER",
    )?;
    ensure_sqlite_column(
        conn,
        "account_policy_versions",
        "switch_generation",
        "INTEGER",
    )?;
    ensure_sqlite_column(
        conn,
        "account_policy_versions",
        "source_policy_digest",
        "TEXT",
    )?;
    ensure_sqlite_column(conn, "account_policy_versions", "account_class", "TEXT")?;
    for (name, column_type) in [
        ("source_policy_digest", "TEXT"),
        ("admission_catalog_generation", "INTEGER"),
        ("admission_catalog_digest", "TEXT"),
        ("admission_switch_generation", "INTEGER"),
        ("admission_switch_digest", "TEXT"),
        ("runtime_manifest_generation", "INTEGER"),
        ("runtime_manifest_digest", "TEXT"),
    ] {
        ensure_sqlite_column(conn, "pricing_admission_snapshots", name, column_type)?;
    }
    ensure_sqlite_column(
        conn,
        "reservation_funding_allocations",
        "allocation_order",
        "INTEGER",
    )?;
    for (name, column_type) in [
        ("activation_policy_effective_version", "INTEGER"),
        ("activation_policy_digest", "TEXT"),
        ("activation_policy_ack_ts", "INTEGER"),
    ] {
        ensure_sqlite_column(conn, "api_keys", name, column_type)?;
    }
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS pricing_catalog_versions_shadow_identity
             ON pricing_catalog_versions(
                 product_id,generation,schema_version,capability_generation,capability_digest,
                 content_digest
             );
         CREATE UNIQUE INDEX IF NOT EXISTS provider_switch_versions_shadow_identity
             ON provider_switch_versions(
                 generation,schema_version,capability_generation,capability_digest,content_digest
             );
         CREATE UNIQUE INDEX IF NOT EXISTS account_policy_versions_shadow_identity
             ON account_policy_versions(
                 account_id,effective_version,policy_id,policy_version,source_policy_digest,
                 product_id,account_class,schema_version,catalog_generation,switch_generation,
                 content_digest
             );
         CREATE UNIQUE INDEX IF NOT EXISTS reservation_funding_allocations_request_order
             ON reservation_funding_allocations(request_id, allocation_order)
             WHERE allocation_order IS NOT NULL;",
    )
    .context("install SQLite shadow attribution identity indexes")?;
    install_sqlite_runtime_pin_guards(conn)?;

    ensure_sqlite_column(conn, "billing_settlement_outbox", "provider", "TEXT")?;
    ensure_sqlite_column(
        conn,
        "billing_settlement_outbox",
        "disposition",
        "TEXT NOT NULL DEFAULT 'settle'",
    )?;
    ensure_sqlite_column(conn, "ledger", "provider", "TEXT")?;
    ensure_sqlite_column(conn, "ledger", "official_nano", "INTEGER")?;
    ensure_sqlite_column(conn, "ledger", "request_id", "TEXT")?;
    ensure_sqlite_column(conn, "usage_events", "request_id", "TEXT")?;
    for table in ["billing_settlement_outbox", "usage_events", "ledger"] {
        for (name, column_type) in SQLITE_ATTRIBUTION_COLUMNS {
            ensure_sqlite_column(conn, table, name, column_type)?;
        }
    }
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS ledger_request_once \
           ON ledger(kind,request_id) WHERE request_id IS NOT NULL; \
         CREATE UNIQUE INDEX IF NOT EXISTS usage_events_request_once \
           ON usage_events(request_id) WHERE request_id IS NOT NULL;",
    )
    .context("install SQLite policy attribution indexes")?;
    install_sqlite_attribution_guards(conn)?;
    Ok(())
}

fn install_sqlite_runtime_pin_guards(conn: &Connection) -> Result<()> {
    let unpinned_rows: i64 = conn.query_row(
        "SELECT
             (SELECT COUNT(*) FROM pricing_catalog_versions
               WHERE capability_generation IS NULL OR capability_generation <= 0)
           + (SELECT COUNT(*) FROM provider_switch_versions
               WHERE capability_generation IS NULL
                  OR capability_generation <= 0
                  OR capability_digest IS NULL
                  OR capability_digest = '')
           + (SELECT COUNT(*) FROM provider_switch_entries
               WHERE (scope_type = 'master' AND catalog_generation IS NOT NULL)
                  OR (scope_type IN ('product', 'segment') AND catalog_generation IS NULL))
           + (SELECT COUNT(*) FROM account_policy_versions
               WHERE switch_generation IS NULL
                  OR switch_generation <= 0
                  OR source_policy_digest IS NULL
                  OR source_policy_digest = ''
                  OR account_class IS NULL
                  OR NOT (
                      (owner_type = 'global_b2c' AND account_class = 'b2c')
                      OR (owner_type = 'b2b_client' AND account_class = 'b2b')
                      OR (owner_type = 'openkeys' AND account_class = 'openkeys')
                      OR (owner_type = 'service' AND account_class = 'service')
                  )
                  OR NOT EXISTS (
                      SELECT 1 FROM provider_switch_versions
                      WHERE generation = account_policy_versions.switch_generation
                  ))
           + (SELECT COUNT(*) FROM account_policy_bindings
               WHERE active_effective_version IS NOT NULL
                 AND NOT EXISTS (
                     SELECT 1 FROM account_policy_versions
                     WHERE account_id = account_policy_bindings.account_id
                       AND effective_version =
                           account_policy_bindings.active_effective_version
                       AND product_id = account_policy_bindings.product_id
                       AND account_class = account_policy_bindings.account_class
                 ))",
        [],
        |row| row.get(0),
    )?;
    if unpinned_rows != 0 {
        anyhow::bail!(
            "SQLite contains pre-writer pricing rows without durable runtime pins; manual audit required"
        );
    }

    const SWITCH_VERSION_INVALID: &str = "
        NEW.capability_generation IS NULL
        OR NEW.capability_generation <= 0
        OR NEW.capability_digest IS NULL
        OR NEW.capability_digest = ''
    ";
    const SWITCH_ENTRY_INVALID: &str = "
        NOT (
            (
                NEW.scope_type = 'master'
                AND NEW.product_id = ''
                AND NEW.segment = ''
                AND NEW.catalog_generation IS NULL
            )
            OR (
                NEW.scope_type = 'product'
                AND NEW.product_id <> ''
                AND NEW.segment = ''
                AND NEW.catalog_generation IS NOT NULL
                AND NEW.catalog_generation > 0
                AND EXISTS (
                    SELECT 1 FROM pricing_catalog_versions
                    WHERE product_id = NEW.product_id
                      AND generation = NEW.catalog_generation
                )
            )
            OR (
                NEW.scope_type = 'segment'
                AND NEW.product_id <> ''
                AND NEW.segment IN ('b2c', 'b2b')
                AND NEW.catalog_generation IS NOT NULL
                AND NEW.catalog_generation > 0
                AND EXISTS (
                    SELECT 1 FROM pricing_catalog_versions
                    WHERE product_id = NEW.product_id
                      AND generation = NEW.catalog_generation
                )
            )
        )
    ";
    const POLICY_VERSION_INVALID: &str = "
        NEW.switch_generation IS NULL
        OR NEW.switch_generation <= 0
        OR NEW.source_policy_digest IS NULL
        OR NEW.source_policy_digest = ''
        OR NEW.account_class IS NULL
        OR NOT (
            (NEW.owner_type = 'global_b2c' AND NEW.account_class = 'b2c')
            OR (NEW.owner_type = 'b2b_client' AND NEW.account_class = 'b2b')
            OR (NEW.owner_type = 'openkeys' AND NEW.account_class = 'openkeys')
            OR (NEW.owner_type = 'service' AND NEW.account_class = 'service')
        )
        OR NOT EXISTS (
            SELECT 1 FROM provider_switch_versions
            WHERE generation = NEW.switch_generation
        )
    ";
    const POLICY_BINDING_INVALID: &str = "
        NEW.active_effective_version IS NOT NULL
        AND NOT EXISTS (
            SELECT 1 FROM account_policy_versions
            WHERE account_id = NEW.account_id
              AND effective_version = NEW.active_effective_version
              AND product_id = NEW.product_id
              AND account_class = NEW.account_class
        )
    ";
    for (table, condition, message) in [
        (
            "provider_switch_versions",
            SWITCH_VERSION_INVALID,
            "invalid provider switch capability pins",
        ),
        (
            "provider_switch_entries",
            SWITCH_ENTRY_INVALID,
            "invalid provider switch catalog pin",
        ),
        (
            "account_policy_versions",
            POLICY_VERSION_INVALID,
            "invalid account policy lineage",
        ),
        (
            "account_policy_bindings",
            POLICY_BINDING_INVALID,
            "account policy binding does not match immutable account class",
        ),
    ] {
        for (suffix, event) in [("insert", "INSERT"), ("update", "UPDATE")] {
            conn.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS {table}_runtime_pins_{suffix}
                 BEFORE {event} ON {table}
                 FOR EACH ROW
                 WHEN {condition}
                 BEGIN
                     SELECT RAISE(ABORT, '{message}');
                 END;"
            ))
            .with_context(|| format!("install SQLite runtime pin guard for {table} {event}"))?;
        }
    }
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS pricing_catalog_versions_capability_pin_insert
         BEFORE INSERT ON pricing_catalog_versions
         FOR EACH ROW
         WHEN NEW.capability_generation IS NULL OR NEW.capability_generation <= 0
         BEGIN
             SELECT RAISE(ABORT, 'invalid pricing catalog capability generation');
         END;
         CREATE TRIGGER IF NOT EXISTS pricing_catalog_versions_capability_pin_update
         BEFORE UPDATE ON pricing_catalog_versions
         FOR EACH ROW
         WHEN NEW.capability_generation IS NULL OR NEW.capability_generation <= 0
         BEGIN
             SELECT RAISE(ABORT, 'invalid pricing catalog capability generation');
         END;",
    )
    .context("install SQLite catalog capability generation guards")?;
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS account_policy_versions_lineage_v1_insert
         BEFORE INSERT ON account_policy_versions
         FOR EACH ROW
         WHEN NEW.source_policy_digest IS NULL
           OR NEW.source_policy_digest = ''
           OR NEW.account_class IS NULL
           OR NOT (
               (NEW.owner_type = 'global_b2c' AND NEW.account_class = 'b2c')
               OR (NEW.owner_type = 'b2b_client' AND NEW.account_class = 'b2b')
               OR (NEW.owner_type = 'openkeys' AND NEW.account_class = 'openkeys')
               OR (NEW.owner_type = 'service' AND NEW.account_class = 'service')
           )
         BEGIN
             SELECT RAISE(ABORT, 'invalid immutable account policy lineage');
         END;
         CREATE TRIGGER IF NOT EXISTS account_policy_versions_lineage_v1_update
         BEFORE UPDATE ON account_policy_versions
         FOR EACH ROW
         WHEN NEW.source_policy_digest IS NULL
           OR NEW.source_policy_digest = ''
           OR NEW.account_class IS NULL
           OR NOT (
               (NEW.owner_type = 'global_b2c' AND NEW.account_class = 'b2c')
               OR (NEW.owner_type = 'b2b_client' AND NEW.account_class = 'b2b')
               OR (NEW.owner_type = 'openkeys' AND NEW.account_class = 'openkeys')
               OR (NEW.owner_type = 'service' AND NEW.account_class = 'service')
           )
         BEGIN
             SELECT RAISE(ABORT, 'invalid immutable account policy lineage');
         END;",
    )
    .context("install SQLite immutable account policy lineage guards")?;
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS pricing_catalog_versions_runtime_pins_delete
         BEFORE DELETE ON pricing_catalog_versions
         FOR EACH ROW
         WHEN EXISTS (
             SELECT 1 FROM provider_switch_entries
             WHERE product_id = OLD.product_id
               AND catalog_generation = OLD.generation
         )
         BEGIN
             SELECT RAISE(ABORT, 'pricing catalog version is pinned by a provider switch');
         END;
         CREATE TRIGGER IF NOT EXISTS pricing_catalog_versions_runtime_pins_update
         BEFORE UPDATE OF product_id, generation ON pricing_catalog_versions
         FOR EACH ROW
         WHEN (
             NEW.product_id <> OLD.product_id
             OR NEW.generation <> OLD.generation
         ) AND EXISTS (
             SELECT 1 FROM provider_switch_entries
             WHERE product_id = OLD.product_id
               AND catalog_generation = OLD.generation
         )
         BEGIN
             SELECT RAISE(ABORT, 'pricing catalog version is pinned by a provider switch');
         END;
         CREATE TRIGGER IF NOT EXISTS provider_switch_versions_policy_refs_delete
         BEFORE DELETE ON provider_switch_versions
         FOR EACH ROW
         WHEN EXISTS (
             SELECT 1 FROM account_policy_versions
             WHERE switch_generation = OLD.generation
         )
         BEGIN
             SELECT RAISE(ABORT, 'provider switch version is pinned by an account policy');
         END;
         CREATE TRIGGER IF NOT EXISTS provider_switch_versions_policy_refs_update
         BEFORE UPDATE OF generation ON provider_switch_versions
         FOR EACH ROW
         WHEN NEW.generation <> OLD.generation
          AND EXISTS (
              SELECT 1 FROM account_policy_versions
              WHERE switch_generation = OLD.generation
          )
         BEGIN
             SELECT RAISE(ABORT, 'provider switch version is pinned by an account policy');
         END;
         CREATE TRIGGER IF NOT EXISTS account_policy_versions_binding_class_update
         BEFORE UPDATE OF account_class ON account_policy_versions
         FOR EACH ROW
         WHEN NEW.account_class <> OLD.account_class
          AND EXISTS (
              SELECT 1 FROM account_policy_bindings
              WHERE account_id = OLD.account_id
                AND active_effective_version = OLD.effective_version
                AND product_id = OLD.product_id
                AND account_class = OLD.account_class
          )
         BEGIN
             SELECT RAISE(ABORT, 'account policy class is pinned by an active binding');
         END;",
    )
    .context("install SQLite runtime pin parent guards")?;
    Ok(())
}

fn install_sqlite_attribution_guards(conn: &Connection) -> Result<()> {
    const COMMON_INVALID: &str = "
        (NEW.attribution_schema_version IS NOT NULL AND NEW.attribution_schema_version <= 0)
        OR (NEW.snapshot_kind IS NOT NULL
            AND NEW.snapshot_kind NOT IN ('policy_v1', 'legacy_scalar'))
        OR (NEW.alias_generation IS NOT NULL AND NEW.alias_generation <= 0)
        OR (NEW.served_canonical_model_id = '')
        OR (NEW.billing_invariant_code = '')
        OR (NEW.pricing_mode IS NOT NULL
            AND NEW.pricing_mode NOT IN ('track', 'discount', 'legacy_scalar'))
        OR (NEW.rule_origin IS NOT NULL AND NEW.rule_origin NOT IN ('managed', 'legacy'))
        OR (NEW.discount_bps IS NOT NULL
            AND (NEW.discount_bps < 0 OR NEW.discount_bps > 9500
                 OR NEW.discount_bps % 100 <> 0))
        OR (NEW.payable_multiplier_bp IS NOT NULL
            AND (NEW.payable_multiplier_bp < 0 OR NEW.payable_multiplier_bp > 10000))
        OR (NEW.track_eligible IS NOT NULL AND NEW.track_eligible NOT IN (0, 1))
        OR (NEW.retention_eligible IS NOT NULL AND NEW.retention_eligible NOT IN (0, 1))
        OR (NEW.commission_eligible IS NOT NULL AND NEW.commission_eligible NOT IN (0, 1))
        OR (NEW.official_cost_json IS NOT NULL AND NOT json_valid(NEW.official_cost_json))
        OR (NEW.funding_allocation_json IS NOT NULL
            AND NOT json_valid(NEW.funding_allocation_json))
        OR ((NEW.paid_funded_nano IS NULL)
            + (NEW.bonus_funded_nano IS NULL)
            + (NEW.other_funded_nano IS NULL)) NOT IN (0, 3)
    ";
    for (table, charged_column, charge_row_guard, table_invalid) in [
        ("billing_settlement_outbox", "actual_nano", "1", "0"),
        (
            "usage_events",
            "charge_nano",
            "1",
            "NEW.tariff_priced_ts IS NOT NULL AND NEW.tariff_priced_ts <> NEW.priced_ts",
        ),
        (
            "ledger",
            "amount_nano",
            "NEW.kind = 'charge'",
            "NEW.official_nano IS NOT NULL AND NEW.official_nano < 0",
        ),
    ] {
        let funding_invalid = format!(
            "(
                NEW.paid_funded_nano IS NOT NULL
                AND (
                    NOT ({charge_row_guard})
                    OR NEW.paid_funded_nano < 0
                    OR NEW.bonus_funded_nano < 0
                    OR NEW.other_funded_nano < 0
                    OR NEW.paid_funded_nano
                       + NEW.bonus_funded_nano
                       + NEW.other_funded_nano <> NEW.{charged_column}
                )
            )"
        );
        for (suffix, event) in [("insert", "INSERT"), ("update", "UPDATE")] {
            conn.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS {table}_policy_attribution_{suffix}
                 BEFORE {event} ON \"{table}\"
                 FOR EACH ROW
                 WHEN ({COMMON_INVALID}) OR ({table_invalid}) OR ({funding_invalid})
                 BEGIN
                     SELECT RAISE(ABORT, 'invalid policy attribution');
                 END;"
            ))
            .with_context(|| format!("install SQLite attribution guard for {table} {event}"))?;
        }
    }
    Ok(())
}

fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Миграция старой модели «ключ = кошелёк» → «аккаунт (баланс) + ключи (доступы)». Для каждого ключа
/// без `account_id` (легаси) атомарно заводим отдельный случайный аккаунт, переносим баланс/расход/
/// наценку и линкуем ключ. Повторный запуск пропускает уже связанные строки.
fn migrate_legacy_keys(c: &Connection) -> Result<()> {
    let legacy: Vec<(String, i64, i64, i64, String)> = {
        let mut stmt = c.prepare(
            "SELECT key, balance_nano, spent_nano, mult_bp, COALESCE(status,'active') \
             FROM api_keys WHERE account_id IS NULL",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let tx = c.unchecked_transaction()?;
    for (key, bal, spent, mult, status) in legacy {
        // AUDIT(C39): never derive wallet identity from a short key suffix. Generate a full random
        // account ID and persist the key→account mapping atomically with the migrated balance.
        let ts = now();
        let acct: String = tx.query_row(
            "INSERT INTO accounts(id, balance_nano, spent_nano, mult_bp, status, created_ts, created) \
             VALUES('acct_' || lower(hex(randomblob(16))),?1,?2,?3,?4,?5,?6) RETURNING id",
            rusqlite::params![bal, spent, mult, status, ts, chrono_like(ts)],
            |r| r.get(0),
        )?;
        let updated = tx.execute(
            "UPDATE api_keys SET account_id=?1, reserved_nano=0 WHERE key=?2 AND account_id IS NULL",
            rusqlite::params![acct, key],
        )?;
        if updated != 1 {
            anyhow::bail!("legacy key migration lost its target row");
        }
    }
    tx.commit()?;
    // AUDIT-TODO(C39): detect and manually split wallets already merged by the historical suffix migration.
    Ok(())
}

fn resolve_token(inline: Option<String>, token_file: Option<String>) -> String {
    if let Some(t) = inline {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }
    if let Some(f) = token_file {
        if !f.trim().is_empty() {
            if let Ok(s) = fs::read_to_string(f.trim()) {
                return s.trim().to_string();
            }
        }
    }
    String::new()
}

/// Активные подписки нужного флота, у которых есть непустой токен.
pub fn load_active(conn: &Connection, fleet: Option<&str>) -> Result<Vec<Sub>> {
    let mut stmt = conn.prepare(
        "SELECT email, token, token_file, proxy, COALESCE(status,'active'), COALESCE(fleet,'prod'), \
         COALESCE(plan,'') FROM subs",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (email, token, token_file, proxy, status, sfleet, plan) = row?;
        if status != "active" {
            continue;
        }
        if let Some(f) = fleet {
            if f != sfleet {
                continue;
            }
        }
        let tok = resolve_token(token, token_file);
        if tok.is_empty() {
            continue;
        }
        out.push(Sub {
            email,
            token: tok,
            proxy: proxy.unwrap_or_default(),
            fleet: sfleet,
            plan,
        });
    }
    Ok(out)
}

// ── CLI-операции реестра ────────────────────────────────────────────────────
pub fn add(conn: &Connection, email: &str, token: &str, proxy: &str, fleet: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO subs(email, token, token_file, proxy, status, fleet, added_ts, added) \
         VALUES(?1, ?2, NULL, ?3, 'active', ?4, ?5, ?6) \
         ON CONFLICT(email) DO UPDATE SET token=excluded.token, token_file=NULL, \
         proxy=excluded.proxy, status='active', fleet=excluded.fleet, \
         auth_state=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 'healthy' ELSE subs.auth_state END, \
         auth_fail_streak=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.auth_fail_streak END, \
         first_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.first_auth_fail_ts END, \
         last_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.last_auth_fail_ts END, \
         last_auth_http=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.last_auth_http END, \
         dead_since_ts=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.dead_since_ts END, \
         dead_reason=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN '' ELSE subs.dead_reason END, \
         auth_token_fp=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN '' ELSE subs.auth_token_fp END",
        rusqlite::params![email, token, proxy, fleet, now(), chrono_like(now())],
    )?;
    Ok(())
}

pub fn add_file(
    conn: &Connection,
    email: &str,
    token_file: &str,
    proxy: &str,
    fleet: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO subs(email, token, token_file, proxy, status, fleet, added_ts, added) \
         VALUES(?1, NULL, ?2, ?3, 'active', ?4, ?5, ?6) \
         ON CONFLICT(email) DO UPDATE SET token=NULL, token_file=excluded.token_file, \
         proxy=excluded.proxy, status='active', fleet=excluded.fleet, \
         auth_state=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 'healthy' ELSE subs.auth_state END, \
         auth_fail_streak=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.auth_fail_streak END, \
         first_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.first_auth_fail_ts END, \
         last_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.last_auth_fail_ts END, \
         last_auth_http=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.last_auth_http END, \
         dead_since_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.dead_since_ts END, \
         dead_reason=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN '' ELSE subs.dead_reason END, \
         auth_token_fp=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN '' ELSE subs.auth_token_fp END",
        rusqlite::params![email, token_file, proxy, fleet, now(), chrono_like(now())],
    )?;
    Ok(())
}

pub fn set_status(conn: &Connection, email: &str, status: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET status=?1 WHERE email=?2",
        rusqlite::params![status, email],
    )?)
}
pub fn set_plan(conn: &Connection, email: &str, plan: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET plan=?1 WHERE email=?2",
        rusqlite::params![plan, email],
    )?)
}

/// (разрешённый токен, proxy) для одной подписки (любого статуса) — для детекта тарифа.
pub fn get_creds(conn: &Connection, email: &str) -> Result<Option<(String, String)>> {
    let row = conn.query_row(
        "SELECT token, token_file, proxy FROM subs WHERE email=?1",
        rusqlite::params![email],
        |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        },
    );
    match row {
        Ok((token, token_file, proxy)) => {
            let tok = resolve_token(token, token_file);
            if tok.is_empty() {
                Ok(None)
            } else {
                Ok(Some((tok, proxy.unwrap_or_default())))
            }
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub fn set_proxy(conn: &Connection, email: &str, proxy: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET proxy=?1 WHERE email=?2",
        rusqlite::params![proxy, email],
    )?)
}
pub fn set_fleet(conn: &Connection, email: &str, fleet: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET fleet=?1 WHERE email=?2",
        rusqlite::params![fleet, email],
    )?)
}
/// Обновить прокси-метаданные (пишет authbot — владелец жизненного цикла прокси). `expire` — дата
/// истечения из IPRoyal (ISO, "" если неизвестно); `ok` — жив ли прокси на fingerprint-free проверке.
pub fn set_proxy_meta(
    conn: &Connection,
    email: &str,
    expire: &str,
    checked_ts: i64,
    ok: bool,
) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET proxy_expire=?1, proxy_checked_ts=?2, proxy_ok=?3 WHERE email=?4",
        rusqlite::params![expire, checked_ts, ok as i64, email],
    )?)
}
/// host:port из строки прокси (без user:pass) — для показа в панели/логах.
pub fn mask_proxy(p: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    let no_scheme = p.split("://").last().unwrap_or(p);
    no_scheme
        .rsplit('@')
        .next()
        .unwrap_or(no_scheme)
        .to_string()
}
pub fn remove(conn: &Connection, email: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET status='deleted' WHERE email=?1",
        rusqlite::params![email],
    )?)
}

/// Disable every subscription (optionally in one fleet) without destroying active lease history.
pub fn clear(conn: &Connection, fleet: Option<&str>) -> Result<usize> {
    Ok(match fleet {
        Some(f) => conn.execute(
            "UPDATE subs SET status='deleted' WHERE COALESCE(fleet,'prod')=?1 AND status<>'deleted'",
            rusqlite::params![f],
        )?,
        None => conn.execute("UPDATE subs SET status='deleted' WHERE status<>'deleted'", [])?,
    })
}

/// Строка списка для CLI (без утечки токена — только флаг наличия).
pub struct SubRow {
    pub email: String,
    pub status: String,
    pub fleet: String,
    pub plan: String,
    pub has_token: bool,
    pub proxy: String,
}

pub fn list(conn: &Connection) -> Result<Vec<SubRow>> {
    let mut stmt = conn.prepare(
        "SELECT email, COALESCE(status,'active'), COALESCE(fleet,'prod'), COALESCE(plan,''), \
         COALESCE(NULLIF(token,''), NULLIF(token_file,'')), COALESCE(proxy,'') \
         FROM subs ORDER BY COALESCE(added_ts,0)",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(SubRow {
            email: r.get::<_, String>(0)?,
            status: r.get::<_, String>(1)?,
            fleet: r.get::<_, String>(2)?,
            plan: r.get::<_, String>(3)?,
            has_token: r
                .get::<_, Option<String>>(4)?
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            proxy: r.get::<_, String>(5)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Строка админ-обзора подписок (движок → панель): БЕЗ токена, прокси — маска host:port + метаданные.
pub struct SubAdmin {
    pub email: String,
    pub status: String,
    pub fleet: String,
    pub has_token: bool,
    pub proxy_host: String,     // host:port (без user:pass)
    pub proxy_expire: String,   // ISO из IPRoyal / ""
    pub proxy_ok: Option<bool>, // None = не проверялся (здоровье в осн. из движка/органики)
    pub added_ts: i64,          // момент добавления токена (срок жизни = added_ts + N дней)
    pub added: String,
    /// Durable auth-health (авторитетно из БД, переживает рестарт): 'healthy'|'suspect'|'dead'.
    pub auth_state: String,
    pub dead_reason: String, // '' если не dead
    pub dead_since_ts: i64,  // 0 если не dead
}

/// Durable auth-health одной подписки. Движок (поллер) пишет это из КОРРЕЛИРОВАННЫХ чистых probe:
/// один 401/403 не приговор (может быть транзиент/битый запрос), но N подряд за ≥T минут = мёртвый
/// токен/бан. Переживает рестарт и blue/green (в отличие от эфемерного in-memory `auth_dead`).
/// Примитивы-сентинелы (0/"" = «нет») — чтобы `pool` не тащил Option через слой.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubHealth {
    pub email: String,
    pub auth_state: String, // 'healthy' | 'suspect' | 'dead'
    pub auth_fail_streak: i64,
    pub first_auth_fail_ts: i64, // 0 = нет текущей серии отказов
    pub last_auth_fail_ts: i64,
    pub last_auth_http: i64,   // 0 = нет
    pub dead_since_ts: i64,    // 0 = не dead
    pub dead_reason: String,   // '' = нет
    pub auth_token_fp: String, // отпечаток токена, к которому относится вердикт (смена → авто-ревайв)
}

pub fn subs_admin(conn: &Connection) -> Result<Vec<SubAdmin>> {
    let mut stmt = conn.prepare(
        "SELECT email, COALESCE(status,'active'), COALESCE(fleet,'prod'), \
         COALESCE(NULLIF(token,''), NULLIF(token_file,'')), COALESCE(proxy,''), \
         COALESCE(proxy_expire,''), proxy_ok, COALESCE(added_ts,0), COALESCE(added,''), \
         COALESCE(auth_state,'healthy'), COALESCE(dead_reason,''), COALESCE(dead_since_ts,0) \
         FROM subs ORDER BY COALESCE(added_ts,0)",
    )?;
    let rows = stmt.query_map([], |r| {
        let proxy: String = r.get(4)?;
        Ok(SubAdmin {
            email: r.get(0)?,
            status: r.get(1)?,
            fleet: r.get(2)?,
            has_token: r
                .get::<_, Option<String>>(3)?
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            proxy_host: mask_proxy(&proxy),
            proxy_expire: r.get(5)?,
            proxy_ok: r.get::<_, Option<i64>>(6)?.map(|n| n != 0),
            added_ts: r.get(7)?,
            added: r.get(8)?,
            auth_state: r.get(9)?,
            dead_reason: r.get(10)?,
            dead_since_ts: r.get(11)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Загрузить durable auth-health всех подписок (движок сеет им in-memory состояние на старте).
pub fn load_sub_health(conn: &Connection, fleet: Option<&str>) -> Result<Vec<SubHealth>> {
    let mut stmt = conn.prepare(
        "SELECT email, COALESCE(auth_state,'healthy'), COALESCE(auth_fail_streak,0), \
         COALESCE(first_auth_fail_ts,0), COALESCE(last_auth_fail_ts,0), COALESCE(last_auth_http,0), \
         COALESCE(dead_since_ts,0), COALESCE(dead_reason,''), COALESCE(auth_token_fp,''), \
         COALESCE(fleet,'prod') FROM subs")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            SubHealth {
                email: r.get(0)?,
                auth_state: r.get(1)?,
                auth_fail_streak: r.get(2)?,
                first_auth_fail_ts: r.get(3)?,
                last_auth_fail_ts: r.get(4)?,
                last_auth_http: r.get(5)?,
                dead_since_ts: r.get(6)?,
                dead_reason: r.get(7)?,
                auth_token_fp: r.get(8)?,
            },
            r.get::<_, String>(9)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (h, sfleet) = row?;
        if let Some(f) = fleet {
            if f != sfleet {
                continue;
            }
        }
        out.push(h);
    }
    Ok(out)
}

/// Записать durable auth-health одной подписки (движок → БД). Идемпотентный upsert по email.
pub fn save_sub_health(conn: &Connection, h: &SubHealth) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET auth_state=?1, auth_fail_streak=?2, first_auth_fail_ts=?3, \
         last_auth_fail_ts=?4, last_auth_http=?5, dead_since_ts=?6, dead_reason=?7, auth_token_fp=?8 \
         WHERE email=?9",
        rusqlite::params![
            h.auth_state, h.auth_fail_streak, h.first_auth_fail_ts, h.last_auth_fail_ts,
            h.last_auth_http, h.dead_since_ts, h.dead_reason, h.auth_token_fp, h.email
        ],
    )?)
}

// ── Биллинг: ключи клиентов с USD-балансом (нанодоллары) ─────────────────────
//
// Слой хранения: только персист+CRUD баланса. САМ подсчёт стоимости (токены→нано) —
// в крейте `metering`; сюда приходит уже готовая сумма списания в нано. Границы держим:
// registry не знает про цены/токены, только про целые нанодоллары на ключе.

/// Строка ключа. Баланс — НЕ здесь (он на аккаунте); ключ = доступ + метка + атрибуция расхода.
#[derive(Clone, Debug)]
pub struct KeyRow {
    pub key: String,
    pub key_id: String,
    pub account_id: Option<String>,
    pub label: Option<String>,
    pub spent_nano: i64, // расход по ЭТОМУ ключу (атрибуция; баланс общий на аккаунте)
    pub reserved_nano: i64,
    pub spend_limit_nano: Option<i64>,
    pub expires_ts: Option<i64>,
    pub created_ts: i64,
    pub last_used_ts: Option<i64>,
    pub status: String,
}

/// Result of atomically replacing a key's mutable spending policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyPolicyUpdate {
    Updated,
    NotFound,
    LimitBelowUsage,
    ExpiryNotFuture,
}

/// Exact immutable policy identity acknowledged before a key becomes usable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyActivationPolicyAck {
    pub effective_policy_version: i64,
    pub policy_digest: String,
}

impl KeyActivationPolicyAck {
    pub fn validate(&self) -> Result<()> {
        if self.effective_policy_version <= 0
            || self.policy_digest.is_empty()
            || self.policy_digest.trim() != self.policy_digest
        {
            anyhow::bail!("invalid key activation policy ACK identity");
        }
        Ok(())
    }
}

/// Выпустить ключ ПОД аккаунт (баланс — на аккаунте, ключ лишь ссылается). `label` — имя ключа.
pub fn key_issue(
    conn: &Connection,
    key: &str,
    account_id: &str,
    label: Option<&str>,
) -> Result<()> {
    key_issue_with_policy(conn, key, account_id, label, None, None)
}

pub fn key_issue_with_policy(
    conn: &Connection,
    key: &str,
    account_id: &str,
    label: Option<&str>,
    spend_limit_nano: Option<i64>,
    expires_ts: Option<i64>,
) -> Result<()> {
    key_issue_with_policy_ack(
        conn,
        key,
        account_id,
        label,
        spend_limit_nano,
        expires_ts,
        None,
    )
}

pub fn key_issue_with_policy_ack(
    conn: &Connection,
    key: &str,
    account_id: &str,
    label: Option<&str>,
    spend_limit_nano: Option<i64>,
    expires_ts: Option<i64>,
    activation_policy_ack: Option<&KeyActivationPolicyAck>,
) -> Result<()> {
    if key.trim().is_empty() || account_id.trim().is_empty() {
        anyhow::bail!("key and account id must not be empty");
    }
    activation_policy_ack
        .map(KeyActivationPolicyAck::validate)
        .transpose()?;
    let tx = conn.unchecked_transaction()?;
    let policy_state = tx.query_row(
        "SELECT binding.policy_enforcement,binding.active_effective_version,policy.content_digest
           FROM account_policy_bindings binding
           LEFT JOIN account_policy_versions policy
             ON policy.account_id=binding.account_id
            AND policy.effective_version=binding.active_effective_version
          WHERE binding.account_id=?1",
        rusqlite::params![account_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    );
    let policy_state = match policy_state {
        Ok(state) => Some(state),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(error) => return Err(error.into()),
    };
    let strict = policy_state
        .as_ref()
        .is_some_and(|state| state.0 == "strict");
    let ack_matches = activation_policy_ack.is_some_and(|ack| {
        policy_state.as_ref().is_some_and(|state| {
            state.1 == Some(ack.effective_policy_version)
                && state.2.as_deref() == Some(ack.policy_digest.as_str())
        })
    });
    if activation_policy_ack.is_some() && !ack_matches {
        anyhow::bail!("key activation policy ACK does not match the exact active policy");
    }
    if strict && !ack_matches {
        anyhow::bail!("strict key activation requires the exact active policy ACK");
    }
    let ack_version = activation_policy_ack.map(|ack| ack.effective_policy_version);
    let ack_digest = activation_policy_ack.map(|ack| ack.policy_digest.as_str());
    let ack_ts = activation_policy_ack.map(|_| now());
    let changed = tx.execute(
        "INSERT INTO api_keys(key, key_id, account_id, label, spent_nano, reserved_nano, \
         spend_limit_nano,expires_ts,status,created_ts,created,activation_policy_effective_version,
         activation_policy_digest,activation_policy_ack_ts) \
         VALUES(?1,'key_' || lower(hex(randomblob(16))),?2,?3,0,0,?4,?5,'active',?6,?7,
                ?8,?9,?10) \
         ON CONFLICT(key) DO UPDATE SET label=excluded.label, \
         spend_limit_nano=excluded.spend_limit_nano,expires_ts=excluded.expires_ts,
         activation_policy_effective_version=COALESCE(
             excluded.activation_policy_effective_version,
             api_keys.activation_policy_effective_version
         ),
         activation_policy_digest=COALESCE(
             excluded.activation_policy_digest,
             api_keys.activation_policy_digest
         ),
         activation_policy_ack_ts=COALESCE(
             excluded.activation_policy_ack_ts,
             api_keys.activation_policy_ack_ts
         ) \
         WHERE api_keys.account_id=excluded.account_id",
        rusqlite::params![
            key,
            account_id,
            label,
            spend_limit_nano,
            expires_ts,
            now(),
            chrono_like(now()),
            ack_version,
            ack_digest,
            ack_ts,
        ],
    )?;
    if changed == 0 {
        anyhow::bail!("key is already owned by another account");
    }
    tx.commit()?;
    Ok(())
}

/// Backward-compatible startup entry point. Only expired request-scoped leases are touched; legacy
/// aggregate holds remain fail-closed because they still have no provable owner or age.
pub fn reconcile_reservations(conn: &Connection) -> Result<usize> {
    let report = sqlite_reconcile_expired(conn, 10_000)?;
    Ok(report.canceled_before_delivery + report.charged_after_delivery + report.processed_outbox)
}

// ── Аккаунты (профиль клиента: ЕДИНЫЙ баланс, N ключей-доступов) ─────────────────────

/// Строка аккаунта. Баланс/резерв/наценка — ЗДЕСЬ (не на ключе). `handle` — внешняя идентичность.
#[derive(Clone, Debug)]
pub struct AccountRow {
    pub id: String,
    pub handle: Option<String>,
    pub balance_nano: i64,
    pub spent_nano: i64,
    pub reserved_nano: i64,
    pub mult_bp: i64,
    pub status: String,
}

/// Резолв ключа → аккаунт (горячий путь авторизации). Активны должны быть И ключ, И аккаунт.
#[derive(Clone, Debug)]
pub struct KeyAuth {
    pub account_id: String,
    pub mult_bp: i64,
    pub balance_nano: i64,
    pub spent_nano: i64,
    pub reserved_nano: i64,
    pub spend_limit_nano: Option<i64>,
    pub expires_ts: Option<i64>,
    pub active: bool, // ключ активен И аккаунт активен
    pub policy_enforcement: Option<pricing::PolicyEnforcement>,
    pub funding_enforcement: Option<pricing::FundingEnforcement>,
    pub reconciliation_state: Option<pricing::ReconciliationState>,
    pub active_policy_effective_version: Option<i64>,
    pub active_policy_digest: Option<String>,
    pub activation_policy_effective_version: Option<i64>,
    pub activation_policy_digest: Option<String>,
    pub activation_policy_ack_ts: Option<i64>,
    pub policy_ack_current: bool,
    pub paid_available_nano: Option<i64>,
    pub track_available_nano: Option<i64>,
}

impl KeyAuth {
    pub fn strict_policy(&self) -> bool {
        self.policy_enforcement == Some(pricing::PolicyEnforcement::Strict)
            || self.funding_enforcement == Some(pricing::FundingEnforcement::Strict)
    }

    pub fn active_at(&self, ts: i64) -> bool {
        self.active
            && self.expires_ts.is_none_or(|expires| expires > ts)
            && (!self.strict_policy() || self.policy_ack_current)
    }
}

/// Создать аккаунт. `handle` (TG id/email) опционален и уникален (когда задан).
pub fn account_create(
    conn: &Connection,
    id: &str,
    handle: Option<&str>,
    mult_bp: i64,
) -> Result<()> {
    if id.trim().is_empty() || handle.is_some_and(|value| value.trim().is_empty()) {
        anyhow::bail!("account id and supplied handle must not be empty");
    }
    if !(0..=10_000).contains(&mult_bp) {
        anyhow::bail!("account multiplier must be within 0..=10000 basis points");
    }
    conn.execute(
        "INSERT INTO accounts(id, handle, balance_nano, spent_nano, reserved_nano, mult_bp, status, created_ts, created) \
         VALUES(?1, ?2, 0, 0, 0, ?3, 'active', ?4, ?5)",
        rusqlite::params![id, handle, mult_bp, now(), chrono_like(now())])?;
    Ok(())
}

pub fn account_get(conn: &Connection, id: &str) -> Result<Option<AccountRow>> {
    one_account(conn, "id", id)
}
/// Найти аккаунт по внешней идентичности (для входа юзера из TG/web).
pub fn account_by_handle(conn: &Connection, handle: &str) -> Result<Option<AccountRow>> {
    one_account(conn, "handle", handle)
}
fn one_account(conn: &Connection, col: &str, val: &str) -> Result<Option<AccountRow>> {
    let sql = format!(
        "SELECT id, handle, balance_nano, spent_nano, reserved_nano, mult_bp, COALESCE(status,'active') \
         FROM accounts WHERE {col}=?1");
    match conn.query_row(&sql, rusqlite::params![val], |r| {
        Ok(AccountRow {
            id: r.get(0)?,
            handle: r.get(1)?,
            balance_nano: r.get(2)?,
            spent_nano: r.get(3)?,
            reserved_nano: r.get(4)?,
            mult_bp: r.get(5)?,
            status: r.get(6)?,
        })
    }) {
        Ok(a) => Ok(Some(a)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Список аккаунтов (для админ-CLI).
pub fn account_list(conn: &Connection) -> Result<Vec<AccountRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, handle, balance_nano, spent_nano, reserved_nano, mult_bp, COALESCE(status,'active') \
         FROM accounts ORDER BY COALESCE(created_ts,0)")?;
    let rows = stmt.query_map([], |r| {
        Ok(AccountRow {
            id: r.get(0)?,
            handle: r.get(1)?,
            balance_nano: r.get(2)?,
            spent_nano: r.get(3)?,
            reserved_nano: r.get(4)?,
            mult_bp: r.get(5)?,
            status: r.get(6)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn account_set_status(conn: &Connection, id: &str, status: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE accounts SET status=?1 WHERE id=?2",
        rusqlite::params![status, id],
    )?)
}

/// Change the price multiplier for future requests. Existing ledger rows remain immutable.
pub fn account_set_mult_bp(conn: &Connection, id: &str, mult_bp: i64) -> Result<usize> {
    if !(0..=10_000).contains(&mult_bp) {
        anyhow::bail!("invalid account multiplier");
    }
    Ok(conn.execute(
        "UPDATE accounts SET mult_bp=?1 WHERE id=?2",
        rusqlite::params![mult_bp, id],
    )?)
}

/// Tombstone an account. Financial history and in-flight reservations remain settleable/auditable.
pub fn account_remove(conn: &Connection, id: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE accounts SET status='deleted' WHERE id=?1 AND status<>'deleted'",
        rusqlite::params![id],
    )?)
}

/// Пополнить баланс аккаунта (`amount` может быть отрицательным = коррекция) + запись в ledger.
/// Возвращает новый баланс, сохранённый исходный баланс точного idempotent replay, либо None, если
/// аккаунта нет. Повтор reference с другими параметрами — ошибка. Атомарно (UPDATE…RETURNING + ledger).
pub fn account_topup(
    conn: &Connection,
    id: &str,
    amount_nano: i64,
    reference: Option<&str>,
) -> Result<Option<i64>> {
    if matches!(reference, Some(r) if r.trim().is_empty()) {
        anyhow::bail!("monetary idempotency reference must not be empty");
    }
    let allocation_amount = amount_nano
        .checked_abs()
        .context("top-up amount cannot be represented as a funding allocation")?;
    let tx = conn.unchecked_transaction()?;
    let strict_funding: bool = tx.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM account_policy_bindings
              WHERE account_id=?1
                AND policy_enforcement='strict'
                AND funding_enforcement='strict'
                AND reconciliation_state='verified'
         )",
        rusqlite::params![id],
        |row| row.get(0),
    )?;
    // Начисляем баланс...
    let bal = match tx.query_row(
        "UPDATE accounts SET balance_nano = balance_nano + ?1 WHERE id = ?2 RETURNING balance_nano",
        rusqlite::params![amount_nano, id],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(b) => b,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Ok(None);
        } // нет аккаунта
        Err(e) => return Err(e.into()),
    };
    let kind = if amount_nano >= 0 { "topup" } else { "adjust" };
    let strict_bucket = if strict_funding {
        let source_ref = reference.unwrap_or("");
        Some(tx.query_row(
            "INSERT INTO funding_buckets(
                 bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts)
             VALUES('fund_' || lower(hex(randomblob(16))),?1,'paid',?2,'any',?3,0,0,1,
                    CASE WHEN ?3>0 THEN 'active' ELSE 'exhausted' END,?4,?4)
             ON CONFLICT(account_id,source_type,source_ref) DO UPDATE SET
                 balance_nano=funding_buckets.balance_nano+excluded.balance_nano,
                 version=funding_buckets.version+1,updated_ts=excluded.updated_ts,
                 status=CASE
                   WHEN funding_buckets.status='retired' THEN funding_buckets.status
                   WHEN funding_buckets.balance_nano+excluded.balance_nano>0 THEN 'active'
                   ELSE 'exhausted'
                 END
             RETURNING bucket_id,version",
            rusqlite::params![id, source_ref, amount_nano, now()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?)
    } else {
        None
    };
    // ...и пишем ledger. UNIQUE откатывает предварительный UPDATE. После конфликта считаем операцию
    // идемпотентным повтором ТОЛЬКО при точном совпадении account + amount + kind.
    match tx.execute(
        "INSERT INTO ledger(account_id, key, kind, amount_nano, ref, balance_after_nano, ts) \
         VALUES(?1, NULL, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, kind, amount_nano, reference, bal, now()],
    ) {
        Ok(_) => {
            if let Some((bucket_id, bucket_version)) = strict_bucket {
                let ledger_id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO ledger_funding_allocations(
                         ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
                         direction,amount_nano)
                     VALUES(?1,?2,?3,'paid',?4,?5,?6)",
                    rusqlite::params![
                        ledger_id,
                        id,
                        bucket_id,
                        bucket_version,
                        if amount_nano >= 0 { "credit" } else { "debit" },
                        allocation_amount,
                    ],
                )?;
            }
            tx.commit()?;
            Ok(Some(bal))
        }
        Err(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            drop(tx); // ROLLBACK before inspecting the existing operation.
            let Some(reference) = reference else {
                return Err(rusqlite::Error::SqliteFailure(e, None).into());
            };
            let existing = conn.query_row(
                "SELECT account_id, kind, amount_nano, balance_after_nano FROM ledger \
                 WHERE ref=?1 AND kind IN ('topup','adjust')",
                rusqlite::params![reference],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                    ))
                },
            );
            match existing {
                Ok((existing_id, existing_kind, existing_amount, Some(original_balance)))
                    if existing_id == id
                        && existing_kind == kind
                        && existing_amount == amount_nano =>
                {
                    Ok(Some(original_balance))
                }
                Ok(_) => {
                    eprintln!(
                        "billing idempotency conflict: parameters differ from the stored operation"
                    );
                    // AUDIT-TODO(C42/C80): expose a typed idempotency conflict through Control API as HTTP 409.
                    anyhow::bail!(
                        "idempotency reference already belongs to a different monetary operation"
                    )
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    Err(rusqlite::Error::SqliteFailure(e, None).into())
                }
                Err(query_err) => Err(query_err.into()),
            }
        }
        Err(e) => Err(e.into()),
    }
}

/// Горячая money-операция для group-commit (reserve/settle) — writer'а. Ссылки (не owned): вызывающий
/// (billing) держит команды, registry лишь применяет их SQL в ОДНОЙ транзакции.
pub enum HotOp<'a> {
    Reserve {
        account_id: &'a str,
        key: &'a str,
        hold: i64,
    },
    Settle {
        account_id: &'a str,
        key: &'a str,
        hold: i64,
        actual: i64,
        reference: Option<&'a str>,
        usage: Option<&'a UsageEventInput>,
    },
}

/// Применить пачку reserve/settle в ОДНОЙ транзакции (group-commit): амортизирует стоимость коммита
/// под нагрузкой. Команды применяются ПОСЛЕДОВАТЕЛЬНО — атомарный reserve (`WHERE balance>=hold`)
/// видит эффекты предыдущих в этой же транзакции ⇒ инвариант `charge≤hold≤balance` сохранён, как при
/// по-одному. Возвращает результаты в порядке `ops` (индекс-в-индекс). Ошибка BEGIN/COMMIT → Err
/// (вызывающий откатывается на обработку по-одному). Per-op ошибки глушатся в None (как в прежнем
/// writer'е: `.ok().flatten()`).
pub fn apply_hot_batch(conn: &Connection, ops: &[HotOp]) -> Result<Vec<Option<i64>>> {
    let tx = conn.unchecked_transaction()?;
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        out.push(match op {
            HotOp::Reserve {
                account_id,
                key,
                hold,
            } => account_reserve_for_key(&tx, account_id, key, *hold)
                .ok()
                .flatten(),
            HotOp::Settle {
                account_id,
                key,
                hold,
                actual,
                reference,
                usage,
            } => account_settle_in(&tx, account_id, key, *hold, *actual, *reference, *usage)
                .ok()
                .flatten(),
        });
    }
    tx.commit()?;
    Ok(out)
}

/// АТОМАРНО зарезервировать `hold` по АККАУНТУ, если баланс покрывает и аккаунт активен. Та же
/// семантика, что была на ключе, но кошелёк — общий на профиль (все ключи юзера тратят из него).
pub fn account_reserve(conn: &Connection, id: &str, hold_nano: i64) -> Result<Option<i64>> {
    let hold = hold_nano.max(0);
    match conn.query_row(
        "UPDATE accounts SET balance_nano = balance_nano - ?1, reserved_nano = reserved_nano + ?1 \
         WHERE id = ?2 AND status = 'active' AND balance_nano >= ?1 RETURNING balance_nano",
        rusqlite::params![hold, id],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(bal) => Ok(Some(bal)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Reserve against both the shared account balance and one key's lifetime spending policy.
pub fn account_reserve_for_key(
    conn: &Connection,
    id: &str,
    key: &str,
    hold_nano: i64,
) -> Result<Option<i64>> {
    let hold = hold_nano.max(0);
    conn.execute_batch("SAVEPOINT key_policy_reserve")?;
    let balance = match conn.query_row(
        "UPDATE accounts SET balance_nano=balance_nano-?1, reserved_nano=reserved_nano+?1 \
         WHERE id=?2 AND status='active' AND balance_nano>=?1 RETURNING balance_nano",
        rusqlite::params![hold, id],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(value) => value,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            conn.execute_batch("ROLLBACK TO key_policy_reserve; RELEASE key_policy_reserve")?;
            return Ok(None);
        }
        Err(error) => {
            let _ =
                conn.execute_batch("ROLLBACK TO key_policy_reserve; RELEASE key_policy_reserve");
            return Err(error.into());
        }
    };
    let updated = match conn.execute(
        "UPDATE api_keys SET reserved_nano=reserved_nano+?1 \
         WHERE key=?2 AND account_id=?3 AND COALESCE(status,'active')='active' \
           AND (expires_ts IS NULL OR expires_ts>CAST(strftime('%s','now') AS INTEGER)) \
           AND (spend_limit_nano IS NULL OR spent_nano+reserved_nano+?1<=spend_limit_nano)",
        rusqlite::params![hold, key, id],
    ) {
        Ok(value) => value,
        Err(error) => {
            let _ =
                conn.execute_batch("ROLLBACK TO key_policy_reserve; RELEASE key_policy_reserve");
            return Err(error.into());
        }
    };
    if updated != 1 {
        conn.execute_batch("ROLLBACK TO key_policy_reserve; RELEASE key_policy_reserve")?;
        return Ok(None);
    }
    conn.execute_batch("RELEASE key_policy_reserve")?;
    Ok(Some(balance))
}

/// Закрыть резерв аккаунта: баланс += hold − actual, spent += actual, reserved −= hold; per-key
/// `spent` += actual (атрибуция расхода по ключу); строка в ledger (charge). ВСЁ в ОДНОЙ транзакции.
pub fn account_settle(
    conn: &Connection,
    id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
) -> Result<Option<i64>> {
    let tx = conn.unchecked_transaction()?;
    let bal = account_settle_in(&tx, id, key, hold_nano, actual_nano, reference, usage)?;
    tx.commit()?;
    Ok(bal)
}

/// SQL-тело settle БЕЗ BEGIN/COMMIT — для group-commit writer'а (несколько settle в одной транзакции).
/// Вызывающий обязан обернуть в транзакцию (`account_settle` — тонкая обёртка). `conn` может быть
/// `&Transaction` (Deref в `&Connection`). Семантика идентична `account_settle`.
pub fn account_settle_in(
    conn: &Connection,
    id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
) -> Result<Option<i64>> {
    let hold = hold_nano.max(0);
    // Exact provider usage may exceed a conservative hold because of provider-added tokens or a
    // response-reported pricing modifier. Forward caps this at hold+$1, matching the account-level
    // overdraft floor; silently clamping here would turn delivered provider usage into lost revenue.
    let actual = actual_nano.max(0);
    // Возвращаем hold, но НЕ БОЛЬШЕ, чем реально числится в reserved_nano: MIN(hold, reserved).
    // Защита от двойного settle (перекрытие деплоя: reconcile уже вернул резерв, затем прилетел
    // settle старого инстанса) — иначе balance получил бы +hold дважды (over-credit) и reserved
    // ушёл бы в минус. MAX(0, …) держит reserved ≥ 0. В норме (reserved≥hold) поведение прежнее.
    let bal = match conn.query_row(
        "UPDATE accounts SET \
         balance_nano = balance_nano + MIN(?1, reserved_nano) - ?2, \
         spent_nano = spent_nano + ?2, \
         reserved_nano = MAX(0, reserved_nano - ?1) WHERE id = ?3 RETURNING balance_nano",
        rusqlite::params![hold, actual, id],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(b) => Some(b),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => return Err(e.into()),
    };
    if let Some(b) = bal {
        conn.execute(
            "UPDATE api_keys SET spent_nano=spent_nano+?1, \
             reserved_nano=MAX(0,reserved_nano-?2) WHERE key=?3",
            rusqlite::params![actual, hold, key],
        )?;
        if actual > 0 {
            // Модель за списанием — из usage того же запроса (пустую строку не пишем → NULL).
            let model = usage.map(|u| u.model.as_str()).filter(|m| !m.is_empty());
            ledger_add(conn, id, Some(key), "charge", actual, reference, b, model)?;
            // usage_events (аналитика) — в ТОЙ ЖЕ транзакции, что и charge (экономим коммит на запрос).
            // Best-effort: ошибка вставки usage НЕ роняет money-коммит (аналитика не критична).
            if let Some(u) = usage {
                let _ = usage_event_add(conn, id, Some(key), u, actual, reference);
            }
        }
    }
    Ok(bal)
}

/// Резолв ключа в аккаунт для авторизации запроса (JOIN api_keys→accounts).
pub fn key_account(conn: &Connection, key: &str) -> Result<Option<KeyAuth>> {
    let row = conn.query_row(
        "SELECT a.id, a.mult_bp, a.balance_nano, k.spent_nano, k.reserved_nano, \
         k.spend_limit_nano, k.expires_ts, \
         (COALESCE(k.status,'active')='active' AND COALESCE(a.status,'active')='active'),
         binding.policy_enforcement,binding.funding_enforcement,binding.reconciliation_state,
         binding.active_effective_version,policy.content_digest,
         k.activation_policy_effective_version,k.activation_policy_digest,
         k.activation_policy_ack_ts,
         COALESCE(
             k.activation_policy_effective_version=binding.active_effective_version
             AND k.activation_policy_digest=policy.content_digest
             AND k.activation_policy_ack_ts IS NOT NULL,
             0
         ),
         CASE WHEN binding.funding_enforcement='strict' THEN (
             SELECT COALESCE(SUM(bucket.balance_nano),0)
               FROM funding_buckets bucket
              WHERE bucket.account_id=a.id AND bucket.eligibility='any'
         ) END,
         CASE WHEN binding.funding_enforcement='strict' THEN (
             SELECT COALESCE(SUM(bucket.balance_nano),0)
               FROM funding_buckets bucket
              WHERE bucket.account_id=a.id AND bucket.eligibility IN ('track','any')
         ) END \
         FROM api_keys k
         JOIN accounts a ON a.id = k.account_id
         LEFT JOIN account_policy_bindings binding ON binding.account_id=a.id
         LEFT JOIN account_policy_versions policy
           ON policy.account_id=binding.account_id
          AND policy.effective_version=binding.active_effective_version
         WHERE k.key = ?1",
        rusqlite::params![key],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, i64>(7)? != 0,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, Option<String>>(9)?,
                r.get::<_, Option<String>>(10)?,
                r.get::<_, Option<i64>>(11)?,
                r.get::<_, Option<String>>(12)?,
                r.get::<_, Option<i64>>(13)?,
                r.get::<_, Option<String>>(14)?,
                r.get::<_, Option<i64>>(15)?,
                r.get::<_, i64>(16)? != 0,
                r.get::<_, Option<i64>>(17)?,
                r.get::<_, Option<i64>>(18)?,
            ))
        },
    );
    let row = match row {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    Ok(Some(KeyAuth {
        account_id: row.0,
        mult_bp: row.1,
        balance_nano: row.2,
        spent_nano: row.3,
        reserved_nano: row.4,
        spend_limit_nano: row.5,
        expires_ts: row.6,
        active: row.7,
        policy_enforcement: row
            .8
            .as_deref()
            .map(pricing::PolicyEnforcement::from_db)
            .transpose()?,
        funding_enforcement: row
            .9
            .as_deref()
            .map(pricing::FundingEnforcement::from_db)
            .transpose()?,
        reconciliation_state: row
            .10
            .as_deref()
            .map(pricing::ReconciliationState::from_db)
            .transpose()?,
        active_policy_effective_version: row.11,
        active_policy_digest: row.12,
        activation_policy_effective_version: row.13,
        activation_policy_digest: row.14,
        activation_policy_ack_ts: row.15,
        policy_ack_current: row.16,
        paid_available_nano: row.17,
        track_available_nano: row.18,
    }))
}

/// Консистентный ОНЛАЙН-бэкап всей БД в `out_path` через `VACUUM INTO` (best-practice для живого
/// SQLite): создаёт целостный снимок, безопасный при WAL и параллельной работе. НЕЛЬЗЯ просто
/// копировать `.db` — без `-wal`/`-shm` копия рассинхронизирована/битая. `out_path` должен НЕ
/// существовать (VACUUM INTO создаёт файл). Восстановление: остановить сервис, положить снимок на
/// место `subscriptions.db`, удалить `-wal`/`-shm`, запустить.
pub fn backup_to(conn: &Connection, out_path: &str) -> Result<()> {
    let esc = out_path.replace('\'', "''"); // путь наш, но экранируем кавычку
    conn.execute_batch(&format!("VACUUM INTO '{esc}'"))?;
    Ok(())
}

/// Свернуть WAL в основную БД и обрезать файл (TRUNCATE). Авто-checkpoint SQLite (порог ~1000 стр.)
/// обычно держит WAL в узде, но под НЕПРЕРЫВНОЙ записью + постоянными читателями (наш случай:
/// reserve/settle на каждом запросе + N read-соединений) чекпоинт может откладываться и WAL растёт.
/// Периодический явный TRUNCATE-чекпоинт держит файл ограниченным. PASSIVE не нужен — вызываем редко
/// из persist_loop; занятость нормальна, вернём Ok даже если часть страниц осталась (не критично).
pub fn wal_checkpoint(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

pub fn ledger_ack(
    conn: &Connection,
    consumer: &str,
    account_id: &str,
    last_ledger_id: i64,
) -> Result<usize> {
    if consumer.trim().is_empty() || last_ledger_id < 0 {
        anyhow::bail!("invalid ledger checkpoint");
    }
    Ok(conn.execute(
        "INSERT INTO ledger_consumer_checkpoints(consumer,account_id,last_ledger_id,updated_ts) \
         VALUES(?1,?2,?3,?4) ON CONFLICT(consumer,account_id) DO UPDATE SET \
         last_ledger_id=MAX(last_ledger_id,excluded.last_ledger_id),updated_ts=excluded.updated_ts",
        rusqlite::params![consumer, account_id, last_ledger_id, now()],
    )?)
}

/// Delete charge detail only after the required pricing consumer has durably acknowledged it.
/// Top-ups/adjustments remain as the long-term accounting record.
pub fn ledger_prune(conn: &Connection, older_than_ts: i64) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM ledger WHERE id IN ( \
           SELECT l.id FROM ledger l JOIN ledger_consumer_checkpoints c \
             ON c.account_id=l.account_id AND c.consumer='pricing' \
           WHERE l.kind='charge' AND l.ts < ?1 AND l.id <= c.last_ledger_id \
           ORDER BY l.id LIMIT 5000 \
         )",
        rusqlite::params![older_than_ts],
    )?)
}

/// Добавить строку в append-only ledger (журнал движений баланса). `model` — Claude-модель за
/// charge-строкой (для точного per-model графика); у topup/adjust модели нет → None.
#[allow(clippy::too_many_arguments)]
fn ledger_add(
    conn: &Connection,
    account_id: &str,
    key: Option<&str>,
    kind: &str,
    amount_nano: i64,
    reference: Option<&str>,
    balance_after: i64,
    model: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO ledger(account_id, key, kind, amount_nano, ref, balance_after_nano, ts, model) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![account_id, key, kind, amount_nano, reference, balance_after, now(), model])?;
    Ok(())
}

/// Traffic predates provider attribution, or was queued by an engine release that only served
/// Claude. Either way the Claude fleet is the only upstream it could have used.
pub const PROVIDER_ANTHROPIC: &str = "anthropic";
/// The OpenAI-compatible Codex home pool.
pub const PROVIDER_OPENAI: &str = "openai";
/// The isolated native Gemini-compatible subscription pool.
pub const PROVIDER_GOOGLE: &str = "google";

fn default_provider() -> String {
    PROVIDER_ANTHROPIC.to_string()
}

/// Разбивка одного оплаченного запроса по корзинам токенов + модель (owned — переживает канал
/// биллинг-актора). `real_nano` — стоимость по официальным ценам (×1.0, до наценки).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct UsageEventInput {
    pub model: String,
    /// Upstream that served the request: `anthropic` for the Claude fleet, `openai` for the
    /// Codex home pool. Defaulted on deserialization so settlement rows queued by a previous
    /// engine release stay readable across a blue-green promotion.
    #[serde(default = "default_provider")]
    pub provider: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_5m_tokens: i64,
    pub cache_write_1h_tokens: i64,
    pub web_search_requests: i64,
    pub real_nano: i64,
    pub speed: String,
    pub inference_geo: String,
    pub input_nano: i64,
    pub output_nano: i64,
    pub cache_read_nano: i64,
    pub cache_write_5m_nano: i64,
    pub cache_write_1h_nano: i64,
    pub web_search_nano: i64,
    pub priced_ts: i64,
}

/// Create one durable SQLite reservation. Exact active replays return the original post-reserve
/// balance; a reused request ID with different parameters or a terminal request fails closed.
pub fn sqlite_reserve_request(
    conn: &Connection,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
    lease_secs: i64,
) -> Result<Option<i64>> {
    if request_id.trim().is_empty()
        || account_id.trim().is_empty()
        || key.trim().is_empty()
        || hold_nano < 0
        || lease_secs <= 0
    {
        anyhow::bail!("invalid SQLite reservation parameters");
    }
    let tx = conn.unchecked_transaction()?;
    let existing = tx.query_row(
        "SELECT account_id,key,hold_nano,state,balance_after_reserve_nano \
         FROM billing_reservations WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        },
    );
    match existing {
        Ok((stored_account, stored_key, stored_hold, state, balance)) => {
            if stored_account != account_id || stored_key != key || stored_hold != hold_nano {
                anyhow::bail!("reservation request ID was reused with different parameters");
            }
            if state == "reserved" || state == "delivering" {
                tx.commit()?;
                return Ok(Some(balance));
            }
            anyhow::bail!("reservation request is already terminal");
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {}
        Err(error) => return Err(error.into()),
    }

    let Some(balance) = account_reserve_for_key(&tx, account_id, key, hold_nano)? else {
        tx.rollback()?;
        return Ok(None);
    };
    let timestamp = now();
    tx.execute(
        "INSERT INTO billing_reservations( \
           request_id,account_id,key,hold_nano,state,balance_after_reserve_nano,lease_until,created_ts,updated_ts) \
         VALUES(?1,?2,?3,?4,'reserved',?5,?6,?7,?7)",
        rusqlite::params![request_id, account_id, key, hold_nano, balance,
            timestamp.saturating_add(lease_secs), timestamp],
    )?;
    tx.commit()?;
    Ok(Some(balance))
}

/// Atomically reserve the charged legacy hold and persist its immutable pricing identity.
///
/// This is a dormant Stage 3B bridge primitive. The existing `sqlite_reserve_request` path remains
/// unchanged and never requires or creates a snapshot. An existing reservation without a snapshot
/// is deliberately a conflict: filling attribution in after the money commit would fabricate an
/// atomic history that did not occur.
pub fn sqlite_reserve_request_with_legacy_snapshot(
    conn: &Connection,
    key: &str,
    lease_secs: i64,
    snapshot: &pricing::LegacyScalarAdmissionSnapshot,
) -> Result<pricing::LegacyScalarReserveOutcome> {
    sqlite_reserve_request_with_legacy_snapshot_guarded(conn, key, lease_secs, snapshot, || true)
}

/// Guarded variant for an async handoff: `commit_gate` runs after all fallible writes and
/// immediately before commit. Returning false rolls the transaction back without durable money,
/// reservation, or snapshot writes. The gate is called only for `Inserted` or exact `Unchanged`
/// success, never for `NotReserved`, conflicts, or earlier failures.
pub fn sqlite_reserve_request_with_legacy_snapshot_guarded(
    conn: &Connection,
    key: &str,
    lease_secs: i64,
    snapshot: &pricing::LegacyScalarAdmissionSnapshot,
    mut commit_gate: impl FnMut() -> bool,
) -> Result<pricing::LegacyScalarReserveOutcome> {
    use pricing::{
        LegacyScalarReserveConflict as Conflict, LegacyScalarReserveOutcome as Outcome,
        LegacyScalarReserveReceipt as Receipt, LegacyScalarSnapshotLookup as Lookup,
    };

    snapshot.validate()?;
    if key.trim().is_empty() || lease_secs <= 0 {
        anyhow::bail!("invalid SQLite legacy snapshot reservation parameters");
    }
    let window_conflict = |trusted_now_ts| -> Result<Option<Conflict>> {
        match snapshot.validate_idempotency_window_at(trusted_now_ts) {
            Ok(()) => Ok(None),
            Err(pricing::LegacyScalarIdempotencyWindowError::Expired) => {
                Ok(Some(Conflict::ExpiredIdempotencyWindow))
            }
            Err(pricing::LegacyScalarIdempotencyWindowError::AdmissionFromFuture) => {
                Ok(Some(Conflict::AdmissionTimestampInFuture))
            }
            Err(pricing::LegacyScalarIdempotencyWindowError::InvalidTrustedTimestamp) => {
                anyhow::bail!("trusted SQLite reservation clock is invalid")
            }
        }
    };
    if let Some(conflict) = window_conflict(now())? {
        return Ok(Outcome::Conflict(conflict));
    }

    let request_id = snapshot.request_id.as_str();
    let account_id = snapshot.account_id.as_str();
    let hold_nano = snapshot.charged_hold_nano;
    let tx = conn.unchecked_transaction()?;
    let existing = match tx.query_row(
        "SELECT account_id,key,hold_nano,state,balance_after_reserve_nano \
         FROM billing_reservations WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        },
    ) {
        Ok(row) => Some(row),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(conflict) = window_conflict(now())? {
        tx.rollback()?;
        return Ok(Outcome::Conflict(conflict));
    }
    if let Some((stored_account, stored_key, stored_hold, state, balance)) = existing {
        let outcome = if stored_account != account_id
            || stored_key != key
            || stored_hold != hold_nano
        {
            Outcome::Conflict(Conflict::ReservationIdentity)
        } else if state != "reserved" && state != "delivering" {
            Outcome::Conflict(Conflict::TerminalReservation)
        } else {
            match pricing::sqlite_legacy_scalar_snapshot_lookup(&tx, request_id)? {
                Lookup::Missing => Outcome::Conflict(Conflict::ExistingReservationWithoutSnapshot),
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
        if matches!(&outcome, Outcome::Unchanged(_)) && !commit_gate() {
            tx.rollback()?;
            return Ok(Outcome::AbortedBeforeCommit);
        }
        tx.commit()?;
        return Ok(outcome);
    }

    let Some(balance) = account_reserve_for_key(&tx, account_id, key, hold_nano)? else {
        tx.rollback()?;
        return Ok(Outcome::NotReserved);
    };
    let timestamp = now();
    tx.execute(
        "INSERT INTO billing_reservations( \
           request_id,account_id,key,hold_nano,state,balance_after_reserve_nano,lease_until,created_ts,updated_ts) \
         VALUES(?1,?2,?3,?4,'reserved',?5,?6,?7,?7)",
        rusqlite::params![request_id, account_id, key, hold_nano, balance,
            timestamp.saturating_add(lease_secs), timestamp],
    )?;
    if let Err(error) = pricing::sqlite_insert_legacy_scalar_admission_snapshot(&tx, snapshot) {
        let _ = tx.rollback();
        return Err(error);
    }
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

fn sqlite_policy_state_matches(
    conn: &Connection,
    snapshot: &pricing::PolicyAdmissionSnapshot,
) -> Result<bool> {
    let (rule_scope, rule_provider, rule_model) = snapshot.rule_scope.db_parts();
    let (scoped_type, scoped_product, scoped_segment) = match snapshot.account_class {
        pricing::AccountClass::B2c => ("segment", snapshot.product_id.as_str(), "b2c"),
        pricing::AccountClass::B2b => ("segment", snapshot.product_id.as_str(), "b2b"),
        pricing::AccountClass::OpenKeys | pricing::AccountClass::Service => {
            ("product", snapshot.product_id.as_str(), "")
        }
    };
    Ok(conn.query_row(
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
           JOIN pricing_catalog_heads catalog_head ON catalog_head.product_id=policy.product_id
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
           WHERE binding.account_id=?1
             AND binding.policy_enforcement='strict'
             AND binding.funding_enforcement='strict'
             AND binding.reconciliation_state='verified'
             AND policy.effective_version=?2
             AND policy.policy_id=?3
             AND policy.policy_version=?4
             AND policy.source_policy_digest=?5
             AND policy.content_digest=?6
             AND policy.product_id=?7
             AND policy.account_class=?8
             AND policy.catalog_generation=?9
             AND policy.switch_generation=?10
             AND catalog.generation=?11
             AND catalog.content_digest=?12
             AND switches.generation=?13
             AND switches.content_digest=?14
             AND rule.rule_id=?15
             AND rule.rule_digest=?16
             AND rule.scope_type=?17
             AND rule.provider_id=?18
             AND rule.canonical_model_id IS ?19
             AND rule.pricing_mode=?20
             AND rule.rule_origin=?21
             AND rule.discount_bps IS ?22
             AND rule.payable_multiplier_bp=?23
             AND rule.track_eligible=?24
             AND rule.retention_eligible=?25
             AND rule.commission_eligible=?26
             AND policy_model.provider_id=?18
             AND policy_model.canonical_model_id=?27
             AND policy_model.enabled=1
             AND admission_model.provider_id=?18
             AND admission_model.canonical_model_id=?27
             AND admission_model.enabled=1
             AND policy_master.provider_id=?18
             AND policy_master.scope_type='master'
             AND policy_master.product_id=''
             AND policy_master.segment=''
             AND policy_master.enabled=1
             AND policy_scoped.provider_id=?18
             AND policy_scoped.scope_type=?28
             AND policy_scoped.product_id=?29
             AND policy_scoped.segment=?30
             AND policy_scoped.catalog_generation=policy_catalog.generation
             AND policy_scoped.enabled=1
             AND admission_master.provider_id=?18
             AND admission_master.scope_type='master'
             AND admission_master.product_id=''
             AND admission_master.segment=''
             AND admission_master.enabled=1
             AND admission_scoped.provider_id=?18
             AND admission_scoped.scope_type=?28
             AND admission_scoped.product_id=?29
             AND admission_scoped.segment=?30
             AND admission_scoped.catalog_generation IN (
                 catalog.generation, policy_catalog.generation
             )
             AND admission_scoped.enabled=1
         )",
        rusqlite::params![
            snapshot.account_id,
            snapshot.effective_policy_version,
            snapshot.policy_id,
            snapshot.policy_version,
            snapshot.source_policy_digest,
            snapshot.policy_digest,
            snapshot.product_id,
            snapshot.account_class.as_str(),
            snapshot.policy_catalog_generation,
            snapshot.policy_switch_generation,
            snapshot.admission_catalog_generation,
            snapshot.admission_catalog_digest,
            snapshot.admission_switch_generation,
            snapshot.admission_switch_digest,
            snapshot.rule_id,
            snapshot.rule_digest,
            rule_scope,
            rule_provider,
            rule_model,
            snapshot.pricing_mode.as_str(),
            snapshot.rule_origin.as_str(),
            snapshot.discount_bps,
            snapshot.payable_multiplier_bp,
            i64::from(snapshot.track_eligible),
            i64::from(snapshot.retention_eligible),
            i64::from(snapshot.commission_eligible),
            snapshot.canonical_model_id,
            scoped_type,
            scoped_product,
            scoped_segment,
        ],
        |row| row.get(0),
    )?)
}

/// Atomically validate the current strict binding, reserve exact eligible funding sources and
/// persist the immutable policy snapshot. Track spends welcome bonus before paid; discount/static
/// requests can reserve only `any` (paid) funding.
pub fn sqlite_reserve_request_with_policy_snapshot(
    conn: &Connection,
    key: &str,
    lease_secs: i64,
    snapshot: &pricing::PolicyAdmissionSnapshot,
) -> Result<pricing::PolicyReserveOutcome> {
    sqlite_reserve_request_with_policy_snapshot_guarded(conn, key, lease_secs, snapshot, || true)
}

/// Guarded strict reserve for the async writer. The callback is the final linearization gate for
/// inserted reservations and exact active replays; a rejected gate rolls the transaction back
/// before any money or snapshot mutation becomes visible.
pub fn sqlite_reserve_request_with_policy_snapshot_guarded(
    conn: &Connection,
    key: &str,
    lease_secs: i64,
    snapshot: &pricing::PolicyAdmissionSnapshot,
    mut commit_gate: impl FnMut() -> bool,
) -> Result<pricing::PolicyReserveOutcome> {
    use pricing::{
        PolicyReserveConflict as Conflict, PolicyReserveOutcome as Outcome,
        PolicyReserveReceipt as Receipt, PolicySnapshotLookup as Lookup,
    };

    snapshot.validate()?;
    if key.trim().is_empty() || lease_secs <= 0 {
        anyhow::bail!("invalid SQLite policy snapshot reservation parameters");
    }
    let window_conflict = |trusted_now_ts| -> Result<Option<Conflict>> {
        match snapshot.validate_idempotency_window_at(trusted_now_ts) {
            Ok(()) => Ok(None),
            Err(pricing::LegacyScalarIdempotencyWindowError::Expired) => {
                Ok(Some(Conflict::ExpiredIdempotencyWindow))
            }
            Err(pricing::LegacyScalarIdempotencyWindowError::AdmissionFromFuture) => {
                Ok(Some(Conflict::AdmissionTimestampInFuture))
            }
            Err(pricing::LegacyScalarIdempotencyWindowError::InvalidTrustedTimestamp) => {
                anyhow::bail!("trusted SQLite reservation clock is invalid")
            }
        }
    };
    if let Some(conflict) = window_conflict(now())? {
        return Ok(Outcome::Conflict(conflict));
    }

    let request_id = snapshot.request_id();
    let account_id = snapshot.account_id();
    let hold = snapshot.charged_hold_nano();
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let existing = tx.query_row(
        "SELECT account_id,key,hold_nano,state,balance_after_reserve_nano
           FROM billing_reservations WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        },
    );
    match existing {
        Ok((stored_account, stored_key, stored_hold, state, balance)) => {
            let outcome =
                if stored_account != account_id || stored_key != key || stored_hold != hold {
                    Outcome::Conflict(Conflict::ReservationIdentity)
                } else if state != "reserved" && state != "delivering" {
                    Outcome::Conflict(Conflict::TerminalReservation)
                } else {
                    match pricing::sqlite_policy_snapshot_lookup(&tx, request_id)? {
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
            if matches!(&outcome, Outcome::Unchanged(_)) && !commit_gate() {
                tx.rollback()?;
                return Ok(Outcome::AbortedBeforeCommit);
            }
            tx.commit()?;
            return Ok(outcome);
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {}
        Err(error) => return Err(error.into()),
    }
    if let Some(conflict) = window_conflict(now())? {
        tx.rollback()?;
        return Ok(Outcome::Conflict(conflict));
    }
    if !sqlite_policy_state_matches(&tx, snapshot)? {
        tx.rollback()?;
        return Ok(Outcome::Conflict(Conflict::PolicyStateChanged));
    }

    let eligibility = if snapshot.track_eligible() {
        "track"
    } else {
        "any"
    };
    let buckets: Vec<(String, i64, i64)> = {
        let mut statement = tx.prepare(
            "SELECT bucket_id,version,balance_nano
               FROM funding_buckets
              WHERE account_id=?1 AND status='active' AND balance_nano>0
                AND ((?2='track' AND eligibility IN ('track','any'))
                  OR (?2='any' AND eligibility='any'))
              ORDER BY CASE source_type
                         WHEN 'welcome_track_bonus' THEN 0
                         WHEN 'paid' THEN 1
                         ELSE 2
                       END, created_ts, bucket_id",
        )?;
        let rows = statement
            .query_map(rusqlite::params![account_id, eligibility], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };
    let eligible_total = buckets.iter().try_fold(0_i64, |total, (_, _, balance)| {
        total
            .checked_add(*balance)
            .context("eligible funding balance overflow")
    })?;
    if eligible_total < hold {
        tx.rollback()?;
        return Ok(Outcome::NotReserved);
    }

    let balance: i64 = match tx.query_row(
        "UPDATE accounts
            SET balance_nano=balance_nano-?1,reserved_nano=reserved_nano+?1
          WHERE id=?2 AND status='active' AND balance_nano>=?1
          RETURNING balance_nano",
        rusqlite::params![hold, account_id],
        |row| row.get(0),
    ) {
        Ok(balance) => balance,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            tx.rollback()?;
            return Ok(Outcome::NotReserved);
        }
        Err(error) => return Err(error.into()),
    };
    if tx.execute(
        "UPDATE api_keys SET reserved_nano=reserved_nano+?1
          WHERE key=?2 AND account_id=?3 AND status='active'
            AND (expires_ts IS NULL OR expires_ts>CAST(strftime('%s','now') AS INTEGER))
            AND (spend_limit_nano IS NULL OR spent_nano+reserved_nano+?1<=spend_limit_nano)",
        rusqlite::params![hold, key, account_id],
    )? != 1
    {
        tx.rollback()?;
        return Ok(Outcome::NotReserved);
    }
    let timestamp = now();
    tx.execute(
        "INSERT INTO billing_reservations(
           request_id,account_id,key,hold_nano,state,balance_after_reserve_nano,lease_until,
           created_ts,updated_ts)
         VALUES(?1,?2,?3,?4,'reserved',?5,?6,?7,?7)",
        rusqlite::params![
            request_id,
            account_id,
            key,
            hold,
            balance,
            timestamp.saturating_add(lease_secs),
            timestamp
        ],
    )?;
    pricing::sqlite_insert_policy_admission_snapshot(&tx, snapshot)?;

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
                SET balance_nano=balance_nano-?1,reserved_nano=reserved_nano+?1,
                    version=?2,updated_ts=?3,
                    status=CASE WHEN balance_nano-?1=0 THEN 'exhausted' ELSE 'active' END
              WHERE bucket_id=?4 AND account_id=?5 AND version=?6 AND balance_nano>=?1",
            rusqlite::params![
                reserved,
                next_version,
                timestamp,
                bucket_id,
                account_id,
                version
            ],
        )? != 1
        {
            anyhow::bail!("strict funding bucket changed during SQLite reserve");
        }
        tx.execute(
            "INSERT INTO reservation_funding_allocations(
               request_id,account_id,bucket_id,bucket_version,reserved_nano,allocation_order)
             VALUES(?1,?2,?3,?4,?5,?6)",
            rusqlite::params![
                request_id,
                account_id,
                bucket_id,
                next_version,
                reserved,
                allocation_order
            ],
        )?;
        remaining -= reserved;
        allocation_order += 1;
    }
    if remaining != 0 {
        anyhow::bail!("strict funding allocation did not cover the reserved hold");
    }
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

/// Mark a reservation as provider-accepted before handing its response body to the client.
pub fn sqlite_mark_delivering(
    conn: &Connection,
    request_id: &str,
    lease_secs: i64,
) -> Result<bool> {
    if lease_secs <= 0 {
        anyhow::bail!("invalid delivery lease");
    }
    let timestamp = now();
    let changed = conn.execute(
        "UPDATE billing_reservations SET state='delivering',lease_until=?2,updated_ts=?3 \
         WHERE request_id=?1 AND state IN ('reserved','delivering')",
        rusqlite::params![request_id, timestamp.saturating_add(lease_secs), timestamp],
    )?;
    Ok(changed == 1)
}

pub fn sqlite_renew_reservation_lease(
    conn: &Connection,
    request_id: &str,
    lease_secs: i64,
) -> Result<bool> {
    if lease_secs <= 0 {
        anyhow::bail!("invalid reservation lease");
    }
    let timestamp = now();
    Ok(conn.execute(
        "UPDATE billing_reservations SET lease_until=?2,updated_ts=?3 \
         WHERE request_id=?1 AND state IN ('reserved','delivering')",
        rusqlite::params![request_id, timestamp.saturating_add(lease_secs), timestamp],
    )? == 1)
}

#[derive(Clone, Debug, serde::Serialize)]
struct PolicyFundingAllocationEvidence {
    bucket_id: String,
    source_type: String,
    bucket_version: i64,
    reserved_nano: i64,
    charged_nano: i64,
    released_nano: i64,
    allocation_order: i64,
}

#[derive(Clone, Debug)]
struct PolicyFundingEvidence {
    allocations: Vec<PolicyFundingAllocationEvidence>,
    paid_funded_nano: i64,
    bonus_funded_nano: i64,
    other_funded_nano: i64,
    allocation_json: String,
}

fn sqlite_policy_funding_evidence(
    conn: &Connection,
    request_id: &str,
    hold_nano: i64,
    actual_nano: i64,
) -> Result<PolicyFundingEvidence> {
    if actual_nano < 0 || actual_nano > hold_nano {
        anyhow::bail!("strict settlement actual must be within the reserved hold");
    }
    let mut statement = conn.prepare(
        "SELECT allocation.bucket_id,bucket.source_type,allocation.bucket_version,
                allocation.reserved_nano,allocation.allocation_order
           FROM reservation_funding_allocations allocation
           JOIN funding_buckets bucket
             ON bucket.bucket_id=allocation.bucket_id
            AND bucket.account_id=allocation.account_id
          WHERE allocation.request_id=?1
          ORDER BY allocation.allocation_order,allocation.bucket_id",
    )?;
    let rows = statement
        .query_map(rusqlite::params![request_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let reserved_total = rows.iter().try_fold(0_i64, |total, row| {
        total
            .checked_add(row.3)
            .context("strict funding reservation total overflow")
    })?;
    if rows.is_empty() || reserved_total != hold_nano {
        anyhow::bail!("strict settlement funding allocations do not cover the hold");
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
        allocations.push(PolicyFundingAllocationEvidence {
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
        anyhow::bail!("strict settlement funding allocations do not cover the actual charge");
    }
    let allocation_json = serde_json::to_string(&allocations)?;
    Ok(PolicyFundingEvidence {
        allocations,
        paid_funded_nano,
        bonus_funded_nano,
        other_funded_nano,
        allocation_json,
    })
}

fn validate_policy_settlement(
    snapshot: &pricing::PolicyAdmissionSnapshot,
    hold_nano: i64,
    actual_nano: i64,
    usage: Option<&UsageEventInput>,
    disposition: &str,
) -> Result<()> {
    snapshot.validate()?;
    if hold_nano != snapshot.charged_hold_nano() || actual_nano < 0 || actual_nano > hold_nano {
        anyhow::bail!("strict settlement amount is outside its immutable admission hold");
    }
    let Some(usage) = usage else {
        let valid_terminal = (disposition == "cancel" && actual_nano == 0)
            || (disposition == "reconcile_full_hold" && actual_nano == hold_nano);
        if !valid_terminal {
            anyhow::bail!("strict settlement without usage must be cancel or full-hold recovery");
        }
        return Ok(());
    };
    if disposition != "settle" {
        anyhow::bail!("strict usage settlement has an invalid disposition");
    }
    if usage.provider != snapshot.provider().as_str() {
        anyhow::bail!("strict settlement provider differs from the fixed admission plane");
    }
    if usage.priced_ts != snapshot.tariff_priced_ts() {
        anyhow::bail!("strict settlement did not use the admission-pinned tariff timestamp");
    }
    if usage.model.trim().is_empty() {
        anyhow::bail!("strict settlement served model must not be empty");
    }
    for amount in [
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_tokens,
        usage.cache_write_5m_tokens,
        usage.cache_write_1h_tokens,
        usage.web_search_requests,
        usage.real_nano,
        usage.input_nano,
        usage.output_nano,
        usage.cache_read_nano,
        usage.cache_write_5m_nano,
        usage.cache_write_1h_nano,
        usage.web_search_nano,
    ] {
        if amount < 0 {
            anyhow::bail!("strict settlement usage contains a negative integer amount");
        }
    }
    if usage.real_nano > snapshot.official_hold_nano()
        || snapshot.charge_for_official_nano(usage.real_nano)? != actual_nano
    {
        anyhow::bail!("strict settlement charge does not match pinned official pricing");
    }
    Ok(())
}

fn policy_official_cost_json(
    snapshot: &pricing::PolicyAdmissionSnapshot,
    usage: Option<&UsageEventInput>,
    disposition: &str,
) -> Result<String> {
    let premium_modifiers: serde_json::Value =
        serde_json::from_str(&snapshot.premium_modifiers_json()?)?;
    let value = match usage {
        Some(usage) => serde_json::json!({
            "schema_version": 1,
            "provider": usage.provider,
            "official_nano": usage.real_nano,
            "input_nano": usage.input_nano,
            "output_nano": usage.output_nano,
            "cache_read_nano": usage.cache_read_nano,
            "cache_write_5m_nano": usage.cache_write_5m_nano,
            "cache_write_1h_nano": usage.cache_write_1h_nano,
            "web_search_nano": usage.web_search_nano,
            "premium_modifiers": premium_modifiers,
        }),
        None => serde_json::json!({
            "schema_version": 1,
            "provider": snapshot.provider().as_str(),
            "official_nano": if disposition == "reconcile_full_hold" {
                snapshot.official_hold_nano()
            } else {
                0
            },
            "disposition": disposition,
            "premium_modifiers": premium_modifiers,
        }),
    };
    Ok(serde_json::to_string(&value)?)
}

fn sqlite_write_policy_attribution(
    conn: &Connection,
    table: &str,
    request_id: &str,
    snapshot: &pricing::PolicyAdmissionSnapshot,
    usage: Option<&UsageEventInput>,
    disposition: &str,
    funding: &PolicyFundingEvidence,
) -> Result<()> {
    let predicate = match table {
        "billing_settlement_outbox" | "usage_events" => "request_id=?1",
        "ledger" => "kind='charge' AND request_id=?1",
        _ => anyhow::bail!("unsupported SQLite policy attribution target"),
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
    let official_cost_json = policy_official_cost_json(snapshot, usage, disposition)?;
    let sql = format!(
        "UPDATE {table} SET
             provider=?2,attribution_schema_version=?3,snapshot_kind='policy_v1',product_id=?4,
             account_class=?5,requested_model_id=?6,canonical_model_id=?7,served_model_id=?8,
             served_canonical_model_id=?8,billing_invariant_code=?9,alias_generation=?10,
             rule_id=?11,rule_digest=?12,rule_scope=?13,pricing_mode=?14,rule_origin=?15,
             discount_bps=?16,payable_multiplier_bp=?17,policy_id=?18,policy_version=?19,
             effective_policy_version=?20,policy_digest=?21,catalog_generation=?22,
             switch_generation=?23,tariff_schedule_id=?24,tariff_priced_ts=?25,
             official_cost_json=?26,paid_funded_nano=?27,bonus_funded_nano=?28,
             other_funded_nano=?29,funding_allocation_json=?30,track_eligible=?31,
             retention_eligible=?32,commission_eligible=?33,snapshot_digest=?34,
             source_policy_digest=?35,admission_catalog_generation=?36,
             admission_catalog_digest=?37,admission_switch_generation=?38,
             admission_switch_digest=?39,runtime_manifest_generation=?40,
             runtime_manifest_digest=?41
           WHERE {predicate}"
    );
    if conn.execute(
        &sql,
        rusqlite::params![
            request_id,
            snapshot.provider().as_str(),
            pricing::POLICY_ADMISSION_SNAPSHOT_SCHEMA_VERSION,
            snapshot.product_id(),
            snapshot.account_class().as_str(),
            snapshot.requested_model_id(),
            snapshot.canonical_model_id(),
            served_model,
            invariant,
            snapshot.alias_generation(),
            snapshot.rule_id(),
            snapshot.rule_digest(),
            rule_scope,
            snapshot.pricing_mode().as_str(),
            snapshot.rule_origin.as_str(),
            snapshot.discount_bps,
            snapshot.payable_multiplier_bp(),
            snapshot.policy_id(),
            snapshot.policy_version(),
            snapshot.effective_policy_version(),
            snapshot.policy_digest(),
            snapshot.policy_catalog_generation(),
            snapshot.policy_switch_generation(),
            snapshot.tariff_schedule_id(),
            snapshot.tariff_priced_ts(),
            official_cost_json,
            funding.paid_funded_nano,
            funding.bonus_funded_nano,
            funding.other_funded_nano,
            funding.allocation_json,
            i64::from(snapshot.track_eligible()),
            i64::from(snapshot.retention_eligible()),
            i64::from(snapshot.commission_eligible()),
            snapshot.snapshot_digest(),
            snapshot.source_policy_digest(),
            snapshot.admission_catalog_generation(),
            snapshot.admission_catalog_digest(),
            snapshot.admission_switch_generation(),
            snapshot.admission_switch_digest(),
            snapshot.runtime_manifest_generation(),
            snapshot.runtime_manifest_digest(),
        ],
    )? != 1
    {
        anyhow::bail!("SQLite policy attribution target row is missing");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn sqlite_enqueue_settlement(
    conn: &Connection,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
    disposition: &str,
) -> Result<Option<i64>> {
    if !matches!(disposition, "settle" | "cancel" | "reconcile_full_hold") {
        anyhow::bail!("invalid SQLite settlement disposition");
    }
    let usage_json = usage.map(serde_json::to_string).transpose()?;
    let tx = conn.unchecked_transaction()?;
    let reservation = tx.query_row(
        "SELECT account_id,key,hold_nano,state,actual_nano,balance_after_settle_nano,reference \
         FROM billing_reservations WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        },
    );
    let (stored_account, stored_key, stored_hold, state, stored_actual, stored_balance, stored_ref) =
        match reservation {
            Ok(row) => row,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                anyhow::bail!("settlement has no durable reservation")
            }
            Err(error) => return Err(error.into()),
        };
    if stored_account != account_id || stored_key != key || stored_hold != hold_nano {
        anyhow::bail!("settlement parameters do not match reservation");
    }
    let policy_snapshot = match pricing::sqlite_policy_snapshot_lookup(&tx, request_id)? {
        pricing::PolicySnapshotLookup::Policy(snapshot) => Some(*snapshot),
        pricing::PolicySnapshotLookup::Missing | pricing::PolicySnapshotLookup::NonPolicy => None,
    };
    let actual = if let Some(snapshot) = policy_snapshot.as_ref() {
        validate_policy_settlement(snapshot, stored_hold, actual_nano, usage, disposition)?;
        actual_nano
    } else {
        actual_nano.max(0)
    };
    if state == "settled" {
        if stored_actual == Some(actual) && stored_ref.as_deref() == reference {
            tx.commit()?;
            return Ok(stored_balance);
        }
        anyhow::bail!("settlement request ID was reused with different parameters");
    }

    let timestamp = now();
    let inserted = tx.execute(
        "INSERT INTO billing_settlement_outbox( \
           request_id,actual_nano,reference,usage_json,disposition,state,attempts,next_attempt_ts,
           created_ts,updated_ts) \
         VALUES(?1,?2,?3,?4,?5,'pending',0,0,?6,?6) ON CONFLICT(request_id) DO NOTHING",
        rusqlite::params![
            request_id,
            actual,
            reference,
            usage_json,
            disposition,
            timestamp
        ],
    )?;
    if inserted == 0 {
        let existing = tx.query_row(
            "SELECT actual_nano,reference,usage_json,disposition
               FROM billing_settlement_outbox WHERE request_id=?1",
            rusqlite::params![request_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;
        if existing
            != (
                actual,
                reference.map(str::to_owned),
                usage_json,
                disposition.to_owned(),
            )
        {
            anyhow::bail!("settlement request ID was reused with different parameters");
        }
    }
    if let Some(snapshot) = policy_snapshot.as_ref() {
        let funding = sqlite_policy_funding_evidence(&tx, request_id, stored_hold, actual)?;
        sqlite_write_policy_attribution(
            &tx,
            "billing_settlement_outbox",
            request_id,
            snapshot,
            usage,
            disposition,
            &funding,
        )?;
    }
    tx.commit()?;
    Ok(None)
}

fn sqlite_process_policy_settlement(
    conn: &Connection,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
    disposition: &str,
    snapshot: &pricing::PolicyAdmissionSnapshot,
) -> Result<i64> {
    validate_policy_settlement(snapshot, hold_nano, actual_nano, usage, disposition)?;
    let funding = sqlite_policy_funding_evidence(conn, request_id, hold_nano, actual_nano)?;
    let released_total = hold_nano - actual_nano;
    let balance: i64 = conn
        .query_row(
            "UPDATE accounts
            SET balance_nano=balance_nano+?1,spent_nano=spent_nano+?2,
                reserved_nano=reserved_nano-?3
          WHERE id=?4 AND reserved_nano>=?3
          RETURNING balance_nano",
            rusqlite::params![released_total, actual_nano, hold_nano, account_id],
            |row| row.get(0),
        )
        .context("strict SQLite reservation/account aggregate invariant failed")?;
    let key_updated = conn.execute(
        "UPDATE api_keys
            SET spent_nano=spent_nano+?1,reserved_nano=reserved_nano-?2
          WHERE key=?3 AND account_id=?4 AND reserved_nano>=?2",
        rusqlite::params![actual_nano, hold_nano, key, account_id],
    )?;
    if key_updated != 1 {
        let key_still_exists = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM api_keys WHERE key=?1)",
            rusqlite::params![key],
            |row| row.get::<_, bool>(0),
        )?;
        if key_still_exists {
            anyhow::bail!("strict SQLite reservation/key aggregate invariant failed");
        }
    }

    let timestamp = now();
    for allocation in &funding.allocations {
        let next_version: i64 = conn
            .query_row(
                "UPDATE funding_buckets
                SET balance_nano=balance_nano+?1,reserved_nano=reserved_nano-?2,
                    spent_nano=spent_nano+?3,version=version+1,updated_ts=?4,
                    status=CASE
                      WHEN status='retired' THEN status
                      WHEN balance_nano+?1>0 THEN 'active'
                      ELSE 'exhausted'
                    END
              WHERE bucket_id=?5 AND account_id=?6 AND reserved_nano>=?2
              RETURNING version",
                rusqlite::params![
                    allocation.released_nano,
                    allocation.reserved_nano,
                    allocation.charged_nano,
                    timestamp,
                    allocation.bucket_id,
                    account_id,
                ],
                |row| row.get(0),
            )
            .with_context(|| {
                format!(
                    "strict SQLite funding bucket {} invariant failed",
                    allocation.bucket_id
                )
            })?;
        if next_version <= allocation.bucket_version {
            anyhow::bail!("strict SQLite funding bucket version did not advance");
        }
        if conn.execute(
            "UPDATE reservation_funding_allocations
                SET charged_nano=?1,released_nano=?2
              WHERE request_id=?3 AND account_id=?4 AND bucket_id=?5
                AND charged_nano IS NULL AND released_nano IS NULL",
            rusqlite::params![
                allocation.charged_nano,
                allocation.released_nano,
                request_id,
                account_id,
                allocation.bucket_id,
            ],
        )? != 1
        {
            anyhow::bail!("strict SQLite funding allocation was already terminalized");
        }
    }

    if let Some(usage) = usage {
        let ledger_id: i64 = conn.query_row(
            "INSERT INTO ledger(
                 account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts,model,
                 provider,official_nano)
             VALUES(?1,?2,'charge',?3,?4,?5,?6,?7,NULLIF(?8,''),?9,?10)
             RETURNING id",
            rusqlite::params![
                account_id,
                key,
                request_id,
                actual_nano,
                reference,
                balance,
                timestamp,
                usage.model,
                usage.provider,
                usage.real_nano,
            ],
            |row| row.get(0),
        )?;
        sqlite_write_policy_attribution(
            conn,
            "ledger",
            request_id,
            snapshot,
            Some(usage),
            disposition,
            &funding,
        )?;
        for allocation in funding
            .allocations
            .iter()
            .filter(|allocation| allocation.charged_nano > 0)
        {
            conn.execute(
                "INSERT INTO ledger_funding_allocations(
                     ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
                     direction,amount_nano)
                 VALUES(?1,?2,?3,?4,?5,'debit',?6)",
                rusqlite::params![
                    ledger_id,
                    account_id,
                    allocation.bucket_id,
                    allocation.source_type,
                    allocation.bucket_version,
                    allocation.charged_nano,
                ],
            )?;
        }
        conn.execute(
            "INSERT INTO usage_events(
                 request_id,account_id,key,model,input_tokens,output_tokens,cache_read_tokens,
                 cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests,real_nano,
                 charge_nano,ref,ts,speed,inference_geo,input_nano,output_nano,cache_read_nano,
                 cache_write_5m_nano,cache_write_1h_nano,web_search_nano,priced_ts,provider)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,
                    ?17,?18,?19,?20,?21,?22,?23,?24)",
            rusqlite::params![
                request_id,
                account_id,
                key,
                usage.model,
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_write_5m_tokens,
                usage.cache_write_1h_tokens,
                usage.web_search_requests,
                usage.real_nano,
                actual_nano,
                reference,
                timestamp,
                usage.speed,
                usage.inference_geo,
                usage.input_nano,
                usage.output_nano,
                usage.cache_read_nano,
                usage.cache_write_5m_nano,
                usage.cache_write_1h_nano,
                usage.web_search_nano,
                usage.priced_ts,
                usage.provider,
            ],
        )?;
        sqlite_write_policy_attribution(
            conn,
            "usage_events",
            request_id,
            snapshot,
            Some(usage),
            disposition,
            &funding,
        )?;
    } else if actual_nano > 0 {
        let ledger_id: i64 = conn.query_row(
            "INSERT INTO ledger(
                 account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts,model,
                 provider,official_nano)
             VALUES(?1,?2,'charge',?3,?4,?5,?6,?7,?8,?9,?10)
             RETURNING id",
            rusqlite::params![
                account_id,
                key,
                request_id,
                actual_nano,
                reference,
                balance,
                timestamp,
                snapshot.canonical_model_id(),
                snapshot.provider().as_str(),
                snapshot.official_hold_nano(),
            ],
            |row| row.get(0),
        )?;
        sqlite_write_policy_attribution(
            conn,
            "ledger",
            request_id,
            snapshot,
            None,
            disposition,
            &funding,
        )?;
        for allocation in funding
            .allocations
            .iter()
            .filter(|allocation| allocation.charged_nano > 0)
        {
            conn.execute(
                "INSERT INTO ledger_funding_allocations(
                     ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
                     direction,amount_nano)
                 VALUES(?1,?2,?3,?4,?5,'debit',?6)",
                rusqlite::params![
                    ledger_id,
                    account_id,
                    allocation.bucket_id,
                    allocation.source_type,
                    allocation.bucket_version,
                    allocation.charged_nano,
                ],
            )?;
        }
    }
    sqlite_write_policy_attribution(
        conn,
        "billing_settlement_outbox",
        request_id,
        snapshot,
        usage,
        disposition,
        &funding,
    )?;
    Ok(balance)
}

/// Apply one already-durable SQLite settlement intent atomically with its ledger/usage rows.
pub fn sqlite_process_settlement(conn: &Connection, request_id: &str) -> Result<Option<i64>> {
    let tx = conn.unchecked_transaction()?;
    let outbox = tx.query_row(
        "SELECT actual_nano,reference,usage_json,state,disposition
           FROM billing_settlement_outbox WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        },
    );
    let (actual, reference, usage_json, outbox_state, disposition) = match outbox {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => anyhow::bail!("settlement outbox row missing"),
        Err(error) => return Err(error.into()),
    };
    let reservation = tx.query_row(
        "SELECT account_id,key,hold_nano,state,actual_nano,balance_after_settle_nano \
         FROM billing_reservations WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        },
    )?;
    if matches!(reservation.3.as_str(), "settled" | "canceled") || outbox_state == "done" {
        if reservation.4 != Some(actual) {
            anyhow::bail!("stored settlement differs from outbox");
        }
        tx.execute(
            "UPDATE billing_settlement_outbox SET state='done',committed_ts=COALESCE(committed_ts,?2), \
             updated_ts=?2,last_error=NULL WHERE request_id=?1",
            rusqlite::params![request_id, now()],
        )?;
        tx.commit()?;
        return Ok(reservation.5);
    }
    if reservation.3 != "reserved" && reservation.3 != "delivering" {
        anyhow::bail!("reservation is not settleable");
    }
    let usage = usage_json
        .as_deref()
        .map(serde_json::from_str::<UsageEventInput>)
        .transpose()?;
    let policy_snapshot = match pricing::sqlite_policy_snapshot_lookup(&tx, request_id)? {
        pricing::PolicySnapshotLookup::Policy(snapshot) => Some(*snapshot),
        pricing::PolicySnapshotLookup::Missing | pricing::PolicySnapshotLookup::NonPolicy => None,
    };
    let balance = if let Some(snapshot) = policy_snapshot.as_ref() {
        sqlite_process_policy_settlement(
            &tx,
            request_id,
            &reservation.0,
            &reservation.1,
            reservation.2,
            actual,
            reference.as_deref(),
            usage.as_ref(),
            &disposition,
            snapshot,
        )?
    } else {
        account_settle_in(
            &tx,
            &reservation.0,
            &reservation.1,
            reservation.2,
            actual,
            reference.as_deref(),
            usage.as_ref(),
        )?
        .ok_or_else(|| anyhow::anyhow!("settlement account no longer exists"))?
    };
    let timestamp = now();
    let final_state = if policy_snapshot.is_some() && disposition == "cancel" {
        "canceled"
    } else {
        "settled"
    };
    tx.execute(
        "UPDATE billing_reservations SET state=?2,actual_nano=?3, \
         balance_after_settle_nano=?4,reference=?5,updated_ts=?6,settled_ts=?6 \
         WHERE request_id=?1 AND state IN ('reserved','delivering')",
        rusqlite::params![
            request_id,
            final_state,
            actual,
            balance,
            reference,
            timestamp
        ],
    )?;
    tx.execute(
        "UPDATE billing_settlement_outbox SET state='done',attempts=attempts+1, \
         updated_ts=?2,committed_ts=?2,last_error=NULL WHERE request_id=?1",
        rusqlite::params![request_id, timestamp],
    )?;
    tx.commit()?;
    Ok(Some(balance))
}

#[allow(clippy::too_many_arguments)]
pub fn sqlite_settle_request(
    conn: &Connection,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
) -> Result<Option<i64>> {
    if let Some(balance) = sqlite_enqueue_settlement(
        conn,
        request_id,
        account_id,
        key,
        hold_nano,
        actual_nano,
        reference,
        usage,
        "settle",
    )? {
        return Ok(Some(balance));
    }
    match sqlite_process_settlement(conn, request_id) {
        Ok(result) => Ok(result),
        Err(error) => {
            let message: String = format!("{error:#}").chars().take(1000).collect();
            let timestamp = now();
            let _ = conn.execute(
                "UPDATE billing_settlement_outbox SET attempts=attempts+1,last_error=?2, \
                 updated_ts=?3,next_attempt_ts=?3+MIN(60,MAX(1,attempts+1)) WHERE request_id=?1",
                rusqlite::params![request_id, message, timestamp],
            );
            Err(error)
        }
    }
}

/// Persist and apply an explicit cancellation. Strict policy reservations distinguish this from a
/// zero-value usage settlement so the immutable snapshot and funding allocations can be validated
/// and returned to their original buckets without weakening the settlement contract.
#[allow(clippy::too_many_arguments)]
pub fn sqlite_cancel_request(
    conn: &Connection,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
) -> Result<Option<i64>> {
    if let Some(balance) = sqlite_enqueue_settlement(
        conn, request_id, account_id, key, hold_nano, 0, None, None, "cancel",
    )? {
        return Ok(Some(balance));
    }
    match sqlite_process_settlement(conn, request_id) {
        Ok(result) => Ok(result),
        Err(error) => {
            let message: String = format!("{error:#}").chars().take(1000).collect();
            let timestamp = now();
            let _ = conn.execute(
                "UPDATE billing_settlement_outbox SET attempts=attempts+1,last_error=?2, \
                 updated_ts=?3,next_attempt_ts=?3+MIN(60,MAX(1,attempts+1)) WHERE request_id=?1",
                rusqlite::params![request_id, message, timestamp],
            );
            Err(error)
        }
    }
}

/// Retry persisted intents, then reconcile expired holds. Reserved requests are canceled; requests
/// marked delivering are charged their approved hold when exact usage never arrived.
pub fn sqlite_reconcile_expired(
    conn: &Connection,
    limit: usize,
) -> Result<crate::pg::ReconcileReport> {
    let limit = limit.clamp(1, 10_000) as i64;
    let timestamp = now();
    let pending: Vec<String> = {
        let mut statement = conn.prepare(
            "SELECT request_id FROM billing_settlement_outbox \
             WHERE state='pending' AND next_attempt_ts<=?1 ORDER BY created_ts LIMIT ?2",
        )?;
        let rows = statement
            .query_map(rusqlite::params![timestamp, limit], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };
    let mut report = crate::pg::ReconcileReport::default();
    for request_id in pending {
        if sqlite_process_settlement(conn, &request_id).is_ok() {
            report.processed_outbox += 1;
        }
    }

    let remaining = (limit as usize).saturating_sub(report.processed_outbox) as i64;
    if remaining == 0 {
        return Ok(report);
    }
    let expired: Vec<(String, String, String, i64, String)> = {
        let mut statement = conn.prepare(
            "SELECT request_id,account_id,key,hold_nano,state FROM billing_reservations \
             WHERE state IN ('reserved','delivering') AND lease_until<=?1 \
             ORDER BY lease_until LIMIT ?2",
        )?;
        let rows = statement
            .query_map(rusqlite::params![timestamp, remaining], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };
    for (request_id, account_id, key, hold, state) in expired {
        let actual = if state == "delivering" { hold } else { 0 };
        let disposition = if state == "delivering" {
            "reconcile_full_hold"
        } else {
            "cancel"
        };
        let reference = if state == "delivering" {
            "lease-expired-delivering"
        } else {
            "lease-expired-reserved"
        };
        let result = sqlite_enqueue_settlement(
            conn,
            &request_id,
            &account_id,
            &key,
            hold,
            actual,
            Some(reference),
            None,
            disposition,
        )
        .and_then(|balance| match balance {
            Some(balance) => Ok(Some(balance)),
            None => sqlite_process_settlement(conn, &request_id),
        });
        match result {
            Ok(_) if state == "delivering" => report.charged_after_delivery += 1,
            Ok(_) => report.canceled_before_delivery += 1,
            Err(error) => {
                eprintln!("SQLite reservation recovery failed for {request_id}: {error:#}")
            }
        }
    }
    Ok(report)
}

pub fn sqlite_maintenance_prune(
    conn: &Connection,
    older_than_ts: i64,
) -> Result<crate::pg::MaintenanceReport> {
    pricing::validate_request_lifecycle_prune_cutoff(older_than_ts, now())?;
    let tx = conn.unchecked_transaction()?;
    let outbox = tx.execute(
        "DELETE FROM billing_settlement_outbox WHERE request_id IN ( \
           SELECT request_id FROM billing_settlement_outbox WHERE state='done' AND committed_ts<?1 \
           ORDER BY committed_ts,request_id LIMIT 5000)",
        rusqlite::params![older_than_ts],
    )?;
    let (pricing_snapshots_cascaded, pricing_shadow_evaluations_cascaded) = tx.query_row(
        "WITH doomed AS ( \
           SELECT r.request_id FROM billing_reservations r \
           WHERE r.state IN ('settled','canceled') AND r.settled_ts<?1 \
             AND r.request_id NOT IN (SELECT request_id FROM billing_settlement_outbox) \
           ORDER BY r.settled_ts,r.request_id LIMIT 5000 \
         ) \
         SELECT \
           (SELECT COUNT(*) FROM pricing_admission_snapshots s \
             WHERE EXISTS (SELECT 1 FROM doomed d WHERE d.request_id=s.request_id)), \
           (SELECT COUNT(*) FROM pricing_shadow_admission_evaluations e \
             WHERE EXISTS (SELECT 1 FROM doomed d WHERE d.request_id=e.request_id))",
        rusqlite::params![older_than_ts],
        |row| {
            Ok((
                row.get::<_, i64>(0)? as usize,
                row.get::<_, i64>(1)? as usize,
            ))
        },
    )?;
    let reservations = tx.execute(
        "DELETE FROM billing_reservations WHERE request_id IN ( \
           SELECT request_id FROM billing_reservations
            WHERE state IN ('settled','canceled') AND settled_ts<?1 \
             AND request_id NOT IN (SELECT request_id FROM billing_settlement_outbox) \
           ORDER BY settled_ts,request_id LIMIT 5000)",
        rusqlite::params![older_than_ts],
    )?;
    tx.commit()?;
    Ok(crate::pg::MaintenanceReport {
        outbox,
        reservations,
        pricing_snapshots_cascaded,
        pricing_shadow_evaluations_cascaded,
        ..Default::default()
    })
}

/// Записать usage-событие (аналитика; НЕ money-строка). Вызывается billing-writer'ом сразу после
/// `account_settle` на той же connection. `charge_nano` — фактически списанное (после наценки).
pub fn usage_event_add(
    conn: &Connection,
    account_id: &str,
    key: Option<&str>,
    u: &UsageEventInput,
    charge_nano: i64,
    reference: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO usage_events(account_id, key, model, input_tokens, output_tokens, \
         cache_read_tokens, cache_write_5m_tokens, cache_write_1h_tokens, web_search_requests, \
         real_nano, charge_nano, ref, ts, speed, inference_geo, input_nano, output_nano, \
         cache_read_nano, cache_write_5m_nano, cache_write_1h_nano, web_search_nano, priced_ts, \
         provider) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
        rusqlite::params![
            account_id,
            key,
            u.model,
            u.input_tokens,
            u.output_tokens,
            u.cache_read_tokens,
            u.cache_write_5m_tokens,
            u.cache_write_1h_tokens,
            u.web_search_requests,
            u.real_nano,
            charge_nano,
            reference,
            now(),
            u.speed,
            u.inference_geo,
            u.input_nano,
            u.output_nano,
            u.cache_read_nano,
            u.cache_write_5m_nano,
            u.cache_write_1h_nano,
            u.web_search_nano,
            u.priced_ts,
            u.provider
        ],
    )?;
    Ok(())
}

/// Агрегат usage по модели за окно. Суммы токенов по корзинам + immutable real/charge nano
/// + число тарифицируемых событий.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageModelAgg {
    pub model: String,
    pub provider: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_5m_tokens: i64,
    pub cache_write_1h_tokens: i64,
    pub web_search_requests: i64,
    pub real_nano: i64,
    pub charge_nano: i64,
    pub input_nano: i64,
    pub output_nano: i64,
    pub cache_read_nano: i64,
    pub cache_write_5m_nano: i64,
    pub cache_write_1h_nano: i64,
    pub web_search_nano: i64,
}

/// Точный дневной срез того же usage-окна. `day_ts` — начало UTC-дня в unix-секундах.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageDailyAgg {
    pub day_ts: i64,
    pub requests: i64,
    pub real_nano: i64,
    pub charge_nano: i64,
}

/// Точный дневной срез по фактически обслужившему API-плану. Имя модели намеренно
/// не участвует: один и тот же model ID может маршрутизироваться разными провайдерами.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageDailyProviderAgg {
    pub day_ts: i64,
    pub provider: String,
    pub requests: i64,
    pub real_nano: i64,
    pub charge_nano: i64,
}

/// Точный per-key срез usage-окна. Полный ключ остаётся внутри engine-процесса и маскируется
/// HTTP-слоем до ответа control API.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageKeyAgg {
    pub key: Option<String>,
    pub requests: i64,
    pub real_nano: i64,
    pub charge_nano: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageReport {
    pub models: Vec<UsageModelAgg>,
    pub daily: Vec<UsageDailyAgg>,
    pub daily_providers: Vec<UsageDailyProviderAgg>,
    pub keys: Vec<UsageKeyAgg>,
}

fn usage_by_model_between(
    conn: &Connection,
    account_id: &str,
    since_ts: i64,
    until_ts: i64,
) -> Result<Vec<UsageModelAgg>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(model,''), COALESCE(NULLIF(provider,''),'anthropic'), COUNT(*), \
         COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), \
         COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_5m_tokens),0), \
         COALESCE(SUM(cache_write_1h_tokens),0), COALESCE(SUM(web_search_requests),0), \
         COALESCE(SUM(real_nano),0), COALESCE(SUM(charge_nano),0), \
         COALESCE(SUM(input_nano),0), COALESCE(SUM(output_nano),0), \
         COALESCE(SUM(cache_read_nano),0), COALESCE(SUM(cache_write_5m_nano),0), \
         COALESCE(SUM(cache_write_1h_nano),0), COALESCE(SUM(web_search_nano),0) \
         FROM usage_events WHERE account_id=?1 AND ts>=?2 AND ts<?3 \
         GROUP BY model, COALESCE(NULLIF(provider,''),'anthropic') ORDER BY SUM(real_nano) DESC, model, COALESCE(NULLIF(provider,''),'anthropic')",
    )?;
    let rows = stmt.query_map(rusqlite::params![account_id, since_ts, until_ts], |r| {
        Ok(UsageModelAgg {
            model: r.get(0)?,
            provider: r.get(1)?,
            requests: r.get(2)?,
            input_tokens: r.get(3)?,
            output_tokens: r.get(4)?,
            cache_read_tokens: r.get(5)?,
            cache_write_5m_tokens: r.get(6)?,
            cache_write_1h_tokens: r.get(7)?,
            web_search_requests: r.get(8)?,
            real_nano: r.get(9)?,
            charge_nano: r.get(10)?,
            input_nano: r.get(11)?,
            output_nano: r.get(12)?,
            cache_read_nano: r.get(13)?,
            cache_write_5m_nano: r.get(14)?,
            cache_write_1h_nano: r.get(15)?,
            web_search_nano: r.get(16)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn usage_by_model(
    conn: &Connection,
    account_id: &str,
    since_ts: i64,
) -> Result<Vec<UsageModelAgg>> {
    usage_by_model_between(conn, account_id, since_ts, i64::MAX)
}

/// Один согласованный usage-отчёт на полуинтервале `[since_ts, until_ts)`. Все три среза
/// читаются из одного snapshot, поэтому параллельный settle не может попасть только в часть отчёта.
pub fn usage_report(
    conn: &Connection,
    account_id: &str,
    since_ts: i64,
    until_ts: i64,
) -> Result<UsageReport> {
    if until_ts <= since_ts {
        return Ok(UsageReport::default());
    }
    let transaction = conn.unchecked_transaction()?;
    let models = usage_by_model_between(&transaction, account_id, since_ts, until_ts)?;
    let daily = {
        let mut stmt = transaction.prepare(
            "SELECT (ts / 86400) * 86400 AS day_ts, COUNT(*), \
             COALESCE(SUM(real_nano),0), COALESCE(SUM(charge_nano),0) \
             FROM usage_events WHERE account_id=?1 AND ts>=?2 AND ts<?3 \
             GROUP BY day_ts ORDER BY day_ts",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![account_id, since_ts, until_ts], |r| {
                Ok(UsageDailyAgg {
                    day_ts: r.get(0)?,
                    requests: r.get(1)?,
                    real_nano: r.get(2)?,
                    charge_nano: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    let daily_providers = {
        let mut stmt = transaction.prepare(
            "SELECT (ts / 86400) * 86400 AS day_ts, COALESCE(NULLIF(provider,''),'anthropic'), COUNT(*), \
             COALESCE(SUM(real_nano),0), COALESCE(SUM(charge_nano),0) \
             FROM usage_events WHERE account_id=?1 AND ts>=?2 AND ts<?3 \
             GROUP BY day_ts, COALESCE(NULLIF(provider,''),'anthropic') ORDER BY day_ts, COALESCE(NULLIF(provider,''),'anthropic')",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![account_id, since_ts, until_ts], |r| {
                Ok(UsageDailyProviderAgg {
                    day_ts: r.get(0)?,
                    provider: r.get(1)?,
                    requests: r.get(2)?,
                    real_nano: r.get(3)?,
                    charge_nano: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    let keys = {
        let mut stmt = transaction.prepare(
            "SELECT key, COUNT(*), COALESCE(SUM(real_nano),0), COALESCE(SUM(charge_nano),0) \
             FROM usage_events WHERE account_id=?1 AND ts>=?2 AND ts<?3 \
             GROUP BY key ORDER BY SUM(real_nano) DESC, key",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![account_id, since_ts, until_ts], |r| {
                Ok(UsageKeyAgg {
                    key: r.get(0)?,
                    requests: r.get(1)?,
                    real_nano: r.get(2)?,
                    charge_nano: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    transaction.commit()?;
    Ok(UsageReport {
        models,
        daily,
        daily_providers,
        keys,
    })
}

/// Агрегат расхода ПО АККАУНТАМ за окно (ts ≥ `since_ts`): списано клиенту (charge) +
/// real-API стоимость + число запросов. Для панели «кто тратит» (сегодня/7д/30д).
#[derive(Debug, Clone, Default)]
pub struct SpendAccountAgg {
    pub account_id: String,
    pub handle: String,
    pub requests: i64,
    pub charge_nano: i64,
    pub real_nano: i64,
    pub last_ts: i64,
}

pub fn spend_by_account(
    conn: &Connection,
    since_ts: i64,
    limit: i64,
) -> Result<Vec<SpendAccountAgg>> {
    spend_by_account_range(conn, since_ts, i64::MAX, limit)
}

/// То же с явной верхней границей: полуоткрытое окно `since_ts ≤ ts < until_ts` (стыкующиеся
/// диапазоны не задваивают события). Для произвольного диапазона панели (/spend-stats?from&to).
pub fn spend_by_account_range(
    conn: &Connection,
    since_ts: i64,
    until_ts: i64,
    limit: i64,
) -> Result<Vec<SpendAccountAgg>> {
    let mut stmt = conn.prepare(
        "SELECT u.account_id, COALESCE(a.handle,''), COUNT(*), \
         COALESCE(SUM(u.charge_nano),0), COALESCE(SUM(u.real_nano),0), COALESCE(MAX(u.ts),0) \
         FROM usage_events u LEFT JOIN accounts a ON a.id=u.account_id \
         WHERE u.ts>=?1 AND u.ts<?2 GROUP BY u.account_id, a.handle \
         ORDER BY SUM(u.charge_nano) DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(rusqlite::params![since_ts, until_ts, limit], |r| {
        Ok(SpendAccountAgg {
            account_id: r.get(0)?,
            handle: r.get(1)?,
            requests: r.get(2)?,
            charge_nano: r.get(3)?,
            real_nano: r.get(4)?,
            last_ts: r.get(5)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Расход по ПРОВАЙДЕРУ за окно (ts ≥ `since_ts`). Claude-флот и Codex-пул сеттлятся в одни и те же
/// денежные таблицы, поэтому «сколько заработал каждый апстрим» читается только из явной колонки.
#[derive(Debug, Clone, Default)]
pub struct SpendProviderAgg {
    pub provider: String,
    pub requests: i64,
    pub charge_nano: i64,
    pub real_nano: i64,
}

pub fn spend_by_provider(conn: &Connection, since_ts: i64) -> Result<Vec<SpendProviderAgg>> {
    spend_by_provider_range(conn, since_ts, i64::MAX)
}

/// То же с явной верхней границей окна: `since_ts ≤ ts < until_ts` — см. spend_by_account_range.
pub fn spend_by_provider_range(
    conn: &Connection,
    since_ts: i64,
    until_ts: i64,
) -> Result<Vec<SpendProviderAgg>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(NULLIF(provider,''),'anthropic'), COUNT(*), \
         COALESCE(SUM(charge_nano),0), COALESCE(SUM(real_nano),0) \
         FROM usage_events WHERE ts>=?1 AND ts<?2 GROUP BY 1 ORDER BY SUM(charge_nano) DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![since_ts, until_ts], |r| {
        Ok(SpendProviderAgg {
            provider: r.get(0)?,
            requests: r.get(1)?,
            charge_nano: r.get(2)?,
            real_nano: r.get(3)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Расход по МОДЕЛИ за окно (ts ≥ `since_ts`): top-`limit` по charge. Группировка по
/// (model, provider): один и тот же model ID может обслуживаться разными апстримами (см.
/// UsageDailyProviderAgg). `model` в usage_events — served id из ответа апстрима, по которому
/// реально посчитан charge (фолбэк — модель запроса), то есть разбивка отражает прайсинг,
/// а не клиентский алиас.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpendModelAgg {
    pub model: String,
    pub provider: String,
    pub requests: i64,
    pub charge_nano: i64,
    pub real_nano: i64,
}

pub fn spend_by_model(conn: &Connection, since_ts: i64, limit: i64) -> Result<Vec<SpendModelAgg>> {
    spend_by_model_range(conn, since_ts, i64::MAX, limit)
}

/// То же с явной верхней границей окна: `since_ts ≤ ts < until_ts` — см. spend_by_account_range.
pub fn spend_by_model_range(
    conn: &Connection,
    since_ts: i64,
    until_ts: i64,
    limit: i64,
) -> Result<Vec<SpendModelAgg>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(NULLIF(model,''),'(unknown)'), COALESCE(NULLIF(provider,''),'anthropic'), \
         COUNT(*), COALESCE(SUM(charge_nano),0), COALESCE(SUM(real_nano),0) \
         FROM usage_events WHERE ts>=?1 AND ts<?2 GROUP BY 1,2 ORDER BY SUM(charge_nano) DESC, 1, 2 \
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(rusqlite::params![since_ts, until_ts, limit], |r| {
        Ok(SpendModelAgg {
            model: r.get(0)?,
            provider: r.get(1)?,
            requests: r.get(2)?,
            charge_nano: r.get(3)?,
            real_nano: r.get(4)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Одна failed-строка settlement_outbox для операционной диагностики. `last_error` обрезан до
/// 200 символов: тексты ошибок settle — внутренние invariant/SQLSTATE детали (request_id, суммы,
/// имена constraint'ов), токенов подписок и ключей там нет, но длинный PG-trace не должен
/// раздувать ответ панели.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettlementFailure {
    pub request_id: String,
    pub actual_nano: i64,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub updated_ts: i64,
}

/// Лаг durable-консьюмера ledger'а: max(ledger.id) против watermark'ов
/// `ledger_consumer_checkpoints` + возраст старейшей неподтверждённой строки. Растущий `unacked`
/// означает, что коммерческий pricing-воркер не дочитывает списания (и ledger_prune остановлен).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LedgerConsumerLag {
    pub consumer: String,
    pub ledger_max_id: i64,
    /// Число (consumer, account_id) watermark'ов; 0 → консьюмер ни разу не подтверждал.
    pub checkpoints: i64,
    /// Минимальный last_ledger_id среди watermark'ов (0, когда checkpoint'ов нет).
    pub checkpoint_min: i64,
    /// Ledger-строки с id > watermark'а своего аккаунта.
    pub unacked: i64,
    /// ts старейшей неподтверждённой строки (0 — лага нет).
    pub oldest_unacked_ts: i64,
}

/// Сводка settlement pipeline для панели «тихие деньги»: counts по state, failed всего и за 24ч,
/// backlog несеттленых старше порога, последние ≤10 failed, лаг pricing-консьюмера. Читается
/// одинаково на обоих backend'ах: у SQLite-зеркала state 'failed' нет (застревшие ретраи видны
/// как `pending_with_error`), PostgreSQL паркует permanent-ошибки в 'failed' (миграция 0004);
/// state 'processing' объявлен в схеме, но пока не пишется ни одним writer'ом.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettlementHealth {
    pub pending: i64,
    pub processing: i64,
    pub done: i64,
    pub failed: i64,
    pub failed_24h: i64,
    /// pending-строки с last_error — ретраи в полёте (единственный сигнал застревания на SQLite).
    pub pending_with_error: i64,
    /// Несеттленые (pending|processing), созданные раньше `backlog_before`.
    pub backlog: i64,
    /// created_ts старейшей несеттленой строки (0 — несеттленых нет).
    pub oldest_unsettled_ts: i64,
    pub recent_failed: Vec<SettlementFailure>,
    pub ledger_consumer: LedgerConsumerLag,
}

fn settlement_consumer_lag(conn: &Connection, consumer: &str) -> Result<LedgerConsumerLag> {
    let ledger_max_id: i64 =
        conn.query_row("SELECT COALESCE(MAX(id),0) FROM ledger", [], |r| r.get(0))?;
    let (checkpoints, checkpoint_min): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(MIN(last_ledger_id),0) \
         FROM ledger_consumer_checkpoints WHERE consumer=?1",
        rusqlite::params![consumer],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (unacked, oldest_unacked_ts): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(MIN(l.ts),0) FROM ledger l \
         JOIN ledger_consumer_checkpoints c ON c.account_id=l.account_id AND c.consumer=?1 \
         WHERE l.id > c.last_ledger_id",
        rusqlite::params![consumer],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(LedgerConsumerLag {
        consumer: consumer.to_string(),
        ledger_max_id,
        checkpoints,
        checkpoint_min,
        unacked,
        oldest_unacked_ts,
    })
}

pub fn settlement_health(
    conn: &Connection,
    backlog_secs: i64,
    consumer: &str,
) -> Result<SettlementHealth> {
    let ts = now();
    let backlog_before = ts - backlog_secs.max(0);
    let failed_since = ts - 86_400;
    let mut health = SettlementHealth::default();
    {
        let mut stmt =
            conn.prepare("SELECT state, COUNT(*) FROM billing_settlement_outbox GROUP BY state")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (state, count) = row?;
            match state.as_str() {
                "pending" => health.pending = count,
                "processing" => health.processing = count,
                "done" => health.done = count,
                "failed" => health.failed = count,
                _ => {}
            }
        }
    }
    health.failed_24h = conn.query_row(
        "SELECT COUNT(*) FROM billing_settlement_outbox WHERE state='failed' AND updated_ts>=?1",
        rusqlite::params![failed_since],
        |r| r.get(0),
    )?;
    health.pending_with_error = conn.query_row(
        "SELECT COUNT(*) FROM billing_settlement_outbox \
         WHERE state='pending' AND last_error IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    health.backlog = conn.query_row(
        "SELECT COUNT(*) FROM billing_settlement_outbox \
         WHERE state IN ('pending','processing') AND created_ts<?1",
        rusqlite::params![backlog_before],
        |r| r.get(0),
    )?;
    health.oldest_unsettled_ts = conn.query_row(
        "SELECT COALESCE(MIN(created_ts),0) FROM billing_settlement_outbox \
         WHERE state IN ('pending','processing')",
        [],
        |r| r.get(0),
    )?;
    {
        let mut stmt = conn.prepare(
            "SELECT request_id, actual_nano, attempts, last_error, updated_ts \
             FROM billing_settlement_outbox WHERE state='failed' \
             ORDER BY updated_ts DESC, request_id LIMIT 10",
        )?;
        let rows = stmt.query_map([], |r| {
            let raw: Option<String> = r.get(3)?;
            Ok(SettlementFailure {
                request_id: r.get(0)?,
                actual_nano: r.get(1)?,
                attempts: r.get(2)?,
                last_error: raw.map(|e| e.chars().take(200).collect()),
                updated_ts: r.get(4)?,
            })
        })?;
        health.recent_failed = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    }
    health.ledger_consumer = settlement_consumer_lag(conn, consumer)?;
    Ok(health)
}

/// Обрезать usage_events под масштаб (как ledger_prune): удалить строки старше `older_than_ts`
/// батчами, отдавая write-lock между ними. Возвращает удалённое.
pub fn usage_prune(conn: &Connection, older_than_ts: i64) -> Result<usize> {
    const BATCH: i64 = 5000;
    let mut total = 0usize;
    loop {
        let n = conn.execute(
            "DELETE FROM usage_events WHERE id IN \
             (SELECT id FROM usage_events WHERE ts < ?1 LIMIT ?2)",
            rusqlite::params![older_than_ts, BATCH],
        )?;
        total += n;
        if (n as i64) < BATCH {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(total)
}

/// Прочитать ключ (для авторизации/`/balance`).
pub fn key_get(conn: &Connection, key: &str) -> Result<Option<KeyRow>> {
    let row = conn.query_row(
        "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
         k.spend_limit_nano,k.expires_ts,COALESCE(k.created_ts,0), \
         (SELECT MAX(u.ts) FROM usage_events u WHERE u.account_id=k.account_id AND u.key=k.key), \
         COALESCE(k.status,'active') \
         FROM api_keys k WHERE k.key=?1",
        rusqlite::params![key],
        |r| {
            Ok(KeyRow {
                key: r.get::<_, String>(0)?,
                key_id: r.get::<_, String>(1)?,
                account_id: r.get::<_, Option<String>>(2)?,
                label: r.get::<_, Option<String>>(3)?,
                spent_nano: r.get::<_, i64>(4)?,
                reserved_nano: r.get(5)?,
                spend_limit_nano: r.get(6)?,
                expires_ts: r.get(7)?,
                created_ts: r.get(8)?,
                last_used_ts: r.get(9)?,
                status: r.get(10)?,
            })
        },
    );
    match row {
        Ok(k) => Ok(Some(k)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn key_set_status(conn: &Connection, key: &str, status: &str) -> Result<usize> {
    key_set_status_with_policy_ack(conn, key, status, None)
}

pub fn key_set_status_with_policy_ack(
    conn: &Connection,
    key: &str,
    status: &str,
    activation_policy_ack: Option<&KeyActivationPolicyAck>,
) -> Result<usize> {
    sqlite_key_set_status_with_policy_ack(conn, "key", key, status, activation_policy_ack)
}

/// Change key status through its non-secret control-plane identifier.
pub fn key_set_status_by_id(conn: &Connection, key_id: &str, status: &str) -> Result<usize> {
    key_set_status_by_id_with_policy_ack(conn, key_id, status, None)
}

pub fn key_set_status_by_id_with_policy_ack(
    conn: &Connection,
    key_id: &str,
    status: &str,
    activation_policy_ack: Option<&KeyActivationPolicyAck>,
) -> Result<usize> {
    sqlite_key_set_status_with_policy_ack(conn, "key_id", key_id, status, activation_policy_ack)
}

fn sqlite_key_set_status_with_policy_ack(
    conn: &Connection,
    identity_column: &str,
    identity: &str,
    status: &str,
    activation_policy_ack: Option<&KeyActivationPolicyAck>,
) -> Result<usize> {
    if !matches!(identity_column, "key" | "key_id") {
        anyhow::bail!("invalid key status identity column");
    }
    activation_policy_ack
        .map(KeyActivationPolicyAck::validate)
        .transpose()?;
    let tx = conn.unchecked_transaction()?;
    let query = format!(
        "SELECT binding.policy_enforcement,binding.active_effective_version,policy.content_digest
           FROM api_keys key
           LEFT JOIN account_policy_bindings binding ON binding.account_id=key.account_id
           LEFT JOIN account_policy_versions policy
             ON policy.account_id=binding.account_id
            AND policy.effective_version=binding.active_effective_version
          WHERE key.{identity_column}=?1"
    );
    let policy_state = tx.query_row(&query, rusqlite::params![identity], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    });
    let policy_state = match policy_state {
        Ok(state) => state,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    let ack_matches = activation_policy_ack.is_some_and(|ack| {
        policy_state.1 == Some(ack.effective_policy_version)
            && policy_state.2.as_deref() == Some(ack.policy_digest.as_str())
    });
    if activation_policy_ack.is_some() && !ack_matches {
        anyhow::bail!("key activation policy ACK does not match the exact active policy");
    }
    if status == "active" && policy_state.0.as_deref() == Some("strict") && !ack_matches {
        anyhow::bail!("strict key reactivation requires the exact active policy ACK");
    }
    let update = format!(
        "UPDATE api_keys SET status=?1,
             activation_policy_effective_version=COALESCE(?2,activation_policy_effective_version),
             activation_policy_digest=COALESCE(?3,activation_policy_digest),
             activation_policy_ack_ts=COALESCE(?4,activation_policy_ack_ts)
          WHERE {identity_column}=?5"
    );
    let changed = tx.execute(
        &update,
        rusqlite::params![
            status,
            activation_policy_ack.map(|ack| ack.effective_policy_version),
            activation_policy_ack.map(|ack| ack.policy_digest.as_str()),
            activation_policy_ack.map(|_| now()),
            identity,
        ],
    )?;
    tx.commit()?;
    Ok(changed)
}

/// Change key label through its non-secret control-plane identifier.
pub fn key_set_label_by_id(conn: &Connection, key_id: &str, label: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE api_keys SET label=?2 WHERE key_id=?1",
        rusqlite::params![key_id, label],
    )?)
}

/// Atomically replace a key policy without allowing a new limit to undercut committed or in-flight
/// usage. `None` clears the corresponding guardrail.
pub fn key_set_policy_by_id(
    conn: &Connection,
    account_id: &str,
    key_id: &str,
    spend_limit_nano: Option<i64>,
    expires_ts: Option<i64>,
) -> Result<KeyPolicyUpdate> {
    let updated = conn.execute(
        "UPDATE api_keys SET spend_limit_nano=?3, expires_ts=?4 \
         WHERE key_id=?1 AND account_id=?2 \
           AND (?3 IS NULL OR (reserved_nano<=?3 AND spent_nano<=?3-reserved_nano)) \
           AND (?4 IS NULL OR ?4>CAST(strftime('%s','now') AS INTEGER))",
        rusqlite::params![key_id, account_id, spend_limit_nano, expires_ts],
    )?;
    if updated == 1 {
        return Ok(KeyPolicyUpdate::Updated);
    }
    let exists = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM api_keys WHERE key_id=?1 AND account_id=?2)",
        rusqlite::params![key_id, account_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Ok(KeyPolicyUpdate::NotFound);
    }
    if expires_ts.is_some_and(|expires| expires <= now()) {
        return Ok(KeyPolicyUpdate::ExpiryNotFuture);
    }
    Ok(KeyPolicyUpdate::LimitBelowUsage)
}

/// Удалить ключ НАВСЕГДА (в отличие от set_status 'disabled' — строка исчезает).
pub fn key_remove(conn: &Connection, key: &str) -> Result<usize> {
    Ok(conn.execute("DELETE FROM api_keys WHERE key=?1", rusqlite::params![key])?)
}

/// Удалить ВСЕ ключи (для очистки/тестов). Возвращает число удалённых.
pub fn key_clear(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("DELETE FROM api_keys", [])?)
}

/// Ключи КОНКРЕТНОГО аккаунта (для дашборда коммерции: список ключей юзера). Ключ маскируется на выводе.
pub fn keys_by_account(conn: &Connection, account_id: &str) -> Result<Vec<KeyRow>> {
    let mut stmt = conn.prepare(
        "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
         k.spend_limit_nano,k.expires_ts,COALESCE(k.created_ts,0),u.last_used_ts, \
         COALESCE(k.status,'active') FROM api_keys k LEFT JOIN ( \
           SELECT key,MAX(ts) AS last_used_ts FROM usage_events WHERE account_id=?1 GROUP BY key \
         ) u ON u.key=k.key WHERE k.account_id=?1 ORDER BY COALESCE(k.created_ts,0)",
    )?;
    let rows = stmt.query_map(rusqlite::params![account_id], |r| {
        Ok(KeyRow {
            key: r.get::<_, String>(0)?,
            key_id: r.get::<_, String>(1)?,
            account_id: r.get::<_, Option<String>>(2)?,
            label: r.get::<_, Option<String>>(3)?,
            spent_nano: r.get::<_, i64>(4)?,
            reserved_nano: r.get(5)?,
            spend_limit_nano: r.get(6)?,
            expires_ts: r.get(7)?,
            created_ts: r.get(8)?,
            last_used_ts: r.get(9)?,
            status: r.get(10)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Строка журнала движений баланса (для истории трат/пополнений в дашборде).
#[derive(Debug, Clone)]
pub struct LedgerRow {
    pub id: i64,
    pub key: Option<String>,
    pub kind: String,     // topup | charge | adjust
    pub amount_nano: i64, // + пополнение / − списание
    pub reference: Option<String>,
    pub balance_after_nano: Option<i64>,
    pub ts: i64,
    pub model: Option<String>, // Claude-модель за charge (для per-model графика); topup/adjust → None
}

/// Последние `limit` строк ledger аккаунта (свежие сверху). Для дашборда «история/расход».
pub fn ledger_recent(conn: &Connection, account_id: &str, limit: i64) -> Result<Vec<LedgerRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, key, kind, amount_nano, ref, balance_after_nano, ts, model \
         FROM ledger WHERE account_id=?1 ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![account_id, limit.clamp(1, 1000)], |r| {
        Ok(LedgerRow {
            id: r.get::<_, i64>(0)?,
            key: r.get::<_, Option<String>>(1)?,
            kind: r.get::<_, String>(2)?,
            amount_nano: r.get::<_, i64>(3)?,
            reference: r.get::<_, Option<String>>(4)?,
            balance_after_nano: r.get::<_, Option<i64>>(5)?,
            ts: r.get::<_, i64>(6)?,
            model: r.get::<_, Option<String>>(7)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Ledger cursor for durable external consumers. Rows are returned oldest-first after `after_id`.
pub fn ledger_after(
    conn: &Connection,
    account_id: &str,
    after_id: i64,
    limit: i64,
) -> Result<Vec<LedgerRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, key, kind, amount_nano, ref, balance_after_nano, ts, model \
         FROM ledger WHERE account_id=?1 AND id>?2 ORDER BY id ASC LIMIT ?3",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![account_id, after_id.max(0), limit.clamp(1, 1000)],
        |r| {
            Ok(LedgerRow {
                id: r.get::<_, i64>(0)?,
                key: r.get::<_, Option<String>>(1)?,
                kind: r.get::<_, String>(2)?,
                amount_nano: r.get::<_, i64>(3)?,
                reference: r.get::<_, Option<String>>(4)?,
                balance_after_nano: r.get::<_, Option<i64>>(5)?,
                ts: r.get::<_, i64>(6)?,
                model: r.get::<_, Option<String>>(7)?,
            })
        },
    )?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Все ключи (для CLI-листинга; ключ маскируется на стороне вывода).
pub fn key_list(conn: &Connection) -> Result<Vec<KeyRow>> {
    let mut stmt = conn.prepare(
        "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
         k.spend_limit_nano,k.expires_ts,COALESCE(k.created_ts,0),u.last_used_ts, \
         COALESCE(k.status,'active') FROM api_keys k LEFT JOIN ( \
           SELECT key,MAX(ts) AS last_used_ts FROM usage_events GROUP BY key \
         ) u ON u.key=k.key ORDER BY COALESCE(k.created_ts,0)",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(KeyRow {
            key: r.get::<_, String>(0)?,
            key_id: r.get::<_, String>(1)?,
            account_id: r.get::<_, Option<String>>(2)?,
            label: r.get::<_, Option<String>>(3)?,
            spent_nano: r.get::<_, i64>(4)?,
            reserved_nano: r.get(5)?,
            spend_limit_nano: r.get(6)?,
            expires_ts: r.get(7)?,
            created_ts: r.get(8)?,
            last_used_ts: r.get(9)?,
            status: r.get(10)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Строка персиста состояния пула (по подписке). Примитивы — registry не знает типов `pool`.
#[derive(Clone, Debug, Default)]
pub struct PoolStateRow {
    pub email: String,
    pub cooling_until: i64,
    pub cap5h_usd: f64,
    pub cap7d_usd: f64,
    pub spent_total_usd: f64,
    /// Process-local increment since the last successful persistence operation.
    pub spent_delta_usd: f64,
    pub util5h: f64,
    pub util7d: f64,
    pub reset5h: i64,
    pub reset7d: i64,
    pub calib_n: i64,
    /// PostgreSQL CAS version. SQLite compatibility rows use zero.
    pub version: i64,
}

/// Primitive durable state for one provider-reported OpenAI/Codex window duration.
///
/// Estimation semantics intentionally live in `forward`; registry only persists integer evidence
/// and applies compare-and-swap updates. A reset timestamp identifies the current concrete window,
/// while the primary key keeps independent duration classes from contaminating each other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexCalibrationRow {
    pub home_id: String,
    pub window_duration_mins: i64,
    pub resets_at: i64,
    pub anchor_used_percent: i64,
    pub anchor_spend_nano: i64,
    pub used_percent: i64,
    pub observed_at: i64,
    pub sum_used_sq: i64,
    pub sum_used_spend_nano: i64,
    pub observed_points: i64,
    pub samples: i64,
    pub current_capacity_nano: Option<i64>,
    pub current_low_nano: Option<i64>,
    pub current_high_nano: Option<i64>,
    pub current_confidence_bp: i64,
    pub last_capacity_nano: Option<i64>,
    pub last_low_nano: Option<i64>,
    pub last_high_nano: Option<i64>,
    pub last_confidence_bp: i64,
    pub last_measured_at: Option<i64>,
    /// Compatibility bit; canonical estimator v8 rows are ready to measure from the cold anchor.
    pub anchor_ready: bool,
    /// Provider utilisation in 10^-8 fraction units. The legacy percent fields remain an integer
    /// compatibility projection for binaries predating engine migration 0015.
    pub anchor_used_fraction_units: i64,
    pub used_fraction_units: i64,
    /// Exact sufficient statistics of the realized workload blend.
    pub observed_fraction_units: i64,
    pub observed_spend_nano: i64,
    pub estimator_version: i64,
    pub version: i64,
    pub updated_ts: i64,
}

/// One raw, deduplicated pairing of provider utilisation and cumulative gateway spend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexWindowObservation {
    pub home_id: String,
    pub window_duration_mins: i64,
    pub resets_at: i64,
    pub observed_at: i64,
    pub used_percent: i64,
    pub used_fraction_units: i64,
    pub gateway_spend_nano: i64,
}

/// Primitive durable state for one explicit Antigravity Gemini quota-summary bucket.
///
/// Estimation semantics live in `forward`. Registry keeps only exact integer evidence and CAS
/// versions. The legacy WLS accumulators remain canonical non-negative decimal strings for
/// backwards-compatible replay; `observed_spend_nano` is the exact v2 cumulative spend leg.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiCalibrationRow {
    pub profile_id: String,
    pub bucket_id: String,
    pub window_kind: String,
    pub window_duration_mins: i64,
    pub resets_at: i64,
    pub anchor_used_fraction_units: i64,
    pub anchor_spend_nano: i64,
    pub anchor_ready: bool,
    pub used_fraction_units: i64,
    pub observed_at: i64,
    pub sum_used_sq: String,
    pub sum_used_spend_nano: String,
    pub observed_fraction_units: i64,
    pub observed_spend_nano: i64,
    pub samples: i64,
    pub current_capacity_nano: Option<i64>,
    pub current_low_nano: Option<i64>,
    pub current_high_nano: Option<i64>,
    pub current_confidence_bp: i64,
    pub last_measured_at: Option<i64>,
    pub estimator_version: i64,
    pub version: i64,
    pub updated_ts: i64,
}

/// One raw, deduplicated pairing of an official Gemini quota fraction and cumulative spend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiWindowObservation {
    pub profile_id: String,
    pub bucket_id: String,
    pub window_kind: String,
    pub window_duration_mins: i64,
    pub resets_at: i64,
    pub observed_at: i64,
    pub used_fraction_units: i64,
    pub gateway_spend_nano: i64,
}

fn validate_gemini_accumulator(value: &str) -> Result<()> {
    let parsed = value
        .parse::<i128>()
        .context("parse Gemini calibration accumulator")?;
    if parsed < 0 || parsed.to_string() != value {
        bail!("Gemini calibration accumulator is not canonical");
    }
    Ok(())
}

fn valid_gemini_window_identity(bucket_id: &str, window_kind: &str, duration_mins: i64) -> bool {
    matches!(
        (bucket_id, window_kind, duration_mins),
        ("gemini-5h", "5h", 300) | ("gemini-weekly", "weekly", 10_080)
    )
}

fn validate_gemini_calibration_row(row: &GeminiCalibrationRow) -> Result<()> {
    if row.profile_id.is_empty()
        || !valid_gemini_window_identity(&row.bucket_id, &row.window_kind, row.window_duration_mins)
        || row.resets_at <= 0
        || !(0..=100_000_000).contains(&row.anchor_used_fraction_units)
        || row.anchor_spend_nano < 0
        || !(0..=100_000_000).contains(&row.used_fraction_units)
        || row.observed_at <= 0
        || row.observed_fraction_units < 0
        || row.observed_spend_nano < 0
        || row.samples < 0
        || row.current_capacity_nano.is_some_and(|value| value < 0)
        || row.current_low_nano.is_some_and(|value| value < 0)
        || row.current_high_nano.is_some_and(|value| value < 0)
        || row.current_low_nano.is_some() && row.current_capacity_nano.is_none()
        || row.current_high_nano.is_some() && row.current_capacity_nano.is_none()
        || !(0..=10_000).contains(&row.current_confidence_bp)
        || row.last_measured_at.is_some_and(|value| value <= 0)
        || row.estimator_version <= 0
        || row.version < 0
        || row.updated_ts <= 0
    {
        bail!("invalid Gemini calibration row");
    }
    validate_gemini_accumulator(&row.sum_used_sq)?;
    validate_gemini_accumulator(&row.sum_used_spend_nano)?;
    Ok(())
}

fn validate_gemini_window_observation(observation: &GeminiWindowObservation) -> Result<()> {
    if observation.profile_id.is_empty()
        || !valid_gemini_window_identity(
            &observation.bucket_id,
            &observation.window_kind,
            observation.window_duration_mins,
        )
        || observation.resets_at <= 0
        || observation.observed_at <= 0
        || !(0..=100_000_000).contains(&observation.used_fraction_units)
        || observation.gateway_spend_nano < 0
    {
        bail!("invalid Gemini calibration observation");
    }
    Ok(())
}

fn validate_gemini_calibration_pair(
    state: &GeminiCalibrationRow,
    observation: &GeminiWindowObservation,
) -> Result<()> {
    validate_gemini_calibration_row(state)?;
    validate_gemini_window_observation(observation)?;
    if state.profile_id != observation.profile_id
        || state.bucket_id != observation.bucket_id
        || state.window_kind != observation.window_kind
        || state.window_duration_mins != observation.window_duration_mins
    {
        bail!("Gemini calibration state/observation mismatch");
    }
    Ok(())
}

/// Atomically credit exact official-price spend and return the durable cumulative total.
pub fn credit_codex_home_spend(
    conn: &Connection,
    home_id: &str,
    delta_nano: i64,
    updated_ts: i64,
) -> Result<i64> {
    if home_id.is_empty() || delta_nano < 0 || updated_ts <= 0 {
        bail!("invalid Codex home spend credit");
    }
    conn.query_row(
        "INSERT INTO codex_home_spend(home_id,spent_nano,updated_ts) VALUES(?1,?2,?3) \
         ON CONFLICT(home_id) DO UPDATE SET \
           spent_nano=codex_home_spend.spent_nano+excluded.spent_nano, \
           updated_ts=excluded.updated_ts \
         RETURNING spent_nano",
        rusqlite::params![home_id, delta_nano, updated_ts],
        |row| row.get(0),
    )
    .context("credit SQLite Codex home spend")
}

/// Atomically credit exact official-price Gemini spend and return the cumulative profile total.
pub fn credit_gemini_profile_spend(
    conn: &Connection,
    profile_id: &str,
    delta_nano: i64,
    updated_ts: i64,
) -> Result<i64> {
    if profile_id.is_empty() || delta_nano < 0 || updated_ts <= 0 {
        bail!("invalid Gemini profile spend credit");
    }
    conn.query_row(
        "INSERT INTO gemini_profile_spend(profile_id,spent_nano,updated_ts) VALUES(?1,?2,?3) \
         ON CONFLICT(profile_id) DO UPDATE SET \
           spent_nano=gemini_profile_spend.spent_nano+excluded.spent_nano, \
           updated_ts=excluded.updated_ts \
         RETURNING spent_nano",
        rusqlite::params![profile_id, delta_nano, updated_ts],
        |row| row.get(0),
    )
    .context("credit SQLite Gemini profile spend")
}

/// Durable account-level health for one Codex home.
///
/// Only the account axis is stored. Transport health belongs to one transport generation and must
/// not survive it: a restarted gateway holds a brand new bridge and deserves a fresh verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexHomeHealthRow {
    pub account_state: String,
    pub auth_fail_streak: i64,
    pub first_auth_fail_ts: i64,
    pub cooling_until: i64,
}

impl Default for CodexHomeHealthRow {
    fn default() -> Self {
        Self {
            account_state: "healthy".to_string(),
            auth_fail_streak: 0,
            first_auth_fail_ts: 0,
            cooling_until: 0,
        }
    }
}

pub fn save_codex_home_health(
    conn: &Connection,
    home_id: &str,
    row: &CodexHomeHealthRow,
    updated_ts: i64,
) -> Result<()> {
    if home_id.is_empty() || updated_ts <= 0 {
        bail!("invalid Codex home health write");
    }
    conn.execute(
        "INSERT INTO codex_home_health( \
           home_id,account_state,auth_fail_streak,first_auth_fail_ts,cooling_until,updated_ts) \
         VALUES(?1,?2,?3,?4,?5,?6) \
         ON CONFLICT(home_id) DO UPDATE SET \
           account_state=excluded.account_state, \
           auth_fail_streak=excluded.auth_fail_streak, \
           first_auth_fail_ts=excluded.first_auth_fail_ts, \
           cooling_until=excluded.cooling_until, \
           updated_ts=excluded.updated_ts",
        rusqlite::params![
            home_id,
            row.account_state,
            row.auth_fail_streak,
            row.first_auth_fail_ts,
            row.cooling_until,
            updated_ts
        ],
    )
    .context("save SQLite Codex home health")?;
    Ok(())
}

/// A home with no stored verdict starts healthy: absence of evidence is not evidence of a fault.
pub fn load_codex_home_health(conn: &Connection, home_id: &str) -> Result<CodexHomeHealthRow> {
    Ok(conn
        .query_row(
            "SELECT account_state,auth_fail_streak,first_auth_fail_ts,cooling_until \
             FROM codex_home_health WHERE home_id=?1",
            rusqlite::params![home_id],
            |row| {
                Ok(CodexHomeHealthRow {
                    account_state: row.get(0)?,
                    auth_fail_streak: row.get(1)?,
                    first_auth_fail_ts: row.get(2)?,
                    cooling_until: row.get(3)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

pub fn codex_home_spend(conn: &Connection, home_id: &str) -> Result<i64> {
    Ok(conn
        .query_row(
            "SELECT spent_nano FROM codex_home_spend WHERE home_id=?1",
            rusqlite::params![home_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

pub fn gemini_profile_spend(conn: &Connection, profile_id: &str) -> Result<i64> {
    Ok(conn
        .query_row(
            "SELECT spent_nano FROM gemini_profile_spend WHERE profile_id=?1",
            rusqlite::params![profile_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

fn sqlite_codex_calibration_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodexCalibrationRow> {
    Ok(CodexCalibrationRow {
        home_id: row.get(0)?,
        window_duration_mins: row.get(1)?,
        resets_at: row.get(2)?,
        anchor_used_percent: row.get(3)?,
        anchor_spend_nano: row.get(4)?,
        used_percent: row.get(5)?,
        observed_at: row.get(6)?,
        sum_used_sq: row.get(7)?,
        sum_used_spend_nano: row.get(8)?,
        observed_points: row.get(9)?,
        samples: row.get(10)?,
        current_capacity_nano: row.get(11)?,
        current_low_nano: row.get(12)?,
        current_high_nano: row.get(13)?,
        current_confidence_bp: row.get(14)?,
        last_capacity_nano: row.get(15)?,
        last_low_nano: row.get(16)?,
        last_high_nano: row.get(17)?,
        last_confidence_bp: row.get(18)?,
        last_measured_at: row.get(19)?,
        estimator_version: row.get(20)?,
        version: row.get(21)?,
        updated_ts: row.get(22)?,
        anchor_ready: row.get(23)?,
        anchor_used_fraction_units: row.get(24)?,
        used_fraction_units: row.get(25)?,
        observed_fraction_units: row.get(26)?,
        observed_spend_nano: row.get(27)?,
    })
}

const CODEX_CALIBRATION_COLUMNS: &str = "home_id,window_duration_mins,resets_at,\
    anchor_used_percent,anchor_spend_nano,used_percent,observed_at,sum_used_sq,\
    sum_used_spend_nano,observed_points,samples,current_capacity_nano,current_low_nano,\
    current_high_nano,current_confidence_bp,last_capacity_nano,last_low_nano,last_high_nano,\
    last_confidence_bp,last_measured_at,estimator_version,version,updated_ts,anchor_ready,\
    COALESCE(anchor_used_fraction_units,anchor_used_percent*1000000),\
    COALESCE(used_fraction_units,used_percent*1000000),\
    COALESCE(observed_fraction_units,observed_points*1000000),\
    COALESCE(observed_spend_nano,0)";

pub fn load_codex_calibration(
    conn: &Connection,
    home_id: &str,
    window_duration_mins: i64,
) -> Result<Option<CodexCalibrationRow>> {
    conn.query_row(
        &format!(
            "SELECT {CODEX_CALIBRATION_COLUMNS} FROM codex_window_calibrations \
             WHERE home_id=?1 AND window_duration_mins=?2"
        ),
        rusqlite::params![home_id, window_duration_mins],
        sqlite_codex_calibration_row,
    )
    .optional()
    .context("load SQLite Codex calibration")
}

/// Load the immutable evidence log for a one-time estimator rebuild.
///
/// The synthetic id is the tie-breaker because provider observations can share a wall-clock
/// second. Runtime updates remain incremental once the stored estimator version is current.
pub fn load_codex_window_observations(
    conn: &Connection,
    home_id: &str,
    window_duration_mins: i64,
) -> Result<Vec<CodexWindowObservation>> {
    let mut statement = conn.prepare(
        "SELECT home_id,window_duration_mins,resets_at,observed_at,used_percent,\
                COALESCE(used_fraction_units,used_percent*1000000),gateway_spend_nano \
         FROM codex_window_observations WHERE home_id=?1 AND window_duration_mins=?2 \
         ORDER BY observed_at,id",
    )?;
    let observations = statement
        .query_map(rusqlite::params![home_id, window_duration_mins], |row| {
            Ok(CodexWindowObservation {
                home_id: row.get(0)?,
                window_duration_mins: row.get(1)?,
                resets_at: row.get(2)?,
                observed_at: row.get(3)?,
                used_percent: row.get(4)?,
                used_fraction_units: row.get(5)?,
                gateway_spend_nano: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("load SQLite Codex window observations")?;
    Ok(observations)
}

/// Persist one estimator result and its raw observation in the same transaction.
///
/// `None` is an ordinary CAS conflict. Callers reload evidence and recompute; no observation from
/// the losing derivation commits on its own.
pub fn save_codex_calibration(
    conn: &Connection,
    state: &CodexCalibrationRow,
    observation: &CodexWindowObservation,
) -> Result<Option<i64>> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .context("begin SQLite Codex calibration CAS")?;
    let values = rusqlite::params![
        state.home_id,
        state.window_duration_mins,
        state.resets_at,
        state.anchor_used_percent,
        state.anchor_spend_nano,
        state.used_percent,
        state.observed_at,
        state.sum_used_sq,
        state.sum_used_spend_nano,
        state.observed_points,
        state.samples,
        state.current_capacity_nano,
        state.current_low_nano,
        state.current_high_nano,
        state.current_confidence_bp,
        state.last_capacity_nano,
        state.last_low_nano,
        state.last_high_nano,
        state.last_confidence_bp,
        state.last_measured_at,
        state.estimator_version,
        state.updated_ts,
        state.version,
        state.anchor_ready,
        state.anchor_used_fraction_units,
        state.used_fraction_units,
        state.observed_fraction_units,
        state.observed_spend_nano,
    ];
    let changed = if state.version == 0 {
        tx.execute(
            "INSERT INTO codex_window_calibrations( \
               home_id,window_duration_mins,resets_at,anchor_used_percent,anchor_spend_nano,\
               used_percent,observed_at,sum_used_sq,sum_used_spend_nano,observed_points,samples,\
               current_capacity_nano,current_low_nano,current_high_nano,current_confidence_bp,\
               last_capacity_nano,last_low_nano,last_high_nano,last_confidence_bp,last_measured_at,\
               estimator_version,updated_ts,version,anchor_ready,anchor_used_fraction_units,\
               used_fraction_units,observed_fraction_units,observed_spend_nano \
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,\
                      ?19,?20,?21,?22,?23+1,?24,?25,?26,?27,?28) \
             ON CONFLICT(home_id,window_duration_mins) DO NOTHING",
            values,
        )?
    } else {
        tx.execute(
            "UPDATE codex_window_calibrations SET \
               resets_at=?3,anchor_used_percent=?4,anchor_spend_nano=?5,used_percent=?6,\
               observed_at=?7,sum_used_sq=?8,sum_used_spend_nano=?9,observed_points=?10,\
               samples=?11,current_capacity_nano=?12,current_low_nano=?13,current_high_nano=?14,\
               current_confidence_bp=?15,last_capacity_nano=?16,last_low_nano=?17,\
               last_high_nano=?18,last_confidence_bp=?19,last_measured_at=?20,\
               estimator_version=?21,updated_ts=?22,version=version+1,anchor_ready=?24,\
               anchor_used_fraction_units=?25,used_fraction_units=?26,\
               observed_fraction_units=?27,observed_spend_nano=?28 \
             WHERE home_id=?1 AND window_duration_mins=?2 AND version=?23",
            values,
        )?
    };
    if changed == 0 {
        return Ok(None);
    }
    tx.execute(
        "INSERT INTO codex_window_observations( \
           home_id,window_duration_mins,resets_at,observed_at,used_percent,used_fraction_units,\
           gateway_spend_nano \
         ) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT DO NOTHING",
        rusqlite::params![
            observation.home_id,
            observation.window_duration_mins,
            observation.resets_at,
            observation.observed_at,
            observation.used_percent,
            observation.used_fraction_units,
            observation.gateway_spend_nano,
        ],
    )?;
    tx.commit()?;
    Ok(Some(state.version.saturating_add(1)))
}

fn sqlite_gemini_calibration_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<GeminiCalibrationRow> {
    Ok(GeminiCalibrationRow {
        profile_id: row.get(0)?,
        bucket_id: row.get(1)?,
        window_kind: row.get(2)?,
        window_duration_mins: row.get(3)?,
        resets_at: row.get(4)?,
        anchor_used_fraction_units: row.get(5)?,
        anchor_spend_nano: row.get(6)?,
        anchor_ready: row.get(7)?,
        used_fraction_units: row.get(8)?,
        observed_at: row.get(9)?,
        sum_used_sq: row.get(10)?,
        sum_used_spend_nano: row.get(11)?,
        observed_fraction_units: row.get(12)?,
        observed_spend_nano: row.get(13)?,
        samples: row.get(14)?,
        current_capacity_nano: row.get(15)?,
        current_low_nano: row.get(16)?,
        current_high_nano: row.get(17)?,
        current_confidence_bp: row.get(18)?,
        last_measured_at: row.get(19)?,
        estimator_version: row.get(20)?,
        version: row.get(21)?,
        updated_ts: row.get(22)?,
    })
}

const GEMINI_CALIBRATION_COLUMNS: &str = "profile_id,bucket_id,window_kind,window_duration_mins,\
    resets_at,anchor_used_fraction_units,anchor_spend_nano,anchor_ready,used_fraction_units,\
    observed_at,sum_used_sq,sum_used_spend_nano,observed_fraction_units,observed_spend_nano,\
    samples,current_capacity_nano,current_low_nano,current_high_nano,\
    current_confidence_bp,last_measured_at,estimator_version,version,updated_ts";

pub fn load_gemini_calibration(
    conn: &Connection,
    profile_id: &str,
    bucket_id: &str,
) -> Result<Option<GeminiCalibrationRow>> {
    let row = conn
        .query_row(
            &format!(
                "SELECT {GEMINI_CALIBRATION_COLUMNS} FROM gemini_window_calibrations \
                 WHERE profile_id=?1 AND bucket_id=?2"
            ),
            rusqlite::params![profile_id, bucket_id],
            sqlite_gemini_calibration_row,
        )
        .optional()
        .context("load SQLite Gemini calibration")?;
    if let Some(row) = &row {
        validate_gemini_calibration_row(row)?;
    }
    Ok(row)
}

pub fn load_gemini_window_observations(
    conn: &Connection,
    profile_id: &str,
    bucket_id: &str,
) -> Result<Vec<GeminiWindowObservation>> {
    let mut statement = conn.prepare(
        "SELECT profile_id,bucket_id,window_kind,window_duration_mins,resets_at,observed_at,\
           used_fraction_units,gateway_spend_nano FROM gemini_window_observations \
         WHERE profile_id=?1 AND bucket_id=?2 ORDER BY observed_at,id",
    )?;
    let observations = statement
        .query_map(rusqlite::params![profile_id, bucket_id], |row| {
            Ok(GeminiWindowObservation {
                profile_id: row.get(0)?,
                bucket_id: row.get(1)?,
                window_kind: row.get(2)?,
                window_duration_mins: row.get(3)?,
                resets_at: row.get(4)?,
                observed_at: row.get(5)?,
                used_fraction_units: row.get(6)?,
                gateway_spend_nano: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("load SQLite Gemini window observations")?;
    Ok(observations)
}

/// Persist one Gemini estimator result and raw observation atomically under optimistic CAS.
pub fn save_gemini_calibration(
    conn: &Connection,
    state: &GeminiCalibrationRow,
    observation: &GeminiWindowObservation,
) -> Result<Option<i64>> {
    validate_gemini_calibration_pair(state, observation)?;
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .context("begin SQLite Gemini calibration CAS")?;
    let values = rusqlite::params![
        state.profile_id,
        state.bucket_id,
        state.window_kind,
        state.window_duration_mins,
        state.resets_at,
        state.anchor_used_fraction_units,
        state.anchor_spend_nano,
        state.anchor_ready,
        state.used_fraction_units,
        state.observed_at,
        state.sum_used_sq,
        state.sum_used_spend_nano,
        state.observed_fraction_units,
        state.observed_spend_nano,
        state.samples,
        state.current_capacity_nano,
        state.current_low_nano,
        state.current_high_nano,
        state.current_confidence_bp,
        state.last_measured_at,
        state.estimator_version,
        state.updated_ts,
        state.version,
    ];
    let changed = if state.version == 0 {
        tx.execute(
            "INSERT INTO gemini_window_calibrations( \
               profile_id,bucket_id,window_kind,window_duration_mins,resets_at,\
               anchor_used_fraction_units,anchor_spend_nano,anchor_ready,used_fraction_units,\
               observed_at,sum_used_sq,sum_used_spend_nano,observed_fraction_units,\
               observed_spend_nano,samples,current_capacity_nano,current_low_nano,current_high_nano,\
               current_confidence_bp,last_measured_at,estimator_version,updated_ts,version \
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,\
                      ?18,?19,?20,?21,?22,?23+1) \
             ON CONFLICT(profile_id,bucket_id) DO NOTHING",
            values,
        )?
    } else {
        tx.execute(
            "UPDATE gemini_window_calibrations SET \
               window_kind=?3,window_duration_mins=?4,resets_at=?5,\
               anchor_used_fraction_units=?6,anchor_spend_nano=?7,anchor_ready=?8,\
               used_fraction_units=?9,observed_at=?10,sum_used_sq=?11,\
               sum_used_spend_nano=?12,observed_fraction_units=?13,\
               observed_spend_nano=?14,samples=?15,current_capacity_nano=?16,\
               current_low_nano=?17,current_high_nano=?18,current_confidence_bp=?19,\
               last_measured_at=?20,estimator_version=?21,updated_ts=?22,version=version+1 \
             WHERE profile_id=?1 AND bucket_id=?2 AND version=?23",
            values,
        )?
    };
    if changed == 0 {
        return Ok(None);
    }
    tx.execute(
        "INSERT INTO gemini_window_observations( \
           profile_id,bucket_id,window_kind,window_duration_mins,resets_at,observed_at,\
           used_fraction_units,gateway_spend_nano \
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT DO NOTHING",
        rusqlite::params![
            observation.profile_id,
            observation.bucket_id,
            observation.window_kind,
            observation.window_duration_mins,
            observation.resets_at,
            observation.observed_at,
            observation.used_fraction_units,
            observation.gateway_spend_nano,
        ],
    )?;
    tx.commit()?;
    Ok(Some(state.version.saturating_add(1)))
}

/// Сохранить снимок состояния пула (upsert по email). Одной транзакцией — атомарно и быстро.
pub fn save_pool_state(conn: &Connection, rows: &[PoolStateRow]) -> Result<()> {
    let ts = now();
    conn.execute_batch("BEGIN")?;
    {
        let mut stmt = conn.prepare(
            "INSERT INTO pool_state(email, cooling_until, cap5h, cap7d, spent_total, util5, util7, \
             reset5, reset7, calib_n, updated_ts) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) \
             ON CONFLICT(email) DO UPDATE SET cooling_until=excluded.cooling_until, cap5h=excluded.cap5h, \
             cap7d=excluded.cap7d, spent_total=excluded.spent_total, util5=excluded.util5, \
             util7=excluded.util7, reset5=excluded.reset5, reset7=excluded.reset7, \
             calib_n=excluded.calib_n, updated_ts=excluded.updated_ts")?;
        for r in rows {
            stmt.execute(rusqlite::params![
                r.email,
                r.cooling_until,
                r.cap5h_usd,
                r.cap7d_usd,
                r.spent_total_usd,
                r.util5h,
                r.util7d,
                r.reset5h,
                r.reset7d,
                r.calib_n,
                ts
            ])?;
        }
    }
    conn.execute_batch("COMMIT")?;
    Ok(())
}

/// Прочитать сохранённое состояние пула (для восстановления на старте).
pub fn load_pool_state(conn: &Connection) -> Result<Vec<PoolStateRow>> {
    let mut stmt = conn.prepare(
        "SELECT email, cooling_until, cap5h, cap7d, spent_total, util5, util7, reset5, reset7, calib_n \
         FROM pool_state")?;
    let rows = stmt.query_map([], |r| {
        Ok(PoolStateRow {
            email: r.get(0)?,
            cooling_until: r.get(1)?,
            cap5h_usd: r.get(2)?,
            cap7d_usd: r.get(3)?,
            spent_total_usd: r.get(4)?,
            spent_delta_usd: 0.0,
            util5h: r.get(5)?,
            util7d: r.get(6)?,
            reset5h: r.get(7)?,
            reset7d: r.get(8)?,
            calib_n: r.get(9)?,
            version: 0,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

// (Синхронная обёртка `Billing` удалена: запросный путь теперь через `forward::AsyncBilling`
//  — DB-акторы, ноль синхронных вызовов на async-воркерах. registry остаётся чистым sync-ядром.)

/// Агрегаты биллинга по всем аккаунтам клиентов (нанодоллары).
#[derive(Clone, Debug, Default)]
pub struct BillingTotals {
    pub balance_nano: i64,  // суммарный остаток на аккаунтах (клиентский флоат)
    pub spent_nano: i64,    // суммарно списано за всё время
    pub reserved_nano: i64, // сейчас в незакрытых резервах (in-flight холды)
    pub active_accounts: i64,
}

/// Суммы по accounts одним запросом (источник истины — БД). Ошибка возвращается вызывающему коду.
pub fn billing_totals(conn: &Connection) -> Result<BillingTotals> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(balance_nano),0), COALESCE(SUM(spent_nano),0), \
         COALESCE(SUM(reserved_nano),0), COALESCE(SUM(CASE WHEN COALESCE(status,'active')='active' \
         THEN 1 ELSE 0 END),0) FROM accounts",
        [],
        |r| {
            Ok(BillingTotals {
                balance_nano: r.get(0)?,
                spent_nano: r.get(1)?,
                reserved_nano: r.get(2)?,
                active_accounts: r.get(3)?,
            })
        },
    )?)
}

/// Простая UTC-строка YYYY-MM-DD HH:MM без внешних крейтов (для колонки `added`).
fn chrono_like(ts: i64) -> String {
    let days = ts.div_euclid(86400);
    let secs = ts.rem_euclid(86400);
    let (h, mi) = (secs / 3600, (secs % 3600) / 60);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        open(":memory:").unwrap()
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
        assert_eq!(codex_home_spend(&c, "home-a").unwrap(), 0);
        assert_eq!(
            credit_codex_home_spend(&c, "home-a", 40_000_000_000, 100).unwrap(),
            40_000_000_000
        );
        assert_eq!(
            credit_codex_home_spend(&c, "home-a", 60_000_000_000, 101).unwrap(),
            100_000_000_000
        );

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
            ).unwrap();
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
        ).unwrap();
        c.execute(
            "INSERT INTO api_keys(key,key_id,balance_nano,spent_nano,mult_bp,status,reserved_nano) \
             VALUES(?1,?2,?3,?4,?5,'active',0)",
            rusqlite::params!["sk-user-b-123456789abc", "legacy_b", 200, 20, 3000],
        ).unwrap();
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
            sqlite_settle_request(&c, "req", "a", "k", 400, 150, Some("provider:req"), None)
                .unwrap(),
            Some(850),
        );
        assert_eq!(
            sqlite_settle_request(&c, "req", "a", "k", 400, 150, Some("provider:req"), None)
                .unwrap(),
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
            sqlite_reserve_request_with_legacy_snapshot(&c, "snapshot-key", 9_999, &snapshot)
                .unwrap();
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
            sqlite_reserve_request_with_legacy_snapshot(&c, "snapshot-key", 60, &different)
                .unwrap(),
            O::Conflict(Conflict::SnapshotPayload)
        );
        assert_eq!(
            sqlite_reserve_request_with_legacy_snapshot(&c, "different-key", 60, &snapshot)
                .unwrap(),
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
            sqlite_reserve_request_with_legacy_snapshot(&c, "snapshot-key", 60, &legacy_only)
                .unwrap(),
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
            sqlite_reserve_request_with_legacy_snapshot(&rejected, "poor-key", 60, &too_large)
                .unwrap(),
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
        assert!(sqlite_reserve_request_with_legacy_snapshot(
            &failing,
            "rollback-key",
            60,
            &snapshot
        )
        .is_err());
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
            sqlite_reserve_request_with_legacy_snapshot(&c, "retention-key", 60, &snapshot)
                .unwrap(),
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
                   (SELECT COUNT(*) FROM ledger)",
                [],
                |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                )),
            )
            .unwrap(),
            (0, 0, 0, ledger_before)
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
        let report = sqlite_reconcile_expired(&c, 100).unwrap();
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
        let report = sqlite_reconcile_expired(&c, 100).unwrap();
        assert_eq!(report.canceled_before_delivery, 1);
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
        ).unwrap();
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
            ..Default::default()
        };
        usage_event_add(&c, "a", Some("k"), &opus, 8_000_000, Some("req1")).unwrap();
        usage_event_add(&c, "a", Some("k"), &opus, 8_000_000, Some("req2")).unwrap();
        let sonnet = UsageEventInput {
            model: "claude-sonnet-5".into(),
            input_tokens: 300,
            output_tokens: 100,
            real_nano: 5_000_000,
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
            input_nano: 20_000_000,
            ..Default::default()
        };
        let second = UsageEventInput {
            model: "claude-opus-4-8".into(),
            provider: PROVIDER_OPENAI.into(),
            output_tokens: 10,
            real_nano: 30_000_000,
            output_nano: 30_000_000,
            ..Default::default()
        };
        let third = UsageEventInput {
            model: "claude-sonnet-5".into(),
            cache_read_tokens: 10,
            real_nano: 5_000_000,
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
            ..Default::default()
        };
        let codex = UsageEventInput {
            model: "gpt-5.6".into(),
            provider: PROVIDER_OPENAI.into(),
            real_nano: 5_000_000,
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
            ..Default::default()
        };
        let gpt = UsageEventInput {
            model: "gpt-5.6".into(),
            provider: PROVIDER_OPENAI.into(),
            real_nano: 5_000_000,
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
        assert_eq!(account_set_mult_bp(&c, "acct", 3500).unwrap(), 1);
        assert_eq!(account_get(&c, "acct").unwrap().unwrap().mult_bp, 3500);
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
            sqlite_reserve_request_with_policy_snapshot(&c, "strict-key", 60, &track_snapshot)
                .unwrap(),
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
}
