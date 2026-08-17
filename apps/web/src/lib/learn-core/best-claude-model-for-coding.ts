import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "best-claude-model-for-coding",
  cluster: "compare",
  title: "Best Claude Model for Coding",
  h1: "The best Claude model for coding",
  description: "Which Claude model should you use for coding? A practical guide to picking Opus, Sonnet or Haiku per task — all available on one apiToken.sale key.",
  keywords: ["best claude model for coding", "claude model for programming", "opus vs sonnet coding", "claude coding model", "which claude for code", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
  dek: "The best model depends on the task. Match the model to the job and you get better output for fewer tokens — and every tier is on one key.",
  sections: [
    { h2: "Sonnet for everyday coding", blocks: [
      { type: "p", text: "Claude Sonnet 5 and Sonnet 4.6 are the default for interactive coding and agent loops: fast, capable, and cost-effective. Start here for most work." },
    ] },
    { h2: "Opus for hard problems", blocks: [
      { type: "p", text: "Use Claude Opus 4.8 for complex refactors, architecture and long, high-stakes sessions where extra reasoning pays off." },
    ] },
    { h2: "Haiku for volume", blocks: [
      { type: "p", text: "Claude Haiku 4.5 handles fast, cheap, high-volume tasks — linting, extraction, quick edits — to stretch your balance." },
      cta(),
    ] },
  ],
  faq: [
    { q: "What is the best Claude model for coding?", a: "Sonnet for everyday coding, Opus for complex reasoning and refactors, Haiku for fast high-volume tasks — all on one apiToken.sale key." },
    { q: "Can I switch models per request?", a: "Yes. One key and balance cover every model, so you can route each request to the best-value tier." },
  ],
  related: ["claude-opus-vs-sonnet", "claude-sonnet-api", "claude-opus-api", "save-tokens-on-claude-api"],
};
