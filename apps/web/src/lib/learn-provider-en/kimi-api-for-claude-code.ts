import type { LearnArticle } from "../learn";

export const article: LearnArticle = {
    slug: "kimi-api-for-claude-code",
    cluster: "integrate",
    title: "Use Kimi K3 in Claude Code",
    h1: "Run Kimi K3 and Kimi for Coding in Claude Code",
    description: "Configure Claude Code for Kimi K3 or Kimi for Coding through apiToken.sale: pin every model tier, preserve the 1M context window and verify the endpoint.",
    keywords: ["kimi claude code", "kimi k3 claude code", "kimi for coding claude code", "claude code custom model", "claude code kimi api", "k3 1m claude code"],
    dek: "Claude Code already speaks Anthropic Messages, so it can run Kimi directly. The reliable setup pins every internal model tier to one Kimi alias; otherwise the main session can work while subagents fail on an inherited Claude model.",
    sections: [
      { h2: "Pin the connection and every model tier", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale
export ANTHROPIC_API_KEY=sk-pool-•••
export ANTHROPIC_MODEL=k3
export ANTHROPIC_DEFAULT_OPUS_MODEL=k3
export ANTHROPIC_DEFAULT_SONNET_MODEL=k3
export ANTHROPIC_DEFAULT_HAIKU_MODEL=k3
export CLAUDE_CODE_SUBAGENT_MODEL=k3
export CLAUDE_CODE_MAX_CONTEXT_TOKENS=1048576
export CLAUDE_CODE_AUTO_COMPACT_WINDOW=1048576

claude --model k3` },
        { type: "p", text: "Use the bare subscription alias on the Anthropic lane. For a 256K model such as k3-256k or kimi-for-coding, keep the tier pins but omit the two 1M context variables." },
      ] },
      { h2: "Verify the route, not the model's introduction", blocks: [
        { type: "list", items: [
          "Open /status and confirm that the Anthropic base URL is apiToken.sale.",
          "Do not ask the model to identify itself: Claude Code's system prompt can make any backend call itself Claude.",
          "Treat none/off as disabling K3 reasoning, not as a model selector. Live coverage kept those turns on the K3 tariff; kimi-k2.6 is not an addressable public model.",
          "Check GET /v1/models before pinning an alias for a long-lived environment.",
        ] },
      ] },
    ],
    faq: [
      { q: "Does Claude Code support Kimi K3?", a: "Yes. Point Claude Code at https://router.apitoken.sale and pin every model tier to an admitted Kimi subscription alias." },
      { q: "Why must every Claude Code model variable be pinned?", a: "Claude Code chooses separate models for its main session, tiers and subagents. An unpinned tier can inherit a Claude ID and fail only when that background path runs." },
      { q: "How do I keep K3's full 1M context in Claude Code?", a: "Use k3 or k3[1m] and set both CLAUDE_CODE_MAX_CONTEXT_TOKENS and CLAUDE_CODE_AUTO_COMPACT_WINDOW to 1048576." },
    ],
    related: ["kimi-api-for-kimi-code", "kimi-api-for-opencode", "kimi-k3-vs-kimi-for-coding", "kimi-api-quickstart"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
