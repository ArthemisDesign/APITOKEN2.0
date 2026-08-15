import { describe, expect, it } from "vitest";
import { forgetAuthToken, rememberAuthToken, rememberedAuthToken, takeRememberedAuthToken } from "./auth-token-memory";

describe("auth token memory", () => {
  it("keeps scrubbed tokens across locale route remounts without browser storage", () => {
    expect(rememberAuthToken("reset-password", "reset-secret")).toBe("reset-secret");
    expect(rememberedAuthToken("reset-password")).toBe("reset-secret");
    expect(rememberedAuthToken("verify-email")).toBe("");

    expect(takeRememberedAuthToken("reset-password")).toBe("reset-secret");
    expect(rememberedAuthToken("reset-password")).toBe("");

    rememberAuthToken("reset-password", "another-secret");
    forgetAuthToken("reset-password");
    expect(rememberedAuthToken("reset-password")).toBe("");
  });
});
