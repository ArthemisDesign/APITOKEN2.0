import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "save-tokens-on-claude-api",
  cluster: "explain",
  title: "How to Save Tokens on the Claude API",
  h1: "How to save tokens on the Claude API",
  description: "Cut Claude API costs with prompt caching, the right model per task, and tighter context. Practical token-saving tactics that stack with the apiToken.sale discount.",
  keywords: ["save tokens claude api", "reduce claude api cost", "claude prompt caching", "claude api optimization", "lower claude api bill", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
  dek: "Your discount lowers the price per token; these tactics lower the number of tokens. Together they compound into a much smaller bill.",
  sections: [
    { h2: "Use prompt caching", blocks: [
      { type: "p", text: "Long, stable context — system prompts, large files, tool definitions — should be cached. Cache reads cost a fraction of fresh input tokens, so repeated context becomes cheap." },
    ] },
    { h2: "Pick the right model", blocks: [
      { type: "p", text: "Do not send every request to Opus. Route cheap or high-volume work to Haiku, keep everyday coding on Sonnet, and reserve Opus for genuinely hard reasoning." },
    ] },
    { h2: "Trim context", blocks: [
      { type: "list", items: [
        "Send only the files and history a task actually needs.",
        "Summarize long threads instead of resending them in full.",
        "Cap max_tokens to what the response really requires.",
      ] },
      cta(),
    ] },
  ],
  faq: [
    { q: "What is the single biggest token saver?", a: "Prompt caching for large, repeated context, combined with choosing the cheapest model that can do the job." },
    { q: "Do these tips stack with the discount?", a: "Yes. The discount lowers price per token; these tactics lower token count, so the savings multiply." },
  ],
  related: ["claude-api-pricing-explained", "cheapest-claude-api", "claude-haiku-api", "claude-opus-vs-sonnet"],
};
