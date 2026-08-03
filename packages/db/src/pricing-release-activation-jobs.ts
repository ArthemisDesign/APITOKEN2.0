import { isDeepStrictEqual } from "node:util";
import {
  canonicalSha256V2Schema,
  pricingReleaseActivationAckV2Schema,
  pricingReleaseActivationRequestV2Schema,
  type PricingReleaseActivationAckV2,
  type PricingReleaseActivationRequestV2,
} from "@claude-api/contracts";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";
import { stage5V2Digest } from "./pricing-stage5-materializer-v2.js";
import {
  capturePricingReleaseActivationAuthorityV2,
  type PricingReleaseActivationAuthorityReadersV2,
} from "./pricing-release-activation-authority.js";

const ACTIVATION_LEASE_INTERVAL = "5 minutes";

export type PricingReleaseActivationJobKindV2 = "cutover" | "recovery";
export type PricingReleaseActivationJobDispositionV2 = "retry" | "dead";

export interface StagePricingReleaseActivationJobV2Input {
  activationKind: PricingReleaseActivationJobKindV2;
  evidenceDigest: string;
  operatorId: string;
  reason: string;
}

export interface ClaimedPricingReleaseActivationJobV2 {
  id: string;
  attempts: number;
  releaseGeneration: bigint;
  releaseDigest: string;
  evidenceDigest: string;
  expectedHeadVersion: bigint;
  payloadDigest: string;
  request: PricingReleaseActivationRequestV2;
}

interface ActivationEvidenceRow {
  evidence_digest: string;
  engine_evidence_digest: string | null;
  engine_captured_at: Date | null;
  target_generation: string;
  target_digest: string;
  recovery_generation: string;
  recovery_digest: string;
  commerce_inventory_digest: string;
  engine_inventory_digest: string;
  openkeys_inventory_digest: string;
  service_inventory_digest: string | null;
  funding_digest: string;
  shadow_digest: string;
  runtime_floor_digest: string;
  legacy_inflight_count: string;
  blocker_count: string;
  passed: boolean;
  observed_at: Date;
  valid_until: Date;
  database_now: Date;
  target_status: string;
  target_engine_digest: string | null;
  recovery_status: string;
  recovery_engine_digest: string | null;
  target_commerce_inventory_digest: string;
  target_engine_inventory_digest: string;
  target_openkeys_inventory_digest: string;
  target_service_inventory_digest: string;
}

interface ActivationJobRow extends ActivationEvidenceRow {
  id: string;
  job_kind: "activate_release" | "activate_recovery";
  release_generation: string;
  release_digest: string;
  idempotency_key: string;
  payload_digest: string;
  expected_head_version: string | null;
  stage8_evidence_digest: string | null;
  activation_payload: unknown;
  attempts: number;
}

export class PricingReleaseActivationJobV2Error extends Error {
  constructor(
    message: string,
    public readonly permanent: boolean,
  ) {
    super(message);
    this.name = "PricingReleaseActivationJobV2Error";
  }
}

function permanent(message: string): PricingReleaseActivationJobV2Error {
  return new PricingReleaseActivationJobV2Error(message, true);
}

function transient(message: string): PricingReleaseActivationJobV2Error {
  return new PricingReleaseActivationJobV2Error(message, false);
}

function parsePositiveSafeInteger(value: string, label: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0 || String(parsed) !== value) {
    throw permanent(`${label} is not a positive safe integer`);
  }
  return parsed;
}

function parseNonNegativeSafeInteger(value: string, label: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 0 || String(parsed) !== value) {
    throw permanent(`${label} is not a non-negative safe integer`);
  }
  return parsed;
}

function epochSeconds(value: Date, label: string): number {
  const seconds = Math.floor(value.getTime() / 1_000);
  if (!Number.isSafeInteger(seconds) || seconds <= 0) {
    throw permanent(`${label} is not a positive safe epoch timestamp`);
  }
  return seconds;
}

function activationPayloadDigest(request: PricingReleaseActivationRequestV2): string {
  return stage5V2Digest("pricing-release-activation-request", request);
}

function activationReceiptDigest(
  ack: Extract<PricingReleaseActivationAckV2, { result: "applied" | "unchanged" }>,
): string {
  return stage5V2Digest("pricing-release-activation-receipt", ack.activation);
}

function activationResultDigest(payloadDigest: string, receiptDigest: string): string {
  return stage5V2Digest("pricing-release-activation-result", {
    request_digest: payloadDigest,
    receipt_digest: receiptDigest,
  });
}

function expectedJobKind(kind: PricingReleaseActivationJobKindV2): ActivationJobRow["job_kind"] {
  return kind === "cutover" ? "activate_release" : "activate_recovery";
}

function idempotencyKey(kind: PricingReleaseActivationJobKindV2, evidenceDigest: string): string {
  return `pricing:v2:activate:${kind}:${evidenceDigest}`;
}

async function loadActivationEvidence(
  client: PoolClient,
  evidenceDigest: string,
): Promise<ActivationEvidenceRow> {
  const result = await client.query<ActivationEvidenceRow>(`
    SELECT evidence.evidence_digest, evidence.engine_evidence_digest,
           evidence.engine_captured_at, evidence.target_generation::text,
           evidence.target_digest, evidence.recovery_generation::text,
           evidence.recovery_digest, evidence.commerce_inventory_digest,
           evidence.engine_inventory_digest, evidence.openkeys_inventory_digest,
           evidence.service_inventory_digest,
           evidence.funding_digest, evidence.shadow_digest,
           evidence.runtime_floor_digest, evidence.legacy_inflight_count::text,
           evidence.blocker_count::text, evidence.passed,
           evidence.observed_at, evidence.valid_until,
           transaction_timestamp() AS database_now,
           target.status AS target_status,
           target.engine_release_digest AS target_engine_digest,
           target.commerce_inventory_digest AS target_commerce_inventory_digest,
           target.engine_inventory_digest AS target_engine_inventory_digest,
           target.openkeys_inventory_digest AS target_openkeys_inventory_digest,
           target.service_inventory_digest AS target_service_inventory_digest,
           recovery.status AS recovery_status,
           recovery.engine_release_digest AS recovery_engine_digest
    FROM pricing_stage8_evidence_v2 evidence
    JOIN pricing_release_plans_v2 target
      ON target.generation = evidence.target_generation
     AND target.content_digest = evidence.target_digest
     AND target.release_kind = 'target'
    JOIN pricing_release_plans_v2 recovery
      ON recovery.generation = evidence.recovery_generation
     AND recovery.content_digest = evidence.recovery_digest
     AND recovery.release_kind = 'recovery'
    WHERE evidence.evidence_digest = $1
    FOR SHARE OF evidence, target, recovery
  `, [evidenceDigest]);
  const row = result.rows[0];
  if (!row) throw permanent("exact Stage 8 evidence and release pair do not exist");
  return row;
}

async function cutoverReceiptForRecovery(
  client: PoolClient,
  evidence: ActivationEvidenceRow,
): Promise<Extract<PricingReleaseActivationAckV2, { result: "applied" | "unchanged" }>> {
  const result = await client.query<{
    activation_id: string;
    evidence_digest: string;
    head_version: string;
    receipt_digest: string;
    receipt_payload: unknown;
    activated_at: Date;
  }>(`
    SELECT activation_id, evidence_digest, head_version::text,
           receipt_digest, receipt_payload, activated_at
    FROM pricing_release_activation_receipts_v2
    WHERE activation_kind = 'cutover'
      AND release_generation = $1 AND release_digest = $2
    FOR SHARE
  `, [evidence.target_generation, evidence.target_digest]);
  if (result.rows.length !== 1 || result.rows[0]!.receipt_payload === null) {
    throw permanent("recovery requires one complete durable cutover receipt");
  }
  const ack = pricingReleaseActivationAckV2Schema.parse(result.rows[0]!.receipt_payload);
  if (ack.result === "rejected" || ack.activation.activation_kind !== "cutover") {
    throw permanent("stored cutover receipt payload is not a successful cutover ACK");
  }
  const row = result.rows[0]!;
  if (
    ack.activation.head.active_generation !== parsePositiveSafeInteger(
      evidence.target_generation,
      "target generation",
    )
    || ack.activation.head.active_digest !== evidence.target_engine_digest
    || ack.activation.from_generation !== null
    || ack.activation.from_digest !== null
    || ack.activation.expected_head_version !== 0
    || ack.activation.head.head_version !== 1
    || ack.activation.head.updated_ts !== ack.activation.activated_ts
    || ack.activation.activation_id !== row.activation_id
    || ack.activation.evidence_digest !== row.evidence_digest
    || String(ack.activation.head.head_version) !== row.head_version
    || activationReceiptDigest(ack) !== row.receipt_digest
    || ack.activation.activated_ts * 1_000 !== row.activated_at.getTime()
  ) {
    throw permanent("stored cutover receipt does not match its durable target identity");
  }
  if (ack.activation.evidence_digest === evidence.evidence_digest) {
    throw permanent("recovery requires fresh Stage 8 evidence after cutover");
  }
  return ack;
}

async function requestFromEvidence(
  client: PoolClient,
  evidence: ActivationEvidenceRow,
  input: Omit<StagePricingReleaseActivationJobV2Input, "evidenceDigest">,
  requireFresh: boolean,
): Promise<PricingReleaseActivationRequestV2> {
  if (!evidence.passed || evidence.blocker_count !== "0") {
    throw permanent("activation requires persisted passed Stage 8 evidence with zero blockers");
  }
  if (evidence.engine_evidence_digest === null || evidence.engine_captured_at === null) {
    throw permanent("activation requires the exact persisted source engine evidence identity");
  }
  canonicalSha256V2Schema.parse(evidence.engine_evidence_digest);
  if (evidence.service_inventory_digest === null) {
    throw permanent("activation requires the exact persisted service inventory identity");
  }
  canonicalSha256V2Schema.parse(evidence.service_inventory_digest);
  if (evidence.target_status !== "prepared" || evidence.recovery_status !== "prepared") {
    throw permanent("activation requires prepared target and recovery releases");
  }
  if (evidence.target_engine_digest === null || evidence.recovery_engine_digest === null) {
    throw permanent("activation requires both immutable engine release digests");
  }
  canonicalSha256V2Schema.parse(evidence.target_engine_digest);
  canonicalSha256V2Schema.parse(evidence.recovery_engine_digest);
  if (requireFresh && evidence.valid_until.getTime() <= evidence.database_now.getTime()) {
    throw permanent("Stage 8 evidence expired before the activation job was durably claimed");
  }
  const targetGeneration = parsePositiveSafeInteger(evidence.target_generation, "target generation");
  const recoveryGeneration = parsePositiveSafeInteger(evidence.recovery_generation, "recovery generation");
  const expectation = input.activationKind === "cutover"
    ? "absent" as const
    : { exact: (await cutoverReceiptForRecovery(client, evidence)).activation.head };
  return pricingReleaseActivationRequestV2Schema.parse({
    activation_kind: input.activationKind,
    expectation,
    evidence: {
      evidence_digest: evidence.evidence_digest,
      target_generation: targetGeneration,
      target_digest: evidence.target_engine_digest,
      recovery_generation: recoveryGeneration,
      recovery_digest: evidence.recovery_engine_digest,
      engine_inventory_digest: evidence.engine_inventory_digest,
      funding_digest: evidence.funding_digest,
      shadow_digest: evidence.shadow_digest,
      runtime_floor_digest: evidence.runtime_floor_digest,
      legacy_inflight_count: parseNonNegativeSafeInteger(
        evidence.legacy_inflight_count,
        "legacy inflight count",
      ),
      engine_captured_ts: epochSeconds(evidence.engine_captured_at, "engine capture time"),
      observed_ts: epochSeconds(evidence.observed_at, "evidence observation time"),
      valid_until_ts: epochSeconds(evidence.valid_until, "evidence expiry time"),
    },
    operator_id: input.operatorId,
    reason: input.reason,
  });
}

async function unresolvedPricingJobCount(client: PoolClient, excludedJobId?: string): Promise<number> {
  const result = await client.query<{ count: string }>(`
    SELECT count(*)::text AS count
    FROM (
      SELECT id::text AS id FROM engine_catalog_jobs
      WHERE status IN ('pending', 'processing', 'retry', 'dead')
      UNION ALL SELECT id::text FROM engine_switch_jobs
      WHERE status IN ('pending', 'processing', 'retry', 'dead')
      UNION ALL SELECT id::text FROM engine_policy_jobs
      WHERE status IN ('pending', 'processing', 'retry', 'dead')
      UNION ALL SELECT id::text FROM engine_pricing_jobs
      WHERE status IN ('pending', 'processing', 'retry')
      UNION ALL SELECT id::text FROM pricing_release_control_jobs_v2
      WHERE status IN ('pending', 'processing', 'retry', 'dead')
        AND ($1::uuid IS NULL OR id <> $1::uuid)
    ) unresolved
  `, [excludedJobId ?? null]);
  return parseNonNegativeSafeInteger(result.rows[0]!.count, "unresolved pricing job count");
}

export async function stagePricingReleaseActivationJobV2(
  database: Database,
  input: StagePricingReleaseActivationJobV2Input,
): Promise<string> {
  const evidenceDigest = canonicalSha256V2Schema.parse(input.evidenceDigest);
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query(
      "SELECT pg_advisory_xact_lock(hashtextextended('pricing-release-v2:control-plane', 0))",
    );
    const evidence = await loadActivationEvidence(client, evidenceDigest);
    const request = await requestFromEvidence(client, evidence, input, false);
    const payloadDigest = activationPayloadDigest(request);
    const key = idempotencyKey(input.activationKind, evidenceDigest);
    const expectedHeadVersion = request.expectation === "absent"
      ? 0
      : request.expectation.exact.head_version;
    const releaseGeneration = input.activationKind === "cutover"
      ? evidence.target_generation
      : evidence.recovery_generation;
    const releaseDigest = input.activationKind === "cutover"
      ? evidence.target_digest
      : evidence.recovery_digest;
    const existing = await client.query<{
      id: string;
      job_kind: string;
      release_generation: string;
      release_digest: string;
      payload_digest: string;
      expected_head_version: string | null;
      stage8_evidence_digest: string | null;
      activation_payload: unknown;
    }>(`
      SELECT id, job_kind, release_generation::text, release_digest, payload_digest,
             expected_head_version::text, stage8_evidence_digest, activation_payload
      FROM pricing_release_control_jobs_v2
      WHERE idempotency_key = $1
      FOR UPDATE
    `, [key]);
    const row = existing.rows[0];
    if (row) {
      if (
        row.job_kind !== expectedJobKind(input.activationKind)
        || row.release_generation !== releaseGeneration
        || row.release_digest !== releaseDigest
        || row.payload_digest !== payloadDigest
        || row.expected_head_version !== String(expectedHeadVersion)
        || row.stage8_evidence_digest !== evidenceDigest
        || !isDeepStrictEqual(
          pricingReleaseActivationRequestV2Schema.parse(row.activation_payload),
          request,
        )
      ) {
        throw permanent("activation idempotency key has a different immutable payload");
      }
      await client.query("COMMIT");
      return row.id;
    }
    if (evidence.valid_until.getTime() <= evidence.database_now.getTime()) {
      throw permanent("Stage 8 evidence expired before activation staging");
    }
    if (await unresolvedPricingJobCount(client) !== 0) {
      throw permanent("activation cannot be staged while another pricing job is unresolved");
    }
    const inserted = await client.query<{ id: string }>(`
      INSERT INTO pricing_release_control_jobs_v2 (
        job_kind, release_generation, release_digest,
        idempotency_key, payload_digest, expected_head_version,
        stage8_evidence_digest, activation_payload
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb)
      RETURNING id
    `, [
      expectedJobKind(input.activationKind),
      releaseGeneration,
      releaseDigest,
      key,
      payloadDigest,
      expectedHeadVersion,
      evidenceDigest,
      JSON.stringify(request),
    ]);
    await client.query("COMMIT");
    return inserted.rows[0]!.id;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

async function loadActivationJob(client: PoolClient, jobId: string): Promise<ActivationJobRow> {
  const result = await client.query<ActivationJobRow>(`
    SELECT job.id, job.job_kind, job.release_generation::text,
           job.release_digest, job.idempotency_key, job.payload_digest,
           job.expected_head_version::text, job.stage8_evidence_digest,
           job.activation_payload, job.attempts,
           evidence.evidence_digest, evidence.engine_evidence_digest,
           evidence.engine_captured_at, evidence.target_generation::text,
           evidence.target_digest, evidence.recovery_generation::text,
           evidence.recovery_digest, evidence.commerce_inventory_digest,
           evidence.engine_inventory_digest, evidence.openkeys_inventory_digest,
           evidence.service_inventory_digest,
           evidence.funding_digest, evidence.shadow_digest,
           evidence.runtime_floor_digest, evidence.legacy_inflight_count::text,
           evidence.blocker_count::text, evidence.passed,
           evidence.observed_at, evidence.valid_until,
           transaction_timestamp() AS database_now,
           target.status AS target_status,
           target.engine_release_digest AS target_engine_digest,
           target.commerce_inventory_digest AS target_commerce_inventory_digest,
           target.engine_inventory_digest AS target_engine_inventory_digest,
           target.openkeys_inventory_digest AS target_openkeys_inventory_digest,
           target.service_inventory_digest AS target_service_inventory_digest,
           recovery.status AS recovery_status,
           recovery.engine_release_digest AS recovery_engine_digest
    FROM pricing_release_control_jobs_v2 job
    JOIN pricing_stage8_evidence_v2 evidence
      ON evidence.evidence_digest = job.stage8_evidence_digest
    JOIN pricing_release_plans_v2 target
      ON target.generation = evidence.target_generation
     AND target.content_digest = evidence.target_digest
     AND target.release_kind = 'target'
    JOIN pricing_release_plans_v2 recovery
      ON recovery.generation = evidence.recovery_generation
     AND recovery.content_digest = evidence.recovery_digest
     AND recovery.release_kind = 'recovery'
    WHERE job.id = $1 AND job.status = 'processing'
    FOR SHARE OF evidence, target, recovery
  `, [jobId]);
  const row = result.rows[0];
  if (!row) throw permanent("claimed activation job lost its evidence or release lineage");
  return row;
}

async function claimedJobFromRow(
  client: PoolClient,
  row: ActivationJobRow,
  authorityReaders: PricingReleaseActivationAuthorityReadersV2,
): Promise<ClaimedPricingReleaseActivationJobV2> {
  if (row.stage8_evidence_digest === null || row.expected_head_version === null) {
    throw permanent("activation job is missing its evidence or head expectation");
  }
  const activationKind: PricingReleaseActivationJobKindV2 = row.job_kind === "activate_release"
    ? "cutover"
    : row.job_kind === "activate_recovery" ? "recovery" : (() => { throw permanent("invalid activation job kind"); })();
  const request = await requestFromEvidence(client, row, {
    activationKind,
    operatorId: pricingReleaseActivationRequestV2Schema.parse(row.activation_payload).operator_id,
    reason: pricingReleaseActivationRequestV2Schema.parse(row.activation_payload).reason,
  }, row.attempts === 1);
  const storedRequest = pricingReleaseActivationRequestV2Schema.parse(row.activation_payload);
  const releaseGeneration = activationKind === "cutover" ? row.target_generation : row.recovery_generation;
  const releaseDigest = activationKind === "cutover" ? row.target_digest : row.recovery_digest;
  const expectedHeadVersion = storedRequest.expectation === "absent"
    ? 0
    : storedRequest.expectation.exact.head_version;
  const payloadDigest = activationPayloadDigest(storedRequest);
  if (
    !isDeepStrictEqual(storedRequest, request)
    || row.job_kind !== expectedJobKind(activationKind)
    || row.release_generation !== releaseGeneration
    || row.release_digest !== releaseDigest
    || row.idempotency_key !== idempotencyKey(activationKind, row.evidence_digest)
    || row.payload_digest !== payloadDigest
    || row.expected_head_version !== String(expectedHeadVersion)
    || row.stage8_evidence_digest !== row.evidence_digest
  ) {
    throw permanent("activation job payload differs from its durable evidence and release identity");
  }
  if (row.attempts === 1) {
    const authority = await capturePricingReleaseActivationAuthorityV2(client, authorityReaders, {
      activationKind,
      targetGeneration: row.target_generation,
      targetEngineDigest: row.target_engine_digest!,
      recoveryGeneration: row.recovery_generation,
      recoveryEngineDigest: row.recovery_engine_digest!,
      targetCommerceInventoryDigest: row.target_commerce_inventory_digest,
      targetEngineInventoryDigest: row.target_engine_inventory_digest,
      targetOpenkeysInventoryDigest: row.target_openkeys_inventory_digest,
      targetServiceInventoryDigest: row.target_service_inventory_digest,
      expectedHead: storedRequest.expectation === "absent"
        ? null
        : storedRequest.expectation.exact,
    });
    const drift = [
      ...authority.blockers.map((blocker) => blocker.code),
      ...(authority.commerceInventoryDigest === row.commerce_inventory_digest
        ? [] : ["commerce_evidence_authority_drift"]),
      ...(authority.engineInventoryDigest === row.engine_inventory_digest
        ? [] : ["engine_evidence_authority_drift"]),
      ...(authority.openkeysInventoryDigest === row.openkeys_inventory_digest
        ? [] : ["openkeys_evidence_authority_drift"]),
      ...(authority.serviceInventoryDigest === row.service_inventory_digest
        ? [] : ["service_evidence_authority_drift"]),
    ];
    if (drift.length > 0) {
      throw permanent(`activation authority changed after Stage 8 evidence: ${[...new Set(drift)].join(",")}`);
    }
  }
  return {
    id: row.id,
    attempts: row.attempts,
    releaseGeneration: BigInt(row.release_generation),
    releaseDigest: row.release_digest,
    evidenceDigest: row.evidence_digest,
    expectedHeadVersion: BigInt(row.expected_head_version),
    payloadDigest,
    request,
  };
}

export async function recoverStalePricingReleaseActivationJobsV2(
  database: Database,
): Promise<number> {
  const result = await database.pool.query(`
    UPDATE pricing_release_control_jobs_v2
    SET status = 'retry', locked_at = NULL, locked_by = NULL,
        next_attempt_at = now(),
        last_error = COALESCE(last_error, 'recovered expired pricing-release activation lease'),
        updated_at = now()
    WHERE job_kind IN ('activate_release', 'activate_recovery')
      AND status = 'processing'
      AND (locked_at IS NULL OR locked_at < now() - interval '${ACTIVATION_LEASE_INTERVAL}')
  `);
  return result.rowCount ?? 0;
}

export async function claimNextPricingReleaseActivationJobV2(
  database: Database,
  workerId: string,
  authorityReaders: PricingReleaseActivationAuthorityReadersV2,
): Promise<ClaimedPricingReleaseActivationJobV2 | null> {
  if (workerId.trim() === "") throw new RangeError("workerId is required");
  const client = await database.pool.connect();
  let transactionOpen = false;
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    transactionOpen = true;
    await client.query("SET LOCAL statement_timeout = '30s'");
    await client.query("SET LOCAL lock_timeout = '5s'");
    await client.query(`
      UPDATE pricing_release_control_jobs_v2
      SET status = 'retry', locked_at = NULL, locked_by = NULL,
          next_attempt_at = now(),
          last_error = COALESCE(last_error, 'recovered expired pricing-release activation lease'),
          updated_at = now()
      WHERE job_kind IN ('activate_release', 'activate_recovery')
        AND status = 'processing'
        AND (locked_at IS NULL OR locked_at < now() - interval '${ACTIVATION_LEASE_INTERVAL}')
    `);
    const candidate = await client.query<{ id: string }>(`
      SELECT id
      FROM pricing_release_control_jobs_v2
      WHERE job_kind IN ('activate_release', 'activate_recovery')
        AND status IN ('pending', 'retry') AND next_attempt_at <= now()
        AND NOT EXISTS (
          SELECT 1 FROM engine_catalog_jobs
          WHERE status IN ('pending', 'processing', 'retry', 'dead')
        )
        AND NOT EXISTS (
          SELECT 1 FROM engine_switch_jobs
          WHERE status IN ('pending', 'processing', 'retry', 'dead')
        )
        AND NOT EXISTS (
          SELECT 1 FROM engine_policy_jobs
          WHERE status IN ('pending', 'processing', 'retry', 'dead')
        )
        AND NOT EXISTS (
          SELECT 1 FROM engine_pricing_jobs
          WHERE status IN ('pending', 'processing', 'retry')
        )
        AND NOT EXISTS (
          SELECT 1 FROM pricing_release_control_jobs_v2 other
          WHERE other.status IN ('pending', 'processing', 'retry', 'dead')
            AND other.id <> pricing_release_control_jobs_v2.id
        )
      ORDER BY next_attempt_at, created_at
      FOR UPDATE SKIP LOCKED
      LIMIT 1
    `);
    const jobId = candidate.rows[0]?.id;
    if (!jobId) {
      await client.query("COMMIT");
      transactionOpen = false;
      return null;
    }
    await client.query(`
      UPDATE pricing_release_control_jobs_v2
      SET status = 'processing', attempts = attempts + 1,
          locked_at = now(), locked_by = $2, updated_at = now()
      WHERE id = $1
    `, [jobId, workerId]);
    try {
      const row = await loadActivationJob(client, jobId);
      const job = await claimedJobFromRow(client, row, authorityReaders);
      if (await unresolvedPricingJobCount(client, jobId) !== 0) {
        throw transient("another pricing job became unresolved after Stage 8 evidence collection");
      }
      await client.query("COMMIT");
      transactionOpen = false;
      return job;
    } catch (error) {
      const reason = error instanceof Error ? error.message : "invalid durable activation job";
      const retryable = error instanceof PricingReleaseActivationJobV2Error && !error.permanent;
      await client.query(`
        UPDATE pricing_release_control_jobs_v2
        SET status = $4,
            attempts = CASE
              WHEN $4 = 'retry' THEN GREATEST(attempts - 1, 0)
              ELSE attempts
            END,
            next_attempt_at = CASE
              WHEN $4 = 'retry' THEN now() + interval '5 seconds'
              ELSE next_attempt_at
            END,
            locked_at = NULL, locked_by = NULL,
            last_error = $2, updated_at = now()
        WHERE id = $1 AND status = 'processing' AND locked_by = $3
      `, [jobId, reason.slice(0, 2_000), workerId, retryable ? "retry" : "dead"]);
      await client.query("COMMIT");
      transactionOpen = false;
      if (retryable) return null;
      throw error;
    }
  } catch (error) {
    if (transactionOpen) await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

function assertReceiptMatchesJob(
  job: ClaimedPricingReleaseActivationJobV2,
  ack: Extract<PricingReleaseActivationAckV2, { result: "applied" | "unchanged" }>,
): void {
  const receipt = ack.activation;
  const expectation = job.request.expectation;
  const expectedFrom = expectation === "absent"
    ? { generation: null, digest: null, headVersion: 0 }
    : {
        generation: expectation.exact.active_generation,
        digest: expectation.exact.active_digest,
        headVersion: expectation.exact.head_version,
      };
  const destination = job.request.activation_kind === "cutover"
    ? {
        generation: job.request.evidence.target_generation,
        digest: job.request.evidence.target_digest,
      }
    : {
        generation: job.request.evidence.recovery_generation,
        digest: job.request.evidence.recovery_digest,
      };
  if (
    receipt.activation_kind !== job.request.activation_kind
    || receipt.from_generation !== expectedFrom.generation
    || receipt.from_digest !== expectedFrom.digest
    || receipt.expected_head_version !== expectedFrom.headVersion
    || receipt.head.active_generation !== destination.generation
    || receipt.head.active_digest !== destination.digest
    || receipt.head.head_version !== expectedFrom.headVersion + 1
    || receipt.head.updated_ts !== receipt.activated_ts
    || receipt.evidence_digest !== job.evidenceDigest
    || receipt.operator_id !== job.request.operator_id
    || receipt.reason !== job.request.reason
  ) {
    throw permanent("activation ACK does not match the immutable durable job");
  }
}

export async function confirmPricingReleaseActivationJobV2(
  database: Database,
  job: ClaimedPricingReleaseActivationJobV2,
  workerId: string,
  untrustedAck: PricingReleaseActivationAckV2,
): Promise<string> {
  const ack = pricingReleaseActivationAckV2Schema.parse(untrustedAck);
  if (ack.result === "rejected") {
    throw permanent(`activation was rejected with ${ack.code}`);
  }
  assertReceiptMatchesJob(job, ack);
  const receiptDigest = activationReceiptDigest(ack);
  const resultDigest = activationResultDigest(job.payloadDigest, receiptDigest);
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const lease = await client.query<{ activation_payload: unknown }>(`
      SELECT activation_payload
      FROM pricing_release_control_jobs_v2
      WHERE id = $1 AND status = 'processing' AND locked_by = $2
        AND release_generation = $3 AND release_digest = $4
        AND stage8_evidence_digest = $5 AND expected_head_version = $6
        AND payload_digest = $7
      FOR UPDATE
    `, [
      job.id,
      workerId,
      job.releaseGeneration,
      job.releaseDigest,
      job.evidenceDigest,
      job.expectedHeadVersion,
      job.payloadDigest,
    ]);
    if (!lease.rows[0]
        || !isDeepStrictEqual(
          pricingReleaseActivationRequestV2Schema.parse(lease.rows[0].activation_payload),
          job.request,
        )) {
      throw new PricingReleaseActivationJobV2Error(`activation job ${job.id} lost its lease`, false);
    }
    const activatedAt = new Date(ack.activation.activated_ts * 1_000);
    const inserted = await client.query<{ activation_id: string }>(`
      INSERT INTO pricing_release_activation_receipts_v2 (
        activation_id, activation_kind, release_generation, release_digest,
        evidence_digest, head_version, receipt_digest, receipt_payload, activated_at
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9)
      ON CONFLICT (activation_id) DO NOTHING
      RETURNING activation_id
    `, [
      ack.activation.activation_id,
      ack.activation.activation_kind,
      job.releaseGeneration,
      job.releaseDigest,
      job.evidenceDigest,
      ack.activation.head.head_version,
      receiptDigest,
      JSON.stringify(ack),
      activatedAt,
    ]);
    if (inserted.rows.length === 0) {
      const existing = await client.query<{
        activation_kind: string;
        release_generation: string;
        release_digest: string;
        evidence_digest: string;
        head_version: string;
        receipt_digest: string;
        receipt_payload: unknown;
        activated_at: Date;
      }>(`
        SELECT activation_kind, release_generation::text, release_digest,
               evidence_digest, head_version::text, receipt_digest,
               receipt_payload, activated_at
        FROM pricing_release_activation_receipts_v2
        WHERE activation_id = $1
        FOR UPDATE
      `, [ack.activation.activation_id]);
      const row = existing.rows[0];
      if (
        !row
        || row.activation_kind !== ack.activation.activation_kind
        || row.release_generation !== job.releaseGeneration.toString()
        || row.release_digest !== job.releaseDigest
        || row.evidence_digest !== job.evidenceDigest
        || row.head_version !== String(ack.activation.head.head_version)
        || row.receipt_digest !== receiptDigest
        || !isDeepStrictEqual(
          pricingReleaseActivationAckV2Schema.parse(row.receipt_payload),
          ack,
        )
        || row.activated_at.getTime() !== activatedAt.getTime()
      ) {
        throw permanent("activation receipt identity conflicts with an existing durable receipt");
      }
    }
    const confirmed = await client.query(`
      UPDATE pricing_release_control_jobs_v2
      SET status = 'confirmed', result_digest = $2, confirmed_at = now(),
          locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
      WHERE id = $1 AND status = 'processing' AND locked_by = $3
        AND payload_digest = $4 AND stage8_evidence_digest = $5
    `, [job.id, resultDigest, workerId, job.payloadDigest, job.evidenceDigest]);
    if (confirmed.rowCount !== 1) {
      throw new PricingReleaseActivationJobV2Error(`activation job ${job.id} lost its lease`, false);
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

export async function releasePricingReleaseActivationJobV2(
  database: Database,
  job: ClaimedPricingReleaseActivationJobV2,
  workerId: string,
  disposition: PricingReleaseActivationJobDispositionV2,
  error: string,
): Promise<void> {
  const delaySeconds = Math.min(3_600, Math.max(5, 2 ** Math.min(job.attempts, 10)));
  const updated = await database.pool.query(`
    UPDATE pricing_release_control_jobs_v2
    SET status = $4,
        next_attempt_at = CASE
          WHEN $4 = 'retry' THEN now() + ($5 * interval '1 second')
          ELSE next_attempt_at
        END,
        locked_at = NULL, locked_by = NULL,
        last_error = $6, updated_at = now()
    WHERE id = $1 AND status = 'processing' AND locked_by = $2
      AND payload_digest = $3
  `, [job.id, workerId, job.payloadDigest, disposition, delaySeconds, error.slice(0, 2_000)]);
  if (updated.rowCount !== 1) {
    throw new PricingReleaseActivationJobV2Error(`activation job ${job.id} lost its lease`, false);
  }
}
