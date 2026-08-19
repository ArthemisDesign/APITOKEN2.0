import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-opus-vs-sonnet",
  cluster: "compare",
  title: "Claude Opus vs Sonnet: Which Model to Use and When",
  h1: "Claude Opus vs Sonnet: which model should you use?",
  description: "Claude Opus vs Sonnet, decided by task: Sonnet 5 for coding and agents, Opus 4.8 for hard reasoning — $1.50/$7.50 vs $2.50/$12.50 per 1M tokens here.",
  keywords: ["claude opus vs sonnet", "opus or sonnet for coding", "which claude model to use", "claude opus 4.8 vs sonnet 5", "claude model comparison", "claude opus vs sonnet pricing", "best claude model for coding", "when to use claude opus", "claude api model routing", "anthropic opus vs sonnet price", "claude sonnet 5 vs opus"],
  dek: "The Claude Opus vs Sonnet question is a routing decision, not a loyalty decision. Sonnet 5 handles daily coding and agent work at 40% of the Opus token price; Opus 4.8 is the escalation tier for hard reasoning and long autonomous runs. Both live on the same apiToken.sale key and prepaid balance, so you can switch per request.",
  sections: [
    { h2: "The short answer: Sonnet by default, Opus on demand", blocks: [
      { type: "p", text: "Use Claude Sonnet 5 for almost everything, and escalate to Claude Opus 4.8 when a task genuinely needs deeper reasoning. Sonnet delivers near-Opus coding quality at 40% of the Opus token price, which makes it the correct default for interactive coding, agent loops and production traffic. Opus earns its premium on a narrow set of jobs: multi-file refactors, architecture decisions and long autonomous sessions where a wrong answer costs more than the tokens." },
      { type: "p", text: "The practical mistake is picking one model for everything. Teams that run all traffic on Opus overpay for routine work; teams that never leave Sonnet burn cycles retrying tasks Sonnet was never going to finish. Treat the two tiers as one system: Sonnet drafts, Opus handles the exceptions." },
    ] },
    { h2: "What actually separates Opus from Sonnet", blocks: [
      { type: "p", text: "The two tiers are not different products. They share the Anthropic Messages API, the same request shape, the same 1M-token context window and the same 128K-token output ceiling. What you buy with Opus is reasoning depth and consistency over long horizons — the ability to hold a large codebase or a multi-step plan together without drifting. What you buy with Sonnet is speed and a much lower meter on the 95% of requests that do not need that." },
      { type: "table", headers: ["", "Claude Opus 4.8", "Claude Sonnet 5"], rows: [
        ["Model ID", "claude-opus-4-8", "claude-sonnet-5"],
        ["Official price (in / out per 1M)", "$5 / $25", "$3 / $15"],
        ["Here (−50%)", "$2.50 / $12.50", "$1.50 / $7.50"],
        ["Cache read (per 1M)", "$0.50", "$0.30"],
        ["Context window", "1M tokens", "1M tokens"],
        ["Max output", "128K tokens", "128K tokens"],
        ["Best for", "Hard reasoning, long agent runs", "Everyday coding and agents"],
      ] },
      { type: "note", text: "Anthropic lists introductory Sonnet 5 pricing of $2/$10 per 1M tokens through 2026-08-31; the standard rate is $3/$15. The older generations — Opus 4.7 and Sonnet 4.6 — stay available at the same rates as their successors, so there is no price reason to pin new work to them." },
    ] },
    { h2: "Tasks where Sonnet is the right tool", blocks: [
      { type: "p", text: "Sonnet wins wherever the work is fast, iterative and volume-driven. Output tokens are the expensive half of every request — five times the input rate on both tiers — so the model that finishes in one pass at a lower output rate almost always beats the stronger model used carelessly." },
      { type: "list", items: [
        "Interactive editing: single-file changes, test generation, refactors you can describe in a paragraph.",
        "Agent loops with many tool calls, where raw token volume dominates the bill.",
        "High-volume production traffic — classification, extraction, drafting, summarization.",
        "Anything latency-sensitive, where a faster first token matters more than the last few points of quality.",
      ] },
    ] },
    { h2: "Tasks where Opus pays for itself", blocks: [
      { type: "list", items: [
        "Large refactors that span many files and punish a missed edge case.",
        "Architecture and design trade-off analysis, where the cost of a bad call dwarfs the token cost.",
        "Long autonomous sessions that must stay coherent over hours of accumulated context.",
        "A final review pass over Sonnet-generated diffs before they merge.",
      ] },
      { type: "p", text: "The escalation trigger should be evidence, not vibes: a Sonnet attempt that failed, a diff touching more files than you can hold in your head, or a decision you cannot afford to reverse. If none of those apply, you are probably buying Opus tokens to do Sonnet work." },
      { type: "note", text: "Both tiers support adaptive thinking — on Sonnet 5 it is on by default when the thinking parameter is omitted, on Opus 4.8 it is the recommended mode. Thinking tokens bill as output tokens, so on Opus you pay $25 per 1M for deliberate reasoning. Enable it where the reasoning is the product; leave it off for mechanical tasks." },
    ] },
    { h2: "Switch models per request on one key", blocks: [
      { type: "p", text: `Routing between tiers is a one-field change. One apiToken.sale key (it looks like ${KEY}) covers Opus, Sonnet and Haiku — plus the supported GPT, Gemini and Kimi models — against a single prepaid balance. There is no per-model plan, no separate signup, and no endpoint change: you swap the model ID in the same Anthropic Messages request.` },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 2048,\n    "messages": [{"role":"user","content":"Review this diff for regressions."}]\n  }'` },
      { type: "p", text: "Change \"claude-sonnet-5\" to \"claude-opus-4-8\" and the same call runs on the top tier. The flat 50% B2C discount applies identically to both, so the relative price ranking never shifts — Sonnet is always the cheaper meter. Every request appears in your dashboard with token-level usage, which makes it easy to see what your routing policy actually costs." },
    ] },
    { h2: "A routing pattern that keeps spend predictable", blocks: [
      { type: "steps", items: [
        "Default every workload to claude-sonnet-5 — interactive sessions, CI agents and production traffic alike.",
        "Define escalation triggers in advance: a failed Sonnet attempt, a multi-file refactor, or an irreversible design decision goes to claude-opus-4-8.",
        "Use Opus as a reviewer rather than a drafter: Sonnet writes the code, Opus audits the diff, so Opus rates apply to a fraction of total tokens.",
        "Reuse long prompts with prompt caching — cache reads bill at $0.30 per 1M on Sonnet 5 and $0.50 on Opus 4.8, far below the input rate, which compounds in long agent loops.",
      ] },
      { type: "p", text: "Run the numbers on your own traffic before committing to a policy: the gap between the tiers is large enough that even a small shift in escalation rate moves the monthly bill." },
      { type: "link", text: "Model the split with the Claude API cost calculator", href: "/tools/claude-api-cost-calculator" },
      { type: "link", text: "Compare all Claude models and prices", href: "/models" },
      cta(),
    ] },
  ],
  faq: [
    { q: "Is Claude Opus better than Sonnet for coding?", a: "Not by default. Sonnet 5 delivers near-Opus quality on everyday coding and editing at 40% of the token price, so it wins on value for most work. Opus 4.8 pulls ahead on complex refactors, architecture and long autonomous runs." },
    { q: "How much more expensive is Opus than Sonnet?", a: "Officially $5/$25 per 1M input/output tokens versus Sonnet's $3/$15. On apiToken.sale the flat 50% discount applies to both: $2.50/$12.50 for Opus 4.8 and $1.50/$7.50 for Sonnet 5." },
    { q: "Can I use Opus and Sonnet with the same API key?", a: "Yes. One key and one prepaid balance cover Opus, Sonnet and Haiku. You switch by changing the model ID in the request — no separate plan, signup or endpoint." },
    { q: "Do Opus and Sonnet have the same context window?", a: "Yes. Both Opus 4.8 and Sonnet 5 offer a 1M-token context window at standard pricing, with no long-context premium, and up to 128K output tokens per response." },
    { q: "Should I still use Opus 4.7 or Sonnet 4.6?", a: "Only if you have prompts or evals pinned to them. Opus 4.7 costs the same as Opus 4.8 and Sonnet 4.6 the same as Sonnet 5, so new work should target the current generation." },
  ],
  related: ["claude-opus-api", "claude-sonnet-api", "claude-haiku-api", "save-tokens-on-claude-api"],
  updated: "2026-08-17",
};
