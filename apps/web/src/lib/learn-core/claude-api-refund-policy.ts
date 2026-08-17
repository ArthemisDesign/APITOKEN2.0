import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-refund-policy",
  cluster: "explain",
  title: "Claude API Refund Policy",
  h1: "Refunds and support",
  description: "Learn how apiToken.sale handles balance, refunds and support. Prepaid balance never expires, and help is available in English and Russian through Telegram.",
  keywords: ["claude api refund", "apitoken refund policy", "claude api support", "claude api money back", "claude api help", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
  dek: "Prepaid balance is designed to be low-risk: it never expires, you spend only what you call, and support is one message away.",
  sections: [
    { h2: "Balance and refunds", blocks: [
      { type: "p", text: "Because balance is prepaid and never expires, unused funds stay available for future usage. Refund handling is processed through the original payment provider; reach out to support with your account details." },
    ] },
    { h2: "Getting help", blocks: [
      { type: "p", text: "Support is available in English and Russian via Telegram, and by email at apitokensale@gmail.com. Most integration questions are answered quickly." },
      cta(),
    ] },
    { h2: "How top-ups and balance work", blocks: [
      { type: "p", text: "You add balance in any whole-dollar amount, and it is drawn down only as requests run. Because it never expires, there is little reason to over-fund — top up as you go." },
      { type: "list", items: [
        "Prepaid, never-expiring balance.",
        "Refunds processed through the original payment provider.",
        "Contact support with your account email for help.",
      ] },
    ] },
  ],
  faq: [
    { q: "Does my balance expire?", a: "No. Prepaid balance never expires and is consumed only by real API usage." },
    { q: "How do I contact support?", a: "Reach support in English or Russian through Telegram, or by email at apitokensale@gmail.com." },
  ],
  related: ["how-billing-works", "claude-api-crypto-payment", "claude-api-supported-countries", "how-to-buy-claude-api-key"],
};
