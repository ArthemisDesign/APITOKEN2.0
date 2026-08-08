import { describe, expect, it } from "vitest";
import type { PricingReleasePolicyV2 } from "@claude-api/contracts";
import {
  assertReleaseStrictEquivalence,
  PricingBackfillEquivalenceError,
  type PricingBackfillStrictRule,
} from "./pricing-backfill.js";

// The pre-opt-out equivalence proof of the release-v2 backfill (phase 2.2): pure scope-walk
// comparison between the release policy the account resolves under and the strict policy it
// is about to be opted into. B2C is the 5000-global identity; B2B is exact rule-set equality.

function strictRule(
  scopeType: "provider" | "model",
  providerId: string,
  payableMultiplierBp: number,
  canonicalModelId: string | null = null,
): PricingBackfillStrictRule {
  return { scopeType, providerId, canonicalModelId, payableMultiplierBp };
}

function releasePolicy(input: {
  accountClass: "b2c" | "b2b";
  rules: Array<{
    scope: "global" | "provider" | "model";
    providerId?: string;
    canonicalModelId?: string;
    payableMultiplierBp: number;
  }>;
  billingMode?: "balance" | "meter_only";
}): Pick<PricingReleasePolicyV2, "policy_id" | "policy_version" | "account_class" | "billing_mode" | "rules"> {
  return {
    policy_id: input.accountClass === "b2c" ? "release-v2:b2c:global" : "release-v2:b2b:acct_test",
    policy_version: 1,
    account_class: input.accountClass,
    billing_mode: input.billingMode ?? "balance",
    rules: input.rules.map((rule, index) => ({
      rule_id: `rule-${index}`,
      rule_digest: `sha256:v2:${"0".repeat(64)}`,
      scope: rule.scope === "global"
        ? { scope: "global" as const }
        : rule.scope === "provider"
          ? { scope: "provider" as const, provider_id: rule.providerId! }
          : {
              scope: "model" as const,
              provider_id: rule.providerId!,
              canonical_model_id: rule.canonicalModelId!,
            },
      discount_bps: 10_000 - rule.payableMultiplierBp,
      payable_multiplier_bp: rule.payableMultiplierBp,
    })),
  };
}

describe("assertReleaseStrictEquivalence (B2C 5000-global identity)", () => {
  it("passes when every strict rule is 5000 and release is the single global 5000", () => {
    expect(() => assertReleaseStrictEquivalence({
      accountClass: "b2c",
      strictFallbackBp: 5_000,
      strictRules: [
        strictRule("provider", "anthropic", 5_000),
        strictRule("provider", "openai", 5_000),
        strictRule("model", "anthropic", 5_000, "claude-opus-5"),
      ],
      releasePolicy: releasePolicy({
        accountClass: "b2c",
        rules: [{ scope: "global", payableMultiplierBp: 5_000 }],
      }),
    })).not.toThrow();
  });

  it("fails when a strict rule diverges from the release global", () => {
    expect(() => assertReleaseStrictEquivalence({
      accountClass: "b2c",
      strictFallbackBp: 5_000,
      strictRules: [strictRule("provider", "anthropic", 6_000)],
      releasePolicy: releasePolicy({
        accountClass: "b2c",
        rules: [{ scope: "global", payableMultiplierBp: 5_000 }],
      }),
    })).toThrowError(PricingBackfillEquivalenceError);
  });

  it("fails when the account effective multiplier is not the 5000 identity", () => {
    expect(() => assertReleaseStrictEquivalence({
      accountClass: "b2c",
      strictFallbackBp: 4_000,
      strictRules: [strictRule("provider", "anthropic", 5_000)],
      releasePolicy: releasePolicy({
        accountClass: "b2c",
        rules: [{ scope: "global", payableMultiplierBp: 5_000 }],
      }),
    })).toThrowError(/effective multiplier is 4000 bp/);
  });

  it("fails when the release global is not the 5000 identity", () => {
    expect(() => assertReleaseStrictEquivalence({
      accountClass: "b2c",
      strictFallbackBp: 5_000,
      strictRules: [strictRule("provider", "anthropic", 5_000)],
      releasePolicy: releasePolicy({
        accountClass: "b2c",
        rules: [{ scope: "global", payableMultiplierBp: 4_000 }],
      }),
    })).toThrowError(/global rule resolves to 4000/);
  });

  it("passes a release scoped override only when the strict side charges the same at that scope", () => {
    const release = releasePolicy({
      accountClass: "b2c",
      rules: [
        { scope: "global", payableMultiplierBp: 5_000 },
        { scope: "model", providerId: "anthropic", canonicalModelId: "claude-opus-5", payableMultiplierBp: 4_000 },
      ],
    });
    expect(() => assertReleaseStrictEquivalence({
      accountClass: "b2c",
      strictFallbackBp: 5_000,
      strictRules: [
        strictRule("provider", "anthropic", 5_000),
        strictRule("model", "anthropic", 4_000, "claude-opus-5"),
      ],
      releasePolicy: release,
    })).not.toThrow();
    // Strict side resolves the override scope at 5000 instead → mismatch.
    expect(() => assertReleaseStrictEquivalence({
      accountClass: "b2c",
      strictFallbackBp: 5_000,
      strictRules: [strictRule("provider", "anthropic", 5_000)],
      releasePolicy: release,
    })).toThrowError(/model:anthropic\/claude-opus-5: release resolves to 4000 bp, strict charges 5000 bp/);
  });

  it("fails when the release policy carries a different account class", () => {
    expect(() => assertReleaseStrictEquivalence({
      accountClass: "b2c",
      strictFallbackBp: 5_000,
      strictRules: [strictRule("provider", "anthropic", 5_000)],
      releasePolicy: {
        ...releasePolicy({ accountClass: "b2c", rules: [{ scope: "global", payableMultiplierBp: 5_000 }] }),
        account_class: "b2b",
      },
    })).toThrowError(/account_class b2b, expected b2c/);
  });

  it("fails a meter-only release policy (service accounts are never backfilled)", () => {
    expect(() => assertReleaseStrictEquivalence({
      accountClass: "b2c",
      strictFallbackBp: 5_000,
      strictRules: [strictRule("provider", "anthropic", 5_000)],
      releasePolicy: releasePolicy({
        accountClass: "b2c",
        billingMode: "meter_only",
        rules: [{ scope: "global", payableMultiplierBp: 5_000 }],
      }),
    })).toThrowError(/meter_only/);
  });
});

describe("assertReleaseStrictEquivalence (B2B scope-set equality)", () => {
  const release = releasePolicy({
    accountClass: "b2b",
    rules: [
      { scope: "provider", providerId: "anthropic", payableMultiplierBp: 6_000 },
      { scope: "model", providerId: "anthropic", canonicalModelId: "claude-opus-5", payableMultiplierBp: 4_000 },
      { scope: "provider", providerId: "openai", payableMultiplierBp: 7_500 },
    ],
  });

  it("passes when both rule sets carry the same scopes with the same payables", () => {
    expect(() => assertReleaseStrictEquivalence({
      accountClass: "b2b",
      strictFallbackBp: 10_000,
      strictRules: [
        strictRule("provider", "anthropic", 6_000),
        strictRule("model", "anthropic", 4_000, "claude-opus-5"),
        strictRule("provider", "openai", 7_500),
      ],
      releasePolicy: release,
    })).not.toThrow();
  });

  it("fails when a model scope diverges", () => {
    expect(() => assertReleaseStrictEquivalence({
      accountClass: "b2b",
      strictFallbackBp: 10_000,
      strictRules: [
        strictRule("provider", "anthropic", 6_000),
        strictRule("model", "anthropic", 5_000, "claude-opus-5"),
        strictRule("provider", "openai", 7_500),
      ],
      releasePolicy: release,
    })).toThrowError(/model:anthropic\/claude-opus-5: release resolves to 4000 bp, strict charges 5000 bp/);
  });

  it("fails when the strict side misses a provider scope the release covers", () => {
    expect(() => assertReleaseStrictEquivalence({
      accountClass: "b2b",
      strictFallbackBp: 10_000,
      strictRules: [
        strictRule("provider", "anthropic", 6_000),
        strictRule("model", "anthropic", 4_000, "claude-opus-5"),
      ],
      releasePolicy: release,
    })).toThrowError(/provider:openai: release resolves to 7500 bp, strict charges 10000 bp/);
  });

  it("fails when the strict side covers a scope the release does not", () => {
    expect(() => assertReleaseStrictEquivalence({
      accountClass: "b2b",
      strictFallbackBp: 10_000,
      strictRules: [
        strictRule("provider", "anthropic", 6_000),
        strictRule("model", "anthropic", 4_000, "claude-opus-5"),
        strictRule("provider", "openai", 7_500),
        strictRule("provider", "gemini", 5_000),
      ],
      releasePolicy: release,
    })).toThrowError(/does not cover provider:gemini/);
  });

  it("fails when a B2B release policy carries a global rule", () => {
    expect(() => assertReleaseStrictEquivalence({
      accountClass: "b2b",
      strictFallbackBp: 10_000,
      strictRules: [strictRule("provider", "anthropic", 6_000)],
      releasePolicy: releasePolicy({
        accountClass: "b2b",
        rules: [
          { scope: "global", payableMultiplierBp: 5_000 },
          { scope: "provider", providerId: "anthropic", payableMultiplierBp: 6_000 },
        ],
      }),
    })).toThrowError(/carries a global rule/);
  });
});
