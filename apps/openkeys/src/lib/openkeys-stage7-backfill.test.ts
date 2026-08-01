import { createHash } from "node:crypto";
import {
  CURRENT_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  OPENKEYS_PRICING_PRODUCT_ID,
  type AccountPolicyBinding,
  type AccountPolicySpec,
  type PricingCatalogSpec,
  type PricingPolicySnapshot,
  type ProviderSwitchSpec,
} from "@claude-api/contracts";
import {
  buildOfficialOpenKeysPolicy,
  canonicalPricingJson,
  officialOpenKeysBinding,
} from "@claude-api/engine-client";
import { describe, expect, it, vi } from "vitest";
import { runStage7OpenKeysBackfill } from "./openkeys-stage7-backfill";

function stage5Digest(label: string, value: unknown): string {
  return `sha256:v1:${createHash("sha256")
    .update(`multi-discount-stage5:${label}\n`, "utf8")
    .update(canonicalPricingJson(value), "utf8")
    .digest("hex")}`;
}

function catalog(): PricingCatalogSpec {
  return {
    product_id: OPENKEYS_PRICING_PRODUCT_ID,
    generation: 1,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_CAPABILITY_DIGEST,
    content_digest: `sha256:v1:${"c".repeat(64)}`,
    entries: CURRENT_PRODUCT_CATALOG_ENTRIES.map((entry) => ({ ...entry })),
  };
}

function switches(): ProviderSwitchSpec {
  return {
    generation: 1,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_CAPABILITY_DIGEST,
    content_digest: `sha256:v1:${"d".repeat(64)}`,
    entries: ["anthropic", "openai"].flatMap((providerId) => [
      { provider_id: providerId, scope: "master" as const, catalog_generation: null, enabled: true },
      {
        provider_id: providerId,
        scope: { product: { product_id: OPENKEYS_PRICING_PRODUCT_ID } },
        catalog_generation: 1,
        enabled: true,
      },
    ]),
  };
}

function artifact() {
  const authority = { catalog: catalog(), switches: switches() };
  const plans = [
    {
      source_id: "source-active",
      account_id: "acct_stage7_active",
      status: "active" as const,
      pricing_contract: "official_1_to_1" as const,
      source_multiplier_bp: 10_000,
      effective_policy: buildOfficialOpenKeysPolicy("acct_stage7_active", authority),
      exception_code: null,
    },
    {
      source_id: "source-disabled",
      account_id: "acct_stage7_disabled",
      status: "disabled" as const,
      pricing_contract: "official_1_to_1" as const,
      source_multiplier_bp: 10_000,
      effective_policy: buildOfficialOpenKeysPolicy("acct_stage7_disabled", authority),
      exception_code: null,
    },
  ];
  const references = plans.map((plan) => ({
    account_id: plan.account_id,
    source_id: plan.source_id,
    source_multiplier_bp: plan.source_multiplier_bp,
    policy_id: plan.effective_policy.policy_id,
    policy_digest: plan.effective_policy.content_digest,
    exception_code: null,
  }));
  const planDigest = `sha256:v1:${"a".repeat(64)}`;
  const draftBase = {
    schema_version: 1 as const,
    plan_digest: planDigest,
    b2b: [],
    openkeys: references,
    unresolved_engine_accounts: [],
  };
  const draft = {
    ...draftBase,
    content_digest: stage5Digest("assignment-matrix-draft", draftBase),
  };
  const matrixBase = {
    schema_version: 1 as const,
    plan_digest: planDigest,
    approved_by: "pricing-owner@example.test",
    approved_at: "2026-08-01T00:00:00+00:00",
    reason: "Reviewed Stage 7 test artifact",
    b2b: [],
    openkeys: references,
    service: [],
    excluded_disabled_accounts: [],
  };
  return {
    dryRun: {
      mode: "dry_run" as const,
      writes_committed: false as const,
      protected_assignment_digest: null,
      plan: {
        schema_version: 1 as const,
        catalogs: [authority.catalog],
        switches: authority.switches,
        protected: { openkeys_accounts: plans },
        plan_digest: planDigest,
        assignment_matrix_draft: draft,
      },
    },
    matrix: {
      ...matrixBase,
      content_digest: stage5Digest("approved-assignment-matrix", matrixBase),
    },
    authority,
    plans,
  };
}

function fakeEngine(
  input: ReturnType<typeof artifact>,
  initial: Map<string, PricingPolicySnapshot> = new Map(),
) {
  const states = new Map<string, PricingPolicySnapshot>();
  for (const plan of input.plans) states.set(plan.account_id, initial.get(plan.account_id) ?? "unbound");
  const prepared = new Map<string, AccountPolicySpec>();
  const binding: AccountPolicyBinding = officialOpenKeysBinding();
  return {
    states,
    prepareAccountPolicy: vi.fn(async (policy: AccountPolicySpec) => {
      prepared.set(policy.account_id, policy);
      return { result: "stored" as const, identity: { policy } };
    }),
    getAccountPricingState: vi.fn(async (accountId: string) => states.get(accountId) ?? "unbound"),
    activateAccountPolicy: vi.fn(async (policy: AccountPolicySpec) => {
      states.set(policy.account_id, { active: { policy, binding } });
      return { result: "applied" as const, identity: { policy } };
    }),
    getActiveAccountPolicy: vi.fn(async (accountId: string) => {
      const state = states.get(accountId);
      return state !== undefined && state !== "unbound" && "active" in state ? state.active : null;
    }),
    getActivePricingCatalog: vi.fn(async () => input.authority.catalog),
    getActiveProviderSwitches: vi.fn(async () => input.authority.switches),
  };
}

describe("Stage 7 OpenKeys inventory backfill", () => {
  it("dry-runs every active and disabled account without mutating policy state", async () => {
    const input = artifact();
    const engine = fakeEngine(input);
    const result = await runStage7OpenKeysBackfill(engine, input.dryRun, input.matrix, "dry_run");

    expect(result).toMatchObject({
      result: "ready",
      counts: { total: 2, active: 1, disabled: 1, unbound: 2, exact: 0, conflict: 0 },
    });
    expect(engine.prepareAccountPolicy).not.toHaveBeenCalled();
    expect(engine.activateAccountPolicy).not.toHaveBeenCalled();
  });

  it("prepares, CAS-activates, exact-readbacks, and then replays unchanged", async () => {
    const input = artifact();
    const engine = fakeEngine(input);
    const applied = await runStage7OpenKeysBackfill(engine, input.dryRun, input.matrix, "apply");
    expect(applied).toMatchObject({
      result: "applied",
      counts: { total: 2, exact: 2, conflict: 0, activated: 2 },
    });
    expect(engine.prepareAccountPolicy).toHaveBeenCalledTimes(2);
    expect(engine.activateAccountPolicy).toHaveBeenCalledTimes(2);
    expect(engine.getActiveAccountPolicy).toHaveBeenCalledTimes(4);

    engine.prepareAccountPolicy.mockClear();
    engine.activateAccountPolicy.mockClear();
    engine.getActiveAccountPolicy.mockClear();
    const replay = await runStage7OpenKeysBackfill(engine, input.dryRun, input.matrix, "apply");
    expect(replay).toMatchObject({
      result: "unchanged",
      counts: { total: 2, exact: 2, conflict: 0, activated: 0 },
    });
    expect(engine.prepareAccountPolicy).not.toHaveBeenCalled();
    expect(engine.activateAccountPolicy).not.toHaveBeenCalled();
    expect(engine.getActiveAccountPolicy).toHaveBeenCalledTimes(2);
  });

  it("fails the complete apply preflight before writes when one account conflicts", async () => {
    const input = artifact();
    const wrongPolicy = {
      ...input.plans[0]!.effective_policy,
      content_digest: `sha256:v1:${"f".repeat(64)}`,
    };
    const initial = new Map<string, PricingPolicySnapshot>([[
      input.plans[0]!.account_id,
      { active: { policy: wrongPolicy, binding: officialOpenKeysBinding() } },
    ]]);
    const engine = fakeEngine(input, initial);
    const result = await runStage7OpenKeysBackfill(engine, input.dryRun, input.matrix, "apply");

    expect(result).toMatchObject({ result: "blocked", counts: { conflict: 1, unbound: 1 } });
    expect(engine.prepareAccountPolicy).not.toHaveBeenCalled();
    expect(engine.activateAccountPolicy).not.toHaveBeenCalled();
  });

  it("rejects a tampered approved matrix before reading or writing engine state", async () => {
    const input = artifact();
    const engine = fakeEngine(input);
    const tampered = structuredClone(input.matrix);
    tampered.openkeys[0]!.source_multiplier_bp = 9_999;

    await expect(runStage7OpenKeysBackfill(engine, input.dryRun, tampered, "dry_run"))
      .rejects.toMatchObject({ code: "assignment_matrix_digest_mismatch" });
    expect(engine.getAccountPricingState).not.toHaveBeenCalled();
    expect(engine.prepareAccountPolicy).not.toHaveBeenCalled();
  });
});
