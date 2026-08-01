import {
  CURRENT_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  OPENKEYS_PRICING_PRODUCT_ID,
  type AccountPolicyBinding,
  type AccountPolicySpec,
  type PricingCatalogSpec,
  type ProviderSwitchSpec,
} from "@claude-api/contracts";
import { describe, expect, it, vi } from "vitest";
import {
  assertNoOpenKeysPricingOverride,
  assertOfficialEngineAccount,
  assertOpenKeysCatalog,
  buildOfficialOpenKeysPolicy,
  OFFICIAL_ONE_TO_ONE_MULT_BP,
  provisionOfficialOpenKeysCredential,
  resolveOpenKeysPricingAuthority,
  type OpenKeysPricingAuthority,
} from "./openkeys-pricing";

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
    } as unknown as PricingEngine;

    await expect(provisionOfficialOpenKeysCredential(engine, {
      accountId: "acct_openkeys_new",
      authority: authority(),
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
  });
});
