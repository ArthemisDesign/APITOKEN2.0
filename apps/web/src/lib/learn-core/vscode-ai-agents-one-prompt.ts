import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "vscode-ai-agents-one-prompt",
  cluster: "integrate",
  title: "Free VS Code AI Agents with Claude",
  h1: "Run free VS Code AI agents on Claude",
  description: "Free VS Code AI agents like Cline and Roo Code on Claude — no Cursor Pro needed. One apiToken.sale key, every Claude model, at a flat 50% off official rates.",
  keywords: ["free vscode ai agent", "cline roo code claude", "vscode claude agent", "cursor alternative free", "claude vscode without cursor", "agentic coding vscode", "cline custom base url", "roo code anthropic api key", "claude api for vscode agents", "free ai coding agent vscode"],
  dek: "A free VS Code AI agent needs exactly two things: an extension like Cline or Roo Code and an Anthropic-compatible API key. Point the extension at the apiToken.sale gateway and Claude executes your one-prompt tasks at a flat 50% off official rates — no Cursor Pro subscription involved.",
  updated: "2026-08-17",
  sections: [
    { h2: "What a one-prompt agent actually requires", blocks: [
      { type: "p", text: "To type one prompt into VS Code and watch an agent plan, edit files, run terminal commands and loop until the task is done, you need a free agent extension and a key for the model — nothing else. Cursor Pro is not a requirement: open-source agents such as Cline and Roo Code accept any Anthropic-compatible endpoint, so Claude runs inside plain VS Code on your own API balance. The extension is free; the only metered component is the model traffic, billed per token." },
      { type: "p", text: "That split matters for your wallet. A subscription bundles a fixed monthly quota you may never use; a per-token key charges only for what the agent actually burns. With an apiToken.sale key, every supported Claude model sits behind one base URL on one prepaid balance, at a flat 50% off official rates." },
    ] },
    { h2: "Connect Cline or Roo Code to the gateway", blocks: [
      { type: "steps", items: [
        "Install Cline or Roo Code from the VS Code Marketplace — both are free and open source.",
        "Open the extension's API provider settings and select Anthropic.",
        `Set the base URL to ${BASE} and paste your ${KEY} key.`,
        "Choose claude-sonnet-5 as the starting model and give the agent its first real task.",
      ] },
      { type: "code", code: `# Cline / Roo Code → API provider settings\nAPI Provider : Anthropic\nBase URL     : ${BASE}\nAPI Key      : ${KEY}\nModel        : claude-sonnet-5` },
      { type: "p", text: "Both extensions speak the standard Anthropic Messages API: streaming responses, tool use and system prompts behave exactly as the specification describes, so the agent cannot tell the gateway from a direct connection. The key works the moment it is created — there is no approval queue or waitlist." },
    ] },
    { h2: "Match the model to the agent step", blocks: [
      { type: "p", text: "An agent loop is not one kind of work. Reading files and applying small edits is cheap, high-volume traffic; untangling a cross-module refactor is not. Because one key covers every Claude model, you switch models inside the extension per task instead of juggling accounts or billing profiles." },
      { type: "table", headers: ["Model ID", "Use it for", "Official in / out ($ per 1M)", "Here (−50%)"], rows: [
        ["claude-haiku-4-5", "Quick edits, lookups, high-volume steps", "$1 / $5", "$0.50 / $2.50"],
        ["claude-sonnet-5", "The default: everyday coding and agent loops", "$3 / $15", "$1.50 / $7.50"],
        ["claude-opus-4-8", "Complex refactors, architecture, long sessions", "$5 / $25", "$2.50 / $12.50"],
      ] },
      { type: "p", text: "A practical pattern: keep the agent on Sonnet 5, drop to Haiku 4.5 for mechanical multi-file chores, and escalate to Opus 4.8 only when the task genuinely needs deep reasoning. The dashboard shows token-level usage per call, so you can see exactly what each session cost." },
      { type: "link", text: "Full Claude model lineup and pricing", href: "/models" },
    ] },
    { h2: "Why agent loops are where the discount compounds", blocks: [
      { type: "p", text: "One prompt in an agentic extension turns into many model calls: the agent re-reads your files, plans, edits, runs tests and re-checks its own output. A task that feels like a single interaction can easily chain dozens of requests. Per-token billing means the cost scales with the loop — and a flat discount on every token scales with it too." },
      { type: "list", items: [
        "Prompt caching is billed at the official cache rates minus your discount, so the long context an agent re-reads on every step comes back cheap.",
        "Output tokens dominate agent sessions — every edit, diff and explanation is output — which is where the per-million savings concentrate.",
        "There is no seat fee and no monthly quota: an idle week costs nothing, a heavy refactoring weekend costs only its tokens.",
      ] },
      { type: "link", text: "Estimate a session with the Claude API cost calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "One key for every agent you run", blocks: [
      { type: "p", text: "The same key is not tied to one extension. It works in Cline, Roo Code, Continue, Cursor, Claude Code and the Anthropic SDKs at the same time, against the same balance — so you can keep an autonomous agent running in one window while a lighter chat extension answers questions in another. Beyond Claude, the identical key also reaches supported GPT, Gemini and Kimi models through their respective protocols, which keeps multi-model workflows on a single prepaid balance that never expires." },
      cta(),
    ] },
    { h2: "If the first prompt fails, check these three things", blocks: [
      { type: "list", items: [
        "401 Unauthorized: the API key or base URL is wrong — re-paste both, with no trailing spaces.",
        "Model not found: the extension is sending an outdated ID; use a current one such as claude-sonnet-5 or claude-opus-4-8.",
        "Slow responses or 429 errors: lower the extension's concurrency and respect the Retry-After header before retrying.",
      ] },
      { type: "note", text: "Some extensions pre-fill Anthropic's default endpoint even after you pick a custom provider. If requests still hit api.anthropic.com, look for a separate \"use custom base URL\" toggle in the provider settings and confirm the field actually saved." },
    ] },
  ],
  faq: [
    { q: "Do I need Cursor Pro to get an AI agent in VS Code?", a: "No. Free open-source extensions like Cline and Roo Code add agentic coding to plain VS Code and accept any Anthropic-compatible endpoint — with an apiToken.sale key, the only cost is per-token model usage." },
    { q: "How do I point Cline or Roo Code at apiToken.sale?", a: "Select Anthropic as the API provider, set the base URL to https://router.apitoken.sale and paste your sk-pool-… key. The same settings work in both extensions." },
    { q: "Which Claude model should a VS Code agent use?", a: "claude-sonnet-5 is the right default for everyday coding loops; escalate to claude-opus-4-8 for complex refactors and drop to claude-haiku-4-5 for cheap, high-volume steps — all on the same key." },
    { q: "What does a Claude agent session cost on this setup?", a: "Billing is per token at official Anthropic rates minus a flat 50%: Sonnet 5 works out to $1.50 input and $7.50 output per million tokens, and prompt caching is billed at the official cache rates minus the same discount." },
    { q: "Can I try a VS Code agent without paying first?", a: "Yes — accounts created with Google or GitHub start with $5 of platform bonus credit, enough for real agent tasks before any top-up. Email/password accounts do not receive the bonus." },
    { q: "Does the same key work outside VS Code?", a: "Yes. The same key covers Cursor, Claude Code and the Anthropic SDKs, and it also reaches supported GPT, Gemini and Kimi models on one prepaid balance." },
  ],
  related: ["claude-api-for-vs-code", "claude-api-key-for-cursor", "claude-code-api-key", "cursor-without-anthropic-account"],
};
