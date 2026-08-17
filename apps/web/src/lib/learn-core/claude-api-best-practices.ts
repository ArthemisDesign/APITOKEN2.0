import type { LearnArticle } from "../learn";
import { BASE, KEY, cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-best-practices",
  cluster: "explain",
  title: "Claude API Best Practices",
  h1: "Claude API best practices",
  description: "Practical best practices for the Claude API on apiToken.sale: model choice, prompt caching, streaming, lifetime key spending limits, expiration, and secure key handling.",
  keywords: ["claude api best practices", "claude api production checklist", "anthropic api best practices", "claude api model routing", "claude api prompt caching", "claude api streaming", "claude api 429 error handling", "claude api key management", "reduce claude api cost", "claude api tips"],
  dek: "Claude API best practices come down to two levers: how many tokens you send and which model burns them. This guide covers model routing, prompt caching, streaming, retry discipline and per-key guardrails — the habits that keep a production integration fast, cheap and safe on apiToken.sale.",
  sections: [
    { h2: "Match the model to the task, not the other way around", blocks: [
      { type: "p", text: "The most reliable Claude API best practice is to stop sending every request to the strongest model. Route each call to the cheapest model that can actually do the work, cache the context you resend, stream anything a human is waiting on, and retry failures with backoff instead of tight loops. Everything below is the working detail behind those moves." },
      { type: "table", headers: ["Workload", "Model to start with", "Why"], rows: [
        ["High-volume classification, extraction, quick edits", "claude-haiku-4-5", "Fastest and cheapest per token; quality is more than enough for narrow tasks"],
        ["Everyday coding, chat, agent loops", "claude-sonnet-5", "The default workhorse — strong reasoning at a mid-tier price"],
        ["Hard refactors, architecture, long ambiguous sessions", "claude-opus-4-8", "Top of the lineup; reserve it for tasks where Sonnet visibly struggles"],
      ] },
      { type: "p", text: "On apiToken.sale every supported model shares one API key and one prepaid balance, so routing is a one-line change of the model ID in the request — no extra accounts, no per-model billing setup. A flat 50% B2C discount applies across providers regardless of which model a task lands on, which makes downshifting to Haiku or Sonnet a pure win." },
      { type: "p", text: "Escalate deliberately, not by default. A common pattern in agents: run the loop on claude-sonnet-5, detect failure signals (repeated tool errors, self-corrections going in circles), and re-issue only that step against claude-opus-4-8. You pay Opus prices on the few steps that need it instead of the whole session." },
      { type: "link", text: "Model selection for coding workloads, compared in depth", href: "/docs/learn/best-claude-model-for-coding" },
    ] },
    { h2: "Cache the context you resend on every call", blocks: [
      { type: "p", text: "If your requests carry a large stable prefix — a long system prompt, tool definitions, a codebase digest, few-shot examples — prompt caching is the single biggest cost lever after model choice. Mark the reusable blocks with cache_control and the API stores them: cache writes cost a small premium over fresh input, but subsequent cache reads cost a fraction of fresh input tokens." },
      { type: "code", code: `curl ${BASE}/v1/messages \\
  -H "x-api-key: ${KEY}" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{
    "model": "claude-sonnet-5",
    "max_tokens": 1024,
    "system": [
      {"type": "text", "text": "...20k tokens of stable instructions...",
       "cache_control": {"type": "ephemeral"}}
    ],
    "messages": [{"role": "user", "content": "Summarize ticket #4821"}]
  }'` },
      { type: "p", text: "Two rules make or break cache hit rates. First, the cached prefix must be byte-identical between calls — even a timestamp injected at the top of the system prompt invalidates everything after it, so put volatile content at the end. Second, the default ephemeral cache lives only a few minutes and refreshes on each hit, so it rewards chatty workloads: agents, chat sessions, batch jobs that reuse one context." },
      { type: "note", text: "Caching is not free storage. A one-off request over a long document pays the cache-write premium with no reads to amortize it. Cache only what you will resend at least twice inside the TTL window." },
    ] },
    { h2: "Stream anything a human is waiting on", blocks: [
      { type: "p", text: "Set stream: true and the API returns tokens over server-sent events as they are generated instead of one blocking response. Streaming costs the same tokens as a buffered call, but the perceived latency drops from \"the whole answer\" to \"the first token\", which is often a second or less. For chat UIs this is the difference between a spinner and a reply that feels instant." },
      { type: "p", text: "Streaming matters just as much for agents. Reading events as they arrive lets you start parsing a tool call the moment its JSON block closes, surface progress to the user, and abort early when the output is clearly going wrong — a killed stream means you stop paying for output tokens you were going to discard anyway." },
      { type: "note", text: "With streaming, the authoritative token usage arrives in the final message_delta event, not up front. Always read the terminal usage before logging cost or updating budgets — never estimate from character counts." },
    ] },
    { h2: "Retry 429s and 5xx with backoff, never in tight loops", blocks: [
      { type: "p", text: "apiToken.sale does not publish a fixed requests-per-minute table: a 429 signals a gateway or upstream capacity limit, and the correct response is patience, not pressure. Honor the Retry-After header when it is present, otherwise retry with exponential backoff plus random jitter, and lower client-side concurrency before raising request rates." },
      { type: "steps", items: [
        "Catch the error and classify it. Retry 429 and 5xx only; a 400, 401 or 403 will fail identically forever, so fix the request or the key instead of retrying.",
        "Wait for the Retry-After interval if the header exists; otherwise wait roughly 1s, then 2s, 4s, 8s — doubling each attempt with a random jitter so parallel workers do not retry in lockstep.",
        "Cap attempts (three to five is typical) and then fail the task visibly. Silent infinite retries burn balance and hide outages.",
        "If 429s persist at your normal load, reduce concurrency and contact support about sustained higher throughput rather than engineering around it.",
      ] },
      { type: "link", text: "Rate limits, Retry-After and throughput on apiToken.sale", href: "/docs/learn/claude-api-rate-limits" },
    ] },
    { h2: "One key per environment, guardrails switched on", blocks: [
      { type: "p", text: "Create a separate, clearly named key for each environment or application — prod-backend, staging-ci, local-dev — instead of sharing one key everywhere. When a key leaks, you revoke exactly that key and the rest of your fleet keeps running; with a shared key, one leak means an emergency rotation of every client at once." },
      { type: "p", text: "The dashboard offers two per-key guardrails, and both are worth setting: an optional lifetime spending limit, which caps the total a key can ever draw from your balance, and an expiration date, after which the key simply stops working. Size the lifetime limit to what that environment should legitimately consume, and give short-lived projects short-lived keys." },
      { type: "list", items: [
        "Keep keys in a secret manager or environment variables — never in source control, client-side code or tickets.",
        "Treat any key that touched a public place (a commit, a log line, a screenshot) as compromised: revoke first, investigate after.",
        "Cap max_tokens per request to what the response actually needs, so a runaway prompt cannot inflate a single call.",
      ] },
      { type: "link", text: "The full key-hygiene playbook", href: "/docs/learn/claude-api-key-security" },
    ] },
    { h2: "Audit the token breakdown, not just the balance", blocks: [
      { type: "p", text: "Every request in your apiToken.sale dashboard is itemized by model, provider and token bucket — input, output and cache legs. Review that breakdown weekly. Cost regressions almost always show up there first: input tokens creeping up because someone started resending full history, output tokens ballooning because max_tokens was raised \"just in case\", cache reads collapsing after a prompt reorder." },
      { type: "p", text: "The economics stack in your favor. Requests are metered at exact provider rates, then the flat 50% B2C discount is applied, and the net draws from a prepaid balance that never expires — so every token you eliminate through caching, routing and tighter context is a token you also never paid full price for. Token tactics reduce the count; the discount reduces the price; together they multiply." },
      { type: "link", text: "Estimate a workload before you run it with the cost calculator", href: "/tools/claude-api-cost-calculator" },
      cta(),
    ] },
  ],
  faq: [
    { q: "What are the most important Claude API best practices?", a: "Route each task to the cheapest capable model, cache large stable context with cache_control, stream user-facing responses, retry 429/5xx with Retry-After and exponential backoff, and use one key per environment with a lifetime spending limit and expiration date." },
    { q: "Which Claude model should I use by default?", a: "Start everyday coding and chat on claude-sonnet-5, push high-volume simple work to claude-haiku-4-5, and reserve claude-opus-4-8 for tasks where Sonnet visibly struggles. On apiToken.sale all three share one key and balance, so switching is a one-line model-ID change." },
    { q: "How do I reduce Claude API costs in production?", a: "Cache repeated context (cache reads cost a fraction of fresh input), downshift easy tasks to cheaper models, cap max_tokens, and review the token-level usage breakdown weekly. On apiToken.sale these tactics stack with the flat 50% B2C discount." },
    { q: "What should I do when the Claude API returns a 429?", a: "Honor the Retry-After header, otherwise retry with exponential backoff and jitter, and reduce concurrency. Never retry 4xx errors like 400 or 401 — fix the request or key instead. For sustained higher throughput, contact support." },
    { q: "Does streaming a response cost more tokens?", a: "No. stream: true delivers the same tokens incrementally over server-sent events; the terminal message_delta event carries the authoritative usage. You pay for generated tokens either way — streaming only changes when you see them." },
    { q: "How should I store and manage Claude API keys?", a: "Keep keys in a secret manager or environment variables, never in git or client code. Create a named key per environment, set its lifetime spending limit and expiration date in the dashboard, and revoke immediately if a key is exposed." },
  ],
  related: ["save-tokens-on-claude-api", "claude-api-rate-limits", "claude-api-key-security", "best-claude-model-for-coding"],
  updated: "2026-08-17",
};
