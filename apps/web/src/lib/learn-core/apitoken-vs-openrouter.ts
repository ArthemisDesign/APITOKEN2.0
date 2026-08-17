import type { LearnArticle } from "../learn";
import { cta, BASE } from "../learn-shared";

export const article: LearnArticle = {
  slug: "apitoken-vs-openrouter",
  cluster: "compare",
  title: "apiToken.sale vs OpenRouter for Claude",
  h1: "apiToken.sale vs OpenRouter for Claude",
  description: "Choosing a Claude gateway? Compare apiToken.sale and OpenRouter: a native Anthropic endpoint and prepaid discount vs a multi-provider router.",
  keywords: ["openrouter alternative", "apitoken vs openrouter", "claude api gateway", "openrouter claude", "best claude api gateway", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
  dek: "Both let you reach Claude without an Anthropic account, but they are built differently. If Claude is your main model, a native Anthropic endpoint keeps things simple.",
  sections: [
    { h2: "Native Anthropic endpoint", blocks: [
      { type: "p", text: `apiToken.sale exposes the standard Anthropic Messages API at ${BASE}, so Claude Code, Cursor and the Anthropic SDKs work with zero adapters. You are not routing through a generic multi-provider abstraction.` },
    ] },
    { h2: "Prepaid discount, not markup", blocks: [
      { type: "list", items: [
        "Flat 50% B2C discount off official Claude spend.",
        "One key and balance for Opus, Sonnet and Haiku.",
        "Card or crypto top-ups that never expire.",
      ] },
      cta(),
    ] },
    { h2: "When to choose each", blocks: [
      { type: "list", items: [
        "apiToken.sale — Claude is your main model and you want a native Anthropic endpoint with a discount.",
        "OpenRouter — you need to route across many providers behind one abstraction.",
        "Both let you start without an Anthropic account; only apiToken.sale discounts Claude spend directly.",
      ] },
    ] },
  ],
  faq: [
    { q: "Why pick a Claude-native gateway?", a: "If Claude is your primary model, a native Anthropic endpoint means your existing Anthropic tools and SDKs work unchanged." },
    { q: "Does apiToken.sale mark prices up?", a: "No — it applies a discount to official Claude spend rather than adding a markup." },
  ],
  related: ["apitoken-vs-anthropic-direct", "cheapest-claude-api", "claude-api-quick-setup", "anthropic-sdk-base-url"],
};
