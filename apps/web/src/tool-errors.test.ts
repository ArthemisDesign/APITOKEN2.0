import { describe, expect, it } from "vitest";
import { buildSitemap } from "./app/sitemap";
import {
  findToolError,
  resolveToolError,
  TOOL_ERROR_LOCALES,
  TOOL_ERROR_TOOLS,
  TOOL_ERRORS,
  toolErrorPath,
  toolErrors,
  toolErrorsIndexPath,
  toolHubPath,
} from "./lib/tool-errors";
import { toolErrorsRu } from "./lib/tool-errors-ru";
import { toolErrorsZh } from "./lib/tool-errors-zh";
import { toolErrorsKo } from "./lib/tool-errors-ko";

const TRANSLATIONS = { ru: toolErrorsRu, zh: toolErrorsZh, ko: toolErrorsKo } as const;

describe("tool error catalog", () => {
  it("gives every tool exactly four error pages — the cluster is 1 + 6 + 24 URLs per locale", () => {
    expect(TOOL_ERROR_TOOLS).toHaveLength(6);
    for (const tool of TOOL_ERROR_TOOLS) {
      expect(toolErrors(tool.slug), tool.slug).toHaveLength(4);
    }
    expect(TOOL_ERRORS).toHaveLength(24);
  });

  it("keeps slugs unique within a tool and stable lookups working", () => {
    const seen = new Set<string>();
    for (const entry of TOOL_ERRORS) {
      const key = `${entry.tool}/${entry.slug}`;
      expect(seen.has(key), key).toBe(false);
      seen.add(key);
      expect(findToolError(entry.tool, entry.slug)).toBe(entry);
    }
  });

  it("gives every entry search strings, causes, fixes and FAQ content", () => {
    for (const entry of TOOL_ERRORS) {
      const key = `${entry.tool}/${entry.slug}`;
      expect(entry.searchStrings.length, key).toBeGreaterThan(0);
      expect(entry.causes.length, key).toBeGreaterThanOrEqual(2);
      expect(entry.fixes.length, key).toBeGreaterThanOrEqual(2);
      expect(entry.faq.length, key).toBeGreaterThanOrEqual(2);
      expect(entry.title.length, key).toBeGreaterThan(10);
      expect(entry.description.length, key).toBeGreaterThan(50);
    }
  });

  it("has unique titles and descriptions across the cluster", () => {
    const titles = TOOL_ERRORS.map((entry) => entry.title);
    const descriptions = TOOL_ERRORS.map((entry) => entry.description);
    expect(new Set(titles).size).toBe(titles.length);
    expect(new Set(descriptions).size).toBe(descriptions.length);
  });
});

describe("tool error localization parity", () => {
  for (const [locale, translations] of Object.entries(TRANSLATIONS)) {
    it(`covers every tool, entry and UI string in ${locale}`, () => {
      for (const tool of TOOL_ERROR_TOOLS) {
        const info = translations.tools[tool.slug];
        expect(info, `${locale} tools/${tool.slug}`).toBeDefined();
        expect(info.title.length).toBeGreaterThan(5);
      }
      for (const entry of TOOL_ERRORS) {
        const key = `${entry.tool}/${entry.slug}`;
        const translated = translations.entries[key];
        expect(translated, `${locale} entries/${key}`).toBeDefined();
        expect(translated.causes, `${locale} ${key} causes`).toHaveLength(entry.causes.length);
        expect(translated.fixes, `${locale} ${key} fixes`).toHaveLength(entry.fixes.length);
        expect(translated.faq, `${locale} ${key} faq`).toHaveLength(entry.faq.length);
        if (entry.snippet) {
          expect(translated.snippetLabel, `${locale} ${key} snippetLabel`).toBeTruthy();
        }
      }
      expect(translations.ui.errorsIn).toContain("{tool}");
      expect(translations.ui.backToTool).toContain("{tool}");
      expect(translations.index.title.length).toBeGreaterThan(5);
    });

    it(`resolves entries with translated prose in ${locale}`, () => {
      for (const entry of TOOL_ERRORS) {
        const resolved = resolveToolError(entry, locale as "ru" | "zh" | "ko", translations);
        expect(resolved.localeTitle).toBe(translations.entries[`${entry.tool}/${entry.slug}`].title);
        // Verbatim strings must never be localized.
        expect(resolved.searchStrings).toEqual(entry.searchStrings);
        if (entry.snippet) expect(resolved.snippet?.code).toBe(entry.snippet.code);
      }
    });
  }
});

describe("tool error sitemap", () => {
  it("publishes the full cluster in every locale — 31 URLs each", () => {
    const urls = new Set(buildSitemap().map((entry) => entry.url));
    for (const locale of TOOL_ERROR_LOCALES) {
      const expected = [
        toolErrorsIndexPath(locale),
        ...TOOL_ERROR_TOOLS.flatMap((tool) => [
          toolHubPath(tool.slug, locale),
          ...toolErrors(tool.slug).map((entry) => toolErrorPath(tool.slug, entry.slug, locale)),
        ]),
      ];
      expect(expected).toHaveLength(31);
      for (const path of expected) {
        expect(urls.has(`https://apitoken.sale${path}`), path).toBe(true);
      }
    }
  });
});
