import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "cursor-without-anthropic-account",
  cluster: "integrate",
  title: "Claude in Cursor Without an Anthropic Account",
  h1: "Run Claude in Cursor without an Anthropic account",
  description: "No Anthropic account? Use Claude in Cursor with an apiToken.sale key instead. Instant access, card or crypto payment, and a flat 50% off official API rates.",
  keywords: ["cursor without anthropic account", "claude cursor no anthropic", "cursor claude api key", "use claude without anthropic account", "claude api key", "anthropic-compatible api", "claude api base url", "claude api setup", "claude api integration", "claude api python", "claude api typescript"],
  dek: "If you cannot or would rather not create an Anthropic account, apiToken.sale issues its own key that Cursor accepts as an Anthropic provider.",
  sections: [
    { h2: "Why this works", blocks: [
      { type: "p", text: "Cursor talks to the Anthropic Messages API. apiToken.sale exposes exactly that API, so Cursor cannot tell the difference — it just uses your key and base URL." },
    ] },
    { h2: "Set it up", blocks: [
      { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : ${BASE}\nAPI key  : ${KEY}\nModel    : claude-opus-4-8` },
      cta(),
    ] },
    { h2: "What you keep", blocks: [
      { type: "list", items: [
        "The full Claude line — Opus, Sonnet and Haiku — on one key.",
        "Standard Anthropic behaviour: streaming, tool use, system prompts.",
        "An optional lifetime spending limit and expiration date per key, plus token-level usage in the dashboard.",
      ] },
      { type: "p", text: "Nothing about how you use Cursor changes; you simply source the key from apiToken.sale instead of Anthropic." },
    ] },
  ],
  faq: [
    { q: "Do I need an Anthropic account for this?", a: "No. apiToken.sale provides the key and balance, so no Anthropic account is required." },
    { q: "Is the integration official Anthropic API?", a: "Cursor uses the standard Anthropic Messages API; apiToken.sale serves that same API at a discount." },
  ],
  related: ["claude-api-key-for-cursor", "claude-api-for-russia", "how-to-buy-claude-api-key", "apitoken-vs-anthropic-direct"],
};
