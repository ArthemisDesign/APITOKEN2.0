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
  close: () => Promise<void>;
}

interface PgFailure {
  code?: string;
  constraint?: string;
  message?: string;
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

async function createMigrationsThrough0021(): Promise<string> {
  const folder = await mkdtemp(join(tmpdir(), "commerce-migrations-0021-"));
  const metadataFolder = join(folder, "meta");
  await mkdir(metadataFolder);

  const journal = JSON.parse(
    await readFile(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
  ) as Journal;
  const legacyEntries = journal.entries.filter(
    (entry) => entry.idx <= LEGACY_MIGRATION_LAST_INDEX,
  );
  expect(legacyEntries.at(-1)?.idx).toBe(LEGACY_MIGRATION_LAST_INDEX);

  await Promise.all(legacyEntries.map((entry) =>
    copyFile(
      join(MIGRATIONS_FOLDER, `${entry.tag}.sql`),
      join(folder, `${entry.tag}.sql`),
    )
  ));
  await writeFile(
    join(metadataFolder, "_journal.json"),
    `${JSON.stringify({ ...journal, entries: legacyEntries }, null, 2)}\n`,
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
    const result = await client.query<{ rows: string }>(`
      SELECT COALESCE(
        jsonb_agg(to_jsonb(snapshot_row) ORDER BY to_jsonb(snapshot_row)::text),
        '[]'::jsonb
      )::text AS rows
      FROM ${identifier} AS snapshot_row
    `);
    snapshot[table] = result.rows[0]!.rows;
  }
  return snapshot;
}

async function expectExpandedTablesEmpty(client: Client): Promise<void> {
  const existing = await client.query<{ table_name: string }>(`
    SELECT table_name
    FROM information_schema.tables
    WHERE table_schema = 'public'
      AND table_name = ANY($1::text[])
    ORDER BY table_name
  `, [[...MULTI_DISCOUNT_TABLES]]);
  expect(existing.rows.map((row) => row.table_name)).toEqual(
    [...MULTI_DISCOUNT_TABLES].sort(),
  );

  for (const table of MULTI_DISCOUNT_TABLES) {
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
    b2cEngineAccountId: `engine-b2c-${randomUUID()}`,
    b2bEngineAccountId: `engine-b2b-${randomUUID()}`,
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
        generation, schema_version, content_digest, actor_type, actor_id, reason
      ) VALUES (1, 1, 'switch-v1', 'system', 'migration-test', 'initial test switches')
    `);
    await client.query(`
      INSERT INTO provider_switch_entries (
        generation, provider_id, scope_type, product_id, segment, enabled
      ) VALUES
        (1, 'anthropic', 'master', '', '', true),
        (1, 'openai', 'master', '', '', true),
        (1, 'anthropic', 'segment', 'main', 'b2c', true),
        (1, 'anthropic', 'segment', 'main', 'b2b', true),
        (1, 'openai', 'segment', 'main', 'b2c', true),
        (1, 'openai', 'segment', 'main', 'b2b', true)
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
        requested_model_id, canonical_model_id, served_model_id,
        served_canonical_model_id, billing_invariant_code, alias_generation,
        rule_id, rule_digest, rule_scope, pricing_mode, rule_origin, discount_bps,
        payable_multiplier_bp, policy_id, policy_version, effective_policy_version,
        policy_digest, catalog_generation, switch_generation, tariff_schedule_id,
        tariff_priced_at, official_nano, charged_nano, official_cost_json,
        paid_funded_nano, bonus_funded_nano, other_funded_nano,
        funding_allocation_json, track_eligible, retention_eligible,
        commission_eligible, snapshot_digest
      ) VALUES (
        $1, 1, 'policy_v1', $3, 'anthropic', 'main', 'b2c',
        'claude-sonnet-latest', 'claude-sonnet', NULL, NULL, NULL, 1,
        'b2c-anthropic', 'effective-b2c-anthropic', 'provider',
        'track', 'managed', NULL, 4000, $4, 1, 1, 'policy-b2c-v1',
        1, 1, 'official-2026-07', now(), 250, 100, '{"input":150,"output":100}'::jsonb,
        60, 40, 0,
        '[{"source_type":"paid","amount_nano":"60"},{"source_type":"welcome_track_bonus","amount_nano":"40"}]'::jsonb,
        true, true, true, 'snapshot-policy-v1'
      ), (
        $2, 1, 'legacy_scalar', NULL, NULL, NULL, NULL,
        NULL, NULL, NULL, NULL, NULL, NULL,
        NULL, NULL, NULL, 'legacy_scalar', 'legacy', NULL, 3750,
        NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 75, NULL,
        NULL, NULL, NULL, NULL, false, false, false, 'snapshot-legacy-3750'
      )
    `, [
      policyUsageEventId,
      legacyUsageEventId,
      `request-${randomUUID()}`,
      graph.b2cPolicyId,
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
    const legacyMigrationsFolder = await createMigrationsThrough0021();
    try {
      await withTemporaryDatabase("upgrade", async (client) => {
        await applyMigrations(client, legacyMigrationsFolder);
        expect(await migrationCount(client)).toBe(22);
        await seedLegacyState(client);
        const before = await captureLegacyState(client);

        await applyMigrations(client, MIGRATIONS_FOLDER);
        expect(await migrationCount(client)).toBe(23);
        expect(await captureLegacyState(client)).toEqual(before);
        await expectExpandedTablesEmpty(client);

        await applyMigrations(client, MIGRATIONS_FOLDER);
        expect(await migrationCount(client)).toBe(23);
        expect(await captureLegacyState(client)).toEqual(before);
        await expectExpandedTablesEmpty(client);
      });
    } finally {
      await rm(legacyMigrationsFolder, { recursive: true, force: true });
    }
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
          ) VALUES ($1, 2, 1, 'main', 1, 'policy-b2c-v2-null', 'admin', 'fault test')
        `, [graph.b2cPolicyId]);
        await client.query(`
          INSERT INTO pricing_policy_rules (
            policy_id, policy_version, product_id, catalog_generation, rule_id,
            rule_digest, scope_type, provider_id, canonical_model_id, pricing_mode,
            rule_origin, discount_bps, payable_multiplier_bp, track_eligible,
            retention_eligible, commission_eligible
          ) VALUES (
            $1, 2, 'main', 1, 'null-discount', 'null-discount-v2',
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
            $1, 2, 1, 'main', 1, 'policy-b2c-v2-fractional', 'admin', 'fault test'
          )
        `, [graph.b2cPolicyId]);
        await client.query(`
          INSERT INTO pricing_policy_rules (
            policy_id, policy_version, product_id, catalog_generation, rule_id,
            rule_digest, scope_type, provider_id, canonical_model_id, pricing_mode,
            rule_origin, discount_bps, payable_multiplier_bp, track_eligible,
            retention_eligible, commission_eligible
          ) VALUES (
            $1, 2, 'main', 1, 'fractional-discount', 'fractional-discount-v2',
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
