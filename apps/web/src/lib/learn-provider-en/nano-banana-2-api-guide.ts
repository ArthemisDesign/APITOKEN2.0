import type { LearnArticle } from "../learn";
import { ROUTER } from "./shared";

export const article: LearnArticle = {
    slug: "nano-banana-2-api-guide",
    cluster: "integrate",
    title: "Nano Banana 2 API Guide",
    h1: "Generate images with the Nano Banana 2 API",
    description: "Use Gemini 3.1 Flash Image (Nano Banana 2) through the native Gemini API: exact model ID, generateContent request, image-output pricing and a flat 50% discount.",
    keywords: ["nano banana 2 api", "gemini 3.1 flash image api", "gemini image generation api", "nano banana api key", "gemini image price", "google image api"],
    dek: "Nano Banana 2 is the public name for Gemini 3.1 Flash Image. It uses the native Gemini generateContent route, accepts multimodal input and returns rendered image parts on the same balance as text models.",
    sections: [
      { h2: "Use the exact model ID", blocks: [
        { type: "code", code: "curl " + ROUTER + "/v1beta/models/gemini-3.1-flash-image:generateContent \\\n  -H \"x-goog-api-key: $APITOKEN_API_KEY\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"contents\":[{\"parts\":[{\"text\":\"Create a clean isometric diagram of a satellite\"}]}]}'" },
        { type: "p", text: "Parse the returned parts by MIME type: text parts contain commentary and image parts contain the rendered asset. Use the exact ID gemini-3.1-flash-image rather than the marketing nickname." },
      ] },
      { h2: "Limits and pricing", blocks: [
        { type: "list", items: [
          "128K context and up to 32K output, smaller than the text Flash line.",
          "Official text input/output rates are $0.50/$3 per 1M; image output is $60 per 1M image tokens.",
          "apiToken.sale prices those legs at $0.25/$1.50 and $30 after the flat 50% discount.",
          "Cached input remains at the full $0.50 official input rate for this image model.",
        ] },
        { type: "note", text: "Use a text Flash model when you only need text. Flash Image is valuable when the response must contain a rendered image, and its image-output leg is priced separately." },
      ] },
    ],
    faq: [
      { q: "What is the API model ID for Nano Banana 2?", a: "gemini-3.1-flash-image on the native Gemini generateContent route." },
      { q: "How much does Nano Banana 2 image output cost?", a: "$60 per 1M image-output tokens officially, or $30 after the flat 50% apiToken.sale discount." },
      { q: "Does it use a separate image API key?", a: "No. Use the same sk-pool key in x-goog-api-key and the same prepaid balance as Gemini text calls." },
    ],
    related: ["nano-banana-2-api-cost", "nano-banana-2-vs-gpt-image-2", "image-generation-api-pricing", "batch-image-generation-api"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
