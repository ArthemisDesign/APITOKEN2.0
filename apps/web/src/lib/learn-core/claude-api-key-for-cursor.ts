import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-key-for-cursor",
  cluster: "integrate",
  title: "Claude API Key for Cursor",
  h1: "Use a Claude API key in Cursor",
  description: "Connect Cursor to Claude with an apiToken.sale key: set the Anthropic base URL to router.apitoken.sale, paste your key, pick a model, and code at a flat 50% off.",
  keywords: ["claude api key for cursor", "cursor claude api", "cursor anthropic key", "use claude in cursor", "cursor without cursor pro", "claude api key", "anthropic-compatible api", "claude api base url", "claude api setup", "claude api integration", "claude api cursor"],
  dek: "Cursor lets you bring your own Anthropic key, which means you can run Claude in Cursor on discounted prepaid balance instead of a bundled plan.",
  sections: [
    { h2: "Three-step setup", blocks: [
      { type: "steps", items: [
        "Open Cursor → Settings → Models → Anthropic API.",
        `Set the base URL to ${BASE} and paste your ${KEY} key.`,
        "Pick a model such as claude-opus-4-8 and start coding.",
      ] },
    ] },
    { h2: "Configuration", blocks: [
      { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : ${BASE}\nAPI key  : ${KEY}\nModel    : claude-opus-4-8` },
      cta(),
    ] },
    { h2: "Troubleshooting", blocks: [
      { type: "list", items: [
        "Cursor ignores the key: confirm you edited the Anthropic provider, not OpenAI.",
        "Model not found: set a current model ID like claude-opus-4-8.",
        "401: re-check the base URL and that the key was pasted in full.",
      ] },
      { type: "p", text: "Once connected, every supported Claude model is available on the same key and balance." },
    ] },
    { h2: "Your Claude API key in Cursor for any language", blocks: [
      { type: "p", text: "The key is language-agnostic — Cursor uses it for Python, JavaScript, TypeScript, Go, Rust or any project, on Windows, macOS and Linux. You are configuring the model provider, not the language." },
    ] },
  ],
  faq: [
    { q: "Can I use my own Claude key in Cursor?", a: "Yes. Cursor's Anthropic provider accepts a custom base URL and key, so you can point it at apiToken.sale." },
    { q: "Do I still need Cursor Pro?", a: "You can run Claude through your own API key and balance; features that require Cursor's own plan are separate from the model provider." },
    { q: "Does the Claude API key work in Cursor on Windows and Mac?", a: "Yes — the Anthropic provider setting is the same across Windows, macOS and Linux." },
  ],
  related: ["cursor-without-anthropic-account", "claude-api-for-vs-code", "claude-api-quick-setup", "claude-sonnet-api"],
  updated: "2026-07-17",
};
