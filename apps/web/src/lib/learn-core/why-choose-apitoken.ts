import type { LearnArticle } from "../learn";
import { BASE, OPENAI_BASE, KEY, cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "why-choose-apitoken",
  cluster: "compare",
  title: "Why Choose apiToken.sale",
  h1: "Why choose apiToken.sale",
  description: "Why developers use one apiToken.sale key for Claude, GPT, Gemini and Kimi: native or compatible APIs, 50% off B2C pricing, and card or crypto payment.",
  keywords: ["why apitoken.sale", "multi provider api", "claude api discount", "gpt api discount", "gemini api discount", "kimi api key", "openai compatible api", "prepaid api balance", "one api key claude gpt gemini", "llm api gateway"],
  dek: "Why apiToken.sale exists: developers who use Claude, GPT, Gemini and Kimi end up juggling four billing accounts, four SDK configurations and four pricing pages. This service collapses that into one prepaid key at a flat 50% off official B2C spend — without flattening the four protocols into one. Below is exactly what stays native, what the discount applies to, and where the limits are.",
  sections: [
    { h2: "One key, four model families", blocks: [
      { type: "p", text: "apiToken.sale is an independent multi-provider API gateway: one key and one prepaid balance reach supported Claude, GPT, Gemini and Kimi models, with no separate Anthropic, OpenAI, Google Cloud or Kimi billing accounts. The key point most roundups miss is that the four families are not squeezed through a single translated API — each provider keeps the protocol its ecosystem already speaks. Streaming, tool use and prompt-caching semantics pass through in each provider's own event format, so client code that works against the official endpoint works here unchanged." },
      { type: "table", headers: ["Provider family", "Protocol served", "Auth header", "Example supported models"], rows: [
        ["Claude", "Anthropic Messages", "x-api-key", "claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5"],
        ["Kimi", "Anthropic Messages, plus the OpenAI-compatible lane", "x-api-key", "kimi/k3, kimi/kimi-for-coding"],
        ["GPT", "OpenAI-compatible", "Authorization: Bearer", "gpt-5.6-terra"],
        ["Gemini", "Native generateContent", "x-goog-api-key", "gemini-3.6-flash"],
      ] },
    ] },
    { h2: "Native protocols instead of a translation layer", blocks: [
      { type: "p", text: "Most multi-provider routers normalize everything into one lowest-common-denominator schema, and the seams show: tool-use payloads, streaming event types and cache controls behave differently after translation. Here the router terminates each protocol in its own shape, so an Anthropic SDK pointed at the gateway behaves as if it were talking to Anthropic, and a Google-shaped client keeps its generateContent routes." },
      { type: "code", code: `# Claude and Kimi — Anthropic Messages\ncurl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":"ping"}]}'\n\n# GPT — OpenAI-compatible\ncurl ${OPENAI_BASE}/chat/completions \\\n  -H "Authorization: Bearer ${KEY}" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"gpt-5.6-terra","messages":[{"role":"user","content":"ping"}]}'\n\n# Gemini — native generateContent\ncurl ${BASE}/v1beta/models/gemini-3.6-flash:generateContent \\\n  -H "x-goog-api-key: ${KEY}" \\\n  -H "content-type: application/json" \\\n  -d '{"contents":[{"parts":[{"text":"ping"}]}]}'` },
      { type: "note", text: "Kimi is the one family available on two lanes: Anthropic Messages for Claude-shaped tooling, and the universal OpenAI-compatible lane for clients that only speak OpenAI. Pick per client, not per account — the same key works on both." },
    ] },
    { h2: "What the 50% discount actually applies to", blocks: [
      { type: "p", text: "The pricing model is one sentence: every request is converted to official provider spend by its exact usage legs, then a flat 50% B2C discount is subtracted. The same rate covers supported models across all four providers — there is no per-provider tier to compare and no catalog of marked-up SKUs." },
      { type: "list", items: [
        "Metering uses the real usage legs of each call: input, output, cache, and any model-specific long-context or image buckets.",
        "The discount is applied after metering, so a 50% cut on official spend is a 50% cut on your actual traffic mix, not on a hypothetical list price.",
        "Charges draw from one prepaid balance in whole-dollar top-ups; the balance never expires and there is no customer subscription, so idle weeks cost nothing.",
      ] },
      { type: "link", text: "Per-model rates across all four providers", href: "/models" },
      { type: "link", text: "Estimate a workload before you top up", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "One balance instead of four billing accounts", blocks: [
      { type: "p", text: "Access is instant and self-serve: create an account, generate a key that looks like sk-pool-…, and it works on the next request. There is no waitlist, no manual review and no provider-side approval — which also removes the four separate sign-up, card-verification and billing-country hurdles that each provider imposes on its own." },
      { type: "p", text: "You top up any whole-dollar amount by bank card or cryptocurrency through a secure checkout provider. That matters twice: teams without a corporate card in a supported billing country can still pay, and crypto top-ups keep the balance funded where card rails are unreliable. If a payment needs to be reversed, refund handling goes through the original payment provider — support in English and Russian is reachable over Telegram when you need it." },
      cta(),
    ] },
    { h2: "Guardrails on the key, visibility in the dashboard", blocks: [
      { type: "p", text: "Each key can carry an optional lifetime spending limit and an expiration date — enough to hand a key to a contractor, a CI job or a side project without watching it daily. The dashboard shows token-level usage per request, broken down by model and provider, so the prepaid balance is auditable rather than a black box." },
      { type: "list", items: [
        "Lifetime spending limit per key: hard cap on cumulative spend, optional.",
        "Expiration date per key: the key stops working after a date you choose, optional.",
        "Token-level breakdown per request: input, output and cache legs, by model and provider.",
      ] },
    ] },
    { h2: "First request in under five minutes", blocks: [
      { type: "steps", items: [
        "Create a free account and generate a key in the dashboard. Sign up with Google or GitHub to start with $5 of platform bonus credit; email/password accounts do not receive the bonus.",
        `For Claude Code and Anthropic-shaped tools: export ANTHROPIC_BASE_URL=${BASE} and ANTHROPIC_API_KEY=${KEY}, then run the tool as usual.`,
        `For OpenAI-shaped clients (Cursor, Continue, Aider, LangChain, LiteLLM): set the base URL to ${OPENAI_BASE} and use the same key as the Bearer token.`,
        `For Gemini clients: keep the Google SDK shape and point it at ${BASE} with the key in x-goog-api-key.`,
        "Send one cheap request and confirm it in the dashboard's token-level usage before wiring the key into a real workload.",
      ] },
    ] },
    { h2: "Where apiToken.sale is not the right fit", blocks: [
      { type: "p", text: "The trade-offs are worth stating plainly. The gateway covers four provider families — if your workload needs a model outside the supported Claude, GPT, Gemini and Kimi lines, a general-purpose router is the better tool. And if your organization already holds enterprise agreements directly with a provider, negotiated terms on that contract may beat a flat B2C discount." },
      { type: "p", text: "For everyone else — solo developers, small teams, and anyone who wants Claude, GPT, Gemini and Kimi behind one key at half the official B2C price, paying by card or crypto without four billing accounts — this is the shortest path from zero to a working multi-provider setup." },
    ] },
  ],
  faq: [
    { q: "What makes apiToken.sale different from other API gateways?", a: "One key and balance cover four provider families at a flat 50% B2C discount, while each client keeps the appropriate native or compatible protocol — Anthropic Messages, OpenAI-compatible, or native Gemini generateContent — instead of a single translated schema." },
    { q: "Is every provider forced through one translated API?", a: "No. Claude and Kimi keep Anthropic Messages, GPT uses OpenAI-compatible routes, and Gemini keeps its native Google-shaped API. Kimi is additionally reachable through the universal OpenAI-compatible lane for clients that require it." },
    { q: "What is apiToken.sale?", a: "An independent multi-provider API gateway for discounted prepaid access to supported Claude, GPT, Gemini and Kimi models without separate provider billing accounts." },
    { q: "Can I try the service before paying?", a: "Yes. Accounts created with Google or GitHub start with $5 of platform bonus credit that works on supported models across all four providers; email/password accounts do not receive the bonus." },
    { q: "Does the prepaid balance expire or auto-renew?", a: "No. The balance never expires and there is no customer subscription — you top up a whole-dollar amount and it is spent only when API requests run." },
    { q: "Which tools work with an apiToken.sale key?", a: "Anything that speaks Anthropic Messages, the OpenAI API shape, or Gemini's generateContent: Claude Code, Cursor, Cline, Continue, Zed, Aider, LangChain, LiteLLM and the official provider SDKs, each pointed at the matching endpoint." },
  ],
  related: ["how-to-buy-claude-api-key", "how-to-buy-gpt-api-key", "how-to-buy-gemini-api-key", "how-to-buy-kimi-api-key"],
  updated: "2026-08-17",
};
