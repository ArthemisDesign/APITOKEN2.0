import type { LearnArticle } from "../learn";
import { OPENAI } from "./shared";

export const article: LearnArticle = {
    slug: "gpt-image-2-api-guide",
    cluster: "integrate",
    title: "GPT Image 2 API Guide",
    h1: "Generate and edit images with the GPT Image 2 API",
    description: "Use GPT Image 2 for image generation and editing through apiToken.sale: exact endpoint, model ID, reference-image limits, token pricing and a 50% discount.",
    keywords: ["gpt image 2 api", "gpt-image-2", "openai image generation api", "gpt image edit api", "gpt image pricing", "image generation api"],
    dek: "GPT Image 2 uses dedicated image routes but the same apiToken.sale key and balance as GPT text models. Generate from a prompt or edit up to five PNG references without opening a separate image plan.",
    sections: [
      { h2: "Call the generation route", blocks: [
        { type: "code", code: `curl ${OPENAI}/images/generations \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{"model":"gpt-image-2","prompt":"A precise technical cutaway of a lunar rover"}'` },
        { type: "p", text: "For edits, send multipart/form-data to /v1/images/edits with the same model and up to five PNG reference images. The current surface returns one non-streaming PNG per call." },
      ] },
      { h2: "How image billing works", blocks: [
        { type: "table", headers: ["Leg", "Official per 1M tokens", "Price here"], rows: [
          ["Text input", "$5", "$2.50"],
          ["Image input", "$8", "$4"],
          ["Image output", "$30", "$15"],
        ] },
        { type: "list", items: [
          "Cached text and image input bills at 25% of the normal input rate.",
          "gpt-image-2 aliases the immutable gpt-image-2-2026-04-21 snapshot.",
          "Image usage settles against the same prepaid balance as GPT, Claude and Gemini calls.",
        ] },
      ] },
    ],
    faq: [
      { q: "What endpoint does GPT Image 2 use?", a: "POST /v1/images/generations for a new image and POST /v1/images/edits for reference-based edits on the OpenAI-compatible base URL." },
      { q: "Can GPT Image 2 edit an existing image?", a: "Yes. The edits route accepts up to five PNG reference images in multipart/form-data." },
      { q: "Does image generation need a separate key or balance?", a: "No. It uses the same Bearer key and prepaid balance as all other supported models." },
    ],
    related: ["gpt-image-2-api-cost", "nano-banana-2-vs-gpt-image-2", "image-editing-api-guide", "image-generation-api-pricing"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
