import {
  CURRENT_GEMINI_CANONICAL_MODELS,
  MULTI_DISCOUNT_GEN3_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN3_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN3_GEMINI_CANONICAL_MODELS,
  MULTI_DISCOUNT_GEN3_MAIN_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN5_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN5_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN5_GEMINI_CANONICAL_MODELS,
  MULTI_DISCOUNT_GEN5_MAIN_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN5_OPENKEYS_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  MULTI_DISCOUNT_TARGET_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_TARGET_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_TARGET_MAIN_CATALOG_ENTRIES,
  MULTI_DISCOUNT_TARGET_OPENKEYS_CATALOG_ENTRIES,
} from "@claude-api/contracts";
import { describe, expect, it } from "vitest";
import { stage5Digest } from "./multi-discount-backfill.js";
import {
  buildStage5V2Capability,
  buildStage5V2CatalogsAndSwitches,
} from "./pricing-stage5-materializer-v2.js";

describe("target pricing capability", () => {
  const capabilityDigest = (generation: number, rawEntries: readonly {
    provider_id: string;
    canonical_model_id: string;
    enabled: true;
  }[]) => {
    const entries = rawEntries.map((entry) => {
      const capabilityData = { pricing_supported: true };
      return {
        ...entry,
        entry_digest: stage5Digest("capability-entry", {
          ...entry,
          capability_data: capabilityData,
        }),
        capability_data: capabilityData,
      };
    });
    const capability = {
      generation,
      schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
      entries,
      aliases: [{
        provider_id: "openai",
        alias_model_id: "gpt-5.6",
        canonical_model_id: "gpt-5.6-sol",
      }],
    };
    return { digest: stage5Digest("capability", capability), entries };
  };

  it("keeps rejected generations immutable and publishes admitted generation 5", () => {
    expect(MULTI_DISCOUNT_GEN3_GEMINI_CANONICAL_MODELS).toEqual([
      "gemini-2.5-flash",
      "gemini-2.5-flash-lite",
      "gemini-2.5-pro",
      "gemini-3.1-flash-image",
      "gemini-3.1-flash-lite",
      "gemini-3.1-pro-preview",
      "gemini-3.5-flash",
      "gemini-3.6-flash",
    ]);
    const generation3 = capabilityDigest(
      MULTI_DISCOUNT_GEN3_CAPABILITY_GENERATION,
      MULTI_DISCOUNT_GEN3_MAIN_CATALOG_ENTRIES,
    );
    expect(generation3.entries).toHaveLength(20);
    expect(generation3.digest).toBe(MULTI_DISCOUNT_GEN3_CAPABILITY_DIGEST);

    expect(CURRENT_GEMINI_CANONICAL_MODELS).toEqual([
      "gemini-2.5-flash",
      "gemini-2.5-flash-lite",
      "gemini-2.5-pro",
      "gemini-3-flash-preview",
      "gemini-3.1-flash-image",
      "gemini-3.1-flash-lite",
      "gemini-3.1-pro-preview",
      "gemini-3.5-flash",
      "gemini-3.6-flash",
    ]);
    const generation4 = capabilityDigest(
      MULTI_DISCOUNT_TARGET_CAPABILITY_GENERATION,
      MULTI_DISCOUNT_TARGET_MAIN_CATALOG_ENTRIES,
    );
    expect(generation4.entries).toHaveLength(21);
    expect(generation4.digest).toBe(MULTI_DISCOUNT_TARGET_CAPABILITY_DIGEST);

    expect(MULTI_DISCOUNT_GEN5_GEMINI_CANONICAL_MODELS).toEqual(CURRENT_GEMINI_CANONICAL_MODELS);
    const generation5 = capabilityDigest(
      MULTI_DISCOUNT_GEN5_CAPABILITY_GENERATION,
      MULTI_DISCOUNT_GEN5_MAIN_CATALOG_ENTRIES,
    );
    expect(generation5.entries).toHaveLength(21);
    expect(generation5.digest).toBe(MULTI_DISCOUNT_GEN5_CAPABILITY_DIGEST);
    expect(generation5.digest).not.toBe(MULTI_DISCOUNT_TARGET_CAPABILITY_DIGEST);
  });

  it("keeps the pinned OpenKeys catalog Gemini-free while the issuance display is universal", () => {
    expect(MULTI_DISCOUNT_GEN5_OPENKEYS_CATALOG_ENTRIES)
      .toEqual(MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES);
    expect(MULTI_DISCOUNT_GEN5_OPENKEYS_CATALOG_ENTRIES)
      .not.toContainEqual(expect.objectContaining({ provider_id: "google" }));
    expect(MULTI_DISCOUNT_TARGET_OPENKEYS_CATALOG_ENTRIES)
      .toEqual(MULTI_DISCOUNT_GEN5_MAIN_CATALOG_ENTRIES);
    expect(MULTI_DISCOUNT_TARGET_OPENKEYS_CATALOG_ENTRIES)
      .toContainEqual(expect.objectContaining({ provider_id: "google" }));
  });

  it("moves Stage 5 target and recovery materialization to admitted generation 5", () => {
    const capability = buildStage5V2Capability();
    const graph = buildStage5V2CatalogsAndSwitches();

    expect(capability).toMatchObject({
      generation: MULTI_DISCOUNT_GEN5_CAPABILITY_GENERATION,
      content_digest: MULTI_DISCOUNT_GEN5_CAPABILITY_DIGEST,
    });
    expect(graph.catalogs.every((catalog) =>
      catalog.capability_generation === MULTI_DISCOUNT_GEN5_CAPABILITY_GENERATION
      && catalog.capability_digest === MULTI_DISCOUNT_GEN5_CAPABILITY_DIGEST
    )).toBe(true);
    expect(graph.catalogs.find((catalog) => catalog.product_id === "main")?.entries)
      .toContainEqual(expect.objectContaining({ canonical_model_id: "gemini-3-flash-preview" }));
  });
});
