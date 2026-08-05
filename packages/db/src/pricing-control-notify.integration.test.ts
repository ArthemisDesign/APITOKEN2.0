import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { Client } from "pg";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { createDatabase, type Database } from "./client.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";
import { stageStoredPricingCatalogControlJob } from "./pricing-control-jobs.js";
import { PricingControlNotifyListener } from "./pricing-control-notify.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) {
    throw new Error(`unsafe PostgreSQL identifier: ${identifier}`);
  }
  return `"${identifier}"`;
}

describe.runIf(Boolean(connectionString))("pricing-control NOTIFY delivery", () => {
  let database: Database;
  let admin: Client;
  let databaseName: string;
  let targetUrl: URL;
  const audit = { actorId: "operator:notify-test", reason: "verify committed jobs wake listeners" };

  beforeAll(async () => {
    databaseName = ["pcn", process.pid, randomUUID().replaceAll("-", "").slice(0, 12)].join("_");
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    targetUrl = new URL(connectionString!);
    targetUrl.pathname = `/${databaseName}`;
    const target = new Client({ connectionString: targetUrl.toString() });
    await target.connect();
    await migrate(drizzle(target), { migrationsFolder: MIGRATIONS_FOLDER });
    await target.end();

    database = createDatabase(targetUrl.toString(), "pricing-control-notify-test");
    await database.pool.query(`
      INSERT INTO provider_capability_versions (generation, schema_version, content_digest, observed_at)
      VALUES (1, 1, 'capability-v1', now())
    `);
    await database.pool.query(`
      INSERT INTO provider_capability_entries (generation, provider_id, canonical_model_id, entry_digest, capability_data)
      VALUES (1, 'anthropic', 'claude-sonnet', 'entry-a1', '{}')
    `);
    await database.pool.query(`
      INSERT INTO product_catalog_versions (
        product_id, generation, schema_version, capability_generation, capability_digest,
        content_digest, actor_type, reason
      ) VALUES ('main', 1, 1, 1, 'capability-v1', 'sha256:v1:' || repeat('a', 64), 'migration', 'test')
    `);
    await database.pool.query(`
      INSERT INTO product_catalog_entries (
        product_id, generation, capability_generation, provider_id, canonical_model_id, enabled
      ) VALUES ('main', 1, 1, 'anthropic', 'claude-sonnet', true)
    `);
  }, TEST_TIMEOUT_MS);

  afterAll(async () => {
    await database.pool.end();
    await admin.query(`DROP DATABASE ${quoteIdentifier(databaseName)}`);
    await admin.end();
  });

  it("wakes a connected listener for a committed job insert with the table payload", async () => {
    const wakes: string[] = [];
    const listener = new PricingControlNotifyListener(targetUrl.toString(), {
      onWake: (table) => wakes.push(table),
      reconnectDelaysMs: [100],
    });
    listener.start();
    try {
      // Staging inserts the durable job and commits; the trigger NOTIFY follows on commit.
      await stageStoredPricingCatalogControlJob(database, "main", 1, audit);
      await expect
        .poll(() => wakes.length, { timeout: 10_000, interval: 50 })
        .toBeGreaterThan(0);
      expect(wakes).toContain("engine_catalog_jobs");
    } finally {
      await listener.stop();
    }
  }, TEST_TIMEOUT_MS);
});
