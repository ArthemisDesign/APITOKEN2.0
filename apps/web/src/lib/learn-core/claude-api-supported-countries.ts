import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-supported-countries",
  cluster: "explain",
  title: "Claude API Supported Countries",
  h1: "Where you can use apiToken.sale",
  description: "apiToken.sale works worldwide with no Anthropic billing-country requirement. Pay by card or crypto and use the Claude API from regions Anthropic does not serve directly.",
  keywords: ["claude api supported countries", "claude api worldwide", "anthropic api country restrictions", "claude api available regions", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
  dek: "Because we issue your key and balance, there is no Anthropic billing-country gate. That opens the Claude API to developers in regions where signing up directly is difficult.",
  sections: [
    { h2: "No billing-country gate", blocks: [
      { type: "list", items: [
        "No Anthropic account or supported billing country required.",
        "Card and cryptocurrency payment options.",
        "Support in English and Russian over Telegram.",
      ] },
      cta(),
    ] },
    { h2: "How payment works across regions", blocks: [
      { type: "p", text: "Because we issue the key and balance, you are not tied to an Anthropic-supported billing country. Pay with a bank card where available, or with cryptocurrency where cards are declined." },
      { type: "list", items: [
        "No Anthropic billing country required.",
        "Card or cryptocurrency at checkout.",
        "Support in English and Russian over Telegram.",
      ] },
    ] },
  ],
  faq: [
    { q: "Is the Claude API available in my country?", a: "apiToken.sale has no billing-country requirement, so you can buy balance and use a key from regions Anthropic does not bill directly." },
    { q: "What about payment restrictions?", a: "You can pay by card or with cryptocurrency, which helps where cards are unavailable." },
  ],
  related: ["claude-api-for-russia", "claude-api-crypto-payment", "how-to-buy-claude-api-key", "claude-api-without-waitlist"],
};
