import { Buffer } from "node:buffer";
import {
  MAIN_PRICING_PRODUCT_ID,
  PRICING_RELEASE_SCHEMA_VERSION_V2,
  pricingReleaseAssignmentExtensionV2Schema,
  pricingReleasePolicyV2Schema,
  type PricingReleaseAssignmentExtensionV2,
  type PricingReleaseHeadV2,
  type PricingReleasePolicyV2,
  type PricingReleaseV2,
} from "@claude-api/contracts";
import type { EngineClient } from "@claude-api/engine-client";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";
import { stage5V2CanonicalJson, stage5V2Digest } from "./pricing-release-digest.js";

export type PricingReleaseProvisioningEngineV2 = Pick<
  EngineClient,
  | "getPricingReleaseHeadV2"
  | "getPricingReleaseV2"
  | "getPricingReleaseRecoveryLinkV2"
  | "getPricingReleasePolicyV2"
  | "preparePricingReleasePolicyV2"
  | "getFundingNormalizationPlanV2"
  | "applyFundingNormalizationV2"
  | "getPricingReleaseAssignmentExtensionV2"
  | "preparePricingReleaseAssignmentExtensionV2"
>;

export type PricingReleaseProvisioningResultV2 =
  | { status: "pre_cutover"; headVersion: null; releaseGeneration: null }
  | { status: "base_assignment" | "extension"; headVersion: number; releaseGeneration: number };

/** SERIALIZABLE conflicts and deadlocks are concurrency facts, not failures: retry next tick. */
export function isSerializationConflictV2(message: string): boolean {
  return /could not serialize|deadlock detected/i.test(message);
}

export class PricingReleaseProvisioningV2Error extends Error {
  constructor(
    public readonly code:
      | "account_mapping_mismatch"
      | "active_release_missing"
      | "activation_receipt_missing"
      | "assignment_conflict"
      | "funding_not_ready"
      | "policy_not_ready"
      | "head_changed",
    message: string,
  ) {
    super(message);
    this.name = "PricingReleaseProvisioningV2Error";
  }
}

interface CommerceAccountV2 {
  customerType: "b2c" | "b2b";
  multiplierBp: number;
}

type ActivationKindV2 = "cutover" | "recovery" | "successor";

/**
 * A cutover and a successor advance both install the evidence TARGET as the active head — they
 * differ only in what they advance from (nothing vs a previous activation). Only a recovery
 * activates the paired recovery release, so every kind check here asks "is this the target lane?"
 * rather than "is this the initial cutover?".
 */
function installsTargetRelease(kind: ActivationKindV2): boolean {
  return kind !== "recovery";
}

interface ActivationPairV2 {
  activationKind: ActivationKindV2;
  targetGeneration: number;
  targetDigest: string;
  targetHeadVersion: number;
  recoveryGeneration: number;
  recoveryDigest: string;
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function positiveSafeInteger(value: string, label: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0 || String(parsed) !== value) {
    throw new PricingReleaseProvisioningV2Error("policy_not_ready", `${label} is not a positive safe integer`);
  }
  return parsed;
}

function sameCanonical(left: unknown, right: unknown): boolean {
  return stage5V2CanonicalJson(left) === stage5V2CanonicalJson(right);
}

async function commerceAccount(
  database: Database,
  userId: string,
  engineAccountId: string,
): Promise<CommerceAccountV2> {
  const result = await database.pool.query<{
    engine_account_id: string | null;
    customer_type: "b2c" | "b2b";
    multiplier_bp: number;
  }>(`
    SELECT account.engine_account_id, profile.customer_type, account.mult_bp AS multiplier_bp
    FROM engine_accounts account
    JOIN customer_profiles profile ON profile.user_id = account.user_id
    WHERE account.user_id = $1
  `, [userId]);
  const row = result.rows[0];
  if (!row || row.engine_account_id !== engineAccountId
      || !Number.isSafeInteger(row.multiplier_bp) || row.multiplier_bp < 0 || row.multiplier_bp > 10_000) {
    throw new PricingReleaseProvisioningV2Error(
      "account_mapping_mismatch",
      "commerce account mapping changed or has an invalid multiplier during release provisioning",
    );
  }
  return { customerType: row.customer_type, multiplierBp: row.multiplier_bp };
}

async function activationPair(
  database: Database,
  head: PricingReleaseHeadV2,
): Promise<ActivationPairV2> {
  const current = await database.pool.query<{
    activation_kind: ActivationKindV2;
    target_generation: string;
    target_digest: string;
    recovery_generation: string;
    recovery_digest: string;
  }>(`
    SELECT receipt.activation_kind,
           target.generation::text AS target_generation,
           target.engine_release_digest AS target_digest,
           recovery.generation::text AS recovery_generation,
           recovery.engine_release_digest AS recovery_digest
    FROM pricing_release_activation_receipts_v2 receipt
    JOIN pricing_stage8_evidence_v2 evidence
      ON evidence.evidence_digest = receipt.evidence_digest
    JOIN pricing_release_plans_v2 target
      ON target.generation = evidence.target_generation
     AND target.content_digest = evidence.target_digest
     AND target.release_kind = 'target'
    JOIN pricing_release_plans_v2 recovery
      ON recovery.generation = evidence.recovery_generation
     AND recovery.content_digest = evidence.recovery_digest
     AND recovery.release_kind = 'recovery'
    WHERE receipt.head_version = $1
      AND target.engine_release_digest IS NOT NULL
      AND recovery.engine_release_digest IS NOT NULL
      AND (
        (receipt.activation_kind IN ('cutover', 'successor')
         AND receipt.release_generation = target.generation
         AND receipt.release_digest = target.content_digest
         AND target.generation = $2
         AND target.engine_release_digest = $3)
        OR
        (receipt.activation_kind = 'recovery'
         AND receipt.release_generation = recovery.generation
         AND receipt.release_digest = recovery.content_digest
         AND recovery.generation = $2
         AND recovery.engine_release_digest = $3)
      )
  `, [head.head_version, head.active_generation, head.active_digest]);
  const row = current.rows[0];
  if (!row) {
    throw new PricingReleaseProvisioningV2Error(
      "activation_receipt_missing",
      "the active pricing head has no exact commerce activation receipt yet",
    );
  }
  const targetGeneration = positiveSafeInteger(row.target_generation, "target release generation");
  const recoveryGeneration = positiveSafeInteger(row.recovery_generation, "recovery release generation");
  if (recoveryGeneration <= targetGeneration) {
    throw new PricingReleaseProvisioningV2Error("activation_receipt_missing", "activation receipt has invalid release order");
  }
  const currentMatches = installsTargetRelease(row.activation_kind)
    ? head.active_generation === targetGeneration && head.active_digest === row.target_digest
    : head.active_generation === recoveryGeneration && head.active_digest === row.recovery_digest;
  if (!currentMatches) {
    throw new PricingReleaseProvisioningV2Error(
      "activation_receipt_missing",
      "activation receipt does not describe the exact active pricing head",
    );
  }
  let targetHeadVersion = head.head_version;
  if (row.activation_kind === "recovery") {
    // The activation that INSTALLED this target may have been the initial cutover or a later
    // successor advance; both are target-lane receipts and exactly one of them exists per target.
    const targetReceipt = await database.pool.query<{ head_version: string }>(`
      SELECT receipt.head_version::text
      FROM pricing_release_activation_receipts_v2 receipt
      JOIN pricing_release_plans_v2 target
        ON target.generation = receipt.release_generation
       AND target.content_digest = receipt.release_digest
       AND target.release_kind = 'target'
      WHERE receipt.activation_kind IN ('cutover', 'successor')
        AND target.generation = $1
        AND target.engine_release_digest = $2
      ORDER BY receipt.head_version DESC
      LIMIT 2
    `, [targetGeneration, row.target_digest]);
    if (targetReceipt.rows.length !== 1) {
      throw new PricingReleaseProvisioningV2Error(
        "activation_receipt_missing",
        "recovery head has no unique originating target activation receipt",
      );
    }
    targetHeadVersion = positiveSafeInteger(targetReceipt.rows[0]!.head_version, "target head version");
  }
  return {
    activationKind: row.activation_kind,
    targetGeneration,
    targetDigest: row.target_digest,
    targetHeadVersion,
    recoveryGeneration,
    recoveryDigest: row.recovery_digest,
  };
}

function extensionCoversHead(
  extension: PricingReleaseAssignmentExtensionV2,
  head: PricingReleaseHeadV2,
  accountId: string,
): boolean {
  if (extension.members.some((member) => member.assignment.account_id !== accountId)) return false;
  if (extension.provisioning_head_generation === head.active_generation
      && extension.provisioning_head_digest === head.active_digest) return true;
  return extension.paired_recovery_generation === head.active_generation
    && extension.paired_recovery_digest === head.active_digest;
}

async function existingExtension(
  engine: PricingReleaseProvisioningEngineV2,
  pair: ActivationPairV2,
  head: PricingReleaseHeadV2,
  accountId: string,
): Promise<PricingReleaseAssignmentExtensionV2 | null> {
  const versions = pair.activationKind === "recovery"
    ? [pair.targetHeadVersion, head.head_version]
    : [head.head_version];
  for (const headVersion of [...new Set(versions)]) {
    const extension = await engine.getPricingReleaseAssignmentExtensionV2(headVersion, accountId);
    if (extension && extensionCoversHead(extension, head, accountId)) return extension;
  }
  return null;
}

async function normalizedFundingGeneration(
  engine: PricingReleaseProvisioningEngineV2,
  accountId: string,
): Promise<number> {
  for (let attempt = 0; attempt < 4; attempt += 1) {
    const plan = await engine.getFundingNormalizationPlanV2(accountId);
    if (!plan) {
      throw new PricingReleaseProvisioningV2Error("funding_not_ready", "engine account disappeared during funding normalization");
    }
    if (plan.status === "normalized" && plan.funding_generation !== null && plan.funding_head_version !== null) {
      return plan.funding_generation;
    }
    if (plan.status === "blocked" || plan.normalization_digest === null || plan.funding_generation === null) {
      throw new PricingReleaseProvisioningV2Error(
        "funding_not_ready",
        `funding normalization is ${plan.status}${plan.blockers[0] ? `: ${plan.blockers[0].code}` : ""}`,
      );
    }
    const applied = await engine.applyFundingNormalizationV2(accountId, {
      expected_source_state_digest: plan.source_state_digest,
      expected_normalization_digest: plan.normalization_digest,
    });
    if (!applied) {
      throw new PricingReleaseProvisioningV2Error("funding_not_ready", "engine account disappeared during funding apply");
    }
    if ((applied.status === "stored" || applied.status === "unchanged")
        && applied.normalization.status === "normalized"
        && applied.normalization.funding_generation !== null
        && applied.normalization.funding_head_version !== null) {
      return applied.normalization.funding_generation;
    }
    if (applied.status === "blocked") {
      throw new PricingReleaseProvisioningV2Error("funding_not_ready", "engine rejected the account-local funding plan as blocked");
    }
  }
  throw new PricingReleaseProvisioningV2Error("funding_not_ready", "funding state kept changing during normalization");
}

async function readStoredPolicy(
  client: PoolClient,
  policyId: string,
  policyVersion: number,
): Promise<PricingReleasePolicyV2 | null> {
  const header = await client.query<{
    policy_id: string;
    policy_version: string;
    owner_type: "global_b2c" | "b2b_client" | "openkeys" | "service";
    owner_id: string;
    account_class: "b2c" | "b2b" | "openkeys" | "service";
    product_id: string | null;
    billing_mode: "balance" | "meter_only";
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
  `, [policyId, policyVersion]);
  const row = header.rows[0];
  if (!row) return null;
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
    ORDER BY scope_type COLLATE "C", COALESCE(provider_id, '') COLLATE "C",
             COALESCE(canonical_model_id, '') COLLATE "C", rule_id COLLATE "C"
  `, [policyId, policyVersion]);
  return pricingReleasePolicyV2Schema.parse({
    policy_id: row.policy_id,
    policy_version: positiveSafeInteger(row.policy_version, "policy version"),
    owner_type: row.owner_type === "openkeys" ? "open_keys" : row.owner_type,
    owner_id: row.owner_id,
    account_class: row.account_class === "openkeys" ? "open_keys" : row.account_class,
    product_id: row.product_id,
    billing_mode: row.billing_mode,
    schema_version: positiveSafeInteger(row.schema_version, "policy schema version"),
    capability_generation: positiveSafeInteger(row.capability_generation, "policy capability generation"),
    capability_digest: row.capability_digest,
    catalog_generation: row.catalog_generation === null
      ? null
      : positiveSafeInteger(row.catalog_generation, "policy catalog generation"),
    catalog_digest: row.catalog_digest,
    switch_generation: row.switch_generation === null
      ? null
      : positiveSafeInteger(row.switch_generation, "policy switch generation"),
    switch_digest: row.switch_digest,
    content_digest: row.content_digest,
    rules: rules.rows.map((rule) => ({
      rule_id: rule.rule_id,
      rule_digest: rule.rule_digest,
      scope: rule.scope_type === "global"
        ? { scope: "global" as const }
        : rule.scope_type === "provider"
          ? { scope: "provider" as const, provider_id: rule.provider_id! }
          : {
              scope: "model" as const,
              provider_id: rule.provider_id!,
              canonical_model_id: rule.canonical_model_id!,
            },
      discount_bps: Number(rule.discount_bps),
      payable_multiplier_bp: Number(rule.payable_multiplier_bp),
    })),
  });
}

async function storePolicy(client: PoolClient, policy: PricingReleasePolicyV2): Promise<void> {
  await client.query(`
    INSERT INTO pricing_policy_documents_v2 (
      policy_id, policy_version, owner_type, owner_id, account_class,
      product_id, billing_mode, schema_version, capability_generation,
      capability_digest, catalog_generation, catalog_digest,
      switch_generation, switch_digest, content_digest
    ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
    ON CONFLICT (policy_id, policy_version) DO NOTHING
  `, [
    policy.policy_id,
    policy.policy_version,
    policy.owner_type === "open_keys" ? "openkeys" : policy.owner_type,
    policy.owner_id,
    policy.account_class === "open_keys" ? "openkeys" : policy.account_class,
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
    await client.query(`
      INSERT INTO pricing_policy_rules_v2 (
        policy_id, policy_version, rule_id, rule_digest, scope_type,
        provider_id, canonical_model_id, discount_bps, payable_multiplier_bp
      ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
      ON CONFLICT (policy_id, policy_version, rule_id) DO NOTHING
    `, [
      policy.policy_id,
      policy.policy_version,
      rule.rule_id,
      rule.rule_digest,
      rule.scope.scope,
      rule.scope.scope === "global" ? null : rule.scope.provider_id,
      rule.scope.scope === "model" ? rule.scope.canonical_model_id : null,
      rule.discount_bps,
      rule.payable_multiplier_bp,
    ]);
  }
  const stored = await readStoredPolicy(client, policy.policy_id, policy.policy_version);
  if (!stored || !sameCanonical(stored, policy)) {
    throw new PricingReleaseProvisioningV2Error(
      "policy_not_ready",
      `stored release-v2 policy ${policy.policy_id}/${policy.policy_version} conflicts with provisioning semantics`,
    );
  }
}

function buildPolicyRule(input: {
  ruleId: string;
  scope: PricingReleasePolicyV2["rules"][number]["scope"];
  discountBps: number;
}): PricingReleasePolicyV2["rules"][number] {
  const base = {
    rule_id: input.ruleId,
    scope: input.scope,
    discount_bps: input.discountBps,
    payable_multiplier_bp: 10_000 - input.discountBps,
  };
  return { ...base, rule_digest: stage5V2Digest("policy-rule", base) };
}

async function dynamicB2bPolicy(
  database: Database,
  userId: string,
  engineAccountId: string,
  multiplierBp: number,
  release: PricingReleaseV2,
): Promise<PricingReleasePolicyV2> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))", [`pricing-release-v2:policy:${engineAccountId}`]);
    const source = await client.query<{
      policy_id: string;
      current_version: string;
    }>(`
      SELECT policy.id AS policy_id, head.current_version::text
      FROM pricing_policies policy
      JOIN pricing_policy_heads head ON head.policy_id = policy.id
      WHERE policy.owner_type = 'b2b_client'
        AND policy.owner_id = $1
        AND policy.product_id = $2
        AND policy.status = 'active'
      FOR SHARE OF policy, head
    `, [userId, MAIN_PRICING_PRODUCT_ID]);
    let policyVersion = 1;
    let rules: PricingReleasePolicyV2["rules"];
    if (source.rows[0]) {
      policyVersion = positiveSafeInteger(source.rows[0].current_version, "source B2B policy version");
      const sourceRules = await client.query<{
        rule_id: string;
        scope_type: "provider" | "model";
        provider_id: string;
        canonical_model_id: string | null;
        pricing_mode: "track" | "discount";
        discount_bps: number | null;
        payable_multiplier_bp: number | null;
      }>(`
        SELECT rule_id, scope_type, provider_id, canonical_model_id,
               pricing_mode, discount_bps, payable_multiplier_bp
        FROM pricing_policy_rules
        WHERE policy_id = $1 AND policy_version = $2
        ORDER BY provider_id COLLATE "C", scope_type COLLATE "C",
                 COALESCE(canonical_model_id, '') COLLATE "C", rule_id COLLATE "C"
      `, [source.rows[0].policy_id, policyVersion]);
      if (sourceRules.rows.length === 0 || sourceRules.rows.some((rule) =>
        rule.pricing_mode !== "discount"
        || rule.discount_bps === null
        || rule.payable_multiplier_bp !== 10_000 - rule.discount_bps)) {
        throw new PricingReleaseProvisioningV2Error(
          "policy_not_ready",
          "B2B source policy contains legacy track or incomplete discount semantics",
        );
      }
      rules = sourceRules.rows.map((rule) => buildPolicyRule({
        ruleId: rule.rule_id,
        scope: rule.scope_type === "provider"
          ? { scope: "provider", provider_id: rule.provider_id }
          : {
              scope: "model",
              provider_id: rule.provider_id,
              canonical_model_id: rule.canonical_model_id!,
            },
        discountBps: rule.discount_bps!,
      }));
    } else {
      rules = [buildPolicyRule({
        ruleId: "anthropic",
        scope: { scope: "provider", provider_id: "anthropic" },
        discountBps: 10_000 - multiplierBp,
      })];
    }
    rules.sort((left, right) =>
      compareUtf8(stage5V2CanonicalJson(left.scope), stage5V2CanonicalJson(right.scope))
      || compareUtf8(left.rule_id, right.rule_id));
    const base = {
      policy_id: `release-v2:b2b:${engineAccountId}`,
      policy_version: policyVersion,
      owner_type: "b2b_client" as const,
      owner_id: userId,
      account_class: "b2b" as const,
      product_id: MAIN_PRICING_PRODUCT_ID,
      billing_mode: "balance" as const,
      schema_version: PRICING_RELEASE_SCHEMA_VERSION_V2,
      capability_generation: release.capability_generation,
      capability_digest: release.capability_digest,
      catalog_generation: release.main_catalog_generation,
      catalog_digest: release.main_catalog_digest,
      switch_generation: release.switch_generation,
      switch_digest: release.switch_digest,
      rules,
    };
    const policy = pricingReleasePolicyV2Schema.parse({
      ...base,
      content_digest: stage5V2Digest("policy", base),
    });
    await storePolicy(client, policy);
    await client.query("COMMIT");
    return policy;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

async function policyForAccount(
  database: Database,
  engine: PricingReleaseProvisioningEngineV2,
  account: CommerceAccountV2,
  userId: string,
  engineAccountId: string,
  release: PricingReleaseV2,
): Promise<PricingReleasePolicyV2> {
  const client = await database.pool.connect();
  let policy: PricingReleasePolicyV2 | null = null;
  let expectedPolicyDigest: string | null = null;
  try {
    if (account.customerType === "b2c") {
      const candidates = new Map<string, { policyId: string; policyVersion: number; policyDigest: string }>();
      for (const assignment of release.assignments.filter((item) => item.account_class === "b2c")) {
        candidates.set(
          `${assignment.policy_id}\0${assignment.policy_version}\0${assignment.policy_digest}`,
          {
            policyId: assignment.policy_id,
            policyVersion: assignment.policy_version,
            policyDigest: assignment.policy_digest,
          },
        );
      }
      if (candidates.size !== 1) {
        throw new PricingReleaseProvisioningV2Error(
          "policy_not_ready",
          "active release has no unique assignment-pinned global B2C policy",
        );
      }
      const candidate = [...candidates.values()][0]!;
      expectedPolicyDigest = candidate.policyDigest;
      policy = await readStoredPolicy(
        client,
        candidate.policyId,
        candidate.policyVersion,
      );
    } else {
      const snapshots = await client.query<{ policy_id: string; policy_version: string; policy_digest: string }>(`
        SELECT snapshot.policy_id, snapshot.policy_version::text, snapshot.policy_digest
        FROM business_invites invitation
        JOIN business_invite_policy_snapshots_v2 snapshot ON snapshot.invite_id = invitation.id
        WHERE invitation.consumed_by_user_id = $1
        ORDER BY invitation.created_at DESC
        LIMIT 2
      `, [userId]);
      if (snapshots.rows.length > 1) {
        throw new PricingReleaseProvisioningV2Error("policy_not_ready", "B2B user has ambiguous release-v2 invitation snapshots");
      }
      if (snapshots.rows[0]) {
        expectedPolicyDigest = snapshots.rows[0].policy_digest;
        policy = await readStoredPolicy(
          client,
          snapshots.rows[0].policy_id,
          positiveSafeInteger(snapshots.rows[0].policy_version, "B2B invitation policy version"),
        );
      }
    }
  } finally {
    client.release();
  }
  if (!policy && account.customerType === "b2b") {
    policy = await dynamicB2bPolicy(database, userId, engineAccountId, account.multiplierBp, release);
  }
  if (!policy || policy.account_class !== account.customerType || policy.billing_mode !== "balance") {
    throw new PricingReleaseProvisioningV2Error("policy_not_ready", "pricing release policy does not match the commerce account class");
  }
  if (policy.product_id !== MAIN_PRICING_PRODUCT_ID
      || policy.capability_generation !== release.capability_generation
      || policy.capability_digest !== release.capability_digest
      || policy.catalog_generation !== release.main_catalog_generation
      || policy.catalog_digest !== release.main_catalog_digest
      || policy.switch_generation !== release.switch_generation
      || policy.switch_digest !== release.switch_digest) {
    throw new PricingReleaseProvisioningV2Error(
      "policy_not_ready",
      "pricing release policy lineage does not match the exact active release",
    );
  }
  if (expectedPolicyDigest !== null && policy.content_digest !== expectedPolicyDigest) {
    throw new PricingReleaseProvisioningV2Error("policy_not_ready", "pricing release policy differs from its pinned assignment authority");
  }
  const prepared = await engine.preparePricingReleasePolicyV2(policy);
  if (prepared.result !== "stored" && prepared.result !== "unchanged") {
    const result = prepared.result === "rejected" ? prepared.code : prepared.result;
    throw new PricingReleaseProvisioningV2Error(
      "policy_not_ready",
      `engine rejected pricing release policy with ${result}`,
    );
  }
  const readback = await engine.getPricingReleasePolicyV2(policy.policy_id, policy.policy_version);
  if (!readback || !sameCanonical(readback, policy)) {
    throw new PricingReleaseProvisioningV2Error("policy_not_ready", "engine policy readback differs from commerce authority");
  }
  return policy;
}

function buildExtension(input: {
  head: PricingReleaseHeadV2;
  pair: ActivationPairV2;
  accountId: string;
  accountClass: "b2c" | "b2b";
  fundingGeneration: number;
  policy: PricingReleasePolicyV2;
}): PricingReleaseAssignmentExtensionV2 {
  const paired = installsTargetRelease(input.pair.activationKind);
  const generations = paired
    ? [input.head.active_generation, input.pair.recoveryGeneration]
    : [input.head.active_generation];
  const assignmentSemantics = {
    account_id: input.accountId,
    account_class: input.accountClass,
    policy_id: input.policy.policy_id,
    policy_version: input.policy.policy_version,
    policy_digest: input.policy.content_digest,
    billing_mode: "balance" as const,
    funding_generation: input.fundingGeneration,
    purpose: null,
    responsible: null,
  };
  const members = generations.map((releaseGeneration) => {
    const assignment = {
      ...assignmentSemantics,
      assignment_digest: stage5V2Digest("assignment-extension-assignment", {
        release_generation: releaseGeneration,
        ...assignmentSemantics,
      }),
    };
    return {
      release_generation: releaseGeneration,
      assignment,
      extension_digest: stage5V2Digest("assignment-extension-member", {
        release_generation: releaseGeneration,
        assignment,
      }),
    };
  });
  const group = {
    provisioning_head_generation: input.head.active_generation,
    provisioning_head_digest: input.head.active_digest,
    provisioning_head_version: input.head.head_version,
    paired_recovery_generation: paired ? input.pair.recoveryGeneration : null,
    paired_recovery_digest: paired ? input.pair.recoveryDigest : null,
    members,
  };
  return pricingReleaseAssignmentExtensionV2Schema.parse({
    ...group,
    extension_group_digest: stage5V2Digest("assignment-extension-group", group),
  });
}

/**
 * Completes the post-cutover account-local chain before a raw customer key may be returned.
 * A null head is the only pre-cutover bypass. Once a head exists, every success is backed by an
 * immutable base assignment or an exact assignment-extension GET readback for the active release.
 */
export async function ensurePricingReleaseProvisioningV2(
  database: Database,
  engine: PricingReleaseProvisioningEngineV2,
  input: { userId: string; engineAccountId: string },
): Promise<PricingReleaseProvisioningResultV2> {
  let observedHead = false;
  for (let attempt = 0; attempt < 4; attempt += 1) {
    const head = await engine.getPricingReleaseHeadV2();
    if (!head) {
      if (observedHead) {
        throw new PricingReleaseProvisioningV2Error("head_changed", "pricing release head disappeared during provisioning");
      }
      return { status: "pre_cutover", headVersion: null, releaseGeneration: null };
    }
    observedHead = true;
    const release = await engine.getPricingReleaseV2(head.active_generation);
    if (!release || release.content_digest !== head.active_digest) {
      throw new PricingReleaseProvisioningV2Error("active_release_missing", "active release readback does not match its head");
    }
    const base = release.assignments.find((assignment) => assignment.account_id === input.engineAccountId);
    if (base) {
      const account = await commerceAccount(database, input.userId, input.engineAccountId);
      if (base.account_class === account.customerType && base.billing_mode === "balance") {
        return {
          status: "base_assignment",
          headVersion: head.head_version,
          releaseGeneration: head.active_generation,
        };
      }
      // A post-cutover B2C→B2B conversion supersedes the immutable b2c base assignment through
      // the append-only class-changing assignment extension — fall through to the extension
      // readback/provisioning path instead of failing key issuance forever. Any other mismatch
      // is a genuine conflict.
      if (!(base.account_class === "b2c" && account.customerType === "b2b" && base.billing_mode === "balance")) {
        throw new PricingReleaseProvisioningV2Error("assignment_conflict", "base assignment conflicts with commerce ownership");
      }
    }

    const pair = await activationPair(database, head);
    const expectedReleaseKind = installsTargetRelease(pair.activationKind) ? "target" : "recovery";
    if (release.release_kind !== expectedReleaseKind) {
      throw new PricingReleaseProvisioningV2Error(
        "activation_receipt_missing",
        "active release kind does not match its exact activation receipt",
      );
    }
    const alreadyStored = await existingExtension(engine, pair, head, input.engineAccountId);
    if (alreadyStored) {
      const account = await commerceAccount(database, input.userId, input.engineAccountId);
      const activeMember = alreadyStored.members.find((member) =>
        member.release_generation === head.active_generation);
      if (!activeMember
          || activeMember.assignment.account_class !== account.customerType
          || activeMember.assignment.billing_mode !== "balance") {
        throw new PricingReleaseProvisioningV2Error(
          "assignment_conflict",
          "stored assignment extension conflicts with commerce ownership",
        );
      }
      return { status: "extension", headVersion: head.head_version, releaseGeneration: head.active_generation };
    }
    const account = await commerceAccount(database, input.userId, input.engineAccountId);
    const fundingGeneration = await normalizedFundingGeneration(engine, input.engineAccountId);
    const policy = await policyForAccount(database, engine, account, input.userId, input.engineAccountId, release);

    if (installsTargetRelease(pair.activationKind)) {
      const link = await engine.getPricingReleaseRecoveryLinkV2(pair.targetGeneration, pair.recoveryGeneration);
      if (!link || link.target_digest !== pair.targetDigest || link.recovery_digest !== pair.recoveryDigest) {
        throw new PricingReleaseProvisioningV2Error("activation_receipt_missing", "engine recovery link differs from activation evidence");
      }
    }
    const extension = buildExtension({
      head,
      pair,
      accountId: input.engineAccountId,
      accountClass: account.customerType,
      fundingGeneration,
      policy,
    });
    const prepared = await engine.preparePricingReleaseAssignmentExtensionV2(extension);
    if (prepared.result === "rejected") {
      if (prepared.code === "stale" || prepared.code === "missing_dependency") continue;
      throw new PricingReleaseProvisioningV2Error(
        "assignment_conflict",
        `engine rejected assignment extension with ${prepared.code}`,
      );
    }
    const readback = await engine.getPricingReleaseAssignmentExtensionV2(
      extension.provisioning_head_version,
      input.engineAccountId,
    );
    if (!readback || !sameCanonical(readback, extension)) {
      throw new PricingReleaseProvisioningV2Error("assignment_conflict", "assignment extension readback differs from the request");
    }
    const finalHead = await engine.getPricingReleaseHeadV2();
    if (finalHead && extensionCoversHead(readback, finalHead, input.engineAccountId)) {
      return {
        status: "extension",
        headVersion: finalHead.head_version,
        releaseGeneration: finalHead.active_generation,
      };
    }
  }
  throw new PricingReleaseProvisioningV2Error("head_changed", "pricing release head kept changing during key provisioning");
}

export type PricingReleasePolicyOverrideResultV2 =
  | { status: "pre_cutover" }
  | { status: "not_covered" }
  | { status: "unchanged" | "override"; headVersion: number; policyVersion: number };

/**
 * Propagates an operator CAS replacement of a live B2B commerce policy head into the release-v2
 * authority for an account already covered by the active release's base manifest. The immutable
 * base assignment is never rewritten: a strictly newer release policy version is prepared and
 * pinned through the append-only assignment extension, and the resolver prefers the extension.
 */
export async function syncPricingReleasePolicyOverrideV2(
  database: Database,
  engine: PricingReleaseProvisioningEngineV2,
  input: { userId: string; engineAccountId: string },
): Promise<PricingReleasePolicyOverrideResultV2> {
  const head = await engine.getPricingReleaseHeadV2();
  if (!head) return { status: "pre_cutover" };
  const release = await engine.getPricingReleaseV2(head.active_generation);
  if (!release || release.content_digest !== head.active_digest) {
    throw new PricingReleaseProvisioningV2Error(
      "active_release_missing",
      "active release readback does not match its head",
    );
  }
  const account = await commerceAccount(database, input.userId, input.engineAccountId);
  if (account.customerType !== "b2b") {
    throw new PricingReleaseProvisioningV2Error(
      "assignment_conflict",
      "release policy override is only defined for B2B accounts",
    );
  }
  const base = release.assignments.find((assignment) => assignment.account_id === input.engineAccountId);
  if (!base) return { status: "not_covered" };
  // A converted account keeps its immutable b2c base assignment forever; the override then
  // carries the class change (b2c base → b2b extension). Every other class/billing/funding
  // shape remains a hard conflict.
  const classChange = base.account_class === "b2c";
  if (!(base.account_class === "b2b" || classChange)
      || base.billing_mode !== "balance"
      || base.funding_generation === null) {
    throw new PricingReleaseProvisioningV2Error(
      "assignment_conflict",
      "base assignment conflicts with B2B balance ownership",
    );
  }
  const policy = await dynamicB2bPolicy(database, input.userId, input.engineAccountId, account.multiplierBp, release);
  if (base.policy_digest === policy.content_digest) {
    return { status: "unchanged", headVersion: head.head_version, policyVersion: base.policy_version };
  }
  // The version-advance guard is meaningful only within one policy lineage; a class change
  // starts the per-account b2b lineage, whose version 1 is not comparable to the global b2c
  // base version.
  if (base.policy_id === policy.policy_id && policy.policy_version <= base.policy_version) {
    throw new PricingReleaseProvisioningV2Error(
      "policy_not_ready",
      `release policy override must advance beyond base version ${base.policy_version}`,
    );
  }
  if (policy.product_id !== MAIN_PRICING_PRODUCT_ID
      || policy.capability_generation !== release.capability_generation
      || policy.capability_digest !== release.capability_digest
      || policy.catalog_generation !== release.main_catalog_generation
      || policy.catalog_digest !== release.main_catalog_digest
      || policy.switch_generation !== release.switch_generation
      || policy.switch_digest !== release.switch_digest) {
    throw new PricingReleaseProvisioningV2Error(
      "policy_not_ready",
      "pricing release policy lineage does not match the exact active release",
    );
  }
  const preparedPolicy = await engine.preparePricingReleasePolicyV2(policy);
  if (preparedPolicy.result !== "stored" && preparedPolicy.result !== "unchanged") {
    const result = preparedPolicy.result === "rejected" ? preparedPolicy.code : preparedPolicy.result;
    throw new PricingReleaseProvisioningV2Error(
      "policy_not_ready",
      `engine rejected pricing release policy with ${result}`,
    );
  }
  const policyReadback = await engine.getPricingReleasePolicyV2(policy.policy_id, policy.policy_version);
  if (!policyReadback || !sameCanonical(policyReadback, policy)) {
    throw new PricingReleaseProvisioningV2Error(
      "policy_not_ready",
      "engine policy readback differs from commerce authority",
    );
  }
  const pair = await activationPair(database, head);
  const extension = buildExtension({
    head,
    pair,
    accountId: input.engineAccountId,
    accountClass: "b2b",
    fundingGeneration: base.funding_generation,
    policy,
  });
  const prepared = await engine.preparePricingReleaseAssignmentExtensionV2(extension);
  if (prepared.result === "rejected") {
    if (prepared.code === "stale") {
      throw new PricingReleaseProvisioningV2Error("head_changed", "pricing release head changed during the policy override");
    }
    throw new PricingReleaseProvisioningV2Error(
      "assignment_conflict",
      `engine rejected the policy override extension with ${prepared.code}`,
    );
  }
  const readback = await engine.getPricingReleaseAssignmentExtensionV2(
    extension.provisioning_head_version,
    input.engineAccountId,
  );
  if (!readback || !sameCanonical(readback, extension)) {
    throw new PricingReleaseProvisioningV2Error(
      "assignment_conflict",
      "policy override extension readback differs from the request",
    );
  }
  return { status: "override", headVersion: head.head_version, policyVersion: policy.policy_version };
}
