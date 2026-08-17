import type { LearnArticle } from "../learn";

export const article: LearnArticle = {
    slug: "kimi-api-for-kimi-code",
    cluster: "integrate",
    title: "Use apiToken.sale in Kimi Code",
    h1: "Run Kimi, Claude, GPT and Gemini in Kimi Code",
    description: "Connect Kimi Code to apiToken.sale through its OpenAI-compatible provider config, declare a namespaced model and protect the API key stored in config.toml.",
    keywords: ["kimi code api", "kimi code custom provider", "kimi code config toml", "kimi code api key", "kimi code k3", "kimi code openai compatible"],
    dek: "Kimi Code accepts a custom OpenAI-compatible provider, so one apiToken.sale provider entry can reach the unified catalog. Each model still needs an explicit local declaration with its real namespace and reviewed context window.",
    sections: [
      { h2: "Install and declare the provider", blocks: [
        { type: "code", code: `curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash

# ~/.kimi-code/config.toml
default_model = "apitoken/k3"

[providers.apitoken]
type = "openai"
base_url = "https://router.apitoken.sale/v1"
api_key = "sk-pool-•••"

[models."apitoken/k3"]
provider = "apitoken"
model = "kimi/k3"
max_context_size = 1048576
display_name = "Kimi K3 (1M)"

chmod 600 ~/.kimi-code/config.toml` },
        { type: "note", text: "Do not run /login for this setup: that binds the CLI to a Kimi membership instead. Kimi Code stores custom-provider credentials only in config.toml, so the file contains the key in plain text and must be locked down." },
      ] },
      { h2: "Start, verify and add models", blocks: [
        { type: "code", code: `kimi -m apitoken/k3

/status

Reply with exactly: connected` },
        { type: "list", items: [
          "/status must show https://router.apitoken.sale/v1 as the provider base URL.",
          "The model field uses the unified catalog namespace, for example kimi/k3, openai/gpt-5.6-terra or google/gemini-3.6-flash.",
          "Declare each additional model in config.toml with its reviewed max_context_size; Kimi Code uses that value to decide when to compact.",
        ] },
      ] },
    ],
    faq: [
      { q: "Can Kimi Code use an apiToken.sale key?", a: "Yes. Add an OpenAI-compatible provider with base_url https://router.apitoken.sale/v1 and store the key in Kimi Code's config.toml." },
      { q: "Can Kimi Code run models other than Kimi?", a: "Yes. The same provider entry reaches the unified catalog; declare each Claude, GPT, Gemini or Kimi model with its namespaced ID and correct context limit." },
      { q: "Why is chmod 600 important?", a: "Kimi Code does not read custom-provider credentials from the shell. The raw API key lives in config.toml, so that file should be readable only by your account." },
    ],
    related: ["kimi-api-for-claude-code", "kimi-api-for-opencode", "how-to-buy-kimi-api-key", "kimi-k3-vs-kimi-for-coding"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
