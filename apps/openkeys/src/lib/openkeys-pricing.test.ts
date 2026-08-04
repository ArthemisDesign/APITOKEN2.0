import { readFileSync } from "node:fs";
import {
  CURRENT_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN2_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  OPENKEYS_PRICING_PRODUCT_ID,
  type AccountPolicyBinding,
  type AccountPolicySpec,
  type PricingCatalogSpec,
  type PricingReleasePolicyV2,
  type ProviderSwitchSpec,
} from "@claude-api/contracts";
import { describe, expect, it, vi } from "vitest";
import { EngineClientError } from "@claude-api/engine-client";
import {
  assertNoOpenKeysPricingOverride,
  assertOfficialEngineAccount,
  assertOpenKeysCatalog,
  buildOfficialOpenKeysPolicy,
  describeIssuanceBlock,
  OFFICIAL_ONE_TO_ONE_MULT_BP,
  OpenKeysPricingError,
  provisionOfficialOpenKeysCredential,
  resolveOpenKeysPricingAuthority,
  type OpenKeysPricingAuthority,
} from "./openkeys-pricing";

const officialPolicyFixture = JSON.parse(readFileSync(
  new URL("../../../../docs/commerce/fixtures/openkeys-official-policy-v1.json", import.meta.url),
  "utf8",
)) as {
  account_id: string;
  policy_id: string;
  owner_id: string;
  content_digest: string;
};

type PricingEngine = Parameters<typeof resolveOpenKeysPricingAuthority>[0];

function catalog(): PricingCatalogSpec {
  return {
    product_id: OPENKEYS_PRICING_PRODUCT_ID,
    generation: 1,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_CAPABILITY_DIGEST,
    content_digest: "catalog-openkeys-v1",
    entries: CURRENT_PRODUCT_CATALOG_ENTRIES.map((entry) => ({ ...entry })),
  };
}

function switches(): ProviderSwitchSpec {
  return {
    generation: 1,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_CAPABILITY_DIGEST,
    content_digest: "switches-v1",
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

function authority(): OpenKeysPricingAuthority {
  return { catalog: catalog(), switches: switches() };
}

function catalogGen2(): PricingCatalogSpec {
  return {
    product_id: OPENKEYS_PRICING_PRODUCT_ID,
    generation: 2,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_GEN2_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST,
    content_digest: "catalog-openkeys-v2",
    entries: MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES.map((entry) => ({ ...entry })),
  };
}

function switchesGen2(): ProviderSwitchSpec {
  return {
    generation: 2,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_GEN2_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST,
    content_digest: "switches-v2",
    entries: ["anthropic", "openai"].flatMap((providerId) => [
      { provider_id: providerId, scope: "master" as const, catalog_generation: null, enabled: true },
      {
        provider_id: providerId,
        scope: { product: { product_id: OPENKEYS_PRICING_PRODUCT_ID } },
        catalog_generation: 2,
        enabled: true,
      },
    ]),
  };
}

describe("OpenKeys official 1:1 pricing", () => {
  it("pins the complete reviewed Anthropic/OpenAI catalog and excludes Gemini", () => {
    const active = catalog();
    expect(() => assertOpenKeysCatalog(active)).not.toThrow();
    expect(active.entries.map((entry) => entry.provider_id)).not.toContain("gemini");
    expect(active.entries.map((entry) => entry.canonical_model_id)).toEqual([
      "claude-haiku-4-5",
      "claude-opus-4-7",
      "claude-opus-4-8",
      "claude-sonnet-4-6",
      "claude-sonnet-5",
      "gpt-5.4",
      "gpt-5.5",
      "gpt-5.6-luna",
      "gpt-5.6-sol",
      "gpt-5.6-terra",
    ]);

    const withGemini = {
      ...active,
      entries: [...active.entries, {
        provider_id: "gemini",
        canonical_model_id: "gemini-future",
        enabled: true,
      }],
    };
    expect(() => assertOpenKeysCatalog(withGemini)).toThrow("exact reviewed Anthropic/OpenAI catalog");
  });

  it("accepts the reviewed generation-2 catalog only with its exact pinned identity", () => {
    const gen2 = catalogGen2();
    expect(() => assertOpenKeysCatalog(gen2)).not.toThrow();
    expect(gen2.entries.map((entry) => entry.canonical_model_id)).toEqual([
      "claude-fable-5",
      "claude-haiku-4-5",
      "claude-opus-4-7",
      "claude-opus-4-8",
      "claude-opus-5",
      "claude-sonnet-4-6",
      "claude-sonnet-5",
      "gpt-5.4",
      "gpt-5.5",
      "gpt-5.6-luna",
      "gpt-5.6-sol",
      "gpt-5.6-terra",
    ]);

    // Generation-2 content under the generation-1 catalog identity is not the
    // reviewed catalog, and neither is a superset/subset of its entries.
    expect(() => assertOpenKeysCatalog({ ...gen2, generation: 1 }))
      .toThrow("exact reviewed Anthropic/OpenAI catalog");
    expect(() =>
      assertOpenKeysCatalog({
        ...gen2,
        entries: gen2.entries.filter((entry) => entry.canonical_model_id !== "claude-fable-5"),
      })
    ).toThrow("exact reviewed Anthropic/OpenAI catalog");
    expect(() =>
      assertOpenKeysCatalog({
        ...gen2,
        entries: gen2.entries.map((entry) =>
          entry.canonical_model_id === "claude-opus-5" ? { ...entry, enabled: false } : entry),
      })
    ).toThrow("exact reviewed Anthropic/OpenAI catalog");
    expect(() => assertOpenKeysCatalog({ ...gen2, capability_generation: 1 }))
      .toThrow("exact reviewed Anthropic/OpenAI catalog");
  });

  it("resolves the generation-2 authority only with matching generation-2 switches", async () => {
    const gen2Authority: OpenKeysPricingAuthority = { catalog: catalogGen2(), switches: switchesGen2() };
    const getActivePricingCatalog = vi.fn(async () => catalogGen2());
    const getActiveProviderSwitches = vi.fn(async (): Promise<ProviderSwitchSpec | null> => switchesGen2());
    const engine = { getActivePricingCatalog, getActiveProviderSwitches } as unknown as PricingEngine;
    await expect(resolveOpenKeysPricingAuthority(engine)).resolves.toEqual(gen2Authority);

    const policy = buildOfficialOpenKeysPolicy("acct_openkeys_gen2", gen2Authority);
    expect(policy).toMatchObject({ catalog_generation: 2, switch_generation: 2 });

    // The transition window (catalog 2 active, switches still 1) stays fail closed.
    getActiveProviderSwitches.mockResolvedValueOnce(switches());
    await expect(resolveOpenKeysPricingAuthority(engine)).rejects.toMatchObject({
      code: "switch_identity_mismatch",
    });
  });

  it("rejects multiplier, discount, and pricing-contract overrides at every caller boundary", () => {
    for (const field of [
      "multBp",
      "mult_bp",
      "multiplierBp",
      "discountBps",
      "discount_bps",
      "pricingContract",
      "pricing_contract",
    ]) {
      expect(() => assertNoOpenKeysPricingOverride({ [field]: 9_999 }), field)
        .toThrow("fixed at 1:1");
    }
    expect(() => assertNoOpenKeysPricingOverride({ faceValueNano: 50_000_000_000n })).not.toThrow();
    expect(() => assertOfficialEngineAccount({ account: "acct_ok", multBp: 9_999 }))
      .toThrow("fixed 1:1 multiplier");
    expect(() => assertOfficialEngineAccount({
      account: "acct_ok",
      multBp: OFFICIAL_ONE_TO_ONE_MULT_BP,
    })).not.toThrow();
  });

  it("uses api_type-independent zero-discount policy rules for both enabled providers", () => {
    const policy = buildOfficialOpenKeysPolicy("acct_openkeys_current", authority());
    expect(policy).toMatchObject({
      account_id: "acct_openkeys_current",
      owner_type: "open_keys",
      account_class: "open_keys",
      product_id: "openkeys",
      replacement_locked: false,
    });
    expect(policy.rules).toHaveLength(2);
    expect(policy.rules.map((rule) => rule.scope)).toEqual([
      { provider: { provider_id: "anthropic" } },
      { provider: { provider_id: "openai" } },
    ]);
    for (const rule of policy.rules) {
      expect(rule).toMatchObject({
        pricing_mode: "discount",
        rule_origin: "managed",
        discount_bps: 0,
        payable_multiplier_bp: 10_000,
        track_eligible: false,
        retention_eligible: false,
        commission_eligible: false,
      });
    }
  });

  it("matches the shared fixed official policy identity used by Stage 5", () => {
    const policy = buildOfficialOpenKeysPolicy(officialPolicyFixture.account_id, authority());
    expect(policy).toMatchObject(officialPolicyFixture);
  });

  it("fails closed until the exact active catalog and switches are available", async () => {
    const getActivePricingCatalog = vi.fn(async () => catalog());
    const getActiveProviderSwitches = vi.fn(async (): Promise<ProviderSwitchSpec | null> => switches());
    const engine = { getActivePricingCatalog, getActiveProviderSwitches } as unknown as PricingEngine;
    await expect(resolveOpenKeysPricingAuthority(engine)).resolves.toEqual(authority());

    getActiveProviderSwitches.mockResolvedValueOnce(null);
    await expect(resolveOpenKeysPricingAuthority(engine)).rejects.toMatchObject({
      code: "pricing_authority_missing",
    });
  });

  describe("describeIssuanceBlock", () => {
    it("передаёт код pricing-ошибки без утечки внутреннего сообщения", () => {
      const reason = describeIssuanceBlock(
        new OpenKeysPricingError("pricing_authority_missing", "internal catalog detail"),
      );
      expect(reason.code).toBe("pricing_authority_missing");
      expect(reason.message).toContain("authority");
      expect(reason.message).not.toContain("internal catalog detail");
    });

    it("сетевую/HTTP-ошибку движка отличает от неподтверждённого authority", () => {
      const reason = describeIssuanceBlock(
        new EngineClientError("engine request failed", undefined, true),
      );
      expect(reason.code).toBe("engine_unavailable");
      expect(reason.message).toContain("Движок недоступен");
    });

    it("прочие ошибки сворачивает в общий код без внутренностей", () => {
      const reason = describeIssuanceBlock(new Error("ENGINE_BASE_URL must be an absolute URL"));
      expect(reason.code).toBe("authority_check_failed");
      expect(reason.message).not.toContain("ENGINE_BASE_URL");
    });
  });

  it("durably ACKs policy before exact face-value credit and issues the secret last", async () => {
    const trace: string[] = [];
    let preparedPolicy: AccountPolicySpec | null = null;
    const binding: AccountPolicyBinding = {
      policy_enforcement: "shadow",
      funding_enforcement: "legacy_single",
      reconciliation_state: "pending",
    };
    const prepareAccountPolicy = vi.fn(async (policy: AccountPolicySpec) => {
      trace.push("prepare-policy");
      preparedPolicy = policy;
      return { result: "stored" as const, identity: { policy } };
    });
    const getAccountPricingState = vi.fn(async () => {
      trace.push("read-state");
      return "unbound" as const;
    });
    const activateAccountPolicy = vi.fn(async (policy: AccountPolicySpec) => {
      trace.push("activate-policy");
      return { result: "applied" as const, identity: { policy } };
    });
    const getActiveAccountPolicy = vi.fn(async () => {
      trace.push("readback-policy");
      return preparedPolicy === null ? null : { policy: preparedPolicy, binding };
    });
    const creditAccount = vi.fn(async (accountId: string, amountNano: bigint, reference: string) => {
      trace.push("credit");
      return { account: accountId, balance_nano: amountNano.toString(), balance: "$50", reference };
    });
    const issueKey = vi.fn(async (accountId: string) => {
      trace.push("issue-secret");
      return {
        key: "sk-pool-official-secret",
        key_id: "key_official",
        account: accountId,
        label: "openkeys",
        spend_limit_nano: null,
        expires_ts: null,
      };
    });
    const engine = {
      prepareAccountPolicy,
      getAccountPricingState,
      activateAccountPolicy,
      getActiveAccountPolicy,
      creditAccount,
      issueKey,
      getPricingReleaseProvisioningContextV2: vi.fn(async () => {
        trace.push("read-release-context");
        return null;
      }),
    } as unknown as PricingEngine;

    await expect(provisionOfficialOpenKeysCredential(engine, {
      accountId: "acct_openkeys_new",
      authority: authority(),
      releaseRequired: false,
      faceValueNano: 50_000_000_000n,
      creditReference: "openkeys:batch:0",
      keyLabel: "openkeys current",
      onCredited: async () => { trace.push("credit-journal"); },
    })).resolves.toMatchObject({ key: "sk-pool-official-secret" });

    expect(trace).toEqual([
      "prepare-policy",
      "read-state",
      "activate-policy",
      "readback-policy",
      "credit",
      "credit-journal",
      "read-release-context",
      "issue-secret",
    ]);
    expect(creditAccount).toHaveBeenCalledWith(
      "acct_openkeys_new",
      50_000_000_000n,
      "openkeys:batch:0",
    );
  });

  it("never credits or issues after a rejected policy ACK", async () => {
    const creditAccount = vi.fn();
    const issueKey = vi.fn();
    const engine = {
      prepareAccountPolicy: vi.fn(async (policy: AccountPolicySpec) => ({
        result: "rejected" as const,
        rejection: "locked" as const,
        identity: { policy },
      })),
      getAccountPricingState: vi.fn(),
      activateAccountPolicy: vi.fn(),
      getActiveAccountPolicy: vi.fn(),
      creditAccount,
      issueKey,
      getPricingReleaseProvisioningContextV2: vi.fn(async () => null),
    } as unknown as PricingEngine;

    await expect(provisionOfficialOpenKeysCredential(engine, {
      accountId: "acct_openkeys_rejected",
      authority: authority(),
      releaseRequired: false,
      faceValueNano: 1_000_000_000n,
      creditReference: "openkeys:batch:1",
      keyLabel: "openkeys rejected",
    })).rejects.toMatchObject({ code: "policy_ack_rejected" });
    expect(creditAccount).not.toHaveBeenCalled();
    expect(issueKey).not.toHaveBeenCalled();
  });

  it("credits safely but never issues a secret when post-cutover extension ACK is rejected", async () => {
    const digest = (seed: string): string => `sha256:v2:${seed.repeat(64)}`;
    const target = {
      generation: 10,
      release_kind: "target" as const,
      schema_version: 2 as const,
      capability_generation: 3,
      capability_digest: digest("a"),
      main_catalog_generation: 3,
      main_catalog_digest: digest("b"),
      openkeys_catalog_generation: 3,
      openkeys_catalog_digest: digest("c"),
      switch_generation: 3,
      switch_digest: digest("d"),
      inventory_digest: digest("e"),
      funding_manifest_digest: digest("f"),
      minimum_runtime_schema_version: 2,
      content_digest: digest("1"),
    };
    const recovery = {
      ...target,
      generation: 11,
      release_kind: "recovery" as const,
      content_digest: digest("2"),
    };
    const context = {
      head: {
        active_generation: 10,
        active_digest: target.content_digest,
        head_version: 1,
        updated_ts: 1,
      },
      activation: {
        activation_id: "1",
        activation_kind: "cutover" as const,
        evidence_digest: digest("3"),
        activated_ts: 1,
      },
      active_release: target,
      paired_recovery: {
        release: recovery,
        recovery_link: {
          target_generation: 10,
          target_digest: target.content_digest,
          recovery_generation: 11,
          recovery_digest: recovery.content_digest,
          link_digest: digest("4"),
        },
      },
    };
    const fullRelease = {
      ...target,
      policy_manifest_digest: digest("5"),
      assignment_manifest_digest: digest("6"),
      assignments: [],
    };
    const normalized = {
      account_id: "acct_openkeys_post_cutover",
      account_status: "active" as const,
      status: "normalized" as const,
      source: "stored_generation" as const,
      source_state_digest: digest("7"),
      normalization_digest: digest("8"),
      funding_generation: 7,
      funding_head_version: 1,
      balance_nano: "1000000000",
      reserved_nano: "0",
      spent_nano: "0",
      lots: [{
        lot_id: "fundv2_openkeys",
        source_type: "paid" as const,
        source_ref: "openkeys:test",
        balance_nano: "1000000000",
        reserved_nano: "0",
        spent_nano: "0",
        version: 1,
        status: "active" as const,
      }],
      blockers: [],
    };
    let preparedPolicy: PricingReleasePolicyV2 | null = null;
    const creditAccount = vi.fn(async () => ({
      account: "acct_openkeys_post_cutover",
      balance_nano: "1000000000",
      balance: "$1.000000000",
      reference: "openkeys:test",
    }));
    const issueKey = vi.fn();
    const engine = {
      creditAccount,
      issueKey,
      getPricingReleaseProvisioningContextV2: vi.fn(async () => context),
      getPricingReleaseV2: vi.fn(async () => fullRelease),
      getFundingNormalizationPlanV2: vi.fn(async () => normalized),
      applyFundingNormalizationV2: vi.fn(),
      preparePricingReleasePolicyV2: vi.fn(async (policy: PricingReleasePolicyV2) => {
        preparedPolicy = policy;
        return { result: "stored" as const, identity: {} } as never;
      }),
      getPricingReleasePolicyV2: vi.fn(async () => preparedPolicy),
      preparePricingReleaseAssignmentExtensionV2: vi.fn(async () => ({
        result: "rejected" as const,
        code: "invalid" as const,
        rejection: { invalid: { reason: "test rejection" } },
        identity: {},
      } as never)),
      getPricingReleaseAssignmentExtensionV2: vi.fn(),
    } as unknown as PricingEngine;

    await expect(provisionOfficialOpenKeysCredential(engine, {
      accountId: "acct_openkeys_post_cutover",
      authority: null,
      releaseRequired: true,
      faceValueNano: 1_000_000_000n,
      creditReference: "openkeys:test",
      keyLabel: "openkeys post-cutover",
    })).rejects.toMatchObject({ code: "assignment_conflict" });
    expect(creditAccount).toHaveBeenCalledOnce();
    expect(issueKey).not.toHaveBeenCalled();
  });
});
