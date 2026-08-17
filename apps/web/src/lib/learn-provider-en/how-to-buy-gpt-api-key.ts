import type { LearnArticle } from "../learn";
import { OPENAI } from "./shared";

export const article: LearnArticle = {
    slug: "how-to-buy-gpt-api-key",
    cluster: "buy",
    title: "How to Buy a GPT API Key",
    h1: "How to buy a GPT API key",
    description: "Buy a GPT API key with prepaid balance, card or crypto payment, and one OpenAI-compatible endpoint for GPT-5.6, GPT-5.5 and GPT Image 2 at 50% off official spend.",
    keywords: ["buy gpt api key", "gpt api key", "buy openai api key", "gpt-5.6 api access", "openai compatible api key", "gpt api prepaid"],
    dek: "One apiToken.sale key opens the GPT catalog without a separate OpenAI Platform account. Add prepaid balance, use the OpenAI-compatible endpoint, and pay 50% less than official token spend on every request.",
    sections: [
      { h2: "Get a GPT key in three steps", blocks: [
        { type: "steps", items: [
          "Create an apiToken.sale account and generate a key in the dashboard.",
          "Top up any whole-dollar amount by card or crypto; there is no fixed bundle or monthly commitment.",
          `Set the OpenAI base URL to ${OPENAI}, authenticate with Authorization: Bearer, and choose a model returned by GET /v1/models.`,
        ] },
        { type: "code", code: `curl ${OPENAI}/responses \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{"model":"gpt-5.6-terra","input":"Reply with exactly: connected"}'` },
      ] },
      { h2: "What the key includes", blocks: [
        { type: "list", items: [
          "Responses and Chat Completions with incremental SSE streaming.",
          "GPT-5.6 Sol, Terra and Luna, previous GPT tiers, and the separate GPT Image 2 routes.",
          "The same prepaid balance and key also cover supported Claude, Gemini and Kimi models.",
          "A flat 50% B2C discount applied to official provider spend on every request.",
        ] },
        { type: "note", text: "Keep the key in a server-side environment variable. A GPT call uses Authorization: Bearer; x-api-key and x-goog-api-key belong to the Anthropic and Gemini protocols." },
      ] },
    ],
    faq: [
      { q: "Do I need an OpenAI account to buy this GPT API key?", a: "No. The key, balance and billing come from apiToken.sale; compatible GPT clients only need the custom base URL and Bearer key." },
      { q: "Can one key run both GPT and Claude?", a: "Yes. The same sk-pool key and balance cover all supported providers; only the endpoint and authorization header change with the protocol." },
      { q: "Is this the OpenAI Platform?", a: "No. It is an independent OpenAI-compatible gateway with its own account, prepaid balance and supported-model catalog." },
    ],
    related: ["openai-api-quickstart", "gpt-api-pricing", "gpt-5-6-sol-vs-terra-vs-luna", "how-billing-works"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
