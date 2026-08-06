import { Buffer } from "node:buffer";
import {
  pricingReleaseActivationOperatorV2Schema,
  pricingStage5RunQueryV2Schema,
  pricingStage5RunV2Schema,
  pricingStageControlMutationReasonV2Schema,
  type PricingCatalogSpec,
  type PricingStage5RunV2,
  type PricingReleasePolicyV2,
  type ProviderSwitchSpec,
  type ServiceAccountInventoryEntryV2,
} from "@claude-api/contracts";
import { EngineClient } from "@claude-api/engine-client";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";
import {
  Stage5MaterializerV2Error,
  buildStage5ServiceInventoryV2,
  buildStage5V2Plan,
  scanStage5EngineInventoryV2,
  scanStage5OpenKeysInventoryV2,
  stage5V2CanonicalJson,
  stage5V2CommerceInventoryDigest,
  stage5V2Digest,
  type Stage5V2B2bPolicyHeadRule,
  type Stage5V2Blocker,
  type Stage5V2CommerceSnapshot,
  type Stage5V2ExistingReleasePolicy,
  type Stage5V2OpenKeysReader,
  type Stage5V2Plan,
  type Stage5V2PlannedAssignment,
  type Stage5V2ReleasePlan,
} from "./pricing-stage5-materializer-v2.js";

export type Stage5MaterializerV2Mode = "dry_run" | "apply";

export interface Stage5MaterializerV2Result {
  mode: Stage5MaterializerV2Mode;
  plan: Stage5V2Plan;
  run_id: string | null;
  writes_committed: boolean;
  engine_prepared: boolean;
  status: "dry_run" | "blocked" | "planned" | "materializing";
}

export interface Stage5MaterializerV2Audit {
  actorId: string;
  reason: string;
}

interface StoredServiceRow {
  service_id: string;
  engine_account_id: string;
  purpose: string;
  responsible: string;
  status: "active" | "disabled";
  source_version: string;
  content_digest: string;
}

interface StoredRunRow {
  run_id: string;
  schema_version: string;
  plan_digest: string;
  commerce_inventory_digest: string;
  engine_scan_first_digest: string;
  engine_scan_second_digest: string;
  openkeys_scan_first_digest: string;
  openkeys_scan_second_digest: string;
  service_inventory_digest: string;
  funding_plan_digest: string;
  target_generation: string;
  target_digest: string | null;
  recovery_generation: string;
  recovery_digest: string | null;
  inventory_artifact: Record<string, unknown>;
  plan_artifact: Record<string, unknown>;
  blocker_count: string;
  status: "blocked" | "planned" | "materializing" | "prepared" | "failed";
}

interface PrepareAck {
  artifact_kind: "main_catalog" | "openkeys_catalog" | "switches" | "policy";
  artifact_id: string;
  artifact_version: number;
  expected_digest: string;
  mutation_result: "stored" | "unchanged";
  readback_digest: string;
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function positiveSafeNumber(value: string, label: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0 || String(parsed) !== value) {
    throw new Stage5MaterializerV2Error(
      "stored_version_invalid",
      `${label} is not a positive safe integer`,
    );
  }
  return parsed;
}

function sameCanonical(left: unknown, right: unknown): boolean {
  return stage5V2CanonicalJson(left) === stage5V2CanonicalJson(right);
}

function planArtifact(plan: Stage5V2Plan): Record<string, unknown> {
  const { inventory_artifact: _inventory, ...artifact } = plan;
  return artifact;
}

async function readLatestEnginePolicies(
  engine: Pick<EngineClient, "getLatestPricingReleasePolicyV2">,
  policyIds: readonly string[],
): Promise<PricingReleasePolicyV2[]> {
  const policies = await Promise.all(
    [...policyIds]
      .sort(compareUtf8)
      .map((policyId) => engine.getLatestPricingReleasePolicyV2(policyId)),
  );
  return policies.filter((policy): policy is PricingReleasePolicyV2 => policy !== null);
}

function reconcileReleasePolicyLineage(
  local: readonly Stage5V2ExistingReleasePolicy[],
  remoteFirst: readonly PricingReleasePolicyV2[],
  remoteSecond: readonly PricingReleasePolicyV2[],
): Stage5V2ExistingReleasePolicy[] {
  if (!sameCanonical(remoteFirst, remoteSecond)) {
    throw new Stage5MaterializerV2Error(
      "engine_policy_lineage_drift",
      "engine pricing release policy lineage changed during Stage 5 collection",
    );
  }
  const identities = new Map<string, Stage5V2ExistingReleasePolicy>();
  for (const policy of [
    ...local,
    ...remoteSecond.map(({ policy_id, policy_version, content_digest }) => ({
      policy_id,
      policy_version,
      content_digest,
    })),
  ]) {
    const key = `${policy.policy_id}\n${policy.policy_version}`;
    const existing = identities.get(key);
    if (existing && existing.content_digest !== policy.content_digest) {
      throw new Stage5MaterializerV2Error(
        "engine_policy_lineage_conflict",
        `local and engine policy evidence conflicts at ${policy.policy_id} version ${policy.policy_version}`,
      );
    }
    identities.set(key, policy);
  }
  return [...identities.values()].sort((left, right) =>
    compareUtf8(left.policy_id, right.policy_id) || left.policy_version - right.policy_version);
}

export async function readStage5V2CommerceAndServiceSnapshot(
  client: PoolClient,
): Promise<{
  commerce: Stage5V2CommerceSnapshot;
  service: ReturnType<typeof buildStage5ServiceInventoryV2>;
  release_policies: Stage5V2ExistingReleasePolicy[];
}> {
  const accounts = await client.query<{
    user_id: string;
    engine_account_record_id: string;
    engine_account_id: string;
    account_class: "b2c" | "b2b";
    profile_multiplier_bp: number;
    commerce_multiplier_bp: number;
    commerce_status: "pending" | "active" | "error" | "disabled";
  }>(`
    SELECT profile.user_id::text,
           account.id::text AS engine_account_record_id,
           account.engine_account_id,
           profile.customer_type::text AS account_class,
           profile.multiplier_bp AS profile_multiplier_bp,
           account.mult_bp AS commerce_multiplier_bp,
           account.status::text AS commerce_status
    FROM customer_profiles profile
    JOIN engine_accounts account ON account.user_id = profile.user_id
    WHERE account.engine_account_id IS NOT NULL
    ORDER BY account.engine_account_id COLLATE "C"
  `);
  const b2bPolicyRules = await client.query<{
    user_id: string;
    scope_type: "provider" | "model";
    provider_id: string;
    canonical_model_id: string | null;
    pricing_mode: string;
    payable_multiplier_bp: number;
  }>(`
    SELECT policy.owner_id AS user_id,
           rule.scope_type::text,
           rule.provider_id,
           rule.canonical_model_id,
           rule.pricing_mode::text,
           rule.payable_multiplier_bp
    FROM pricing_policies policy
    JOIN pricing_policy_heads head ON head.policy_id = policy.id
    JOIN pricing_policy_rules rule
      ON rule.policy_id = policy.id AND rule.policy_version = head.current_version
    WHERE policy.owner_type = 'b2b_client' AND policy.status = 'active'
    ORDER BY policy.owner_id COLLATE "C", rule.provider_id COLLATE "C",
             rule.scope_type COLLATE "C", COALESCE(rule.canonical_model_id, '') COLLATE "C"
  `);
  const policyRulesByUser = new Map<string, Stage5V2B2bPolicyHeadRule[]>();
  for (const rule of b2bPolicyRules.rows) {
    const rules = policyRulesByUser.get(rule.user_id) ?? [];
    rules.push({
      scope_type: rule.scope_type,
      provider_id: rule.provider_id,
      canonical_model_id: rule.canonical_model_id,
      pricing_mode: rule.pricing_mode,
      payable_multiplier_bp: rule.payable_multiplier_bp,
    });
    policyRulesByUser.set(rule.user_id, rules);
  }
  const accountRows = accounts.rows.map((account) => ({
    ...account,
    policy_rules: account.account_class === "b2b"
      ? policyRulesByUser.get(account.user_id) ?? null
      : null,
  }));
  const invitations = await client.query<{
    invite_id: string;
    multiplier_bp: number;
    expires_at: Date;
  }>(`
    SELECT id::text AS invite_id, multiplier_bp, expires_at
    FROM business_invites
    WHERE consumed_at IS NULL
      AND revoked_at IS NULL
      AND superseded_by_invite_id IS NULL
      AND expires_at > now()
    ORDER BY id
  `);
  const services = await client.query<StoredServiceRow>(`
    SELECT service_id, engine_account_id, purpose, responsible, status,
           source_version::text, content_digest
    FROM service_account_inventory_v2
    ORDER BY service_id COLLATE "C"
  `);
  const releasePolicyRows = await client.query<{
    policy_id: string;
    policy_version: string;
    content_digest: string;
  }>(`
    SELECT policy_id, policy_version::text, content_digest
    FROM pricing_policy_documents_v2
    WHERE policy_id LIKE 'release-v2:%'
    ORDER BY policy_id COLLATE "C", policy_version
  `);
  const serviceAccounts: ServiceAccountInventoryEntryV2[] = services.rows.map((row) => ({
    service_id: row.service_id,
    engine_account_id: row.engine_account_id,
    purpose: row.purpose,
    responsible: row.responsible,
    status: row.status,
    source_version: positiveSafeNumber(row.source_version, "service source version"),
    content_digest: row.content_digest,
  }));
  const releasePolicies: Stage5V2ExistingReleasePolicy[] = releasePolicyRows.rows.map((row) => ({
    policy_id: row.policy_id,
    policy_version: positiveSafeNumber(row.policy_version, "release policy version"),
    content_digest: row.content_digest,
  }));
  return {
    commerce: {
      accounts: accountRows,
      invitations: invitations.rows.map((row) => ({
        invite_id: row.invite_id,
        multiplier_bp: row.multiplier_bp,
        expires_at: row.expires_at.toISOString(),
      })),
    },
    service: buildStage5ServiceInventoryV2(serviceAccounts),
    release_policies: releasePolicies,
  };
}

async function generationReservation(
  client: PoolClient,
  headGeneration: number,
  expectedPlanDigest?: string,
): Promise<{ target: number; recovery: number }> {
  if (expectedPlanDigest !== undefined) {
    const existing = await client.query<{ target_generation: string; recovery_generation: string }>(`
      SELECT target_generation::text, recovery_generation::text
      FROM pricing_stage5_runs_v2
      WHERE plan_digest = $1
    `, [expectedPlanDigest]);
    if (existing.rows[0]) {
      return {
        target: positiveSafeNumber(existing.rows[0].target_generation, "stored target generation"),
        recovery: positiveSafeNumber(existing.rows[0].recovery_generation, "stored recovery generation"),
      };
    }
  }
  const result = await client.query<{ generation: string }>(`
    SELECT GREATEST(
      COALESCE((SELECT max(generation) FROM pricing_release_plans_v2), 0),
      COALESCE((SELECT max(recovery_generation) FROM pricing_stage5_runs_v2
                WHERE status NOT IN ('blocked', 'failed')), 0),
      $1::bigint
    )::text AS generation
  `, [headGeneration]);
  const maximum = Number(result.rows[0]!.generation);
  if (!Number.isSafeInteger(maximum) || maximum < 0 || maximum > Number.MAX_SAFE_INTEGER - 2) {
    throw new Stage5MaterializerV2Error(
      "release_generation_exhausted",
      "cannot reserve two safe pricing release generations",
    );
  }
  return { target: maximum + 1, recovery: maximum + 2 };
}

export async function collectStage5V2Plan(
  database: Database,
  engine: Pick<
    EngineClient,
    | "getLatestPricingReleasePolicyV2"
    | "getPricingReleaseInventoryV2"
    | "getPricingReleaseHeadV2"
    | "getPricingReleaseV2"
  >,
  openkeys: Stage5V2OpenKeysReader,
  options: { expectedPlanDigest?: string } = {},
): Promise<Stage5V2Plan> {
  const [engineFirst, openkeysFirst, headFirst] = await Promise.all([
    scanStage5EngineInventoryV2(engine),
    scanStage5OpenKeysInventoryV2(openkeys),
    engine.getPricingReleaseHeadV2(),
  ]);
  const client = await database.pool.connect();
  let snapshot: Awaited<ReturnType<typeof readStage5V2CommerceAndServiceSnapshot>>;
  let reservation: { target: number; recovery: number };
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    snapshot = await readStage5V2CommerceAndServiceSnapshot(client);
    reservation = await generationReservation(
      client,
      headFirst?.active_generation ?? 0,
      options.expectedPlanDigest,
    );
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
  const provisionalPlan = buildStage5V2Plan({
    commerce: snapshot.commerce,
    service: snapshot.service,
    existing_release_policies: snapshot.release_policies,
    engine_first: engineFirst,
    engine_second: engineFirst,
    openkeys_first: openkeysFirst,
    openkeys_second: openkeysFirst,
    head_first: headFirst,
    head_second: headFirst,
    target_generation: reservation.target,
    recovery_generation: reservation.recovery,
  });
  const policyIds = provisionalPlan.policies.map((policy) => policy.policy_id);
  const remotePoliciesFirst = await readLatestEnginePolicies(engine, policyIds);
  const [engineSecond, openkeysSecond, headSecond, targetExisting, recoveryExisting, remotePoliciesSecond] =
    await Promise.all([
      scanStage5EngineInventoryV2(engine),
      scanStage5OpenKeysInventoryV2(openkeys),
      engine.getPricingReleaseHeadV2(),
      engine.getPricingReleaseV2(reservation.target),
      engine.getPricingReleaseV2(reservation.recovery),
      readLatestEnginePolicies(engine, policyIds),
    ]);
  return buildStage5V2Plan({
    commerce: snapshot.commerce,
    service: snapshot.service,
    existing_release_policies: reconcileReleasePolicyLineage(
      snapshot.release_policies,
      remotePoliciesFirst,
      remotePoliciesSecond,
    ),
    engine_first: engineFirst,
    engine_second: engineSecond,
    openkeys_first: openkeysFirst,
    openkeys_second: openkeysSecond,
    head_first: headFirst,
    head_second: headSecond,
    target_generation: reservation.target,
    recovery_generation: reservation.recovery,
    occupied_generations: [
      ...(targetExisting === null ? [] : [reservation.target]),
      ...(recoveryExisting === null ? [] : [reservation.recovery]),
    ],
  });
}

async function readStoredRun(client: PoolClient, planDigest: string): Promise<StoredRunRow | null> {
  const result = await client.query<StoredRunRow>(`
    SELECT run_id::text, schema_version::text, plan_digest, commerce_inventory_digest,
           engine_scan_first_digest, engine_scan_second_digest,
           openkeys_scan_first_digest, openkeys_scan_second_digest,
           service_inventory_digest, funding_plan_digest,
           target_generation::text, target_digest,
           recovery_generation::text, recovery_digest,
           inventory_artifact, plan_artifact, blocker_count::text, status
    FROM pricing_stage5_runs_v2
    WHERE plan_digest = $1
  `, [planDigest]);
  return result.rows[0] ?? null;
}

export async function readPricingStage5RunV2(
  database: Database,
  planDigest: string,
): Promise<PricingStage5RunV2 | null> {
  const query = pricingStage5RunQueryV2Schema.parse({ plan_digest: planDigest });
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    const result = await client.query<{
      run_id: string;
      plan_digest: string;
      status: StoredRunRow["status"];
      target_generation: string;
      target_plan_digest: string | null;
      target_release_digest: string | null;
      recovery_generation: string;
      recovery_plan_digest: string | null;
      recovery_release_digest: string | null;
      blocker_count: string;
    }>(`
      SELECT run.run_id::text, run.plan_digest, run.status,
             run.target_generation::text, target.content_digest AS target_plan_digest,
             run.target_digest AS target_release_digest,
             run.recovery_generation::text, recovery.content_digest AS recovery_plan_digest,
             run.recovery_digest AS recovery_release_digest, run.blocker_count::text
      FROM pricing_stage5_runs_v2 run
      LEFT JOIN pricing_release_plans_v2 target ON target.generation = run.target_generation
      LEFT JOIN pricing_release_plans_v2 recovery ON recovery.generation = run.recovery_generation
      WHERE run.plan_digest = $1
    `, [query.plan_digest]);
    const row = result.rows[0];
    const parsed = row === undefined ? null : pricingStage5RunV2Schema.parse(row);
    await client.query("COMMIT");
    return parsed;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

function expectedStoredRun(plan: Stage5V2Plan, status: StoredRunRow["status"]): Omit<StoredRunRow, "run_id"> {
  return {
    schema_version: "2",
    plan_digest: plan.plan_digest,
    commerce_inventory_digest: plan.commerce_inventory_digest,
    engine_scan_first_digest: plan.engine_scan_first_digest,
    engine_scan_second_digest: plan.engine_scan_second_digest,
    openkeys_scan_first_digest: plan.openkeys_scan_first_digest,
    openkeys_scan_second_digest: plan.openkeys_scan_second_digest,
    service_inventory_digest: plan.service_inventory_digest,
    funding_plan_digest: plan.funding_plan_digest,
    target_generation: String(plan.target_generation),
    target_digest: null,
    recovery_generation: String(plan.recovery_generation),
    recovery_digest: null,
    inventory_artifact: plan.inventory_artifact,
    plan_artifact: planArtifact(plan),
    blocker_count: String(plan.blockers.length),
    status,
  };
}

async function ensureRun(
  client: PoolClient,
  plan: Stage5V2Plan,
  status: "blocked" | "planned",
): Promise<{ runId: string; inserted: boolean }> {
  const inserted = await client.query<{ run_id: string }>(`
    INSERT INTO pricing_stage5_runs_v2 (
      schema_version, plan_digest, commerce_inventory_digest,
      engine_scan_first_digest, engine_scan_second_digest,
      openkeys_scan_first_digest, openkeys_scan_second_digest,
      service_inventory_digest, funding_plan_digest,
      target_generation, target_digest, recovery_generation, recovery_digest,
      inventory_artifact, plan_artifact, blocker_count, status
    ) VALUES (
      2, $1, $2, $3, $4, $5, $6, $7, $8,
      $9, NULL, $10, NULL, $11::jsonb, $12::jsonb, $13, $14
    )
    ON CONFLICT (plan_digest) DO NOTHING
    RETURNING run_id::text
  `, [
    plan.plan_digest,
    plan.commerce_inventory_digest,
    plan.engine_scan_first_digest,
    plan.engine_scan_second_digest,
    plan.openkeys_scan_first_digest,
    plan.openkeys_scan_second_digest,
    plan.service_inventory_digest,
    plan.funding_plan_digest,
    plan.target_generation,
    plan.recovery_generation,
    JSON.stringify(plan.inventory_artifact),
    JSON.stringify(planArtifact(plan)),
    plan.blockers.length,
    status,
  ]);
  const stored = await readStoredRun(client, plan.plan_digest);
  if (!stored) {
    throw new Stage5MaterializerV2Error("stage5_run_missing", "Stage 5 run disappeared after insert");
  }
  const {
    run_id: runId,
    status: storedStatus,
    inventory_artifact: _storedMovingEvidence,
    ...identity
  } = stored;
  const {
    inventory_artifact: _freshMovingEvidence,
    ...expectedIdentity
  } = expectedStoredRun(plan, storedStatus);
  const allowedReplayStatus = status === "planned"
    ? ["planned", "materializing"]
    : ["blocked"];
  if (!allowedReplayStatus.includes(storedStatus)
      || !sameCanonical(
        { ...identity, status: storedStatus },
        { ...expectedIdentity, status: storedStatus },
      )) {
    throw new Stage5MaterializerV2Error(
      "stage5_run_digest_collision",
      "stored Stage 5 run has different immutable content for the same plan digest",
    );
  }
  return {
    runId: inserted.rows[0]?.run_id ?? runId,
    inserted: inserted.rowCount === 1,
  };
}

async function recordStage5MaterializationAudit(
  client: PoolClient,
  runId: string,
  plan: Stage5V2Plan,
  status: "blocked" | "planned",
  inserted: boolean,
  audit: Stage5MaterializerV2Audit | undefined,
): Promise<void> {
  if (audit === undefined) return;
  await client.query(`
    INSERT INTO audit_log (
      actor_type, actor_id, action, target_type, target_id, metadata
    ) VALUES (
      'admin', $1, 'pricing_stage5_materialization_requested',
      'pricing_stage5_run_v2', $2,
      jsonb_build_object(
        'plan_digest', $3::text,
        'target_generation', $4::text,
        'target_plan_digest', $5::text,
        'recovery_generation', $6::text,
        'recovery_plan_digest', $7::text,
        'blocker_count', $8::text,
        'requested_status', $9::text,
        'idempotent_replay', $10::boolean,
        'reason', $11::text
      )
    )
  `, [
    audit.actorId,
    runId,
    plan.plan_digest,
    String(plan.target_generation),
    plan.target.content_digest,
    String(plan.recovery_generation),
    plan.recovery.content_digest,
    String(plan.blockers.length),
    status,
    !inserted,
    audit.reason,
  ]);
}

async function ensureBlockers(
  client: PoolClient,
  runId: string,
  blockers: readonly Stage5V2Blocker[],
): Promise<void> {
  for (const item of blockers) {
    await client.query(`
      INSERT INTO pricing_stage5_blockers_v2 (
        run_id, blocker_digest, blocker_code, blocker_context, subject_id, detail
      ) VALUES ($1, $2, $3, $4, $5, $6)
      ON CONFLICT (run_id, blocker_digest) DO NOTHING
    `, [
      runId,
      item.blocker_digest,
      item.blocker_code,
      item.blocker_context,
      item.subject_id,
      item.detail,
    ]);
  }
  const stored = await client.query<{
    blocker_digest: string;
    blocker_code: string;
    blocker_context: Stage5V2Blocker["blocker_context"];
    subject_id: string;
    detail: string;
  }>(`
    SELECT blocker_digest, blocker_code, blocker_context, subject_id, detail
    FROM pricing_stage5_blockers_v2
    WHERE run_id = $1
    ORDER BY blocker_context COLLATE "C", subject_id COLLATE "C", blocker_code COLLATE "C"
  `, [runId]);
  if (!sameCanonical(stored.rows, blockers)) {
    throw new Stage5MaterializerV2Error(
      "stage5_blocker_conflict",
      "stored Stage 5 blockers differ from the exact plan",
    );
  }
}

async function ensureCapability(client: PoolClient, plan: Stage5V2Plan): Promise<void> {
  const capability = plan.capability;
  await client.query(`
    INSERT INTO provider_capability_versions (
      generation, schema_version, content_digest, source_runtime, source_revision, observed_at
    ) VALUES ($1, $2, $3, 'pricing-release-v2', $3, now())
    ON CONFLICT (generation) DO NOTHING
  `, [capability.generation, capability.schema_version, capability.content_digest]);
  for (const entry of capability.entries) {
    await client.query(`
      INSERT INTO provider_capability_entries (
        generation, provider_id, canonical_model_id, entry_digest, capability_data
      ) VALUES ($1, $2, $3, $4, $5::jsonb)
      ON CONFLICT (generation, provider_id, canonical_model_id) DO NOTHING
    `, [
      capability.generation,
      entry.provider_id,
      entry.canonical_model_id,
      entry.entry_digest,
      JSON.stringify(entry.capability_data),
    ]);
  }
  for (const alias of capability.aliases) {
    await client.query(`
      INSERT INTO provider_capability_aliases (
        generation, provider_id, alias_model_id, canonical_model_id
      ) VALUES ($1, $2, $3, $4)
      ON CONFLICT (generation, provider_id, alias_model_id) DO NOTHING
    `, [capability.generation, alias.provider_id, alias.alias_model_id, alias.canonical_model_id]);
  }
  const header = await client.query<{
    generation: string;
    schema_version: string;
    content_digest: string;
  }>(`
    SELECT generation::text, schema_version::text, content_digest
    FROM provider_capability_versions WHERE generation = $1
  `, [capability.generation]);
  const entries = await client.query<{
    provider_id: string;
    canonical_model_id: string;
    entry_digest: string;
    capability_data: Record<string, unknown>;
  }>(`
    SELECT provider_id, canonical_model_id, entry_digest, capability_data
    FROM provider_capability_entries WHERE generation = $1
    ORDER BY provider_id COLLATE "C", canonical_model_id COLLATE "C"
  `, [capability.generation]);
  const aliases = await client.query<{
    provider_id: string;
    alias_model_id: string;
    canonical_model_id: string;
  }>(`
    SELECT provider_id, alias_model_id, canonical_model_id
    FROM provider_capability_aliases WHERE generation = $1
    ORDER BY provider_id COLLATE "C", alias_model_id COLLATE "C"
  `, [capability.generation]);
  const expectedEntries = [...capability.entries]
    .sort((left, right) => compareUtf8(left.provider_id, right.provider_id)
      || compareUtf8(left.canonical_model_id, right.canonical_model_id))
    .map(({ provider_id, canonical_model_id, entry_digest, capability_data }) =>
      ({ provider_id, canonical_model_id, entry_digest, capability_data }));
  if (!sameCanonical(header.rows, [{
    generation: String(capability.generation),
    schema_version: String(capability.schema_version),
    content_digest: capability.content_digest,
  }]) || !sameCanonical(entries.rows, expectedEntries)
      || !sameCanonical(aliases.rows, capability.aliases)) {
    throw new Stage5MaterializerV2Error(
      "capability_generation_conflict",
      "stored target capability generation differs from the reviewed projection",
    );
  }
}

async function ensureCatalog(client: PoolClient, catalog: PricingCatalogSpec): Promise<void> {
  await client.query(`
    INSERT INTO product_catalog_versions (
      product_id, generation, schema_version, capability_generation,
      capability_digest, content_digest, actor_type, actor_id, reason
    ) VALUES ($1, $2, $3, $4, $5, $6, 'operator', 'pricing-stage5-v2',
              'prepare dormant pricing release v2 catalog')
    ON CONFLICT (product_id, generation) DO NOTHING
  `, [
    catalog.product_id,
    catalog.generation,
    catalog.schema_version,
    catalog.capability_generation,
    catalog.capability_digest,
    catalog.content_digest,
  ]);
  for (const entry of catalog.entries) {
    await client.query(`
      INSERT INTO product_catalog_entries (
        product_id, generation, capability_generation,
        provider_id, canonical_model_id, enabled
      ) VALUES ($1, $2, $3, $4, $5, $6)
      ON CONFLICT (product_id, generation, provider_id, canonical_model_id) DO NOTHING
    `, [
      catalog.product_id,
      catalog.generation,
      catalog.capability_generation,
      entry.provider_id,
      entry.canonical_model_id,
      entry.enabled,
    ]);
  }
  const stored = await client.query<{
    product_id: string;
    generation: string;
    schema_version: string;
    capability_generation: string;
    capability_digest: string;
    content_digest: string;
  }>(`
    SELECT product_id, generation::text, schema_version::text,
           capability_generation::text, capability_digest, content_digest
    FROM product_catalog_versions
    WHERE product_id = $1 AND generation = $2
  `, [catalog.product_id, catalog.generation]);
  const entries = await client.query<{
    provider_id: string;
    canonical_model_id: string;
    enabled: boolean;
  }>(`
    SELECT provider_id, canonical_model_id, enabled
    FROM product_catalog_entries
    WHERE product_id = $1 AND generation = $2
    ORDER BY provider_id COLLATE "C", canonical_model_id COLLATE "C"
  `, [catalog.product_id, catalog.generation]);
  const expectedEntries = [...catalog.entries].sort((left, right) =>
    compareUtf8(left.provider_id, right.provider_id)
    || compareUtf8(left.canonical_model_id, right.canonical_model_id));
  if (!sameCanonical(stored.rows, [{
    product_id: catalog.product_id,
    generation: String(catalog.generation),
    schema_version: String(catalog.schema_version),
    capability_generation: String(catalog.capability_generation),
    capability_digest: catalog.capability_digest,
    content_digest: catalog.content_digest,
  }]) || !sameCanonical(entries.rows, expectedEntries)) {
    throw new Stage5MaterializerV2Error(
      "catalog_generation_conflict",
      `stored catalog ${catalog.product_id}/${catalog.generation} differs from Stage 5`,
    );
  }
}

function switchDbParts(scope: ProviderSwitchSpec["entries"][number]["scope"]): {
  scope_type: "master" | "product" | "segment";
  product_id: string;
  segment: string;
} {
  if (scope === "master") return { scope_type: "master", product_id: "", segment: "" };
  if ("product" in scope) {
    return { scope_type: "product", product_id: scope.product.product_id, segment: "" };
  }
  return {
    scope_type: "segment",
    product_id: scope.segment.product_id,
    segment: scope.segment.segment,
  };
}

async function ensureSwitches(client: PoolClient, switches: ProviderSwitchSpec): Promise<void> {
  await client.query(`
    INSERT INTO provider_switch_versions (
      generation, schema_version, capability_generation, capability_digest,
      content_digest, actor_type, actor_id, reason
    ) VALUES ($1, $2, $3, $4, $5, 'operator', 'pricing-stage5-v2',
              'prepare dormant pricing release v2 provider switches')
    ON CONFLICT (generation) DO NOTHING
  `, [
    switches.generation,
    switches.schema_version,
    switches.capability_generation,
    switches.capability_digest,
    switches.content_digest,
  ]);
  for (const entry of switches.entries) {
    const scope = switchDbParts(entry.scope);
    await client.query(`
      INSERT INTO provider_switch_entries (
        generation, provider_id, scope_type, product_id, segment,
        catalog_generation, enabled
      ) VALUES ($1, $2, $3, $4, $5, $6, $7)
      ON CONFLICT (generation, provider_id, scope_type, product_id, segment) DO NOTHING
    `, [
      switches.generation,
      entry.provider_id,
      scope.scope_type,
      scope.product_id,
      scope.segment,
      entry.catalog_generation,
      entry.enabled,
    ]);
  }
  const header = await client.query<{
    generation: string;
    schema_version: string;
    capability_generation: string;
    capability_digest: string;
    content_digest: string;
  }>(`
    SELECT generation::text, schema_version::text, capability_generation::text,
           capability_digest, content_digest
    FROM provider_switch_versions WHERE generation = $1
  `, [switches.generation]);
  const entries = await client.query<{
    provider_id: string;
    scope_type: "master" | "product" | "segment";
    product_id: string;
    segment: string;
    catalog_generation: string | null;
    enabled: boolean;
  }>(`
    SELECT provider_id, scope_type, product_id, segment,
           catalog_generation::text, enabled
    FROM provider_switch_entries WHERE generation = $1
    ORDER BY provider_id COLLATE "C", scope_type COLLATE "C",
             product_id COLLATE "C", segment COLLATE "C"
  `, [switches.generation]);
  const expectedEntries = switches.entries.map((entry) => ({
    provider_id: entry.provider_id,
    ...switchDbParts(entry.scope),
    catalog_generation: entry.catalog_generation === null ? null : String(entry.catalog_generation),
    enabled: entry.enabled,
  })).sort((left, right) => compareUtf8(left.provider_id, right.provider_id)
    || compareUtf8(left.scope_type, right.scope_type)
    || compareUtf8(left.product_id, right.product_id)
    || compareUtf8(left.segment, right.segment));
  if (!sameCanonical(header.rows, [{
    generation: String(switches.generation),
    schema_version: String(switches.schema_version),
    capability_generation: String(switches.capability_generation),
    capability_digest: switches.capability_digest,
    content_digest: switches.content_digest,
  }]) || !sameCanonical(entries.rows, expectedEntries)) {
    throw new Stage5MaterializerV2Error(
      "switch_generation_conflict",
      `stored switches generation ${switches.generation} differs from Stage 5`,
    );
  }
}

function policyDbClass(value: PricingReleasePolicyV2["account_class"]): string {
  return value === "open_keys" ? "openkeys" : value;
}

function policyDbOwner(value: PricingReleasePolicyV2["owner_type"]): string {
  return value === "open_keys" ? "openkeys" : value;
}

function ruleDbParts(rule: PricingReleasePolicyV2["rules"][number]): {
  scope_type: "global" | "provider" | "model";
  provider_id: string | null;
  canonical_model_id: string | null;
} {
  if (rule.scope.scope === "global") {
    return { scope_type: "global", provider_id: null, canonical_model_id: null };
  }
  if (rule.scope.scope === "provider") {
    return {
      scope_type: "provider",
      provider_id: rule.scope.provider_id,
      canonical_model_id: null,
    };
  }
  return {
    scope_type: "model",
    provider_id: rule.scope.provider_id,
    canonical_model_id: rule.scope.canonical_model_id,
  };
}

async function ensurePolicy(client: PoolClient, policy: PricingReleasePolicyV2): Promise<void> {
  await client.query(`
    INSERT INTO pricing_policy_documents_v2 (
      policy_id, policy_version, owner_type, owner_id, account_class,
      product_id, billing_mode, schema_version, capability_generation,
      capability_digest, catalog_generation, catalog_digest,
      switch_generation, switch_digest, content_digest
    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
    ON CONFLICT (policy_id, policy_version) DO NOTHING
  `, [
    policy.policy_id,
    policy.policy_version,
    policyDbOwner(policy.owner_type),
    policy.owner_id,
    policyDbClass(policy.account_class),
    policy.product_id,
    policy.billing_mode,
    policy.schema_version,
    policy.capability_generation,
    policy.capability_digest,
    policy.catalog_generation,
    policy.catalog_digest,
    policy.switch_generation,
    policy.switch_digest,
    policy.content_digest,
  ]);
  for (const rule of policy.rules) {
    const scope = ruleDbParts(rule);
    await client.query(`
      INSERT INTO pricing_policy_rules_v2 (
        policy_id, policy_version, rule_id, rule_digest, scope_type,
        provider_id, canonical_model_id, discount_bps, payable_multiplier_bp
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
      ON CONFLICT (policy_id, policy_version, rule_id) DO NOTHING
    `, [
      policy.policy_id,
      policy.policy_version,
      rule.rule_id,
      rule.rule_digest,
      scope.scope_type,
      scope.provider_id,
      scope.canonical_model_id,
      rule.discount_bps,
      rule.payable_multiplier_bp,
    ]);
  }
  const header = await client.query<{
    policy_id: string;
    policy_version: string;
    owner_type: string;
    owner_id: string;
    account_class: string;
    product_id: string | null;
    billing_mode: string;
    schema_version: string;
    capability_generation: string;
    capability_digest: string;
    catalog_generation: string | null;
    catalog_digest: string | null;
    switch_generation: string | null;
    switch_digest: string | null;
    content_digest: string;
  }>(`
    SELECT policy_id, policy_version::text, owner_type, owner_id, account_class,
           product_id, billing_mode, schema_version::text,
           capability_generation::text, capability_digest,
           catalog_generation::text, catalog_digest,
           switch_generation::text, switch_digest, content_digest
    FROM pricing_policy_documents_v2
    WHERE policy_id = $1 AND policy_version = $2
  `, [policy.policy_id, policy.policy_version]);
  const rules = await client.query<{
    rule_id: string;
    rule_digest: string;
    scope_type: "global" | "provider" | "model";
    provider_id: string | null;
    canonical_model_id: string | null;
    discount_bps: string;
    payable_multiplier_bp: string;
  }>(`
    SELECT rule_id, rule_digest, scope_type, provider_id, canonical_model_id,
           discount_bps::text, payable_multiplier_bp::text
    FROM pricing_policy_rules_v2
    WHERE policy_id = $1 AND policy_version = $2
    ORDER BY rule_id COLLATE "C"
  `, [policy.policy_id, policy.policy_version]);
  const expectedRules = policy.rules.map((rule) => ({
    rule_id: rule.rule_id,
    rule_digest: rule.rule_digest,
    ...ruleDbParts(rule),
    discount_bps: String(rule.discount_bps),
    payable_multiplier_bp: String(rule.payable_multiplier_bp),
  })).sort((left, right) => compareUtf8(left.rule_id, right.rule_id));
  if (!sameCanonical(header.rows, [{
    policy_id: policy.policy_id,
    policy_version: String(policy.policy_version),
    owner_type: policyDbOwner(policy.owner_type),
    owner_id: policy.owner_id,
    account_class: policyDbClass(policy.account_class),
    product_id: policy.product_id,
    billing_mode: policy.billing_mode,
    schema_version: String(policy.schema_version),
    capability_generation: String(policy.capability_generation),
    capability_digest: policy.capability_digest,
    catalog_generation: policy.catalog_generation === null ? null : String(policy.catalog_generation),
    catalog_digest: policy.catalog_digest,
    switch_generation: policy.switch_generation === null ? null : String(policy.switch_generation),
    switch_digest: policy.switch_digest,
    content_digest: policy.content_digest,
  }]) || !sameCanonical(rules.rows, expectedRules)) {
    throw new Stage5MaterializerV2Error(
      "policy_version_conflict",
      `stored policy ${policy.policy_id}/${policy.policy_version} differs from Stage 5`,
    );
  }
}

async function ensureInvitationSnapshots(client: PoolClient, plan: Stage5V2Plan): Promise<void> {
  for (const snapshot of plan.invitation_snapshots) {
    await client.query(`
      INSERT INTO business_invite_policy_snapshots_v2 (
        invite_id, policy_id, policy_version, policy_digest, snapshot_digest
      ) VALUES ($1, $2, $3, $4, $5)
      ON CONFLICT (invite_id) DO NOTHING
    `, [
      snapshot.invite_id,
      snapshot.policy_id,
      snapshot.policy_version,
      snapshot.policy_digest,
      snapshot.snapshot_digest,
    ]);
  }
  const ids = plan.invitation_snapshots.map((snapshot) => snapshot.invite_id);
  if (ids.length === 0) return;
  const stored = await client.query<{
    invite_id: string;
    policy_id: string;
    policy_version: string;
    policy_digest: string;
    snapshot_digest: string;
  }>(`
    SELECT invite_id::text, policy_id, policy_version::text,
           policy_digest, snapshot_digest
    FROM business_invite_policy_snapshots_v2
    WHERE invite_id = ANY($1::uuid[])
    ORDER BY invite_id
  `, [ids]);
  const expected = plan.invitation_snapshots.map((snapshot) => ({
    ...snapshot,
    policy_version: String(snapshot.policy_version),
  })).sort((left, right) => compareUtf8(left.invite_id, right.invite_id));
  if (!sameCanonical(stored.rows, expected)) {
    throw new Stage5MaterializerV2Error(
      "invitation_snapshot_conflict",
      "stored B2B invitation policy snapshots differ from Stage 5",
    );
  }
}

async function ensureReleasePlan(client: PoolClient, release: Stage5V2ReleasePlan): Promise<void> {
  await client.query(`
    INSERT INTO pricing_release_plans_v2 (
      generation, release_kind, schema_version,
      commerce_inventory_digest, engine_inventory_digest,
      openkeys_inventory_digest, service_inventory_digest,
      policy_manifest_digest, assignment_manifest_digest,
      funding_manifest_digest, engine_release_digest, content_digest, status
    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, NULL, $10, 'planned')
    ON CONFLICT (generation) DO NOTHING
  `, [
    release.generation,
    release.release_kind,
    release.schema_version,
    release.commerce_inventory_digest,
    release.engine_inventory_digest,
    release.openkeys_inventory_digest,
    release.service_inventory_digest,
    release.policy_manifest_digest,
    release.assignment_manifest_digest,
    release.content_digest,
  ]);
  for (const item of release.assignments) {
    await client.query(`
      INSERT INTO pricing_release_assignments_v2 (
        release_generation, engine_account_id, account_class,
        owner_context, owner_id, policy_id, policy_version, policy_digest,
        billing_mode, funding_generation, purpose, responsible, assignment_digest
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, $10, $11, $12)
      ON CONFLICT (release_generation, engine_account_id) DO NOTHING
    `, [
      item.release_generation,
      item.engine_account_id,
      item.account_class,
      item.owner_context,
      item.owner_id,
      item.policy_id,
      item.policy_version,
      item.policy_digest,
      item.billing_mode,
      item.purpose,
      item.responsible,
      item.assignment_digest,
    ]);
  }
  const header = await client.query<{
    generation: string;
    release_kind: "target" | "recovery";
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
  }>(`
    SELECT generation::text, release_kind, schema_version::text,
           commerce_inventory_digest, engine_inventory_digest,
           openkeys_inventory_digest, service_inventory_digest,
           policy_manifest_digest, assignment_manifest_digest,
           funding_manifest_digest, engine_release_digest, content_digest
    FROM pricing_release_plans_v2 WHERE generation = $1
  `, [release.generation]);
  const assignments = await client.query<{
    release_generation: string;
    engine_account_id: string;
    account_class: Stage5V2PlannedAssignment["account_class"];
    owner_context: Stage5V2PlannedAssignment["owner_context"];
    owner_id: string;
    policy_id: string;
    policy_version: string;
    policy_digest: string;
    billing_mode: Stage5V2PlannedAssignment["billing_mode"];
    funding_generation: string | null;
    purpose: string | null;
    responsible: string | null;
    assignment_digest: string;
  }>(`
    SELECT release_generation::text, engine_account_id, account_class,
           owner_context, owner_id, policy_id, policy_version::text,
           policy_digest, billing_mode, funding_generation::text,
           purpose, responsible, assignment_digest
    FROM pricing_release_assignments_v2
    WHERE release_generation = $1
    ORDER BY engine_account_id COLLATE "C"
  `, [release.generation]);
  const expectedAssignments = release.assignments.map((item) => ({
    ...item,
    release_generation: String(item.release_generation),
    policy_version: String(item.policy_version),
  }));
  if (!sameCanonical(header.rows, [{
    generation: String(release.generation),
    release_kind: release.release_kind,
    schema_version: String(release.schema_version),
    commerce_inventory_digest: release.commerce_inventory_digest,
    engine_inventory_digest: release.engine_inventory_digest,
    openkeys_inventory_digest: release.openkeys_inventory_digest,
    service_inventory_digest: release.service_inventory_digest,
    policy_manifest_digest: release.policy_manifest_digest,
    assignment_manifest_digest: release.assignment_manifest_digest,
    funding_manifest_digest: null,
    engine_release_digest: null,
    content_digest: release.content_digest,
  }]) || !sameCanonical(assignments.rows, expectedAssignments)) {
    throw new Stage5MaterializerV2Error(
      "release_plan_conflict",
      `stored ${release.release_kind} release skeleton differs from Stage 5`,
    );
  }
}

async function persistBlockedPlan(
  database: Database,
  plan: Stage5V2Plan,
  audit?: Stage5MaterializerV2Audit,
): Promise<string> {
  if (plan.engine_scan_first_digest !== plan.engine_scan_second_digest
      || plan.openkeys_scan_first_digest !== plan.openkeys_scan_second_digest) {
    throw new Stage5MaterializerV2Error(
      "unstable_inventory",
      "unstable exhaustive scans cannot be persisted as Stage 5 evidence; repeat the scan",
    );
  }
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query("SELECT pg_advisory_xact_lock(hashtextextended('pricing-stage5-v2:materialize', 0))");
    const current = await readStage5V2CommerceAndServiceSnapshot(client);
    if (stage5V2CommerceInventoryDigest(current.commerce) !== plan.commerce_inventory_digest
        || current.service.inventory_digest !== plan.service_inventory_digest) {
      throw new Stage5MaterializerV2Error(
        "source_snapshot_stale",
        "commerce or service authority changed after the exact Stage 5 plan was built",
      );
    }
    const ensured = await ensureRun(client, plan, "blocked");
    const runId = ensured.runId;
    await ensureBlockers(client, runId, plan.blockers);
    await recordStage5MaterializationAudit(client, runId, plan, "blocked", ensured.inserted, audit);
    await client.query("COMMIT");
    return runId;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

async function persistLocalPlan(
  database: Database,
  plan: Stage5V2Plan,
  audit?: Stage5MaterializerV2Audit,
): Promise<string> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query("SELECT pg_advisory_xact_lock(hashtextextended('pricing-stage5-v2:materialize', 0))");
    const current = await readStage5V2CommerceAndServiceSnapshot(client);
    if (stage5V2CommerceInventoryDigest(current.commerce) !== plan.commerce_inventory_digest
        || current.service.inventory_digest !== plan.service_inventory_digest) {
      throw new Stage5MaterializerV2Error(
        "source_snapshot_stale",
        "commerce or service authority changed after the exact Stage 5 plan was built",
      );
    }
    const ensured = await ensureRun(client, plan, "planned");
    const runId = ensured.runId;
    await ensureCapability(client, plan);
    for (const catalog of plan.catalogs) await ensureCatalog(client, catalog);
    await ensureSwitches(client, plan.switches);
    for (const policy of plan.policies) await ensurePolicy(client, policy);
    await ensureInvitationSnapshots(client, plan);
    await ensureReleasePlan(client, plan.target);
    await ensureReleasePlan(client, plan.recovery);
    await recordStage5MaterializationAudit(client, runId, plan, "planned", ensured.inserted, audit);
    await client.query("COMMIT");
    return runId;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

type PrepareArtifactKind =
  | "main_catalog"
  | "openkeys_catalog"
  | "switches"
  | "policy_b2c"
  | "policy_b2b"
  | "policy_openkeys"
  | "policy_service";
type PrepareMutationResult =
  | { result: "stored" | "unchanged" }
  | { result: "applied" }
  | {
    result: "rejected";
    code:
      | "invalid"
      | "missing_dependency"
      | "stale"
      | "version_conflict"
      | "cas_mismatch"
      | "policy_cas_mismatch"
      | "locked";
  };

export function stage5V2PrepareMutationResult(
  result: PrepareMutationResult,
  artifactKind: PrepareArtifactKind,
  label: string,
): "stored" | "unchanged" {
  if (result.result === "stored" || result.result === "unchanged") return result.result;
  const rejection = result.result === "rejected" ? result.code : result.result;
  throw new Stage5MaterializerV2Error(
    `engine_${artifactKind}_prepare_${rejection}`,
    `${label} prepare was rejected with ${rejection}`,
  );
}

async function prepareEngineArtifacts(
  engine: Pick<
    EngineClient,
    | "preparePricingCatalog"
    | "getPricingCatalogVersion"
    | "prepareProviderSwitches"
    | "getProviderSwitchVersion"
    | "preparePricingReleasePolicyV2"
    | "getPricingReleasePolicyV2"
  >,
  plan: Stage5V2Plan,
): Promise<PrepareAck[]> {
  const acks: PrepareAck[] = [];
  for (const catalog of plan.catalogs) {
    const artifactKind = catalog.product_id === "main" ? "main_catalog" : "openkeys_catalog";
    const mutation = await engine.preparePricingCatalog(catalog);
    const result = stage5V2PrepareMutationResult(
      mutation,
      artifactKind,
      `catalog ${catalog.product_id}/${catalog.generation}`,
    );
    const readback = await engine.getPricingCatalogVersion(catalog.product_id, catalog.generation);
    if (!readback || !sameCanonical(readback, catalog)) {
      throw new Stage5MaterializerV2Error(
        "engine_catalog_readback_mismatch",
        `catalog ${catalog.product_id}/${catalog.generation} readback differs from prepare`,
      );
    }
    acks.push({
      artifact_kind: catalog.product_id === "main" ? "main_catalog" : "openkeys_catalog",
      artifact_id: catalog.product_id,
      artifact_version: catalog.generation,
      expected_digest: catalog.content_digest,
      mutation_result: result,
      readback_digest: readback.content_digest,
    });
  }
  const switchMutation = await engine.prepareProviderSwitches(plan.switches);
  const switchResult = stage5V2PrepareMutationResult(
    switchMutation,
    "switches",
    `switches ${plan.switches.generation}`,
  );
  const switchReadback = await engine.getProviderSwitchVersion(plan.switches.generation);
  if (!switchReadback || !sameCanonical(switchReadback, plan.switches)) {
    throw new Stage5MaterializerV2Error(
      "engine_switch_readback_mismatch",
      `switches ${plan.switches.generation} readback differs from prepare`,
    );
  }
  acks.push({
    artifact_kind: "switches",
    artifact_id: "global",
    artifact_version: plan.switches.generation,
    expected_digest: plan.switches.content_digest,
    mutation_result: switchResult,
    readback_digest: switchReadback.content_digest,
  });
  for (const policy of plan.policies) {
    const accountClass = policy.account_class === "open_keys" ? "openkeys" : policy.account_class;
    const mutation = await engine.preparePricingReleasePolicyV2(policy);
    const result = stage5V2PrepareMutationResult(
      mutation,
      `policy_${accountClass}`,
      `policy ${policy.policy_id}/${policy.policy_version}`,
    );
    const readback = await engine.getPricingReleasePolicyV2(policy.policy_id, policy.policy_version);
    if (!readback || !sameCanonical(readback, policy)) {
      throw new Stage5MaterializerV2Error(
        "engine_policy_readback_mismatch",
        `policy ${policy.policy_id}/${policy.policy_version} readback differs from prepare`,
      );
    }
    acks.push({
      artifact_kind: "policy",
      artifact_id: policy.policy_id,
      artifact_version: policy.policy_version,
      expected_digest: policy.content_digest,
      mutation_result: result,
      readback_digest: readback.content_digest,
    });
  }
  return acks;
}

async function persistPrepareAcks(
  database: Database,
  runId: string,
  plan: Stage5V2Plan,
  acks: readonly PrepareAck[],
): Promise<void> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query("SELECT pg_advisory_xact_lock(hashtextextended('pricing-stage5-v2:materialize', 0))");
    const run = await readStoredRun(client, plan.plan_digest);
    if (!run || run.run_id !== runId || !["planned", "materializing"].includes(run.status)) {
      throw new Stage5MaterializerV2Error(
        "stage5_run_not_materializable",
        "Stage 5 run changed before durable prepare ACKs were stored",
      );
    }
    for (const ack of acks) {
      const ackBase = { run_id: runId, ...ack };
      const ackDigest = stage5V2Digest("prepare-ack", ackBase);
      await client.query(`
        INSERT INTO pricing_stage5_prepare_acks_v2 (
          run_id, artifact_kind, artifact_id, artifact_version,
          expected_digest, mutation_result, readback_digest, ack_digest
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (run_id, artifact_kind, artifact_id, artifact_version) DO NOTHING
      `, [
        runId,
        ack.artifact_kind,
        ack.artifact_id,
        ack.artifact_version,
        ack.expected_digest,
        ack.mutation_result,
        ack.readback_digest,
        ackDigest,
      ]);
    }
    const stored = await client.query<{
      artifact_kind: PrepareAck["artifact_kind"];
      artifact_id: string;
      artifact_version: string;
      expected_digest: string;
      mutation_result: PrepareAck["mutation_result"];
      readback_digest: string;
    }>(`
      SELECT artifact_kind, artifact_id, artifact_version::text,
             expected_digest, mutation_result, readback_digest
      FROM pricing_stage5_prepare_acks_v2
      WHERE run_id = $1
      ORDER BY artifact_kind COLLATE "C", artifact_id COLLATE "C", artifact_version
    `, [runId]);
    const expected = acks.map(({ mutation_result: _result, ...ack }) => ({
      ...ack,
      artifact_version: String(ack.artifact_version),
    })).sort((left, right) => compareUtf8(left.artifact_kind, right.artifact_kind)
      || compareUtf8(left.artifact_id, right.artifact_id)
      || Number(left.artifact_version) - Number(right.artifact_version));
    const storedIdentities = stored.rows.map(({ mutation_result: _result, ...ack }) => ack);
    if (!sameCanonical(storedIdentities, expected)) {
      throw new Stage5MaterializerV2Error(
        "prepare_ack_conflict",
        "durable engine prepare ACK set is incomplete or differs from readback",
      );
    }
    await client.query(`
      UPDATE pricing_stage5_runs_v2
      SET status = 'materializing', updated_at = now()
      WHERE run_id = $1 AND status IN ('planned', 'materializing')
    `, [runId]);
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function runStage5MaterializerV2(
  database: Database,
  engine: EngineClient,
  openkeys: Stage5V2OpenKeysReader,
  options: {
    mode: Stage5MaterializerV2Mode;
    expectedPlanDigest?: string;
    audit?: Stage5MaterializerV2Audit;
  },
): Promise<Stage5MaterializerV2Result> {
  const audit = options.audit === undefined ? undefined : {
    actorId: pricingReleaseActivationOperatorV2Schema.parse(options.audit.actorId.trim()),
    reason: pricingStageControlMutationReasonV2Schema.parse(options.audit.reason),
  };
  if (options.mode === "apply" && options.expectedPlanDigest === undefined) {
    throw new Stage5MaterializerV2Error(
      "expected_plan_digest_required",
      "Stage 5 apply requires the exact digest from a fresh dry run",
    );
  }
  const plan = await collectStage5V2Plan(database, engine, openkeys, {
    ...(options.expectedPlanDigest === undefined
      ? {}
      : { expectedPlanDigest: options.expectedPlanDigest }),
  });
  if (options.mode === "dry_run") {
    return {
      mode: "dry_run",
      plan,
      run_id: null,
      writes_committed: false,
      engine_prepared: false,
      status: "dry_run",
    };
  }
  if (plan.plan_digest !== options.expectedPlanDigest) {
    throw new Stage5MaterializerV2Error(
      "expected_plan_stale",
      `fresh Stage 5 plan is ${plan.plan_digest}, not ${options.expectedPlanDigest}`,
    );
  }
  if (plan.blockers.length > 0) {
    const runId = await persistBlockedPlan(database, plan, audit);
    return {
      mode: "apply",
      plan,
      run_id: runId,
      writes_committed: true,
      engine_prepared: false,
      status: "blocked",
    };
  }
  const runId = await persistLocalPlan(database, plan, audit);
  const acks = await prepareEngineArtifacts(engine, plan);
  await persistPrepareAcks(database, runId, plan, acks);
  return {
    mode: "apply",
    plan,
    run_id: runId,
    writes_committed: true,
    engine_prepared: true,
    status: "materializing",
  };
}
