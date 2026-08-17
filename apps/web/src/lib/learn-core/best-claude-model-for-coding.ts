import type { LearnArticle } from "../learn";
import { BASE, KEY, cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "best-claude-model-for-coding",
  cluster: "compare",
  title: "Best Claude Model for Coding",
  h1: "The best Claude model for coding",
  description: "Which Claude model should you use for coding? A practical guide to picking Opus, Sonnet or Haiku per task — all available on one apiToken.sale key.",
  keywords: ["best claude model for coding", "claude model for programming", "opus vs sonnet vs haiku", "which claude model should i use", "claude sonnet for coding", "claude opus for coding", "claude haiku for coding", "claude model comparison", "claude api model routing", "claude api discount"],
  dek: "The best Claude model for coding is not one model — it is a per-task decision between Sonnet, Opus and Haiku. This guide gives you the routing rules, the real token rates at 50% off, and the exact request change that switches tiers on a single apiToken.sale key.",
  sections: [
    { h2: "The short answer: Sonnet by default, Opus for the hard parts", blocks: [
      { type: "p", text: "The best Claude model for coding is Claude Sonnet 5 for the bulk of the work, Claude Opus 4.8 for the sessions where a wrong answer costs you hours, and Claude Haiku 4.5 for the mechanical volume in between. Pick per task, not per project: the same endpoint, key and prepaid balance serve all three tiers, so the model is just a string you set on each request." },
      { type: "p", text: "That split follows how the tiers are built. Sonnet trades a little peak reasoning for speed and a much lower token rate — exactly what an interactive loop of edit, run, read the error, edit again rewards. Opus spends more compute per token and holds long, ambiguous threads together better. Haiku is tuned for latency and price, not depth, and that is a feature when the task has no depth to miss." },
    ] },
    { h2: "The three tiers at a glance", blocks: [
      { type: "p", text: "All four model IDs below are served at a flat 50% off official Anthropic token pricing, billed from one prepaid balance:" },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (−50%)", "Best for"], rows: [
        ["Claude Sonnet 5 (claude-sonnet-5)", "$3 / $15", "$1.50 / $7.50", "Everyday coding, agents, code review"],
        ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50", "Same role, previous generation"],
        ["Claude Opus 4.8 (claude-opus-4-8)", "$5 / $25", "$2.50 / $12.50", "Hard refactors, architecture, long sessions"],
        ["Claude Haiku 4.5 (claude-haiku-4-5)", "$1 / $5", "$0.50 / $2.50", "Linting, extraction, high-volume edits"],
      ] },
      { type: "p", text: "Opus 4.8 and Sonnet 5 both expose a 1M-token context window, so the choice between them is about reasoning depth and rate, not about how much code you can load into the prompt. Sonnet 4.6 remains available on the same key if a pinned toolchain still expects the previous generation." },
    ] },
    { h2: "When Opus 4.8 earns its higher rate", blocks: [
      { type: "p", text: "Reach for Opus when the task has real ambiguity: a cross-module refactor where choosing the right abstraction is the actual question, a design review of a system you did not write, or a debugging session where the symptom sits three layers away from the cause. In those sessions a weaker model does not fail loudly — it produces plausible code that is subtly wrong, and you pay the difference back in review time." },
      { type: "p", text: "Opus also earns its rate in long agent runs. An agent that plans, edits and verifies for twenty minutes straight is compounding small judgment calls, and one better early decision saves whole branches of wasted tool calls. For a bounded, well-specified ticket, though, Sonnet lands the same diff faster and cheaper — escalating there is pure waste." },
      { type: "note", text: "A practical escalation signal: if Sonnet has gone two full loops without converging — repeating the same failed fix or thrashing between approaches — stop, restart the session on Opus with the error log in the prompt, and let it re-plan from scratch." },
    ] },
    { h2: "Haiku 4.5 for the work you should not pay Sonnet rates for", blocks: [
      { type: "p", text: "A large share of the coding traffic in a real project is mechanical: lint fixes, log classification, extracting symbols from a diff, generating commit messages, first-pass test scaffolding. Haiku 4.5 handles this well at one third of Sonnet's input rate, and its latency makes it the right engine for anything that fires on every save or every CI job." },
      { type: "list", items: [
        "Pre-commit and CI hooks: lint explanations, conventional-commit messages, changelog drafts.",
        "Extraction and routing: pulling structured fields out of logs, stack traces or code before a bigger model reasons over them.",
        "High-fan-out agent steps: scoring candidate files or ranking search hits before Sonnet reads the shortlist.",
      ] },
      { type: "p", text: "The pattern that works in practice is a pipeline: Haiku filters and compresses, Sonnet does the work, Opus reviews the risky parts. Each stage pays only for the judgment it actually needs, and the cheap stages keep the expensive ones focused on a short, clean input." },
    ] },
    { h2: "Switch models per request, not per account", blocks: [
      { type: "p", text: `Because apiToken.sale exposes the standard Anthropic Messages API at ${BASE} with your key in the x-api-key header, changing tiers is a one-line change to the model field — no new credentials, no plan change, no second provider:` },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Refactor this function"}]\n  }'` },
      { type: "steps", items: [
        `Point the client at the router once: base URL ${BASE}, API key ${KEY}. Cursor, Claude Code, Continue and the Anthropic SDKs all accept a custom endpoint.`,
        "Default the tool to claude-sonnet-5 so everyday interactive work lands on the workhorse tier.",
        "Override per session for heavy work — in Claude Code, ANTHROPIC_MODEL=claude-opus-4-8 starts that session on Opus while everything else stays on Sonnet.",
        "In code you control, route explicitly: preprocessing calls go out with claude-haiku-4-5, the core loop with claude-sonnet-5, the final review with claude-opus-4-8.",
      ] },
    ] },
    { h2: "What model routing does to a prepaid balance", blocks: [
      { type: "p", text: "The 50% discount applies identically to every tier, so routing decisions multiply rather than add: a Haiku-routed CI hook at $0.50 per million input tokens is effectively free next to an Opus review session, and that spread is the entire point of mixing models. The balance is prepaid and shared, so a heavy Opus week simply drains it faster — there is no separate plan to upgrade or downgrade, and per-request switching costs nothing but the tokens themselves." },
      { type: "p", text: "Before committing to a routing policy, tally a typical day by tier: how many requests are mechanical, how many are real coding loops, how many are genuinely hard. Multiply each bucket by the rates in the table above and you have a defensible monthly number instead of a guess." },
      { type: "link", text: "Estimate your model mix in the free cost calculator", href: "/tools/claude-api-cost-calculator" },
      { type: "link", text: "Compare every Claude model and price side by side", href: "/models" },
      cta(),
    ] },
  ],
  faq: [
    { q: "What is the best Claude model for coding?", a: "Claude Sonnet 5 is the right default for daily coding and agent loops. Use Claude Opus 4.8 for complex refactors, architecture and long high-stakes sessions, and Claude Haiku 4.5 for fast, high-volume tasks like linting and extraction." },
    { q: "Can I switch Claude models per API request?", a: "Yes. One key and one prepaid balance cover every model, so switching is a one-line change to the model field on the standard Messages API request — no new credentials or plan change." },
    { q: "Is Claude Opus worth it for coding?", a: "For bounded, well-specified tasks, no — Sonnet lands the same diff at roughly 60% of Opus's token rate. Opus pays for itself on ambiguous work: cross-module refactors, design reviews and long agent runs where one better early decision saves many wasted tool calls." },
    { q: "Which Claude model should I set in Cursor or Claude Code?", a: "Default to claude-sonnet-5. For a heavy session, override per run — in Claude Code, ANTHROPIC_MODEL=claude-opus-4-8 puts just that session on Opus while the rest of your tooling stays on Sonnet." },
    { q: "Does one apiToken.sale key cover Opus, Sonnet and Haiku?", a: "Yes. Every supported Claude model — Opus 4.8, Sonnet 5, Sonnet 4.6 and Haiku 4.5 — runs on the same key and prepaid balance, each at 50% off official Anthropic token pricing." },
  ],
  related: ["claude-opus-vs-sonnet", "claude-sonnet-api", "claude-opus-api", "save-tokens-on-claude-api"],
  updated: "2026-08-17",
};
