/**
 * Pricing release orchestrator v2 — one durable intent drives a full successor release cycle.
 *
 * The orchestrator never replaces a gate: every sub-step runs through its existing durable job
 * (catalog/switch delivery, Stage 5 materialize, Stage 6 funding normalization, Stage 7 shadow
 * rollout, Stage 8 capture, Stage 9 activation) with unchanged evidence and audit. The state
 * machine only sequences them, re-cycles on inventory drift (bounded, like the converge bridge),
 * re-captures on evidence TTL expiry and reconciles a lost activation ACK. At most one active
 * orchestration exists (partial unique index, migration 0044).
 */
import { randomUUID } from "node:crypto";
import {
  canonicalSha256V2Schema,
  pricingReleaseActivationOperatorV2Schema,
  pricingStageControlMutationReasonV2Schema,
} from "@claude-api/contracts";
import { EngineClientError, type EngineClient } from "@claude-api/engine-client";
import type { Database } from "./client.js";
import {
  stageStoredPricingCatalogControlJob,
  stageStoredProviderSwitchControlJob,
} from "./pricing-control-jobs.js";
import {
  getFundingNormalizationStageStatusV2,
  stageFundingNormalizationJobV2,
} from "./funding-normalization-jobs.js";
import {
  readPricingShadowRolloutControlV2,
  stagePricingShadowRolloutV2,
} from "./pricing-shadow-rollout-jobs-v2.js";
import { stagePricingStage8CaptureJobV2 } from "./pricing-stage8-capture-jobs-v2.js";
import {
  reconcileLostPricingActivationReceiptV2,
  stagePricingReleaseActivationJobV2,
} from "./pricing-release-activation-jobs.js";
import {
  Stage5MaterializerV2Error,
  stage5V2Digest,
  type Stage5V2OpenKeysReader,
} from "./pricing-stage5-materializer-v2.js";
import { runStage5MaterializerV2 } from "./pricing-stage5-materializer-v2-store.js";

export type PricingReleaseOrchestrationStepV2 =
  | "materialize_pair"
  | "deliver_catalogs"
  | "normalize_funding"
  | "rollout"
  | "capture"
  | "activate"
  | "verify";

const MAX_CYCLES = 3;
const CAPTURE_WINDOW_SECONDS = 900;
const CAPTURE_WINDOW_LAG_SECONDS = 30;
const CAPTURE_MIN_SAMPLES = 8;
const CAPTURE_FINANCIAL_SAMPLE = 100;

/** Capture blockers that mean "the window was not quiet or the plane was busy" — re-capture. */
const TRANSIENT_CAPTURE_BLOCKERS = new Set([
  "authority_changed_during_validation_window",
  "pricing_control_job_backlog_or_failure",
]);

/** At most this many capture attempts per pair before the intent dies. */
const MAX_CAPTURES_PER_PAIR = 5;

/** Capture blockers that mean "the world moved under the pair" — a fresh cycle is the remedy. */
const DRIFT_BLOCKER_CODES = new Set([
  "target_release_identity_drift",
  "recovery_release_identity_drift",
  "target_release_assignment_inventory_drift",
  "recovery_release_assignment_inventory_drift",
  "engine_inventory_changed_between_scans",
  "openkeys_inventory_changed_between_scans",
  "commerce_inventory_drift",
  "engine_inventory_drift",
  "openkeys_inventory_drift",
  "service_inventory_drift",
]);

export class PricingReleaseOrchestrationV2Error extends Error {
  constructor(
    message: string,
    readonly permanent: boolean,
  ) {
    super(message);
    this.name = "PricingReleaseOrchestrationV2Error";
  }
}

function permanent(message: string): PricingReleaseOrchestrationV2Error {
  return new PricingReleaseOrchestrationV2Error(message, true);
}

/** SERIALIZABLE conflicts and deadlocks are concurrency facts, not failures: retry next tick. */
export function isSerializationConflictV2(message: string): boolean {
  return /could not serialize|deadlock detected/i.test(message);
}

export interface StagePricingReleaseOrchestrationV2Input {
  idempotencyKey: string;
  capabilityGeneration: number;
  operatorId: string;
  reason: string;
}

export interface PricingReleaseOrchestrationReadersV2 {
  engine: EngineClient;
  openkeys: Stage5V2OpenKeysReader;
}

interface OrchestrationRow {
  id: string;
  step: PricingReleaseOrchestrationStepV2;
  status: string;
  cycle: number;
  capability_generation: string;
  stage5_run_id: string | null;
  target_generation: string | null;
  recovery_generation: string | null;
  evidence_digest: string | null;
  activation_kind: string | null;
  operator_id: string;
  reason: string;
  database_now: Date;
}

const ORCHESTRATION_COLUMNS = `
  id::text, step, status, cycle, capability_generation::text,
  stage5_run_id::text, target_generation::text, recovery_generation::text,
  evidence_digest, activation_kind, operator_id, reason,
  transaction_timestamp() AS database_now
`;

export async function stagePricingReleaseOrchestrationV2(
  database: Database,
  untrustedInput: StagePricingReleaseOrchestrationV2Input,
): Promise<{ orchestrationId: string; idempotentReplay: boolean }> {
  const capabilityGeneration = untrustedInput.capabilityGeneration;
  if (!Number.isSafeInteger(capabilityGeneration) || capabilityGeneration <= 0) {
    throw new RangeError("capability generation must be a positive safe integer");
  }
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
    .test(untrustedInput.idempotencyKey)) {
    throw new TypeError("idempotencyKey must be a UUID");
  }
  const operatorId = pricingReleaseActivationOperatorV2Schema.parse(untrustedInput.operatorId);
  const reason = pricingStageControlMutationReasonV2Schema.parse(untrustedInput.reason);
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    const existing = await client.query<{
      id: string;
      capability_generation: string;
      operator_id: string;
      reason: string;
    }>(`
      SELECT id::text, capability_generation::text, operator_id, reason
      FROM pricing_release_orchestrations_v2
      WHERE idempotency_key = $1
      FOR UPDATE
    `, [untrustedInput.idempotencyKey]);
    const row = existing.rows[0];
    if (row) {
      if (
        row.capability_generation !== String(capabilityGeneration)
        || row.operator_id !== operatorId
        || row.reason !== reason
      ) {
        throw permanent("orchestration idempotency key has a different immutable payload");
      }
      await client.query("COMMIT");
      return { orchestrationId: row.id, idempotentReplay: true };
    }
    const inserted = await client.query<{ id: string }>(`
      INSERT INTO pricing_release_orchestrations_v2 (
        idempotency_key, capability_generation, step, operator_id, reason
      ) VALUES ($1, $2, 'materialize_pair', $3, $4)
      RETURNING id
    `, [untrustedInput.idempotencyKey, capabilityGeneration, operatorId, reason])
      .catch((error: unknown) => {
        if (
          typeof error === "object" && error !== null && "code" in error
          && (error as { code: string }).code === "23505"
        ) {
          throw permanent("another pricing release orchestration is already active");
        }
        throw error;
      });
    await client.query(`
      INSERT INTO audit_log (
        actor_type, actor_id, action, target_type, target_id, metadata
      ) VALUES (
        'admin', $1, 'pricing_release_orchestration_staged',
        'pricing_release_orchestration_v2', $2,
        jsonb_build_object(
          'capability_generation', $3::text,
          'reason', $4::text
        )
      )
    `, [operatorId, inserted.rows[0]!.id, String(capabilityGeneration), reason]);
    await client.query("COMMIT");
    return { orchestrationId: inserted.rows[0]!.id, idempotentReplay: false };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export interface PricingReleaseOrchestrationControlEntryV2 {
  id: string;
  idempotency_key: string;
  capability_generation: string;
  step: PricingReleaseOrchestrationStepV2;
  status: string;
  cycle: number;
  stage5_run_id: string | null;
  target_generation: string | null;
  recovery_generation: string | null;
  evidence_digest: string | null;
  activation_kind: string | null;
  operator_id: string;
  reason: string;
  last_error: string | null;
  confirmed_at: Date | null;
  created_at: Date;
  updated_at: Date;
}

export interface PricingReleaseOrchestrationControlV2 {
  orchestrations: PricingReleaseOrchestrationControlEntryV2[];
}

export async function readPricingReleaseOrchestrationControlV2(
  database: Database,
): Promise<PricingReleaseOrchestrationControlV2> {
  const result = await database.pool.query(`
    SELECT id::text, idempotency_key, capability_generation::text, step, status, cycle,
           stage5_run_id::text, target_generation::text, recovery_generation::text,
           evidence_digest, activation_kind, operator_id, reason, last_error,
           confirmed_at, created_at, updated_at
    FROM pricing_release_orchestrations_v2
    ORDER BY created_at DESC
    LIMIT 10
  `);
  return {
    orchestrations: result.rows.map((row) => ({
      id: row.id,
      idempotency_key: row.idempotency_key,
      capability_generation: row.capability_generation,
      step: row.step,
      status: row.status,
      cycle: row.cycle,
      stage5_run_id: row.stage5_run_id,
      target_generation: row.target_generation,
      recovery_generation: row.recovery_generation,
      evidence_digest: row.evidence_digest,
      activation_kind: row.activation_kind,
      operator_id: row.operator_id,
      reason: row.reason,
      last_error: row.last_error,
      confirmed_at: row.confirmed_at,
      created_at: row.created_at,
      updated_at: row.updated_at,
    })),
  };
}

async function loadActiveOrchestration(
  database: Database,
): Promise<OrchestrationRow | null> {
  const result = await database.pool.query<OrchestrationRow>(`
    SELECT ${ORCHESTRATION_COLUMNS}
    FROM pricing_release_orchestrations_v2
    WHERE status = 'active'
  `);
  return result.rows[0] ?? null;
}

/** CAS-guarded transition: only applies when the row is still in the expected step. */
async function transition(
  database: Database,
  id: string,
  expectedStep: PricingReleaseOrchestrationStepV2,
  patch: string,
  parameters: unknown[],
): Promise<void> {
  const result = await database.pool.query(
    `UPDATE pricing_release_orchestrations_v2
     SET ${patch}, updated_at = now()
     WHERE id = $1 AND step = $2 AND status = 'active'`,
    [id, expectedStep, ...parameters],
  );
  if (result.rowCount !== 1) {
    throw new PricingReleaseOrchestrationV2Error(
      `orchestration ${id} moved while step ${expectedStep} was advancing`,
      false,
    );
  }
}

async function kill(
  database: Database,
  id: string,
  expectedStep: PricingReleaseOrchestrationStepV2,
  message: string,
): Promise<void> {
  await transition(database, id, expectedStep, "status = 'dead', last_error = $3", [message]);
}

async function freshCycle(
  database: Database,
  row: OrchestrationRow,
  reason: string,
): Promise<void> {
  if (row.cycle >= MAX_CYCLES) {
    await kill(
      database,
      row.id,
      row.step,
      `inventory kept drifting through ${MAX_CYCLES} cycles; last drift: ${reason}`,
    );
    return;
  }
  await transition(
    database,
    row.id,
    row.step,
    `step = 'materialize_pair', cycle = cycle + 1,
     stage5_run_id = NULL, target_generation = NULL, recovery_generation = NULL,
     evidence_digest = NULL, activation_kind = NULL, last_error = $3`,
    [`fresh cycle after drift: ${reason}`],
  );
}

async function stepMaterializePair(
  database: Database,
  readers: PricingReleaseOrchestrationReadersV2,
  row: OrchestrationRow,
): Promise<void> {
  const dryRun = await runStage5MaterializerV2(database, readers.engine, readers.openkeys, {
    mode: "dry_run",
  });
  const capabilityGeneration = Number(dryRun.plan.capability.generation);
  if (capabilityGeneration !== Number(row.capability_generation)) {
    await kill(
      database,
      row.id,
      "materialize_pair",
      `deployed constants plan capability generation ${capabilityGeneration}, ` +
        `not the intended ${row.capability_generation}`,
    );
    return;
  }
  if (dryRun.plan.blockers.length > 0) {
    await kill(
      database,
      row.id,
      "materialize_pair",
      `Stage 5 dry run blocked: ${dryRun.plan.blockers.map((blocker) => blocker.blocker_code).join(",")}`,
    );
    return;
  }
  let applied;
  try {
    applied = await runStage5MaterializerV2(database, readers.engine, readers.openkeys, {
      mode: "apply",
      expectedPlanDigest: dryRun.plan.plan_digest,
      audit: { actorId: row.operator_id, reason: row.reason },
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (
      error instanceof Stage5MaterializerV2Error
      && (error as { code?: string }).code === "expected_plan_stale"
    ) {
      // The inventory moved between the dry run and apply: wait a tick and re-plan.
      return;
    }
    if (isSerializationConflictV2(message)) return;
    throw error;
  }
  if (applied.status !== "materializing" || applied.run_id === null) {
    await kill(
      database,
      row.id,
      "materialize_pair",
      `Stage 5 apply ended ${applied.status}: ${applied.plan.blockers.map((b) => b.blocker_code).join(",")}`,
    );
    return;
  }
  await transition(
    database,
    row.id,
    "materialize_pair",
    `step = 'deliver_catalogs', stage5_run_id = $3,
     target_generation = $4, recovery_generation = $5, last_error = NULL`,
    [applied.run_id, applied.plan.target_generation, applied.plan.recovery_generation],
  );
}

async function stepDeliverCatalogs(
  database: Database,
  row: OrchestrationRow,
): Promise<void> {
  const audit = { actorId: row.operator_id, reason: row.reason };
  const catalogGenerations = await database.pool.query<{
    product_id: string;
    generation: string;
  }>(`
    SELECT product_id, max(generation)::text AS generation
    FROM product_catalog_versions
    WHERE capability_generation = $1
    GROUP BY product_id
  `, [row.capability_generation]);
  const switchGeneration = await database.pool.query<{ generation: string }>(`
    SELECT max(generation)::text AS generation
    FROM provider_switch_versions
    WHERE capability_generation = $1
  `, [row.capability_generation]);
  const catalogs = new Map(
    catalogGenerations.rows.map((catalog) => [catalog.product_id, Number(catalog.generation)]),
  );
  const main = catalogs.get("main");
  const openkeys = catalogs.get("openkeys");
  const switches = switchGeneration.rows[0]?.generation;
  if (main === undefined || openkeys === undefined || !switches) {
    await kill(
      database,
      row.id,
      "deliver_catalogs",
      `no stored catalog/switch versions for capability generation ${row.capability_generation}`,
    );
    return;
  }
  await stageStoredPricingCatalogControlJob(database, "main", main, audit);
  await stageStoredPricingCatalogControlJob(database, "openkeys", openkeys, audit);
  await stageStoredProviderSwitchControlJob(database, Number(switches), audit);
  const pending = await database.pool.query<{ subject: string }>(`
    SELECT 'catalog:' || job.product_id || ':' || job.generation::text AS subject
    FROM engine_catalog_jobs job
    WHERE (job.product_id, job.generation) IN (('main', $1), ('openkeys', $2))
      AND (job.status IN ('pending', 'processing', 'retry')
        OR (job.status = 'dead' AND NOT EXISTS(
          SELECT 1 FROM engine_catalog_jobs newer
          WHERE newer.product_id = job.product_id AND newer.status = 'confirmed'
            AND newer.generation > job.generation)))
    UNION ALL
    SELECT 'switch:' || job.generation::text
    FROM engine_switch_jobs job
    WHERE job.generation = $3
      AND (job.status IN ('pending', 'processing', 'retry')
        OR (job.status = 'dead' AND NOT EXISTS(
          SELECT 1 FROM engine_switch_jobs newer
          WHERE newer.status = 'confirmed' AND newer.generation > job.generation)))
  `, [main, openkeys, Number(switches)]);
  if (pending.rows.length > 0) return;
  const delivered = await database.pool.query<{ missing: string }>(`
    SELECT 'catalog:' || product_id || ':' || generation::text AS missing
    FROM engine_catalog_jobs
    WHERE (product_id, generation) IN (('main', $1), ('openkeys', $2)) AND status = 'confirmed'
  `, [main, openkeys]);
  const confirmedSwitches = await database.pool.query<{ ok: boolean }>(`
    SELECT EXISTS(
      SELECT 1 FROM engine_switch_jobs
      WHERE generation = $1 AND status = 'confirmed'
    ) AS ok
  `, [Number(switches)]);
  if (delivered.rows.length !== 2 || confirmedSwitches.rows[0]?.ok !== true) {
    await kill(
      database,
      row.id,
      "deliver_catalogs",
      "catalog/switch delivery jobs for the new generation are missing or unconfirmed",
    );
    return;
  }
  await transition(database, row.id, "deliver_catalogs", "step = 'normalize_funding', last_error = NULL", []);
}

async function stepNormalizeFunding(
  database: Database,
  row: OrchestrationRow,
): Promise<void> {
  const run = await database.pool.query<{ plan_digest: string }>(`
    SELECT plan_digest FROM pricing_stage5_runs_v2 WHERE run_id = $1
  `, [row.stage5_run_id]);
  const planDigest = run.rows[0]?.plan_digest;
  if (!planDigest) throw permanent("orchestration lost its Stage 5 run");
  let status = await getFundingNormalizationStageStatusV2(database, planDigest);
  if (status.job_id === null) {
    // Stage once, then watch the durable job: re-staging an existing (even dead) job would
    // either conflict with its immutable payload or shadow its verdict behind a fresh row.
    await stageFundingNormalizationJobV2(database, {
      planDigest,
      audit: { actorId: row.operator_id, reason: row.reason },
    });
    status = await getFundingNormalizationStageStatusV2(database, planDigest);
  }
  if (status.job_status === "dead") {
    const message = `funding normalization dead: ${status.job_last_error ?? "no error recorded"}`;
    // An account provisioned mid-cycle invalidates the pair's inventory; a fresh cycle
    // re-materializes with the live inventory, exactly like capture/rollout drift.
    if (/inventory|drift/i.test(message)) {
      await freshCycle(database, row, message);
      return;
    }
    await kill(database, row.id, "normalize_funding", message);
    return;
  }
  if (status.target_status !== "prepared" || status.recovery_status !== "prepared") return;
  await transition(database, row.id, "normalize_funding", "step = 'rollout', last_error = NULL", []);
}

async function stepRollout(
  database: Database,
  readers: PricingReleaseOrchestrationReadersV2,
  row: OrchestrationRow,
): Promise<void> {
  try {
    await stagePricingShadowRolloutV2(database, readers.engine, {
      idempotencyKey: randomUUID(),
      stage5RunId: row.stage5_run_id!,
      actorId: row.operator_id,
      reason: row.reason,
    });
  } catch (error) {
    // A transient engine outage must not move the state machine at all: the next tick retries.
    if (error instanceof EngineClientError && error.retryable) throw error;
    const message = error instanceof Error ? error.message : String(error);
    // A serialization conflict with a concurrent delivery is equally transient.
    if (isSerializationConflictV2(message)) return;
    // The rollout refuses to stage against an inventory that moved after the Stage 5 run — the
    // pair is stale; a fresh cycle re-materializes with the live inventory.
    if (/inventory|drift/i.test(message)) {
      await freshCycle(database, row, `rollout staging refused: ${message}`);
      return;
    }
    await kill(database, row.id, "rollout", `rollout staging rejected: ${message}`);
    return;
  }
  const control = await readPricingShadowRolloutControlV2(database);
  const rollout = control.rollouts.find((entry) => entry.stage5RunId === row.stage5_run_id);
  if (!rollout) return;
  if (rollout.status === "confirmed") {
    await transition(database, row.id, "rollout", "step = 'capture', last_error = NULL", []);
    return;
  }
  if (rollout.status === "blocked" || rollout.status === "failed" || rollout.status === "dead") {
    const message = rollout.lastError ?? "no error recorded";
    if (/inventory|drift/i.test(message)) {
      await freshCycle(database, row, `rollout ${rollout.status}: ${message}`);
      return;
    }
    await kill(database, row.id, "rollout", `rollout ${rollout.status}: ${message}`);
  }
}

async function stepCapture(
  database: Database,
  row: OrchestrationRow,
): Promise<void> {
  const latest = await database.pool.query<{
    status: string;
    result_combined_evidence_digest: string | null;
  }>(`
    SELECT status, result_combined_evidence_digest
    FROM pricing_stage8_capture_jobs_v2
    WHERE target_generation = $1 AND recovery_generation = $2
    ORDER BY created_at DESC
    LIMIT 1
  `, [row.target_generation, row.recovery_generation]);
  const current = latest.rows[0];
  // A capture whose 15-minute window still covers the rollout's own policy writes (or any other
  // recent authority churn) is guaranteed to arrive blocked. Wait until the rollout that fed
  // this pair completed at least one full window ago; registrations landing inside the window
  // remain covered by the bounded transient re-capture below.
  const quietForCapture = async (): Promise<boolean> => {
    const databaseNow = Math.floor(row.database_now.getTime() / 1_000);
    const rollout = await database.pool.query<{ completed_at: Date | null }>(`
      SELECT completed_at FROM pricing_shadow_rollouts_v2 WHERE stage5_run_id = $1
    `, [row.stage5_run_id]);
    const completedAt = rollout.rows[0]?.completed_at;
    if (!completedAt) return false;
    // The capture window ends 30 s before "now", so its start reaches WINDOW+LAG seconds back:
    // the rollout churn must be older than that whole reach, or every staged window still
    // contains the tail writes and arrives blocked (the fifth intent burned all five attempts
    // in 25 seconds exactly this way).
    return completedAt.getTime() / 1_000 + CAPTURE_WINDOW_SECONDS + CAPTURE_WINDOW_LAG_SECONDS
      < databaseNow;
  };
  const stageNewCapture = async () => {
    const databaseNow = Math.floor(row.database_now.getTime() / 1_000);
    const windowEnd = databaseNow - CAPTURE_WINDOW_LAG_SECONDS;
    await stagePricingStage8CaptureJobV2(database, {
      idempotencyKey: randomUUID(),
      request: {
        target_generation: Number(row.target_generation),
        recovery_generation: Number(row.recovery_generation),
        window_start_ts: windowEnd - CAPTURE_WINDOW_SECONDS,
        window_end_ts: windowEnd,
        min_samples_per_provider: CAPTURE_MIN_SAMPLES,
        financial_sample_size: CAPTURE_FINANCIAL_SAMPLE,
        // The client-edge Gemini admission aggregate is an audit-only field; the orchestrator
        // has no independent edge source, so automated captures record 0 explicitly.
        gemini_client_admissions: 0,
      },
      operatorId: row.operator_id,
      reason: row.reason,
    });
  };
  if (!current) {
    if (await quietForCapture()) await stageNewCapture();
    return;
  }
  if (current.status === "pending" || current.status === "processing"
      || current.status === "retry") {
    return;
  }
  if (current.status === "passed" && current.result_combined_evidence_digest) {
    if (row.evidence_digest === current.result_combined_evidence_digest) {
      // We already consumed this evidence and it expired before activation: capture fresh.
      await stageNewCapture();
      return;
    }
    await transition(
      database,
      row.id,
      "capture",
      "step = 'activate', evidence_digest = $3, last_error = NULL",
      [current.result_combined_evidence_digest],
    );
    return;
  }
  if (current.status === "blocked") {
    const artifact = await database.pool.query<{ combined_payload_json: unknown }>(`
      SELECT artifact.combined_payload_json
      FROM pricing_stage8_capture_artifacts_v2 artifact
      JOIN pricing_stage8_capture_jobs_v2 job ON job.id = artifact.job_id
      WHERE job.target_generation = $1 AND job.recovery_generation = $2
      ORDER BY artifact.created_at DESC
      LIMIT 1
    `, [row.target_generation, row.recovery_generation]);
    const rawPayload = artifact.rows[0]?.combined_payload_json as unknown;
    const payload = (typeof rawPayload === "string" ? JSON.parse(rawPayload) : rawPayload) as
      | { blockers?: Array<{ code?: string }> }
      | undefined;
    const codes = (payload?.blockers ?? [])
      .map((blocker) => blocker.code ?? "unknown")
      .sort();
    if (codes.length > 0 && codes.every((code) => DRIFT_BLOCKER_CODES.has(code))) {
      await freshCycle(database, row, `capture blocked by drift: ${codes.join(",")}`);
      return;
    }
    if (codes.length > 0 && codes.every((code) => TRANSIENT_CAPTURE_BLOCKERS.has(code))) {
      const attempts = await database.pool.query<{ count: string }>(`
        SELECT count(*)::text FROM pricing_stage8_capture_jobs_v2
        WHERE target_generation = $1 AND recovery_generation = $2
      `, [row.target_generation, row.recovery_generation]);
      if (Number(attempts.rows[0]!.count) >= MAX_CAPTURES_PER_PAIR) {
        await kill(
          database,
          row.id,
          "capture",
          `capture never found a quiet window in ${MAX_CAPTURES_PER_PAIR} attempts: ${codes.join(",")}`,
        );
        return;
      }
      // The pair is intact; only the window/plane was busy. Wait for a quiet window first —
      // the same blocked job is re-evaluated each tick until the fresh capture can start clean.
      if (!await quietForCapture()) return;
      await stageNewCapture();
      await transition(
        database,
        row.id,
        "capture",
        "evidence_digest = NULL, last_error = $3",
        [`re-capture after transient blockers: ${codes.join(",")}`],
      );
      return;
    }
    await kill(database, row.id, "capture", `capture blocked: ${codes.join(",") || "unknown"}`);
    return;
  }
  await kill(database, row.id, "capture", `capture job ended ${current.status}`);
}

async function stepActivate(
  database: Database,
  readers: PricingReleaseOrchestrationReadersV2,
  row: OrchestrationRow,
): Promise<void> {
  // Poll BEFORE any staging: once the CAS commits, the live head IS the target, so re-deriving
  // the kind would read 'recovery' and the re-stage would throw (fresh-evidence guard) or fail
  // validation — while the durable job already holds the terminal verdict.
  const job = await database.pool.query<{ id: string; status: string; last_error: string | null }>(`
    SELECT id::text, status, last_error
    FROM pricing_release_control_jobs_v2
    WHERE stage8_evidence_digest = $1
      AND job_kind IN ('activate_release', 'activate_recovery', 'activate_successor')
    ORDER BY created_at DESC
    LIMIT 1
  `, [row.evidence_digest]);
  const current = job.rows[0];
  if (!current) {
    if (row.activation_kind !== null) throw permanent("activation job lost after staging");
    const head = await readers.engine.getPricingReleaseHeadV2();
    const kind = head === null
      ? "cutover"
      : head.active_generation === Number(row.target_generation) ? "recovery" : "successor";
    try {
      await stagePricingReleaseActivationJobV2(database, {
        activationKind: kind,
        evidenceDigest: row.evidence_digest!,
        operatorId: row.operator_id,
        reason: row.reason,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (/expired/i.test(message)) {
        await transition(
          database,
          row.id,
          "activate",
          "step = 'capture', activation_kind = NULL, last_error = $3",
          [`evidence expired before activation staging: ${message}`],
        );
        return;
      }
      throw error;
    }
    await transition(database, row.id, "activate", "activation_kind = $3", [kind]);
    return;
  }
  if (current.status === "pending" || current.status === "processing"
      || current.status === "retry") {
    return;
  }
  if (current.status === "confirmed") {
    await transition(database, row.id, "activate", "step = 'verify', last_error = NULL", []);
    return;
  }
  // A dead activation job may be a committed CAS whose ACK the worker could not store. The
  // reconcile lane only succeeds when the engine attests the exact activation; a genuinely
  // rejected activation fails closed here.
  const reconciled = await reconcileLostPricingActivationReceiptV2(
    database,
    { engine: readers.engine },
    { jobId: current.id, actorId: row.operator_id, reason: row.reason },
  ).catch((error: unknown) => error);
  if (reconciled instanceof Error) {
    await kill(database, row.id, "activate", current.last_error ?? "activation job dead");
    return;
  }
  await transition(database, row.id, "activate", "step = 'verify', last_error = NULL", []);
}

async function stepVerify(
  database: Database,
  readers: PricingReleaseOrchestrationReadersV2,
  row: OrchestrationRow,
): Promise<void> {
  const head = await readers.engine.getPricingReleaseHeadV2();
  if (head?.active_generation !== Number(row.target_generation)) {
    await kill(
      database,
      row.id,
      "verify",
      `engine head is ${head?.active_generation ?? "absent"}, not the orchestrated ${row.target_generation}`,
    );
    return;
  }
  const resultDigest = stage5V2Digest("pricing-release-orchestration-result", {
    orchestration_id: row.id,
    capability_generation: row.capability_generation,
    target_generation: row.target_generation,
    recovery_generation: row.recovery_generation,
    evidence_digest: row.evidence_digest,
    activation_kind: row.activation_kind,
  });
  await transition(
    database,
    row.id,
    "verify",
    "status = 'confirmed', result_digest = $3, confirmed_at = now(), last_error = NULL",
    [resultDigest],
  );
}

/**
 * Advances the single active orchestration by at most one step. Sub-jobs execute in their own
 * worker lanes; this only stages them and reads their durable states. Returns the active row id
 * when an orchestration exists, null otherwise.
 */
export async function advancePricingReleaseOrchestrationV2(
  database: Database,
  readers: PricingReleaseOrchestrationReadersV2,
): Promise<string | null> {
  const row = await loadActiveOrchestration(database);
  if (!row) return null;
  switch (row.step) {
    case "materialize_pair":
      await stepMaterializePair(database, readers, row);
      break;
    case "deliver_catalogs":
      await stepDeliverCatalogs(database, row);
      break;
    case "normalize_funding":
      await stepNormalizeFunding(database, row);
      break;
    case "rollout":
      await stepRollout(database, readers, row);
      break;
    case "capture":
      await stepCapture(database, row);
      break;
    case "activate":
      await stepActivate(database, readers, row);
      break;
    case "verify":
      await stepVerify(database, readers, row);
      break;
  }
  return row.id;
}
