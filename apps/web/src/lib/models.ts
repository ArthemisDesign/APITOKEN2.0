// Model catalog for the /models programmatic SEO pages — two providers, one key and balance.
// Claude prices are official Anthropic per-million-token rates; GPT prices are official OpenAI
// per-million-token rates from the pinned engine catalog (crates/metering/src/codex.rs). The
// discounted price shown to users derives from the flat B2C pricing model (50% off official
// spend on every request) and applies to both providers through the same account multiplier.
// Keep numbers in sync with the providers' price lists and the engine catalog.

export type ModelProvider = "anthropic" | "openai";

export type ClaudeModel = {
  provider: "anthropic";
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

export type OpenAiModel = {
  provider: "openai";
  /** URL slug under /models/. */
  slug: string;
  /** Exact API model ID on the OpenAI-compatible endpoint. */
  id: string;
  name: string;
  tier: "Flagship" | "Balanced" | "Fast";
  /** <title> without brand suffix. */
  title: string;
  description: string;
  keywords: string[];
  dek: string;
  /** Official OpenAI $ per 1M tokens. */
  inputPerM: number;
  cachedInputPerM: number;
  cacheWritePerM: number;
  outputPerM: number;
  /** Supported reasoning effort levels. */
  efforts: string[];
  context: string;
  maxOutput: string;
  bestFor: string[];
  notes: string[];
  faq: Array<{ q: string; a: string }>;
  /** Related learn-article slugs. */
  related: string[];
};

export type CatalogModel = ClaudeModel | OpenAiModel;

export const DISCOUNT_FLAT = 0.5;

/** Price after the flat (50%) discount, formatted. */
export function priceHere(officialPerM: number): string {
  return formatUsd(officialPerM * (1 - DISCOUNT_FLAT));
}

export function formatUsd(value: number): string {
  // Three decimals keep official cache-write rates like $3.125 exact; trailing zeros strip.
  const rounded = Math.round(value * 1000) / 1000;
  return `$${rounded.toFixed(3).replace(/\.?0+$/, (m) => (m === ".000" ? "" : m.replace(/0+$/, "")))}`;
}

export const ANTHROPIC_BASE_URL = "https://api.apitoken.sale";
export const OPENAI_BASE_URL = "https://openai.api.apitoken.sale/v1";

export const claudeModels: ClaudeModel[] = [
  {
    provider: "anthropic",
    slug: "claude-opus-4-8",
    id: "claude-opus-4-8",
    name: "Claude Opus 4.8",
    tier: "Opus",
    title: "Claude Opus 4.8 API — Price per Token & Access",
    description: "Claude Opus 4.8 API pricing: official $5/$25 per 1M tokens, $2.50/$12.50 with the flat 50% apiToken.sale discount. Instant key, prepaid balance, same Anthropic API.",
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
      { q: "How much does the Claude Opus 4.8 API cost?", a: "Officially $5 per 1M input tokens and $25 per 1M output tokens. On apiToken.sale the same requests cost 50% less — $2.50/$12.50 at the flat discount applied to every call." },
      { q: "What is the model ID for Claude Opus 4.8?", a: "claude-opus-4-8. Use it unchanged with the Anthropic SDK, Claude Code, Cursor or any compatible tool pointed at https://api.apitoken.sale." },
      { q: "Is Opus 4.8 worth the price over Sonnet?", a: "For hard agentic and reasoning work, usually yes. For routine coding, Sonnet 5 delivers near-Opus quality at 40% of the token price — many teams route by task." },
    ],
    related: ["claude-opus-api", "best-claude-model-for-coding", "claude-api-pricing-explained", "cheapest-claude-api"],
  },
  {
    provider: "anthropic",
    slug: "claude-opus-4-7",
    id: "claude-opus-4-7",
    name: "Claude Opus 4.7",
    tier: "Opus",
    title: "Claude Opus 4.7 API — Price per Token & Access",
    description: "Claude Opus 4.7 API pricing: official $5/$25 per 1M tokens, $2.50/$12.50 with the flat 50% apiToken.sale discount. Same Anthropic endpoint, instant prepaid access.",
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
      { q: "How much does the Claude Opus 4.7 API cost?", a: "Officially $5 per 1M input tokens and $25 per 1M output tokens — the same as Opus 4.8. With the flat 50% apiToken.sale discount that is $2.50/$12.50." },
      { q: "Should I use Opus 4.7 or 4.8?", a: "They cost the same, so new projects should default to claude-opus-4-8. Keep 4.7 when you have prompts or evals pinned to it." },
      { q: "Does my key work for both?", a: "Yes — one apiToken.sale key and balance covers every supported Claude and GPT model; you switch by changing the model ID and endpoint." },
    ],
    related: ["claude-opus-api", "claude-api-pricing-explained", "claude-opus-vs-sonnet", "how-billing-works"],
  },
  {
    provider: "anthropic",
    slug: "claude-sonnet-5",
    id: "claude-sonnet-5",
    name: "Claude Sonnet 5",
    tier: "Sonnet",
    title: "Claude Sonnet 5 API — Price per Token & Access",
    description: "Claude Sonnet 5 API pricing: official $3/$15 per 1M tokens, $1.50/$7.50 with the flat 50% apiToken.sale discount. Near-Opus coding quality at Sonnet cost.",
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
      { q: "How much does the Claude Sonnet 5 API cost?", a: "The standard official rate is $3 per 1M input tokens and $15 per 1M output tokens (Anthropic lists an introductory $2/$10 through August 2026). apiToken.sale applies your flat 50% discount on top of official spend." },
      { q: "What is the model ID for Claude Sonnet 5?", a: "claude-sonnet-5 — use it as-is in the Anthropic SDK, Claude Code, Cursor, Cline or any compatible tool." },
      { q: "Is Sonnet 5 good enough for coding?", a: "For most coding it is the sweet spot: near-Opus quality on agentic and editing tasks at a much lower per-token price. Route only the hardest reasoning to Opus." },
    ],
    related: ["claude-sonnet-api", "best-claude-model-for-coding", "claude-opus-vs-sonnet", "save-tokens-on-claude-api"],
  },
  {
    provider: "anthropic",
    slug: "claude-sonnet-4-6",
    id: "claude-sonnet-4-6",
    name: "Claude Sonnet 4.6",
    tier: "Sonnet",
    title: "Claude Sonnet 4.6 API — Price per Token & Access",
    description: "Claude Sonnet 4.6 API pricing: official $3/$15 per 1M tokens, $1.50/$7.50 with the flat 50% apiToken.sale discount. Proven balanced model on the same Anthropic API.",
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
      { q: "How much does the Claude Sonnet 4.6 API cost?", a: "Officially $3 per 1M input tokens and $15 per 1M output tokens. With the flat 50% apiToken.sale discount that is $1.50/$7.50." },
      { q: "Sonnet 4.6 or Sonnet 5?", a: "They share a list price, and Sonnet 5 is stronger on coding and agentic work — prefer it for new projects. Stay on 4.6 when your prompts and evals are pinned to it." },
      { q: "Can I switch models without a new key?", a: "Yes. One key and one prepaid balance cover every supported Claude and GPT model — switching is a model-ID and endpoint change." },
    ],
    related: ["claude-sonnet-api", "claude-3-5-vs-claude-4", "claude-api-pricing-explained", "how-billing-works"],
  },
  {
    provider: "anthropic",
    slug: "claude-haiku-4-5",
    id: "claude-haiku-4-5",
    name: "Claude Haiku 4.5",
    tier: "Haiku",
    title: "Claude Haiku 4.5 API — Price per Token & Access",
    description: "Claude Haiku 4.5 API pricing: official $1/$5 per 1M tokens, $0.50/$2.50 with the flat 50% apiToken.sale discount. The cheapest and fastest Claude model.",
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
      { q: "How much does the Claude Haiku 4.5 API cost?", a: "Officially $1 per 1M input tokens and $5 per 1M output tokens. With the flat 50% apiToken.sale discount that is $0.50/$2.50 — the cheapest way to run Claude." },
      { q: "What is Haiku 4.5 good for?", a: "High-volume, low-latency work: classification, extraction, summarization, routing and simple chat. For complex reasoning, step up to Sonnet 5 or Opus 4.8." },
      { q: "What is the model ID?", a: "claude-haiku-4-5. It works on the same apiToken.sale key and balance as every other supported Claude and GPT model." },
    ],
    related: ["claude-haiku-api", "save-tokens-on-claude-api", "cheapest-claude-api", "best-claude-model-for-coding"],
  },
];

// GPT rates mirror the pinned engine catalog (crates/metering/src/codex.rs): official OpenAI
// standard token pricing, cached input at 10% of input, cache write at 125% of input for the
// 5.6 line and 100% for 5.5/5.4, and long-context pricing (2× input, 1.5× output on the whole
// request) above 272K input tokens. gpt-5.6 is a convenience alias of gpt-5.6-sol and is
// deliberately not listed as a separate page — one canonical pricing identity per model.
export const openaiModels: OpenAiModel[] = [
  {
    provider: "openai",
    slug: "gpt-5-6-sol",
    id: "gpt-5.6-sol",
    name: "GPT-5.6 Sol",
    tier: "Flagship",
    title: "GPT-5.6 Sol API — Price per Token & Access",
    description: "GPT-5.6 Sol API pricing: official $5/$30 per 1M tokens, $2.50/$15 with the flat 50% apiToken.sale discount. OpenAI-compatible endpoint, one key, prepaid balance.",
    keywords: ["gpt-5.6 api", "gpt-5.6 sol price", "gpt-5.6 api cost", "gpt-5.6-sol", "gpt-5.6 token pricing", "openai compatible api"],
    dek: "GPT-5.6 Sol is the flagship of the GPT-5.6 line — the strongest reasoning and agentic coding model on the OpenAI-compatible endpoint. Here is what it costs per token, and how to run it cheaper on the same key and balance.",
    inputPerM: 5,
    cachedInputPerM: 0.5,
    cacheWritePerM: 6.25,
    outputPerM: 30,
    efforts: ["none", "low", "medium", "high", "xhigh", "max"],
    context: "272K tokens",
    maxOutput: "32K tokens",
    bestFor: [
      "Agentic coding in Codex CLI, opencode and OpenAI-compatible tools.",
      "Hard reasoning with adjustable effort, up to the max level.",
      "Long multi-turn sessions with cached-input pricing.",
    ],
    notes: [
      "gpt-5.6 is a convenience alias of gpt-5.6-sol — the same model at the same price.",
      "Text and image input, text output. SSE streaming on both Responses and Chat Completions.",
      "Requests above 272K input tokens bill at OpenAI long-context rates: 2× input and 1.5× output on the whole request.",
    ],
    faq: [
      { q: "How much does the GPT-5.6 Sol API cost?", a: "Officially $5 per 1M input tokens and $30 per 1M output tokens, with cached input at $0.50. On apiToken.sale the same requests cost 50% less — $2.50/$15 at the flat discount applied to every call." },
      { q: "What is the model ID for GPT-5.6 Sol?", a: "gpt-5.6-sol (gpt-5.6 is an alias of the same model). Use it with the OpenAI SDK, Codex CLI, opencode or any OpenAI-compatible tool pointed at https://openai.api.apitoken.sale/v1." },
      { q: "Does the same key really work for GPT and Claude?", a: "Yes. One sk-pool key and one prepaid balance cover both surfaces: Anthropic Messages API for Claude models and the OpenAI-compatible API for GPT models. The same discount applies to both." },
    ],
    related: ["openai-api-quickstart", "codex-cli-setup", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "openai",
    slug: "gpt-5-6-terra",
    id: "gpt-5.6-terra",
    name: "GPT-5.6 Terra",
    tier: "Balanced",
    title: "GPT-5.6 Terra API — Price per Token & Access",
    description: "GPT-5.6 Terra API pricing: official $2.50/$15 per 1M tokens, $1.25/$7.50 with the flat 50% apiToken.sale discount. Balanced GPT-5.6 tier on one prepaid balance.",
    keywords: ["gpt-5.6 terra api", "gpt-5.6 terra price", "gpt-5.6-terra", "gpt-5.6 token pricing", "openai compatible api"],
    dek: "GPT-5.6 Terra is the balanced tier of the GPT-5.6 line — half the flagship token price, with the same reasoning-effort controls and the full 272K context.",
    inputPerM: 2.5,
    cachedInputPerM: 0.25,
    cacheWritePerM: 3.125,
    outputPerM: 15,
    efforts: ["none", "low", "medium", "high", "xhigh", "max"],
    context: "272K tokens",
    maxOutput: "32K tokens",
    bestFor: [
      "Day-to-day coding and chat at half the flagship price.",
      "Agentic workflows where flagship cost is not justified.",
      "High-volume production traffic on the OpenAI-compatible API.",
    ],
    notes: [
      "Same reasoning-effort range as the flagship, including max.",
      "Text and image input, text output. SSE streaming on both Responses and Chat Completions.",
      "Requests above 272K input tokens bill at OpenAI long-context rates: 2× input and 1.5× output on the whole request.",
    ],
    faq: [
      { q: "How much does the GPT-5.6 Terra API cost?", a: "Officially $2.50 per 1M input tokens and $15 per 1M output tokens, with cached input at $0.25. With the flat 50% apiToken.sale discount that is $1.25/$7.50." },
      { q: "Terra or Sol?", a: "Terra is the balanced default for most workloads at half the price; route the hardest reasoning to gpt-5.6-sol. Both run on the same key, balance and endpoint." },
      { q: "What is the model ID?", a: "gpt-5.6-terra. Point any OpenAI-compatible client at https://openai.api.apitoken.sale/v1 and send it as the model parameter." },
    ],
    related: ["openai-api-quickstart", "codex-cli-setup", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "openai",
    slug: "gpt-5-6-luna",
    id: "gpt-5.6-luna",
    name: "GPT-5.6 Luna",
    tier: "Fast",
    title: "GPT-5.6 Luna API — Price per Token & Access",
    description: "GPT-5.6 Luna API pricing: official $1/$6 per 1M tokens, $0.50/$3 with the flat 50% apiToken.sale discount. The fastest, cheapest GPT-5.6 tier.",
    keywords: ["gpt-5.6 luna api", "gpt-5.6 luna price", "gpt-5.6-luna", "cheapest gpt model", "openai compatible api"],
    dek: "GPT-5.6 Luna is the fast, economical tier of the GPT-5.6 line — built for high-volume, latency-sensitive work at one fifth of the flagship price.",
    inputPerM: 1,
    cachedInputPerM: 0.1,
    cacheWritePerM: 1.25,
    outputPerM: 6,
    efforts: ["none", "low", "medium", "high", "xhigh", "max"],
    context: "272K tokens",
    maxOutput: "32K tokens",
    bestFor: [
      "Classification, extraction and summarization at scale.",
      "Latency-sensitive chat and routing layers.",
      "Cheap pre-processing before a Sol or Terra call.",
    ],
    notes: [
      "Same reasoning-effort range as the flagship, including max.",
      "Text and image input, text output. SSE streaming on both Responses and Chat Completions.",
      "Requests above 272K input tokens bill at OpenAI long-context rates: 2× input and 1.5× output on the whole request.",
    ],
    faq: [
      { q: "How much does the GPT-5.6 Luna API cost?", a: "Officially $1 per 1M input tokens and $6 per 1M output tokens, with cached input at $0.10. With the flat 50% apiToken.sale discount that is $0.50/$3 — the cheapest way to run GPT-5.6." },
      { q: "What is Luna good for?", a: "High-volume, low-latency work: classification, extraction, summarization, routing and simple chat. For complex reasoning, step up to Terra or Sol." },
      { q: "What is the model ID?", a: "gpt-5.6-luna. It works on the same apiToken.sale key, balance and OpenAI-compatible endpoint as every other GPT model." },
    ],
    related: ["openai-api-quickstart", "codex-cli-setup", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "openai",
    slug: "gpt-5-5",
    id: "gpt-5.5",
    name: "GPT-5.5",
    tier: "Flagship",
    title: "GPT-5.5 API — Price per Token & Access",
    description: "GPT-5.5 API pricing: official $5/$30 per 1M tokens, $2.50/$15 with the flat 50% apiToken.sale discount. Previous-generation flagship on the OpenAI-compatible endpoint.",
    keywords: ["gpt-5.5 api", "gpt-5.5 price", "gpt-5.5 api cost", "gpt-5.5 token pricing", "openai compatible api"],
    dek: "GPT-5.5 is the previous-generation flagship — pinned for workloads evaluated against it, at the same list price as GPT-5.6 Sol.",
    inputPerM: 5,
    cachedInputPerM: 0.5,
    cacheWritePerM: 5,
    outputPerM: 30,
    efforts: ["none", "low", "medium", "high", "xhigh"],
    context: "272K tokens",
    maxOutput: "32K tokens",
    bestFor: [
      "Workloads pinned to GPT-5.5 for reproducibility.",
      "Agentic coding and multi-step reasoning.",
      "Pipelines migrating gradually to the GPT-5.6 line.",
    ],
    notes: [
      "Reasoning efforts none through xhigh; the max level is exclusive to the GPT-5.6 line.",
      "Same list price as gpt-5.6-sol — most new work should target the 5.6 line.",
      "Requests above 272K input tokens bill at OpenAI long-context rates: 2× input and 1.5× output on the whole request.",
    ],
    faq: [
      { q: "How much does the GPT-5.5 API cost?", a: "Officially $5 per 1M input tokens and $30 per 1M output tokens — the same as GPT-5.6 Sol. With the flat 50% apiToken.sale discount that is $2.50/$15." },
      { q: "GPT-5.5 or GPT-5.6 Sol?", a: "They cost the same, so new projects should default to gpt-5.6-sol. Keep 5.5 when you have prompts or evals pinned to it." },
      { q: "What is the model ID?", a: "gpt-5.5 — use it as-is on the OpenAI-compatible endpoint at https://openai.api.apitoken.sale/v1." },
    ],
    related: ["openai-api-quickstart", "codex-cli-setup", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "openai",
    slug: "gpt-5-4",
    id: "gpt-5.4",
    name: "GPT-5.4",
    tier: "Balanced",
    title: "GPT-5.4 API — Price per Token & Access",
    description: "GPT-5.4 API pricing: official $2.50/$15 per 1M tokens, $1.25/$7.50 with the flat 50% apiToken.sale discount. Proven balanced tier on one prepaid balance.",
    keywords: ["gpt-5.4 api", "gpt-5.4 price", "gpt-5.4 api cost", "gpt-5.4 token pricing", "openai compatible api"],
    dek: "GPT-5.4 is the proven balanced tier of the previous generation — a workhorse for coding and production pipelines, at the same list price as GPT-5.6 Terra.",
    inputPerM: 2.5,
    cachedInputPerM: 0.25,
    cacheWritePerM: 2.5,
    outputPerM: 15,
    efforts: ["none", "low", "medium", "high", "xhigh"],
    context: "272K tokens",
    maxOutput: "32K tokens",
    bestFor: [
      "Pipelines tuned and evaluated against GPT-5.4.",
      "Balanced coding and content workloads.",
      "Teams migrating gradually to GPT-5.6 Terra.",
    ],
    notes: [
      "Reasoning efforts none through xhigh; the max level is exclusive to the GPT-5.6 line.",
      "Same list price as gpt-5.6-terra — new projects should usually start there.",
      "Requests above 272K input tokens bill at OpenAI long-context rates: 2× input and 1.5× output on the whole request.",
    ],
    faq: [
      { q: "How much does the GPT-5.4 API cost?", a: "Officially $2.50 per 1M input tokens and $15 per 1M output tokens. With the flat 50% apiToken.sale discount that is $1.25/$7.50." },
      { q: "GPT-5.4 or GPT-5.6 Terra?", a: "They share a list price, and Terra is the newer balanced tier — prefer it for new projects. Stay on 5.4 when your prompts and evals are pinned to it." },
      { q: "What is the model ID?", a: "gpt-5.4. One apiToken.sale key and balance covers it alongside every other Claude and GPT model." },
    ],
    related: ["openai-api-quickstart", "codex-cli-setup", "how-billing-works", "why-choose-apitoken"],
  },
];

export const claudeModelBySlug: Record<string, ClaudeModel> = Object.fromEntries(
  claudeModels.map((model) => [model.slug, model]),
);

export const openaiModelBySlug: Record<string, OpenAiModel> = Object.fromEntries(
  openaiModels.map((model) => [model.slug, model]),
);

export const catalogModelBySlug: Record<string, CatalogModel> = { ...claudeModelBySlug, ...openaiModelBySlug };

export function modelPath(slug: string): string {
  return `/models/${slug}`;
}

export const MODELS_HUB_PATH = "/models";
