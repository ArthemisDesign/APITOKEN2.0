import { accountPolicySpecSchema } from "@claude-api/contracts";
import { describe, expect, it } from "vitest";
import {
  buildLockedOpenkeysSuccessorPolicyV1,
  PricingShadowRolloutV2Error,
  pricingShadowPolicyAckDigestV2,
  pricingShadowRolloutSubjectDigestV2,
} from "./pricing-shadow-rollout-jobs-v2.js";

const SHA256_V2 = /^sha256:v2:[0-9a-f]{64}$/;

const legacySource = accountPolicySpecSchema.parse({
  account_id: "acct_ok_legacy",
  effective_version: 1,
  policy_id: "policy:openkeys:legacy:3f6b1f24-9f32-4a2f-9b6e-1a2b3c4d5e6f",
  policy_version: 1,
  source_policy_digest: "sha256:v1:legacy-source",
  owner_type: "open_keys",
  owner_id: "3f6b1f24-9f32-4a2f-9b6e-1a2b3c4d5e6f",
  account_class: "open_keys",
  product_id: "openkeys",
  schema_version: 1,
  catalog_generation: 1,
  switch_generation: 1,
  content_digest: "sha256:v1:legacy-content",
  replacement_locked: true,
  rules: [
    {
      rule_id: "provider:anthropic:legacy",
      rule_digest: "sha256:v1:rule-a",
      scope: { provider: { provider_id: "anthropic" } },
      pricing_mode: "discount",
      rule_origin: "legacy",
      discount_bps: null,
      payable_multiplier_bp: 7_000,
      track_eligible: false,
      retention_eligible: false,
      commission_eligible: false,
    },
  ],
});

describe("shadow rollout locked OpenKeys successor", () => {
  it("builds the only accepted managed 1:1 successor one version ahead of the live source", () => {
    const successor = buildLockedOpenkeysSuccessorPolicyV1({
      source: legacySource,
      catalogGeneration: 5,
      switchGeneration: 5,
    });
    expect(accountPolicySpecSchema.parse(successor)).toEqual(successor);
    expect(successor.effective_version).toBe(legacySource.effective_version + 1);
    expect(successor.policy_version).toBe(legacySource.policy_version + 1);
    expect(successor.policy_id).toBe(legacySource.policy_id);
    expect(successor.source_policy_digest).toBe(legacySource.content_digest);
    expect(successor.owner_type).toBe("open_keys");
    expect(successor.owner_id).toBe(legacySource.owner_id);
    expect(successor.account_class).toBe("open_keys");
    expect(successor.product_id).toBe("openkeys");
    expect(successor.catalog_generation).toBe(5);
    expect(successor.switch_generation).toBe(5);
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

describe("shadow rollout bounded error diagnostics", () => {
  it.each([
    ["legacy OpenKeys multiplier drifted for acct_private", "openkeys_multiplier_drift"],
    [
      "canonical OpenKeys account acct_private has no active engine policy lineage",
      "openkeys_lineage_missing",
    ],
    [
      "legacy OpenKeys account acct_private lineage lost its replacement lock without the canonical successor",
      "openkeys_lock_drift",
    ],
    ["canonical OpenKeys account acct_private lineage is unexpectedly locked", "openkeys_lock_drift"],
    ["unknown conflict for acct_private", "shadow_rollout_conflict"],
  ])("classifies %s without embedding the subject in the code", (message, code) => {
    const error = new PricingShadowRolloutV2Error(message, true);
    expect(error.code).toBe(code);
    expect(error.code).not.toContain("acct_private");
  });
});
