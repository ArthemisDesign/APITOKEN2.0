import { isDeepStrictEqual } from "node:util";
import {
  canonicalSha256V2Schema,
  pricingReleaseActivationAckV2Schema,
  pricingReleaseActivationRequestV2Schema,
  type PricingReleaseActivationAckV2,
  type PricingReleaseActivationRequestV2,
  type PricingReleaseProvisioningContextV2,
} from "@claude-api/contracts";
import { EngineClientError } from "@claude-api/engine-client";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";
import {
  Stage5MaterializerV2Error,
  stage5V2Digest,
} from "./pricing-stage5-materializer-v2.js";
import {
  capturePricingReleaseActivationAuthorityV2,
  type PricingReleaseActivationAuthorityReadersV2,
} from "./pricing-release-activation-authority.js";

const ACTIVATION_LEASE_INTERVAL = "5 minutes";

export type PricingReleaseActivationJobKindV2 = "cutover" | "recovery" | "successor";
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

export interface PricingReleaseActivationControlV2 {
  databaseObservedAt: Date;
  unresolvedPricingJobs: number;
  releases: Array<{
    generation: string;
    releaseKind: "target" | "recovery";
    status: string;
    contentDigest: string;
    engineReleaseDigest: string | null;
    commerceInventoryDigest: string;
    engineInventoryDigest: string;
    openkeysInventoryDigest: string;
    serviceInventoryDigest: string;
    createdAt: Date;
    updatedAt: Date;
  }>;
  evidence: Array<{
    evidenceDigest: string;
    engineEvidenceDigest: string | null;
    engineCapturedAt: Date | null;
    targetGeneration: string;
    targetDigest: string;
    recoveryGeneration: string;
    recoveryDigest: string;
    serviceInventoryDigest: string | null;
    legacyInflightCount: string;
    blockerCount: string;
    passed: boolean;
    observedAt: Date;
    validUntil: Date;
    targetStatus: string;
    recoveryStatus: string;
    targetEngineDigest: string | null;
    recoveryEngineDigest: string | null;
    fresh: boolean;
    sourceComplete: boolean;
    localBlockers: string[];
  }>;
  jobs: Array<{
    id: string;
    activationKind: PricingReleaseActivationJobKindV2;
    releaseGeneration: string;
    releaseDigest: string;
    evidenceDigest: string;
    status: string;
    attempts: number;
    operatorId: string | null;
    reason: string | null;
    lastError: string | null;
    resultDigest: string | null;
    confirmedAt: Date | null;
    createdAt: Date;
    updatedAt: Date;
  }>;
  receipts: Array<{
    activationId: string;
    activationKind: PricingReleaseActivationJobKindV2;
    releaseGeneration: string;
    releaseDigest: string;
    evidenceDigest: string;
    headVersion: string;
    receiptDigest: string;
    activatedAt: Date;
    createdAt: Date;
  }>;
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
  job_kind: "activate_release" | "activate_recovery" | "activate_successor";
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

function classifyAuthorityCaptureFailure(error: unknown): never {
  if (error instanceof EngineClientError) {
    throw error.retryable
      ? transient("engine activation authority is temporarily unavailable")
      : permanent("engine activation authority returned an invalid response");
  }
  if (
    error instanceof Stage5MaterializerV2Error
    && error.code === "openkeys_inventory_unavailable"
  ) {
    throw transient("OpenKeys activation authority is temporarily unavailable");
  }
  if (error instanceof TypeError && /fetch failed/i.test(error.message)) {
    throw transient("activation authority transport is temporarily unavailable");
  }
  throw error;
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
  return kind === "cutover"
    ? "activate_release"
    : kind === "recovery" ? "activate_recovery" : "activate_successor";
}

function activationJobKindV2(jobKind: ActivationJobRow["job_kind"]): PricingReleaseActivationJobKindV2 {
  if (jobKind === "activate_release") return "cutover";
  if (jobKind === "activate_recovery") return "recovery";
  if (jobKind === "activate_successor") return "successor";
  throw permanent("invalid activation job kind");
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

/**
 * The recovery expectation is the durable receipt of the activation that INSTALLED the target
 * head — the initial cutover or a later successor advance; both activate the evidence target.
 */
async function targetReceiptForRecovery(
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
    WHERE activation_kind IN ('cutover', 'successor')
      AND release_generation = $1 AND release_digest = $2
    FOR SHARE
  `, [evidence.target_generation, evidence.target_digest]);
  if (result.rows.length !== 1 || result.rows[0]!.receipt_payload === null) {
    throw permanent("recovery requires one complete durable target activation receipt");
  }
  const ack = pricingReleaseActivationAckV2Schema.parse(result.rows[0]!.receipt_payload);
  if (ack.result === "rejected") {
    throw permanent("stored target receipt payload is not a successful target ACK");
  }
  const kind = ack.activation.activation_kind;
  if (kind !== "cutover" && kind !== "successor") {
    throw permanent("stored target receipt payload is not a successful target ACK");
  }
  const row = result.rows[0]!;
  const originMatches = kind === "cutover"
    ? ack.activation.from_generation === null
      && ack.activation.from_digest === null
      && ack.activation.expected_head_version === 0
      && ack.activation.head.head_version === 1
    : ack.activation.from_generation !== null
      && ack.activation.from_digest !== null
      && ack.activation.expected_head_version > 0
      && ack.activation.head.head_version === ack.activation.expected_head_version + 1;
  if (
    ack.activation.head.active_generation !== parsePositiveSafeInteger(
      evidence.target_generation,
      "target generation",
    )
    || ack.activation.head.active_digest !== evidence.target_engine_digest
    || !originMatches
    || ack.activation.head.updated_ts !== ack.activation.activated_ts
    || ack.activation.activation_id !== row.activation_id
    || ack.activation.evidence_digest !== row.evidence_digest
    || String(ack.activation.head.head_version) !== row.head_version
    || activationReceiptDigest(ack) !== row.receipt_digest
    || ack.activation.activated_ts * 1_000 !== row.activated_at.getTime()
  ) {
    throw permanent("stored target receipt does not match its durable target identity");
  }
  if (ack.activation.evidence_digest === evidence.evidence_digest) {
    throw permanent("recovery requires fresh Stage 8 evidence after the target activation");
  }
  return ack;
}

/**
 * The successor expectation is the exact CURRENT head, proven by the newest durable activation
 * receipt regardless of its kind (cutover, recovery or an earlier successor). The engine CAS
 * still re-validates it against the live head; this read only pins the immutable durable shape.
 */
async function successorHeadForActivation(
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
    ORDER BY head_version DESC
    LIMIT 1
    FOR SHARE
  `);
  const row = result.rows[0];
  if (!row || row.receipt_payload === null) {
    throw permanent("successor requires one complete durable activation receipt");
  }
  const ack = pricingReleaseActivationAckV2Schema.parse(row.receipt_payload);
  if (ack.result === "rejected") {
    throw permanent("stored activation receipt payload is not a successful ACK");
  }
  if (
    ack.activation.activation_id !== row.activation_id
    || ack.activation.evidence_digest !== row.evidence_digest
    || String(ack.activation.head.head_version) !== row.head_version
    || activationReceiptDigest(ack) !== row.receipt_digest
    || ack.activation.activated_ts * 1_000 !== row.activated_at.getTime()
  ) {
    throw permanent("stored activation receipt does not match its durable head identity");
  }
  if (ack.activation.evidence_digest === evidence.evidence_digest) {
    throw permanent("successor requires fresh Stage 8 evidence after the previous activation");
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
    : input.activationKind === "recovery"
      ? { exact: (await targetReceiptForRecovery(client, evidence)).activation.head }
      : { exact: (await successorHeadForActivation(client, evidence)).activation.head };
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
      SELECT id::text AS id FROM engine_catalog_jobs job
      WHERE job.status IN ('pending', 'processing', 'retry')
         OR (job.status = 'dead' AND NOT EXISTS(
           SELECT 1 FROM engine_catalog_jobs newer
           WHERE newer.product_id = job.product_id AND newer.status = 'confirmed'
             AND newer.generation > job.generation))
      UNION ALL SELECT id::text FROM engine_switch_jobs job
      WHERE job.status IN ('pending', 'processing', 'retry')
         OR (job.status = 'dead' AND NOT EXISTS(
           SELECT 1 FROM engine_switch_jobs newer
           WHERE newer.status = 'confirmed' AND newer.generation > job.generation))
      UNION ALL SELECT id::text FROM engine_policy_jobs job
      WHERE job.status IN ('pending', 'processing', 'retry')
         OR (job.status = 'dead' AND NOT EXISTS(
           SELECT 1 FROM engine_policy_jobs newer
           WHERE newer.binding_id = job.binding_id AND newer.status = 'confirmed'
             AND newer.effective_version > job.effective_version))
      UNION ALL SELECT id::text FROM engine_pricing_jobs
      WHERE status IN ('pending', 'processing', 'retry')
      UNION ALL SELECT job.id::text FROM pricing_release_control_jobs_v2 job
      WHERE (job.status IN ('pending', 'processing', 'retry')
         OR (job.status = 'dead' AND NOT EXISTS(
           SELECT 1 FROM pricing_release_control_jobs_v2 newer
           WHERE newer.job_kind = job.job_kind AND newer.status = 'confirmed'
             AND newer.release_generation > job.release_generation)))
        AND ($1::uuid IS NULL OR job.id <> $1::uuid)
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
    const releaseGeneration = input.activationKind === "recovery"
      ? evidence.recovery_generation
      : evidence.target_generation;
    const releaseDigest = input.activationKind === "recovery"
      ? evidence.recovery_digest
      : evidence.target_digest;
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
    await client.query(`
      INSERT INTO audit_log (
        actor_type, actor_id, action, target_type, target_id, metadata
      ) VALUES (
        'admin', $1, 'pricing_release_activation_staged',
        'pricing_release_control_job_v2', $2,
        jsonb_build_object(
          'activation_kind', $3::text,
          'evidence_digest', $4::text,
          'release_generation', $5::text,
          'release_digest', $6::text,
          'expected_head_version', $7::text,
          'reason', $8::text
        )
      )
    `, [
      input.operatorId,
      inserted.rows[0]!.id,
      input.activationKind,
      evidenceDigest,
      releaseGeneration,
      releaseDigest,
      String(expectedHeadVersion),
      input.reason,
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

export async function readPricingReleaseActivationControlV2(
  database: Database,
  limit = 20,
): Promise<PricingReleaseActivationControlV2> {
  if (!Number.isInteger(limit) || limit < 1 || limit > 100) {
    throw new RangeError("activation control limit must be an integer from 1 to 100");
  }
  const client = await database.pool.connect();
  let transactionOpen = false;
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    transactionOpen = true;
    const observed = await client.query<{ database_now: Date }>(
      "SELECT transaction_timestamp() AS database_now",
    );
    const databaseObservedAt = observed.rows[0]!.database_now;
    const unresolvedPricingJobs = await unresolvedPricingJobCount(client);
    const releases = await client.query<{
      generation: string;
      release_kind: "target" | "recovery";
      status: string;
      content_digest: string;
      engine_release_digest: string | null;
      commerce_inventory_digest: string;
      engine_inventory_digest: string;
      openkeys_inventory_digest: string;
      service_inventory_digest: string;
      created_at: Date;
      updated_at: Date;
    }>(`
      SELECT generation::text, release_kind, status, content_digest,
             engine_release_digest, commerce_inventory_digest,
             engine_inventory_digest, openkeys_inventory_digest,
             service_inventory_digest, created_at, updated_at
      FROM pricing_release_plans_v2
      ORDER BY generation DESC
      LIMIT $1
    `, [limit]);
    const evidence = await client.query<{
      evidence_digest: string;
      engine_evidence_digest: string | null;
      engine_captured_at: Date | null;
      target_generation: string;
      target_digest: string;
      recovery_generation: string;
      recovery_digest: string;
      service_inventory_digest: string | null;
      legacy_inflight_count: string;
      blocker_count: string;
      passed: boolean;
      observed_at: Date;
      valid_until: Date;
      target_status: string;
      recovery_status: string;
      target_engine_digest: string | null;
      recovery_engine_digest: string | null;
    }>(`
      SELECT evidence.evidence_digest, evidence.engine_evidence_digest,
             evidence.engine_captured_at, evidence.target_generation::text,
             evidence.target_digest, evidence.recovery_generation::text,
             evidence.recovery_digest, evidence.service_inventory_digest,
             evidence.legacy_inflight_count::text, evidence.blocker_count::text,
             evidence.passed, evidence.observed_at, evidence.valid_until,
             target.status AS target_status, recovery.status AS recovery_status,
             target.engine_release_digest AS target_engine_digest,
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
      ORDER BY evidence.observed_at DESC, evidence.evidence_digest
      LIMIT $1
    `, [limit]);
    const jobs = await client.query<{
      id: string;
      job_kind: "activate_release" | "activate_recovery" | "activate_successor";
      release_generation: string;
      release_digest: string;
      stage8_evidence_digest: string;
      activation_payload: unknown;
      status: string;
      attempts: number;
      last_error: string | null;
      result_digest: string | null;
      confirmed_at: Date | null;
      created_at: Date;
      updated_at: Date;
    }>(`
      SELECT id, job_kind, release_generation::text, release_digest,
             stage8_evidence_digest, activation_payload, status, attempts,
             last_error, result_digest, confirmed_at, created_at, updated_at
      FROM pricing_release_control_jobs_v2
      WHERE job_kind IN ('activate_release', 'activate_recovery', 'activate_successor')
      ORDER BY created_at DESC, id
      LIMIT $1
    `, [limit]);
    const receipts = await client.query<{
      activation_id: string;
      activation_kind: PricingReleaseActivationJobKindV2;
      release_generation: string;
      release_digest: string;
      evidence_digest: string;
      head_version: string;
      receipt_digest: string;
      activated_at: Date;
      created_at: Date;
    }>(`
      SELECT activation_id, activation_kind, release_generation::text,
             release_digest, evidence_digest, head_version::text,
             receipt_digest, activated_at, created_at
      FROM pricing_release_activation_receipts_v2
      ORDER BY head_version DESC
      LIMIT $1
    `, [limit]);
    await client.query("COMMIT");
    transactionOpen = false;

    return {
      databaseObservedAt,
      unresolvedPricingJobs,
      releases: releases.rows.map((row) => ({
        generation: row.generation,
        releaseKind: row.release_kind,
        status: row.status,
        contentDigest: row.content_digest,
        engineReleaseDigest: row.engine_release_digest,
        commerceInventoryDigest: row.commerce_inventory_digest,
        engineInventoryDigest: row.engine_inventory_digest,
        openkeysInventoryDigest: row.openkeys_inventory_digest,
        serviceInventoryDigest: row.service_inventory_digest,
        createdAt: row.created_at,
        updatedAt: row.updated_at,
      })),
      evidence: evidence.rows.map((row) => {
        const fresh = row.valid_until.getTime() > databaseObservedAt.getTime();
        const sourceComplete = row.engine_evidence_digest !== null
          && row.engine_captured_at !== null
          && row.service_inventory_digest !== null;
        const localBlockers = [
          ...(!row.passed || row.blocker_count !== "0" ? ["stage8_not_passed"] : []),
          ...(row.engine_evidence_digest === null || row.engine_captured_at === null
            ? ["engine_source_identity_missing"] : []),
          ...(row.service_inventory_digest === null ? ["service_inventory_identity_missing"] : []),
          ...(!fresh ? ["evidence_expired"] : []),
          ...(row.target_status !== "prepared" ? ["target_release_not_prepared"] : []),
          ...(row.recovery_status !== "prepared" ? ["recovery_release_not_prepared"] : []),
          ...(row.target_engine_digest === null ? ["target_engine_digest_missing"] : []),
          ...(row.recovery_engine_digest === null ? ["recovery_engine_digest_missing"] : []),
          ...(unresolvedPricingJobs !== 0 ? ["unresolved_pricing_jobs"] : []),
        ];
        return {
          evidenceDigest: row.evidence_digest,
          engineEvidenceDigest: row.engine_evidence_digest,
          engineCapturedAt: row.engine_captured_at,
          targetGeneration: row.target_generation,
          targetDigest: row.target_digest,
          recoveryGeneration: row.recovery_generation,
          recoveryDigest: row.recovery_digest,
          serviceInventoryDigest: row.service_inventory_digest,
          legacyInflightCount: row.legacy_inflight_count,
          blockerCount: row.blocker_count,
          passed: row.passed,
          observedAt: row.observed_at,
          validUntil: row.valid_until,
          targetStatus: row.target_status,
          recoveryStatus: row.recovery_status,
          targetEngineDigest: row.target_engine_digest,
          recoveryEngineDigest: row.recovery_engine_digest,
          fresh,
          sourceComplete,
          localBlockers,
        };
      }),
      jobs: jobs.rows.map((row) => {
        const payload = pricingReleaseActivationRequestV2Schema.safeParse(row.activation_payload);
        return {
          id: row.id,
          activationKind: activationJobKindV2(row.job_kind),
          releaseGeneration: row.release_generation,
          releaseDigest: row.release_digest,
          evidenceDigest: row.stage8_evidence_digest,
          status: row.status,
          attempts: row.attempts,
          operatorId: payload.success ? payload.data.operator_id : null,
          reason: payload.success ? payload.data.reason : null,
          lastError: row.last_error,
          resultDigest: row.result_digest,
          confirmedAt: row.confirmed_at,
          createdAt: row.created_at,
          updatedAt: row.updated_at,
        };
      }),
      receipts: receipts.rows.map((row) => ({
        activationId: row.activation_id,
        activationKind: row.activation_kind,
        releaseGeneration: row.release_generation,
        releaseDigest: row.release_digest,
        evidenceDigest: row.evidence_digest,
        headVersion: row.head_version,
        receiptDigest: row.receipt_digest,
        activatedAt: row.activated_at,
        createdAt: row.created_at,
      })),
    };
  } catch (error) {
    if (transactionOpen) await client.query("ROLLBACK");
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
  const activationKind: PricingReleaseActivationJobKindV2 = activationJobKindV2(row.job_kind);
  const request = await requestFromEvidence(client, row, {
    activationKind,
    operatorId: pricingReleaseActivationRequestV2Schema.parse(row.activation_payload).operator_id,
    reason: pricingReleaseActivationRequestV2Schema.parse(row.activation_payload).reason,
  }, row.attempts === 1);
  const storedRequest = pricingReleaseActivationRequestV2Schema.parse(row.activation_payload);
  const releaseGeneration = activationKind === "recovery" ? row.recovery_generation : row.target_generation;
  const releaseDigest = activationKind === "recovery" ? row.recovery_digest : row.target_digest;
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
    let authority: Awaited<ReturnType<typeof capturePricingReleaseActivationAuthorityV2>>;
    try {
      authority = await capturePricingReleaseActivationAuthorityV2(client, authorityReaders, {
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
    } catch (error) {
      classifyAuthorityCaptureFailure(error);
    }
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
    WHERE job_kind IN ('activate_release', 'activate_recovery', 'activate_successor')
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
      WHERE job_kind IN ('activate_release', 'activate_recovery', 'activate_successor')
        AND status = 'processing'
        AND (locked_at IS NULL OR locked_at < now() - interval '${ACTIVATION_LEASE_INTERVAL}')
    `);
    const candidate = await client.query<{ id: string }>(`
      SELECT id
      FROM pricing_release_control_jobs_v2
      WHERE job_kind IN ('activate_release', 'activate_recovery', 'activate_successor')
        AND status IN ('pending', 'retry') AND next_attempt_at <= now()
        AND NOT EXISTS (
          SELECT 1 FROM engine_catalog_jobs job
          WHERE job.status IN ('pending', 'processing', 'retry')
             OR (job.status = 'dead' AND NOT EXISTS(
               SELECT 1 FROM engine_catalog_jobs newer
               WHERE newer.product_id = job.product_id AND newer.status = 'confirmed'
                 AND newer.generation > job.generation))
        )
        AND NOT EXISTS (
          SELECT 1 FROM engine_switch_jobs job
          WHERE job.status IN ('pending', 'processing', 'retry')
             OR (job.status = 'dead' AND NOT EXISTS(
               SELECT 1 FROM engine_switch_jobs newer
               WHERE newer.status = 'confirmed' AND newer.generation > job.generation))
        )
        AND NOT EXISTS (
          SELECT 1 FROM engine_policy_jobs job
          WHERE job.status IN ('pending', 'processing', 'retry')
             OR (job.status = 'dead' AND NOT EXISTS(
               SELECT 1 FROM engine_policy_jobs newer
               WHERE newer.binding_id = job.binding_id AND newer.status = 'confirmed'
                 AND newer.effective_version > job.effective_version))
        )
        AND NOT EXISTS (
          SELECT 1 FROM engine_pricing_jobs
          WHERE status IN ('pending', 'processing', 'retry')
        )
        AND NOT EXISTS (
          SELECT 1 FROM pricing_release_control_jobs_v2 other
          WHERE (other.status IN ('pending', 'processing', 'retry')
             OR (other.status = 'dead' AND NOT EXISTS(
               SELECT 1 FROM pricing_release_control_jobs_v2 newer
               WHERE newer.job_kind = other.job_kind AND newer.status = 'confirmed'
                 AND newer.release_generation > other.release_generation)))
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
  const destination = job.request.activation_kind === "recovery"
    ? {
        generation: job.request.evidence.recovery_generation,
        digest: job.request.evidence.recovery_digest,
      }
    : {
        generation: job.request.evidence.target_generation,
        digest: job.request.evidence.target_digest,
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

export interface PricingReleaseActivationReconcileReadersV2 {
  engine: {
    getPricingReleaseProvisioningContextV2(): Promise<PricingReleaseProvisioningContextV2 | null>;
  };
}

export interface PricingReleaseActivationReconcileResultV2 {
  jobId: string;
  activationId: string;
  resultDigest: string;
  status: "reconciled" | "unchanged";
}

/**
 * Repairs the one durable gap a lost or misasserted activation ACK leaves behind: the engine CAS
 * committed (the head moved) but commerce never stored the receipt, so the dead job would
 * otherwise block every later advance (the successor expectation and the recovery target receipt
 * are read only from durable receipts). The reconcile never asks the engine to mutate anything:
 * it reads the provisioning context, requires the engine-attested activation to match the dead
 * job's immutable request exactly (kind, evidence digest, destination head and monotonic head
 * version), rebuilds the full receipt from the two exact identities, and atomically stores the
 * receipt, confirms the job and writes the audit event. A head that has already moved past the
 * job's destination, or any identity disagreement, fails closed with no writes.
 */
export async function reconcileLostPricingActivationReceiptV2(
  database: Database,
  readers: PricingReleaseActivationReconcileReadersV2,
  input: { jobId: string; actorId: string; reason: string },
): Promise<PricingReleaseActivationReconcileResultV2> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query("SET LOCAL statement_timeout = '30s'");
    await client.query("SET LOCAL lock_timeout = '5s'");
    const jobResult = await client.query<{
      id: string;
      status: string;
      release_generation: string;
      release_digest: string;
      payload_digest: string;
      expected_head_version: string;
      stage8_evidence_digest: string;
      activation_payload: unknown;
    }>(`
      SELECT id, status, release_generation::text, release_digest, payload_digest,
             expected_head_version::text, stage8_evidence_digest, activation_payload
      FROM pricing_release_control_jobs_v2
      WHERE id = $1
      FOR UPDATE
    `, [input.jobId]);
    const row = jobResult.rows[0];
    if (!row) throw permanent("reconcile requires an existing activation job");
    const request = pricingReleaseActivationRequestV2Schema.parse(row.activation_payload);
    const existing = await client.query<{ activation_id: string }>(`
      SELECT activation_id FROM pricing_release_activation_receipts_v2
      WHERE release_generation = $1 AND release_digest = $2 AND evidence_digest = $3
    `, [row.release_generation, row.release_digest, row.stage8_evidence_digest]);
    if (row.status === "confirmed" && existing.rows[0]) {
      await client.query("COMMIT");
      return {
        jobId: row.id,
        activationId: existing.rows[0].activation_id,
        resultDigest: row.payload_digest,
        status: "unchanged",
      };
    }
    if (row.status !== "dead") {
      throw permanent(`reconcile requires a terminal dead job, not ${row.status}`);
    }

    const context = await readers.engine.getPricingReleaseProvisioningContextV2();
    if (!context) throw permanent("reconcile requires an active engine provisioning context");
    const destination = request.activation_kind === "recovery"
      ? {
          generation: request.evidence.recovery_generation,
          digest: request.evidence.recovery_digest,
        }
      : {
          generation: request.evidence.target_generation,
          digest: request.evidence.target_digest,
        };
    const expectedFrom = request.expectation === "absent"
      ? { generation: null, digest: null, headVersion: 0 }
      : {
          generation: request.expectation.exact.active_generation,
          digest: request.expectation.exact.active_digest,
          headVersion: request.expectation.exact.head_version,
        };
    if (
      context.activation.activation_kind !== request.activation_kind
      || context.activation.evidence_digest !== request.evidence.evidence_digest
      || context.head.active_generation !== destination.generation
      || context.head.active_digest !== destination.digest
      || context.head.head_version !== expectedFrom.headVersion + 1
      || context.head.updated_ts !== context.activation.activated_ts
      || context.active_release.generation !== destination.generation
      || context.active_release.content_digest !== destination.digest
    ) {
      throw permanent("engine provisioning context does not attest the dead job's activation");
    }
    const ack = pricingReleaseActivationAckV2Schema.parse({
      result: "applied",
      activation: {
        activation_id: String(context.activation.activation_id),
        activation_kind: context.activation.activation_kind,
        from_generation: expectedFrom.generation,
        from_digest: expectedFrom.digest,
        expected_head_version: expectedFrom.headVersion,
        head: context.head,
        evidence_digest: context.activation.evidence_digest,
        operator_id: request.operator_id,
        reason: request.reason,
        activated_ts: context.activation.activated_ts,
      },
    });
    const job: ClaimedPricingReleaseActivationJobV2 = {
      id: row.id,
      attempts: 0,
      releaseGeneration: BigInt(row.release_generation),
      releaseDigest: row.release_digest,
      evidenceDigest: row.stage8_evidence_digest,
      expectedHeadVersion: BigInt(row.expected_head_version),
      payloadDigest: row.payload_digest,
      request,
    };
    assertReceiptMatchesJob(job, ack as Extract<PricingReleaseActivationAckV2, { result: "applied" | "unchanged" }>);
    const receiptDigest = activationReceiptDigest(ack as Extract<PricingReleaseActivationAckV2, { result: "applied" | "unchanged" }>);
    const resultDigest = activationResultDigest(job.payloadDigest, receiptDigest);
    const activatedAt = new Date(context.activation.activated_ts * 1_000);
    await client.query(`
      INSERT INTO pricing_release_activation_receipts_v2 (
        activation_id, activation_kind, release_generation, release_digest,
        evidence_digest, head_version, receipt_digest, receipt_payload, activated_at
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9)
    `, [
      String(context.activation.activation_id),
      context.activation.activation_kind,
      row.release_generation,
      row.release_digest,
      row.stage8_evidence_digest,
      context.head.head_version,
      receiptDigest,
      JSON.stringify(ack),
      activatedAt,
    ]);
    const confirmed = await client.query(`
      UPDATE pricing_release_control_jobs_v2
      SET status = 'confirmed', result_digest = $2, confirmed_at = now(),
          locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
      WHERE id = $1 AND status = 'dead'
    `, [row.id, resultDigest]);
    if (confirmed.rowCount !== 1) {
      throw permanent("activation job changed while its receipt was reconciled");
    }
    await client.query(`
      INSERT INTO audit_log (
        actor_type, actor_id, action, target_type, target_id, metadata
      ) VALUES (
        'admin', $1, 'pricing_release_activation_reconciled',
        'pricing_release_control_job_v2', $2,
        jsonb_build_object(
          'activation_id', $3::text,
          'activation_kind', $4::text,
          'evidence_digest', $5::text,
          'head_version', $6::text,
          'reason', $7::text
        )
      )
    `, [
      input.actorId,
      row.id,
      String(context.activation.activation_id),
      context.activation.activation_kind,
      row.stage8_evidence_digest,
      String(context.head.head_version),
      input.reason,
    ]);
    await client.query("COMMIT");
    return {
      jobId: row.id,
      activationId: String(context.activation.activation_id),
      resultDigest,
      status: "reconciled",
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
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
