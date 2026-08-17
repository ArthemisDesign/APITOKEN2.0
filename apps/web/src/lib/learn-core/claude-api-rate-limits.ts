import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-rate-limits",
  cluster: "explain",
  title: "Claude API Rate Limits",
  h1: "Understanding Claude API rate limits",
  description: "What a 429 means on apiToken.sale, how to handle it with Retry-After and backoff, and how key spending guardrails differ from throughput limits.",
  keywords: ["claude api rate limits", "claude api 429", "anthropic rate limit", "claude api throughput", "claude api retry", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
  dek: "Rate limits keep the gateway stable and your balance safe. Handling them well means smoother tools and no wasted spend.",
  sections: [
    { h2: "Traffic limits and spending guardrails", blocks: [
      { type: "p", text: "apiToken.sale does not publish a fixed RPM table. A 429 can represent a gateway or upstream capacity limit. The dashboard does not configure request throughput: its per-key guardrails are an optional lifetime spending limit and expiration date." },
    ] },
    { h2: "Handling a 429", blocks: [
      { type: "list", items: [
        "Respect the Retry-After header and back off exponentially.",
        "Reduce concurrency rather than hammering the endpoint.",
        "Contact support if you need sustained higher throughput.",
      ] },
      cta(),
    ] },
  ],
  faq: [
    { q: "What are the Claude API rate limits?", a: "apiToken.sale does not publish a fixed RPM number. If you receive a 429, honor Retry-After, back off and reduce concurrency; contact support when you need sustained higher throughput." },
    { q: "What should I do on a 429?", a: "Respect Retry-After, back off, and reduce concurrency; contact support for sustained higher limits." },
  ],
  related: ["claude-api-best-practices", "claude-api-streaming", "how-billing-works", "claude-api-key-security"],
};
