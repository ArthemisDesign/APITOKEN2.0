import { randomUUID } from "node:crypto";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  createDatabase,
  runCatalogGen2,
  runStage5Backfill,
  type Database,
  type Stage5Inventory,
} from "./index.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

describe.runIf(Boolean(connectionString))("catalog generation 2", () => {
  let admin: Client;
  let seedClient: Client;
  let databaseName: string;
  let databaseUrl: string;
  let database: Database;

  beforeAll(async () => {
    databaseName = `gen2_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 12)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    databaseUrl = url.toString();
    seedClient = new Client({ connectionString: databaseUrl });
    await seedClient.connect();
    await migrate(drizzle(seedClient), { migrationsFolder: MIGRATIONS_FOLDER });
    database = createDatabase(databaseUrl, "catalog-gen2-test");
  }, TEST_TIMEOUT_MS);

  afterAll(async () => {
    await database?.pool.end();
    await seedClient?.end();
    if (admin) {
      await admin.query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1 AND pid <> pg_backend_pid()",
        [databaseName],
      );
      await admin.query(`DROP DATABASE IF EXISTS ${quoteIdentifier(databaseName)}`);
      await admin.end();
    }
  }, TEST_TIMEOUT_MS);

  beforeEach(async () => {
    const tables = await seedClient.query<{ tablename: string }>(`
      SELECT tablename FROM pg_tables
      WHERE schemaname = 'public' AND tablename <> '__drizzle_migrations'
      ORDER BY tablename
    `);
    if (tables.rows.length > 0) {
      await seedClient.query(
        `TRUNCATE TABLE ${tables.rows.map((row) => quoteIdentifier(row.tablename)).join(", ")} RESTART IDENTITY CASCADE`,
      );
    }
  });

  async function seedGeneration1(): Promise<void> {
    const b2cUserId = randomUUID();
    const inviteId = randomUUID();
    await seedClient.query(`
      INSERT INTO users (id, email, display_name, email_verified, status)
      VALUES ($1, $2, 'B2C', true, 'active')
    `, [b2cUserId, `${b2cUserId}@example.test`]);
    await seedClient.query(`
      INSERT INTO customer_profiles (
        user_id, customer_type, current_tier, multiplier_bp, pricing_month_start
      ) VALUES ($1, 'b2c', 0, 4000, now())
    `, [b2cUserId]);
    await seedClient.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, 'acct_gen2_b2c', 4000, 'active')
    `, [randomUUID(), b2cUserId]);
    await seedClient.query(`
      INSERT INTO business_invites (id, token_hash, multiplier_bp, expires_at, created_by_actor)
      VALUES ($1, $2, 6000, now() + interval '30 days', 'test')
    `, [inviteId, `hash-${inviteId}`]);
    const inventory: Stage5Inventory = {
      schema_version: 1,
      engine_accounts: [
        { account_id: "acct_gen2_b2c", multiplier_bp: 4000, status: "active" },
      ],
      openkeys_accounts: [],
    };
    const stage5 = await runStage5Backfill(database, inventory, { mode: "safe" });
    expect(stage5.writes_committed).toBe(true);
  }

  async function rowCounts(): Promise<Record<string, string>> {
    const result = await seedClient.query<{ capability_versions: string }>(`
      SELECT (
        SELECT count(*)::text FROM provider_capability_versions
      ) AS capability_versions
    `);
    const capabilityEntries = await seedClient.query<{ count: string }>(
      "SELECT count(*)::text AS count FROM provider_capability_entries WHERE generation = 2",
    );
    const catalogEntries = await seedClient.query<{ count: string }>(
      "SELECT count(*)::text AS count FROM product_catalog_entries WHERE generation = 2",
    );
    const switchEntries = await seedClient.query<{ count: string }>(
      "SELECT count(*)::text AS count FROM provider_switch_entries WHERE generation = 2",
    );
    const catalogJobs = await seedClient.query<{ count: string }>(
      "SELECT count(*)::text AS count FROM engine_catalog_jobs WHERE generation = 2",
    );
    const switchJobs = await seedClient.query<{ count: string }>(
      "SELECT count(*)::text AS count FROM engine_switch_jobs WHERE generation = 2",
    );
    return {
      capability_versions: result.rows[0]!.capability_versions,
      gen2_capability_entries: capabilityEntries.rows[0]!.count,
      gen2_catalog_entries: catalogEntries.rows[0]!.count,
      gen2_switch_entries: switchEntries.rows[0]!.count,
      gen2_catalog_jobs: catalogJobs.rows[0]!.count,
      gen2_switch_jobs: switchJobs.rows[0]!.count,
    };
  }

  async function heads(): Promise<Record<string, string>> {
    const capability = await seedClient.query<{ active_generation: string }>(
      "SELECT active_generation::text FROM provider_capability_head WHERE singleton = 1",
    );
    const catalogs = await seedClient.query<{ product_id: string; active_generation: string }>(
      "SELECT product_id, active_generation::text FROM product_catalog_heads ORDER BY product_id",
    );
    const switches = await seedClient.query<{ active_generation: string }>(
      "SELECT active_generation::text FROM provider_switch_head WHERE singleton = 1",
    );
    return {
      capability: capability.rows[0]!.active_generation,
      main: catalogs.rows.find((row) => row.product_id === "main")!.active_generation,
      openkeys: catalogs.rows.find((row) => row.product_id === "openkeys")!.active_generation,
      switches: switches.rows[0]!.active_generation,
    };
  }

  it("plans generation 2 in dry_run without any writes", async () => {
    await seedGeneration1();
    const before = await rowCounts();

    const result = await runCatalogGen2(database, { mode: "dry_run" });
    expect(result.writes_committed).toBe(false);
    expect(result.foundation.matches_reviewed_generation_1).toBe(true);
    expect(result.foundation.already_materialized).toBe(false);
    expect(result.plan.capability.content_digest).toBe(
      "sha256:v1:9b23acd863d22abe2a6ed12096a4bb68a07b8d5c196351f1a15d38f11029bcd0",
    );
    expect(result.plan.catalogs.map((catalog) => catalog.product_id)).toEqual(["main", "openkeys"]);
    expect(result.plan.switches.generation).toBe(2);
    expect(await rowCounts()).toEqual(before);
  }, TEST_TIMEOUT_MS);

  it("materializes capability, catalogs, switches, and control jobs atomically", async () => {
    await seedGeneration1();
    expect(await heads()).toEqual({ capability: "1", main: "1", openkeys: "1", switches: "1" });

    const result = await runCatalogGen2(database, { mode: "apply" });
    expect(result.writes_committed).toBe(true);
    expect(await heads()).toEqual({ capability: "2", main: "2", openkeys: "2", switches: "2" });
    expect(await rowCounts()).toEqual({
      capability_versions: "2",
      gen2_capability_entries: "12",
      gen2_catalog_entries: "24",
      gen2_switch_entries: "10",
      gen2_catalog_jobs: "2",
      gen2_switch_jobs: "1",
    });

    // Generation 1 stays intact next to generation 2.
    const gen1Entries = await seedClient.query<{ count: string }>(
      "SELECT count(*)::text AS count FROM product_catalog_entries WHERE generation = 1",
    );
    expect(gen1Entries.rows[0]!.count).toBe("20");

    // Job payloads are the exact immutable specs the worker will deliver.
    const catalogJob = await seedClient.query<{ status: string; payload: unknown }>(`
      SELECT status, payload FROM engine_catalog_jobs
      WHERE product_id = 'openkeys' AND generation = 2
    `);
    expect(catalogJob.rows[0]!.status).toBe("pending");
    expect(catalogJob.rows[0]!.payload).toMatchObject({
      product_id: "openkeys",
      generation: 2,
      capability_generation: 2,
      capability_digest:
        "sha256:v1:9b23acd863d22abe2a6ed12096a4bb68a07b8d5c196351f1a15d38f11029bcd0",
      content_digest:
        "sha256:v1:3b019fc3cfd619b5d4a81451aceafebf0c40de3b8c2cc150aa5b7a28b0102760",
    });
    const switchJob = await seedClient.query<{ status: string; payload: unknown }>(
      "SELECT status, payload FROM engine_switch_jobs WHERE generation = 2",
    );
    expect(switchJob.rows[0]!.status).toBe("pending");
    expect(switchJob.rows[0]!.payload).toMatchObject({
      generation: 2,
      content_digest:
        "sha256:v1:ddbe078beec31d4f8b77e027ff3e9dad5477be6d10dafd4c99956abd9a74febd",
    });
  }, TEST_TIMEOUT_MS);

  it("replays an exact apply idempotently", async () => {
    await seedGeneration1();
    await runCatalogGen2(database, { mode: "apply" });
    const before = await rowCounts();

    const replay = await runCatalogGen2(database, { mode: "apply" });
    expect(replay.writes_committed).toBe(true);
    expect(replay.foundation.already_materialized).toBe(true);
    expect(await rowCounts()).toEqual(before);
    expect(await heads()).toEqual({ capability: "2", main: "2", openkeys: "2", switches: "2" });
  }, TEST_TIMEOUT_MS);

  it("fails closed when the generation-1 foundation drifted", async () => {
    await seedGeneration1();
    await seedClient.query(`
      UPDATE product_catalog_entries SET enabled = false
      WHERE product_id = 'main' AND generation = 1
        AND provider_id = 'anthropic' AND canonical_model_id = 'claude-opus-4-8'
    `);
    await expect(runCatalogGen2(database, { mode: "apply" })).rejects.toMatchObject({
      name: "CatalogGen2Error",
      code: "immutable_version_conflict",
    });
    expect(await heads()).toEqual({ capability: "1", main: "1", openkeys: "1", switches: "1" });
  }, TEST_TIMEOUT_MS);

  it("fails closed when a head already moved past this plan", async () => {
    await seedGeneration1();
    await runCatalogGen2(database, { mode: "apply" });
    await seedClient.query(`
      INSERT INTO provider_switch_versions (
        generation, schema_version, capability_generation, capability_digest,
        content_digest, actor_type, actor_id, reason
      ) VALUES (99, 1, 2,
        'sha256:v1:9b23acd863d22abe2a6ed12096a4bb68a07b8d5c196351f1a15d38f11029bcd0',
        'sha256:v1:future', 'admin', 'test', 'future switch generation')
    `);
    await seedClient.query(
      "UPDATE provider_switch_head SET active_generation = 99 WHERE singleton = 1",
    );
    await expect(runCatalogGen2(database, { mode: "apply" })).rejects.toMatchObject({
      name: "CatalogGen2Error",
      code: "foundation_mismatch",
    });
  }, TEST_TIMEOUT_MS);
});
