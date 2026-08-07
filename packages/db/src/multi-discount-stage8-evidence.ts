import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import JSONbigFactory from "json-bigint";
import { pricingReleaseHeadV2Schema } from "@claude-api/contracts";
import type { PoolClient } from "pg";
import { z } from "zod";
import type { Database } from "./client.js";
import {
  scanStage5OpenKeysInventoryV2,
  stage5V2CommerceInventoryDigest,
  stage5V2Digest,
} from "./pricing-stage5-materializer-v2.js";
import { readStage5V2CommerceAndServiceSnapshot } from "./pricing-stage5-materializer-v2-store.js";
import {
  capturePricingReleaseActivationAuthorityV2,
  type PricingReleaseActivationAuthorityReadersV2,
} from "./pricing-release-activation-authority.js";

const JSONbig = JSONbigFactory({ alwaysParseAsBig: true, useNativeBigInt: true });
const ENGINE_EVIDENCE_DOMAIN = Buffer.from(
  "claude-api/multi-discount-stage8/engine-evidence/v2\0",
  "utf8",
);
const STAGE8_COMBINED_SCHEMA_VERSION = 2;
const STAGE8_ENGINE_SCHEMA_VERSION = 2n;
const STAGE8_ENGINE_MAX_AGE_SECONDS = 120n;
const STAGE8_CLOCK_SKEW_SECONDS = 5n;
const STAGE8_EVIDENCE_TTL_SECONDS = 300;
const SHA256_V2_PATTERN = /^sha256:v2:[0-9a-f]{64}$/;

const sha256V1Schema = z.string().regex(/^sha256:v1:[0-9a-f]{64}$/);
const sha256V2Schema = z.string().regex(SHA256_V2_PATTERN);
const nonEmptyStringSchema = z.string().min(1);
const nonNegativeI64Schema = z.bigint().min(0n).max(9_223_372_036_854_775_807n);
const positiveI64Schema = nonNegativeI64Schema.refine((value) => value > 0n);
const basisPointsSchema = nonNegativeI64Schema.refine((value) => value <= 10_000n);

const engineBlockerSchema = z.object({
  code: nonEmptyStringSchema,
  count: positiveI64Schema,
  subject_digests: z.array(sha256V1Schema).max(20),
}).strict();

const engineCatalogSchema = z.object({
  product_id: nonEmptyStringSchema,
  generation: positiveI64Schema,
  schema_version: positiveI64Schema,
  capability_generation: positiveI64Schema,
  capability_digest: nonEmptyStringSchema,
  content_digest: nonEmptyStringSchema,
  enabled_entries: nonNegativeI64Schema,
}).strict();

const engineSwitchSchema = z.object({
  generation: positiveI64Schema,
  schema_version: positiveI64Schema,
  capability_generation: positiveI64Schema,
  capability_digest: nonEmptyStringSchema,
  content_digest: nonEmptyStringSchema,
  entries: nonNegativeI64Schema,
}).strict();

const engineRuntimeCapabilitySchema = z.object({
  schema_version: positiveI64Schema,
  generation: positiveI64Schema,
  digest: nonEmptyStringSchema,
}).strict();

const engineRuntimeManifestSchema = z.object({
  generation: positiveI64Schema,
  digest: nonEmptyStringSchema,
  capabilities: z.array(engineRuntimeCapabilitySchema).min(1),
}).strict();

const engineReleaseHeadSchema = z.object({
  active_generation: positiveI64Schema,
  active_digest: nonEmptyStringSchema,
  head_version: positiveI64Schema,
  updated_ts: positiveI64Schema,
}).strict();

const engineReleasePairSchema = z.object({
  target_generation: positiveI64Schema,
  target_digest: sha256V2Schema.nullable(),
  recovery_generation: positiveI64Schema,
  recovery_digest: sha256V2Schema.nullable(),
  recovery_link_digest: sha256V2Schema.nullable(),
  inventory_digest: sha256V2Schema.nullable(),
  funding_digest: sha256V2Schema.nullable(),
  target_assignment_count: nonNegativeI64Schema,
  recovery_assignment_count: nonNegativeI64Schema,
  active_head: engineReleaseHeadSchema.nullable(),
}).strict();

const engineFinancialSampleSchema = z.object({
  subject_digest: sha256V1Schema,
  evaluation_digest: nonEmptyStringSchema,
  provider_id: z.enum(["anthropic", "openai", "google"]),
  account_class: z.enum(["b2c", "b2b", "openkeys", "service"]),
  authorized_multiplier_bp: basisPointsSchema,
  payable_multiplier_bp: basisPointsSchema,
  official_hold_nano: nonNegativeI64Schema,
  legacy_hold_nano: nonNegativeI64Schema,
  policy_hold_nano: nonNegativeI64Schema,
  comparison_result: z.enum(["equal", "different"]),
}).strict();

const countRecordSchema = z.record(z.string().min(1), nonNegativeI64Schema);
const engineCountsSchema = z.object({
  total_accounts: nonNegativeI64Schema,
  active_accounts: nonNegativeI64Schema,
  account_classes: countRecordSchema,
  reconciled_accounts: nonNegativeI64Schema,
  snapshots_by_provider: countRecordSchema,
  evaluations_by_outcome: countRecordSchema,
  comparisons: countRecordSchema,
  scalar_parity_rows: nonNegativeI64Schema,
  policy_divergence_rows: nonNegativeI64Schema,
  gemini_usage_rows: nonNegativeI64Schema,
  gemini_outbox_rows: nonNegativeI64Schema,
  live_runtime_instances: nonNegativeI64Schema,
  release_capable_runtime_instances: nonNegativeI64Schema,
  legacy_inflight_reservations: nonNegativeI64Schema,
  legacy_inflight_outbox_rows: nonNegativeI64Schema,
}).strict();

const stage8EngineEvidenceV2Schema = z.object({
  schema_version: z.literal(STAGE8_ENGINE_SCHEMA_VERSION),
  captured_ts: positiveI64Schema,
  window_start_ts: positiveI64Schema,
  window_end_ts: positiveI64Schema,
  min_samples_per_provider: positiveI64Schema,
  gemini_client_admissions: nonNegativeI64Schema,
  passed: z.boolean(),
  release: engineReleasePairSchema,
  runtime_manifest: engineRuntimeManifestSchema,
  catalogs: z.array(engineCatalogSchema),
  switches: engineSwitchSchema.nullable(),
  counts: engineCountsSchema,
  financial_samples: z.array(engineFinancialSampleSchema).max(1_000),
  engine_inventory_digest: sha256V2Schema,
  funding_digest: sha256V2Schema,
  shadow_digest: sha256V2Schema,
  runtime_floor_digest: sha256V2Schema,
  legacy_inflight_count: nonNegativeI64Schema,
  blockers: z.array(engineBlockerSchema),
  evidence_digest: sha256V2Schema,
}).strict().superRefine((report, context) => {
  if (report.window_end_ts <= report.window_start_ts || report.window_end_ts > report.captured_ts) {
    context.addIssue({ code: z.ZodIssueCode.custom, message: "invalid Stage 8 evidence window" });
  }
  if (report.release.recovery_generation <= report.release.target_generation) {
    context.addIssue({ code: z.ZodIssueCode.custom, message: "invalid target/recovery order" });
  }
  const legacyCount = report.counts.legacy_inflight_reservations
    + report.counts.legacy_inflight_outbox_rows;
  if (legacyCount !== report.legacy_inflight_count) {
    context.addIssue({ code: z.ZodIssueCode.custom, message: "legacy inflight count mismatch" });
  }
  if (report.passed !== (report.blockers.length === 0)) {
    context.addIssue({ code: z.ZodIssueCode.custom, message: "passed/blocker state mismatch" });
  }
});

export type Stage8EngineEvidenceV2 = z.infer<typeof stage8EngineEvidenceV2Schema>;

export interface Stage8CombinedBlocker {
  source: "commerce" | "engine";
  code: string;
  count: string;
  subject_digests: string[];
}

export interface Stage8CombinedEvidenceV2 {
  schema_version: 2;
  observed_at: string;
  valid_until: string;
  passed: boolean;
  write_result: "stored" | "unchanged" | "not_persisted";
  source: {
    engine_evidence_digest: string;
    engine_captured_ts: string;
    engine_window_start_ts: string;
    engine_window_end_ts: string;
  };
  releases: {
    target: { generation: string; commerce_digest: string | null; engine_digest: string | null };
    recovery: { generation: string; commerce_digest: string | null; engine_digest: string | null };
  };
  inventories: {
    commerce_digest: string;
    engine_digest: string;
    openkeys_digest: string;
    service_digest: string;
  };
  sales_contract_digest: string;
  funding_digest: string;
  shadow_digest: string;
  runtime_floor_digest: string;
  legacy_inflight_count: string;
  blocker_count: string;
  blockers: Stage8CombinedBlocker[];
  evidence_digest: string;
}

interface ReleaseRow {
  generation: string;
  release_kind: "target" | "recovery";
  status: string;
  schema_version: string;
  commerce_inventory_digest: string;
  engine_inventory_digest: string;
  openkeys_inventory_digest: string;
  service_inventory_digest: string;
  policy_manifest_digest: string;
  assignment_manifest_digest: string;
  funding_manifest_digest: string | null;
  engine_release_digest: string | null;
  content_digest: string;
  assignment_count: string;
}

interface ReleaseAssignmentLineageRow {
  release_generation: string;
  engine_account_id: string;
  account_class: string;
  owner_context: string;
  owner_id: string;
  policy_id: string;
  policy_version: string;
  policy_digest: string;
  billing_mode: string;
  funding_generation: string | null;
  purpose: string | null;
  responsible: string | null;
}

interface StoredEvidenceRow {
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
  sales_contract_digest: string;
  funding_digest: string;
  shadow_digest: string;
  runtime_floor_digest: string;
  legacy_inflight_count: string;
  blocker_count: string;
  passed: boolean;
  observed_at: Date;
  valid_until: Date;
}

const SALES_CONTRACT_IDENTITY = {
  schema_version: 2,
  eligible_account_class: "b2c",
  commission_basis: "paid_funded_nano",
  pricing_mode_affects_eligibility: false,
  welcome_bonus_eligible: false,
} as const;

export const STAGE8_SALES_CONTRACT_DIGEST = stage5V2Digest(
  "stage8-sales-contract",
  SALES_CONTRACT_IDENTITY,
);

export class Stage8EvidenceV2Error extends Error {
  constructor(
    public readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "Stage8EvidenceV2Error";
  }
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function orderedCountRecord(record: Record<string, bigint>): Record<string, bigint> {
  return Object.fromEntries(
    Object.entries(record).sort(([left], [right]) => compareUtf8(left, right)),
  );
}

function orderedEngineDigestValue(report: Stage8EngineEvidenceV2): Stage8EngineEvidenceV2 {
  return {
    ...report,
    counts: {
      ...report.counts,
      account_classes: orderedCountRecord(report.counts.account_classes),
      snapshots_by_provider: orderedCountRecord(report.counts.snapshots_by_provider),
      evaluations_by_outcome: orderedCountRecord(report.counts.evaluations_by_outcome),
      comparisons: orderedCountRecord(report.counts.comparisons),
    },
    evidence_digest: "",
  };
}

export function stage8EngineEvidenceDigestV2(report: Stage8EngineEvidenceV2): string {
  const encoded = Buffer.from(JSONbig.stringify(orderedEngineDigestValue(report)), "utf8");
  const length = Buffer.alloc(8);
  length.writeBigUInt64BE(BigInt(encoded.length));
  const hex = createHash("sha256")
    .update(ENGINE_EVIDENCE_DOMAIN)
    .update(length)
    .update(encoded)
    .digest("hex");
  return `sha256:v2:${hex}`;
}

function validateEngineEvidence(value: unknown): Stage8EngineEvidenceV2 {
  const report = stage8EngineEvidenceV2Schema.parse(value);
  if (stage8EngineEvidenceDigestV2(report) !== report.evidence_digest) {
    throw new Stage8EvidenceV2Error(
      "engine_evidence_digest_mismatch",
      "Stage 8 engine evidence does not match its canonical v2 digest",
    );
  }
  return report;
}

export function parseStage8EngineEvidenceV2(raw: string): Stage8EngineEvidenceV2 {
  let value: unknown;
  try {
    value = JSONbig.parse(raw);
  } catch (error) {
    throw new Stage8EvidenceV2Error(
      "engine_evidence_json_invalid",
      `Stage 8 engine evidence is not valid integer-preserving JSON: ${error instanceof Error ? error.message : "unknown error"}`,
    );
  }
  try {
    return validateEngineEvidence(value);
  } catch (error) {
    if (error instanceof Stage8EvidenceV2Error) throw error;
    throw new Stage8EvidenceV2Error(
      "engine_evidence_shape_invalid",
      `Stage 8 engine evidence has an invalid schema-v2 shape: ${error instanceof Error ? error.message : "unknown error"}`,
    );
  }
}

function localBlocker(code: string, subjects: readonly string[]): Stage8CombinedBlocker | null {
  const unique = [...new Set(subjects)].sort(compareUtf8);
  if (unique.length === 0) return null;
  return {
    source: "commerce",
    code,
    count: String(unique.length),
    subject_digests: unique.slice(0, 20).map((subject) =>
      stage5V2Digest("stage8-commerce-subject", subject)),
  };
}

function pushLocalBlocker(
  blockers: Stage8CombinedBlocker[],
  code: string,
  subjects: readonly string[],
): void {
  const candidate = localBlocker(code, subjects);
  if (candidate) blockers.push(candidate);
}

function requireReleaseDigest(value: string | null, label: string): string | null {
  if (value === null) return null;
  if (!SHA256_V2_PATTERN.test(value)) {
    throw new Stage8EvidenceV2Error("release_digest_invalid", `${label} is not a canonical sha256:v2 digest`);
  }
  return value;
}

function sameReleaseLineage(left: ReleaseRow, right: ReleaseRow): boolean {
  return left.schema_version === right.schema_version
    && left.commerce_inventory_digest === right.commerce_inventory_digest
    && left.engine_inventory_digest === right.engine_inventory_digest
    && left.openkeys_inventory_digest === right.openkeys_inventory_digest
    && left.service_inventory_digest === right.service_inventory_digest
    && left.policy_manifest_digest === right.policy_manifest_digest
    && left.funding_manifest_digest === right.funding_manifest_digest
    && left.assignment_count === right.assignment_count;
}

function sameAssignmentLineage(
  rows: readonly ReleaseAssignmentLineageRow[],
  targetGeneration: bigint,
  recoveryGeneration: bigint,
): boolean {
  const target = targetGeneration.toString();
  const recovery = recoveryGeneration.toString();
  const normalized = (generation: string): Array<Omit<ReleaseAssignmentLineageRow, "release_generation">> =>
    rows
      .filter((row) => row.release_generation === generation)
      .map(({ release_generation: _generation, ...row }) => row);
  return JSON.stringify(normalized(target)) === JSON.stringify(normalized(recovery));
}

async function pendingPricingJobs(client: PoolClient): Promise<string[]> {
  const rows = await client.query<{ subject: string }>(`
    SELECT concat(kind, ':', id::text) AS subject
    FROM (
      SELECT 'catalog' AS kind, job.id, job.status FROM engine_catalog_jobs job
      WHERE job.status IN ('pending', 'processing', 'retry')
         OR (job.status = 'dead' AND NOT EXISTS(
           SELECT 1 FROM engine_catalog_jobs newer
           WHERE newer.product_id = job.product_id AND newer.status = 'confirmed'
             AND newer.generation > job.generation))
      UNION ALL SELECT 'switch', job.id, job.status FROM engine_switch_jobs job
      WHERE job.status IN ('pending', 'processing', 'retry')
         OR (job.status = 'dead' AND NOT EXISTS(
           SELECT 1 FROM engine_switch_jobs newer
           WHERE newer.status = 'confirmed' AND newer.generation > job.generation))
      UNION ALL SELECT 'policy', job.id, job.status FROM engine_policy_jobs job
      WHERE job.status IN ('pending', 'processing', 'retry')
         OR (job.status = 'dead' AND NOT EXISTS(
           SELECT 1 FROM engine_policy_jobs newer
           WHERE newer.binding_id = job.binding_id AND newer.status = 'confirmed'
             AND newer.effective_version > job.effective_version))
      UNION ALL SELECT 'release', job.id, job.status FROM pricing_release_control_jobs_v2 job
      WHERE job.status IN ('pending', 'processing', 'retry')
         OR (job.status = 'dead' AND NOT EXISTS(
           SELECT 1 FROM pricing_release_control_jobs_v2 newer
           WHERE newer.job_kind = job.job_kind AND newer.status = 'confirmed'
             AND newer.release_generation > job.release_generation))
    ) job
    ORDER BY concat(kind, ':', id::text) COLLATE "C"
  `);
  return rows.rows.map((row) => row.subject);
}

function storedIdentity(row: StoredEvidenceRow): Record<string, unknown> {
  return {
    evidence_digest: row.evidence_digest,
    engine_evidence_digest: row.engine_evidence_digest,
    engine_captured_at: row.engine_captured_at?.toISOString() ?? null,
    target_generation: row.target_generation,
    target_digest: row.target_digest,
    recovery_generation: row.recovery_generation,
    recovery_digest: row.recovery_digest,
    commerce_inventory_digest: row.commerce_inventory_digest,
    engine_inventory_digest: row.engine_inventory_digest,
    openkeys_inventory_digest: row.openkeys_inventory_digest,
    service_inventory_digest: row.service_inventory_digest,
    sales_contract_digest: row.sales_contract_digest,
    funding_digest: row.funding_digest,
    shadow_digest: row.shadow_digest,
    runtime_floor_digest: row.runtime_floor_digest,
    legacy_inflight_count: row.legacy_inflight_count,
    blocker_count: row.blocker_count,
    passed: row.passed,
    observed_at: row.observed_at.toISOString(),
    valid_until: row.valid_until.toISOString(),
  };
}

export async function collectStage8CombinedEvidenceV2(
  database: Database,
  readers: PricingReleaseActivationAuthorityReadersV2,
  untrustedEngineEvidence: Stage8EngineEvidenceV2,
): Promise<Stage8CombinedEvidenceV2> {
  const engine = validateEngineEvidence(untrustedEngineEvidence);
  const openkeysFirst = await scanStage5OpenKeysInventoryV2(readers.openkeys);
  const openkeysSecond = await scanStage5OpenKeysInventoryV2(readers.openkeys);
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query("SET LOCAL statement_timeout = '30s'");
    await client.query("SET LOCAL lock_timeout = '5s'");
    await client.query(
      "SELECT pg_advisory_xact_lock(hashtextextended('pricing-release-v2:control-plane', 0))",
    );
    const observed = await client.query<{ observed_at: Date }>(
      "SELECT transaction_timestamp() AS observed_at",
    );
    const observedAt = observed.rows[0]!.observed_at;
    const validUntil = new Date(observedAt.getTime() + STAGE8_EVIDENCE_TTL_SECONDS * 1_000);
    const current = await readStage5V2CommerceAndServiceSnapshot(client);
    const commerceDigest = stage5V2CommerceInventoryDigest(current.commerce);
    const serviceDigest = current.service.inventory_digest;
    const releases = await client.query<ReleaseRow>(`
      SELECT plan.generation::text, plan.release_kind, plan.status, plan.schema_version::text,
             plan.commerce_inventory_digest, plan.engine_inventory_digest,
             plan.openkeys_inventory_digest, plan.service_inventory_digest,
             plan.policy_manifest_digest, plan.assignment_manifest_digest,
             plan.funding_manifest_digest, plan.engine_release_digest, plan.content_digest,
             (SELECT count(*)::text FROM pricing_release_assignments_v2 assignment
               WHERE assignment.release_generation = plan.generation) AS assignment_count
      FROM pricing_release_plans_v2 plan
      WHERE plan.generation IN ($1, $2)
      ORDER BY plan.generation
    `, [engine.release.target_generation, engine.release.recovery_generation]);
    const assignmentLineage = await client.query<ReleaseAssignmentLineageRow>(`
      SELECT release_generation::text, engine_account_id, account_class, owner_context,
             owner_id, policy_id, policy_version::text, policy_digest, billing_mode,
             funding_generation::text, purpose, responsible
      FROM pricing_release_assignments_v2
      WHERE release_generation IN ($1, $2)
      ORDER BY release_generation, engine_account_id COLLATE "C"
    `, [engine.release.target_generation, engine.release.recovery_generation]);
    const target = releases.rows.find((row) => row.generation === engine.release.target_generation.toString()) ?? null;
    const recovery = releases.rows.find((row) => row.generation === engine.release.recovery_generation.toString()) ?? null;
    const blockers: Stage8CombinedBlocker[] = engine.blockers.map((item) => ({
      source: "engine",
      code: item.code,
      count: item.count.toString(),
      subject_digests: item.subject_digests,
    }));

    const observedTs = BigInt(Math.floor(observedAt.getTime() / 1_000));
    if (engine.captured_ts > observedTs + STAGE8_CLOCK_SKEW_SECONDS) {
      pushLocalBlocker(blockers, "engine_evidence_captured_in_future", [engine.evidence_digest]);
    } else if (observedTs - engine.captured_ts > STAGE8_ENGINE_MAX_AGE_SECONDS) {
      pushLocalBlocker(blockers, "engine_evidence_stale", [engine.evidence_digest]);
    }
    if (openkeysFirst.inventory_digest !== openkeysSecond.inventory_digest) {
      pushLocalBlocker(blockers, "openkeys_inventory_changed_between_scans", [
        openkeysFirst.inventory_digest,
        openkeysSecond.inventory_digest,
      ]);
    }
    pushLocalBlocker(blockers, "target_release_missing", target ? [] : [engine.release.target_generation.toString()]);
    pushLocalBlocker(blockers, "recovery_release_missing", recovery ? [] : [engine.release.recovery_generation.toString()]);

    const activeHead = engine.release.active_head === null ? null : pricingReleaseHeadV2Schema.parse({
      active_generation: Number(engine.release.active_head.active_generation),
      active_digest: engine.release.active_head.active_digest,
      head_version: Number(engine.release.active_head.head_version),
      updated_ts: Number(engine.release.active_head.updated_ts),
    });
    const authority = target && recovery ? await capturePricingReleaseActivationAuthorityV2(
      client,
      readers,
      {
        activationKind: activeHead === null
          ? "cutover"
          : activeHead.active_generation === Number(engine.release.target_generation)
            ? "recovery"
            : "successor",
        targetGeneration: target.generation,
        targetEngineDigest: target.engine_release_digest ?? "",
        recoveryGeneration: recovery.generation,
        recoveryEngineDigest: recovery.engine_release_digest ?? "",
        targetCommerceInventoryDigest: target.commerce_inventory_digest,
        targetEngineInventoryDigest: target.engine_inventory_digest,
        targetOpenkeysInventoryDigest: target.openkeys_inventory_digest,
        targetServiceInventoryDigest: target.service_inventory_digest,
        expectedHead: activeHead,
      },
    ) : null;
    for (const item of authority?.blockers ?? []) {
      const candidate = localBlocker(item.code, item.subjectDigests);
      if (candidate) blockers.push({ ...candidate, count: String(item.count) });
    }
    const currentCommerceDigest = authority?.commerceInventoryDigest ?? commerceDigest;
    const currentEngineDigest = authority?.engineInventoryDigest ?? engine.engine_inventory_digest;
    const currentOpenkeysDigest = authority?.openkeysInventoryDigest ?? openkeysSecond.inventory_digest;
    const currentServiceDigest = authority?.serviceInventoryDigest ?? serviceDigest;
    const postCutover = activeHead !== null;

    if (target) {
      const drift: string[] = [];
      if (target.release_kind !== "target") drift.push("kind");
      if (target.status !== "prepared") drift.push(`status:${target.status}`);
      if (target.schema_version !== "2") drift.push("schema");
      if (!postCutover && target.commerce_inventory_digest !== currentCommerceDigest) drift.push("commerce-inventory");
      if (target.engine_inventory_digest !== currentEngineDigest) drift.push("engine-inventory");
      if (!postCutover && target.openkeys_inventory_digest !== currentOpenkeysDigest) drift.push("openkeys-inventory");
      if (!postCutover && target.service_inventory_digest !== currentServiceDigest) drift.push("service-inventory");
      if (target.funding_manifest_digest !== engine.funding_digest) drift.push("funding");
      if (target.engine_release_digest !== engine.release.target_digest) drift.push("engine-release");
      if (target.assignment_count !== engine.release.target_assignment_count.toString()) drift.push("assignments");
      pushLocalBlocker(blockers, "target_release_identity_drift", drift);
    }
    if (recovery) {
      const drift: string[] = [];
      if (recovery.release_kind !== "recovery") drift.push("kind");
      if (recovery.status !== "prepared") drift.push(`status:${recovery.status}`);
      if (recovery.schema_version !== "2") drift.push("schema");
      if (!postCutover && recovery.commerce_inventory_digest !== currentCommerceDigest) drift.push("commerce-inventory");
      if (recovery.engine_inventory_digest !== currentEngineDigest) drift.push("engine-inventory");
      if (!postCutover && recovery.openkeys_inventory_digest !== currentOpenkeysDigest) drift.push("openkeys-inventory");
      if (!postCutover && recovery.service_inventory_digest !== currentServiceDigest) drift.push("service-inventory");
      if (recovery.funding_manifest_digest !== engine.funding_digest) drift.push("funding");
      if (recovery.engine_release_digest !== engine.release.recovery_digest) drift.push("engine-release");
      if (recovery.assignment_count !== engine.release.recovery_assignment_count.toString()) drift.push("assignments");
      pushLocalBlocker(blockers, "recovery_release_identity_drift", drift);
    }
    if (target && recovery
        && (!sameReleaseLineage(target, recovery)
          || !sameAssignmentLineage(
            assignmentLineage.rows,
            engine.release.target_generation,
            engine.release.recovery_generation,
          ))) {
      pushLocalBlocker(blockers, "target_recovery_commerce_lineage_mismatch", [
        `${target.generation}:${recovery.generation}`,
      ]);
    }
    if (engine.release.inventory_digest !== engine.engine_inventory_digest) {
      pushLocalBlocker(blockers, "engine_release_inventory_digest_mismatch", [engine.evidence_digest]);
    }
    if (engine.release.funding_digest !== engine.funding_digest) {
      pushLocalBlocker(blockers, "engine_release_funding_digest_mismatch", [engine.evidence_digest]);
    }
    pushLocalBlocker(blockers, "pricing_control_job_backlog_or_failure", await pendingPricingJobs(client));

    blockers.sort((left, right) =>
      compareUtf8(left.source, right.source) || compareUtf8(left.code, right.code));
    const passed = engine.passed && blockers.length === 0;
    const targetCommerceDigest = requireReleaseDigest(target?.content_digest ?? null, "target commerce digest");
    const recoveryCommerceDigest = requireReleaseDigest(recovery?.content_digest ?? null, "recovery commerce digest");
    const identity = {
      schema_version: STAGE8_COMBINED_SCHEMA_VERSION,
      observed_at: observedAt.toISOString(),
      valid_until: validUntil.toISOString(),
      passed,
      source: {
        engine_evidence_digest: engine.evidence_digest,
        engine_captured_ts: engine.captured_ts.toString(),
        engine_window_start_ts: engine.window_start_ts.toString(),
        engine_window_end_ts: engine.window_end_ts.toString(),
      },
      releases: {
        target: {
          generation: engine.release.target_generation.toString(),
          commerce_digest: targetCommerceDigest,
          engine_digest: engine.release.target_digest,
        },
        recovery: {
          generation: engine.release.recovery_generation.toString(),
          commerce_digest: recoveryCommerceDigest,
          engine_digest: engine.release.recovery_digest,
        },
      },
      inventories: {
        commerce_digest: currentCommerceDigest,
        engine_digest: currentEngineDigest,
        openkeys_digest: currentOpenkeysDigest,
        service_digest: currentServiceDigest,
      },
      sales_contract_digest: STAGE8_SALES_CONTRACT_DIGEST,
      funding_digest: engine.funding_digest,
      shadow_digest: engine.shadow_digest,
      runtime_floor_digest: engine.runtime_floor_digest,
      legacy_inflight_count: engine.legacy_inflight_count.toString(),
      blocker_count: String(blockers.length),
      blockers,
    };
    const evidenceDigest = stage5V2Digest("stage8-combined-evidence", identity);
    let writeResult: Stage8CombinedEvidenceV2["write_result"] = "not_persisted";
    if (targetCommerceDigest !== null && recoveryCommerceDigest !== null) {
      const inserted = await client.query<{ evidence_digest: string }>(`
        INSERT INTO pricing_stage8_evidence_v2 (
          evidence_digest, engine_evidence_digest, engine_captured_at,
          target_generation, target_digest,
          recovery_generation, recovery_digest,
          commerce_inventory_digest, engine_inventory_digest, openkeys_inventory_digest,
          service_inventory_digest,
          sales_contract_digest, funding_digest, shadow_digest, runtime_floor_digest,
          legacy_inflight_count, blocker_count, passed, observed_at, valid_until
        ) VALUES (
          $1, $2, to_timestamp($3::bigint), $4, $5, $6, $7, $8, $9, $10,
          $11, $12, $13, $14, $15, $16, $17, $18, $19, $20
        )
        ON CONFLICT (evidence_digest) DO NOTHING
        RETURNING evidence_digest
      `, [
        evidenceDigest,
        engine.evidence_digest,
        engine.captured_ts.toString(),
        engine.release.target_generation,
        targetCommerceDigest,
        engine.release.recovery_generation,
        recoveryCommerceDigest,
        currentCommerceDigest,
        currentEngineDigest,
        currentOpenkeysDigest,
        currentServiceDigest,
        STAGE8_SALES_CONTRACT_DIGEST,
        engine.funding_digest,
        engine.shadow_digest,
        engine.runtime_floor_digest,
        engine.legacy_inflight_count,
        blockers.length,
        passed,
        observedAt,
        validUntil,
      ]);
      const stored = await client.query<StoredEvidenceRow>(`
        SELECT evidence_digest, engine_evidence_digest, engine_captured_at,
               target_generation::text, target_digest,
               recovery_generation::text, recovery_digest,
               commerce_inventory_digest, engine_inventory_digest, openkeys_inventory_digest,
               service_inventory_digest,
               sales_contract_digest, funding_digest, shadow_digest, runtime_floor_digest,
               legacy_inflight_count::text, blocker_count::text, passed, observed_at, valid_until
        FROM pricing_stage8_evidence_v2 WHERE evidence_digest = $1
      `, [evidenceDigest]);
      const expectedStored = {
        evidence_digest: evidenceDigest,
        engine_evidence_digest: engine.evidence_digest,
        engine_captured_at: new Date(Number(engine.captured_ts) * 1_000).toISOString(),
        target_generation: engine.release.target_generation.toString(),
        target_digest: targetCommerceDigest,
        recovery_generation: engine.release.recovery_generation.toString(),
        recovery_digest: recoveryCommerceDigest,
        commerce_inventory_digest: currentCommerceDigest,
        engine_inventory_digest: currentEngineDigest,
        openkeys_inventory_digest: currentOpenkeysDigest,
        service_inventory_digest: currentServiceDigest,
        sales_contract_digest: STAGE8_SALES_CONTRACT_DIGEST,
        funding_digest: engine.funding_digest,
        shadow_digest: engine.shadow_digest,
        runtime_floor_digest: engine.runtime_floor_digest,
        legacy_inflight_count: engine.legacy_inflight_count.toString(),
        blocker_count: String(blockers.length),
        passed,
        observed_at: observedAt.toISOString(),
        valid_until: validUntil.toISOString(),
      };
      if (!stored.rows[0]
          || JSON.stringify(storedIdentity(stored.rows[0])) !== JSON.stringify(expectedStored)) {
        throw new Stage8EvidenceV2Error(
          "combined_evidence_digest_collision",
          "stored Stage 8 evidence differs for the same combined digest",
        );
      }
      writeResult = inserted.rows.length === 1 ? "stored" : "unchanged";
    }
    await client.query("COMMIT");
    return {
      ...identity,
      schema_version: STAGE8_COMBINED_SCHEMA_VERSION as 2,
      write_result: writeResult,
      evidence_digest: evidenceDigest,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
