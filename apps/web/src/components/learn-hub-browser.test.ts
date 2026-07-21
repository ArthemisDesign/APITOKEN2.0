import { describe, expect, it } from "vitest";
import { filterHubArticles } from "./learn-hub-browser";

const articles = [
  { slug: "buy", cluster: "buy" as const, title: "Buy an API key", description: "Account setup", href: "/buy" },
  { slug: "cursor", cluster: "integrate" as const, title: "Use Claude in Cursor", description: "Editor integration", href: "/cursor" },
  { slug: "cost", cluster: "explain" as const, title: "Token pricing", description: "Understand API cost", href: "/cost" },
];

describe("Learn hub filtering", () => {
  it("searches titles and descriptions without case sensitivity", () => {
    expect(filterHubArticles(articles, "CURSOR", "all").map((article) => article.slug)).toEqual(["cursor"]);
    expect(filterHubArticles(articles, "account", "all").map((article) => article.slug)).toEqual(["buy"]);
  });

  it("combines the query and topic filters", () => {
    expect(filterHubArticles(articles, "api", "explain").map((article) => article.slug)).toEqual(["cost"]);
    expect(filterHubArticles(articles, "cursor", "buy")).toEqual([]);
  });
});
