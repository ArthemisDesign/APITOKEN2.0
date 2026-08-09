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
import {
  CLAUDE_PROVIDER_PARITY,
  learnProviderParityEn,
  PARITY_PROVIDERS,
} from "./learn-provider-parity";
import { buildArticleJsonLd, buildArticleMetadata } from "./learn-page";
import { buildLlms } from "./llms";
import { absoluteUrl } from "./seo";

describe("learn cluster", () => {
  it("has unique slugs", () => {
    const slugs = learnArticles.map((article) => article.slug);
    expect(new Set(slugs).size).toBe(slugs.length);
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

  it("maps every article in the original 47-page catalog to GPT, Gemini and Kimi", () => {
    const providerArticleSlugs = new Set([
      ...learnProviderEn.map((article) => article.slug),
      ...learnProviderParityEn.map((article) => article.slug),
    ]);
    const originalCatalogSlugs = learnArticles
      .filter((article) => !providerArticleSlugs.has(article.slug))
      .map((article) => article.slug)
      .sort();

    expect(originalCatalogSlugs).toHaveLength(47);
    expect(Object.keys(CLAUDE_PROVIDER_PARITY).sort()).toEqual(originalCatalogSlugs);

    for (const [source, targets] of Object.entries(CLAUDE_PROVIDER_PARITY)) {
      expect(learnArticlesBySlug[source], source).toBeDefined();
      expect(Object.keys(targets).sort(), source).toEqual([...PARITY_PROVIDERS].sort());
      for (const provider of PARITY_PROVIDERS) {
        const target = targets[provider];
        expect(learnArticlesBySlug[target], `${source} -> ${provider}: ${target}`).toBeDefined();
        for (const locale of LOCALES) {
          expect(resolveArticle(target, locale), `${source} -> ${provider}: ${target} @ ${locale}`).not.toBeNull();
        }
      }
    }
  });

  it("keeps generated provider guides substantive, unique and fully localized", () => {
    expect(learnProviderParityEn).toHaveLength(119);
    expect(new Set(learnProviderParityEn.map((article) => article.slug)).size).toBe(learnProviderParityEn.length);
    expect(new Set(learnProviderParityEn.map((article) => article.title)).size).toBe(learnProviderParityEn.length);
    expect(new Set(learnProviderParityEn.map((article) => article.description)).size).toBe(learnProviderParityEn.length);

    for (const locale of LOCALES) {
      const localized = learnProviderParityEn.map((article) => resolveArticle(article.slug, locale)!.content);
      expect(new Set(localized.map((content) => content.title)).size, `${locale} titles`).toBe(localized.length);
      expect(new Set(localized.map((content) => content.description)).size, `${locale} descriptions`).toBe(localized.length);
    }

    for (const article of learnProviderParityEn) {
      expect(article.sections.length, `${article.slug} sections`).toBeGreaterThanOrEqual(4);
      expect(article.faq.length, `${article.slug} FAQs`).toBeGreaterThanOrEqual(3);
      expect(article.related.length, `${article.slug} related`).toBeGreaterThanOrEqual(3);
      expect(article.published, `${article.slug} published`).toMatch(/^\d{4}-\d{2}-\d{2}$/);
      expect(article.updated, `${article.slug} updated`).toMatch(/^\d{4}-\d{2}-\d{2}$/);

      for (const locale of LOCALES) {
        const resolved = resolveArticle(article.slug, locale)!;
        expect(resolved.content.sections.length, `${article.slug} @ ${locale} sections`).toBe(article.sections.length);
        expect(resolved.content.faq.length, `${article.slug} @ ${locale} FAQs`).toBe(article.faq.length);
        expect(JSON.stringify(resolved.content).length, `${article.slug} @ ${locale} depth`).toBeGreaterThan(1_800);
      }
    }
  });

  it("keeps exact provider protocols in every generated guide", () => {
    for (const article of learnProviderParityEn) {
      for (const locale of LOCALES) {
        const content = JSON.stringify(resolveArticle(article.slug, locale)!.content);
        if (article.slug.startsWith("gpt-")) {
          expect(content, `${article.slug} @ ${locale}`).toContain("Authorization: Bearer");
          expect(content, `${article.slug} @ ${locale}`).toContain("gpt-5.6-terra");
        } else if (article.slug.startsWith("gemini-")) {
          expect(content, `${article.slug} @ ${locale}`).toContain("x-goog-api-key");
          expect(content, `${article.slug} @ ${locale}`).toContain("gemini-3.6-flash");
        } else if (article.slug.startsWith("kimi-")) {
          expect(content, `${article.slug} @ ${locale}`).toContain("x-api-key");
          expect(content, `${article.slug} @ ${locale}`).toContain("kimi/kimi-for-coding");
          expect(content, `${article.slug} @ ${locale}`).not.toMatch(/incremental SSE|инкрементальн.*SSE|增量 SSE|증분 SSE/i);
        }
      }
    }
  });

  it("publishes every localized parity URL with canonical, hreflang and structured data", () => {
    const sitemapUrls = new Set(buildSitemap().map((entry) => entry.url));

    for (const article of learnProviderParityEn) {
      for (const locale of LOCALES) {
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
        expect(metadata.robots, `${article.slug} @ ${locale} robots`).toBeUndefined();

        const graph = buildArticleJsonLd(article.slug, locale)?.["@graph"] ?? [];
        expect(graph.some((node) => node["@type"] === "Article"), `${article.slug} @ ${locale} Article schema`).toBe(true);
        expect(graph.some((node) => node["@type"] === "FAQPage"), `${article.slug} @ ${locale} FAQ schema`).toBe(true);
      }
    }
  });

  it("exposes GPT, Gemini and Kimi parity to AI-readable indexes", () => {
    for (const locale of LOCALES) {
      const llms = buildLlms(locale);
      expect(llms, locale).toContain("Kimi");
      expect(llms, locale).toContain("kimi/k3");
      expect(llms, locale).toContain(learnPath("kimi-api-prompt-caching", locale));
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
  it("publishes every article in ru and zh with the same structure as en", () => {
    for (const article of learnArticles) {
      for (const locale of LOCALES) {
        const resolved = resolveArticle(article.slug, locale);
        expect(resolved, `${article.slug} @ ${locale}`).not.toBeNull();
        // structure parity with the English source
        expect(resolved!.content.sections.length).toBe(article.sections.length);
        expect(resolved!.content.faq.length).toBe(article.faq.length);
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
    for (const locale of LOCALES) {
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
      expect(JSON.stringify(kimi), locale).not.toMatch(/incremental SSE|инкрементальн.*SSE|增量 SSE|증분 SSE/i);
    }
  });
});
