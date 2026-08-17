import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-pricing-explained",
  cluster: "explain",
  title: "Claude API Pricing Explained",
  h1: "How Claude API pricing works",
  description: "Understand Claude API pricing: per-token input and output rates, prompt caching, and how apiToken.sale applies a flat 50% discount.",
  keywords: ["claude api pricing", "claude api cost per token", "claude token pricing", "claude api cost", "anthropic api pricing explained", "how much does claude api cost", "claude opus price per million tokens", "claude sonnet api price", "claude api prompt caching cost", "how claude api pricing works"],
  dek: "Claude API pricing is per-token metering with separate input and output rates, cheaper cache reads, and no per-request fee. This guide walks through the exact arithmetic — token counts, model rate cards, caching and thinking — and shows where apiToken.sale's flat 50% discount enters the calculation.",
  sections: [
    { h2: "What you actually pay for: input and output tokens", blocks: [
      { type: "p", text: "Claude API pricing is pure per-token metering. Every request is billed for the tokens you send in — prompt, system instructions, conversation history, tool definitions — and the tokens the model generates back. Input and output carry separate rates, output costs more, and there is no per-request fee, seat license or minimum spend. Multiply the two token counts by the model's two rates and you have the exact cost of a call." },
      { type: "p", text: "A token is roughly three quarters of an English word, about four characters. Code, JSON and non-English text tokenize denser, so a source file costs more tokens than the same number of words in prose. Output is priced higher for a mechanical reason: input is processed in parallel in one pass, while every output token requires its own forward pass through the model." },
      { type: "p", text: "You never have to estimate any of this. The Messages API reports exactly what a call consumed in the usage object of its response — and on streaming requests, in the terminal event:" },
      { type: "code", code: `"usage": {
  "input_tokens": 12480,
  "cache_read_input_tokens": 0,
  "output_tokens": 1523
}` },
      { type: "p", text: "Take Claude Sonnet 5 at $3 per million input tokens and $15 per million output. The call above costs 12,480 × $3/M + 1,523 × $15/M ≈ $0.0374 + $0.0228 ≈ $0.06 at official rates. That is arithmetic on authoritative numbers, not a guess — which is why the usage object, not the character count of your prompt, is the source of truth for budgeting." },
    ] },
    { h2: "Claude API token pricing by model", blocks: [
      { type: "p", text: "Anthropic splits the lineup into three tiers. Opus is the premium tier for hard reasoning and long refactors, Sonnet is the balanced default for everyday coding, and Haiku is the cheapest for high-volume, low-complexity work. Rates scale with capability — there is a five-to-one spread between Opus 4.8 and Haiku 4.5 on input — so routing each task to the weakest model that handles it well is the single biggest lever on your bill." },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (−50%)"], rows: [
        ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
      ] },
      { type: "p", text: "The third column is the same table after the apiToken.sale discount: your flat 50% B2C rate applies to every model, so the ranking never changes but each line costs half. Agentic coding loops feel this most — they chain dozens of turns, and each turn resends the accumulated context as fresh input, so per-token rates compound quickly into real money." },
      { type: "link", text: "Per-model pages with cache rates and context windows", href: "/models" },
      { type: "link", text: "Estimate your monthly spend in the free cost calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "What prompt caching, thinking and streaming do to the bill", blocks: [
      { type: "p", text: "Prompt caching meters cache writes and cache reads separately from fresh input. On Anthropic's official rate card a cache write costs slightly more than fresh input, while a cache read costs about a tenth of it. Long stable prefixes — system prompts, tool definitions, large reference files — are what you cache, and on repeated calls they become nearly free." },
      { type: "list", items: [
        "Thinking tokens are billed as output. Extended thinking generates reasoning you may never see in the reply, and all of it lands in the output bucket at the full output rate.",
        "Streaming and non-streaming requests are billed identically. Server-sent events change when you see the tokens, not what they cost.",
        "A cache read is only cheap if it hits. Editing the middle of a cached prefix invalidates everything from the edit point onward, and the next call pays full input price again.",
      ] },
      { type: "note", text: "Cache entries live about five minutes by default, and each read refreshes the timer; a longer one-hour TTL is available at a higher write price. Bursty traffic with gaps longer than the TTL pays the write premium repeatedly without harvesting cheap reads — batch related calls together, or accept that the first call of each session re-warms the cache at full input cost." },
    ] },
    { h2: "Where the flat 50% discount enters the math", blocks: [
      { type: "p", text: "apiToken.sale changes none of the mechanics above. Your request hits the same Anthropic Messages API, the same model IDs answer it, and the usage object reports the same token counts. What changes is settlement: each call is converted to official Anthropic spend first, then your flat 50% B2C discount is subtracted before anything touches your prepaid balance. There is no subscription and no markup — the discount is the pricing." },
      { type: "p", text: "The worked example from the first section makes it concrete: that $0.06 Sonnet 5 call settles at $0.03. The same prepaid balance also meters supported GPT, Gemini and Kimi models against their own official rate cards, with the same discount applied." },
      { type: "p", text: "Every request appears in your dashboard with token-level detail — model, input, output and cache buckets — so you can reconcile the arithmetic in this guide against your real traffic instead of trusting an end-of-month invoice." },
      cta(),
    ] },
  ],
  faq: [
    { q: "How is the Claude API priced?", a: "Per token, split into input and output at separate rates, with cheaper cache reads billed as their own bucket. Larger models cost more per token — from Haiku 4.5 at $1/$5 per million up to Opus 4.8 at $5/$25." },
    { q: "How much does the Claude API cost per million tokens?", a: "Official rates run from $1 input / $5 output on Claude Haiku 4.5 to $5 / $25 on Claude Opus 4.8, with Sonnet at $3 / $15. On apiToken.sale every one of those numbers is halved by the flat 50% B2C discount." },
    { q: "How does the apiToken.sale discount apply to Claude API pricing?", a: "Each request is first converted to official Anthropic spend using its real token counts, then the flat 50% discount is subtracted and the net amount is drawn from your prepaid balance. The same mechanics cover cache reads and thinking tokens." },
  ],
  related: ["cheapest-claude-api", "save-tokens-on-claude-api", "how-billing-works", "apitoken-vs-anthropic-direct"],
  updated: "2026-08-17",
};
