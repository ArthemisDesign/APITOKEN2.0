import type { LearnArticle } from "../learn";
import { cta, BASE } from "../learn-shared";

export const article: LearnArticle = {
  slug: "apitoken-vs-proxyapi",
  cluster: "compare",
  title: "apiToken.sale vs ProxyAPI for Claude",
  h1: "apiToken.sale vs ProxyAPI",
  description: "Comparing Claude API resellers: apiToken.sale offers a native Anthropic endpoint with a flat 50% discount, card or crypto payment, and one key for every model.",
  keywords: ["proxyapi alternative", "apitoken vs proxyapi", "claude api reseller", "proxyapi claude", "claude api без proxyapi", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
  dek: "Both let you reach Claude without an Anthropic account. The differences are in how you pay, how much you save, and whether the endpoint is truly Anthropic-native.",
  sections: [
    { h2: "Native Anthropic endpoint", blocks: [
      { type: "p", text: `apiToken.sale exposes the standard Anthropic Messages API at ${BASE}, so Claude Code, Cursor and the Anthropic SDKs work unchanged — no adapter layer between you and Claude.` },
    ] },
    { h2: "Discount, not markup", blocks: [
      { type: "list", items: [
        "Flat 50% B2C discount off official Claude spend.",
        "One prepaid key and balance for Opus, Sonnet and Haiku.",
        "Card or cryptocurrency top-ups that never expire.",
      ] },
      cta(),
    ] },
    { h2: "When each fits", blocks: [
      { type: "list", items: [
        "apiToken.sale — a native Anthropic endpoint with a flat discount, lifetime key spending limits and optional expiration.",
        "A generic reseller — may suit you if you already use its other providers.",
        "Both remove the Anthropic-account barrier; the difference is price and how native the Claude access is.",
      ] },
    ] },
  ],
  faq: [
    { q: "Is apiToken.sale cheaper than a standard reseller?", a: "It applies a flat 50% discount to official Claude spend rather than adding a markup on top of list prices." },
    { q: "Do my Anthropic tools still work?", a: "Yes — it is the native Anthropic Messages API, so Claude Code, Cursor and the SDKs need only a base-URL change." },
  ],
  related: ["apitoken-vs-anthropic-direct", "apitoken-vs-openrouter", "cheapest-claude-api", "claude-api-for-russia"],
};
