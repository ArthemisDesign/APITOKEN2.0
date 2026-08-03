import { accountPolicySpecSchema } from "@claude-api/contracts";
import { describe, expect, it } from "vitest";
import { buildStage5OpenKeysPlan } from "./multi-discount-backfill.js";
import {
  buildLegacyLockedOpenKeysPolicyV1,
  buildLockedOpenkeysSuccessorPolicyV1,
  pricingShadowPolicyAckDigestV2,
  pricingShadowRolloutSubjectDigestV2,
} from "./pricing-shadow-rollout-jobs-v2.js";

const SHA256_V2 = /^sha256:v2:[0-9a-f]{64}$/;

describe("shadow rollout locked OpenKeys derivation", () => {
  it("rebuilds the exact replacement-locked legacy policy identity", () => {
    const derived = buildLegacyLockedOpenKeysPolicyV1({
      accountId: "acct_ok_legacy",
      sourceId: "3f6b1f24-9f32-4a2f-9b6e-1a2b3c4d5e6f",
      multiplierBp: 7_000,
    });
    const authority = buildStage5OpenKeysPlan({
      source_id: "3f6b1f24-9f32-4a2f-9b6e-1a2b3c4d5e6f",
      account_id: "acct_ok_legacy",
      multiplier_bp: 7_000,
      status: "active",
      pricing_contract: "legacy",
    });
    expect(authority.effective_policy).not.toBeNull();
    expect(derived).toEqual(authority.effective_policy);
    expect(derived.replacement_locked).toBe(true);
    expect(derived.effective_version).toBe(1);
    expect(accountPolicySpecSchema.parse(derived)).toEqual(derived);
  });

  it("builds the only accepted managed 1:1 successor one version ahead", () => {
    const legacy = buildLegacyLockedOpenKeysPolicyV1({
      accountId: "acct_ok_legacy",
      sourceId: "3f6b1f24-9f32-4a2f-9b6e-1a2b3c4d5e6f",
      multiplierBp: 7_000,
    });
    const successor = buildLockedOpenkeysSuccessorPolicyV1({
      accountId: "acct_ok_legacy",
      sourceId: "3f6b1f24-9f32-4a2f-9b6e-1a2b3c4d5e6f",
      catalogGeneration: 5,
      switchGeneration: 5,
    });
    expect(accountPolicySpecSchema.parse(successor)).toEqual(successor);
    expect(successor.effective_version).toBe(legacy.effective_version + 1);
    expect(successor.policy_version).toBe(legacy.policy_version + 1);
    expect(successor.policy_id).toBe(legacy.policy_id);
    expect(successor.owner_type).toBe("open_keys");
    expect(successor.owner_id).toBe(legacy.owner_id);
    expect(successor.account_class).toBe("open_keys");
    expect(successor.product_id).toBe("openkeys");
    expect(successor.replacement_locked).toBe(false);
    expect(successor.schema_version).toBe(1);
    expect(successor.content_digest).toMatch(SHA256_V2);
    expect(successor.rules).toHaveLength(2);
    for (const rule of successor.rules) {
      expect(rule.rule_digest).toMatch(SHA256_V2);
      expect("provider" in rule.scope).toBe(true);
      expect(rule.pricing_mode).toBe("discount");
      expect(rule.rule_origin).toBe("managed");
      expect(rule.discount_bps).toBe(0);
      expect(rule.payable_multiplier_bp).toBe(10_000);
      expect(rule.track_eligible).toBe(false);
      expect(rule.retention_eligible).toBe(false);
      expect(rule.commission_eligible).toBe(false);
    }
    expect(new Set(successor.rules.map((rule) =>
      "provider" in rule.scope ? rule.scope.provider.provider_id : "")))
      .toEqual(new Set(["anthropic", "openai"]));
  });
});

describe("shadow rollout evidence digests", () => {
  it("computes deterministic ack and subject digests", () => {
    const ack = { result: "applied", identity: { policy: "opaque" } };
    expect(pricingShadowPolicyAckDigestV2(ack)).toBe(pricingShadowPolicyAckDigestV2({ ...ack }));
    expect(pricingShadowPolicyAckDigestV2(ack)).toMatch(SHA256_V2);
    expect(pricingShadowPolicyAckDigestV2({ result: "unchanged", identity: {} }))
      .not.toBe(pricingShadowPolicyAckDigestV2(ack));
    expect(pricingShadowRolloutSubjectDigestV2("acct_a")).toMatch(SHA256_V2);
    expect(pricingShadowRolloutSubjectDigestV2("acct_a"))
      .not.toBe(pricingShadowRolloutSubjectDigestV2("acct_b"));
  });
});
