import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import {
  accountPolicySpecSchema,
  pricingCatalogSpecSchema,
  providerSwitchSpecSchema,
  type AccountPolicyBinding,
  type AccountPolicySpec,
  type PricingCatalogSpec,
  type PricingPolicySnapshot,
  type ProviderSwitchSpec,
} from "@claude-api/contracts";
import {
  assertOpenKeysCatalog,
  assertOpenKeysSwitches,
  buildOfficialOpenKeysPolicy,
  canonicalPricingJson,
  officialOpenKeysBinding,
  stage7OpenKeysDigest,
} from "@claude-api/engine-client";
import { z } from "zod";

const sha256DigestSchema = z.string().regex(/^sha256:v1:[0-9a-f]{64}$/);
const accountIdSchema = z.string().startsWith("acct_").max(200);

const assignmentReferenceSchema = z.object({
  account_id: accountIdSchema,
  source_id: z.string().trim().min(1).max(200),
  source_multiplier_bp: z.number().int().min(0).max(100_000),
  policy_id: z.string().nullable(),
  policy_digest: z.string().nullable(),
  exception_code: z.string().nullable(),
}).strict();

const stage5OpenKeysPlanSchema = z.object({
  source_id: z.string().trim().min(1).max(200),
  account_id: accountIdSchema,
  status: z.enum(["active", "disabled"]),
  pricing_contract: z.enum(["legacy", "official_1_to_1"]),
  source_multiplier_bp: z.number().int().min(0).max(100_000),
  effective_policy: accountPolicySpecSchema.nullable(),
  exception_code: z.string().nullable(),
}).strict();

const assignmentDraftSchema = z.object({
  schema_version: z.literal(1),
  plan_digest: sha256DigestSchema,
  b2b: z.array(assignmentReferenceSchema),
  openkeys: z.array(assignmentReferenceSchema),
  unresolved_engine_accounts: z.array(accountIdSchema),
  content_digest: sha256DigestSchema,
}).strict();

const stage5DryRunSchema = z.object({
  mode: z.literal("dry_run"),
  writes_committed: z.literal(false),
  protected_assignment_digest: z.null(),
  plan: z.object({
    schema_version: z.literal(1),
    catalogs: z.array(pricingCatalogSpecSchema),
    switches: providerSwitchSpecSchema,
    protected: z.object({
      openkeys_accounts: z.array(stage5OpenKeysPlanSchema),
    }).passthrough(),
    plan_digest: sha256DigestSchema,
    assignment_matrix_draft: assignmentDraftSchema,
  }).passthrough(),
}).strict();

const stage5AssignmentMatrixSchema = z.object({
  schema_version: z.literal(1),
  plan_digest: sha256DigestSchema,
  approved_by: z.string().trim().min(1).max(200),
  approved_at: z.string().datetime({ offset: true }),
  reason: z.string().trim().min(1).max(2_000),
  b2b: z.array(assignmentReferenceSchema),
  openkeys: z.array(assignmentReferenceSchema),
  service: z.array(z.unknown()),
  excluded_disabled_accounts: z.array(accountIdSchema),
  content_digest: sha256DigestSchema,
}).strict();

type Stage5OpenKeysPlan = z.infer<typeof stage5OpenKeysPlanSchema>;
type Stage5AssignmentReference = z.infer<typeof assignmentReferenceSchema>;

export type Stage7OpenKeysBackfillMode = "dry_run" | "apply";
export type Stage7OpenKeysState = "unbound" | "exact" | "conflict";

export interface Stage7OpenKeysSubject {
  account_id: string;
  source_id: string;
  status: "active" | "disabled";
  pricing_contract: "legacy" | "official_1_to_1";
  policy_id: string | null;
  policy_digest: string | null;
  state: Stage7OpenKeysState;
  action: "none" | "activated";
  detail: string | null;
}

export interface Stage7OpenKeysBackfillReport {
  schema_version: 1;
  mode: Stage7OpenKeysBackfillMode;
  plan_digest: string;
  assignment_matrix_digest: string;
  result: "ready" | "blocked" | "applied" | "unchanged";
  counts: {
    total: number;
    active: number;
    disabled: number;
    unbound: number;
    exact: number;
    conflict: number;
    activated: number;
  };
  subjects: Stage7OpenKeysSubject[];
  content_digest: string;
}

export class Stage7OpenKeysBackfillError extends Error {
  constructor(readonly code: string, message: string) {
    super(message);
    this.name = "Stage7OpenKeysBackfillError";
  }
}

interface Stage7PricingEngine {
  prepareAccountPolicy(policy: AccountPolicySpec): Promise<{ result: string }>;
  activateAccountPolicy(
    policy: AccountPolicySpec,
    binding: AccountPolicyBinding,
    expectation: "unbound",
  ): Promise<{ result: string }>;
  getAccountPricingState(accountId: string): Promise<PricingPolicySnapshot>;
  getActiveAccountPolicy(accountId: string): Promise<{
    policy: AccountPolicySpec;
    binding: AccountPolicyBinding;
  } | null>;
  getActivePricingCatalog(productId: string): Promise<PricingCatalogSpec | null>;
  getActiveProviderSwitches(): Promise<ProviderSwitchSpec | null>;
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function stage5Digest(label: string, value: unknown): string {
  const hex = createHash("sha256")
    .update(`multi-discount-stage5:${label}\n`, "utf8")
    .update(canonicalPricingJson(value), "utf8")
    .digest("hex");
  return `sha256:v1:${hex}`;
}

function sameCanonical(left: unknown, right: unknown): boolean {
  return canonicalPricingJson(left) === canonicalPricingJson(right);
}

function withoutDigest<T extends { content_digest: string }>(value: T): Omit<T, "content_digest"> {
  const { content_digest: _digest, ...base } = value;
  return base;
}

function sortedReferences(values: Stage5AssignmentReference[]): Stage5AssignmentReference[] {
  return [...values].sort((left, right) => compareUtf8(left.account_id, right.account_id));
}

function referenceForPlan(plan: Stage5OpenKeysPlan): Stage5AssignmentReference {
  return {
    account_id: plan.account_id,
    source_id: plan.source_id,
    source_multiplier_bp: plan.source_multiplier_bp,
    policy_id: plan.effective_policy?.policy_id ?? null,
    policy_digest: plan.effective_policy?.content_digest ?? null,
    exception_code: plan.exception_code,
  };
}

function assertUnique(values: string[], label: string): void {
  if (new Set(values).size !== values.length) {
    throw new Stage7OpenKeysBackfillError("duplicate_identity", `duplicate ${label} identity`);
  }
}

function effectiveRuleToStage5Source(rule: AccountPolicySpec["rules"][number]) {
  const scope = "provider" in rule.scope
    ? {
      scope_type: "provider" as const,
      provider_id: rule.scope.provider.provider_id,
      canonical_model_id: null,
    }
    : {
      scope_type: "model" as const,
      provider_id: rule.scope.model.provider_id,
      canonical_model_id: rule.scope.model.canonical_model_id,
    };
  const base = {
    rule_id: rule.rule_id,
    ...scope,
    pricing_mode: rule.pricing_mode,
    rule_origin: rule.rule_origin,
    discount_bps: rule.discount_bps,
    payable_multiplier_bp: rule.payable_multiplier_bp,
    track_eligible: rule.track_eligible,
    retention_eligible: rule.retention_eligible,
    commission_eligible: rule.commission_eligible,
  };
  return { ...base, rule_digest: stage5Digest("source-rule", base) };
}

function assertLegacyPolicy(plan: Stage5OpenKeysPlan, policy: AccountPolicySpec): void {
  const expectedPolicyId = `policy:openkeys:legacy:${plan.source_id}`;
  const expectedProviders = ["anthropic", "openai"];
  if (
    policy.account_id !== plan.account_id ||
    policy.policy_id !== expectedPolicyId ||
    policy.owner_type !== "open_keys" ||
    policy.owner_id !== plan.source_id ||
    policy.account_class !== "open_keys" ||
    policy.product_id !== "openkeys" ||
    policy.effective_version !== 1 ||
    policy.policy_version !== 1 ||
    policy.schema_version !== 1 ||
    policy.replacement_locked !== true ||
    policy.rules.length !== expectedProviders.length
  ) {
    throw new Stage7OpenKeysBackfillError(
      "legacy_policy_shape_mismatch",
      `legacy OpenKeys policy ${plan.account_id} does not match its locked source identity`,
    );
  }

  const rules = [...policy.rules].sort((left, right) => {
    const leftProvider = "provider" in left.scope ? left.scope.provider.provider_id : "";
    const rightProvider = "provider" in right.scope ? right.scope.provider.provider_id : "";
    return compareUtf8(leftProvider, rightProvider);
  });
  for (const [index, providerId] of expectedProviders.entries()) {
    const rule = rules[index]!;
    const { rule_digest: _ruleDigest, ...base } = rule;
    if (
      !("provider" in rule.scope) ||
      rule.scope.provider.provider_id !== providerId ||
      rule.rule_id !== `provider:${providerId}:legacy` ||
      rule.pricing_mode !== "discount" ||
      rule.rule_origin !== "legacy" ||
      rule.discount_bps !== null ||
      rule.payable_multiplier_bp !== plan.source_multiplier_bp ||
      rule.track_eligible ||
      rule.retention_eligible ||
      rule.commission_eligible ||
      rule.rule_digest !== stage5Digest("effective-rule", base)
    ) {
      throw new Stage7OpenKeysBackfillError(
        "legacy_rule_mismatch",
        `legacy OpenKeys rule ${plan.account_id}/${providerId} is not the exact Stage 5 projection`,
      );
    }
  }

  const sourceRules = rules.map(effectiveRuleToStage5Source);
  const sourceDigest = stage5Digest("openkeys-source-policy", {
    policy_id: expectedPolicyId,
    owner_id: plan.source_id,
    product_id: "openkeys",
    replacement_locked: true,
    version: 1,
    rules: sourceRules,
  });
  if (policy.source_policy_digest !== sourceDigest) {
    throw new Stage7OpenKeysBackfillError(
      "legacy_source_digest_mismatch",
      `legacy OpenKeys source digest ${plan.account_id} is invalid`,
    );
  }
  if (policy.content_digest !== stage5Digest("effective-policy", withoutDigest(policy))) {
    throw new Stage7OpenKeysBackfillError(
      "legacy_effective_digest_mismatch",
      `legacy OpenKeys effective digest ${plan.account_id} is invalid`,
    );
  }
}

function report(
  mode: Stage7OpenKeysBackfillMode,
  planDigest: string,
  matrixDigest: string,
  result: Stage7OpenKeysBackfillReport["result"],
  subjects: Stage7OpenKeysSubject[],
): Stage7OpenKeysBackfillReport {
  const ordered = [...subjects].sort((left, right) => compareUtf8(left.account_id, right.account_id));
  const counts = {
    total: ordered.length,
    active: ordered.filter((subject) => subject.status === "active").length,
    disabled: ordered.filter((subject) => subject.status === "disabled").length,
    unbound: ordered.filter((subject) => subject.state === "unbound").length,
    exact: ordered.filter((subject) => subject.state === "exact").length,
    conflict: ordered.filter((subject) => subject.state === "conflict").length,
    activated: ordered.filter((subject) => subject.action === "activated").length,
  };
  const base = {
    schema_version: 1 as const,
    mode,
    plan_digest: planDigest,
    assignment_matrix_digest: matrixDigest,
    result,
    counts,
    subjects: ordered,
  };
  return { ...base, content_digest: stage7OpenKeysDigest("batch-report", base) };
}

function classifyState(
  plan: Stage5OpenKeysPlan,
  state: PricingPolicySnapshot | null,
  binding: AccountPolicyBinding,
): Pick<Stage7OpenKeysSubject, "state" | "detail"> {
  if (plan.exception_code !== null || plan.effective_policy === null) {
    return {
      state: "conflict",
      detail: plan.exception_code ?? "missing_effective_policy",
    };
  }
  if (state === "unbound") return { state: "unbound", detail: null };
  if (
    state !== null &&
    "active" in state &&
    sameCanonical(state.active.policy, plan.effective_policy) &&
    sameCanonical(state.active.binding, binding)
  ) {
    return { state: "exact", detail: null };
  }
  return { state: "conflict", detail: "engine_policy_state_mismatch" };
}

function assertMutationAccepted(ack: { result: string }, phase: string, accountId: string): void {
  if (ack.result === "rejected") {
    throw new Stage7OpenKeysBackfillError(
      "policy_ack_rejected",
      `engine rejected Stage 7 ${phase} for ${accountId}`,
    );
  }
}

export async function runStage7OpenKeysBackfill(
  engine: Stage7PricingEngine,
  rawDryRun: unknown,
  rawMatrix: unknown,
  mode: Stage7OpenKeysBackfillMode,
): Promise<Stage7OpenKeysBackfillReport> {
  const dryRun = stage5DryRunSchema.parse(rawDryRun);
  const matrix = stage5AssignmentMatrixSchema.parse(rawMatrix);
  const draft = dryRun.plan.assignment_matrix_draft;

  if (draft.content_digest !== stage5Digest("assignment-matrix-draft", withoutDigest(draft))) {
    throw new Stage7OpenKeysBackfillError(
      "assignment_draft_digest_mismatch",
      "Stage 5 assignment draft digest is invalid",
    );
  }
  if (matrix.content_digest !== stage5Digest("approved-assignment-matrix", withoutDigest(matrix))) {
    throw new Stage7OpenKeysBackfillError(
      "assignment_matrix_digest_mismatch",
      "approved Stage 5 assignment matrix digest is invalid",
    );
  }
  if (
    matrix.plan_digest !== dryRun.plan.plan_digest ||
    draft.plan_digest !== dryRun.plan.plan_digest
  ) {
    throw new Stage7OpenKeysBackfillError(
      "assignment_matrix_stale",
      "Stage 5 dry-run, draft, and approved matrix target different plans",
    );
  }
  if (!sameCanonical(sortedReferences(matrix.openkeys), sortedReferences(draft.openkeys))) {
    throw new Stage7OpenKeysBackfillError(
      "openkeys_assignment_mismatch",
      "approved OpenKeys references do not exactly match the Stage 5 dry-run",
    );
  }

  const plans = [...dryRun.plan.protected.openkeys_accounts]
    .sort((left, right) => compareUtf8(left.account_id, right.account_id));
  assertUnique(plans.map((plan) => plan.account_id), "OpenKeys account");
  assertUnique(plans.map((plan) => plan.source_id), "OpenKeys source");
  if (!sameCanonical(sortedReferences(plans.map(referenceForPlan)), sortedReferences(draft.openkeys))) {
    throw new Stage7OpenKeysBackfillError(
      "openkeys_plan_reference_mismatch",
      "Stage 5 OpenKeys policy payloads do not match their approved references",
    );
  }

  const catalog = dryRun.plan.catalogs.find((candidate) => candidate.product_id === "openkeys");
  if (catalog === undefined) {
    throw new Stage7OpenKeysBackfillError("openkeys_catalog_missing", "Stage 5 plan has no OpenKeys catalog");
  }
  assertOpenKeysCatalog(catalog);
  assertOpenKeysSwitches(dryRun.plan.switches, catalog);
  const authority = { catalog, switches: dryRun.plan.switches };

  for (const plan of plans) {
    if (plan.effective_policy === null) continue;
    if (plan.pricing_contract === "official_1_to_1") {
      const expected = buildOfficialOpenKeysPolicy(plan.account_id, authority);
      if (!sameCanonical(plan.effective_policy, expected)) {
        throw new Stage7OpenKeysBackfillError(
          "official_policy_mismatch",
          `official OpenKeys policy ${plan.account_id} is not the canonical Stage 7 identity`,
        );
      }
    } else {
      assertLegacyPolicy(plan, plan.effective_policy);
    }
  }

  const [activeCatalog, activeSwitches] = await Promise.all([
    engine.getActivePricingCatalog("openkeys"),
    engine.getActiveProviderSwitches(),
  ]);
  if (!sameCanonical(activeCatalog, catalog) || !sameCanonical(activeSwitches, dryRun.plan.switches)) {
    throw new Stage7OpenKeysBackfillError(
      "engine_authority_mismatch",
      "engine active OpenKeys catalog/switch authority does not exactly match the approved Stage 5 plan",
    );
  }

  const binding = officialOpenKeysBinding();
  const subjects: Stage7OpenKeysSubject[] = [];
  for (const plan of plans) {
    const state = plan.effective_policy === null
      ? null
      : await engine.getAccountPricingState(plan.account_id);
    const classification = classifyState(plan, state, binding);
    subjects.push({
      account_id: plan.account_id,
      source_id: plan.source_id,
      status: plan.status,
      pricing_contract: plan.pricing_contract,
      policy_id: plan.effective_policy?.policy_id ?? null,
      policy_digest: plan.effective_policy?.content_digest ?? null,
      state: classification.state,
      action: "none",
      detail: classification.detail,
    });
  }

  const conflicts = subjects.filter((subject) => subject.state === "conflict");
  if (conflicts.length > 0) {
    return report(mode, dryRun.plan.plan_digest, matrix.content_digest, "blocked", subjects);
  }
  if (mode === "dry_run") {
    return report(mode, dryRun.plan.plan_digest, matrix.content_digest, "ready", subjects);
  }

  for (const subject of subjects) {
    if (subject.state === "exact") continue;
    const plan = plans.find((candidate) => candidate.account_id === subject.account_id)!;
    const policy = plan.effective_policy!;
    const prepared = await engine.prepareAccountPolicy(policy);
    assertMutationAccepted(prepared, "prepare", policy.account_id);

    const state = await engine.getAccountPricingState(policy.account_id);
    if (
      state !== "unbound" &&
      "active" in state &&
      sameCanonical(state.active.policy, policy) &&
      sameCanonical(state.active.binding, binding)
    ) {
      subject.state = "exact";
      continue;
    }
    if (state !== "unbound") {
      throw new Stage7OpenKeysBackfillError(
        "policy_cas_precondition_changed",
        `OpenKeys policy state changed before activation for ${policy.account_id}`,
      );
    }

    const activated = await engine.activateAccountPolicy(policy, binding, "unbound");
    assertMutationAccepted(activated, "activation", policy.account_id);
    const readback = await engine.getActiveAccountPolicy(policy.account_id);
    if (
      readback === null ||
      !sameCanonical(readback.policy, policy) ||
      !sameCanonical(readback.binding, binding)
    ) {
      throw new Stage7OpenKeysBackfillError(
        "policy_ack_readback_mismatch",
        `Stage 7 policy readback is not exact for ${policy.account_id}`,
      );
    }
    subject.state = "exact";
    subject.action = "activated";
  }

  for (const plan of plans) {
    const policy = plan.effective_policy!;
    const readback = await engine.getActiveAccountPolicy(plan.account_id);
    if (
      readback === null ||
      !sameCanonical(readback.policy, policy) ||
      !sameCanonical(readback.binding, binding)
    ) {
      throw new Stage7OpenKeysBackfillError(
        "final_policy_readback_mismatch",
        `final Stage 7 policy readback is not exact for ${plan.account_id}`,
      );
    }
  }

  const activatedCount = subjects.filter((subject) => subject.action === "activated").length;
  return report(
    mode,
    dryRun.plan.plan_digest,
    matrix.content_digest,
    activatedCount > 0 ? "applied" : "unchanged",
    subjects,
  );
}
