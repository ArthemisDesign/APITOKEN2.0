import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-sonnet-api",
  cluster: "free",
  title: "Claude Sonnet API Access",
  h1: "Claude Sonnet API access: Sonnet 5 and Sonnet 4.6",
  description: "Claude Sonnet API access via apiToken.sale: Sonnet 5 and 4.6 model IDs, Messages API examples and prompt caching at a flat 50% off official rates.",
  keywords: ["claude sonnet api", "claude sonnet 5 api", "claude-sonnet-5", "claude sonnet api pricing", "claude sonnet 4.6 api", "sonnet api key", "claude sonnet api example", "claude messages api streaming", "claude sonnet prompt caching", "best claude model for coding", "claude api free credits", "try claude api free"],
  dek: "The Claude Sonnet API is the default tier for daily coding and agent work — fast enough for interactive edits, strong enough for real tool-use loops. This guide covers the live model IDs, a working Messages API call, streaming, prompt caching and what Sonnet costs on apiToken.sale at a flat 50% off official pricing.",
  sections: [
    { h2: "Claude Sonnet API: models, IDs and limits", blocks: [
      { type: "p", text: "The Claude Sonnet API is Anthropic's balanced model tier, served over the standard Messages API: you POST a model ID and a list of messages to /v1/messages and get back text, tool calls and token usage. On apiToken.sale you reach it at the same protocol shape — point any Anthropic-compatible client at the router base URL, authenticate with x-api-key, and nothing else in your code changes. Two Sonnet generations are live on one prepaid balance: claude-sonnet-5 and claude-sonnet-4-6." },
      { type: "table", headers: ["Model ID", "Context", "Max output", "Official in / out ($ per 1M)", "Here (−50%)"], rows: [
        ["claude-sonnet-5", "1M tokens", "128K tokens", "$3 / $15", "$1.50 / $7.50"],
        ["claude-sonnet-4-6", "1M tokens", "128K tokens", "$3 / $15", "$1.50 / $7.50"],
      ] },
    ] },
    { h2: "Make your first Sonnet call", blocks: [
      { type: "p", text: "If you have ever called Anthropic's Messages API, this is the same request with a different base URL and key. One apiToken.sale key covers every supported Claude, GPT, Gemini and Kimi model, so the call below is also the template for everything else on the platform." },
      { type: "steps", items: [
        "Create an account and generate a key from the dashboard — it looks like sk-pool-… and works the moment it is issued.",
        `Send POST /v1/messages to ${BASE} with the x-api-key and anthropic-version headers, exactly as you would against Anthropic.`,
        "Set the model field to claude-sonnet-5 (or claude-sonnet-4-6) and read the usage object in the response — spend is drawn from your prepaid balance at the official rate minus 50%.",
      ] },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "messages": [\n      {"role": "user", "content": "Refactor this function for readability."}\n    ]\n  }'` },
    ] },
    { h2: "Sonnet 5 or Sonnet 4.6: which ID to send", blocks: [
      { type: "p", text: "Both IDs share a list price, so the choice is behavioral, not budgetary. Sonnet 5 is the stronger coding and agentic model and the right default for anything new; it ships with introductory official rates, and the engine always applies the current effective rate before your discount. It also runs adaptive thinking by default when you omit the thinking parameter, so reasoning depth scales with the task instead of a fixed budget. Sonnet 4.6 supports the same adaptive thinking with effort defaulting to high, and remains the right pick when your prompts, evals and regression baselines are pinned to it." },
      { type: "list", items: [
        "New project or no strong preference: claude-sonnet-5.",
        "Prompts and eval suites tuned against 4.6 behavior: stay on claude-sonnet-4-6 until you re-baseline.",
        "Same context window, same output ceiling, same price — migrating later is a one-line model-ID change.",
      ] },
      { type: "link", text: "Claude Sonnet 4.6 on the model catalog", href: "/models/claude-sonnet-4-6" },
    ] },
    { h2: "Token pricing, including the introductory rate", blocks: [
      { type: "p", text: "The standard official rate for both Sonnet generations is $3 per 1M input tokens and $15 per 1M output tokens; here that is $1.50 / $7.50 after the flat 50% discount. Anthropic lists introductory pricing of $2 / $10 for Sonnet 5 through 2026-08-31, and the engine always applies the current effective rate before your discount — so while the introductory window is open, your spend tracks the lower official rate automatically. Output tokens cost five times input tokens, which is why response length discipline matters more than prompt trimming on chatty workloads." },
      { type: "note", text: "When the introductory window closes, the effective official rate moves to the standard $3 / $15 and your discounted spend moves with it — no key, plan or code change is needed on your side." },
      { type: "link", text: "Claude Sonnet 5 pricing in detail (cache rates, context, FAQ)", href: "/models/claude-sonnet-5" },
    ] },
    { h2: "Cut repeat-context cost with prompt caching", blocks: [
      { type: "p", text: "Agent loops re-send the same prefix on every turn: system prompt, tool definitions, repo context. The Messages API lets you mark that prefix with a cache_control breakpoint; Anthropic then holds it in a short-lived cache (five-minute TTL, refreshed on each hit) and bills subsequent reads of it at a fraction of the input price. On Sonnet workloads this is the single biggest cost lever, and it stacks with the 50% discount." },
      { type: "table", headers: ["Cache operation ($ per 1M)", "Official", "Here (−50%)"], rows: [
        ["5-minute cache write", "$3.75", "$1.875"],
        ["Cache read", "$0.30", "$0.15"],
      ] },
      { type: "p", text: "Put the breakpoint after the last stable block — system prompt plus tools plus retrieved context — and keep volatile per-turn content after it. A breakpoint on text that changes every call never hits and only costs you the write premium." },
      { type: "link", text: "Estimate a cached workload on the cost calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "Streaming responses and long outputs", blocks: [
      { type: "p", text: "Set stream: true and the API returns server-sent events instead of one blocking response: message_start, a sequence of content_block_delta events carrying the text as it is generated, then message_delta and message_stop. Render the deltas incrementally in your UI and take the final token usage from the terminal events — that is the record your balance is billed on. Streaming changes latency perception, not price: the same tokens are metered either way." },
      { type: "p", text: "The 128K output ceiling means a full file rewrite or a long structured extraction fits in one response. Use the headroom deliberately — a habit of unconstrained max_tokens plus verbose outputs is how a cheap Sonnet workload quietly becomes an expensive one." },
      { type: "note", text: "If a stream drops mid-generation, do not retry in a tight loop: issue one fresh request and reconcile spend from the terminal usage you did receive." },
    ] },
    { h2: "One balance across Sonnet, Opus and Haiku", blocks: [
      { type: "p", text: "Sonnet shares its key and prepaid balance with the rest of the catalog, which makes model routing trivial: send bulk classification and extraction to Haiku, keep Sonnet as the default for coding and agents, and escalate only genuinely hard reasoning to Opus. Switching tiers is a model-ID change on the same request shape — no new credentials, no separate billing relationship, no waitlist between you and any tier." },
      cta(),
    ] },
  ],
  faq: [
    { q: "What is the model ID for Claude Sonnet 5 in the API?", a: "claude-sonnet-5. Pass it as the model field of a Messages API request; the previous generation is claude-sonnet-4-6. Both work on the same apiToken.sale key." },
    { q: "How much does the Claude Sonnet API cost per token?", a: "The standard official rate is $3 per 1M input and $15 per 1M output tokens, with an introductory $2 / $10 listed through 2026-08-31. apiToken.sale applies a flat 50% discount to the effective official rate, so standard-rate spend lands at $1.50 / $7.50." },
    { q: "Is Sonnet good enough for coding agents, or do I need Opus?", a: "Sonnet 5 is the recommended default for everyday coding and agent workflows — near-Opus quality at a much lower token price. Reserve Opus for the hardest reasoning and long, high-stakes sessions." },
    { q: "Can I use the Claude Sonnet API from Cursor, Claude Code or the Anthropic SDK?", a: "Yes. Any Anthropic-compatible client works: set the base URL to the apiToken.sale router, authenticate with x-api-key, and keep the rest of your configuration unchanged." },
    { q: "Does Sonnet support prompt caching and a 1M-token context?", a: "Both Sonnet 5 and Sonnet 4.6 offer a 1M-token context window, 128K max output and prompt caching — cache reads bill at $0.30 per 1M tokens officially, $0.15 after the discount." },
    { q: "How can I try the Claude Sonnet API for free?", a: "Sign up with Google or GitHub and the account starts with $5 of platform bonus credit, usable on Sonnet and every other supported Claude, GPT, Gemini and Kimi model. Email/password accounts do not receive the bonus." },
  ],
  related: ["claude-opus-vs-sonnet", "claude-opus-api", "claude-haiku-api", "claude-api-key-for-cursor"],
  updated: "2026-08-17",
};
