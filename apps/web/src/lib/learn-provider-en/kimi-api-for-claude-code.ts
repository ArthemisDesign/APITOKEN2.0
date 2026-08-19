import type { LearnArticle } from "../learn";
import { cta, KEY } from "../learn-shared";
import { ROUTER } from "./shared";

export const article: LearnArticle = {
    slug: "kimi-api-for-claude-code",
    cluster: "integrate",
    title: "Kimi API for Claude Code: K3 and Kimi for Coding",
    h1: "Run Kimi K3 and Kimi for Coding in Claude Code",
    description: "Kimi API for Claude Code: point Claude Code at Kimi K3 or Kimi for Coding via apiToken.sale, pin every model tier, keep the 1M context window, pay 50% less.",
    keywords: ["kimi claude code", "kimi k3 claude code", "kimi for coding claude code", "claude code custom model", "claude code kimi api", "claude code anthropic_base_url", "claude code subagent model", "k3 1m claude code", "claude code without claude subscription", "kimi api anthropic messages endpoint"],
    dek: "Claude Code speaks Anthropic Messages to whatever endpoint you name, so a Kimi subscription alias on the apiToken.sale router works with no plugin and no patch. The reliable setup pins every internal model tier to Kimi — an unpinned tier inherits a Claude ID and fails only when that background path runs. Usage lands on one prepaid balance at half the official Kimi token rates.",
    sections: [
      { h2: "Claude Code already speaks Kimi's protocol", blocks: [
        { type: "p", text: `Claude Code sends Anthropic Messages requests to whatever ANTHROPIC_BASE_URL says, and the router at ${ROUTER} answers that protocol for Kimi subscription aliases. No plugin, proxy or fork is involved: you change environment variables, and every session, tier decision and subagent call goes to Kimi instead of Anthropic. Billing moves to your prepaid apiToken.sale balance at a flat 50% below the official Kimi token rates.` },
        { type: "p", text: "The one thing that makes this setup fail silently is Claude Code's internal model map. It keeps separate models for the main session, its Opus/Sonnet/Haiku tiers and its subagents. Setting only ANTHROPIC_MODEL redirects the visible conversation while background paths — title generation, compaction, Task subagents — still carry inherited Claude IDs and break the moment they run." },
        cta(),
      ] },
      { h2: "Pin the endpoint and every model tier", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=${ROUTER}
export ANTHROPIC_API_KEY=${KEY}
export ANTHROPIC_MODEL=k3
export ANTHROPIC_DEFAULT_OPUS_MODEL=k3
export ANTHROPIC_DEFAULT_SONNET_MODEL=k3
export ANTHROPIC_DEFAULT_HAIKU_MODEL=k3
export CLAUDE_CODE_SUBAGENT_MODEL=k3
export CLAUDE_CODE_MAX_CONTEXT_TOKENS=1048576
export CLAUDE_CODE_AUTO_COMPACT_WINDOW=1048576

claude --model k3` },
        { type: "p", text: "The three ANTHROPIC_DEFAULT_* variables cover Claude Code's tier routing, CLAUDE_CODE_SUBAGENT_MODEL covers Task subagents, and the two context variables raise both the window and the auto-compact ceiling to K3's 1M tokens. Use the bare subscription alias on the Anthropic lane; the scoped GET /v1/models catalog shows the namespaced kimi/* spellings, so check it before pinning an alias into a long-lived environment." },
        { type: "note", text: "Do not skip the two 1M variables on the k3 alias and do not keep them on a 256K alias. They tell Claude Code how much context it may use before compacting, and a value the served model does not support distorts that decision in both directions." },
      ] },
      { h2: "Match the alias to the session", blocks: [
        { type: "table", headers: ["Alias", "Context", "Official hit / miss / output", "Here after 50%"], rows: [
          ["kimi-for-coding", "256K", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi-for-coding-highspeed", "256K", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
          ["k3-256k", "256K", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["k3 · k3[1m]", "1M", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
        ] },
        { type: "p", text: "Figures are per 1M tokens; Kimi caching is automatic, so cache hits and misses bill as separate legs. For a 256K alias such as k3-256k or kimi-for-coding, keep the tier pins exactly as above but omit CLAUDE_CODE_MAX_CONTEXT_TOKENS and CLAUDE_CODE_AUTO_COMPACT_WINDOW. k3[1m] is a compatibility spelling of K3's 1M mode — the router normalizes it to the provider's real k3 wire model, so both forms cost the same." },
        { type: "p", text: "A practical split: kimi-for-coding as the daily driver for edits and test loops, k3 when the session needs long-context reasoning over a whole repository, and kimi-for-coding-highspeed only when latency justifies exactly double the base token rates." },
        { type: "link", text: "Full K3 vs Kimi for Coding comparison", href: "/docs/learn/kimi-k3-vs-kimi-for-coding" },
      ] },
      { h2: "Verify the route, not the model's introduction", blocks: [
        { type: "steps", items: [
          "Start a session and run /status. Confirm the Anthropic base URL is apiToken.sale before trusting anything else in the session.",
          "Send one trivial prompt — \"Reply with exactly: connected\". A clean answer proves the key, the base URL and the balance in a single round trip.",
          `Check the scoped catalog before pinning an alias long-term: curl ${ROUTER}/v1/models with your key lists what the key can actually call.`,
          "Exercise a Task subagent once. It is the path most likely to carry an unpinned tier, and you want that failure on the first day, not mid-refactor.",
        ] },
        { type: "note", text: "Do not ask the model to identify itself as a verification method. Claude Code's system prompt can make any backend call itself Claude, so the introduction proves nothing about which model is serving the turn — /status and the request path are the evidence." },
      ] },
      { h2: "Reasoning switches are not model selectors", blocks: [
        { type: "p", text: "Setting the model slot to none or off disables K3 reasoning; it does not switch you to a different or older Kimi model. Those turns stay on the K3 tariff either way. kimi-k2.6 is not an addressable public model on the router, so typing it selects nothing — use the aliases from the scoped catalog." },
        { type: "p", text: "K3 supports low, high and max reasoning effort, with high as the default; Kimi for Coding runs with thinking enabled. Reasoning tokens are a subset of output and bill at the output rate — they are never added again as a separate token class, so a thinking-heavy session shows up as output volume, not a surcharge." },
      ] },
      { h2: "What a Kimi session costs on prepaid balance", blocks: [
        { type: "p", text: "Every turn is metered per token at the official Kimi rates above, with the flat 50% discount subtracted before the charge touches your prepaid balance. There is no subscription and no seat fee: an idle week costs nothing, and a heavy refactor costs exactly the tokens it consumed at half the official spend. The same balance covers supported Claude, GPT and Gemini models, so a Claude Code session on Kimi draws from the same pool as everything else you run." },
        { type: "list", items: [
          "Set a lifetime spending limit on the key and inspect settled usage in the dashboard.",
          "Default to kimi-for-coding and escalate whole-repository sessions to k3 rather than running everything at K3 rates.",
          "Reserve kimi-for-coding-highspeed for latency-sensitive loops; its rates are exactly double the base tier.",
          "Treat a balance-exhausted response as the signal it is — top up and the next request succeeds; retrying changes nothing.",
        ] },
        { type: "link", text: "Per-alias Kimi rates and cache legs", href: "/docs/learn/kimi-api-pricing" },
        { type: "link", text: "Live catalog of all supported models and prices", href: "/models" },
      ] },
    ],
    faq: [
      { q: "Does Claude Code support Kimi K3?", a: "Yes. Point ANTHROPIC_BASE_URL at https://router.apitoken.sale, authenticate with your apiToken.sale key and pin every model tier to an admitted Kimi subscription alias — no plugin is needed because Claude Code already speaks Anthropic Messages." },
      { q: "Why must every Claude Code model variable be pinned?", a: "Claude Code chooses separate models for its main session, its tiers and its subagents. An unpinned tier can inherit a Claude ID and fail only when that background path runs, so a session can look healthy while compaction or a Task call is broken." },
      { q: "How do I keep K3's full 1M context in Claude Code?", a: "Use k3 or k3[1m] and set both CLAUDE_CODE_MAX_CONTEXT_TOKENS and CLAUDE_CODE_AUTO_COMPACT_WINDOW to 1048576. On 256K aliases such as k3-256k or kimi-for-coding, omit both variables." },
      { q: "Is kimi-k2.6 a valid model ID in Claude Code?", a: "No. kimi-k2.6 is not an addressable public model on the router, and none/off in the model slot disables K3 reasoning rather than selecting another model. Use the subscription aliases returned by the scoped GET /v1/models catalog." },
      { q: "What does a Claude Code session on Kimi cost?", a: "Usage bills per token at official Kimi rates with a flat 50% discount on prepaid balance — Kimi for Coding at $0.19 / $0.95 / $4 per 1M cache-hit, cache-miss and output tokens before the discount, High Speed at exactly double that." },
    ],
    related: ["kimi-api-for-kimi-code", "kimi-api-for-opencode", "kimi-k3-vs-kimi-for-coding", "kimi-api-quickstart"],
    published: "2026-08-09",
    updated: "2026-08-17",
  };
