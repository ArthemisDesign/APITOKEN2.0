import { describe, expect, it } from "vitest";
import { DASHBOARD_PROVIDERS } from "./providers";

describe("dashboard provider metadata", () => {
  it("maps the engine Google provider id to the complete Gemini card", () => {
    const google = DASHBOARD_PROVIDERS.find((provider) => provider.id === "google");

    expect(google).toMatchObject({
      name: "Gemini",
      api: "Google Gemini API",
      logo: "/assets/providers/gemini.svg",
      endpoint: "router.apitoken.sale",
      auth: "x-goog-api-key",
      docsPath: "/docs",
    });
  });
});
