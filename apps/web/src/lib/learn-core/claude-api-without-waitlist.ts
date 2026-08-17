import type { LearnArticle } from "../learn";
import { BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-without-waitlist",
  cluster: "buy",
  title: "Claude API With No Waitlist or Approval",
  h1: "Claude API access with no waitlist",
  description: "Skip the Anthropic waitlist and approval. Create an account on apiToken.sale, generate a Claude API key, and make your first call in minutes.",
  keywords: ["claude api no waitlist", "claude api instant access", "claude api without approval", "get claude api key fast", "claude api no anthropic account", "buy claude api", "claude api access", "claude api tokens", "claude api top up", "claude api reseller", "claude api provider"],
  dek: "Looking for a Claude API with no waitlist usually means you have already lost time to signup friction somewhere else. apiToken.sale removes the queue entirely: create an account, generate a key, and your first Messages API call succeeds in the same sitting. No approval step, no sales call, no company verification.",
  sections: [
    { h2: "Where the direct path slows down", blocks: [
      { type: "p", text: "You can get a working Claude API key right now, without any waitlist or approval: sign up on apiToken.sale, generate a key in the dashboard, and it answers the very next request. The key talks to the same Anthropic Messages API with the same model IDs, so existing code and tools work unchanged. Nothing in this article asks you to wait for anything except your own typing." },
      { type: "p", text: "It is worth being precise about what the waitlist question actually is. Anthropic's own Console is self-serve in principle, but in principle hides real friction: account creation, phone verification, a supported payment method, and a credit purchase all stand between you and the first token. Rate limits are organized into usage tiers that rise with cumulative spend, so a brand-new account starts with the tightest limits no matter what you are prepared to pay. And if your card or region is not supported, the process simply stops at checkout." },
      { type: "p", text: "That is the gap a self-serve gateway fills. The approval you skip is not a feature gate on the models — it is the account provisioning, payment onboarding and tier warm-up around them." },
    ] },
    { h2: "What a self-serve key actually is", blocks: [
      { type: "p", text: `apiToken.sale issues its own key — it looks like ${KEY} — drawn against a prepaid balance you control. There is no Anthropic account to create, no invite to wait for, and no manual review between signup and your first successful request. The same single key works across supported Claude, GPT, Gemini and Kimi models, so you are not managing one credential per provider.` },
      { type: "table", headers: ["Client protocol", "Endpoint", "Auth header"], rows: [
        [`Anthropic Messages (Claude, Kimi)`, `${BASE}/v1/messages`, "x-api-key"],
        ["OpenAI-compatible (GPT and the universal lane)", OPENAI_BASE, "Authorization: Bearer"],
        ["Native Gemini", BASE, "x-goog-api-key"],
      ] },
      { type: "p", text: "For Claude you keep the exact Messages API shape: claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5 and the rest of the supported line, with streaming and tool use intact. Only the base URL and the key change — your request bodies, SDK versions and parsing code do not." },
    ] },
    { h2: "Make the first call before you pay", blocks: [
      { type: "steps", items: [
        "Create an account with Google or GitHub — this is what grants the $5 platform bonus credit, so do not reach for a card yet.",
        `Open the dashboard and generate a key (it looks like ${KEY}). It is live the moment it appears; there is no activation queue.`,
        "Send one request from your terminal and watch it show up in your usage metering.",
      ] },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-haiku-4-5",\n    "max_tokens": 256,\n    "messages": [{"role":"user","content":"Reply with one sentence."}]\n  }'` },
      { type: "p", text: "New accounts created with Google or GitHub start with $5 of platform bonus credit, which is enough to validate the whole flow — auth, model routing, metering — against real supported models before you top up. Email-and-password accounts work identically but do not receive the bonus, so pick the sign-in method deliberately." },
    ] },
    { h2: "Point your editor or agent at the router", blocks: [
      { type: "p", text: "Because nothing about the protocol changes, every Anthropic-compatible tool is a two-field configuration: base URL plus key. Claude Code reads both from the environment:" },
      { type: "code", code: `# Claude Code\nexport ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# then just run\nclaude` },
      { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : ${BASE}\nAPI key  : ${KEY}\nModel    : claude-sonnet-5` },
      { type: "p", text: "The same two fields cover Cline, Continue, Zed and Aider, and the official Anthropic SDKs accept them as base_url and api_key. If a tool works with the Anthropic API at all, it works here on the day you create the key — that is the practical meaning of no waitlist." },
    ] },
    { h2: "What “no waitlist” does not mean", blocks: [
      { type: "list", items: [
        "It does not mean free. Requests are metered against your balance from the first call; the $5 Google/GitHub bonus is a starting credit, not a tier.",
        "It does not mean anonymous. You still create an account, and the bonus depends on how you create it — Google or GitHub, not email and password.",
        "It does not mean instant crypto settlement. A card top-up credits in seconds; a crypto top-up credits after the network confirms the transaction.",
        "It does not mean enterprise procurement disappears. B2C access is fully self-serve, and the only conversation that still happens is negotiated B2B volume pricing.",
      ] },
      { type: "p", text: "None of these are queues. They are the ordinary rules of a prepaid service, stated plainly so the first surprise you get is how uneventful the setup is." },
    ] },
    { h2: "Paying once the bonus is spent", blocks: [
      { type: "p", text: "Top-ups are any whole-dollar amount, paid by bank card or cryptocurrency through a secure checkout provider. The balance is prepaid, never expires, and is spent only when API requests run — there is no subscription renewing in the background and no monthly minimum to justify." },
      { type: "p", text: "Every request is converted to official Anthropic API spend and then discounted: B2C accounts get a flat 50% off official spend on every request, automatically. Current per-model rates are listed on the model pages, so you can price a workload before you run it." },
      { type: "link", text: "Compare supported Claude models and their current rates", href: "/models" },
    ] },
  ],
  faq: [
    { q: "Is there really no waitlist for the Claude API?", a: "Correct. Access is self-serve and instant — you generate a key in the dashboard and it works on the very next request, with no manual review in between." },
    { q: "Do I need an Anthropic account to get a key?", a: "No. apiToken.sale issues its own key and prepaid balance, so there is no Anthropic account, invite or approval involved — but you still call the same Anthropic Messages API with the same model IDs." },
    { q: "Do I need to talk to sales?", a: "No. B2C access is fully self-serve. Only negotiated B2B volume pricing involves a conversation." },
    { q: "Can I test the API before paying anything?", a: "Yes. Accounts created with Google or GitHub start with $5 of platform bonus credit, enough for real calls against supported models. Email-and-password accounts do not receive the bonus." },
    { q: "Which Claude models are available immediately?", a: "The full supported line — Claude Opus 4.8 and 4.7, Sonnet 5 and 4.6, and Haiku 4.5 — all on the same key, metered per request." },
    { q: "Will my existing Anthropic SDK code work?", a: `Yes. Point the client at ${BASE} and use your apiToken.sale key; request bodies, streaming and tool use are unchanged.` },
  ],
  related: ["how-to-buy-claude-api-key", "claude-api-quick-setup", "claude-api-activation-time", "free-claude-api-key"],
  updated: "2026-08-17",
};
