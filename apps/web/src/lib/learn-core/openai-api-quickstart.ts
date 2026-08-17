import type { LearnArticle } from "../learn";
import { cta, OPENAI_BASE } from "../learn-shared";

export const article: LearnArticle = {
  slug: "openai-api-quickstart",
  cluster: "integrate",
  title: "OpenAI-Compatible API Quickstart — GPT-5.6 on One Key",
  h1: "OpenAI-compatible API quickstart: Responses and Chat Completions",
  description: "Run GPT-5.6 models on apiToken.sale through the OpenAI-compatible API — Responses and Chat Completions with SSE streaming, one sk-pool key and balance shared with Claude, at a flat 50% off.",
  keywords: ["openai compatible api", "gpt-5.6 api", "responses api", "chat completions custom base url", "openai sdk base_url", "gpt api key", "gpt-5.6 price", "gpt-5.6-sol", "openai api alternative"],
  dek: "Your sk-pool key is not Claude-only. The same key and prepaid balance also serve the GPT-5 line through an OpenAI-compatible endpoint — standard Responses and Chat Completions calls, official OpenAI SDKs, SSE streaming, and the same flat 50% discount.",
  sections: [
    { h2: "Three steps to your first GPT call", blocks: [
      { type: "steps", items: [
        "Create a free account and generate one API key (it looks like sk-pool-…) — the same key already covers Claude models too.",
        `Point your client at ${OPENAI_BASE} and authenticate with Authorization: Bearer — not x-api-key; that header belongs to the Anthropic surface.`,
        "Confirm the enabled models with GET https://router.apitoken.sale/v1/models — the unified catalog namespaces IDs by provider (anthropic/*, openai/*, google/*) — then send a Responses request.",
      ] },
      { type: "code", code: `curl ${OPENAI_BASE}/responses \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{\n    "model": "gpt-5.6-sol",\n    "input": "Reply with exactly: connected"\n  }'` },
      cta(),
    ] },
    { h2: "Use the official OpenAI SDK", blocks: [
      { type: "p", text: "The official SDKs work unchanged — only base_url and the key change. Keep the key in a server-side environment variable in production." },
      { type: "code", code: `import os\nfrom openai import OpenAI\n\nclient = OpenAI(\n    api_key=os.environ["APITOKEN_API_KEY"],\n    base_url="${OPENAI_BASE}",\n)\n\nresponse = client.responses.create(\n    model="gpt-5.6-sol",\n    input="Reply with exactly: connected",\n)\nprint(response.output_text)` },
      { type: "p", text: "Chat Completions is served on the same host if your client expects it — the model ID and key stay the same." },
      { type: "code", code: `completion = client.chat.completions.create(\n    model="gpt-5.6-sol",\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(completion.choices[0].message.content)` },
    ] },
    { h2: "Which GPT models are available", blocks: [
      { type: "p", text: "The served set is pinned and priced in the engine; GET https://router.apitoken.sale/v1/models is always the live answer. Today the line covers three GPT-5.6 tiers and two previous-generation models:" },
      { type: "table", headers: ["Model ID", "Tier", "Official in / out ($ per 1M)", "Cached input"], rows: [
        ["gpt-5.6-sol (alias: gpt-5.6)", "Flagship", "$5 / $30", "$0.50"],
        ["gpt-5.6-terra", "Balanced", "$2 / $12", "$0.20"],
        ["gpt-5.6-luna", "Fast", "$0.20 / $1.20", "$0.02"],
        ["gpt-5.5", "Previous flagship", "$5 / $30", "$0.50"],
        ["gpt-5.4", "Previous balanced", "$2.50 / $15", "$0.25"],
      ] },
      { type: "list", items: [
        "Reasoning effort is adjustable per request — none through xhigh on every model, plus max on the GPT-5.6 line.",
        "Every model accepts text and image input and streams over SSE on both Responses and Chat Completions.",
        "Requests above 272K input tokens bill at OpenAI long-context rates: 2× input and 1.5× output on the whole request.",
        "Your B2C discount applies here exactly as it does to Claude usage — one balance, one rate, 50% off official spend.",
      ] },
      { type: "link", text: "Full per-model specs and discounted prices", href: "/models" },
    ] },
    { h2: "What the endpoint does and does not cover", blocks: [
      { type: "p", text: "This is an independent OpenAI-compatible service, not the OpenAI Platform. It serves model discovery, streaming Responses and Chat Completions, plus dedicated GPT Image 2 generation and edit routes. Audio, file, realtime, assistants, batch and fine-tuning endpoints are not available." },
      { type: "note", text: "Errors come in the OpenAI envelope — {\"error\":{\"message\",\"type\",\"param\",\"code\"}}. A 401 means the key or the auth header is wrong (use Bearer, not x-api-key), a 402 means the shared prepaid balance needs a top-up, and a 404 means the model ID is not enabled — check GET https://router.apitoken.sale/v1/models." },
    ] },
  ],
  faq: [
    { q: "Does the same key work beyond GPT?", a: "Yes. One sk-pool key and prepaid balance also cover supported Claude, Gemini and Kimi models; use the protocol and authentication header documented for each provider." },
    { q: "Which auth header does the OpenAI-compatible endpoint use?", a: "Authorization: Bearer sk-pool-…. The x-api-key header is only for the Anthropic surface — sending it to the OpenAI endpoint returns a 401." },
    { q: "Responses or Chat Completions?", a: "Both are served with SSE streaming. Use Responses for new code and the official SDKs; Chat Completions works for clients and frameworks that expect the classic shape." },
    { q: "How is GPT usage billed?", a: "Per token at official OpenAI rates — including cached-input and long-context pricing — then your flat 50% B2C discount is subtracted before the charge touches your prepaid balance, exactly like Claude usage." },
  ],
  related: ["how-to-buy-gpt-api-key", "gpt-api-pricing", "gpt-5-6-sol-vs-terra-vs-luna", "codex-cli-setup"],
  published: "2026-07-29",
  updated: "2026-07-29",
};
