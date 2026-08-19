import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-crypto-payment",
  cluster: "buy",
  title: "Pay for the Claude API with Crypto",
  h1: "Pay for the Claude API with cryptocurrency",
  description: "Buy Claude API balance with USDT, BTC or a bank card on apiToken.sale. No Anthropic account, instant activation, prepaid balance that never expires.",
  keywords: ["claude api crypto payment", "buy claude api with crypto", "claude api usdt", "pay anthropic api with crypto", "claude api bitcoin", "claude api without bank card", "claude api top up crypto", "claude api prepaid balance", "buy claude api", "claude api access", "claude api tokens"],
  dek: "You can make a Claude API crypto payment without an Anthropic account, a supported billing country, or a bank card at all. On apiToken.sale you top up a prepaid balance with USDT, BTC or other major coins through a secure checkout provider, and the same balance also accepts card top-ups whenever you prefer one. The balance never expires and is spent only when API requests run.",
  sections: [
    { h2: "What a crypto top-up looks like end to end", blocks: [
      { type: "p", text: "The whole flow happens inside checkout and takes one transaction. There is no exchange account to wire through on our side, no invoice, and no manual approval step — the payment provider confirms your transfer and the platform credits your balance." },
      { type: "steps", items: [
        "Open the dashboard, choose top-up, and pick cryptocurrency at checkout — card stays available for any future top-up.",
        "Enter any whole-dollar amount. There is no fixed product catalog; you fund exactly what you plan to spend.",
        "Send the amount to the address the secure payment provider shows you, from any wallet or exchange.",
        "Wait for the on-chain confirmation. Once the network confirms the transaction, the balance credits to your account automatically.",
      ] },
      { type: "note", text: "Confirmation time depends on the coin and network you send from, not on the platform — a stablecoin on a fast network confirms quicker than a busy BTC mempool. The crediting rule is the same either way: balance appears after the network confirms." },
      { type: "p", text: "Three habits make crypto top-ups boring in the good way. Send on the exact network the checkout shows for your coin — a USDT transfer on the wrong chain is the classic unrecoverable mistake. Prefer a stablecoin if you want the amount that leaves your wallet to equal the amount credited, since a volatile coin can drift between send and confirmation. And keep the transaction hash: if you ever need support to trace a payment, the hash plus your account email is the fastest way to get it resolved." },
    ] },
    { h2: "Card or crypto: what actually differs per top-up", blocks: [
      { type: "p", text: "Both rails land in the same prepaid balance, draw down only when requests run, and never expire. The choice is per top-up, not per account, so you can fund with USDT this month and a card next month without anything else changing." },
      { type: "table", headers: ["", "Bank card", "Crypto (USDT, BTC, …)"], rows: [
        ["Where you pay", "Secure checkout provider", "Secure checkout provider"],
        ["When balance credits", "When checkout confirms the payment", "After the on-chain confirmation"],
        ["Amount", "Any whole-dollar amount", "Any whole-dollar amount"],
        ["Anthropic account needed", "No", "No"],
        ["Best when", "Your card works and passes 3-D Secure", "Cards are declined, unsupported, or you keep funds in crypto"],
      ] },
    ] },
    { h2: "When crypto is the practical rail for Claude API access", blocks: [
      { type: "p", text: "Anthropic's own billing requires a supported country and a working payment method, which is exactly where many developers get stuck. A crypto top-up sidesteps that entire gate: you are not opening an Anthropic account at all, so no supported billing country is required." },
      { type: "list", items: [
        "Your region has no supported Anthropic billing country, so direct signup is not available.",
        "Your bank declines cross-border or API-related card charges, or your card simply is not accepted at checkout.",
        "You hold working funds in stablecoins and would rather spend USDT than convert through a bank first.",
        "You want to keep API spend off a shared or corporate card statement.",
      ] },
      { type: "p", text: "Because the balance is prepaid and never expires, a crypto top-up is not a subscription commitment. Fund once, draw the balance down as requests run, and top up again — by either rail — only when you need to. Unused funds simply stay available, so there is little reason to over-fund: small, regular top-ups work exactly as well as one large one." },
      { type: "link", text: "How to access the Claude API from Russia and restricted regions", href: "/docs/learn/claude-api-for-russia" },
    ] },
    { h2: "Spending the balance: one key for Claude, GPT, Gemini and Kimi", blocks: [
      { type: "p", text: `Once the balance lands, generate a key in the dashboard — it looks like sk-pool-… and activates instantly, with no waitlist. That single key serves the standard Anthropic Messages API at ${BASE}, so Claude Code, Cursor, Cline and the official Anthropic SDKs work against it unchanged; the same balance also covers supported GPT, Gemini and Kimi models. Every request is metered at official provider rates, then your flat 50% B2C discount is applied before the charge hits your balance.` },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
      { type: "p", text: "If anything goes wrong with a payment or a top-up, support is available in English and Russian through Telegram, or by email at apitokensale@gmail.com, and refunds are processed through the original payment provider." },
      { type: "link", text: "Compare every supported model and its price", href: "/models" },
      cta(),
    ] },
  ],
  faq: [
    { q: "Can I buy Claude API access with USDT?", a: "Yes. Choose crypto at checkout and send USDT or another supported stablecoin to the address shown; your Claude API balance credits once the network confirms the transaction. Send on the exact network the checkout displays and keep the transaction hash until the balance lands." },
    { q: "Which payment methods are supported, and can I mix them?", a: "Bank card and cryptocurrency (USDT and other stablecoins, BTC and major coins) through a secure checkout provider. The choice is per top-up, so you can switch between card and crypto freely, and either way you can fund any whole-dollar amount." },
    { q: "Do I need an Anthropic account or a supported billing country to pay with crypto?", a: "No. apiToken.sale issues its own key and prepaid balance, so there is no Anthropic signup, no billing-country gate, and no waitlist — the key works on the next request after you generate it. The balance never expires and is consumed only by real API usage." },
  ],
  related: ["claude-api-for-russia", "how-to-buy-claude-api-key", "how-billing-works", "claude-api-refund-policy"],
  updated: "2026-08-17",
};
