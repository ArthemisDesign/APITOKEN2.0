import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-refund-policy",
  cluster: "explain",
  title: "Claude API Refund Policy",
  h1: "Claude API refund policy: balance, refunds and support",
  description: "Claude API refund policy on apiToken.sale: unspent top-ups are refundable within 5 calendar days via the original payment provider; balance never expires.",
  keywords: ["claude api refund", "claude api refund policy", "apitoken refund policy", "claude api money back", "prepaid api balance refund", "claude api support", "apitoken support telegram", "claude api billing help", "claude api payment issue", "anthropic api refund"],
  dek: "The Claude API refund question has a short answer on a prepaid platform: an untouched top-up can be refunded within 5 calendar days, and anything already spent on real usage cannot. Balance never expires, so money you do not refund simply stays available for future calls.",
  sections: [
    { h2: "Can you get your money back? The short answer", blocks: [
      { type: "p", text: "Yes, if you act before you spend. A top-up is refundable when both conditions hold: you request the refund within 5 calendar days of that top-up, and none of the balance from that top-up has been consumed — the paid balance must be completely unused. Eligible refunds are processed back through the original payment provider, so the money returns the same way it arrived." },
      { type: "p", text: "The moment any part of a paid top-up is spent on API requests, that top-up becomes final and non-refundable in full. There are no partial refunds for partially used balances. This is the trade that makes a prepaid, never-expiring balance workable: you commit small amounts, and the platform commits to keeping them valid forever." },
      { type: "p", text: "It helps to see why the rule is shaped this way. Postpaid API billing has no refund question at all — you are invoiced after the fact for exactly what you consumed. Prepaid inverts the sequence: money moves first, usage later. The only refundable unit in that model is the untouched top-up, because consumed tokens are already spent with the upstream provider and cannot be un-spent by anyone." },
    ] },
    { h2: "The two conditions every refund must meet", blocks: [
      { type: "list", items: [
        "Time: the request reaches support within 5 calendar days after the specific top-up you want refunded — not within 5 days of signing up or of your last login.",
        "Usage: zero consumption from that top-up. Even a single billed request against the paid balance closes the window for that top-up.",
      ] },
      { type: "p", text: "Each top-up is evaluated on its own. If you funded your account more than once, the refund window and the unspent requirement apply to the individual payment you are asking about, identified by its order identifier." },
      { type: "note", text: "Free credit is always spent before paid balance. If your account holds promotional credit — for example the $5 welcome bonus — your first requests consume that credit and leave your paid top-up untouched, so early testing does not by itself void a refund." },
    ] },
    { h2: "What is never refundable", blocks: [
      { type: "p", text: "Some parts of your balance are excluded from refunds regardless of timing or usage:" },
      { type: "list", items: [
        "Any paid balance already consumed by API usage — no partial refunds.",
        "Promotional credit, including the $5 welcome bonus for Google or GitHub sign-ups.",
        "Payment processing and crypto network fees already incurred on a completed payment.",
      ] },
      { type: "table", headers: ["Scenario", "Refund outcome"], rows: [
        ["Top-up made 3 days ago, balance untouched", "Refundable — within the 5-day window, returned through the original payment provider"],
        ["Top-up partially spent on requests", "Non-refundable — the top-up is final in full once any part is consumed"],
        ["$5 Google/GitHub welcome bonus", "Never refundable — promotional credit"],
        ["Card processing or crypto network fee", "Never refundable — fees already incurred"],
        ["Unspent top-up older than 5 days", "Outside the standard window — contact support; mandatory consumer rights and service-failure remedies still apply"],
      ] },
      { type: "link", text: "How crypto top-ups work: coins, network fees and confirmation times", href: "/docs/learn/claude-api-crypto-payment" },
    ] },
    { h2: "How to request a refund, step by step", blocks: [
      { type: "steps", items: [
        "Write to apitokensale@gmail.com from the email address on your account, or message support in Telegram — English and Russian both work.",
        "Include the order identifier of the top-up you want refunded so support can match your request to the exact provider payment.",
        "Expect identity, ownership and anti-fraud checks before release; they protect the original payer, not slow you down.",
        "The refund is returned to the original payment method where technically possible, and the payment provider's own processing time applies on top.",
      ] },
      { type: "note", text: "Refund routes depend on the payment provider shown at checkout. A card payment and a crypto payment do not necessarily support the same return path, so the provider's capabilities — not just the platform's decision — shape how the money comes back." },
      { type: "p", text: "Before you write, open the dashboard and confirm the balance from that top-up is genuinely untouched. Every request is listed there with its model and a token-level breakdown, so you can verify zero consumption yourself instead of asking support to check. If the ledger shows even one billed request after the top-up, the refund window for that payment is already closed and the practical move is to keep the balance for future usage — it does not expire." },
    ] },
    { h2: "Why the balance model limits your downside anyway", blocks: [
      { type: "p", text: "Because the balance never expires, a top-up you cannot refund is not money lost — it stays on your account and is drawn down only as requests run. Top-ups accept any whole-dollar amount, and you can switch between bank card and cryptocurrency per top-up, so the practical strategy is to fund small and often rather than park a large sum you might later want back." },
      { type: "p", text: "The same balance covers supported Claude, GPT, Gemini and Kimi models, so an unspent amount is never stranded on a model you stopped using. Every request appears in the dashboard with a token-level breakdown, which makes it easy to verify exactly when a paid balance started being consumed." },
      { type: "link", text: "The full billing guide: balance, per-request metering and the 50% discount", href: "/docs/learn/how-billing-works" },
      cta(),
    ] },
    { h2: "Reaching a human: support in English and Russian", blocks: [
      { type: "p", text: "Refund, billing and integration questions all go to the same place: support on Telegram or email at apitokensale@gmail.com, answered in English or Russian. Most integration questions are answered quickly, and billing or refund messages go straight to a human rather than a queue bot. When you write about money, always include your account email and the order identifier — it is the single detail that turns a back-and-forth into a one-message resolution." },
      { type: "p", text: "For technical problems rather than billing, the fastest reports follow the same discipline as a good bug report: name the model you called, paste the exact error response, and give the approximate time of the request. With those three facts support can locate the failing call immediately instead of asking you to reproduce it." },
    ] },
  ],
  faq: [
    { q: "Does my apiToken.sale balance expire?", a: "No. Prepaid balance never expires and is consumed only by real API usage, so an unrefunded top-up stays available indefinitely." },
    { q: "Can I get a refund if I already spent part of my balance?", a: "No. A top-up is refundable only while its paid balance is completely unused and the request arrives within 5 calendar days of that top-up; any consumption makes it final in full." },
    { q: "Is the $5 welcome bonus refundable?", a: "No. Promotional credit — including the $5 bonus for accounts created with Google or GitHub — and payment or network fees are never refundable. Free credit is always spent before paid balance." },
    { q: "How do I request a refund from apiToken.sale?", a: "Email apitokensale@gmail.com from your account email — or message support in Telegram — and include the order identifier of the unspent top-up. Refunds go back through the original payment provider." },
    { q: "Where does the refunded money go?", a: "To the original payment method where technically possible. The supported return route depends on the checkout provider, and its processing times apply." },
    { q: "What languages does support answer in?", a: "English and Russian, via Telegram or email at apitokensale@gmail.com." },
  ],
  related: ["how-billing-works", "claude-api-crypto-payment", "claude-api-supported-countries", "how-to-buy-claude-api-key"],
  updated: "2026-08-17",
};
