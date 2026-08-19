import type { LearnArticle } from "../learn";
import { OPENAI } from "./shared";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
    slug: "kimi-api-for-opencode",
    cluster: "integrate",
    title: "Kimi API for OpenCode: Router Plugin Setup",
    h1: "Run Kimi K3 and Kimi for Coding in OpenCode",
    description: "Kimi API for OpenCode: set up the apiToken.sale router plugin with a key-scoped live catalog, apitoken/kimi/* model IDs and one prepaid key at 50% off.",
    keywords: ["kimi opencode", "kimi api opencode", "kimi k3 opencode", "kimi for coding setup", "opencode custom provider", "kimi coding agent", "opencode router plugin", "opencode.jsonc provider", "kimi k3 coding agent", "opencode models apitoken"],
    dek: "OpenCode runs the Kimi API through one apiToken.sale config plugin: the installer registers an apitoken provider on the router's OpenAI-compatible lane, and the plugin rebuilds the model list from the key-scoped live catalog on every start. You address K3 and Kimi for Coding explicitly as apitoken/kimi/{model}, and usage settles against the same prepaid balance as Claude, GPT and Gemini.",
    sections: [
      { h2: "One plugin instead of a hand-written provider list", blocks: [
        { type: "p", text: "The direct answer to \"Kimi in OpenCode\": install the apiToken.sale router plugin, restart, and pick an explicit apitoken/kimi/* model. There is no static provider block to maintain — on every startup the plugin fetches your personal GET /v1/models response and translates the authoritative limits, capabilities and current prices into OpenCode's native model schema. What the catalog does not return for your key simply does not appear in the picker." },
        { type: "p", text: "The installer puts a small loader in OpenCode's global auto-load directory, seeds a verified fallback runtime, and merges one apitoken provider into ~/.config/opencode/opencode.jsonc — keeping a backup of whatever was there before. The provider entry speaks @ai-sdk/openai-compatible against the router's /v1 lane and holds either a literal sk-pool-… key or OpenCode's standard {env:NAME} placeholder:" },
        { type: "code", code: `// ~/.config/opencode/opencode.jsonc — the entry the installer adds\n{\n  "provider": {\n    "apitoken": {\n      "npm": "@ai-sdk/openai-compatible",\n      "options": {\n        "baseURL": "${OPENAI}",\n        "apiKey": "{env:APITOKEN_API_KEY}"\n      }\n    }\n  }\n}` },
        { type: "note", text: "Prefer the {env:APITOKEN_API_KEY} placeholder over pasting the key: the secret then lives in your shell profile instead of a config file you might commit or sync." },
      ] },
      { h2: "Install and prove the connection", blocks: [
        { type: "steps", items: [
          "Run the installer; it merges the apitoken provider into your existing opencode.jsonc and keeps a backup.",
          "Export APITOKEN_API_KEY in the shell that launches OpenCode if you chose the placeholder form, then restart OpenCode so the plugin fetches the key-scoped catalog.",
          "List what your key can actually see: opencode models apitoken. This output — not a blog post — is the source of truth for available Kimi IDs.",
          "Run one deterministic prompt with an explicit namespaced model. A clean answer proves the key, the base URL and the balance in a single round trip.",
        ] },
        { type: "code", code: `curl -fsSL https://raw.githubusercontent.com/apitokensale-admin/apitoken.sale/main/opencode/install.sh | bash\n\nexport APITOKEN_API_KEY=sk-pool-•••\n\nopencode models apitoken\n\nopencode run --model apitoken/kimi/kimi-for-coding "Reply with exactly: connected"` },
      ] },
      { h2: "Choose between the four Kimi aliases", blocks: [
        { type: "p", text: "Model access is catalog-driven, so confirm an alias in opencode models apitoken before pinning it in a project. All four share one balance; the choice is cost, context and latency. Figures are per 1M tokens, at official rates with the flat 50% apiToken.sale discount applied:" },
        { type: "table", headers: ["OpenCode model ID", "Role", "Official hit / miss / output", "You pay after 50%"], rows: [
          ["apitoken/kimi/kimi-for-coding", "Economical coding default", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["apitoken/kimi/kimi-for-coding-highspeed", "Lower latency at exactly double rates", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
          ["apitoken/kimi/k3-256k", "K3 reasoning in the 256K context mode", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["apitoken/kimi/k3", "K3 with the full 1M context", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
        ] },
        { type: "list", items: [
          "Default everyday agent loops to kimi-for-coding — the lowest general coding rate in the published Kimi set.",
          "Reach for highspeed only when latency visibly pays for itself; every rate leg is exactly double the base alias.",
          "Use k3-256k when you want K3 reasoning without paying attention to the 1M context mode, and k3 when the catalog exposes the full window for long codebases.",
          "Kimi caching is automatic: repeated context bills at the hit rate, a newly cached token is a miss, and reasoning tokens are a subset of output billed at the output rate — never a fourth leg.",
        ] },
        { type: "link", text: "Kimi pricing mechanics in depth: cache legs, aliases, High Speed", href: "/docs/learn/kimi-api-pricing" },
      ] },
      { h2: "Why the live catalog beats a static model list", blocks: [
        { type: "p", text: "A hand-written provider config freezes model IDs, context limits and prices at the day you typed them. The plugin instead re-reads the key-scoped /v1/models on every OpenCode start, so retired or unavailable aliases never linger in local config, and limits come from the router's authoritative fields rather than from a substring heuristic on the model name. Because the catalog is scoped to your key, it only ever offers models that are currently routable and priced for you." },
        { type: "p", text: "If the catalog is briefly unreachable, the plugin falls back to an encrypted local last-good snapshot — AES-256-GCM, mode 0600, bound to the exact credential and base URL, fresh for 15 minutes and reusable for at most 7 days. Models served from the snapshot are explicitly marked \"[stale metadata; pricing unavailable]\", and cost is never shown from cache: prices reappear only after the next successful live discovery." },
        { type: "note", text: "Do not paste Kimi's internal tariff IDs (such as kimi-k2.7-code) into OpenCode. The router accepts the public subscription aliases the catalog returns, and the plugin registers exactly those." },
      ] },
      { h2: "The three failures a Kimi session will actually throw", blocks: [
        { type: "list", items: [
          "401 — the key is wrong, revoked, or the baseURL lost its /v1 suffix. Reproduce the call with curl against " + OPENAI + "/models outside OpenCode to isolate which half is broken.",
          "404 — the model ID is not enabled for your key right now. Check opencode models apitoken instead of assuming the alias you typed exists.",
          "402 — the shared prepaid balance is empty. Retrying with backoff will not help; top up and the next request succeeds.",
        ] },
        { type: "p", text: "All three are configuration or balance problems, not model problems — none of them is fixed by re-sending the same prompt. The 401 in particular almost always reduces to the missing /v1 suffix or an extra character pasted into the key." },
      ] },
      { h2: "What an OpenCode session costs on prepaid balance", blocks: [
        { type: "p", text: "Billing is per token at official Kimi rates with the flat 50% discount subtracted before the charge touches your prepaid balance. There is no subscription and no seat fee: an idle week costs nothing, and a heavy refactor session costs exactly the tokens it consumed at half the official spend. The balance is shared across the supported Claude, GPT, Gemini and Kimi namespaces, so OpenCode sessions draw from the same pool as everything else you run." },
        { type: "list", items: [
          "Top up any whole-dollar amount by card or crypto — no separate Kimi plan is required on your side.",
          "Set a lifetime spending limit on the key and inspect settled usage in the dashboard; a 402 is the meter running out, nothing else breaking.",
          "Keep long agent loops on kimi-for-coding and escalate to k3 only for hard reasoning or long-context work — that split is where the real savings live.",
        ] },
        cta(),
        { type: "link", text: "Full per-model specs and discounted prices", href: "/models" },
      ] },
      { h2: "If you also drive Kimi from Claude Code or Kimi Code", blocks: [
        { type: "p", text: "The same key works in the other coding agents, but their configuration is different. Claude Code talks to the router's Anthropic Messages endpoint and needs every model tier pinned — main, Opus, Sonnet, Haiku and the subagent model variables all set to one Kimi alias. Kimi Code takes an explicit OpenAI-compatible provider block in its own config.toml with the key stored in the file, which then needs chmod 600." },
        { type: "p", text: "OpenCode is the only one of the three that consumes the live catalog directly, which makes it the safest setup for switching between K3 and Kimi for Coding without hand-maintaining provider limits — the other two trust whatever you pinned." },
        { type: "link", text: "Pin Kimi aliases in Claude Code", href: "/docs/learn/kimi-api-for-claude-code" },
        { type: "link", text: "Declare the provider in Kimi Code's config.toml", href: "/docs/learn/kimi-api-for-kimi-code" },
      ] },
    ],
    faq: [
      { q: "Does OpenCode support Kimi models?", a: "Yes. The apiToken.sale router plugin registers the live Kimi namespace, and OpenCode selects models explicitly as apitoken/kimi/{model} — for example apitoken/kimi/kimi-for-coding." },
      { q: "Why use the router plugin instead of a static model list?", a: "The plugin re-fetches the key-scoped /v1/models catalog on every start, so model IDs, limits and availability stay aligned with what your key can actually run. A static config keeps offering retired or unavailable aliases until you edit it by hand." },
      { q: "What happens in OpenCode when the catalog is unreachable?", a: "The plugin restores capability metadata from an encrypted last-good snapshot bound to your credential and base URL. Cached models are marked \"[stale metadata; pricing unavailable]\" and show no cost until the next successful live discovery." },
      { q: "How much does Kimi for Coding cost in OpenCode?", a: "Official rates are $0.19 per 1M cache-hit tokens, $0.95 per 1M cache-miss tokens and $4 per 1M output tokens, and apiToken.sale charges half of that on prepaid balance. The highspeed alias is exactly double each leg." },
      { q: "Which Kimi model should be my OpenCode default?", a: "Make apitoken/kimi/kimi-for-coding the default for everyday agent loops, escalate to apitoken/kimi/k3 for hard reasoning or long-context codebase work, and reserve highspeed for sessions where latency is visibly worth double rates." },
      { q: "Can Claude Code use Kimi too?", a: "Yes, with a different setup. Point Claude Code at the router's Anthropic Messages endpoint and pin its main, Opus, Sonnet, Haiku and subagent model variables to one Kimi alias." },
    ],
    related: ["kimi-api-for-claude-code", "kimi-api-for-kimi-code", "kimi-api-quickstart", "kimi-k3-vs-kimi-for-coding"],
    published: "2026-08-09",
    updated: "2026-08-17",
  };
