import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-best-practices",
  cluster: "explain",
  title: "Claude API Best Practices",
  h1: "Claude API best practices",
  description: "Practical best practices for the Claude API on apiToken.sale: model choice, prompt caching, streaming, lifetime key spending limits, expiration, and secure key handling.",
  keywords: ["claude api best practices", "claude api tips", "claude api production", "claude api guidelines", "anthropic api best practices", "claude api key", "anthropic-compatible api", "claude api base url", "claude api setup", "claude api integration"],
  dek: "A short checklist to get reliable, economical results from the Claude API in production.",
  sections: [
    { h2: "The checklist", blocks: [
      { type: "list", items: [
        "Pick the cheapest model that can do each task; escalate only when needed.",
        "Cache large, stable context to slash input cost.",
        "Stream responses for responsive agents and UIs.",
        "Set an optional lifetime spending limit and expiration date on each key.",
        "Handle 429s with Retry-After and backoff.",
        "Watch the token-level usage breakdown to catch waste early.",
      ] },
      cta(),
    ] },
    { h2: "Keep costs and reliability in check", blocks: [
      { type: "list", items: [
        "Cap max_tokens to what each response actually needs.",
        "Retry 429/5xx with exponential backoff, not tight loops.",
        "Use separate, clearly named keys per environment so a leak can be revoked without replacing every client.",
        "Review token-level usage weekly to catch regressions early.",
      ] },
    ] },
  ],
  faq: [
    { q: "What is the most impactful best practice?", a: "Match the model to the task and cache repeated context — together they cut cost the most." },
    { q: "How do I keep keys safe?", a: "Store keys in a secret manager, set an appropriate lifetime spending limit and expiration date, and revoke a key immediately if it is exposed." },
  ],
  related: ["save-tokens-on-claude-api", "claude-api-rate-limits", "claude-api-key-security", "best-claude-model-for-coding"],
};
