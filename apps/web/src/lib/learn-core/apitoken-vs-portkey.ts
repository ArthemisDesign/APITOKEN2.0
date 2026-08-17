import type { LearnArticle } from "../learn";
import { cta, BASE } from "../learn-shared";

export const article: LearnArticle = {
  slug: "apitoken-vs-portkey",
  cluster: "compare",
  title: "apiToken.sale vs Portkey for Claude",
  h1: "apiToken.sale vs Portkey",
  description: "Portkey is an AI gateway for routing and observability using your own provider keys. apiToken.sale provides the Claude key and balance itself, at a discount. Here is when to use each.",
  keywords: ["portkey alternative", "apitoken vs portkey", "ai gateway claude", "portkey claude api", "claude api gateway", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
  dek: "These tools solve different problems. Portkey sits in front of provider keys you already own; apiToken.sale is where the Claude key and discounted balance come from.",
  sections: [
    { h2: "Different jobs", blocks: [
      { type: "p", text: "Portkey adds routing, caching, and observability on top of API keys you bring. It does not sell you Claude access or a discount — you still need a funded Anthropic account behind it." },
      { type: "p", text: `apiToken.sale is the source of the key and balance: a native Anthropic endpoint at ${BASE} with a flat 50% off, no Anthropic account required.` },
    ] },
    { h2: "They can even combine", blocks: [
      { type: "p", text: "If you like Portkey's observability, you can point it at an apiToken.sale key as the Anthropic provider and get the discount underneath." },
      cta(),
    ] },
  ],
  faq: [
    { q: "Does Portkey give me a Claude discount?", a: "No — Portkey is a gateway over keys you already own. apiToken.sale is what provides the discounted Claude key and balance." },
    { q: "Can I use both together?", a: "Yes. Use an apiToken.sale key as Portkey's Anthropic provider to keep observability while paying less." },
  ],
  related: ["apitoken-vs-openrouter", "claude-api-gateway", "cheapest-claude-api", "anthropic-sdk-base-url"],
};
