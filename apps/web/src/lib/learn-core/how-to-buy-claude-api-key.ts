import type { LearnArticle } from "../learn";
import { BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "how-to-buy-claude-api-key",
  cluster: "buy",
  title: "How to Buy a Claude API Key",
  h1: "How to buy a Claude API key",
  description: "Buy a Claude API key in minutes: prepaid balance, card or crypto checkout, one key for every Claude model at a flat 50% off official spend — no Anthropic account required.",
  keywords: ["buy claude api key", "how to buy claude api key", "claude api key", "purchase claude api access", "anthropic api key", "buy claude api with crypto", "claude api key without anthropic account", "claude api prepaid balance", "claude api top up", "claude api access", "claude api discount"],
  dek: "If you want to buy a Claude API key without an Anthropic account, an invite, or a company card, the whole process takes about five minutes: create an account, top up a prepaid balance by card or crypto, and generate a key. That key calls the same Anthropic Messages API as one issued by Anthropic itself — Opus, Sonnet and Haiku included — at a flat 50% off official spend. Below is the exact purchase flow, the billing math, and the endpoints to point your tools at.",
  sections: [
    { h2: "The five-minute purchase: account, balance, key", blocks: [
      { type: "p", text: "Buying a Claude API key on apiToken.sale is three moves: sign up, top up a prepaid balance, click generate. The key is active on the very next request — no waitlist, no manual review, and no Anthropic account or approval at any point in the flow." },
      { type: "steps", items: [
        "Create an account with Google, GitHub, or email and password. Accounts created with Google or GitHub start with $5 of platform bonus credit, valid on supported Claude, GPT, Gemini and Kimi models; email/password accounts do not receive the bonus.",
        "Top up any whole-dollar amount — there is no fixed product catalog and no minimum plan. Pay by bank card or with cryptocurrency through a secure checkout provider.",
        `Open the dashboard and generate an API key. It looks like ${KEY} and works immediately; the same key also covers the supported GPT, Gemini and Kimi models on the platform.`,
        "Verify the key with one request (the curl at the end of this guide). A 200 with real output means you are done; a 401 means the key or the header name is wrong.",
      ] },
    ] },
    { h2: "No Anthropic account, invite, or company card", blocks: [
      { type: "p", text: "Buying direct from Anthropic means an Anthropic account, and for many buyers that is the blocker: the signup, the approval, the card requirement. apiToken.sale replaces that entire layer — it issues its own key and its own prepaid balance, so the only things you need are an email address (or a Google/GitHub login) and a way to pay." },
      { type: "p", text: "What you get in return is not a clone or a rerouted third-party model. The gateway serves the same Anthropic Messages API and the same Claude models, and your requests behave the same way. Only three things differ from buying direct: the price per call, how you sign up, and how you pay." },
    ] },
    { h2: "One key, every Claude model — and three protocols", blocks: [
      { type: "p", text: "The key is not tied to one model or one tool. A single balance covers the full supported Claude line, each under its standard model ID:" },
      { type: "list", items: [
        "Claude Opus 4.8 (claude-opus-4-8) and Opus 4.7",
        "Claude Sonnet 5 (claude-sonnet-5) and Sonnet 4.6",
        "Claude Haiku 4.5 (claude-haiku-4-5)",
      ] },
      { type: "p", text: "The Anthropic lane serves the Messages API unchanged: SSE streaming, tool use and system prompts behave exactly as they do against Anthropic's own endpoint. What changes from client to client is only the protocol lane you point at — all three share the same key and the same balance:" },
      { type: "table", headers: ["Protocol lane", "Endpoint", "Auth header"], rows: [
        ["Anthropic Messages (Claude, Kimi)", `${BASE}/v1/messages`, "x-api-key"],
        ["OpenAI-compatible (GPT and OpenAI-shaped clients)", `${OPENAI_BASE}`, "Authorization: Bearer"],
        ["Native Gemini", `${BASE}`, "x-goog-api-key"],
      ] },
      { type: "p", text: "Because the wire format is stock Anthropic, the key drops into every Anthropic-compatible tool with no plugin or proxy: Claude Code, Cursor, Cline, Continue, Zed and the official Anthropic SDKs. Nothing about the protocol changes; only the price does." },
    ] },
    { h2: "Prepaid balance math: how the 50% discount is applied", blocks: [
      { type: "p", text: "There is no subscription and no monthly fee. Your balance is prepaid, never expires, and is spent only when API requests run — idle weeks cost nothing. Billing works in three steps on every call:" },
      { type: "list", items: [
        "The request is metered at official Anthropic token rates first.",
        "Your active discount is subtracted: B2C accounts get a flat 50% off official spend on every request.",
        "The net amount is drawn from your prepaid balance — so $50 of balance covers $100 of official-rate usage.",
      ] },
      { type: "note", text: "When the balance runs out, requests fail with an insufficient-balance error until you top up again — there is no overdraft and no surprise charge to your card." },
      { type: "link", text: "Per-model pricing, including cache rates", href: "/models" },
      { type: "link", text: "Estimate a month of usage in the free cost calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "Pointing Claude Code, Cursor and the SDKs at your key", blocks: [
      { type: "p", text: "Every Anthropic-compatible client needs exactly two values changed: the base URL and the credential. Prompts, streaming code and tool definitions stay as they are. For Claude Code and other shell-driven agents, export the two environment variables:" },
      { type: "code", code: `export ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_AUTH_TOKEN=${KEY}` },
      { type: "p", text: "The official SDKs take the same two arguments:" },
      { type: "code", code: `import anthropic\n\nclient = anthropic.Anthropic(\n    base_url="${BASE}",\n    api_key="${KEY}",\n)` },
      { type: "p", text: `In Cursor, Cline, Continue and Zed the same two fields live in the provider settings — for example Cursor → Settings → Models → Anthropic API. Paste the key, set the base URL to ${BASE}, pick a model such as claude-opus-4-8, and requests flow through your prepaid balance with the discount applied.` },
      { type: "note", text: `If a client only offers an "OpenAI-compatible" provider type, use ${OPENAI_BASE} with an Authorization: Bearer header instead — the x-api-key header belongs to the Anthropic Messages lane.` },
    ] },
    { h2: "Verify the purchase with one request", blocks: [
      { type: "p", text: "Before you wire the key into a big project, send one minimal call. It costs a fraction of a cent and proves the whole chain — key, balance, endpoint — end to end:" },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-haiku-4-5",\n    "max_tokens": 64,\n    "messages": [{"role":"user","content":"ping"}]\n  }'` },
      { type: "list", items: [
        "401 Unauthorized — the key is missing or mistyped, the header is not x-api-key, or the base URL is wrong.",
        "400 Bad Request — check the model ID (for example claude-haiku-4-5) and that max_tokens is set.",
        "402 / insufficient balance — the balance is empty; top up any whole-dollar amount.",
        "429 Too Many Requests — respect the Retry-After header and lower concurrency.",
      ] },
    ] },
  ],
  faq: [
    { q: "Do I need an Anthropic account to buy a Claude API key?", a: "No. apiToken.sale issues its own key and prepaid balance, so you can start without an Anthropic account, an invite, or approval — and there is no company-card requirement either." },
    { q: "How fast is the key active after purchase?", a: "Instantly. You generate the key in the dashboard and it works on the next request — there is no waitlist or manual review." },
    { q: "What is the minimum I can spend to start?", a: "Top-ups are any whole-dollar amount, so you can start with a few dollars. New accounts created with Google or GitHub also get $5 of platform bonus credit." },
    { q: "Can I pay for a Claude API key with cryptocurrency?", a: "Yes. Checkout accepts bank cards and cryptocurrency through a secure payment provider, and the topped-up balance never expires." },
    { q: "Is this the official Claude API?", a: "Yes — it serves the same Anthropic Messages API and the same Claude models, including streaming and tool use. Only the price, and the way you sign up and pay, are different." },
  ],
  related: ["claude-api-quick-setup", "cheapest-claude-api", "claude-api-crypto-payment", "free-claude-api-key"],
  updated: "2026-08-17",
};
