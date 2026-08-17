import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "how-billing-works",
  cluster: "explain",
  title: "How Billing Works on apiToken.sale",
  h1: "How billing works",
  description: "Understand one prepaid balance for Claude, GPT, Gemini and Kimi: exact provider-rate metering, a flat B2C discount and token-level usage in the dashboard.",
  keywords: ["multi provider api billing", "claude api billing", "gpt api billing", "gemini api billing", "kimi api billing", "prepaid api balance", "api usage tracking"],
  dek: "Billing is prepaid and transparent. Claude, GPT, Gemini and Kimi requests draw from one balance after exact provider-rate metering and your discount, with a breakdown you can audit.",
  sections: [
    { h2: "Prepaid balance", blocks: [
      { type: "p", text: "You top up any whole-dollar amount. Balance never expires and there is no customer subscription, so idle time costs nothing. The same balance covers supported Claude, GPT, Gemini and Kimi models." },
    ] },
    { h2: "Per-request metering", blocks: [
      { type: "list", items: [
        "Each call is converted to official provider spend by its exact usage legs: input, output, cache and any model-specific long-context or image buckets.",
        "Your flat 50% B2C discount is subtracted across every supported provider.",
        "The net amount is deducted from your prepaid balance.",
      ] },
    ] },
    { h2: "Full visibility", blocks: [
      { type: "p", text: "Every request appears in your dashboard with its model and provider and a token breakdown, so you always know where your balance goes." },
      cta(),
    ] },
  ],
  faq: [
    { q: "Is billing prepaid or postpaid?", a: "Prepaid. You fund a balance in advance and requests draw it down; there is no monthly invoice." },
    { q: "Does one balance cover Claude, GPT, Gemini and Kimi?", a: "Yes. Each provider is metered against its own official rate card, then the same B2C discount applies and the charge draws from one prepaid balance." },
    { q: "Can I see token-level usage?", a: "Yes. The dashboard breaks usage down by model, provider and token bucket." },
  ],
  related: ["claude-api-pricing-explained", "gpt-api-pricing", "gemini-api-pricing", "kimi-api-pricing"],
  updated: "2026-08-09",
};
