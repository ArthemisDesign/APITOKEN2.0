import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { Client } from "pg";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { createDatabase, type Database } from "./client.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";
import {
  PricingControlJobStageError,
  stageStoredPricingCatalogControlJob,
  stageStoredProviderSwitchControlJob,
} from "./pricing-control-jobs.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) {
    throw new Error(`unsafe PostgreSQL identifier: ${identifier}`);
  }
  return `"${identifier}"`;
}

describe.runIf(Boolean(connectionString))("stored pricing control job staging", () => {
  let database: Database;
  let admin: Client;
  let databaseName: string;
  const audit = { actorId: "operator:test", reason: "converge commerce heads with the engine" };

  beforeAll(async () => {
    databaseName = [
      "pcj",
      process.pid,
      randomUUID().replaceAll("-", "").slice(0, 12),
    ].join("_");
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const targetUrl = new URL(connectionString!);
    targetUrl.pathname = `/${databaseName}`;
    const target = new Client({ connectionString: targetUrl.toString() });
    await target.connect();
    await migrate(drizzle(target), { migrationsFolder: MIGRATIONS_FOLDER });
    await target.end();

    database = createDatabase(targetUrl.toString(), "pricing-control-jobs-test");
    await database.pool.query(`
      INSERT INTO provider_capability_versions (generation, schema_version, content_digest, observed_at)
      VALUES
        (1, 1, 'capability-v1', now()),
        (3, 1, 'capability-v3', now())
    `);
    await database.pool.query(`
      INSERT INTO provider_capability_entries (generation, provider_id, canonical_model_id, entry_digest, capability_data)
      VALUES
        (1, 'anthropic', 'claude-sonnet', 'entry-a1', '{}'),
        (3, 'anthropic', 'claude-sonnet', 'entry-a3', '{}'),
        (3, 'google', 'gemini-3-flash-preview', 'entry-g3', '{}')
    `);
    await database.pool.query(`
      INSERT INTO product_catalog_versions (
        product_id, generation, schema_version, capability_generation, capability_digest,
        content_digest, actor_type, reason
      ) VALUES
        ('main', 1, 1, 1, 'capability-v1', 'sha256:v1:' || repeat('a', 64), 'migration', 'test'),
        ('main', 3, 1, 3, 'capability-v3', 'sha256:v1:' || repeat('b', 64), 'migration', 'test')
    `);
    await database.pool.query(`
      INSERT INTO product_catalog_entries (
        product_id, generation, capability_generation, provider_id, canonical_model_id, enabled
      ) VALUES
        ('main', 1, 1, 'anthropic', 'claude-sonnet', true),
        ('main', 3, 3, 'anthropic', 'claude-sonnet', true),
        ('main', 3, 3, 'google', 'gemini-3-flash-preview', true)
    `);
    await database.pool.query(`
      INSERT INTO provider_switch_versions (
        generation, schema_version, capability_generation, capability_digest, content_digest,
        actor_type, reason
      ) VALUES
        (1, 1, 1, 'capability-v1', 'sha256:v1:' || repeat('c', 64), 'migration', 'test'),
        (3, 1, 3, 'capability-v3', 'sha256:v1:' || repeat('d', 64), 'migration', 'test')
    `);
    await database.pool.query(`
      INSERT INTO provider_switch_entries (
        generation, provider_id, scope_type, product_id, segment, catalog_generation, enabled
      ) VALUES
        (1, 'anthropic', 'master', '', '', NULL, true),
        (3, 'anthropic', 'master', '', '', NULL, true),
        (3, 'google', 'product', 'main', '', 3, true)
    `);
  }, TEST_TIMEOUT_MS);

  afterAll(async () => {
    await database.pool.end();
    await admin.query(`DROP DATABASE ${quoteIdentifier(databaseName)}`);
    await admin.end();
  });

  it("stages catalog and switch jobs from stored versions and advances the heads", async () => {
    const catalogJobId = await stageStoredPricingCatalogControlJob(database, "main", 3, audit);
    const switchJobId = await stageStoredProviderSwitchControlJob(database, 3, audit);

    // Exact replay returns the same durable job without a second insert.
    await expect(stageStoredPricingCatalogControlJob(database, "main", 3, audit))
      .resolves.toBe(catalogJobId);
    await expect(stageStoredProviderSwitchControlJob(database, 3, audit))
      .resolves.toBe(switchJobId);

    const jobs = await database.pool.query<{ count: string }>(`
      SELECT count(*)::text AS count FROM engine_catalog_jobs WHERE product_id = 'main' AND generation = 3
    `);
    expect(jobs.rows[0]!.count).toBe("1");
    const switchJobs = await database.pool.query<{ count: string }>(`
      SELECT count(*)::text AS count FROM engine_switch_jobs WHERE generation = 3
    `);
    expect(switchJobs.rows[0]!.count).toBe("1");

    const catalogHead = await database.pool.query<{ active_generation: string }>(`
      SELECT active_generation::text FROM product_catalog_heads WHERE product_id = 'main'
    `);
    expect(catalogHead.rows[0]!.active_generation).toBe("3");
    const switchHead = await database.pool.query<{ active_generation: string }>(`
      SELECT active_generation::text FROM provider_switch_head WHERE singleton = 1
    `);
    expect(switchHead.rows[0]!.active_generation).toBe("3");

    const catalogPayload = await database.pool.query<{ payload: unknown }>(`
      SELECT payload FROM engine_catalog_jobs WHERE id = $1
    `, [catalogJobId]);
    expect(catalogPayload.rows[0]!.payload).toMatchObject({
      product_id: "main",
      generation: 3,
      entries: [
        { provider_id: "anthropic", canonical_model_id: "claude-sonnet", enabled: true },
        { provider_id: "google", canonical_model_id: "gemini-3-flash-preview", enabled: true },
      ],
    });

    const audits = await database.pool.query<{ action: string }>(`
      SELECT action FROM audit_log WHERE actor_id = $1 ORDER BY created_at
    `, [audit.actorId]);
    expect(audits.rows.map((row) => row.action)).toEqual([
      "pricing_catalog.convergence_staged",
      "provider_switches.convergence_staged",
    ]);
  }, TEST_TIMEOUT_MS);

  it("refuses a generation that commerce has never stored", async () => {
    await expect(stageStoredPricingCatalogControlJob(database, "main", 99, audit))
      .rejects.toBeInstanceOf(PricingControlJobStageError);
    await expect(stageStoredProviderSwitchControlJob(database, 99, audit))
      .rejects.toBeInstanceOf(PricingControlJobStageError);
  });

  it("rejects staging an older generation once the head advanced", async () => {
    await expect(stageStoredPricingCatalogControlJob(database, "main", 1, audit))
      .rejects.toThrow("catalog control target is stale");
    await expect(stageStoredProviderSwitchControlJob(database, 1, audit))
      .rejects.toThrow("provider-switch control target is stale");
  });
});
