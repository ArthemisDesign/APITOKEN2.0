import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import type {
  FundingNormalizationPlanV2,
  PricingReleaseInventoryAccountV2,
} from "@claude-api/contracts";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";

const sha256V2Pattern = /^sha256:v2:[0-9a-f]{64}$/;

export type FundingNormalizationParentStatus =
  | "pending"
  | "processing"
  | "retry"
  | "confirmed"
  | "dead";
export type FundingNormalizationAccountStatus =
  | "pending"
  | "processing"
  | "retry"
  | "ready"
  | "blocker";

export interface FundingNormalizationJobV2 {
  id: string;
  releaseGeneration: bigint;
  releaseDigest: string;
  payloadDigest: string;
  attempts: number;
  engineInventoryDigest: string;
  serviceInventoryDigest: string;
  fundingManifestDigest: string;
}

export interface FundingNormalizationQueueRowV2 {
  engineAccountId: string;
  fundingGeneration: bigint | null;
  expectedSourceDigest: string;
  targetFundingDigest: string | null;
  appliedFundingDigest: string | null;
  normalizationSource: FundingNormalizationPlanV2["source"] | null;
  blockers: FundingNormalizationPlanV2["blockers"] | null;
  status: FundingNormalizationAccountStatus;
  attempts: number;
  nextAttemptAt: Date;
  lockedAt: Date | null;
  lockedBy: string | null;
  lastError: string | null;
  updatedAt: Date;
}

export interface FundingNormalizationServiceInventoryRowV2 {
  serviceId: string;
  engineAccountId: string;
  purpose: string;
  responsible: string;
  status: "active" | "disabled";
  sourceVersion: bigint;
  contentDigest: string;
}

export interface FundingNormalizationStateV2 {
  release: {
    generation: bigint;
    releaseDigest: string;
    releaseKind: "target" | "recovery";
    status: "planned" | "materializing" | "prepared" | "active" | "superseded" | "failed";
    engineInventoryDigest: string;
    serviceInventoryDigest: string;
    fundingManifestDigest: string;
  };
  queue: FundingNormalizationQueueRowV2[];
  services: FundingNormalizationServiceInventoryRowV2[];
}

export interface FundingNormalizationCoverageV2 {
  balanceAccountIds: string[];
  serviceAccountIds: string[];
  missingAccountIds: string[];
  extraAccountIds: string[];
  dueBlockerAccountIds: string[];
  pendingCount: number;
  processingCount: number;
  retryCount: number;
  blockerCount: number;
  readyCount: number;
  engineInventoryDigest: string;
  serviceInventoryDigest: string;
}

export interface ClaimedFundingNormalizationAccountV2 {
  engineAccountId: string;
  attempts: number;
}

export type FundingNormalizationPlanObservationV2 =
  | "observed"
  | "stored"
  | "unchanged"
  | "stale"
  | "blocked"
  | "conflict";

export class FundingNormalizationJobV2Error extends Error {
  constructor(message: string, readonly terminal: boolean) {
    super(message);
    this.name = "FundingNormalizationJobV2Error";
  }
}

interface ParentJobRow {
  id: string;
  release_generation: string;
  release_digest: string;
  payload_digest: string;
  attempts: number;
  engine_inventory_digest: string;
  service_inventory_digest: string;
  funding_manifest_digest: string;
}

interface ReleaseRow {
  generation: string;
  release_digest: string;
  release_kind: "target" | "recovery";
  status: FundingNormalizationStateV2["release"]["status"];
  engine_inventory_digest: string;
  service_inventory_digest: string;
  funding_manifest_digest: string;
}

interface QueueRow {
  engine_account_id: string;
  funding_generation: string | null;
  expected_source_digest: string;
  target_funding_digest: string | null;
  applied_funding_digest: string | null;
  normalization_source: FundingNormalizationPlanV2["source"] | null;
  blockers: FundingNormalizationPlanV2["blockers"] | null;
  status: FundingNormalizationAccountStatus;
  attempts: number;
  next_attempt_at: Date;
  locked_at: Date | null;
  locked_by: string | null;
  last_error: string | null;
  updated_at: Date;
}

interface ServiceRow {
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
      Object.entries(value)
        .sort(([left], [right]) => compareUtf8(left, right))
        .map(([key, child]) => [key, canonicalValue(child)]),
    );
  }
  return value;
}

function canonicalDigest(scope: string, value: unknown): string {
  const canonical = JSON.stringify(canonicalValue({ scope, value }));
  return `sha256:v2:${createHash("sha256").update(canonical, "utf8").digest("hex")}`;
}

function assertPositiveDuration(value: number, label: string): void {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new RangeError(`${label} must be a positive safe integer`);
  }
}

function assertNonEmpty(value: string, label: string): void {
  if (value.length === 0) throw new TypeError(`${label} must not be empty`);
}

function parsePositiveBigInt(value: string, label: string): bigint {
  const parsed = BigInt(value);
  if (parsed <= 0n) throw new Error(`${label} must be positive`);
  return parsed;
}

function requireSha256V2(value: string, label: string): void {
  if (!sha256V2Pattern.test(value)) {
    throw new FundingNormalizationJobV2Error(`${label} is not a canonical sha256:v2 digest`, true);
  }
}

function inventoryIdentity(accounts: readonly PricingReleaseInventoryAccountV2[]): Array<{
  account_id: string;
  status: PricingReleaseInventoryAccountV2["status"];
  multiplier_bp: number;
}> {
  return accounts
    .map((account) => ({
      account_id: account.account_id,
      status: account.status,
      multiplier_bp: account.multiplier_bp,
    }))
    .sort((left, right) => compareUtf8(left.account_id, right.account_id));
}

/** Stable release coverage deliberately excludes live balance/reserved/spent and funding-head data. */
export function fundingNormalizationEngineInventoryDigestV2(
  accounts: readonly PricingReleaseInventoryAccountV2[],
): string {
  return canonicalDigest("pricing-funding-normalization-engine-inventory-v2", inventoryIdentity(accounts));
}

export function sameFundingNormalizationInventoryIdentityV2(
  left: readonly PricingReleaseInventoryAccountV2[],
  right: readonly PricingReleaseInventoryAccountV2[],
): boolean {
  return JSON.stringify(inventoryIdentity(left)) === JSON.stringify(inventoryIdentity(right));
}

export function fundingNormalizationServiceInventoryDigestV2(
  services: readonly FundingNormalizationServiceInventoryRowV2[],
): string {
  const manifest = services
    .map((service) => ({
      service_id: service.serviceId,
      engine_account_id: service.engineAccountId,
      purpose: service.purpose,
      responsible: service.responsible,
      status: service.status,
      source_version: service.sourceVersion.toString(),
      content_digest: service.contentDigest,
    }))
    .sort((left, right) => compareUtf8(left.service_id, right.service_id));
  return canonicalDigest("pricing-funding-normalization-service-inventory-v2", manifest);
}

export function fundingNormalizationManifestDigestV2(
  rows: readonly Pick<
    FundingNormalizationQueueRowV2,
    "engineAccountId" | "fundingGeneration" | "appliedFundingDigest"
  >[],
): string {
  const manifest = rows
    .map((row) => {
      if (row.fundingGeneration === null || row.appliedFundingDigest === null) {
        throw new FundingNormalizationJobV2Error(
          `funding manifest row ${row.engineAccountId} is not ready`,
          false,
        );
      }
      requireSha256V2(row.appliedFundingDigest, `funding digest for ${row.engineAccountId}`);
      return {
        account_id: row.engineAccountId,
        funding_generation: row.fundingGeneration.toString(),
        funding_digest: row.appliedFundingDigest,
      };
    })
    .sort((left, right) => compareUtf8(left.account_id, right.account_id));
  return canonicalDigest("pricing-funding-normalization-manifest-v2", manifest);
}

function fundingNormalizationPayloadDigestV2(release: ReleaseRow): string {
  return canonicalDigest("pricing-funding-normalization-job-v2", {
    release_generation: release.generation,
    release_digest: release.release_digest,
    engine_inventory_digest: release.engine_inventory_digest,
    service_inventory_digest: release.service_inventory_digest,
    funding_manifest_digest: release.funding_manifest_digest,
  });
}

function parentFromRow(row: ParentJobRow): FundingNormalizationJobV2 {
  return {
    id: row.id,
    releaseGeneration: parsePositiveBigInt(row.release_generation, "release generation"),
    releaseDigest: row.release_digest,
    payloadDigest: row.payload_digest,
    attempts: row.attempts,
    engineInventoryDigest: row.engine_inventory_digest,
    serviceInventoryDigest: row.service_inventory_digest,
    fundingManifestDigest: row.funding_manifest_digest,
  };
}

function queueFromRow(row: QueueRow): FundingNormalizationQueueRowV2 {
  return {
    engineAccountId: row.engine_account_id,
    fundingGeneration: row.funding_generation === null
      ? null
      : parsePositiveBigInt(row.funding_generation, "funding generation"),
    expectedSourceDigest: row.expected_source_digest,
    targetFundingDigest: row.target_funding_digest,
    appliedFundingDigest: row.applied_funding_digest,
    normalizationSource: row.normalization_source,
    blockers: row.blockers,
    status: row.status,
    attempts: row.attempts,
    nextAttemptAt: row.next_attempt_at,
    lockedAt: row.locked_at,
    lockedBy: row.locked_by,
    lastError: row.last_error,
    updatedAt: row.updated_at,
  };
}

function serviceFromRow(row: ServiceRow): FundingNormalizationServiceInventoryRowV2 {
  return {
    serviceId: row.service_id,
    engineAccountId: row.engine_account_id,
    purpose: row.purpose,
    responsible: row.responsible,
    status: row.status,
    sourceVersion: parsePositiveBigInt(row.source_version, "service source version"),
    contentDigest: row.content_digest,
  };
}

async function selectRelease(
  client: PoolClient,
  releaseGeneration: bigint,
  releaseDigest: string,
  lock: "" | "FOR SHARE" | "FOR UPDATE" = "FOR SHARE",
): Promise<ReleaseRow> {
  const result = await client.query<ReleaseRow>(`
    SELECT generation::text, content_digest AS release_digest, release_kind, status,
           engine_inventory_digest, service_inventory_digest, funding_manifest_digest
    FROM pricing_release_plans_v2
    WHERE generation = $1 AND content_digest = $2
    ${lock}
  `, [releaseGeneration, releaseDigest]);
  const row = result.rows[0];
  if (!row) {
    throw new FundingNormalizationJobV2Error("exact target release plan does not exist", true);
  }
  if (row.release_kind !== "target") {
    throw new FundingNormalizationJobV2Error("funding normalization requires a target release", true);
  }
  if (row.status === "active" || row.status === "superseded" || row.status === "failed") {
    throw new FundingNormalizationJobV2Error(
      `target release cannot normalize funding while status is ${row.status}`,
      true,
    );
  }
  requireSha256V2(row.engine_inventory_digest, "target engine inventory digest");
  requireSha256V2(row.service_inventory_digest, "target service inventory digest");
  requireSha256V2(row.funding_manifest_digest, "target funding manifest digest");
  return row;
}

export async function stageFundingNormalizationJobV2(
  database: Database,
  input: { releaseGeneration: bigint; releaseDigest: string },
): Promise<string> {
  if (input.releaseGeneration <= 0n) throw new RangeError("releaseGeneration must be positive");
  assertNonEmpty(input.releaseDigest, "releaseDigest");
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    const release = await selectRelease(client, input.releaseGeneration, input.releaseDigest, "FOR SHARE");
    const payloadDigest = fundingNormalizationPayloadDigestV2(release);
    const idempotencyKey = `pricing:v2:normalize-funding:${release.generation}:${release.release_digest}`;
    const existing = await client.query<{
      id: string;
      job_kind: string;
      release_generation: string;
      release_digest: string;
      payload_digest: string;
    }>(`
      SELECT id, job_kind, release_generation::text, release_digest, payload_digest
      FROM pricing_release_control_jobs_v2
      WHERE idempotency_key = $1
      FOR UPDATE
    `, [idempotencyKey]);
    const row = existing.rows[0];
    if (row) {
      if (
        row.job_kind !== "normalize_funding"
        || row.release_generation !== release.generation
        || row.release_digest !== release.release_digest
        || row.payload_digest !== payloadDigest
      ) {
        throw new FundingNormalizationJobV2Error(
          "funding normalization idempotency key has a different immutable payload",
          true,
        );
      }
      await client.query("COMMIT");
      return row.id;
    }
    const inserted = await client.query<{ id: string }>(`
      INSERT INTO pricing_release_control_jobs_v2 (
        job_kind, release_generation, release_digest, idempotency_key, payload_digest
      ) VALUES ('normalize_funding', $1, $2, $3, $4)
      RETURNING id
    `, [input.releaseGeneration, input.releaseDigest, idempotencyKey, payloadDigest]);
    await client.query("COMMIT");
    return inserted.rows[0]!.id;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

async function recoverExpiredFundingNormalizationLeases(
  client: PoolClient,
  leaseMs: number,
): Promise<{ parents: number; accounts: number }> {
  assertPositiveDuration(leaseMs, "leaseMs");
  const parents = await client.query(`
    UPDATE pricing_release_control_jobs_v2
    SET status = 'retry', locked_at = NULL, locked_by = NULL,
        next_attempt_at = now(),
        last_error = COALESCE(last_error, 'recovered expired funding-normalization parent lease'),
        updated_at = now()
    WHERE job_kind = 'normalize_funding' AND status = 'processing'
      AND (locked_at IS NULL OR locked_at < now() - $1 * interval '1 millisecond')
  `, [leaseMs]);
  const accounts = await client.query(`
    UPDATE pricing_funding_normalizations_v2 account
    SET status = 'retry', locked_at = NULL, locked_by = NULL,
        next_attempt_at = now(),
        last_error = COALESCE(last_error, 'recovered expired funding-normalization account lease'),
        updated_at = now()
    WHERE account.status = 'processing'
      AND (account.locked_at IS NULL OR account.locked_at < now() - $1 * interval '1 millisecond')
  `, [leaseMs]);
  return { parents: parents.rowCount ?? 0, accounts: accounts.rowCount ?? 0 };
}

export async function recoverStaleFundingNormalizationJobsV2(
  database: Database,
  leaseMs: number,
): Promise<{ parents: number; accounts: number }> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const recovered = await recoverExpiredFundingNormalizationLeases(client, leaseMs);
    await client.query("COMMIT");
    return recovered;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function claimNextFundingNormalizationJobV2(
  database: Database,
  workerId: string,
  leaseMs: number,
): Promise<FundingNormalizationJobV2 | null> {
  assertNonEmpty(workerId, "workerId");
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await recoverExpiredFundingNormalizationLeases(client, leaseMs);
    const claimed = await client.query<ParentJobRow>(`
      WITH candidate AS (
        SELECT job.id
        FROM pricing_release_control_jobs_v2 job
        JOIN pricing_release_plans_v2 release
          ON release.generation = job.release_generation
         AND release.content_digest = job.release_digest
         AND release.release_kind = 'target'
         AND release.status IN ('planned', 'materializing', 'prepared')
        WHERE job.job_kind = 'normalize_funding'
          AND job.status IN ('pending', 'retry')
          AND job.next_attempt_at <= now()
        ORDER BY job.next_attempt_at, job.created_at
        FOR UPDATE OF job SKIP LOCKED
        LIMIT 1
      )
      UPDATE pricing_release_control_jobs_v2 job
      SET status = 'processing', attempts = job.attempts + 1,
          locked_at = now(), locked_by = $1, updated_at = now()
      FROM candidate, pricing_release_plans_v2 release
      WHERE job.id = candidate.id
        AND release.generation = job.release_generation
        AND release.content_digest = job.release_digest
      RETURNING job.id, job.release_generation::text, job.release_digest,
                job.payload_digest, job.attempts,
                release.engine_inventory_digest, release.service_inventory_digest,
                release.funding_manifest_digest
    `, [workerId]);
    await client.query("COMMIT");
    const row = claimed.rows[0];
    if (!row) return null;
    const job = parentFromRow(row);
    requireSha256V2(job.engineInventoryDigest, "target engine inventory digest");
    requireSha256V2(job.serviceInventoryDigest, "target service inventory digest");
    requireSha256V2(job.fundingManifestDigest, "target funding manifest digest");
    return job;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function renewFundingNormalizationJobLeaseV2(
  database: Database,
  job: FundingNormalizationJobV2,
  workerId: string,
): Promise<void> {
  const renewed = await database.pool.query(`
    UPDATE pricing_release_control_jobs_v2
    SET locked_at = now(), updated_at = now()
    WHERE id = $1 AND job_kind = 'normalize_funding'
      AND status = 'processing' AND locked_by = $2
      AND release_generation = $3 AND release_digest = $4 AND payload_digest = $5
  `, [job.id, workerId, job.releaseGeneration, job.releaseDigest, job.payloadDigest]);
  if (renewed.rowCount !== 1) {
    throw new FundingNormalizationJobV2Error(`funding normalization job ${job.id} lost its lease`, false);
  }
}

async function selectFundingNormalizationState(
  client: PoolClient,
  job: FundingNormalizationJobV2,
  releaseLock: "" | "FOR SHARE" = "FOR SHARE",
): Promise<FundingNormalizationStateV2> {
  const release = await selectRelease(client, job.releaseGeneration, job.releaseDigest, releaseLock);
  const expectedPayload = fundingNormalizationPayloadDigestV2(release);
  if (expectedPayload !== job.payloadDigest) {
    throw new FundingNormalizationJobV2Error("target release changed behind immutable job payload", true);
  }
  const queue = await client.query<QueueRow>(`
    SELECT engine_account_id, funding_generation::text, expected_source_digest,
           target_funding_digest, applied_funding_digest, normalization_source,
           blockers, status, attempts, next_attempt_at, locked_at, locked_by,
           last_error, updated_at
    FROM pricing_funding_normalizations_v2
    WHERE release_generation = $1
    ORDER BY engine_account_id COLLATE "C"
  `, [job.releaseGeneration]);
  const services = await client.query<ServiceRow>(`
    SELECT service_id, engine_account_id, purpose, responsible, status,
           source_version::text, content_digest
    FROM service_account_inventory_v2
    ORDER BY service_id COLLATE "C"
  `);
  return {
    release: {
      generation: parsePositiveBigInt(release.generation, "release generation"),
      releaseDigest: release.release_digest,
      releaseKind: release.release_kind,
      status: release.status,
      engineInventoryDigest: release.engine_inventory_digest,
      serviceInventoryDigest: release.service_inventory_digest,
      fundingManifestDigest: release.funding_manifest_digest,
    },
    queue: queue.rows.map(queueFromRow),
    services: services.rows.map(serviceFromRow),
  };
}

export async function getFundingNormalizationStateV2(
  database: Database,
  job: FundingNormalizationJobV2,
): Promise<FundingNormalizationStateV2> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    const state = await selectFundingNormalizationState(client, job, "");
    await client.query("COMMIT");
    return state;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export function buildFundingNormalizationCoverageV2(
  inventory: readonly PricingReleaseInventoryAccountV2[],
  state: FundingNormalizationStateV2,
  now = new Date(),
): FundingNormalizationCoverageV2 {
  const inventoryById = new Map<string, PricingReleaseInventoryAccountV2>();
  for (const account of inventory) {
    if (inventoryById.has(account.account_id)) {
      throw new FundingNormalizationJobV2Error(`duplicate engine inventory account ${account.account_id}`, false);
    }
    inventoryById.set(account.account_id, account);
  }
  const serviceIds = new Set<string>();
  const serviceEngineIds = new Set<string>();
  for (const service of state.services) {
    if (serviceIds.has(service.serviceId) || serviceEngineIds.has(service.engineAccountId)) {
      throw new FundingNormalizationJobV2Error("service inventory contains a duplicate identity", true);
    }
    serviceIds.add(service.serviceId);
    serviceEngineIds.add(service.engineAccountId);
    const engine = inventoryById.get(service.engineAccountId);
    if (!engine) {
      throw new FundingNormalizationJobV2Error(
        `service account ${service.engineAccountId} is absent from engine inventory`,
        true,
      );
    }
    if (engine.status !== service.status) {
      throw new FundingNormalizationJobV2Error(
        `service account ${service.engineAccountId} status differs between inventories`,
        true,
      );
    }
  }

  const engineInventoryDigest = fundingNormalizationEngineInventoryDigestV2(inventory);
  const serviceInventoryDigest = fundingNormalizationServiceInventoryDigestV2(state.services);
  if (engineInventoryDigest !== state.release.engineInventoryDigest) {
    throw new FundingNormalizationJobV2Error(
      "engine identity inventory no longer matches the target release plan",
      true,
    );
  }
  if (serviceInventoryDigest !== state.release.serviceInventoryDigest) {
    throw new FundingNormalizationJobV2Error(
      "service inventory no longer matches the target release plan",
      true,
    );
  }

  const balanceAccountIds = [...inventoryById.keys()]
    .filter((accountId) => !serviceEngineIds.has(accountId))
    .sort(compareUtf8);
  const balanceSet = new Set(balanceAccountIds);
  const queueById = new Map(state.queue.map((row) => [row.engineAccountId, row]));
  const missingAccountIds = balanceAccountIds.filter((accountId) => !queueById.has(accountId));
  const extraAccountIds = state.queue
    .filter((row) => !balanceSet.has(row.engineAccountId))
    .map((row) => row.engineAccountId)
    .sort(compareUtf8);
  const dueBlockerAccountIds = state.queue
    .filter((row) => row.status === "blocker" && row.nextAttemptAt.getTime() <= now.getTime())
    .sort((left, right) =>
      left.nextAttemptAt.getTime() - right.nextAttemptAt.getTime()
      || compareUtf8(left.engineAccountId, right.engineAccountId))
    .map((row) => row.engineAccountId);
  const counts = {
    pendingCount: 0,
    processingCount: 0,
    retryCount: 0,
    blockerCount: 0,
    readyCount: 0,
  };
  for (const row of state.queue) {
    if (row.status === "pending") counts.pendingCount += 1;
    else if (row.status === "processing") counts.processingCount += 1;
    else if (row.status === "retry") counts.retryCount += 1;
    else if (row.status === "blocker") counts.blockerCount += 1;
    else counts.readyCount += 1;
  }
  return {
    balanceAccountIds,
    serviceAccountIds: [...serviceEngineIds].sort(compareUtf8),
    missingAccountIds,
    extraAccountIds,
    dueBlockerAccountIds,
    ...counts,
    engineInventoryDigest,
    serviceInventoryDigest,
  };
}

function planDisposition(
  plan: FundingNormalizationPlanV2,
  observation: FundingNormalizationPlanObservationV2,
): {
  status: FundingNormalizationAccountStatus;
  appliedDigest: string | null;
  lastError: string | null;
} {
  requireSha256V2(plan.source_state_digest, `source state digest for ${plan.account_id}`);
  if (plan.normalization_digest !== null) {
    requireSha256V2(plan.normalization_digest, `normalization digest for ${plan.account_id}`);
  }
  if (observation === "conflict") {
    return { status: "blocker", appliedDigest: null, lastError: "engine returned conflict" };
  }
  if (plan.status === "blocked") {
    const onlyActiveLegacy = plan.blockers.length > 0
      && plan.blockers.every((blocker) => blocker.code === "active_legacy_reservation");
    return {
      status: onlyActiveLegacy ? "retry" : "blocker",
      appliedDigest: null,
      lastError: plan.blockers.map((blocker) => `${blocker.code}: ${blocker.detail}`).join("; "),
    };
  }
  if (plan.normalization_digest === null || plan.funding_generation === null) {
    throw new FundingNormalizationJobV2Error(
      `engine returned incomplete target identity for ${plan.account_id}`,
      true,
    );
  }
  if (
    plan.status === "normalized"
    || observation === "stored"
    || observation === "unchanged"
  ) {
    return { status: "ready", appliedDigest: plan.normalization_digest, lastError: null };
  }
  return {
    status: observation === "observed" ? "pending" : "retry",
    appliedDigest: null,
    lastError: observation === "stale" ? "engine returned stale normalization plan" : null,
  };
}

export async function storeFundingNormalizationPlanV2(
  database: Database,
  job: FundingNormalizationJobV2,
  workerId: string,
  plan: FundingNormalizationPlanV2,
  observation: FundingNormalizationPlanObservationV2,
  retryMs: number,
): Promise<FundingNormalizationAccountStatus> {
  assertPositiveDuration(retryMs, "retryMs");
  const disposition = planDisposition(plan, observation);
  const blockers = plan.blockers.length === 0 ? null : plan.blockers;
  const targetDigest = plan.normalization_digest;
  const fundingGeneration = plan.funding_generation;
  const result = await database.pool.query<{ status: FundingNormalizationAccountStatus }>(`
    INSERT INTO pricing_funding_normalizations_v2 (
      release_generation, engine_account_id, funding_generation,
      expected_source_digest, target_funding_digest, applied_funding_digest,
      normalization_source, blockers, status, next_attempt_at, last_error
    ) VALUES (
      $1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9,
      CASE WHEN $9 IN ('retry', 'blocker') THEN now() + $10 * interval '1 millisecond' ELSE now() END,
      $11
    )
    ON CONFLICT (release_generation, engine_account_id) DO UPDATE
    SET funding_generation = EXCLUDED.funding_generation,
        expected_source_digest = EXCLUDED.expected_source_digest,
        target_funding_digest = EXCLUDED.target_funding_digest,
        applied_funding_digest = EXCLUDED.applied_funding_digest,
        normalization_source = EXCLUDED.normalization_source,
        blockers = EXCLUDED.blockers,
        status = EXCLUDED.status,
        next_attempt_at = EXCLUDED.next_attempt_at,
        locked_at = NULL, locked_by = NULL,
        last_error = EXCLUDED.last_error, updated_at = now()
    WHERE pricing_funding_normalizations_v2.status <> 'processing'
       OR pricing_funding_normalizations_v2.locked_by = $12
    RETURNING status
  `, [
    job.releaseGeneration,
    plan.account_id,
    fundingGeneration,
    plan.source_state_digest,
    targetDigest,
    disposition.appliedDigest,
    plan.source,
    blockers === null ? null : JSON.stringify(blockers),
    disposition.status,
    retryMs,
    disposition.lastError,
    workerId,
  ]);
  const row = result.rows[0];
  if (!row) {
    throw new FundingNormalizationJobV2Error(
      `funding normalization account ${plan.account_id} is leased by another worker`,
      false,
    );
  }
  return row.status;
}

export async function claimNextFundingNormalizationAccountV2(
  database: Database,
  job: FundingNormalizationJobV2,
  workerId: string,
): Promise<ClaimedFundingNormalizationAccountV2 | null> {
  const claimed = await database.pool.query<{
    engine_account_id: string;
    attempts: number;
  }>(`
    WITH parent AS (
      SELECT id
      FROM pricing_release_control_jobs_v2
      WHERE id = $1 AND status = 'processing' AND locked_by = $2
        AND release_generation = $3 AND release_digest = $4 AND payload_digest = $5
      FOR SHARE
    ), candidate AS (
      SELECT account.engine_account_id
      FROM pricing_funding_normalizations_v2 account, parent
      WHERE account.release_generation = $3
        AND account.status IN ('pending', 'retry')
        AND account.next_attempt_at <= now()
      ORDER BY account.next_attempt_at, account.created_at, account.engine_account_id COLLATE "C"
      FOR UPDATE OF account SKIP LOCKED
      LIMIT 1
    )
    UPDATE pricing_funding_normalizations_v2 account
    SET status = 'processing', attempts = account.attempts + 1,
        locked_at = now(), locked_by = $2, updated_at = now()
    FROM candidate
    WHERE account.release_generation = $3
      AND account.engine_account_id = candidate.engine_account_id
    RETURNING account.engine_account_id, account.attempts
  `, [job.id, workerId, job.releaseGeneration, job.releaseDigest, job.payloadDigest]);
  const row = claimed.rows[0];
  return row ? { engineAccountId: row.engine_account_id, attempts: row.attempts } : null;
}

export async function retryFundingNormalizationAccountV2(
  database: Database,
  job: FundingNormalizationJobV2,
  workerId: string,
  accountId: string,
  error: string,
  retryMs: number,
): Promise<void> {
  assertPositiveDuration(retryMs, "retryMs");
  const updated = await database.pool.query(`
    UPDATE pricing_funding_normalizations_v2
    SET status = 'retry', next_attempt_at = now() + $5 * interval '1 millisecond',
        locked_at = NULL, locked_by = NULL, last_error = $6, updated_at = now()
    WHERE release_generation = $1 AND engine_account_id = $2
      AND status = 'processing' AND locked_by = $3
      AND EXISTS (
        SELECT 1 FROM pricing_release_control_jobs_v2 parent
        WHERE parent.id = $4 AND parent.status = 'processing' AND parent.locked_by = $3
      )
  `, [job.releaseGeneration, accountId, workerId, job.id, retryMs, error]);
  if (updated.rowCount !== 1) {
    throw new FundingNormalizationJobV2Error(
      `funding normalization account ${accountId} lost its lease before retry`,
      false,
    );
  }
}

export async function retryFundingNormalizationJobV2(
  database: Database,
  job: FundingNormalizationJobV2,
  workerId: string,
  error: string,
  retryMs: number,
): Promise<void> {
  assertPositiveDuration(retryMs, "retryMs");
  const updated = await database.pool.query(`
    UPDATE pricing_release_control_jobs_v2
    SET status = 'retry', next_attempt_at = now() + $6 * interval '1 millisecond',
        locked_at = NULL, locked_by = NULL, last_error = $7, updated_at = now()
    WHERE id = $1 AND job_kind = 'normalize_funding'
      AND status = 'processing' AND locked_by = $2
      AND release_generation = $3 AND release_digest = $4 AND payload_digest = $5
  `, [
    job.id,
    workerId,
    job.releaseGeneration,
    job.releaseDigest,
    job.payloadDigest,
    retryMs,
    error,
  ]);
  if (updated.rowCount !== 1) {
    throw new FundingNormalizationJobV2Error(`funding normalization job ${job.id} lost its lease`, false);
  }
}

export async function failFundingNormalizationJobV2(
  database: Database,
  job: FundingNormalizationJobV2,
  workerId: string,
  error: string,
): Promise<void> {
  const updated = await database.pool.query(`
    UPDATE pricing_release_control_jobs_v2
    SET status = 'dead', locked_at = NULL, locked_by = NULL,
        last_error = $6, updated_at = now()
    WHERE id = $1 AND job_kind = 'normalize_funding'
      AND status = 'processing' AND locked_by = $2
      AND release_generation = $3 AND release_digest = $4 AND payload_digest = $5
  `, [job.id, workerId, job.releaseGeneration, job.releaseDigest, job.payloadDigest, error]);
  if (updated.rowCount !== 1) {
    throw new FundingNormalizationJobV2Error(`funding normalization job ${job.id} lost its lease`, false);
  }
}

function exactAccountSet(actual: readonly string[], expected: readonly string[]): boolean {
  if (actual.length !== expected.length) return false;
  const sortedActual = [...actual].sort(compareUtf8);
  const sortedExpected = [...expected].sort(compareUtf8);
  return sortedActual.every((accountId, index) => accountId === sortedExpected[index]);
}

export async function confirmFundingNormalizationJobV2(
  database: Database,
  job: FundingNormalizationJobV2,
  workerId: string,
  evidence: {
    engineInventory: readonly PricingReleaseInventoryAccountV2[];
  },
): Promise<string> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    const parent = await client.query<{ id: string }>(`
      SELECT id FROM pricing_release_control_jobs_v2
      WHERE id = $1 AND job_kind = 'normalize_funding'
        AND status = 'processing' AND locked_by = $2
        AND release_generation = $3 AND release_digest = $4 AND payload_digest = $5
      FOR UPDATE
    `, [job.id, workerId, job.releaseGeneration, job.releaseDigest, job.payloadDigest]);
    if (!parent.rows[0]) {
      throw new FundingNormalizationJobV2Error(`funding normalization job ${job.id} lost its lease`, false);
    }
    const state = await selectFundingNormalizationState(client, job);
    const coverage = buildFundingNormalizationCoverageV2(evidence.engineInventory, state);
    const queueIds = state.queue.map((row) => row.engineAccountId);
    if (!exactAccountSet(queueIds, coverage.balanceAccountIds)) {
      throw new FundingNormalizationJobV2Error(
        "final funding queue has missing or extra balance accounts",
        false,
      );
    }
    const incomplete = state.queue.filter((row) =>
      row.status !== "ready"
      || row.fundingGeneration === null
      || row.targetFundingDigest === null
      || row.appliedFundingDigest !== row.targetFundingDigest
      || row.blockers !== null);
    if (incomplete.length > 0) {
      throw new FundingNormalizationJobV2Error(
        `final funding queue still has ${incomplete.length} incomplete accounts`,
        false,
      );
    }
    const fundingManifestDigest = fundingNormalizationManifestDigestV2(state.queue);
    if (fundingManifestDigest !== state.release.fundingManifestDigest) {
      throw new FundingNormalizationJobV2Error(
        "applied funding manifest does not match the immutable target release plan",
        true,
      );
    }
    const resultDigest = canonicalDigest("pricing-funding-normalization-result-v2", {
      release_generation: job.releaseGeneration.toString(),
      release_digest: job.releaseDigest,
      engine_inventory_digest: coverage.engineInventoryDigest,
      service_inventory_digest: coverage.serviceInventoryDigest,
      funding_manifest_digest: fundingManifestDigest,
      balance_account_ids: coverage.balanceAccountIds,
    });
    const confirmed = await client.query(`
      UPDATE pricing_release_control_jobs_v2
      SET status = 'confirmed', result_digest = $2, confirmed_at = now(),
          locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
      WHERE id = $1 AND status = 'processing'
    `, [job.id, resultDigest]);
    if (confirmed.rowCount !== 1) {
      throw new FundingNormalizationJobV2Error(`funding normalization job ${job.id} lost its lease`, false);
    }
    await client.query("COMMIT");
    return resultDigest;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
