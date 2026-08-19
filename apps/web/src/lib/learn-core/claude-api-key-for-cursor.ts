import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-key-for-cursor",
  cluster: "integrate",
  title: "Claude API Key for Cursor",
  h1: "Use a Claude API key in Cursor",
  description: "Get a Claude API key for Cursor with no Anthropic account: point Cursor's Anthropic provider at router.apitoken.sale and code at a flat 50% off official rates.",
  keywords: ["claude api key for cursor", "cursor claude api", "cursor anthropic api key", "use claude in cursor with api key", "cursor custom anthropic base url", "cursor bring your own api key", "cursor without cursor pro", "claude api key", "anthropic-compatible api", "claude api base url"],
  dek: "A Claude API key for Cursor replaces the bundled plan with your own Anthropic-compatible endpoint, and Cursor's settings make that a two-minute change. Point the Anthropic provider at apiToken.sale and the same prepaid balance that covers GPT, Gemini and Kimi powers Cursor's chat, inline edit and agent at a flat 50% off official token rates. No extension, no proxy, no waitlist.",
  sections: [
    { h2: "Point Cursor's Anthropic provider at router.apitoken.sale", blocks: [
      { type: "p", text: "Cursor's built-in Anthropic provider accepts a custom base URL and API key, so any endpoint that speaks the Anthropic Messages API can drive Cursor's chat, Composer and agent. apiToken.sale serves exactly that API, which means a Claude API key for Cursor is one settings change: swap the endpoint, paste the key, choose a model. Everything Cursor sends — system prompts, tool definitions, streamed tokens — travels over the standard protocol, so behaviour inside the editor is identical to a key issued by Anthropic." },
      { type: "steps", items: [
        "Open the apiToken.sale dashboard and generate a key (it looks like sk-pool-…). One key covers every supported Claude model, plus GPT, Gemini and Kimi.",
        "In Cursor, go to Settings → Models and scroll to the Anthropic section. It is a separate provider from OpenAI — editing the wrong one is the most common setup mistake.",
        `Set the Anthropic base URL to ${BASE} and paste your ${KEY} key, then let Cursor verify the connection.`,
        "Enable a current model ID such as claude-opus-4-8 — type it into the model list if the dropdown does not offer it — and select it in the chat model picker.",
      ] },
      { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : ${BASE}\nAPI key  : ${KEY}\nModel    : claude-opus-4-8` },
    ] },
    { h2: "Prove the key before you debug Cursor", blocks: [
      { type: "p", text: "When Claude does not answer in Cursor, the fault is either the key or the editor — decide which in thirty seconds instead of toggling settings blindly. The router exposes the Anthropic Messages API, so a single curl with the x-api-key and anthropic-version headers exercises the exact path Cursor will use. If JSON comes back, your key, balance and endpoint are healthy and any remaining problem lives inside Cursor's settings." },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 16,\n    "messages": [{"role": "user", "content": "ping"}]\n  }'` },
      { type: "p", text: "A 401 here means the key was pasted incompletely or the base URL has a typo. A model-not-found error means the ID is stale — use a current one like claude-sonnet-5 or claude-opus-4-8. Only when curl succeeds but Cursor still fails should you suspect the editor: re-open Settings → Models, confirm you edited the Anthropic provider rather than OpenAI, and re-run Cursor's own verify button." },
    ] },
    { h2: "Match the model to what Cursor is actually doing", blocks: [
      { type: "p", text: "Cursor does not burn tokens uniformly. An agentic Composer run that reads and rewrites a dozen files can consume orders of magnitude more than a single inline edit, so the right model depends on the surface, not on a blanket 'best model' answer. Because every supported Claude model sits behind the same key and prepaid balance, switching costs nothing but a dropdown selection — route the cheap work down and reserve the expensive tier for the sessions that need it." },
      { type: "table", headers: ["Cursor surface", "Model to pick", "Why"], rows: [
        ["Agent / Composer multi-file edits", "claude-opus-4-8", "Strongest reasoning; fewer failed edit loops on hard refactors"],
        ["Everyday chat and inline edits", "claude-sonnet-5", "Near-Opus coding quality at a much lower token price"],
        ["Quick questions and small completions", "claude-haiku-4-5", "Fastest and cheapest; ideal for throwaway queries"],
      ] },
      { type: "p", text: "Usage is metered per token against your balance, so a day of Haiku-and-Sonnet work with occasional Opus escalations stays far below an all-Opus habit. The dashboard shows token-level usage, so you can see which Cursor surface actually costs you money and adjust." },
      { type: "link", text: "Current Claude model IDs and per-token rates", href: "/models" },
      { type: "link", text: "Estimate a month of Cursor usage with the cost calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "Prepaid balance, one key, no Cursor Pro dependency", blocks: [
      { type: "p", text: "The arrangement is deliberately simple: you top up a prepaid balance by card or crypto, every request draws it down at official token rates minus a flat 50% discount, and nothing renews or expires on a schedule. There is no seat, no bundle and no subscription to cancel — when the balance runs out, requests stop until you top up again. Features that require Cursor's own paid plan are separate from the model provider: bringing your own key replaces how Claude is billed, not what Cursor ships." },
      cta(),
      { type: "p", text: "Each key can carry an optional lifetime spending limit and an expiration date, which makes a dedicated Cursor key a clean way to cap what the editor can spend — create one key for Cursor, another for scripts, and read their usage separately in the dashboard. The key is also language- and platform-agnostic: Cursor uses the same setting for Python, TypeScript, Go or Rust projects, and the Anthropic provider configuration is identical on Windows, macOS and Linux. You are configuring the model endpoint, not the language." },
      { type: "p", text: "And because the same key speaks to GPT, Gemini and Kimi models on the same balance, a second machine, a teammate's editor or a different tool never needs a new account — just the same base URL and key pasted into whatever client supports an Anthropic-compatible endpoint." },
    ] },
  ],
  faq: [
    { q: "Can I use my own Claude API key in Cursor instead of Cursor Pro?", a: "Yes. Cursor's Anthropic provider accepts a custom base URL and key, so you can point it at apiToken.sale and run Claude on your own prepaid balance. Features tied to Cursor's own plan are separate from the model provider." },
    { q: "Why does Cursor say my Claude API key is invalid?", a: "Almost always one of three causes: you edited the OpenAI provider instead of Anthropic, the base URL is not exactly https://router.apitoken.sale, or the key was pasted incompletely. A quick curl to /v1/messages with your x-api-key header tells you whether the key itself is fine." },
    { q: "Which Claude model should I select in Cursor — and does it work on Windows and Mac?", a: "claude-sonnet-5 is the right default for chat and inline edits, claude-opus-4-8 for long agentic Composer sessions, and claude-haiku-4-5 for quick cheap questions — all on the same key and balance. The Anthropic provider setting is identical on Windows, macOS and Linux." },
  ],
  related: ["cursor-without-anthropic-account", "claude-api-for-vs-code", "claude-api-quick-setup", "claude-sonnet-api"],
  updated: "2026-08-17",
};
