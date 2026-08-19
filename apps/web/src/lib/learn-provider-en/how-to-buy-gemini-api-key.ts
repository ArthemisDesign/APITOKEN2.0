import type { LearnArticle } from "../learn";
import { ROUTER } from "./shared";

export const article: LearnArticle = {
    slug: "how-to-buy-gemini-api-key",
    cluster: "buy",
    title: "How to Buy a Gemini API Key",
    h1: "How to buy a Gemini API key",
    description: "Buy a Gemini API key without a Google Cloud billing account: top up by card or crypto and call native generateContent at 50% off official spend.",
    keywords: ["buy gemini api key", "how to buy gemini api key", "gemini api key", "google gemini api access", "gemini api key without google cloud", "gemini api prepaid balance", "buy gemini api with crypto", "gemini api card payment", "cheap gemini api", "gemini api 50% discount", "gemini api key instant activation"],
    dek: "You buy a Gemini API key on apiToken.sale the way you buy prepaid credit: create an account, top up any whole-dollar amount by card or crypto, and generate a key in the dashboard. That key authenticates against the native Google Gemini protocol — x-goog-api-key, generateContent, the official SDK — at a flat 50% off official token spend. No Google Cloud project, billing account, or waitlist is involved.",
    sections: [
      { h2: "From checkout to a working key in five minutes", blocks: [
        { type: "p", text: "Buying a Gemini API key here is a prepaid purchase, not a subscription: sign up, add balance, generate one sk-pool key. The key is active on the very next request — there is no approval step, waitlist, or manual review anywhere in the flow." },
        { type: "steps", items: [
          "Create an account with Google, GitHub, or email and password. Google and GitHub signups start with $5 of platform bonus credit, valid on supported Gemini, GPT, Claude and Kimi models; email/password accounts do not receive the bonus.",
          "Top up any whole-dollar amount. Checkout accepts bank cards and cryptocurrency through a secure payment provider, and the balance never expires.",
          "Open the dashboard and generate an API key. It looks like sk-pool-… and immediately covers every supported provider, not just Gemini.",
          "Verify the key with the curl in the last section of this guide. A 200 with real output confirms the key, balance and route end to end.",
        ] },
      ] },
      { h2: "No Google Cloud project or billing account required", blocks: [
        { type: "p", text: "Buying Gemini access direct from Google means an AI Studio or Google Cloud account with a linked billing profile, and for many buyers that layer is the blocker. apiToken.sale owns the gateway account and the upstream billing, so the only things you bring are a login (Google, GitHub, or email) and a way to pay." },
        { type: "p", text: "What you receive is not a reshaped proxy API. The gateway serves the native Gemini protocol — the same URL grammar, request bodies and response shapes as Google's own endpoint — so existing Gemini code keeps working after exactly two configuration changes: the base URL and the key." },
      ] },
      { h2: "The Gemini catalog on one key", blocks: [
        { type: "p", text: "A single key covers the supported Gemini line; the model ID in the request path is the only switch between tiers. Representative text rates per 1M tokens:" },
        { type: "table", headers: ["Model ID", "Tier", "Official input / output", "After 50% discount"], rows: [
          ["gemini-3.6-flash", "Flash — everyday default", "$1.50 / $7.50", "$0.75 / $3.75"],
          ["gemini-3.1-pro-preview", "Pro — hardest reasoning", "$2 / $12", "$1 / $6"],
          ["gemini-3.1-flash-lite", "Flash-Lite — bulk steps", "$0.25 / $1.50", "$0.125 / $0.75"],
          ["gemini-2.5-flash-lite", "Cheapest text floor", "$0.10 / $0.40", "$0.05 / $0.20"],
        ] },
        { type: "list", items: [
          "Gemini 3.1 Pro Preview requests above 200K input tokens bill the whole request at the long-context rates: $4 input and $18 output per 1M.",
          "Gemini 3.1 Flash Image (Nano Banana 2) generates images on the same route; image output is $60 per 1M image tokens officially, $30 after the discount.",
          "Cached input on the text models bills at 10% of the fresh-input rate, so prompt-cache-heavy workloads settle even lower.",
        ] },
        { type: "link", text: "Gemini 3.6 Flash rates, context and output limits", href: "/models/gemini-3-6-flash" },
        { type: "link", text: "Full Gemini pricing breakdown, including cache and image legs", href: "/docs/learn/gemini-api-pricing" },
      ] },
      { h2: "Native protocol: official SDK, streaming, free token counting", blocks: [
        { type: "p", text: "Because the wire format is stock Gemini, the official Google GenAI SDK works with only the base URL and key changed:" },
        { type: "code", code: `import os\nfrom google import genai\nfrom google.genai import types\n\nclient = genai.Client(\n    api_key=os.environ["APITOKEN_API_KEY"],\n    http_options=types.HttpOptions(base_url="${ROUTER}"),\n)\n\nresponse = client.models.generate_content(\n    model="gemini-3.6-flash",\n    contents="Reply with exactly: connected",\n)\nprint(response.text)` },
        { type: "list", items: [
          "Streaming uses streamGenerateContent?alt=sse on the same model path and delivers incremental chunks.",
          "countTokens runs on the same path and is free — use it to estimate large prompts before spending on generation.",
          "Keep the key in an environment variable such as APITOKEN_API_KEY, never in source code.",
        ] },
        { type: "note", text: "Configure the SDK with the bare host only. The Google SDK appends /v1beta itself; if your base URL already ends in /v1beta, the doubled segment produces a 404 on every call." },
      ] },
      { h2: "How the prepaid balance and 50% discount settle", blocks: [
        { type: "p", text: "There is no monthly fee and no seat license; the balance is spent only when requests run. Every call settles in three steps:" },
        { type: "list", items: [
          "The request is metered at official Google token rates first, including cached-input and long-context legs.",
          "Your active discount is subtracted — B2C accounts get a flat 50% off official spend on every request.",
          "The net amount leaves your prepaid balance, so $50 of balance covers $100 of official-rate usage.",
        ] },
        { type: "note", text: "When the balance reaches zero, requests fail with an insufficient-balance error until you top up again — there is no overdraft and no surprise charge to your card." },
      ] },
      { h2: "The same balance also runs GPT, Claude and Kimi", blocks: [
        { type: "p", text: "The key is not Gemini-only. One prepaid balance backs all four supported providers; what changes per provider is the endpoint, the auth header and the model ID:" },
        { type: "table", headers: ["Provider", "Base URL", "Auth header"], rows: [
          ["Gemini", `${ROUTER}`, "x-goog-api-key"],
          ["Claude and Kimi", `${ROUTER}/v1/messages`, "x-api-key"],
          ["GPT", `${ROUTER}/v1`, "Authorization: Bearer"],
        ] },
        { type: "p", text: "In practice this means a Gemini prototype can add a Claude or GPT fallback without a second account, a second bill, or a second credential to manage." },
      ] },
      { h2: "Verify the key with one request", blocks: [
        { type: "p", text: "Before wiring the key into a project, send one minimal generateContent call. It costs a fraction of a cent and proves the whole chain — key, balance, endpoint:" },
        { type: "code", code: `curl ${ROUTER}/v1beta/models/gemini-3.6-flash:generateContent \\\n  -H "x-goog-api-key: $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{"contents":[{"parts":[{"text":"Reply with exactly: connected"}]}]}'` },
        { type: "list", items: [
          "401 — the key is missing or mistyped, or the header is not x-goog-api-key; x-api-key and Authorization: Bearer belong to the other lanes.",
          "404 — the model ID is not in the catalog, or /v1beta appears twice in the URL from an SDK base-URL mistake.",
          "402 / insufficient balance — the balance is empty; top up any whole-dollar amount.",
          "429 — rate limited; respect the Retry-After header and lower concurrency.",
        ] },
      ] },
    ],
    faq: [
      { q: "Do I need a Google Cloud project to buy a Gemini API key?", a: "No. apiToken.sale owns the gateway account and the upstream billing; your client only needs the custom base URL and the sk-pool key sent as x-goog-api-key." },
      { q: "Which header authenticates Gemini requests?", a: "x-goog-api-key. Do not send Anthropic's x-api-key or the OpenAI-style Authorization: Bearer on the native Gemini routes — each provider lane has its own header." },
      { q: "Can I pay for a Gemini API key with cryptocurrency?", a: "Yes. Checkout accepts bank cards and crypto through a secure payment provider, top-ups are any whole-dollar amount, and the balance never expires." },
      { q: "What is the cheapest way to try the key?", a: "Accounts created with Google or GitHub start with $5 of platform bonus credit, and Gemini 2.5 Flash-Lite bills at $0.05 input and $0.20 output per 1M tokens after the discount — enough for extensive testing." },
      { q: "Can the same key call GPT, Claude and Kimi?", a: "Yes. The key and balance are shared across all supported providers; you switch the endpoint, auth header and model ID, never the account." },
      { q: "Does the Gemini protocol here match Google's own API?", a: "Yes — generateContent, streamGenerateContent?alt=sse, countTokens and the official Google GenAI SDK all work unchanged. Only the base URL and the key differ." },
    ],
    related: ["gemini-api-quickstart", "gemini-api-pricing", "gemini-pro-vs-flash-vs-flash-lite", "how-billing-works"],
    published: "2026-08-09",
    updated: "2026-08-17",
  };
