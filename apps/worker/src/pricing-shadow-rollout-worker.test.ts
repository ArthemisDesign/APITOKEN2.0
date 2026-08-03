import assert from "node:assert/strict";
import test from "node:test";
import type { AccountPolicySpec } from "@claude-api/contracts";
import { EngineClientError } from "@claude-api/engine-client";
import type { ClaimedPricingShadowPolicyJobV2 } from "@claude-api/db";
import {
  deliverPricingShadowPolicyJobV2,
  PricingShadowRolloutDeliveryError,
  pricingShadowRolloutDisposition,
  type ShadowRolloutEngine,
} from "./pricing-shadow-rollout-worker.service.js";

function engine(mock: Record<string, unknown>): ShadowRolloutEngine {
  return mock as unknown as ShadowRolloutEngine;
}

const shadowBinding = {
  policy_enforcement: "shadow" as const,
  funding_enforcement: "legacy_single" as const,
  reconciliation_state: "verified" as const,
};

function policy(accountId: string, version = 1, digest = "sha256:target-policy"): AccountPolicySpec {
  return {
    account_id: accountId,
    effective_version: version,
    policy_id: "release-v2:global-b2c",
    policy_version: 2,
    source_policy_digest: "sha256:source",
    owner_type: "global_b2c",
    owner_id: "global-b2c",
    account_class: "b2c",
    product_id: "main",
    schema_version: 1,
    catalog_generation: 5,
    switch_generation: 5,
    content_digest: digest,
    replacement_locked: false,
    rules: [{
      rule_id: "global-b2c-50:provider:anthropic",
      rule_digest: "sha256:rule",
      scope: { provider: { provider_id: "anthropic" } },
      pricing_mode: "discount",
      rule_origin: "managed",
      discount_bps: 5_000,
      payable_multiplier_bp: 5_000,
      track_eligible: false,
      retention_eligible: false,
      commission_eligible: false,
    }],
  };
}

function shadowJob(overrides?: Partial<ClaimedPricingShadowPolicyJobV2>): ClaimedPricingShadowPolicyJobV2 {
  return {
    id: "job-1",
    rolloutId: "rollout-1",
    engineAccountId: "acct_b2c",
    accountClass: "b2c",
    ownerContext: "commerce",
    attempts: 1,
    requestDigest: "sha256:v2:request",
    payload: { kind: "policy_shadow", policy: policy("acct_b2c"), binding: shadowBinding },
    ...overrides,
  };
}

function lockedJob(): ClaimedPricingShadowPolicyJobV2 {
  const successor = policy("acct_ok_legacy", 2, "sha256:managed-policy");
  successor.owner_type = "open_keys";
  successor.account_class = "open_keys";
  successor.product_id = "openkeys";
  return {
    id: "job-locked",
    rolloutId: "rollout-1",
    engineAccountId: "acct_ok_legacy",
    accountClass: "openkeys",
    ownerContext: "openkeys",
    attempts: 1,
    requestDigest: "sha256:v2:request-locked",
    payload: {
      kind: "locked_openkeys_transition",
      policy: successor,
      expected_active: {
        target: { version: 1, content_digest: "sha256:legacy-policy" },
        binding: {
          policy_enforcement: "legacy_scalar",
          funding_enforcement: "legacy_single",
          reconciliation_state: "pending",
        },
      },
    },
  };
}

test("delivers an unbound generic shadow policy through prepare, readback and activate", async () => {
  const events: string[] = [];
  const target = policy("acct_b2c");
  const ack = await deliverPricingShadowPolicyJobV2(engine({
    getAccountPricingState: async () => {
      events.push("state");
      return "unbound";
    },
    getActiveAccountPolicy: async () => null,
    getAccountPolicyVersion: async () => {
      events.push("readback");
      return target;
    },
    prepareAccountPolicy: async () => {
      events.push("prepare");
      return { result: "stored", identity: { policy: target } };
    },
    activateAccountPolicy: async (_policy: unknown, _binding: unknown, expectation: unknown) => {
      events.push(`activate:${String(expectation)}`);
      return { result: "applied", identity: {} };
    },
    lockedOpenkeysPolicyTransition: async () => {
      throw new Error("must not call the locked transition for a generic job");
    },
  }), shadowJob());
  assert.equal(ack.result, "applied");
  assert.equal(ack.source, "engine_ack");
  assert.deepEqual(events, ["state", "prepare", "readback", "activate:unbound"]);
});

test("confirms an already exact generic shadow policy from readback without mutation", async () => {
  const target = policy("acct_b2c");
  const ack = await deliverPricingShadowPolicyJobV2(engine({
    getAccountPricingState: async () => ({
      active: { policy: target, binding: shadowBinding },
    }),
    getActiveAccountPolicy: async () => null,
    getAccountPolicyVersion: async () => null,
    prepareAccountPolicy: async () => {
      throw new Error("must not prepare an already exact policy");
    },
    activateAccountPolicy: async () => {
      throw new Error("must not activate an already exact policy");
    },
    lockedOpenkeysPolicyTransition: async () => {
      throw new Error("must not call the locked transition");
    },
  }), shadowJob());
  assert.equal(ack.result, "unchanged");
  assert.equal(ack.source, "engine_readback");
});

test("blocks when the engine holds a different policy under the target version", async () => {
  const conflict = policy("acct_b2c", 1, "sha256:other-content");
  const failure = await deliverPricingShadowPolicyJobV2(engine({
    getAccountPricingState: async () => ({
      active: { policy: conflict, binding: shadowBinding },
    }),
    getActiveAccountPolicy: async () => null,
    getAccountPolicyVersion: async () => null,
    prepareAccountPolicy: async () => {
      throw new Error("must not prepare after a version conflict");
    },
    activateAccountPolicy: async () => {
      throw new Error("must not activate after a version conflict");
    },
    lockedOpenkeysPolicyTransition: async () => {
      throw new Error("must not call the locked transition");
    },
  }), shadowJob()).catch((error) => error);
  assert.ok(failure instanceof PricingShadowRolloutDeliveryError);
  assert.equal(failure.disposition, "blocked");
});

test("delivers the locked OpenKeys transition only after the exact legacy readback", async () => {
  const events: string[] = [];
  const job = lockedJob();
  const payload = job.payload as Extract<typeof job.payload, { kind: "locked_openkeys_transition" }>;
  const legacy = policy("acct_ok_legacy", 1, "sha256:legacy-policy");
  legacy.replacement_locked = true;
  const ack = await deliverPricingShadowPolicyJobV2(engine({
    getAccountPricingState: async () => "unbound",
    getActiveAccountPolicy: async () => {
      events.push("active");
      return { policy: legacy, binding: payload.expected_active.binding };
    },
    getAccountPolicyVersion: async () => null,
    prepareAccountPolicy: async () => {
      throw new Error("generic prepare is forbidden for locked OpenKeys");
    },
    activateAccountPolicy: async () => {
      throw new Error("generic activate is forbidden for locked OpenKeys");
    },
    lockedOpenkeysPolicyTransition: async (accountId: string, request: { policy: AccountPolicySpec; expected_active: { target: { version: number } } }) => {
      events.push(`transition:${accountId}`);
      assert.equal(request.policy.effective_version, 2);
      assert.equal(request.expected_active.target.version, 1);
      return { result: "applied", identity: {} };
    },
  }), job);
  assert.equal(ack.result, "applied");
  assert.deepEqual(events, ["active", "transition:acct_ok_legacy"]);
});

test("blocks the locked transition on expectation drift and typed rejections", async () => {
  const job = lockedJob();
  const drifted = policy("acct_ok_legacy", 1, "sha256:other-legacy");
  drifted.replacement_locked = true;
  const drift = await deliverPricingShadowPolicyJobV2(engine({
    getAccountPricingState: async () => "unbound",
    getActiveAccountPolicy: async () => ({
      policy: drifted,
      binding: {
        policy_enforcement: "legacy_scalar" as const,
        funding_enforcement: "legacy_single" as const,
        reconciliation_state: "pending" as const,
      },
    }),
    getAccountPolicyVersion: async () => null,
    prepareAccountPolicy: async () => {
      throw new Error("forbidden");
    },
    activateAccountPolicy: async () => {
      throw new Error("forbidden");
    },
    lockedOpenkeysPolicyTransition: async () => {
      throw new Error("must not transition after drift");
    },
  }), job).catch((error) => error);
  assert.ok(drift instanceof PricingShadowRolloutDeliveryError);
  assert.equal(drift.disposition, "blocked");

  const payload = job.payload as Extract<typeof job.payload, { kind: "locked_openkeys_transition" }>;
  const legacy = policy("acct_ok_legacy", 1, "sha256:legacy-policy");
  legacy.replacement_locked = true;
  const rejected = await deliverPricingShadowPolicyJobV2(engine({
    getAccountPricingState: async () => "unbound",
    getActiveAccountPolicy: async () => ({ policy: legacy, binding: payload.expected_active.binding }),
    getAccountPolicyVersion: async () => null,
    prepareAccountPolicy: async () => {
      throw new Error("forbidden");
    },
    activateAccountPolicy: async () => {
      throw new Error("forbidden");
    },
    lockedOpenkeysPolicyTransition: async () => ({
      result: "rejected" as const,
      code: "policy_cas_mismatch" as const,
      identity: {},
      rejection: { policy_cas_mismatch: { actual: "unbound" as const } },
    }),
  }), job).catch((error) => error);
  assert.ok(rejected instanceof PricingShadowRolloutDeliveryError);
  assert.equal(rejected.disposition, "blocked");
  assert.match(rejected.message, /policy_cas_mismatch/);
});

test("classifies transient transport as retry and protocol failures as blocked", () => {
  assert.equal(pricingShadowRolloutDisposition(new EngineClientError("timeout", undefined, true)), "retry");
  assert.equal(pricingShadowRolloutDisposition(new EngineClientError("malformed", 200, false)), "blocked");
  assert.equal(pricingShadowRolloutDisposition(new Error("unexpected")), "retry");
  assert.equal(
    pricingShadowRolloutDisposition(new PricingShadowRolloutDeliveryError("typed", "blocked")),
    "blocked",
  );
});
