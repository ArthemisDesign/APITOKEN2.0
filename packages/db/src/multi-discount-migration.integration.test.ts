import { randomUUID } from "node:crypto";
import {
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { describe, expect, it } from "vitest";
import type {
  AccountPolicyBinding,
  AccountPolicySpec,
  PricingCatalogSpec,
  ProviderSwitchSpec,
} from "@claude-api/contracts";
import {
  claimNextPricingControlJob,
  claimNextPricingJob,
  confirmPricingControlJob,
  createDatabase,
  getCustomerPricingPolicyView,
  recoverStalePricingControlJobs,
  stageAccountPolicyControlJob,
  stagePricingCatalogControlJob,
  stageProviderSwitchControlJob,
} from "./index.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;
const LEGACY_MIGRATION_LAST_INDEX = 21;

const MULTI_DISCOUNT_TABLES = [
  "account_policy_bindings",
  "account_policy_reconciliations",
  "account_policy_rules",
  "account_policy_versions",
  "business_invite_policy_bindings",
  "engine_catalog_jobs",
  "engine_policy_jobs",
  "engine_switch_jobs",
  "pricing_policies",
  "pricing_policy_heads",
  "pricing_policy_rules",
  "pricing_policy_versions",
  "pricing_usage_attributions",
  "pricing_usage_funding_allocations",
  "product_catalog_entries",
  "product_catalog_heads",
  "product_catalog_versions",
  "provider_capability_aliases",
  "provider_capability_entries",
  "provider_capability_head",
  "provider_capability_versions",
  "provider_switch_entries",
  "provider_switch_head",
  "provider_switch_versions",
] as const;

const PRICING_RELEASE_V2_TABLES = [
  "business_invite_policy_snapshots_v2",
  "pricing_funding_normalizations_v2",
  "pricing_policy_documents_v2",
  "pricing_policy_rules_v2",
  "pricing_release_activation_receipts_v2",
  "pricing_release_assignments_v2",
  "pricing_release_control_jobs_v2",
  "pricing_release_plans_v2",
  "pricing_stage8_evidence_v2",
  "service_account_inventory_v2",
] as const;

const PRICING_STAGE5_EVIDENCE_TABLES = [
  "pricing_stage5_blockers_v2",
  "pricing_stage5_prepare_acks_v2",
  "pricing_stage5_runs_v2",
] as const;

const LEGACY_STATE_TABLES = [
  "business_invites",
  "customer_profiles",
  "device_sightings",
  "email_outbox",
  "engine_accounts",
  "engine_pricing_jobs",
  "pricing_usage_cursors",
  "pricing_usage_events",
  "signup_profiles",
  "users",
] as const;

interface Journal {
  version: string;
  dialect: string;
  entries: Array<{
    idx: number;
    version: string;
    when: number;
    tag: string;
    breakpoints: boolean;
  }>;
}

interface TemporaryDatabase {
  client: Client;
  connectionString: string;
  close: () => Promise<void>;
}

interface PgFailure {
  code?: string;
  constraint?: string;
  message?: string;
  cause?: PgFailure;
}

interface ValidGraph {
  b2cBindingId: string;
  b2bBindingId: string;
  b2cPolicyId: string;
  b2bPolicyId: string;
  invitationPolicyId: string;
  inviteId: string;
  b2cUserId: string;
  b2bUserId: string;
  b2cEngineRecordId: string;
  b2bEngineRecordId: string;
  b2cEngineAccountId: string;
  b2bEngineAccountId: string;
}

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) {
    throw new Error(`unsafe PostgreSQL identifier: ${identifier}`);
  }
  return `"${identifier}"`;
}

async function createTemporaryDatabase(label: string): Promise<TemporaryDatabase> {
  if (!connectionString) throw new Error("TEST_DATABASE_URL is required");

  const databaseName = [
    "md0022",
    label.replace(/[^a-z0-9]/g, "").slice(0, 10),
    process.pid,
    randomUUID().replaceAll("-", "").slice(0, 12),
  ].join("_");
  const admin = new Client({ connectionString });
  await admin.connect();

  try {
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
  } catch (error) {
    await admin.end();
    throw error;
  }

  const targetUrl = new URL(connectionString);
  targetUrl.pathname = `/${databaseName}`;
  const target = new Client({ connectionString: targetUrl.toString() });

  try {
    await target.connect();
  } catch (error) {
    await admin.query(`DROP DATABASE ${quoteIdentifier(databaseName)}`);
    await admin.end();
    throw error;
  }

  let closed = false;
  return {
    client: target,
    connectionString: targetUrl.toString(),
    close: async () => {
      if (closed) return;
      closed = true;
      let cleanupError: unknown;

      try {
        await target.end();
      } catch (error) {
        cleanupError = error;
      }

      try {
        await admin.query(
          "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1 AND pid <> pg_backend_pid()",
          [databaseName],
        );
        await admin.query(`DROP DATABASE ${quoteIdentifier(databaseName)}`);
      } catch (error) {
        cleanupError ??= error;
      }

      try {
        await admin.end();
      } catch (error) {
        cleanupError ??= error;
      }

      if (cleanupError !== undefined) throw cleanupError;
    },
  };
}

async function withTemporaryDatabase<T>(
  label: string,
  action: (client: Client) => Promise<T>,
): Promise<T> {
  const database = await createTemporaryDatabase(label);
  try {
    return await action(database.client);
  } finally {
    await database.close();
  }
}

async function applyMigrations(client: Client, migrationsFolder: string): Promise<void> {
  await migrate(drizzle(client), { migrationsFolder });
}

async function createMigrationsThrough(lastIndex: number): Promise<string> {
  const folder = await mkdtemp(join(tmpdir(), `commerce-migrations-${lastIndex}-`));
  const metadataFolder = join(folder, "meta");
  await mkdir(metadataFolder);

  const journal = JSON.parse(
    await readFile(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
  ) as Journal;
  const selectedEntries = journal.entries.filter((entry) => entry.idx <= lastIndex);
  expect(selectedEntries.at(-1)?.idx).toBe(lastIndex);

  await Promise.all(selectedEntries.map((entry) =>
    copyFile(
      join(MIGRATIONS_FOLDER, `${entry.tag}.sql`),
      join(folder, `${entry.tag}.sql`),
    )
  ));
  await writeFile(
    join(metadataFolder, "_journal.json"),
    `${JSON.stringify({ ...journal, entries: selectedEntries }, null, 2)}\n`,
  );
  return folder;
}

async function migrationCount(client: Client): Promise<number> {
  const result = await client.query<{ count: number }>(
    'SELECT count(*)::int AS count FROM "drizzle"."__drizzle_migrations"',
  );
  return result.rows[0]!.count;
}

async function captureLegacyState(client: Client): Promise<Record<string, string>> {
  const snapshot: Record<string, string> = {};
  for (const table of LEGACY_STATE_TABLES) {
    const identifier = quoteIdentifier(table);
    const expandedColumns = table === "business_invites"
      ? [
          "encrypted_token",
          "revoked_at",
          "revoked_by_actor",
          "superseded_by_invite_id",
          "idempotency_key",
          "created_by_actor",
        ]
      : table === "email_outbox" ? ["business_invite_id"] : [];
    const result = await client.query<{ rows: string }>(`
      SELECT COALESCE(
        jsonb_agg(
          to_jsonb(snapshot_row) - $1::text[]
          ORDER BY (to_jsonb(snapshot_row) - $1::text[])::text
        ),
        '[]'::jsonb
      )::text AS rows
      FROM ${identifier} AS snapshot_row
    `, [expandedColumns]);
    snapshot[table] = result.rows[0]!.rows;
  }
  return snapshot;
}

async function expectExpandedTablesEmpty(client: Client): Promise<void> {
  const expectedTables = [
    ...MULTI_DISCOUNT_TABLES,
    ...PRICING_RELEASE_V2_TABLES,
    ...PRICING_STAGE5_EVIDENCE_TABLES,
  ];
  const existing = await client.query<{ table_name: string }>(`
    SELECT table_name
    FROM information_schema.tables
    WHERE table_schema = 'public'
      AND table_name = ANY($1::text[])
    ORDER BY table_name
  `, [expectedTables]);
  expect(existing.rows.map((row) => row.table_name)).toEqual(
    expectedTables.sort(),
  );

  for (const table of expectedTables) {
    const result = await client.query<{ count: number }>(
      `SELECT count(*)::int AS count FROM ${quoteIdentifier(table)}`,
    );
    expect(result.rows).toEqual([{ count: 0 }]);
  }
}

async function seedLegacyState(client: Client): Promise<void> {
  const b2cUserId = randomUUID();
  const b2bUserId = randomUUID();
  const b2cEngineRecordId = randomUUID();
  const b2bEngineRecordId = randomUUID();
  const consumedInviteId = randomUUID();
  const pendingInviteId = randomUUID();
  const fixedTime = "2026-07-20T10:00:00.000Z";

  await client.query(`
    INSERT INTO users (
      id, email, display_name, email_verified, created_at, updated_at
    ) VALUES
      ($1, 'legacy-b2c@test.invalid', 'Legacy B2C', true, $3, $3),
      ($2, 'legacy-b2b@test.invalid', 'Legacy B2B', true, $3, $3)
  `, [b2cUserId, b2bUserId, fixedTime]);
  await client.query(`
    INSERT INTO customer_profiles (
      user_id, customer_type, current_tier, multiplier_bp, pricing_month_start,
      cumulative_topup_nano, tier_window_start, tier_window_spent_nano,
      referral_floor_bps, free_balance_nano, created_at, updated_at
    ) VALUES
      ($1, 'b2c', 1, 4000, $3, 25000000000, $3, 7000000000, 500, 4000000000, $3, $3),
      ($2, 'b2b', NULL, 3750, $3, 0, NULL, 0, 0, 0, $3, $3)
  `, [b2cUserId, b2bUserId, fixedTime]);
  await client.query(`
    INSERT INTO engine_accounts (
      id, user_id, engine_account_id, mult_bp, status, created_at, updated_at
    ) VALUES
      ($1, $3, 'legacy-engine-b2c', 4000, 'active', $5, $5),
      ($2, $4, 'legacy-engine-b2b', 3750, 'active', $5, $5)
  `, [
    b2cEngineRecordId,
    b2bEngineRecordId,
    b2cUserId,
    b2bUserId,
    fixedTime,
  ]);
  await client.query(`
    INSERT INTO business_invites (
      id, email, token_hash, multiplier_bp, expires_at, consumed_at,
      consumed_by_user_id, created_at
    ) VALUES
      ($1, 'pending-business@test.invalid', 'legacy-pending-token', 3750,
        '2027-01-01T00:00:00Z', NULL, NULL, $4),
      ($2, 'legacy-b2b@test.invalid', 'legacy-consumed-token', 6250,
        '2027-01-01T00:00:00Z', $4, $3, $4)
  `, [pendingInviteId, consumedInviteId, b2bUserId, fixedTime]);
  await client.query(`
    INSERT INTO engine_pricing_jobs (
      id, user_id, engine_account_id, multiplier_bp, reason, status,
      attempts, confirmed_at, created_at, updated_at
    ) VALUES
      ($1, $3, 'legacy-engine-b2c', 4000, 'b2c_topup', 'confirmed', 1, $5, $5, $5),
      ($2, $4, 'legacy-engine-b2b', 3750, 'business_invite', 'pending', 0, NULL, $5, $5)
  `, [randomUUID(), randomUUID(), b2cUserId, b2bUserId, fixedTime]);
  await client.query(`
    INSERT INTO pricing_usage_cursors (
      engine_account_id, user_id, last_ledger_id, updated_at
    ) VALUES
      ('legacy-engine-b2c', $1, 41, $3),
      ('legacy-engine-b2b', $2, 18, $3)
  `, [b2cUserId, b2bUserId, fixedTime]);
  await client.query(`
    INSERT INTO pricing_usage_events (
      id, user_id, engine_account_id, ledger_entry_id, amount_nano,
      real_funded_nano, occurred_at, created_at
    ) VALUES
      ($1, $3, 'legacy-engine-b2c', 41, 123456789, 100000000, $5, $5),
      ($2, $4, 'legacy-engine-b2b', 18, 987654321, 987654321, $5, $5)
  `, [randomUUID(), randomUUID(), b2cUserId, b2bUserId, fixedTime]);
  await client.query(`
    INSERT INTO signup_profiles (
      user_id, email_canonical, ip_address, ip_subnet, user_agent,
      device_hash, bonus_granted, created_at
    ) VALUES
      ($1, 'legacy-b2c@test.invalid', '203.0.113.10', '203.0.113.0/24',
        'migration-test/b2c', 'legacy-device-b2c', true, $3),
      ($2, 'legacy-b2b@test.invalid', '198.51.100.20', '198.51.100.0/24',
        'migration-test/b2b', 'legacy-device-b2b', false, $3)
  `, [b2cUserId, b2bUserId, fixedTime]);
  await client.query(`
    INSERT INTO device_sightings (
      device_hash, user_id, first_seen_at, last_seen_at
    ) VALUES
      ('legacy-device-b2c', $1, $3, $3),
      ('legacy-device-b2b', $2, $3, $3)
  `, [b2cUserId, b2bUserId, fixedTime]);
  await client.query(`
    INSERT INTO email_outbox (
      id, user_id, recipient, template, payload, status, attempts,
      last_error, created_at, updated_at
    ) VALUES (
      $1, $2, 'legacy-b2c@test.invalid', 'verify_email', '{}'::jsonb,
      'canceled', 0, 'superseded after successful email verification', $3, $3
    )
  `, [randomUUID(), b2cUserId, fixedTime]);
}

async function expectDatabaseFailure(
  client: Client,
  action: () => Promise<void>,
  expected: { code: string; constraint?: string },
): Promise<void> {
  await client.query("BEGIN");
  let failure: PgFailure | undefined;
  try {
    try {
      await action();
      await client.query("SET CONSTRAINTS ALL IMMEDIATE");
    } catch (error) {
      failure = error as PgFailure;
    }
  } finally {
    await client.query("ROLLBACK");
  }

  expect(failure, "mutation unexpectedly succeeded").toBeDefined();
  expect(failure).toMatchObject(expected);
}

async function insertValidGraph(client: Client): Promise<ValidGraph> {
  const graph: ValidGraph = {
    b2cBindingId: randomUUID(),
    b2bBindingId: randomUUID(),
    b2cPolicyId: `policy-b2c-${randomUUID()}`,
    b2bPolicyId: `policy-b2b-${randomUUID()}`,
    invitationPolicyId: `policy-invite-${randomUUID()}`,
    inviteId: randomUUID(),
    b2cUserId: randomUUID(),
    b2bUserId: randomUUID(),
    b2cEngineRecordId: randomUUID(),
    b2bEngineRecordId: randomUUID(),
    b2cEngineAccountId: `acct_b2c_${randomUUID()}`,
    b2bEngineAccountId: `acct_b2b_${randomUUID()}`,
  };

  await client.query("BEGIN");
  try {
    await client.query(`
      INSERT INTO users (id, email, display_name, email_verified) VALUES
        ($1, $3, 'Policy B2C', true),
        ($2, $4, 'Policy B2B', true)
    `, [
      graph.b2cUserId,
      graph.b2bUserId,
      `b2c-${graph.b2cUserId}@test.invalid`,
      `b2b-${graph.b2bUserId}@test.invalid`,
    ]);
    await client.query(`
      INSERT INTO engine_accounts (
        id, user_id, engine_account_id, mult_bp, status
      ) VALUES
        ($1, $3, $5, 4000, 'active'),
        ($2, $4, $6, 3750, 'active')
    `, [
      graph.b2cEngineRecordId,
      graph.b2bEngineRecordId,
      graph.b2cUserId,
      graph.b2bUserId,
      graph.b2cEngineAccountId,
      graph.b2bEngineAccountId,
    ]);
    await client.query(`
      INSERT INTO business_invites (
        id, email, token_hash, multiplier_bp, expires_at
      ) VALUES ($1, $2, $3, 3750, '2027-01-01T00:00:00Z')
    `, [
      graph.inviteId,
      `invite-${graph.inviteId}@test.invalid`,
      `token-${graph.inviteId}`,
    ]);

    await client.query(`
      INSERT INTO provider_capability_versions (
        generation, schema_version, content_digest, source_runtime,
        source_revision, observed_at
      ) VALUES (1, 1, 'capability-v1', 'migration-test', 'test-revision', now())
    `);
    await client.query(`
      INSERT INTO provider_capability_entries (
        generation, provider_id, canonical_model_id, entry_digest, capability_data
      ) VALUES
        (1, 'anthropic', 'claude-sonnet', 'cap-anthropic-sonnet', '{"streaming":true}'::jsonb),
        (1, 'openai', 'gpt-5', 'cap-openai-gpt5', '{"streaming":true}'::jsonb)
    `);
    await client.query(`
      INSERT INTO provider_capability_aliases (
        generation, provider_id, alias_model_id, canonical_model_id
      ) VALUES (1, 'anthropic', 'claude-sonnet-latest', 'claude-sonnet')
    `);
    await client.query(`
      INSERT INTO provider_capability_head (singleton, active_generation)
      VALUES (1, 1)
    `);

    await client.query(`
      INSERT INTO product_catalog_versions (
        product_id, generation, schema_version, capability_generation,
        capability_digest, content_digest, actor_type, actor_id, reason
      ) VALUES (
        'main', 1, 1, 1, 'capability-v1', 'catalog-main-v1',
        'system', 'migration-test', 'initial test catalog'
      )
    `);
    await client.query(`
      INSERT INTO product_catalog_entries (
        product_id, generation, capability_generation, provider_id,
        canonical_model_id, enabled
      ) VALUES
        ('main', 1, 1, 'anthropic', 'claude-sonnet', true),
        ('main', 1, 1, 'openai', 'gpt-5', true)
    `);
    await client.query(`
      INSERT INTO product_catalog_heads (product_id, active_generation)
      VALUES ('main', 1)
    `);

    await client.query(`
      INSERT INTO provider_switch_versions (
        generation, schema_version, capability_generation, capability_digest,
        content_digest, actor_type, actor_id, reason
      ) VALUES (
        1, 1, 1, 'capability-v1', 'switch-v1',
        'system', 'migration-test', 'initial test switches'
      )
    `);
    await client.query(`
      INSERT INTO provider_switch_entries (
        generation, provider_id, scope_type, product_id, segment,
        catalog_generation, enabled
      ) VALUES
        (1, 'anthropic', 'master', '', '', NULL, true),
        (1, 'openai', 'master', '', '', NULL, true),
        (1, 'anthropic', 'segment', 'main', 'b2c', 1, true),
        (1, 'anthropic', 'segment', 'main', 'b2b', 1, true),
        (1, 'openai', 'segment', 'main', 'b2c', 1, true),
        (1, 'openai', 'segment', 'main', 'b2b', 1, true)
    `);
    await client.query(`
      INSERT INTO provider_switch_head (singleton, active_generation)
      VALUES (1, 1)
    `);

    await client.query(`
      INSERT INTO pricing_policies (
        id, owner_type, owner_id, product_id
      ) VALUES
        ($1, 'global_b2c', 'global', 'main'),
        ($2, 'b2b_client', $4, 'main'),
        ($3, 'b2b_invitation', $5, 'main')
    `, [
      graph.b2cPolicyId,
      graph.b2bPolicyId,
      graph.invitationPolicyId,
      graph.b2bUserId,
      graph.inviteId,
    ]);
    await client.query(`
      INSERT INTO pricing_policy_versions (
        policy_id, version, schema_version, product_id, catalog_generation,
        content_digest, actor_type, actor_id, reason
      ) VALUES
        ($1, 1, 1, 'main', 1, 'policy-b2c-v1', 'system', 'migration-test', 'B2C defaults'),
        ($2, 1, 1, 'main', 1, 'policy-b2b-v1', 'admin', 'migration-test', 'legacy B2B copy'),
        ($3, 1, 1, 'main', 1, 'policy-invite-v1', 'admin', 'migration-test', 'invite terms')
    `, [graph.b2cPolicyId, graph.b2bPolicyId, graph.invitationPolicyId]);
    await client.query(`
      INSERT INTO pricing_policy_rules (
        policy_id, policy_version, product_id, catalog_generation, rule_id,
        rule_digest, scope_type, provider_id, canonical_model_id, pricing_mode,
        rule_origin, discount_bps, payable_multiplier_bp, track_eligible,
        retention_eligible, commission_eligible
      ) VALUES
        ($1, 1, 'main', 1, 'b2c-anthropic', 'rule-b2c-anthropic',
          'provider', 'anthropic', NULL, 'track', 'managed', NULL, NULL, true, true, true),
        ($1, 1, 'main', 1, 'b2c-openai', 'rule-b2c-openai',
          'provider', 'openai', NULL, 'track', 'managed', NULL, NULL, true, true, true),
        ($2, 1, 'main', 1, 'b2b-anthropic', 'rule-b2b-anthropic',
          'provider', 'anthropic', NULL, 'discount', 'legacy', NULL, 3750, false, false, false),
        ($3, 1, 'main', 1, 'invite-anthropic', 'rule-invite-anthropic',
          'provider', 'anthropic', NULL, 'discount', 'legacy', NULL, 3750, false, false, false)
    `, [graph.b2cPolicyId, graph.b2bPolicyId, graph.invitationPolicyId]);
    await client.query(`
      INSERT INTO pricing_policy_heads (
        policy_id, current_version, current_digest
      ) VALUES
        ($1, 1, 'policy-b2c-v1'),
        ($2, 1, 'policy-b2b-v1'),
        ($3, 1, 'policy-invite-v1')
    `, [graph.b2cPolicyId, graph.b2bPolicyId, graph.invitationPolicyId]);

    await client.query(`
      INSERT INTO business_invite_policy_bindings (
        invite_id, invitation_policy_id, current_policy_version, current_policy_digest
      ) VALUES ($1, $2, 1, 'policy-invite-v1')
    `, [graph.inviteId, graph.invitationPolicyId]);

    await client.query(`
      INSERT INTO account_policy_bindings (
        id, user_id, engine_account_record_id, engine_account_id,
        account_class, product_id, policy_id
      ) VALUES
        ($1, $3, $5, $7, 'b2c', 'main', $9),
        ($2, $4, $6, $8, 'b2b', 'main', $10)
    `, [
      graph.b2cBindingId,
      graph.b2bBindingId,
      graph.b2cUserId,
      graph.b2bUserId,
      graph.b2cEngineRecordId,
      graph.b2bEngineRecordId,
      graph.b2cEngineAccountId,
      graph.b2bEngineAccountId,
      graph.b2cPolicyId,
      graph.b2bPolicyId,
    ]);
    await client.query(`
      INSERT INTO account_policy_versions (
        binding_id, effective_version, policy_id, policy_version, policy_digest,
        product_id, account_class, schema_version, catalog_generation,
        switch_generation, content_digest
      ) VALUES
        ($1, 1, $3, 1, 'policy-b2c-v1', 'main', 'b2c', 1, 1, 1, 'account-b2c-v1'),
        ($2, 1, $4, 1, 'policy-b2b-v1', 'main', 'b2b', 1, 1, 1, 'account-b2b-v1')
    `, [
      graph.b2cBindingId,
      graph.b2bBindingId,
      graph.b2cPolicyId,
      graph.b2bPolicyId,
    ]);
    await client.query(`
      INSERT INTO account_policy_rules (
        binding_id, effective_version, product_id, catalog_generation, rule_id,
        rule_digest, scope_type, provider_id, canonical_model_id, pricing_mode,
        rule_origin, discount_bps, payable_multiplier_bp, track_eligible,
        retention_eligible, commission_eligible
      ) VALUES
        ($1, 1, 'main', 1, 'b2c-anthropic', 'effective-b2c-anthropic',
          'provider', 'anthropic', NULL, 'track', 'managed', NULL, 4000, true, true, true),
        ($1, 1, 'main', 1, 'b2c-openai', 'effective-b2c-openai',
          'provider', 'openai', NULL, 'track', 'managed', NULL, 4000, true, true, true),
        ($2, 1, 'main', 1, 'b2b-anthropic', 'effective-b2b-anthropic',
          'provider', 'anthropic', NULL, 'discount', 'legacy', NULL, 3750, false, false, false)
    `, [graph.b2cBindingId, graph.b2bBindingId]);
    await client.query(`
      UPDATE account_policy_bindings
      SET desired_effective_version = 1,
          desired_digest = CASE
            WHEN id = $1 THEN 'account-b2c-v1'
            ELSE 'account-b2b-v1'
          END,
          policy_enforcement = 'shadow',
          sync_state = 'pending',
          updated_at = now()
      WHERE id IN ($1, $2)
    `, [graph.b2cBindingId, graph.b2bBindingId]);

    await client.query(`
      INSERT INTO engine_policy_jobs (
        id, binding_id, effective_version, engine_account_id, policy_id,
        policy_version, catalog_generation, switch_generation, schema_version,
        content_digest, payload, status, attempts, ack_effective_version,
        ack_policy_version, ack_catalog_generation, ack_switch_generation,
        ack_schema_version, ack_content_digest, ack_payload, confirmed_at
      ) VALUES (
        $1, $2, 1, $3, $4, 1, 1, 1, 1, 'account-b2c-v1',
        '{"kind":"policy"}'::jsonb, 'confirmed', 1, 1, 1, 1, 1, 1,
        'account-b2c-v1', '{"accepted":true}'::jsonb, now()
      )
    `, [
      randomUUID(),
      graph.b2cBindingId,
      graph.b2cEngineAccountId,
      graph.b2cPolicyId,
    ]);
    await client.query(`
      UPDATE account_policy_bindings
      SET applied_effective_version = 1,
          applied_digest = 'account-b2c-v1',
          policy_enforcement = 'shadow',
          sync_state = 'confirmed',
          last_ack_at = now(),
          updated_at = now()
      WHERE id = $1
    `, [graph.b2cBindingId]);

    const policyUsageEventId = randomUUID();
    const legacyUsageEventId = randomUUID();
    await client.query(`
      INSERT INTO pricing_usage_events (
        id, user_id, engine_account_id, ledger_entry_id, amount_nano,
        real_funded_nano, occurred_at
      ) VALUES
        ($1, $3, $5, 1, 100, 60, now()),
        ($2, $4, $6, 1, 75, 75, now())
    `, [
      policyUsageEventId,
      legacyUsageEventId,
      graph.b2cUserId,
      graph.b2bUserId,
      graph.b2cEngineAccountId,
      graph.b2bEngineAccountId,
    ]);
    await client.query(`
      INSERT INTO pricing_usage_attributions (
        pricing_usage_event_id, attribution_schema_version, snapshot_kind,
        engine_request_id, provider_id, product_id, account_class,
        binding_id, requested_model_id, canonical_model_id, served_model_id,
        served_canonical_model_id, billing_invariant_code, alias_generation,
        rule_id, rule_digest, rule_scope, pricing_mode, rule_origin, discount_bps,
        payable_multiplier_bp, policy_id, policy_version, effective_policy_version,
        effective_policy_digest, policy_digest, catalog_generation,
        switch_generation, tariff_schedule_id,
        tariff_priced_at, official_nano, charged_nano, official_cost_json,
        paid_funded_nano, bonus_funded_nano, other_funded_nano,
        funding_allocation_json, track_eligible, retention_eligible,
        commission_eligible, snapshot_digest
      ) VALUES (
        $1, 1, 'policy_v1', $3, 'anthropic', 'main', 'b2c', $5,
        'claude-sonnet-latest', 'claude-sonnet', NULL, NULL, NULL, 1,
        'b2c-anthropic', 'effective-b2c-anthropic', 'provider',
        'track', 'managed', NULL, 4000, $4, 1, 1, 'account-b2c-v1', 'policy-b2c-v1',
        1, 1, 'official-2026-07', now(), 250, 100, '{"input":150,"output":100}'::jsonb,
        60, 40, 0,
        '[
          {
            "ordinal": 0,
            "engine_bucket_id": "paid-bucket",
            "bucket_version": "1",
            "source_type": "paid",
            "source_ref": "payment:test",
            "amount_nano": "60"
          },
          {
            "ordinal": 1,
            "engine_bucket_id": "welcome-bucket",
            "bucket_version": "1",
            "source_type": "welcome_track_bonus",
            "source_ref": "welcome:test",
            "amount_nano": "40"
          }
        ]'::jsonb,
        true, true, true, 'snapshot-policy-v1'
      ), (
        $2, 1, 'legacy_scalar', NULL, NULL, NULL, NULL, NULL,
        NULL, NULL, NULL, NULL, NULL, NULL,
        NULL, NULL, NULL, 'legacy_scalar', 'legacy', NULL, 3750,
        NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 75, NULL,
        NULL, NULL, NULL, NULL, false, false, false, 'snapshot-legacy-3750'
      )
    `, [
      policyUsageEventId,
      legacyUsageEventId,
      `request-${randomUUID()}`,
      graph.b2cPolicyId,
      graph.b2cBindingId,
    ]);
    await client.query(`
      INSERT INTO pricing_usage_funding_allocations (
        pricing_usage_event_id, ordinal, engine_bucket_id, bucket_version,
        source_type, source_ref, amount_nano
      ) VALUES
        ($1, 0, 'paid-bucket', 1, 'paid', 'payment:test', 60),
        ($1, 1, 'welcome-bucket', 1, 'welcome_track_bonus', 'welcome:test', 40)
    `, [policyUsageEventId]);

    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  }

  return graph;
}

describe.runIf(Boolean(connectionString))("multi-discount migration", () => {
  it("upgrades an exact 0021 database without changing legacy rows and remains idempotent", async () => {
    const legacyMigrationsFolder = await createMigrationsThrough(
      LEGACY_MIGRATION_LAST_INDEX,
    );
    try {
      await withTemporaryDatabase("upgrade", async (client) => {
        await applyMigrations(client, legacyMigrationsFolder);
        expect(await migrationCount(client)).toBe(22);
        await seedLegacyState(client);
        const before = await captureLegacyState(client);

        await applyMigrations(client, MIGRATIONS_FOLDER);
        expect(await migrationCount(client)).toBe(33);
        expect(await captureLegacyState(client)).toEqual(before);
        await expectExpandedTablesEmpty(client);

        await applyMigrations(client, MIGRATIONS_FOLDER);
        expect(await migrationCount(client)).toBe(33);
        expect(await captureLegacyState(client)).toEqual(before);
        await expectExpandedTablesEmpty(client);
      });
    } finally {
      await rm(legacyMigrationsFolder, { recursive: true, force: true });
    }
  }, TEST_TIMEOUT_MS);

  it("preserves pre-expand blocker rows and accepts exact blocked plans without target identity", async () => {
    const migrationsThrough0026 = await createMigrationsThrough(26);
    try {
      await withTemporaryDatabase("funding-blockers", async (client) => {
        await applyMigrations(client, migrationsThrough0026);
        await client.query(`
          INSERT INTO pricing_release_plans_v2 (
            generation, release_kind, schema_version,
            commerce_inventory_digest, engine_inventory_digest,
            openkeys_inventory_digest, service_inventory_digest,
            policy_manifest_digest, assignment_manifest_digest,
            funding_manifest_digest, engine_release_digest, content_digest
          ) VALUES (
            1, 'target', 2,
            'commerce', 'engine', 'openkeys', 'service',
            'policies', 'assignments', 'funding', 'engine-release', 'release'
          )
        `);
        await client.query(`
          INSERT INTO pricing_funding_normalizations_v2 (
            release_generation, engine_account_id, funding_generation,
            expected_source_digest, target_funding_digest, status
          ) VALUES (1, 'acct_legacy_blocker', 1, 'source', 'target', 'blocker')
        `);

        await applyMigrations(client, MIGRATIONS_FOLDER);
        expect(await migrationCount(client)).toBe(33);
        const legacy = await client.query(`
          SELECT funding_generation::text, target_funding_digest,
                 normalization_source, blockers, status
          FROM pricing_funding_normalizations_v2
          WHERE release_generation = 1 AND engine_account_id = 'acct_legacy_blocker'
        `);
        expect(legacy.rows[0]).toEqual({
          funding_generation: "1",
          target_funding_digest: "target",
          normalization_source: null,
          blockers: null,
          status: "blocker",
        });

        await client.query(`
          INSERT INTO pricing_funding_normalizations_v2 (
            release_generation, engine_account_id, funding_generation,
            expected_source_digest, target_funding_digest,
            normalization_source, blockers, status
          ) VALUES (
            1, 'acct_exact_blocker', NULL,
            'sha256:v2:source', NULL, 'ledger_replay',
            '[{"code":"active_legacy_reservation","detail":"retry account locally"}]'::jsonb,
            'blocker'
          )
        `);
      });
    } finally {
      await rm(migrationsThrough0026, { recursive: true, force: true });
    }
  }, TEST_TIMEOUT_MS);

  it("refuses an out-of-order 0022-to-0023 upgrade after a new-table writer starts", async () => {
    const migrationsThrough0022 = await createMigrationsThrough(22);
    const database = await createTemporaryDatabase("preflight");
    const peer = new Client({ connectionString: database.connectionString });
    try {
      await peer.connect();
      await applyMigrations(database.client, migrationsThrough0022);
      expect(await migrationCount(database.client)).toBe(23);

      await peer.query("BEGIN");
      await peer.query(`
          INSERT INTO provider_switch_versions (
            generation, schema_version, content_digest, actor_type, reason
          ) VALUES (1, 1, 'premature-switch', 'test', 'preflight coverage')
      `);

      let lockFailure: PgFailure | undefined;
      try {
        await applyMigrations(database.client, MIGRATIONS_FOLDER);
      } catch (error) {
        lockFailure = error as PgFailure;
      }
      expect(lockFailure?.cause ?? lockFailure).toMatchObject({ code: "55P03" });
      expect(await migrationCount(database.client)).toBe(23);

      await peer.query("COMMIT");

      let populatedFailure: PgFailure | undefined;
      try {
        await applyMigrations(database.client, MIGRATIONS_FOLDER);
      } catch (error) {
        populatedFailure = error as PgFailure;
      }
      expect(populatedFailure?.cause ?? populatedFailure).toMatchObject({
        code: "23514",
        constraint: "multi_discount_invariants_empty_preflight",
      });
      expect(await migrationCount(database.client)).toBe(23);
      const preserved = await database.client.query<{ count: number }>(
        "SELECT count(*)::int AS count FROM provider_switch_versions",
      );
      expect(preserved.rows).toEqual([{ count: 1 }]);
    } finally {
      await peer.query("ROLLBACK").catch(() => undefined);
      await peer.end().catch(() => undefined);
      await database.close();
      await rm(migrationsThrough0022, { recursive: true, force: true });
    }
  }, TEST_TIMEOUT_MS);

  it("claims catalog, switches, and policy in dependency order and persists exact ACKs", async () => {
    const temporary = await createTemporaryDatabase("controljobs");
    const database = createDatabase(temporary.connectionString, "multi-discount-control-job-test");
    try {
      await applyMigrations(temporary.client, MIGRATIONS_FOLDER);
      const graph = await insertValidGraph(temporary.client);
      const catalog: PricingCatalogSpec = {
        product_id: "main",
        generation: 1,
        schema_version: 1,
        capability_generation: 1,
        capability_digest: "capability-v1",
        content_digest: "catalog-main-v1",
        entries: [
          { provider_id: "anthropic", canonical_model_id: "claude-sonnet", enabled: true },
          { provider_id: "openai", canonical_model_id: "gpt-5", enabled: true },
        ],
      };
      const switches: ProviderSwitchSpec = {
        generation: 1,
        schema_version: 1,
        capability_generation: 1,
        capability_digest: "capability-v1",
        content_digest: "switch-v1",
        entries: [
          { provider_id: "anthropic", scope: "master", catalog_generation: null, enabled: true },
          {
            provider_id: "anthropic",
            scope: { segment: { product_id: "main", segment: "b2b" } },
            catalog_generation: 1,
            enabled: true,
          },
          {
            provider_id: "anthropic",
            scope: { segment: { product_id: "main", segment: "b2c" } },
            catalog_generation: 1,
            enabled: true,
          },
          { provider_id: "openai", scope: "master", catalog_generation: null, enabled: true },
          {
            provider_id: "openai",
            scope: { segment: { product_id: "main", segment: "b2b" } },
            catalog_generation: 1,
            enabled: true,
          },
          {
            provider_id: "openai",
            scope: { segment: { product_id: "main", segment: "b2c" } },
            catalog_generation: 1,
            enabled: true,
          },
        ],
      };
      const policy: AccountPolicySpec = {
        account_id: graph.b2bEngineAccountId,
        effective_version: 1,
        policy_id: graph.b2bPolicyId,
        policy_version: 1,
        source_policy_digest: "policy-b2b-v1",
        owner_type: "b2b_client",
        owner_id: graph.b2bUserId,
        account_class: "b2b",
        product_id: "main",
        schema_version: 1,
        catalog_generation: 1,
        switch_generation: 1,
        content_digest: "account-b2b-v1",
        replacement_locked: false,
        rules: [{
          rule_id: "b2b-anthropic",
          rule_digest: "effective-b2b-anthropic",
          scope: { provider: { provider_id: "anthropic" } },
          pricing_mode: "discount",
          rule_origin: "legacy",
          discount_bps: null,
          payable_multiplier_bp: 3750,
          track_eligible: false,
          retention_eligible: false,
          commission_eligible: false,
        }],
      };
      const binding: AccountPolicyBinding = {
        policy_enforcement: "shadow",
        funding_enforcement: "legacy_single",
        reconciliation_state: "pending",
      };

      const catalogJobId = await stagePricingCatalogControlJob(database, {
        ...catalog,
        entries: [...catalog.entries].reverse(),
      });
      await expect(stagePricingCatalogControlJob(database, catalog)).resolves.toBe(catalogJobId);
      const switchJobId = await stageProviderSwitchControlJob(database, {
        ...switches,
        entries: [...switches.entries].reverse(),
      });
      await expect(stageProviderSwitchControlJob(database, switches)).resolves.toBe(switchJobId);
      const policyJobId = await stageAccountPolicyControlJob(database, { policy, binding });
      await expect(stageAccountPolicyControlJob(database, { policy, binding })).resolves.toBe(policyJobId);
      await temporary.client.query(`
        INSERT INTO engine_pricing_jobs (
          id, user_id, engine_account_id, multiplier_bp, reason
        ) VALUES ($1, $2, $3, 3750, 'legacy_pending')
      `, [randomUUID(), graph.b2bUserId, graph.b2bEngineAccountId]);

      const catalogJob = await claimNextPricingControlJob(database, "worker-control");
      expect(catalogJob).toMatchObject({ kind: "catalog", id: catalogJobId, attempts: 1, spec: catalog });
      if (!catalogJob || catalogJob.kind !== "catalog") throw new Error("catalog job was not claimed");
      // The engine committed this activation, but the commerce worker lost the ACK before it
      // could confirm the job. Lease recovery must replay the same immutable target and accept the
      // engine's idempotent `unchanged` acknowledgement.
      await temporary.client.query(`
        UPDATE engine_catalog_jobs
        SET locked_at = now() - interval '6 minutes'
        WHERE id = $1
      `, [catalogJob.id]);
      await expect(recoverStalePricingControlJobs(database)).resolves.toBe(1);
      const replayedCatalogJob = await claimNextPricingControlJob(database, "worker-control-replay");
      expect(replayedCatalogJob).toMatchObject({
        kind: "catalog",
        id: catalogJob.id,
        attempts: 2,
        spec: catalog,
      });
      if (!replayedCatalogJob || replayedCatalogJob.kind !== "catalog") {
        throw new Error("catalog job was not replayed after its lost ACK");
      }
      await confirmPricingControlJob(database, replayedCatalogJob, {
        result: "unchanged",
        identity: {
          catalog,
          expectation: { exact: { version: 1, content_digest: "catalog-main-v1" } },
        },
      });

      const switchJob = await claimNextPricingControlJob(database, "worker-control");
      expect(switchJob).toMatchObject({ kind: "switches", id: switchJobId, attempts: 1, spec: switches });
      if (!switchJob || switchJob.kind !== "switches") throw new Error("switch job was not claimed");
      await confirmPricingControlJob(database, switchJob, {
        result: "applied",
        identity: { switches, expectation: "absent" },
      });

      const policyJob = await claimNextPricingControlJob(database, "worker-control");
      expect(policyJob).toMatchObject({
        kind: "policy",
        id: policyJobId,
        attempts: 1,
        bindingId: graph.b2bBindingId,
        spec: policy,
        binding,
      });
      if (!policyJob || policyJob.kind !== "policy") throw new Error("policy job was not claimed");
      await confirmPricingControlJob(database, policyJob, {
        result: "applied",
        identity: {
          policy,
          activation: {
            account_id: graph.b2bEngineAccountId,
            effective_version: 1,
            content_digest: "account-b2b-v1",
            binding,
          },
          expectation: "unbound",
        },
      });

      await expect(claimNextPricingControlJob(database, "worker-control")).resolves.toBeNull();
      await expect(claimNextPricingJob(database, "worker-scalar")).resolves.toBeNull();
      const state = await temporary.client.query(`
        SELECT
          (SELECT status FROM engine_catalog_jobs WHERE product_id = 'main') AS catalog_status,
          (SELECT status FROM engine_switch_jobs WHERE generation = 1) AS switch_status,
          (SELECT status FROM engine_policy_jobs WHERE binding_id = $1) AS policy_status,
          (SELECT sync_state FROM account_policy_bindings WHERE id = $1) AS sync_state,
          (SELECT applied_effective_version::text FROM account_policy_bindings WHERE id = $1) AS applied_version,
          (SELECT reason FROM engine_pricing_jobs WHERE user_id = $2) AS scalar_reason
      `, [graph.b2bBindingId, graph.b2bUserId]);
      expect(state.rows).toEqual([{
        catalog_status: "confirmed",
        switch_status: "confirmed",
        policy_status: "confirmed",
        sync_state: "confirmed",
        applied_version: "1",
        scalar_reason: "drained_to_versioned_policy:legacy_pending",
      }]);
    } finally {
      await database.pool.end();
      await temporary.close();
    }
  }, TEST_TIMEOUT_MS);

  it("projects desired and applied customer policies with catalog, switches, and effective rules", async () => {
    const temporary = await createTemporaryDatabase("customerview");
    const database = createDatabase(temporary.connectionString, "multi-discount-customer-view-test");
    try {
      await applyMigrations(temporary.client, MIGRATIONS_FOLDER);
      const graph = await insertValidGraph(temporary.client);

      const b2c = await getCustomerPricingPolicyView(database, graph.b2cUserId);
      expect(b2c).toHaveLength(1);
      expect(b2c[0]).toMatchObject({
        accountClass: "b2c",
        productId: "main",
        policyEnforcement: "shadow",
        fundingEnforcement: "legacy_single",
        reconciliationState: "pending",
        syncState: "confirmed",
        inSync: true,
        desired: {
          effectiveVersion: "1",
          policyVersion: "1",
          catalogGeneration: "1",
          switchGeneration: "1",
        },
        applied: {
          effectiveVersion: "1",
          providers: [
            {
              providerId: "anthropic",
              available: true,
              models: [{
                modelId: "claude-sonnet",
                available: true,
                unavailableReasons: [],
                rule: {
                  ruleId: "b2c-anthropic",
                  scope: "provider",
                  pricingMode: "track",
                  payableMultiplierBp: 4000,
                  trackEligible: true,
                },
              }],
            },
            {
              providerId: "openai",
              available: true,
              models: [{
                modelId: "gpt-5",
                available: true,
                unavailableReasons: [],
                rule: { ruleId: "b2c-openai", scope: "provider" },
              }],
            },
          ],
        },
      });

      const b2b = await getCustomerPricingPolicyView(database, graph.b2bUserId);
      expect(b2b).toMatchObject([{
        accountClass: "b2b",
        syncState: "pending",
        inSync: false,
        applied: null,
        desired: {
          providers: [
            {
              providerId: "anthropic",
              available: true,
              models: [{ modelId: "claude-sonnet", available: true }],
            },
            {
              providerId: "openai",
              available: false,
              models: [{
                modelId: "gpt-5",
                available: false,
                unavailableReasons: ["missing_pricing_rule"],
                rule: null,
              }],
            },
          ],
        },
      }]);
    } finally {
      await database.pool.end();
      await temporary.close();
    }
  }, TEST_TIMEOUT_MS);

  it("rejects unstable Stage 5 scans and prepare ACKs without exact readback", async () => {
    await withTemporaryDatabase("stage5-evidence", async (client) => {
      await applyMigrations(client, MIGRATIONS_FOLDER);
      const digest = (character: string, version = 2) =>
        `sha256:v${version}:${character.repeat(64)}`;
      const runId = randomUUID();
      const insertRun = async (engineSecondDigest: string): Promise<void> => {
        await client.query(`
          INSERT INTO pricing_stage5_runs_v2 (
            run_id, schema_version, plan_digest, commerce_inventory_digest,
            engine_scan_first_digest, engine_scan_second_digest,
            openkeys_scan_first_digest, openkeys_scan_second_digest,
            service_inventory_digest, funding_plan_digest,
            target_generation, target_digest, recovery_generation, recovery_digest,
            inventory_artifact, plan_artifact, blocker_count, status
          ) VALUES (
            $1, 2, $2, $3, $4, $5, $6, $6, $7, $8,
            1, $9, 2, $10, '{}'::jsonb, '{}'::jsonb, 0, 'planned'
          )
        `, [
          runId,
          digest("1"),
          digest("2"),
          digest("3"),
          engineSecondDigest,
          digest("4"),
          digest("5"),
          digest("6"),
          digest("7"),
          digest("8"),
        ]);
      };

      await expectDatabaseFailure(client, async () => {
        await insertRun(digest("9"));
      }, { code: "23514", constraint: "pricing_stage5_runs_v2_shape_check" });

      await insertRun(digest("3"));
      await expectDatabaseFailure(client, async () => {
        await client.query(`
          INSERT INTO pricing_stage5_prepare_acks_v2 (
            run_id, artifact_kind, artifact_id, artifact_version,
            expected_digest, mutation_result, readback_digest, ack_digest
          ) VALUES ($1, 'capability', 'pricing-capability', 3, $2, 'stored', $3, $4)
        `, [runId, digest("a", 1), digest("b", 1), digest("c")]);
      }, { code: "23514", constraint: "pricing_stage5_prepare_acks_v2_shape_check" });

      await client.query(`
        INSERT INTO pricing_stage5_prepare_acks_v2 (
          run_id, artifact_kind, artifact_id, artifact_version,
          expected_digest, mutation_result, readback_digest, ack_digest
        ) VALUES ($1, 'capability', 'pricing-capability', 3, $2, 'stored', $2, $3)
      `, [runId, digest("a", 1), digest("c")]);
      const stored = await client.query<{ runs: number; acks: number }>(`
        SELECT
          (SELECT count(*)::int FROM pricing_stage5_runs_v2) AS runs,
          (SELECT count(*)::int FROM pricing_stage5_prepare_acks_v2) AS acks
      `);
      expect(stored.rows).toEqual([{ runs: 1, acks: 1 }]);
    });
  }, TEST_TIMEOUT_MS);

  it("guards two-phase release finalization without freezing live money writers", async () => {
    await withTemporaryDatabase("two-phase-release", async (client) => {
      await applyMigrations(client, MIGRATIONS_FOLDER);
      const digest = (value: string) =>
        `sha256:v2:${Buffer.from(value).toString("hex").padEnd(64, "0").slice(0, 64)}`;
      const generation = 41;

      await client.query(`
        INSERT INTO pricing_policy_documents_v2 (
          policy_id, policy_version, owner_type, owner_id, account_class,
          product_id, billing_mode, schema_version,
          capability_generation, capability_digest,
          catalog_generation, catalog_digest, switch_generation, switch_digest,
          content_digest
        ) VALUES (
          'policy:two-phase', 1, 'global_b2c', 'global', 'b2c',
          'main', 'balance', 2,
          3, $1, 3, $2, 3, $3, $4
        )
      `, [digest("capability"), digest("catalog"), digest("switch"), digest("policy")]);
      await client.query(`
        INSERT INTO pricing_release_plans_v2 (
          generation, release_kind, schema_version,
          commerce_inventory_digest, engine_inventory_digest,
          openkeys_inventory_digest, service_inventory_digest,
          policy_manifest_digest, assignment_manifest_digest,
          funding_manifest_digest, engine_release_digest, content_digest, status
        ) VALUES (
          $1, 'target', 2, $2, $3, $4, $5, $6, $7,
          NULL, NULL, $8, 'materializing'
        )
      `, [
        generation,
        digest("commerce"),
        digest("engine"),
        digest("openkeys"),
        digest("service"),
        digest("policies"),
        digest("assignments"),
        digest("release-plan"),
      ]);
      await client.query(`
        INSERT INTO pricing_release_assignments_v2 (
          release_generation, engine_account_id, account_class, owner_context,
          owner_id, policy_id, policy_version, policy_digest, billing_mode,
          funding_generation, purpose, responsible, assignment_digest
        ) VALUES (
          $1, 'acct_two_phase', 'b2c', 'commerce', 'user:two-phase',
          'policy:two-phase', 1, $2, 'balance', NULL, NULL, NULL, $3
        )
      `, [generation, digest("policy"), digest("assignment")]);

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE pricing_release_plans_v2 SET status = 'prepared' WHERE generation = $1
        `, [generation]);
      }, { code: "23514", constraint: "pricing_release_plans_v2_finalize_guard" });

      await client.query(`
        INSERT INTO pricing_funding_normalizations_v2 (
          release_generation, engine_account_id, funding_generation,
          expected_source_digest, target_funding_digest, applied_funding_digest,
          normalization_source, blockers, status
        ) VALUES (
          $1, 'acct_two_phase', 7, $2, $3, $3, 'ledger_replay', NULL, 'ready'
        )
      `, [generation, digest("source"), digest("funding-account")]);
      await client.query(`
        UPDATE pricing_release_plans_v2
           SET funding_manifest_digest = $2, updated_at = now()
         WHERE generation = $1
      `, [generation, digest("funding-manifest")]);
      await client.query(`
        UPDATE pricing_release_plans_v2
           SET engine_release_digest = $2, updated_at = now()
         WHERE generation = $1
      `, [generation, digest("engine-release")]);

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE pricing_release_plans_v2 SET status = 'prepared' WHERE generation = $1
        `, [generation]);
      }, { code: "23514", constraint: "pricing_release_plans_v2_finalize_guard" });

      await client.query(`
        UPDATE pricing_release_assignments_v2
           SET funding_generation = 7
         WHERE release_generation = $1 AND engine_account_id = 'acct_two_phase'
      `, [generation]);

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE pricing_release_assignments_v2
             SET funding_generation = 8
           WHERE release_generation = $1 AND engine_account_id = 'acct_two_phase'
        `, [generation]);
      }, { code: "23514", constraint: "pricing_release_assignments_v2_immutable_guard" });
      await client.query(`
        UPDATE pricing_release_plans_v2
           SET status = 'prepared', updated_at = now()
         WHERE generation = $1
      `, [generation]);

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE pricing_release_assignments_v2
             SET funding_generation = 7
           WHERE release_generation = $1 AND engine_account_id = 'acct_two_phase'
        `, [generation]);
      }, { code: "23514", constraint: "pricing_release_assignments_v2_immutable_guard" });
      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE pricing_release_plans_v2 SET status = 'materializing' WHERE generation = $1
        `, [generation]);
      }, { code: "23514", constraint: "pricing_release_plans_v2_finalize_guard" });
      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE pricing_release_plans_v2
             SET funding_manifest_digest = $2
           WHERE generation = $1
        `, [generation, digest("replacement")]);
      }, { code: "23514", constraint: "pricing_release_plans_v2_finalize_guard" });

      const runId = randomUUID();
      await client.query(`
        INSERT INTO pricing_stage5_runs_v2 (
          run_id, schema_version, plan_digest, commerce_inventory_digest,
          engine_scan_first_digest, engine_scan_second_digest,
          openkeys_scan_first_digest, openkeys_scan_second_digest,
          service_inventory_digest, funding_plan_digest,
          target_generation, target_digest, recovery_generation, recovery_digest,
          inventory_artifact, plan_artifact, blocker_count, status
        ) VALUES (
          $1, 2, $2, $3, $4, $4, $5, $5, $6, $7,
          41, NULL, 42, NULL, '{}'::jsonb, '{}'::jsonb, 0, 'materializing'
        )
      `, [
        runId,
        digest("stage5-plan"),
        digest("stage5-commerce"),
        digest("stage5-engine"),
        digest("stage5-openkeys"),
        digest("stage5-service"),
        digest("stage5-funding"),
      ]);
      await client.query(`
        UPDATE pricing_stage5_runs_v2
           SET target_digest = $2, recovery_digest = $3,
               status = 'prepared', updated_at = now()
         WHERE run_id = $1
      `, [runId, digest("target-release"), digest("recovery-release")]);
      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE pricing_stage5_runs_v2 SET target_digest = $2 WHERE run_id = $1
        `, [runId, digest("different-target")]);
      }, { code: "23514", constraint: "pricing_stage5_runs_v2_finalize_guard" });

      const finalized = await client.query(`
        SELECT plan.status, assignment.funding_generation::text,
               run.status AS run_status, run.target_digest IS NOT NULL AS target_finalized
          FROM pricing_release_plans_v2 plan
          JOIN pricing_release_assignments_v2 assignment
            ON assignment.release_generation = plan.generation
          JOIN pricing_stage5_runs_v2 run ON run.target_generation = plan.generation
         WHERE plan.generation = $1
      `, [generation]);
      expect(finalized.rows).toEqual([{
        status: "prepared",
        funding_generation: "7",
        run_status: "prepared",
        target_finalized: true,
      }]);
    });
  }, TEST_TIMEOUT_MS);

  it("accepts a complete structural graph and rejects schema-level corruption", async () => {
    await withTemporaryDatabase("constraints", async (client) => {
      await applyMigrations(client, MIGRATIONS_FOLDER);
      const graph = await insertValidGraph(client);

      const valid = await client.query<{
        confirmed_bindings: number;
        legacy_3750_rules: number;
        null_served_snapshots: number;
        normalized_allocations: number;
      }>(`
        SELECT
          (SELECT count(*)::int
             FROM account_policy_bindings
            WHERE id = $1
              AND desired_effective_version = 1
              AND applied_effective_version = 1
              AND policy_enforcement = 'shadow'
              AND sync_state = 'confirmed') AS confirmed_bindings,
          (SELECT count(*)::int
             FROM account_policy_rules
            WHERE binding_id = $2
              AND rule_origin = 'legacy'
              AND payable_multiplier_bp = 3750) AS legacy_3750_rules,
          (SELECT count(*)::int
             FROM pricing_usage_attributions
            WHERE snapshot_kind = 'policy_v1'
              AND served_model_id IS NULL
              AND served_canonical_model_id IS NULL
              AND billing_invariant_code IS NULL) AS null_served_snapshots,
          (SELECT count(*)::int
             FROM pricing_usage_funding_allocations
            WHERE source_type IN ('paid', 'welcome_track_bonus')) AS normalized_allocations
      `, [graph.b2cBindingId, graph.b2bBindingId]);
      expect(valid.rows).toEqual([{
        confirmed_bindings: 1,
        legacy_3750_rules: 1,
        null_served_snapshots: 1,
        normalized_allocations: 2,
      }]);

      await client.query(`
        INSERT INTO pricing_policy_versions (
          policy_id, version, schema_version, product_id, catalog_generation,
          content_digest, actor_type, reason
        ) VALUES (
          $1, 2, 1, 'main', 1, 'policy-b2c-v2',
          'admin', 'pending strict-policy coverage'
        )
      `, [graph.b2cPolicyId]);
      await client.query(`
        INSERT INTO pricing_policy_rules (
          policy_id, policy_version, product_id, catalog_generation, rule_id,
          rule_digest, scope_type, provider_id, canonical_model_id, pricing_mode,
          rule_origin, discount_bps, payable_multiplier_bp, track_eligible,
          retention_eligible, commission_eligible
        )
        SELECT
          policy_id, 2, product_id, catalog_generation, rule_id,
          rule_digest || '-v2', scope_type, provider_id, canonical_model_id,
          pricing_mode, rule_origin, discount_bps, payable_multiplier_bp,
          track_eligible, retention_eligible, commission_eligible
        FROM pricing_policy_rules
        WHERE policy_id = $1 AND policy_version = 1
      `, [graph.b2cPolicyId]);
      await client.query(`
        INSERT INTO account_policy_versions (
          binding_id, effective_version, policy_id, policy_version, policy_digest,
          product_id, account_class, schema_version, catalog_generation,
          switch_generation, content_digest
        ) VALUES (
          $1, 2, $2, 2, 'policy-b2c-v2', 'main', 'b2c', 1, 1, 1,
          'account-b2c-v2'
        )
      `, [graph.b2cBindingId, graph.b2cPolicyId]);
      await client.query(`
        INSERT INTO account_policy_rules (
          binding_id, effective_version, product_id, catalog_generation, rule_id,
          rule_digest, scope_type, provider_id, canonical_model_id, pricing_mode,
          rule_origin, discount_bps, payable_multiplier_bp, track_eligible,
          retention_eligible, commission_eligible
        )
        SELECT
          binding_id, 2, product_id, catalog_generation, rule_id,
          rule_digest || '-v2', scope_type, provider_id, canonical_model_id,
          pricing_mode, rule_origin, discount_bps, payable_multiplier_bp,
          track_eligible, retention_eligible, commission_eligible
        FROM account_policy_rules
        WHERE binding_id = $1 AND effective_version = 1
      `, [graph.b2cBindingId]);
      await client.query(`
        UPDATE account_policy_bindings
        SET desired_effective_version = 2,
            desired_digest = 'account-b2c-v2',
            policy_enforcement = 'strict',
            reconciliation_state = 'verified',
            sync_state = 'pending',
            updated_at = now()
        WHERE id = $1
      `, [graph.b2cBindingId]);
      const pendingStrict = await client.query<{
        desired_effective_version: string;
        applied_effective_version: string;
        sync_state: string;
      }>(`
        SELECT
          desired_effective_version::text,
          applied_effective_version::text,
          sync_state
        FROM account_policy_bindings
        WHERE id = $1
      `, [graph.b2cBindingId]);
      expect(pendingStrict.rows).toEqual([{
        desired_effective_version: "2",
        applied_effective_version: "1",
        sync_state: "pending",
      }]);

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE account_policy_bindings
          SET sync_state = 'confirmed', updated_at = now()
          WHERE id = $1
        `, [graph.b2cBindingId]);
      }, {
        code: "23514",
        constraint: "account_policy_bindings_enforcement_check",
      });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          INSERT INTO provider_switch_versions (
            generation, schema_version, capability_generation, capability_digest,
            content_digest, actor_type, reason
          ) VALUES (
            2, 1, 1, 'forged-capability-digest',
            'switch-v2', 'admin', 'foreign-key coverage'
          )
        `);
      }, {
        code: "23503",
        constraint: "provider_switch_versions_capability_fk",
      });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          INSERT INTO provider_switch_entries (
            generation, provider_id, scope_type, product_id, segment,
            catalog_generation, enabled
          ) VALUES (1, 'anthropic', 'product', 'main', '', 999, true)
        `);
      }, {
        code: "23503",
        constraint: "provider_switch_entries_catalog_fk",
      });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE pricing_usage_attributions
          SET effective_policy_digest = 'forged-effective-digest'
          WHERE snapshot_kind = 'policy_v1'
        `);
      }, {
        code: "23503",
        constraint: "pricing_usage_attributions_effective_fk",
      });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE pricing_usage_attributions
          SET paid_funded_nano = NULL,
              bonus_funded_nano = NULL,
              other_funded_nano = NULL,
              funding_allocation_json = NULL
          WHERE snapshot_kind = 'policy_v1'
        `);
      }, {
        code: "23514",
        constraint: "pricing_usage_attributions_policy_funding_check",
      });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE pricing_usage_funding_allocations
          SET engine_bucket_id = NULL
          WHERE pricing_usage_event_id IN (
            SELECT pricing_usage_event_id
            FROM pricing_usage_attributions
            WHERE snapshot_kind = 'policy_v1'
          )
        `);
      }, { code: "23502" });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          INSERT INTO engine_catalog_jobs (
            id, product_id, generation, schema_version, content_digest,
            payload, status, confirmed_at
          ) VALUES (
            $1, 'main', 1, 1, 'catalog-main-v1', '{}'::jsonb, 'confirmed', now()
          )
        `, [randomUUID()]);
      }, { code: "23514", constraint: "engine_catalog_jobs_ack_check" });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE account_policy_bindings
          SET desired_digest = 'forged-digest', updated_at = now()
          WHERE id = $1
        `, [graph.b2bBindingId]);
      }, {
        code: "23503",
        constraint: "account_policy_bindings_desired_fk",
      });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          INSERT INTO pricing_policies (
            id, owner_type, owner_id, product_id
          ) VALUES ($1, 'b2b_client', $2, 'openkeys')
        `, [`wrong-product-${randomUUID()}`, graph.b2bUserId]);
      }, { code: "23514", constraint: "pricing_policies_owner_check" });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          INSERT INTO pricing_policy_versions (
            policy_id, version, schema_version, product_id, catalog_generation,
            content_digest, actor_type, reason
          ) VALUES ($1, 3, 1, 'main', 1, 'policy-b2c-v3-null', 'admin', 'fault test')
        `, [graph.b2cPolicyId]);
        await client.query(`
          INSERT INTO pricing_policy_rules (
            policy_id, policy_version, product_id, catalog_generation, rule_id,
            rule_digest, scope_type, provider_id, canonical_model_id, pricing_mode,
            rule_origin, discount_bps, payable_multiplier_bp, track_eligible,
            retention_eligible, commission_eligible
          ) VALUES (
            $1, 3, 'main', 1, 'null-discount', 'null-discount-v3',
            'provider', 'anthropic', NULL, 'discount', 'managed', NULL, 8000,
            false, false, false
          )
        `, [graph.b2cPolicyId]);
      }, {
        code: "23514",
        constraint: "pricing_policy_rules_pricing_check",
      });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          INSERT INTO pricing_policy_versions (
            policy_id, version, schema_version, product_id, catalog_generation,
            content_digest, actor_type, reason
          ) VALUES (
            $1, 4, 1, 'main', 1, 'policy-b2c-v4-fractional', 'admin', 'fault test'
          )
        `, [graph.b2cPolicyId]);
        await client.query(`
          INSERT INTO pricing_policy_rules (
            policy_id, policy_version, product_id, catalog_generation, rule_id,
            rule_digest, scope_type, provider_id, canonical_model_id, pricing_mode,
            rule_origin, discount_bps, payable_multiplier_bp, track_eligible,
            retention_eligible, commission_eligible
          ) VALUES (
            $1, 4, 'main', 1, 'fractional-discount', 'fractional-discount-v4',
            'provider', 'anthropic', NULL, 'discount', 'managed', 9450, 550,
            false, false, false
          )
        `, [graph.b2cPolicyId]);
      }, {
        code: "23514",
        constraint: "pricing_policy_rules_pricing_check",
      });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE business_invite_policy_bindings
          SET copied_to_user_id = $2
          WHERE invite_id = $1
        `, [graph.inviteId, graph.b2bUserId]);
      }, {
        code: "23514",
        constraint: "business_invite_policy_bindings_redemption_check",
      });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          UPDATE business_invite_policy_bindings
          SET redeemed_source_policy_version = 1,
              redeemed_source_policy_digest = 'policy-invite-v1',
              copied_to_user_id = $2,
              copied_to_binding_id = $3,
              copied_client_policy_id = $4,
              copied_client_policy_version = 1,
              copied_client_policy_digest = 'policy-b2b-v1',
              redeemed_at = now(),
              updated_at = now()
          WHERE invite_id = $1
        `, [
          graph.inviteId,
          graph.b2bUserId,
          graph.b2cBindingId,
          graph.b2bPolicyId,
        ]);
      }, {
        code: "23503",
        constraint: "business_invite_policy_bindings_copy_target_fk",
      });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          INSERT INTO engine_policy_jobs (
            id, binding_id, effective_version, engine_account_id, policy_id,
            policy_version, catalog_generation, switch_generation, schema_version,
            content_digest, payload
          ) VALUES (
            $1, $2, 1, 'wrong-engine-account', $3, 1, 1, 1, 1,
            'account-b2b-v1', '{}'::jsonb
          )
        `, [randomUUID(), graph.b2bBindingId, graph.b2bPolicyId]);
      }, {
        code: "23503",
        constraint: "engine_policy_jobs_binding_target_fk",
      });

      await expectDatabaseFailure(client, async () => {
        const usageEventId = randomUUID();
        await client.query(`
          INSERT INTO pricing_usage_events (
            id, user_id, engine_account_id, ledger_entry_id, amount_nano,
            real_funded_nano, occurred_at
          ) VALUES ($1, $2, $3, 2002, 100, 60, now())
        `, [usageEventId, graph.b2cUserId, graph.b2cEngineAccountId]);
        await client.query(`
          INSERT INTO pricing_usage_attributions (
            pricing_usage_event_id, attribution_schema_version, snapshot_kind,
            pricing_mode, rule_origin, payable_multiplier_bp, charged_nano,
            paid_funded_nano, track_eligible, retention_eligible,
            commission_eligible, snapshot_digest
          ) VALUES (
            $1, 1, 'legacy_scalar', 'legacy_scalar', 'legacy', 4000, 100,
            60, false, false, false, 'partial-funding'
          )
        `, [usageEventId]);
      }, {
        code: "23514",
        constraint: "pricing_usage_attributions_funding_check",
      });

      await expectDatabaseFailure(client, async () => {
        await client.query(`
          INSERT INTO provider_switch_entries (
            generation, provider_id, scope_type, product_id, segment, enabled
          ) VALUES (1, 'anthropic', 'segment', 'main', 'service', true)
        `);
      }, {
        code: "23514",
        constraint: "provider_switch_entries_scope_check",
      });
    });
  }, TEST_TIMEOUT_MS);
});
