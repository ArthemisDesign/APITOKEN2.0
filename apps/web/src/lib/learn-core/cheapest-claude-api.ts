import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "cheapest-claude-api",
  cluster: "buy",
  title: "Cheapest Claude API — Flat 50% Discount",
  h1: "The cheapest way to use the Claude API",
  description: "The cheapest Claude API: buy discounted Claude API tokens at a flat 50% off. apiToken.sale sells the identical Anthropic API from prepaid balance with a flat Claude API discount.",
  keywords: ["cheapest claude api", "claude api discount", "claude api tokens", "discounted claude api", "cheap claude api", "claude api cheaper than anthropic", "buy claude api", "claude api access", "claude api top up", "claude api reseller", "claude api provider"],
  dek: "The Claude API is priced per token, and those tokens add up fast on long coding sessions. apiToken.sale gives you the identical API for 50% less by pooling prepaid balance and applying a flat discount.",
  sections: [
    { h2: "Why it is cheaper", blocks: [
      { type: "p", text: "You send the same request to the same Anthropic Messages API and get the same response. The only thing under the hood is billing: each call is metered at official rates, then your discount is subtracted before it touches your balance." },
      { type: "list", items: [
        "B2C accounts get a flat 50% off official spend.",
        "The same flat rate applies to every request — nothing to unlock.",
        "B2B volume pricing is negotiated separately.",
      ] },
    ] },
    { h2: "Where the savings show up most", blocks: [
      { type: "p", text: "Agentic coding, long multi-turn sessions and prompt-cache-heavy workflows burn the most tokens — so they see the biggest absolute savings. Choosing the right model for each task compounds it further." },
      { type: "note", text: "Tip: route quick, cheap work to Haiku and reserve Opus for hard reasoning to stretch your balance further." },
    ] },
    { h2: "No subscription, no lock-in", blocks: [
      { type: "p", text: "There is no monthly fee. You top up prepaid balance that never expires and spend it only when requests run, so idle days cost nothing." },
      cta(),
    ] },
    { h2: "How the Claude API discount is applied", blocks: [
      { type: "p", text: "There is no markup and no separate cheaper model — you get discounted access to the exact same Claude API." },
      { type: "list", items: [
        "Each request is metered at official Anthropic token rates.",
        "Your flat 50% discount is subtracted.",
        "The net amount is drawn from your prepaid balance.",
      ] },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (\u221250%)"], rows: [
        ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
      ] },
      { type: "link", text: "Full per-model pricing, including cache rates", href: "/models" },
      { type: "link", text: "Estimate your monthly cost in the free calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
  ],
  faq: [
    { q: "Is this really the same Claude API?", a: "Yes — the same Anthropic Messages API, same model IDs, same request and response format. Only the price per call is lower." },
    { q: "How much can I save?", a: "B2C pricing is a flat 50% below official API spend on every request." },
    { q: "Are there hidden fees or subscriptions?", a: "No. Balance is prepaid, never expires, and is consumed only by real API usage — there is no monthly charge." },
    { q: "Is there a cheaper Claude API than buying from Anthropic directly?", a: "Yes. apiToken.sale sells the identical Anthropic API at a flat 50% off official spend, with no subscription." },
  ],
  related: ["claude-api-pricing-explained", "save-tokens-on-claude-api", "apitoken-vs-anthropic-direct", "how-billing-works"],
  updated: "2026-07-17",
};
