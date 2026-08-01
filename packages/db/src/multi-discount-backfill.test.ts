import { readFileSync } from "node:fs";
import {
  CURRENT_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  OPENKEYS_PRICING_PRODUCT_ID,
  type PricingCatalogSpec,
  type ProviderSwitchSpec,
} from "@claude-api/contracts";
import { buildOfficialOpenKeysPolicy } from "@claude-api/engine-client";
import { describe, expect, it } from "vitest";
import { buildStage5OpenKeysPlan } from "./multi-discount-backfill.js";

interface OfficialPolicyFixture {
  schema_version: number;
  account_id: string;
  policy_id: string;
  owner_id: string;
  catalog_generation: number;
  switch_generation: number;
  content_digest: string;
}

const fixture = JSON.parse(readFileSync(
  new URL("../../../docs/commerce/fixtures/openkeys-official-policy-v1.json", import.meta.url),
  "utf8",
)) as OfficialPolicyFixture;

function authority(): { catalog: PricingCatalogSpec; switches: ProviderSwitchSpec } {
  const catalog: PricingCatalogSpec = {
    product_id: OPENKEYS_PRICING_PRODUCT_ID,
    generation: fixture.catalog_generation,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_CAPABILITY_DIGEST,
    content_digest: "stage5-fixture-catalog",
    entries: CURRENT_PRODUCT_CATALOG_ENTRIES.map((entry) => ({ ...entry })),
  };
  return {
    catalog,
    switches: {
      generation: fixture.switch_generation,
      schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
      capability_generation: MULTI_DISCOUNT_CAPABILITY_GENERATION,
      capability_digest: MULTI_DISCOUNT_CAPABILITY_DIGEST,
      content_digest: "stage5-fixture-switches",
      entries: ["anthropic", "openai"].flatMap((providerId) => [
        { provider_id: providerId, scope: "master" as const, catalog_generation: null, enabled: true },
        {
          provider_id: providerId,
          scope: { product: { product_id: OPENKEYS_PRICING_PRODUCT_ID } },
          catalog_generation: fixture.catalog_generation,
          enabled: true,
        },
      ]),
    },
  };
}

describe("Stage 5 OpenKeys policy projection", () => {
  it("uses the shared Stage 7 official identity and fixed production digest", () => {
    const stage5 = buildStage5OpenKeysPlan({
      source_id: "318f5e77-d173-4b55-845a-fcd6542677ef",
      account_id: fixture.account_id,
      multiplier_bp: 10_000,
      status: "active",
      pricing_contract: "official_1_to_1",
    });
    const canonical = buildOfficialOpenKeysPolicy(fixture.account_id, authority());

    expect(stage5.effective_policy).toEqual(canonical);
    expect(stage5.effective_policy).toMatchObject({
      policy_id: fixture.policy_id,
      owner_id: fixture.owner_id,
      schema_version: fixture.schema_version,
      content_digest: fixture.content_digest,
    });
  });

  it("keeps legacy identities source-specific, economically exact, and replacement-locked", () => {
    const left = buildStage5OpenKeysPlan({
      source_id: "legacy-left",
      account_id: "acct_stage5_legacy_left",
      multiplier_bp: 7_000,
      status: "active",
      pricing_contract: "legacy",
    });
    const right = buildStage5OpenKeysPlan({
      source_id: "legacy-right",
      account_id: "acct_stage5_legacy_right",
      multiplier_bp: 7_000,
      status: "disabled",
      pricing_contract: "legacy",
    });

    expect(left.effective_policy).toMatchObject({
      policy_id: "policy:openkeys:legacy:legacy-left",
      owner_id: "legacy-left",
      replacement_locked: true,
      rules: [
        { rule_origin: "legacy", payable_multiplier_bp: 7_000 },
        { rule_origin: "legacy", payable_multiplier_bp: 7_000 },
      ],
    });
    expect(right.effective_policy).toMatchObject({
      policy_id: "policy:openkeys:legacy:legacy-right",
      owner_id: "legacy-right",
      replacement_locked: true,
    });
    expect(left.effective_policy?.content_digest).not.toBe(right.effective_policy?.content_digest);
  });
});
