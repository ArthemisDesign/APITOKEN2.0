import { randomBytes, randomUUID } from "node:crypto";
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
import { fileURLToPath } from "node:url";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { describe, expect, it } from "vitest";

const connectionString = process.env.TEST_OPENKEYS_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;
const MIGRATIONS_FOLDER = fileURLToPath(
  new URL("../../../../packages/openkeys-db/migrations", import.meta.url),
);

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
}

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) {
    throw new Error(`unsafe PostgreSQL identifier: ${identifier}`);
  }
  return `"${identifier}"`;
}

function newViewToken(): string {
  return randomBytes(16).toString("base64url");
}

async function createTemporaryDatabase(): Promise<TemporaryDatabase> {
  if (!connectionString) throw new Error("TEST_OPENKEYS_DATABASE_URL is required");

  const databaseName = `ok_contract_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 12)}`;
  const admin = new Client({ connectionString });
  await admin.connect();
  await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);

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

async function createMigrationsThrough0006(): Promise<string> {
  const folder = await mkdtemp(join(tmpdir(), "openkeys-migrations-0006-"));
  const metadataFolder = join(folder, "meta");
  await mkdir(metadataFolder);

  const journal = JSON.parse(
    await readFile(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
  ) as Journal;
  const selectedEntries = journal.entries.filter((entry) => entry.idx <= 6);
  expect(selectedEntries.at(-1)?.tag).toBe("0006_openkeys_api_type");

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

async function expectDatabaseFailure(
  client: Client,
  action: () => Promise<void>,
  expected: PgFailure,
): Promise<void> {
  await client.query("BEGIN");
  let failure: PgFailure | undefined;
  try {
    await action();
  } catch (error) {
    failure = error as PgFailure;
  } finally {
    await client.query("ROLLBACK");
  }
  expect(failure, "mutation unexpectedly succeeded").toBeDefined();
  expect(failure).toMatchObject(expected);
}

async function insertBatch(
  client: Client,
  multBp: number,
  pricingContract?: "legacy" | "official_1_to_1",
): Promise<string> {
  const id = randomUUID();
  if (pricingContract === undefined) {
    await client.query(`
      INSERT INTO openkeys_batches (
        id, face_value_nano, mult_bp, quantity, created_by
      ) VALUES ($1, 50000000000, $2, 1, 'migration-test')
    `, [id, multBp]);
  } else {
    await client.query(`
      INSERT INTO openkeys_batches (
        id, face_value_nano, mult_bp, pricing_contract, quantity, created_by
      ) VALUES ($1, 50000000000, $2, $3, 1, 'migration-test')
    `, [id, multBp, pricingContract]);
  }
  return id;
}

async function insertKey(
  client: Client,
  batchId: string,
  multBp: number,
  pricingContract?: "legacy" | "official_1_to_1",
): Promise<string> {
  const id = randomUUID();
  const values = [
    id,
    batchId,
    newViewToken(),
    `engine-account-${id}`,
    `engine-key-${id}`,
    `sk-pool-test…${id.slice(-4)}`,
    multBp,
  ];
  if (pricingContract === undefined) {
    await client.query(`
      INSERT INTO openkeys_keys (
        id, batch_id, view_token, engine_account_id, engine_key_id,
        key_masked, face_value_nano, mult_bp
      ) VALUES ($1, $2, $3, $4, $5, $6, 50000000000, $7)
    `, values);
  } else {
    await client.query(`
      INSERT INTO openkeys_keys (
        id, batch_id, view_token, engine_account_id, engine_key_id,
        key_masked, face_value_nano, mult_bp, pricing_contract
      ) VALUES ($1, $2, $3, $4, $5, $6, 50000000000, $7, $8)
    `, [...values, pricingContract]);
  }
  return id;
}

describe("OpenKeys pricing-contract migration declaration", () => {
  it("is the next schema-only migration with a legacy-compatible default", async () => {
    const migrationSql = await readFile(
      join(MIGRATIONS_FOLDER, "0007_openkeys_pricing_contract_expand.sql"),
      "utf8",
    );
    const journal = JSON.parse(
      await readFile(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as Journal;

    expect(journal.entries.at(-1)).toMatchObject({
      idx: 7,
      version: "7",
      tag: "0007_openkeys_pricing_contract_expand",
      breakpoints: true,
    });
    expect(journal.entries.at(-1)!.when).toBeGreaterThan(journal.entries.at(-2)!.when);
    expect(migrationSql).toContain(
      'ADD COLUMN "pricing_contract" text DEFAULT \'legacy\' NOT NULL',
    );
    expect(migrationSql).toContain(
      `CHECK ("pricing_contract" <> 'official_1_to_1' OR "mult_bp" = 10000)`,
    );
    expect(migrationSql).toContain('"openkeys_keys_batch_contract_fk"');
    expect(migrationSql).toContain("NOT VALID");
    expect(migrationSql).not.toContain("VALIDATE CONSTRAINT");
    expect(migrationSql).not.toMatch(
      /^(?:INSERT|UPDATE|DELETE|TRUNCATE|DROP|CREATE FUNCTION|CREATE TRIGGER)\b/im,
    );
  });
});

describe.runIf(Boolean(connectionString))("OpenKeys pricing-contract migration", () => {
  it("preserves legacy rows and enforces 1:1 without breaking the old writer", async () => {
    const legacyMigrations = await createMigrationsThrough0006();
    const database = await createTemporaryDatabase();
    try {
      await migrate(drizzle(database.client), { migrationsFolder: legacyMigrations });
      const legacyBatchId = await insertBatch(database.client, 4000);
      await insertKey(database.client, legacyBatchId, 4000);
      const oneToOneLegacyBatchId = await insertBatch(database.client, 10_000);
      await insertKey(database.client, oneToOneLegacyBatchId, 10_000);

      const before = await database.client.query<{ batches: string; keys: string }>(`
        SELECT
          (
            SELECT COALESCE(
              jsonb_agg(to_jsonb(batch_row) ORDER BY batch_row.id),
              '[]'::jsonb
            )::text
            FROM openkeys_batches AS batch_row
          ) AS batches,
          (
            SELECT COALESCE(
              jsonb_agg(to_jsonb(key_row) ORDER BY key_row.id),
              '[]'::jsonb
            )::text
            FROM openkeys_keys AS key_row
          ) AS keys
      `);
      const migrationsBefore = await migrationCount(database.client);

      await migrate(drizzle(database.client), { migrationsFolder: MIGRATIONS_FOLDER });
      expect(await migrationCount(database.client)).toBe(migrationsBefore + 1);

      const after = await database.client.query<{ batches: string; keys: string }>(`
        SELECT
          (
            SELECT COALESCE(
              jsonb_agg(to_jsonb(batch_row) - 'pricing_contract' ORDER BY batch_row.id),
              '[]'::jsonb
            )::text
            FROM openkeys_batches AS batch_row
          ) AS batches,
          (
            SELECT COALESCE(
              jsonb_agg(to_jsonb(key_row) - 'pricing_contract' ORDER BY key_row.id),
              '[]'::jsonb
            )::text
            FROM openkeys_keys AS key_row
          ) AS keys
      `);
      expect(after.rows).toEqual(before.rows);

      const backfilled = await database.client.query<{
        legacy_batches: number;
        legacy_keys: number;
      }>(`
        SELECT
          (SELECT count(*)::int FROM openkeys_batches WHERE pricing_contract = 'legacy')
            AS legacy_batches,
          (SELECT count(*)::int FROM openkeys_keys WHERE pricing_contract = 'legacy')
            AS legacy_keys
      `);
      expect(backfilled.rows).toEqual([{ legacy_batches: 2, legacy_keys: 2 }]);

      const oldWriterBatchId = await insertBatch(database.client, 3750);
      const oldWriterKeyId = await insertKey(database.client, oldWriterBatchId, 3750);
      const oldWriterResult = await database.client.query<{
        batch_contract: string;
        key_contract: string;
      }>(`
        SELECT
          batch.pricing_contract AS batch_contract,
          key.pricing_contract AS key_contract
        FROM openkeys_keys AS key
        JOIN openkeys_batches AS batch ON batch.id = key.batch_id
        WHERE key.id = $1
      `, [oldWriterKeyId]);
      expect(oldWriterResult.rows).toEqual([{
        batch_contract: "legacy",
        key_contract: "legacy",
      }]);

      const officialBatchId = await insertBatch(
        database.client,
        10_000,
        "official_1_to_1",
      );
      const officialKeyId = await insertKey(
        database.client,
        officialBatchId,
        10_000,
        "official_1_to_1",
      );

      await expectDatabaseFailure(database.client, async () => {
        await insertKey(database.client, officialBatchId, 10_000);
      }, {
        code: "23503",
        constraint: "openkeys_keys_batch_contract_fk",
      });
      await expectDatabaseFailure(database.client, async () => {
        await insertBatch(database.client, 9999, "official_1_to_1");
      }, {
        code: "23514",
        constraint: "openkeys_batches_official_1_to_1",
      });
      await expectDatabaseFailure(database.client, async () => {
        await insertKey(database.client, officialBatchId, 9999, "official_1_to_1");
      }, {
        code: "23514",
        constraint: "openkeys_keys_official_1_to_1",
      });
      await expectDatabaseFailure(database.client, async () => {
        await database.client.query(
          "UPDATE openkeys_keys SET mult_bp = 9999 WHERE id = $1",
          [officialKeyId],
        );
      }, {
        code: "23514",
        constraint: "openkeys_keys_official_1_to_1",
      });

      await migrate(drizzle(database.client), { migrationsFolder: MIGRATIONS_FOLDER });
      expect(await migrationCount(database.client)).toBe(migrationsBefore + 1);
    } finally {
      await database.close();
      await rm(legacyMigrations, { recursive: true, force: true });
    }
  }, TEST_TIMEOUT_MS);
});
