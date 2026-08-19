import type { LearnArticle } from "../learn";
import { BASE, KEY, cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "how-billing-works",
  cluster: "explain",
  title: "How Billing Works on apiToken.sale",
  h1: "How billing works",
  description: "How apiToken.sale billing works: one prepaid balance for Claude, GPT, Gemini and Kimi, metered per request at official rates with a flat 50% discount.",
  keywords: [
    "multi provider api billing",
    "how apitoken billing works",
    "prepaid api balance",
    "claude api billing",
    "gpt api billing",
    "gemini api billing",
    "kimi api billing",
    "pay as you go llm api",
    "api credit never expires",
    "llm api usage tracking",
  ],
  dek: "apiToken.sale billing is prepaid: you top up a balance once and every Claude, GPT, Gemini and Kimi request draws it down. Each call is metered at the provider's official rate card, your flat 50% B2C discount is subtracted, and only the net amount touches your balance.",
  sections: [
    { h2: "One prepaid balance for Claude, GPT, Gemini and Kimi", blocks: [
      { type: "p", text: "Billing on apiToken.sale is prepaid and pay-as-you-go: you fund a balance in advance, and API requests draw it down — there is no monthly invoice and no customer subscription. The same balance covers every supported Claude, GPT, Gemini and Kimi model, so you never manage four separate provider accounts or four separate bills." },
      { type: "p", text: "You top up any whole-dollar amount. The balance never expires, which changes how you should fund it: there is no reason to preload a large sum \"just in case\", and no reason to hoard either. Add what the next few weeks of work need, and top up again when the dashboard says you are running low. Idle time costs nothing — a balance you do not spend this month is still there next quarter." },
    ] },
    { h2: "How a single request becomes a charge", blocks: [
      { type: "p", text: "Every call is priced from the usage the provider actually reports, not from a flat per-request fee. The router converts the response's usage fields into official provider spend, leg by leg:" },
      { type: "list", items: [
        "Input tokens — your prompt, system instructions and conversation history, at the model's official input rate.",
        "Output tokens — the model's reply, at its official output rate, which is usually higher than input.",
        "Cache legs — cache writes and cache reads are metered separately from regular input, and cache reads are much cheaper.",
        "Model-specific buckets — long-context surcharges or image legs where a model's rate card defines them.",
      ] },
      { type: "p", text: "Because metering follows each provider's own rate card, a Claude request is priced exactly like Anthropic would price it and a Gemini request exactly like Google would — before your discount enters the picture. Streaming responses are metered the same way as non-streaming ones; the final usage arrives with the stream, and the charge is identical." },
    ] },
    { h2: "Where the 50% discount enters the math", blocks: [
      { type: "p", text: "The discount is applied after official spend is computed, never before. Official spend comes first, then your flat 50% B2C discount is subtracted across every supported provider, and the net amount is deducted from your prepaid balance. A worked example on Claude Haiku 4.5 (official $1 per million input tokens, $5 per million output tokens):" },
      { type: "table", headers: ["Step", "Calculation", "Amount"], rows: [
        ["Input leg (3,000 tokens)", "3,000 × $1 / 1M", "$0.003"],
        ["Output leg (800 tokens)", "800 × $5 / 1M", "$0.004"],
        ["Official provider spend", "$0.003 + $0.004", "$0.007"],
        ["Flat 50% B2C discount", "$0.007 × 50%", "−$0.0035"],
        ["Deducted from your balance", "$0.007 − $0.0035", "$0.0035"],
      ] },
      { type: "p", text: "The same order of operations applies to GPT, Gemini and Kimi requests against their own official rate cards. Prompt caching compounds with the discount rather than replacing it: a cache read is already cheaper at the official rate, and the 50% comes off that cheaper figure too." },
    ] },
    { h2: "Funding the balance: cards, crypto and amounts", blocks: [
      { type: "p", text: "At checkout you can pay by bank card or with cryptocurrency through a secure payment provider; crypto top-ups credit once the network confirms the transaction. Either way the funds land as prepaid balance in your account and are spent only when requests run — you can switch between card and crypto per top-up." },
      cta(),
      { type: "p", text: "If the bonus or a top-up does not behave as expected, support is available in English and Russian via Telegram, or by email at apitokensale@gmail.com. Refund handling is processed through the original payment provider." },
    ] },
    { h2: "What happens when the balance hits zero", blocks: [
      { type: "p", text: "Requests start failing with HTTP 402 and an insufficient-balance error — your key is not revoked and your account is not closed, the calls simply stop going through until you top up. Any whole-dollar amount restores service immediately, so the fix is a checkout visit, not a support ticket." },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-haiku-4-5",\n    "max_tokens": 64,\n    "messages": [{"role":"user","content":"ping"}]\n  }'\n\n# With an empty balance the response is:\n# HTTP 402 — insufficient balance; top up any whole-dollar amount to resume.` },
      { type: "note", text: "For unattended workloads — agents, cron jobs, CI evals — check the dashboard balance before long runs. A 402 mid-queue is a failed job, and retries cannot fix an empty balance the way they fix a rate limit." },
    ] },
    { h2: "Auditing where the money went", blocks: [
      { type: "p", text: "Every request appears in your dashboard with its model and provider and a token-level breakdown, so the balance is auditable line by line rather than a black box that shrinks. When one integration suddenly starts costing more, the per-request ledger tells you which model and which token leg changed — usually a longer system prompt on the input side or a chattier output." },
      { type: "p", text: "To plan a top-up rather than reconcile one, estimate the workload first: per-model official and discounted rates are listed on the model pages, and the cost calculator turns a prompt-plus-reply token estimate into a dollar figure before you spend anything." },
      { type: "link", text: "Per-model rates, cache pricing and context windows", href: "/models" },
      { type: "link", text: "Estimate a workload with the Claude API cost calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
  ],
  faq: [
    { q: "Is apiToken.sale billing prepaid or postpaid?", a: "Prepaid. You fund a balance in advance in any whole-dollar amount, and requests draw it down — there is no monthly invoice and no subscription." },
    { q: "Does one balance cover Claude, GPT, Gemini and Kimi?", a: "Yes. Each provider is metered against its own official rate card, the same flat 50% B2C discount applies, and the net charge draws from one shared prepaid balance." },
    { q: "How is the 50% discount calculated per request?", a: "The request's usage legs (input, output, cache and any model-specific buckets) are converted to official provider spend first; then 50% is subtracted, and only the remainder is deducted from your balance." },
    { q: "What happens when my apiToken.sale balance runs out?", a: "Requests return HTTP 402 with an insufficient-balance error. Your key stays valid — topping up any whole-dollar amount restores service immediately." },
    { q: "Does unused balance expire, and can I get a refund?", a: "Balance never expires; unused funds simply remain available. Refunds are handled through the original payment provider — contact support via Telegram or at apitokensale@gmail.com." },
    { q: "Can I see token-level usage for each request?", a: "Yes. The dashboard lists every request with its model, provider and token breakdown, so you can audit exactly where the balance went." },
  ],
  related: ["claude-api-pricing-explained", "gpt-api-pricing", "gemini-api-pricing", "kimi-api-pricing"],
  updated: "2026-08-17",
};
