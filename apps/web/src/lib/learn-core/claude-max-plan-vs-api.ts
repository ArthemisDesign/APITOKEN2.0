import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-max-plan-vs-api",
  cluster: "compare",
  title: "Claude Max Plan vs the Claude API",
  h1: "Claude Max subscription vs the API",
  description: "When to use a Claude subscription vs the Claude API. apiToken.sale gives pay-as-you-go API access to every model with no monthly fee and a flat 50% off.",
  keywords: ["claude max plan", "claude subscription vs api", "claude max vs api", "claude api pay as you go", "claude without subscription", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api", "claude api tokens"],
  dek: "A flat Claude subscription and pay-as-you-go API billing suit different usage. For programmatic and bursty use, the API on prepaid balance is usually the better deal.",
  sections: [
    { h2: "Subscription vs per-token", blocks: [
      { type: "p", text: "A fixed monthly plan makes sense for steady, heavy interactive use in one app. But it is wasteful for spiky usage, and it does not give you a programmable API key for your own tools." },
    ] },
    { h2: "Why the API often wins", blocks: [
      { type: "list", items: [
        "Pay only for the tokens you actually use — no monthly floor.",
        "One key drives Claude Code, Cursor, agents and production calls.",
        "apiToken.sale takes a flat 50% off official token rates.",
      ] },
      cta(),
    ] },
  ],
  faq: [
    { q: "Is the API cheaper than a Claude subscription?", a: "For bursty or programmatic usage, pay-as-you-go API billing avoids paying a flat monthly fee for idle time, and apiToken.sale discounts it further." },
    { q: "Can I use the API in coding tools?", a: "Yes — the API key works in Claude Code, Cursor, VS Code agents and the SDKs, which a subscription does not provide." },
  ],
  related: ["claude-code-without-subscription", "claude-api-pricing-explained", "cheapest-claude-api", "how-billing-works"],
};
