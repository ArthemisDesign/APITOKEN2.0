import type { LearnArticle } from "../learn";
import { BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "free-claude-api-key",
  cluster: "free",
  title: "Free Claude API Key — $5 Credit, No Card Required",
  h1: "Get a free Claude API key in minutes",
  description: "Get a free Claude API key with $5 of platform bonus credit: sign up with Google or GitHub, no card and no Anthropic account required, and call every supported Claude model.",
  keywords: ["free claude api key", "claude api free", "free claude api", "claude api free tier", "free anthropic api key", "claude api no card", "claude api no credit card", "claude api free credits", "try claude api free", "how to get a free claude api key"],
  dek: "A free Claude API key is one OAuth signup away: create an apiToken.sale account with Google or GitHub and $5 of platform bonus credit lands on your balance — no card, no Anthropic account, no waitlist. The key speaks the standard Anthropic Messages API from the first request, so existing tools work unmodified. Email and password signups get a working account but not the bonus.",
  sections: [
    { h2: "The short answer: free credit, not a free tier", blocks: [
      { type: "p", text: "You can hold a working free Claude API key about two minutes from now. Register with Google or GitHub, open the dashboard, generate a key, and the $5 platform welcome bonus is already on your balance — no payment details asked for at any point. Every supported Claude model is callable immediately, at the same endpoints paid balance uses." },
      { type: "p", text: "Understand what the offer is and is not. It is a one-time credit grant for evaluating the gateway with real traffic, not an ongoing free tier with a monthly refill. There is no sandbox mode and no reduced feature set: streaming, tool use and long context all behave exactly as they do for paying accounts, because the billing path is the only thing that differs." },
    ] },
    { h2: "Claim the bonus with Google or GitHub", blocks: [
      { type: "steps", items: [
        "Create the account with Google or GitHub OAuth. This signup route is what attaches the $5 welcome bonus — there is no approval queue, invite or manual review.",
        "Open the dashboard and generate an API key. It looks like sk-pool-…, and one key covers the full supported Claude line — Opus, Sonnet and Haiku — on a single balance.",
        `Pick the wire protocol your tool expects: the Anthropic Messages API at ${BASE} with an x-api-key header, or the OpenAI-compatible lane at ${OPENAI_BASE} with Authorization: Bearer.`,
      ] },
      { type: "note", text: "Signed up with email and password first? That account works but never receives the welcome bonus — the grant is tied to the OAuth signup method, not to being new. If your dashboard shows a zero bonus, sign out and register again through Google or GitHub instead of topping up out of frustration." },
    ] },
    { h2: "Prove the integration with a cheap first call", blocks: [
      { type: "p", text: "Do not burn the bonus on an Opus monologue. Validate the plumbing with the smallest useful request: Haiku, a short prompt, a hard max_tokens cap. A 200 response with a usage block confirms auth, routing and metering end to end." },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-haiku-4-5",\n    "max_tokens": 256,\n    "messages": [{"role":"user","content":"Reply with the word ok"}]\n  }'` },
      { type: "p", text: "Troubleshooting is mechanical. A 401 means the key or base URL is wrong. A 400 usually means a missing max_tokens or a mistyped model ID. An insufficient-balance error on a brand-new account almost always means the account was created with email and password, so no bonus was ever attached." },
    ] },
    { h2: "How far the free credit actually stretches", blocks: [
      { type: "p", text: "Every request is metered at official Anthropic token rates, then the flat 50% B2C discount is subtracted before the balance moves. That means the bonus buys roughly double what the same dollars would buy at list price." },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (−50%)", "≈ output tokens per $5"], rows: [
        ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50", "400K"],
        ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50", "670K"],
        ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50", "2M"],
      ] },
      { type: "p", text: "Read the last column as an evaluation budget, not a production one. Two million Haiku output tokens is enough to wire up your editor, replay a realistic workload and compare model quality on your own prompts. The same bonus spread across Opus runs dry in a single afternoon of agentic coding — which is itself useful information before you spend real money." },
      { type: "link", text: "Full per-model pricing, including cache rates", href: "/models" },
      { type: "link", text: "Estimate your monthly cost in the free calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "The same key also calls GPT, Gemini and Kimi", blocks: [
      { type: "p", text: `The bonus is valid on supported models from all four providers, and the key you already generated is the only credential you need. Claude runs on the Anthropic Messages lane; GPT models run on the OpenAI-compatible lane; Gemini answers natively at ${BASE} with an x-goog-api-key header; Kimi works on Anthropic Messages and through the universal OpenAI-compatible lane.` },
      { type: "p", text: "This is the underrated part of the free credit: it is a cross-provider benchmark budget. Run the same prompt set against Claude and its alternatives, measure quality on your tasks instead of vendor leaderboards, and only then decide where the paid balance goes." },
    ] },
    { h2: "When the credit runs out", blocks: [
      { type: "p", text: "Top up any whole-dollar amount — there is no fixed package catalog — by bank card or cryptocurrency through a secure checkout provider. The flat discount applies automatically to every subsequent request; nothing needs to be unlocked or negotiated." },
      { type: "p", text: "There is no subscription and no monthly minimum, and prepaid balance never expires, so an idle month costs exactly nothing. Treat the first top-up as a continuation of the evaluation: add a small amount, point one real project at the gateway, and scale only when the numbers justify it." },
    ] },
  ],
  faq: [
    { q: "Is the free Claude API key a sandbox or the real API?", a: "The real one. The $5 Google/GitHub bonus runs against the same supported models and endpoints as paid balance, including streaming and tool use — only the billing source differs." },
    { q: "Do I need a credit card to get a free Claude API key?", a: "No card is required at any point. Create the account with Google or GitHub and the $5 platform bonus appears without any payment details." },
    { q: "Why didn't I receive the $5 welcome bonus?", a: "Only accounts created through Google or GitHub OAuth receive it. Email and password registrations create a fully usable account but are not eligible for the grant." },
    { q: "Does the free credit or balance expire?", a: "The bonus is a one-time grant for new accounts rather than a refilling free tier, and prepaid balance never expires — there is no monthly fee eating it while you are idle." },
    { q: "Can I use the free key in Cursor, Claude Code or the Anthropic SDK?", a: `Yes. Any Anthropic-compatible client works: set the base URL to ${BASE}, send your key as x-api-key, and keep the anthropic-version header exactly as the official API expects.` },
    { q: "Which models can the free credit call?", a: "Every supported Claude model — Opus 4.8 and 4.7, Sonnet 5 and 4.6, Haiku 4.5 — plus the supported GPT, Gemini and Kimi lines on the same key and balance." },
  ],
  related: ["claude-api-free-trial", "how-to-buy-claude-api-key", "claude-code-without-subscription", "cheapest-claude-api"],
  updated: "2026-08-17",
};
