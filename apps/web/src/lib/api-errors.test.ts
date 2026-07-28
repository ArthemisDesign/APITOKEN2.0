import { describe, expect, it } from "vitest";
import { API_ERRORS, ERROR_CODES, errorsUi, resolveApiErrors } from "./api-errors";
import { apiErrorsRu } from "./api-errors-ru";

describe("api error catalog", () => {
  it("has unique, url-safe codes", () => {
    expect(new Set(ERROR_CODES).size).toBe(ERROR_CODES.length);
    for (const code of ERROR_CODES) expect(code).toMatch(/^[a-z0-9-]+$/);
  });

  it("gives every entry a verbatim message, a cause and a fix", () => {
    for (const entry of API_ERRORS) {
      expect(entry.message.length, entry.code).toBeGreaterThan(0);
      expect(entry.type.length, entry.code).toBeGreaterThan(0);
      expect(entry.causes.length, entry.code).toBeGreaterThan(0);
      expect(entry.fixes.length, entry.code).toBeGreaterThan(0);
    }
  });

  it("translates every entry into Russian with the same structure", () => {
    for (const entry of API_ERRORS) {
      const ru = apiErrorsRu[entry.code];
      expect(ru, `missing ru translation for ${entry.code}`).toBeDefined();
      expect(ru.title.length, entry.code).toBeGreaterThan(0);
      expect(ru.causes.length, entry.code).toBe(entry.causes.length);
      expect(ru.fixes.length, entry.code).toBe(entry.fixes.length);
      if (entry.snippet) expect(ru.snippetLabel, entry.code).toBeDefined();
    }
  });

  it("carries no Russian translations for codes that no longer exist", () => {
    for (const code of Object.keys(apiErrorsRu)) expect(ERROR_CODES).toContain(code);
  });

  // The whole page exists so that a pasted error string matches. Translating the
  // response text itself would break exactly that, so the resolved Russian view
  // must keep message/type/alsoSearchedAs identical to the English catalog.
  it("keeps response strings untranslated in every locale", () => {
    const ru = resolveApiErrors("ru", apiErrorsRu);
    expect(ru.length).toBe(API_ERRORS.length);
    ru.forEach((entry, index) => {
      const source = API_ERRORS[index];
      expect(entry.message).toBe(source.message);
      expect(entry.type).toBe(source.type);
      expect(entry.status).toBe(source.status);
      expect(entry.alsoSearchedAs).toEqual(source.alsoSearchedAs);
      expect(entry.snippet?.code).toBe(source.snippet?.code);
    });
  });

  it("falls back to English when a translation is missing rather than dropping the entry", () => {
    const resolved = resolveApiErrors("ru", {});
    expect(resolved.length).toBe(API_ERRORS.length);
    expect(resolved[0].localeTitle).toBe(API_ERRORS[0].title);
  });

  it("localises every UI string in both locales", () => {
    const keys = Object.keys(errorsUi.en) as (keyof typeof errorsUi.en)[];
    for (const key of keys) {
      expect(errorsUi.ru[key], `ru.${key}`).toBeTruthy();
      expect(errorsUi.en[key], `en.${key}`).toBeTruthy();
    }
  });

  it("documents the two responses that exist only on this gateway", () => {
    const gatewayOnly = API_ERRORS.filter((entry) => entry.surface === "apitoken").map((e) => e.code);
    expect(gatewayOnly).toContain("insufficient-balance");
    expect(gatewayOnly).toContain("invalid-beta-header");
  });
});
