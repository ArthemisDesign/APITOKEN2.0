import type { LearnArticle } from "../learn";
import { OPENAI, ROUTER } from "./shared";

export const article: LearnArticle = {
    slug: "how-to-buy-gpt-api-key",
    cluster: "buy",
    title: "How to Buy a GPT API Key",
    h1: "How to buy a GPT API key",
    description: "Buy a GPT API key with a prepaid balance and card or crypto checkout. One OpenAI-compatible endpoint serves GPT-5.6 Sol, Terra and Luna, GPT-5.5 and GPT Image 2 at 50% off official spend.",
    keywords: ["buy gpt api key", "gpt api key", "how to buy gpt api key", "buy openai api key", "gpt-5.6 api access", "openai compatible api key", "gpt api prepaid", "gpt api key without openai account", "gpt api pay with crypto", "cheap gpt-5.6 api"],
    dek: "To buy a GPT API key without an OpenAI Platform account, create an apiToken.sale account, top up a prepaid balance by card or crypto, and generate one sk-pool key. That key authenticates against an OpenAI-compatible endpoint with Authorization: Bearer and serves GPT-5.6, GPT-5.5 and GPT Image 2 at 50% off official token spend. This guide walks the purchase, the billing math and the exact client configuration.",
    sections: [
      { h2: "The purchase: account, balance, key", blocks: [
        { type: "steps", items: [
          "Create an apiToken.sale account with Google, GitHub, or email and password. Google and GitHub signups start with $5 of platform bonus credit, valid on supported Claude, GPT, Gemini and Kimi models; email/password accounts do not receive the bonus.",
          "Top up any whole-dollar amount by bank card or cryptocurrency. There is no fixed bundle and no monthly commitment — the balance is prepaid and is spent only when requests run.",
          "Generate an API key in the dashboard. It looks like sk-pool-… and works immediately; the same key and balance also cover the supported Claude, Gemini and Kimi models.",
          `Verify the key with the curl below before wiring it into a project. A 200 with real output closes the loop; a 401 means the key or the header name is wrong.`,
        ] },
        { type: "code", code: `curl ${OPENAI}/responses \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{"model":"gpt-5.6-terra","input":"Reply with exactly: connected"}'` },
        { type: "p", text: "Buying a GPT API key here is three moves — sign up, top up, generate — and the key is active on the very next request: no OpenAI Platform account, no waitlist and no manual review anywhere in the flow. If the curl above returns output text, the whole chain of key, balance and endpoint is proven end to end." },
      ] },
      { h2: "Which GPT models the key serves", blocks: [
        { type: "p", text: `The catalog served to your key is always the live answer at GET ${OPENAI}/models — read it instead of assuming a model name from OpenAI's docs exists here. Today the line covers three GPT-5.6 tiers, two previous-generation models, and the separate GPT Image 2 generation and edit routes:` },
        { type: "table", headers: ["Model ID", "Tier", "Official in / cached / out ($ per 1M)", "After 50% discount"], rows: [
          ["gpt-5.6-sol (alias: gpt-5.6)", "Flagship", "$5 / $0.50 / $30", "$2.50 / $0.25 / $15"],
          ["gpt-5.6-terra", "Balanced", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gpt-5.6-luna", "Fast", "$0.20 / $0.02 / $1.20", "$0.10 / $0.01 / $0.60"],
          ["gpt-5.5", "Previous flagship", "$5 / — / $30", "$2.50 / — / $15"],
          ["gpt-5.4", "Previous balanced", "$2.50 / — / $15", "$1.25 / — / $7.50"],
        ] },
        { type: "list", items: [
          "All three GPT-5.6 tiers share a 400K context window, up to 128K output, text and image input, and adjustable reasoning effort — none through xhigh, plus max on the GPT-5.6 line.",
          "Responses and Chat Completions both work with incremental SSE streaming, so existing generate-or-stream loops port without structural changes.",
          "GPT Image 2 runs on its own generation and edit routes rather than the chat surface; it bills against the same balance.",
        ] },
        { type: "link", text: "Per-model specs and discounted prices", href: "/models" },
        { type: "link", text: "Full GPT pricing breakdown: cache writes and long context", href: "/docs/learn/gpt-api-pricing" },
      ] },
      { h2: "How the prepaid balance and the 50% discount settle", blocks: [
        { type: "p", text: "There is no subscription and no monthly fee. Every request is metered at official OpenAI token rates first, your flat 50% B2C discount is subtracted, and the net amount is drawn from the prepaid balance — so $50 of balance covers $100 of official-rate usage. The dashboard records the settled token usage and exact discounted charge for each request." },
        { type: "list", items: [
          "Cached input is priced separately and far cheaper than fresh input ($0.50 vs $5 per 1M on the flagship), so a stable prompt prefix across calls is real money.",
          "Cache writes bill at 125% of normal input; cached reads bill at 10% of input.",
          "Reasoning tokens appear in output usage and are not charged a second time as a separate leg.",
        ] },
        { type: "note", text: "Above 272K input tokens, long-context rates apply to the whole request — 2× on input and 1.5× on output, before the discount. A request at 273K costs more than twice one at 270K; split oversized contexts before crossing the boundary." },
        { type: "note", text: "When the balance runs out, requests fail with an insufficient-balance error until you top up again — there is no overdraft and no surprise charge to your card." },
      ] },
      { h2: "Configuring the official SDK and existing clients", blocks: [
        { type: "p", text: "Every OpenAI-compatible client needs exactly two values changed: the base URL and the credential. Prompts, streaming code and tool definitions stay as they are — the official SDKs work unchanged:" },
        { type: "code", code: `import os\nfrom openai import OpenAI\n\nclient = OpenAI(\n    api_key=os.environ["APITOKEN_API_KEY"],\n    base_url="${OPENAI}",\n)\n\nresponse = client.responses.create(\n    model="gpt-5.6-terra",\n    input="Reply with exactly: connected",\n)\nprint(response.output_text)` },
        { type: "p", text: "Frameworks that expect the classic Chat Completions shape — older LangChain chains, LiteLLM configs, most open-source chat UIs — work on the same host with the same key and model IDs; only the method name changes to client.chat.completions.create with a messages array." },
        { type: "note", text: "Keep the key in a server-side environment variable, never in client-side code or a committed file. A GPT call authenticates with Authorization: Bearer; x-api-key belongs to the Anthropic Messages lane and x-goog-api-key to the native Gemini lane — sending either here returns a 401." },
      ] },
      { h2: "What this gateway is — and is not", blocks: [
        { type: "p", text: "This is an independent OpenAI-compatible gateway with its own account, prepaid balance and supported-model catalog — not the OpenAI Platform. For pure text and vision chat workloads the surface is complete: model discovery, Responses, Chat Completions, streaming, and the GPT Image 2 routes. Audio, file, realtime, assistants, batch and fine-tuning endpoints are not available; an app that depends on those is not a candidate for migration." },
        { type: "p", text: "Errors arrive in the standard OpenAI envelope, so existing error-handling code keeps working. Three status codes cover nearly everything you will hit while integrating:" },
        { type: "list", items: [
          "401 — the key is missing or mistyped, or you sent x-api-key instead of Authorization: Bearer. Reproduce with curl outside your app to isolate which half is broken.",
          "402 — the prepaid balance needs a top-up; no retry or backoff fixes an empty balance.",
          `404 — the model ID is not enabled on your key; check GET ${OPENAI}/models instead of assuming.`,
        ] },
        { type: "link", text: "From key to first streaming response: the full quickstart", href: "/docs/learn/openai-api-quickstart" },
      ] },
    ],
    faq: [
      { q: "Do I need an OpenAI account to buy this GPT API key?", a: "No. The key, balance and billing come from apiToken.sale; compatible GPT clients only need the custom base URL and the Bearer key. There is no waitlist or manual review." },
      { q: "Can I pay for a GPT API key with cryptocurrency?", a: "Yes. Checkout accepts bank cards and cryptocurrency, and top-ups are any whole-dollar amount with no fixed bundle. New accounts created with Google or GitHub also start with $5 of platform bonus credit." },
      { q: "Can one key run both GPT and Claude?", a: "Yes. The same sk-pool key and prepaid balance cover all supported providers; only the endpoint and authorization header change with the protocol — Bearer on the OpenAI-compatible lane, x-api-key on Anthropic Messages." },
      { q: "How much does GPT-5.6 cost here per 1M tokens?", a: "Officially Sol is $5 input and $30 output, Terra $2/$12 and Luna $0.20/$1.20; apiToken.sale applies a flat 50% discount to those exact legs, so Terra settles at $1/$6." },
      { q: "Is this the OpenAI Platform?", a: "No. It is an independent OpenAI-compatible gateway with its own account, prepaid balance and supported-model catalog. Responses, Chat Completions, streaming and GPT Image 2 are served; audio, realtime, assistants, batch and fine-tuning are not." },
    ],
    related: ["openai-api-quickstart", "gpt-api-pricing", "gpt-5-6-sol-vs-terra-vs-luna", "how-billing-works"],
    published: "2026-08-09",
    updated: "2026-08-17",
  };
