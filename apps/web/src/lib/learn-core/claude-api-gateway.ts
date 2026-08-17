import type { LearnArticle } from "../learn";
import { cta, BASE } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-gateway",
  cluster: "explain",
  title: "What Is a Claude API Gateway?",
  h1: "What a Claude API gateway is",
  description: "A Claude API gateway sits between your tools and Anthropic, adding access, billing and control. apiToken.sale is a native gateway with a flat 50% discount.",
  keywords: ["claude api gateway", "what is an api gateway", "anthropic gateway", "claude proxy", "claude api access layer", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
  dek: "A gateway is a thin layer between your code and the model provider. A good Claude gateway is invisible to your tools while improving access, price and control.",
  sections: [
    { h2: "What a gateway does", blocks: [
      { type: "list", items: [
        "Presents the standard Anthropic Messages API so tools work unchanged.",
        "Handles access and billing — here, prepaid balance at a discount.",
        "Adds per-key lifetime spending limits, optional expiration and usage visibility.",
      ] },
    ] },
    { h2: "Native, not a translation layer", blocks: [
      { type: "p", text: `apiToken.sale is Anthropic-native: point any client at ${BASE}/v1/messages and it behaves exactly like api.anthropic.com — plus your discount and dashboard controls.` },
      cta(),
    ] },
    { h2: "What to look for in a gateway", blocks: [
      { type: "list", items: [
        "Native Anthropic API, so tools and SDKs work unchanged.",
        "Transparent per-token billing you can audit in a dashboard.",
        "Per-key controls: an optional lifetime spending limit and expiration date.",
        "No lock-in — prepaid balance that never expires.",
      ] },
    ] },
  ],
  faq: [
    { q: "Does a gateway change the API?", a: "No. A native Claude gateway speaks the standard Anthropic Messages API, so your tools and SDKs are unchanged." },
    { q: "Why use a gateway instead of Anthropic directly?", a: "For a discount, instant access without an Anthropic account, and optional lifetime spending limits and expiration dates for individual keys." },
  ],
  related: ["apitoken-vs-anthropic-direct", "claude-api-key-security", "cheapest-claude-api", "anthropic-sdk-base-url"],
};
