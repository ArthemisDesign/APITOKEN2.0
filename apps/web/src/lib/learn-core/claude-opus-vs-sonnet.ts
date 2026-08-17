import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-opus-vs-sonnet",
  cluster: "compare",
  title: "Claude Opus vs Sonnet — Which to Use",
  h1: "Claude Opus vs Sonnet: which model to use",
  description: "Opus or Sonnet? A practical guide to picking the right Claude model for coding and agents — and using both on one apiToken.sale key and balance.",
  keywords: ["claude opus vs sonnet", "which claude model", "opus or sonnet coding", "best claude model", "claude model comparison", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
  dek: "Opus and Sonnet solve different problems. Choosing well is the easiest way to get better results and spend fewer tokens — and you can keep both on one key.",
  sections: [
    { h2: "Use Sonnet by default", blocks: [
      { type: "p", text: "Sonnet 5 and Sonnet 4.6 handle the vast majority of coding and agent work quickly and cost-effectively. Start here." },
    ] },
    { h2: "Escalate to Opus for hard problems", blocks: [
      { type: "p", text: "Reach for Opus 4.8 on complex refactors, architecture, and long high-stakes sessions where extra reasoning pays for itself." },
      { type: "note", text: "Because one key covers both, you can route each task to the right tier without juggling providers." },
      { type: "table", headers: ["", "Claude Opus 4.8", "Claude Sonnet 5"], rows: [
        ["Official price (in / out per 1M)", "$5 / $25", "$3 / $15"],
        ["Here (\u221250%)", "$2.50 / $12.50", "$1.50 / $7.50"],
        ["Context window", "1M tokens", "1M tokens"],
        ["Best for", "Hard reasoning, long agent runs", "Everyday coding and agents"],
      ] },
      { type: "link", text: "Compare all Claude models and prices", href: "/models" },
      cta(),
    ] },
  ],
  faq: [
    { q: "Which is better for coding?", a: "Sonnet is the recommended default for daily coding; use Opus for complex reasoning and long refactors." },
    { q: "Can I use both on one account?", a: "Yes. Opus, Sonnet and Haiku all share the same key and prepaid balance." },
  ],
  related: ["claude-opus-api", "claude-sonnet-api", "claude-haiku-api", "save-tokens-on-claude-api"],
  updated: "2026-07-17",
};
