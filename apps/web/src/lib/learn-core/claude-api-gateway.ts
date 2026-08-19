import type { LearnArticle } from "../learn";
import { cta, BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-gateway",
  cluster: "explain",
  title: "What Is a Claude API Gateway?",
  h1: "What a Claude API gateway is, and when you need one",
  description: "What a Claude API gateway does: native Anthropic Messages API, per-key spending limits, and a flat 50% B2C discount — no Anthropic account needed.",
  keywords: ["claude api gateway", "what is an api gateway", "anthropic api gateway", "claude api proxy", "claude gateway vs proxy", "claude api access layer", "how claude api works", "claude api explained", "claude api pricing", "anthropic api"],
  dek: "A Claude API gateway accepts standard Anthropic Messages API requests on one side and forwards them to the model provider on the other, adding authentication, billing and key management in between. Your tools cannot tell the difference — but your invoice can. Here is how the layer works and how to judge one.",
  sections: [
    { h2: "What a Claude API gateway actually does", blocks: [
      { type: "p", text: "A Claude API gateway is a service that speaks the Anthropic Messages API to your code and relays each request to the model upstream. Your tools point at the gateway exactly as they would point at api.anthropic.com; the gateway owns everything around the request — who is calling, what it costs, and which key is allowed to spend. You use one for practical reasons: a lower price, instant access without a provider account, or controls the provider does not offer." },
      { type: "list", items: [
        "Presents the standard Anthropic Messages API, so SDKs and tools work unchanged.",
        "Authenticates your key and enforces per-key guardrails before anything goes upstream.",
        "Meters every request at official provider rates and handles billing — here, a prepaid balance at a flat 50% B2C discount.",
        "Records per-request usage with a token breakdown you can audit in a dashboard.",
      ] },
    ] },
    { h2: "Gateway vs proxy vs going direct", blocks: [
      { type: "p", text: "People use \"gateway\" and \"proxy\" interchangeably, and the difference matters when you pick one. A reverse proxy forwards bytes and understands nothing about them. A gateway understands the protocol and owns part of the request lifecycle — authentication, metering and settlement. Going direct means Anthropic does all of that for you, at official rates, with an Anthropic account." },
      { type: "table", headers: ["Approach", "Protocol", "Billing", "Key controls"], rows: [
        ["Anthropic direct", "Native Messages API", "Official per-token rates, billed by Anthropic", "Standard console keys"],
        ["Plain reverse proxy", "Whatever the proxy happens to pass through", "None of its own — billing stays upstream", "None"],
        ["apiToken.sale gateway", "Native Messages API, unchanged", "Prepaid balance at a flat 50% below official B2C spend", "Optional lifetime spending limit and expiration date per key"],
      ] },
      { type: "p", text: "The practical test: if deleting the middle layer changes nothing but the base URL, it was a proxy. If you would lose your billing, your limits and your usage history with it, it was a gateway." },
    ] },
    { h2: "How a request moves through the gateway", blocks: [
      { type: "steps", items: [
        `Your client sends a standard Messages API request to ${BASE}/v1/messages with your key in the x-api-key header.`,
        "The gateway authenticates the key and checks its guardrails — the optional lifetime spending limit and expiration date — before routing anything upstream.",
        "The model generates the answer. Streaming requests come back as standard Anthropic SSE events, token by token.",
        "Usage is metered at official provider rates, your flat 50% B2C discount is subtracted, and the net amount draws down your prepaid balance.",
        "The request shows up in your dashboard with its model and token-level breakdown, so spend is never a surprise.",
      ] },
      { type: "note", text: "Pointing an existing client at a gateway should mean changing two things: the base URL and the key. Keep your anthropic-version header and your model IDs. A service that demands a different request shape is a translation layer, not a native gateway — expect subtle breakage in streaming, tool use and prompt caching." },
    ] },
    { h2: "Native protocol, not a translation layer", blocks: [
      { type: "p", text: `apiToken.sale is Anthropic-native: any client that works against api.anthropic.com works against ${BASE}/v1/messages, byte for byte. A minimal request looks exactly like the Anthropic docs say it should:` },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "messages": [{"role": "user", "content": "Hello"}]\n  }'` },
      { type: "p", text: `The same key is not limited to the Anthropic lane. It also speaks the OpenAI-compatible protocol at ${OPENAI_BASE} with an Authorization: Bearer header, and the native Gemini protocol with x-goog-api-key — so one gateway key covers supported Claude, GPT, Gemini and Kimi models without a second account.` },
    ] },
    { h2: "Where the 50% discount comes from", blocks: [
      { type: "p", text: "The gateway buys capacity in bulk and sells it from a prepaid, pooled balance. Each of your calls is first converted to official Anthropic spend — input, output, cache reads and writes metered separately — then the flat 50% B2C discount is subtracted, and only the net amount touches your balance. The balance never expires and there is no customer subscription, so idle time costs nothing." },
      { type: "p", text: "Because metering mirrors the official rate card, the usual levers still work: cache reads are far cheaper than fresh input, and Haiku costs a fraction of Opus per token. The discount stacks on top of whatever your prompt engineering already saves." },
      { type: "link", text: "Estimate a workload before you top up", href: "/tools/claude-api-cost-calculator" },
      { type: "link", text: "Per-model rates, context windows and cache pricing", href: "/models" },
    ] },
    { h2: "Key-level controls direct access does not give you", blocks: [
      { type: "list", items: [
        "An optional lifetime spending limit per key — a hard ceiling, useful for keys embedded in tools or handed to a team.",
        "An optional expiration date, so a temporary key dies on schedule instead of living forever in someone's config.",
        "Per-request usage visibility in the dashboard, broken down by model and token bucket.",
        "Instant issuance with no Anthropic account, waitlist or billing-country requirement — payable by bank card or cryptocurrency.",
      ] },
      cta(),
    ] },
    { h2: "When you do not need a gateway", blocks: [
      { type: "p", text: "Be honest about the trade. If you already have frictionless Anthropic billing, an enterprise agreement, or a compliance requirement to contract with the model provider directly, going direct is the right call — a gateway adds a party to your request path, and that party has to be trustworthy. If what you want is the same models at half the official spend, instant onboarding, and prepaid predictability, a native gateway is the pragmatic choice." },
    ] },
  ],
  faq: [
    { q: "Does a Claude API gateway change the API or the models?", a: "A native gateway changes neither. It speaks the standard Anthropic Messages API and serves the same model IDs, so your SDK, streaming, tool use and prompt caching behave exactly as they do against api.anthropic.com." },
    { q: "Is a Claude API gateway the same thing as a proxy?", a: "No. A proxy only forwards traffic, while a gateway understands the protocol and adds authentication, per-token metering, billing and key controls such as a lifetime spending limit and an expiration date." },
    { q: "Why use a Claude gateway instead of Anthropic directly?", a: "For a flat 50% B2C discount off official spend, instant access without an Anthropic account or waitlist, card or crypto payment, and optional per-key guardrails. The API surface stays identical." },
    { q: "Can one gateway key also call GPT, Gemini and Kimi?", a: "Yes. The same apiToken.sale key works across supported Claude, GPT, Gemini and Kimi models — Anthropic Messages with x-api-key, OpenAI-compatible with Authorization: Bearer, or native Gemini with x-goog-api-key." },
    { q: "Will my existing Anthropic SDK code work through a gateway?", a: "Yes. Set the SDK's base URL to https://router.apitoken.sale, swap in your gateway key, and keep your model IDs and message code unchanged." },
  ],
  related: ["apitoken-vs-anthropic-direct", "claude-api-key-security", "cheapest-claude-api", "anthropic-sdk-base-url"],
  updated: "2026-08-17",
};
