import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "apitoken-vs-proxyapi",
  cluster: "compare",
  title: "apiToken.sale vs ProxyAPI for Claude",
  h1: "apiToken.sale vs ProxyAPI",
  description: "Comparing Claude API resellers: apiToken.sale offers a native Anthropic endpoint with a flat 50% discount, card or crypto payment, and one key for every model.",
  keywords: ["proxyapi alternative", "apitoken vs proxyapi", "proxyapi claude", "claude api без proxyapi", "claude api reseller", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api without anthropic account", "claude api vs anthropic", "best claude api"],
  dek: "Developers searching for a ProxyAPI alternative usually want two things: Claude without an Anthropic account, and a lower bill. apiToken.sale delivers both — a native Anthropic Messages API endpoint with a flat 50% discount on official Claude spend, payable by card or crypto. This comparison breaks down where the two services genuinely differ: protocol fidelity, pricing mechanics, and key controls.",
  sections: [
    { h2: "Is apiToken.sale a real ProxyAPI alternative?", blocks: [
      { type: "p", text: "Yes — and for Claude-heavy workloads it is the more direct option. Both services let you reach Claude without an Anthropic account, but apiToken.sale exposes the native Anthropic Messages API at a flat 50% below official token rates, while a standard reseller re-sells access at list price or with a markup on top. If Claude is the model you actually burn tokens on, that combination — native protocol plus a real discount — is the whole comparison." },
      { type: "p", text: "The account barrier is identical on both sides: neither service asks for an Anthropic account, a waitlist, or a billing profile in a specific country. What you are choosing between is not access but economics and fidelity — how much each token costs, and whether the endpoint speaks Anthropic's protocol natively or translates it through an adapter layer." },
    ] },
    { h2: "What the flat 50% discount does to a Claude bill", blocks: [
      { type: "p", text: "Every request is metered at official Anthropic token rates, then the flat 50% B2C discount is applied before the charge touches your prepaid balance. There is no subscription tier and no per-seat fee — you top up, spend, and watch the usage line by line in the dashboard." },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "apiToken.sale (−50%)"], rows: [
        ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
      ] },
      { type: "list", items: [
        "Top up by bank card or cryptocurrency; the balance never expires, so a burst project and a quiet month each cost exactly what they used.",
        "One prepaid key and one balance cover Opus, Sonnet and Haiku — and the same key works for supported GPT, Gemini and Kimi models, so you are not juggling a reseller per provider.",
        "Spend is auditable per request, with model and token breakdown, instead of a monthly invoice you reconcile after the fact.",
      ] },
      { type: "link", text: "Full per-model pricing, including cache rates", href: "/models" },
      { type: "link", text: "Estimate your monthly Claude spend in the free calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "Native Anthropic Messages API vs a translated endpoint", blocks: [
      { type: "p", text: `Most comparison pages skip the protocol question, but it decides whether your tooling survives the move. apiToken.sale serves the standard Anthropic Messages API at ${BASE}: same endpoints, same model IDs (claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5), same request and response format your code already expects. Claude Code, Cursor and the official Anthropic SDKs work unchanged — you edit a base URL, not your application. There is no adapter layer between you and Claude.` },
      { type: "p", text: "That matters because the Messages API carries features adapters tend to drop: SSE streaming with stream:true, prompt caching through cache_control breakpoints (cached input bills at the lower cache-read rate), tool-use blocks and system prompts. A proxy that re-serializes your request into another provider's shape can silently lose one of these — and you find out when a cached workload suddenly bills like an uncached one." },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-sonnet-5","max_tokens":256,"messages":[{"role":"user","content":"ping"}]}'` },
    ] },
    { h2: "How to switch from ProxyAPI in one base-URL change", blocks: [
      { type: "steps", items: [
        `Create a free account and generate a key in the dashboard (it looks like ${KEY}). One key covers every supported Claude model.`,
        `Point your tools at the native endpoint: export ANTHROPIC_BASE_URL=${BASE} and ANTHROPIC_API_KEY for Claude Code, or paste the same pair into Cursor's Anthropic provider settings.`,
        "Send one real request and confirm two things: a normal Anthropic-shaped response, and a metered line in the dashboard with the 50% discount applied. Then remove the old reseller credentials from your shell rc files and tool settings so nothing silently falls back.",
      ] },
      { type: "code", code: `# Claude Code — ~/.zshrc or ~/.bashrc\nexport ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# Cursor → Settings → Models → Anthropic API\n# Base URL : ${BASE}\n# API key  : ${KEY}` },
      { type: "note", text: "Two failure modes cover almost every broken switch: a stale shell that still exports the old reseller's variables (open a fresh terminal before launching claude), and editing Cursor's OpenAI provider instead of its Anthropic one. A 401 after the change means the key or the base URL is wrong — re-check both, in that order." },
    ] },
    { h2: "Key guardrails: lifetime spending limits and expiration", blocks: [
      { type: "p", text: "A reseller key is often all-or-nothing: whoever holds the key spends the balance. apiToken.sale keys carry two explicit guardrails you set in the dashboard — a lifetime spending limit that caps total cumulative spend, and an optional expiration date after which the key stops authenticating. For a key that lives in a CI job or on a shared workstation, that is the difference between a leaked key and a leaked budget." },
      { type: "list", items: [
        "Lifetime spending limit: the key stops spending once cumulative usage reaches the cap you set.",
        "Optional expiration date: the key dies on schedule, not when someone remembers to revoke it.",
        "Per-request metering: model, token counts and the discounted charge for every call, visible in the dashboard.",
      ] },
    ] },
    { h2: "When staying on a multi-provider reseller makes sense", blocks: [
      { type: "p", text: "Be honest about the workload. If Claude is an occasional side model and you already run several other providers through your current reseller, staying put is defensible — one integration you already debugged. The case for moving is strongest when Claude is the daily driver: Claude Code sessions, agent loops, batch jobs — anywhere a 50% discount and protocol fidelity compound over millions of tokens." },
      { type: "p", text: "The multi-provider argument also cuts both ways: the same apiToken.sale key and balance serve supported GPT, Gemini and Kimi models, so moving Claude over does not strand the rest of your stack. If something goes wrong mid-migration, support answers in English and Russian on Telegram or at apitokensale@gmail.com, and refunds, if ever needed, go through the original payment provider via that same support channel." },
      cta(),
    ] },
  ],
  faq: [
    { q: "Is apiToken.sale cheaper than ProxyAPI for Claude?", a: "apiToken.sale meters requests at official Anthropic token rates and applies a flat 50% B2C discount — Claude Sonnet 5 works out to $1.50 / $7.50 per 1M input/output tokens instead of the official $3 / $15. A standard reseller sells at list price or adds a markup on top of it, so on Claude-heavy workloads the gap compounds quickly." },
    { q: "Will Claude Code and Cursor still work after switching from ProxyAPI?", a: `Yes. apiToken.sale serves the native Anthropic Messages API, so Claude Code, Cursor and the official SDKs need only a base-URL change to ${BASE} plus your key — streaming, tool use and prompt caching behave exactly as with api.anthropic.com.` },
    { q: "Do I need an Anthropic account or a specific billing country?", a: "No — like ProxyAPI, apiToken.sale removes the Anthropic-account barrier entirely. You register, top up by bank card or cryptocurrency, and the prepaid balance never expires." },
    { q: "Is there a free way to test apiToken.sale before moving my workloads?", a: "New accounts created with Google or GitHub start with $5 of platform bonus credit, valid on supported Claude, GPT, Gemini and Kimi models; email/password accounts do not receive the bonus. That is enough to run a real Claude Code session and compare the metered cost yourself." },
    { q: "Can I keep using GPT or Gemini if I move my Claude traffic?", a: "Yes — the same prepaid key and balance cover supported GPT, Gemini and Kimi models alongside Claude Opus, Sonnet and Haiku, so consolidating on one key does not strand your other providers." },
  ],
  related: ["apitoken-vs-anthropic-direct", "apitoken-vs-openrouter", "cheapest-claude-api", "claude-api-for-russia"],
  updated: "2026-08-17",
};
