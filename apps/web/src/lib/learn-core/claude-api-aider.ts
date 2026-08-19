import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-aider",
  cluster: "integrate",
  title: "Use the Claude API with Aider",
  h1: "Use the Claude API with Aider",
  description: "Use the Claude API with Aider: export ANTHROPIC_API_BASE pointing at router.apitoken.sale, pick a Claude model, and pair-program at a flat 50% off.",
  keywords: ["claude api aider", "aider anthropic", "aider claude", "aider anthropic api base", "aider claude api key", "aider custom anthropic endpoint", "aider cheap claude", "aider weak model", "aider token usage", "aider claude sonnet"],
  dek: "Aider reaches the Claude API through LiteLLM, and LiteLLM honours ANTHROPIC_API_BASE — so two environment variables reroute your whole claude-api-aider setup to the discounted gateway. Same models, same commands, same git workflow; every token bills at a flat 50% off official rates against a prepaid balance.",
  published: "2026-07-17",
  updated: "2026-08-17",
  sections: [
    { h2: "Point Aider at the gateway endpoint", blocks: [
      { type: "p", text: "Yes, Aider works with a custom Claude endpoint, and the change takes under a minute. Aider routes Anthropic traffic through LiteLLM under the hood, and LiteLLM honours the ANTHROPIC_API_BASE environment variable — so no config file, plugin, or patch is required. Export the endpoint and your key, then start Aider exactly as before." },
      { type: "code", code: `export ANTHROPIC_API_KEY=${KEY}\nexport ANTHROPIC_API_BASE=${BASE}\n\naider --model anthropic/claude-opus-4-8` },
      { type: "p", text: "The key comes from the apiToken.sale dashboard and looks like sk-pool-…; one key works across every supported model, so the same exports also cover any other Claude model you pass to --model later." },
      { type: "note", text: "ANTHROPIC_API_BASE is not the variable Claude Code reads (that one is ANTHROPIC_BASE_URL). Aider goes through LiteLLM and wants the API_BASE spelling — if you run both tools on this key, export both variables; they do not conflict." },
      { type: "p", text: "On Windows, set the same two variables in PowerShell instead of export: $env:ANTHROPIC_API_KEY and $env:ANTHROPIC_API_BASE, or use setx to persist them across sessions. Aider behaves identically — LiteLLM reads the process environment on every platform." },
    ] },
    { h2: "Make the setup survive new shells", blocks: [
      { type: "p", text: "Exports die with the shell that set them. Put the exports in your shell profile (~/.zshrc, ~/.bashrc) so every terminal starts ready, and move the model choices into Aider's own YAML configuration — Aider reads .aider.conf.yml from your home directory, the git root, or the current directory, in that order." },
      { type: "code", code: `# ~/.aider.conf.yml\nmodel: anthropic/claude-sonnet-5\nweak-model: anthropic/claude-haiku-4-5\ncache-prompts: true` },
      { type: "p", text: "Keep secrets out of this file: the key stays in the environment, the config holds only behaviour. A project-level .aider.conf.yml committed to the repo then pins the model choice for the whole team without pinning anyone's key." },
    ] },
    { h2: "Choose a Claude model per Aider role", blocks: [
      { type: "p", text: "Aider does not use one model — it uses up to three, and each role has a different price-performance sweet spot." },
      { type: "table", headers: ["Aider role", "Flag", "Model", "Use it for"], rows: [
        ["Main chat model", "--model", "anthropic/claude-sonnet-5", "The everyday default; near-Opus coding quality for most sessions"],
        ["Main model, hardest jobs", "--model", "anthropic/claude-opus-4-8", "Deep multi-file refactors and long agentic edits"],
        ["Weak model", "--weak-model", "anthropic/claude-haiku-4-5", "Commit messages and chat-history summaries"],
        ["Editor model (architect mode)", "--editor-model", "anthropic/claude-sonnet-5", "Turning the architect model's plan into concrete diffs"],
      ] },
      { type: "p", text: "The weak model is the quiet win: Aider calls it on every commit and every history summarisation, so pointing it at Haiku shaves cost off work you never read anyway. All three models run on the same key and the same discount — switching roles never means switching accounts." },
      { type: "link", text: "Compare current Claude models and token prices", href: "/models" },
    ] },
    { h2: "Why long Aider sessions cost what they cost", blocks: [
      { type: "p", text: "Aider is token-hungry by design, and it helps to know where the tokens go before judging a bill. Every turn resends the repo map, the full content of each file you added to the chat, and the conversation history as input tokens; every edit comes back as output tokens. A two-hour refactoring session is really hundreds of thousands of tokens, not a few chat messages." },
      { type: "list", items: [
        "Repo map: a compressed outline of the whole repository, resent as it changes.",
        "Added files: everything you /add joins the prompt in full on every request until you /drop it.",
        "Edit format: diff-style formats resend less code than whole-file rewrites.",
        "Multi-file edits: each touched file bills input and output tokens of its own.",
      ] },
      { type: "p", text: "This is exactly where the flat 50% discount compounds: a session that would cost $10 at official token rates costs $5 here, and the gap widens with every hour the session runs. Two habits stack on top of it: a Haiku weak model takes over the constant trickle of commit-message and summarisation calls, and diff-style edit formats keep output tokens proportional to the change instead of the file. Neither changes how Aider behaves — only what the same work costs." },
      cta(),
    ] },
    { h2: "Trim tokens inside a running session", blocks: [
      { type: "steps", items: [
        "Run /tokens early and often — Aider prints the token count of the current context and the running session total, so you see the cost of a bloated chat before you send it.",
        "/drop any file you are done editing. Files stay in the prompt until removed, and a stale large file is the most common silent token leak.",
        "/clear between unrelated tasks. Chat history is resent with every message; a new task deserves a fresh context.",
        "Switch down for cheap questions with /model anthropic/claude-haiku-4-5, then switch back — model changes mid-session need no restart.",
        "Start Aider with --cache-prompts (or cache-prompts: true in the config) so repeated file context is served from Anthropic prompt caching instead of being billed as fresh input every turn.",
      ] },
      { type: "link", text: "Estimate a session before you run it with the cost calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "Troubleshooting: Aider still talks to Anthropic directly", blocks: [
      { type: "list", items: [
        "Exports made in a different shell — Aider reads its own process environment, so export the variables in the same shell that launches it, or in your shell profile.",
        "An old Aider or LiteLLM version — the endpoint override lives in LiteLLM, so upgrade with pip install -U aider-chat before debugging anything else.",
        "A 401 on the first request — the key is mistyped or revoked; the endpoint is fine, the credential is not.",
      ] },
      { type: "p", text: "To isolate which half is broken, bypass Aider and hit the gateway directly with a minimal Messages call:" },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-haiku-4-5","max_tokens":16,"messages":[{"role":"user","content":"ping"}]}'` },
      { type: "p", text: "A JSON reply proves the endpoint and key are healthy and the problem sits in Aider's environment; an error here means fix the exports, not the tool." },
    ] },
  ],
  faq: [
    { q: "Does Aider work with a custom Claude endpoint?", a: "Yes. Aider uses LiteLLM for Anthropic models, and LiteLLM honours the ANTHROPIC_API_BASE environment variable — set it to https://router.apitoken.sale and start Aider normally." },
    { q: "Which Claude model is best in Aider?", a: "claude-sonnet-5 is the best default for most coding; switch to claude-opus-4-8 for the hardest multi-file work. Set claude-haiku-4-5 as the weak model so commit messages and summaries bill at Haiku rates — all three run on the same key." },
    { q: "How much cheaper is a long Aider session?", a: "Every request is billed at official token rates minus your flat 50% discount, so a session that would cost $10 direct costs $5 here." },
    { q: "Is ANTHROPIC_API_BASE the same as ANTHROPIC_BASE_URL?", a: "No. Aider reaches Anthropic through LiteLLM, which reads ANTHROPIC_API_BASE; Claude Code reads ANTHROPIC_BASE_URL. Exporting both is harmless if you use both tools." },
    { q: "Can I mix Claude models in one Aider session?", a: "Yes. Pass --model for the main chat model, --weak-model for bookkeeping tasks, and /model mid-session to switch without restarting — one API key covers every supported model." },
    { q: "Do I need a config file to get started?", a: "No. The two environment variables are enough for a first run; .aider.conf.yml is only for persisting model choices like cache-prompts and the weak model across shells and projects." },
  ],
  related: ["claude-api-litellm", "claude-code-without-subscription", "best-claude-model-for-coding", "save-tokens-on-claude-api"],
};
