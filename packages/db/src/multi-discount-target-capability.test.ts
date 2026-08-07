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
  GPT_IMAGE_2_CANONICAL_MODEL,
  CLAUDE_OPUS_4_6_CANONICAL_MODEL,
  CLAUDE_OPUS_4_5_CANONICAL_MODEL,
  CLAUDE_SONNET_4_5_CANONICAL_MODEL,
  MULTI_DISCOUNT_GEN7_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN7_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN7_MAIN_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN7_OPENKEYS_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN6_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN6_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN6_MAIN_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN6_OPENKEYS_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  MULTI_DISCOUNT_TARGET_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_TARGET_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_TARGET_MAIN_CATALOG_ENTRIES,
  MULTI_DISCOUNT_TARGET_OPENKEYS_CATALOG_ENTRIES,
} from "@claude-api/contracts";
import { describe, expect, it } from "vitest";
import { stage5Digest } from "./pricing-release-digest.js";
import {
  buildStage5V2Capability,
  buildStage5V2CatalogsAndSwitches,
} from "./pricing-stage5-materializer-v2.js";

describe("target pricing capability", () => {
  const capabilityDigest = (
    generation: number,
    rawEntries: readonly {
      provider_id: string;
      canonical_model_id: string;
      enabled: true;
    }[],
    aliases = [{
      provider_id: "openai",
      alias_model_id: "gpt-5.6",
      canonical_model_id: "gpt-5.6-sol",
    }],
  ) => {
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
      aliases,
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

    const generation6 = capabilityDigest(
      MULTI_DISCOUNT_GEN6_CAPABILITY_GENERATION,
      MULTI_DISCOUNT_GEN6_MAIN_CATALOG_ENTRIES,
      [
        {
          provider_id: "openai",
          alias_model_id: "gpt-5.6",
          canonical_model_id: "gpt-5.6-sol",
        },
        {
          provider_id: "openai",
          alias_model_id: "gpt-image-2",
          canonical_model_id: GPT_IMAGE_2_CANONICAL_MODEL,
        },
      ],
    );
    expect(generation6.entries).toHaveLength(22);
    expect(generation6.digest).toBe(MULTI_DISCOUNT_GEN6_CAPABILITY_DIGEST);
    expect(generation6.digest).not.toBe(MULTI_DISCOUNT_GEN5_CAPABILITY_DIGEST);

    const generation7 = capabilityDigest(
      MULTI_DISCOUNT_GEN7_CAPABILITY_GENERATION,
      MULTI_DISCOUNT_GEN7_MAIN_CATALOG_ENTRIES,
      [
        {
          provider_id: "anthropic",
          alias_model_id: "claude-haiku-4-5-20251001",
          canonical_model_id: "claude-haiku-4-5",
        },
        {
          provider_id: "anthropic",
          alias_model_id: "claude-opus-4-5-20251101",
          canonical_model_id: CLAUDE_OPUS_4_5_CANONICAL_MODEL,
        },
        {
          provider_id: "anthropic",
          alias_model_id: "claude-sonnet-4-5-20250929",
          canonical_model_id: CLAUDE_SONNET_4_5_CANONICAL_MODEL,
        },
        {
          provider_id: "openai",
          alias_model_id: "gpt-5.6",
          canonical_model_id: "gpt-5.6-sol",
        },
        {
          provider_id: "openai",
          alias_model_id: "gpt-image-2",
          canonical_model_id: GPT_IMAGE_2_CANONICAL_MODEL,
        },
      ],
    );
    expect(generation7.entries).toHaveLength(25);
    expect(generation7.digest).toBe(MULTI_DISCOUNT_GEN7_CAPABILITY_DIGEST);
    expect(generation7.digest).not.toBe(MULTI_DISCOUNT_GEN6_CAPABILITY_DIGEST);
  });

  it("keeps frozen OpenKeys generations and adds only GPT Image 2 in generation 6", () => {
    expect(MULTI_DISCOUNT_GEN5_OPENKEYS_CATALOG_ENTRIES)
      .toEqual(MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES);
    expect(MULTI_DISCOUNT_GEN5_OPENKEYS_CATALOG_ENTRIES)
      .not.toContainEqual(expect.objectContaining({ provider_id: "google" }));
    expect(MULTI_DISCOUNT_GEN6_OPENKEYS_CATALOG_ENTRIES).toEqual([
      ...MULTI_DISCOUNT_GEN5_OPENKEYS_CATALOG_ENTRIES,
      {
        provider_id: "openai",
        canonical_model_id: GPT_IMAGE_2_CANONICAL_MODEL,
        enabled: true,
      },
    ]);
    expect(MULTI_DISCOUNT_GEN7_OPENKEYS_CATALOG_ENTRIES).toEqual([
      ...MULTI_DISCOUNT_GEN6_OPENKEYS_CATALOG_ENTRIES,
      { provider_id: "anthropic", canonical_model_id: CLAUDE_OPUS_4_6_CANONICAL_MODEL, enabled: true },
      { provider_id: "anthropic", canonical_model_id: CLAUDE_OPUS_4_5_CANONICAL_MODEL, enabled: true },
      { provider_id: "anthropic", canonical_model_id: CLAUDE_SONNET_4_5_CANONICAL_MODEL, enabled: true },
    ]);
    expect(MULTI_DISCOUNT_TARGET_OPENKEYS_CATALOG_ENTRIES)
      .toEqual(MULTI_DISCOUNT_GEN5_MAIN_CATALOG_ENTRIES);
    expect(MULTI_DISCOUNT_TARGET_OPENKEYS_CATALOG_ENTRIES)
      .toContainEqual(expect.objectContaining({ provider_id: "google" }));
    expect(MULTI_DISCOUNT_TARGET_OPENKEYS_CATALOG_ENTRIES)
      .not.toContainEqual(expect.objectContaining({ canonical_model_id: GPT_IMAGE_2_CANONICAL_MODEL }));
  });

  it("moves Stage 5 target and recovery materialization to admitted generation 7", () => {
    const capability = buildStage5V2Capability();
    const graph = buildStage5V2CatalogsAndSwitches();

    expect(capability).toMatchObject({
      generation: MULTI_DISCOUNT_GEN7_CAPABILITY_GENERATION,
      content_digest: MULTI_DISCOUNT_GEN7_CAPABILITY_DIGEST,
    });
    expect(graph.catalogs.every((catalog) =>
      catalog.capability_generation === MULTI_DISCOUNT_GEN7_CAPABILITY_GENERATION
      && catalog.capability_digest === MULTI_DISCOUNT_GEN7_CAPABILITY_DIGEST
    )).toBe(true);
    for (const productId of ["main", "openkeys"]) {
      const entries = graph.catalogs.find((catalog) => catalog.product_id === productId)?.entries;
      expect(entries).toContainEqual(expect.objectContaining({
        provider_id: "openai",
        canonical_model_id: GPT_IMAGE_2_CANONICAL_MODEL,
      }));
      for (const model of [
        CLAUDE_OPUS_4_6_CANONICAL_MODEL,
        CLAUDE_OPUS_4_5_CANONICAL_MODEL,
        CLAUDE_SONNET_4_5_CANONICAL_MODEL,
      ]) {
        expect(entries).toContainEqual(expect.objectContaining({
          provider_id: "anthropic",
          canonical_model_id: model,
        }));
      }
    }
  });
});
