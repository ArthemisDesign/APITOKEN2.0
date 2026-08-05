import { describe, expect, it, vi } from "vitest";
import type { FundingNormalizationPlanV2 } from "@claude-api/contracts";
import {
  AccountStrictCutoverPreflightError,
  ensureAccountStrictCutoverPreflight,
  type AccountStrictCutoverPreflightTransport,
} from "./strict-cutover-preflight.js";

const ACCOUNT = "acct_preflight";
const SOURCE_DIGEST = `sha256:v2:${"a".repeat(64)}`;
const NORMALIZATION_DIGEST = `sha256:v2:${"b".repeat(64)}`;

function plan(status: "ready" | "blocked" | "normalized"): FundingNormalizationPlanV2 {
  return {
    account_id: ACCOUNT,
    account_status: "active",
    status,
    source: "aggregate_paid_only",
    source_state_digest: SOURCE_DIGEST,
    normalization_digest: status === "blocked" ? null : NORMALIZATION_DIGEST,
    funding_generation: status === "normalized" ? 1 : null,
    funding_head_version: status === "normalized" ? 1 : null,
    balance_nano: "5000000000",
    reserved_nano: "0",
    spent_nano: "0",
    lots: [],
    blockers: status === "blocked"
      ? [{ code: "active_legacy_reservation", detail: "reservation r1 is open" }]
      : [],
  };
}

function pricingState(policyEnforcement: string) {
  return {
    active: {
      policy: { effective_version: 7, content_digest: "engine-digest-v7" },
      binding: {
        policy_enforcement: policyEnforcement,
        funding_enforcement: policyEnforcement === "strict" ? "strict" : "legacy_single",
        reconciliation_state: "verified",
      },
    },
  };
}

const KEYS = [
  { key_id: "key_active", key_masked: "sk-pool-act…ive", label: "prod", status: "active", spent_nano: "0", spent: "$0.000000000" },
  { key_id: "key_disabled", key_masked: "sk-pool-dis…ed", label: null, status: "disabled", spent_nano: "0", spent: "$0.000000000" },
];

function fakeTransport(input: {
  plan?: FundingNormalizationPlanV2 | null;
  applyResult?: unknown;
  state?: unknown;
}) {
  const calls: string[] = [];
  const keyStamps: Array<{ keyId: string; ack: unknown }> = [];
  const applyRequests: unknown[] = [];
  const engine = {
    getFundingNormalizationPlanV2: vi.fn(async () => {
      calls.push("plan");
      return input.plan === undefined ? plan("ready") : input.plan;
    }),
    applyFundingNormalizationV2: vi.fn(async (_account: string, request: unknown) => {
      calls.push("apply");
      applyRequests.push(request);
      return input.applyResult === undefined
        ? { status: "stored", normalization: plan("normalized") }
        : input.applyResult;
    }),
    getAccountPricingState: vi.fn(async () => {
      calls.push("state");
      return input.state === undefined ? pricingState("shadow") : input.state;
    }),
    listKeys: vi.fn(async () => {
      calls.push("keys");
      return KEYS;
    }),
    setKeyStatus: vi.fn(async (keyId: string, _status: string, ack: unknown) => {
      keyStamps.push({ keyId, ack });
    }),
  } as unknown as AccountStrictCutoverPreflightTransport;
  return { engine, calls, keyStamps, applyRequests };
}

describe("ensureAccountStrictCutoverPreflight", () => {
  it("normalizes funding and stamps only the active keys with the exact active-policy ACK", async () => {
    const fake = fakeTransport({});
    await expect(ensureAccountStrictCutoverPreflight(fake.engine, ACCOUNT)).resolves.toEqual({
      funding: "normalized",
      keysStamped: 1,
    });
    expect(fake.calls).toEqual(["plan", "apply", "state", "keys"]);
    expect(fake.applyRequests).toEqual([{
      expected_source_state_digest: SOURCE_DIGEST,
      expected_normalization_digest: NORMALIZATION_DIGEST,
    }]);
    expect(fake.keyStamps).toEqual([{
      keyId: "key_active",
      ack: { effectivePolicyVersion: 7, policyDigest: "engine-digest-v7" },
    }]);
  });

  it("reports nothing_to_normalize when the engine has no plan and never applies", async () => {
    const fake = fakeTransport({ plan: null });
    await expect(ensureAccountStrictCutoverPreflight(fake.engine, ACCOUNT)).resolves.toEqual({
      funding: "nothing_to_normalize",
      keysStamped: 1,
    });
    expect(fake.calls).toEqual(["plan", "state", "keys"]);
  });

  it("skips the apply when funding is already normalized", async () => {
    const fake = fakeTransport({ plan: plan("normalized") });
    await expect(ensureAccountStrictCutoverPreflight(fake.engine, ACCOUNT)).resolves.toEqual({
      funding: "already_normalized",
      keysStamped: 1,
    });
    expect(fake.calls).toEqual(["plan", "state", "keys"]);
  });

  it("fails loudly with the engine blockers when funding normalization is blocked", async () => {
    const fake = fakeTransport({ plan: plan("blocked") });
    const failure = await ensureAccountStrictCutoverPreflight(fake.engine, ACCOUNT).catch((error) => error);
    expect(failure).toBeInstanceOf(AccountStrictCutoverPreflightError);
    expect(failure.code).toBe("funding_blocked");
    expect(failure.message).toContain("active_legacy_reservation: reservation r1 is open");
    expect(fake.calls).toEqual(["plan"]);
  });

  it("fails when a ready plan carries no normalization target", async () => {
    const readyWithoutTarget = { ...plan("ready"), normalization_digest: null };
    const fake = fakeTransport({ plan: readyWithoutTarget });
    const failure = await ensureAccountStrictCutoverPreflight(fake.engine, ACCOUNT).catch((error) => error);
    expect(failure).toBeInstanceOf(AccountStrictCutoverPreflightError);
    expect(failure.code).toBe("funding_target_missing");
    expect(fake.calls).toEqual(["plan"]);
  });

  it("fails when the normalization target vanishes between plan and apply", async () => {
    const fake = fakeTransport({ applyResult: null });
    const failure = await ensureAccountStrictCutoverPreflight(fake.engine, ACCOUNT).catch((error) => error);
    expect(failure).toBeInstanceOf(AccountStrictCutoverPreflightError);
    expect(failure.code).toBe("funding_vanished");
    expect(fake.calls).toEqual(["plan", "apply"]);
  });

  it("fails when the account has no active engine policy to cut over", async () => {
    const fake = fakeTransport({ plan: null, state: "unbound" });
    const failure = await ensureAccountStrictCutoverPreflight(fake.engine, ACCOUNT).catch((error) => error);
    expect(failure).toBeInstanceOf(AccountStrictCutoverPreflightError);
    expect(failure.code).toBe("no_active_policy");
    expect(fake.calls).toEqual(["plan", "state"]);
  });

  it("never re-stamps keys on an already-strict account", async () => {
    const fake = fakeTransport({ plan: null, state: pricingState("strict") });
    await expect(ensureAccountStrictCutoverPreflight(fake.engine, ACCOUNT)).resolves.toEqual({
      funding: "nothing_to_normalize",
      keysStamped: 0,
    });
    expect(fake.calls).toEqual(["plan", "state"]);
  });
});
