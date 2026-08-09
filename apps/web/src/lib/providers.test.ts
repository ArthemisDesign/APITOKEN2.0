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

  it("gives KIMI a card on the Anthropic protocol with its own colour", () => {
    const kimi = DASHBOARD_PROVIDERS.find((provider) => provider.id === "kimi");

    expect(kimi).toMatchObject({
      name: "Kimi",
      // KIMI speaks Anthropic Messages, so the connection line must be the Anthropic one.
      api: "Anthropic Messages API",
      logo: "/assets/providers/kimi.svg",
      endpoint: "router.apitoken.sale",
      auth: "x-api-key",
      docsPath: "/docs",
    });
    const colors = DASHBOARD_PROVIDERS.map((provider) => provider.color);
    expect(new Set(colors).size).toBe(colors.length);
  });
});
