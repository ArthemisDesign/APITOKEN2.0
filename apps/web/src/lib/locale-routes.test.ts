import { describe, expect, it } from "vitest";
import { documentLanguageForPathname, localeHref, localeRoute, supportsRussianRoute, withoutRussianPrefix } from "./locale-routes";

describe("locale routes", () => {
  it("derives the server document language from the locale route prefix", () => {
    expect(documentLanguageForPathname("/ru/docs")).toBe("ru");
    expect(documentLanguageForPathname("/ko/docs/learn")).toBe("ko");
    expect(documentLanguageForPathname("/zh/docs/learn/article")).toBe("zh-CN");
    expect(documentLanguageForPathname("/models")).toBe("en");
  });

  it("maps only routes with real Russian mirrors", () => {
    for (const path of ["/", "/login", "/register", "/docs", "/docs/learn", "/docs/learn/buying-a-key", "/dashboard"]) {
      expect(supportsRussianRoute(path), path).toBe(true);
      expect(localeRoute(path, "ru"), path).toBe(path === "/" ? "/ru" : `/ru${path}`);
    }
  });

  it("does not manufacture misleading Russian URLs for English-only pages", () => {
    for (const path of ["/about", "/blog", "/changelog", "/contacts", "/models/claude-opus-4-8", "/status", "/tools/claude-api-cost-calculator"]) {
      expect(supportsRussianRoute(path), path).toBe(false);
      expect(localeRoute(path, "ru"), path).toBeNull();
    }
  });

  it("returns safely to the English equivalent", () => {
    expect(withoutRussianPrefix("/ru")).toBe("/");
    expect(withoutRussianPrefix("/ru/docs/learn/example")).toBe("/docs/learn/example");
    expect(localeRoute("/ru/login", "en")).toBe("/login");
    expect(localeHref("/login?verified=1#form", "ru")).toBe("/ru/login?verified=1#form");
    expect(localeHref("https://example.com", "ru")).toBe("https://example.com");
  });
});
