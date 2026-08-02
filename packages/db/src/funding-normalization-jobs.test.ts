import { describe, expect, it } from "vitest";
import type { PricingReleaseInventoryAccountV2 } from "@claude-api/contracts";
import {
  buildFundingNormalizationCoverageV2,
  fundingNormalizationEngineInventoryDigestV2,
  fundingNormalizationServiceInventoryDigestV2,
  sameFundingNormalizationInventoryIdentityV2,
  type FundingNormalizationStateV2,
} from "./funding-normalization-jobs.js";

function account(
  accountId: string,
  overrides: Partial<PricingReleaseInventoryAccountV2> = {},
): PricingReleaseInventoryAccountV2 {
  return {
    account_id: accountId,
    status: "active",
    multiplier_bp: 10_000,
    balance_nano: "100",
    reserved_nano: "20",
    spent_nano: "30",
    funding_generation: null,
    funding_head_version: null,
    ...overrides,
  };
}

function state(inventory: readonly PricingReleaseInventoryAccountV2[]): FundingNormalizationStateV2 {
  const services = [{
    serviceId: "internal",
    engineAccountId: "acct_service",
    purpose: "internal automation",
    responsible: "platform",
    status: "active" as const,
    sourceVersion: 1n,
    contentDigest: `sha256:v2:${"a".repeat(64)}`,
  }];
  return {
    release: {
      generation: 1n,
      releaseDigest: "release",
      releaseKind: "target",
      status: "planned",
      engineInventoryDigest: fundingNormalizationEngineInventoryDigestV2(inventory),
      serviceInventoryDigest: fundingNormalizationServiceInventoryDigestV2(services),
      fundingManifestDigest: "funding",
    },
    services,
    queue: [],
  };
}

describe("funding normalization coverage", () => {
  it("ignores live money/head drift but detects identity drift", () => {
    const first = [account("acct_a")];
    const moneyChanged = [account("acct_a", {
      balance_nano: "999",
      reserved_nano: "111",
      spent_nano: "222",
      funding_generation: 7,
      funding_head_version: 9,
    })];
    expect(sameFundingNormalizationInventoryIdentityV2(first, moneyChanged)).toBe(true);
    expect(fundingNormalizationEngineInventoryDigestV2(first))
      .toBe(fundingNormalizationEngineInventoryDigestV2(moneyChanged));
    expect(sameFundingNormalizationInventoryIdentityV2(first, [
      account("acct_a", { multiplier_bp: 5000 }),
    ])).toBe(false);
  });

  it("excludes exact service inventory and reports missing balance accounts", () => {
    const inventory = [account("acct_customer"), account("acct_service")];
    const coverage = buildFundingNormalizationCoverageV2(inventory, state(inventory));
    expect(coverage.balanceAccountIds).toEqual(["acct_customer"]);
    expect(coverage.serviceAccountIds).toEqual(["acct_service"]);
    expect(coverage.missingAccountIds).toEqual(["acct_customer"]);
    expect(coverage.extraAccountIds).toEqual([]);
  });

  it("fails closed when the target inventory digest is stale", () => {
    const inventory = [account("acct_customer"), account("acct_service")];
    const stale = state(inventory);
    stale.release.engineInventoryDigest = fundingNormalizationEngineInventoryDigestV2([
      ...inventory,
      account("acct_new"),
    ]);
    expect(() => buildFundingNormalizationCoverageV2(inventory, stale))
      .toThrow(/no longer matches the target release plan/);
  });
});
