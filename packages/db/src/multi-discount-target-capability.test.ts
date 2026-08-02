import {
  CURRENT_GEMINI_CANONICAL_MODELS,
  MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  MULTI_DISCOUNT_TARGET_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_TARGET_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_TARGET_MAIN_CATALOG_ENTRIES,
  MULTI_DISCOUNT_TARGET_OPENKEYS_CATALOG_ENTRIES,
} from "@claude-api/contracts";
import { describe, expect, it } from "vitest";
import { stage5Digest } from "./multi-discount-backfill.js";

describe("target pricing capability", () => {
  it("reproduces the reviewed generation-3 digest over the exact Gemini model set", () => {
    expect(CURRENT_GEMINI_CANONICAL_MODELS).toEqual([
      "gemini-2.5-flash",
      "gemini-2.5-flash-lite",
      "gemini-2.5-pro",
      "gemini-3.1-flash-image",
      "gemini-3.1-flash-lite",
      "gemini-3.1-pro-preview",
      "gemini-3.5-flash",
      "gemini-3.6-flash",
    ]);

    const entries = MULTI_DISCOUNT_TARGET_MAIN_CATALOG_ENTRIES.map((entry) => {
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
      generation: MULTI_DISCOUNT_TARGET_CAPABILITY_GENERATION,
      schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
      entries,
      aliases: [{
        provider_id: "openai",
        alias_model_id: "gpt-5.6",
        canonical_model_id: "gpt-5.6-sol",
      }],
    };

    expect(entries).toHaveLength(20);
    expect(stage5Digest("capability", capability)).toBe(MULTI_DISCOUNT_TARGET_CAPABILITY_DIGEST);
  });

  it("does not silently add Gemini to the OpenKeys catalog", () => {
    expect(MULTI_DISCOUNT_TARGET_OPENKEYS_CATALOG_ENTRIES)
      .toEqual(MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES);
    expect(MULTI_DISCOUNT_TARGET_OPENKEYS_CATALOG_ENTRIES)
      .not.toContainEqual(expect.objectContaining({ provider_id: "google" }));
  });
});
