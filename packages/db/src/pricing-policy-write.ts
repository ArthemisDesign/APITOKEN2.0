import { Buffer } from "node:buffer";
import { randomUUID } from "node:crypto";
import {
  MAIN_PRICING_PRODUCT_ID,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  accountPolicyBindingSchema,
  accountPolicySpecSchema,
  providerSwitchEditorMutationSchema,
  pricingPolicyEditorRulesSchema,
  type AccountPolicyBinding,
  type AccountPolicySpec,
  type PricingPolicyDeliveryRepairResponseV2,
  type PricingPolicyEditorRule,
  type ProviderSwitchEditorMutation,
  type ProviderSwitchSpec,
} from "@claude-api/contracts";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";
import { stage5Digest } from "./pricing-release-digest.js";
import { pricingReleaseCutoverCompleted } from "./pricing-control-jobs.js";

type ManagedOwnerType = "global_b2c" | "b2b_client" | "b2b_invitation" | "service";
type MaterializedOwnerType = Exclude<ManagedOwnerType, "b2b_invitation">;
type AccountClass = "b2c" | "b2b" | "service";

interface SourceRule {
  rule_id: string;
  rule_digest: string;
  scope_type: "provider" | "model";
  provider_id: string;
  canonical_model_id: string | null;
  pricing_mode: "track" | "discount";
  rule_origin: "managed";
  discount_bps: number | null;
  payable_multiplier_bp: number | null;
  track_eligible: boolean;
  retention_eligible: boolean;
  commission_eligible: boolean;
}

interface SourcePolicy {
  policy_id: string;
  owner_type: ManagedOwnerType;
  owner_id: string;
  product_id: string;
  replacement_locked: false;
  version: number;
  content_digest: string;
  rules: SourceRule[];
}

// Read-side rule view: the write schema (`PricingPolicyEditorRule`) accepts only `discount`,
// but historical policy versions may still store the retired `track` mode and stay readable.
export type ManagedPricingPolicyRuleView = Omit<PricingPolicyEditorRule, "pricingMode"> & {
  pricingMode: "track" | "discount";
};

export interface ManagedPricingPolicyView {
  policyId: string;
  ownerType: ManagedOwnerType;
  ownerId: string;
  productId: string;
  currentVersion: number;
  currentDigest: string;
  catalogGeneration: number;
  currentActorType: string;
  currentActorId: string | null;
  currentReason: string;
  currentCreatedAt: string;
  servicePurpose: string | null;
  serviceResponsible: string | null;
  rules: ManagedPricingPolicyRuleView[];
  targets: Array<{
    bindingId: string;
    accountId: string;
    accountClass: AccountClass;
    desiredVersion: number | null;
    appliedVersion: number | null;
    syncState: "legacy" | "pending" | "confirmed" | "failed";
    deliveryState: "pending" | "processing" | "retry" | "confirmed" | "superseded" | "dead" | "missing";
    lastError: string | null;
  }>;
}

export interface ManagedPricingCatalogView {
  productId: string;
  catalogGeneration: number;
  switchGeneration: number;
  switchDigest: string;
  switchSyncState: "pending" | "processing" | "retry" | "confirmed" | "superseded" | "dead" | "missing";
  switchLastError: string | null;
  providers: Array<{
    providerId: string;
    masterEnabled: boolean;
    productEnabled: boolean;
    b2cEnabled: boolean;
    b2bEnabled: boolean;
    models: string[];
  }>;
}

export class PricingPolicyWriteError extends Error {
  constructor(
    public readonly code:
      | "foundation_missing"
      | "policy_not_found"
      | "version_conflict"
      | "invalid_owner_rule"
      | "rule_outside_catalog"
      | "invitation_not_editable"
      | "provisioning_policy_missing"
      | "release_cycle_required",
    message: string,
  ) {
    super(message);
    this.name = "PricingPolicyWriteError";
  }
}

/** SERIALIZABLE conflicts and deadlocks are concurrency facts, not failures: retry next tick. */
export function isSerializationConflictV2(message: string): boolean {
  return /could not serialize|deadlock detected/i.test(message);
}

export class PricingPolicyDeliveryRepairError extends Error {
  constructor(
    public readonly code:
      | "repair_job_not_found"
      | "repair_job_changed"
      | "repair_not_eligible"
      | "repair_precondition_changed",
    message: string,
  ) {
    super(message);
    this.name = "PricingPolicyDeliveryRepairError";
  }
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function sourceRuleKey(rule: SourceRule): readonly string[] {
  return [rule.provider_id, rule.scope_type, rule.canonical_model_id ?? "", rule.rule_id];
}

function compareKeys(left: readonly string[], right: readonly string[]): number {
  for (let index = 0; index < left.length; index += 1) {
    const compared = compareUtf8(left[index]!, right[index]!);
    if (compared !== 0) return compared;
  }
  return 0;
}

function scopeParts(rule: PricingPolicyEditorRule): {
  scopeType: "provider" | "model";
  providerId: string;
  canonicalModelId: string | null;
} {
  return "provider" in rule.scope
    ? { scopeType: "provider", providerId: rule.scope.provider.providerId, canonicalModelId: null }
    : {
        scopeType: "model",
        providerId: rule.scope.model.providerId,
        canonicalModelId: rule.scope.model.canonicalModelId,
      };
}

function editorRuleFromSource(rule: SourceRule): ManagedPricingPolicyRuleView {
  return {
    scope: rule.scope_type === "provider"
      ? { provider: { providerId: rule.provider_id } }
      : { model: { providerId: rule.provider_id, canonicalModelId: rule.canonical_model_id! } },
    pricingMode: rule.pricing_mode,
    discountBps: rule.discount_bps,
  };
}

function ruleId(input: PricingPolicyEditorRule): string {
  const scope = scopeParts(input);
  const target = scope.scopeType === "provider"
    ? `provider:${scope.providerId}`
    : `model:${scope.providerId}:${scope.canonicalModelId}`;
  return `${target}:${input.pricingMode}`;
}

function buildSourceRule(input: PricingPolicyEditorRule): SourceRule {
  // The editor schema accepts only static discount rules; the retired `track` mode is refused
  // at the schema boundary for every owner type before this builder runs.
  const scope = scopeParts(input);
  const base = {
    rule_id: ruleId(input),
    scope_type: scope.scopeType,
    provider_id: scope.providerId,
    canonical_model_id: scope.canonicalModelId,
    pricing_mode: input.pricingMode,
    rule_origin: "managed" as const,
    discount_bps: input.discountBps,
    payable_multiplier_bp: 10_000 - input.discountBps!,
    track_eligible: false,
    retention_eligible: false,
    commission_eligible: false,
  };
  return { ...base, rule_digest: stage5Digest("source-rule", base) };
}

function buildSourcePolicy(input: {
  policyId: string;
  ownerType: ManagedOwnerType;
  ownerId: string;
  productId: string;
  version: number;
  rules: readonly PricingPolicyEditorRule[];
}): SourcePolicy {
  const parsedRules = pricingPolicyEditorRulesSchema.parse(input.rules);
  const rules = parsedRules.map((rule) => buildSourceRule(rule))
    .sort((left, right) => compareKeys(sourceRuleKey(left), sourceRuleKey(right)));
  const base = {
    policy_id: input.policyId,
    owner_type: input.ownerType,
    owner_id: input.ownerId,
    product_id: input.productId,
    replacement_locked: false as const,
    version: input.version,
    rules,
  };
  return { ...base, content_digest: stage5Digest("source-policy", base) };
}

function positiveVersion(value: string, label: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0 || String(parsed) !== value) {
    throw new Error(`${label} is not a positive safe integer`);
  }
  return parsed;
}

async function activeCatalog(client: PoolClient, productId: string): Promise<{
  generation: number;
  providers: Set<string>;
  models: Set<string>;
}> {
  const head = await client.query<{ active_generation: string }>(`
    SELECT active_generation::text
    FROM product_catalog_heads
    WHERE product_id = $1
    FOR SHARE
  `, [productId]);
  if (!head.rows[0]) {
    throw new PricingPolicyWriteError("foundation_missing", `product catalog ${productId} is not materialized`);
  }
  const generation = positiveVersion(head.rows[0].active_generation, "catalog generation");
  const entries = await client.query<{ provider_id: string; canonical_model_id: string }>(`
    SELECT provider_id, canonical_model_id
    FROM product_catalog_entries
    WHERE product_id = $1 AND generation = $2 AND enabled
  `, [productId, generation]);
  return {
    generation,
    providers: new Set(entries.rows.map((entry) => entry.provider_id)),
    models: new Set(entries.rows.map((entry) => `${entry.provider_id}\0${entry.canonical_model_id}`)),
  };
}

function validateRulesAgainstCatalog(
  rules: readonly PricingPolicyEditorRule[],
  catalog: { providers: Set<string>; models: Set<string> },
): void {
  for (const rule of rules) {
    const scope = scopeParts(rule);
    const exists = scope.scopeType === "provider"
      ? catalog.providers.has(scope.providerId)
      : catalog.models.has(`${scope.providerId}\0${scope.canonicalModelId}`);
    if (!exists) {
      throw new PricingPolicyWriteError(
        "rule_outside_catalog",
        `rule scope ${scope.providerId}/${scope.canonicalModelId ?? "*"} is outside the active product catalog`,
      );
    }
  }
}

async function storeSourcePolicyVersion(
  client: PoolClient,
  policy: SourcePolicy,
  catalogGeneration: number,
  actorId: string,
  reason: string,
): Promise<void> {
  await client.query(`
    INSERT INTO pricing_policy_versions (
      policy_id, version, schema_version, product_id, catalog_generation,
      content_digest, actor_type, actor_id, reason
    ) VALUES ($1, $2, $3, $4, $5, $6, 'admin', $7, $8)
  `, [
    policy.policy_id,
    policy.version,
    MULTI_DISCOUNT_SCHEMA_VERSION,
    policy.product_id,
    catalogGeneration,
    policy.content_digest,
    actorId,
    reason,
  ]);
  for (const rule of policy.rules) {
    await client.query(`
      INSERT INTO pricing_policy_rules (
        policy_id, policy_version, product_id, catalog_generation,
        rule_id, rule_digest, scope_type, provider_id, canonical_model_id,
        pricing_mode, rule_origin, discount_bps, payable_multiplier_bp,
        track_eligible, retention_eligible, commission_eligible
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9,
                $10, $11, $12, $13, $14, $15, $16)
    `, [
      policy.policy_id,
      policy.version,
      policy.product_id,
      catalogGeneration,
      rule.rule_id,
      rule.rule_digest,
      rule.scope_type,
      rule.provider_id,
      rule.canonical_model_id,
      rule.pricing_mode,
      rule.rule_origin,
      rule.discount_bps,
      rule.payable_multiplier_bp,
      rule.track_eligible,
      rule.retention_eligible,
      rule.commission_eligible,
    ]);
  }
}

async function sourcePolicyById(
  client: PoolClient,
  policyId: string,
  lock: "none" | "share" | "update" = "share",
): Promise<{
  policy: SourcePolicy;
  catalogGeneration: number;
}> {
  const result = await client.query<{
    policy_id: string;
    owner_type: ManagedOwnerType;
    owner_id: string;
    product_id: string;
    replacement_locked: boolean;
    current_version: string;
    current_digest: string;
    catalog_generation: string;
  }>(`
    SELECT policy.id AS policy_id, policy.owner_type, policy.owner_id, policy.product_id,
           policy.replacement_locked, head.current_version::text, head.current_digest,
           version.catalog_generation::text
    FROM pricing_policies policy
    JOIN pricing_policy_heads head ON head.policy_id = policy.id
    JOIN pricing_policy_versions version
      ON version.policy_id = head.policy_id AND version.version = head.current_version
    WHERE policy.id = $1 AND policy.status = 'active'
    ${lock === "none" ? "" : `FOR ${lock === "update" ? "UPDATE" : "SHARE"} OF policy, head`}
  `, [policyId]);
  const row = result.rows[0];
  if (!row) throw new PricingPolicyWriteError("policy_not_found", `pricing policy ${policyId} was not found`);
  if (row.replacement_locked) throw new PricingPolicyWriteError("policy_not_found", `pricing policy ${policyId} is immutable`);
  const version = positiveVersion(row.current_version, "source policy version");
  const rules = await client.query<SourceRule>(`
    SELECT rule_id, rule_digest, scope_type, provider_id, canonical_model_id,
           pricing_mode, rule_origin, discount_bps, payable_multiplier_bp,
           track_eligible, retention_eligible, commission_eligible
    FROM pricing_policy_rules
    WHERE policy_id = $1 AND policy_version = $2
    ORDER BY provider_id COLLATE "C", scope_type COLLATE "C",
             COALESCE(canonical_model_id, '') COLLATE "C", rule_id COLLATE "C"
  `, [policyId, version]);
  return {
    policy: {
      policy_id: row.policy_id,
      owner_type: row.owner_type,
      owner_id: row.owner_id,
      product_id: row.product_id,
      replacement_locked: false,
      version,
      content_digest: row.current_digest,
      rules: rules.rows,
    },
    catalogGeneration: positiveVersion(row.catalog_generation, "source policy catalog generation"),
  };
}

async function activeSwitchGeneration(client: PoolClient): Promise<number> {
  const result = await client.query<{ active_generation: string }>(`
    SELECT active_generation::text FROM provider_switch_head WHERE singleton = 1 FOR SHARE
  `);
  if (!result.rows[0]) {
    throw new PricingPolicyWriteError("foundation_missing", "provider switches are not materialized");
  }
  return positiveVersion(result.rows[0].active_generation, "switch generation");
}

function effectiveRule(source: SourceRule, multiplierBp: number): AccountPolicySpec["rules"][number] {
  const base = {
    rule_id: source.rule_id,
    scope: source.scope_type === "provider"
      ? { provider: { provider_id: source.provider_id } }
      : { model: { provider_id: source.provider_id, canonical_model_id: source.canonical_model_id! } },
    pricing_mode: source.pricing_mode,
    rule_origin: source.rule_origin,
    discount_bps: source.discount_bps,
    payable_multiplier_bp: source.pricing_mode === "track" ? multiplierBp : source.payable_multiplier_bp!,
    track_eligible: source.track_eligible,
    retention_eligible: source.retention_eligible,
    commission_eligible: source.commission_eligible,
  };
  return { ...base, rule_digest: stage5Digest("effective-rule", base) };
}

function effectiveRuleKey(rule: AccountPolicySpec["rules"][number]): readonly string[] {
  return "provider" in rule.scope
    ? [rule.scope.provider.provider_id, "provider", "", rule.rule_id]
    : [rule.scope.model.provider_id, "model", rule.scope.model.canonical_model_id, rule.rule_id];
}

async function materializeBinding(
  client: PoolClient,
  bindingId: string,
  source: SourcePolicy,
  catalogGeneration: number,
): Promise<{ effectiveVersion: number; digest: string; jobId: string }> {
  const bindingResult = await client.query<{
    id: string;
    user_id: string | null;
    engine_account_id: string;
    account_class: AccountClass;
    product_id: string;
    policy_id: string;
    policy_enforcement: AccountPolicyBinding["policy_enforcement"];
    funding_enforcement: AccountPolicyBinding["funding_enforcement"];
    reconciliation_state: AccountPolicyBinding["reconciliation_state"];
    multiplier_bp: number | null;
  }>(`
    SELECT binding.id::text, binding.user_id::text, binding.engine_account_id,
           binding.account_class, binding.product_id, binding.policy_id,
           binding.policy_enforcement, binding.funding_enforcement,
           binding.reconciliation_state, profile.multiplier_bp
    FROM account_policy_bindings binding
    LEFT JOIN customer_profiles profile ON profile.user_id = binding.user_id
    WHERE binding.id = $1
    FOR UPDATE OF binding
  `, [bindingId]);
  const bindingRow = bindingResult.rows[0];
  if (!bindingRow || bindingRow.policy_id !== source.policy_id || bindingRow.product_id !== source.product_id) {
    throw new PricingPolicyWriteError("policy_not_found", `policy binding ${bindingId} does not match its source policy`);
  }
  const multiplierBp = bindingRow.account_class === "b2c"
    ? bindingRow.multiplier_bp
    : 10_000;
  if (multiplierBp === null || multiplierBp < 0 || multiplierBp > 10_000) {
    throw new PricingPolicyWriteError("provisioning_policy_missing", `binding ${bindingId} has no valid effective multiplier`);
  }
  const switchGeneration = await activeSwitchGeneration(client);
  const current = await client.query<{ maximum: string }>(`
    SELECT COALESCE(max(effective_version), 0)::text AS maximum
    FROM account_policy_versions
    WHERE binding_id = $1
  `, [bindingId]);
  const maximum = Number(current.rows[0]?.maximum ?? "0");
  if (!Number.isSafeInteger(maximum) || maximum < 0) throw new Error("effective policy version is malformed");
  const effectiveVersion = maximum + 1;
  const rules = source.rules.map((rule) => effectiveRule(rule, multiplierBp))
    .sort((left, right) => compareKeys(effectiveRuleKey(left), effectiveRuleKey(right)));
  const ownerType = source.owner_type as MaterializedOwnerType;
  const base = {
    account_id: bindingRow.engine_account_id,
    effective_version: effectiveVersion,
    policy_id: source.policy_id,
    policy_version: source.version,
    source_policy_digest: source.content_digest,
    owner_type: ownerType,
    owner_id: source.owner_id,
    account_class: bindingRow.account_class,
    product_id: source.product_id,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    catalog_generation: catalogGeneration,
    switch_generation: switchGeneration,
    replacement_locked: false,
    rules,
  };
  const policy = accountPolicySpecSchema.parse({
    ...base,
    content_digest: stage5Digest("effective-policy", base),
  });
  const binding = accountPolicyBindingSchema.parse({
    // Before the global release head exists, the scalar path remains authoritative and a
    // prepared account policy is shadow-only. Reconciliation evidence by itself must never
    // manufacture the invalid strict-policy + legacy-funding combination: strictness advances
    // only through the atomic release-v2 binding after funding has advanced with it.
    policy_enforcement: bindingRow.policy_enforcement === "legacy_scalar"
      ? "shadow"
      : bindingRow.policy_enforcement,
    funding_enforcement: bindingRow.funding_enforcement,
    reconciliation_state: bindingRow.reconciliation_state,
  });
  await client.query(`
    INSERT INTO account_policy_versions (
      binding_id, effective_version, policy_id, policy_version, policy_digest,
      product_id, account_class, schema_version, catalog_generation,
      switch_generation, content_digest, replacement_locked
    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, false)
  `, [
    bindingId,
    policy.effective_version,
    policy.policy_id,
    policy.policy_version,
    policy.source_policy_digest,
    policy.product_id,
    policy.account_class,
    policy.schema_version,
    policy.catalog_generation,
    policy.switch_generation,
    policy.content_digest,
  ]);
  for (const rule of policy.rules) {
    const scope = "provider" in rule.scope
      ? { type: "provider", provider: rule.scope.provider.provider_id, model: null }
      : {
          type: "model",
          provider: rule.scope.model.provider_id,
          model: rule.scope.model.canonical_model_id,
        };
    await client.query(`
      INSERT INTO account_policy_rules (
        binding_id, effective_version, product_id, catalog_generation,
        rule_id, rule_digest, scope_type, provider_id, canonical_model_id,
        pricing_mode, rule_origin, discount_bps, payable_multiplier_bp,
        track_eligible, retention_eligible, commission_eligible
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9,
                $10, $11, $12, $13, $14, $15, $16)
    `, [
      bindingId,
      policy.effective_version,
      policy.product_id,
      policy.catalog_generation,
      rule.rule_id,
      rule.rule_digest,
      scope.type,
      scope.provider,
      scope.model,
      rule.pricing_mode,
      rule.rule_origin,
      rule.discount_bps,
      rule.payable_multiplier_bp,
      rule.track_eligible,
      rule.retention_eligible,
      rule.commission_eligible,
    ]);
  }
  await client.query(`
    UPDATE account_policy_bindings
    SET desired_effective_version = $2, desired_digest = $3,
        sync_state = CASE
          WHEN applied_effective_version = $2 AND applied_digest = $3 THEN 'confirmed'
          ELSE 'pending'
        END,
        last_error = NULL, updated_at = now()
    WHERE id = $1
  `, [bindingId, policy.effective_version, policy.content_digest]);
  const jobId = randomUUID();
  await client.query(`
    INSERT INTO engine_policy_jobs (
      id, binding_id, effective_version, engine_account_id, policy_id,
      policy_version, catalog_generation, switch_generation, schema_version,
      content_digest, payload
    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::jsonb)
  `, [
    jobId,
    bindingId,
    policy.effective_version,
    policy.account_id,
    policy.policy_id,
    policy.policy_version,
    policy.catalog_generation,
    policy.switch_generation,
    policy.schema_version,
    policy.content_digest,
    JSON.stringify({ policy, binding }),
  ]);
  return { effectiveVersion, digest: policy.content_digest, jobId };
}

interface DeadPolicyDeliveryRepairRow {
  job_id: string;
  job_status: "pending" | "processing" | "retry" | "confirmed" | "superseded" | "dead";
  effective_version: string;
  engine_account_id: string;
  policy_id: string;
  policy_version: string;
  catalog_generation: string;
  switch_generation: string;
  schema_version: string;
  content_digest: string;
  payload: unknown;
  binding_id: string;
  binding_engine_account_id: string;
  binding_policy_id: string;
  policy_enforcement: AccountPolicyBinding["policy_enforcement"];
  funding_enforcement: AccountPolicyBinding["funding_enforcement"];
  reconciliation_state: AccountPolicyBinding["reconciliation_state"];
  sync_state: "legacy" | "pending" | "confirmed" | "failed";
  desired_effective_version: string | null;
  desired_digest: string | null;
  applied_effective_version: string | null;
  applied_digest: string | null;
}

async function replayedPolicyDeliveryRepair(
  client: PoolClient,
  row: DeadPolicyDeliveryRepairRow,
): Promise<PricingPolicyDeliveryRepairResponseV2 | null> {
  const replay = await client.query<{
    replacement_job_id: string;
    replacement_effective_version: string;
    replacement_content_digest: string;
  }>(`
    SELECT replacement.id::text AS replacement_job_id,
           replacement.effective_version::text AS replacement_effective_version,
           replacement.content_digest AS replacement_content_digest
    FROM audit_log audit
    JOIN engine_policy_jobs replacement
      ON replacement.id::text = audit.metadata->>'replacementJobId'
    WHERE audit.action = 'pricing.policy_delivery.compatibility_repaired'
      AND audit.target_type = 'engine_policy_job'
      AND audit.target_id = $1
      AND audit.metadata->>'supersededJobId' = $1
      AND audit.metadata->>'bindingId' = $2
      AND audit.metadata->>'engineAccountId' = $3
      AND audit.metadata->>'previousEffectiveVersion' = $4
      AND audit.metadata->>'previousContentDigest' = $5
    ORDER BY audit.created_at DESC, audit.id DESC
    LIMIT 1
  `, [
    row.job_id,
    row.binding_id,
    row.engine_account_id,
    row.effective_version,
    row.content_digest,
  ]);
  const stored = replay.rows[0];
  if (!stored) return null;
  return {
    status: "unchanged",
    superseded_job_id: row.job_id,
    replacement_job_id: stored.replacement_job_id,
    binding_id: row.binding_id,
    engine_account_id: row.engine_account_id,
    previous_effective_version: positiveVersion(row.effective_version, "previous effective version"),
    replacement_effective_version: positiveVersion(
      stored.replacement_effective_version,
      "replacement effective version",
    ),
    replacement_content_digest: stored.replacement_content_digest,
  };
}

/**
 * Rebuild one exact terminal pre-cutover delivery that was produced before the
 * strict-policy + legacy-funding guard existed. Immutable policy/job history is retained: the
 * invalid job becomes superseded and a newer shadow-policy delivery is queued atomically.
 */
export async function repairDeadPreCutoverPolicyDelivery(
  database: Database,
  input: {
    jobId: string;
    expectedEffectiveVersion: number;
    expectedContentDigest: string;
    actorId: string;
    reason: string;
  },
): Promise<PricingPolicyDeliveryRepairResponseV2> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    const result = await client.query<DeadPolicyDeliveryRepairRow>(`
      SELECT job.id::text AS job_id, job.status AS job_status,
             job.effective_version::text, job.engine_account_id, job.policy_id,
             job.policy_version::text, job.catalog_generation::text,
             job.switch_generation::text, job.schema_version::text,
             job.content_digest, job.payload,
             binding.id::text AS binding_id,
             binding.engine_account_id AS binding_engine_account_id,
             binding.policy_id AS binding_policy_id,
             binding.policy_enforcement, binding.funding_enforcement,
             binding.reconciliation_state, binding.sync_state,
             binding.desired_effective_version::text,
             binding.desired_digest,
             binding.applied_effective_version::text,
             binding.applied_digest
      FROM engine_policy_jobs job
      JOIN account_policy_bindings binding ON binding.id = job.binding_id
      WHERE job.id = $1
      FOR UPDATE OF job, binding
    `, [input.jobId]);
    const row = result.rows[0];
    if (!row) {
      throw new PricingPolicyDeliveryRepairError(
        "repair_job_not_found",
        `pricing policy delivery ${input.jobId} was not found`,
      );
    }
    const effectiveVersion = positiveVersion(row.effective_version, "effective version");
    if (
      effectiveVersion !== input.expectedEffectiveVersion
      || row.content_digest !== input.expectedContentDigest
    ) {
      throw new PricingPolicyDeliveryRepairError(
        "repair_job_changed",
        `pricing policy delivery ${input.jobId} does not match the expected immutable identity`,
      );
    }

    if (row.job_status === "superseded") {
      const replay = await replayedPolicyDeliveryRepair(client, row);
      if (!replay) {
        throw new PricingPolicyDeliveryRepairError(
          "repair_precondition_changed",
          `pricing policy delivery ${input.jobId} was superseded outside this repair`,
        );
      }
      await client.query("COMMIT");
      return replay;
    }
    if (row.job_status !== "dead") {
      throw new PricingPolicyDeliveryRepairError(
        "repair_not_eligible",
        `pricing policy delivery ${input.jobId} is ${row.job_status}, not dead`,
      );
    }

    const rawPayload = row.payload as { policy?: unknown; binding?: unknown } | null;
    const policy = accountPolicySpecSchema.safeParse(rawPayload?.policy);
    const rejectedBinding = accountPolicyBindingSchema.safeParse(rawPayload?.binding);
    const exactRejectedIdentity = policy.success
      && rejectedBinding.success
      && policy.data.account_id === row.engine_account_id
      && policy.data.effective_version === effectiveVersion
      && policy.data.policy_id === row.policy_id
      && policy.data.policy_version === positiveVersion(row.policy_version, "policy version")
      && policy.data.catalog_generation === positiveVersion(row.catalog_generation, "catalog generation")
      && policy.data.switch_generation === positiveVersion(row.switch_generation, "switch generation")
      && policy.data.schema_version === positiveVersion(row.schema_version, "schema version")
      && policy.data.content_digest === row.content_digest
      && rejectedBinding.data.policy_enforcement === "strict"
      && rejectedBinding.data.funding_enforcement === "legacy_single"
      && rejectedBinding.data.reconciliation_state === "verified";
    if (!exactRejectedIdentity) {
      throw new PricingPolicyDeliveryRepairError(
        "repair_not_eligible",
        `pricing policy delivery ${input.jobId} is not the historical strict + legacy_single incompatibility`,
      );
    }

    const currentBindingIsExact = row.binding_id.length > 0
      && row.binding_engine_account_id === row.engine_account_id
      && row.binding_policy_id === row.policy_id
      && row.policy_enforcement === "legacy_scalar"
      && row.funding_enforcement === "legacy_single"
      && row.reconciliation_state === "verified"
      && row.sync_state === "failed"
      && row.desired_effective_version === row.effective_version
      && row.desired_digest === row.content_digest
      && row.applied_effective_version === null
      && row.applied_digest === null;
    if (!currentBindingIsExact) {
      throw new PricingPolicyDeliveryRepairError(
        "repair_precondition_changed",
        `pricing policy binding ${row.binding_id} changed after the terminal delivery`,
      );
    }

    const source = await sourcePolicyById(client, row.policy_id, "share");
    if (
      source.policy.version !== policy.data.policy_version
      || source.policy.content_digest !== policy.data.source_policy_digest
      || source.catalogGeneration !== policy.data.catalog_generation
    ) {
      throw new PricingPolicyDeliveryRepairError(
        "repair_precondition_changed",
        `source policy ${row.policy_id} changed after the terminal delivery`,
      );
    }

    const replacement = await materializeBinding(
      client,
      row.binding_id,
      source.policy,
      source.catalogGeneration,
    );
    const superseded = await client.query(`
      UPDATE engine_policy_jobs
      SET status = 'superseded',
          last_error = 'superseded by audited pre-cutover compatibility repair',
          updated_at = now()
      WHERE id = $1 AND status = 'dead'
    `, [row.job_id]);
    if ((superseded.rowCount ?? 0) !== 1) {
      throw new PricingPolicyDeliveryRepairError(
        "repair_precondition_changed",
        `pricing policy delivery ${input.jobId} changed during repair`,
      );
    }
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'pricing.policy_delivery.compatibility_repaired',
              'engine_policy_job', $2, $3::jsonb)
    `, [input.actorId, row.job_id, JSON.stringify({
      supersededJobId: row.job_id,
      replacementJobId: replacement.jobId,
      bindingId: row.binding_id,
      engineAccountId: row.engine_account_id,
      previousEffectiveVersion: effectiveVersion,
      previousContentDigest: row.content_digest,
      replacementEffectiveVersion: replacement.effectiveVersion,
      replacementContentDigest: replacement.digest,
      reason: input.reason,
    })]);
    const response: PricingPolicyDeliveryRepairResponseV2 = {
      status: "queued",
      superseded_job_id: row.job_id,
      replacement_job_id: replacement.jobId,
      binding_id: row.binding_id,
      engine_account_id: row.engine_account_id,
      previous_effective_version: effectiveVersion,
      replacement_effective_version: replacement.effectiveVersion,
      replacement_content_digest: replacement.digest,
    };
    await client.query("COMMIT");
    return response;
  } catch (error) {
    await client.query("ROLLBACK");
    if (
      error instanceof PricingPolicyDeliveryRepairError
      || !(error instanceof Error)
    ) {
      throw error;
    }
    if ((error as Error & { code?: string }).code === "40001") {
      throw new PricingPolicyDeliveryRepairError(
        "repair_precondition_changed",
        "pricing policy delivery changed concurrently; read it again before repair",
      );
    }
    throw error;
  } finally {
    client.release();
  }
}

async function createPolicyIdentityAndVersion(client: PoolClient, input: {
  policyId: string;
  ownerType: ManagedOwnerType;
  ownerId: string;
  productId: string;
  rules: readonly PricingPolicyEditorRule[];
  actorId: string;
  reason: string;
}): Promise<SourcePolicy> {
  const catalog = await activeCatalog(client, input.productId);
  validateRulesAgainstCatalog(input.rules, catalog);
  const policy = buildSourcePolicy({ ...input, version: 1 });
  await client.query(`
    INSERT INTO pricing_policies (id, owner_type, owner_id, product_id, replacement_locked, status)
    VALUES ($1, $2, $3, $4, false, 'active')
  `, [policy.policy_id, policy.owner_type, policy.owner_id, policy.product_id]);
  await storeSourcePolicyVersion(client, policy, catalog.generation, input.actorId, input.reason);
  await client.query(`
    INSERT INTO pricing_policy_heads (policy_id, current_version, current_digest)
    VALUES ($1, 1, $2)
  `, [policy.policy_id, policy.content_digest]);
  return policy;
}

export async function createBusinessInvitationPolicy(
  client: PoolClient,
  input: {
    inviteId: string;
    rules: readonly PricingPolicyEditorRule[];
    actorId: string;
    reason: string;
  },
): Promise<SourcePolicy> {
  const policyId = `policy:main:invite:${input.inviteId}`;
  const existing = await client.query<{ invitation_policy_id: string }>(`
    SELECT invitation_policy_id FROM business_invite_policy_bindings WHERE invite_id = $1 FOR UPDATE
  `, [input.inviteId]);
  if (existing.rows[0]) {
    const stored = await sourcePolicyById(client, existing.rows[0].invitation_policy_id, "update");
    const expected = buildSourcePolicy({
      policyId,
      ownerType: "b2b_invitation",
      ownerId: input.inviteId,
      productId: MAIN_PRICING_PRODUCT_ID,
      version: stored.policy.version,
      rules: input.rules,
    });
    if (stored.policy.content_digest !== expected.content_digest) {
      throw new PricingPolicyWriteError("version_conflict", "invitation idempotency key already has another policy");
    }
    return stored.policy;
  }
  const policy = await createPolicyIdentityAndVersion(client, {
    policyId,
    ownerType: "b2b_invitation",
    ownerId: input.inviteId,
    productId: MAIN_PRICING_PRODUCT_ID,
    rules: input.rules,
    actorId: input.actorId,
    reason: input.reason,
  });
  await client.query(`
    INSERT INTO business_invite_policy_bindings (
      invite_id, invitation_policy_id, current_policy_version, current_policy_digest
    ) VALUES ($1, $2, $3, $4)
  `, [input.inviteId, policy.policy_id, policy.version, policy.content_digest]);
  return policy;
}

/**
 * Enqueues (or re-arms) the durable legacy-scalar delivery job for an account. The scalar on
 * customer_profiles is the desired state; this job carries it to the engine exactly once per
 * change. Shared by the commerce pricing flows and the managed-policy writer.
 */
export async function enqueuePricingJob(client: PoolClient, input: {
  userId: string; engineAccountId: string; multiplierBp: number; reason: string;
}): Promise<string> {
  const existing = await client.query<{ id: string; status: "pending" | "processing" | "retry" | "confirmed" }>(`
    SELECT id, status FROM engine_pricing_jobs WHERE user_id = $1 FOR UPDATE
  `, [input.userId]);
  const row = existing.rows[0];
  if (row?.status === "processing") {
    // Do not revoke an active lease or let a newer generation run concurrently. The desired
    // multiplier is already durable on customer_profiles; confirmPricingJob requeues this row
    // after the in-flight engine request completes if that desired value changed.
    return row.id;
  }
  if (row) {
    const updated = await client.query<{ id: string }>(`
      UPDATE engine_pricing_jobs SET
        engine_account_id = $2, multiplier_bp = $3, reason = $4,
        status = 'pending', attempts = 0, next_attempt_at = now(),
        locked_at = NULL, locked_by = NULL, last_error = NULL,
        confirmed_at = NULL, updated_at = now()
      WHERE id = $1
      RETURNING id
    `, [row.id, input.engineAccountId, input.multiplierBp, input.reason]);
    return updated.rows[0]!.id;
  }
  const id = randomUUID();
  await client.query(`
    INSERT INTO engine_pricing_jobs (
      id, user_id, engine_account_id, multiplier_bp, reason
    ) VALUES ($1, $2, $3, $4, $5)
  `, [id, input.userId, input.engineAccountId, input.multiplierBp, input.reason]);
  return id;
}

/**
 * A saved b2b_client policy whose rules are a uniform set of provider-level discounts expresses
 * exactly one payable multiplier — and pre-cutover the legacy scalar is the ONLY price the
 * engine enforces (the policy itself ships as shadow). Keeping the two apart made policy edits
 * silently ineffective: the panel showed the new policy while billing followed the stale scalar.
 * Returns the multiplier when the rules are uniform, or null when they cannot be represented as
 * one scalar (mixed scopes, track rules, per-provider differences) — in that case the scalar
 * stays untouched and the custom policy activates with the release cutover.
 */
function uniformProviderDiscountMultiplier(rules: readonly PricingPolicyEditorRule[]): number | null {
  if (rules.length === 0) return null;
  let discountBps: number | null = null;
  for (const rule of rules) {
    if (!("provider" in rule.scope) || rule.pricingMode !== "discount" || rule.discountBps === null) {
      return null;
    }
    if (discountBps === null) discountBps = rule.discountBps;
    else if (discountBps !== rule.discountBps) return null;
  }
  return 10_000 - discountBps!;
}

/**
 * Syncs the authoritative legacy scalar (customer_profiles + engine_accounts + the durable
 * engine delivery job) with a uniform b2b_client policy. A no-op when the scalar already
 * matches, so re-saving an unchanged policy never churns the engine job.
 */
async function syncBusinessClientScalar(
  client: PoolClient,
  input: { userId: string; multiplierBp: number; reason: string },
): Promise<{ synced: boolean; jobId: string | null; previousMultiplierBp: number | null }> {
  const account = await client.query<{
    multiplier_bp: number;
    engine_account_id: string | null;
  }>(`
    SELECT cp.multiplier_bp, ea.engine_account_id
    FROM customer_profiles cp
    JOIN engine_accounts ea ON ea.user_id = cp.user_id
    WHERE cp.user_id = $1
    FOR UPDATE OF cp, ea
  `, [input.userId]);
  const row = account.rows[0];
  if (!row || row.engine_account_id === null || row.multiplier_bp === input.multiplierBp) {
    return { synced: false, jobId: null, previousMultiplierBp: row?.multiplier_bp ?? null };
  }
  await client.query(`
    UPDATE customer_profiles SET multiplier_bp = $2, updated_at = now() WHERE user_id = $1
  `, [input.userId, input.multiplierBp]);
  await client.query(`
    UPDATE engine_accounts SET mult_bp = $2, updated_at = now() WHERE user_id = $1
  `, [input.userId, input.multiplierBp]);
  const jobId = await enqueuePricingJob(client, {
    userId: input.userId,
    engineAccountId: row.engine_account_id,
    multiplierBp: input.multiplierBp,
    reason: input.reason,
  });
  return { synced: true, jobId, previousMultiplierBp: row.multiplier_bp };
}

export async function updateManagedPricingPolicy(database: Database, input: {
  ownerType: ManagedOwnerType;
  ownerId: string;
  productId?: string;
  expectedVersion: number;
  rules: readonly PricingPolicyEditorRule[];
  actorId: string;
  reason: string;
}): Promise<ManagedPricingPolicyView> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    if (input.ownerType === "b2b_invitation") {
      const invitation = await client.query<{ id: string }>(`
        SELECT id::text FROM business_invites
        WHERE id = $1 AND consumed_at IS NULL AND revoked_at IS NULL
          AND superseded_by_invite_id IS NULL AND expires_at > transaction_timestamp()
        FOR UPDATE
      `, [input.ownerId]);
      if (!invitation.rows[0]) {
        throw new PricingPolicyWriteError("invitation_not_editable", "only an active unredeemed invitation can be edited");
      }
    }
    if ((input.ownerType === "global_b2c" || input.ownerType === "service")
      && (await pricingReleaseCutoverCompleted(client))) {
      // Post-cutover the active release pins the global B2C policy and the service meter_only
      // authority: a save here would version the commerce document and stage legacy-lane
      // deliveries that never move release-v2 billing, silently diverging the panel from
      // enforced prices. Refuse loudly instead.
      throw new PricingPolicyWriteError(
        "release_cycle_required",
        `the ${input.ownerType === "global_b2c" ? "global B2C" : "service"} policy is pinned by the active pricing release; changing it post-cutover requires a new release cycle`,
      );
    }
    const identity = await client.query<{ id: string }>(`
      SELECT id FROM pricing_policies
      WHERE owner_type = $1 AND owner_id = $2 AND product_id = $3 AND status = 'active'
      FOR UPDATE
    `, [input.ownerType, input.ownerId, input.productId ?? MAIN_PRICING_PRODUCT_ID]);
    const policyId = identity.rows[0]?.id;
    if (!policyId) throw new PricingPolicyWriteError("policy_not_found", "managed pricing policy was not found");
    const current = await sourcePolicyById(client, policyId, "update");
    if (current.policy.version !== input.expectedVersion) {
      throw new PricingPolicyWriteError(
        "version_conflict",
        `policy version changed from ${input.expectedVersion} to ${current.policy.version}`,
      );
    }
    const catalog = await activeCatalog(client, current.policy.product_id);
    validateRulesAgainstCatalog(input.rules, catalog);
    const next = buildSourcePolicy({
      policyId,
      ownerType: current.policy.owner_type,
      ownerId: current.policy.owner_id,
      productId: current.policy.product_id,
      version: current.policy.version + 1,
      rules: input.rules,
    });
    await storeSourcePolicyVersion(client, next, catalog.generation, input.actorId, input.reason);
    await client.query(`
      UPDATE pricing_policy_heads
      SET current_version = $2, current_digest = $3, updated_at = now()
      WHERE policy_id = $1
    `, [policyId, next.version, next.content_digest]);

    if (input.ownerType === "b2b_invitation") {
      await client.query(`
        UPDATE business_invite_policy_bindings
        SET current_policy_version = $2, current_policy_digest = $3, updated_at = now()
        WHERE invite_id = $1
      `, [input.ownerId, next.version, next.content_digest]);
    } else {
      const bindings = await client.query<{
        id: string;
        policy_enforcement: string;
        desired_effective_version: string | null;
        applied_effective_version: string | null;
      }>(`
        SELECT id::text, policy_enforcement,
               desired_effective_version::text AS desired_effective_version,
               applied_effective_version::text AS applied_effective_version
        FROM account_policy_bindings WHERE policy_id = $1 ORDER BY id FOR UPDATE
      `, [policyId]);
      for (const binding of bindings.rows) {
        const enginePolicyId = await engineRunPolicyId(client, binding);
        if (enginePolicyId !== null && enginePolicyId !== policyId && binding.policy_enforcement === "strict") {
          // A strict binding whose engine lineage runs a different policy identity cannot be
          // re-pointed by the legacy lane: the engine would reject the delivery with
          // version_conflict, so nothing is staged; instead any drifted desired state is
          // folded back to the engine-confirmed applied state. The identity switch for a
          // strict account ships via the release-cutover lane.
          await closeLegacyDeliveryDrift(client, binding.id);
          continue;
        }
        // A shadow lineage mismatch (e.g. a converted B2C customer whose lineage was created
        // by the Stage 5 backfill) is staged as a normal delivery: the engine accepts a
        // shadow rebind pre-cutover, so the new policy identity is delivered now.
        await materializeBinding(client, binding.id, next, catalog.generation);
      }
    }
    let scalarSync: { synced: boolean; jobId: string | null; previousMultiplierBp: number | null; multiplierBp: number } | null = null;
    if (input.ownerType === "b2b_client") {
      // While the account is pre-strict the legacy scalar stays the enforced bridge, so a
      // uniform provider-discount policy must move the scalar with it — otherwise the panel
      // shows the new policy while billing follows the stale multiplier. Non-uniform rules
      // cannot be one scalar; they are enforced by the strict chain below instead.
      const multiplierBp = uniformProviderDiscountMultiplier(input.rules);
      if (multiplierBp !== null) {
        const synced = await syncBusinessClientScalar(client, {
          userId: input.ownerId,
          multiplierBp,
          reason: `b2b_policy_scalar_sync:${policyId}`,
        });
        scalarSync = { ...synced, multiplierBp };
      }
      // Every b2b_client policy save chains the per-account strict enforcement
      // (docs/commerce/MULTI-DISCOUNT.md decision 14): an already-strict binding advances
      // strict→strict via the delivery staged above, and a non-strict binding is flagged so
      // the pricing worker sweep cuts it over as soon as this exact version confirms under
      // shadow. Older versions, the legacy scalar, and B2C rules never price new charges again.
      // Post-cutover the release-v2 authority owns pricing: the caller's extension-lane sync
      // (syncPricingReleasePolicyOverrideV2) pins the new version for the exact active head,
      // and no legacy chain is armed.
      if (!(await pricingReleaseCutoverCompleted(client))) {
        await client.query(`
          UPDATE account_policy_bindings
          SET strict_chain_pending = true, updated_at = now()
          WHERE policy_id = $1 AND policy_enforcement <> 'strict'
        `, [policyId]);
      }
    }
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'pricing_policy.updated', 'pricing_policy', $2, $3::jsonb)
    `, [input.actorId, policyId, JSON.stringify({
      ownerType: input.ownerType,
      ownerId: input.ownerId,
      previousVersion: current.policy.version,
      version: next.version,
      digest: next.content_digest,
      reason: input.reason,
      ...(scalarSync === null ? {} : {
        scalarSync: {
          synced: scalarSync.synced,
          multiplierBp: scalarSync.multiplierBp,
          previousMultiplierBp: scalarSync.previousMultiplierBp,
          jobId: scalarSync.jobId,
        },
      }),
    })]);
    const view = await managedPolicyView(client, policyId);
    await client.query("COMMIT");
    return view;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

async function managedPolicyView(client: PoolClient, policyId: string): Promise<ManagedPricingPolicyView> {
  const source = await sourcePolicyById(client, policyId, "none");
  const metadata = await client.query<{
    actor_type: string;
    actor_id: string | null;
    reason: string;
    created_at: Date;
  }>(`
    SELECT actor_type, actor_id, reason, created_at
    FROM pricing_policy_versions
    WHERE policy_id = $1 AND version = $2
  `, [policyId, source.policy.version]);
  const currentMetadata = metadata.rows[0];
  if (!currentMetadata) throw new Error(`pricing policy ${policyId} current version metadata is missing`);
  const serviceAssignment = source.policy.owner_type === "service"
    ? await client.query<{ purpose: string | null; responsible: string | null }>(`
        SELECT metadata->>'purpose' AS purpose, metadata->>'responsible' AS responsible
        FROM audit_log
        WHERE action = 'pricing.service_assignment.applied'
          AND target_type = 'pricing_policy' AND target_id = $1
        ORDER BY created_at DESC, id DESC
        LIMIT 1
      `, [policyId])
    : { rows: [] as Array<{ purpose: string | null; responsible: string | null }> };
  const targets = await client.query<{
    binding_id: string;
    engine_account_id: string;
    account_class: AccountClass;
    desired_effective_version: string | null;
    applied_effective_version: string | null;
    sync_state: "legacy" | "pending" | "confirmed" | "failed";
    delivery_state: "pending" | "processing" | "retry" | "confirmed" | "superseded" | "dead" | null;
    last_error: string | null;
  }>(`
    SELECT id::text AS binding_id, engine_account_id, account_class,
           desired_effective_version::text, applied_effective_version::text,
           binding.sync_state, COALESCE(job.last_error, binding.last_error) AS last_error,
           job.status AS delivery_state
    FROM account_policy_bindings binding
    LEFT JOIN LATERAL (
      SELECT status, last_error
      FROM engine_policy_jobs
      WHERE binding_id = binding.id
        AND effective_version = binding.desired_effective_version
      ORDER BY created_at DESC, id DESC
      LIMIT 1
    ) job ON TRUE
    WHERE binding.policy_id = $1
    ORDER BY binding.engine_account_id COLLATE "C"
  `, [policyId]);
  return {
    policyId: source.policy.policy_id,
    ownerType: source.policy.owner_type,
    ownerId: source.policy.owner_id,
    productId: source.policy.product_id,
    currentVersion: source.policy.version,
    currentDigest: source.policy.content_digest,
    catalogGeneration: source.catalogGeneration,
    currentActorType: currentMetadata.actor_type,
    currentActorId: currentMetadata.actor_id,
    currentReason: currentMetadata.reason,
    currentCreatedAt: currentMetadata.created_at.toISOString(),
    servicePurpose: serviceAssignment.rows[0]?.purpose ?? null,
    serviceResponsible: serviceAssignment.rows[0]?.responsible ?? null,
    rules: source.policy.rules.map(editorRuleFromSource),
    targets: targets.rows.map((target) => ({
      bindingId: target.binding_id,
      accountId: target.engine_account_id,
      accountClass: target.account_class,
      desiredVersion: target.desired_effective_version === null
        ? null
        : positiveVersion(target.desired_effective_version, "desired effective version"),
      appliedVersion: target.applied_effective_version === null
        ? null
        : positiveVersion(target.applied_effective_version, "applied effective version"),
      syncState: target.sync_state,
      deliveryState: target.delivery_state ?? "missing",
      lastError: target.last_error,
    })),
  };
}

export async function getManagedPricingPolicy(database: Database, input: {
  ownerType: ManagedOwnerType;
  ownerId: string;
  productId?: string;
}): Promise<ManagedPricingPolicyView | null> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    const identity = await client.query<{ id: string }>(`
      SELECT id FROM pricing_policies
      WHERE owner_type = $1 AND owner_id = $2 AND product_id = $3 AND status = 'active'
    `, [input.ownerType, input.ownerId, input.productId ?? MAIN_PRICING_PRODUCT_ID]);
    const view = identity.rows[0] ? await managedPolicyView(client, identity.rows[0].id) : null;
    await client.query("COMMIT");
    return view;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function listManagedServicePricingPolicies(
  database: Database,
): Promise<ManagedPricingPolicyView[]> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    const policies = await client.query<{ id: string }>(`
      SELECT id
      FROM pricing_policies
      WHERE owner_type = 'service' AND status = 'active'
      ORDER BY product_id COLLATE "C", owner_id COLLATE "C", id COLLATE "C"
    `);
    const views: ManagedPricingPolicyView[] = [];
    for (const policy of policies.rows) views.push(await managedPolicyView(client, policy.id));
    await client.query("COMMIT");
    return views;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

async function managedPricingCatalogView(client: PoolClient, productId: string): Promise<ManagedPricingCatalogView> {
  const head = await client.query<{
    catalog_generation: string;
    switch_generation: string;
    switch_digest: string;
    switch_status: ManagedPricingCatalogView["switchSyncState"] | null;
    switch_error: string | null;
  }>(`
    SELECT catalog.active_generation::text AS catalog_generation,
           switches.active_generation::text AS switch_generation,
           version.content_digest AS switch_digest,
           job.status AS switch_status, job.last_error AS switch_error
    FROM product_catalog_heads catalog
    CROSS JOIN provider_switch_head switches
    JOIN provider_switch_versions version ON version.generation = switches.active_generation
    LEFT JOIN engine_switch_jobs job ON job.generation = switches.active_generation
    WHERE catalog.product_id = $1 AND switches.singleton = 1
  `, [productId]);
  const header = head.rows[0];
  if (!header) throw new PricingPolicyWriteError("foundation_missing", "managed pricing catalog is not materialized");
  const catalogGeneration = positiveVersion(header.catalog_generation, "catalog generation");
  const switchGeneration = positiveVersion(header.switch_generation, "switch generation");
  const models = await client.query<{ provider_id: string; canonical_model_id: string }>(`
    SELECT provider_id, canonical_model_id
    FROM product_catalog_entries
    WHERE product_id = $1 AND generation = $2 AND enabled
    ORDER BY provider_id COLLATE "C", canonical_model_id COLLATE "C"
  `, [productId, catalogGeneration]);
  const switches = await client.query<{
    provider_id: string;
    scope_type: "master" | "product" | "segment";
    product_id: string;
    segment: string;
    enabled: boolean;
  }>(`
    SELECT provider_id, scope_type, product_id, segment, enabled
    FROM provider_switch_entries
    WHERE generation = $1
      AND (scope_type = 'master' OR product_id = $2)
    ORDER BY provider_id COLLATE "C", scope_type COLLATE "C", product_id COLLATE "C", segment COLLATE "C"
  `, [switchGeneration, productId]);
  const providerIds = [...new Set(models.rows.map((model) => model.provider_id))];
  return {
    productId,
    catalogGeneration,
    switchGeneration,
    switchDigest: header.switch_digest,
    switchSyncState: header.switch_status ?? "missing",
    switchLastError: header.switch_error,
    providers: providerIds.map((providerId) => {
      const providerSwitches = switches.rows.filter((entry) => entry.provider_id === providerId);
      const enabled = (scopeType: "master" | "product" | "segment", segment = ""): boolean =>
        providerSwitches.some((entry) =>
          entry.scope_type === scopeType
          && (scopeType === "master" || entry.product_id === productId)
          && entry.segment === segment
          && entry.enabled);
      return {
        providerId,
        masterEnabled: enabled("master"),
        productEnabled: enabled("product"),
        b2cEnabled: enabled("segment", "b2c"),
        b2bEnabled: enabled("segment", "b2b"),
        models: models.rows
          .filter((model) => model.provider_id === providerId)
          .map((model) => model.canonical_model_id),
      };
    }),
  };
}

export async function getManagedPricingCatalog(
  database: Database,
  productId = MAIN_PRICING_PRODUCT_ID,
): Promise<ManagedPricingCatalogView> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    const view = await managedPricingCatalogView(client, productId);
    await client.query("COMMIT");
    return view;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function updateManagedProviderSwitches(
  database: Database,
  input: ProviderSwitchEditorMutation & { actorId: string },
): Promise<ManagedPricingCatalogView> {
  const parsed = providerSwitchEditorMutationSchema.parse({
    expectedGeneration: input.expectedGeneration,
    reason: input.reason,
    providers: input.providers,
  });
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    const current = await client.query<{
      generation: string;
      schema_version: string;
      capability_generation: string;
      capability_digest: string;
    }>(`
      SELECT version.generation::text, version.schema_version::text,
             version.capability_generation::text, version.capability_digest
      FROM provider_switch_head head
      JOIN provider_switch_versions version ON version.generation = head.active_generation
      WHERE head.singleton = 1
      FOR UPDATE OF head
    `);
    const currentRow = current.rows[0];
    if (!currentRow) throw new PricingPolicyWriteError("foundation_missing", "provider switches are not materialized");
    const currentGeneration = positiveVersion(currentRow.generation, "switch generation");
    if (currentGeneration !== parsed.expectedGeneration) {
      throw new PricingPolicyWriteError(
        "version_conflict",
        `provider switch generation changed from ${parsed.expectedGeneration} to ${currentGeneration}`,
      );
    }
    const catalog = await activeCatalog(client, MAIN_PRICING_PRODUCT_ID);
    const changes = new Map(parsed.providers.map((provider) => [provider.providerId, provider]));
    for (const providerId of changes.keys()) {
      if (!catalog.providers.has(providerId)) {
        throw new PricingPolicyWriteError("rule_outside_catalog", `provider ${providerId} is outside the active product catalog`);
      }
    }
    const stored = await client.query<{
      provider_id: string;
      scope_type: "master" | "product" | "segment";
      product_id: string;
      segment: string;
      catalog_generation: string | null;
      enabled: boolean;
    }>(`
      SELECT provider_id, scope_type, product_id, segment,
             catalog_generation::text, enabled
      FROM provider_switch_entries
      WHERE generation = $1
      ORDER BY provider_id COLLATE "C", scope_type COLLATE "C", product_id COLLATE "C", segment COLLATE "C"
    `, [currentGeneration]);
    for (const [providerId] of changes) {
      const required = stored.rows.filter((entry) => entry.provider_id === providerId && (
        entry.scope_type === "master"
        || (entry.product_id === MAIN_PRICING_PRODUCT_ID && (
          entry.scope_type === "product"
          || (entry.scope_type === "segment" && (entry.segment === "b2c" || entry.segment === "b2b"))
        ))
      ));
      if (required.length !== 4) {
        throw new PricingPolicyWriteError("foundation_missing", `provider ${providerId} is missing required managed switches`);
      }
    }
    const nextGeneration = currentGeneration + 1;
    const entries: ProviderSwitchSpec["entries"] = stored.rows.map((entry) => {
      const change = changes.get(entry.provider_id);
      let enabled = entry.enabled;
      if (change) {
        if (entry.scope_type === "master") enabled = change.masterEnabled;
        else if (entry.product_id === MAIN_PRICING_PRODUCT_ID && entry.scope_type === "product") enabled = change.productEnabled;
        else if (entry.product_id === MAIN_PRICING_PRODUCT_ID && entry.scope_type === "segment" && entry.segment === "b2c") enabled = change.b2cEnabled;
        else if (entry.product_id === MAIN_PRICING_PRODUCT_ID && entry.scope_type === "segment" && entry.segment === "b2b") enabled = change.b2bEnabled;
      }
      return {
        provider_id: entry.provider_id,
        scope: entry.scope_type === "master"
          ? "master" as const
          : entry.scope_type === "product"
            ? { product: { product_id: entry.product_id } }
            : { segment: { product_id: entry.product_id, segment: entry.segment as "b2c" | "b2b" } },
        catalog_generation: entry.catalog_generation === null
          ? null
          : positiveVersion(entry.catalog_generation, "switch catalog generation"),
        enabled,
      };
    });
    const base = {
      generation: nextGeneration,
      schema_version: positiveVersion(currentRow.schema_version, "switch schema version"),
      capability_generation: positiveVersion(currentRow.capability_generation, "switch capability generation"),
      capability_digest: currentRow.capability_digest,
      entries,
    };
    const spec: ProviderSwitchSpec = { ...base, content_digest: stage5Digest("switches", base) };
    await client.query(`
      INSERT INTO provider_switch_versions (
        generation, schema_version, capability_generation, capability_digest,
        content_digest, actor_type, actor_id, reason
      ) VALUES ($1, $2, $3, $4, $5, 'admin', $6, $7)
    `, [
      spec.generation,
      spec.schema_version,
      spec.capability_generation,
      spec.capability_digest,
      spec.content_digest,
      input.actorId,
      parsed.reason,
    ]);
    for (const entry of stored.rows.map((row, index) => ({ row, spec: spec.entries[index]! }))) {
      await client.query(`
        INSERT INTO provider_switch_entries (
          generation, provider_id, scope_type, product_id, segment, catalog_generation, enabled
        ) VALUES ($1, $2, $3, $4, $5, $6, $7)
      `, [
        spec.generation,
        entry.row.provider_id,
        entry.row.scope_type,
        entry.row.product_id,
        entry.row.segment,
        entry.spec.catalog_generation,
        entry.spec.enabled,
      ]);
    }
    await client.query(`
      UPDATE provider_switch_head SET active_generation = $1, updated_at = now() WHERE singleton = 1
    `, [spec.generation]);
    await client.query(`
      INSERT INTO engine_switch_jobs (id, generation, schema_version, content_digest, payload)
      VALUES ($1, $2, $3, $4, $5::jsonb)
    `, [randomUUID(), spec.generation, spec.schema_version, spec.content_digest, JSON.stringify(spec)]);
    const bindings = await client.query<{ binding_id: string; policy_id: string }>(`
      SELECT binding.id::text AS binding_id, binding.policy_id
      FROM account_policy_bindings binding
      JOIN pricing_policies policy ON policy.id = binding.policy_id
      WHERE policy.status = 'active'
      ORDER BY binding.policy_id COLLATE "C", binding.id
      FOR UPDATE OF binding
    `);
    const sources = new Map<string, Awaited<ReturnType<typeof sourcePolicyById>>>();
    for (const binding of bindings.rows) {
      let source = sources.get(binding.policy_id);
      if (!source) {
        source = await sourcePolicyById(client, binding.policy_id, "share");
        sources.set(binding.policy_id, source);
      }
      // Switch generations are global authority. Every bound account receives a new immutable
      // effective version pinned to S2; otherwise re-enable could remain blocked by its S1 policy
      // lineage and Stage 8 would correctly report stale active policies.
      await materializeBinding(client, binding.binding_id, source.policy, source.catalogGeneration);
    }
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'provider_switches.updated', 'provider_switch_generation', $2, $3::jsonb)
    `, [input.actorId, String(spec.generation), JSON.stringify({
      previousGeneration: currentGeneration,
      generation: spec.generation,
      digest: spec.content_digest,
      providers: parsed.providers,
      rematerializedBindings: bindings.rowCount ?? bindings.rows.length,
      reason: parsed.reason,
    })]);
    const view = await managedPricingCatalogView(client, MAIN_PRICING_PRODUCT_ID);
    await client.query("COMMIT");
    return view;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function copyBusinessInvitationPolicyToUser(
  client: PoolClient,
  input: { inviteId: string; userId: string },
): Promise<{ policyId: string; policyVersion: number; policyDigest: string } | null> {
  const binding = await client.query<{ invitation_policy_id: string }>(`
    SELECT invitation_policy_id
    FROM business_invite_policy_bindings
    WHERE invite_id = $1
    FOR UPDATE
  `, [input.inviteId]);
  if (!binding.rows[0]) return null;
  const invitation = await sourcePolicyById(client, binding.rows[0].invitation_policy_id);
  const policyId = `policy:main:b2b:${input.userId}`;
  // Re-validate at the write boundary: invitation rules are discount-only, and the parse both
  // proves it and narrows the read-side view type back to the editor rule type.
  const rules = pricingPolicyEditorRulesSchema.parse(invitation.policy.rules.map(editorRuleFromSource));
  const copied = await createPolicyIdentityAndVersion(client, {
    policyId,
    ownerType: "b2b_client",
    ownerId: input.userId,
    productId: MAIN_PRICING_PRODUCT_ID,
    rules,
    actorId: `invite:${input.inviteId}`,
    reason: `Copied exact invitation policy version ${invitation.policy.version}`,
  });
  await client.query(`
    INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
    VALUES ('system', NULL, 'business_invite.policy_copied', 'pricing_policy', $1, $2::jsonb)
  `, [policyId, JSON.stringify({
    inviteId: input.inviteId,
    invitationPolicyId: invitation.policy.policy_id,
    invitationPolicyVersion: invitation.policy.version,
    invitationPolicyDigest: invitation.policy.content_digest,
    clientPolicyVersion: copied.version,
    clientPolicyDigest: copied.content_digest,
  })]);
  return { policyId, policyVersion: copied.version, policyDigest: copied.content_digest };
}

export async function copyBusinessInvitationPolicyToReplacement(
  client: PoolClient,
  input: { sourceInviteId: string; replacementInviteId: string; actorId: string; reason: string },
): Promise<{ policyId: string; policyVersion: number; policyDigest: string } | null> {
  const binding = await client.query<{ invitation_policy_id: string }>(`
    SELECT invitation_policy_id
    FROM business_invite_policy_bindings
    WHERE invite_id = $1
    FOR UPDATE
  `, [input.sourceInviteId]);
  if (!binding.rows[0]) return null;
  const source = await sourcePolicyById(client, binding.rows[0].invitation_policy_id);
  const copied = await createBusinessInvitationPolicy(client, {
    inviteId: input.replacementInviteId,
    rules: pricingPolicyEditorRulesSchema.parse(source.policy.rules.map(editorRuleFromSource)),
    actorId: input.actorId,
    reason: input.reason,
  });
  await client.query(`
    INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
    VALUES ('admin', $1, 'business_invite.policy_rotated', 'pricing_policy', $2, $3::jsonb)
  `, [input.actorId, copied.policy_id, JSON.stringify({
    sourceInviteId: input.sourceInviteId,
    sourcePolicyId: source.policy.policy_id,
    sourcePolicyVersion: source.policy.version,
    sourcePolicyDigest: source.policy.content_digest,
    replacementInviteId: input.replacementInviteId,
    replacementPolicyVersion: copied.version,
    replacementPolicyDigest: copied.content_digest,
    reason: input.reason,
  })]);
  return {
    policyId: copied.policy_id,
    policyVersion: copied.version,
    policyDigest: copied.content_digest,
  };
}

/**
 * The legacy engine delivery lane stores an immutable policy lineage per account and rejects any
 * prepare whose identity (policy/owner/class) differs from what the account already runs; identity
 * changes reach the engine only through the release-cutover locked transition. Returns the policy
 * the engine currently runs for the binding (its applied version, or the staged desired version
 * when nothing was applied yet), or null when no delivery lineage exists at all.
 */
async function engineRunPolicyId(
  client: PoolClient,
  binding: { id: string; desired_effective_version: string | null; applied_effective_version: string | null },
): Promise<string | null> {
  const reference = binding.applied_effective_version ?? binding.desired_effective_version;
  if (reference === null) return null;
  const version = await client.query<{ policy_id: string }>(`
    SELECT policy_id FROM account_policy_versions
    WHERE binding_id = $1 AND effective_version = $2
  `, [binding.id, reference]);
  return version.rows[0]?.policy_id ?? null;
}

/**
 * Aligns a strict binding whose legacy lane is closed (the engine runs a different policy
 * identity) with the engine-confirmed state: any staged-but-undeliverable desired version is
 * dropped back to the applied one and the dead-delivery error is cleared. The identity switch
 * for a strict account is delivered by the release-cutover lane, never by rewriting or
 * retrying legacy jobs. Shadow bindings never take this path: the engine accepts a shadow
 * rebind pre-cutover, so their identity switch is staged as a normal delivery.
 */
async function closeLegacyDeliveryDrift(client: PoolClient, bindingId: string): Promise<void> {
  await client.query(`
    UPDATE account_policy_bindings
    SET desired_effective_version = applied_effective_version,
        desired_digest = applied_digest,
        sync_state = CASE WHEN applied_effective_version IS NULL THEN 'legacy' ELSE 'confirmed' END,
        last_error = NULL, updated_at = now()
    WHERE id = $1
      AND (desired_effective_version IS DISTINCT FROM applied_effective_version
           OR desired_digest IS DISTINCT FROM applied_digest
           OR sync_state IN ('pending', 'failed')
           OR last_error IS NOT NULL)
  `, [bindingId]);
}

/**
 * Provisions the managed B2B client policy for a manually converted customer, reaching the same
 * end state as invitation redemption: an active source policy with a single Anthropic discount
 * rule mirroring the negotiated scalar multiplier and the account binding aimed at that policy.
 * A long-lived B2C customer already carries their single allowed binding (UNIQUE user_id) from
 * the Stage 5 backfill — aimed at the global B2C policy — so the conversion re-points that row
 * instead of inserting a second one. A conflicting delivery lineage blocks staging only for a
 * strict binding: the legacy lane keeps an immutable policy identity per strict account, so the
 * identity switch there ships via the release-cutover lane and the scalar multiplier stays
 * authoritative until then. A shadow binding (the backfilled B2C lineage) is re-pointed with a
 * normally staged delivery — the engine accepts a shadow rebind pre-cutover. Idempotent:
 * when the policy exists and the binding already aims at it, nothing is written and
 * provisioned=false is returned (a pending shadow rebind is still staged when the engine
 * lineage has not moved yet).
 */
export async function provisionBusinessClientPolicy(client: PoolClient, input: {
  userId: string;
  engineAccountRecordId: string;
  engineAccountId: string;
  multiplierBp: number;
  actorId: string;
  reason: string;
}): Promise<{
  policyId: string;
  policyVersion: number;
  policyDigest: string;
  jobId: string | null;
  provisioned: boolean;
}> {
  const identity = await client.query<{ id: string }>(`
    SELECT id FROM pricing_policies
    WHERE owner_type = 'b2b_client' AND owner_id = $1 AND product_id = $2 AND status = 'active'
    FOR UPDATE
  `, [input.userId, MAIN_PRICING_PRODUCT_ID]);
  let source: SourcePolicy;
  let catalogGeneration: number;
  let provisioned = false;
  if (identity.rows[0]) {
    const stored = await sourcePolicyById(client, identity.rows[0].id, "update");
    source = stored.policy;
    catalogGeneration = stored.catalogGeneration;
  } else {
    const discountBps = 10_000 - input.multiplierBp;
    if (!Number.isInteger(discountBps) || discountBps < 0 || discountBps > 9_500 || discountBps % 100 !== 0) {
      throw new RangeError("business multiplier does not map to a whole-percent managed policy discount");
    }
    await createPolicyIdentityAndVersion(client, {
      policyId: `policy:main:b2b:${input.userId}`,
      ownerType: "b2b_client",
      ownerId: input.userId,
      productId: MAIN_PRICING_PRODUCT_ID,
      rules: [{
        scope: { provider: { providerId: "anthropic" } },
        pricingMode: "discount",
        discountBps,
      }],
      actorId: input.actorId,
      reason: input.reason,
    });
    const stored = await sourcePolicyById(client, `policy:main:b2b:${input.userId}`, "update");
    source = stored.policy;
    catalogGeneration = stored.catalogGeneration;
    provisioned = true;
  }
  const existingBinding = await client.query<{
    id: string;
    account_class: string;
    policy_id: string;
    policy_enforcement: string;
    desired_effective_version: string | null;
    applied_effective_version: string | null;
  }>(`
    SELECT id::text, account_class, policy_id, policy_enforcement,
           desired_effective_version::text AS desired_effective_version,
           applied_effective_version::text AS applied_effective_version
    FROM account_policy_bindings WHERE user_id = $1 FOR UPDATE
  `, [input.userId]);
  let bindingId = existingBinding.rows[0]?.id ?? null;
  if (bindingId === null) {
    bindingId = randomUUID();
    await client.query(`
      INSERT INTO account_policy_bindings (
        id, user_id, engine_account_record_id, engine_account_id,
        account_class, product_id, policy_id,
        policy_enforcement, funding_enforcement, reconciliation_state, sync_state
      ) VALUES ($1, $2, $3, $4, 'b2b', $5, $6,
                'legacy_scalar', 'legacy_single', 'verified', 'legacy')
    `, [
      bindingId,
      input.userId,
      input.engineAccountRecordId,
      input.engineAccountId,
      MAIN_PRICING_PRODUCT_ID,
      source.policy_id,
    ]);
    provisioned = true;
  } else if (existingBinding.rows[0]!.policy_id !== source.policy_id
    || existingBinding.rows[0]!.account_class !== "b2b") {
    // The Stage 5 backfill bound existing B2C accounts to the global policy (shadow
    // enforcement). Conversion re-points that row to the new client policy; the
    // effective-version history continues on the same binding. The engine delivery of the
    // identity switch is staged below: the engine accepts a shadow rebind pre-cutover.
    await client.query(`
      UPDATE account_policy_bindings
      SET account_class = 'b2b', policy_id = $2, updated_at = now()
      WHERE id = $1
    `, [bindingId, source.policy_id]);
    provisioned = true;
  }
  const enginePolicyId = await engineRunPolicyId(client, existingBinding.rows[0] ?? {
    id: bindingId,
    desired_effective_version: null,
    applied_effective_version: null,
  });
  let jobId: string | null = null;
  const lineageMismatch = enginePolicyId !== null && enginePolicyId !== source.policy_id;
  if (lineageMismatch && existingBinding.rows[0]?.policy_enforcement === "strict") {
    // A strict account already runs a different policy identity and the legacy lane keeps that
    // lineage immutable: a prepare would be rejected with version_conflict, so no delivery is
    // staged; instead any drifted desired state left behind by earlier staging is folded back
    // to the engine-confirmed applied state.
    await closeLegacyDeliveryDrift(client, bindingId);
  } else if (provisioned || lineageMismatch) {
    // Fresh provisioning stages the first delivery; a shadow lineage mismatch (converted B2C
    // customer) stages the rebind delivery the engine now accepts — including on an idempotent
    // re-run that finds the policy and binding already in place.
    jobId = (await materializeBinding(client, bindingId, source, catalogGeneration)).jobId;
  }
  // The conversion contract (docs/commerce/MULTI-DISCOUNT.md decision 13) chains the
  // per-account strict cutover automatically: as soon as the staged shadow delivery confirms,
  // the pricing worker sweep flips enforcement to the client's own policy without a second
  // operator action. An already-strict binding is left untouched — its saves advance
  // strict→strict directly. Post-cutover the release-v2 authority owns pricing and the
  // assignment extension lane carries the client's policy, so no legacy chain is armed.
  if (!(await pricingReleaseCutoverCompleted(client))) {
    await client.query(`
      UPDATE account_policy_bindings
      SET strict_chain_pending = true, updated_at = now()
      WHERE id = $1 AND policy_enforcement <> 'strict'
    `, [bindingId]);
  }
  return {
    policyId: source.policy_id,
    policyVersion: source.version,
    policyDigest: source.content_digest,
    jobId,
    provisioned,
  };
}

/**
 * Aligns the commerce-side scalar mirrors (`customer_profiles.multiplier_bp` +
 * `engine_accounts.mult_bp`) with the account's release-effective fallback. Post-cutover the
 * scalar is dormant for current billing — the release authority prices every non-opted-out
 * account and never reads `accounts.mult_bp` — but it is the fallback the strict/policy_v1
 * admission applies to scopes without a matching policy rule (e.g. providers outside the
 * strict policy's provider rules), so by opt-out time it must equal what the release path
 * resolves for those scopes. The ENGINE-side write is owned by the caller's lane (the
 * backfill writes `account_set_mult_bp` directly and asserts it before arming; new accounts
 * are born aligned) — the legacy `engine_pricing_jobs` stream is deliberately not used here
 * because `claimNextPricingJob` drains scalar jobs for strict-bound accounts without an
 * engine write. Idempotent: a no-op when both mirrors already match.
 */
export async function syncProvisionedAccountScalar(client: PoolClient, input: {
  userId: string;
  engineAccountId: string;
  multiplierBp: number;
}): Promise<void> {
  await client.query(`
    UPDATE customer_profiles
    SET multiplier_bp = $2
    WHERE user_id = $1 AND multiplier_bp <> $2
  `, [input.userId, input.multiplierBp]);
  await client.query(`
    UPDATE engine_accounts
    SET mult_bp = $2, updated_at = now()
    WHERE user_id = $1 AND mult_bp <> $2
  `, [input.userId, input.multiplierBp]);
}

/** Standalone variant of syncProvisionedAccountScalar for callers outside a materialization transaction. */
export async function alignProvisionedAccountScalar(database: Database, input: {
  userId: string;
  engineAccountId: string;
  multiplierBp: number;
}): Promise<void> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await syncProvisionedAccountScalar(client, input);
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function materializeProvisionedUserPolicy(database: Database, input: {
  userId: string;
  engineAccountId: string;
}, options?: {
  /**
   * Registration provisioning (the default) arms the direct strict chain in the same
   * transaction. The existing-account backfill passes `false`: it must prove
   * release/strict equivalence BEFORE the chain is allowed to stage the cutover, so it
   * arms the flag itself only after the check passes.
   */
  armStrictChain?: boolean;
  /**
   * The release-effective fallback multiplier the scalar mirrors are aligned to. The
   * existing-account backfill passes the value DERIVED from the account's live release
   * policy (global rule payable, else full price). The default is the class identity:
   * B2C 5000 (the engine-validated release global) and B2B 10000 (release B2B policies
   * cannot carry a global rule, so rule-less scopes have no release fallback and full
   * price is the only honest strict fallback). New accounts are born aligned, so for
   * registration this is a convergence no-op.
   */
  alignScalarBp?: number;
}): Promise<{ policyRequired: boolean; ready: boolean; jobId: string | null }> {
  const armChain = options?.armStrictChain ?? true;
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    const account = await client.query<{
      record_id: string;
      customer_type: "b2c" | "b2b";
      engine_account_id: string | null;
      status: "pending" | "active" | "error" | "disabled";
    }>(`
      SELECT account.id::text AS record_id, profile.customer_type,
             account.engine_account_id, account.status
      FROM engine_accounts account
      JOIN customer_profiles profile ON profile.user_id = account.user_id
      WHERE account.user_id = $1
      FOR UPDATE OF account
    `, [input.userId]);
    const row = account.rows[0];
    if (!row) throw new PricingPolicyWriteError("provisioning_policy_missing", "engine account mapping is missing");
    if (row.status === "disabled") throw new PricingPolicyWriteError("provisioning_policy_missing", "engine account is disabled");
    if (row.engine_account_id !== null && row.engine_account_id !== input.engineAccountId) {
      throw new PricingPolicyWriteError("provisioning_policy_missing", "engine account mapping changed during provisioning");
    }
    await syncProvisionedAccountScalar(client, {
      userId: input.userId,
      engineAccountId: input.engineAccountId,
      multiplierBp: options?.alignScalarBp ?? (row.customer_type === "b2c" ? 5_000 : 10_000),
    });
    const ownerType: ManagedOwnerType = row.customer_type === "b2c" ? "global_b2c" : "b2b_client";
    const ownerId = row.customer_type === "b2c" ? "global-b2c" : input.userId;
    const identity = await client.query<{ id: string }>(`
      SELECT id FROM pricing_policies
      WHERE owner_type = $1 AND owner_id = $2 AND product_id = $3 AND status = 'active'
      FOR SHARE
    `, [ownerType, ownerId, MAIN_PRICING_PRODUCT_ID]);
    if (!identity.rows[0]) {
      if (row.customer_type === "b2b") {
        const invitationHasPolicy = await client.query<{ present: boolean }>(`
          SELECT EXISTS (
            SELECT 1 FROM business_invites invitation
            JOIN business_invite_policy_bindings policy ON policy.invite_id = invitation.id
            WHERE invitation.consumed_by_user_id = $1
          ) AS present
        `, [input.userId]);
        if (invitationHasPolicy.rows[0]?.present) {
          throw new PricingPolicyWriteError(
            "provisioning_policy_missing",
            "redeemed invitation policy was not copied to the B2B customer",
          );
        }
      }
      await client.query(`
        UPDATE engine_accounts
        SET engine_account_id = $2, status = 'active', last_error = NULL, updated_at = now()
        WHERE user_id = $1
      `, [input.userId, input.engineAccountId]);
      await client.query("COMMIT");
      return { policyRequired: false, ready: true, jobId: null };
    }
    const source = await sourcePolicyById(client, identity.rows[0].id);
    let binding = await client.query<{
      id: string;
      desired_effective_version: string | null;
      applied_effective_version: string | null;
      desired_digest: string | null;
      applied_digest: string | null;
      sync_state: "legacy" | "pending" | "confirmed" | "failed";
    }>(`
      SELECT id::text, desired_effective_version::text, applied_effective_version::text,
             desired_digest, applied_digest, sync_state
      FROM account_policy_bindings WHERE user_id = $1 FOR UPDATE
    `, [input.userId]);
    if (!binding.rows[0]) {
      const bindingId = randomUUID();
      await client.query(`
        UPDATE engine_accounts
        SET engine_account_id = $2, status = 'pending', last_error = NULL, updated_at = now()
        WHERE user_id = $1
      `, [input.userId, input.engineAccountId]);
      await client.query(`
        INSERT INTO account_policy_bindings (
          id, user_id, engine_account_record_id, engine_account_id,
          account_class, product_id, policy_id,
          policy_enforcement, funding_enforcement, reconciliation_state, sync_state
        ) VALUES ($1, $2, $3, $4, $5, $6, $7,
                  'legacy_scalar', 'legacy_single', 'verified', 'legacy')
      `, [
        bindingId,
        input.userId,
        row.record_id,
        input.engineAccountId,
        row.customer_type,
        MAIN_PRICING_PRODUCT_ID,
        source.policy.policy_id,
      ]);
      binding = await client.query(`
        SELECT id::text, desired_effective_version::text, applied_effective_version::text,
               desired_digest, applied_digest, sync_state
        FROM account_policy_bindings WHERE id = $1 FOR UPDATE
      `, [bindingId]);
    } else {
      await client.query(`
        UPDATE engine_accounts
        SET engine_account_id = $2,
            status = (CASE WHEN $3 = 'confirmed' THEN 'active' ELSE 'pending' END)::engine_account_status,
            last_error = NULL, updated_at = now()
        WHERE user_id = $1
      `, [input.userId, input.engineAccountId, binding.rows[0].sync_state]);
    }
    const bindingRow = binding.rows[0]!;
    if (bindingRow.desired_effective_version !== null) {
      const desired = await client.query<{
        policy_version: string;
        policy_digest: string;
        content_digest: string;
        job_id: string | null;
        job_status: string | null;
      }>(`
        SELECT version.policy_version::text, version.policy_digest, version.content_digest,
               job.id::text AS job_id, job.status AS job_status
        FROM account_policy_versions version
        LEFT JOIN engine_policy_jobs job
          ON job.binding_id = version.binding_id AND job.effective_version = version.effective_version
        WHERE version.binding_id = $1 AND version.effective_version = $2
      `, [bindingRow.id, bindingRow.desired_effective_version]);
      const desiredRow = desired.rows[0];
      // A desired version whose delivery died (terminal prepare rejection) must not be reused:
      // falling through re-materializes a fresh effective version with live dependency pins.
      if (
        desiredRow
        && desiredRow.job_status !== "dead"
        && positiveVersion(desiredRow.policy_version, "desired source policy version") === source.policy.version
        && desiredRow.policy_digest === source.policy.content_digest
        && desiredRow.content_digest === bindingRow.desired_digest
      ) {
        const ready = bindingRow.sync_state === "confirmed"
          && bindingRow.desired_effective_version === bindingRow.applied_effective_version
          && bindingRow.desired_digest === bindingRow.applied_digest;
        if (ready) {
          await client.query(`UPDATE engine_accounts SET status = 'active', updated_at = now() WHERE user_id = $1`, [input.userId]);
        }
        if (armChain) await armDirectStrictChain(client, bindingRow.id);
        await linkRedeemedInvitation(client, input.userId, bindingRow.id, source.policy);
        await client.query("COMMIT");
        return { policyRequired: true, ready, jobId: desiredRow.job_id };
      }
    }
    // A source policy versioned before the latest catalog advance pins a catalog generation the
    // engine can no longer validate against the live switch head (capability lineage mismatch),
    // which kills the fresh account's first delivery and strands it in pending forever. Re-pin
    // the managed source policy first — a new immutable version with identical rules at the live
    // catalog head — then materialize from it. Older effective policies keep referencing their
    // own immutable source versions; nothing already delivered is rewritten.
    let stagedSource = source;
    const liveCatalog = await activeCatalog(client, source.policy.product_id);
    if (source.catalogGeneration !== liveCatalog.generation) {
      if (source.policy.rules.some((rule) =>
        rule.pricing_mode !== "discount" || rule.discount_bps === null)) {
        // The editor schema only carries static discount rules; a historical track-mode source
        // needs an operator review, never a silent semantic change during provisioning.
        throw new PricingPolicyWriteError(
          "provisioning_policy_missing",
          "managed source policy carries a legacy track rule and cannot be re-pinned automatically",
        );
      }
      const repinned = buildSourcePolicy({
        policyId: source.policy.policy_id,
        ownerType: source.policy.owner_type,
        ownerId: source.policy.owner_id,
        productId: source.policy.product_id,
        version: source.policy.version + 1,
        rules: source.policy.rules.map((rule) => ({
          scope: rule.scope_type === "provider"
            ? { provider: { providerId: rule.provider_id } }
            : {
                model: {
                  providerId: rule.provider_id,
                  canonicalModelId: rule.canonical_model_id ?? "",
                },
              },
          pricingMode: "discount" as const,
          discountBps: rule.discount_bps,
        })),
      });
      await storeSourcePolicyVersion(
        client,
        repinned,
        liveCatalog.generation,
        "pricing-release-provisioning",
        `re-pin to live catalog generation ${liveCatalog.generation} (was ${source.catalogGeneration})`,
      );
      const bumped = await client.query(`
        UPDATE pricing_policy_heads
        SET current_version = $2, current_digest = $3, updated_at = now()
        WHERE policy_id = $1 AND current_version = $4
      `, [
        source.policy.policy_id,
        repinned.version,
        repinned.content_digest,
        source.policy.version,
      ]);
      if (bumped.rowCount !== 1) {
        throw new PricingPolicyWriteError(
          "version_conflict",
          "managed source policy head moved during provisioning",
        );
      }
      stagedSource = { policy: repinned, catalogGeneration: liveCatalog.generation };
    }
    const staged = await materializeBinding(
      client,
      bindingRow.id,
      stagedSource.policy,
      stagedSource.catalogGeneration,
    );
    if (armChain) await armDirectStrictChain(client, bindingRow.id);
    await linkRedeemedInvitation(client, input.userId, bindingRow.id, source.policy);
    await client.query("COMMIT");
    return { policyRequired: true, ready: false, jobId: staged.jobId };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/**
 * New-account provisioning arms the direct strict chain (release-v2 retirement, phase 2.1):
 * the pricing worker drives the binding shadow→strict and then writes the engine opt-out
 * marker. Re-arming an already-armed binding is a no-op, so provisioning retries and the
 * bounded confirmation polls stay idempotent; an already-strict binding is left untouched —
 * its chain finishes through the opt-out step, not through re-staging.
 */
async function armDirectStrictChain(client: PoolClient, bindingId: string): Promise<void> {
  await client.query(`
    UPDATE account_policy_bindings
    SET strict_chain_pending = true, updated_at = now()
    WHERE id = $1 AND policy_enforcement <> 'strict'
  `, [bindingId]);
}

async function linkRedeemedInvitation(
  client: PoolClient,
  userId: string,
  bindingId: string,
  clientPolicy: SourcePolicy,
): Promise<void> {
  await client.query(`
    UPDATE business_invite_policy_bindings policy
    SET redeemed_source_policy_version = policy.current_policy_version,
        redeemed_source_policy_digest = policy.current_policy_digest,
        copied_to_user_id = $1,
        copied_to_binding_id = $2,
        copied_client_policy_id = $3,
        copied_client_policy_version = $4,
        copied_client_policy_digest = $5,
        redeemed_at = COALESCE(policy.redeemed_at, now()),
        updated_at = now()
    FROM business_invites invitation
    WHERE policy.invite_id = invitation.id
      AND invitation.consumed_by_user_id = $1
      AND policy.redeemed_at IS NULL
  `, [userId, bindingId, clientPolicy.policy_id, clientPolicy.version, clientPolicy.content_digest]);
}

export async function assertUserPolicyReadyForKey(database: Database, userId: string): Promise<void> {
  const result = await database.pool.query<{
    managed_policy_id: string | null;
    policy_id: string | null;
    desired_effective_version: string | null;
    applied_effective_version: string | null;
    desired_digest: string | null;
    applied_digest: string | null;
    sync_state: string | null;
  }>(`
    SELECT policy.id AS managed_policy_id, binding.policy_id,
           binding.desired_effective_version::text,
           binding.applied_effective_version::text, binding.desired_digest,
           binding.applied_digest, binding.sync_state
    FROM engine_accounts account
    JOIN customer_profiles profile ON profile.user_id = account.user_id
    LEFT JOIN pricing_policies policy
      ON policy.product_id = $2
     AND policy.status = 'active'
     AND (
       (profile.customer_type = 'b2c' AND policy.owner_type = 'global_b2c' AND policy.owner_id = 'global-b2c')
       OR (profile.customer_type = 'b2b' AND policy.owner_type = 'b2b_client' AND policy.owner_id = account.user_id::text)
     )
    LEFT JOIN account_policy_bindings binding ON binding.user_id = account.user_id
    WHERE account.user_id = $1
  `, [userId, MAIN_PRICING_PRODUCT_ID]);
  const row = result.rows[0];
  if (!row?.managed_policy_id) return;
  if (
    row.policy_id !== row.managed_policy_id
    || row.sync_state !== "confirmed"
    || row.desired_effective_version === null
    || row.desired_effective_version !== row.applied_effective_version
    || row.desired_digest === null
    || row.desired_digest !== row.applied_digest
  ) {
    throw new PricingPolicyWriteError(
      "provisioning_policy_missing",
      "pricing policy is still waiting for an exact engine ACK",
    );
  }
}

export interface UserPolicyBindingState {
  bindingId: string;
  engineAccountId: string;
  policyEnforcement: string;
  syncState: string;
  strictChainPending: boolean;
  desiredEffectiveVersion: number | null;
  appliedEffectiveVersion: number | null;
  desiredDigest: string | null;
  appliedDigest: string | null;
}

/**
 * Read-side view of one account's policy binding for the request-path strict-chain checks in
 * key issuance. `strictChainConfirmed` semantics are evaluated by the caller: enforcement
 * strict + sync confirmed + desired = applied (version and digest).
 */
export async function readUserPolicyBindingState(
  database: Database,
  userId: string,
): Promise<UserPolicyBindingState | null> {
  const result = await database.pool.query<{
    binding_id: string;
    engine_account_id: string;
    policy_enforcement: string;
    sync_state: string;
    strict_chain_pending: boolean;
    desired_effective_version: string | null;
    applied_effective_version: string | null;
    desired_digest: string | null;
    applied_digest: string | null;
  }>(`
    SELECT id::text AS binding_id, engine_account_id, policy_enforcement, sync_state,
           strict_chain_pending, desired_effective_version::text, applied_effective_version::text,
           desired_digest, applied_digest
    FROM account_policy_bindings
    WHERE user_id = $1
  `, [userId]);
  const row = result.rows[0];
  if (!row) return null;
  return {
    bindingId: row.binding_id,
    engineAccountId: row.engine_account_id,
    policyEnforcement: row.policy_enforcement,
    syncState: row.sync_state,
    strictChainPending: row.strict_chain_pending,
    desiredEffectiveVersion: row.desired_effective_version === null
      ? null
      : positiveVersion(row.desired_effective_version, "desired effective policy version"),
    appliedEffectiveVersion: row.applied_effective_version === null
      ? null
      : positiveVersion(row.applied_effective_version, "applied effective policy version"),
    desiredDigest: row.desired_digest,
    appliedDigest: row.applied_digest,
  };
}
