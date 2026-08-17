import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-litellm",
  cluster: "integrate",
  title: "Use the Claude API with LiteLLM",
  h1: "Use the Claude API with LiteLLM",
  description: "Route LiteLLM to Claude through apiToken.sale: set api_base to router.apitoken.sale in litellm_params or the proxy config and pay 50% less per token.",
  keywords: ["claude api litellm", "litellm anthropic", "litellm claude", "litellm api_base anthropic", "litellm proxy claude", "litellm claude api key"],
  dek: "LiteLLM speaks to Anthropic natively and lets you override the endpoint per model, so one config line sends all your Claude traffic through the discounted gateway.",
  published: "2026-07-17",
  updated: "2026-07-17",
  sections: [
    { h2: "Direct SDK call", blocks: [
      { type: "code", code: `import litellm\n\nresponse = litellm.completion(\n    model="anthropic/claude-opus-4-8",\n    api_base="${BASE}",\n    api_key="${KEY}",\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
      cta(),
    ] },
    { h2: "LiteLLM proxy config", blocks: [
      { type: "code", code: `# config.yaml\nmodel_list:\n  - model_name: claude-opus-4-8\n    litellm_params:\n      model: anthropic/claude-opus-4-8\n      api_base: ${BASE}\n      api_key: ${KEY}` },
      { type: "p", text: "Run the proxy with this config and every client of your LiteLLM gateway transparently uses the discounted Claude endpoint — useful when many services share one routing layer." },
    ] },
    { h2: "Why route Claude through LiteLLM here", blocks: [
      { type: "list", items: [
        "One place to switch all services to the cheaper endpoint.",
        "Same anthropic/ model prefix and parameters you already use.",
        "Spend tracked per key in the apiToken.sale dashboard with token detail.",
      ] },
    ] },
  ],
  faq: [
    { q: "Does LiteLLM support a custom Anthropic api_base?", a: "Yes — pass api_base in litellm.completion() or in litellm_params in the proxy config, and LiteLLM sends Anthropic-format requests to https://router.apitoken.sale." },
    { q: "Do I keep the anthropic/ model prefix?", a: "Yes. Use anthropic/claude-opus-4-8 (or any supported model) so LiteLLM applies the Anthropic protocol; only the endpoint and key change." },
    { q: "Does this work for tools built on LiteLLM?", a: "Yes — anything that routes through LiteLLM (including many coding agents) inherits the discounted endpoint from the same configuration." },
  ],
  related: ["claude-api-langchain", "claude-api-aider", "anthropic-sdk-base-url", "claude-api-gateway"],
};
