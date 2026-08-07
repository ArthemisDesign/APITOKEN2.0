import { Buffer } from "node:buffer";
import { createHash, randomUUID } from "node:crypto";
import type {
  AccountPolicyBinding,
  AccountPolicySpec,
  PricingCatalogSpec,
  ProviderSwitchSpec,
} from "@claude-api/contracts";
import {
  MAIN_PRICING_PRODUCT_ID,
  MULTI_DISCOUNT_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  OPENKEYS_PRICING_PRODUCT_ID,
} from "@claude-api/contracts";
import { buildOfficialOpenKeysPolicy } from "@claude-api/engine-client";
import type { PoolClient } from "pg";
import { z } from "zod";
import type { Database } from "./client.js";
import { stage5Digest, STAGE5_CATALOG_MODELS } from "./pricing-release-digest.js";

export { stage5Digest, STAGE5_CATALOG_MODELS } from "./pricing-release-digest.js";

type AccountPolicyRule = AccountPolicySpec["rules"][number];

export const STAGE5_SCHEMA_VERSION = MULTI_DISCOUNT_SCHEMA_VERSION;
export const STAGE5_CAPABILITY_GENERATION = MULTI_DISCOUNT_CAPABILITY_GENERATION;
export const STAGE5_CAPABILITY_DIGEST = MULTI_DISCOUNT_CAPABILITY_DIGEST;
export const STAGE5_MAIN_PRODUCT_ID = MAIN_PRICING_PRODUCT_ID;
export const STAGE5_OPENKEYS_PRODUCT_ID = OPENKEYS_PRICING_PRODUCT_ID;

const inventoryAccountSchema = z.object({
  account_id: z.string().startsWith("acct_").max(200),
  multiplier_bp: z.number().int().min(0).max(100_000),
  status: z.enum(["active", "disabled"]),
}).strict();

const openKeysInventoryAccountSchema = z.object({
  source_id: z.string().trim().min(1).max(200),
  account_id: z.string().startsWith("acct_").max(200),
  multiplier_bp: z.number().int().min(0).max(100_000),
  status: z.enum(["active", "disabled"]),
  pricing_contract: z.enum(["legacy", "official_1_to_1"]),
}).strict();

export const stage5InventorySchema = z.object({
  schema_version: z.literal(STAGE5_SCHEMA_VERSION),
  engine_accounts: z.array(inventoryAccountSchema),
  openkeys_accounts: z.array(openKeysInventoryAccountSchema),
}).strict();

export type Stage5Inventory = z.infer<typeof stage5InventorySchema>;

export interface Stage5SourceRule {
  rule_id: string;
  rule_digest: string;
  scope_type: "provider" | "model";
  provider_id: string;
  canonical_model_id: string | null;
  pricing_mode: "track" | "discount";
  rule_origin: "managed" | "legacy";
  discount_bps: number | null;
  payable_multiplier_bp: number | null;
  track_eligible: boolean;
  retention_eligible: boolean;
  commission_eligible: boolean;
}

export interface Stage5SourcePolicy {
  policy_id: string;
  owner_type: "global_b2c" | "b2b_client" | "b2b_invitation" | "service";
  owner_id: string;
  product_id: string;
  replacement_locked: boolean;
  version: number;
  content_digest: string;
  rules: Stage5SourceRule[];
}

export interface Stage5CommerceAccountSnapshot {
  user_id: string;
  engine_account_record_id: string;
  engine_account_id: string;
  account_class: "b2c" | "b2b";
  profile_multiplier_bp: number;
  commerce_multiplier_bp: number;
  commerce_status: "pending" | "active" | "error" | "disabled";
}

export interface Stage5InvitationSnapshot {
  invite_id: string;
  multiplier_bp: number;
  expires_at: string;
}

export interface Stage5AccountPlan {
  binding_id: string;
  user_id: string | null;
  engine_account_record_id: string | null;
  engine_account_id: string;
  account_class: "b2c" | "b2b" | "service";
  source_multiplier_bp: number;
  source_policy: Stage5SourcePolicy;
  effective_policy: AccountPolicySpec;
  binding: AccountPolicyBinding;
}

export interface Stage5InvitationPlan {
  invite_id: string;
  source_multiplier_bp: number;
  policy: Stage5SourcePolicy;
}

export interface Stage5OpenKeysPlan {
  source_id: string;
  account_id: string;
  status: "active" | "disabled";
  pricing_contract: "legacy" | "official_1_to_1";
  source_multiplier_bp: number;
  effective_policy: AccountPolicySpec | null;
  exception_code: string | null;
}

export interface Stage5Blocker {
  scope: "safe" | "protected";
  code: string;
  subject_id: string;
  detail: string;
}

export interface Stage5AssignmentReference {
  account_id: string;
  source_id: string;
  source_multiplier_bp: number;
  policy_id: string | null;
  policy_digest: string | null;
  exception_code: string | null;
}

export interface Stage5AssignmentMatrixDraft {
  schema_version: 1;
  plan_digest: string;
  b2b: Stage5AssignmentReference[];
  openkeys: Stage5AssignmentReference[];
  unresolved_engine_accounts: string[];
  content_digest: string;
}

export interface Stage5BackfillPlan {
  schema_version: 1;
  capability: {
    generation: 1;
    schema_version: 1;
    content_digest: string;
    entries: Array<{
      provider_id: string;
      canonical_model_id: string;
      entry_digest: string;
      capability_data: Record<string, unknown>;
    }>;
    aliases: Array<{
      provider_id: string;
      alias_model_id: string;
      canonical_model_id: string;
    }>;
  };
  catalogs: PricingCatalogSpec[];
  switches: ProviderSwitchSpec;
  safe: {
    b2c_accounts: Stage5AccountPlan[];
    invitations: Stage5InvitationPlan[];
  };
  protected: {
    b2b_accounts: Stage5AccountPlan[];
    openkeys_accounts: Stage5OpenKeysPlan[];
    unresolved_engine_accounts: string[];
  };
  blockers: Stage5Blocker[];
  inventory_digest: string;
  source_digest: string;
  plan_digest: string;
  assignment_matrix_draft: Stage5AssignmentMatrixDraft;
}

const serviceRuleSchema = z.object({
  rule_id: z.string().trim().min(1).max(200),
  scope: z.union([
    z.object({ provider: z.object({ provider_id: z.string().trim().min(1).max(200) }).strict() }).strict(),
    z.object({ model: z.object({
      provider_id: z.string().trim().min(1).max(200),
      canonical_model_id: z.string().trim().min(1).max(200),
    }).strict() }).strict(),
  ]),
  discount_bps: z.number().int().min(0).max(9_500).refine((value) => value % 100 === 0),
}).strict();

const serviceAssignmentSchema = z.object({
  account_id: z.string().startsWith("acct_").max(200),
  product_id: z.enum([STAGE5_MAIN_PRODUCT_ID, STAGE5_OPENKEYS_PRODUCT_ID]),
  owner_id: z.string().trim().min(1).max(200),
  policy_id: z.string().trim().min(1).max(200),
  purpose: z.string().trim().min(3).max(500),
  responsible: z.string().trim().min(1).max(200),
  rules: z.array(serviceRuleSchema).min(1),
}).strict();

const assignmentReferenceSchema = z.object({
  account_id: z.string().startsWith("acct_").max(200),
  source_id: z.string().trim().min(1).max(200),
  source_multiplier_bp: z.number().int().min(0).max(100_000),
  policy_id: z.string().nullable(),
  policy_digest: z.string().nullable(),
  exception_code: z.string().nullable(),
}).strict();

export const stage5AssignmentMatrixSchema = z.object({
  schema_version: z.literal(STAGE5_SCHEMA_VERSION),
  plan_digest: z.string().startsWith("sha256:v1:"),
  approved_by: z.string().trim().min(1).max(200),
  approved_at: z.string().datetime({ offset: true }),
  reason: z.string().trim().min(1).max(2_000),
  b2b: z.array(assignmentReferenceSchema),
  openkeys: z.array(assignmentReferenceSchema),
  service: z.array(serviceAssignmentSchema),
  excluded_disabled_accounts: z.array(z.string().startsWith("acct_").max(200)),
  content_digest: z.string().startsWith("sha256:v1:"),
}).strict();

export type Stage5AssignmentMatrix = z.infer<typeof stage5AssignmentMatrixSchema>;
export type Stage5ServiceAssignment = z.infer<typeof serviceAssignmentSchema>;

export type Stage5BackfillMode = "dry_run" | "safe" | "approved";

export interface Stage5BackfillResult {
  mode: Stage5BackfillMode;
  plan: Stage5BackfillPlan;
  protected_assignment_digest: string | null;
  writes_committed: boolean;
}

export class Stage5BackfillError extends Error {
  constructor(
    public readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "Stage5BackfillError";
  }
}

interface SnapshotRows {
  commerceAccounts: Stage5CommerceAccountSnapshot[];
  invitations: Stage5InvitationSnapshot[];
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function canonicalValue(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalValue);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .filter(([, child]) => child !== undefined)
        .sort(([left], [right]) => compareUtf8(left, right))
        .map(([key, child]) => [key, canonicalValue(child)]),
    );
  }
  return value;
}

function canonicalJson(value: unknown): string {
  return JSON.stringify(canonicalValue(value));
}

function deterministicUuid(label: string): string {
  const bytes = createHash("sha256").update(`multi-discount-stage5:${label}`, "utf8").digest().subarray(0, 16);
  bytes[6] = (bytes[6]! & 0x0f) | 0x50;
  bytes[8] = (bytes[8]! & 0x3f) | 0x80;
  const hex = bytes.toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function uniqueBy<T>(
  values: readonly T[],
  keyOf: (value: T) => string,
  label: string,
): Map<string, T> {
  const result = new Map<string, T>();
  for (const value of values) {
    const key = keyOf(value);
    if (result.has(key)) {
      throw new Stage5BackfillError("duplicate_inventory_identity", `duplicate ${label} identity ${key}`);
    }
    result.set(key, value);
  }
  return result;
}

function sourceRule(input: Omit<Stage5SourceRule, "rule_digest">): Stage5SourceRule {
  return { ...input, rule_digest: stage5Digest("source-rule", input) };
}

function effectiveRule(input: Omit<AccountPolicyRule, "rule_digest">): AccountPolicyRule {
  return { ...input, rule_digest: stage5Digest("effective-rule", input) };
}

function sourcePolicy(input: Omit<Stage5SourcePolicy, "content_digest">): Stage5SourcePolicy {
  const normalized = {
    ...input,
    rules: [...input.rules].sort((left, right) =>
      compareUtf8(left.provider_id, right.provider_id) ||
      compareUtf8(left.scope_type, right.scope_type) ||
      compareUtf8(left.canonical_model_id ?? "", right.canonical_model_id ?? "") ||
      compareUtf8(left.rule_id, right.rule_id)),
  };
  return { ...normalized, content_digest: stage5Digest("source-policy", normalized) };
}

function effectivePolicy(input: Omit<AccountPolicySpec, "content_digest">): AccountPolicySpec {
  const normalized = {
    ...input,
    rules: [...input.rules].sort((left, right) => {
      const leftRow = effectiveRuleRow(left);
      const rightRow = effectiveRuleRow(right);
      return compareUtf8(leftRow.provider_id, rightRow.provider_id) ||
        compareUtf8(leftRow.scope_type, rightRow.scope_type) ||
        compareUtf8(leftRow.canonical_model_id ?? "", rightRow.canonical_model_id ?? "") ||
        compareUtf8(leftRow.rule_id, rightRow.rule_id);
    }),
  };
  return { ...normalized, content_digest: stage5Digest("effective-policy", normalized) };
}

function trackSourceRule(providerId: "anthropic" | "openai"): Stage5SourceRule {
  return sourceRule({
    rule_id: `provider:${providerId}:track`,
    scope_type: "provider",
    provider_id: providerId,
    canonical_model_id: null,
    pricing_mode: "track",
    rule_origin: "managed",
    discount_bps: null,
    payable_multiplier_bp: null,
    track_eligible: true,
    retention_eligible: true,
    commission_eligible: true,
  });
}

function trackEffectiveRule(
  providerId: "anthropic" | "openai",
  multiplierBp: number,
): AccountPolicyRule {
  return effectiveRule({
    rule_id: `provider:${providerId}:track`,
    scope: { provider: { provider_id: providerId } },
    pricing_mode: "track",
    rule_origin: "managed",
    discount_bps: null,
    payable_multiplier_bp: multiplierBp,
    track_eligible: true,
    retention_eligible: true,
    commission_eligible: true,
  });
}

function discountSourceRule(
  providerId: "anthropic" | "openai",
  multiplierBp: number,
  origin: "managed" | "legacy" = "managed",
): Stage5SourceRule {
  return sourceRule({
    rule_id: `provider:${providerId}:${origin === "legacy" ? "legacy" : "discount"}`,
    scope_type: "provider",
    provider_id: providerId,
    canonical_model_id: null,
    pricing_mode: "discount",
    rule_origin: origin,
    discount_bps: origin === "managed" ? 10_000 - multiplierBp : null,
    payable_multiplier_bp: multiplierBp,
    track_eligible: false,
    retention_eligible: false,
    commission_eligible: false,
  });
}

function discountEffectiveRule(
  providerId: "anthropic" | "openai",
  multiplierBp: number,
  origin: "managed" | "legacy" = "managed",
): AccountPolicyRule {
  return effectiveRule({
    rule_id: `provider:${providerId}:${origin === "legacy" ? "legacy" : "discount"}`,
    scope: { provider: { provider_id: providerId } },
    pricing_mode: "discount",
    rule_origin: origin,
    discount_bps: origin === "managed" ? 10_000 - multiplierBp : null,
    payable_multiplier_bp: multiplierBp,
    track_eligible: false,
    retention_eligible: false,
    commission_eligible: false,
  });
}

function validManagedMultiplier(multiplierBp: number): boolean {
  const discountBps = 10_000 - multiplierBp;
  return discountBps >= 0 && discountBps <= 9_500 && discountBps % 100 === 0;
}

function buildCatalog(productId: string): PricingCatalogSpec {
  const base = {
    product_id: productId,
    generation: 1,
    schema_version: 1 as const,
    capability_generation: STAGE5_CAPABILITY_GENERATION,
    capability_digest: STAGE5_CAPABILITY_DIGEST,
    entries: STAGE5_CATALOG_MODELS.map((entry) => ({ ...entry })),
  };
  return { ...base, content_digest: stage5Digest("catalog", base) };
}

function buildSwitches(): ProviderSwitchSpec {
  const entries: ProviderSwitchSpec["entries"] = [];
  for (const providerId of ["anthropic", "openai"] as const) {
    entries.push(
      { provider_id: providerId, scope: "master", catalog_generation: null, enabled: true },
      {
        provider_id: providerId,
        scope: { product: { product_id: STAGE5_MAIN_PRODUCT_ID } },
        catalog_generation: 1,
        enabled: true,
      },
      {
        provider_id: providerId,
        scope: { product: { product_id: STAGE5_OPENKEYS_PRODUCT_ID } },
        catalog_generation: 1,
        enabled: true,
      },
      {
        provider_id: providerId,
        scope: { segment: { product_id: STAGE5_MAIN_PRODUCT_ID, segment: "b2c" } },
        catalog_generation: 1,
        enabled: true,
      },
      {
        provider_id: providerId,
        scope: { segment: { product_id: STAGE5_MAIN_PRODUCT_ID, segment: "b2b" } },
        catalog_generation: 1,
        enabled: true,
      },
    );
  }
  entries.sort((left, right) => {
    const leftScope = switchScopeParts(left.scope);
    const rightScope = switchScopeParts(right.scope);
    return compareUtf8(left.provider_id, right.provider_id) ||
      compareUtf8(leftScope.scopeType, rightScope.scopeType) ||
      compareUtf8(leftScope.productId, rightScope.productId) ||
      compareUtf8(leftScope.segment, rightScope.segment);
  });
  const base = {
    generation: 1,
    schema_version: STAGE5_SCHEMA_VERSION,
    capability_generation: STAGE5_CAPABILITY_GENERATION,
    capability_digest: STAGE5_CAPABILITY_DIGEST,
    entries,
  };
  return { ...base, content_digest: stage5Digest("switches", base) };
}

function buildB2cAccountPlan(account: Stage5CommerceAccountSnapshot, multiplierBp: number): Stage5AccountPlan {
  const policy = sourcePolicy({
    policy_id: "policy:main:global-b2c",
    owner_type: "global_b2c",
    owner_id: "global-b2c",
    product_id: STAGE5_MAIN_PRODUCT_ID,
    replacement_locked: false,
    version: 1,
    rules: [trackSourceRule("anthropic"), trackSourceRule("openai")],
  });
  const effective = effectivePolicy({
    account_id: account.engine_account_id,
    effective_version: 1,
    policy_id: policy.policy_id,
    policy_version: policy.version,
    source_policy_digest: policy.content_digest,
    owner_type: "global_b2c",
    owner_id: policy.owner_id,
    account_class: "b2c",
    product_id: STAGE5_MAIN_PRODUCT_ID,
    schema_version: STAGE5_SCHEMA_VERSION,
    catalog_generation: 1,
    switch_generation: 1,
    replacement_locked: false,
    rules: [trackEffectiveRule("anthropic", multiplierBp), trackEffectiveRule("openai", multiplierBp)],
  });
  return {
    binding_id: deterministicUuid(`binding:${account.engine_account_id}`),
    user_id: account.user_id,
    engine_account_record_id: account.engine_account_record_id,
    engine_account_id: account.engine_account_id,
    account_class: "b2c",
    source_multiplier_bp: multiplierBp,
    source_policy: policy,
    effective_policy: effective,
    binding: {
      policy_enforcement: "shadow",
      funding_enforcement: "legacy_single",
      reconciliation_state: "pending",
    },
  };
}

function buildB2bAccountPlan(account: Stage5CommerceAccountSnapshot, multiplierBp: number): Stage5AccountPlan {
  const policy = sourcePolicy({
    policy_id: `policy:main:b2b:${account.user_id}`,
    owner_type: "b2b_client",
    owner_id: account.user_id,
    product_id: STAGE5_MAIN_PRODUCT_ID,
    replacement_locked: false,
    version: 1,
    rules: [discountSourceRule("anthropic", multiplierBp)],
  });
  const effective = effectivePolicy({
    account_id: account.engine_account_id,
    effective_version: 1,
    policy_id: policy.policy_id,
    policy_version: policy.version,
    source_policy_digest: policy.content_digest,
    owner_type: "b2b_client",
    owner_id: account.user_id,
    account_class: "b2b",
    product_id: STAGE5_MAIN_PRODUCT_ID,
    schema_version: STAGE5_SCHEMA_VERSION,
    catalog_generation: 1,
    switch_generation: 1,
    replacement_locked: false,
    rules: [discountEffectiveRule("anthropic", multiplierBp)],
  });
  return {
    binding_id: deterministicUuid(`binding:${account.engine_account_id}`),
    user_id: account.user_id,
    engine_account_record_id: account.engine_account_record_id,
    engine_account_id: account.engine_account_id,
    account_class: "b2b",
    source_multiplier_bp: multiplierBp,
    source_policy: policy,
    effective_policy: effective,
    binding: {
      policy_enforcement: "shadow",
      funding_enforcement: "legacy_single",
      reconciliation_state: "pending",
    },
  };
}

function buildInvitationPlan(invite: Stage5InvitationSnapshot): Stage5InvitationPlan {
  const policy = sourcePolicy({
    policy_id: `policy:main:invite:${invite.invite_id}`,
    owner_type: "b2b_invitation",
    owner_id: invite.invite_id,
    product_id: STAGE5_MAIN_PRODUCT_ID,
    replacement_locked: false,
    version: 1,
    rules: [discountSourceRule("anthropic", invite.multiplier_bp)],
  });
  return { invite_id: invite.invite_id, source_multiplier_bp: invite.multiplier_bp, policy };
}

export function buildStage5OpenKeysPlan(
  account: Stage5Inventory["openkeys_accounts"][number],
): Stage5OpenKeysPlan {
  let exceptionCode: string | null = null;
  if (account.pricing_contract === "legacy" && !(account.multiplier_bp >= 1 && account.multiplier_bp <= 10_000)) {
    exceptionCode = "legacy_openkeys_multiplier_unrepresentable";
  } else if (account.pricing_contract === "official_1_to_1" && account.multiplier_bp !== 10_000) {
    exceptionCode = "current_openkeys_not_one_to_one";
  }
  if (exceptionCode !== null) {
    return {
      source_id: account.source_id,
      account_id: account.account_id,
      status: account.status,
      pricing_contract: account.pricing_contract,
      source_multiplier_bp: account.multiplier_bp,
      effective_policy: null,
      exception_code: exceptionCode,
    };
  }
  if (account.pricing_contract === "official_1_to_1") {
    return {
      source_id: account.source_id,
      account_id: account.account_id,
      status: account.status,
      pricing_contract: account.pricing_contract,
      source_multiplier_bp: account.multiplier_bp,
      effective_policy: buildOfficialOpenKeysPolicy(account.account_id, {
        catalog: buildCatalog(STAGE5_OPENKEYS_PRODUCT_ID),
        switches: buildSwitches(),
      }),
      exception_code: null,
    };
  }

  const origin = "legacy" as const;
  const policyId = `policy:openkeys:${account.pricing_contract}:${account.source_id}`;
  const sourceRules = [
    discountSourceRule("anthropic", account.multiplier_bp, origin),
    discountSourceRule("openai", account.multiplier_bp, origin),
  ];
  const sourceDigest = stage5Digest("openkeys-source-policy", {
    policy_id: policyId,
    owner_id: account.source_id,
    product_id: STAGE5_OPENKEYS_PRODUCT_ID,
    replacement_locked: account.pricing_contract === "legacy",
    version: 1,
    rules: sourceRules,
  });
  const effective = effectivePolicy({
    account_id: account.account_id,
    effective_version: 1,
    policy_id: policyId,
    policy_version: 1,
    source_policy_digest: sourceDigest,
    owner_type: "open_keys",
    owner_id: account.source_id,
    account_class: "open_keys",
    product_id: STAGE5_OPENKEYS_PRODUCT_ID,
    schema_version: STAGE5_SCHEMA_VERSION,
    catalog_generation: 1,
    switch_generation: 1,
    replacement_locked: account.pricing_contract === "legacy",
    rules: [
      discountEffectiveRule("anthropic", account.multiplier_bp, origin),
      discountEffectiveRule("openai", account.multiplier_bp, origin),
    ],
  });
  return {
    source_id: account.source_id,
    account_id: account.account_id,
    status: account.status,
    pricing_contract: account.pricing_contract,
    source_multiplier_bp: account.multiplier_bp,
    effective_policy: effective,
    exception_code: null,
  };
}

function assignmentReferenceForAccount(plan: Stage5AccountPlan): Stage5AssignmentReference {
  return {
    account_id: plan.engine_account_id,
    source_id: plan.user_id ?? plan.effective_policy.owner_id,
    source_multiplier_bp: plan.source_multiplier_bp,
    policy_id: plan.effective_policy.policy_id,
    policy_digest: plan.effective_policy.content_digest,
    exception_code: null,
  };
}

function assignmentReferenceForOpenKeys(plan: Stage5OpenKeysPlan): Stage5AssignmentReference {
  return {
    account_id: plan.account_id,
    source_id: plan.source_id,
    source_multiplier_bp: plan.source_multiplier_bp,
    policy_id: plan.effective_policy?.policy_id ?? null,
    policy_digest: plan.effective_policy?.content_digest ?? null,
    exception_code: plan.exception_code,
  };
}

function assignmentDraft(input: Omit<Stage5AssignmentMatrixDraft, "content_digest">): Stage5AssignmentMatrixDraft {
  return { ...input, content_digest: stage5Digest("assignment-matrix-draft", input) };
}

function capabilityProjection(): Stage5BackfillPlan["capability"] {
  return {
    generation: STAGE5_CAPABILITY_GENERATION,
    schema_version: STAGE5_SCHEMA_VERSION,
    content_digest: STAGE5_CAPABILITY_DIGEST,
    entries: STAGE5_CATALOG_MODELS.map((entry) => {
      const capabilityData = { pricing_supported: true };
      return {
        provider_id: entry.provider_id,
        canonical_model_id: entry.canonical_model_id,
        entry_digest: stage5Digest("capability-entry", { ...entry, capability_data: capabilityData }),
        capability_data: capabilityData,
      };
    }),
    aliases: [{
      provider_id: "openai",
      alias_model_id: "gpt-5.6",
      canonical_model_id: "gpt-5.6-sol",
    }],
  };
}

function sortedStrings(values: Iterable<string>): string[] {
  return [...values].sort(compareUtf8);
}

function sortedAccounts<T extends { engine_account_id: string }>(values: T[]): T[] {
  return values.sort((left, right) => compareUtf8(left.engine_account_id, right.engine_account_id));
}

function buildStage5Plan(snapshot: SnapshotRows, rawInventory: Stage5Inventory): Stage5BackfillPlan {
  const inventory = stage5InventorySchema.parse(rawInventory);
  const engineById = uniqueBy(inventory.engine_accounts, (account) => account.account_id, "engine account");
  const openKeysByEngineId = uniqueBy(inventory.openkeys_accounts, (account) => account.account_id, "OpenKeys engine account");
  uniqueBy(inventory.openkeys_accounts, (account) => account.source_id, "OpenKeys source");

  const blockers: Stage5Blocker[] = [];
  const commerceEngineIds = new Set<string>();
  const b2cAccounts: Stage5AccountPlan[] = [];
  const b2bAccounts: Stage5AccountPlan[] = [];

  for (const account of sortedAccounts([...snapshot.commerceAccounts])) {
    commerceEngineIds.add(account.engine_account_id);
    const actual = engineById.get(account.engine_account_id);
    if (!actual) {
      blockers.push({
        scope: account.account_class === "b2c" ? "safe" : "protected",
        code: "commerce_account_missing_from_engine_inventory",
        subject_id: account.engine_account_id,
        detail: "commerce engine account was not present in the exact engine inventory",
      });
      continue;
    }
    if (
      (account.commerce_status !== "active" && account.commerce_status !== "disabled") ||
      actual.status !== account.commerce_status
    ) {
      blockers.push({
        scope: account.account_class === "b2c" ? "safe" : "protected",
        code: "legacy_account_status_drift",
        subject_id: account.engine_account_id,
        detail: `engine=${actual.status} commerce=${account.commerce_status}`,
      });
      continue;
    }
    if (
      actual.multiplier_bp !== account.commerce_multiplier_bp ||
      actual.multiplier_bp !== account.profile_multiplier_bp
    ) {
      blockers.push({
        scope: account.account_class === "b2c" ? "safe" : "protected",
        code: "legacy_multiplier_drift",
        subject_id: account.engine_account_id,
        detail: `engine=${actual.multiplier_bp} engine_accounts=${account.commerce_multiplier_bp} profile=${account.profile_multiplier_bp}`,
      });
      continue;
    }
    if (actual.multiplier_bp < 0 || actual.multiplier_bp > 10_000) {
      blockers.push({
        scope: account.account_class === "b2c" ? "safe" : "protected",
        code: "legacy_multiplier_out_of_policy_range",
        subject_id: account.engine_account_id,
        detail: `multiplier ${actual.multiplier_bp} is outside 0..10000`,
      });
      continue;
    }
    if (account.account_class === "b2c") {
      b2cAccounts.push(buildB2cAccountPlan(account, actual.multiplier_bp));
    } else if (!validManagedMultiplier(actual.multiplier_bp)) {
      blockers.push({
        scope: "protected",
        code: "b2b_multiplier_unrepresentable",
        subject_id: account.engine_account_id,
        detail: `multiplier ${actual.multiplier_bp} is not a supported 1% static discount step`,
      });
    } else {
      b2bAccounts.push(buildB2bAccountPlan(account, actual.multiplier_bp));
    }
  }

  const invitationPlans: Stage5InvitationPlan[] = [];
  for (const invite of [...snapshot.invitations].sort((left, right) => compareUtf8(left.invite_id, right.invite_id))) {
    if (!validManagedMultiplier(invite.multiplier_bp)) {
      blockers.push({
        scope: "safe",
        code: "invitation_multiplier_unrepresentable",
        subject_id: invite.invite_id,
        detail: `multiplier ${invite.multiplier_bp} is not a supported 1% static discount step`,
      });
      continue;
    }
    invitationPlans.push(buildInvitationPlan(invite));
  }

  const openKeysPlans = inventory.openkeys_accounts
    .map(buildStage5OpenKeysPlan)
    .sort((left, right) => compareUtf8(left.account_id, right.account_id));
  for (const plan of openKeysPlans) {
    const engine = engineById.get(plan.account_id);
    if (!engine) {
      blockers.push({
        scope: "protected",
        code: "openkeys_account_missing_from_engine_inventory",
        subject_id: plan.account_id,
        detail: "OpenKeys account was not present in the exact engine inventory",
      });
    } else if (engine.multiplier_bp !== plan.source_multiplier_bp || engine.status !== plan.status) {
      blockers.push({
        scope: "protected",
        code: "openkeys_inventory_drift",
        subject_id: plan.account_id,
        detail: `engine multiplier/status=${engine.multiplier_bp}/${engine.status}, OpenKeys=${plan.source_multiplier_bp}/${plan.status}`,
      });
    }
    if (commerceEngineIds.has(plan.account_id)) {
      blockers.push({
        scope: "protected",
        code: "cross_context_account_collision",
        subject_id: plan.account_id,
        detail: "engine account is claimed by both commerce and OpenKeys",
      });
    }
    if (plan.exception_code) {
      blockers.push({
        scope: "protected",
        code: plan.exception_code,
        subject_id: plan.account_id,
        detail: "OpenKeys economics cannot be represented by the target contract",
      });
    }
  }

  const claimed = new Set([...commerceEngineIds, ...openKeysByEngineId.keys()]);
  const unresolved = sortedStrings(
    inventory.engine_accounts
      .filter((account) => !claimed.has(account.account_id))
      .map((account) => account.account_id),
  );
  for (const accountId of unresolved) {
    blockers.push({
      scope: "protected",
      code: "engine_account_requires_explicit_classification",
      subject_id: accountId,
      detail: "account must be explicitly assigned as service or excluded as disabled",
    });
  }

  const capability = capabilityProjection();
  const catalogs = [buildCatalog(STAGE5_MAIN_PRODUCT_ID), buildCatalog(STAGE5_OPENKEYS_PRODUCT_ID)];
  const switches = buildSwitches();
  const inventoryDigest = stage5Digest("inventory", inventory);
  const sourceDigest = stage5Digest("legacy-source", snapshot);
  const planBase = {
    schema_version: 1 as const,
    capability,
    catalogs,
    switches,
    safe: { b2c_accounts: b2cAccounts, invitations: invitationPlans },
    protected: {
      b2b_accounts: b2bAccounts,
      openkeys_accounts: openKeysPlans,
      unresolved_engine_accounts: unresolved,
    },
    blockers,
    inventory_digest: inventoryDigest,
    source_digest: sourceDigest,
  };
  const planDigest = stage5Digest("plan", planBase);
  const draft = assignmentDraft({
    schema_version: STAGE5_SCHEMA_VERSION,
    plan_digest: planDigest,
    b2b: b2bAccounts.map(assignmentReferenceForAccount),
    openkeys: openKeysPlans.map(assignmentReferenceForOpenKeys),
    unresolved_engine_accounts: unresolved,
  });
  return { ...planBase, plan_digest: planDigest, assignment_matrix_draft: draft };
}

async function readStage5Snapshot(client: PoolClient): Promise<SnapshotRows> {
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
    ORDER BY id
  `);
  return {
    commerceAccounts: accounts.rows,
    invitations: invitations.rows.map((row) => ({
      invite_id: row.invite_id,
      multiplier_bp: row.multiplier_bp,
      expires_at: row.expires_at.toISOString(),
    })),
  };
}

export async function planStage5Backfill(
  database: Database,
  inventory: Stage5Inventory,
): Promise<Stage5BackfillPlan> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    const snapshot = await readStage5Snapshot(client);
    const plan = buildStage5Plan(snapshot, inventory);
    await client.query("COMMIT");
    return plan;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

function sortedReferences(values: Stage5AssignmentReference[]): Stage5AssignmentReference[] {
  return [...values].sort((left, right) => compareUtf8(left.account_id, right.account_id));
}

export function buildStage5AssignmentMatrix(
  plan: Stage5BackfillPlan,
  input: {
    approved_by: string;
    approved_at: string;
    reason: string;
    service: Stage5ServiceAssignment[];
    excluded_disabled_accounts?: string[];
  },
): Stage5AssignmentMatrix {
  const base = {
    schema_version: 1 as const,
    plan_digest: plan.plan_digest,
    approved_by: input.approved_by,
    approved_at: input.approved_at,
    reason: input.reason,
    b2b: sortedReferences(plan.assignment_matrix_draft.b2b),
    openkeys: sortedReferences(plan.assignment_matrix_draft.openkeys),
    service: [...input.service].sort((left, right) => compareUtf8(left.account_id, right.account_id)),
    excluded_disabled_accounts: sortedStrings(input.excluded_disabled_accounts ?? []),
  };
  return stage5AssignmentMatrixSchema.parse({
    ...base,
    content_digest: stage5Digest("approved-assignment-matrix", base),
  });
}

function sameCanonical(left: unknown, right: unknown): boolean {
  return canonicalJson(left) === canonicalJson(right);
}

export function validateStage5AssignmentMatrix(
  plan: Stage5BackfillPlan,
  rawMatrix: Stage5AssignmentMatrix,
  inventory: Stage5Inventory,
): Stage5AssignmentMatrix {
  const matrix = stage5AssignmentMatrixSchema.parse(rawMatrix);
  const { content_digest: _digest, ...base } = matrix;
  const expectedDigest = stage5Digest("approved-assignment-matrix", base);
  if (matrix.content_digest !== expectedDigest) {
    throw new Stage5BackfillError("assignment_matrix_digest_mismatch", "approved assignment matrix digest is invalid");
  }
  if (matrix.plan_digest !== plan.plan_digest) {
    throw new Stage5BackfillError("assignment_matrix_stale", "approved assignment matrix targets a different source plan");
  }
  if (!sameCanonical(sortedReferences(matrix.b2b), sortedReferences(plan.assignment_matrix_draft.b2b))) {
    throw new Stage5BackfillError("b2b_assignment_mismatch", "approved B2B assignments do not exactly match the plan");
  }
  if (!sameCanonical(sortedReferences(matrix.openkeys), sortedReferences(plan.assignment_matrix_draft.openkeys))) {
    throw new Stage5BackfillError("openkeys_assignment_mismatch", "approved OpenKeys assignments do not exactly match the plan");
  }
  uniqueBy(matrix.service, (assignment) => assignment.account_id, "service assignment");
  const exclusions = new Set(matrix.excluded_disabled_accounts);
  if (exclusions.size !== matrix.excluded_disabled_accounts.length) {
    throw new Stage5BackfillError("duplicate_disabled_exclusion", "disabled exclusion list contains duplicates");
  }
  const serviceIds = new Set(matrix.service.map((assignment) => assignment.account_id));
  const unresolved = new Set(plan.protected.unresolved_engine_accounts);
  const inventoryById = uniqueBy(inventory.engine_accounts, (account) => account.account_id, "engine account");
  for (const accountId of unresolved) {
    if (serviceIds.has(accountId) === exclusions.has(accountId)) {
      throw new Stage5BackfillError(
        "unresolved_assignment",
        `unclassified engine account ${accountId} must have exactly one service or disabled-exclusion decision`,
      );
    }
    if (exclusions.has(accountId) && inventoryById.get(accountId)?.status !== "disabled") {
      throw new Stage5BackfillError("active_account_excluded", `active engine account ${accountId} cannot be excluded`);
    }
  }
  for (const accountId of [...serviceIds, ...exclusions]) {
    if (!unresolved.has(accountId)) {
      throw new Stage5BackfillError("assignment_outside_plan", `assignment for ${accountId} is outside the unresolved plan set`);
    }
  }
  return matrix;
}

// Persistence is intentionally implemented below the pure planner. Keeping the plan and approval
// contract executable allows operators to review exact identities and hashes before any protected
// account is changed, while safe B2C/invitation materialization remains deterministic.

function versionNumber(value: string, label: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0 || String(parsed) !== value) {
    throw new Stage5BackfillError("malformed_stored_version", `${label} is not a positive safe integer`);
  }
  return parsed;
}

function assertStored(kind: string, expected: unknown, actual: unknown): void {
  if (!sameCanonical(expected, actual)) {
    throw new Stage5BackfillError(
      "immutable_version_conflict",
      `${kind} already exists with different immutable content`,
    );
  }
}

async function ensureCapabilityProjection(
  client: PoolClient,
  capability: Stage5BackfillPlan["capability"],
): Promise<void> {
  await client.query(`
    INSERT INTO provider_capability_versions (
      generation, schema_version, content_digest, source_runtime, source_revision, observed_at
    ) VALUES ($1, $2, $3, 'claude-api', $3, now())
    ON CONFLICT (generation) DO NOTHING
  `, [capability.generation, capability.schema_version, capability.content_digest]);
  const header = await client.query<{
    generation: string;
    schema_version: string;
    content_digest: string;
    source_runtime: string | null;
    source_revision: string | null;
  }>(`
    SELECT generation::text, schema_version::text, content_digest, source_runtime, source_revision
    FROM provider_capability_versions WHERE generation = $1
  `, [capability.generation]);
  const row = header.rows[0];
  if (!row) throw new Stage5BackfillError("capability_insert_lost", "capability projection was not stored");
  assertStored("capability projection", {
    generation: capability.generation,
    schema_version: capability.schema_version,
    content_digest: capability.content_digest,
    source_runtime: "claude-api",
    source_revision: capability.content_digest,
  }, {
    generation: versionNumber(row.generation, "capability generation"),
    schema_version: versionNumber(row.schema_version, "capability schema version"),
    content_digest: row.content_digest,
    source_runtime: row.source_runtime,
    source_revision: row.source_revision,
  });

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
  const entries = await client.query<{
    provider_id: string;
    canonical_model_id: string;
    entry_digest: string;
    capability_data: Record<string, unknown>;
  }>(`
    SELECT provider_id, canonical_model_id, entry_digest, capability_data
    FROM provider_capability_entries
    WHERE generation = $1
    ORDER BY provider_id COLLATE "C", canonical_model_id COLLATE "C"
  `, [capability.generation]);
  assertStored("capability entries", capability.entries, entries.rows);

  for (const alias of capability.aliases) {
    await client.query(`
      INSERT INTO provider_capability_aliases (
        generation, provider_id, alias_model_id, canonical_model_id
      ) VALUES ($1, $2, $3, $4)
      ON CONFLICT (generation, provider_id, alias_model_id) DO NOTHING
    `, [capability.generation, alias.provider_id, alias.alias_model_id, alias.canonical_model_id]);
  }
  const aliases = await client.query<{
    provider_id: string;
    alias_model_id: string;
    canonical_model_id: string;
  }>(`
    SELECT provider_id, alias_model_id, canonical_model_id
    FROM provider_capability_aliases
    WHERE generation = $1
    ORDER BY provider_id COLLATE "C", alias_model_id COLLATE "C"
  `, [capability.generation]);
  assertStored("capability aliases", capability.aliases, aliases.rows);

  const head = await client.query<{ active_generation: string }>(`
    SELECT active_generation::text FROM provider_capability_head WHERE singleton = 1 FOR UPDATE
  `);
  if (!head.rows[0]) {
    await client.query(`
      INSERT INTO provider_capability_head (singleton, active_generation) VALUES (1, $1)
    `, [capability.generation]);
  } else if (versionNumber(head.rows[0].active_generation, "capability head") < capability.generation) {
    await client.query(`
      UPDATE provider_capability_head SET active_generation = $1, updated_at = now() WHERE singleton = 1
    `, [capability.generation]);
  }
}

async function ensureCatalog(client: PoolClient, catalog: PricingCatalogSpec): Promise<void> {
  await client.query(`
    INSERT INTO product_catalog_versions (
      product_id, generation, schema_version, capability_generation, capability_digest,
      content_digest, actor_type, actor_id, reason
    ) VALUES ($1, $2, $3, $4, $5, $6, 'migration', 'multi-discount-stage5',
              'Initial explicit Anthropic/OpenAI product catalog')
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
        product_id, generation, capability_generation, provider_id, canonical_model_id, enabled
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
  const header = await client.query<{
    product_id: string;
    generation: string;
    schema_version: string;
    capability_generation: string;
    capability_digest: string;
    content_digest: string;
  }>(`
    SELECT product_id, generation::text, schema_version::text,
           capability_generation::text, capability_digest, content_digest
    FROM product_catalog_versions WHERE product_id = $1 AND generation = $2
  `, [catalog.product_id, catalog.generation]);
  const row = header.rows[0];
  if (!row) throw new Stage5BackfillError("catalog_insert_lost", `catalog ${catalog.product_id} was not stored`);
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
  assertStored(`catalog ${catalog.product_id}`, catalog, {
    product_id: row.product_id,
    generation: versionNumber(row.generation, "catalog generation"),
    schema_version: versionNumber(row.schema_version, "catalog schema version"),
    capability_generation: versionNumber(row.capability_generation, "catalog capability generation"),
    capability_digest: row.capability_digest,
    content_digest: row.content_digest,
    entries: entries.rows,
  });

  const head = await client.query<{ active_generation: string }>(`
    SELECT active_generation::text FROM product_catalog_heads WHERE product_id = $1 FOR UPDATE
  `, [catalog.product_id]);
  if (!head.rows[0]) {
    await client.query(`
      INSERT INTO product_catalog_heads (product_id, active_generation) VALUES ($1, $2)
    `, [catalog.product_id, catalog.generation]);
  } else if (versionNumber(head.rows[0].active_generation, "catalog head") < catalog.generation) {
    await client.query(`
      UPDATE product_catalog_heads SET active_generation = $2, updated_at = now() WHERE product_id = $1
    `, [catalog.product_id, catalog.generation]);
  }

  const payload = JSON.stringify(catalog);
  await client.query(`
    INSERT INTO engine_catalog_jobs (
      id, product_id, generation, schema_version, content_digest, payload
    ) VALUES ($1, $2, $3, $4, $5, $6::jsonb)
    ON CONFLICT (product_id, generation) DO NOTHING
  `, [randomUUID(), catalog.product_id, catalog.generation, catalog.schema_version, catalog.content_digest, payload]);
  const job = await client.query<{ schema_version: string; content_digest: string; payload: unknown }>(`
    SELECT schema_version::text, content_digest, payload
    FROM engine_catalog_jobs WHERE product_id = $1 AND generation = $2
  `, [catalog.product_id, catalog.generation]);
  const jobRow = job.rows[0];
  if (!jobRow) throw new Stage5BackfillError("catalog_job_insert_lost", "catalog control job was not stored");
  assertStored(`catalog job ${catalog.product_id}`, {
    schema_version: catalog.schema_version,
    content_digest: catalog.content_digest,
    payload: catalog,
  }, {
    schema_version: versionNumber(jobRow.schema_version, "catalog job schema version"),
    content_digest: jobRow.content_digest,
    payload: jobRow.payload,
  });
}

function switchScopeParts(scope: ProviderSwitchSpec["entries"][number]["scope"]): {
  scopeType: "master" | "product" | "segment";
  productId: string;
  segment: string;
} {
  if (scope === "master") return { scopeType: "master", productId: "", segment: "" };
  if ("product" in scope) {
    return { scopeType: "product", productId: scope.product.product_id, segment: "" };
  }
  return {
    scopeType: "segment",
    productId: scope.segment.product_id,
    segment: scope.segment.segment,
  };
}

async function ensureSwitches(client: PoolClient, switches: ProviderSwitchSpec): Promise<void> {
  await client.query(`
    INSERT INTO provider_switch_versions (
      generation, schema_version, capability_generation, capability_digest,
      content_digest, actor_type, actor_id, reason
    ) VALUES ($1, $2, $3, $4, $5, 'migration', 'multi-discount-stage5',
              'Initial explicit Anthropic/OpenAI provider gates')
    ON CONFLICT (generation) DO NOTHING
  `, [
    switches.generation,
    switches.schema_version,
    switches.capability_generation,
    switches.capability_digest,
    switches.content_digest,
  ]);
  for (const entry of switches.entries) {
    const scope = switchScopeParts(entry.scope);
    await client.query(`
      INSERT INTO provider_switch_entries (
        generation, provider_id, scope_type, product_id, segment, catalog_generation, enabled
      ) VALUES ($1, $2, $3, $4, $5, $6, $7)
      ON CONFLICT (generation, provider_id, scope_type, product_id, segment) DO NOTHING
    `, [
      switches.generation,
      entry.provider_id,
      scope.scopeType,
      scope.productId,
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
  const row = header.rows[0];
  if (!row) throw new Stage5BackfillError("switch_insert_lost", "provider switches were not stored");
  const entries = await client.query<{
    provider_id: string;
    scope_type: "master" | "product" | "segment";
    product_id: string;
    segment: "" | "b2c" | "b2b";
    catalog_generation: string | null;
    enabled: boolean;
  }>(`
    SELECT provider_id, scope_type, product_id, segment, catalog_generation::text, enabled
    FROM provider_switch_entries WHERE generation = $1
    ORDER BY provider_id COLLATE "C", scope_type COLLATE "C", product_id COLLATE "C", segment COLLATE "C"
  `, [switches.generation]);
  const storedEntries: ProviderSwitchSpec["entries"] = entries.rows.map((entry) => ({
    provider_id: entry.provider_id,
    scope: entry.scope_type === "master"
      ? "master"
      : entry.scope_type === "product"
        ? { product: { product_id: entry.product_id } }
        : { segment: { product_id: entry.product_id, segment: entry.segment as "b2c" | "b2b" } },
    catalog_generation: entry.catalog_generation === null
      ? null
      : versionNumber(entry.catalog_generation, "switch catalog generation"),
    enabled: entry.enabled,
  }));
  assertStored("provider switches", switches, {
    generation: versionNumber(row.generation, "switch generation"),
    schema_version: versionNumber(row.schema_version, "switch schema version"),
    capability_generation: versionNumber(row.capability_generation, "switch capability generation"),
    capability_digest: row.capability_digest,
    content_digest: row.content_digest,
    entries: storedEntries,
  });

  const head = await client.query<{ active_generation: string }>(`
    SELECT active_generation::text FROM provider_switch_head WHERE singleton = 1 FOR UPDATE
  `);
  if (!head.rows[0]) {
    await client.query(`INSERT INTO provider_switch_head (singleton, active_generation) VALUES (1, $1)`, [switches.generation]);
  } else if (versionNumber(head.rows[0].active_generation, "switch head") < switches.generation) {
    await client.query(`
      UPDATE provider_switch_head SET active_generation = $1, updated_at = now() WHERE singleton = 1
    `, [switches.generation]);
  }

  await client.query(`
    INSERT INTO engine_switch_jobs (
      id, generation, schema_version, content_digest, payload
    ) VALUES ($1, $2, $3, $4, $5::jsonb)
    ON CONFLICT (generation) DO NOTHING
  `, [randomUUID(), switches.generation, switches.schema_version, switches.content_digest, JSON.stringify(switches)]);
  const job = await client.query<{ schema_version: string; content_digest: string; payload: unknown }>(`
    SELECT schema_version::text, content_digest, payload FROM engine_switch_jobs WHERE generation = $1
  `, [switches.generation]);
  const jobRow = job.rows[0];
  if (!jobRow) throw new Stage5BackfillError("switch_job_insert_lost", "switch control job was not stored");
  assertStored("switch job", {
    schema_version: switches.schema_version,
    content_digest: switches.content_digest,
    payload: switches,
  }, {
    schema_version: versionNumber(jobRow.schema_version, "switch job schema version"),
    content_digest: jobRow.content_digest,
    payload: jobRow.payload,
  });
}

async function ensureSourcePolicy(client: PoolClient, policy: Stage5SourcePolicy): Promise<void> {
  await client.query(`
    INSERT INTO pricing_policies (
      id, owner_type, owner_id, product_id, replacement_locked, status
    ) VALUES ($1, $2, $3, $4, $5, 'active')
    ON CONFLICT (id) DO NOTHING
  `, [policy.policy_id, policy.owner_type, policy.owner_id, policy.product_id, policy.replacement_locked]);
  const identity = await client.query<{
    id: string;
    owner_type: Stage5SourcePolicy["owner_type"];
    owner_id: string;
    product_id: string;
    replacement_locked: boolean;
    status: string;
  }>(`
    SELECT id, owner_type, owner_id, product_id, replacement_locked, status
    FROM pricing_policies WHERE id = $1
  `, [policy.policy_id]);
  const identityRow = identity.rows[0];
  if (!identityRow) throw new Stage5BackfillError("policy_insert_lost", `policy ${policy.policy_id} was not stored`);
  assertStored(`policy identity ${policy.policy_id}`, {
    id: policy.policy_id,
    owner_type: policy.owner_type,
    owner_id: policy.owner_id,
    product_id: policy.product_id,
    replacement_locked: policy.replacement_locked,
    status: "active",
  }, identityRow);

  await client.query(`
    INSERT INTO pricing_policy_versions (
      policy_id, version, schema_version, product_id, catalog_generation,
      content_digest, actor_type, actor_id, reason
    ) VALUES ($1, $2, $3, $4, 1, $5, 'migration', 'multi-discount-stage5',
              'Backfilled from immutable legacy scalar economics')
    ON CONFLICT (policy_id, version) DO NOTHING
  `, [policy.policy_id, policy.version, STAGE5_SCHEMA_VERSION, policy.product_id, policy.content_digest]);
  for (const rule of policy.rules) {
    await client.query(`
      INSERT INTO pricing_policy_rules (
        policy_id, policy_version, product_id, catalog_generation,
        rule_id, rule_digest, scope_type, provider_id, canonical_model_id,
        pricing_mode, rule_origin, discount_bps, payable_multiplier_bp,
        track_eligible, retention_eligible, commission_eligible
      ) VALUES ($1, $2, $3, 1, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
      ON CONFLICT (policy_id, policy_version, rule_id) DO NOTHING
    `, [
      policy.policy_id,
      policy.version,
      policy.product_id,
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
  const version = await client.query<{
    policy_id: string;
    version: string;
    schema_version: string;
    product_id: string;
    catalog_generation: string;
    content_digest: string;
  }>(`
    SELECT policy_id, version::text, schema_version::text, product_id,
           catalog_generation::text, content_digest
    FROM pricing_policy_versions WHERE policy_id = $1 AND version = $2
  `, [policy.policy_id, policy.version]);
  const versionRow = version.rows[0];
  if (!versionRow) throw new Stage5BackfillError("policy_version_insert_lost", "policy version was not stored");
  assertStored(`policy version ${policy.policy_id}`, {
    policy_id: policy.policy_id,
    version: policy.version,
    schema_version: STAGE5_SCHEMA_VERSION,
    product_id: policy.product_id,
    catalog_generation: 1,
    content_digest: policy.content_digest,
  }, {
    policy_id: versionRow.policy_id,
    version: versionNumber(versionRow.version, "policy version"),
    schema_version: versionNumber(versionRow.schema_version, "policy schema version"),
    product_id: versionRow.product_id,
    catalog_generation: versionNumber(versionRow.catalog_generation, "policy catalog generation"),
    content_digest: versionRow.content_digest,
  });
  const rules = await client.query<Stage5SourceRule>(`
    SELECT rule_id, rule_digest, scope_type, provider_id, canonical_model_id,
           pricing_mode, rule_origin, discount_bps, payable_multiplier_bp,
           track_eligible, retention_eligible, commission_eligible
    FROM pricing_policy_rules WHERE policy_id = $1 AND policy_version = $2
    ORDER BY provider_id COLLATE "C", scope_type COLLATE "C",
             COALESCE(canonical_model_id, '') COLLATE "C", rule_id COLLATE "C"
  `, [policy.policy_id, policy.version]);
  assertStored(`policy rules ${policy.policy_id}`, policy.rules, rules.rows);

  const head = await client.query<{ current_version: string; current_digest: string }>(`
    SELECT current_version::text, current_digest
    FROM pricing_policy_heads WHERE policy_id = $1 FOR UPDATE
  `, [policy.policy_id]);
  if (!head.rows[0]) {
    await client.query(`
      INSERT INTO pricing_policy_heads (policy_id, current_version, current_digest)
      VALUES ($1, $2, $3)
    `, [policy.policy_id, policy.version, policy.content_digest]);
  } else {
    const current = versionNumber(head.rows[0].current_version, "policy head");
    if (current === policy.version && head.rows[0].current_digest !== policy.content_digest) {
      throw new Stage5BackfillError("immutable_version_conflict", `policy head ${policy.policy_id} has a different digest`);
    }
    if (current < policy.version) {
      await client.query(`
        UPDATE pricing_policy_heads
        SET current_version = $2, current_digest = $3, updated_at = now()
        WHERE policy_id = $1
      `, [policy.policy_id, policy.version, policy.content_digest]);
    }
  }
}

async function ensureInvitation(client: PoolClient, invitation: Stage5InvitationPlan): Promise<void> {
  await ensureSourcePolicy(client, invitation.policy);
  await client.query(`
    INSERT INTO business_invite_policy_bindings (
      invite_id, invitation_policy_id, current_policy_version, current_policy_digest
    ) VALUES ($1, $2, $3, $4)
    ON CONFLICT (invite_id) DO NOTHING
  `, [
    invitation.invite_id,
    invitation.policy.policy_id,
    invitation.policy.version,
    invitation.policy.content_digest,
  ]);
  const stored = await client.query<{
    invite_id: string;
    invitation_policy_id: string;
    current_policy_version: string;
    current_policy_digest: string;
    redeemed_at: Date | null;
  }>(`
    SELECT invite_id::text, invitation_policy_id, current_policy_version::text,
           current_policy_digest, redeemed_at
    FROM business_invite_policy_bindings WHERE invite_id = $1
  `, [invitation.invite_id]);
  const row = stored.rows[0];
  if (!row) throw new Stage5BackfillError("invite_binding_insert_lost", "invitation binding was not stored");
  assertStored(`invitation binding ${invitation.invite_id}`, {
    invite_id: invitation.invite_id,
    invitation_policy_id: invitation.policy.policy_id,
    current_policy_version: invitation.policy.version,
    current_policy_digest: invitation.policy.content_digest,
    redeemed_at: null,
  }, {
    invite_id: row.invite_id,
    invitation_policy_id: row.invitation_policy_id,
    current_policy_version: versionNumber(row.current_policy_version, "invitation policy version"),
    current_policy_digest: row.current_policy_digest,
    redeemed_at: row.redeemed_at,
  });
}

function effectiveRuleRow(rule: AccountPolicyRule): Stage5SourceRule & { payable_multiplier_bp: number } {
  if ("provider" in rule.scope) {
    return {
      ...rule,
      scope_type: "provider",
      provider_id: rule.scope.provider.provider_id,
      canonical_model_id: null,
    };
  }
  return {
    ...rule,
    scope_type: "model",
    provider_id: rule.scope.model.provider_id,
    canonical_model_id: rule.scope.model.canonical_model_id,
  };
}

async function ensureAccountPolicy(client: PoolClient, account: Stage5AccountPlan): Promise<void> {
  await ensureSourcePolicy(client, account.source_policy);
  await client.query(`
    INSERT INTO account_policy_bindings (
      id, user_id, engine_account_record_id, engine_account_id,
      account_class, product_id, policy_id,
      policy_enforcement, funding_enforcement, reconciliation_state, sync_state
    ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'legacy_scalar', 'legacy_single', 'pending', 'legacy')
    ON CONFLICT (id) DO NOTHING
  `, [
    account.binding_id,
    account.user_id,
    account.engine_account_record_id,
    account.engine_account_id,
    account.account_class,
    account.effective_policy.product_id,
    account.source_policy.policy_id,
  ]);
  const identity = await client.query<{
    id: string;
    user_id: string | null;
    engine_account_record_id: string | null;
    engine_account_id: string;
    account_class: Stage5AccountPlan["account_class"];
    product_id: string;
    policy_id: string;
  }>(`
    SELECT id::text, user_id::text, engine_account_record_id::text,
           engine_account_id, account_class, product_id, policy_id
    FROM account_policy_bindings WHERE id = $1
    FOR UPDATE
  `, [account.binding_id]);
  const identityRow = identity.rows[0];
  if (!identityRow) throw new Stage5BackfillError("account_binding_insert_lost", "account binding was not stored");
  assertStored(`account binding ${account.engine_account_id}`, {
    id: account.binding_id,
    user_id: account.user_id,
    engine_account_record_id: account.engine_account_record_id,
    engine_account_id: account.engine_account_id,
    account_class: account.account_class,
    product_id: account.effective_policy.product_id,
    policy_id: account.source_policy.policy_id,
  }, identityRow);

  const policy = account.effective_policy;
  await client.query(`
    INSERT INTO account_policy_versions (
      binding_id, effective_version, policy_id, policy_version, policy_digest,
      product_id, account_class, schema_version, catalog_generation,
      switch_generation, content_digest, replacement_locked
    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
    ON CONFLICT (binding_id, effective_version) DO NOTHING
  `, [
    account.binding_id,
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
    policy.replacement_locked,
  ]);
  for (const rule of policy.rules) {
    const row = effectiveRuleRow(rule);
    await client.query(`
      INSERT INTO account_policy_rules (
        binding_id, effective_version, product_id, catalog_generation,
        rule_id, rule_digest, scope_type, provider_id, canonical_model_id,
        pricing_mode, rule_origin, discount_bps, payable_multiplier_bp,
        track_eligible, retention_eligible, commission_eligible
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
      ON CONFLICT (binding_id, effective_version, rule_id) DO NOTHING
    `, [
      account.binding_id,
      policy.effective_version,
      policy.product_id,
      policy.catalog_generation,
      row.rule_id,
      row.rule_digest,
      row.scope_type,
      row.provider_id,
      row.canonical_model_id,
      row.pricing_mode,
      row.rule_origin,
      row.discount_bps,
      row.payable_multiplier_bp,
      row.track_eligible,
      row.retention_eligible,
      row.commission_eligible,
    ]);
  }
  const version = await client.query<{
    effective_version: string;
    policy_id: string;
    policy_version: string;
    policy_digest: string;
    product_id: string;
    account_class: "b2c" | "b2b" | "service";
    schema_version: string;
    catalog_generation: string;
    switch_generation: string;
    content_digest: string;
    replacement_locked: boolean;
  }>(`
    SELECT effective_version::text, policy_id, policy_version::text, policy_digest,
           product_id, account_class, schema_version::text, catalog_generation::text,
           switch_generation::text, content_digest, replacement_locked
    FROM account_policy_versions WHERE binding_id = $1 AND effective_version = $2
  `, [account.binding_id, policy.effective_version]);
  const versionRow = version.rows[0];
  if (!versionRow) throw new Stage5BackfillError("account_policy_insert_lost", "account policy was not stored");
  const storedRules = await client.query<Stage5SourceRule & { payable_multiplier_bp: number }>(`
    SELECT rule_id, rule_digest, scope_type, provider_id, canonical_model_id,
           pricing_mode, rule_origin, discount_bps, payable_multiplier_bp,
           track_eligible, retention_eligible, commission_eligible
    FROM account_policy_rules WHERE binding_id = $1 AND effective_version = $2
    ORDER BY provider_id COLLATE "C", scope_type COLLATE "C",
             COALESCE(canonical_model_id, '') COLLATE "C", rule_id COLLATE "C"
  `, [account.binding_id, policy.effective_version]);
  const reconstructed: AccountPolicySpec = {
    account_id: account.engine_account_id,
    effective_version: versionNumber(versionRow.effective_version, "effective policy version"),
    policy_id: versionRow.policy_id,
    policy_version: versionNumber(versionRow.policy_version, "source policy version"),
    source_policy_digest: versionRow.policy_digest,
    owner_type: policy.owner_type,
    owner_id: policy.owner_id,
    account_class: versionRow.account_class,
    product_id: versionRow.product_id,
    schema_version: versionNumber(versionRow.schema_version, "account policy schema version"),
    catalog_generation: versionNumber(versionRow.catalog_generation, "account catalog generation"),
    switch_generation: versionNumber(versionRow.switch_generation, "account switch generation"),
    content_digest: versionRow.content_digest,
    replacement_locked: versionRow.replacement_locked,
    rules: storedRules.rows.map((rule) => ({
      rule_id: rule.rule_id,
      rule_digest: rule.rule_digest,
      scope: rule.scope_type === "provider"
        ? { provider: { provider_id: rule.provider_id } }
        : { model: { provider_id: rule.provider_id, canonical_model_id: rule.canonical_model_id! } },
      pricing_mode: rule.pricing_mode,
      rule_origin: rule.rule_origin,
      discount_bps: rule.discount_bps,
      payable_multiplier_bp: rule.payable_multiplier_bp,
      track_eligible: rule.track_eligible,
      retention_eligible: rule.retention_eligible,
      commission_eligible: rule.commission_eligible,
    })),
  };
  assertStored(`effective account policy ${account.engine_account_id}`, policy, reconstructed);

  const current = await client.query<{
    desired_effective_version: string | null;
    desired_digest: string | null;
  }>(`
    SELECT desired_effective_version::text, desired_digest
    FROM account_policy_bindings WHERE id = $1 FOR UPDATE
  `, [account.binding_id]);
  const desired = current.rows[0]!;
  if (desired.desired_effective_version !== null) {
    const desiredVersion = versionNumber(desired.desired_effective_version, "desired policy version");
    if (desiredVersion === policy.effective_version && desired.desired_digest !== policy.content_digest) {
      throw new Stage5BackfillError("immutable_version_conflict", `binding ${account.binding_id} has a different desired digest`);
    }
  }
  if (
    desired.desired_effective_version === null ||
    versionNumber(desired.desired_effective_version, "desired policy version") <= policy.effective_version
  ) {
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
      account.binding_id,
      policy.effective_version,
      policy.content_digest,
      account.binding.policy_enforcement,
      account.binding.funding_enforcement,
      account.binding.reconciliation_state,
    ]);
  }

  const payload = { policy, binding: account.binding };
  await client.query(`
    INSERT INTO engine_policy_jobs (
      id, binding_id, effective_version, engine_account_id, policy_id,
      policy_version, catalog_generation, switch_generation, schema_version,
      content_digest, payload
    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::jsonb)
    ON CONFLICT (binding_id, effective_version) DO NOTHING
  `, [
    randomUUID(),
    account.binding_id,
    policy.effective_version,
    account.engine_account_id,
    policy.policy_id,
    policy.policy_version,
    policy.catalog_generation,
    policy.switch_generation,
    policy.schema_version,
    policy.content_digest,
    JSON.stringify(payload),
  ]);
  const job = await client.query<{
    engine_account_id: string;
    policy_id: string;
    policy_version: string;
    catalog_generation: string;
    switch_generation: string;
    schema_version: string;
    content_digest: string;
    payload: unknown;
  }>(`
    SELECT engine_account_id, policy_id, policy_version::text, catalog_generation::text,
           switch_generation::text, schema_version::text, content_digest, payload
    FROM engine_policy_jobs WHERE binding_id = $1 AND effective_version = $2
  `, [account.binding_id, policy.effective_version]);
  const jobRow = job.rows[0];
  if (!jobRow) throw new Stage5BackfillError("policy_job_insert_lost", "policy control job was not stored");
  assertStored(`policy job ${account.engine_account_id}`, {
    engine_account_id: account.engine_account_id,
    policy_id: policy.policy_id,
    policy_version: policy.policy_version,
    catalog_generation: policy.catalog_generation,
    switch_generation: policy.switch_generation,
    schema_version: policy.schema_version,
    content_digest: policy.content_digest,
    payload,
  }, {
    engine_account_id: jobRow.engine_account_id,
    policy_id: jobRow.policy_id,
    policy_version: versionNumber(jobRow.policy_version, "job source policy version"),
    catalog_generation: versionNumber(jobRow.catalog_generation, "job catalog generation"),
    switch_generation: versionNumber(jobRow.switch_generation, "job switch generation"),
    schema_version: versionNumber(jobRow.schema_version, "job schema version"),
    content_digest: jobRow.content_digest,
    payload: jobRow.payload,
  });

  for (const scope of ["classification", "policy"] as const) {
    const reconciliationId = deterministicUuid(`reconciliation:${scope}:${account.engine_account_id}:1`);
    const details = {
      stage: 5,
      engine_account_id: account.engine_account_id,
      source_multiplier_bp: account.source_multiplier_bp,
      effective_policy_digest: policy.content_digest,
    };
    await client.query(`
      INSERT INTO account_policy_reconciliations (
        id, binding_id, effective_version, scope, status,
        legacy_account_class, legacy_multiplier_bp, expected_digest, observed_digest,
        details, completed_at
      ) VALUES ($1, $2, $3, $4, 'verified', $5, $6, $7, $7, $8::jsonb, now())
      ON CONFLICT (id) DO NOTHING
    `, [
      reconciliationId,
      account.binding_id,
      policy.effective_version,
      scope,
      account.account_class,
      account.source_multiplier_bp,
      policy.content_digest,
      JSON.stringify(details),
    ]);
    const stored = await client.query<{
      binding_id: string;
      effective_version: string;
      scope: string;
      status: string;
      legacy_account_class: string;
      legacy_multiplier_bp: number;
      expected_digest: string;
      observed_digest: string;
      details: unknown;
    }>(`
      SELECT binding_id::text, effective_version::text, scope, status,
             legacy_account_class, legacy_multiplier_bp, expected_digest, observed_digest, details
      FROM account_policy_reconciliations WHERE id = $1
    `, [reconciliationId]);
    const row = stored.rows[0];
    if (!row) throw new Stage5BackfillError("reconciliation_insert_lost", "account reconciliation was not stored");
    assertStored(`account ${scope} reconciliation`, {
      binding_id: account.binding_id,
      effective_version: policy.effective_version,
      scope,
      status: "verified",
      legacy_account_class: account.account_class,
      legacy_multiplier_bp: account.source_multiplier_bp,
      expected_digest: policy.content_digest,
      observed_digest: policy.content_digest,
      details,
    }, {
      ...row,
      effective_version: versionNumber(row.effective_version, "reconciliation effective version"),
    });
  }
}

function sourceRuleFromServiceRule(
  rule: Stage5ServiceAssignment["rules"][number],
): Stage5SourceRule {
  const scopeType = "provider" in rule.scope ? "provider" : "model";
  const providerId = "provider" in rule.scope
    ? rule.scope.provider.provider_id
    : rule.scope.model.provider_id;
  const canonicalModelId = "provider" in rule.scope ? null : rule.scope.model.canonical_model_id;
  return sourceRule({
    rule_id: rule.rule_id,
    scope_type: scopeType,
    provider_id: providerId,
    canonical_model_id: canonicalModelId,
    pricing_mode: "discount",
    rule_origin: "managed",
    discount_bps: rule.discount_bps,
    payable_multiplier_bp: 10_000 - rule.discount_bps,
    track_eligible: false,
    retention_eligible: false,
    commission_eligible: false,
  });
}

function effectiveRuleFromServiceRule(
  rule: Stage5ServiceAssignment["rules"][number],
): AccountPolicyRule {
  return effectiveRule({
    rule_id: rule.rule_id,
    scope: rule.scope,
    pricing_mode: "discount",
    rule_origin: "managed",
    discount_bps: rule.discount_bps,
    payable_multiplier_bp: 10_000 - rule.discount_bps,
    track_eligible: false,
    retention_eligible: false,
    commission_eligible: false,
  });
}

function buildServiceAccountPlan(
  assignment: Stage5ServiceAssignment,
  inventoryAccount: Stage5Inventory["engine_accounts"][number],
  catalogs: PricingCatalogSpec[],
): Stage5AccountPlan {
  const catalog = catalogs.find((candidate) => candidate.product_id === assignment.product_id);
  if (!catalog) throw new Stage5BackfillError("service_catalog_missing", `no catalog for ${assignment.product_id}`);
  const catalogModels = new Set(catalog.entries.map((entry) => `${entry.provider_id}\0${entry.canonical_model_id}`));
  const catalogProviders = new Set(catalog.entries.map((entry) => entry.provider_id));
  const scopes = new Set<string>();
  for (const rule of assignment.rules) {
    const providerId = "provider" in rule.scope
      ? rule.scope.provider.provider_id
      : rule.scope.model.provider_id;
    const modelId = "provider" in rule.scope ? null : rule.scope.model.canonical_model_id;
    const scopeKey = `${providerId}\0${modelId ?? ""}`;
    if (scopes.has(scopeKey)) {
      throw new Stage5BackfillError("duplicate_service_rule_scope", `service ${assignment.account_id} repeats rule scope`);
    }
    scopes.add(scopeKey);
    if (!catalogProviders.has(providerId) || (modelId !== null && !catalogModels.has(`${providerId}\0${modelId}`))) {
      throw new Stage5BackfillError(
        "service_rule_outside_catalog",
        `service ${assignment.account_id} rule references ${providerId}/${modelId ?? "*"} outside its catalog`,
      );
    }
  }
  const policy = sourcePolicy({
    policy_id: assignment.policy_id,
    owner_type: "service",
    owner_id: assignment.owner_id,
    product_id: assignment.product_id,
    replacement_locked: false,
    version: 1,
    rules: assignment.rules.map(sourceRuleFromServiceRule),
  });
  const effective = effectivePolicy({
    account_id: assignment.account_id,
    effective_version: 1,
    policy_id: policy.policy_id,
    policy_version: 1,
    source_policy_digest: policy.content_digest,
    owner_type: "service",
    owner_id: assignment.owner_id,
    account_class: "service",
    product_id: assignment.product_id,
    schema_version: STAGE5_SCHEMA_VERSION,
    catalog_generation: 1,
    switch_generation: 1,
    replacement_locked: false,
    rules: assignment.rules.map(effectiveRuleFromServiceRule),
  });
  return {
    binding_id: deterministicUuid(`binding:${assignment.account_id}`),
    user_id: null,
    engine_account_record_id: null,
    engine_account_id: assignment.account_id,
    account_class: "service",
    source_multiplier_bp: inventoryAccount.multiplier_bp,
    source_policy: policy,
    effective_policy: effective,
    binding: {
      policy_enforcement: "shadow",
      funding_enforcement: "legacy_single",
      reconciliation_state: "pending",
    },
  };
}

function assertNoSafeBlockers(plan: Stage5BackfillPlan): void {
  const blocker = plan.blockers.find((candidate) => candidate.scope === "safe");
  if (blocker) {
    throw new Stage5BackfillError(
      blocker.code,
      `safe Stage 5 apply is blocked for ${blocker.subject_id}: ${blocker.detail}`,
    );
  }
}

function assertNoUnresolvedProtectedBlockers(plan: Stage5BackfillPlan): void {
  const blocker = plan.blockers.find(
    (candidate) => candidate.scope === "protected" &&
      candidate.code !== "engine_account_requires_explicit_classification",
  );
  if (blocker) {
    throw new Stage5BackfillError(
      blocker.code,
      `approved Stage 5 apply is blocked for ${blocker.subject_id}: ${blocker.detail}`,
    );
  }
}

export async function runStage5Backfill(
  database: Database,
  rawInventory: Stage5Inventory,
  options: {
    mode: Stage5BackfillMode;
    assignment_matrix?: Stage5AssignmentMatrix;
  },
): Promise<Stage5BackfillResult> {
  const inventory = stage5InventorySchema.parse(rawInventory);
  if (options.mode === "dry_run") {
    const plan = await planStage5Backfill(database, inventory);
    return {
      mode: options.mode,
      plan,
      protected_assignment_digest: null,
      writes_committed: false,
    };
  }
  if (options.mode === "approved" && !options.assignment_matrix) {
    throw new Stage5BackfillError(
      "assignment_matrix_required",
      "approved Stage 5 apply requires the exact reviewed B2B/service/OpenKeys assignment matrix",
    );
  }

  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query("LOCK TABLE customer_profiles, engine_accounts, business_invites IN SHARE MODE");
    const snapshot = await readStage5Snapshot(client);
    const plan = buildStage5Plan(snapshot, inventory);
    assertNoSafeBlockers(plan);

    let approvedMatrix: Stage5AssignmentMatrix | null = null;
    if (options.mode === "approved") {
      assertNoUnresolvedProtectedBlockers(plan);
      approvedMatrix = validateStage5AssignmentMatrix(plan, options.assignment_matrix!, inventory);
    }

    await ensureCapabilityProjection(client, plan.capability);
    for (const catalog of plan.catalogs) await ensureCatalog(client, catalog);
    await ensureSwitches(client, plan.switches);
    for (const invitation of plan.safe.invitations) await ensureInvitation(client, invitation);
    for (const account of plan.safe.b2c_accounts) await ensureAccountPolicy(client, account);

    if (approvedMatrix) {
      for (const account of plan.protected.b2b_accounts) await ensureAccountPolicy(client, account);
      const inventoryById = uniqueBy(inventory.engine_accounts, (account) => account.account_id, "engine account");
      for (const assignment of approvedMatrix.service) {
        const inventoryAccount = inventoryById.get(assignment.account_id);
        if (!inventoryAccount) {
          throw new Stage5BackfillError("service_inventory_missing", `service ${assignment.account_id} is absent from inventory`);
        }
        await ensureAccountPolicy(
          client,
          buildServiceAccountPlan(assignment, inventoryAccount, plan.catalogs),
        );
        const assignmentEvidence = {
          accountId: assignment.account_id,
          productId: assignment.product_id,
          ownerId: assignment.owner_id,
          policyId: assignment.policy_id,
          purpose: assignment.purpose,
          responsible: assignment.responsible,
          assignmentMatrixDigest: approvedMatrix.content_digest,
          approvedAt: approvedMatrix.approved_at,
          reason: approvedMatrix.reason,
        };
        await client.query(`
          INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
          SELECT 'admin', $1, 'pricing.service_assignment.applied', 'pricing_policy', $2, $3::jsonb
          WHERE NOT EXISTS (
            SELECT 1 FROM audit_log
            WHERE action = 'pricing.service_assignment.applied'
              AND target_type = 'pricing_policy' AND target_id = $2
              AND metadata->>'assignmentMatrixDigest' = $4
          )
        `, [
          approvedMatrix.approved_by,
          assignment.policy_id,
          JSON.stringify(assignmentEvidence),
          approvedMatrix.content_digest,
        ]);
      }
    }

    await client.query("COMMIT");
    return {
      mode: options.mode,
      plan,
      protected_assignment_digest: approvedMatrix?.content_digest ?? null,
      writes_committed: true,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
