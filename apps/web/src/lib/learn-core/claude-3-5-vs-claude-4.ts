import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-3-5-vs-claude-4",
  cluster: "compare",
  title: "Claude 3.5 vs Claude 4 — What Changed and How to Migrate",
  h1: "Claude 3.5 vs Claude 4: what changed",
  description: "Claude 3.5 vs Claude 4: what actually improved, the model-ID mapping from claude-3-5-sonnet to the current line, per-token pricing, and the one-line migration on apiToken.sale.",
  keywords: ["claude 3.5 vs 4", "claude 4 vs 3.5", "migrate from claude 3.5", "claude-3-5-sonnet-20241022 replacement", "claude model migration", "claude sonnet 5 vs 3.5 sonnet", "claude 4 model ids", "claude 3.5 sonnet discontinued", "claude api pricing", "upgrade claude model"],
  dek: "Claude 3.5 vs Claude 4 is not a close call: the current line is stronger at agentic coding, reasoning and long-context consistency, and the Messages API it runs on is unchanged. The migration is a model-ID swap — this guide gives you the exact mapping, the price impact, and the things worth re-testing before you flip the string.",
  sections: [
    { h2: "What actually changed between 3.5 and the 4-series", blocks: [
      { type: "p", text: "The current Claude line beats 3.5 at the work most API users actually pay for: agentic coding, multi-step reasoning and staying coherent across long contexts. The API did not change — same Messages endpoint, same request and response shape, same headers — so the interesting question is not \"should I switch\" but \"which ID do I switch to\". The answer is below, and the edit itself is one line." },
      { type: "p", text: "Three improvements are concrete enough to plan around. First, agentic coding: tool use, multi-file edits and long autonomous runs fail noticeably less often than on 3.5 Sonnet, which is why the current models are the default in Claude Code and most coding agents. Second, context: the Opus and Sonnet line serves a 1M-token window at standard pricing, where the 3.5 generation topped out at 200K — long-document and large-repo workloads stop needing chunking workarounds. Third, reasoning control: current models support adaptive thinking with a configurable effort range, so you can buy more deliberation only on the requests that need it." },
      { type: "p", text: "Output style shifts too. The newer models write denser, more direct prose and follow formatting instructions more literally. That is usually an improvement, but prompts tuned against 3.5's habits deserve a re-run — more on that in the re-testing section." },
    ] },
    { h2: "Model ID mapping: where each 3.5 model lands", blocks: [
      { type: "p", text: "Anthropic retires older model IDs over time, and the current catalog — here and upstream — is the new generation. If your config still names a 3.5-era ID, this is the mapping:" },
      { type: "table", headers: ["3.5-era ID in your config", "Current replacement", "Previous-generation option"], rows: [
        ["claude-3-5-sonnet-20241022", "claude-sonnet-5", "claude-sonnet-4-6"],
        ["claude-3-5-haiku-20241022", "claude-haiku-4-5", "—"],
        ["claude-3-opus-20240229", "claude-opus-4-8", "claude-opus-4-7"],
      ] },
      { type: "p", text: "Default to the middle column. The right column exists for one situation: you have prompts or evals pinned to a specific generation and want a proven midpoint while you re-calibrate. The previous-generation options list at the same per-token price as the current ones, so there is no savings reason to stay on them — only a stability reason." },
    ] },
    { h2: "What the upgrade does to your token bill", blocks: [
      { type: "p", text: "At list price the move is close to neutral. Sonnet 5 lists at $3/$15 per 1M input/output tokens — exactly what 3.5 Sonnet cost — and Anthropic has run introductory pricing of $2/$10 on it through 2026-08-31. The Opus tier dropped hard: 3 Opus listed at $15/$75, so Opus 4.8 at $5/$25 is a third of what that tier used to cost. Haiku 4.5 lists slightly above the old 3.5 Haiku, but it is a far more capable model for the same slot in your architecture." },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (−50%)"], rows: [
        ["Claude Sonnet 5 / 4.6", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
      ] },
      { type: "p", text: "On apiToken.sale every request is converted to official Anthropic spend first, then the flat 50% B2C discount is subtracted before it touches your prepaid balance. The ranking between tiers stays the same; every row just lands cheaper than the 3.5-era official bill you were paying." },
      { type: "link", text: "Estimate your exact workload on the cost calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "The migration is a one-line diff", blocks: [
      { type: "p", text: "Because the wire protocol is identical, migrating is changing the value of one JSON field. Everything else — endpoint, headers, max_tokens, the messages array, the response's content blocks, stop_reason and usage — stays exactly as your code already handles it." },
      { type: "code", code: `# Before — Claude 3.5\ncurl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-3-5-sonnet-20241022","max_tokens":1024,"messages":[{"role":"user","content":"Hello"}]}'\n\n# After — only the model field changes\n  -d '{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":"Hello"}]}'` },
      { type: "p", text: "On apiToken.sale there is no credential work at all: the same sk-pool key and the same base URL already serve every supported Claude model, so the model ID is genuinely the whole change. If you configure models through an environment variable or a settings panel — ANTHROPIC_MODEL, a Cursor model field, a Continue config entry — update it there and redeploy." },
    ] },
    { h2: "Worth re-testing, not just re-pointing", blocks: [
      { type: "p", text: "A protocol-compatible swap is not a behavior-identical swap. Budget one eval pass before you promote the new ID to production:" },
      { type: "list", items: [
        "System prompts tuned on 3.5: the newer models follow instructions more literally, so workaround phrasing you added for 3.5 (\"remember to…\", repeated constraints) can now over-constrain the output. Run your prompt suite and delete the scaffolding that is no longer needed.",
        "Output length: current models tend to answer more thoroughly. If you trimmed max_tokens to keep 3.5 terse, check for stop_reason: max_tokens truncations after the switch.",
        "Thinking is opt-in: adaptive thinking changes both latency and token spend. Leave it off on latency-sensitive paths until you have measured it, and enable it deliberately on hard-reasoning paths.",
        "Agent loops: tool-use schemas are unchanged, but the new models call tools more eagerly and recover from tool errors differently. Watch one full agent run before trusting your loop guards.",
      ] },
      { type: "note", text: "If a prompt or eval suite is genuinely pinned to older behavior, move to the previous-generation ID (claude-sonnet-4-6 or claude-opus-4-7) as a stepping stone instead of jumping two generations at once — same price, smaller behavioral leap." },
    ] },
    { h2: "Cut over gradually — it is one string per request", blocks: [
      { type: "p", text: "There is no account-level migration to schedule, so you can move at whatever granularity suits you: per environment, per feature flag, per request. A common pattern is sending a fixed percentage of traffic to the new model ID while the old config stays intact, then comparing outputs and the dashboard's per-request token detail before committing. Because one key and one prepaid balance cover every supported Claude model — plus GPT, Gemini and Kimi on the same account — a gradual rollout costs you no extra credentials, plans or provider accounts." },
      { type: "link", text: "Current Claude lineup with per-model pricing", href: "/models" },
      cta(),
    ] },
  ],
  faq: [
    { q: "Is Claude 4 better than Claude 3.5 for coding?", a: "Yes — the current line improves on 3.5 most visibly in agentic coding, multi-step reasoning and long-context consistency, and it runs on the same Messages API. For coding workloads there is no reason to start anything new on a 3.5-era model ID." },
    { q: "What replaced claude-3-5-sonnet-20241022?", a: "claude-sonnet-5 is the direct successor at the same $3/$15 list price; claude-sonnet-4-6 is the previous-generation option if your prompts are pinned. The swap is a one-line change to the model field." },
    { q: "Do I need to change my code to migrate from Claude 3.5?", a: "Only the model ID. Endpoint, headers (x-api-key and anthropic-version), max_tokens, message shape and response parsing are all unchanged, so existing Messages API code keeps working." },
    { q: "Is Claude 4 more expensive than Claude 3.5?", a: "At list price, Sonnet 5 costs exactly what 3.5 Sonnet did ($3/$15 per 1M tokens) and the Opus tier is far cheaper than 3 Opus was. On apiToken.sale a flat 50% discount applies to official spend, so every tier lands below the old official bill." },
    { q: "Can I run old and new models side by side during migration?", a: "Yes. One apiToken.sale key and balance cover every supported Claude model, so you can route a share of traffic to the new model ID while the old config stays live, and compare per-request token usage in the dashboard." },
    { q: "Will my Claude 3.5 prompts still work on the newer models?", a: "Almost always, since the prompt format is identical — but outputs will shift: instructions are followed more literally and answers run more thorough. Re-test prompts that were heavily tuned to 3.5's behavior before promoting the new ID to production." },
  ],
  related: ["best-claude-model-for-coding", "claude-opus-vs-sonnet", "claude-sonnet-api", "claude-api-quick-setup"],
  updated: "2026-08-17",
};
