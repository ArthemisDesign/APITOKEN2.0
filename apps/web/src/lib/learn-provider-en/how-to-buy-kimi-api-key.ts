import type { LearnArticle } from "../learn";
import { OPENAI, ROUTER } from "./shared";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
    slug: "how-to-buy-kimi-api-key",
    cluster: "buy",
    title: "How to Buy a Kimi API Key",
    h1: "How to buy a Kimi API key",
    description: "How to buy a Kimi API key: one prepaid key covers K3 and Kimi for Coding at 50% of official rates — pay by card or crypto, get a $5 welcome bonus.",
    keywords: ["buy kimi api key", "kimi api key", "kimi k3 api", "kimi for coding api key", "moonshot kimi api", "kimi api prepaid", "kimi api pay as you go", "kimi api without moonshot account", "kimi api anthropic compatible", "cheap kimi api access"],
    dek: "Buying a Kimi API key here means one prepaid key that unlocks the whole kimi/* namespace — K3 and Kimi for Coding — at half the official token rates. You sign up, top up a whole-dollar amount by card or crypto, and call either the Anthropic Messages lane or the OpenAI-compatible one. This guide walks through the purchase, the first paid request, and the billing rules worth knowing before money moves.",
    sections: [
      { h2: "What buying a Kimi API key actually gets you", blocks: [
        { type: "p", text: "You do not buy a Kimi-specific key, and you do not need a Moonshot account. One apiToken.sale key — it looks like sk-pool-… — covers the Kimi namespace alongside supported Claude, GPT and Gemini models, and every request settles against the same prepaid balance at 50% of official provider rates." },
        { type: "p", text: "The purchase itself takes minutes: create an account, generate the key in the dashboard, and top up any whole-dollar amount by bank card or cryptocurrency. There is no separate Kimi plan, no subscription and no monthly minimum — the balance is prepaid, never expires, and is drawn down only by real usage." },
      ] },
      { h2: "From sign-up to a live key in one sitting", blocks: [
        { type: "steps", items: [
          "Create an apiToken.sale account. Sign up with Google or GitHub if you want the $5 platform bonus credit — email/password accounts work but start with a zero balance.",
          "Open the dashboard and generate an API key. It is live immediately; there is no approval step or waitlist.",
          "Top up any whole-dollar amount by card or crypto. Each top-up is independent, so you can fund a small amount first and add more later.",
          "Read GET " + ROUTER + "/v1/models with your key and pick a kimi/* ID from the catalog it returns. The response is scoped to your key, so it only lists models that are currently routable and priced for you.",
        ] },
        cta(),
      ] },
      { h2: "Prove the purchase with one paid request", blocks: [
        { type: "p", text: "Kimi speaks the Anthropic Messages protocol natively on the router, so the cheapest sanity check is a single non-streaming call with a small token cap. It verifies auth, the model alias and metering in one round trip." },
        { type: "code", code: "curl " + ROUTER + "/v1/messages \\\n  -H \"x-api-key: $APITOKEN_API_KEY\" \\\n  -H \"anthropic-version: 2023-06-01\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"model\":\"kimi/kimi-for-coding\",\"max_tokens\":256,\"messages\":[{\"role\":\"user\",\"content\":\"Reply with exactly: connected\"}]}'" },
        { type: "p", text: "A 200 response returns content blocks plus an Anthropic-shaped usage object, so existing usage parsers keep working. The dashboard shows the same consumption with a token-level breakdown, which lets you watch exactly what a request cost against your balance." },
        { type: "note", text: "A 402 response means the balance is empty, not that the key or model alias is broken. Top up and retry the identical request — the key stays valid." },
      ] },
      { h2: "Which Kimi alias to spend on", blocks: [
        { type: "p", text: "Public Kimi IDs on the router are subscription aliases, not official Open Platform IDs. Kimi publishes separate cache-hit, cache-miss and output rates, and apiToken.sale charges exactly half of each leg. Figures below are per 1M tokens." },
        { type: "table", headers: ["Public alias", "Official hit / miss / output", "You pay after 50%"], rows: [
          ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
        ] },
        { type: "list", items: [
          "K3 exposes 256K and 1M context spellings; pick k3-256k when the task does not need the full window.",
          "Kimi for Coding is the low-cost coding default; the high-speed alias costs exactly double the base token rates, so reserve it for latency-sensitive work.",
          "Reasoning tokens bill at the output rate as part of output — they are not a separate charge.",
          "Never substitute an official ID such as kimi-k2.7-code. The router accepts the aliases shown by GET /v1/models, and that response is authoritative because availability can shift with provider capacity and account policy.",
        ] },
        { type: "link", text: "Full Kimi rate anatomy: cache legs, aliases and spend control", href: "/docs/learn/kimi-api-pricing" },
      ] },
      { h2: "Two wire formats, one key and balance", blocks: [
        { type: "p", text: "Kimi is a provider namespace on the router, not a fourth protocol. Anthropic-native tools — the Anthropic SDK, Claude Code, Kimi Code — call POST /v1/messages with the x-api-key header. OpenAI-compatible clients reach the same kimi/* aliases through the universal /v1 lane with a Bearer token." },
        { type: "code", code: "curl " + OPENAI + "/chat/completions \\\n  -H \"Authorization: Bearer $APITOKEN_API_KEY\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"model\":\"kimi/kimi-for-coding\",\"messages\":[{\"role\":\"user\",\"content\":\"Reply with exactly: connected\"}]}'" },
        { type: "note", text: "The Messages route accepts stream: true, but provider-boundary chunk incrementality is still under live validation. Use non-streaming calls when exact chunk timing matters, and pin streaming behavior in your own integration tests before relying on it." },
        { type: "link", text: "Run Kimi inside Claude Code with every model tier pinned", href: "/docs/learn/kimi-api-for-claude-code" },
      ] },
      { h2: "Payment, refunds and balance rules to know first", blocks: [
        { type: "list", items: [
          "Top-ups accept any whole-dollar amount, paid by bank card or cryptocurrency — you can switch method per top-up.",
          "Prepaid balance never expires and is consumed only by real API usage across every supported provider.",
          "Free credit, including the $5 welcome bonus, is always spent before paid balance, so early testing does not touch your top-up.",
          "A top-up is refundable within 5 calendar days only while its balance is completely unused; once any part is spent, that top-up is final. Refunds return through the original payment provider, and promotional credit is never refundable.",
          "Support answers in English and Russian on Telegram or at apitokensale@gmail.com — include your account email and order identifier for billing questions.",
        ] },
        { type: "p", text: "The practical strategy on a prepaid platform is to fund small and often. A top-up you end up not needing is not lost — it sits on the balance indefinitely — but keeping commitments small makes the refund window meaningful if you change your mind early." },
      ] },
      { h2: "Keep spend bounded after the purchase", blocks: [
        { type: "p", text: "Set a lifetime spending limit on the key so a runaway loop cannot drain the balance, and give the key an expiration date if it is issued for a fixed project. Both controls live in the dashboard next to the key itself." },
        { type: "p", text: "From there, the fastest next reads are the quickstart for SDK wiring and the model catalog for live prices. The same key you just bought also calls supported Claude, GPT and Gemini models, so cross-model comparisons cost you nothing extra in setup." },
        { type: "link", text: "Kimi API quickstart: curl and the Anthropic Python SDK end to end", href: "/docs/learn/kimi-api-quickstart" },
        { type: "link", text: "Compare every supported model and its live price", href: "/models" },
      ] },
    ],
    faq: [
      { q: "Do I need a Moonshot account to buy a Kimi API key?", a: "No. The account, key, balance and billing all come from apiToken.sale; no separate Kimi plan or Moonshot registration is required on your side." },
      { q: "How much does the Kimi API cost here?", a: "Half of official rates. Kimi for Coding runs $0.095 / $0.475 / $2 per 1M cache-hit, cache-miss and output tokens, K3 runs $0.15 / $1.50 / $7.50, and the high-speed alias costs exactly double the base Kimi for Coding rates." },
      { q: "Which endpoint and header does Kimi use?", a: "Anthropic Messages at " + ROUTER + "/v1/messages with x-api-key, or the universal OpenAI-compatible /v1 lane with Authorization: Bearer. Both accept the same kimi/* aliases and draw from the same balance." },
      { q: "Can I pay for a Kimi API key with crypto?", a: "Yes. Top-ups accept any whole-dollar amount by bank card or cryptocurrency, and you can switch payment method on each top-up." },
      { q: "Is there a free way to test Kimi before paying?", a: "Yes. New accounts created with Google or GitHub start with $5 of platform bonus credit, which is spent before any paid balance and works on Kimi like any other supported model." },
      { q: "What happens if my balance hits zero mid-work?", a: "Requests return 402 until you top up. The key stays valid, the balance never expires, and a top-up of any whole-dollar amount resumes service immediately." },
    ],
    related: ["kimi-api-quickstart", "kimi-api-for-claude-code", "kimi-api-for-kimi-code", "kimi-api-pricing"],
    published: "2026-08-09",
    updated: "2026-08-17",
  };
