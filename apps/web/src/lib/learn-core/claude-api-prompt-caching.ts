import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-prompt-caching",
  cluster: "explain",
  title: "Prompt Caching on the Claude API",
  h1: "Claude API prompt caching: how it works and what it saves",
  description: "Claude prompt caching: cache reads cost 0.1× the input price (0.05× with the apiToken.sale discount). Breakpoints, TTLs and the pricing math.",
  keywords: ["claude prompt caching", "claude api cache", "anthropic prompt cache", "cache_control claude api", "claude cache read pricing", "claude api cache breakpoints", "anthropic messages api caching", "reduce claude api cost caching", "claude cache ttl", "claude api base url", "claude api key"],
  dek: "Claude prompt caching lets you mark stable context — system prompts, tool definitions, reference files — so repeat requests read it from cache at a fraction of the input price instead of paying full freight every call. This guide covers breakpoints, cache TTLs, the write-versus-read pricing math, and how cached usage appears on your apiToken.sale bill.",
  sections: [
    { h2: "What prompt caching does to a Claude API bill", blocks: [
      { type: "p", text: "Prompt caching tells Anthropic to store a reusable prefix of your request — anything up to a breakpoint you set — so the next request with the same prefix reads it from cache instead of reprocessing it. Cache reads cost a fraction of fresh input tokens, while cache writes cost a small premium over input. If your application resends the same large context (a system prompt, a codebase snapshot, a document set), caching converts the most expensive part of every call into the cheapest." },
      { type: "p", text: "Cache writes and cache reads are metered as separate token buckets in the API response and on your bill, so you can always see exactly what the cache earned you. Nothing about the response itself changes — same model, same quality, same streaming behavior." },
    ] },
    { h2: "Placing cache_control breakpoints in a request", blocks: [
      { type: "p", text: "You opt in per request by adding a cache_control marker to a content block. Everything before the marker — system prompt, tool definitions, earlier messages — becomes the cacheable prefix. Here is a real request against apiToken.sale with the system prompt and tools cached:" },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "system": [\n      {\n        "type": "text",\n        "text": "You are a senior reviewer... (long stable instructions)",\n        "cache_control": {"type": "ephemeral"}\n      }\n    ],\n    "messages": [{"role": "user", "content": "Review this diff: ..."}]\n  }'` },
      { type: "list", items: [
        "You can set up to four cache_control breakpoints per request — a common layout is one after tools, one after the system prompt, and one after a large reference document.",
        "Caching matches prefixes exactly, from the first token. Change one character in the system prompt and everything after it misses the cache.",
        "Only blocks above the minimum cacheable size are stored — about 1,024 tokens on Sonnet and Opus models, more on Haiku. Short prompts silently skip caching.",
        "Put volatile content (timestamps, user-specific data) after the last breakpoint, never inside the prefix.",
      ] },
    ] },
    { h2: "Cache write vs cache read pricing", blocks: [
      { type: "p", text: "Anthropic prices cache operations as multipliers of the model's input token rate. Writing is a one-time premium per cached block; reading is where the money comes back. On apiToken.sale the flat 50% B2C discount applies to every usage leg, cache legs included, after the official spend is computed." },
      { type: "table", headers: ["Usage leg", "Official rate (× input price)", "Effective here (−50%)"], rows: [
        ["Fresh input tokens", "1×", "0.5×"],
        ["Cache write, 5-minute TTL", "1.25×", "0.625×"],
        ["Cache write, 1-hour TTL", "2×", "1×"],
        ["Cache read", "0.1×", "0.05×"],
      ] },
      { type: "p", text: "The default cache entry lives five minutes and the timer resets on every hit, so an active session keeps its cache warm indefinitely. A one-hour TTL is available at a higher write cost for bursty workloads. A cache read costs one-tenth of fresh input — and one-twentieth once the discount lands — so a prefix read three times within its TTL is already cheaper than sending it fresh twice." },
      { type: "note", text: "Cache entries are not shared across accounts and never leak between apiToken.sale customers. Your cached prefix is only reusable by requests authenticated under the same upstream account context." },
    ] },
    { h2: "Workloads that hit the cache — and ones that never will", blocks: [
      { type: "list", items: [
        "Coding agents and IDE assistants that resend the same repo context, CLAUDE.md, and tool schemas with every turn.",
        "RAG pipelines querying a fixed document set — cache the corpus, vary only the question.",
        "Chatbots with long stable system prompts and few-shot example libraries.",
        "Batch jobs that classify or extract from many short items against one large instruction block.",
      ] },
      { type: "p", text: "Caching does nothing for one-off questions, prompts that change on every call, or prefixes below the minimum size. If each request is genuinely unique, you pay the write premium and never collect a read — measure before you blanket-enable it." },
    ] },
    { h2: "Confirming cache hits in the usage object", blocks: [
      { type: "p", text: "Every Messages API response reports the cache legs directly in its usage block. A warm cache looks like this:" },
      { type: "code", code: `"usage": {\n  "input_tokens": 38,\n  "cache_creation_input_tokens": 0,\n  "cache_read_input_tokens": 14802,\n  "output_tokens": 412\n}` },
      { type: "p", text: "Watch cache_read_input_tokens across requests: a healthy integration shows most of the context landing there after the first call, with cache_creation_input_tokens near zero until the TTL lapses. On apiToken.sale the same legs show up in your dashboard — every request is listed with model, provider and a token-level breakdown, and every cache line is visible in your usage detail, so the savings are auditable rather than implied." },
    ] },
    { h2: "Stacking the cache with the prepaid discount", blocks: [
      { type: "p", text: "Caching lowers the token count you pay full price for; the apiToken.sale discount lowers the price per token. They compound. Concrete math on Claude Sonnet 5 (official $3 per 1M input tokens): resending a 100,000-token context fresh costs $0.30 per call. Read from cache it costs $0.03, and after the flat 50% B2C discount the call's context leg lands at $0.015 — a 20× reduction on the part of the bill that used to dominate it." },
      { type: "p", text: "Billing stays prepaid and simple: one balance covers supported Claude, GPT, Gemini and Kimi models, each metered at its official rate card before the discount. Top up once, and a well-cached workload stretches the same balance far further than uncached traffic." },
      cta(),
      { type: "link", text: "Per-model input, output and cache rates", href: "/models" },
      { type: "link", text: "Model a cached workload in the Claude API cost calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
  ],
  faq: [
    { q: "How much cheaper are Claude cache reads?", a: "Cache reads are billed at 0.1× the model's input token price, while cache writes cost 1.25× (five-minute TTL) or 2× (one-hour TTL). On apiToken.sale the flat 50% B2C discount applies on top, bringing a cache read to 0.05× the list input price." },
    { q: "How long does the Claude prompt cache last?", a: "A cache entry lives five minutes by default, and every cache hit resets that timer, so an active session stays warm indefinitely. A one-hour TTL is available at a higher write rate for bursty traffic." },
    { q: "Why is my Claude prompt cache not hitting?", a: "The usual causes: the prefix changed (caching matches from the first token, so any edit invalidates everything after it), the block is below the minimum cacheable size (about 1,024 tokens on Sonnet and Opus models), the five-minute TTL lapsed between calls, or cache_control was placed on a block that varies per request." },
    { q: "Does prompt caching work through apiToken.sale?", a: "Yes. Send standard Messages API requests with cache_control to https://router.apitoken.sale/v1/messages using your sk-pool-… key in the x-api-key header. Cache creation and read legs are metered at Anthropic's official rates, then your discount is applied." },
    { q: "Do cached tokens still draw from my prepaid balance?", a: "Yes, but at cache rates: cache writes at 1.25–2× input and reads at 0.1× input, converted to official Anthropic spend and then reduced by your flat 50% B2C discount. Each request's cache legs are visible in the dashboard usage breakdown." },
  ],
  related: ["save-tokens-on-claude-api", "claude-api-pricing-explained", "cheapest-claude-api", "how-billing-works"],
  updated: "2026-08-17",
};
