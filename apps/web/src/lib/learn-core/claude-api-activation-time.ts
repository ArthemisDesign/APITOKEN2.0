import type { LearnArticle } from "../learn";
import { BASE, KEY, cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-activation-time",
  cluster: "explain",
  title: "Claude API Activation Time: Seconds, Not Days",
  h1: "How fast your Claude API key activates",
  description: "Claude API activation time on apiToken.sale is zero — the key works on the next request. Card top-ups clear in seconds; no waitlist, no review.",
  keywords: ["claude api activation time", "how fast claude api key", "instant claude api key", "claude api ready to use", "claude api key not working yet", "how long until claude api key works", "claude api first request", "claude api no waitlist", "claude api card payment", "claude api crypto top up time"],
  dek: "Claude API activation time on apiToken.sale is effectively zero: a generated key works on the very next request, with no manual review and no waitlist. The only real clock is payment confirmation — seconds for a card, one network confirmation for crypto. Below is the honest timeline for each stage, plus a one-minute way to prove a fresh key works.",
  sections: [
    { h2: "The short answer: seconds, not days", blocks: [
      { type: "p", text: "Your apiToken.sale Claude API key is active the moment you generate it. There is no approval queue, no manual review, and no provisioning delay between the dashboard and the router. If you already have balance — for example the $5 signup bonus — your first API call succeeds immediately; with a card top-up in between, the realistic end-to-end time from landing on the site to a finished response is a few minutes, and most of that is you typing card details." },
      { type: "table", headers: ["Stage", "Typical time", "What actually happens"], rows: [
        ["Create an account", "Under a minute", "Google, GitHub, or email and password — no application form"],
        ["Generate a key", "Instant", "The key (it looks like sk-pool-…) is accepted by the router as soon as it appears in the dashboard"],
        ["Card top-up", "Seconds", "Balance credits as soon as the card payment confirms"],
        ["Crypto top-up", "Minutes to an hour", "Balance credits once the network confirms the transaction; speed depends on the coin and the fee you choose"],
        ["First API request", "Instant", "Any Anthropic-compatible client works on the next call"],
      ] },
    ] },
    { h2: "What activation means when there is nothing to provision", blocks: [
      { type: "p", text: "Activation feels instant because there is nothing to wait for. The key is not an account being opened at a provider somewhere — it is a credential the router recognizes, and it is written into that router at generation time. No batch job, no reviewer, no business-hours gate." },
      { type: "p", text: "This is different from signing up with a provider directly, where billing verification, supported-country checks, or an approval step can sit between you and a working key. apiToken.sale issues its own keys and its own prepaid balance, so no third-party approval sits in the loop. It is also account-wide: the same key already covers supported Claude, GPT, Gemini and Kimi models, so there is no per-model activation either." },
    ] },
    { h2: "The one clock you do not control: payment confirmation", blocks: [
      { type: "p", text: "Key generation is instant, so the only stage that can add a real wait is funding. Card top-ups clear in seconds — by the time you switch back to the dashboard, the balance is there. A crypto top-up credits after the network confirms the transaction, and that timing is not ours: it depends on the coin, current network congestion, and the fee you set." },
      { type: "note", text: "The classic 'my key doesn't work' report is a low-fee crypto transfer still waiting for confirmation. During that window the key is already fully active — calls simply fail with insufficient balance until the top-up credits. Paying by card, or choosing a reasonable network fee, removes the wait entirely." },
      { type: "list", items: [
        "Want to start right now? Pay by card — seconds to credit.",
        "Paying in crypto? Do not set the minimum fee on a congested network.",
        "Do not want to pay at all yet? Accounts created with Google or GitHub start with $5 of platform bonus credit, so you can validate the whole flow first.",
      ] },
    ] },
    { h2: "A timed walkthrough: signup to finished response", blocks: [
      { type: "p", text: "This is the whole activation procedure. Time yourself — the slow part will be reaching for your wallet." },
      { type: "steps", items: [
        "Create the account (use Google or GitHub if you want the $5 bonus credit) and open the dashboard.",
        "Generate a key. The string appears immediately and is already valid — copy it now.",
        `Point any Anthropic-compatible tool at ${BASE} and send your key in the x-api-key header with anthropic-version: 2023-06-01.`,
        "Send one small request and watch it show up in the dashboard usage feed. If the response came back, activation is done — because it was done before you started.",
      ] },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-haiku-4-5",\n    "max_tokens": 64,\n    "messages": [{"role":"user","content":"Reply with: active"}]\n  }'` },
      { type: "p", text: "Use Haiku for this smoke test: it is the cheapest supported Claude model, so the proof call costs a fraction of a cent against your balance or bonus credit." },
      cta(),
    ] },
    { h2: "If a fresh key does not answer right away", blocks: [
      { type: "p", text: "Because activation is instant, a failing first call is never 'still activating' — it is configuration or balance. The error code tells you which:" },
      { type: "list", items: [
        `401 Unauthorized — the key is missing or mistyped in x-api-key, or the base URL is not ${BASE}. Note the Anthropic lane uses x-api-key, not Authorization: Bearer.`,
        "402 / insufficient balance — the key works fine; the balance is empty or a crypto top-up has not confirmed yet.",
        "400 Bad Request — a misspelled model ID or a missing max_tokens field. Nothing to do with activation.",
        "429 Too Many Requests — you are rate limited; respect the Retry-After header and lower concurrency.",
      ] },
      { type: "p", text: "If the dashboard shows your payment as confirmed but the balance has not moved, contact support — it is available in English and Russian over Telegram." },
    ] },
    { h2: "Activate once, then stop thinking about it", blocks: [
      { type: "p", text: "There is no renewal that could lapse and deactivate your access: the balance is prepaid, never expires, and there is no customer subscription. Your key keeps working until you delete it yourself — or until guardrails you set deliberately, such as a lifetime spending limit or an expiration date, say otherwise. Top up any whole-dollar amount whenever you need more; the credit lands as fast as the timelines above, every time." },
      { type: "link", text: "Every model your key unlocks, with per-model pricing", href: "/models" },
    ] },
  ],
  faq: [
    { q: "How long does Claude API activation take on apiToken.sale?", a: "Key generation is instant — the key works on the next request. Add seconds for a card top-up or one network confirmation for a crypto top-up, and a realistic first-call time is a few minutes end to end." },
    { q: "Is there a waitlist or manual review before my key works?", a: "No. Access is self-serve: there is no approval step between creating the account, generating the key, and making a successful call." },
    { q: "Why does my new key return insufficient balance right after I paid?", a: "Almost always a crypto top-up still waiting for network confirmation — credit lands after the network confirms, and timing depends on the coin and fee. Card payments confirm in seconds." },
    { q: "Can I use the key before topping up?", a: "Yes, if you created the account with Google or GitHub: those accounts start with $5 of platform bonus credit, which is spendable immediately on supported Claude, GPT, Gemini and Kimi models." },
    { q: "Does my key or balance expire if I do not use it?", a: "The prepaid balance never expires and there is no subscription to lapse. The key works until you delete it or it hits a limit you set yourself, such as an expiration date." },
  ],
  related: ["claude-api-without-waitlist", "how-to-buy-claude-api-key", "how-billing-works", "claude-api-quick-setup"],
  updated: "2026-08-17",
};
