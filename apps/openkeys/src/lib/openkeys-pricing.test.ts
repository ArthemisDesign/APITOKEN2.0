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
  describeIssuanceBlock,
  OFFICIAL_ONE_TO_ONE_MULT_BP,
  OpenKeysPricingError,
  provisionOfficialOpenKeysCredential,
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

type PricingEngine = Parameters<typeof provisionOfficialOpenKeysCredential>[0];

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



});
