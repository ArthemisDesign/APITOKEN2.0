import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import type {
  FundingNormalizationPlanV2,
  PricingReleaseInventoryAccountV2,
  PricingReleaseRecoveryLinkV2,
  PricingReleaseV2,
} from "@claude-api/contracts";
import {
  MAIN_PRICING_PRODUCT_ID,
  OPENKEYS_PRICING_PRODUCT_ID,
  PRICING_RELEASE_SCHEMA_VERSION_V2,
  pricingCatalogSpecSchema,
  pricingReleaseRecoveryLinkV2Schema,
  pricingReleaseV2Schema,
  providerSwitchSpecSchema,
} from "@claude-api/contracts";
import type { EngineClient } from "@claude-api/engine-client";
import type { PoolClient } from "pg";
import { z } from "zod";
import type { Database } from "./client.js";
import {
  buildStage5ServiceInventoryV2,
  stage5V2CanonicalJson,
  stage5V2Digest,
  stage5V2EngineIdentityDigest,
} from "./pricing-stage5-materializer-v2.js";

const sha256V2Pattern = /^sha256:v2:[0-9a-f]{64}$/;
const sha256V2Schema = z.string().regex(sha256V2Pattern);

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
  fundingManifestDigest: string | null;
  stage5RunId: string;
  stage5PlanDigest: string;
  fundingPlanDigest: string;
  recoveryGeneration: bigint;
  recoveryPlanDigest: string;
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
    fundingManifestDigest: string | null;
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
  funding_manifest_digest: string | null;
  stage5_run_id: string;
  stage5_plan_digest: string;
  funding_plan_digest: string;
  recovery_generation: string;
  recovery_plan_digest: string;
}

interface ReleaseRow {
  generation: string;
  release_digest: string;
  release_kind: "target" | "recovery";
  status: FundingNormalizationStateV2["release"]["status"];
  engine_inventory_digest: string;
  commerce_inventory_digest: string;
  openkeys_inventory_digest: string;
  service_inventory_digest: string;
  policy_manifest_digest: string;
  assignment_manifest_digest: string;
  funding_manifest_digest: string | null;
  engine_release_digest: string | null;
}

interface Stage5RunRow {
  run_id: string;
  plan_digest: string;
  funding_plan_digest: string;
  target_generation: string;
  recovery_generation: string;
  target_digest: string | null;
  recovery_digest: string | null;
  plan_artifact: unknown;
  blocker_count: string;
  status: "blocked" | "planned" | "materializing" | "prepared" | "failed";
}

interface ReleaseAssignmentRow {
  release_generation: string;
  engine_account_id: string;
  account_class: "b2c" | "b2b" | "openkeys" | "service";
  owner_context: "commerce" | "openkeys" | "service";
  owner_id: string;
  policy_id: string;
  policy_version: string;
  policy_digest: string;
  billing_mode: "balance" | "meter_only";
  funding_generation: string | null;
  purpose: string | null;
  responsible: string | null;
  assignment_digest: string;
}

interface Stage6PrepareAck {
  artifact_kind: "target_release" | "recovery_release" | "recovery_link";
  artifact_id: string;
  artifact_version: number;
  expected_digest: string;
  mutation_result: "stored" | "unchanged";
  readback_digest: string;
}

export interface FundingNormalizationReleaseBundleV2 {
  stage5RunId: string;
  target: PricingReleaseV2;
  recovery: PricingReleaseV2;
  recoveryLink: PricingReleaseRecoveryLinkV2;
  fundingManifestDigest: string;
}

export type FundingNormalizationReleaseEngineV2 = Pick<
  EngineClient,
  | "preparePricingReleaseV2"
  | "getPricingReleaseV2"
  | "preparePricingReleaseRecoveryLinkV2"
  | "getPricingReleaseRecoveryLinkV2"
>;

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

function positiveSafeNumber(value: bigint | string, label: string): number {
  const parsed = typeof value === "bigint" ? value : BigInt(value);
  const number = Number(parsed);
  if (!Number.isSafeInteger(number) || number <= 0 || BigInt(number) !== parsed) {
    throw new FundingNormalizationJobV2Error(`${label} must be a positive safe integer`, true);
  }
  return number;
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
  return stage5V2EngineIdentityDigest(accounts);
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
  return buildStage5ServiceInventoryV2(services.map((service) => ({
    service_id: service.serviceId,
    engine_account_id: service.engineAccountId,
    purpose: service.purpose,
    responsible: service.responsible,
    status: service.status,
    source_version: positiveSafeNumber(service.sourceVersion, `service ${service.serviceId} source version`),
    content_digest: service.contentDigest,
  }))).inventory_digest;
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

function fundingNormalizationPayloadDigestV2(
  run: Stage5RunRow,
  target: ReleaseRow,
  recovery: ReleaseRow,
): string {
  return canonicalDigest("pricing-funding-normalization-job-v2", {
    stage5_run_id: run.run_id,
    stage5_plan_digest: run.plan_digest,
    funding_plan_digest: run.funding_plan_digest,
    target_generation: target.generation,
    target_plan_digest: target.release_digest,
    recovery_generation: recovery.generation,
    recovery_plan_digest: recovery.release_digest,
    engine_inventory_digest: target.engine_inventory_digest,
    service_inventory_digest: target.service_inventory_digest,
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
    stage5RunId: row.stage5_run_id,
    stage5PlanDigest: row.stage5_plan_digest,
    fundingPlanDigest: row.funding_plan_digest,
    recoveryGeneration: parsePositiveBigInt(row.recovery_generation, "recovery generation"),
    recoveryPlanDigest: row.recovery_plan_digest,
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
  expectedKind: "target" | "recovery" = "target",
  lock: "" | "FOR SHARE" | "FOR UPDATE" = "FOR SHARE",
): Promise<ReleaseRow> {
  const result = await client.query<ReleaseRow>(`
    SELECT generation::text, content_digest AS release_digest, release_kind, status,
           commerce_inventory_digest, engine_inventory_digest, openkeys_inventory_digest,
           service_inventory_digest, policy_manifest_digest, assignment_manifest_digest,
           funding_manifest_digest, engine_release_digest
    FROM pricing_release_plans_v2
    WHERE generation = $1 AND content_digest = $2
    ${lock}
  `, [releaseGeneration, releaseDigest]);
  const row = result.rows[0];
  if (!row) {
    throw new FundingNormalizationJobV2Error("exact target release plan does not exist", true);
  }
  if (row.release_kind !== expectedKind) {
    throw new FundingNormalizationJobV2Error(
      `funding normalization requires a ${expectedKind} release`,
      true,
    );
  }
  if (row.status === "active" || row.status === "superseded" || row.status === "failed") {
    throw new FundingNormalizationJobV2Error(
      `${expectedKind} release cannot normalize funding while status is ${row.status}`,
      true,
    );
  }
  requireSha256V2(row.engine_inventory_digest, `${expectedKind} engine inventory digest`);
  requireSha256V2(row.service_inventory_digest, `${expectedKind} service inventory digest`);
  if (row.funding_manifest_digest !== null) {
    requireSha256V2(row.funding_manifest_digest, `${expectedKind} funding manifest digest`);
  }
  if (row.engine_release_digest !== null) {
    requireSha256V2(row.engine_release_digest, `${expectedKind} engine release digest`);
  }
  return row;
}

async function selectStage5Run(
  client: PoolClient,
  planDigest: string,
  lock: "" | "FOR SHARE" | "FOR UPDATE" = "FOR SHARE",
): Promise<Stage5RunRow> {
  const result = await client.query<Stage5RunRow>(`
    SELECT run_id::text, plan_digest, funding_plan_digest,
           target_generation::text, recovery_generation::text,
           target_digest, recovery_digest, plan_artifact, blocker_count::text, status
    FROM pricing_stage5_runs_v2
    WHERE plan_digest = $1
    ${lock}
  `, [planDigest]);
  const row = result.rows[0];
  if (!row) throw new FundingNormalizationJobV2Error("exact Stage 5 plan does not exist", true);
  requireSha256V2(row.plan_digest, "Stage 5 plan digest");
  requireSha256V2(row.funding_plan_digest, "Stage 5 funding plan digest");
  if (row.blocker_count !== "0" || row.status === "blocked" || row.status === "failed") {
    throw new FundingNormalizationJobV2Error(
      `Stage 5 plan cannot normalize funding while status is ${row.status}`,
      true,
    );
  }
  return row;
}

export async function stageFundingNormalizationJobV2(
  database: Database,
  input: { planDigest: string },
): Promise<string> {
  requireSha256V2(input.planDigest, "Stage 5 plan digest");
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query("SELECT pg_advisory_xact_lock(hashtextextended('pricing-stage6-v2:stage', 0))");
    const run = await selectStage5Run(client, input.planDigest, "FOR UPDATE");
    const targetGeneration = parsePositiveBigInt(run.target_generation, "target generation");
    const recoveryGeneration = parsePositiveBigInt(run.recovery_generation, "recovery generation");
    const planRows = await client.query<{
      generation: string;
      release_kind: "target" | "recovery";
      content_digest: string;
    }>(`
      SELECT generation::text, release_kind, content_digest
      FROM pricing_release_plans_v2
      WHERE generation IN ($1, $2)
      ORDER BY generation
    `, [targetGeneration, recoveryGeneration]);
    const targetDigest = planRows.rows.find((row) => row.release_kind === "target")?.content_digest;
    const recoveryDigest = planRows.rows.find((row) => row.release_kind === "recovery")?.content_digest;
    if (!targetDigest || !recoveryDigest || planRows.rows.length !== 2) {
      throw new FundingNormalizationJobV2Error("Stage 5 target or recovery skeleton is missing", true);
    }
    const target = await selectRelease(client, targetGeneration, targetDigest, "target", "FOR UPDATE");
    const recovery = await selectRelease(client, recoveryGeneration, recoveryDigest, "recovery", "FOR UPDATE");
    if (
      target.engine_inventory_digest !== recovery.engine_inventory_digest
      || target.service_inventory_digest !== recovery.service_inventory_digest
      || target.commerce_inventory_digest !== recovery.commerce_inventory_digest
      || target.openkeys_inventory_digest !== recovery.openkeys_inventory_digest
      || target.policy_manifest_digest !== recovery.policy_manifest_digest
    ) {
      throw new FundingNormalizationJobV2Error("Stage 5 target and recovery lineage differs", true);
    }
    const payloadDigest = fundingNormalizationPayloadDigestV2(run, target, recovery);
    const idempotencyKey = `pricing:v2:normalize-funding:${run.plan_digest}`;
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
        || row.release_generation !== target.generation
        || row.release_digest !== target.release_digest
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
    if (run.status !== "materializing" || run.target_digest !== null || run.recovery_digest !== null) {
      throw new FundingNormalizationJobV2Error(
        "Stage 6 requires one fully ACKed, unfinalized Stage 5 materialization",
        true,
      );
    }
    const inserted = await client.query<{ id: string }>(`
      INSERT INTO pricing_release_control_jobs_v2 (
        job_kind, release_generation, release_digest, idempotency_key, payload_digest
      ) VALUES ('normalize_funding', $1, $2, $3, $4)
      RETURNING id
    `, [targetGeneration, target.release_digest, idempotencyKey, payloadDigest]);
    await client.query(`
      UPDATE pricing_stage5_runs_v2
      SET status = 'materializing', updated_at = now()
      WHERE run_id = $1 AND status IN ('planned', 'materializing')
    `, [run.run_id]);
    await client.query(`
      UPDATE pricing_release_plans_v2
      SET status = 'materializing', updated_at = now()
      WHERE generation IN ($1, $2) AND status IN ('planned', 'materializing')
    `, [targetGeneration, recoveryGeneration]);
    await client.query("COMMIT");
    return inserted.rows[0]!.id;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export interface FundingNormalizationStageStatusV2 {
  stage5_plan_digest: string;
  stage5_status: Stage5RunRow["status"];
  target_generation: string;
  target_plan_digest: string;
  target_release_digest: string | null;
  target_status: FundingNormalizationStateV2["release"]["status"];
  recovery_generation: string;
  recovery_plan_digest: string;
  recovery_release_digest: string | null;
  recovery_status: FundingNormalizationStateV2["release"]["status"];
  job_id: string | null;
  job_status: FundingNormalizationParentStatus | null;
  job_attempts: number | null;
  job_last_error: string | null;
  job_result_digest: string | null;
  pending_accounts: number;
  processing_accounts: number;
  retry_accounts: number;
  ready_accounts: number;
  blocker_accounts: number;
  target_funding_manifest_digest: string | null;
  recovery_funding_manifest_digest: string | null;
}

export async function getFundingNormalizationStageStatusV2(
  database: Database,
  planDigest: string,
): Promise<FundingNormalizationStageStatusV2> {
  requireSha256V2(planDigest, "Stage 5 plan digest");
  const result = await database.pool.query<{
    stage5_plan_digest: string;
    stage5_status: Stage5RunRow["status"];
    target_generation: string;
    target_plan_digest: string;
    target_release_digest: string | null;
    target_status: FundingNormalizationStateV2["release"]["status"];
    recovery_generation: string;
    recovery_plan_digest: string;
    recovery_release_digest: string | null;
    recovery_status: FundingNormalizationStateV2["release"]["status"];
    job_id: string | null;
    job_status: FundingNormalizationParentStatus | null;
    job_attempts: number | null;
    job_last_error: string | null;
    job_result_digest: string | null;
    pending_accounts: string;
    processing_accounts: string;
    retry_accounts: string;
    ready_accounts: string;
    blocker_accounts: string;
    target_funding_manifest_digest: string | null;
    recovery_funding_manifest_digest: string | null;
  }>(`
    SELECT run.plan_digest AS stage5_plan_digest, run.status AS stage5_status,
           run.target_generation::text, target.content_digest AS target_plan_digest,
           run.target_digest AS target_release_digest, target.status AS target_status,
           run.recovery_generation::text, recovery.content_digest AS recovery_plan_digest,
           run.recovery_digest AS recovery_release_digest, recovery.status AS recovery_status,
           job.id::text AS job_id, job.status AS job_status,
           job.attempts AS job_attempts, job.last_error AS job_last_error,
           job.result_digest AS job_result_digest,
           count(normalization.engine_account_id) FILTER (WHERE normalization.status = 'pending')::text AS pending_accounts,
           count(normalization.engine_account_id) FILTER (WHERE normalization.status = 'processing')::text AS processing_accounts,
           count(normalization.engine_account_id) FILTER (WHERE normalization.status = 'retry')::text AS retry_accounts,
           count(normalization.engine_account_id) FILTER (WHERE normalization.status = 'ready')::text AS ready_accounts,
           count(normalization.engine_account_id) FILTER (WHERE normalization.status = 'blocker')::text AS blocker_accounts,
           target.funding_manifest_digest AS target_funding_manifest_digest,
           recovery.funding_manifest_digest AS recovery_funding_manifest_digest
    FROM pricing_stage5_runs_v2 run
    JOIN pricing_release_plans_v2 target ON target.generation = run.target_generation
    JOIN pricing_release_plans_v2 recovery ON recovery.generation = run.recovery_generation
    LEFT JOIN pricing_release_control_jobs_v2 job
      ON job.job_kind = 'normalize_funding'
     AND job.release_generation = run.target_generation
     AND job.release_digest = target.content_digest
     AND job.idempotency_key = 'pricing:v2:normalize-funding:' || run.plan_digest
    LEFT JOIN pricing_funding_normalizations_v2 normalization
      ON normalization.release_generation = run.target_generation
    WHERE run.plan_digest = $1
    GROUP BY run.run_id, target.generation, recovery.generation, job.id
  `, [planDigest]);
  const row = result.rows[0];
  if (!row) throw new FundingNormalizationJobV2Error("exact Stage 5 plan does not exist", true);
  const toCount = (value: string): number => {
    const count = Number(value);
    if (!Number.isSafeInteger(count) || count < 0) {
      throw new FundingNormalizationJobV2Error("Stage 6 status count is invalid", true);
    }
    return count;
  };
  return {
    ...row,
    pending_accounts: toCount(row.pending_accounts),
    processing_accounts: toCount(row.processing_accounts),
    retry_accounts: toCount(row.retry_accounts),
    ready_accounts: toCount(row.ready_accounts),
    blocker_accounts: toCount(row.blocker_accounts),
  };
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
      FROM candidate, pricing_release_plans_v2 release,
           pricing_stage5_runs_v2 stage5,
           pricing_release_plans_v2 recovery
      WHERE job.id = candidate.id
        AND release.generation = job.release_generation
        AND release.content_digest = job.release_digest
        AND stage5.target_generation = release.generation
        AND job.idempotency_key = 'pricing:v2:normalize-funding:' || stage5.plan_digest
        AND stage5.status IN ('materializing', 'prepared')
        AND recovery.generation = stage5.recovery_generation
        AND recovery.release_kind = 'recovery'
      RETURNING job.id, job.release_generation::text, job.release_digest,
                job.payload_digest, job.attempts,
                release.engine_inventory_digest, release.service_inventory_digest,
                release.funding_manifest_digest,
                stage5.run_id::text AS stage5_run_id,
                stage5.plan_digest AS stage5_plan_digest,
                stage5.funding_plan_digest,
                stage5.recovery_generation::text,
                recovery.content_digest AS recovery_plan_digest
    `, [workerId]);
    await client.query("COMMIT");
    const row = claimed.rows[0];
    if (!row) return null;
    const job = parentFromRow(row);
    requireSha256V2(job.engineInventoryDigest, "target engine inventory digest");
    requireSha256V2(job.serviceInventoryDigest, "target service inventory digest");
    if (job.fundingManifestDigest !== null) {
      requireSha256V2(job.fundingManifestDigest, "target funding manifest digest");
    }
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
  releaseLock: "" | "FOR SHARE" | "FOR UPDATE" = "FOR SHARE",
): Promise<FundingNormalizationStateV2> {
  const release = await selectRelease(client, job.releaseGeneration, job.releaseDigest, "target", releaseLock);
  const run = await selectStage5Run(client, job.stage5PlanDigest, releaseLock);
  const recovery = await selectRelease(
    client,
    job.recoveryGeneration,
    job.recoveryPlanDigest,
    "recovery",
    releaseLock,
  );
  if (
    run.run_id !== job.stage5RunId
    || run.target_generation !== job.releaseGeneration.toString()
    || run.recovery_generation !== job.recoveryGeneration.toString()
  ) {
    throw new FundingNormalizationJobV2Error("Stage 5 lineage changed behind the normalization job", true);
  }
  const expectedPayload = fundingNormalizationPayloadDigestV2(run, release, recovery);
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
    )
    SELECT
      $1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9,
      CASE WHEN $9 IN ('retry', 'blocker') THEN now() + $10 * interval '1 millisecond' ELSE now() END,
      $11
    FROM pricing_release_control_jobs_v2 parent
    WHERE parent.id = $13 AND parent.job_kind = 'normalize_funding'
      AND parent.status = 'processing' AND parent.locked_by = $12
      AND parent.release_generation = $1
      AND parent.release_digest = $14 AND parent.payload_digest = $15
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
    job.id,
    job.releaseDigest,
    job.payloadDigest,
  ]);
  const row = result.rows[0];
  if (!row) {
    throw new FundingNormalizationJobV2Error(
      `funding normalization job or account ${plan.account_id} lost its lease`,
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

const stage5FinalizationArtifactSchema = z.object({
  schema_version: z.literal(PRICING_RELEASE_SCHEMA_VERSION_V2),
  plan_digest: sha256V2Schema,
  funding_plan_digest: sha256V2Schema,
  target_generation: z.number().int().safe().positive(),
  recovery_generation: z.number().int().safe().positive(),
  capability: z.object({
    generation: z.number().int().safe().positive(),
    content_digest: z.string().min(1),
  }).passthrough(),
  catalogs: z.array(pricingCatalogSpecSchema).length(2),
  switches: providerSwitchSpecSchema,
  target: z.object({
    generation: z.number().int().safe().positive(),
    release_kind: z.literal("target"),
    content_digest: sha256V2Schema,
  }).passthrough(),
  recovery: z.object({
    generation: z.number().int().safe().positive(),
    release_kind: z.literal("recovery"),
    content_digest: sha256V2Schema,
  }).passthrough(),
}).passthrough();

function sameCanonical(left: unknown, right: unknown): boolean {
  return stage5V2CanonicalJson(left) === stage5V2CanonicalJson(right);
}

async function selectReleaseAssignments(
  client: PoolClient,
  generation: bigint,
): Promise<ReleaseAssignmentRow[]> {
  const result = await client.query<ReleaseAssignmentRow>(`
    SELECT release_generation::text, engine_account_id, account_class, owner_context, owner_id,
           policy_id, policy_version::text, policy_digest, billing_mode,
           funding_generation::text, purpose, responsible, assignment_digest
    FROM pricing_release_assignments_v2
    WHERE release_generation = $1
    ORDER BY engine_account_id COLLATE "C"
  `, [generation]);
  return result.rows;
}

function comparableAssignment(row: ReleaseAssignmentRow): Record<string, unknown> {
  return {
    engine_account_id: row.engine_account_id,
    account_class: row.account_class,
    owner_context: row.owner_context,
    owner_id: row.owner_id,
    policy_id: row.policy_id,
    policy_version: row.policy_version,
    policy_digest: row.policy_digest,
    billing_mode: row.billing_mode,
    purpose: row.purpose,
    responsible: row.responsible,
  };
}

function assertReleaseGraphsMatch(
  target: readonly ReleaseAssignmentRow[],
  recovery: readonly ReleaseAssignmentRow[],
): void {
  if (!sameCanonical(target.map(comparableAssignment), recovery.map(comparableAssignment))) {
    throw new FundingNormalizationJobV2Error(
      "Stage 5 target and recovery assignment graphs differ",
      true,
    );
  }
}

function buildEngineReleaseV2(
  run: Stage5RunRow,
  release: ReleaseRow,
  assignments: readonly ReleaseAssignmentRow[],
): PricingReleaseV2 {
  const artifact = stage5FinalizationArtifactSchema.parse(run.plan_artifact);
  const artifactRelease = release.release_kind === "target" ? artifact.target : artifact.recovery;
  if (
    artifact.plan_digest !== run.plan_digest
    || artifact.funding_plan_digest !== run.funding_plan_digest
    || artifact.target_generation !== positiveSafeNumber(run.target_generation, "target generation")
    || artifact.recovery_generation !== positiveSafeNumber(run.recovery_generation, "recovery generation")
    || artifactRelease.generation !== positiveSafeNumber(release.generation, `${release.release_kind} generation`)
    || artifactRelease.release_kind !== release.release_kind
    || artifactRelease.content_digest !== release.release_digest
  ) {
    throw new FundingNormalizationJobV2Error("Stage 5 plan artifact differs from relational lineage", true);
  }
  const mainCatalog = artifact.catalogs.find((catalog) => catalog.product_id === MAIN_PRICING_PRODUCT_ID);
  const openKeysCatalog = artifact.catalogs.find((catalog) => catalog.product_id === OPENKEYS_PRICING_PRODUCT_ID);
  if (!mainCatalog || !openKeysCatalog) {
    throw new FundingNormalizationJobV2Error("Stage 5 plan lacks one exact product catalog", true);
  }
  if (release.funding_manifest_digest === null) {
    throw new FundingNormalizationJobV2Error("funding manifest is not finalized", false);
  }
  const wireAssignments = assignments.map((assignment) => ({
    account_id: assignment.engine_account_id,
    account_class: assignment.account_class === "openkeys" ? "open_keys" as const : assignment.account_class,
    policy_id: assignment.policy_id,
    policy_version: positiveSafeNumber(assignment.policy_version, "policy version"),
    policy_digest: assignment.policy_digest,
    billing_mode: assignment.billing_mode,
    funding_generation: assignment.funding_generation === null
      ? null
      : positiveSafeNumber(assignment.funding_generation, "funding generation"),
    purpose: assignment.purpose,
    responsible: assignment.responsible,
    assignment_digest: assignment.assignment_digest,
  }));
  const base = {
    generation: positiveSafeNumber(release.generation, `${release.release_kind} generation`),
    release_kind: release.release_kind,
    schema_version: PRICING_RELEASE_SCHEMA_VERSION_V2,
    capability_generation: artifact.capability.generation,
    capability_digest: artifact.capability.content_digest,
    main_catalog_generation: mainCatalog.generation,
    main_catalog_digest: mainCatalog.content_digest,
    openkeys_catalog_generation: openKeysCatalog.generation,
    openkeys_catalog_digest: openKeysCatalog.content_digest,
    switch_generation: artifact.switches.generation,
    switch_digest: artifact.switches.content_digest,
    inventory_digest: release.engine_inventory_digest,
    policy_manifest_digest: release.policy_manifest_digest,
    assignment_manifest_digest: release.assignment_manifest_digest,
    funding_manifest_digest: release.funding_manifest_digest,
    minimum_runtime_schema_version: PRICING_RELEASE_SCHEMA_VERSION_V2,
    assignments: wireAssignments,
  };
  return pricingReleaseV2Schema.parse({
    ...base,
    content_digest: stage5V2Digest("engine-release", base),
  });
}

function buildRecoveryLinkV2(
  target: PricingReleaseV2,
  recovery: PricingReleaseV2,
): PricingReleaseRecoveryLinkV2 {
  const base = {
    target_generation: target.generation,
    target_digest: target.content_digest,
    recovery_generation: recovery.generation,
    recovery_digest: recovery.content_digest,
  };
  return pricingReleaseRecoveryLinkV2Schema.parse({
    ...base,
    link_digest: stage5V2Digest("recovery-link", base),
  });
}

function assertCompleteFundingState(
  state: FundingNormalizationStateV2,
  coverage: FundingNormalizationCoverageV2,
): string {
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
  return fundingNormalizationManifestDigestV2(state.queue);
}

async function finalizeFundingManifestV2(
  database: Database,
  job: FundingNormalizationJobV2,
  workerId: string,
  evidence: {
    engineInventory: readonly PricingReleaseInventoryAccountV2[];
  },
): Promise<FundingNormalizationReleaseBundleV2> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query("SELECT pg_advisory_xact_lock(hashtextextended('pricing-stage6-v2:finalize', 0))");
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
    const state = await selectFundingNormalizationState(client, job, "FOR UPDATE");
    const coverage = buildFundingNormalizationCoverageV2(evidence.engineInventory, state);
    const fundingManifestDigest = assertCompleteFundingState(state, coverage);
    if (state.release.fundingManifestDigest !== null
        && state.release.fundingManifestDigest !== fundingManifestDigest) {
      throw new FundingNormalizationJobV2Error(
        "final funding manifest conflicts with a previously finalized identity",
        true,
      );
    }
    const targetAssignments = await selectReleaseAssignments(client, job.releaseGeneration);
    const recoveryAssignments = await selectReleaseAssignments(client, job.recoveryGeneration);
    assertReleaseGraphsMatch(targetAssignments, recoveryAssignments);
    const expectedAllAccounts = [...coverage.balanceAccountIds, ...coverage.serviceAccountIds].sort(compareUtf8);
    if (
      !exactAccountSet(targetAssignments.map((row) => row.engine_account_id), expectedAllAccounts)
      || !exactAccountSet(recoveryAssignments.map((row) => row.engine_account_id), expectedAllAccounts)
    ) {
      throw new FundingNormalizationJobV2Error(
        "Stage 5 assignment graph no longer covers the exact engine inventory",
        false,
      );
    }
    const queueById = new Map(state.queue.map((row) => [row.engineAccountId, row]));
    for (const assignment of targetAssignments) {
      if (assignment.billing_mode === "meter_only") {
        if (assignment.funding_generation !== null) {
          throw new FundingNormalizationJobV2Error("service assignment unexpectedly has funding", true);
        }
        continue;
      }
      const queue = queueById.get(assignment.engine_account_id);
      if (!queue?.fundingGeneration) {
        throw new FundingNormalizationJobV2Error(
          `missing ready funding identity for ${assignment.engine_account_id}`,
          false,
        );
      }
      const updated = await client.query(`
        UPDATE pricing_release_assignments_v2
        SET funding_generation = $3
        WHERE release_generation IN ($1, $2) AND engine_account_id = $4
          AND billing_mode = 'balance'
          AND (funding_generation IS NULL OR funding_generation = $3)
      `, [job.releaseGeneration, job.recoveryGeneration, queue.fundingGeneration, assignment.engine_account_id]);
      if (updated.rowCount !== 2) {
        throw new FundingNormalizationJobV2Error(
          `funding assignment ${assignment.engine_account_id} could not finalize exactly once per release`,
          true,
        );
      }
    }
    await client.query(`
      INSERT INTO pricing_funding_normalizations_v2 (
        release_generation, engine_account_id, funding_generation,
        expected_source_digest, target_funding_digest, applied_funding_digest,
        normalization_source, blockers, status, attempts, next_attempt_at, last_error
      )
      SELECT $2, engine_account_id, funding_generation,
             expected_source_digest, target_funding_digest, applied_funding_digest,
             normalization_source, NULL, 'ready', attempts, now(), NULL
      FROM pricing_funding_normalizations_v2
      WHERE release_generation = $1 AND status = 'ready'
      ON CONFLICT (release_generation, engine_account_id) DO NOTHING
    `, [job.releaseGeneration, job.recoveryGeneration]);
    const recoveryQueue = await client.query<QueueRow>(`
      SELECT engine_account_id, funding_generation::text, expected_source_digest,
             target_funding_digest, applied_funding_digest, normalization_source,
             blockers, status, attempts, next_attempt_at, locked_at, locked_by,
             last_error, updated_at
      FROM pricing_funding_normalizations_v2
      WHERE release_generation = $1
      ORDER BY engine_account_id COLLATE "C"
    `, [job.recoveryGeneration]);
    const recoveryQueueRows = recoveryQueue.rows.map(queueFromRow);
    const queueEvidence = (rows: readonly FundingNormalizationQueueRowV2[]) => rows.map((row) => ({
      engine_account_id: row.engineAccountId,
      funding_generation: row.fundingGeneration?.toString() ?? null,
      expected_source_digest: row.expectedSourceDigest,
      target_funding_digest: row.targetFundingDigest,
      applied_funding_digest: row.appliedFundingDigest,
      normalization_source: row.normalizationSource,
      blockers: row.blockers,
      status: row.status,
    }));
    if (
      fundingNormalizationManifestDigestV2(recoveryQueueRows) !== fundingManifestDigest
      || !sameCanonical(queueEvidence(recoveryQueueRows), queueEvidence(state.queue))
      || recoveryQueueRows.some((row) => row.status !== "ready" || row.blockers !== null)
    ) {
      throw new FundingNormalizationJobV2Error(
        "recovery funding evidence differs from the target manifest",
        true,
      );
    }
    const plans = await client.query(`
      UPDATE pricing_release_plans_v2
      SET funding_manifest_digest = $3, status = 'materializing', updated_at = now()
      WHERE generation IN ($1, $2)
        AND status IN ('planned', 'materializing')
        AND (funding_manifest_digest IS NULL OR funding_manifest_digest = $3)
    `, [job.releaseGeneration, job.recoveryGeneration, fundingManifestDigest]);
    if (plans.rowCount !== 2) {
      throw new FundingNormalizationJobV2Error("target/recovery funding manifests did not finalize together", true);
    }
    const run = await selectStage5Run(client, job.stage5PlanDigest, "FOR UPDATE");
    const target = await selectRelease(client, job.releaseGeneration, job.releaseDigest, "target", "FOR UPDATE");
    const recovery = await selectRelease(
      client,
      job.recoveryGeneration,
      job.recoveryPlanDigest,
      "recovery",
      "FOR UPDATE",
    );
    const finalizedTargetAssignments = await selectReleaseAssignments(client, job.releaseGeneration);
    const finalizedRecoveryAssignments = await selectReleaseAssignments(client, job.recoveryGeneration);
    const targetRelease = buildEngineReleaseV2(run, target, finalizedTargetAssignments);
    const recoveryRelease = buildEngineReleaseV2(run, recovery, finalizedRecoveryAssignments);
    const recoveryLink = buildRecoveryLinkV2(targetRelease, recoveryRelease);
    await client.query("COMMIT");
    return {
      stage5RunId: run.run_id,
      target: targetRelease,
      recovery: recoveryRelease,
      recoveryLink,
      fundingManifestDigest,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

function successfulMutationResult(result: { result: string }, label: string): "stored" | "unchanged" {
  if (result.result === "stored" || result.result === "unchanged") return result.result;
  throw new FundingNormalizationJobV2Error(`${label} prepare was rejected with ${result.result}`, true);
}

async function prepareEngineReleaseBundleV2(
  engine: FundingNormalizationReleaseEngineV2,
  bundle: FundingNormalizationReleaseBundleV2,
): Promise<Stage6PrepareAck[]> {
  const acks: Stage6PrepareAck[] = [];
  for (const [kind, release] of [
    ["target_release", bundle.target],
    ["recovery_release", bundle.recovery],
  ] as const) {
    const mutation = await engine.preparePricingReleaseV2(release);
    const result = successfulMutationResult(mutation, `${kind} ${release.generation}`);
    const readback = await engine.getPricingReleaseV2(release.generation);
    if (!readback || !sameCanonical(readback, release)) {
      throw new FundingNormalizationJobV2Error(`${kind} readback differs from prepare`, true);
    }
    acks.push({
      artifact_kind: kind,
      artifact_id: `pricing-release-v2:${release.release_kind}`,
      artifact_version: release.generation,
      expected_digest: release.content_digest,
      mutation_result: result,
      readback_digest: readback.content_digest,
    });
  }
  const linkMutation = await engine.preparePricingReleaseRecoveryLinkV2(bundle.recoveryLink);
  const linkResult = successfulMutationResult(linkMutation, "recovery link");
  const linkReadback = await engine.getPricingReleaseRecoveryLinkV2(
    bundle.recoveryLink.target_generation,
    bundle.recoveryLink.recovery_generation,
  );
  if (!linkReadback || !sameCanonical(linkReadback, bundle.recoveryLink)) {
    throw new FundingNormalizationJobV2Error("recovery link readback differs from prepare", true);
  }
  acks.push({
    artifact_kind: "recovery_link",
    artifact_id: "pricing-release-v2:target-recovery",
    artifact_version: bundle.recoveryLink.recovery_generation,
    expected_digest: bundle.recoveryLink.link_digest,
    mutation_result: linkResult,
    readback_digest: linkReadback.link_digest,
  });
  return acks;
}

async function commitFundingNormalizationFinalizationV2(
  database: Database,
  job: FundingNormalizationJobV2,
  workerId: string,
  expectedBundle: FundingNormalizationReleaseBundleV2,
  acks: readonly Stage6PrepareAck[],
  evidence: { engineInventory: readonly PricingReleaseInventoryAccountV2[] },
): Promise<string> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query("SELECT pg_advisory_xact_lock(hashtextextended('pricing-stage6-v2:finalize', 0))");
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
    const state = await selectFundingNormalizationState(client, job, "FOR UPDATE");
    const coverage = buildFundingNormalizationCoverageV2(evidence.engineInventory, state);
    const fundingManifestDigest = assertCompleteFundingState(state, coverage);
    if (fundingManifestDigest !== expectedBundle.fundingManifestDigest) {
      throw new FundingNormalizationJobV2Error("funding manifest changed before final commit", true);
    }
    const run = await selectStage5Run(client, job.stage5PlanDigest, "FOR UPDATE");
    const target = await selectRelease(client, job.releaseGeneration, job.releaseDigest, "target", "FOR UPDATE");
    const recovery = await selectRelease(
      client,
      job.recoveryGeneration,
      job.recoveryPlanDigest,
      "recovery",
      "FOR UPDATE",
    );
    const actualBundle: FundingNormalizationReleaseBundleV2 = {
      stage5RunId: run.run_id,
      target: buildEngineReleaseV2(run, target, await selectReleaseAssignments(client, job.releaseGeneration)),
      recovery: buildEngineReleaseV2(run, recovery, await selectReleaseAssignments(client, job.recoveryGeneration)),
      recoveryLink: expectedBundle.recoveryLink,
      fundingManifestDigest,
    };
    actualBundle.recoveryLink = buildRecoveryLinkV2(actualBundle.target, actualBundle.recovery);
    if (!sameCanonical(actualBundle, expectedBundle)) {
      throw new FundingNormalizationJobV2Error("release bundle changed after engine prepare", true);
    }
    if (acks.length !== 3) {
      throw new FundingNormalizationJobV2Error("engine release ACK set is incomplete", true);
    }
    for (const ack of acks) {
      if (ack.expected_digest !== ack.readback_digest) {
        throw new FundingNormalizationJobV2Error("engine release ACK readback digest differs", true);
      }
      const ackBase = { run_id: run.run_id, ...ack };
      const ackDigest = stage5V2Digest("prepare-ack", ackBase);
      const inserted = await client.query<{
        expected_digest: string;
        readback_digest: string;
      }>(`
        INSERT INTO pricing_stage5_prepare_acks_v2 (
          run_id, artifact_kind, artifact_id, artifact_version,
          expected_digest, mutation_result, readback_digest, ack_digest
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (run_id, artifact_kind, artifact_id, artifact_version) DO UPDATE
        SET expected_digest = pricing_stage5_prepare_acks_v2.expected_digest
        RETURNING expected_digest, readback_digest
      `, [
        run.run_id,
        ack.artifact_kind,
        ack.artifact_id,
        ack.artifact_version,
        ack.expected_digest,
        ack.mutation_result,
        ack.readback_digest,
        ackDigest,
      ]);
      if (!sameCanonical(inserted.rows[0], {
        expected_digest: ack.expected_digest,
        readback_digest: ack.readback_digest,
      })) {
        throw new FundingNormalizationJobV2Error("stored engine release ACK conflicts with readback", true);
      }
    }
    for (const release of [actualBundle.target, actualBundle.recovery]) {
      const updated = await client.query(`
        UPDATE pricing_release_plans_v2
        SET engine_release_digest = $2, status = 'prepared', updated_at = now()
        WHERE generation = $1 AND status IN ('materializing', 'prepared')
          AND funding_manifest_digest = $3
          AND (engine_release_digest IS NULL OR engine_release_digest = $2)
      `, [release.generation, release.content_digest, fundingManifestDigest]);
      if (updated.rowCount !== 1) {
        throw new FundingNormalizationJobV2Error(`${release.release_kind} release did not finalize`, true);
      }
    }
    const runUpdated = await client.query(`
      UPDATE pricing_stage5_runs_v2
      SET target_digest = $2, recovery_digest = $3,
          status = 'prepared', updated_at = now()
      WHERE run_id = $1 AND status IN ('materializing', 'prepared')
        AND (target_digest IS NULL OR target_digest = $2)
        AND (recovery_digest IS NULL OR recovery_digest = $3)
    `, [run.run_id, actualBundle.target.content_digest, actualBundle.recovery.content_digest]);
    if (runUpdated.rowCount !== 1) {
      throw new FundingNormalizationJobV2Error("Stage 5 run did not finalize with both release identities", true);
    }
    const resultDigest = canonicalDigest("pricing-funding-normalization-result-v2", {
      stage5_plan_digest: run.plan_digest,
      target_generation: actualBundle.target.generation,
      target_digest: actualBundle.target.content_digest,
      recovery_generation: actualBundle.recovery.generation,
      recovery_digest: actualBundle.recovery.content_digest,
      recovery_link_digest: actualBundle.recoveryLink.link_digest,
      engine_inventory_digest: coverage.engineInventoryDigest,
      service_inventory_digest: coverage.serviceInventoryDigest,
      funding_manifest_digest: fundingManifestDigest,
      balance_account_ids: coverage.balanceAccountIds,
    });
    const confirmed = await client.query(`
      UPDATE pricing_release_control_jobs_v2
      SET status = 'confirmed', result_digest = $2, confirmed_at = now(),
          locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
      WHERE id = $1 AND status = 'processing' AND locked_by = $3
    `, [job.id, resultDigest, workerId]);
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

export async function confirmFundingNormalizationJobV2(
  database: Database,
  engine: FundingNormalizationReleaseEngineV2,
  job: FundingNormalizationJobV2,
  workerId: string,
  evidence: { engineInventory: readonly PricingReleaseInventoryAccountV2[] },
): Promise<string> {
  const bundle = await finalizeFundingManifestV2(database, job, workerId, evidence);
  const acks = await prepareEngineReleaseBundleV2(engine, bundle);
  return commitFundingNormalizationFinalizationV2(
    database,
    job,
    workerId,
    bundle,
    acks,
    evidence,
  );
}
