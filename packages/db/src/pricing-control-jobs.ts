import { randomUUID } from "node:crypto";
import { Buffer } from "node:buffer";
import { isDeepStrictEqual } from "node:util";
import {
  accountPolicyBindingSchema,
  accountPolicySpecSchema,
  policyActiveExpectationSchema,
  pricingActiveExpectationSchema,
  pricingCatalogSpecSchema,
  providerSwitchSpecSchema,
  type AccountPolicyBinding,
  type AccountPolicySpec,
  type PricingCatalogSpec,
  type PricingMutationAck,
  type ProviderSwitchSpec,
} from "@claude-api/contracts";
import type { PoolClient } from "pg";
import { z } from "zod";
import type { Database } from "./client.js";

const catalogJobPayloadSchema = pricingCatalogSpecSchema;
const switchJobPayloadSchema = providerSwitchSpecSchema;
export const policyControlJobPayloadSchema = z.object({
  policy: accountPolicySpecSchema,
  binding: accountPolicyBindingSchema,
}).strict();

const catalogActivationIdentitySchema = z.object({
  catalog: pricingCatalogSpecSchema,
  expectation: pricingActiveExpectationSchema,
}).strict();
const switchActivationIdentitySchema = z.object({
  switches: providerSwitchSpecSchema,
  expectation: pricingActiveExpectationSchema,
}).strict();
const policyActivationIdentitySchema = z.object({
  policy: accountPolicySpecSchema,
  activation: z.object({
    account_id: z.string(),
    effective_version: z.number().int().safe().positive(),
    content_digest: z.string(),
    binding: accountPolicyBindingSchema,
  }).strict(),
  expectation: policyActiveExpectationSchema,
}).strict();

interface ClaimedJobBase {
  id: string;
  attempts: number;
}

export interface ClaimedCatalogControlJob extends ClaimedJobBase {
  kind: "catalog";
  spec: PricingCatalogSpec;
}

export interface ClaimedSwitchControlJob extends ClaimedJobBase {
  kind: "switches";
  spec: ProviderSwitchSpec;
}

export interface ClaimedPolicyControlJob extends ClaimedJobBase {
  kind: "policy";
  bindingId: string;
  spec: AccountPolicySpec;
  binding: AccountPolicyBinding;
}

export type ClaimedPricingControlJob =
  | ClaimedCatalogControlJob
  | ClaimedSwitchControlJob
  | ClaimedPolicyControlJob;

export type PricingControlJobDisposition = "retry" | "superseded" | "dead";

export class PricingControlJobStageError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PricingControlJobStageError";
  }
}

interface VersionedJobRow {
  id: string;
  attempts: number;
  generation: string;
  schema_version: string;
  content_digest: string;
  payload: unknown;
}

interface PolicyJobRow {
  id: string;
  binding_id: string;
  attempts: number;
  effective_version: string;
  policy_version: string;
  catalog_generation: string;
  switch_generation: string;
  schema_version: string;
  content_digest: string;
  engine_account_id: string;
  policy_id: string;
  payload: unknown;
}

function safeVersion(value: string, field: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0 || String(parsed) !== value) {
    throw new Error(`${field} is not a positive safe integer`);
  }
  return parsed;
}

function assertCatalogRow(row: VersionedJobRow, spec: PricingCatalogSpec): void {
  if (
    spec.generation !== safeVersion(row.generation, "catalog generation") ||
    spec.schema_version !== safeVersion(row.schema_version, "catalog schema version") ||
    spec.content_digest !== row.content_digest
  ) {
    throw new Error(`catalog job ${row.id} payload does not match its durable target`);
  }
}

function assertSwitchRow(row: VersionedJobRow, spec: ProviderSwitchSpec): void {
  if (
    spec.generation !== safeVersion(row.generation, "switch generation") ||
    spec.schema_version !== safeVersion(row.schema_version, "switch schema version") ||
    spec.content_digest !== row.content_digest
  ) {
    throw new Error(`switch job ${row.id} payload does not match its durable target`);
  }
}

function assertPolicyRow(row: PolicyJobRow, spec: AccountPolicySpec): void {
  if (
    spec.account_id !== row.engine_account_id ||
    spec.effective_version !== safeVersion(row.effective_version, "effective policy version") ||
    spec.policy_id !== row.policy_id ||
    spec.policy_version !== safeVersion(row.policy_version, "source policy version") ||
    spec.catalog_generation !== safeVersion(row.catalog_generation, "policy catalog generation") ||
    spec.switch_generation !== safeVersion(row.switch_generation, "policy switch generation") ||
    spec.schema_version !== safeVersion(row.schema_version, "policy schema version") ||
    spec.content_digest !== row.content_digest
  ) {
    throw new Error(`policy job ${row.id} payload does not match its durable target`);
  }
}

function sameJson(left: unknown, right: unknown): boolean {
  return isDeepStrictEqual(left, right);
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function compareStringTuples(left: readonly string[], right: readonly string[]): number {
  for (let index = 0; index < left.length; index += 1) {
    const compared = compareUtf8(left[index]!, right[index]!);
    if (compared !== 0) return compared;
  }
  return 0;
}

function switchScopeKey(scope: ProviderSwitchSpec["entries"][number]["scope"]): readonly string[] {
  if (scope === "master") return ["master", "", ""];
  if ("product" in scope) return ["product", scope.product.product_id, ""];
  return ["segment", scope.segment.product_id, scope.segment.segment];
}

function policyRuleKey(rule: AccountPolicySpec["rules"][number]): readonly string[] {
  if ("provider" in rule.scope) {
    return [rule.scope.provider.provider_id, "provider", "", rule.rule_id];
  }
  return [
    rule.scope.model.provider_id,
    "model",
    rule.scope.model.canonical_model_id,
    rule.rule_id,
  ];
}

export async function stagePricingCatalogControlJob(
  database: Database,
  input: PricingCatalogSpec,
): Promise<string> {
  const spec = pricingCatalogSpecSchema.parse(input);
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const version = await client.query<{
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
      FOR SHARE
    `, [spec.product_id, spec.generation]);
    const row = version.rows[0];
    if (!row) throw new Error("catalog version must exist before its control job is staged");
    const entries = await client.query<{
      provider_id: string;
      canonical_model_id: string;
      enabled: boolean;
    }>(`
      SELECT provider_id, canonical_model_id, enabled
      FROM product_catalog_entries
      WHERE product_id = $1 AND generation = $2
      ORDER BY provider_id, canonical_model_id
    `, [spec.product_id, spec.generation]);
    const stored = pricingCatalogSpecSchema.parse({
      product_id: row.product_id,
      generation: safeVersion(row.generation, "catalog generation"),
      schema_version: safeVersion(row.schema_version, "catalog schema version"),
      capability_generation: safeVersion(row.capability_generation, "catalog capability generation"),
      capability_digest: row.capability_digest,
      content_digest: row.content_digest,
      entries: entries.rows,
    });
    const normalized = { ...spec, entries: [...spec.entries].sort((left, right) =>
      compareStringTuples(
        [left.provider_id, left.canonical_model_id],
        [right.provider_id, right.canonical_model_id],
      )) };
    stored.entries.sort((left, right) => compareStringTuples(
      [left.provider_id, left.canonical_model_id],
      [right.provider_id, right.canonical_model_id],
    ));
    if (!sameJson(stored, normalized)) {
      throw new Error("catalog control payload does not match the immutable commerce version");
    }

    const head = await client.query<{ active_generation: string }>(`
      SELECT active_generation::text
      FROM product_catalog_heads
      WHERE product_id = $1
      FOR UPDATE
    `, [spec.product_id]);
    const active = head.rows[0]?.active_generation;
    if (active !== undefined && safeVersion(active, "catalog head") > spec.generation) {
      throw new Error("catalog control target is stale");
    }
    if (active === undefined) {
      await client.query(`
        INSERT INTO product_catalog_heads (product_id, active_generation)
        VALUES ($1, $2)
      `, [spec.product_id, spec.generation]);
    } else if (safeVersion(active, "catalog head") < spec.generation) {
      await client.query(`
        UPDATE product_catalog_heads
        SET active_generation = $2, updated_at = now()
        WHERE product_id = $1
      `, [spec.product_id, spec.generation]);
    }
    const jobId = await insertImmutableJob(client, {
      table: "engine_catalog_jobs",
      lookup: "product_id = $1 AND generation = $2",
      lookupValues: [spec.product_id, spec.generation],
      insertSql: `
        INSERT INTO engine_catalog_jobs (
          id, product_id, generation, schema_version, content_digest, payload
        ) VALUES ($1, $2, $3, $4, $5, $6::jsonb)
      `,
      insertValues: [
        randomUUID(),
        spec.product_id,
        spec.generation,
        spec.schema_version,
        spec.content_digest,
        JSON.stringify(stored),
      ],
      payload: stored,
    });
    await client.query("COMMIT");
    return jobId;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export interface StoredControlJobStageAudit {
  actorId: string;
  reason: string;
}

async function recordControlJobStageAudit(
  database: Database,
  audit: StoredControlJobStageAudit,
  action: string,
  targetType: string,
  targetId: string,
  jobId: string,
): Promise<void> {
  await database.pool.query(`
    INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
    VALUES ('admin', $1, $2, $3, $4, $5::jsonb)
  `, [
    audit.actorId,
    action,
    targetType,
    targetId,
    JSON.stringify({ jobId, reason: audit.reason }),
  ]);
}

/**
 * Stages a catalog convergence job from the stored immutable version — the operator passes only
 * the product and generation, never a wire payload. Exact replay returns the existing job.
 */
export async function stageStoredPricingCatalogControlJob(
  database: Database,
  productId: string,
  generation: number,
  audit: StoredControlJobStageAudit,
): Promise<string> {
  const version = await database.pool.query<{
    schema_version: string;
    capability_generation: string;
    capability_digest: string;
    content_digest: string;
  }>(`
    SELECT schema_version::text, capability_generation::text, capability_digest, content_digest
    FROM product_catalog_versions
    WHERE product_id = $1 AND generation = $2
  `, [productId, generation]);
  const row = version.rows[0];
  if (!row) {
    throw new PricingControlJobStageError(
      `catalog version ${productId}/${generation} does not exist in commerce storage`,
    );
  }
  const entries = await database.pool.query<{
    provider_id: string;
    canonical_model_id: string;
    enabled: boolean;
  }>(`
    SELECT provider_id, canonical_model_id, enabled
    FROM product_catalog_entries
    WHERE product_id = $1 AND generation = $2
    ORDER BY provider_id, canonical_model_id
  `, [productId, generation]);
  const spec = pricingCatalogSpecSchema.parse({
    product_id: productId,
    generation,
    schema_version: safeVersion(row.schema_version, "catalog schema version"),
    capability_generation: safeVersion(row.capability_generation, "catalog capability generation"),
    capability_digest: row.capability_digest,
    content_digest: row.content_digest,
    entries: entries.rows,
  });
  const existingCatalogJob = await database.pool.query<{ id: string }>(`
    SELECT id FROM engine_catalog_jobs WHERE product_id = $1 AND generation = $2
  `, [productId, generation]);
  const jobId = await stagePricingCatalogControlJob(database, spec);
  if (!existingCatalogJob.rows[0]) {
    await recordControlJobStageAudit(
      database, audit, "pricing_catalog.convergence_staged", "product_catalog",
      `catalog:${productId}:${generation}`, jobId,
    );
  }
  return jobId;
}

/**
 * Stages a provider-switch convergence job from the stored immutable version. Exact replay
 * returns the existing job.
 */
export async function stageStoredProviderSwitchControlJob(
  database: Database,
  generation: number,
  audit: StoredControlJobStageAudit,
): Promise<string> {
  const version = await database.pool.query<{
    schema_version: string;
    capability_generation: string;
    capability_digest: string;
    content_digest: string;
  }>(`
    SELECT schema_version::text, capability_generation::text, capability_digest, content_digest
    FROM provider_switch_versions
    WHERE generation = $1
  `, [generation]);
  const row = version.rows[0];
  if (!row) {
    throw new PricingControlJobStageError(
      `provider-switch generation ${generation} does not exist in commerce storage`,
    );
  }
  const entries = await database.pool.query<{
    provider_id: string;
    scope_type: "master" | "product" | "segment";
    product_id: string;
    segment: "" | "b2c" | "b2b";
    catalog_generation: string | null;
    enabled: boolean;
  }>(`
    SELECT provider_id, scope_type, product_id, segment,
           catalog_generation::text, enabled
    FROM provider_switch_entries
    WHERE generation = $1
    ORDER BY provider_id, scope_type, product_id, segment
  `, [generation]);
  const spec = providerSwitchSpecSchema.parse({
    generation,
    schema_version: safeVersion(row.schema_version, "switch schema version"),
    capability_generation: safeVersion(row.capability_generation, "switch capability generation"),
    capability_digest: row.capability_digest,
    content_digest: row.content_digest,
    entries: entries.rows.map((entry) => ({
      provider_id: entry.provider_id,
      scope: entry.scope_type === "master"
        ? "master"
        : entry.scope_type === "product"
          ? { product: { product_id: entry.product_id } }
          : { segment: { product_id: entry.product_id, segment: entry.segment } },
      catalog_generation: entry.catalog_generation === null
        ? null
        : safeVersion(entry.catalog_generation, "switch catalog generation"),
      enabled: entry.enabled,
    })),
  });
  const existingSwitchJob = await database.pool.query<{ id: string }>(`
    SELECT id FROM engine_switch_jobs WHERE generation = $1
  `, [generation]);
  const jobId = await stageProviderSwitchControlJob(database, spec);
  if (!existingSwitchJob.rows[0]) {
    await recordControlJobStageAudit(
      database, audit, "provider_switches.convergence_staged", "provider_switches",
      `switches:${generation}`, jobId,
    );
  }
  return jobId;
}

export async function stageProviderSwitchControlJob(
  database: Database,
  input: ProviderSwitchSpec,
): Promise<string> {
  const spec = providerSwitchSpecSchema.parse(input);
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const version = await client.query<{
      generation: string;
      schema_version: string;
      capability_generation: string;
      capability_digest: string;
      content_digest: string;
    }>(`
      SELECT generation::text, schema_version::text, capability_generation::text,
             capability_digest, content_digest
      FROM provider_switch_versions
      WHERE generation = $1
      FOR SHARE
    `, [spec.generation]);
    const row = version.rows[0];
    if (!row) throw new Error("provider-switch version must exist before its control job is staged");
    const entries = await client.query<{
      provider_id: string;
      scope_type: "master" | "product" | "segment";
      product_id: string;
      segment: "" | "b2c" | "b2b";
      catalog_generation: string | null;
      enabled: boolean;
    }>(`
      SELECT provider_id, scope_type, product_id, segment,
             catalog_generation::text, enabled
      FROM provider_switch_entries
      WHERE generation = $1
      ORDER BY provider_id, scope_type, product_id, segment
    `, [spec.generation]);
    const stored = providerSwitchSpecSchema.parse({
      generation: safeVersion(row.generation, "switch generation"),
      schema_version: safeVersion(row.schema_version, "switch schema version"),
      capability_generation: safeVersion(row.capability_generation, "switch capability generation"),
      capability_digest: row.capability_digest,
      content_digest: row.content_digest,
      entries: entries.rows.map((entry) => ({
        provider_id: entry.provider_id,
        scope: entry.scope_type === "master"
          ? "master"
          : entry.scope_type === "product"
            ? { product: { product_id: entry.product_id } }
            : { segment: { product_id: entry.product_id, segment: entry.segment } },
        catalog_generation: entry.catalog_generation === null
          ? null
          : safeVersion(entry.catalog_generation, "switch catalog generation"),
        enabled: entry.enabled,
      })),
    });
    const compareSwitches = (
      left: ProviderSwitchSpec["entries"][number],
      right: ProviderSwitchSpec["entries"][number],
    ): number => compareUtf8(left.provider_id, right.provider_id) ||
      compareStringTuples(switchScopeKey(left.scope), switchScopeKey(right.scope));
    const normalized = { ...spec, entries: [...spec.entries].sort(compareSwitches) };
    const normalizedStored = { ...stored, entries: [...stored.entries].sort(compareSwitches) };
    if (!sameJson(normalizedStored, normalized)) {
      throw new Error("switch control payload does not match the immutable commerce version");
    }

    const head = await client.query<{ active_generation: string }>(`
      SELECT active_generation::text FROM provider_switch_head
      WHERE singleton = 1 FOR UPDATE
    `);
    const active = head.rows[0]?.active_generation;
    if (active !== undefined && safeVersion(active, "switch head") > spec.generation) {
      throw new Error("provider-switch control target is stale");
    }
    if (active === undefined) {
      await client.query(`
        INSERT INTO provider_switch_head (singleton, active_generation) VALUES (1, $1)
      `, [spec.generation]);
    } else if (safeVersion(active, "switch head") < spec.generation) {
      await client.query(`
        UPDATE provider_switch_head SET active_generation = $1, updated_at = now()
        WHERE singleton = 1
      `, [spec.generation]);
    }
    const jobId = await insertImmutableJob(client, {
      table: "engine_switch_jobs",
      lookup: "generation = $1",
      lookupValues: [spec.generation],
      insertSql: `
        INSERT INTO engine_switch_jobs (
          id, generation, schema_version, content_digest, payload
        ) VALUES ($1, $2, $3, $4, $5::jsonb)
      `,
      insertValues: [
        randomUUID(),
        spec.generation,
        spec.schema_version,
        spec.content_digest,
        JSON.stringify(normalizedStored),
      ],
      payload: normalizedStored,
    });
    await client.query("COMMIT");
    return jobId;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function stageAccountPolicyControlJob(
  database: Database,
  input: { policy: AccountPolicySpec; binding: AccountPolicyBinding },
): Promise<string> {
  const payload = policyControlJobPayloadSchema.parse(input);
  const { policy, binding } = payload;
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const target = await client.query<{
      binding_id: string;
      engine_account_id: string;
      account_class: AccountPolicySpec["account_class"];
      product_id: string;
      policy_id: string;
      effective_version: string;
      policy_version: string;
      policy_digest: string;
      schema_version: string;
      catalog_generation: string;
      switch_generation: string;
      content_digest: string;
      replacement_locked: boolean;
      owner_type: AccountPolicySpec["owner_type"];
      owner_id: string;
      desired_effective_version: string | null;
      desired_digest: string | null;
    }>(`
      SELECT binding.id AS binding_id, binding.engine_account_id,
             binding.account_class, binding.product_id, binding.policy_id,
             version.effective_version::text, version.policy_version::text,
             version.policy_digest, version.schema_version::text,
             version.catalog_generation::text, version.switch_generation::text,
             version.content_digest, version.replacement_locked,
             source.owner_type, source.owner_id,
             binding.desired_effective_version::text, binding.desired_digest
      FROM account_policy_bindings binding
      JOIN account_policy_versions version
        ON version.binding_id = binding.id AND version.effective_version = $2
      JOIN pricing_policies source ON source.id = version.policy_id
      WHERE binding.engine_account_id = $1
      FOR UPDATE OF binding
    `, [policy.account_id, policy.effective_version]);
    const row = target.rows[0];
    if (!row) throw new Error("account policy version and binding must exist before its job is staged");
    const rules = await client.query<{
      rule_id: string;
      rule_digest: string;
      scope_type: "provider" | "model";
      provider_id: string;
      canonical_model_id: string | null;
      pricing_mode: "track" | "discount";
      rule_origin: "managed" | "legacy";
      discount_bps: number | null;
      payable_multiplier_bp: number;
      track_eligible: boolean;
      retention_eligible: boolean;
      commission_eligible: boolean;
    }>(`
      SELECT rule_id, rule_digest, scope_type, provider_id, canonical_model_id,
             pricing_mode, rule_origin, discount_bps, payable_multiplier_bp,
             track_eligible, retention_eligible, commission_eligible
      FROM account_policy_rules
      WHERE binding_id = $1 AND effective_version = $2
      ORDER BY provider_id, scope_type, COALESCE(canonical_model_id, ''), rule_id
    `, [row.binding_id, policy.effective_version]);
    const stored = accountPolicySpecSchema.parse({
      account_id: row.engine_account_id,
      effective_version: safeVersion(row.effective_version, "effective policy version"),
      policy_id: row.policy_id,
      policy_version: safeVersion(row.policy_version, "source policy version"),
      source_policy_digest: row.policy_digest,
      owner_type: row.owner_type,
      owner_id: row.owner_id,
      account_class: row.account_class,
      product_id: row.product_id,
      schema_version: safeVersion(row.schema_version, "policy schema version"),
      catalog_generation: safeVersion(row.catalog_generation, "policy catalog generation"),
      switch_generation: safeVersion(row.switch_generation, "policy switch generation"),
      content_digest: row.content_digest,
      replacement_locked: row.replacement_locked,
      rules: rules.rows.map((rule) => ({
        rule_id: rule.rule_id,
        rule_digest: rule.rule_digest,
        scope: rule.scope_type === "provider"
          ? { provider: { provider_id: rule.provider_id } }
          : { model: {
              provider_id: rule.provider_id,
              canonical_model_id: rule.canonical_model_id,
            } },
        pricing_mode: rule.pricing_mode,
        rule_origin: rule.rule_origin,
        discount_bps: rule.discount_bps,
        payable_multiplier_bp: rule.payable_multiplier_bp,
        track_eligible: rule.track_eligible,
        retention_eligible: rule.retention_eligible,
        commission_eligible: rule.commission_eligible,
      })),
    });
    const normalized = { ...policy, rules: [...policy.rules].sort((left, right) =>
      compareStringTuples(policyRuleKey(left), policyRuleKey(right))) };
    stored.rules.sort((left, right) =>
      compareStringTuples(policyRuleKey(left), policyRuleKey(right)));
    if (!sameJson(stored, normalized)) {
      throw new Error("policy control payload does not match the immutable commerce version");
    }
    const desiredVersion = row.desired_effective_version === null
      ? null
      : safeVersion(row.desired_effective_version, "desired effective policy version");
    if (desiredVersion !== null && desiredVersion > policy.effective_version) {
      throw new Error("account policy control target is stale");
    }
    if (
      desiredVersion === policy.effective_version &&
      row.desired_digest !== null && row.desired_digest !== policy.content_digest
    ) {
      throw new Error("account policy effective version already has a different desired digest");
    }
    await client.query(`
      UPDATE account_policy_bindings
      SET desired_effective_version = $2, desired_digest = $3,
          policy_enforcement = $4, funding_enforcement = $5,
          reconciliation_state = $6,
          sync_state = CASE
            WHEN applied_effective_version = $2 AND applied_digest = $3 THEN 'confirmed'
            ELSE 'pending'
          END,
          last_error = NULL, updated_at = now()
      WHERE id = $1
    `, [
      row.binding_id,
      policy.effective_version,
      policy.content_digest,
      binding.policy_enforcement,
      binding.funding_enforcement,
      binding.reconciliation_state,
    ]);
    const jobId = await insertImmutableJob(client, {
      table: "engine_policy_jobs",
      lookup: "binding_id = $1 AND effective_version = $2",
      lookupValues: [row.binding_id, policy.effective_version],
      insertSql: `
        INSERT INTO engine_policy_jobs (
          id, binding_id, effective_version, engine_account_id, policy_id,
          policy_version, catalog_generation, switch_generation, schema_version,
          content_digest, payload
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::jsonb)
      `,
      insertValues: [
        randomUUID(),
        row.binding_id,
        policy.effective_version,
        policy.account_id,
        policy.policy_id,
        policy.policy_version,
        policy.catalog_generation,
        policy.switch_generation,
        policy.schema_version,
        policy.content_digest,
        JSON.stringify({ policy: stored, binding }),
      ],
      payload: { policy: stored, binding },
    });
    await client.query("COMMIT");
    return jobId;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

async function insertImmutableJob(client: PoolClient, input: {
  table: "engine_catalog_jobs" | "engine_switch_jobs" | "engine_policy_jobs";
  lookup: string;
  lookupValues: unknown[];
  insertSql: string;
  insertValues: unknown[];
  payload: unknown;
}): Promise<string> {
  const existing = await client.query<{ id: string; payload: unknown }>(`
    SELECT id, payload FROM ${input.table} WHERE ${input.lookup} FOR UPDATE
  `, input.lookupValues);
  const row = existing.rows[0];
  if (row) {
    if (!sameJson(row.payload, input.payload)) {
      throw new Error(`${input.table} target already has a different immutable payload`);
    }
    return row.id;
  }
  await client.query(input.insertSql, input.insertValues);
  return String(input.insertValues[0]);
}

export async function recoverStalePricingControlJobs(database: Database): Promise<number> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const recovered = await recoverExpiredLeases(client);
    await client.query("COMMIT");
    return recovered;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function claimNextPricingControlJob(
  database: Database,
  workerId: string,
): Promise<ClaimedPricingControlJob | null> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await recoverExpiredLeases(client);
    await supersedeObsoleteJobs(client);

    const catalog = await claimCatalogJob(client, workerId);
    if (catalog) {
      await client.query("COMMIT");
      return catalog;
    }
    const switches = await claimSwitchJob(client, workerId);
    if (switches) {
      await client.query("COMMIT");
      return switches;
    }
    const policy = await claimPolicyJob(client, workerId);
    await client.query("COMMIT");
    return policy;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

async function recoverExpiredLeases(client: PoolClient): Promise<number> {
  let recovered = 0;
  for (const table of ["engine_catalog_jobs", "engine_switch_jobs", "engine_policy_jobs"] as const) {
    const result = await client.query(`
      UPDATE ${table}
      SET status = 'retry', locked_at = NULL, locked_by = NULL, next_attempt_at = now(),
          last_error = COALESCE(last_error, 'recovered expired pricing-control lease'),
          updated_at = now()
      WHERE status = 'processing'
        AND (locked_at IS NULL OR locked_at < now() - interval '5 minutes')
    `);
    recovered += result.rowCount ?? 0;
  }
  return recovered;
}

async function supersedeObsoleteJobs(client: PoolClient): Promise<void> {
  await client.query(`
    UPDATE engine_catalog_jobs job
    SET status = 'superseded', locked_at = NULL, locked_by = NULL,
        last_error = 'superseded by newer commerce catalog head', updated_at = now()
    FROM product_catalog_heads head
    WHERE head.product_id = job.product_id
      AND head.active_generation <> job.generation
      AND job.status IN ('pending', 'retry')
  `);
  await client.query(`
    UPDATE engine_switch_jobs job
    SET status = 'superseded', locked_at = NULL, locked_by = NULL,
        last_error = 'superseded by newer commerce switch head', updated_at = now()
    FROM provider_switch_head head
    WHERE head.singleton = 1
      AND head.active_generation <> job.generation
      AND job.status IN ('pending', 'retry')
  `);
  await client.query(`
    UPDATE engine_policy_jobs job
    SET status = 'superseded', locked_at = NULL, locked_by = NULL,
        last_error = 'superseded by newer effective account policy', updated_at = now()
    FROM account_policy_bindings binding
    WHERE binding.id = job.binding_id
      AND job.status IN ('pending', 'retry')
      AND (
        binding.desired_effective_version IS NULL
        OR binding.desired_digest IS NULL
        OR binding.desired_effective_version <> job.effective_version
        OR binding.desired_digest <> job.content_digest
      )
  `);
}

async function claimCatalogJob(
  client: PoolClient,
  workerId: string,
): Promise<ClaimedCatalogControlJob | null> {
  const result = await client.query<VersionedJobRow>(`
    WITH candidate AS (
      SELECT job.id
      FROM engine_catalog_jobs job
      JOIN product_catalog_heads head
        ON head.product_id = job.product_id
       AND head.active_generation = job.generation
      WHERE job.status IN ('pending', 'retry') AND job.next_attempt_at <= now()
      ORDER BY job.next_attempt_at, job.created_at
      FOR UPDATE OF job SKIP LOCKED
      LIMIT 1
    )
    UPDATE engine_catalog_jobs job
    SET status = 'processing', attempts = job.attempts + 1,
        locked_at = now(), locked_by = $1, updated_at = now()
    FROM candidate
    WHERE job.id = candidate.id
    RETURNING job.id, job.attempts, job.generation::text, job.schema_version::text,
              job.content_digest, job.payload
  `, [workerId]);
  const row = result.rows[0];
  if (!row) return null;
  const spec = catalogJobPayloadSchema.parse(row.payload);
  assertCatalogRow(row, spec);
  return { kind: "catalog", id: row.id, attempts: row.attempts, spec };
}

async function claimSwitchJob(
  client: PoolClient,
  workerId: string,
): Promise<ClaimedSwitchControlJob | null> {
  const result = await client.query<VersionedJobRow>(`
    WITH candidate AS (
      SELECT job.id
      FROM engine_switch_jobs job
      JOIN provider_switch_head head
        ON head.singleton = 1 AND head.active_generation = job.generation
      WHERE job.status IN ('pending', 'retry') AND job.next_attempt_at <= now()
        AND NOT EXISTS (
          SELECT 1
          FROM provider_switch_entries entry
          LEFT JOIN engine_catalog_jobs catalog_job
            ON catalog_job.product_id = entry.product_id
           AND catalog_job.generation = entry.catalog_generation
           AND catalog_job.status = 'confirmed'
          WHERE entry.generation = job.generation
            AND entry.catalog_generation IS NOT NULL
            AND catalog_job.id IS NULL
        )
      ORDER BY job.next_attempt_at, job.created_at
      FOR UPDATE OF job SKIP LOCKED
      LIMIT 1
    )
    UPDATE engine_switch_jobs job
    SET status = 'processing', attempts = job.attempts + 1,
        locked_at = now(), locked_by = $1, updated_at = now()
    FROM candidate
    WHERE job.id = candidate.id
    RETURNING job.id, job.attempts, job.generation::text, job.schema_version::text,
              job.content_digest, job.payload
  `, [workerId]);
  const row = result.rows[0];
  if (!row) return null;
  const spec = switchJobPayloadSchema.parse(row.payload);
  assertSwitchRow(row, spec);
  return { kind: "switches", id: row.id, attempts: row.attempts, spec };
}

async function claimPolicyJob(
  client: PoolClient,
  workerId: string,
): Promise<ClaimedPolicyControlJob | null> {
  const result = await client.query<PolicyJobRow>(`
    WITH candidate AS (
      SELECT job.id
      FROM engine_policy_jobs job
      JOIN account_policy_bindings binding
        ON binding.id = job.binding_id
       AND binding.desired_effective_version = job.effective_version
       AND binding.desired_digest = job.content_digest
      JOIN account_policy_versions policy
        ON policy.binding_id = job.binding_id
       AND policy.effective_version = job.effective_version
      JOIN engine_catalog_jobs catalog_job
        ON catalog_job.product_id = policy.product_id
       AND catalog_job.generation = job.catalog_generation
       AND catalog_job.status = 'confirmed'
      JOIN engine_switch_jobs switch_job
        ON switch_job.generation = job.switch_generation
       AND switch_job.status = 'confirmed'
      WHERE job.status IN ('pending', 'retry') AND job.next_attempt_at <= now()
      ORDER BY job.next_attempt_at, job.created_at
      FOR UPDATE OF job SKIP LOCKED
      LIMIT 1
    )
    UPDATE engine_policy_jobs job
    SET status = 'processing', attempts = job.attempts + 1,
        locked_at = now(), locked_by = $1, updated_at = now()
    FROM candidate
    WHERE job.id = candidate.id
    RETURNING job.id, job.binding_id, job.attempts, job.effective_version::text,
              job.policy_version::text, job.catalog_generation::text,
              job.switch_generation::text, job.schema_version::text,
              job.content_digest, job.engine_account_id, job.policy_id, job.payload
  `, [workerId]);
  const row = result.rows[0];
  if (!row) return null;
  const payload = policyControlJobPayloadSchema.parse(row.payload);
  assertPolicyRow(row, payload.policy);
  return {
    kind: "policy",
    id: row.id,
    attempts: row.attempts,
    bindingId: row.binding_id,
    spec: payload.policy,
    binding: payload.binding,
  };
}

export async function confirmPricingControlJob(
  database: Database,
  job: ClaimedPricingControlJob,
  ack: PricingMutationAck,
): Promise<void> {
  if (ack.result === "rejected" || (ack.result !== "applied" && ack.result !== "unchanged")) {
    throw new Error(`pricing-control job ${job.id} did not receive an activation ACK`);
  }
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    let updated = 0;
    if (job.kind === "catalog") {
      const identity = catalogActivationIdentitySchema.parse(ack.identity);
      if (JSON.stringify(identity.catalog) !== JSON.stringify(job.spec)) {
        throw new Error(`catalog ACK for job ${job.id} does not match its durable payload`);
      }
      const result = await client.query(`
        UPDATE engine_catalog_jobs
        SET status = 'confirmed', ack_generation = $2, ack_schema_version = $3,
            ack_content_digest = $4, ack_payload = $5::jsonb, confirmed_at = now(),
            locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
        WHERE id = $1 AND status = 'processing'
          AND generation = $2 AND schema_version = $3 AND content_digest = $4
      `, [
        job.id,
        job.spec.generation,
        job.spec.schema_version,
        job.spec.content_digest,
        JSON.stringify(ack),
      ]);
      updated = result.rowCount ?? 0;
    } else if (job.kind === "switches") {
      const identity = switchActivationIdentitySchema.parse(ack.identity);
      if (JSON.stringify(identity.switches) !== JSON.stringify(job.spec)) {
        throw new Error(`switch ACK for job ${job.id} does not match its durable payload`);
      }
      const result = await client.query(`
        UPDATE engine_switch_jobs
        SET status = 'confirmed', ack_generation = $2, ack_schema_version = $3,
            ack_content_digest = $4, ack_payload = $5::jsonb, confirmed_at = now(),
            locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
        WHERE id = $1 AND status = 'processing'
          AND generation = $2 AND schema_version = $3 AND content_digest = $4
      `, [
        job.id,
        job.spec.generation,
        job.spec.schema_version,
        job.spec.content_digest,
        JSON.stringify(ack),
      ]);
      updated = result.rowCount ?? 0;
    } else {
      const identity = policyActivationIdentitySchema.parse(ack.identity);
      if (
        JSON.stringify(identity.policy) !== JSON.stringify(job.spec) ||
        identity.activation.account_id !== job.spec.account_id ||
        identity.activation.effective_version !== job.spec.effective_version ||
        identity.activation.content_digest !== job.spec.content_digest ||
        JSON.stringify(identity.activation.binding) !== JSON.stringify(job.binding)
      ) {
        throw new Error(`policy ACK for job ${job.id} does not match its durable payload`);
      }
      const result = await client.query(`
        UPDATE engine_policy_jobs
        SET status = 'confirmed', ack_effective_version = $2, ack_policy_version = $3,
            ack_catalog_generation = $4, ack_switch_generation = $5,
            ack_schema_version = $6, ack_content_digest = $7, ack_payload = $8::jsonb,
            confirmed_at = now(), locked_at = NULL, locked_by = NULL,
            last_error = NULL, updated_at = now()
        WHERE id = $1 AND status = 'processing'
          AND effective_version = $2 AND policy_version = $3
          AND catalog_generation = $4 AND switch_generation = $5
          AND schema_version = $6 AND content_digest = $7
      `, [
        job.id,
        job.spec.effective_version,
        job.spec.policy_version,
        job.spec.catalog_generation,
        job.spec.switch_generation,
        job.spec.schema_version,
        job.spec.content_digest,
        JSON.stringify(ack),
      ]);
      updated = result.rowCount ?? 0;
      if (updated === 1) {
        await client.query(`
          UPDATE account_policy_bindings
          SET applied_effective_version = $2, applied_digest = $3, last_ack_at = now(),
              policy_enforcement = $4,
              funding_enforcement = $5,
              reconciliation_state = $6,
              sync_state = CASE
                WHEN desired_effective_version = $2 AND desired_digest = $3 THEN 'confirmed'
                ELSE 'pending'
              END,
              last_error = NULL, updated_at = now()
          WHERE id = $1
        `, [
          job.bindingId,
          job.spec.effective_version,
          job.spec.content_digest,
          job.binding.policy_enforcement,
          job.binding.funding_enforcement,
          job.binding.reconciliation_state,
        ]);
        await client.query(`
          UPDATE engine_accounts account
          SET status = 'active', last_error = NULL, updated_at = now()
          FROM account_policy_bindings binding
          WHERE binding.id = $1
            AND binding.user_id = account.user_id
            AND binding.engine_account_id = account.engine_account_id
            AND binding.sync_state = 'confirmed'
            AND binding.desired_effective_version = $2
            AND binding.applied_effective_version = $2
            AND binding.desired_digest = $3
            AND binding.applied_digest = $3
            AND account.status = 'pending'
        `, [job.bindingId, job.spec.effective_version, job.spec.content_digest]);
      }
    }
    if (updated !== 1) {
      throw new Error(`pricing-control job ${job.id} lost its processing lease`);
    }
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function releasePricingControlJob(
  database: Database,
  job: ClaimedPricingControlJob,
  disposition: PricingControlJobDisposition,
  error: string,
): Promise<void> {
  const delaySeconds = Math.min(3600, Math.max(5, 2 ** Math.min(job.attempts, 10)));
  const table = job.kind === "catalog"
    ? "engine_catalog_jobs"
    : job.kind === "switches" ? "engine_switch_jobs" : "engine_policy_jobs";
  const truncatedError = error.slice(0, 2000);
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query(`
      UPDATE ${table}
      SET status = $2,
          next_attempt_at = CASE
            WHEN $2 = 'retry' THEN now() + ($4 * interval '1 second')
            ELSE next_attempt_at
          END,
          locked_at = NULL, locked_by = NULL, last_error = $3, updated_at = now()
      WHERE id = $1 AND status = 'processing'
    `, [job.id, disposition, truncatedError, delaySeconds]);
    if ((result.rowCount ?? 0) !== 1) {
      throw new Error(`pricing-control job ${job.id} lost its processing lease`);
    }
    if (job.kind === "policy") {
      await client.query(`
        UPDATE account_policy_bindings
        SET sync_state = CASE WHEN $2 = 'dead' THEN 'failed' ELSE 'pending' END,
            last_error = $3, updated_at = now()
        WHERE id = $1 AND desired_effective_version = $4 AND desired_digest = $5
      `, [
        job.bindingId,
        disposition,
        truncatedError,
        job.spec.effective_version,
        job.spec.content_digest,
      ]);
    }
    await client.query("COMMIT");
  } catch (releaseError) {
    await client.query("ROLLBACK");
    throw releaseError;
  } finally {
    client.release();
  }
}
