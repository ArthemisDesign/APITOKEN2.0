import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "vscode-ai-agents-one-prompt",
  cluster: "integrate",
  title: "Free VS Code AI Agents with Claude",
  h1: "Run free VS Code AI agents on Claude",
  description: "Set up free VS Code agents like Cline and Roo Code with an apiToken.sale Claude key — no Cursor Pro needed. One endpoint, every Claude model, at a discount.",
  keywords: ["free vscode ai agent", "cline roo code claude", "vscode claude agent", "cursor alternative free", "claude vscode without cursor", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api", "claude api agent"],
  dek: "You do not need Cursor Pro to get agentic coding. Free VS Code agents accept any Anthropic-compatible key, so Claude runs in VS Code on discounted balance.",
  sections: [
    { h2: "Point the agent at Claude", blocks: [
      { type: "steps", items: [
        "Install a free agent extension such as Cline or Roo Code.",
        "Choose Anthropic as the API provider.",
        `Set the base URL to ${BASE}, paste your ${KEY} key, and pick a model like claude-sonnet-5.`,
      ] },
      cta(),
    ] },
    { h2: "Pick the right model per task", blocks: [
      { type: "list", items: [
        "claude-sonnet-5 — the default for everyday coding and agent loops.",
        "claude-opus-4-8 — complex refactors, architecture and long sessions.",
        "claude-haiku-4-5 — fast, cheap edits and high-volume steps.",
      ] },
      { type: "p", text: "Because one key covers every model, you can switch per task in the extension without changing accounts or billing." },
    ] },
  ],
  faq: [
    { q: "Do I need Cursor Pro for AI coding?", a: "No. Free VS Code agents like Cline and Roo Code work with an apiToken.sale Claude key." },
    { q: "Which model should I pick?", a: "claude-sonnet-5 for everyday coding; claude-opus-4-8 for complex tasks." },
  ],
  related: ["claude-api-for-vs-code", "claude-api-key-for-cursor", "claude-code-api-key", "cursor-without-anthropic-account"],
};
