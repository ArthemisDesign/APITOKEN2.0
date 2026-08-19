import type { LearnArticle } from "../learn";
import { BASE, OPENAI_BASE, KEY, cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-opus-api",
  cluster: "free",
  title: "Claude Opus API Access",
  h1: "Claude Opus 4.8 through the API",
  description: "Claude Opus API access on one apiToken.sale key: Opus 4.8 and 4.7 at a flat 50% off — $2.50/$12.50 per 1M tokens. Prepaid, no Anthropic account.",
  keywords: ["claude opus api", "claude opus 4.8 api", "claude opus api key", "claude opus api pricing", "claude opus api cost", "claude opus discount", "opus api without anthropic account", "claude opus prompt caching", "claude api free credits", "try claude api free"],
  dek: "The Claude Opus API is Anthropic's top tier for hard reasoning, multi-file refactors and long agent sessions. On apiToken.sale, Opus 4.8 and Opus 4.7 run on the same prepaid key and balance as every other supported model, metered at official rates and then discounted 50%. This guide covers the real prices, a working request, and how to keep long Opus runs affordable.",
  sections: [
    { h2: "One key for both current Opus models", blocks: [
      { type: "p", text: `You reach the Claude Opus API through the standard Anthropic Messages API: set the base URL to ${BASE}, authenticate with the x-api-key header, and pass claude-opus-4-8 as the model. apiToken.sale serves Opus 4.8 and Opus 4.7 on one prepaid key at a flat 50% off official token rates — no Anthropic account, no billing-country gate, no waitlist.` },
      { type: "p", text: "Opus is the tier you rent for work where a wrong answer costs more than tokens: architecture decisions, complex refactors, long autonomous agent runs. Routine tasks belong on a cheaper Claude, and section four of this guide covers that split." },
    ] },
    { h2: "What Opus costs per token here", blocks: [
      { type: "p", text: "Every request is metered against Anthropic's official rate card by its exact usage legs — input, output and cache — and the flat 50% B2C discount is subtracted before the charge hits your prepaid balance. Nothing is rounded up or bundled." },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (\u221250%)"], rows: [
        ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
      ] },
      { type: "p", text: "Prompt caching is metered as its own legs on the same rate card, and the discount applies to those as well:" },
      { type: "table", headers: ["Cache leg (Opus)", "Official ($ per 1M)", "Here (\u221250%)"], rows: [
        ["Cache write (5-minute TTL)", "$6.25", "$3.125"],
        ["Cache read", "$0.50", "$0.25"],
      ] },
      { type: "note", text: "Opus 4.8 keeps standard pricing across its full 1M-token context window — there is no long-context premium — and returns up to 128K output tokens per response. Adaptive thinking is the recommended mode, and thinking tokens bill as output." },
      { type: "link", text: "Claude Opus 4.8 pricing in detail (cache rates, context, FAQ)", href: "/models/claude-opus-4-8" },
    ] },
    { h2: "Your first Opus request in two minutes", blocks: [
      { type: "p", text: "The wire format is Anthropic's own. If your code already talks to api.anthropic.com, you change exactly two things: the base URL and the key." },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role": "user", "content": "Review this diff for regressions"}]\n  }'` },
      { type: "p", text: "Claude Code and other Anthropic-native tools pick the same settings up from the environment:" },
      { type: "code", code: `export ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}` },
      { type: "p", text: `Tools that only speak OpenAI chat completions can use the OpenAI-compatible lane at ${OPENAI_BASE} instead — Authorization: Bearer ${KEY}, same model ID. Either way the request draws from the same prepaid balance at the same discounted rate.` },
    ] },
    { h2: "When Opus is worth it — and when it is not", blocks: [
      { type: "list", items: [
        "Complex refactors and multi-file changes where one mistake cascades through the codebase.",
        "Architecture, planning and high-stakes review work.",
        "Long agent sessions where consistency and prompt-cache reuse matter.",
        "An orchestrator or advisor pass that reviews and steers output from cheaper models.",
      ] },
      { type: "p", text: "For everyday coding, Sonnet 5 delivers near-Opus quality at 40% of the token price, and Haiku 4.5 covers high-volume, latency-sensitive work at one fifth of the Opus input rate. Because one key and one balance cover every tier, routing is a per-request decision — you change the model ID, not the provider." },
    ] },
    { h2: "Keeping long Opus sessions affordable", blocks: [
      { type: "list", items: [
        "Cache the stable prefix. System prompts, tool definitions and repo context belong behind a cache breakpoint: a cache read is officially $0.50 per 1M tokens on Opus instead of $5 for fresh input, and your discount halves that again.",
        "Only cache what repeats. A cache write costs more than plain input ($6.25 vs $5 per 1M officially), so a breakpoint pays off only when the same prefix is sent at least twice.",
        "Cap max_tokens to what the task actually needs, and summarize long threads instead of resending full history.",
        "Push subtasks down a tier: let Haiku or Sonnet handle search, extraction and drafts, and reserve Opus for the genuinely hard steps.",
      ] },
      { type: "p", text: "These tactics compound with the discount: caching and routing lower the token count, the 50% rate lowers the price per token, and every leg is visible in the dashboard usage breakdown." },
      { type: "link", text: "Estimate an Opus workload before you run it", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "Free start, prepaid after", blocks: [
      { type: "steps", items: [
        "Create an account with Google or GitHub — new accounts on those providers start with $5 of platform bonus credit, enough for genuine Opus calls. Email/password accounts do not receive the bonus.",
        "Open the dashboard and generate a key (it looks like sk-pool-…). It activates instantly and works across supported Claude, GPT, Gemini and Kimi models.",
        `Point your tool at ${BASE} with that key and run a real Opus request against the bonus balance.`,
        "When the bonus runs low, top up any whole-dollar amount by bank card or crypto (USDT, BTC and other major coins) through the secure checkout provider. The balance never expires and there is no subscription.",
      ] },
      cta(),
      { type: "p", text: "If anything is unclear mid-setup, support answers in English and Russian via Telegram, or by email at apitokensale@gmail.com." },
    ] },
  ],
  faq: [
    { q: "How do I get a Claude Opus API key without an Anthropic account?", a: "Register on apiToken.sale with Google or GitHub and generate a key in the dashboard — it activates instantly, with no waitlist and no billing-country check. New Google/GitHub accounts start with $5 of platform bonus credit." },
    { q: "What model ID do I use for Opus in the API?", a: "claude-opus-4-8 for the current generation, claude-opus-4-7 for the previous one. Both run on the same key and prepaid balance, so switching is a one-field change in the request." },
    { q: "How much does the Claude Opus API cost per token?", a: "Officially $5 per 1M input tokens and $25 per 1M output tokens. On apiToken.sale the flat 50% discount applies to every call, so the same request costs $2.50/$12.50, with cache legs metered separately at their own discounted rates." },
    { q: "Is Opus worth it over Sonnet for coding?", a: "For hard reasoning, complex refactors and long agent runs, yes. For everyday coding, Sonnet 5 delivers near-Opus quality at 40% of the token price — many teams route per task on one key." },
    { q: "Does my prepaid balance expire if I only use Opus occasionally?", a: "No. The balance never expires and there is no subscription or monthly minimum, so idle time costs nothing and occasional Opus sessions simply draw it down slowly." },
  ],
  related: ["claude-opus-vs-sonnet", "claude-sonnet-api", "claude-haiku-api", "save-tokens-on-claude-api"],
  updated: "2026-08-17",
};
