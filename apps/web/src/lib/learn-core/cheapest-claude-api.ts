import type { LearnArticle } from "../learn";
import { BASE, KEY, cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "cheapest-claude-api",
  cluster: "buy",
  title: "Cheapest Claude API — Flat 50% Discount",
  h1: "The cheapest way to use the Claude API",
  description: "The cheapest Claude API: the identical Anthropic Messages API at a flat 50% off official spend. Prepaid balance, no subscription, every Claude model discounted.",
  keywords: ["cheapest claude api", "claude api discount", "cheap claude api", "discounted claude api tokens", "claude api cheaper than anthropic", "claude api 50% off", "buy claude api credits", "claude api prepaid balance", "claude api pricing", "claude api reseller", "cheapest way to use claude api"],
  dek: "The cheapest Claude API is not a smaller model or a throttled clone — it is the same Anthropic Messages API billed at a flat 50% below official rates. apiToken.sale meters every request at Anthropic's published token prices, halves the result, and draws it from a prepaid balance that never expires. This page shows the discounted per-model prices, how the billing pipeline works, and how to point an existing client at it.",
  sections: [
    { h2: "The short answer: same API, half the bill", blocks: [
      { type: "p", text: "The cheapest way to use the Claude API is to buy the identical API at a flat 50% discount through apiToken.sale. You send the same request to the same Anthropic Messages API with the same model IDs and get the same response — the only thing that changes is what the call costs you. There is no cheaper substitute model, no markup dressed up as a discount, and no tier you have to unlock." },
      { type: "p", text: "The mechanics are deliberately boring. Each request is metered at official Anthropic token rates, exactly as if you had called Anthropic directly. Your flat 50% discount is then subtracted, and only the net amount is drawn from your prepaid balance. A call that would cost $0.20 at official rates draws $0.10." },
      { type: "list", items: [
        "B2C accounts get the flat 50% off official spend on every request — nothing to unlock, no volume threshold.",
        "The discount applies identically to input, output, and cache tokens, so the shape of your workload never changes the percentage.",
        "B2B volume pricing is negotiated separately from the public B2C rate.",
      ] },
    ] },
    { h2: "Claude API prices with the 50% discount applied", blocks: [
      { type: "p", text: "Anthropic prices Claude per million tokens, split into input and output, with larger models costing more per token. The discount preserves that ranking — Opus stays the premium tier, Sonnet the balanced default, Haiku the cheapest — and simply halves every number:" },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (\u221250%)"], rows: [
        ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
      ] },
      { type: "p", text: "Cache reads and cache writes follow Anthropic's own multipliers on top of these rates, and the 50% comes off after those are applied — so a cache-heavy workflow saves twice: once on the caching discount itself, once on the flat rate." },
      { type: "link", text: "Full per-model pricing, including cache rates", href: "/models" },
      { type: "link", text: "Estimate your monthly cost in the free calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "Which workloads feel the discount the most", blocks: [
      { type: "p", text: "Percentage discounts are flat, but absolute savings scale with token burn. Three workload shapes dominate real Claude bills:" },
      { type: "list", items: [
        "Agentic coding loops, where a single task fans out into dozens of tool-call round trips and each one resends the growing context.",
        "Long multi-turn sessions, where conversation history is re-billed as input on every turn.",
        "Cache-heavy pipelines, where a large stable system prompt or repository context is read from cache repeatedly — already cheap per call, and halved again here.",
      ] },
      { type: "p", text: "Model choice compounds the effect. Routing a high-volume task from Opus to Haiku cuts the per-token price roughly tenfold before the discount; the 50% then applies to whichever tier you picked. The cheapest Claude API call is a Haiku call on discounted balance — $0.50 per million input tokens." },
      { type: "note", text: "Tip: route quick, cheap work to Haiku and reserve Opus for hard reasoning to stretch your balance further." },
    ] },
    { h2: "Prepaid balance instead of a monthly subscription", blocks: [
      { type: "p", text: "There is no monthly fee and no plan to pick. You top up a prepaid balance that never expires, and it is consumed only when requests actually run — idle days, idle weeks, and abandoned side projects cost nothing. That matters for the most common real-world usage pattern: bursts of heavy agentic work separated by days of silence, which is exactly the pattern a flat monthly subscription punishes." },
      { type: "p", text: "Because the balance never expires, topping up during a heavy week is not a commitment. Whatever is left sits there until the next project, the next prototype, or the next late-night refactor." },
      cta(),
    ] },
    { h2: "Switch an existing client in two environment variables", blocks: [
      { type: "p", text: "Any tool built on the Anthropic SDK — Claude Code, Cursor, Continue, Aider, LangChain, LiteLLM — reads its endpoint and credential from the environment when no explicit override is set. Point both at apiToken.sale and the tool runs unchanged, billing your discounted balance instead:" },
      { type: "code", code: `export ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# your client now sends the same Messages API requests,\n# metered at official rates minus 50%` },
      { type: "p", text: "Model IDs, request bodies, streaming, tool use, and prompt-caching headers all behave exactly as they do against Anthropic directly. If a request works on the official API, the identical request works here — the difference appears only on your balance, not in your code." },
    ] },
    { h2: "What \"cheapest\" does not mean here", blocks: [
      { type: "p", text: "Cheap Claude access usually comes in three flavors, and it is worth knowing which one you are looking at. Repackaged smaller models are cheap because they are weaker — fine until the task gets hard. Shared or resold subscriptions are cheap until they hit rate limits or get revoked. The third flavor is what this page describes: the genuine Anthropic Messages API at full capability, made cheaper purely on the billing side by pooling prepaid balance and passing the discount through." },
      { type: "p", text: "The practical test is simple: if the model IDs, the response format, and the feature set (streaming, tool use, prompt caching) match Anthropic's documentation exactly, you are on the real API. Everything here does." },
    ] },
  ],
  faq: [
    { q: "Is the cheapest Claude API the same as buying from Anthropic?", a: "Yes — the same Anthropic Messages API, same model IDs, same request and response format. Each call is metered at official token rates, then a flat 50% is subtracted before it touches your balance." },
    { q: "How much cheaper is apiToken.sale than Anthropic direct?", a: "B2C pricing is a flat 50% below official API spend on every request, across all Claude models and token types. B2B volume pricing is negotiated separately." },
    { q: "What is the cheapest Claude model per token?", a: "Claude Haiku 4.5 at $1 / $5 per million input/output tokens officially — $0.50 / $2.50 after the flat 50% discount." },
    { q: "Is there a monthly fee or does prepaid balance expire?", a: "No monthly fee, and the prepaid balance never expires. It is consumed only by real API usage, so idle periods cost nothing." },
    { q: "Can I use the discounted Claude API with Claude Code, Cursor, or LangChain?", a: "Yes. Set ANTHROPIC_BASE_URL to https://router.apitoken.sale and ANTHROPIC_API_KEY to your key — any Anthropic SDK-based tool then works unchanged at the discounted rate." },
  ],
  related: ["claude-api-pricing-explained", "save-tokens-on-claude-api", "apitoken-vs-anthropic-direct", "how-billing-works"],
  updated: "2026-08-17",
};
