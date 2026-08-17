import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-aider",
  cluster: "integrate",
  title: "Use the Claude API with Aider",
  h1: "Use the Claude API with Aider",
  description: "Run Aider on Claude through apiToken.sale: export ANTHROPIC_API_BASE and your key, pick a Claude model, and pair-program in the terminal at a flat 50% off.",
  keywords: ["claude api aider", "aider anthropic", "aider claude", "aider anthropic api base", "aider claude api key", "aider cheap claude"],
  dek: "Aider is a terminal pair-programmer that burns tokens fast on long sessions. Point it at the discounted gateway with two environment variables and keep the exact same workflow.",
  published: "2026-07-17",
  updated: "2026-07-17",
  sections: [
    { h2: "Two environment variables", blocks: [
      { type: "code", code: `export ANTHROPIC_API_KEY=${KEY}\nexport ANTHROPIC_API_BASE=${BASE}\n\naider --model anthropic/claude-opus-4-8` },
      { type: "p", text: "Aider routes Anthropic traffic through LiteLLM under the hood, which honours ANTHROPIC_API_BASE — so no config file is required." },
      cta(),
    ] },
    { h2: "Picking a model for Aider", blocks: [
      { type: "list", items: [
        "anthropic/claude-opus-4-8 — hardest refactors and long agentic edits.",
        "anthropic/claude-sonnet-5 — the everyday default; near-Opus coding quality.",
        "anthropic/claude-haiku-4-5 — quick edits and cheap experimentation.",
      ] },
      { type: "p", text: "Long Aider sessions are exactly where the token discount compounds: repo maps, diffs and multi-file edits all bill as input and output tokens." },
    ] },
  ],
  faq: [
    { q: "Does Aider work with a custom Claude endpoint?", a: "Yes. Aider uses LiteLLM for Anthropic models, and LiteLLM honours the ANTHROPIC_API_BASE environment variable — set it to https://router.apitoken.sale and start Aider normally." },
    { q: "Which Claude model is best in Aider?", a: "claude-sonnet-5 is the best default for most coding; switch to claude-opus-4-8 for the hardest multi-file work. Both run on the same key." },
    { q: "How much cheaper is a long Aider session?", a: "Every request is billed at official token rates minus your flat 50% discount, so a session that would cost $10 direct costs $5 here." },
  ],
  related: ["claude-api-litellm", "claude-code-without-subscription", "best-claude-model-for-coding", "save-tokens-on-claude-api"],
};
