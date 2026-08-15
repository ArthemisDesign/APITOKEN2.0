import { describe, expect, it, vi } from "vitest";
import { documentLanguageForPathname, languagePreferenceBootstrapScript, localeDestination, localeHref, localeRoute, supportsRussianRoute, withoutRussianPrefix } from "./locale-routes";

describe("locale routes", () => {
  it("derives the server document language from the locale route prefix", () => {
    expect(documentLanguageForPathname("/ru/docs")).toBe("ru");
    expect(documentLanguageForPathname("/ko/docs/learn")).toBe("ko");
    expect(documentLanguageForPathname("/zh/docs/learn/article")).toBe("zh-CN");
    expect(documentLanguageForPathname("/models")).toBe("en");
  });

  it("maps only routes with real Russian mirrors", () => {
    for (const path of ["/", "/login", "/register", "/docs", "/docs/learn", "/docs/learn/buying-a-key", "/dashboard", "/int-codex", "/int-opencode"]) {
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

  it("preserves referral, invitation, and fragment data when changing language", () => {
    expect(localeDestination("/", "ru", "?ref=partner-code", "#pricing")).toBe("/ru?ref=partner-code#pricing");
    expect(localeDestination("/ru/register", "en", "?ref=partner-code&invite=invite-token", "#form")).toBe(
      "/register?ref=partner-code&invite=invite-token#form",
    );
    expect(localeDestination("/about", "ru", "?ref=partner-code")).toBeNull();
  });

  it("restores Russian before React while preserving sensitive URL data", () => {
    const replace = vi.fn();
    const runBootstrap = new Function("localStorage", "location", languagePreferenceBootstrapScript);
    runBootstrap(
      { getItem: () => "ru" },
      { pathname: "/reset-password", search: "?token=secret", hash: "#step", replace },
    );
    expect(replace).toHaveBeenCalledWith("/ru/reset-password?token=secret#step");
  });

  it("does not restore a saved language over unsupported or already localized URLs", () => {
    const replace = vi.fn();
    const runBootstrap = new Function("localStorage", "location", languagePreferenceBootstrapScript);
    for (const pathname of ["/about", "/auth/callback", "/ru/dashboard"]) {
      runBootstrap({ getItem: () => "ru" }, { pathname, search: "", hash: "", replace });
    }
    runBootstrap({ getItem: () => "en" }, { pathname: "/dashboard", search: "", hash: "", replace });
    expect(replace).not.toHaveBeenCalled();
  });
});
