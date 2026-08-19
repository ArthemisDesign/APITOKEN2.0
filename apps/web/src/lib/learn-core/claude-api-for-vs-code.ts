import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-for-vs-code",
  cluster: "integrate",
  title: "Claude API in VS Code (Cline, Continue)",
  h1: "Use the Claude API in VS Code",
  description: "Run the Claude API in VS Code with Cline or Continue: set the Anthropic base URL to router.apitoken.sale, paste your key, and pay per token at 50% off.",
  keywords: ["claude api vs code", "cline claude api", "continue claude api", "claude in vscode", "vscode anthropic api key", "claude api key", "anthropic-compatible api", "claude api base url", "cline custom base url", "continue config claude api", "claude api vscode"],
  dek: "Setting up the Claude API in VS Code comes down to a single setting: free extensions like Cline and Continue accept any Anthropic-compatible endpoint. Point one at https://router.apitoken.sale with an apiToken.sale key and Claude edits, answers and reviews inside the editor, billed per token from prepaid balance at 50% off official API pricing.",
  updated: "2026-08-17",
  sections: [
    { h2: "One base URL and one key is the entire integration", blocks: [
      { type: "p", text: `You do not need an Anthropic Console account to run Claude inside VS Code. The free Cline and Continue extensions both accept any Anthropic-compatible endpoint, so the whole job is pointing them at ${BASE}, pasting your apiToken.sale key and choosing a model. From that moment Claude answers questions, edits files and reviews diffs without leaving the editor, billed per token from your prepaid balance at a flat 50% off official API pricing.` },
      { type: "p", text: "Nothing exotic happens under the hood. The extension sends ordinary Anthropic Messages requests with an x-api-key header to the gateway; the gateway authenticates your sk-pool-… key, routes the call to the requested Claude model and meters the tokens. Because the wire protocol is untouched, the features these extensions rely on — streaming responses, tool use, large context windows — behave exactly as they would against Anthropic's own endpoint." },
      { type: "table", headers: ["Setting", "What to enter"], rows: [
        ["API provider", "Anthropic (built into both extensions)"],
        ["Base URL", BASE],
        ["API key", `${KEY} — generated in the apiToken.sale dashboard`],
        ["Model", "claude-opus-4-8 for hard tasks, claude-sonnet-5 for everything else"],
        ["Cost", "Per token from prepaid balance, 50% off official rates"],
      ] },
      cta(),
    ] },
    { h2: "Set up Cline in four steps", blocks: [
      { type: "steps", items: [
        "Install Cline from the VS Code Marketplace and open its settings via the gear icon.",
        "Set API Provider to Anthropic and enable the custom base URL option.",
        `Paste ${BASE} as the base URL and your ${KEY} key below it.`,
        "Enter claude-opus-4-8 as the model, then run a small throwaway task to confirm the key works before trusting it with a real one.",
      ] },
      { type: "code", code: `# Cline → Settings\nAPI Provider : Anthropic\nBase URL     : ${BASE}\nAPI Key      : ${KEY}\nModel        : claude-opus-4-8` },
    ] },
    { h2: "Keep autonomous Cline sessions cheap", blocks: [
      { type: "p", text: "Cline is the strong default when you want autonomous edits: give it a task and it reads files, plans, applies changes, runs terminal commands and re-checks its own diff in a loop until the job is done. That loop is exactly why per-token pricing matters — one task can mean dozens of Messages calls, and on a discounted gateway the same session costs half the official rate." },
      { type: "list", items: [
        "Prompt caching is billed at the cheaper official cache rates minus your discount, so re-reading the same large files across turns costs a fraction of the naive price.",
        "Route routine chores to claude-sonnet-5 and reserve claude-opus-4-8 for the problems that genuinely need it — you can switch models mid-session without touching the key.",
        "Watch the dashboard: it shows token-level usage per request, so a runaway agent loop is visible before it eats the balance.",
      ] },
    ] },
    { h2: "Point Continue at the same gateway", blocks: [
      { type: "p", text: "Continue is the lighter option: it shines at inline chat, quick edits and in-editor help rather than autonomous multi-file runs, and it is configured from a single file you can commit to a repo or sync across machines. Like Cline it is free — only the API usage touches your balance." },
      { type: "code", code: `# ~/.continue/config.yaml\nname: local\nversion: 1.0.0\nschema: v1\nmodels:\n  - name: Claude Opus 4.8 (apiToken.sale)\n    provider: anthropic\n    model: claude-opus-4-8\n    apiBase: ${BASE}\n    apiKey: ${KEY}\n    roles: [chat, edit, apply]` },
      { type: "note", text: `Older Continue releases read ~/.continue/config.json instead; the equivalent entry sets "provider": "anthropic", "apiBase": "${BASE}", "apiKey" and "model", and it still works. Keep the autocomplete role on a small dedicated model — Claude earns its tokens on chat, edit and apply.` },
    ] },
    { h2: "Cline or Continue: pick by workflow, not by price", blocks: [
      { type: "p", text: "Both extensions are free and both bill the same prepaid balance, so the choice is purely about how you like to work. Many developers install both on one key: Cline for delegated tasks, Continue for quick questions and inline rewrites." },
      { type: "table", headers: ["", "Cline", "Continue"], rows: [
        ["Best at", "Autonomous multi-file tasks", "Inline chat and quick edits"],
        ["Interaction", "Plan/act loop with approvals", "Sidebar chat and inline edit"],
        ["Token profile", "Higher — read/edit/verify loops", "Lower — mostly single-shot prompts"],
        ["Extension price", "Free", "Free"],
      ] },
      { type: "p", text: "The same sk-pool-… key also works simultaneously in Cursor, Roo Code and the Anthropic SDK — one balance across every tool. If you split your time between VS Code and Cursor, the Cursor guide walks the same two-minute flow, and the model catalog lists every model the key unlocks." },
      { type: "link", text: "Claude API key for Cursor", href: "/docs/learn/claude-api-key-for-cursor" },
      { type: "link", text: "Model catalog and per-token prices", href: "/models" },
    ] },
    { h2: "The three errors everyone hits", blocks: [
      { type: "list", items: [
        `401 Unauthorized: the key or the base URL is wrong. Re-paste the full sk-pool-… key and make sure the base URL is exactly ${BASE}, with no extra path segment or typo.`,
        "Model not found: the extension's bundled model list lags behind. Type a current ID explicitly — claude-sonnet-5 or claude-opus-4-8 — instead of picking a stale entry.",
        "Slow responses or 429: you are over the rate limit. Lower the extension's concurrency and respect the Retry-After header before resubmitting.",
      ] },
      { type: "p", text: "If a request fails mid-task in Cline, do not just retry the whole task — the agent keeps its plan, so resuming after fixing the key or model ID avoids paying for the same context twice." },
    ] },
  ],
  faq: [
    { q: "Which VS Code extensions work with an apiToken.sale key?", a: `Any extension that supports an Anthropic-compatible endpoint, including Cline and Continue. Set the provider to Anthropic, override the base URL with ${BASE} and paste your key.` },
    { q: "Do I have to pay for Cline or Continue themselves?", a: "No. Both extensions are free; you only pay for Claude API usage, drawn per token from your prepaid balance at 50% off official rates. Accounts created with Google or GitHub start with $5 of platform bonus credit." },
    { q: "What base URL do I enter for Claude in VS Code?", a: `Set the Anthropic provider's custom base URL to ${BASE} and use your sk-pool-… key from the apiToken.sale dashboard. The setting is identical in Cline and Continue.` },
    { q: "Why does the extension say the model was not found?", a: "Bundled model lists in extensions lag behind. Type a current model ID explicitly — claude-sonnet-5 or claude-opus-4-8 — rather than choosing a stale dropdown entry." },
    { q: "Can I use the same key in VS Code and Cursor at the same time?", a: "Yes. One key works across Cline, Continue, Cursor, Roo Code and the Anthropic SDK simultaneously, all drawing on the same prepaid balance with token-level usage in the dashboard." },
  ],
  related: ["claude-api-key-for-cursor", "anthropic-sdk-base-url", "claude-api-quick-setup", "claude-code-without-subscription"],
};
