import { describe, expect, it } from "vitest";
import { buildZenFeed } from "./app/zen.xml/route";

// Дзен принимает ленту только при ≥10 материалах, полном тексте от 300 знаков и
// ограниченном наборе HTML-тегов (без <table>/<pre>/<code>): см. комментарий в роуте.
describe("zen feed", () => {
  const feed = buildZenFeed();
  const items = feed.split("<item>").slice(1);

  it("has enough items for Dzen onboarding", () => {
    expect(items.length).toBeGreaterThanOrEqual(10);
  });

  it("every item carries the required elements", () => {
    for (const item of items) {
      expect(item).toContain("<title>");
      expect(item).toContain("<link>https://apitoken.sale/ru/docs/learn/");
      expect(item).toContain('<guid isPermaLink="false">');
      expect(item).toContain("<pubDate>");
      expect(item).toContain("<content:encoded><![CDATA[");
      expect(item).toContain('type="image/png"');
    }
  });

  it("body is full-text and uses only Dzen-supported tags", () => {
    for (const item of items) {
      const body = item.match(/<!\[CDATA\[([\s\S]*?)\]\]>/)?.[1] ?? "";
      expect(body.length).toBeGreaterThanOrEqual(300);
      expect(body).not.toContain("<table");
      expect(body).not.toContain("<pre");
      expect(body).not.toContain("<code");
      // Ссылки в теле должны быть абсолютными, иначе они мертвы на Дзене.
      expect(body).not.toMatch(/href="\//);
    }
  });

  it("declares the noindex/format categories", () => {
    expect(feed).toContain("<category>format-article</category>");
    expect(feed).toContain("<category>noindex</category>");
  });
});
