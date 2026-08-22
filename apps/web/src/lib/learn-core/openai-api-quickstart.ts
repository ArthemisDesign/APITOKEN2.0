import type { LearnArticle } from "../learn";
import { cta, OPENAI_BASE } from "../learn-shared";

export const article: LearnArticle = {
  slug: "openai-api-quickstart",
  cluster: "integrate",
  title: "OpenAI-Compatible API Quickstart: GPT-5.6",
  h1: "OpenAI-compatible API quickstart: from curl to the official SDK",
  description: "OpenAI-compatible API quickstart: GPT-5.6 on apiToken.sale via Responses and Chat Completions with SSE streaming — one key, balance shared with Claude, 50% off.",
  keywords: ["openai compatible api", "openai compatible api quickstart", "gpt-5.6 api", "responses api example", "chat completions custom base url", "openai sdk base_url", "gpt api key alternative", "gpt-5.6-sol", "openai api endpoint redirect", "gpt-5.6 price per token"],
  dek: "Looking for an OpenAI-compatible API you can hit in the next five minutes? Point any OpenAI client at https://router.apitoken.sale/v1 with one sk-pool key and the same prepaid balance that already covers Claude. Responses and Chat Completions both stream over SSE, and GPT-5.6 usage bills at official OpenAI token rates minus your flat 50% discount.",
  sections: [
    { h2: "Your first GPT-5.6 response in three steps", blocks: [
      { type: "p", text: "The whole migration from OpenAI's API to this endpoint is a base URL and a header swap. There is no new SDK to learn, no adapter layer, and no separate account for GPT — the key you may already use for Claude is the same credential here, and the same prepaid balance meters both providers." },
      { type: "steps", items: [
        "Create a free account and generate one API key — it looks like sk-pool-… and already covers supported Claude, Gemini and Kimi models on their own protocol surfaces.",
        `Point your client at ${OPENAI_BASE} and authenticate with Authorization: Bearer — do not send x-api-key; that header belongs to the Anthropic Messages surface and will be rejected here.`,
        "Confirm the enabled model set with GET https://router.apitoken.sale/v1/models — the unified catalog namespaces IDs by provider (anthropic/*, openai/*, google/*) — then send the Responses request below.",
      ] },
      { type: "code", code: `curl ${OPENAI_BASE}/responses \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{\n    "model": "gpt-5.6-sol",\n    "input": "Reply with exactly: connected"\n  }'` },
      { type: "p", text: "If the body comes back with output text, you are done — every other client you own is a one-line configuration change away from working the same way." },
      cta(),
    ] },
    { h2: "Two constructor arguments switch the official SDK", blocks: [
      { type: "p", text: "The official OpenAI SDKs work unchanged. Only base_url and the key change, and the key should live in a server-side environment variable in production — never in client-side code or a committed file." },
      { type: "code", code: `import os\nfrom openai import OpenAI\n\nclient = OpenAI(\n    api_key=os.environ["APITOKEN_API_KEY"],\n    base_url="${OPENAI_BASE}",\n)\n\nresponse = client.responses.create(\n    model="gpt-5.6-sol",\n    input="Reply with exactly: connected",\n)\nprint(response.output_text)` },
      { type: "p", text: "Frameworks that hard-code the Chat Completions shape — older LangChain chains, LiteLLM configs, most open-source chat UIs — work on the same host with the same model ID and key:" },
      { type: "code", code: `completion = client.chat.completions.create(\n    model="gpt-5.6-sol",\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(completion.choices[0].message.content)` },
      { type: "p", text: "Which surface should new code target? Responses. Both endpoints stream over SSE with identical models, pricing and discount, but Responses is the surface current OpenAI tooling builds around — it keeps reasoning items and tool calls in one typed stream and exposes conveniences like response.output_text. Chat Completions exists for clients and frameworks that expect the classic messages array; nothing you build on one surface locks you out of the other." },
    ] },
    { h2: "Model IDs, per-token prices and the 272K trap", blocks: [
      { type: "p", text: "The served set is pinned and priced in the engine, and GET https://router.apitoken.sale/v1/models is always the live answer. Today the line covers three GPT-5.6 tiers plus two previous-generation models kept for compatibility:" },
      { type: "table", headers: ["Model ID", "Tier", "Official in / out ($ per 1M)", "Cached input"], rows: [
        ["gpt-5.6-sol (alias: gpt-5.6)", "Flagship", "$4 / $20 (temporary)", "$0.40"],
        ["gpt-5.6-terra", "Balanced", "$2 / $12", "$0.20"],
        ["gpt-5.6-luna", "Fast", "$0.20 / $1.20", "$0.02"],
        ["gpt-5.5", "Previous flagship", "$5 / $30", "$0.50"],
        ["gpt-5.4", "Previous balanced", "$2.50 / $15", "$0.25"],
      ] },
      { type: "list", items: [
        "Sol's temporary official input/cached/cache-write/output rates are $4/$0.40/$5/$20 through 2026-11-21 inclusive, or $2/$0.20/$2.50/$10 after the flat 50% discount. Standard $5 input and $30 output return on 2026-11-22 UTC.",
        "Pick by tier: gpt-5.6-sol for the hardest reasoning, gpt-5.6-terra as the daily driver, gpt-5.6-luna for high-volume cheap calls. The alias gpt-5.6 tracks the flagship.",
        "Reasoning effort is adjustable per request — none through xhigh on every model, plus max on the GPT-5.6 line.",
        "Every model accepts text and image input and streams over SSE on both Responses and Chat Completions.",
        "Cached input is priced separately and far cheaper than fresh input ($0.40 vs $4 per 1M on promotional Sol) — keeping a stable prompt prefix across calls is real money, not a micro-optimization.",
        "Your flat 50% B2C discount applies here exactly as it does to Claude usage — one balance, one rate, half off official spend.",
      ] },
      { type: "note", text: "The 272K threshold is the trap: above it, OpenAI long-context rates apply to the whole request — 2× on input and 1.5× on output, not just the overflow. On promotional Sol, 270K input plus 2K output costs $1.12 official; 273K plus 2K costs $2.244. Split oversized contexts or trim history before you cross the boundary." },
      { type: "link", text: "Full per-model specs and discounted prices", href: "/models" },
    ] },
    { h2: "What this endpoint is — and is not", blocks: [
      { type: "p", text: "This is an independent OpenAI-compatible service, not the OpenAI Platform. It serves model discovery, streaming Responses and Chat Completions, plus dedicated GPT Image 2 generation and edit routes. Audio, file, realtime, assistants, batch and fine-tuning endpoints are not available — if your app depends on those, it is not a candidate for migration. For pure text and vision chat workloads, though, the surface here is complete: nothing in a standard generate-or-stream loop touches an endpoint that is missing." },
      { type: "p", text: "Errors arrive in the standard OpenAI envelope — {\"error\":{\"message\",\"type\",\"param\",\"code\"}} — so existing error-handling code keeps working. Three status codes cover almost everything you will see while integrating:" },
      { type: "list", items: [
        "401 — the key is wrong, revoked, or you sent x-api-key instead of Authorization: Bearer. Reproduce with curl outside your app to isolate which half is broken.",
        "402 — the shared prepaid balance needs a top-up; no retry or backoff fixes an empty balance.",
        "404 — the model ID is not enabled on your key; check GET https://router.apitoken.sale/v1/models instead of assuming a name from OpenAI's docs exists here.",
      ] },
    ] },
  ],
  faq: [
    { q: "Can I use my existing OpenAI SDK with a custom base URL?", a: `Yes — pass api_key and base_url="${OPENAI_BASE}" to the official client and everything else stays the same. Keep the key in a server-side environment variable in production.` },
    { q: "Does one API key really cover GPT, Claude, Gemini and Kimi?", a: "Yes. One sk-pool key and one prepaid balance serve all four providers; you use the protocol and auth header documented for each surface (Bearer here, x-api-key on the Anthropic Messages endpoint)." },
    { q: "Responses API or Chat Completions for a new project?", a: "Responses. Both stream over SSE with the same models and pricing, but Responses is the surface current OpenAI SDKs and tooling build around; Chat Completions exists for clients that expect the classic shape." },
    { q: "Why do I get a 401 on the OpenAI-compatible endpoint?", a: "Almost always the auth header: this endpoint wants Authorization: Bearer sk-pool-…, and the x-api-key header from Anthropic-style setups returns a 401 here." },
  ],
  related: ["how-to-buy-gpt-api-key", "gpt-api-pricing", "gpt-5-6-sol-vs-terra-vs-luna", "codex-cli-setup"],
  published: "2026-07-29",
  updated: "2026-08-17",
};
