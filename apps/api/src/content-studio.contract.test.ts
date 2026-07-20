import { generateContentDraftsSchema } from "@claude-api/contracts";
import { describe, expect, it } from "vitest";

describe("Content Studio draft generation contract", () => {
  it("accepts every built-in platform profile, including the one-character X key", () => {
    const profiles = ["blog", "reddit", "vc-ru", "dzen", "habr", "medium", "x", "telegram", "linkedin"];

    expect(generateContentDraftsSchema.parse({ profiles, locale: "en" })).toEqual({ profiles, locale: "en" });
  });

  it("still rejects unsafe profile keys", () => {
    expect(generateContentDraftsSchema.safeParse({ profiles: ["X"], locale: "en" }).success).toBe(false);
    expect(generateContentDraftsSchema.safeParse({ profiles: ["../x"], locale: "en" }).success).toBe(false);
  });
});
