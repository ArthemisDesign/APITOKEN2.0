import { describe, expect, it } from "vitest";
import { learnArticles, learnArticlesBySlug, renderLearnMarkdown } from "./learn";

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
    const md = renderLearnMarkdown(learnArticles[0]!, "https://apitoken.sale");
    expect(md).toContain("---");
    expect(md).toContain(`# ${learnArticles[0]!.h1}`);
    expect(md).toContain("## Frequently asked questions");
  });
});
