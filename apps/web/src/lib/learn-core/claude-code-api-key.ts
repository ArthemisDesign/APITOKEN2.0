import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-code-api-key",
  cluster: "integrate",
  title: "Claude Code API Key: Setup in Two Environment Variables",
  h1: "Run Claude Code on an apiToken.sale API key",
  description: "Get a Claude Code API key without an Anthropic subscription: point ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY at router.apitoken.sale and run every Claude model on prepaid balance at a flat 50% off.",
  keywords: ["claude code api key", "claude code setup", "claude code anthropic base url", "claude code environment variables", "claude code custom api key", "claude code without anthropic account", "anthropic_api_key claude code", "claude code pay as you go", "claude code model settings", "claude code api key invalid"],
  dek: "Claude Code takes its endpoint and credential from two environment variables, so a Claude Code API key from apiToken.sale is a drop-in replacement for subscription billing: set ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY once and the CLI runs unchanged against your prepaid balance. Below is the exact setup, which model to run per session, and fixes for the three errors everyone hits first.",
  sections: [
    { h2: "Set ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY", blocks: [
      { type: "p", text: "Claude Code reads its endpoint from ANTHROPIC_BASE_URL and its credential from ANTHROPIC_API_KEY. Point both at apiToken.sale and the CLI works exactly as before — it simply bills your prepaid balance at a flat 50% off official Anthropic spend instead of a monthly subscription. There is no plugin, proxy, or wrapper involved." },
      { type: "steps", items: [
        "Create a free account on apiToken.sale and generate a key in the dashboard. It looks like sk-pool-… and the same key covers every supported Claude model.",
        `Export the two variables in your shell: ANTHROPIC_BASE_URL=${BASE} and ANTHROPIC_API_KEY=${KEY}.`,
        "Run claude in any project directory and ask it something small — a one-line question is enough to confirm the key is live.",
      ] },
      { type: "code", code: `export ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# then just run it in your project\nclaude` },
    ] },
    { h2: "Keep the variables across terminals", blocks: [
      { type: "p", text: "An export only lives for the current shell session. Close the terminal and Claude Code loses its credentials, which is the most common reason a working setup 'stops working' the next day. Put both lines in your shell startup file — ~/.zshrc on macOS, ~/.bashrc on most Linux setups — and they load automatically." },
      { type: "code", code: `echo 'export ANTHROPIC_BASE_URL=${BASE}' >> ~/.zshrc\necho 'export ANTHROPIC_API_KEY=${KEY}' >> ~/.zshrc\nsource ~/.zshrc` },
      { type: "note", text: "The variables must be exported, not just assigned, because Claude Code runs as a child process of your shell and only inherits exported variables. If you edited the startup file by hand, open a new terminal or run source on the file before launching claude." },
    ] },
    { h2: "Pick the model per session, not per month", blocks: [
      { type: "p", text: "Model choice is the single biggest cost lever in Claude Code, and a prepaid key makes it a per-session decision instead of a plan decision. One key serves the whole supported line, so you can default to a mid-tier model and escalate only when the task earns it." },
      { type: "table", headers: ["Model ID", "Use it for"], rows: [
        ["claude-sonnet-5", "Everyday coding: features, tests, small fixes. The sensible default for most sessions."],
        ["claude-opus-4-8", "Hard refactors, multi-file reasoning, and long agentic sessions where mistakes are expensive."],
        ["claude-haiku-4-5", "Quick questions, cheap experimentation, and high-volume steps where speed matters more than depth."],
      ] },
      { type: "p", text: "Switch mid-session with the /model command, or set the model at launch: claude --model claude-opus-4-8. A practical pattern is to start on claude-sonnet-5 and only move up to Opus when Sonnet stalls on the same problem twice." },
      { type: "link", text: "Per-model prices and context windows", href: "/models" },
    ] },
    { h2: "What changes and what stays the same", blocks: [
      { type: "p", text: "Bringing your own key changes billing and nothing else. Claude Code is the same binary talking to the same Anthropic Messages API, so the features you use daily keep working." },
      { type: "list", items: [
        "Agentic editing, tool use, and streaming behave identically — only the billing endpoint changed.",
        "Model IDs are unchanged: claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5.",
        "The same key also works in Cursor, Cline, Continue, Aider and the official Anthropic SDKs, so one balance covers your whole toolchain.",
        "The balance is prepaid and never expires; it is spent only when requests actually run, at a flat 50% off official spend for B2C accounts.",
      ] },
    ] },
    { h2: "See what each session costs", blocks: [
      { type: "p", text: "The dashboard shows per-request token usage, so a long Claude Code session is no longer a black box — you can see exactly which prompts and models consumed the balance. Top up any whole-dollar amount by bank card or cryptocurrency; there is no fixed product catalog and no monthly commitment." },
      { type: "link", text: "Estimate a month of Claude Code usage in the free calculator", href: "/tools/claude-api-cost-calculator" },
      cta(),
    ] },
    { h2: "Fix the three errors everyone hits first", blocks: [
      { type: "table", headers: ["Symptom", "Likely cause", "Fix"], rows: [
        ["Authentication or 401 error", "Typo in a variable, or the variable was never exported", "Re-check both ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY, then restart the shell so they are exported."],
        ["Claude Code ignores the key and uses a subscription", "An old subscription login is still active", "Run /logout (or check /status) inside Claude Code, then relaunch so the environment key is used."],
        ["429 rate limit responses", "Too much burst concurrency", "Honor Retry-After, back off, and reduce parallelism; contact support for sustained higher throughput."],
      ] },
      { type: "note", text: "Support is available in English and Russian over Telegram if an error persists after the fixes above." },
    ] },
    { h2: "Give Claude Code its own key", blocks: [
      { type: "p", text: "Because one apiToken.sale account can issue multiple named keys, give Claude Code a dedicated key instead of sharing one across every tool. If a key ever leaks from a shell history file or a committed dotfile, you revoke that one key and the rest of your setup keeps running." },
      { type: "list", items: [
        "Set a lifetime spending limit on the Claude Code key to cap the blast radius of a runaway session.",
        "Add an expiration date if the key is for a short-lived project or a contractor machine.",
        "Keep the key in the environment or a secret manager — never in git, dotfiles you publish, or chat messages.",
      ] },
    ] },
  ],
  faq: [
    { q: "How do I get an API key for Claude Code?", a: "Create a free apiToken.sale account, generate a key in the dashboard (it looks like sk-pool-…), then set ANTHROPIC_BASE_URL to https://router.apitoken.sale and ANTHROPIC_API_KEY to the key. Run claude and you are done." },
    { q: "Can I use Claude Code without an Anthropic subscription?", a: "Yes. Claude Code accepts a plain API key, and with an apiToken.sale key it bills prepaid balance at a flat 50% off official spend. The balance never expires, so light users stop paying for idle months." },
    { q: "Which model should I set in Claude Code?", a: "Default to claude-sonnet-5 for everyday coding, switch to claude-opus-4-8 with /model for hard refactors and long sessions, and use claude-haiku-4-5 for cheap, high-volume steps." },
    { q: "Why does Claude Code say my API key is invalid?", a: "Almost always a typo or a variable that was set but not exported. Re-check both environment variables, restart your shell, and if Claude Code still uses a subscription login, run /logout first." },
    { q: "How do I track what Claude Code costs on a prepaid key?", a: "The apiToken.sale dashboard shows per-request token usage for the key. Set a lifetime spending limit on the key if you want a hard ceiling on what a session can burn." },
  ],
  related: ["claude-code-without-subscription", "claude-api-key-for-cursor", "anthropic-sdk-base-url", "best-claude-model-for-coding"],
  updated: "2026-08-17",
};
