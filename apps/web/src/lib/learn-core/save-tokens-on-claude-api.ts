import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "save-tokens-on-claude-api",
  cluster: "explain",
  title: "How to Save Tokens on the Claude API",
  h1: "How to save tokens on the Claude API",
  description: "How to save tokens on the Claude API: prompt caching, per-task model routing and tighter context — practical tactics that stack with a flat 50% off.",
  keywords: ["save tokens claude api", "reduce claude api cost", "claude prompt caching", "claude api optimization", "lower claude api bill", "claude api token usage", "claude api cost optimization", "cheapest claude model for task", "claude max_tokens", "anthropic api cache control"],
  dek: "Saving tokens on the Claude API comes down to three levers: send fewer input tokens, generate fewer output tokens, and pay less per token through model choice and prompt caching. Each lever is a concrete change to the requests you already send — and all of them stack with the apiToken.sale discount, which cuts the price side of the equation in half.",
  sections: [
    { h2: "Where Claude API tokens actually go", blocks: [
      { type: "p", text: "Your Claude API bill is input tokens plus output tokens, metered separately per model. Two facts in the rate card tell you where to optimize: output tokens cost five times more than input on every current Claude model, and a multi-turn conversation resends its entire history as fresh input on every call. So the biggest savings come from generating less output, resending less context, and letting cheaper token classes — cache reads and smaller models — do more of the work." },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (−50%)"], rows: [
        ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
      ] },
      { type: "p", text: "Read that table as a routing table, not just a price list. Haiku is five times cheaper than Opus per token at both input and output, and the flat 50% B2C discount halves every row without changing the ranking — so model choice saves the same proportion here as it does against official pricing." },
    ] },
    { h2: "Cache the context you resend on every request", blocks: [
      { type: "p", text: "Prompt caching is the single largest token saver for anything with a stable, repeated prefix: long system prompts, tool definitions, large reference files. You mark the end of the stable block with a cache_control breakpoint; the first call writes the cache (metered separately), and subsequent calls read it back at a fraction of the fresh-input price. A cache entry lives about five minutes, and every read refreshes it, so an active session keeps its cache warm indefinitely." },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "system": [\n      {\n        "type": "text",\n        "text": "<your long, stable system prompt>",\n        "cache_control": {"type": "ephemeral"}\n      }\n    ],\n    "messages": [{"role":"user","content":"Refactor the parser."}]\n  }'` },
      { type: "note", text: "Cache matching is prefix-exact: change one byte early in the prompt and everything after that point is re-billed as fresh input. Put volatile content — timestamps, user state, the current question — after the cached breakpoint, never before it." },
    ] },
    { h2: "Route each request to the cheapest model that can handle it", blocks: [
      { type: "p", text: "Sending every request to Opus is the most expensive habit in Claude API usage. Most production traffic — classification, extraction, formatting, autocomplete-style edits, tool-result parsing — does not need frontier reasoning, and paying Opus rates for it is pure waste. Match the model to the task and the per-token spread does the saving for you." },
      { type: "steps", items: [
        "Default new workloads to claude-sonnet-5 — the balanced tier for everyday coding and writing.",
        "Push high-volume or mechanical work down to claude-haiku-4-5: tagging, summarizing short inputs, schema conversion, simple Q&A.",
        "Escalate instead of defaulting: run the cheap model first and retry on claude-opus-4-8 only when the answer fails your checks.",
        "Inside agent loops, keep planning on the strong model but run parsing and formatting steps on Haiku.",
      ] },
      { type: "p", text: "The classic pattern is a cascade: Haiku attempts the request, a cheap validation decides whether the output is acceptable, and only failures climb to Sonnet or Opus. You pay frontier prices only for the small share of traffic that genuinely needs it." },
    ] },
    { h2: "Send less context, ask for less output", blocks: [
      { type: "p", text: "Every file, message and tool definition in the request is billed again on every call, and every token of the reply is billed at the 5× output rate. Trimming both sides is unglamorous but immediate — it requires no platform features, just discipline about what goes into the request." },
      { type: "list", items: [
        "Send only the files and history a task actually needs; a targeted excerpt beats a whole-repo dump.",
        "Summarize long threads into a running brief instead of resending the full transcript each turn.",
        "Drop tool definitions the current step cannot call — they are billed as input on every turn.",
        "Cap max_tokens to what the response really requires; a runaway completion is billed to the last token.",
        "Ask for a diff or patch instead of a full file rewrite, and for JSON when you parse the reply anyway — prose padding is billed at output rates.",
      ] },
      { type: "p", text: "Because output costs five times input, trimming 1,000 output tokens saves as much as trimming 5,000 input tokens. When in doubt, shorten the answer you ask for before you shorten the context you send." },
    ] },
    { h2: "Read the usage object before you tune anything", blocks: [
      { type: "p", text: "Every Messages API response ends with exact accounting in its usage field. Log these numbers per feature or endpoint before optimizing — guesses about where tokens go are usually wrong, and the usage object turns optimization into arithmetic." },
      { type: "code", code: `"usage": {\n  "input_tokens": 1520,\n  "output_tokens": 212,\n  "cache_creation_input_tokens": 8134,\n  "cache_read_input_tokens": 0\n}` },
      { type: "p", text: "On a warm cache, most of your input should move into cache_read_input_tokens while plain input_tokens collapses to the new question alone. The apiToken.sale dashboard shows the same token-level breakdown for every request, so you can watch the shift happen after each change instead of waiting for the monthly bill." },
      { type: "link", text: "Estimate your monthly spend in the free calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "Token discipline and the discount compound", blocks: [
      { type: "p", text: "Billing on apiToken.sale runs in a fixed order: each call is converted to official Anthropic spend from its exact usage components — input, output, cache write, cache read — then the flat 50% B2C discount is subtracted, and the net amount is drawn from your prepaid balance. Caching and routing shrink the official spend; the discount halves whatever remains. The two multiply, so a workload that cuts its token count in half effectively costs a quarter of the official price." },
      { type: "p", text: "The balance itself never expires and top-ups accept any whole-dollar amount, so there is no subscription clock pressuring you to burn tokens you would rather save. Current per-model rates, including cache pricing, are on the models page." },
      { type: "link", text: "Current model lineup and per-model pricing", href: "/models" },
      cta(),
    ] },
  ],
  faq: [
    { q: "What is the single biggest way to save tokens on the Claude API?", a: "Prompt caching for large, repeated context — system prompts, files, tool definitions — combined with routing each task to the cheapest model that can do it. Cache reads cost a fraction of fresh input tokens, and Haiku is five times cheaper per token than Opus." },
    { q: "How do I see how many tokens a Claude API request used?", a: "Every response carries a usage object with input_tokens, output_tokens, cache_creation_input_tokens and cache_read_input_tokens. The apiToken.sale dashboard shows the same per-request token breakdown." },
    { q: "Which Claude model should I use to save money?", a: "Default to claude-sonnet-5 for everyday work, drop high-volume mechanical tasks to claude-haiku-4-5 ($1/$5 per 1M officially, $0.50/$2.50 with the discount), and reserve claude-opus-4-8 for genuinely hard reasoning." },
    { q: "Does setting max_tokens lower my Claude API bill?", a: "You pay for output tokens actually generated, so a tight max_tokens cap prevents runaway completions from billing to the limit. If a reply ends with stop_reason: max_tokens, the cap cut the answer off — raise it deliberately rather than retrying the same request." },
    { q: "Do these token-saving tactics stack with the apiToken.sale discount?", a: "Yes. Caching and model routing reduce the number of tokens billed at official rates, then the flat 50% B2C discount halves what remains — the savings multiply." },
  ],
  related: ["claude-api-pricing-explained", "cheapest-claude-api", "claude-haiku-api", "claude-opus-vs-sonnet"],
  updated: "2026-08-17",
};
