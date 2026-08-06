import type {
  OpenKeysPricingInventoryAccountV2,
  PricingReleaseInventoryAccountV2,
  PricingReleaseProvisioningContextV2,
  ServiceAccountInventoryEntryV2,
} from "@claude-api/contracts";
import {
  buildOpenKeysPricingReleasePolicyV2,
  buildServicePricingReleasePolicyV2,
} from "@claude-api/engine-client";
import { describe, expect, it } from "vitest";
import {
  buildStage5ServiceInventoryV2,
  buildStage5V2CatalogsAndSwitches,
  buildStage5V2Plan,
  createStage5OpenKeysInventoryReaderV2,
  scanStage5EngineInventoryV2,
  scanStage5OpenKeysInventoryV2,
  stage5V2Digest,
  stage5V2EngineFullDigest,
  stage5V2EngineIdentityDigest,
  type Stage5V2EngineScan,
  type Stage5V2OpenKeysScan,
} from "./pricing-stage5-materializer-v2.js";

function engineAccount(
  accountId: string,
  options: Partial<PricingReleaseInventoryAccountV2> = {},
): PricingReleaseInventoryAccountV2 {
  return {
    account_id: accountId,
    status: "active",
    multiplier_bp: 10_000,
    balance_nano: "100",
    reserved_nano: "0",
    spent_nano: "0",
    funding_generation: null,
    funding_head_version: null,
    ...options,
  };
}

function engineScan(accounts: PricingReleaseInventoryAccountV2[]): Stage5V2EngineScan {
  return {
    accounts,
    identity_digest: stage5V2EngineIdentityDigest(accounts),
    full_digest: stage5V2EngineFullDigest(accounts),
  };
}

function openkeysAccount(
  accountId: string,
  lifecycle: OpenKeysPricingInventoryAccountV2["lifecycle"] = "active",
): OpenKeysPricingInventoryAccountV2 {
  const identity = {
    account_id: accountId,
    source_id: "10000000-0000-4000-8000-000000000001",
    lifecycle,
    pricing_contract: "legacy" as const,
    source_multiplier_bp: 3_700,
  };
  return { ...identity, content_digest: stage5V2Digest("test-openkeys", identity) };
}

function openkeysScan(accounts: OpenKeysPricingInventoryAccountV2[]): Stage5V2OpenKeysScan {
  return {
    accounts,
    inventory_digest: stage5V2Digest("test-openkeys-manifest", accounts),
  };
}

function serviceAccount(
  accountId = "acct_service",
  status: ServiceAccountInventoryEntryV2["status"] = "active",
): ServiceAccountInventoryEntryV2 {
  const identity = {
    service_id: "internal-worker",
    engine_account_id: accountId,
    purpose: "internal metered automation",
    responsible: "platform-team",
    status,
    source_version: 1,
  };
  return { ...identity, content_digest: stage5V2Digest("test-service", identity) };
}

function completePlanInput(): Parameters<typeof buildStage5V2Plan>[0] {
  const engineFirstAccounts = [
    engineAccount("acct_b2b", { multiplier_bp: 7_000 }),
    engineAccount("acct_b2c", { multiplier_bp: 4_000 }),
    engineAccount("acct_openkeys", { status: "disabled", multiplier_bp: 3_700 }),
    engineAccount("acct_service"),
  ];
  const engineSecondAccounts = engineFirstAccounts.map((account) => ({
    ...account,
    balance_nano: String(BigInt(account.balance_nano) + 50n),
    spent_nano: String(BigInt(account.spent_nano) + 10n),
  }));
  const openkeys = [openkeysAccount("acct_openkeys", "removed")];
  return {
    commerce: {
      accounts: [
        {
          user_id: "20000000-0000-4000-8000-000000000001",
          engine_account_record_id: "30000000-0000-4000-8000-000000000001",
          engine_account_id: "acct_b2c",
          account_class: "b2c" as const,
          profile_multiplier_bp: 4_000,
          commerce_multiplier_bp: 4_000,
          commerce_status: "active" as const,
          policy_rules: null,
        },
        {
          user_id: "20000000-0000-4000-8000-000000000002",
          engine_account_record_id: "30000000-0000-4000-8000-000000000002",
          engine_account_id: "acct_b2b",
          account_class: "b2b" as const,
          profile_multiplier_bp: 7_000,
          commerce_multiplier_bp: 7_000,
          commerce_status: "active" as const,
          policy_rules: null,
        },
      ],
      invitations: [{
        invite_id: "40000000-0000-4000-8000-000000000001",
        multiplier_bp: 6_500,
        expires_at: "2030-01-01T00:00:00.000Z",
      }],
    },
    service: buildStage5ServiceInventoryV2([serviceAccount()]),
    engine_first: engineScan(engineFirstAccounts),
    engine_second: engineScan(engineSecondAccounts),
    openkeys_first: openkeysScan(openkeys),
    openkeys_second: openkeysScan(openkeys),
    head_first: null,
    head_second: null,
    target_generation: 1,
    recovery_generation: 2,
  };
}

describe("pricing Stage 5 v2 planner", () => {
  it("keeps Gemini main-only and admits GPT Image 2 to both customer products", () => {
    const { catalogs, switches } = buildStage5V2CatalogsAndSwitches();
    const googleEntries = catalogs[0].entries.filter((entry) => entry.provider_id === "google");
    expect(googleEntries).toHaveLength(9);
    expect(googleEntries).toContainEqual(expect.objectContaining({
      canonical_model_id: "gemini-3-flash-preview",
    }));
    expect(catalogs[1].entries.some((entry) => entry.provider_id === "google")).toBe(false);
    for (const catalog of catalogs) {
      expect(catalog.entries).toContainEqual({
        provider_id: "openai",
        canonical_model_id: "gpt-image-2-2026-04-21",
        enabled: true,
      });
    }
    expect(switches.entries).toContainEqual({
      provider_id: "google",
      scope: { product: { product_id: "main" } },
      catalog_generation: 6,
      enabled: true,
    });
    expect(switches.entries).not.toContainEqual(expect.objectContaining({
      provider_id: "google",
      scope: { product: { product_id: "openkeys" } },
    }));
    for (const catalog of catalogs) {
      expect(catalog.entries).toEqual([...catalog.entries].sort((left, right) =>
        left.provider_id.localeCompare(right.provider_id)
        || left.canonical_model_id.localeCompare(right.canonical_model_id)));
    }
  });

  it("builds complete deterministic target and recovery skeletons without live funding identities", () => {
    const plan = buildStage5V2Plan(completePlanInput());

    expect(plan.blockers).toEqual([]);
    expect(plan.capability.generation).toBe(6);
    expect(plan.catalogs.every((catalog) => catalog.generation === 6)).toBe(true);
    expect(plan.switches.generation).toBe(6);
    expect(plan.policies.every((policy) => policy.policy_version === 3)).toBe(true);
    expect(plan.target.assignments).toHaveLength(4);
    expect(plan.recovery.assignments).toHaveLength(4);
    expect(plan.target.assignments.every((item) => item.funding_generation === null)).toBe(true);
    expect(plan.target.funding_manifest_digest).toBeNull();
    expect(plan.target.engine_release_digest).toBeNull();
    expect(plan.target_digest).toBeNull();
    expect(plan.recovery_digest).toBeNull();

    const b2c = plan.policies.find((policy) => policy.policy_id === "release-v2:b2c:global")!;
    expect(b2c.rules).toEqual([expect.objectContaining({
      scope: { scope: "global" },
      discount_bps: 5_000,
      payable_multiplier_bp: 5_000,
    })]);
    const b2b = plan.policies.find((policy) => policy.policy_id === "release-v2:b2b:acct_b2b")!;
    expect(b2b.rules).toEqual([expect.objectContaining({
      scope: { scope: "provider", provider_id: "anthropic" },
      discount_bps: 3_000,
      payable_multiplier_bp: 7_000,
    })]);
    const openkeys = plan.policies.find((policy) => policy.policy_id === "release-v2:openkeys:global")!;
    expect(openkeys.rules).toEqual([expect.objectContaining({ discount_bps: 0 })]);
    const service = plan.policies.find((policy) => policy.policy_id === "release-v2:service:internal-worker")!;
    expect(service).toMatchObject({ billing_mode: "meter_only", product_id: null, rules: [] });
    expect(plan.target.assignments.find((item) => item.engine_account_id === "acct_openkeys"))
      .toMatchObject({ account_class: "openkeys", billing_mode: "balance" });
    expect(plan.target.assignments.find((item) => item.engine_account_id === "acct_service"))
      .toMatchObject({
        account_class: "service",
        billing_mode: "meter_only",
        purpose: "internal metered automation",
        responsible: "platform-team",
      });
  });

  it("carries the operator-approved B2B policy head rules into the target policy", () => {
    const input = completePlanInput();
    input.commerce.accounts[1]!.policy_rules = [
      {
        scope_type: "provider",
        provider_id: "anthropic",
        canonical_model_id: null,
        pricing_mode: "discount",
        payable_multiplier_bp: 7_000,
      },
      {
        scope_type: "provider",
        provider_id: "google",
        canonical_model_id: null,
        pricing_mode: "discount",
        payable_multiplier_bp: 7_000,
      },
      {
        scope_type: "model",
        provider_id: "openai",
        canonical_model_id: "gpt-5.5",
        pricing_mode: "discount",
        payable_multiplier_bp: 8_000,
      },
    ];
    const plan = buildStage5V2Plan(input);

    expect(plan.blockers).toEqual([]);
    const b2b = plan.policies.find((policy) => policy.policy_id === "release-v2:b2b:acct_b2b")!;
    expect(b2b.rules).toHaveLength(3);
    expect(b2b.rules).toEqual(expect.arrayContaining([
      expect.objectContaining({
        rule_id: "provider:anthropic",
        scope: { scope: "provider", provider_id: "anthropic" },
        discount_bps: 3_000,
        payable_multiplier_bp: 7_000,
      }),
      expect.objectContaining({
        rule_id: "provider:google",
        scope: { scope: "provider", provider_id: "google" },
        discount_bps: 3_000,
        payable_multiplier_bp: 7_000,
      }),
      expect.objectContaining({
        rule_id: "model:openai:gpt-5.5",
        scope: { scope: "model", provider_id: "openai", canonical_model_id: "gpt-5.5" },
        discount_bps: 2_000,
        payable_multiplier_bp: 8_000,
      }),
    ]));
  });

  it("blocks a B2B policy head that drops or reprices the live anthropic scalar rule", () => {
    const missing = completePlanInput();
    missing.commerce.accounts[1]!.policy_rules = [{
      scope_type: "provider",
      provider_id: "google",
      canonical_model_id: null,
      pricing_mode: "discount",
      payable_multiplier_bp: 7_000,
    }];
    expect(buildStage5V2Plan(missing).blockers).toContainEqual(expect.objectContaining({
      blocker_code: "b2b_policy_anthropic_rule_mismatch",
      subject_id: "acct_b2b",
    }));

    const repriced = completePlanInput();
    repriced.commerce.accounts[1]!.policy_rules = [{
      scope_type: "provider",
      provider_id: "anthropic",
      canonical_model_id: null,
      pricing_mode: "discount",
      payable_multiplier_bp: 6_500,
    }];
    expect(buildStage5V2Plan(repriced).blockers).toContainEqual(expect.objectContaining({
      blocker_code: "b2b_policy_anthropic_rule_mismatch",
      subject_id: "acct_b2b",
    }));
  });

  it("blocks a B2B policy head rule the release-v2 target cannot express", () => {
    const input = completePlanInput();
    input.commerce.accounts[1]!.policy_rules = [
      {
        scope_type: "provider",
        provider_id: "anthropic",
        canonical_model_id: null,
        pricing_mode: "discount",
        payable_multiplier_bp: 7_000,
      },
      {
        scope_type: "provider",
        provider_id: "google",
        canonical_model_id: null,
        pricing_mode: "track",
        payable_multiplier_bp: 7_000,
      },
    ];
    expect(buildStage5V2Plan(input).blockers).toContainEqual(expect.objectContaining({
      blocker_code: "b2b_policy_rule_unsupported",
      subject_id: "acct_b2b",
    }));
  });

  it("keeps the baseline policy identity for an anthropic-only B2B head", () => {
    const withHead = completePlanInput();
    withHead.commerce.accounts[1]!.policy_rules = [{
      scope_type: "provider",
      provider_id: "anthropic",
      canonical_model_id: null,
      pricing_mode: "discount",
      payable_multiplier_bp: 7_000,
    }];
    const withoutHead = completePlanInput();
    const headPlan = buildStage5V2Plan(withHead);
    const basePlan = buildStage5V2Plan(withoutHead);

    expect(headPlan.blockers).toEqual([]);
    const headPolicy = headPlan.policies.find((policy) => policy.policy_id === "release-v2:b2b:acct_b2b")!;
    const basePolicy = basePlan.policies.find((policy) => policy.policy_id === "release-v2:b2b:acct_b2b")!;
    expect(headPolicy).toEqual(basePolicy);
    expect(headPlan.target.assignments.find((item) => item.engine_account_id === "acct_b2b"))
      .toEqual(basePlan.target.assignments.find((item) => item.engine_account_id === "acct_b2b"));
  });

  it("bumps the release policy version when persisted content changes", () => {
    const baseline = buildStage5V2Plan(completePlanInput());
    const baselinePolicy = baseline.policies
      .find((policy) => policy.policy_id === "release-v2:b2b:acct_b2b")!;

    const unchanged = completePlanInput();
    unchanged.existing_release_policies = [{
      policy_id: baselinePolicy.policy_id,
      policy_version: baselinePolicy.policy_version,
      content_digest: baselinePolicy.content_digest,
    }];
    const unchangedPlan = buildStage5V2Plan(unchanged);
    expect(unchangedPlan.policies.find((policy) => policy.policy_id === baselinePolicy.policy_id))
      .toEqual(baselinePolicy);

    const extended = completePlanInput();
    extended.existing_release_policies = [{
      policy_id: baselinePolicy.policy_id,
      policy_version: baselinePolicy.policy_version,
      content_digest: baselinePolicy.content_digest,
    }];
    extended.commerce.accounts[1]!.policy_rules = [
      {
        scope_type: "provider",
        provider_id: "anthropic",
        canonical_model_id: null,
        pricing_mode: "discount",
        payable_multiplier_bp: 7_000,
      },
      {
        scope_type: "provider",
        provider_id: "google",
        canonical_model_id: null,
        pricing_mode: "discount",
        payable_multiplier_bp: 7_000,
      },
    ];
    const extendedPlan = buildStage5V2Plan(extended);
    expect(extendedPlan.blockers).toEqual([]);
    const bumped = extendedPlan.policies
      .find((policy) => policy.policy_id === baselinePolicy.policy_id)!;
    expect(bumped.policy_version).toBe(baselinePolicy.policy_version + 1);
    expect(bumped.rules).toHaveLength(2);
    expect(extendedPlan.target.assignments
      .find((item) => item.engine_account_id === "acct_b2b")!.policy_version)
      .toBe(bumped.policy_version);
  });

  it("shares the exact Stage 5 policy identity with post-cutover external-owner writers", () => {
    const plan = buildStage5V2Plan(completePlanInput());
    const mainCatalog = plan.catalogs.find((catalog) => catalog.product_id === "main")!;
    const openkeysCatalog = plan.catalogs.find((catalog) => catalog.product_id === "openkeys")!;
    const release = {
      generation: plan.target_generation,
      release_kind: "target" as const,
      schema_version: 2 as const,
      capability_generation: plan.capability.generation,
      capability_digest: plan.capability.content_digest,
      main_catalog_generation: mainCatalog.generation,
      main_catalog_digest: mainCatalog.content_digest,
      openkeys_catalog_generation: openkeysCatalog.generation,
      openkeys_catalog_digest: openkeysCatalog.content_digest,
      switch_generation: plan.switches.generation,
      switch_digest: plan.switches.content_digest,
      inventory_digest: stage5V2Digest("test-inventory", []),
      funding_manifest_digest: stage5V2Digest("test-funding", []),
      minimum_runtime_schema_version: 2,
      content_digest: stage5V2Digest("test-release", []),
    };
    const context: PricingReleaseProvisioningContextV2 = {
      head: {
        active_generation: release.generation,
        active_digest: release.content_digest,
        head_version: 1,
        updated_ts: 1,
      },
      activation: {
        activation_id: "1",
        activation_kind: "cutover",
        evidence_digest: stage5V2Digest("test-evidence", []),
        activated_ts: 1,
      },
      active_release: release,
      paired_recovery: {
        release: { ...release, generation: plan.recovery_generation, release_kind: "recovery" },
        recovery_link: {
          target_generation: release.generation,
          target_digest: release.content_digest,
          recovery_generation: plan.recovery_generation,
          recovery_digest: release.content_digest,
          link_digest: stage5V2Digest("test-link", []),
        },
      },
    };
    expect(buildOpenKeysPricingReleasePolicyV2(context)).toEqual(
      plan.policies.find((policy) => policy.policy_id === "release-v2:openkeys:global"),
    );
    expect(buildServicePricingReleasePolicyV2(context, "internal-worker")).toEqual(
      plan.policies.find((policy) => policy.policy_id === "release-v2:service:internal-worker"),
    );
  });

  it("treats moving money as Stage 6 state while blocking ownership and scalar ambiguity", () => {
    const input = completePlanInput();
    input.commerce.accounts[1]!.profile_multiplier_bp = 6_900;
    input.service = buildStage5ServiceInventoryV2([serviceAccount("acct_b2c")]);
    const plan = buildStage5V2Plan(input);

    expect(input.engine_first.full_digest).not.toBe(input.engine_second.full_digest);
    expect(input.engine_first.identity_digest).toBe(input.engine_second.identity_digest);
    expect(plan.blockers.map((item) => item.blocker_code)).toEqual(expect.arrayContaining([
      "engine_account_owner_collision",
      "engine_account_missing_owner",
      "b2b_multiplier_source_mismatch",
    ]));
    expect(plan.blockers).not.toContainEqual(expect.objectContaining({
      blocker_code: "engine_inventory_changed_between_scans",
    }));
  });

  it("keeps the reviewed plan identity stable while preserving fresh moving-money evidence", () => {
    const firstInput = completePlanInput();
    const first = buildStage5V2Plan(firstInput);
    const secondInput = completePlanInput();
    secondInput.engine_first.accounts[0]!.balance_nano = "900";
    secondInput.engine_second.accounts[0]!.balance_nano = "950";
    secondInput.engine_first.full_digest = stage5V2EngineFullDigest(secondInput.engine_first.accounts);
    secondInput.engine_second.full_digest = stage5V2EngineFullDigest(secondInput.engine_second.accounts);
    const second = buildStage5V2Plan(secondInput);

    expect(second.plan_digest).toBe(first.plan_digest);
    expect(second.inventory_artifact).not.toEqual(first.inventory_artifact);
  });

  it("blocks commerce status drift instead of assigning an ambiguous owner state", () => {
    const input = completePlanInput();
    input.commerce.accounts[0]!.commerce_status = "pending";
    const plan = buildStage5V2Plan(input);

    expect(plan.blockers).toContainEqual(expect.objectContaining({
      blocker_code: "commerce_status_drift",
      subject_id: "acct_b2c",
    }));
  });
});

describe("pricing Stage 5 v2 exhaustive scanners", () => {
  it("normalizes OpenKeys transport and strict-contract failures without leaking credentials", async () => {
    const unavailable = createStage5OpenKeysInventoryReaderV2({
      baseUrl: "http://127.0.0.1:3410",
      controlKey: "secret-control-key",
      fetch: async () => { throw new Error("request contained secret-control-key"); },
    });
    await expect(unavailable.getPage({ limit: 500 })).rejects.toMatchObject({
      code: "openkeys_inventory_unavailable",
      message: "OpenKeys inventory request failed",
    });

    const malformed = createStage5OpenKeysInventoryReaderV2({
      baseUrl: "http://127.0.0.1:3410",
      controlKey: "secret-control-key",
      fetch: async () => new Response(JSON.stringify({ inventory: { accounts: [] } }), { status: 200 }),
    });
    await expect(malformed.getPage({ limit: 500 })).rejects.toMatchObject({
      code: "openkeys_inventory_malformed",
      message: "OpenKeys inventory response does not match the strict contract",
    });
  });

  it("exhausts the engine cursor and hashes stable identity separately from money", async () => {
    const pages = [
      {
        accounts: [engineAccount("acct_a")],
        next_after_account_id: "acct_a",
      },
      {
        accounts: [engineAccount("acct_b")],
        next_after_account_id: null,
      },
    ];
    const scan = await scanStage5EngineInventoryV2({
      getPricingReleaseInventoryV2: async () => pages.shift()!,
    } as never);
    expect(scan.accounts.map((account) => account.account_id)).toEqual(["acct_a", "acct_b"]);
    expect(scan.identity_digest).toMatch(/^sha256:v2:[0-9a-f]{64}$/);
  });

  it("requires one OpenKeys manifest digest across every page", async () => {
    const first = openkeysAccount("acct_a");
    const second = { ...openkeysAccount("acct_b"), source_id: "10000000-0000-4000-8000-000000000002" };
    const digest = stage5V2Digest("manifest", [first, second]);
    const pages = [
      { inventory_digest: digest, accounts: [first], next_after_account_id: "acct_a" },
      { inventory_digest: digest, accounts: [second], next_after_account_id: null },
    ];
    const scan = await scanStage5OpenKeysInventoryV2({ getPage: async () => pages.shift()! });
    expect(scan.inventory_digest).toBe(digest);
    expect(scan.accounts).toHaveLength(2);
  });

  it("rejects a manifest change before cursor exhaustion", async () => {
    const account = openkeysAccount("acct_a");
    const pages = [
      {
        inventory_digest: stage5V2Digest("manifest", 1),
        accounts: [account],
        next_after_account_id: "acct_a",
      },
      {
        inventory_digest: stage5V2Digest("manifest", 2),
        accounts: [],
        next_after_account_id: null,
      },
    ];
    await expect(scanStage5OpenKeysInventoryV2({ getPage: async () => pages.shift()! }))
      .rejects.toMatchObject({ code: "openkeys_manifest_changed_during_scan" });
  });
});
