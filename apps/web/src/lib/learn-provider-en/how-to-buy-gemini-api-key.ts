import type { LearnArticle } from "../learn";
import { ROUTER } from "./shared";

export const article: LearnArticle = {
    slug: "how-to-buy-gemini-api-key",
    cluster: "buy",
    title: "How to Buy a Gemini API Key",
    h1: "How to buy a Gemini API key",
    description: "Buy a Gemini API key with prepaid balance, card or crypto payment, native Google Gemini endpoints and one account for Gemini, GPT, Claude and Kimi at 50% off official spend.",
    keywords: ["buy gemini api key", "gemini api key", "google gemini api access", "gemini api prepaid", "gemini api payment", "cheap gemini api"],
    dek: "An apiToken.sale key gives you native Gemini API access without a separate Google Cloud billing setup. Top up once, send the key as x-goog-api-key, and use the same balance across every supported provider.",
    sections: [
      { h2: "Get a Gemini key in three steps", blocks: [
        { type: "steps", items: [
          "Create an apiToken.sale account and generate one sk-pool key in the dashboard.",
          "Add any whole-dollar prepaid amount by card or crypto; the balance does not expire.",
          "Set the Gemini base URL to " + ROUTER + ", send the key as x-goog-api-key, and choose a model returned by GET /v1beta/models.",
        ] },
        { type: "code", code: "curl " + ROUTER + "/v1beta/models/gemini-3.6-flash:generateContent \\\n  -H \"x-goog-api-key: $APITOKEN_API_KEY\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"contents\":[{\"parts\":[{\"text\":\"Reply with exactly: connected\"}]}]}'" },
      ] },
      { h2: "What you can run", blocks: [
        { type: "list", items: [
          "Pro, Flash and Flash-Lite text models on the native Gemini protocol.",
          "Gemini 3.1 Flash Image (Nano Banana 2) for image generation.",
          "generateContent, streamGenerateContent and countTokens with Google-shaped requests and responses.",
          "A flat 50% B2C discount and the same key/balance used by GPT, Claude and Kimi.",
        ] },
        { type: "note", text: "Use the bare host as the Google SDK base URL. The SDK appends /v1beta itself; adding it twice produces a 404." },
      ] },
    ],
    faq: [
      { q: "Do I need a Google Cloud project for this Gemini key?", a: "No. apiToken.sale owns the gateway account and billing; your client only needs the custom base URL and sk-pool key." },
      { q: "Which header authenticates Gemini requests?", a: "x-goog-api-key. Do not use the Anthropic x-api-key header or OpenAI Authorization: Bearer on the native Gemini routes." },
      { q: "Can the same key call GPT and Gemini?", a: "Yes. The key and balance are shared; switch the endpoint, protocol and model ID for each provider." },
    ],
    related: ["gemini-api-quickstart", "gemini-api-pricing", "gemini-pro-vs-flash-vs-flash-lite", "how-billing-works"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
