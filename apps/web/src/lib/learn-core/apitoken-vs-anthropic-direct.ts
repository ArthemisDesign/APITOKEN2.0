import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "apitoken-vs-anthropic-direct",
  cluster: "compare",
  title: "apiToken.sale vs Anthropic Direct",
  h1: "apiToken.sale vs buying from Anthropic directly",
  description: "Compare apiToken.sale and Anthropic direct: identical Messages API and models, but with a flat 50% off, no account requirement, and card or crypto payment.",
  keywords: ["claude api vs anthropic direct", "apitoken vs anthropic", "anthropic api alternative", "cheaper than anthropic api", "claude api reseller", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
  dek: "apiToken.sale is not a different API — it is the same Anthropic Messages API, resold from prepaid balance at a discount. Here is what actually changes and what does not.",
  sections: [
    { h2: "What stays the same", blocks: [
      { type: "list", items: [
        "The same Anthropic Messages API, endpoints and streaming.",
        "The same model IDs (claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5).",
        "The same request and response format your code already expects.",
      ] },
    ] },
    { h2: "What changes", blocks: [
      { type: "list", items: [
        "Price: a flat 50% below official spend for B2C.",
        "Onboarding: no Anthropic account, waitlist or billing-country requirement.",
        "Payment: bank card or cryptocurrency.",
      ] },
      cta(),
    ] },
    { h2: "Who each is for", blocks: [
      { type: "p", text: "If you already have frictionless Anthropic billing and enterprise agreements, direct may suit you. If you want the same models cheaper, faster to start, and payable by card or crypto, apiToken.sale is the pragmatic choice." },
    ] },
  ],
  faq: [
    { q: "Is apiToken.sale the real Claude API?", a: "Yes — it serves the same Anthropic Messages API and models. Only pricing and onboarding differ." },
    { q: "Why is it cheaper than Anthropic direct?", a: "Balance is prepaid and pooled, and a flat 50% discount is applied to official spend." },
  ],
  related: ["cheapest-claude-api", "apitoken-vs-openrouter", "claude-api-pricing-explained", "how-billing-works"],
};
