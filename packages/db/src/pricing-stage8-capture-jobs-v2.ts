import { Buffer } from "node:buffer";
import { isDeepStrictEqual } from "node:util";
import {
  pricingReleaseActivationOperatorV2Schema,
  pricingReleaseActivationReasonV2Schema,
  pricingStage8CaptureRequestV2Schema,
  type PricingStage8CaptureRequestV2,
} from "@claude-api/contracts";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";
import {
  parseStage8EngineEvidenceV2,
  type Stage8CombinedBlocker,
  type Stage8CombinedEvidenceV2,
  type Stage8EngineEvidenceV2,
} from "./multi-discount-stage8-evidence.js";
import { stage5V2Digest } from "./pricing-stage5-materializer-v2.js";

const SHA256_V2_PATTERN = /^sha256:v2:[0-9a-f]{64}$/;
const STAGE8_SUBJECT_DIGEST_PATTERN = /^sha256:v(?:1|2):[0-9a-f]{64}$/;
const ENGINE_ARTIFACT_MAX_BYTES = 16 * 1024 * 1024;
const COMBINED_ARTIFACT_MAX_BYTES = 4 * 1024 * 1024;

export type PricingStage8CaptureJobStatusV2 =
  | "pending"
  | "processing"
  | "retry"
  | "passed"
  | "blocked"
  | "dead";
export type PricingStage8CaptureJobDispositionV2 = "retry" | "dead";

export interface StagePricingStage8CaptureJobV2Input {
  idempotencyKey: string;
  request: PricingStage8CaptureRequestV2;
  operatorId: string;
  reason: string;
}

export interface StagedPricingStage8CaptureJobV2 {
  jobId: string;
  requestDigest: string;
}

export interface ClaimedPricingStage8CaptureJobV2 {
  id: string;
  idempotencyKey: string;
  requestDigest: string;
  request: PricingStage8CaptureRequestV2;
  operatorId: string;
  reason: string;
  attempts: number;
}

export interface PricingStage8CaptureArtifactV2 {
  artifactId: string;
  evidence: Stage8EngineEvidenceV2;
}

export interface PricingStage8CaptureControlJobV2 {
  id: string;
  idempotencyKey: string;
  requestDigest: string;
  targetGeneration: string;
  recoveryGeneration: string;
  windowStartAt: Date;
  windowEndAt: Date;
  minSamplesPerProvider: string;
  financialSampleSize: number;
  geminiClientAdmissions: string;
  operatorId: string;
  reason: string;
  status: PricingStage8CaptureJobStatusV2;
  attempts: number;
  nextAttemptAt: Date;
  lockedAt: Date | null;
  lockedBy: string | null;
  lastError: string | null;
  resultEngineEvidenceDigest: string | null;
  resultCombinedEvidenceDigest: string | null;
  resultPassed: boolean | null;
  completedAt: Date | null;
  createdAt: Date;
  updatedAt: Date;
}

export interface PricingStage8CaptureControlArtifactV2 {
  id: string;
  jobId: string;
  attempt: number;
  engineEvidenceDigest: string;
  engineCapturedAt: Date;
  combinedEvidenceDigest: string | null;
  combinedPassed: boolean | null;
  combinedWriteResult: Stage8CombinedEvidenceV2["write_result"] | null;
  combinedObservedAt: Date | null;
  combinedValidUntil: Date | null;
  combinedBlockerCount: string | null;
  combinedBlockers: Stage8CombinedBlocker[] | null;
  combinedBlockersTruncated: boolean | null;
  completedAt: Date | null;
  createdAt: Date;
}

export interface PricingStage8CaptureControlV2 {
  databaseObservedAt: Date;
  countsByStatus: Record<PricingStage8CaptureJobStatusV2, number>;
  jobs: PricingStage8CaptureControlJobV2[];
  artifacts: PricingStage8CaptureControlArtifactV2[];
}

export class PricingStage8CaptureJobV2Error extends Error {
  constructor(message: string, readonly permanent: boolean) {
    super(message);
    this.name = "PricingStage8CaptureJobV2Error";
  }
}

interface CaptureJobRow {
  id: string;
  idempotency_key: string;
  request_digest: string;
  target_generation: string;
  recovery_generation: string;
  window_start_at: Date;
  window_end_at: Date;
  min_samples_per_provider: string;
  financial_sample_size: number;
  gemini_client_admissions: string;
  operator_id: string;
  reason: string;
  status: PricingStage8CaptureJobStatusV2;
  attempts: number;
  next_attempt_at: Date;
  locked_at: Date | null;
  locked_by: string | null;
  last_error: string | null;
  result_engine_evidence_digest: string | null;
  result_combined_evidence_digest: string | null;
  result_passed: boolean | null;
  completed_at: Date | null;
  created_at: Date;
  updated_at: Date;
}

interface CaptureArtifactRow {
  id: string;
  job_id: string;
  attempt: number;
  engine_evidence_digest: string;
  engine_captured_at: Date;
  engine_payload_json: string;
  combined_evidence_digest: string | null;
  combined_payload_json: string | null;
  combined_passed: boolean | null;
  combined_write_result: Stage8CombinedEvidenceV2["write_result"] | null;
  completed_at: Date | null;
  created_at: Date;
}

interface CaptureArtifactControlRow {
  id: string;
  job_id: string;
  attempt: number;
  engine_evidence_digest: string;
  engine_captured_at: Date;
  combined_evidence_digest: string | null;
  combined_passed: boolean | null;
  combined_write_result: Stage8CombinedEvidenceV2["write_result"] | null;
  combined_observed_at: string | null;
  combined_valid_until: string | null;
  combined_blocker_count: string | null;
  combined_blockers: unknown;
  completed_at: Date | null;
  created_at: Date;
}

function permanent(message: string): PricingStage8CaptureJobV2Error {
  return new PricingStage8CaptureJobV2Error(message, true);
}

function transient(message: string): PricingStage8CaptureJobV2Error {
  return new PricingStage8CaptureJobV2Error(message, false);
}

function assertPositiveDuration(value: number, label: string): void {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new RangeError(`${label} must be a positive safe integer`);
  }
}

function safeNumber(value: string, label: string, positive: boolean): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed)
      || String(parsed) !== value
      || (positive ? parsed <= 0 : parsed < 0)) {
    throw permanent(`${label} is not a ${positive ? "positive" : "nonnegative"} safe integer`);
  }
  return parsed;
}

function epochSeconds(value: Date): number {
  if (value.getUTCMilliseconds() !== 0) throw permanent("capture window lost whole-second precision");
  const seconds = value.getTime() / 1_000;
  if (!Number.isSafeInteger(seconds) || seconds <= 0) throw permanent("capture window is not a positive epoch second");
  return seconds;
}

function requestIdentity(input: StagePricingStage8CaptureJobV2Input): Record<string, unknown> {
  return {
    schema_version: 2,
    idempotency_key: input.idempotencyKey,
    request: input.request,
    operator_id: input.operatorId,
    reason: input.reason,
  };
}

export function pricingStage8CaptureRequestDigestV2(
  input: StagePricingStage8CaptureJobV2Input,
): string {
  return stage5V2Digest("stage8-managed-capture-request", requestIdentity(input));
}

function claimedFromRow(row: CaptureJobRow): ClaimedPricingStage8CaptureJobV2 {
  const request = pricingStage8CaptureRequestV2Schema.parse({
    target_generation: safeNumber(row.target_generation, "target generation", true),
    recovery_generation: safeNumber(row.recovery_generation, "recovery generation", true),
    window_start_ts: epochSeconds(row.window_start_at),
    window_end_ts: epochSeconds(row.window_end_at),
    min_samples_per_provider: safeNumber(row.min_samples_per_provider, "provider minimum", true),
    financial_sample_size: row.financial_sample_size,
    gemini_client_admissions: safeNumber(row.gemini_client_admissions, "Gemini admissions", false),
  });
  const identity = {
    idempotencyKey: row.idempotency_key,
    request,
    operatorId: pricingReleaseActivationOperatorV2Schema.parse(row.operator_id),
    reason: pricingReleaseActivationReasonV2Schema.parse(row.reason),
  };
  if (pricingStage8CaptureRequestDigestV2(identity) !== row.request_digest) {
    throw permanent("durable Stage 8 capture request digest does not match its immutable inputs");
  }
  return {
    id: row.id,
    idempotencyKey: row.idempotency_key,
    requestDigest: row.request_digest,
    request,
    operatorId: identity.operatorId,
    reason: identity.reason,
    attempts: row.attempts,
  };
}

function sameStagedRequest(row: CaptureJobRow, input: StagePricingStage8CaptureJobV2Input): boolean {
  try {
    const claimed = claimedFromRow(row);
    return claimed.idempotencyKey === input.idempotencyKey
      && claimed.requestDigest === pricingStage8CaptureRequestDigestV2(input)
      && isDeepStrictEqual(claimed.request, input.request)
      && claimed.operatorId === input.operatorId
      && claimed.reason === input.reason;
  } catch {
    return false;
  }
}

export async function stagePricingStage8CaptureJobV2(
  database: Database,
  untrustedInput: StagePricingStage8CaptureJobV2Input,
): Promise<StagedPricingStage8CaptureJobV2> {
  const input: StagePricingStage8CaptureJobV2Input = {
    idempotencyKey: untrustedInput.idempotencyKey,
    request: pricingStage8CaptureRequestV2Schema.parse(untrustedInput.request),
    operatorId: pricingReleaseActivationOperatorV2Schema.parse(untrustedInput.operatorId),
    reason: pricingReleaseActivationReasonV2Schema.parse(untrustedInput.reason),
  };
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
    .test(input.idempotencyKey)) {
    throw new TypeError("idempotencyKey must be a UUID");
  }
  const requestDigest = pricingStage8CaptureRequestDigestV2(input);
  const client = await database.pool.connect();
  let transactionOpen = false;
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    transactionOpen = true;
    await client.query(
      "SELECT pg_advisory_xact_lock(hashtextextended('pricing-stage8-capture-v2:stage', 0))",
    );
    const observed = await client.query<{ database_now: Date }>(
      "SELECT transaction_timestamp() AS database_now",
    );
    const databaseNowSeconds = Math.floor(observed.rows[0]!.database_now.getTime() / 1_000);
    if (input.request.window_end_ts > databaseNowSeconds) {
      throw permanent("Stage 8 capture window must end in the past");
    }
    const existing = await client.query<CaptureJobRow>(`
      SELECT id, idempotency_key, request_digest,
             target_generation::text, recovery_generation::text,
             window_start_at, window_end_at, min_samples_per_provider::text,
             financial_sample_size, gemini_client_admissions::text,
             operator_id, reason, status, attempts, next_attempt_at,
             locked_at, locked_by, last_error,
             result_engine_evidence_digest, result_combined_evidence_digest,
             result_passed, completed_at, created_at, updated_at
      FROM pricing_stage8_capture_jobs_v2
      WHERE idempotency_key = $1
      FOR UPDATE
    `, [input.idempotencyKey]);
    const row = existing.rows[0];
    if (row) {
      if (!sameStagedRequest(row, input)) {
        throw permanent("Stage 8 capture idempotency key has a different immutable request");
      }
      await client.query("COMMIT");
      transactionOpen = false;
      return { jobId: row.id, requestDigest };
    }

    const inserted = await client.query<{ id: string }>(`
      INSERT INTO pricing_stage8_capture_jobs_v2 (
        idempotency_key, request_digest,
        target_generation, recovery_generation,
        window_start_at, window_end_at,
        min_samples_per_provider, financial_sample_size,
        gemini_client_admissions, operator_id, reason
      ) VALUES (
        $1, $2, $3, $4, to_timestamp($5), to_timestamp($6), $7, $8, $9, $10, $11
      )
      RETURNING id
    `, [
      input.idempotencyKey,
      requestDigest,
      input.request.target_generation,
      input.request.recovery_generation,
      input.request.window_start_ts,
      input.request.window_end_ts,
      input.request.min_samples_per_provider,
      input.request.financial_sample_size,
      input.request.gemini_client_admissions,
      input.operatorId,
      input.reason,
    ]);
    const jobId = inserted.rows[0]!.id;
    await client.query(`
      INSERT INTO audit_log (
        actor_type, actor_id, action, target_type, target_id, metadata
      ) VALUES (
        'admin', $1, 'pricing_stage8_capture_staged',
        'pricing_stage8_capture_job_v2', $2,
        jsonb_build_object(
          'request_digest', $3::text,
          'target_generation', $4::text,
          'recovery_generation', $5::text,
          'window_start_ts', $6::text,
          'window_end_ts', $7::text,
          'min_samples_per_provider', $8::text,
          'financial_sample_size', $9::text,
          'gemini_client_admissions', $10::text,
          'reason', $11::text
        )
      )
    `, [
      input.operatorId,
      jobId,
      requestDigest,
      String(input.request.target_generation),
      String(input.request.recovery_generation),
      String(input.request.window_start_ts),
      String(input.request.window_end_ts),
      String(input.request.min_samples_per_provider),
      String(input.request.financial_sample_size),
      String(input.request.gemini_client_admissions),
      input.reason,
    ]);
    await client.query("COMMIT");
    transactionOpen = false;
    return { jobId, requestDigest };
  } catch (error) {
    if (transactionOpen) await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

async function recoverStaleCaptureJobs(
  client: Pick<PoolClient, "query">,
  leaseMs: number,
  maxAttempts: number,
): Promise<number> {
  assertPositiveDuration(leaseMs, "Stage 8 capture leaseMs");
  assertPositiveDuration(maxAttempts, "Stage 8 capture maxAttempts");
  const result = await client.query(`
    UPDATE pricing_stage8_capture_jobs_v2
    SET status = CASE WHEN attempts >= $2 THEN 'dead' ELSE 'retry' END,
        next_attempt_at = CASE WHEN attempts >= $2 THEN next_attempt_at ELSE now() END,
        locked_at = NULL, locked_by = NULL,
        last_error = CASE
          WHEN attempts >= $2 THEN 'Stage 8 capture lease expired at the maximum attempt count'
          ELSE COALESCE(last_error, 'recovered expired Stage 8 capture lease')
        END,
        completed_at = CASE WHEN attempts >= $2 THEN now() ELSE NULL END,
        updated_at = now()
    WHERE status = 'processing'
      AND (locked_at IS NULL OR locked_at < now() - ($1 * interval '1 millisecond'))
  `, [leaseMs, maxAttempts]);
  return result.rowCount ?? 0;
}

export async function recoverStalePricingStage8CaptureJobsV2(
  database: Database,
  leaseMs: number,
  maxAttempts: number,
): Promise<number> {
  return recoverStaleCaptureJobs(database.pool, leaseMs, maxAttempts);
}

export async function claimNextPricingStage8CaptureJobV2(
  database: Database,
  workerId: string,
  leaseMs: number,
  maxAttempts: number,
): Promise<ClaimedPricingStage8CaptureJobV2 | null> {
  if (workerId.trim() === "") throw new RangeError("workerId is required");
  assertPositiveDuration(leaseMs, "Stage 8 capture leaseMs");
  assertPositiveDuration(maxAttempts, "Stage 8 capture maxAttempts");
  const client = await database.pool.connect();
  let transactionOpen = false;
  try {
    await client.query("BEGIN");
    transactionOpen = true;
    await client.query(
      "SELECT pg_advisory_xact_lock(hashtextextended('pricing-stage8-capture-v2:claim', 0))",
    );
    await recoverStaleCaptureJobs(client, leaseMs, maxAttempts);
    const candidate = await client.query<{ id: string }>(`
      SELECT id
      FROM pricing_stage8_capture_jobs_v2
      WHERE status IN ('pending', 'retry')
        AND next_attempt_at <= now()
        AND NOT EXISTS (
          SELECT 1 FROM pricing_stage8_capture_jobs_v2 active
          WHERE active.status = 'processing'
        )
      ORDER BY next_attempt_at, created_at, id
      FOR UPDATE SKIP LOCKED
      LIMIT 1
    `);
    const jobId = candidate.rows[0]?.id;
    if (!jobId) {
      await client.query("COMMIT");
      transactionOpen = false;
      return null;
    }
    const claimed = await client.query<CaptureJobRow>(`
      UPDATE pricing_stage8_capture_jobs_v2
      SET status = 'processing', attempts = attempts + 1,
          locked_at = now(), locked_by = $2, last_error = NULL, updated_at = now()
      WHERE id = $1
      RETURNING id, idempotency_key, request_digest,
                target_generation::text, recovery_generation::text,
                window_start_at, window_end_at, min_samples_per_provider::text,
                financial_sample_size, gemini_client_admissions::text,
                operator_id, reason, status, attempts, next_attempt_at,
                locked_at, locked_by, last_error,
                result_engine_evidence_digest, result_combined_evidence_digest,
                result_passed, completed_at, created_at, updated_at
    `, [jobId, workerId]);
    try {
      const job = claimedFromRow(claimed.rows[0]!);
      await client.query("COMMIT");
      transactionOpen = false;
      return job;
    } catch (error) {
      const reason = error instanceof Error ? error.message : "invalid durable Stage 8 capture job";
      await client.query(`
        UPDATE pricing_stage8_capture_jobs_v2
        SET status = 'dead', locked_at = NULL, locked_by = NULL,
            last_error = $2, completed_at = now(), updated_at = now()
        WHERE id = $1 AND status = 'processing' AND locked_by = $3
      `, [jobId, reason.slice(0, 2_000), workerId]);
      await client.query("COMMIT");
      transactionOpen = false;
      throw error;
    }
  } catch (error) {
    if (transactionOpen) await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

function assertEngineEvidenceMatchesJob(
  job: ClaimedPricingStage8CaptureJobV2,
  evidence: Stage8EngineEvidenceV2,
): void {
  const request = job.request;
  if (
    evidence.release.target_generation !== BigInt(request.target_generation)
    || evidence.release.recovery_generation !== BigInt(request.recovery_generation)
    || evidence.window_start_ts !== BigInt(request.window_start_ts)
    || evidence.window_end_ts !== BigInt(request.window_end_ts)
    || evidence.min_samples_per_provider !== BigInt(request.min_samples_per_provider)
    || evidence.gemini_client_admissions !== BigInt(request.gemini_client_admissions)
    || evidence.financial_samples.length > request.financial_sample_size
  ) {
    throw permanent("engine Stage 8 artifact differs from its immutable capture request");
  }
}

async function lockClaimedJob(
  client: PoolClient,
  job: ClaimedPricingStage8CaptureJobV2,
  workerId: string,
): Promise<void> {
  const lease = await client.query<{ request_digest: string; attempts: number }>(`
    SELECT request_digest, attempts
    FROM pricing_stage8_capture_jobs_v2
    WHERE id = $1 AND status = 'processing' AND locked_by = $2
      AND request_digest = $3 AND attempts = $4
    FOR UPDATE
  `, [job.id, workerId, job.requestDigest, job.attempts]);
  if (!lease.rows[0]) throw transient(`Stage 8 capture job ${job.id} lost its lease`);
}

export async function persistPricingStage8EngineArtifactV2(
  database: Database,
  job: ClaimedPricingStage8CaptureJobV2,
  workerId: string,
  rawEnginePayload: string,
): Promise<PricingStage8CaptureArtifactV2> {
  if (Buffer.byteLength(rawEnginePayload, "utf8") > ENGINE_ARTIFACT_MAX_BYTES) {
    throw permanent("engine Stage 8 artifact exceeds the durable size bound");
  }
  const evidence = parseStage8EngineEvidenceV2(rawEnginePayload);
  assertEngineEvidenceMatchesJob(job, evidence);
  const capturedAt = new Date(Number(evidence.captured_ts) * 1_000);
  if (!Number.isSafeInteger(Number(evidence.captured_ts)) || Number.isNaN(capturedAt.getTime())) {
    throw permanent("engine Stage 8 captured timestamp is not safely representable");
  }
  const client = await database.pool.connect();
  let transactionOpen = false;
  try {
    await client.query("BEGIN");
    transactionOpen = true;
    await lockClaimedJob(client, job, workerId);
    const inserted = await client.query<{ id: string }>(`
      INSERT INTO pricing_stage8_capture_artifacts_v2 (
        job_id, attempt, engine_evidence_digest, engine_captured_at, engine_payload_json
      ) VALUES ($1, $2, $3, $4, $5)
      ON CONFLICT (job_id, attempt) DO NOTHING
      RETURNING id
    `, [job.id, job.attempts, evidence.evidence_digest, capturedAt, rawEnginePayload]);
    let artifactId = inserted.rows[0]?.id;
    if (!artifactId) {
      const existing = await client.query<CaptureArtifactRow>(`
        SELECT id, job_id, attempt, engine_evidence_digest, engine_captured_at,
               engine_payload_json, combined_evidence_digest, combined_payload_json,
               combined_passed, combined_write_result, completed_at, created_at
        FROM pricing_stage8_capture_artifacts_v2
        WHERE job_id = $1 AND attempt = $2
        FOR UPDATE
      `, [job.id, job.attempts]);
      const row = existing.rows[0];
      if (!row
          || row.engine_evidence_digest !== evidence.evidence_digest
          || row.engine_captured_at.getTime() !== capturedAt.getTime()
          || row.engine_payload_json !== rawEnginePayload
          || row.combined_evidence_digest !== null) {
        throw permanent("Stage 8 attempt artifact conflicts with an existing immutable capture");
      }
      artifactId = row.id;
    }
    await client.query("COMMIT");
    transactionOpen = false;
    return { artifactId, evidence };
  } catch (error) {
    if (transactionOpen) await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

function assertCombinedEvidenceMatchesArtifact(
  job: ClaimedPricingStage8CaptureJobV2,
  engineEvidence: Stage8EngineEvidenceV2,
  combined: Stage8CombinedEvidenceV2,
): void {
  const {
    write_result: _writeResult,
    evidence_digest: _evidenceDigest,
    ...combinedIdentity
  } = combined;
  const expectedDigest = stage5V2Digest("stage8-combined-evidence", combinedIdentity);
  const observedAt = Date.parse(combined.observed_at);
  const validUntil = Date.parse(combined.valid_until);
  const engineBlockers = new Map(engineEvidence.blockers.map((blocker) => [
    `${blocker.code}\0${blocker.count.toString()}\0${blocker.subject_digests.join("\0")}`,
    false,
  ]));
  for (const blocker of combined.blockers) {
    const key = `${blocker.code}\0${blocker.count}\0${blocker.subject_digests.join("\0")}`;
    if (blocker.source === "engine" && engineBlockers.has(key)) engineBlockers.set(key, true);
  }
  if (
    !SHA256_V2_PATTERN.test(combined.evidence_digest)
    || combined.evidence_digest !== expectedDigest
    || !Number.isFinite(observedAt)
    || !Number.isFinite(validUntil)
    || validUntil <= observedAt
    || validUntil - observedAt > 300_000
    || combined.source.engine_evidence_digest !== engineEvidence.evidence_digest
    || combined.source.engine_captured_ts !== engineEvidence.captured_ts.toString()
    || combined.source.engine_window_start_ts !== engineEvidence.window_start_ts.toString()
    || combined.source.engine_window_end_ts !== engineEvidence.window_end_ts.toString()
    || combined.releases.target.generation !== String(job.request.target_generation)
    || combined.releases.recovery.generation !== String(job.request.recovery_generation)
    || combined.releases.target.engine_digest !== engineEvidence.release.target_digest
    || combined.releases.recovery.engine_digest !== engineEvidence.release.recovery_digest
    || combined.funding_digest !== engineEvidence.funding_digest
    || combined.shadow_digest !== engineEvidence.shadow_digest
    || combined.runtime_floor_digest !== engineEvidence.runtime_floor_digest
    || combined.legacy_inflight_count !== engineEvidence.legacy_inflight_count.toString()
    || combined.blocker_count !== String(combined.blockers.length)
    || combined.passed !== (combined.blockers.length === 0)
    || (combined.passed && combined.write_result === "not_persisted")
    || [...engineBlockers.values()].some((present) => !present)
  ) {
    throw permanent("combined Stage 8 artifact differs from its engine source or durable request");
  }
}

export async function completePricingStage8CaptureJobV2(
  database: Database,
  job: ClaimedPricingStage8CaptureJobV2,
  workerId: string,
  artifactId: string,
  combined: Stage8CombinedEvidenceV2,
  rawCombinedPayload: string,
): Promise<void> {
  if (Buffer.byteLength(rawCombinedPayload, "utf8") > COMBINED_ARTIFACT_MAX_BYTES) {
    throw permanent("combined Stage 8 artifact exceeds the durable size bound");
  }
  let parsedCombined: unknown;
  try {
    parsedCombined = JSON.parse(rawCombinedPayload);
  } catch {
    throw permanent("combined Stage 8 artifact is not valid JSON");
  }
  if (!isDeepStrictEqual(parsedCombined, combined)) {
    throw permanent("combined Stage 8 raw artifact differs from its collected result");
  }
  const client = await database.pool.connect();
  let transactionOpen = false;
  try {
    await client.query("BEGIN");
    transactionOpen = true;
    await lockClaimedJob(client, job, workerId);
    const artifact = await client.query<CaptureArtifactRow>(`
      SELECT id, job_id, attempt, engine_evidence_digest, engine_captured_at,
             engine_payload_json, combined_evidence_digest, combined_payload_json,
             combined_passed, combined_write_result, completed_at, created_at
      FROM pricing_stage8_capture_artifacts_v2
      WHERE id = $1 AND job_id = $2 AND attempt = $3
      FOR UPDATE
    `, [artifactId, job.id, job.attempts]);
    const row = artifact.rows[0];
    if (!row || row.combined_evidence_digest !== null) {
      throw transient(`Stage 8 capture artifact ${artifactId} is unavailable for completion`);
    }
    const engineEvidence = parseStage8EngineEvidenceV2(row.engine_payload_json);
    if (engineEvidence.evidence_digest !== row.engine_evidence_digest) {
      throw permanent("durable Stage 8 engine artifact digest changed before completion");
    }
    assertCombinedEvidenceMatchesArtifact(job, engineEvidence, combined);
    const completed = await client.query(`
      UPDATE pricing_stage8_capture_artifacts_v2
      SET combined_evidence_digest = $2,
          combined_payload_json = $3,
          combined_passed = $4,
          combined_write_result = $5,
          completed_at = now()
      WHERE id = $1 AND combined_evidence_digest IS NULL
    `, [
      artifactId,
      combined.evidence_digest,
      rawCombinedPayload,
      combined.passed,
      combined.write_result,
    ]);
    if (completed.rowCount !== 1) {
      throw transient(`Stage 8 capture artifact ${artifactId} lost its completion lease`);
    }
    const status: PricingStage8CaptureJobStatusV2 = combined.passed ? "passed" : "blocked";
    const finished = await client.query(`
      UPDATE pricing_stage8_capture_jobs_v2
      SET status = $2,
          result_engine_evidence_digest = $3,
          result_combined_evidence_digest = $4,
          result_passed = $5,
          completed_at = now(),
          locked_at = NULL, locked_by = NULL, last_error = NULL,
          updated_at = now()
      WHERE id = $1 AND status = 'processing' AND locked_by = $6
        AND request_digest = $7 AND attempts = $8
    `, [
      job.id,
      status,
      row.engine_evidence_digest,
      combined.evidence_digest,
      combined.passed,
      workerId,
      job.requestDigest,
      job.attempts,
    ]);
    if (finished.rowCount !== 1) throw transient(`Stage 8 capture job ${job.id} lost its lease`);
    await client.query("COMMIT");
    transactionOpen = false;
  } catch (error) {
    if (transactionOpen) await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function releasePricingStage8CaptureJobV2(
  database: Database,
  job: ClaimedPricingStage8CaptureJobV2,
  workerId: string,
  disposition: PricingStage8CaptureJobDispositionV2,
  error: string,
  retryMs: number,
  maxAttempts: number,
): Promise<Extract<PricingStage8CaptureJobStatusV2, "retry" | "dead">> {
  assertPositiveDuration(retryMs, "Stage 8 capture retryMs");
  assertPositiveDuration(maxAttempts, "Stage 8 capture maxAttempts");
  const status: Extract<PricingStage8CaptureJobStatusV2, "retry" | "dead"> =
    disposition === "retry" && job.attempts < maxAttempts
    ? "retry"
    : "dead";
  const updated = await database.pool.query(`
    UPDATE pricing_stage8_capture_jobs_v2
    SET status = $4,
        next_attempt_at = CASE
          WHEN $4 = 'retry' THEN now() + ($5 * interval '1 millisecond')
          ELSE next_attempt_at
        END,
        completed_at = CASE WHEN $4 = 'dead' THEN now() ELSE NULL END,
        locked_at = NULL, locked_by = NULL,
        last_error = $6, updated_at = now()
    WHERE id = $1 AND status = 'processing' AND locked_by = $2
      AND request_digest = $3 AND attempts = $7
  `, [
    job.id,
    workerId,
    job.requestDigest,
    status,
    retryMs,
    error.slice(0, 2_000),
    job.attempts,
  ]);
  if (updated.rowCount !== 1) throw transient(`Stage 8 capture job ${job.id} lost its lease`);
  return status;
}

function serializeJob(row: CaptureJobRow): PricingStage8CaptureControlJobV2 {
  return {
    id: row.id,
    idempotencyKey: row.idempotency_key,
    requestDigest: row.request_digest,
    targetGeneration: row.target_generation,
    recoveryGeneration: row.recovery_generation,
    windowStartAt: row.window_start_at,
    windowEndAt: row.window_end_at,
    minSamplesPerProvider: row.min_samples_per_provider,
    financialSampleSize: row.financial_sample_size,
    geminiClientAdmissions: row.gemini_client_admissions,
    operatorId: row.operator_id,
    reason: row.reason,
    status: row.status,
    attempts: row.attempts,
    nextAttemptAt: row.next_attempt_at,
    lockedAt: row.locked_at,
    lockedBy: row.locked_by,
    lastError: row.last_error,
    resultEngineEvidenceDigest: row.result_engine_evidence_digest,
    resultCombinedEvidenceDigest: row.result_combined_evidence_digest,
    resultPassed: row.result_passed,
    completedAt: row.completed_at,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function parseControlCombinedArtifact(row: CaptureArtifactControlRow): {
  observedAt: Date;
  validUntil: Date;
  blockerCount: string;
  blockers: Stage8CombinedBlocker[];
  blockersTruncated: boolean;
} | null {
  if (row.combined_evidence_digest === null) return null;
  const observedAt = row.combined_observed_at === null ? null : new Date(row.combined_observed_at);
  const validUntil = row.combined_valid_until === null ? null : new Date(row.combined_valid_until);
  const rawBlockers = row.combined_blockers;
  if (
    observedAt === null
    || validUntil === null
    || Number.isNaN(observedAt.getTime())
    || Number.isNaN(validUntil.getTime())
    || row.combined_blocker_count === null
    || !/^[0-9]+$/.test(row.combined_blocker_count)
    || !Array.isArray(rawBlockers)
    || rawBlockers.length > 100
  ) {
    throw permanent("durable combined Stage 8 control artifact differs from its indexed result");
  }
  const blockers = rawBlockers.map((untrusted): Stage8CombinedBlocker => {
    if (typeof untrusted !== "object" || untrusted === null || Array.isArray(untrusted)) {
      throw permanent("durable combined Stage 8 blocker is not an object");
    }
    const blocker = untrusted as Record<string, unknown>;
    const keys = Object.keys(blocker).sort().join(",");
    if (
      keys !== "code,count,source,subject_digests"
      || (blocker.source !== "commerce" && blocker.source !== "engine")
      || typeof blocker.code !== "string"
      || blocker.code.length === 0
      || typeof blocker.count !== "string"
      || !/^[1-9][0-9]*$/.test(blocker.count)
      || !Array.isArray(blocker.subject_digests)
      || blocker.subject_digests.length > 20
      || blocker.subject_digests.some((digest) =>
        typeof digest !== "string" || !STAGE8_SUBJECT_DIGEST_PATTERN.test(digest))
    ) {
      throw permanent("durable combined Stage 8 blocker has an invalid sanitized shape");
    }
    return {
      source: blocker.source,
      code: blocker.code,
      count: blocker.count,
      subject_digests: blocker.subject_digests as string[],
    };
  });
  const blockerCount = BigInt(row.combined_blocker_count);
  if (
    blockerCount < BigInt(blockers.length)
    || row.combined_passed !== (blockerCount === 0n)
    || row.combined_write_result === null
  ) {
    throw permanent("durable combined Stage 8 blocker summary differs from its indexed result");
  }
  return {
    observedAt,
    validUntil,
    blockerCount: row.combined_blocker_count,
    blockers,
    blockersTruncated: blockerCount > BigInt(blockers.length),
  };
}

export async function readPricingStage8CaptureControlV2(
  database: Database,
  limit = 20,
): Promise<PricingStage8CaptureControlV2> {
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > 100) {
    throw new RangeError("Stage 8 capture control limit must be an integer from 1 to 100");
  }
  const client = await database.pool.connect();
  let transactionOpen = false;
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    transactionOpen = true;
    const observed = await client.query<{ database_now: Date }>(
      "SELECT transaction_timestamp() AS database_now",
    );
    const counts = await client.query<{ status: PricingStage8CaptureJobStatusV2; count: string }>(`
      SELECT status, count(*)::text AS count
      FROM pricing_stage8_capture_jobs_v2
      GROUP BY status
    `);
    const jobs = await client.query<CaptureJobRow>(`
      SELECT id, idempotency_key, request_digest,
             target_generation::text, recovery_generation::text,
             window_start_at, window_end_at, min_samples_per_provider::text,
             financial_sample_size, gemini_client_admissions::text,
             operator_id, reason, status, attempts, next_attempt_at,
             locked_at, locked_by, last_error,
             result_engine_evidence_digest, result_combined_evidence_digest,
             result_passed, completed_at, created_at, updated_at
      FROM pricing_stage8_capture_jobs_v2
      ORDER BY created_at DESC, id
      LIMIT $1
    `, [limit]);
    const artifacts = await client.query<CaptureArtifactControlRow>(`
      SELECT id, job_id, attempt, engine_evidence_digest, engine_captured_at,
             combined_evidence_digest, combined_passed, combined_write_result,
             combined_payload_json::jsonb ->> 'observed_at' AS combined_observed_at,
             combined_payload_json::jsonb ->> 'valid_until' AS combined_valid_until,
             combined_payload_json::jsonb ->> 'blocker_count' AS combined_blocker_count,
             CASE WHEN combined_payload_json IS NULL THEN NULL ELSE
               jsonb_path_query_array(combined_payload_json::jsonb -> 'blockers', '$[0 to 99]')
             END AS combined_blockers,
             completed_at, created_at
      FROM pricing_stage8_capture_artifacts_v2
      ORDER BY created_at DESC, id
      LIMIT $1
    `, [Math.min(limit * 2, 100)]);
    await client.query("COMMIT");
    transactionOpen = false;
    const countsByStatus: Record<PricingStage8CaptureJobStatusV2, number> = {
      pending: 0,
      processing: 0,
      retry: 0,
      passed: 0,
      blocked: 0,
      dead: 0,
    };
    for (const row of counts.rows) {
      countsByStatus[row.status] = safeNumber(row.count, `${row.status} job count`, false);
    }
    return {
      databaseObservedAt: observed.rows[0]!.database_now,
      countsByStatus,
      jobs: jobs.rows.map(serializeJob),
      artifacts: artifacts.rows.map((row) => {
        const combined = parseControlCombinedArtifact(row);
        return {
          id: row.id,
          jobId: row.job_id,
          attempt: row.attempt,
          engineEvidenceDigest: row.engine_evidence_digest,
          engineCapturedAt: row.engine_captured_at,
          combinedEvidenceDigest: row.combined_evidence_digest,
          combinedPassed: row.combined_passed,
          combinedWriteResult: row.combined_write_result,
          combinedObservedAt: combined?.observedAt ?? null,
          combinedValidUntil: combined?.validUntil ?? null,
          combinedBlockerCount: combined?.blockerCount ?? null,
          combinedBlockers: combined?.blockers ?? null,
          combinedBlockersTruncated: combined?.blockersTruncated ?? null,
          completedAt: row.completed_at,
          createdAt: row.created_at,
        };
      }),
    };
  } catch (error) {
    if (transactionOpen) await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
