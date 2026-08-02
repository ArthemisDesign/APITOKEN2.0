import "server-only";
import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import {
  openKeysPricingInventoryAccountV2Schema,
  openKeysPricingInventoryPageV2Schema,
  type OpenKeysPricingInventoryAccountV2,
  type OpenKeysPricingInventoryPageV2,
} from "@claude-api/contracts";
import type { OpenkeysDatabase } from "@claude-api/openkeys-db";
import { getDatabase } from "./db";

export interface OpenKeysPricingInventorySourceRowV2 {
  sourceId: string;
  engineAccountId: string;
  status: "active" | "disabled";
  removed: boolean;
  pricingContract: "legacy" | "official_1_to_1";
  sourceMultiplierBp: number;
}

export interface OpenKeysPricingInventoryOptionsV2 {
  afterAccountId?: string;
  limit?: number;
}

interface StoredInventoryRow {
  source_id: string;
  engine_account_id: string;
  status: "active" | "disabled";
  removed: boolean;
  pricing_contract: "legacy" | "official_1_to_1";
  source_multiplier_bp: number;
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function canonicalValue(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalValue);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .sort(([left], [right]) => compareUtf8(left, right))
        .map(([key, child]) => [key, canonicalValue(child)]),
    );
  }
  return value;
}

function digest(scope: string, value: unknown): string {
  const canonical = JSON.stringify(canonicalValue({ scope, value }));
  return `sha256:v2:${createHash("sha256").update(canonical, "utf8").digest("hex")}`;
}

function assertOptions(options: OpenKeysPricingInventoryOptionsV2): {
  afterAccountId: string | undefined;
  limit: number;
} {
  const limit = options.limit ?? 500;
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > 500) {
    throw new RangeError("OpenKeys pricing inventory limit must be within 1..=500");
  }
  const afterAccountId = options.afterAccountId;
  if (
    afterAccountId !== undefined
    && (!afterAccountId.startsWith("acct_") || afterAccountId.length > 200)
  ) {
    throw new TypeError("OpenKeys pricing inventory cursor is invalid");
  }
  return { afterAccountId, limit };
}

function inventoryAccount(row: OpenKeysPricingInventorySourceRowV2): OpenKeysPricingInventoryAccountV2 {
  const identity = {
    account_id: row.engineAccountId,
    source_id: row.sourceId,
    lifecycle: row.removed ? "removed" as const : row.status,
    pricing_contract: row.pricingContract,
    source_multiplier_bp: row.sourceMultiplierBp,
  };
  return openKeysPricingInventoryAccountV2Schema.parse({
    ...identity,
    content_digest: digest("openkeys-pricing-inventory-account-v2", identity),
  });
}

export function buildOpenKeysPricingInventoryPageV2(
  sourceRows: readonly OpenKeysPricingInventorySourceRowV2[],
  options: OpenKeysPricingInventoryOptionsV2 = {},
): OpenKeysPricingInventoryPageV2 {
  const { afterAccountId, limit } = assertOptions(options);
  const accounts = sourceRows
    .map(inventoryAccount)
    .sort((left, right) => compareUtf8(left.account_id, right.account_id));
  for (let index = 1; index < accounts.length; index += 1) {
    if (accounts[index - 1]!.account_id === accounts[index]!.account_id) {
      throw new Error(`duplicate OpenKeys engine account ${accounts[index]!.account_id}`);
    }
  }
  const inventoryDigest = digest("openkeys-pricing-inventory-manifest-v2", accounts);
  const remaining = afterAccountId === undefined
    ? accounts
    : accounts.filter((account) => compareUtf8(account.account_id, afterAccountId) > 0);
  const pageAccounts = remaining.slice(0, limit);
  const nextAfterAccountId = remaining.length > limit
    ? pageAccounts.at(-1)!.account_id
    : null;
  return openKeysPricingInventoryPageV2Schema.parse({
    inventory_digest: inventoryDigest,
    accounts: pageAccounts,
    next_after_account_id: nextAfterAccountId,
  });
}

export async function loadOpenKeysPricingInventoryPageV2(
  options: OpenKeysPricingInventoryOptionsV2 = {},
  database: OpenkeysDatabase = getDatabase(),
): Promise<OpenKeysPricingInventoryPageV2> {
  assertOptions(options);
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    const result = await client.query<StoredInventoryRow>(`
      SELECT source.source_id, source.engine_account_id, source.status, source.removed,
             source.pricing_contract, source.source_multiplier_bp
      FROM (
        SELECT key.id::text AS source_id, key.engine_account_id,
               key.status::text AS status, key.removed_at IS NOT NULL AS removed,
               key.pricing_contract, key.mult_bp AS source_multiplier_bp
        FROM openkeys_keys key

        UNION ALL

        SELECT job.id::text AS source_id, job.engine_account_id,
               CASE WHEN job.status = 'compensated' THEN 'disabled' ELSE 'active' END AS status,
               false AS removed, batch.pricing_contract,
               batch.mult_bp AS source_multiplier_bp
        FROM openkeys_issuance_jobs job
        JOIN openkeys_batches batch ON batch.id = job.batch_id
        WHERE job.engine_account_id IS NOT NULL
          AND NOT EXISTS (
            SELECT 1 FROM openkeys_keys key
            WHERE key.engine_account_id = job.engine_account_id
          )
      ) source
      ORDER BY source.engine_account_id COLLATE "C"
    `);
    const page = buildOpenKeysPricingInventoryPageV2(result.rows.map((row) => ({
      sourceId: row.source_id,
      engineAccountId: row.engine_account_id,
      status: row.status,
      removed: row.removed,
      pricingContract: row.pricing_contract,
      sourceMultiplierBp: row.source_multiplier_bp,
    })), options);
    await client.query("COMMIT");
    return page;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
