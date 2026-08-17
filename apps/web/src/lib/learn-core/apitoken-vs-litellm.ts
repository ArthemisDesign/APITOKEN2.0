import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "apitoken-vs-litellm",
  cluster: "compare",
  title: "apiToken.sale vs LiteLLM for Claude",
  h1: "apiToken.sale vs LiteLLM",
  description: "LiteLLM is a self-hosted proxy that unifies model APIs but needs your own funded keys. apiToken.sale is a hosted, discounted Claude endpoint with nothing to run.",
  keywords: ["litellm alternative", "apitoken vs litellm", "litellm claude", "self-hosted claude proxy", "claude api hosted", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
  dek: "LiteLLM is great if you want to self-host a proxy across many providers. apiToken.sale is the opposite trade-off: nothing to run, and the Claude balance comes discounted.",
  sections: [
    { h2: "Self-hosted vs hosted", blocks: [
      { type: "list", items: [
        "LiteLLM: you run and maintain the proxy, and you still fund each provider yourself.",
        "apiToken.sale: fully hosted native Anthropic endpoint, no infrastructure to manage.",
        "apiToken.sale adds a flat 50% discount on Claude spend that a bare proxy cannot.",
      ] },
      cta(),
    ] },
    { h2: "When to choose each", blocks: [
      { type: "list", items: [
        "apiToken.sale — you want a hosted, discounted Claude endpoint with nothing to run.",
        "LiteLLM — you want to self-host a unified proxy across many providers you fund yourself.",
        "You can even put LiteLLM in front of an apiToken.sale key to keep the discount underneath.",
      ] },
    ] },
  ],
  faq: [
    { q: "Does LiteLLM discount Claude?", a: "No. LiteLLM routes to providers you fund yourself; the discount comes from apiToken.sale's pooled prepaid balance." },
    { q: "Do I need to host anything with apiToken.sale?", a: "No — it is a hosted endpoint. You only change your base URL and key." },
  ],
  related: ["apitoken-vs-portkey", "apitoken-vs-openrouter", "claude-api-gateway", "anthropic-sdk-base-url"],
};
