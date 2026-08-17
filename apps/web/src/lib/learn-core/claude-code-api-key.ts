import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-code-api-key",
  cluster: "integrate",
  title: "Set Up Claude Code with an API Key",
  h1: "Use Claude Code with an apiToken.sale key",
  description: "Configure Claude Code with an apiToken.sale key in two environment variables and run every Claude model on prepaid balance at a flat 50% off.",
  keywords: ["claude code api key", "claude code setup", "claude code anthropic base url", "claude code custom key", "run claude code cheap", "claude api key", "anthropic-compatible api", "claude api base url", "claude api setup", "claude api integration"],
  dek: "Claude Code reads two environment variables. Point them at apiToken.sale and you keep every feature while billing against discounted prepaid balance.",
  sections: [
    { h2: "Two variables", blocks: [
      { type: "code", code: `export ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# then just run\nclaude` },
      { type: "p", text: "That is the whole setup. Use claude-opus-4-8 for hard work and claude-sonnet-5 for everyday coding." },
      cta(),
    ] },
    { h2: "Verify and choose a model", blocks: [
      { type: "p", text: "Run a small prompt first to confirm the key works, then set your default model. If Claude Code reports an auth error, re-check both environment variables and restart your shell so they are exported." },
      { type: "list", items: [
        "Everyday coding: claude-sonnet-5.",
        "Hard refactors and long sessions: claude-opus-4-8.",
        "See per-request token usage in the dashboard to track spend.",
      ] },
    ] },
  ],
  faq: [
    { q: "How do I point Claude Code at apiToken.sale?", a: "Set ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY to your apiToken.sale endpoint and key, then run claude." },
    { q: "Do I keep all Claude Code features?", a: "Yes — only billing changes, from subscription to discounted prepaid usage." },
  ],
  related: ["claude-code-without-subscription", "claude-api-key-for-cursor", "anthropic-sdk-base-url", "best-claude-model-for-coding"],
};
