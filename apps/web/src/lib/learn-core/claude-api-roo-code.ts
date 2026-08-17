import type { LearnArticle } from "../learn";
import { cta, BASE } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-roo-code",
  cluster: "integrate",
  title: "Use the Claude API with Roo Code",
  h1: "Use the Claude API with Roo Code",
  description: "Connect Roo Code in VS Code to Claude through apiToken.sale: choose the Anthropic provider, enable the custom base URL, paste your key and code at a flat 50% off.",
  keywords: ["claude api roo code", "roo code anthropic", "roo code claude", "roo code custom base url", "roo code api key", "roo code cheap claude"],
  dek: "Roo Code is an agentic VS Code extension with a native Anthropic provider and a custom base URL option — which makes it a two-minute setup on the discounted gateway.",
  published: "2026-07-17",
  updated: "2026-07-17",
  sections: [
    { h2: "Setup in three steps", blocks: [
      { type: "steps", items: [
        "Open Roo Code settings and choose Anthropic as the API provider.",
        `Enable the custom base URL option and set it to ${BASE}; paste your sk-pool-… key.`,
        "Pick a model such as claude-opus-4-8 or claude-sonnet-5 and start a task.",
      ] },
      cta(),
    ] },
    { h2: "Why Roo Code burns tokens — and how to pay less", blocks: [
      { type: "p", text: "Agentic extensions read files, plan, edit and re-check in loops, so a single task can run many model calls. That is precisely the workload where a per-token discount matters most: the same session, 50% cheaper, with token-level visibility in the dashboard." },
      { type: "list", items: [
        "Route everyday tasks to claude-sonnet-5 and hard ones to claude-opus-4-8.",
        "Prompt caching is billed at the cheaper official cache rates minus your discount.",
        "One key covers Roo Code, Cline, Cursor and the SDKs simultaneously.",
      ] },
    ] },
  ],
  faq: [
    { q: "Does Roo Code support a custom Anthropic base URL?", a: "Yes — the Anthropic provider settings include a custom base URL option; set it to https://router.apitoken.sale and use your apiToken.sale key." },
    { q: "Which models does Roo Code get on this key?", a: "Every supported Claude model — Opus 4.8 and 4.7, Sonnet 5 and 4.6, Haiku 4.5 — on one key and one prepaid balance." },
    { q: "Is this different from using Cline?", a: "The setup is nearly identical: both are VS Code agents with an Anthropic provider that accepts a custom base URL. Use whichever agent you prefer; the key works in both." },
  ],
  related: ["claude-api-for-vs-code", "claude-api-key-for-cursor", "claude-api-langchain", "cheapest-claude-api"],
};
