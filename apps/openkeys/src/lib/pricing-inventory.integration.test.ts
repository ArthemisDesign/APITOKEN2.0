import { randomBytes, randomUUID } from "node:crypto";
import { fileURLToPath } from "node:url";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { createOpenkeysDatabase, type OpenkeysDatabase } from "@claude-api/openkeys-db";
import { Client } from "pg";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { loadOpenKeysPricingInventoryPageV2 } from "./pricing-inventory";

const connectionString = process.env.TEST_OPENKEYS_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;
const MIGRATIONS_FOLDER = fileURLToPath(
  new URL("../../../../packages/openkeys-db/migrations", import.meta.url),
);

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

describe.runIf(Boolean(connectionString))("OpenKeys pricing inventory PostgreSQL producer", () => {
  let admin: Client;
  let seed: Client;
  let database: OpenkeysDatabase;
  let databaseName: string;

  beforeAll(async () => {
    databaseName = `ok_inventory_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 12)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    seed = new Client({ connectionString: url.toString() });
    await seed.connect();
    await migrate(drizzle(seed), { migrationsFolder: MIGRATIONS_FOLDER });
    database = createOpenkeysDatabase(url.toString(), "openkeys-pricing-inventory-test");

    const legacyBatch = randomUUID();
    const officialBatch = randomUUID();
    await seed.query(`
      INSERT INTO openkeys_batches (
        id, face_value_nano, mult_bp, pricing_contract, quantity, created_by
      ) VALUES
        ($1, 5000000000, 4000, 'legacy', 1, 'inventory-test'),
        ($2, 5000000000, 10000, 'official_1_to_1', 1, 'inventory-test')
    `, [legacyBatch, officialBatch]);
    await seed.query(`
      INSERT INTO openkeys_keys (
        id, batch_id, view_token, engine_account_id, engine_key_id, key_masked,
        face_value_nano, mult_bp, pricing_contract, status,
        disabled_at, removed_at, removed_by, removal_reason
      ) VALUES
        ($1, $2, $3, 'acct_openkeys_b', 'key-b', 'sk…b', 5000000000, 4000, 'legacy',
         'disabled', now(), now(), 'inventory-test', 'test removal'),
        ($4, $5, $6, 'acct_openkeys_a', 'key-a', 'sk…a', 5000000000, 10000, 'official_1_to_1',
         'active', NULL, NULL, NULL, NULL)
    `, [
      randomUUID(),
      legacyBatch,
      randomBytes(16).toString("base64url"),
      randomUUID(),
      officialBatch,
      randomBytes(16).toString("base64url"),
    ]);
    await seed.query(`
      INSERT INTO openkeys_issuance_jobs (
        id, batch_id, item_index, status, engine_account_id, engine_key_id
      ) VALUES
        ($1, $2, 1, 'account_created', 'acct_openkeys_c', NULL),
        ($3, $4, 1, 'compensated', 'acct_openkeys_d', NULL),
        ($5, $2, 2, 'completed', 'acct_openkeys_a', 'key-a')
    `, [randomUUID(), officialBatch, randomUUID(), legacyBatch, randomUUID()]);
  }, TEST_TIMEOUT_MS);

  afterAll(async () => {
    await database?.pool.end();
    await seed?.end();
    if (admin) {
      await admin.query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1 AND pid <> pg_backend_pid()",
        [databaseName],
      );
      await admin.query(`DROP DATABASE IF EXISTS ${quoteIdentifier(databaseName)}`);
      await admin.end();
    }
  }, TEST_TIMEOUT_MS);

  it("reads key and issuance-journal accounts without duplicates or secrets", async () => {
    const pages: Awaited<ReturnType<typeof loadOpenKeysPricingInventoryPageV2>>[] = [];
    let afterAccountId: string | undefined;
    do {
      const page = await loadOpenKeysPricingInventoryPageV2({
        ...(afterAccountId === undefined ? {} : { afterAccountId }),
        limit: 1,
      }, database);
      pages.push(page);
      afterAccountId = page.next_after_account_id ?? undefined;
    } while (afterAccountId !== undefined);
    const accounts = pages.flatMap((page) => page.accounts);

    expect(accounts).toHaveLength(4);
    const [first, second, third, fourth] = accounts;
    expect(first).toEqual(expect.objectContaining({
      account_id: "acct_openkeys_a",
      lifecycle: "active",
      pricing_contract: "official_1_to_1",
      source_multiplier_bp: 10_000,
    }));
    expect(second).toEqual(expect.objectContaining({
      account_id: "acct_openkeys_b",
      lifecycle: "removed",
      pricing_contract: "legacy",
      source_multiplier_bp: 4000,
    }));
    expect(third).toEqual(expect.objectContaining({
      account_id: "acct_openkeys_c",
      lifecycle: "active",
      pricing_contract: "official_1_to_1",
      source_multiplier_bp: 10_000,
    }));
    expect(fourth).toEqual(expect.objectContaining({
      account_id: "acct_openkeys_d",
      lifecycle: "disabled",
      pricing_contract: "legacy",
      source_multiplier_bp: 4000,
    }));
    expect(new Set(pages.map((page) => page.inventory_digest))).toHaveLength(1);
    expect(Object.keys(first!).sort()).toEqual([
      "account_id",
      "content_digest",
      "lifecycle",
      "pricing_contract",
      "source_id",
      "source_multiplier_bp",
    ]);
  });
});
