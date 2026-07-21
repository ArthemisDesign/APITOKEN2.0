import { describe, expect, it } from "vitest";
import {
  articlesForLocale,
  learnArticles,
  learnArticlesBySlug,
  LOCALES,
  renderLearnMarkdown,
  resolveArticle,
} from "./learn";

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
      expect(flatten(resolved)).toContain("https://api.apitoken.sale");
      expect(flatten(resolved)).toContain("claude-opus-4-8");
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
});
