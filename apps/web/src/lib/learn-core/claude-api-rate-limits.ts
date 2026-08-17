import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-rate-limits",
  cluster: "explain",
  title: "Claude API Rate Limits",
  h1: "Understanding Claude API rate limits",
  description: "What a 429 means on apiToken.sale, how Claude API rate limits measure requests and tokens per minute, and how to retry with Retry-After and exponential backoff.",
  keywords: ["claude api rate limits", "claude api 429", "anthropic rate limit", "claude api rate limit exceeded", "claude api requests per minute", "claude api tokens per minute", "claude api retry-after header", "claude api exponential backoff", "fix claude api 429 error", "claude api throughput", "anthropic rate_limit_error"],
  dek: "Claude API rate limits are per-minute ceilings on requests and tokens, and hitting one returns HTTP 429 instead of a completion. This guide shows how to read that response, build a retry policy that respects Retry-After, and tell throughput limits apart from the spending guardrails on your apiToken.sale key.",
  sections: [
    { h2: "What a Claude API rate limit actually is", blocks: [
      { type: "p", text: "Claude API rate limits are throughput ceilings: how many requests and how many tokens your account may push through per minute. Exceed one and the API answers with HTTP 429 instead of a completion. apiToken.sale does not publish a fixed RPM table — a 429 there signals gateway or upstream capacity, and the durable fix is disciplined retries plus lower concurrency, not a bigger number in a config file." },
      { type: "p", text: "On Anthropic's own API the ceilings are measured three ways: requests per minute, input tokens per minute and output tokens per minute, tracked per organization. Going direct, those ceilings rise through usage tiers as your cumulative spend grows. All three counters reset every minute, which is why a thirty-second burst can fail while your hourly average looks trivial." },
    ] },
    { h2: "Reading a 429 response", blocks: [
      { type: "p", text: "A well-built client treats 429 as data, not failure. The body carries an error of type rate_limit_error, and the response usually includes a retry-after header with the number of seconds the server wants you to wait." },
      { type: "code", code: `curl -i ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 64,\n    "messages": [{"role":"user","content":"hi"}]\n  }'\n\n# when throttled:\n# HTTP/2 429\n# retry-after: 17\n# {"type":"error","error":{"type":"rate_limit_error","message":"..."}}` },
      { type: "note", text: "retry-after is a hint, not a contract, and not every 429 carries one. When the header is missing, fall back to exponential backoff with jitter — and never retry a 429 in a tight loop, which only deepens the congestion that caused it." },
    ] },
    { h2: "A retry policy that survives production", blocks: [
      { type: "steps", items: [
        "Queue bursts instead of firing them: each worker sends one request at a time.",
        "On 429, read retry-after and sleep at least that many seconds.",
        "When the header is absent, sleep base × 2^attempt plus random jitter, capped at ~30 seconds.",
        "Stop after 4–6 attempts and fail the job loudly instead of retrying forever.",
        "Log the model, wait time and attempt count so you have a pattern to show support if 429s persist.",
      ] },
      { type: "code", code: `const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));\n\nasync function callClaude(body: unknown): Promise<Response> {\n  for (let attempt = 0; attempt < 5; attempt++) {\n    const res = await fetch("${BASE}/v1/messages", {\n      method: "POST",\n      headers: {\n        "x-api-key": process.env.APITOKEN_KEY!,\n        "anthropic-version": "2023-06-01",\n        "content-type": "application/json",\n      },\n      body: JSON.stringify(body),\n    });\n    if (res.status !== 429 && res.status < 500) return res;\n    const retryAfter = Number(res.headers.get("retry-after"));\n    const wait = retryAfter > 0\n      ? retryAfter * 1000\n      : Math.min(1000 * 2 ** attempt, 30_000) * (0.5 + Math.random() / 2);\n    await sleep(wait);\n  }\n  throw new Error("Claude API still rate-limited after 5 attempts");\n}` },
    ] },
    { h2: "Why bursts trip limits before your averages do", blocks: [
      { type: "p", text: "Because the counters are per-minute, concurrency is the real lever. Fifty parallel calls at the top of a minute can exhaust the request budget even if you send nothing for the next hour. Token counters amplify this: every concurrent long generation keeps consuming output-token budget while it runs, so ten simultaneous 4,000-token answers put far more pressure on the limit than ten quick ones." },
      { type: "p", text: "Streaming changes none of the accounting. A streamed call is still one request, metered and billed by the same input and output tokens as a non-streamed one — it only lets you render tokens sooner and abort early when an agent has what it needs." },
    ] },
    { h2: "Throughput limits are not spending limits", blocks: [
      { type: "p", text: "Two systems get confused here. A rate limit is a traffic shaper: transient, per-minute, resolved by waiting. A spending guardrail is a budget brake: it decides how much a key may ever spend. On apiToken.sale the dashboard does not configure request throughput at all — the per-key guardrails it offers are an optional lifetime spending limit and an expiration date. A 429 says slow down; it says nothing about your balance, and topping up will not clear it." },
      { type: "table", headers: ["", "Throughput limit", "Key spending guardrail"], rows: [
        ["What it caps", "Requests and tokens per minute", "Total lifetime spend on one key"],
        ["How it appears", "HTTP 429 with rate_limit_error", "Key stops spending at its set limit"],
        ["Where it lives", "Gateway and upstream capacity", "Your apiToken.sale dashboard, per key"],
        ["The right response", "Retry-After, backoff, less concurrency", "Raise or remove the limit deliberately"],
      ] },
    ] },
    { h2: "Lowering 429 pressure without raising the limit", blocks: [
      { type: "list", items: [
        "Stagger cron and batch jobs with random offsets so they do not stampede the same minute.",
        "Cap worker concurrency and let a queue absorb bursts.",
        "Trim context so each request carries fewer input tokens.",
        "Cap max_tokens to what the response actually needs.",
        "Cache large, stable context with prompt caching to cut billed input cost on repeats.",
      ] },
      { type: "p", text: "Most 429 storms are self-inflicted: a retry loop without jitter, a deploy that doubles workers, a scheduled job fanning out at :00. Fix the shape of the traffic before shopping for a higher ceiling." },
    ] },
    { h2: "When 429s become a capacity conversation", blocks: [
      { type: "p", text: "If smoothed traffic and correct retries still produce regular 429s at your target load, that is a capacity question, not a code question. Contact support with the model you use, your target requests and tokens per minute, and the shape of the workload — sustained higher throughput is handled as an account conversation, not a self-serve slider." },
      cta(),
    ] },
  ],
  faq: [
    { q: "What are the rate limits for the Claude API?", a: "On Anthropic direct, limits are requests per minute plus input and output tokens per minute per organization, rising through usage tiers with cumulative spend. apiToken.sale publishes no fixed RPM table; a 429 there reflects gateway or upstream capacity and is handled with Retry-After and backoff." },
    { q: "How do I fix a Claude API 429 error?", a: "Honor the retry-after header when present, otherwise back off exponentially with jitter, and cut concurrency. If 429s persist at production load after that, contact support about sustained higher throughput." },
    { q: "Does a 429 rate limit error cost money?", a: "A request rejected with 429 fails before generation, so it produces no tokens and no usage to meter. Only completed calls draw down your prepaid balance." },
    { q: "Does streaming use more of my rate limit?", a: "No. A streamed response is a single request and is metered and billed identically to a non-streamed one; streaming only changes when you see the tokens." },
    { q: "Can I set a requests-per-minute limit on my apiToken.sale key?", a: "No. Request throughput is not a per-key setting. The dashboard's per-key guardrails are an optional lifetime spending limit and an expiration date." },
    { q: "What is the Retry-After header in the Claude API?", a: "It is the number of seconds the server suggests waiting before your next attempt. Treat it as a minimum; when it is absent, use exponential backoff with jitter instead." },
  ],
  related: ["claude-api-best-practices", "claude-api-streaming", "how-billing-works", "claude-api-key-security"],
  updated: "2026-08-17",
};
