import type { LearnArticle } from "../learn";
import { BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-for-russia",
  cluster: "buy",
  title: "Claude API from Russia and Restricted Regions",
  h1: "Using the Claude API from Russia",
  description: "Access the Claude API from Russia and other restricted regions with apiToken.sale — no Anthropic account, pay by card or crypto, one key for every Claude model.",
  keywords: ["claude api russia", "claude api из россии", "anthropic api russia", "claude api restricted regions", "claude api без vpn", "оплата claude api из россии", "buy claude api from russia", "claude api without foreign card", "claude api unsupported country", "claude api access", "claude api top up"],
  dek: "Every search for Claude API access from Russia ends at the same wall: Anthropic requires a supported billing country and a matching payment method, so sign-up stalls at checkout before you ever see a key. apiToken.sale routes around that wall — you top up prepaid balance by bank card or cryptocurrency and call the same Anthropic Messages API with your own key. No Anthropic account, no waitlist, no company verification.",
  sections: [
    { h2: "Why Anthropic sign-up stalls at a Russian card", blocks: [
      { type: "p", text: "Anthropic gates API keys behind billing: opening an account requires a supported billing country and a payment method issued there, and from Russia that check fails before a key is ever generated. The models themselves are ordinary HTTPS endpoints — the lock sits on payment and account creation, not on the API protocol. apiToken.sale removes exactly that lock by issuing the key and the balance itself, so the billing-country question never comes up." },
      { type: "p", text: "This is also why the usual workarounds disappoint. A VPN changes where your traffic appears to originate, but it does not produce a card from a supported billing country, and borrowed foreign cards tend to die at the first re-verification. The durable fix is to stop depending on Anthropic's checkout at all." },
    ] },
    { h2: "What apiToken.sale changes — and what it does not", blocks: [
      { type: "p", text: "The service acts as the billing layer Anthropic will not be in your region. You create a free account, top up balance, and generate a key that looks like sk-pool-… — no Anthropic account, no supported billing country, no waitlist, no company verification. Activation is instant: the key works on the very next request." },
      { type: "list", items: [
        "No Anthropic account or billing country required at any step.",
        "Bank card or cryptocurrency at checkout — your choice per top-up.",
        "Instant key activation, with no manual review.",
        "Support in Russian and English over Telegram.",
      ] },
      { type: "p", text: "One honest limitation. Buying balance and generating a key are not region-locked, but network reachability of the API endpoint depends on your own connection. If your network can reach the router, everything works end to end; if it cannot, that is a routing question on your side, not a billing one." },
    ] },
    { h2: "Prepaid balance instead of a foreign subscription", blocks: [
      { type: "p", text: "Top-ups are any whole-dollar amount — there is no fixed plan to pick and no monthly fee. The balance never expires and is spent only when requests actually run: each request is converted to official Anthropic API spend, then your discount is applied. B2C accounts get a flat 50% off official spend on every request, so you fund the balance once and draw it down as you build. Card and crypto feed the same balance, and you can switch between them per top-up — useful when one method is declined and the other is not." },
      { type: "table", headers: ["Top-up method", "When the balance credits", "When it makes sense"], rows: [
        ["Bank card", "At checkout", "The simplest path when the payment goes through"],
        ["Crypto (USDT, BTC and other major coins)", "After on-chain confirmation", "Cards are declined or you prefer not to use one"],
      ] },
    ] },
    { h2: "One key, every Claude model, one balance", blocks: [
      { type: "p", text: `The same ${KEY} key unlocks the full supported Claude line, and the same balance also covers supported GPT, Gemini and Kimi models — useful when one project mixes providers. The model IDs below are exactly what you put in the request's model field.` },
      { type: "table", headers: ["Model", "Model ID", "Reach for it when"], rows: [
        ["Claude Opus 4.8", "claude-opus-4-8", "The hardest reasoning and long agentic coding runs"],
        ["Claude Opus 4.7", "claude-opus-4-7", "Opus-class work pinned to the previous generation"],
        ["Claude Sonnet 5", "claude-sonnet-5", "Everyday coding and chat at mid-tier cost"],
        ["Claude Sonnet 4.6", "claude-sonnet-4-6", "Stable Sonnet behavior behind existing prompts"],
        ["Claude Haiku 4.5", "claude-haiku-4-5", "High-volume, latency-sensitive calls"],
      ] },
      { type: "link", text: "Per-model pricing, cache rates and context windows", href: "/models" },
    ] },
    { h2: "Pointing Claude Code, Cursor and the SDK at the router", blocks: [
      { type: "p", text: `Nothing about the protocol changes. The router speaks the Anthropic Messages API — POST /v1/messages with x-api-key and anthropic-version headers — so every Anthropic-compatible tool works by overriding the base URL. Claude Code needs two environment variables:` },
      { type: "code", code: `export ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# Claude Code now talks to the router\nclaude` },
      { type: "p", text: "Before wiring up an IDE, prove the whole path — billing, key and network — with one request:" },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-haiku-4-5",\n    "max_tokens": 64,\n    "messages": [{"role":"user","content":"Reply with: ok"}]\n  }'` },
      { type: "p", text: `Cursor and Cline take the same two values in their settings panels (Anthropic provider, then Base URL and API key), and the Python and TypeScript SDKs accept a base URL in the client constructor. Tools that only speak the OpenAI protocol can use the OpenAI-compatible lane at ${OPENAI_BASE} with Authorization: Bearer and the same key.` },
      { type: "p", text: "Streaming behaves exactly as upstream: pass \"stream\": true and the answer arrives as incremental server-sent events, so Claude Code and chat frontends render tokens as they are generated instead of waiting for the full response." },
      { type: "note", text: `If the curl call returns JSON, your network path is fine and any remaining setup is client-side. If it times out, test reachability of ${BASE} itself before touching any config.` },
    ] },
    { h2: "From sign-up to first request in one sitting", blocks: [
      { type: "steps", items: [
        "Create a free apiToken.sale account — sign up with Google or GitHub to start with $5 of platform bonus credit (email/password accounts do not receive the bonus).",
        "Top up any whole-dollar amount by bank card or cryptocurrency; a crypto top-up credits once the network confirms the transaction.",
        "Generate an API key in the dashboard — it looks like sk-pool-… and works across supported Claude, GPT, Gemini and Kimi models.",
        `Export ANTHROPIC_BASE_URL=${BASE} and ANTHROPIC_API_KEY with your key, or paste the same two values into Cursor or Cline.`,
        "Run the curl smoke test from the previous section; a JSON answer means billing, key and network path are all live.",
      ] },
      { type: "note", text: "Treat the sk-pool-… key like a password: anyone holding it can spend your balance. Keep it in environment variables or your tool's settings, never in source control." },
    ] },
  ],
  faq: [
    { q: "Can I pay for the Claude API from Russia without a foreign card?", a: "Yes. Checkout accepts a bank card or cryptocurrency through a payment provider, and no supported Anthropic billing country is required at any step." },
    { q: "Do I need a VPN to use the Claude API from Russia?", a: "Not for purchase or key generation — nothing there is region-locked. Network reachability of the router endpoint depends on your own connection, so test it with a single curl call before configuring your tools." },
    { q: "Is this the same Claude API that Anthropic sells?", a: "Yes — the same Anthropic Messages API, the same model IDs like claude-opus-4-8, and the same request and response format. What differs is how you sign up and pay: prepaid balance, with B2C accounts at a flat 50% off official API spend." },
    { q: "Does the prepaid balance expire?", a: "No. The balance never expires and is consumed only by real API usage — there is no monthly fee and no fixed plan." },
    { q: "Is support available in Russian?", a: "Yes — support answers in Russian and English over Telegram." },
  ],
  related: ["claude-api-crypto-payment", "claude-api-supported-countries", "how-to-buy-claude-api-key", "claude-api-without-waitlist"],
  updated: "2026-08-17",
};
