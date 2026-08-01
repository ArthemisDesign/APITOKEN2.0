import {
  CURRENT_ANTHROPIC_CANONICAL_MODELS,
  CURRENT_OPENAI_CANONICAL_MODELS,
  CURRENT_PRODUCT_CATALOG_ENTRIES,
  MAIN_PRICING_PRODUCT_ID,
  MULTI_DISCOUNT_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN2_ANTHROPIC_CANONICAL_MODELS,
  MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN2_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  OPENKEYS_PRICING_PRODUCT_ID,
  pricingCatalogEntrySchema,
} from "@claude-api/contracts";
import { describe, expect, it } from "vitest";
import {
  buildCatalogGen2Plan,
  buildGen2CapabilityProjection,
  buildGen2Catalog,
  buildGen2Switches,
  GEN2_CAPABILITY_GENERATION,
  GEN2_CATALOG_GENERATION,
  GEN2_SWITCH_GENERATION,
} from "./multi-discount-catalog-gen2.js";
import { stage5Digest, STAGE5_CATALOG_MODELS } from "./multi-discount-backfill.js";

describe("frozen generation-1 multi-discount pins", () => {
  it("keeps the generation-1 constants byte-identical", () => {
    expect(MULTI_DISCOUNT_SCHEMA_VERSION).toBe(1);
    expect(MULTI_DISCOUNT_CAPABILITY_GENERATION).toBe(1);
    expect(MULTI_DISCOUNT_CAPABILITY_DIGEST).toBe(
      "sha256:v1:88da6b622727dda8aac0e1cd1749524f4929f7738f097c2dd3b81ba1cc14e7fd",
    );
    expect([...CURRENT_ANTHROPIC_CANONICAL_MODELS]).toEqual([
      "claude-haiku-4-5",
      "claude-opus-4-7",
      "claude-opus-4-8",
      "claude-sonnet-4-6",
      "claude-sonnet-5",
    ]);
    expect([...CURRENT_OPENAI_CANONICAL_MODELS]).toEqual([
      "gpt-5.4",
      "gpt-5.5",
      "gpt-5.6-luna",
      "gpt-5.6-sol",
      "gpt-5.6-terra",
    ]);
    expect(CURRENT_PRODUCT_CATALOG_ENTRIES.map((entry) => ({ ...entry }))).toEqual([
      ...CURRENT_ANTHROPIC_CANONICAL_MODELS.map((canonicalModelId) => ({
        provider_id: "anthropic",
        canonical_model_id: canonicalModelId,
        enabled: true,
      })),
      ...CURRENT_OPENAI_CANONICAL_MODELS.map((canonicalModelId) => ({
        provider_id: "openai",
        canonical_model_id: canonicalModelId,
        enabled: true,
      })),
    ]);
  });
});

describe("generation-2 catalog plan", () => {
  it("reproduces the pinned capability digest from the documented formula", () => {
    const capability = buildGen2CapabilityProjection();
    expect(capability.generation).toBe(2);
    expect(capability.generation).toBe(GEN2_CAPABILITY_GENERATION);
    expect(capability.generation).toBe(MULTI_DISCOUNT_GEN2_CAPABILITY_GENERATION);
    expect(capability.schema_version).toBe(1);
    expect(capability.content_digest).toBe(MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST);
    expect(capability.content_digest).toBe(
      "sha256:v1:9b23acd863d22abe2a6ed12096a4bb68a07b8d5c196351f1a15d38f11029bcd0",
    );
    expect(capability.aliases).toEqual([{
      provider_id: "openai",
      alias_model_id: "gpt-5.6",
      canonical_model_id: "gpt-5.6-sol",
    }]);
    expect(capability.entries).toHaveLength(12);
    // Generation-1 models keep byte-identical capability entry digests; only the
    // two new Anthropic models extend the set.
    const byModel = new Map(capability.entries.map((entry) => [entry.canonical_model_id, entry]));
    expect(byModel.get("claude-opus-4-8")?.entry_digest).toBe(
      "sha256:v1:a0e394f3ee0232ebf6ebf13ad4276f3f0aeebb90a06a163b8c0f92134a4803f3",
    );
    expect(byModel.get("gpt-5.6-sol")?.entry_digest).toBe(
      "sha256:v1:927ce44318df81468a4f57411fb9f26a05f100da62b655eb5c968c3dd6f58e07",
    );
    expect(byModel.get("claude-opus-5")?.entry_digest).toBe(
      "sha256:v1:6f31e376c306d45b625eb3bdfca395a42f8959def51b5ce09a864460399b3605",
    );
    expect(byModel.get("claude-fable-5")?.entry_digest).toBe(
      "sha256:v1:9dab9e5f7c8c72cb35b92989444b9672f0fbedcc57dccbfe595134ace00dabb9",
    );
  });

  it("extends generation 1 with exactly claude-opus-5 and claude-fable-5", () => {
    expect([...MULTI_DISCOUNT_GEN2_ANTHROPIC_CANONICAL_MODELS]).toEqual([
      "claude-fable-5",
      "claude-haiku-4-5",
      "claude-opus-4-7",
      "claude-opus-4-8",
      "claude-opus-5",
      "claude-sonnet-4-6",
      "claude-sonnet-5",
    ]);
    const gen1Keys = new Set(
      CURRENT_PRODUCT_CATALOG_ENTRIES.map((entry) => `${entry.provider_id} ${entry.canonical_model_id}`),
    );
    const gen2Keys = MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES.map(
      (entry) => `${entry.provider_id} ${entry.canonical_model_id}`,
    );
    expect(new Set(gen2Keys).size).toBe(12);
    for (const key of gen1Keys) expect(gen2Keys).toContain(key);
    expect(gen2Keys.filter((key) => !gen1Keys.has(key)).sort()).toEqual([
      "anthropic claude-fable-5",
      "anthropic claude-opus-5",
    ]);
    for (const entry of MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES) {
      expect(() => pricingCatalogEntrySchema.parse(entry)).not.toThrow();
      expect(entry.enabled).toBe(true);
    }
  });

  it("builds pinned generation-2 catalog specs for both products", () => {
    for (const [productId, contentDigest] of [
      [MAIN_PRICING_PRODUCT_ID, "sha256:v1:807fbe80c12a03e773e2f5067bc04a66b5a41e42d4bfdc8f85fe5656a5013616"],
      [OPENKEYS_PRICING_PRODUCT_ID, "sha256:v1:3b019fc3cfd619b5d4a81451aceafebf0c40de3b8c2cc150aa5b7a28b0102760"],
    ] as const) {
      const catalog = buildGen2Catalog(productId);
      expect(catalog).toMatchObject({
        product_id: productId,
        generation: GEN2_CATALOG_GENERATION,
        schema_version: 1,
        capability_generation: 2,
        capability_digest: MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST,
        content_digest: contentDigest,
      });
      expect(catalog.entries).toHaveLength(12);
      expect(catalog.entries.every((entry) => entry.enabled)).toBe(true);
    }
  });

  it("re-pins every scoped generation-2 switch to catalog generation 2", () => {
    const switches = buildGen2Switches();
    expect(switches).toMatchObject({
      generation: GEN2_SWITCH_GENERATION,
      schema_version: 1,
      capability_generation: 2,
      capability_digest: MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST,
      content_digest: "sha256:v1:ddbe078beec31d4f8b77e027ff3e9dad5477be6d10dafd4c99956abd9a74febd",
    });
    expect(switches.entries).toHaveLength(10);
    for (const entry of switches.entries) {
      expect(entry.enabled).toBe(true);
      if (entry.scope === "master") {
        expect(entry.catalog_generation).toBeNull();
      } else {
        expect(entry.catalog_generation).toBe(GEN2_CATALOG_GENERATION);
      }
    }
  });

  it("keeps the whole plan deterministic", () => {
    const left = buildCatalogGen2Plan();
    const right = buildCatalogGen2Plan();
    expect(left).toEqual(right);
    expect(left.plan_digest).toMatch(/^sha256:v1:[0-9a-f]{64}$/);
    expect(left.catalogs.map((catalog) => catalog.product_id)).toEqual(["main", "openkeys"]);
  });

  it("verifies the generation-1 foundation against Stage 5's own builder", () => {
    // The foundation check compares the durable generation-1 digests with the
    // exact Stage 5 identity, recomputed here through Stage 5's exported pieces.
    for (const productId of [MAIN_PRICING_PRODUCT_ID, OPENKEYS_PRICING_PRODUCT_ID]) {
      const stage5Gen1Digest = stage5Digest("catalog", {
        product_id: productId,
        generation: 1,
        schema_version: 1,
        capability_generation: MULTI_DISCOUNT_CAPABILITY_GENERATION,
        capability_digest: MULTI_DISCOUNT_CAPABILITY_DIGEST,
        entries: STAGE5_CATALOG_MODELS.map((entry) => ({ ...entry })),
      });
      expect(stage5Gen1Digest).toBe(
        productId === MAIN_PRICING_PRODUCT_ID
          ? "sha256:v1:8f8446d7ba49e9ccc3ac8211d607e3a1d4121995cd756931eea1e9a24cca5910"
          : "sha256:v1:0bb25e5a19c9a67284cee9b384bf47b1fd61225ae6a46190fc6965fd0c46d956",
      );
    }
  });
});
