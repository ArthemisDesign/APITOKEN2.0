// Claude model catalog for the /models programmatic SEO pages.
// Prices are official Anthropic per-million-token rates; the discount range
// shown to users derives from the live B2C pricing model (60% base, up to 70%
// with cumulative top-ups). Keep numbers in sync with Anthropic's price list.

export type ClaudeModel = {
  /** URL slug under /models/. */
  slug: string;
  /** Exact API model ID. */
  id: string;
  name: string;
  tier: "Opus" | "Sonnet" | "Haiku";
  /** <title> without brand suffix. */
  title: string;
  description: string;
  keywords: string[];
  dek: string;
  /** Official Anthropic $ per 1M tokens. */
  inputPerM: number;
  outputPerM: number;
  cacheReadPerM: number;
  cacheWrite5mPerM: number;
  context: string;
  maxOutput: string;
  bestFor: string[];
  notes: string[];
  faq: Array<{ q: string; a: string }>;
  /** Related learn-article slugs. */
  related: string[];
};

export const DISCOUNT_BASE = 0.6;
export const DISCOUNT_MAX = 0.7;

/** Price after the base (60%) discount, formatted. */
export function priceFrom(officialPerM: number): string {
  return formatUsd(officialPerM * (1 - DISCOUNT_BASE));
}

/** Price after the maximum (70%) discount, formatted. */
export function priceBest(officialPerM: number): string {
  return formatUsd(officialPerM * (1 - DISCOUNT_MAX));
}

export function formatUsd(value: number): string {
  const rounded = Math.round(value * 100) / 100;
  return `$${rounded.toFixed(2).replace(/\.?0+$/, (m) => (m === ".00" ? "" : m.replace(/0+$/, "")))}`;
}

export const claudeModels: ClaudeModel[] = [
  {
    slug: "claude-opus-4-8",
    id: "claude-opus-4-8",
    name: "Claude Opus 4.8",
    tier: "Opus",
    title: "Claude Opus 4.8 API — Price per Token & Access",
    description: "Claude Opus 4.8 API pricing: official $5/$25 per 1M tokens, from $2/$10 with the apiToken.sale discount. Instant key, prepaid balance, same Anthropic API.",
    keywords: ["claude opus 4.8 api", "claude opus 4.8 price", "claude opus 4.8 api cost", "opus 4.8 token pricing", "claude-opus-4-8", "buy claude opus api"],
    dek: "Claude Opus 4.8 is Anthropic's most capable Opus-tier model — the default choice for agentic coding, long-horizon tasks and hard reasoning. Here is what it costs per token, and how to run it cheaper on the same API.",
    inputPerM: 5,
    outputPerM: 25,
    cacheReadPerM: 0.5,
    cacheWrite5mPerM: 6.25,
    context: "1M tokens",
    maxOutput: "128K tokens",
    bestFor: [
      "Agentic coding in Claude Code, Cursor and Cline.",
      "Long-horizon autonomous tasks and complex refactors.",
      "The hardest reasoning, planning and review work.",
    ],
    notes: [
      "Adaptive thinking is the recommended mode; thinking tokens bill as output.",
      "1M-token context window at standard pricing — no long-context premium.",
    ],
    faq: [
      { q: "How much does the Claude Opus 4.8 API cost?", a: "Officially $5 per 1M input tokens and $25 per 1M output tokens. On apiToken.sale the same requests start 60% cheaper — from $2/$10 — and reach $1.50/$7.50 at the maximum 70% discount." },
      { q: "What is the model ID for Claude Opus 4.8?", a: "claude-opus-4-8. Use it unchanged with the Anthropic SDK, Claude Code, Cursor or any compatible tool pointed at https://api.apitoken.sale." },
      { q: "Is Opus 4.8 worth the price over Sonnet?", a: "For hard agentic and reasoning work, usually yes. For routine coding, Sonnet 5 delivers near-Opus quality at 40% of the token price — many teams route by task." },
    ],
    related: ["claude-opus-api", "best-claude-model-for-coding", "claude-api-pricing-explained", "cheapest-claude-api"],
  },
  {
    slug: "claude-opus-4-7",
    id: "claude-opus-4-7",
    name: "Claude Opus 4.7",
    tier: "Opus",
    title: "Claude Opus 4.7 API — Price per Token & Access",
    description: "Claude Opus 4.7 API pricing: official $5/$25 per 1M tokens, from $2/$10 with the apiToken.sale discount. Same Anthropic endpoint, instant prepaid access.",
    keywords: ["claude opus 4.7 api", "claude opus 4.7 price", "opus 4.7 api cost", "claude-opus-4-7", "opus 4.7 token pricing"],
    dek: "Claude Opus 4.7 is the previous-generation Opus — still a top-tier model for agentic work and deep reasoning, priced identically to Opus 4.8.",
    inputPerM: 5,
    outputPerM: 25,
    cacheReadPerM: 0.5,
    cacheWrite5mPerM: 6.25,
    context: "1M tokens",
    maxOutput: "128K tokens",
    bestFor: [
      "Workloads pinned to Opus 4.7 for reproducibility.",
      "Agentic coding and multi-step reasoning.",
      "Vision-heavy tasks with high-resolution image support.",
    ],
    notes: [
      "Same per-token price as Opus 4.8 — most new work should target claude-opus-4-8.",
      "Supports adaptive thinking and the full effort range.",
    ],
    faq: [
      { q: "How much does the Claude Opus 4.7 API cost?", a: "Officially $5 per 1M input tokens and $25 per 1M output tokens — the same as Opus 4.8. With the apiToken.sale discount that starts at $2/$10 and reaches $1.50/$7.50." },
      { q: "Should I use Opus 4.7 or 4.8?", a: "They cost the same, so new projects should default to claude-opus-4-8. Keep 4.7 when you have prompts or evals pinned to it." },
      { q: "Does my key work for both?", a: "Yes — one apiToken.sale key and balance covers every supported Claude model; you switch by changing the model ID." },
    ],
    related: ["claude-opus-api", "claude-api-pricing-explained", "claude-opus-vs-sonnet", "how-billing-works"],
  },
  {
    slug: "claude-sonnet-5",
    id: "claude-sonnet-5",
    name: "Claude Sonnet 5",
    tier: "Sonnet",
    title: "Claude Sonnet 5 API — Price per Token & Access",
    description: "Claude Sonnet 5 API pricing: official $3/$15 per 1M tokens, from $1.20/$6 with the apiToken.sale discount. Near-Opus coding quality at Sonnet cost.",
    keywords: ["claude sonnet 5 api", "claude sonnet 5 price", "sonnet 5 api cost", "claude-sonnet-5", "sonnet 5 token pricing"],
    dek: "Claude Sonnet 5 brings near-Opus quality to coding and agentic work at 40% of the Opus token price — the best default for most development workloads.",
    inputPerM: 3,
    outputPerM: 15,
    cacheReadPerM: 0.3,
    cacheWrite5mPerM: 3.75,
    context: "1M tokens",
    maxOutput: "128K tokens",
    bestFor: [
      "Day-to-day coding — the default in most editors.",
      "Agentic workflows where Opus cost is not justified.",
      "High-volume production API traffic.",
    ],
    notes: [
      "Anthropic lists introductory pricing of $2/$10 per 1M tokens through 2026-08-31; the standard rate is $3/$15.",
      "Adaptive thinking is on by default when the thinking parameter is omitted.",
    ],
    faq: [
      { q: "How much does the Claude Sonnet 5 API cost?", a: "The standard official rate is $3 per 1M input tokens and $15 per 1M output tokens (Anthropic lists an introductory $2/$10 through August 2026). apiToken.sale applies your 60–70% discount on top of official spend." },
      { q: "What is the model ID for Claude Sonnet 5?", a: "claude-sonnet-5 — use it as-is in the Anthropic SDK, Claude Code, Cursor, Cline or any compatible tool." },
      { q: "Is Sonnet 5 good enough for coding?", a: "For most coding it is the sweet spot: near-Opus quality on agentic and editing tasks at a much lower per-token price. Route only the hardest reasoning to Opus." },
    ],
    related: ["claude-sonnet-api", "best-claude-model-for-coding", "claude-opus-vs-sonnet", "save-tokens-on-claude-api"],
  },
  {
    slug: "claude-sonnet-4-6",
    id: "claude-sonnet-4-6",
    name: "Claude Sonnet 4.6",
    tier: "Sonnet",
    title: "Claude Sonnet 4.6 API — Price per Token & Access",
    description: "Claude Sonnet 4.6 API pricing: official $3/$15 per 1M tokens, from $1.20/$6 with the apiToken.sale discount. Proven balanced model on the same Anthropic API.",
    keywords: ["claude sonnet 4.6 api", "claude sonnet 4.6 price", "sonnet 4.6 api cost", "claude-sonnet-4-6", "sonnet 4.6 token pricing"],
    dek: "Claude Sonnet 4.6 is the previous-generation balanced model — a proven workhorse for coding and production pipelines, at the same list price as Sonnet 5.",
    inputPerM: 3,
    outputPerM: 15,
    cacheReadPerM: 0.3,
    cacheWrite5mPerM: 3.75,
    context: "1M tokens",
    maxOutput: "128K tokens",
    bestFor: [
      "Pipelines tuned and evaluated against Sonnet 4.6.",
      "Balanced coding and content workloads.",
      "Teams migrating gradually to Sonnet 5.",
    ],
    notes: [
      "Same list price as Sonnet 5 — new projects should usually start on claude-sonnet-5.",
      "Supports adaptive thinking; effort defaults to high.",
    ],
    faq: [
      { q: "How much does the Claude Sonnet 4.6 API cost?", a: "Officially $3 per 1M input tokens and $15 per 1M output tokens. With the apiToken.sale discount that starts at $1.20/$6 and reaches $0.90/$4.50." },
      { q: "Sonnet 4.6 or Sonnet 5?", a: "They share a list price, and Sonnet 5 is stronger on coding and agentic work — prefer it for new projects. Stay on 4.6 when your prompts and evals are pinned to it." },
      { q: "Can I switch models without a new key?", a: "Yes. One key and one prepaid balance cover every supported Claude model — switching is just a model-ID change." },
    ],
    related: ["claude-sonnet-api", "claude-3-5-vs-claude-4", "claude-api-pricing-explained", "how-billing-works"],
  },
  {
    slug: "claude-haiku-4-5",
    id: "claude-haiku-4-5",
    name: "Claude Haiku 4.5",
    tier: "Haiku",
    title: "Claude Haiku 4.5 API — Price per Token & Access",
    description: "Claude Haiku 4.5 API pricing: official $1/$5 per 1M tokens, from $0.40/$2 with the apiToken.sale discount. The cheapest and fastest Claude model.",
    keywords: ["claude haiku 4.5 api", "claude haiku price", "haiku 4.5 api cost", "claude-haiku-4-5", "cheapest claude model"],
    dek: "Claude Haiku 4.5 is the fastest and cheapest Claude model — built for high-volume, latency-sensitive work like classification, extraction and routing.",
    inputPerM: 1,
    outputPerM: 5,
    cacheReadPerM: 0.1,
    cacheWrite5mPerM: 1.25,
    context: "200K tokens",
    maxOutput: "64K tokens",
    bestFor: [
      "Classification, extraction and summarization at scale.",
      "Latency-sensitive chat and routing layers.",
      "Cheap pre-processing before an Opus or Sonnet call.",
    ],
    notes: [
      "200K context window and 64K max output — smaller than the Opus/Sonnet line.",
      "Pairs well with model routing: send bulk work to Haiku, hard reasoning to Opus.",
    ],
    faq: [
      { q: "How much does the Claude Haiku 4.5 API cost?", a: "Officially $1 per 1M input tokens and $5 per 1M output tokens. With the apiToken.sale discount that starts at $0.40/$2 and reaches $0.30/$1.50 — the cheapest way to run Claude." },
      { q: "What is Haiku 4.5 good for?", a: "High-volume, low-latency work: classification, extraction, summarization, routing and simple chat. For complex reasoning, step up to Sonnet 5 or Opus 4.8." },
      { q: "What is the model ID?", a: "claude-haiku-4-5. It works on the same apiToken.sale key and balance as every other supported Claude model." },
    ],
    related: ["claude-haiku-api", "save-tokens-on-claude-api", "cheapest-claude-api", "best-claude-model-for-coding"],
  },
];

export const claudeModelBySlug: Record<string, ClaudeModel> = Object.fromEntries(
  claudeModels.map((model) => [model.slug, model]),
);

export function modelPath(slug: string): string {
  return `/models/${slug}`;
}

export const MODELS_HUB_PATH = "/models";
