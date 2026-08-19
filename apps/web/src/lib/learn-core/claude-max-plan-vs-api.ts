import type { LearnArticle } from "../learn";
import { cta, BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-max-plan-vs-api",
  cluster: "compare",
  title: "Claude Max Plan vs the Claude API",
  h1: "Claude Max subscription vs the API",
  description: "Claude Max plan vs the Claude API: what the $100/$200 subscription buys, where it stops, and when pay-as-you-go tokens at 50% off are the better deal.",
  keywords: ["claude max plan", "claude max vs api", "claude subscription vs api", "is claude max worth it", "claude max plan price", "claude max usage limits", "claude api pay as you go", "claude code without max plan", "claude without subscription", "cheap claude api", "claude api tokens"],
  dek: "The Claude Max plan is Anthropic's top subscription tier, built for people who live inside claude.ai and Claude Code all day. The Claude API is the metered version of the same models, built for software and for anyone whose usage is spiky. This guide compares the Claude Max plan against pay-as-you-go API billing so you can pick the one that matches how you actually work.",
  sections: [
    { h2: "What a Claude Max plan actually includes", blocks: [
      { type: "p", text: "Claude Max is a subscription for Anthropic's own products, not a developer product. It sits above the $20/month Pro plan in two tiers — $100/month and $200/month — and buys you a much larger interactive usage allowance in claude.ai, the desktop and mobile apps, and Claude Code signed in with your account. The allowance is governed by session-based limits that reset on a rolling five-hour window, with weekly caps on top during heavy use. Hit the ceiling and you wait for the window to reset." },
      { type: "p", text: "The critical detail most comparisons skip: a Max subscription includes zero API usage. Anthropic bills the subscription and the API as two separate systems. Not one token of Messages API credit comes with the $200 tier." },
    ] },
    { h2: "The jobs a subscription cannot do", blocks: [
      { type: "p", text: "A subscription authenticates you as a person inside Anthropic's apps. It cannot authenticate your software. There is no API key attached to a Max seat, so anything that expects a key simply cannot use it." },
      { type: "list", items: [
        "Production backends and SaaS features that call the Messages API from your own code.",
        "CI pipelines, batch jobs, cron-driven agents and anything that runs unattended.",
        "Third-party tools that ask for an API key: Cursor, VS Code agents, Continue, Aider, LangChain, LiteLLM.",
        "Programmatic control over system prompts, temperature, tool use and structured output at scale.",
        "Team or service usage — a Max plan is a single-user seat, not infrastructure.",
      ] },
      { type: "note", text: "If your end goal is Claude Code specifically, you do not need Max for that either — Claude Code runs fine on an API key and bills per token instead of against a session allowance." },
    ] },
    { h2: "How the API meters the same models", blocks: [
      { type: "p", text: "The API has no monthly fee and no session windows. Every request is metered by tokens in (your prompt and context) and tokens out (the model's reply), with cache reads priced far below fresh input and cache writes metered separately. Streaming responses are billed identically to non-streaming ones. You get the same frontier models — Opus, Sonnet, Haiku — with exact, auditable per-request usage instead of an opaque allowance gauge." },
      { type: "p", text: "The consequence for budgeting is simple: idle time costs nothing. A quiet week where you make three API calls costs three calls, not a share of a $200 subscription." },
    ] },
    { h2: "Running the break-even numbers", blocks: [
      { type: "p", text: "The honest comparison is usage shape, not sticker price. Max wins for sustained, heavy interactive work — hours of Claude Code every weekday, long claude.ai sessions — where a per-token bill could exceed $200. The API wins for everything bursty, programmatic or mixed across tools, because there is no floor and no cap: you pay for the tokens you burn and nothing else." },
      { type: "p", text: "On apiToken.sale the math shifts further toward the API. Every request is metered at official Anthropic rates, then a flat 50% B2C discount is subtracted before it touches your balance. That means $200 of prepaid balance covers $400 of official API spend — twice the token budget of a $200 Max month, with no expiry and no reset window." },
      { type: "table", headers: ["Your usage pattern", "Better fit"], rows: [
        ["Hours of interactive Claude Code every weekday", "Claude Max can make sense"],
        ["A few coding sessions a week, uneven schedule", "Pay-as-you-go API"],
        ["Agents, CI, scripts or a production app", "API — a subscription cannot do this"],
        ["Cursor, Continue, Aider or other key-based tools", "API key required"],
        ["Claude plus GPT, Gemini or Kimi in the same project", "One prepaid multi-provider balance"],
      ] },
      { type: "link", text: "Estimate your monthly token spend before you commit", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "Claude Code on API balance, no Max required", blocks: [
      { type: "p", text: "Pointing Claude Code at a pay-as-you-go key takes two environment variables. Every feature stays intact — the only thing that changes is billing, from a subscription allowance to per-token usage on your prepaid balance." },
      { type: "code", code: `export ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# then just run\nclaude` },
      { type: "p", text: `The same key also speaks the OpenAI-compatible protocol at ${OPENAI_BASE} with an Authorization: Bearer header, so the tools in your stack that expect OpenAI-shaped endpoints work without a second account.` },
      { type: "link", text: "Full walkthrough: Claude Code without a subscription", href: "/docs/learn/claude-code-without-subscription" },
    ] },
    { h2: "Half the token price, one key for four providers", blocks: [
      { type: "p", text: "apiToken.sale sells prepaid, pay-as-you-go access to the identical Anthropic Messages API at a flat 50% off official token rates. The balance never expires, there is no monthly charge, and every request shows up in your dashboard with a token-level breakdown — input, output and cache legs — so you always know where the money went." },
      { type: "list", items: [
        "One key covers supported Claude, GPT, Gemini and Kimi models — no per-provider accounts.",
        "Anthropic Messages protocol with x-api-key, OpenAI-compatible with Bearer, or native Gemini with x-goog-api-key.",
        "Top up only when you need to; unused balance simply waits.",
      ] },
      { type: "link", text: "Per-model pricing with cache rates", href: "/models" },
      cta(),
    ] },
  ],
  faq: [
    { q: "Does the Claude Max plan include API access?", a: "No. Claude Max is a subscription for Anthropic's own apps and Claude Code; the API is billed separately and no subscription tier bundles API tokens." },
    { q: "Is Claude Max worth it compared to the API?", a: "For heavy daily interactive use in Claude Code and claude.ai, the $100 or $200 tier can beat per-token billing. For bursty, programmatic or multi-tool usage, pay-as-you-go API access — especially at apiToken.sale's flat 50% discount — is almost always cheaper." },
    { q: "Can I use Claude Code without a Max or Pro subscription?", a: "Yes. Set ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY to point at a prepaid API key and Claude Code works identically, billed per token against your balance." },
    { q: "What happens when I hit Claude Max usage limits?", a: "You are throttled until your session window resets — limits operate on rolling five-hour windows with additional weekly caps. On API billing there is no session allowance; usage is bounded by your balance, not a timer." },
    { q: "Is $200 of API credit worth more than a $200 Max month?", a: "On apiToken.sale, yes: at a flat 50% off official rates, $200 of prepaid balance covers $400 of official Anthropic spend, never expires, and works in any tool that accepts an API key." },
    { q: "Can one API key serve Claude and other model providers?", a: "Yes — one apiToken.sale key works across supported Claude, GPT, Gemini and Kimi models, drawing from a single prepaid balance." },
  ],
  related: ["claude-code-without-subscription", "claude-api-pricing-explained", "cheapest-claude-api", "how-billing-works"],
  updated: "2026-08-17",
};
