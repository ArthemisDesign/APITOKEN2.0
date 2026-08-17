import type { LearnArticle } from "../learn";
import { ROUTER } from "./shared";

export const article: LearnArticle = {
    slug: "gemini-api-quickstart",
    cluster: "integrate",
    title: "Gemini API Quickstart",
    h1: "Gemini API quickstart: curl and Google GenAI SDK",
    description: "Make your first Gemini API call through apiToken.sale with curl or the Google GenAI SDK, native generateContent, x-goog-api-key and an explicit Gemini model ID.",
    keywords: ["gemini api quickstart", "gemini api tutorial", "google genai sdk base url", "gemini generatecontent", "gemini api curl", "gemini api example"],
    dek: "The gateway preserves the native Google Gemini protocol. Change the base URL and API key, keep generateContent and the official SDK shapes, and always select an explicit model.",
    sections: [
      { h2: "First request with curl", blocks: [
        { type: "code", code: "curl " + ROUTER + "/v1beta/models/gemini-3.6-flash:generateContent \\\n  -H \"x-goog-api-key: $APITOKEN_API_KEY\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"contents\":[{\"parts\":[{\"text\":\"Reply with exactly: connected\"}]}]}'" },
        { type: "p", text: "For incremental output, call streamGenerateContent?alt=sse. countTokens is available on the same model path when you want a free input estimate before generation." },
      ] },
      { h2: "Use the official Python SDK", blocks: [
        { type: "code", code: [
          "import os",
          "from google import genai",
          "from google.genai import types",
          "",
          "client = genai.Client(",
          "    api_key=os.environ[\"APITOKEN_API_KEY\"],",
          "    http_options=types.HttpOptions(base_url=\"" + ROUTER + "\"),",
          ")",
          "",
          "response = client.models.generate_content(",
          "    model=\"gemini-3.6-flash\",",
          "    contents=\"Reply with exactly: connected\",",
          ")",
          "print(response.text)",
        ].join("\n") },
        { type: "list", items: [
          "Pass the bare base URL; do not append /v1beta in SDK configuration.",
          "Pass a concrete model ID. A client's automatic default may not be in the gateway catalog.",
          "Keep APITOKEN_API_KEY in the environment rather than source code.",
        ] },
      ] },
    ],
    faq: [
      { q: "Does the official Google GenAI SDK work?", a: "Yes. Set HttpOptions(base_url) to https://router.apitoken.sale and provide the apiToken.sale key; request and response shapes stay native." },
      { q: "How do I stream Gemini output?", a: "Use /v1beta/models/{model}:streamGenerateContent?alt=sse with x-goog-api-key, or the matching SDK streaming method." },
      { q: "Why does a doubled /v1beta return 404?", a: "The Google SDK appends its API version. Configure only the bare host so the final request contains one /v1beta segment." },
    ],
    related: ["how-to-buy-gemini-api-key", "gemini-api-pricing", "gemini-pro-vs-flash-vs-flash-lite", "nano-banana-2-api-guide"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
