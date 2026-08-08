import { readFileSync } from "node:fs";
import {
  CURRENT_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN2_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN5_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN5_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN5_OPENKEYS_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN6_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN6_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN6_OPENKEYS_CATALOG_ENTRIES,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  OPENKEYS_PRICING_PRODUCT_ID,
  type AccountPolicyBinding,
  type AccountPolicySpec,
  type PricingCatalogSpec,
  type ProviderSwitchSpec,
} from "@claude-api/contracts";
import { describe, expect, it, vi } from "vitest";
import { EngineClientError } from "@claude-api/engine-client";
import {
  assertNoOpenKeysPricingOverride,
  assertOfficialEngineAccount,
  assertOpenKeysCatalog,
  assertOpenKeysSwitches,
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

function catalogGen5(): PricingCatalogSpec {
  return {
    product_id: OPENKEYS_PRICING_PRODUCT_ID,
    generation: 5,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_GEN5_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_GEN5_CAPABILITY_DIGEST,
    content_digest: "catalog-openkeys-v5",
    entries: MULTI_DISCOUNT_GEN5_OPENKEYS_CATALOG_ENTRIES.map((entry) => ({ ...entry })),
  };
}

function switchesGen5(): ProviderSwitchSpec {
  return {
    generation: 5,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_GEN5_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_GEN5_CAPABILITY_DIGEST,
    content_digest: "switches-v5",
    entries: ["anthropic", "openai"].flatMap((providerId) => [
      { provider_id: providerId, scope: "master" as const, catalog_generation: null, enabled: true },
      {
        provider_id: providerId,
        scope: { product: { product_id: OPENKEYS_PRICING_PRODUCT_ID } },
        catalog_generation: 5,
        enabled: true,
      },
    ]),
  };
}

function catalogGen6(): PricingCatalogSpec {
  return {
    product_id: OPENKEYS_PRICING_PRODUCT_ID,
    generation: 6,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_GEN6_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_GEN6_CAPABILITY_DIGEST,
    content_digest: "catalog-openkeys-v6",
    entries: MULTI_DISCOUNT_GEN6_OPENKEYS_CATALOG_ENTRIES.map((entry) => ({ ...entry })),
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

  it("accepts the reviewed generation-5 catalog only with its exact pinned identity", () => {
    const gen5 = catalogGen5();
    expect(() => assertOpenKeysCatalog(gen5)).not.toThrow();
    expect(gen5.entries.map((entry) => entry.provider_id)).not.toContain("gemini");
    expect(gen5.entries.map((entry) => entry.canonical_model_id)).toEqual([
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

    // Generation-5 content under the generation-2 catalog identity is not the
    // reviewed catalog, and neither is a capability mismatch.
    expect(() => assertOpenKeysCatalog({ ...gen5, generation: 2 }))
      .toThrow("exact reviewed Anthropic/OpenAI catalog");
    expect(() =>
      assertOpenKeysCatalog({
        ...gen5,
        capability_generation: MULTI_DISCOUNT_GEN2_CAPABILITY_GENERATION,
        capability_digest: MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST,
      })
    ).toThrow("exact reviewed Anthropic/OpenAI catalog");
    expect(() =>
      assertOpenKeysCatalog({
        ...gen5,
        entries: gen5.entries.filter((entry) => entry.canonical_model_id !== "claude-fable-5"),
      })
    ).toThrow("exact reviewed Anthropic/OpenAI catalog");
  });

  it("accepts generation 6 only with the exact GPT Image 2 addition", () => {
    const gen6 = catalogGen6();
    expect(() => assertOpenKeysCatalog(gen6)).not.toThrow();
    expect(gen6.entries.map((entry) => entry.canonical_model_id)).toEqual([
      ...catalogGen5().entries.map((entry) => entry.canonical_model_id),
      "gpt-image-2-2026-04-21",
    ]);
    expect(() => assertOpenKeysCatalog({
      ...gen6,
      entries: gen6.entries.filter((entry) => entry.canonical_model_id !== "gpt-image-2-2026-04-21"),
    })).toThrow("exact reviewed Anthropic/OpenAI catalog");
    expect(() => assertOpenKeysCatalog({
      ...gen6,
      capability_digest: MULTI_DISCOUNT_GEN5_CAPABILITY_DIGEST,
    })).toThrow("exact reviewed Anthropic/OpenAI catalog");
  });

  it("resolves the generation-5 authority only with matching generation-5 switches", async () => {
    const gen5Authority: OpenKeysPricingAuthority = { catalog: catalogGen5(), switches: switchesGen5() };
    const getActivePricingCatalog = vi.fn(async () => catalogGen5());
    const getActiveProviderSwitches = vi.fn(async (): Promise<ProviderSwitchSpec | null> => switchesGen5());
    const engine = { getActivePricingCatalog, getActiveProviderSwitches } as unknown as PricingEngine;
    await expect(resolveOpenKeysPricingAuthority(engine)).resolves.toEqual(gen5Authority);

    const policy = buildOfficialOpenKeysPolicy("acct_openkeys_gen5", gen5Authority);
    expect(policy).toMatchObject({ catalog_generation: 5, switch_generation: 5 });

    // A product switch pinned to another catalog generation stays fail closed.
    const driftedSwitches: ProviderSwitchSpec = {
      ...switchesGen5(),
      entries: switchesGen5().entries.map((entry) =>
        typeof entry.scope === "object" ? { ...entry, catalog_generation: 2 } : entry),
    };
    expect(() => assertOpenKeysSwitches(driftedSwitches, catalogGen5()))
      .toThrow("only enabled Anthropic and OpenAI product switches");
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

  it("activates strict, credits, issues the ACKed key, and opts out before returning the secret", async () => {
    const trace: string[] = [];
    let preparedPolicy: AccountPolicySpec | null = null;
    const binding: AccountPolicyBinding = {
      policy_enforcement: "strict",
      funding_enforcement: "strict",
      reconciliation_state: "verified",
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
    const optOutPricingReleaseV2 = vi.fn(async () => {
      trace.push("opt-out");
      return { result: "applied" as const, identity: {}, pricing_release_opt_out_ts: 1_700_000_000 };
    });
    const engine = {
      prepareAccountPolicy,
      getAccountPricingState,
      activateAccountPolicy,
      getActiveAccountPolicy,
      creditAccount,
      issueKey,
      optOutPricingReleaseV2,
    } as unknown as PricingEngine;

    await expect(provisionOfficialOpenKeysCredential(engine, {
      accountId: "acct_openkeys_new",
      authority: authority(),
      faceValueNano: 50_000_000_000n,
      creditReference: "openkeys:batch:0",
      keyLabel: "openkeys current",
      onCredited: async () => { trace.push("credit-journal"); },
    })).resolves.toMatchObject({ key: "sk-pool-official-secret" });

    // Strict activation → credit → ACKed key → opt-out: the account is servable (the opt-out
    // guard sees the strict binding and the ACKed key) before the secret is ever returned.
    expect(trace).toEqual([
      "prepare-policy",
      "read-state",
      "activate-policy",
      "readback-policy",
      "credit",
      "credit-journal",
      "issue-secret",
      "opt-out",
    ]);
    expect(creditAccount).toHaveBeenCalledWith(
      "acct_openkeys_new",
      50_000_000_000n,
      "openkeys:batch:0",
    );
    expect(issueKey).toHaveBeenCalledWith("acct_openkeys_new", {
      label: "openkeys current",
      activationPolicyAck: {
        effectivePolicyVersion: 1,
        policyDigest: preparedPolicy!.content_digest,
      },
    });
    expect(optOutPricingReleaseV2).toHaveBeenCalledWith({
      accountId: "acct_openkeys_new",
      createdBy: "openkeys",
      reason: "new OpenKeys issuance on the direct strict path",
    });
  });

  it("never credits or issues after a rejected policy ACK", async () => {
    const creditAccount = vi.fn();
    const issueKey = vi.fn();
    const optOutPricingReleaseV2 = vi.fn();
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
      optOutPricingReleaseV2,
    } as unknown as PricingEngine;

    await expect(provisionOfficialOpenKeysCredential(engine, {
      accountId: "acct_openkeys_rejected",
      authority: authority(),
      faceValueNano: 1_000_000_000n,
      creditReference: "openkeys:batch:1",
      keyLabel: "openkeys rejected",
    })).rejects.toMatchObject({ code: "policy_ack_rejected" });
    expect(creditAccount).not.toHaveBeenCalled();
    expect(issueKey).not.toHaveBeenCalled();
    expect(optOutPricingReleaseV2).not.toHaveBeenCalled();
  });

  it("fails the issuance loudly when the engine rejects the opt-out marker", async () => {
    const optOutPricingReleaseV2 = vi.fn(async () => ({
      result: "rejected" as const,
      code: "missing_dependency" as const,
      identity: {},
      rejection: { missing_dependency: { dependency: "active_strict_policy_binding" } },
    }));
    let preparedPolicy: AccountPolicySpec | null = null;
    const binding: AccountPolicyBinding = {
      policy_enforcement: "strict",
      funding_enforcement: "strict",
      reconciliation_state: "verified",
    };
    const engine = {
      prepareAccountPolicy: vi.fn(async (policy: AccountPolicySpec) => {
        preparedPolicy = policy;
        return { result: "stored" as const, identity: { policy } };
      }),
      getAccountPricingState: vi.fn(async () => "unbound" as const),
      activateAccountPolicy: vi.fn(async (policy: AccountPolicySpec) => ({
        result: "applied" as const,
        identity: { policy },
      })),
      getActiveAccountPolicy: vi.fn(async () =>
        preparedPolicy === null ? null : { policy: preparedPolicy, binding }),
      creditAccount: vi.fn(async (accountId: string) => ({
        account: accountId,
        balance_nano: "1000000000",
        balance: "$1.000000000",
        reference: "openkeys:test",
      })),
      issueKey: vi.fn(async (accountId: string) => ({
        key: "sk-pool-never-returned",
        key_id: "key_never_returned",
        account: accountId,
        label: "openkeys",
        spend_limit_nano: null,
        expires_ts: null,
      })),
      optOutPricingReleaseV2,
    } as unknown as PricingEngine;

    // The throw aborts the issuance job: its compensation disables the half-provisioned account
    // instead of handing out a secret the engine refuses to serve.
    await expect(provisionOfficialOpenKeysCredential(engine, {
      accountId: "acct_openkeys_guarded",
      authority: authority(),
      faceValueNano: 1_000_000_000n,
      creditReference: "openkeys:test",
      keyLabel: "openkeys guarded",
    })).rejects.toMatchObject({ code: "pricing_opt_out_rejected" });
    expect(optOutPricingReleaseV2).toHaveBeenCalledOnce();
  });
});
