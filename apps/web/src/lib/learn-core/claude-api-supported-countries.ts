import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-supported-countries",
  cluster: "explain",
  title: "Claude API Supported Countries",
  h1: "Where the Claude API works — and what actually gates it",
  description: "Which countries can use the Claude API? Anthropic gates direct accounts by billing country. apiToken.sale has no such gate: pay by card or crypto and call Claude from any region.",
  keywords: ["claude api supported countries", "is claude api available in my country", "claude api available countries", "anthropic api country restrictions", "claude api not available in your country", "anthropic supported billing countries", "claude api worldwide access", "use claude api from unsupported region", "claude api without anthropic account", "buy claude api from anywhere"],
  dek: "Searching for Claude API supported countries usually means one thing: Anthropic would not bill you where you live. The gate is Anthropic's billing-country list, not the models themselves. apiToken.sale issues your key and balance directly, so there is no billing-country requirement — you use the Claude API worldwide and pay by card or crypto.",
  sections: [
    { h2: "Is the Claude API available in your country?", blocks: [
      { type: "p", text: "The Claude API has no technical region lock — the gate is Anthropic's billing. If your country is not on Anthropic's supported list for direct billing, you cannot finish signup or attach a payment method, even though the models themselves are reachable over the network. apiToken.sale removes that gate: we issue your key and balance ourselves, so the Claude API is usable from regions Anthropic does not serve directly, with no Anthropic account at all." },
      { type: "p", text: "Anthropic publishes the list of countries where it sells API access directly, and that list shifts over time. When yours is missing, direct signup typically fails at the payment step — not because the API refuses your traffic, but because there is no way to put money behind the account. Every request you would have made still works fine technically; only the billing relationship is blocked." },
      { type: "p", text: "Because the account, key and balance all live on apiToken.sale, nothing in the flow asks where you are. You sign up, top up and call the API from wherever your laptop or servers happen to be, and the same applies to teammates in other regions sharing one balance through their own keys." },
      { type: "table", headers: ["Requirement", "Anthropic direct", "apiToken.sale"], rows: [
        ["Supported billing country", "Required to open a paid account", "Not required — no Anthropic account needed"],
        ["Payment method", "Card from a supported region", "Bank card, or cryptocurrency such as USDT and BTC"],
        ["Waitlist and verification", "Waitlist and approval can apply", "None — keys activate instantly, no company verification"],
        ["Where you can call from", "Billing tied to supported countries", `Worldwide, via ${BASE}`],
      ] },
      cta(),
    ] },
    { h2: "Paying and calling from a region Anthropic does not serve", blocks: [
      { type: "p", text: "Checkout adapts to your region instead of rejecting it. Pay by bank card where cards work; where they are declined or unavailable, choose cryptocurrency — USDT and other stablecoins, BTC and major coins — through a secure payment provider, and the balance credits once the network confirms the transaction. You top up any whole-dollar amount, the balance is prepaid and never expires, and every request is metered at official Anthropic rates with a flat 50% B2C discount applied." },
      { type: "steps", items: [
        "Create an account on apiToken.sale — no approval step and no billing-country form.",
        "Top up any whole-dollar amount by bank card, or pick crypto and send the exact amount shown; the balance credits after on-chain confirmation.",
        "Generate an API key (it looks like sk-pool-…). One key covers supported Claude, GPT, Gemini and Kimi models on a single balance.",
        `Point your client at ${BASE} using the Anthropic Messages protocol with the x-api-key header, then send a request.`,
      ] },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-sonnet-5","max_tokens":256,"messages":[{"role":"user","content":"ping"}]}'` },
      { type: "p", text: "Nothing about the protocol changes with your geography. What you get is the same Anthropic Messages API — streaming, tool use and system prompts included — across the full supported Claude line: Opus 4.8 and 4.7, Sonnet 5 and 4.6, Haiku 4.5." },
      { type: "list", items: [
        "Works in Claude Code, Cursor, Cline, Continue, Zed and the official Anthropic SDKs — set the base URL and the rest of your code stays untouched.",
        "Every request appears in the dashboard with its model, provider and token breakdown, so spend is auditable from anywhere.",
        "Refunds are processed through the original payment provider; contact support with your account email if you need one.",
      ] },
      { type: "note", text: "One honest caveat: network reachability depends on your own connection — buying balance and generating a key carry no geographic gate, but no service can route around a local network block. Support answers in English and Russian over Telegram, or by email at apitokensale@gmail.com." },
      { type: "link", text: "Browse every supported model with per-token pricing on the models page.", href: "/models" },
      { type: "link", text: "Estimate a workload before you top up with the Claude API cost calculator.", href: "/tools/claude-api-cost-calculator" },
    ] },
  ],
  faq: [
    { q: "Is the Claude API available in my country?", a: "With apiToken.sale, effectively yes. There is no billing-country requirement because we issue the key and balance ourselves, so you can buy balance and use the Claude API from regions Anthropic does not bill directly." },
    { q: "What can I do if Anthropic does not accept my card or my country?", a: "Pay apiToken.sale by bank card or with cryptocurrency such as USDT or BTC through a secure checkout provider. The prepaid balance never expires, and each request is billed at official Anthropic rates minus a flat 50% B2C discount." },
  ],
  related: ["claude-api-for-russia", "claude-api-crypto-payment", "how-to-buy-claude-api-key", "claude-api-without-waitlist"],
  updated: "2026-08-17",
};
