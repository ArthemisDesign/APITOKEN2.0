import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import {
  PRICING_RELEASE_SCHEMA_VERSION_V2,
  serviceAccountInventoryEntryV2Schema,
  serviceAccountInventoryV2Schema,
  type PricingReleaseInventoryAccountV2,
  type ServiceAccountInventoryEntryV2,
  type ServiceAccountInventoryV2,
} from "@claude-api/contracts";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";

export type ServiceAccountInventoryWriteStatusV2 = "stored" | "unchanged";

export interface ServiceAccountInventoryMutationInputV2 {
  serviceId: string;
  expectedSourceVersion: number | null;
  expectedContentDigest: string | null;
  engineAccountId: string;
  purpose: string;
  responsible: string;
  status: "active" | "disabled";
  engineInventoryDigest: string;
  actorId: string;
  reason: string;
}

export interface ServiceAccountInventoryMutationResultV2 {
  status: ServiceAccountInventoryWriteStatusV2;
  account: ServiceAccountInventoryEntryV2;
  inventory: ServiceAccountInventoryV2;
  engine_inventory_digest: string;
}

export class ServiceAccountInventoryV2Error extends Error {
  constructor(
    public readonly code:
      | "version_conflict"
      | "account_owned_by_commerce"
      | "engine_account_already_registered"
      | "concurrent_update",
    message: string,
  ) {
    super(message);
    this.name = "ServiceAccountInventoryV2Error";
  }
}

interface StoredServiceAccountRowV2 {
  service_id: string;
  engine_account_id: string;
  purpose: string;
  responsible: string;
  status: "active" | "disabled";
  source_version: string;
  content_digest: string;
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function canonicalValue(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalValue);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .filter(([, child]) => child !== undefined)
        .sort(([left], [right]) => compareUtf8(left, right))
        .map(([key, child]) => [key, canonicalValue(child)]),
    );
  }
  return value;
}

export function serviceAccountInventoryDigestV2(label: string, value: unknown): string {
  const digest = createHash("sha256")
    .update(`pricing-service-account-inventory-v2:${label}\n`, "utf8")
    .update(JSON.stringify(canonicalValue(value)), "utf8")
    .digest("hex");
  return `sha256:v2:${digest}`;
}

export function engineAccountIdentityInventoryDigestV2(
  accounts: readonly Pick<PricingReleaseInventoryAccountV2, "account_id" | "status" | "multiplier_bp">[],
): string {
  const identities = accounts
    .map((account) => ({
      account_id: account.account_id,
      status: account.status,
      multiplier_bp: account.multiplier_bp,
    }))
    .sort((left, right) => compareUtf8(left.account_id, right.account_id));
  return serviceAccountInventoryDigestV2("engine-identity-inventory", identities);
}

function positiveSafeVersion(value: string, label: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0 || String(parsed) !== value) {
    throw new Error(`${label} is not a positive safe integer`);
  }
  return parsed;
}

function entryFromRow(row: StoredServiceAccountRowV2): ServiceAccountInventoryEntryV2 {
  return {
    service_id: row.service_id,
    engine_account_id: row.engine_account_id,
    purpose: row.purpose,
    responsible: row.responsible,
    status: row.status,
    source_version: positiveSafeVersion(row.source_version, "service source version"),
    content_digest: row.content_digest,
  };
}

function buildEntry(
  input: Pick<
    ServiceAccountInventoryMutationInputV2,
    "serviceId" | "engineAccountId" | "purpose" | "responsible" | "status"
  >,
  sourceVersion: number,
): ServiceAccountInventoryEntryV2 {
  const identity = {
    service_id: input.serviceId,
    engine_account_id: input.engineAccountId,
    purpose: input.purpose,
    responsible: input.responsible,
    status: input.status,
    source_version: sourceVersion,
  };
  return serviceAccountInventoryEntryV2Schema.parse({
    ...identity,
    content_digest: serviceAccountInventoryDigestV2("entry", identity),
  });
}

function buildInventory(accounts: readonly ServiceAccountInventoryEntryV2[]): ServiceAccountInventoryV2 {
  const sorted = [...accounts].sort((left, right) => compareUtf8(left.service_id, right.service_id));
  const identity = {
    schema_version: PRICING_RELEASE_SCHEMA_VERSION_V2,
    accounts: sorted,
  };
  return serviceAccountInventoryV2Schema.parse({
    ...identity,
    inventory_digest: serviceAccountInventoryDigestV2("manifest", identity),
  });
}

async function readRows(client: PoolClient): Promise<ServiceAccountInventoryEntryV2[]> {
  const rows = await client.query<StoredServiceAccountRowV2>(`
    SELECT service_id, engine_account_id, purpose, responsible, status,
           source_version::text, content_digest
    FROM service_account_inventory_v2
    ORDER BY service_id COLLATE "C"
  `);
  return rows.rows.map(entryFromRow);
}

export async function readServiceAccountInventoryV2(database: Database): Promise<ServiceAccountInventoryV2> {
  const client = await database.pool.connect();
  try {
    return buildInventory(await readRows(client));
  } finally {
    client.release();
  }
}

function sameServiceState(
  current: ServiceAccountInventoryEntryV2,
  input: ServiceAccountInventoryMutationInputV2,
): boolean {
  return current.engine_account_id === input.engineAccountId
    && current.purpose === input.purpose
    && current.responsible === input.responsible
    && current.status === input.status;
}

export async function upsertServiceAccountInventoryV2(
  database: Database,
  input: ServiceAccountInventoryMutationInputV2,
): Promise<ServiceAccountInventoryMutationResultV2> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    const currentResult = await client.query<StoredServiceAccountRowV2>(`
      SELECT service_id, engine_account_id, purpose, responsible, status,
             source_version::text, content_digest
      FROM service_account_inventory_v2
      WHERE service_id = $1
      FOR UPDATE
    `, [input.serviceId]);
    const current = currentResult.rows[0] ? entryFromRow(currentResult.rows[0]) : null;

    const commerceOwner = await client.query<{ id: string }>(`
      SELECT id::text
      FROM engine_accounts
      WHERE engine_account_id = $1
      LIMIT 1
    `, [input.engineAccountId]);
    if (commerceOwner.rows[0]) {
      throw new ServiceAccountInventoryV2Error(
        "account_owned_by_commerce",
        `engine account ${input.engineAccountId} already belongs to a commerce customer`,
      );
    }

    const duplicate = await client.query<{ service_id: string }>(`
      SELECT service_id
      FROM service_account_inventory_v2
      WHERE engine_account_id = $1 AND service_id <> $2
      FOR UPDATE
    `, [input.engineAccountId, input.serviceId]);
    if (duplicate.rows[0]) {
      throw new ServiceAccountInventoryV2Error(
        "engine_account_already_registered",
        `engine account ${input.engineAccountId} is already registered as ${duplicate.rows[0].service_id}`,
      );
    }

    if (current && sameServiceState(current, input)) {
      const inventory = buildInventory(await readRows(client));
      await client.query("COMMIT");
      return {
        status: "unchanged",
        account: current,
        inventory,
        engine_inventory_digest: input.engineInventoryDigest,
      };
    }

    const expectedMatches = current === null
      ? input.expectedSourceVersion === null && input.expectedContentDigest === null
      : current.source_version === input.expectedSourceVersion
        && current.content_digest === input.expectedContentDigest;
    if (!expectedMatches) {
      throw new ServiceAccountInventoryV2Error(
        "version_conflict",
        `service account inventory ${input.serviceId} changed since it was read`,
      );
    }

    const nextVersion = current === null ? 1 : current.source_version + 1;
    const next = buildEntry(input, nextVersion);
    await client.query(`
      INSERT INTO service_account_inventory_v2 (
        service_id, engine_account_id, purpose, responsible, status,
        source_version, content_digest
      ) VALUES ($1, $2, $3, $4, $5, $6, $7)
      ON CONFLICT (service_id) DO UPDATE SET
        engine_account_id = EXCLUDED.engine_account_id,
        purpose = EXCLUDED.purpose,
        responsible = EXCLUDED.responsible,
        status = EXCLUDED.status,
        source_version = EXCLUDED.source_version,
        content_digest = EXCLUDED.content_digest,
        updated_at = now()
    `, [
      next.service_id,
      next.engine_account_id,
      next.purpose,
      next.responsible,
      next.status,
      next.source_version,
      next.content_digest,
    ]);
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'pricing.service_account_inventory.updated',
              'service_account', $2, $3::jsonb)
    `, [input.actorId, input.serviceId, JSON.stringify({
      previousSourceVersion: current?.source_version ?? null,
      previousContentDigest: current?.content_digest ?? null,
      sourceVersion: next.source_version,
      contentDigest: next.content_digest,
      engineAccountId: next.engine_account_id,
      engineStatus: next.status,
      engineInventoryDigest: input.engineInventoryDigest,
      purpose: next.purpose,
      responsible: next.responsible,
      reason: input.reason,
    })]);
    const inventory = buildInventory(await readRows(client));
    await client.query("COMMIT");
    return {
      status: "stored",
      account: next,
      inventory,
      engine_inventory_digest: input.engineInventoryDigest,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    if (
      error instanceof ServiceAccountInventoryV2Error
      || !(error instanceof Error)
    ) {
      throw error;
    }
    const code = (error as Error & { code?: string }).code;
    if (code === "40001" || code === "23505") {
      throw new ServiceAccountInventoryV2Error(
        "concurrent_update",
        "service account inventory changed concurrently; read the current inventory and retry",
      );
    }
    throw error;
  } finally {
    client.release();
  }
}
