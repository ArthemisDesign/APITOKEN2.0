import type { LearnArticle } from "../learn";
import { cta, OPENAI_BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "codex-cli-setup",
  cluster: "integrate",
  title: "Set Up Codex CLI with apiToken.sale — GPT-5.6 Profile",
  h1: "Run Codex CLI on apiToken.sale",
  description: "Configure Codex CLI with a named model_providers profile pointing at the apiToken.sale OpenAI-compatible endpoint — GPT-5.6 models on prepaid balance at a flat 50% off, no ChatGPT account needed.",
  keywords: ["codex cli setup", "codex config.toml", "codex custom model provider", "codex api key", "codex cli gpt-5.6", "codex responses api", "codex cli without chatgpt", "openai codex cli"],
  dek: "Codex CLI runs entirely on API-key authentication when you give it a custom model provider. One TOML profile points it at apiToken.sale, and your prepaid balance covers every session — no ChatGPT login, at a flat 50% below official spend.",
  sections: [
    { h2: "Create the profile", blocks: [
      { type: "p", text: "Save this as ~/.codex/apitoken.config.toml. A named profile leaves your default Codex configuration and any ChatGPT login untouched — you opt in per run." },
      { type: "code", code: `# ~/.codex/apitoken.config.toml\nmodel = "gpt-5.6-sol"\nmodel_provider = "apitoken"\n\n[model_providers.apitoken]\nname = "apiToken.sale"\nbase_url = "${OPENAI_BASE}"\nwire_api = "responses"\nenv_key = "APITOKEN_API_KEY"` },
      { type: "p", text: "env_key names the environment variable Codex reads the key from — the secret stays in your shell, never in the TOML file." },
      cta(),
    ] },
    { h2: "Run and verify", blocks: [
      { type: "code", code: `export APITOKEN_API_KEY=${KEY}\ncodex --profile apitoken` },
      { type: "list", items: [
        "Always pass --profile apitoken explicitly so there is no ambiguity about which provider — and which env var — is active.",
        "Switch models per project by editing the model line: gpt-5.6-sol for the hardest work, gpt-5.6-terra for the daily driver, gpt-5.6-luna for fast cheap steps.",
        "GET " + OPENAI_BASE + "/models with the same Bearer key lists the currently enabled set — the unified catalog namespaces IDs by provider (anthropic/*, openai/*, google/*).",
      ] },
      { type: "note", text: "wire_api = \"responses\" is the right value for this gateway — it serves both Responses and Chat Completions, and Codex streams over Responses. Set it only to \"chat\" if a specific client requires the classic shape." },
    ] },
    { h2: "Errors you might hit", blocks: [
      { type: "list", items: [
        "Missing APITOKEN_API_KEY — the variable named by env_key is not exported in the shell that runs codex. Export it in that same shell, or in your shell profile.",
        "stream error: unexpected status 401 — the key is wrong, revoked, or the base_url lost its /v1 suffix. Reproduce with curl outside Codex to isolate which half is broken.",
        "stream error: unexpected status 404 — the model ID is not enabled; check GET https://router.apitoken.sale/v1/models instead of assuming.",
        "402 — the shared prepaid balance needs a top-up; backoff will not fix it.",
      ] },
      { type: "link", text: "The full Codex error playbook — config.toml, auth.json, stream errors", href: "/errors/codex" },
    ] },
  ],
  faq: [
    { q: "Do I need a ChatGPT account or subscription?", a: "No. With a custom model_providers profile and the provider's API key in the environment, Codex runs entirely on API-key authentication — the ChatGPT login in auth.json is irrelevant." },
    { q: "Does this change my default Codex setup?", a: "No. The profile lives in its own file and activates only when you pass --profile apitoken. Your default configuration and login stay as they were." },
    { q: "Is the discount the same as for Claude?", a: "Yes. GPT-5.6 usage is metered at official OpenAI token rates and your flat 50% B2C discount applies to the same prepaid balance." },
    { q: "Responses or Chat Completions for wire_api?", a: "Use wire_api = \"responses\" — the gateway serves both, and Codex is built around the Responses stream. The Chat Completions shape exists for clients that require it." },
  ],
  related: ["openai-api-quickstart", "gpt-5-6-sol-vs-terra-vs-luna", "gpt-api-pricing", "how-billing-works"],
  published: "2026-07-29",
  updated: "2026-07-29",
};
