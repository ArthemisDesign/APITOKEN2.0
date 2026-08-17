import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-pricing-explained",
  cluster: "explain",
  title: "Claude API Pricing Explained",
  h1: "How Claude API pricing works",
  description: "Understand Claude API pricing: per-token input and output rates, prompt caching, and how apiToken.sale applies a flat 50% discount.",
  keywords: ["claude api pricing", "claude api tokens", "claude token pricing", "claude api cost", "how claude api pricing works", "anthropic api pricing explained", "how claude api works", "claude api explained", "anthropic api"],
  dek: "Claude is billed per token — separately for input and output — with discounts for cached content. apiToken.sale keeps those mechanics identical and layers a discount on top.",
  sections: [
    { h2: "Tokens, input and output", blocks: [
      { type: "p", text: "Every request is metered by tokens in (your prompt and context) and tokens out (the model's reply). Output tokens usually cost more than input, and larger models cost more per token." },
    ] },
    { h2: "Caching and thinking", blocks: [
      { type: "list", items: [
        "Cache writes and cache reads are metered separately, and cache reads are much cheaper.",
        "Thinking tokens count toward output on reasoning-heavy calls.",
        "Streaming and non-streaming requests are billed the same way.",
      ] },
    ] },
    { h2: "The apiToken.sale discount", blocks: [
      { type: "p", text: "Each call is converted to official Anthropic spend, then your discount is subtracted: B2C takes a flat 50% off every request. Every request is visible in your dashboard with token-level detail." },
      cta(),
    ] },
    { h2: "Claude API token pricing by model", blocks: [
      { type: "p", text: "Larger models cost more per token: Opus is the premium tier, Sonnet is the balanced default, and Haiku is the cheapest. Your discount applies to all of them, so the ranking stays the same but every price is lower." },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (\u221250%)"], rows: [
        ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
      ] },
      { type: "link", text: "Per-model pages with cache rates and context windows", href: "/models" },
    ] },
  ],
  faq: [
    { q: "How is the Claude API priced?", a: "Per token, split into input and output, with separate cheaper rates for cache reads. Larger models cost more per token." },
    { q: "How does the discount apply?", a: "Official spend is calculated first, then your flat 50% B2C discount is subtracted before it touches your balance." },
    { q: "How are Claude API tokens priced?", a: "Per token, split into input and output, with cheaper cache reads. apiToken.sale applies your flat 50% discount on top of the official token rates." },
  ],
  related: ["cheapest-claude-api", "save-tokens-on-claude-api", "how-billing-works", "apitoken-vs-anthropic-direct"],
  updated: "2026-07-17",
};
