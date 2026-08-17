import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-prompt-caching",
  cluster: "explain",
  title: "Prompt Caching on the Claude API",
  h1: "Cutting costs with Claude prompt caching",
  description: "Prompt caching makes repeated context on the Claude API much cheaper. How it works on apiToken.sale, when to use it, and how it stacks with your discount.",
  keywords: ["claude prompt caching", "claude api cache", "anthropic prompt cache", "reduce claude cost caching", "claude cache read", "claude api key", "anthropic-compatible api", "claude api base url", "claude api setup", "claude api integration", "claude code api"],
  dek: "If you send the same large context repeatedly — system prompts, files, tool definitions — caching turns those tokens from expensive to nearly free.",
  sections: [
    { h2: "How caching saves money", blocks: [
      { type: "p", text: "Cache writes and cache reads are metered separately, and cache reads cost a fraction of fresh input tokens. Stable, reused context is the ideal candidate." },
    ] },
    { h2: "It stacks with your discount", blocks: [
      { type: "p", text: "Caching lowers the token count; your apiToken.sale discount lowers the price per token. Together they compound into a much smaller bill, and every cache line is visible in your usage breakdown." },
      cta(),
    ] },
  ],
  faq: [
    { q: "How much does prompt caching save?", a: "Cache reads cost a fraction of fresh input tokens, so repeated large context becomes far cheaper." },
    { q: "Does caching work with the discount?", a: "Yes — caching reduces token count and the discount reduces price per token, so the savings multiply." },
  ],
  related: ["save-tokens-on-claude-api", "claude-api-pricing-explained", "cheapest-claude-api", "how-billing-works"],
};
