import { randomUUID } from "node:crypto";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";
import {
  readServiceAccountInventoryV2,
  upsertServiceAccountInventoryV2,
} from "./service-account-inventory-v2.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;
const ENGINE_INVENTORY_DIGEST = `sha256:v2:${"a".repeat(64)}`;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

describe.runIf(Boolean(connectionString))("service-account inventory v2 authority", () => {
  let admin: Client;
  let seedClient: Client;
  let database: Database;
  let databaseName: string;

  beforeAll(async () => {
    databaseName = `service_inventory_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 8)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    seedClient = new Client({ connectionString: url.toString() });
    await seedClient.connect();
    await migrate(drizzle(seedClient), { migrationsFolder: MIGRATIONS_FOLDER });
    database = createDatabase(url.toString(), "service-inventory-v2-test");
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

  const baseInput = {
    serviceId: "crm-parsing",
    expectedSourceVersion: null,
    expectedContentDigest: null,
    engineAccountId: "acct_service_crm",
    purpose: "CRM ingestion and parsing",
    responsible: "platform",
    status: "active" as const,
    engineInventoryDigest: ENGINE_INVENTORY_DIGEST,
    actorId: "owner@example.test",
    reason: "register the existing engine-native service account",
  };

  it("stores a versioned row, replays exact state, and updates only through exact CAS", async () => {
    const stored = await upsertServiceAccountInventoryV2(database, baseInput);
    expect(stored).toMatchObject({
      status: "stored",
      account: {
        service_id: "crm-parsing",
        engine_account_id: "acct_service_crm",
        source_version: 1,
      },
      engine_inventory_digest: ENGINE_INVENTORY_DIGEST,
    });
    expect(stored.account.content_digest).toMatch(/^sha256:v2:[0-9a-f]{64}$/);
    expect(stored.inventory.inventory_digest).toMatch(/^sha256:v2:[0-9a-f]{64}$/);

    const replay = await upsertServiceAccountInventoryV2(database, baseInput);
    expect(replay).toMatchObject({ status: "unchanged", account: stored.account });

    const updated = await upsertServiceAccountInventoryV2(database, {
      ...baseInput,
      expectedSourceVersion: stored.account.source_version,
      expectedContentDigest: stored.account.content_digest,
      purpose: "CRM ingestion, parsing, and enrichment",
      reason: "record the complete current workload",
    });
    expect(updated).toMatchObject({ status: "stored", account: { source_version: 2 } });
    expect(updated.account.content_digest).not.toBe(stored.account.content_digest);

    await expect(upsertServiceAccountInventoryV2(database, {
      ...baseInput,
      expectedSourceVersion: stored.account.source_version,
      expectedContentDigest: stored.account.content_digest,
      purpose: "stale overwrite",
    })).rejects.toMatchObject({ code: "version_conflict" });

    const audit = await seedClient.query<{ count: string }>(`
      SELECT count(*)::text AS count
      FROM audit_log
      WHERE action = 'pricing.service_account_inventory.updated'
    `);
    expect(audit.rows[0]?.count).toBe("2");
    await expect(readServiceAccountInventoryV2(database)).resolves.toMatchObject({
      schema_version: 2,
      accounts: [updated.account],
    });
  });

  it("rejects commerce ownership and duplicate service identities before mutation", async () => {
    const userId = randomUUID();
    await seedClient.query(`
      INSERT INTO users (id, email, display_name, status)
      VALUES ($1, $2, 'Owned user', 'active')
    `, [userId, `${userId}@example.test`]);
    await seedClient.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, 'acct_commerce_owned', 4000, 'active')
    `, [randomUUID(), userId]);

    await expect(upsertServiceAccountInventoryV2(database, {
      ...baseInput,
      engineAccountId: "acct_commerce_owned",
    })).rejects.toMatchObject({ code: "account_owned_by_commerce" });

    const first = await upsertServiceAccountInventoryV2(database, baseInput);
    await expect(upsertServiceAccountInventoryV2(database, {
      ...baseInput,
      serviceId: "duplicate-crm",
    })).rejects.toMatchObject({ code: "engine_account_already_registered" });
    expect(first.account.source_version).toBe(1);
  });
});
