import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "cursor-without-anthropic-account",
  cluster: "integrate",
  title: "Claude in Cursor Without an Anthropic Account",
  h1: "Run Claude in Cursor without an Anthropic account",
  description: "No Anthropic account? Use Claude in Cursor with an apiToken.sale key instead. Instant access, card or crypto payment, and a flat 50% off official API rates.",
  keywords: ["cursor without anthropic account", "use claude in cursor without anthropic", "cursor anthropic api key", "cursor custom anthropic base url", "cursor bring your own api key", "claude api key for cursor", "run claude in cursor", "cursor byok claude", "anthropic-compatible api", "claude in cursor no subscription"],
  dek: "Using Cursor without an Anthropic account comes down to one detail: Cursor's Anthropic provider accepts any compatible base URL and key, and apiToken.sale serves exactly that API. This guide walks through the settings, shows which Cursor features actually run on your key, and explains how prepaid billing at a flat 50% off official API rates replaces the Anthropic invoice.",
  sections: [
    { h2: "Yes — Cursor only needs a base URL and a key", blocks: [
      { type: "p", text: "You do not need an Anthropic account to run Claude in Cursor. Cursor's Anthropic provider lets you override the base URL and paste your own API key, and apiToken.sale issues a key that Cursor accepts in exactly that slot. Sign up, paste two values into Settings, and Claude answers inside Cursor with no Anthropic involvement at any point." },
      { type: "p", text: "The reason this works is that Cursor talks to the Anthropic Messages API: a POST to /v1/messages with an x-api-key header and an anthropic-version header. apiToken.sale exposes exactly that API at its router, so Cursor cannot tell the difference — it sends the same request shape it would send to Anthropic and gets the same response shape back. Streaming, tool use and system prompts all behave the standard Anthropic way, because the protocol on the wire is the standard Anthropic protocol." },
      { type: "p", text: "Requests travel straight from your machine to the endpoint you configured. There is no relay through Cursor's servers for BYOK traffic and no extra moving part to debug: if the endpoint answers, Cursor works." },
    ] },
    { h2: "Point Cursor's Anthropic provider at apiToken.sale", blocks: [
      { type: "steps", items: [
        "Open Cursor → Settings → Models and scroll to the Anthropic API section.",
        `Set the base URL to ${BASE} and paste your apiToken.sale key (it looks like ${KEY}) into the API key field.`,
        "Add a current model ID to the model list — claude-opus-4-8 is the safe default — and make sure the toggle next to it is on.",
        "Open a chat, select that model, and send any message. A streamed reply confirms the key, the base URL and billing are all live.",
      ] },
      { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : ${BASE}\nAPI key  : ${KEY}\nModel    : claude-opus-4-8\n\n# Optional: verify the endpoint before you even open Cursor\ncurl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-opus-4-8","max_tokens":64,"messages":[{"role":"user","content":"ping"}]}'` },
      { type: "p", text: "The curl check is worth the thirty seconds it costs. If it returns a JSON completion, every later problem is inside Cursor's settings; if it returns an auth error, the key itself is wrong and no amount of clicking in Cursor will fix it." },
    ] },
    { h2: "What runs on your key inside Cursor — and what does not", blocks: [
      { type: "p", text: "Bringing your own Anthropic key reroutes the features that call a Claude model. It does not change how Cursor's own features are delivered, and it does not unlock anything Cursor gates behind its own plan." },
      { type: "list", items: [
        "Chat, Composer and agent mode run on the Claude model you selected, and the tokens bill against your prepaid balance.",
        "Inline edit (Cmd/Ctrl+K) uses the same selected model and the same key.",
        "Cursor Tab autocomplete is served by Cursor's own autocomplete models, not the Anthropic API — your key is never involved, and Tab availability still depends on your Cursor plan.",
        "Features Cursor reserves for its own subscribers stay reserved; a model provider key changes where model calls go, not what your Cursor license includes.",
      ] },
      { type: "note", text: "The common confusion: Claude answers in chat but Tab suggestions are gone. That is expected — Tab never used your Anthropic key, even when the key came from Anthropic itself. The two systems have separate billing and separate providers." },
    ] },
    { h2: "One key, the whole Claude lineup", blocks: [
      { type: "p", text: "A single apiToken.sale key unlocks the full Claude line — Opus, Sonnet and Haiku — so you can switch tiers inside Cursor without juggling credentials. Add each model ID in Settings → Models and pick per task:" },
      { type: "table", headers: ["Model ID", "Tier", "Where it earns its place in Cursor"], rows: [
        ["claude-opus-4-8", "Opus", "Agent mode on multi-file refactors and the hardest reasoning tasks"],
        ["claude-sonnet-5", "Sonnet", "Daily driver for chat, inline edits and most agent runs"],
        ["claude-haiku-4-5", "Haiku", "Fast, low-cost iterations — renames, small fixes, quick questions"],
      ] },
      { type: "p", text: "Because all three draw from the same balance, the practical workflow is to default to Sonnet, drop to Haiku for throwaway prompts, and reserve Opus for tasks where a wrong answer costs more than the tokens do." },
      { type: "link", text: "Current model lineup with per-model pricing", href: "/models" },
    ] },
    { h2: "Prepaid billing instead of an Anthropic invoice", blocks: [
      { type: "p", text: "There is no Anthropic account on the other side, so there is no Anthropic invoice either. You top up an apiToken.sale balance by card or crypto, and every request from Cursor decrements it per token at a flat 50% off official API rates. Instant access: the key works as soon as it is generated, with no waitlist and no usage-tier review." },
      { type: "list", items: [
        "Token-level usage per key in the dashboard, so you can see exactly what Cursor costs you per day.",
        "An optional lifetime spending limit and expiration date per key — issue a dedicated key for Cursor, cap it, and a runaway agent loop can never burn more than you allowed.",
        "The same balance also covers GPT, Gemini and Kimi models if you later point other tools at it; nothing is locked to Cursor or to Claude.",
      ] },
      { type: "link", text: "Estimate a month of Cursor usage in the free calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "From signup to the first answer", blocks: [
      { type: "steps", items: [
        "Create an apiToken.sale account — sign up with Google or GitHub to start with $5 of platform bonus credit (email/password accounts do not receive the bonus).",
        "Top up by card or crypto when you need more balance; the bonus credit is enough to validate the whole setup first.",
        "Generate an API key (sk-pool-…) and, if you want a hard ceiling, set its lifetime spending limit and expiration date.",
        "Paste the base URL and key into Cursor as shown above, select claude-opus-4-8, and send your first prompt.",
      ] },
      cta(),
      { type: "p", text: "Nothing about how you use Cursor changes after this. You simply source the key and the balance from apiToken.sale instead of Anthropic — and you never create an Anthropic account at all." },
    ] },
    { h2: "The three setup errors people actually hit", blocks: [
      { type: "list", items: [
        "401 Unauthorized: the key was pasted truncated or with a stray space, or you edited the OpenAI provider instead of the Anthropic one. Re-paste the full key in the Anthropic section.",
        "Model not found: the model ID is not in Cursor's model list or is outdated. Add the exact string claude-opus-4-8 and enable it.",
        "Verify button fails: the base URL is wrong. It must be the bare router origin — no /v1 suffix, no trailing path — because Cursor appends the Messages API path itself.",
      ] },
      { type: "note", text: "If chat works but responses stop mid-stream on very long agent runs, check the key's lifetime spending limit in the dashboard first — an exhausted or expired key fails exactly this way." },
    ] },
  ],
  faq: [
    { q: "Do I need an Anthropic account to use Claude in Cursor?", a: "No. apiToken.sale provides the key and the prepaid balance, and Cursor accepts that key in its Anthropic provider slot — no Anthropic account is created or required at any step." },
    { q: "Is this the official Anthropic API?", a: "Cursor speaks the standard Anthropic Messages API, and apiToken.sale serves that same API at its router at a flat 50% off official rates. Request and response shapes, streaming, tool use and system prompts all behave the standard way." },
    { q: "Does Cursor Tab autocomplete work with my own Anthropic key?", a: "Tab is served by Cursor's own autocomplete models, not the Anthropic API, so it is unaffected by which key you paste — its availability depends on your Cursor plan, not on your API key." },
    { q: "Which Claude models can I use in Cursor with this setup?", a: "The full line on one key: Opus, Sonnet and Haiku. Add model IDs such as claude-opus-4-8 in Settings → Models and switch per task." },
    { q: "How do I pay for usage without an Anthropic account?", a: "You top up a prepaid apiToken.sale balance by card or crypto, and Cursor usage decrements it per token at a flat 50% off official API rates. New accounts created with Google or GitHub start with $5 of bonus credit." },
    { q: "Can I cap how much the Cursor key is allowed to spend?", a: "Yes. Each key can carry an optional lifetime spending limit and an expiration date, and the dashboard shows token-level usage per key, so a dedicated Cursor key is easy to budget." },
  ],
  related: ["claude-api-key-for-cursor", "claude-api-for-russia", "how-to-buy-claude-api-key", "apitoken-vs-anthropic-direct"],
  updated: "2026-08-17",
};
