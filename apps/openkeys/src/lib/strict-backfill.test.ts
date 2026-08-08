import {
  CURRENT_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  OPENKEYS_PRICING_PRODUCT_ID,
  type PricingCatalogSpec,
  type ProviderSwitchSpec,
} from "@claude-api/contracts";
import { describe, expect, it } from "vitest";
import {
  buildOfficialOpenKeysPolicy,
  EngineClientError,
  officialOpenKeysStrictBinding,
  type OpenKeysPricingAuthority,
} from "@claude-api/engine-client";
import { backfillOpenKeysAccount, type OpenKeysStrictBackfillEngine } from "./strict-backfill";

// The existing-OpenKeys-account strict backfill (release-v2 retirement, phase 2.2):
// preflight → official 1:1 strict policy → exact-CAS activation → key re-stamp → opt-out,
// idempotent, with service/meter-only and disabled accounts excluded before any mutation.

const ACCOUNT = "acct_openkeys_backfill";

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

const LEGACY_STATE = {
  active: {
    policy: { effective_version: 3, content_digest: "legacy-policy-digest" },
    binding: {
      policy_enforcement: "shadow",
      funding_enforcement: "legacy_single",
      reconciliation_state: "verified",
    },
  },
};

function fakeEngine(input: {
  accountClass?: string | null;
  accountStatus?: "active" | "disabled";
  alreadyOfficial?: boolean;
  optOut?: { result: "applied" } | { result: "rejected"; code: string };
  activationThrows?: Error;
  readbackMismatch?: boolean;
}) {
  const calls: string[] = [];
  const stamps: Array<{ keyId: string; ack: unknown }> = [];
  const auth = authority();
  const officialPolicy = buildOfficialOpenKeysPolicy(ACCOUNT, auth);
  const officialBinding = officialOpenKeysStrictBinding();
  const engine = {
    getAccount: async () => {
      calls.push("getAccount");
      return {
        account: ACCOUNT,
        balance_nano: "0",
        spent_nano: "0",
        reserved_nano: "0",
        balance: "$0.00",
        mult_bp: 10_000,
        status: input.accountStatus ?? "active",
        handle: null,
        funding: input.accountClass === null
          ? null
          : { account_class: input.accountClass ?? "openkeys" },
      };
    },
    getAccountPricingState: async () => {
      calls.push("getAccountPricingState");
      return LEGACY_STATE;
    },
    getActiveAccountPolicy: async () => {
      calls.push("getActiveAccountPolicy");
      if (input.alreadyOfficial) return { policy: officialPolicy, binding: officialBinding };
      if (input.readbackMismatch && calls.includes("activateAccountPolicy")) {
        return { policy: { ...officialPolicy, content_digest: "other-digest" }, binding: officialBinding };
      }
      return calls.includes("activateAccountPolicy") && !input.activationThrows
        ? { policy: officialPolicy, binding: officialBinding }
        : null;
    },
    getFundingNormalizationPlanV2: async () => {
      calls.push("getFundingNormalizationPlanV2");
      return null;
    },
    applyFundingNormalizationV2: async () => null,
    listKeys: async () => [
      { key_id: "key_active", key_masked: "sk-ok-…", label: "stock", status: "active", spent_nano: "0", spent: "$0.000000000" },
      { key_id: "key_disabled", key_masked: "sk-ok-…", label: null, status: "disabled", spent_nano: "0", spent: "$0.000000000" },
    ],
    setKeyStatus: async (keyId: string, _status: string, ack: unknown) => {
      calls.push(`setKeyStatus:${keyId}`);
      stamps.push({ keyId, ack });
    },
    prepareAccountPolicy: async () => {
      calls.push("prepareAccountPolicy");
      return { result: "stored", identity: {} };
    },
    activateAccountPolicy: async () => {
      calls.push("activateAccountPolicy");
      if (input.activationThrows) throw input.activationThrows;
      return { result: "applied", identity: {} };
    },
    optOutPricingReleaseV2: async () => {
      calls.push("optOutPricingReleaseV2");
      return input.optOut ?? { result: "applied", identity: {}, pricing_release_opt_out_ts: 1_700_000_000 };
    },
  };
  return {
    engine: engine as unknown as OpenKeysStrictBackfillEngine,
    calls,
    stamps,
    officialPolicy,
  };
}

describe("backfillOpenKeysAccount", () => {
  it("migrates a release-covered account: preflight, exact-CAS activation, key re-stamps, opt-out", async () => {
    const { engine, calls, stamps, officialPolicy } = fakeEngine({});
    const result = await backfillOpenKeysAccount(engine, authority(), ACCOUNT);
    expect(result).toEqual({ accountId: ACCOUNT, outcome: "opted_out" });

    const activationAt = calls.indexOf("activateAccountPolicy");
    expect(calls[0]).toBe("getAccount");
    expect(calls.indexOf("getFundingNormalizationPlanV2")).toBeLessThan(activationAt);
    expect(calls.indexOf("prepareAccountPolicy")).toBeLessThan(activationAt);
    // The shared preflight first stamps every active key with the CURRENT legacy policy ACK
    // (a key created before the flip must be admitted by the outgoing shadow head); then the
    // pre-flip re-stamp moves it to the NEW strict ACK (the migration-0016 trigger admits the
    // flip only when all active keys carry it), and the post-flip re-stamp converges
    // race-created keys. The disabled key is never touched.
    const officialAck = {
      effectivePolicyVersion: officialPolicy.effective_version,
      policyDigest: officialPolicy.content_digest,
    };
    expect(stamps).toEqual([
      { keyId: "key_active", ack: { effectivePolicyVersion: 3, policyDigest: "legacy-policy-digest" } },
      { keyId: "key_active", ack: officialAck },
      { keyId: "key_active", ack: officialAck },
    ]);
    expect(calls.indexOf("setKeyStatus:key_active")).toBeLessThan(activationAt);
    expect(calls.lastIndexOf("setKeyStatus:key_active")).toBeGreaterThan(activationAt);
    expect(calls.at(-1)).toBe("optOutPricingReleaseV2");
  });

  it("an account already on the official strict policy skips straight to the opt-out", async () => {
    const { engine, calls } = fakeEngine({ alreadyOfficial: true });
    const result = await backfillOpenKeysAccount(engine, authority(), ACCOUNT);
    expect(result).toEqual({ accountId: ACCOUNT, outcome: "opted_out" });
    expect(calls).not.toContain("prepareAccountPolicy");
    expect(calls).not.toContain("activateAccountPolicy");
    expect(calls).toContain("optOutPricingReleaseV2");
  });

  it("skips service/meter-only and unknown-class engine accounts before any mutation", async () => {
    for (const accountClass of ["service", "b2c", null] as const) {
      const { engine, calls } = fakeEngine({ accountClass });
      const result = await backfillOpenKeysAccount(engine, authority(), ACCOUNT);
      expect(result.outcome).toBe("skipped");
      expect(calls).not.toContain("prepareAccountPolicy");
      expect(calls).not.toContain("optOutPricingReleaseV2");
    }
  });

  it("skips a disabled engine account", async () => {
    const { engine, calls } = fakeEngine({ accountStatus: "disabled" });
    const result = await backfillOpenKeysAccount(engine, authority(), ACCOUNT);
    expect(result).toEqual({
      accountId: ACCOUNT,
      outcome: "skipped",
      detail: "engine account is disabled",
    });
    expect(calls).not.toContain("prepareAccountPolicy");
  });

  it("a rejected opt-out is a recorded per-account failure, never a forced write", async () => {
    const { engine } = fakeEngine({
      optOut: { result: "rejected", code: "missing_dependency" },
    });
    const result = await backfillOpenKeysAccount(engine, authority(), ACCOUNT);
    expect(result).toEqual({
      accountId: ACCOUNT,
      outcome: "failed",
      detail: "pricing release opt-out rejected with missing_dependency",
    });
  });

  it("a drain-blocked strict flip (engine 503) fails the account calmly for the next admin call", async () => {
    const { engine } = fakeEngine({
      activationThrows: new EngineClientError("billing authority unavailable", 503, true),
    });
    const result = await backfillOpenKeysAccount(engine, authority(), ACCOUNT);
    expect(result.outcome).toBe("failed");
    expect(result.detail).toContain("billing authority unavailable");
  });

  it("an activation that does not read back with the exact identity fails closed", async () => {
    const { engine, calls } = fakeEngine({ readbackMismatch: true });
    const result = await backfillOpenKeysAccount(engine, authority(), ACCOUNT);
    expect(result.outcome).toBe("failed");
    expect(result.detail).toContain("did not read back");
    expect(calls).not.toContain("optOutPricingReleaseV2");
  });
});
