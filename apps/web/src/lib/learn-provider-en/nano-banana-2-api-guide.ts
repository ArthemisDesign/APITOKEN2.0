import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";
import { ROUTER } from "./shared";

export const article: LearnArticle = {
    slug: "nano-banana-2-api-guide",
    cluster: "integrate",
    title: "Nano Banana 2 API Guide",
    h1: "Generate images with the Nano Banana 2 API",
    description: "Use Gemini 3.1 Flash Image (Nano Banana 2) through the native Gemini API: exact model ID, generateContent requests, image-size controls, per-leg pricing and a flat 50% discount.",
    keywords: ["nano banana 2 api", "gemini 3.1 flash image api", "nano banana 2 api key", "gemini image generation api", "gemini-3.1-flash-image model id", "nano banana 2 generatecontent", "nano banana 2 image editing api", "gemini flash image pricing", "nano banana 2 image size", "google genai sdk image generation"],
    dek: "Nano Banana 2 is the public name for Gemini 3.1 Flash Image, and the Nano Banana 2 API is the native Gemini generateContent route with one exact model ID: gemini-3.1-flash-image. It accepts multimodal input, returns rendered image parts next to text in the same response, and bills against the same prepaid balance as Gemini text calls at half the official per-token rates.",
    sections: [
      { h2: "One model ID on the native Gemini route", blocks: [
        { type: "p", text: "Nano Banana 2 is not a separate product with its own endpoint. It is the public name for Gemini 3.1 Flash Image, and you call it through the native Gemini generateContent route with the exact model ID gemini-3.1-flash-image. Your apiToken.sale key goes in the x-goog-api-key header, and every call settles against the same prepaid balance as Gemini text models." },
        { type: "code", code: "curl " + ROUTER + "/v1beta/models/gemini-3.1-flash-image:generateContent \\\n  -H \"x-goog-api-key: $APITOKEN_API_KEY\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"contents\":[{\"parts\":[{\"text\":\"Create a clean isometric diagram of a satellite\"}]}]}'" },
        { type: "p", text: "The response is a standard generateContent payload. Parse candidates[0].content.parts by MIME type: text parts carry the model's commentary, and image parts carry the rendered asset as base64 inline data. One response can mix both, so never assume the first part is the image — iterate the array and branch on the MIME type of each part." },
        { type: "note", text: "The gateway routes by model ID, not by marketing name. Send gemini-3.1-flash-image exactly; \"nano-banana-2\" is a nickname, not a valid ID." },
      ] },
      { h2: "Control size and aspect ratio in generationConfig", blocks: [
        { type: "p", text: "Two fields in generationConfig decide what you pay for and what you can ship. imageConfig.imageSize selects the output class — the live route accepts 1K, 2K and 4K — and imageConfig.aspectRatio fixes the frame. responseModalities declares what the response may contain; pass [\"TEXT\",\"IMAGE\"] when you want commentary next to the render, or [\"IMAGE\"] alone when you only want pixels." },
        { type: "code", code: "curl " + ROUTER + "/v1beta/models/gemini-3.1-flash-image:generateContent \\\n  -H \"x-goog-api-key: $APITOKEN_API_KEY\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"contents\":[{\"parts\":[{\"text\":\"A square product illustration of a matte black travel mug\"}]}],\"generationConfig\":{\"responseModalities\":[\"TEXT\",\"IMAGE\"],\"imageConfig\":{\"imageSize\":\"1K\",\"aspectRatio\":\"1:1\"}}}'" },
        { type: "list", items: [
          "Start at 1K and promote an asset to 2K or 4K only when it fails a delivery-resolution check — image output is the expensive leg.",
          "State the aspect ratio explicitly instead of describing it in prose; a prose request can be interpreted loosely, the parameter cannot.",
          "Keep the prompt concrete: subject, material, lighting, background and framing in one or two sentences beats a paragraph of adjectives.",
        ] },
      ] },
      { h2: "Edit existing images with multimodal input", blocks: [
        { type: "p", text: "The same route edits as well as generates. Put the source image into contents as an inline_data part with its MIME type and base64 payload, then add a text part with the instruction. The model returns the edited render as a new image part, so generation and editing share one code path — only the request contents differ." },
        { type: "code", code: "{\n  \"contents\": [{\n    \"parts\": [\n      { \"inline_data\": { \"mime_type\": \"image/png\", \"data\": \"<base64 source image>\" } },\n      { \"text\": \"Replace the background with a soft studio gradient, keep the product untouched\" }\n    ]\n  }],\n  \"generationConfig\": {\n    \"responseModalities\": [\"TEXT\", \"IMAGE\"],\n    \"imageConfig\": { \"imageSize\": \"1K\", \"aspectRatio\": \"1:1\" }\n  }\n}" },
        { type: "p", text: "Every reference image you attach is billed as input tokens, so keep the set bounded. Reuse one small, curated set of references across candidates instead of uploading a large collection with every attempt." },
      ] },
      { h2: "Pricing per leg and model limits", blocks: [
        { type: "p", text: "Flash Image is metered per token on three legs, exactly like a text model — the image leg is simply a fourth, more expensive one. apiToken.sale applies a flat 50% discount after official usage is calculated, so every leg lands at half price on a regular account." },
        { type: "table", headers: ["Leg", "Official per 1M tokens", "Here after 50%"], rows: [
          ["Text input", "$0.50", "$0.25"],
          ["Text output", "$3", "$1.50"],
          ["Image output", "$60 per 1M image tokens", "$30 per 1M image tokens"],
        ] },
        { type: "list", items: [
          "Context window is 128K tokens with up to 32K of output — smaller than the text Flash line, so trim long reference sets and prompts.",
          "Image output is metered in image tokens, not per file; the billed amount scales with the size class you selected.",
          "Cached input is not discounted for this image model: it bills at the full $0.50 official input rate.",
          "All legs draw from the same prepaid balance as your Claude, GPT, Gemini and Kimi calls — no separate image plan.",
        ] },
        { type: "note", text: "Flash Image earns its price only when the response must contain pixels. For prompt rewriting, captioning or classification, call a text Flash model on the same key — its output leg is an order of magnitude cheaper than the image leg." },
      ] },
      { h2: "Generate from the Google GenAI SDK", blocks: [
        { type: "p", text: "The gateway preserves the native Gemini protocol, so the official Google GenAI SDK works with two changes: your key and the base URL. Request and response shapes stay exactly as documented by Google." },
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
          "    model=\"gemini-3.1-flash-image\",",
          "    contents=\"A clean isometric diagram of a satellite\",",
          ")",
          "for part in response.candidates[0].content.parts:",
          "    if part.inline_data is not None:",
          "        with open(\"satellite.png\", \"wb\") as f:",
          "            f.write(part.inline_data.data)",
          "    elif part.text:",
          "        print(part.text)",
        ].join("\n") },
        { type: "note", text: "Pass the bare host as base_url. The SDK appends /v1beta itself; a doubled /v1beta/v1beta path returns a 404." },
      ] },
      { h2: "First call checklist and budget guardrails", blocks: [
        { type: "steps", items: [
          "Create a free account and generate a key in the dashboard — it looks like sk-pool-… and works across the supported Claude, GPT, Gemini and Kimi models.",
          "Run a free countTokens call on gemini-3.1-flash-image to estimate the input leg before you buy any image output.",
          "Send the minimal 1K request above and confirm you can decode and save the returned image part.",
          "Open the dashboard and reconcile the call: token usage per leg, the applied 50% discount and the remaining balance are visible after each request.",
        ] },
        { type: "p", text: "Top-ups are whole-dollar amounts and the prepaid balance never expires — you pay per token when you generate, and nothing when you do not. For a per-size cost breakdown of the 1K, 2K and 4K image-output legs, see the companion cost guide." },
        { type: "link", text: "Nano Banana 2 API cost by image size", href: "/docs/learn/nano-banana-2-api-cost" },
        { type: "link", text: "Full model catalog and per-model pricing", href: "/models" },
        cta(),
      ] },
    ],
    faq: [
      { q: "What is the exact API model ID for Nano Banana 2?", a: "gemini-3.1-flash-image on the native Gemini generateContent route. \"Nano Banana 2\" is the public nickname and is not accepted as a model ID." },
      { q: "How much does Nano Banana 2 image output cost?", a: "Officially $60 per 1M image-output tokens, or $30 after the flat 50% apiToken.sale discount. Text input and output are separate legs at $0.50/$3 official, $0.25/$1.50 here." },
      { q: "Does Nano Banana 2 need a separate image API key?", a: "No. Use the same sk-pool key in the x-goog-api-key header and the same prepaid balance as Gemini text calls." },
      { q: "Can Nano Banana 2 edit an existing image?", a: "Yes. Send the source as an inline_data part with its MIME type and base64 data next to a text instruction in contents; the edited render comes back as a new image part." },
      { q: "What context and output limits does Gemini 3.1 Flash Image have?", a: "A 128K-token context window and up to 32K tokens of output — smaller than the text Flash line, so keep prompts and reference sets bounded." },
      { q: "Is cached input cheaper for Nano Banana 2?", a: "No. For this image model, cached input bills at the full $0.50 official input rate — do not budget a cache discount." },
    ],
    related: ["nano-banana-2-api-cost", "nano-banana-2-vs-gpt-image-2", "image-generation-api-pricing", "batch-image-generation-api"],
    published: "2026-08-09",
    updated: "2026-08-17",
  };
