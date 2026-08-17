import type { LearnArticle } from "../learn";
import { OPENAI } from "./shared";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
    slug: "gpt-image-2-api-guide",
    cluster: "integrate",
    title: "GPT Image 2 API Guide",
    h1: "Generate and edit images with the GPT Image 2 API",
    description: "Use GPT Image 2 for image generation and editing through apiToken.sale: exact endpoint, model ID, reference-image limits, token pricing and a 50% discount.",
    keywords: ["gpt image 2 api", "gpt-image-2", "gpt image 2 api guide", "openai image generation api", "gpt image edit api", "gpt image 2 pricing", "images generations endpoint", "openai images edits api", "gpt image 2 model id", "image generation api prepaid"],
    dek: "The GPT Image 2 API on apiToken.sale is two routes — POST /v1/images/generations for new assets and POST /v1/images/edits for reference-based edits — behind the same Bearer key and prepaid balance as your GPT text calls. Usage bills per token at half the official OpenAI rates. This guide gives the exact requests, the SDK path, the real price table and the surface limits worth knowing before you ship.",
    sections: [
      { h2: "The generation route in one request", blocks: [
        { type: "p", text: "GPT Image 2 is an image model you call over the OpenAI-compatible surface: send a prompt to /v1/images/generations with model gpt-image-2 and an Authorization: Bearer header, and you get back one PNG. No separate image plan, no second key — the same sk-pool credential and prepaid balance that cover GPT, Claude and Gemini calls settle image usage too." },
        { type: "code", code: `curl ${OPENAI}/images/generations \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{\n    "model": "gpt-image-2",\n    "prompt": "A precise technical cutaway of a lunar rover",\n    "background": "opaque",\n    "quality": "low",\n    "size": "auto"\n  }'` },
        { type: "p", text: "The three control fields shown are the documented set on this surface: background accepts opaque, quality accepts low, and size accepts auto. Requests outside that profile are rejected rather than silently approximated — a transparent background, for example, returns an error instead of a flattened PNG. Keep the shipped profile bounded and treat any extra parameter as unsupported until a call proves otherwise." },
        { type: "note", text: "The current surface returns one non-streaming PNG per call. Do not build a progress UI around streamed partial images, and set the client timeout for a single blocking request that renders a full asset." },
      ] },
      { h2: "Edit existing images with up to five PNG references", blocks: [
        { type: "p", text: "Edits go to a different route and a different content type. POST multipart/form-data to /v1/images/edits with the same gpt-image-2 model, your prompt, and between one and five PNG reference images. The references are how you ask for a targeted change — restyle this product shot, swap this background, extend this banner — instead of regenerating from scratch." },
        { type: "code", code: `curl ${OPENAI}/images/edits \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -F "model=gpt-image-2" \\\n  -F "prompt=Replace the backdrop with a seamless light-gray studio sweep" \\\n  -F "image[]=@packshot.png" \\\n  -F "image[]=@brand-swatch.png"` },
        { type: "list", items: [
          "References must be PNG files; convert JPEG or WebP assets before upload instead of relying on server-side coercion.",
          "The cap is five references per call — pick the few that carry the instruction rather than dumping the whole asset library.",
          "Every reference is billed as image input, so edits cost more than a pure-prompt generation of the same output.",
          "The response shape matches generation: one non-streaming PNG per call.",
        ] },
        { type: "link", text: "Deeper editing workflows: masks, batches and acceptance checks", href: "/docs/learn/image-editing-api-guide" },
      ] },
      { h2: "Call it from the official OpenAI SDK", blocks: [
        { type: "p", text: "No custom HTTP layer is needed in application code. The official OpenAI SDKs expose the images API, and switching the client to apiToken.sale is the same two constructor arguments as for text models: base_url and api_key." },
        { type: "code", code: `import os\nfrom openai import OpenAI\n\nclient = OpenAI(\n    api_key=os.environ["APITOKEN_API_KEY"],\n    base_url="${OPENAI}",\n)\n\nresult = client.images.generate(\n    model="gpt-image-2",\n    prompt="A clean isometric diagram of a wind turbine",\n    quality="low",\n    size="auto",\n)\n\npng_bytes = result.data[0].b64_json  # decode base64 and write to disk` },
        { type: "p", text: "For edits, the same client exposes images.edits with the reference files opened in binary mode. Keep the key in a server-side environment variable; image endpoints are exactly as sensitive as chat ones because they draw from the same balance." },
      ] },
      { h2: "What a generation actually costs", blocks: [
        { type: "p", text: "There is no honest fixed price per picture. GPT Image 2 bills per token across four usage legs — text input (your prompt), image input (references on edits), cached input and image output — and the request total follows the terminal usage the API reports, not the PNG's byte size or dimensions." },
        { type: "table", headers: ["Usage leg", "Official per 1M tokens", "Price here"], rows: [
          ["Fresh text input", "$5", "$2.50"],
          ["Fresh image input", "$8", "$4"],
          ["Cached text input", "$1.25", "$0.625"],
          ["Cached image input", "$2", "$1"],
          ["Image output", "$30", "$15"],
        ] },
        { type: "list", items: [
          "Every leg gets the flat 50% B2C discount; cached text and image input bill at 25% of the normal input rate before the discount applies.",
          "Read the usage object on each response and log it next to the asset — it is the billing authority, and it is what the dashboard charge reconciles against.",
          "gpt-image-2 is an alias of the immutable gpt-image-2-2026-04-21 snapshot, so behavior does not drift between calls; pin the dated ID if you want that guarantee spelled out in code.",
        ] },
        { type: "note", text: "Resist quoting a per-image price on your own pricing page from a handful of test renders. Output usage varies with the asset, and a number derived from three samples will be wrong in production. Sum the legs from real usage over a week, then decide." },
        { type: "link", text: "Full cost model and savings math", href: "/docs/learn/gpt-image-2-api-cost" },
      ] },
      { h2: "Limits of the current image surface", blocks: [
        { type: "p", text: "Plan around what the route provably does today rather than what the marketing name suggests. The confirmed profile is deliberately narrow:" },
        { type: "list", items: [
          "One PNG per call, non-streaming — batch workloads loop the endpoint instead of asking for n images in one request.",
          "Controls are background opaque, quality low, size auto; anything else, including transparency, is rejected.",
          "Edits accept one to five PNG references in multipart/form-data and nothing else.",
          "Image usage settles against the same prepaid balance as GPT, Claude and Gemini calls — one pool to watch, not four.",
        ] },
        { type: "p", text: "If you need a different image model for comparison, the Gemini-side image route is documented alongside this one, and the head-to-head guide covers where each wins." },
        { type: "link", text: "Nano Banana 2 vs GPT Image 2, compared on the same tasks", href: "/docs/learn/nano-banana-2-vs-gpt-image-2" },
      ] },
      { h2: "Keep image spend contained on a shared balance", blocks: [
        { type: "p", text: "Because image output is the expensive leg and batch loops multiply it, give the image worker its own API key with a lifetime spending limit. A runaway render job then stops at its own ceiling instead of draining the balance your chat traffic depends on, and per-key usage in the dashboard tells you exactly which worker spent what." },
        { type: "steps", items: [
          "Create a dedicated key in the dashboard for the image pipeline and set its lifetime spending limit to the batch budget.",
          "Send one bounded generation request (the curl above) and confirm the returned PNG plus a usage object with the expected legs.",
          "Run your real prompt set in a small loop, record terminal usage per asset and reconcile the total against the dashboard charge.",
          "Only then scale to full batch volume, keeping the key's limit aligned with the budget you actually approved.",
        ] },
        { type: "link", text: "Per-model rates across every supported provider", href: "/models" },
        cta(),
      ] },
    ],
    faq: [
      { q: "What endpoint does the GPT Image 2 API use?", a: "POST /v1/images/generations for a new image and POST /v1/images/edits for reference-based edits, both on the OpenAI-compatible base URL https://router.apitoken.sale/v1 with an Authorization: Bearer header." },
      { q: "Can GPT Image 2 edit an existing image?", a: "Yes. The edits route accepts multipart/form-data with one to five PNG reference images plus a prompt, and returns one PNG with the requested change applied." },
      { q: "What is the exact model ID for GPT Image 2?", a: "Use gpt-image-2, which aliases the immutable gpt-image-2-2026-04-21 snapshot. Pin the dated ID in code if you want the snapshot spelled out explicitly." },
      { q: "How much does GPT Image 2 cost per image?", a: "There is no fixed per-image price: billing follows terminal usage across text input ($5/M official), image input ($8/M), cached input (25% of fresh) and image output ($30/M), with a flat 50% discount on every leg here — $2.50, $4 and $15 per 1M respectively." },
      { q: "Does GPT Image 2 support transparent backgrounds or streaming?", a: "No on both. The confirmed profile is background opaque, quality low, size auto, one non-streaming PNG per call; transparency requests are rejected rather than approximated." },
      { q: "Does image generation need a separate key or balance?", a: "No. It uses the same Bearer key and prepaid balance as all other supported models — GPT, Claude and Gemini included — though a dedicated key with a lifetime spending limit is sensible for batch image workers." },
    ],
    related: ["gpt-image-2-api-cost", "nano-banana-2-vs-gpt-image-2", "image-editing-api-guide", "image-generation-api-pricing"],
    published: "2026-08-09",
    updated: "2026-08-17",
  };
