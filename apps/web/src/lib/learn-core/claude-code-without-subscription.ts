import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-code-without-subscription",
  cluster: "free",
  title: "Use Claude Code Without a Subscription",
  h1: "Claude Code without the $200/month plan",
  description: "Run Claude Code on pay-as-you-go API balance instead of a monthly subscription. Set ANTHROPIC_BASE_URL to router.apitoken.sale and pay only for what you use.",
  keywords: ["claude code without subscription", "claude code api key", "claude code pay as you go", "claude code without max plan", "claude code no subscription", "run claude code cheap", "claude code anthropic_base_url", "claude code prepaid api", "claude code cost per session", "claude code billing alternative"],
  dek: "You can use Claude Code without a subscription by pointing it at any Anthropic-compatible API key. On apiToken.sale that means prepaid balance at a flat 50% off official token rates — no monthly fee, no seat, no idle-time cost.",
  sections: [
    { h2: "Claude Code only needs an API key, not a plan", blocks: [
      { type: "p", text: "Yes — Claude Code works without any Anthropic subscription. The CLI reads two environment variables, ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY, and talks to whatever Anthropic Messages API endpoint they point at. Give it a prepaid apiToken.sale key and every session bills per token against your balance instead of a flat monthly plan." },
      { type: "p", text: "Nothing about the tool changes. Same agent loop, same file edits, same terminal workflow — the only difference is where the request lands and how it is billed." },
    ] },
    { h2: "What the $200/month plan actually buys", blocks: [
      { type: "p", text: "A top-tier Claude plan is a consumer subscription: a fixed fee for interactive use inside Anthropic's own apps, with usage caps you cannot meter directly. It makes sense if you chat heavily every single day and never touch the API." },
      { type: "p", text: "It is a poor fit when your usage is spiky, when you want a programmable key for your own scripts and tools, or when you would rather pay $0 on a week you barely code. Pay-as-you-go API billing inverts the model: no fee for existing, a charge only when tokens actually flow." },
    ] },
    { h2: "Switch Claude Code to pay-as-you-go in two variables", blocks: [
      { type: "steps", items: [
        "Create a free account on apiToken.sale and top up any whole-dollar amount — the balance never expires.",
        "Generate an API key in the dashboard (it looks like sk-pool-…). One key covers every supported Claude model, plus GPT, Gemini and Kimi on the same balance.",
        "Export the two variables below, restart your shell, and run claude. Verify with /status inside Claude Code — it shows the active endpoint and auth source.",
      ] },
      { type: "code", code: `# ~/.zshrc or ~/.bashrc\nexport ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# new shell, then just run\nclaude` },
      { type: "note", text: "If Claude Code reports an auth error after the switch, the usual cause is a stale shell: the variables must be exported before the claude process starts. Open a fresh terminal or source your rc file, then retry." },
      cta(),
    ] },
    { h2: "What a session costs per token", blocks: [
      { type: "p", text: "Every request is metered at official Anthropic token rates, then the flat 50% B2C discount is subtracted before the charge touches your balance. Agentic coding is input-heavy — the repo context, tool results and conversation history are re-sent each turn — so the input column is where your money goes." },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (−50%)"], rows: [
        ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
      ] },
      { type: "p", text: "The dashboard shows each request with its model and token breakdown, so a long session is auditable line by line rather than a black-box subscription day." },
      { type: "link", text: "Full per-model pricing, including cache rates", href: "/models" },
      { type: "link", text: "Estimate a month of Claude Code usage in the free calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "Stretching balance on long agentic sessions", blocks: [
      { type: "p", text: "Model choice is the biggest lever. Claude Code lets you switch models mid-session, so route routine edits to Sonnet and reserve Opus for the hard multi-file refactors where it earns its rate." },
      { type: "list", items: [
        "Everyday coding and quick fixes: claude-sonnet-5.",
        "Deep refactors and long reasoning chains: claude-opus-4-8.",
        "Triage, renaming, boilerplate: claude-haiku-4-5 at a fifth of Opus input cost.",
      ] },
      { type: "note", text: "Prompt caching helps on repeated context: cached input tokens bill at a lower cache-read rate, so keeping one long session beats restarting a fresh conversation that re-reads the whole repo." },
    ] },
    { h2: "One key past Claude Code", blocks: [
      { type: "p", text: "The same key and balance drive every Anthropic-compatible tool — Cursor, Cline, Continue, Zed, Aider, the official SDKs — and the same prepaid balance also covers supported GPT, Gemini and Kimi models through the router's OpenAI-compatible and native Gemini lanes. A subscription never gives you that key at all." },
      { type: "link", text: "Point the Anthropic SDK at the same endpoint", href: "/docs/learn/anthropic-sdk-base-url" },
      { type: "link", text: "Use the same key inside Cursor", href: "/docs/learn/claude-api-key-for-cursor" },
    ] },
    { h2: "When the subscription still makes sense", blocks: [
      { type: "p", text: "Be honest about your pattern. If you run Claude Code eight hours a day, every workday, at maximum intensity, a flat plan can cost less than raw per-token spend — that is the usage profile subscriptions are priced for. Everyone else — weekend projects, burst-driven freelancers, teams mixing several AI tools on one budget — pays for idle days they never use. Prepaid balance has no idle days: top up as you go, and refunds, if ever needed, go through the original payment provider via support (Telegram, English and Russian, or apitokensale@gmail.com)." },
    ] },
  ],
  faq: [
    { q: "Can I use Claude Code without a Claude subscription?", a: "Yes. Set ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY and Claude Code runs on any Anthropic-compatible API key — including a prepaid apiToken.sale key — with no plan required." },
    { q: "Do I lose Claude Code features without a subscription?", a: "No. The CLI behaves identically; only billing changes, from a flat monthly plan to per-token usage against prepaid balance." },
    { q: "How much does a Claude Code session cost on prepaid balance?", a: "Requests are metered at official Anthropic rates minus a flat 50% B2C discount. Sonnet 5 works out to $1.50 / $7.50 per 1M input/output tokens; Opus 4.8 to $2.50 / $12.50." },
    { q: "Does Claude Code work with a custom ANTHROPIC_BASE_URL?", a: "Yes, that variable is exactly how the CLI selects its endpoint. Point it at https://router.apitoken.sale and it serves the same Anthropic Messages API with the same model IDs." },
    { q: "Is there a free way to try Claude Code first?", a: "New apiToken.sale accounts created with Google or GitHub get $5 of platform bonus credit, which covers a real Claude Code trial session before you top up." },
  ],
  related: ["claude-api-key-for-cursor", "cheapest-claude-api", "claude-opus-api", "anthropic-sdk-base-url"],
  updated: "2026-08-17",
};
