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
        },
        {
          user_id: "20000000-0000-4000-8000-000000000002",
          engine_account_record_id: "30000000-0000-4000-8000-000000000002",
          engine_account_id: "acct_b2b",
          account_class: "b2b" as const,
          profile_multiplier_bp: 7_000,
          commerce_multiplier_bp: 7_000,
          commerce_status: "active" as const,
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
  it("pins Gemini only in main while keeping OpenKeys explicit", () => {
    const { catalogs, switches } = buildStage5V2CatalogsAndSwitches();
    const googleEntries = catalogs[0].entries.filter((entry) => entry.provider_id === "google");
    expect(googleEntries).toHaveLength(8);
    expect(googleEntries).not.toContainEqual(expect.objectContaining({
      canonical_model_id: "gemini-3-flash-preview",
    }));
    expect(catalogs[1].entries.some((entry) => entry.provider_id === "google")).toBe(false);
    expect(switches.entries).toContainEqual({
      provider_id: "google",
      scope: { product: { product_id: "main" } },
      catalog_generation: 3,
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
