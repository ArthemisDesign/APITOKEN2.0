import type { LearnArticle } from "../learn";

export const article: LearnArticle = {
    slug: "kimi-api-for-opencode",
    cluster: "integrate",
    title: "Use the Kimi API in OpenCode",
    h1: "Run Kimi K3 and Kimi for Coding in OpenCode",
    description: "Connect OpenCode to Kimi through apiToken.sale with the router plugin, a key-scoped model catalog, explicit kimi/* IDs and one prepaid API key.",
    keywords: ["kimi opencode", "kimi api opencode", "kimi k3 opencode", "kimi for coding setup", "opencode custom provider", "kimi coding agent"],
    dek: "OpenCode can address the Kimi namespace explicitly and consumes the router's live catalog. That makes it the safest coding-agent setup for switching between K3 and Kimi for Coding without hand-maintaining provider limits.",
    sections: [
      { h2: "Install and verify", blocks: [
        { type: "steps", items: [
          "Run the apiToken.sale OpenCode installer; it merges the router plugin into your existing config and keeps a backup.",
          "Restart OpenCode so the plugin fetches the key-scoped model catalog.",
          "Run one deterministic prompt with an explicit namespaced model.",
        ] },
        { type: "code", code: "curl -fsSL https://raw.githubusercontent.com/apitokensale-admin/apitoken.sale/main/opencode/install.sh | bash\n\nopencode run --model apitoken/kimi/kimi-for-coding \"Reply with exactly: connected\"" },
      ] },
      { h2: "Choose a Kimi model safely", blocks: [
        { type: "list", items: [
          "apitoken/kimi/kimi-for-coding — economical coding default.",
          "apitoken/kimi/kimi-for-coding-highspeed — lower latency at double token rates.",
          "apitoken/kimi/k3-256k — K3 reasoning in the smaller context mode.",
          "apitoken/kimi/k3 — K3 with the full 1M context when the catalog exposes it.",
        ] },
        { type: "note", text: "Claude Code and Kimi Code also support Kimi, but their configuration is different: Claude Code needs every model tier pinned, while Kimi Code uses an explicit OpenAI-compatible provider block." },
      ] },
    ],
    faq: [
      { q: "Does OpenCode support Kimi models?", a: "Yes. The apiToken.sale router plugin registers the live Kimi namespace and OpenCode selects models as apitoken/kimi/{model}." },
      { q: "Why use the router plugin instead of a static model list?", a: "It keeps model IDs, limits and availability aligned with the key-scoped live catalog, so retired or unavailable aliases do not linger in local config." },
      { q: "Can Claude Code use Kimi too?", a: "Yes, with a different setup. Point Claude Code at the Anthropic endpoint and pin its main, Opus, Sonnet, Haiku and subagent model variables to one Kimi alias." },
    ],
    related: ["kimi-api-for-claude-code", "kimi-api-for-kimi-code", "kimi-api-quickstart", "kimi-k3-vs-kimi-for-coding"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
