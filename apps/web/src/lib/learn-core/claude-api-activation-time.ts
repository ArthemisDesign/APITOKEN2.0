import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-activation-time",
  cluster: "explain",
  title: "How Fast Is Claude API Activation?",
  h1: "How fast your Claude API key activates",
  description: "apiToken.sale keys activate instantly. Generate a key, top up, and make a successful Claude API call within minutes — no manual review or waitlist.",
  keywords: ["claude api activation time", "how fast claude api key", "instant claude api key", "claude api ready time", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api", "claude api worldwide", "claude api china"],
  dek: "There is no waiting period between creating your key and using it. Activation is instant, so the only limit on speed is how fast you paste the key into your tool.",
  sections: [
    { h2: "Instant by design", blocks: [
      { type: "p", text: "Keys are live the moment you generate them. Top-ups credit your balance as soon as payment confirms, and card payments confirm in seconds." },
      cta(),
    ] },
    { h2: "What can add a short delay", blocks: [
      { type: "p", text: "The only wait is payment confirmation. Card top-ups clear in seconds; a crypto top-up credits once the network confirms the transaction, which depends on the coin and fee you choose." },
      { type: "list", items: [
        "Key generation: instant.",
        "Card top-up: seconds.",
        "Crypto top-up: after network confirmation.",
      ] },
    ] },
  ],
  faq: [
    { q: "How long until my key works?", a: "Immediately. There is no manual review — a freshly generated key works on the next request." },
    { q: "How long do top-ups take?", a: "Card payments credit in seconds; crypto credits after the network confirms the transaction." },
  ],
  related: ["claude-api-without-waitlist", "how-to-buy-claude-api-key", "how-billing-works", "claude-api-quick-setup"],
};
