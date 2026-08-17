import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-crypto-payment",
  cluster: "buy",
  title: "Pay for the Claude API with Crypto",
  h1: "Pay for the Claude API with cryptocurrency",
  description: "Buy Claude API balance with cryptocurrency or bank card on apiToken.sale. No Anthropic account, instant activation, prepaid balance that never expires.",
  keywords: ["claude api crypto payment", "buy claude api with crypto", "claude api usdt", "pay anthropic api crypto", "claude api bitcoin", "buy claude api", "claude api access", "claude api tokens", "claude api top up", "claude api reseller", "claude api provider"],
  dek: "If a bank card is not an option — or you simply prefer crypto — you can fund your Claude API balance with cryptocurrency such as USDT or BTC and start immediately, with no Anthropic account.",
  sections: [
    { h2: "Card or crypto, your choice", blocks: [
      { type: "p", text: "At checkout you can pay by bank card or with cryptocurrency through a secure payment provider. Either way the balance lands prepaid in your account and is spent only when requests run." },
    ] },
    { h2: "Why crypto helps", blocks: [
      { type: "list", items: [
        "No supported Anthropic billing country required.",
        "Useful where cards are declined or unavailable.",
        "Balance never expires, so you fund once and draw down as you build.",
      ] },
      cta(),
    ] },
    { h2: "What to expect at checkout", blocks: [
      { type: "p", text: "Choose crypto at checkout, send the amount to the address shown, and your balance credits once the network confirms. Card remains available if you prefer it for a specific top-up." },
      { type: "list", items: [
        "Balance credits after on-chain confirmation.",
        "Any whole-dollar amount; balance never expires.",
        "Switch between card and crypto per top-up.",
      ] },
    ] },
    { h2: "Which cryptocurrencies you can pay with", blocks: [
      { type: "p", text: "Crypto top-ups go through a secure payment provider, so common coins are supported." },
      { type: "list", items: [
        "USDT and other stablecoins.",
        "BTC and major cryptocurrencies.",
        "Balance credits once the network confirms the transaction.",
      ] },
    ] },
  ],
  faq: [
    { q: "Which payment methods are supported?", a: "You can pay by bank card or with cryptocurrency through a checkout provider." },
    { q: "Does the balance expire?", a: "No. Prepaid balance never expires and is consumed only by real API usage." },
    { q: "Can I buy Claude API access with USDT?", a: "Yes — you can top up your Claude API balance with USDT or other supported cryptocurrencies at checkout." },
  ],
  related: ["claude-api-for-russia", "how-to-buy-claude-api-key", "how-billing-works", "claude-api-refund-policy"],
  updated: "2026-07-17",
};
