import type { LearnArticle } from "../learn";
import { cta, BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-free-trial",
  cluster: "free",
  title: "Claude API Free Trial — Start in Minutes",
  h1: "Try the Claude API free",
  description: "Start coding with Claude in minutes. New accounts created with Google or GitHub get $5 of platform bonus credit, with no card required.",
  keywords: ["claude api free trial", "try claude api", "claude api test", "claude api sandbox", "claude api demo", "free claude api", "claude api free", "claude api no credit card", "claude api free credits", "try claude api free", "claude api free tier"],
  dek: "There is no separate trial to apply for — a Claude API free trial here means $5 of platform bonus credit on a new Google or GitHub account, spent on real calls against every supported model. No card, no sandbox mode, nothing to cancel. This guide shows how to claim the credit, what to test first, and how far $5 actually goes.",
  sections: [
    { h2: "What the free trial actually is", blocks: [
      { type: "p", text: "A Claude API free trial on apiToken.sale is not a demo environment and not a limited sandbox. New accounts created with Google or GitHub start with $5 of platform bonus credit, and that credit buys real, metered calls against every supported model — the same endpoints, keys and streaming behavior paying customers get. No card is required, and there is no plan to cancel afterwards." },
      { type: "p", text: "One eligibility rule matters: the bonus is attached to the sign-up method, not to the account itself. Registering with an email address and password creates a fully working account, but it starts with a zero balance — so choose Google or GitHub at registration if you want the free start." },
      cta(),
    ] },
    { h2: "Claim the credit and generate a key in one sitting", blocks: [
      { type: "steps", items: [
        "Sign up with Google or GitHub. The $5 platform bonus lands on your balance automatically — this is exactly the step that email and password registration does not trigger.",
        "Open the dashboard and generate an API key. It looks like sk-pool-… and one key covers every supported Claude, GPT, Gemini and Kimi model, so there is no per-model setup to configure.",
        `Pick the protocol your tool already speaks. Anthropic-native clients call ${BASE} with an x-api-key header; OpenAI-compatible clients call ${OPENAI_BASE} with Authorization: Bearer. Both lanes draw from the same trial balance.`,
      ] },
      { type: "p", text: "From sign-up to a working key is a few minutes of clicking. There is no approval step, no sales call and no waitlist between you and the first request — the key is live the moment it is generated." },
    ] },
    { h2: "Prove the gateway with one real call", blocks: [
      { type: "p", text: "The fastest sanity check is a single non-streaming Messages request with a small token cap. The point is to confirm auth, the base URL and the model ID — not to generate prose." },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-haiku-4-5",\n    "max_tokens": 64,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
      { type: "p", text: "A 200 response with content blocks and a usage object means the whole path works: key, endpoint and metering. Every response reports the exact tokens it consumed and the dashboard shows the remaining balance, so you can watch what the trial costs in real time instead of guessing." },
      { type: "note", text: "A 402 response means the balance is empty, not that the key is broken. Top up any whole-dollar amount and retry the identical request — the key itself stays valid." },
    ] },
    { h2: "A trial checklist that fits in one sitting", blocks: [
      { type: "p", text: `Treat the $5 as a verification budget. The goal is not to build something during the trial; it is to prove that everything you plan to use behaves correctly through the gateway before real money is involved.` },
      { type: "table", headers: ["Checkpoint", "What a pass looks like"], rows: [
        ["First 200 OK", "JSON with content blocks and a usage object — key, base URL and model ID are all correct"],
        ["SSE streaming", "With stream: true, tokens arrive incrementally as server-sent events instead of one buffered response"],
        ["Tool use", "The model returns a tool_use block and continues correctly after your tool_result reply"],
        ["Your editor", "Cursor, VS Code, Continue, Aider or Claude Code completes a real request pointed at the gateway"],
        ["A second provider", "The same key answers on the OpenAI-compatible lane with a supported GPT or Gemini model"],
      ] },
    ] },
    { h2: "Stretch the $5 across more tests", blocks: [
      { type: "p", text: "Five dollars goes surprisingly far if you evaluate like an engineer instead of chatting like a user. Four habits decide whether the credit lasts an afternoon or a week:" },
      { type: "list", items: [
        "Iterate on claude-haiku-4-5. Run wiring tests, prompt drafts and error-path checks against the cheapest supported Claude model; switch to claude-sonnet-5 or claude-opus-4-8 only for final quality comparisons.",
        "Cap max_tokens aggressively. The Messages API requires the field anyway, and a low ceiling stops a rambling completion from eating the budget.",
        "Reuse long context with prompt caching. If your test loop re-sends the same large prompt, marking the stable prefix for caching makes repeat calls bill a fraction of the input tokens.",
        "Read the usage object on every response. input_tokens and output_tokens tell you the exact cost shape of your workload before you scale it to production traffic.",
      ] },
    ] },
    { h2: "The trial covers more than Claude", blocks: [
      { type: "p", text: "The bonus credit is platform-wide, not Claude-only. The same key and the same balance call supported GPT, Gemini and Kimi models, which makes the trial genuinely useful for cross-model evaluation: send one prompt to several models and compare the answers side by side on your own tasks." },
      { type: "code", code: `curl ${OPENAI_BASE}/chat/completions \\\n  -H "Authorization: Bearer ${KEY}" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "gpt-5.6-terra",\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
      { type: "p", text: "Gemini-native tools authenticate with an x-goog-api-key header against the same router host, and Kimi models answer on both the Anthropic Messages lane and the OpenAI-compatible one. One balance, four providers, zero extra sign-ups." },
    ] },
    { h2: "When the credit runs out", blocks: [
      { type: "p", text: "There is no trial expiry date and no plan to pick. When the balance gets low, top up any whole-dollar amount — your flat discount applies immediately — and keep calling with the same key. The prepaid balance never expires, there is no subscription and no monthly minimum, so after the trial you only ever pay for the tokens you actually use." },
      { type: "link", text: "Estimate your real monthly spend from the trial's usage numbers", href: "/tools/claude-api-cost-calculator" },
      { type: "link", text: "Compare every supported model and its price", href: "/models" },
      { type: "link", text: "Wire up curl, Python, Node and your IDE end to end", href: "/docs/learn/claude-api-quick-setup" },
    ] },
  ],
  faq: [
    { q: "Is the Claude API free trial a separate sandbox?", a: "No. The $5 Google/GitHub platform bonus runs against the same production endpoints and supported models as paid balance — there is no demo mode and no restricted feature set." },
    { q: "How do I start the Claude API free trial without a credit card?", a: "Create a new account with Google or GitHub. The $5 platform bonus is added automatically, no card is asked for, and email/password accounts are not eligible for the bonus." },
    { q: "Which models can I test during the trial?", a: "Every supported Claude model — claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5 and the rest — plus supported GPT, Gemini and Kimi models on the same key and balance." },
    { q: "What happens when the trial balance hits zero?", a: "Calls return 402 until you top up any whole-dollar amount; your flat discount applies immediately, the key stays valid, and the prepaid balance never expires." },
    { q: "Can I use the trial with Cursor or Claude Code?", a: "Yes. Point the tool's Anthropic base URL at https://router.apitoken.sale, paste the sk-pool-… key, and the requests bill against the trial credit like any other call." },
  ],
  related: ["free-claude-api-key", "claude-api-without-waitlist", "claude-api-quick-setup", "claude-haiku-api"],
  updated: "2026-08-17",
};
