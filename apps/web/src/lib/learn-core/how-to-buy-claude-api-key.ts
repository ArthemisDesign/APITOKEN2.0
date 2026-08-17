import type { LearnArticle } from "../learn";
import { quickSetupSteps, cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "how-to-buy-claude-api-key",
  cluster: "buy",
  title: "How to Buy a Claude API Key",
  h1: "How to buy a Claude API key",
  description: "Buy a Claude API key in minutes with apiToken.sale — one key for every Claude model, prepaid balance, card or crypto payment, no Anthropic account required.",
  keywords: ["buy claude api key", "how to buy claude api", "claude api key", "purchase claude api access", "anthropic api key", "buy claude api", "claude api access", "claude api tokens", "claude api top up", "claude api reseller", "claude api provider"],
  dek: "You do not need an Anthropic account, an invite, or a company card to buy a Claude API key. With apiToken.sale you top up prepaid balance, generate one key, and call the same Anthropic Messages API at a discount — instant Claude API access for Opus, Sonnet and Haiku.",
  sections: [
    { h2: "Get your key in three steps", blocks: [quickSetupSteps, cta()] },
    { h2: "How payment works", blocks: [
      { type: "p", text: "Top up any whole-dollar amount you like — there is no fixed product catalog. Your balance is prepaid, never expires, and is spent only when API requests run." },
      { type: "list", items: [
        "Pay by bank card or with cryptocurrency through a secure checkout provider.",
        "Every request is converted to official Anthropic API spend, then your active discount is applied.",
        "B2C accounts get a flat 50% off official spend on every request.",
      ] },
    ] },
    { h2: "What you can do with the key", blocks: [
      { type: "p", text: "One key unlocks the full supported Claude line — Opus, Sonnet and Haiku — across Claude Code, Cursor, Cline, Continue, Zed and the official Anthropic SDKs. Nothing about the protocol changes; only the price does." },
    ] },
    { h2: "Which Claude models and tools you get", blocks: [
      { type: "p", text: "One Claude API key unlocks the full supported line on a single balance, and works in every Anthropic-compatible tool." },
      { type: "list", items: [
        "Models: Claude Opus 4.8 and 4.7, Sonnet 5 and 4.6, Haiku 4.5.",
        "Tools: Claude Code, Cursor, Cline, Continue, Zed and the Anthropic SDKs.",
        "Formats: the Anthropic Messages API with streaming and tool use.",
      ] },
    ] },
  ],
  faq: [
    { q: "Do I need an Anthropic account to buy a Claude API key?", a: "No. apiToken.sale issues its own key and balance, so you can start without an Anthropic account, invite, or approval." },
    { q: "How fast is the key active?", a: "Instantly. You generate the key in the dashboard and it works on the next request — there is no waitlist or manual review." },
    { q: "How much does it cost to start?", a: "You can top up any whole-dollar amount. New accounts created with Google or GitHub also get $5 of platform bonus credit." },
    { q: "Is this the official Claude API?", a: "Yes — it serves the same Anthropic Messages API and the same Claude models. Only the price and the way you sign up and pay are different." },
  ],
  related: ["claude-api-quick-setup", "cheapest-claude-api", "claude-api-crypto-payment", "free-claude-api-key"],
  updated: "2026-07-17",
};
