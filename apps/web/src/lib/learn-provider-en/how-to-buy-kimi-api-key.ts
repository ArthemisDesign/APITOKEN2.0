import type { LearnArticle } from "../learn";
import { ROUTER } from "./shared";

export const article: LearnArticle = {
    slug: "how-to-buy-kimi-api-key",
    cluster: "buy",
    title: "How to Buy a Kimi API Key",
    h1: "How to buy a Kimi API key",
    description: "Buy one prepaid API key for Kimi K3 and Kimi for Coding, use Anthropic Messages or OpenAI-compatible clients, and pay 50% less than official API spend.",
    keywords: ["buy kimi api key", "kimi api key", "kimi k3 api", "kimi for coding api", "moonshot kimi api", "kimi api prepaid"],
    dek: "Kimi is available as its own model namespace on the unified router. Use the native Anthropic Messages lane or an OpenAI-compatible client, while usage settles against the same prepaid balance as Claude, GPT and Gemini.",
    sections: [
      { h2: "Get access in three steps", blocks: [
        { type: "steps", items: [
          "Create an apiToken.sale account and generate a sk-pool key.",
          "Top up any whole-dollar amount by card or crypto; no separate Kimi plan is required on your side.",
          "Read GET https://router.apitoken.sale/v1/models and choose a kimi/* ID that the live catalog exposes for your key.",
        ] },
        { type: "code", code: "curl " + ROUTER + "/v1/messages \\\n  -H \"x-api-key: $APITOKEN_API_KEY\" \\\n  -H \"anthropic-version: 2023-06-01\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"model\":\"kimi/kimi-for-coding\",\"max_tokens\":256,\"messages\":[{\"role\":\"user\",\"content\":\"Reply with exactly: connected\"}]}'" },
      ] },
      { h2: "What makes the Kimi route different", blocks: [
        { type: "list", items: [
          "Kimi is a provider namespace, not a fourth wire format: use POST /v1/messages with x-api-key or the universal OpenAI-compatible /v1 lane.",
          "Public IDs are subscription aliases such as kimi/k3 and kimi/kimi-for-coding, not internal tariff model names.",
          "K3 exposes 256K and 1M context spellings; Kimi for Coding has normal and high-speed aliases.",
          "The live /v1/models response is authoritative because model availability can depend on provider capacity and account policy.",
        ] },
      ] },
    ],
    faq: [
      { q: "Does Kimi need a separate API key?", a: "No. The same sk-pool key and balance cover Kimi and the other supported providers." },
      { q: "Which endpoint does Kimi use?", a: "Use https://router.apitoken.sale/v1/messages for Anthropic Messages, or the router's /v1 Chat Completions lane for an OpenAI-compatible client. Both accept public kimi/* IDs." },
      { q: "Why should I check /v1/models first?", a: "The catalog is scoped to the key and only returns models that are currently routable and priced for it." },
    ],
    related: ["kimi-api-quickstart", "kimi-api-for-claude-code", "kimi-api-for-kimi-code", "kimi-api-pricing"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
