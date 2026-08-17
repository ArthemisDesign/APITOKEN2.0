import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
    slug: "kimi-api-for-kimi-code",
    cluster: "integrate",
    title: "Use apiToken.sale in Kimi Code",
    h1: "Run Kimi, Claude, GPT and Gemini in Kimi Code",
    description: "Configure Kimi Code with an OpenAI-compatible provider for apiToken.sale: the exact config.toml provider and models tables, namespaced catalog IDs, context windows and key hygiene.",
    keywords: ["kimi code api", "kimi code custom provider", "kimi code config.toml", "kimi code api key", "kimi code openai compatible provider", "kimi code third party provider", "kimi code claude gpt gemini", "kimi code base_url", "kimi code models table", "kimi code prepaid api"],
    dek: "Kimi Code supports third-party OpenAI-compatible providers natively, so one apiToken.sale provider block in config.toml reaches the whole unified catalog — Kimi, Claude, GPT and Gemini on a single prepaid key. The integration is two TOML tables: a provider that holds the endpoint and credential, and one model alias per model you want to run.",
    sections: [
      { h2: "Kimi Code already speaks the router's protocol", blocks: [
        { type: "p", text: "To use a Kimi API key from apiToken.sale in Kimi Code, declare a provider with type = \"openai\" and base_url https://router.apitoken.sale/v1 in ~/.kimi-code/config.toml, then bind each model to it through a [models] alias. No plugin, proxy or patch is involved: the CLI's openai provider type speaks the Chat Completions protocol, which is exactly what the router's universal lane serves." },
        { type: "p", text: "The config model is deliberately split in two. A provider entry owns the protocol, endpoint and credential; a model entry owns the alias you type, the wire ID sent to the server, and the context window the CLI budgets against. That split is what makes a multi-provider key comfortable here — the provider is written once, and adding Claude, GPT or Gemini later means adding one small table per model, not a new credential." },
        { type: "p", text: "One behavior matters before you write anything: Kimi Code resolves credentials only from the config file. It checks the provider's api_key field, then the [providers.<name>.env] sub-table, and fails loudly at startup if neither is set. Exporting a variable in your shell does nothing — the CLI never falls back to shell environment variables for provider credentials." },
        cta(),
      ] },
      { h2: "Install the CLI and write the provider block", blocks: [
        { type: "steps", items: [
          "Install Kimi Code with the official script (no pre-installed Node.js required): curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash — the script verifies the checksum and puts the kimi executable on your PATH.",
          "Create an apiToken.sale account, top up any whole-dollar amount by card or crypto, and generate one API key (it looks like sk-pool-…). The same key and balance cover supported Kimi, Claude, GPT and Gemini models.",
          "Write the provider and a first model alias into ~/.kimi-code/config.toml as shown below.",
          "Lock the file down with chmod 600 ~/.kimi-code/config.toml — it holds the key in plain text.",
        ] },
        { type: "code", code: `# ~/.kimi-code/config.toml
default_model = "apitoken/k3"

[providers.apitoken]
type = "openai"
base_url = "https://router.apitoken.sale/v1"
api_key = "sk-pool-•••"

[models."apitoken/k3"]
provider = "apitoken"
model = "kimi/k3"
max_context_size = 1048576
display_name = "Kimi K3 (1M)"` },
        { type: "note", text: "Do not run /login for this setup. That command starts an OAuth device-code flow into the Kimi Code managed service, which bills a Kimi membership instead of your prepaid balance — managed accounts do not even appear in the /provider list. The hand-written provider block is the deterministic route; the interactive /provider manager exists, but it is built around public catalogs, not a prepaid multi-provider gateway." },
      ] },
      { h2: "Prove the route before the first real task", blocks: [
        { type: "steps", items: [
          "Start a session on the alias: kimi -m apitoken/k3.",
          "Run /status and confirm the active model reads apitoken/k3 — it reports the session runtime state: version, model, working directory and permission mode.",
          "Send one deterministic prompt: Reply with exactly: connected. A clean answer proves key, base_url and balance in a single round trip.",
          "List what the key can actually reach: curl https://router.apitoken.sale/v1/models -H \"Authorization: Bearer sk-pool-•••\" — the catalog is scoped to the key, so it shows only models currently routable and priced for it.",
        ] },
        { type: "note", text: "If you edit config.toml while the TUI is open, run /reload. It applies provider and model changes without restarting the CLI; a new shell export would not, because the file is the only credential source." },
      ] },
      { h2: "One provider block, every model family", blocks: [
        { type: "p", text: "The alias (the [models.\"...\"] key) is a local name only. What the router routes on is the model field, and it expects the unified catalog's namespaced IDs — kimi/k3, openai/gpt-5.6-terra, google/gemini-3.6-flash. Because the provider already holds the endpoint and key, each additional model is three lines:" },
        { type: "code", code: `[models."apitoken/kimi-for-coding"]
provider = "apitoken"
model = "kimi/kimi-for-coding"
max_context_size = 262144

[models."apitoken/gpt-terra"]
provider = "apitoken"
model = "openai/gpt-5.6-terra"
max_context_size = 400000   # review against the model page

[models."apitoken/gemini-flash"]
provider = "apitoken"
model = "google/gemini-3.6-flash"
max_context_size = 1048576  # review against the model page` },
        { type: "list", items: [
          "max_context_size is required per alias. The CLI uses it for overflow checks and for deciding when automatic compaction fires, so copy the model's reviewed window — 1048576 for K3's 1M mode, 262144 for Kimi for Coding — rather than guessing.",
          "Kimi Code auto-detects capabilities such as thinking, vision and tool use from known model name prefixes. For a namespaced gateway ID it may not recognize, declare them explicitly, e.g. capabilities = [\"thinking\", \"tool_use\"]; declared tags are unioned with the detected ones.",
          "Switch between declared aliases mid-session with /model — no restart and no config edit.",
          "All providers stream by default; if a gateway ever returns reasoning under a non-standard field name, the model alias accepts a reasoning_key override.",
        ] },
        { type: "link", text: "Reviewed context windows and per-model prices", href: "/models" },
      ] },
      { h2: "What these sessions cost on prepaid balance", blocks: [
        { type: "table", headers: ["Model to declare", "Official hit / miss / output", "Charged here after 50%"], rows: [
          ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
        ] },
        { type: "p", text: "Figures are per 1M tokens. Kimi caching is automatic, terminal usage reports which input was served from cache, and reasoning tokens bill as output — they are not a separate token class. apiToken.sale applies a flat 50% B2C discount to official rates, so a Kimi Code session costs exactly the tokens it consumed at half the official spend; an idle week costs nothing." },
        { type: "p", text: "The balance is shared across every alias on the key, so a Claude-heavy day and a Kimi-heavy day draw from the same pool. The practical guardrails are a lifetime spending limit and an expiration date per key, plus settled usage in the dashboard. A 402 mid-session means the pool is empty — top up and the next request succeeds; retrying will not." },
      ] },
      { h2: "Failures that are configuration, not model quality", blocks: [
        { type: "list", items: [
          "Startup fails before any request — the provider has no credential. Write api_key (or the [providers.apitoken.env] sub-table) in config.toml; a shell export is never read.",
          "401 on the first turn — the key is wrong or revoked, or base_url lost its /v1 suffix. Reproduce with the curl catalog call to isolate which half is broken.",
          "404 for a model you just declared — that ID is not in the key-scoped catalog. Trust GET /v1/models over memory, and re-check it before pinning an alias into a long-lived config.",
          "Compaction fires far earlier than expected — max_context_size is declared below the model's real window, so the CLI thinks it is out of room.",
          "The key sits in plain text — that is by design for this provider type, which is why chmod 600 is part of the setup and why the file belongs outside any synced or committed directory.",
        ] },
      ] },
    ],
    faq: [
      { q: "Can Kimi Code use an apiToken.sale key without /login?", a: "Yes. /login binds the CLI to the Kimi Code managed service over OAuth; a hand-written [providers] entry with type = \"openai\" and base_url https://router.apitoken.sale/v1 works entirely on the sk-pool key and never touches that flow." },
      { q: "Does Kimi Code read API keys from environment variables?", a: "No. Credential resolution is the provider's api_key field first, then the [providers.<name>.env] sub-table inside config.toml; if both are absent, startup fails. Shell exports are not consulted for provider credentials." },
      { q: "Can one provider block run Claude, GPT and Gemini in Kimi Code?", a: "Yes. The provider owns the endpoint and key; each model is a separate [models] alias whose model field carries the unified catalog's namespaced ID, such as openai/gpt-5.6-terra or google/gemini-3.6-flash." },
      { q: "What max_context_size should I declare for Kimi models?", a: "1048576 for K3's 1M mode and 262144 for Kimi for Coding. The CLI uses the value for overflow checks and compaction timing, so an understated number silently shrinks your usable session." },
      { q: "How do I switch models mid-session in Kimi Code?", a: "Run /model and pick any alias declared in your [models] table. Editing config.toml under a running TUI takes effect after /reload." },
    ],
    related: ["kimi-api-for-claude-code", "kimi-api-for-opencode", "how-to-buy-kimi-api-key", "kimi-k3-vs-kimi-for-coding"],
    published: "2026-08-09",
    updated: "2026-08-17",
  };
