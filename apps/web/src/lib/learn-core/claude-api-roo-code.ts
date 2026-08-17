import type { LearnArticle } from "../learn";
import { cta, BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-roo-code",
  cluster: "integrate",
  title: "Use the Claude API with Roo Code",
  h1: "Use the Claude API with Roo Code",
  description: "Connect Roo Code in VS Code to Claude through apiToken.sale: pick the Anthropic provider, enable the custom base URL, paste your key and code at a flat 50% off.",
  keywords: ["claude api roo code", "roo code anthropic provider", "roo code custom base url", "roo code claude api key", "roo code model per mode", "roo code api configuration profile", "roo code prompt caching", "roo code openai compatible provider", "roo code vs cline", "roo code cheap claude"],
  dek: "The Claude API works in Roo Code through the extension's native Anthropic provider: tick the custom-base-URL box, point it at the apiToken.sale gateway, and paste one prepaid key. This guide walks the exact provider settings, per-mode model pinning across Roo's Code, Architect and Ask modes, and the four errors you will actually hit.",
  published: "2026-07-17",
  updated: "2026-08-17",
  sections: [
    { h2: "Connect Roo Code to the gateway in one profile", blocks: [
      { type: "p", text: `Roo Code ships a native Anthropic provider with an optional custom base URL, so wiring it to the discounted gateway is three fields, not a plugin or a proxy. Set the provider to Anthropic, point the base URL at ${BASE}, paste your ${KEY} key — and every task runs on Claude at a flat 50% off official token rates.` },
      { type: "steps", items: [
        "Open Roo Code → Settings → Providers and create a new API configuration profile; name it something like \"apiToken\" so you can tell it apart from a direct-Anthropic profile later.",
        `Set API Provider to Anthropic, tick the "Use custom base URL" checkbox and enter ${BASE} — exactly that, with no trailing path.`,
        `Paste your apiToken.sale key (${KEY}) into the API Key field. The key is sent as x-api-key, the standard Anthropic Messages header.`,
        "Save the profile, choose claude-sonnet-5 as the model, and run a small task — ask Roo to explain a file — to confirm the round trip before you hand it a real refactor.",
      ] },
      { type: "code", code: `# Roo Code → Settings → Providers (profile "apiToken")\nAPI Provider : Anthropic\n[x] Use custom base URL\nBase URL     : ${BASE}\nAPI Key      : ${KEY}\nModel        : claude-sonnet-5` },
      cta(),
    ] },
    { h2: "Anthropic provider or OpenAI-compatible — stay on the Anthropic lane", blocks: [
      { type: "p", text: `Roo Code also offers an "OpenAI Compatible" provider, and the same key answers on both protocols: Anthropic Messages at ${BASE} and an OpenAI-shaped lane at ${OPENAI_BASE} with Authorization: Bearer. For Claude, keep the Anthropic provider. Roo Code's Claude-specific controls — the prompt-caching toggle, extended-thinking options and the tool-use plumbing its agent loop depends on — are built against the Messages API shape, and you lose them on the generic lane.` },
      { type: "code", code: `# Only for tools without an Anthropic option:\nAPI Provider : OpenAI Compatible\nBase URL     : ${OPENAI_BASE}\nAPI Key      : ${KEY}\nModel ID     : claude-sonnet-5   # typed by hand, no dropdown` },
      { type: "note", text: "On the OpenAI-compatible lane Roo Code does not fetch a model list — the model ID is a free-text field. A typo there surfaces as \"model not found\", not as an auth error, which sends people chasing the wrong problem." },
    ] },
    { h2: "Pin a different Claude model to each Roo mode", blocks: [
      { type: "p", text: "Roo Code's API configuration profiles are not just for credentials: you can bind a profile to each mode and switch models with the mode. That turns the model choice from a global compromise into a per-mode decision, which is how you keep an agentic loop fast without paying Opus prices for every file read." },
      { type: "table", headers: ["Roo Code mode", "Model to pin", "Why"], rows: [
        ["Ask", "claude-haiku-4-5", "Q&A about code is short, read-only work; the cheapest Claude is usually enough."],
        ["Code", "claude-sonnet-5", "The everyday driver for edits, test runs and tool loops — near-Opus coding quality at a mid-tier rate."],
        ["Architect", "claude-opus-4-8", "Planning is where a wrong call costs the most downstream; spend the strong model here."],
        ["Debug", "claude-sonnet-5", "Shares the Code profile; promote a stubborn bug to claude-opus-4-8 manually when the loop stalls."],
      ] },
      { type: "p", text: "Every supported Claude generation sits behind the same key and the same prepaid balance — Opus 4.8 and 4.7, Sonnet 5 and 4.6, Haiku 4.5 — so a profile per model costs nothing to maintain and nothing extra to fund." },
    ] },
    { h2: "What an agentic loop does to your token bill", blocks: [
      { type: "p", text: "Roo Code does not chat; it loops. One task reads files, drafts a plan, edits, runs the result and re-checks — each iteration a full model call carrying the system prompt, tool schemas and conversation so far. That is precisely the workload where a per-token discount matters most: the identical session, 50% cheaper, with token-level visibility in the apiToken.sale dashboard so you can see which mode is spending." },
      { type: "note", text: "Turn Roo Code's prompt-caching option on. Roo resends the same large prefix — its system prompt, your rules files, the repo context — on every call in a loop, and cached input is billed at the cheaper official cache rates minus your discount. On long sessions the cache line is usually the biggest single saver." },
      { type: "p", text: "Two ways to sanity-check the spend before a big task: estimate the tokens with the cost calculator, and confirm the exact per-model rates on the catalog page." },
      { type: "link", text: "Estimate a Roo Code session in the Claude API cost calculator", href: "/tools/claude-api-cost-calculator" },
      { type: "link", text: "Per-model rates for every supported Claude model", href: "/models" },
    ] },
    { h2: "Troubleshoot the errors Roo Code actually throws", blocks: [
      { type: "list", items: [
        `401 Unauthorized — the key or the base URL is wrong. Re-paste the key and check the base URL is exactly ${BASE}; a stray trailing slash or a /v1 suffix on the Anthropic lane breaks routing.`,
        "Model not found — the model ID is stale or mistyped. Use a current ID such as claude-sonnet-5, claude-opus-4-8 or claude-haiku-4-5.",
        "429 rate limit — Roo fires tool calls in bursts. Raise the \"Rate limit\" setting (minimum seconds between requests) in provider settings instead of hammering retry.",
        "Context window overflow — long sessions accumulate file reads faster than you expect. Start a fresh task per unit of work rather than stretching one thread, or let Roo condense the context when it offers.",
      ] },
      { type: "note", text: "Checkpoints are worth enabling on agentic edits: Roo snapshots the workspace before changes, so a bad loop is a one-click revert instead of a git archaeology session." },
    ] },
    { h2: "Reuse the same key in Cline, Cursor and the SDKs", blocks: [
      { type: "p", text: "The key is not married to Roo Code. One key covers Roo Code, Cline, Cursor and the Anthropic and OpenAI SDKs simultaneously, all drawing on the same prepaid balance — so you can trial a different agent without a second account or a second top-up." },
      { type: "p", text: "If you come from Cline, nothing conceptual changes: both are VS Code agents with an Anthropic provider that accepts a custom base URL, and the setup differs only in where the settings live. Roo Code adds mode-based profiles and finer auto-approval controls; Cline is the leaner single-mode agent. Pick the agent on workflow, not on the key — it works in both." },
    ] },
  ],
  faq: [
    { q: "Does Roo Code support a custom Anthropic base URL?", a: `Yes. The Anthropic provider in Roo Code's settings has a "Use custom base URL" checkbox; enable it, set the URL to ${BASE} and authenticate with your apiToken.sale key.` },
    { q: "Which Claude models can Roo Code use on this key?", a: "Every supported Claude model — Opus 4.8 and 4.7, Sonnet 5 and 4.6, Haiku 4.5 — on one key and one prepaid balance, so you can pin different models to different Roo Code modes." },
    { q: "Can Roo Code use a different model per mode?", a: "Yes. API configuration profiles can be bound per mode, so Ask can run claude-haiku-4-5, Code claude-sonnet-5 and Architect claude-opus-4-8 without touching settings between tasks." },
    { q: "Does prompt caching still work through the gateway?", a: "Yes. Keep Roo Code's prompt-caching option enabled; cached input is billed at the cheaper official cache rates minus your flat 50% discount, which compounds on long agentic loops." },
    { q: "Is Roo Code setup different from Cline?", a: "Barely. Both are VS Code agents whose Anthropic provider accepts a custom base URL, so the same key and the same URL work in either; use whichever agent's workflow you prefer." },
  ],
  related: ["claude-api-for-vs-code", "claude-api-key-for-cursor", "claude-api-langchain", "cheapest-claude-api"],
};
