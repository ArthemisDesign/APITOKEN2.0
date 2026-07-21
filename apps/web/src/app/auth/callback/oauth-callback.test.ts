import { describe, expect, it } from "vitest";
import { oauthFeedbackIsSuccess } from "./oauth-callback";

describe("OAuth callback feedback", () => {
  it("never presents confirmation failures as success", () => {
    expect(oauthFeedbackIsSuccess("loading")).toBe(true);
    expect(oauthFeedbackIsSuccess("error")).toBe(false);
  });
});
