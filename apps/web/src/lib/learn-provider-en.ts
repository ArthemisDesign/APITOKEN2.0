import type { LearnArticle } from "./learn";

const ROUTER = "https://router.apitoken.sale";
const OPENAI = `${ROUTER}/v1`;

/**
 * Provider-specific SEO expansion. The original Learn cluster grew around Claude;
 * these guides cover the distinct search intents, protocols and model choices for
 * GPT, Gemini and Kimi without duplicating the generic billing/security articles.
 */
export const learnProviderEn: LearnArticle[] = [
  // ─────────────────────────── GPT ───────────────────────────
  {
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
  },
  {
    slug: "gpt-api-pricing",
    cluster: "explain",
    title: "GPT API Pricing Explained",
    h1: "GPT API pricing: input, cache, output and long context",
    description: "Understand GPT API pricing for GPT-5.6 Sol, Terra and Luna: input, cached input, cache write, output, long-context rates and the flat 50% apiToken.sale discount.",
    keywords: ["gpt api pricing", "gpt-5.6 price", "gpt api cost", "gpt token pricing", "gpt-5.6 sol price", "cheapest gpt api"],
    dek: "GPT cost is a sum of exact token legs, not a price per request. The model tier, cached tokens and input length determine official spend; apiToken.sale then removes 50% from that spend.",
    sections: [
      { h2: "Current GPT-5.6 rates", blocks: [
        { type: "table", headers: ["Model", "Official input / cached / output", "Price here after 50%"], rows: [
          ["gpt-5.6-sol", "$5 / $0.50 / $30", "$2.50 / $0.25 / $15"],
          ["gpt-5.6-terra", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gpt-5.6-luna", "$0.20 / $0.02 / $1.20", "$0.10 / $0.01 / $0.60"],
        ] },
        { type: "p", text: "Rates are per 1M tokens. gpt-5.6 is an alias of gpt-5.6-sol, so it has the same price rather than a separate tariff." },
      ] },
      { h2: "Cache write and long-context rules", blocks: [
        { type: "list", items: [
          "GPT-5.6 cache writes bill at 125% of normal input; cached reads bill at 10% of input.",
          "Above 272K input tokens, the whole request uses 2× input and 1.5× output rates.",
          "Reasoning tokens appear in output usage and are not charged a second time as a separate leg.",
          "The dashboard records the settled token usage and exact discounted charge for each request.",
        ] },
        { type: "note", text: "A cheaper model often saves more than prompt trimming: Terra costs 40% of Sol per token, while Luna costs 4% of Sol. Route by task difficulty instead of using the flagship everywhere." },
      ] },
    ],
    faq: [
      { q: "How much does GPT-5.6 cost per 1M tokens?", a: "Officially Sol is $5 input and $30 output, Terra $2/$12, and Luna $0.20/$1.20. apiToken.sale applies a flat 50% discount to those exact legs." },
      { q: "What counts as cached input?", a: "Repeated prompt prefixes that the provider serves from cache. The terminal usage determines the cached leg; you are not charged both cached and fresh input for the same token." },
      { q: "When does long-context pricing start?", a: "When input exceeds 272K tokens. The whole request then bills at 2× input and 1.5× output before the 50% discount." },
    ],
    related: ["gpt-5-6-sol-vs-terra-vs-luna", "how-to-buy-gpt-api-key", "openai-api-quickstart", "save-tokens-on-claude-api"],
    published: "2026-08-09",
    updated: "2026-08-09",
  },
  {
    slug: "gpt-5-6-sol-vs-terra-vs-luna",
    cluster: "compare",
    title: "GPT-5.6 Sol vs Terra vs Luna",
    h1: "GPT-5.6 Sol, Terra and Luna compared",
    description: "Compare GPT-5.6 Sol, Terra and Luna by price, reasoning effort, context and best use case, then choose the right GPT model for coding and production workloads.",
    keywords: ["gpt-5.6 sol vs terra", "gpt-5.6 terra vs luna", "best gpt-5.6 model", "gpt-5.6 models", "gpt-5.6 comparison", "gpt model for coding"],
    dek: "The GPT-5.6 family shares a 400K context window, 128K maximum output and the full reasoning-effort range. The practical difference is how much capability and latency you buy per token.",
    sections: [
      { h2: "Choose by workload", blocks: [
        { type: "table", headers: ["Tier", "Best fit", "Official input / output"], rows: [
          ["Sol", "Hard reasoning, long-horizon agents, difficult code review", "$5 / $30"],
          ["Terra", "Everyday coding, production chat, balanced agents", "$2 / $12"],
          ["Luna", "Classification, extraction, routing, high-volume simple work", "$0.20 / $1.20"],
        ] },
        { type: "p", text: "Terra is the safest default: it keeps Sol's controls and context at 40% of the token price. Escalate to Sol when evals show a quality gap; send predictable bulk work to Luna." },
      ] },
      { h2: "What stays the same", blocks: [
        { type: "list", items: [
          "400K context and up to 128K output.",
          "Text and image input with text output.",
          "Responses and Chat Completions, both with SSE streaming.",
          "Reasoning effort from none through max on the GPT-5.6 line.",
          "One endpoint, key and balance, so a router can switch models per task.",
        ] },
      ] },
    ],
    faq: [
      { q: "Which GPT-5.6 model is best for coding?", a: "Start with Terra for day-to-day coding. Use Sol for the hardest architecture or agentic tasks and Luna for cheap deterministic sub-steps." },
      { q: "Do Sol, Terra and Luna use different endpoints?", a: "No. All three use the same OpenAI-compatible base URL and key; only the model ID changes." },
      { q: "Does Terra support the max reasoning effort?", a: "Yes. Sol, Terra and Luna expose the same GPT-5.6 reasoning-effort set, including max." },
    ],
    related: ["gpt-api-pricing", "openai-api-quickstart", "codex-cli-setup", "gpt-image-2-api-guide"],
    published: "2026-08-09",
    updated: "2026-08-09",
  },
  {
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
    related: ["openai-api-quickstart", "gpt-api-pricing", "nano-banana-2-api-guide", "how-billing-works"],
    published: "2026-08-09",
    updated: "2026-08-09",
  },

  // ─────────────────────────── GEMINI ───────────────────────────
  {
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
  },
  {
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
  },
  {
    slug: "gemini-api-pricing",
    cluster: "explain",
    title: "Gemini API Pricing Explained",
    h1: "Gemini API pricing: Pro, Flash, Flash-Lite and image output",
    description: "Compare Gemini API token prices for Pro, Flash, Flash-Lite and Nano Banana 2, including cached input, long context, image output and apiToken.sale's flat 50% discount.",
    keywords: ["gemini api pricing", "gemini api cost", "gemini token price", "gemini flash price", "gemini pro price", "cheap gemini api"],
    dek: "Gemini pricing depends on model tier, cached input, output modality and — for Pro — context length. The gateway settles those exact official legs, then applies a flat 50% discount.",
    sections: [
      { h2: "Representative text-model rates", blocks: [
        { type: "table", headers: ["Model", "Official input / cached / output", "Price here after 50%"], rows: [
          ["gemini-3.1-pro-preview", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gemini-3.6-flash", "$1.50 / $0.15 / $7.50", "$0.75 / $0.075 / $3.75"],
          ["gemini-3.1-flash-lite", "$0.25 / $0.025 / $1.50", "$0.125 / $0.0125 / $0.75"],
          ["gemini-2.5-flash-lite", "$0.10 / $0.01 / $0.40", "$0.05 / $0.005 / $0.20"],
        ] },
        { type: "p", text: "All figures are per 1M tokens. Cached input is an independent usage leg reported by the provider; it is not added on top of fresh input for the same tokens." },
      ] },
      { h2: "Long context and images", blocks: [
        { type: "list", items: [
          "Gemini 3.1 Pro Preview requests above 200K input tokens use $4 input and $18 output per 1M on the whole request.",
          "Gemini 3.1 Flash Image charges text output at $3 and image output at $60 per 1M image tokens.",
          "Flash Image cached input bills at the full input rate; it does not receive the text-model cache discount.",
          "The 50% B2C discount applies after the exact official legs are calculated.",
        ] },
      ] },
    ],
    faq: [
      { q: "What is the cheapest Gemini model?", a: "Among the published text tiers, Gemini 2.5 Flash-Lite is $0.10 input and $0.40 output per 1M official, or $0.05/$0.20 after the flat 50% discount." },
      { q: "When does Gemini long-context pricing apply?", a: "For Gemini 3.1 Pro Preview above 200K input tokens. The whole request then uses the higher input, cached-input and output rates." },
      { q: "How is Gemini image output priced?", a: "Gemini 3.1 Flash Image bills rendered output at $60 per 1M image-output tokens officially, or $30 after the flat 50% discount." },
    ],
    related: ["gemini-pro-vs-flash-vs-flash-lite", "how-to-buy-gemini-api-key", "nano-banana-2-api-guide", "how-billing-works"],
    published: "2026-08-09",
    updated: "2026-08-09",
  },
  {
    slug: "gemini-pro-vs-flash-vs-flash-lite",
    cluster: "compare",
    title: "Gemini Pro vs Flash vs Flash-Lite",
    h1: "Gemini Pro, Flash and Flash-Lite compared",
    description: "Compare Gemini Pro, Flash and Flash-Lite by price, context, reasoning and best use case. Choose the right Gemini model for coding, agents and high-volume API work.",
    keywords: ["gemini pro vs flash", "gemini flash vs flash lite", "best gemini model", "gemini models comparison", "gemini model for coding", "gemini 3.6 flash"],
    dek: "Use the tier as a routing decision, not a loyalty choice: Pro for the hardest reasoning, Flash as the coding default, and Flash-Lite for cheap high-volume steps. One key can use all three.",
    sections: [
      { h2: "Choose by task", blocks: [
        { type: "table", headers: ["Tier", "Best fit", "Recommended current ID"], rows: [
          ["Pro", "Hard reasoning, planning, deep codebase and document analysis", "gemini-3.1-pro-preview"],
          ["Flash", "Everyday coding, multimodal agents, balanced production traffic", "gemini-3.6-flash"],
          ["Flash-Lite", "Classification, extraction, routing and cheap pre-processing", "gemini-3.1-flash-lite"],
          ["Image", "Image generation and editing", "gemini-3.1-flash-image"],
        ] },
        { type: "p", text: "Gemini 3.6 Flash is the best starting point for most new text workloads. Move only the hardest calls to Pro and the most predictable bulk calls to Flash-Lite." },
      ] },
      { h2: "Context and cost trade-offs", blocks: [
        { type: "list", items: [
          "The current text models expose a 1M-token context and up to 64K output.",
          "Pro has a long-context premium above 200K input; Flash and Flash-Lite keep flat rates across their window.",
          "Cached input normally bills at 10% of fresh input on the text models.",
          "Use countTokens before very large calls and route by measured quality, not model name alone.",
        ] },
      ] },
    ],
    faq: [
      { q: "Which Gemini model should I use for coding?", a: "Start with Gemini 3.6 Flash. Escalate difficult architecture and review work to 3.1 Pro Preview; use Flash-Lite for cheap deterministic sub-tasks." },
      { q: "Is Flash-Lite limited to a smaller context?", a: "No. The published text Flash-Lite models retain the 1M-token context; their advantage is lower cost and latency for simpler work." },
      { q: "Can I switch tiers without a new key?", a: "Yes. Keep the same Gemini base URL and x-goog-api-key, and change only the model ID." },
    ],
    related: ["gemini-api-pricing", "gemini-api-quickstart", "nano-banana-2-api-guide", "best-claude-model-for-coding"],
    published: "2026-08-09",
    updated: "2026-08-09",
  },
  {
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
    related: ["gemini-api-quickstart", "gemini-api-pricing", "gpt-image-2-api-guide", "gemini-pro-vs-flash-vs-flash-lite"],
    published: "2026-08-09",
    updated: "2026-08-09",
  },

  // ─────────────────────────── KIMI ───────────────────────────
  {
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
  },
  {
    slug: "kimi-api-quickstart",
    cluster: "integrate",
    title: "Kimi API Quickstart",
    h1: "Kimi API quickstart with the Anthropic SDK",
    description: "Call Kimi K3 and Kimi for Coding through apiToken.sale using the Anthropic Messages API, x-api-key, namespaced model IDs, streaming and one shared balance.",
    keywords: ["kimi api quickstart", "kimi api tutorial", "kimi anthropic api", "kimi k3 api example", "kimi for coding api", "kimi api curl"],
    dek: "Kimi speaks the Anthropic Messages protocol on the unified router. Existing Anthropic clients need only a custom base URL, the apiToken.sale key and an explicit kimi/* model ID.",
    sections: [
      { h2: "First request with curl", blocks: [
        { type: "code", code: "curl " + ROUTER + "/v1/messages \\\n  -H \"x-api-key: $APITOKEN_API_KEY\" \\\n  -H \"anthropic-version: 2023-06-01\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"model\":\"kimi/k3-256k\",\"max_tokens\":256,\"messages\":[{\"role\":\"user\",\"content\":\"Reply with exactly: connected\"}]}'" },
        { type: "p", text: "Set stream: true for incremental SSE. Terminal usage follows the Anthropic response shape, so existing usage parsers keep working." },
      ] },
      { h2: "Use the Anthropic Python SDK", blocks: [
        { type: "code", code: [
          "import os",
          "from anthropic import Anthropic",
          "",
          "client = Anthropic(",
          "    api_key=os.environ[\"APITOKEN_API_KEY\"],",
          "    base_url=\"" + ROUTER + "\",",
          ")",
          "",
          "message = client.messages.create(",
          "    model=\"kimi/kimi-for-coding\",",
          "    max_tokens=512,",
          "    messages=[{\"role\": \"user\", \"content\": \"Reply with exactly: connected\"}],",
          ")",
          "print(message.content[0].text)",
        ].join("\n") },
        { type: "note", text: "Do not substitute an official Open Platform ID such as kimi-k2.7-code. The public router accepts the subscription aliases shown by GET /v1/models. OpenAI-compatible clients can reach the same Kimi aliases through the universal /v1 lane." },
      ] },
    ],
    faq: [
      { q: "Can I use the Anthropic SDK for Kimi?", a: "Yes. Point its base_url at https://router.apitoken.sale and choose a kimi/* model ID from the scoped catalog." },
      { q: "Does Kimi support streaming on this route?", a: "Yes. Set stream: true and consume the normal incremental Anthropic SSE events." },
      { q: "What model ID should I start with?", a: "Use kimi/kimi-for-coding for a coding-oriented default or kimi/k3-256k when you need K3 reasoning without the full 1M window." },
    ],
    related: ["how-to-buy-kimi-api-key", "kimi-api-for-claude-code", "kimi-api-for-kimi-code", "kimi-api-for-opencode"],
    published: "2026-08-09",
    updated: "2026-08-09",
  },
  {
    slug: "kimi-api-pricing",
    cluster: "explain",
    title: "Kimi API Pricing Explained",
    h1: "Kimi API pricing: cache hits, misses, output and speed",
    description: "Understand Kimi API pricing for K3, Kimi for Coding and High Speed: cache-hit, cache-miss and output rates, alias mapping and apiToken.sale's 50% discount.",
    keywords: ["kimi api pricing", "kimi k3 price", "kimi for coding price", "kimi token cost", "kimi k2.7 code price", "cheap kimi api"],
    dek: "Kimi publishes cache-hit, cache-miss and output rates rather than one input price. apiToken.sale prices the model actually served, keeps those usage legs disjoint, and applies a flat 50% discount.",
    sections: [
      { h2: "Official rates behind the public aliases", blocks: [
        { type: "table", headers: ["Public alias", "Official hit / miss / output", "Price here after 50%"], rows: [
          ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
        ] },
        { type: "p", text: "Figures are per 1M tokens. Kimi caching is automatic. The provider publishes no separate cache-write price, so a newly cached token is a cache miss rather than a free or hidden fourth leg." },
      ] },
      { h2: "How to control spend", blocks: [
        { type: "list", items: [
          "Use Kimi for Coding for the lowest general coding rate in the published Kimi set.",
          "Use High Speed only when latency justifies exactly double the base token rates.",
          "Use k3-256k instead of the full 1M spelling when the task does not need the larger context mode.",
          "Set a lifetime key spending limit and inspect settled usage in the dashboard.",
        ] },
        { type: "note", text: "Reasoning tokens are a subset of output and bill at the output rate. They are not added again as a separate token class." },
      ] },
    ],
    faq: [
      { q: "How much does Kimi for Coding cost?", a: "Official replacement rates are $0.19 per 1M cache-hit tokens, $0.95 per 1M cache-miss tokens and $4 per 1M output tokens; apiToken.sale charges half." },
      { q: "Why are there cache-hit and cache-miss prices?", a: "Kimi automatically caches repeated context. Terminal usage identifies which input was served from cache, and each leg gets its own official rate." },
      { q: "Does High Speed cost more?", a: "Yes. Its cache-hit, cache-miss and output rates are exactly double the base Kimi for Coding rates." },
    ],
    related: ["kimi-k3-vs-kimi-for-coding", "how-to-buy-kimi-api-key", "kimi-api-quickstart", "how-billing-works"],
    published: "2026-08-09",
    updated: "2026-08-09",
  },
  {
    slug: "kimi-k3-vs-kimi-for-coding",
    cluster: "compare",
    title: "Kimi K3 vs Kimi for Coding",
    h1: "Kimi K3 and Kimi for Coding compared",
    description: "Compare Kimi K3, K3 256K, Kimi for Coding and High Speed by context, reasoning controls, latency and token price for coding and agent workloads.",
    keywords: ["kimi k3 vs kimi for coding", "kimi k3 api", "kimi k2.7 code", "best kimi model for coding", "kimi models comparison", "kimi highspeed"],
    dek: "K3 is the reasoning and long-context family; Kimi for Coding is the economical coding family. High Speed buys latency at double the rate, while K3's aliases choose a 256K or 1M context mode.",
    sections: [
      { h2: "Model-family map", blocks: [
        { type: "table", headers: ["Public ID", "Context", "Best fit"], rows: [
          ["kimi/kimi-for-coding", "256K", "Everyday coding and economical agent loops"],
          ["kimi/kimi-for-coding-highspeed", "256K", "Latency-sensitive coding where speed pays for itself"],
          ["kimi/k3-256k", "256K", "K3 reasoning without the full-context mode"],
          ["kimi/k3 · kimi/k3[1m]", "1M", "Long codebases, documents and hard reasoning"],
        ] },
        { type: "p", text: "k3[1m] is a compatibility spelling of K3's 1M mode, not a separately priced model. The router normalizes it to the provider's real k3 wire model." },
      ] },
      { h2: "Reasoning and routing", blocks: [
        { type: "list", items: [
          "K3 supports low, high and max reasoning effort; high is the default.",
          "Kimi for Coding and High Speed run with thinking enabled.",
          "Model access is catalog-driven, so check the scoped /v1/models response before pinning an alias.",
          "A practical router sends everyday code to Kimi for Coding and escalates large or difficult work to K3.",
        ] },
      ] },
    ],
    faq: [
      { q: "Which Kimi model is best for coding?", a: "Kimi for Coding is the economical default. Choose K3 for harder reasoning or long-context codebase work, and High Speed only when lower latency is worth double rates." },
      { q: "Are k3 and k3[1m] different models?", a: "No. They select the same K3 1M mode; the bracket form is a compatibility alias." },
      { q: "Can I request Kimi's internal official model IDs?", a: "No. Use the public subscription aliases returned by the router catalog, not internal tariff IDs such as kimi-k2.7-code." },
    ],
    related: ["kimi-api-pricing", "kimi-api-quickstart", "kimi-api-for-claude-code", "how-to-buy-kimi-api-key"],
    published: "2026-08-09",
    updated: "2026-08-09",
  },
  {
    slug: "kimi-api-for-opencode",
    cluster: "integrate",
    title: "Use the Kimi API in OpenCode",
    h1: "Run Kimi K3 and Kimi for Coding in OpenCode",
    description: "Connect OpenCode to Kimi through apiToken.sale with the router plugin, live model catalog, explicit kimi/* IDs, streaming and one prepaid API key.",
    keywords: ["kimi opencode", "kimi api opencode", "kimi k3 opencode", "kimi for coding setup", "opencode custom provider", "kimi coding agent"],
    dek: "OpenCode can address the Kimi namespace explicitly and consumes the router's live catalog. That makes it the safest coding-agent setup for switching between K3 and Kimi for Coding without hand-maintaining provider limits.",
    sections: [
      { h2: "Install and verify", blocks: [
        { type: "steps", items: [
          "Run the apiToken.sale OpenCode installer; it merges the router plugin into your existing config and keeps a backup.",
          "Restart OpenCode so the plugin fetches the key-scoped model catalog.",
          "Run one deterministic prompt with an explicit namespaced model.",
        ] },
        { type: "code", code: "curl -fsSL https://raw.githubusercontent.com/apitokensale-admin/apitoken.sale/main/opencode/install.sh | bash\n\nopencode run --model apitoken/kimi/kimi-for-coding \"Reply with exactly: connected\"" },
      ] },
      { h2: "Choose a Kimi model safely", blocks: [
        { type: "list", items: [
          "apitoken/kimi/kimi-for-coding — economical coding default.",
          "apitoken/kimi/kimi-for-coding-highspeed — lower latency at double token rates.",
          "apitoken/kimi/k3-256k — K3 reasoning in the smaller context mode.",
          "apitoken/kimi/k3 — K3 with the full 1M context when the catalog exposes it.",
        ] },
        { type: "note", text: "Claude Code and Kimi Code also support Kimi, but their configuration is different: Claude Code needs every model tier pinned, while Kimi Code uses an explicit OpenAI-compatible provider block." },
      ] },
    ],
    faq: [
      { q: "Does OpenCode support Kimi models?", a: "Yes. The apiToken.sale router plugin registers the live Kimi namespace and OpenCode selects models as apitoken/kimi/{model}." },
      { q: "Why use the router plugin instead of a static model list?", a: "It keeps model IDs, limits and availability aligned with the key-scoped live catalog, so retired or unavailable aliases do not linger in local config." },
      { q: "Can Claude Code use Kimi too?", a: "Yes, with a different setup. Point Claude Code at the Anthropic endpoint and pin its main, Opus, Sonnet, Haiku and subagent model variables to one Kimi alias." },
    ],
    related: ["kimi-api-for-claude-code", "kimi-api-for-kimi-code", "kimi-api-quickstart", "kimi-k3-vs-kimi-for-coding"],
    published: "2026-08-09",
    updated: "2026-08-09",
  },
  {
    slug: "kimi-api-for-claude-code",
    cluster: "integrate",
    title: "Use Kimi K3 in Claude Code",
    h1: "Run Kimi K3 and Kimi for Coding in Claude Code",
    description: "Configure Claude Code for Kimi K3 or Kimi for Coding through apiToken.sale: pin every model tier, preserve the 1M context window and verify the endpoint.",
    keywords: ["kimi claude code", "kimi k3 claude code", "kimi for coding claude code", "claude code custom model", "claude code kimi api", "k3 1m claude code"],
    dek: "Claude Code already speaks Anthropic Messages, so it can run Kimi directly. The reliable setup pins every internal model tier to one Kimi alias; otherwise the main session can work while subagents fail on an inherited Claude model.",
    sections: [
      { h2: "Pin the connection and every model tier", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale
export ANTHROPIC_API_KEY=sk-pool-•••
export ANTHROPIC_MODEL=k3
export ANTHROPIC_DEFAULT_OPUS_MODEL=k3
export ANTHROPIC_DEFAULT_SONNET_MODEL=k3
export ANTHROPIC_DEFAULT_HAIKU_MODEL=k3
export CLAUDE_CODE_SUBAGENT_MODEL=k3
export CLAUDE_CODE_MAX_CONTEXT_TOKENS=1048576
export CLAUDE_CODE_AUTO_COMPACT_WINDOW=1048576

claude --model k3` },
        { type: "p", text: "Use the bare subscription alias on the Anthropic lane. For a 256K model such as k3-256k or kimi-for-coding, keep the tier pins but omit the two 1M context variables." },
      ] },
      { h2: "Verify the route, not the model's introduction", blocks: [
        { type: "list", items: [
          "Open /status and confirm that the Anthropic base URL is apiToken.sale.",
          "Do not ask the model to identify itself: Claude Code's system prompt can make any backend call itself Claude.",
          "Keep thinking enabled. Turning it off can change which underlying Kimi model serves the request.",
          "Check GET /v1/models before pinning an alias for a long-lived environment.",
        ] },
      ] },
    ],
    faq: [
      { q: "Does Claude Code support Kimi K3?", a: "Yes. Point Claude Code at https://router.apitoken.sale and pin every model tier to an admitted Kimi subscription alias." },
      { q: "Why must every Claude Code model variable be pinned?", a: "Claude Code chooses separate models for its main session, tiers and subagents. An unpinned tier can inherit a Claude ID and fail only when that background path runs." },
      { q: "How do I keep K3's full 1M context in Claude Code?", a: "Use k3 or k3[1m] and set both CLAUDE_CODE_MAX_CONTEXT_TOKENS and CLAUDE_CODE_AUTO_COMPACT_WINDOW to 1048576." },
    ],
    related: ["kimi-api-for-kimi-code", "kimi-api-for-opencode", "kimi-k3-vs-kimi-for-coding", "kimi-api-quickstart"],
    published: "2026-08-09",
    updated: "2026-08-09",
  },
  {
    slug: "kimi-api-for-kimi-code",
    cluster: "integrate",
    title: "Use apiToken.sale in Kimi Code",
    h1: "Run Kimi, Claude, GPT and Gemini in Kimi Code",
    description: "Connect Kimi Code to apiToken.sale through its OpenAI-compatible provider config, declare a namespaced model and protect the API key stored in config.toml.",
    keywords: ["kimi code api", "kimi code custom provider", "kimi code config toml", "kimi code api key", "kimi code k3", "kimi code openai compatible"],
    dek: "Kimi Code accepts a custom OpenAI-compatible provider, so one apiToken.sale provider entry can reach the unified catalog. Each model still needs an explicit local declaration with its real namespace and reviewed context window.",
    sections: [
      { h2: "Install and declare the provider", blocks: [
        { type: "code", code: `curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash

# ~/.kimi-code/config.toml
default_model = "apitoken/k3"

[providers.apitoken]
type = "openai"
base_url = "https://router.apitoken.sale/v1"
api_key = "sk-pool-•••"

[models."apitoken/k3"]
provider = "apitoken"
model = "kimi/k3"
max_context_size = 1048576
display_name = "Kimi K3 (1M)"

chmod 600 ~/.kimi-code/config.toml` },
        { type: "note", text: "Do not run /login for this setup: that binds the CLI to a Kimi membership instead. Kimi Code stores custom-provider credentials only in config.toml, so the file contains the key in plain text and must be locked down." },
      ] },
      { h2: "Start, verify and add models", blocks: [
        { type: "code", code: `kimi -m apitoken/k3

/status

Reply with exactly: connected` },
        { type: "list", items: [
          "/status must show https://router.apitoken.sale/v1 as the provider base URL.",
          "The model field uses the unified catalog namespace, for example kimi/k3, openai/gpt-5.6-terra or google/gemini-3.6-flash.",
          "Declare each additional model in config.toml with its reviewed max_context_size; Kimi Code uses that value to decide when to compact.",
        ] },
      ] },
    ],
    faq: [
      { q: "Can Kimi Code use an apiToken.sale key?", a: "Yes. Add an OpenAI-compatible provider with base_url https://router.apitoken.sale/v1 and store the key in Kimi Code's config.toml." },
      { q: "Can Kimi Code run models other than Kimi?", a: "Yes. The same provider entry reaches the unified catalog; declare each Claude, GPT, Gemini or Kimi model with its namespaced ID and correct context limit." },
      { q: "Why is chmod 600 important?", a: "Kimi Code does not read custom-provider credentials from the shell. The raw API key lives in config.toml, so that file should be readable only by your account." },
    ],
    related: ["kimi-api-for-claude-code", "kimi-api-for-opencode", "how-to-buy-kimi-api-key", "kimi-k3-vs-kimi-for-coding"],
    published: "2026-08-09",
    updated: "2026-08-09",
  },
];
