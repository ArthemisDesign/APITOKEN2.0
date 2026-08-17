import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-opus-api",
  cluster: "free",
  title: "Claude Opus API Access",
  h1: "Claude Opus 4.8 through the API",
  description: "Access Claude Opus 4.8 and 4.7 through one apiToken.sale key at a flat 50% off official rates. Best for complex reasoning, refactors and long agent sessions.",
  keywords: ["claude opus api", "claude opus 4.8 api", "opus api key", "claude opus pricing", "claude opus discount", "free claude api", "claude api free", "claude api no credit card", "claude api free credits", "try claude api free", "claude api free tier"],
  dek: "Opus is Claude's most capable tier — the model to reach for on hard reasoning, architecture and long agentic runs. apiToken.sale gives you Opus 4.8 and 4.7 on the same key and balance as every other model.",
  sections: [
    { h2: "When to use Opus", blocks: [
      { type: "list", items: [
        "Complex refactors and multi-file changes.",
        "Architecture, planning and high-stakes reasoning.",
        "Long sessions where consistency and cache reuse matter.",
      ] },
    ] },
    { h2: "Opus on your balance", blocks: [
      { type: "p", text: "Opus 4.8 (model ID claude-opus-4-8) and Opus 4.7 are billed at official token rates minus your discount, so you get the top tier for a fraction of the list price." },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (\u221250%)"], rows: [
        ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
      ] },
      { type: "link", text: "Claude Opus 4.8 pricing in detail (cache rates, context, FAQ)", href: "/models/claude-opus-4-8" },
      cta(),
    ] },
  ],
  faq: [
    { q: "Which Opus models are available?", a: "Claude Opus 4.8 (claude-opus-4-8) and Claude Opus 4.7, on the same key and prepaid balance as Sonnet and Haiku." },
    { q: "Is Opus worth the extra tokens?", a: "For complex reasoning, refactors and long agent runs, yes. For fast, cheap tasks, Haiku or Sonnet is usually the better value." },
  ],
  related: ["claude-opus-vs-sonnet", "claude-sonnet-api", "claude-haiku-api", "save-tokens-on-claude-api"],
  updated: "2026-07-17",
};
