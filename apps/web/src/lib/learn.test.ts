import { describe, expect, it } from "vitest";
import { buildSitemap } from "../app/sitemap";
import {
  articlesForLocale,
  learnPath,
  learnArticles,
  learnArticlesBySlug,
  LOCALES,
  orderLearnHubArticles,
  renderLearnMarkdown,
  resolveArticle,
} from "./learn";
import { learnProviderEn } from "./learn-provider-en";
import { learnImageSeoEn } from "./learn-image-seo";
import { buildArticleJsonLd, buildArticleMetadata } from "./learn-page";
import { buildLlms } from "./llms";
import { absoluteUrl } from "./seo";

function textValues(value: unknown): string[] {
  if (typeof value === "string") return [value];
  if (Array.isArray(value)) return value.flatMap(textValues);
  if (value && typeof value === "object") return Object.values(value).flatMap(textValues);
  return [];
}

describe("learn cluster", () => {
  it("has unique slugs", () => {
    const slugs = learnArticles.map((article) => article.slug);
    expect(new Set(slugs).size).toBe(slugs.length);
  });

  it("keeps the catalog limited to the hand-written articles", () => {
    // 47 core guides + 16 manual provider guides + 8 image-generation guides.
    // Templated near-duplicate provider pages were removed: Google treated them
    // as duplicates and refused to index them.
    expect(learnArticles).toHaveLength(71);
    expect(learnArticles).toHaveLength(
      47 + learnProviderEn.length + learnImageSeoEn.length,
    );
  });

  it("only links related guides that exist", () => {
    for (const article of learnArticles) {
      for (const related of article.related) {
        expect(learnArticlesBySlug[related], `${article.slug} -> ${related}`).toBeDefined();
      }
    }
  });

  it("every article has metadata, keywords and at least one FAQ", () => {
    for (const article of learnArticles) {
      expect(article.title.length).toBeGreaterThan(0);
      expect(article.description.length).toBeGreaterThan(0);
      expect(article.keywords.length).toBeGreaterThan(0);
      expect(article.sections.length).toBeGreaterThan(0);
      expect(article.faq.length).toBeGreaterThan(0);
    }
  });

  it("renders self-contained markdown with front matter and headings", () => {
    const article = resolveArticle(learnArticles[0]!.slug, "en")!;
    const md = renderLearnMarkdown(article, "https://apitoken.sale");
    expect(md).toContain("---");
    expect(md).toContain(`# ${article.content.h1}`);
  });
  it("publishes distinct high-intent clusters for GPT, Gemini and Kimi", () => {
    expect(learnProviderEn).toHaveLength(16);
    expect(learnProviderEn.filter((article) => article.slug.includes("gpt"))).toHaveLength(4);
    expect(learnProviderEn.filter((article) => article.slug.includes("gemini") || article.slug.includes("nano-banana"))).toHaveLength(5);
    expect(learnProviderEn.filter((article) => article.slug.includes("kimi"))).toHaveLength(7);
    expect(new Set(learnProviderEn.map((article) => article.title)).size).toBe(learnProviderEn.length);
    expect(new Set(learnProviderEn.map((article) => article.description)).size).toBe(learnProviderEn.length);

    for (const article of learnProviderEn) {
      expect(article.sections.length, `${article.slug} sections`).toBeGreaterThanOrEqual(2);
      expect(article.faq.length, `${article.slug} FAQs`).toBeGreaterThanOrEqual(3);
      expect(article.published, `${article.slug} published`).toMatch(/^\d{4}-\d{2}-\d{2}$/);
      expect(article.updated, `${article.slug} updated`).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    }
  });

  it("publishes a substantive localized image-generation SEO cluster", () => {
    const expectedSlugs = [
      "nano-banana-2-api-cost",
      "gpt-image-2-api-cost",
      "nano-banana-2-vs-gpt-image-2",
      "image-generation-api-pricing",
      "cheapest-image-generation-api",
      "image-editing-api-guide",
      "batch-image-generation-api",
      "image-generation-api-for-ecommerce",
    ];

    expect(learnImageSeoEn.map((article) => article.slug)).toEqual(expectedSlugs);
    expect(new Set(learnImageSeoEn.map((article) => article.title)).size).toBe(learnImageSeoEn.length);
    expect(new Set(learnImageSeoEn.map((article) => article.description)).size).toBe(learnImageSeoEn.length);

    const sitemapUrls = new Set(buildSitemap().map((entry) => entry.url));
    for (const article of learnImageSeoEn) {
      expect(article.sections.length, `${article.slug} sections`).toBeGreaterThanOrEqual(3);
      expect(article.faq.length, `${article.slug} FAQs`).toBeGreaterThanOrEqual(4);
      expect(article.related.length, `${article.slug} related`).toBeGreaterThanOrEqual(4);
      expect(article.published, `${article.slug} published`).toBe("2026-08-09");
      expect(article.updated, `${article.slug} updated`).toBe("2026-08-09");

      for (const locale of LOCALES) {
        const resolved = resolveArticle(article.slug, locale)!;
        expect(resolved, `${article.slug} @ ${locale}`).not.toBeNull();
        expect(resolved.content.sections.length, `${article.slug} @ ${locale} sections`).toBe(article.sections.length);
        expect(resolved.content.faq.length, `${article.slug} @ ${locale} FAQs`).toBe(article.faq.length);
        const minimumDepth = locale === "zh" ? 1_500 : 1_800;
        expect(JSON.stringify(resolved.content).length, `${article.slug} @ ${locale} depth`).toBeGreaterThan(minimumDepth);

        const blocks = resolved.content.sections.flatMap((section) => section.blocks);
        expect(blocks.some((block) => block.type === "table"), `${article.slug} @ ${locale} table`).toBe(true);
        expect(blocks.some((block) => block.type === "steps"), `${article.slug} @ ${locale} steps`).toBe(true);
        expect(JSON.stringify(resolved.content), `${article.slug} @ ${locale} B2C policy`).toMatch(/50%|五折/);

        const path = learnPath(article.slug, locale);
        expect(sitemapUrls, `${article.slug} @ ${locale} sitemap`).toContain(absoluteUrl(path));
        const metadata = buildArticleMetadata(article.slug, locale)!;
        expect(metadata.alternates?.canonical, `${article.slug} @ ${locale} canonical`).toBe(absoluteUrl(path));
        expect(Object.keys(metadata.alternates?.languages ?? {}).sort(), `${article.slug} @ ${locale} hreflang`).toEqual([
          "en",
          "ko",
          "ru",
          "x-default",
          "zh-CN",
        ]);
        const graph = buildArticleJsonLd(article.slug, locale)?.["@graph"] ?? [];
        expect(graph.some((node) => node["@type"] === "Article"), `${article.slug} @ ${locale} Article schema`).toBe(true);
        expect(graph.some((node) => node["@type"] === "FAQPage"), `${article.slug} @ ${locale} FAQ schema`).toBe(true);
      }
    }
  });

  it("keeps image savings claims tied to authoritative usage and published controls", () => {
    for (const locale of LOCALES) {
      const nano = JSON.stringify(resolveArticle("nano-banana-2-api-cost", locale)!.content);
      expect(nano, locale).toContain("gemini-3.1-flash-image");
      expect(nano, locale).toContain("$0.0336");
      expect(nano, locale).toContain("$0.0756");
      expect(nano, locale).toContain("1K");
      expect(nano, locale).toContain("4K");
      expect(nano, locale).toContain("0.5K");
      expect(nano, locale).toContain("OpenKeys");

      const gpt = JSON.stringify(resolveArticle("gpt-image-2-api-cost", locale)!.content);
      expect(gpt, locale).toContain("gpt-image-2");
      expect(gpt, locale).toMatch(/terminal usage|终态 usage/);
      expect(gpt, locale).toContain("$15");
      expect(gpt, locale).toContain("opaque/low/auto");
      expect(gpt, locale).toMatch(/1–5|1~5|one to five/);
      expect(gpt, locale).toContain("OpenKeys");

      const comparison = JSON.stringify(resolveArticle("nano-banana-2-vs-gpt-image-2", locale)!.content);
      expect(comparison, locale).toContain("x-goog-api-key");
      expect(comparison, locale).toContain("Authorization: Bearer");
      expect(comparison, locale).not.toMatch(/always cheaper|всегда дешевле|总是更便宜|항상 더 저렴/i);

      const llms = buildLlms(locale);
      for (const article of learnImageSeoEn) expect(llms, `${article.slug} @ ${locale} llms`).toContain(learnPath(article.slug, locale));
    }
  });

  it("does not present Kimi High Speed as the low-cost tier", () => {
    // The hand-written kimi-api-pricing rates table must keep High Speed at
    // exactly double the Kimi for Coding rates in every locale.
    for (const locale of LOCALES) {
      const content = resolveArticle("kimi-api-pricing", locale)!.content;
      const ratesTable = content.sections
        .flatMap((section) => section.blocks)
        .find((block) => block.type === "table");
      expect(ratesTable?.type, locale).toBe("table");
      if (!ratesTable || ratesTable.type !== "table") continue;
      expect(ratesTable.rows.find((row) => row[0] === "kimi/kimi-for-coding"), locale).toContain("$0.19 / $0.95 / $4");
      expect(ratesTable.rows.find((row) => row[0] === "kimi/kimi-for-coding-highspeed"), locale).toContain("$0.38 / $1.90 / $8");
    }

    const comparison = JSON.stringify(resolveArticle("kimi-k3-vs-kimi-for-coding", "en")!.content);
    expect(comparison).toContain("Latency-sensitive coding where speed pays for itself");
  });

  it("exposes GPT, Gemini and Kimi guides to AI-readable indexes", () => {
    for (const locale of LOCALES) {
      const llms = buildLlms(locale);
      expect(llms, locale).toContain("Kimi");
      expect(llms, locale).toContain("kimi/k3");
      expect(llms, locale).toContain(learnPath("kimi-api-pricing", locale));
      expect(llms, locale).toContain("Kimi uses the Messages shape and accepts stream:true, but public chunk incrementality remains a preview capability under live validation");
    }
  });

  it("features equivalent provider journeys first in every hub cluster", () => {
    const expected = {
      buy: [
        "how-to-buy-claude-api-key",
        "how-to-buy-gpt-api-key",
        "how-to-buy-gemini-api-key",
        "how-to-buy-kimi-api-key",
      ],
      integrate: [
        "claude-api-quick-setup",
        "openai-api-quickstart",
        "gemini-api-quickstart",
        "kimi-api-quickstart",
        "claude-code-api-key",
        "codex-cli-setup",
        "kimi-api-for-opencode",
        "kimi-api-for-claude-code",
        "kimi-api-for-kimi-code",
      ],
      compare: [
        "claude-opus-vs-sonnet",
        "gpt-5-6-sol-vs-terra-vs-luna",
        "gemini-pro-vs-flash-vs-flash-lite",
        "kimi-k3-vs-kimi-for-coding",
      ],
      explain: [
        "claude-api-pricing-explained",
        "gpt-api-pricing",
        "gemini-api-pricing",
        "kimi-api-pricing",
      ],
    } as const;

    for (const locale of LOCALES) {
      const ordered = orderLearnHubArticles(
        articlesForLocale(locale)
          .map((slug) => resolveArticle(slug, locale))
          .filter((article) => article !== null),
      );

      for (const [cluster, slugs] of Object.entries(expected)) {
        expect(
          ordered.filter((article) => article.cluster === cluster).slice(0, slugs.length).map((article) => article.slug),
          `${locale} ${cluster}`,
        ).toEqual(slugs);
      }
    }
  });
});

describe("learn localization", () => {
  // Korean localizations still mirror the pre-rewrite article structure:
  // the ko quality wave was deferred (see research/SEO_LEARN_HANDOFF_2026-08-17.md).
  // Structural parity is enforced for the locales that were reworked.
  const STRUCTURE_SYNCED_LOCALES = LOCALES.filter((locale) => locale !== "ko");

  it("publishes every article in ru and zh with the same structure as en", () => {
    for (const article of learnArticles) {
      const english = resolveArticle(article.slug, "en")!;
      for (const locale of STRUCTURE_SYNCED_LOCALES) {
        const resolved = resolveArticle(article.slug, locale);
        expect(resolved, `${article.slug} @ ${locale}`).not.toBeNull();
        // structure parity with the English source
        expect(resolved!.content.sections.length).toBe(english.content.sections.length);
        expect(resolved!.content.faq.length).toBe(english.content.faq.length);
        expect(resolved!.content.keywords.length).toBeGreaterThan(0);
      }
    }
  });

  it("exposes the same article set across locales", () => {
    const en = articlesForLocale("en").sort();
    for (const locale of LOCALES) {
      expect(articlesForLocale(locale).sort(), locale).toEqual(en);
    }
  });

  it("keeps product facts verbatim in translations (base URL, model IDs)", () => {
    const ru = resolveArticle("claude-api-quick-setup", "ru")!;
    const zh = resolveArticle("claude-api-quick-setup", "zh")!;
    const flatten = (a: typeof ru) => JSON.stringify(a.content);
    for (const resolved of [ru, zh]) {
      expect(flatten(resolved)).toContain("https://router.apitoken.sale");
      expect(flatten(resolved)).toContain("claude-opus-4-8");
    }
  });

  it("keeps each provider on its real protocol in every locale", () => {
    for (const locale of STRUCTURE_SYNCED_LOCALES) {
      const gpt = JSON.stringify(resolveArticle("how-to-buy-gpt-api-key", locale)!.content);
      const gemini = JSON.stringify(resolveArticle("gemini-api-quickstart", locale)!.content);
      const kimi = JSON.stringify(resolveArticle("kimi-api-quickstart", locale)!.content);
      const kimiOpenCode = JSON.stringify(resolveArticle("kimi-api-for-opencode", locale)!.content);
      const kimiClaudeCode = JSON.stringify(resolveArticle("kimi-api-for-claude-code", locale)!.content);
      const kimiCode = JSON.stringify(resolveArticle("kimi-api-for-kimi-code", locale)!.content);

      expect(gpt, locale).toContain("Authorization: Bearer");
      expect(gpt, locale).toContain("gpt-5.6-terra");
      expect(gemini, locale).toContain("x-goog-api-key");
      expect(gemini, locale).toContain("gemini-3.6-flash");
      expect(kimi, locale).toContain("x-api-key");
      expect(kimi, locale).toContain("kimi/kimi-for-coding");
      expect(kimiOpenCode, locale).toContain("apitoken/kimi/kimi-for-coding");
      expect(kimiClaudeCode, locale).toContain("ANTHROPIC_DEFAULT_OPUS_MODEL=k3");
      expect(kimiClaudeCode, locale).toContain("claude --model k3");
      expect(kimiCode, locale).toContain('type = \\"openai\\"');
      expect(kimiCode, locale).toContain('base_url = \\"https://router.apitoken.sale/v1\\"');
      expect(kimiCode, locale).toContain('model = \\"kimi/k3\\"');
    }
  });

  it("describes only the implemented key guardrails in every locale", () => {
    const localized = Object.fromEntries(LOCALES.map((locale) => [
      locale,
      JSON.stringify({
        rateLimits: resolveArticle("claude-api-rate-limits", locale)!.content,
        security: resolveArticle("claude-api-key-security", locale)!.content,
      }),
    ])) as Record<(typeof LOCALES)[number], string>;

    expect(localized.en).toContain("lifetime spending limit");
    expect(localized.en).toContain("expiration date");
    expect(localized.ru).toContain("общий лимит расходов");
    expect(localized.ru).toContain("дата истечения");
    expect(localized.ko).toContain("평생 누적 지출 한도");
    expect(localized.ko).toContain("만료일");
    expect(localized.zh).toContain("终身累计消费上限");
    expect(localized.zh).toContain("到期日期");

    expect(localized.en).not.toMatch(/daily and monthly|model scoping|IP controls|rotation without downtime|configurable per-key caps/i);
    expect(localized.ru).not.toMatch(/дневн.*месячн|ограничение моделей|контроль по IP|без простоя/i);
    expect(localized.ko).not.toMatch(/일일.*월별|모델 범위 지정|IP 제어|다운타임 없이/i);
    expect(localized.zh).not.toMatch(/每日.*每月|模型限定|IP 管控|不停机轮换/i);
  });

  it("does not advertise unproven incremental Kimi streaming", () => {
    for (const locale of LOCALES) {
      const kimi = learnProviderEn
        .filter((article) => article.slug.includes("kimi"))
        .map((article) => resolveArticle(article.slug, locale)!.content);
      for (const value of textValues(kimi)) {
        expect(value, `${locale}: ${value}`).not.toMatch(/incremental SSE|инкрементальн.*SSE|增量 SSE|증분 SSE/i);
      }
    }
  });
});
