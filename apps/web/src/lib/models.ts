// Model catalog for the /models programmatic SEO pages — three providers, one key and balance.
// Claude prices are official Anthropic per-million-token rates; GPT prices are official OpenAI
// per-million-token rates from the pinned engine catalog (crates/metering/src/codex.rs); Gemini
// prices are official Google per-million-token rates from crates/metering/src/gemini.rs. The
// discounted price shown to users derives from the flat B2C pricing model (50% off official
// spend on every request) and applies to all providers through the same account multiplier.
// Keep numbers in sync with the providers' price lists and the engine catalog.

export type ModelProvider = "anthropic" | "openai" | "gemini";

export type ClaudeModel = {
  provider: "anthropic";
  /** URL slug under /models/. */
  slug: string;
  /** Exact API model ID. */
  id: string;
  name: string;
  tier: "Mythos" | "Opus" | "Sonnet" | "Haiku";
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
  tier: "Flagship" | "Balanced" | "Fast" | "Image";
  /** <title> without brand suffix. */
  title: string;
  description: string;
  keywords: string[];
  dek: string;
  /** Official OpenAI $ per 1M tokens. */
  inputPerM: number;
  cachedInputPerM: number;
  /** Zero on image models: the image wire has no cache-write billing. */
  cacheWritePerM: number;
  /** Text models: per 1M text-output tokens. Image models: per 1M image-output tokens. */
  outputPerM: number;
  /** Official $ per 1M image-input tokens — image models only. */
  imageInputPerM?: number;
  /** Official $ per 1M image-output tokens — image models only. */
  imageOutputPerM?: number;
  /** Supported reasoning effort levels — text models only. */
  efforts?: string[];
  context: string;
  maxOutput: string;
  bestFor: string[];
  notes: string[];
  faq: Array<{ q: string; a: string }>;
  /** Related learn-article slugs. */
  related: string[];
};

export type GeminiModel = {
  provider: "gemini";
  /** URL slug under /models/. */
  slug: string;
  /** Exact API model ID on the native Gemini endpoint. */
  id: string;
  name: string;
  tier: "Pro" | "Flash" | "Flash-Lite" | "Image";
  /** <title> without brand suffix. */
  title: string;
  description: string;
  keywords: string[];
  dek: string;
  /** Official Google $ per 1M tokens. */
  inputPerM: number;
  cachedInputPerM: number;
  outputPerM: number;
  /** Long-context rates (whole request) above the input threshold — 3.1 Pro Preview only. */
  longContext?: { threshold: string; inputPerM: number; cachedInputPerM: number; outputPerM: number };
  /** Official $ per 1M image-output tokens — image models only. */
  imageOutputPerM?: number;
  context: string;
  maxOutput: string;
  bestFor: string[];
  notes: string[];
  faq: Array<{ q: string; a: string }>;
  /** Related learn-article slugs. */
  related: string[];
};


export type KimiModel = {
  provider: "kimi";
  slug: string;
  /** Subscription alias — exactly what a client sends. Official Open Platform ids are tariff
   *  keys the gateway refuses on the wire, so they are deliberately not published here. */
  id: string;
  name: string;
  tier: "K3" | "Coding";
  title: string;
  description: string;
  keywords: string[];
  dek: string;
  /** Official Moonshot $ per 1M tokens, reviewed against platform.kimi.ai. */
  inputPerM: number;
  cachedInputPerM: number;
  outputPerM: number;
  /** KIMI publishes no separate cache-write rate: a write is a miss, so this equals input. */
  cacheWritePerM: number;
  context: string;
  maxOutput: string;
  bestFor: string[];
  notes: string[];
  faq: Array<{ q: string; a: string }>;
  related: string[];
};

export type CatalogModel = ClaudeModel | OpenAiModel | GeminiModel | KimiModel;

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

// Unified router endpoint — the recommended entry point: native Anthropic,
// OpenAI and Gemini lanes plus the OpenAI-compatible universal lane on one host.
// The legacy per-provider hosts below keep serving existing integrations.
export const ROUTER_BASE_URL = "https://router.apitoken.sale";
export const ROUTER_OPENAI_BASE_URL = "https://router.apitoken.sale/v1";
export const ANTHROPIC_BASE_URL = "https://api.apitoken.sale";
export const OPENAI_BASE_URL = "https://openai.api.apitoken.sale/v1";
export const GEMINI_BASE_URL = "https://gemini.api.apitoken.sale";

export const claudeModels: ClaudeModel[] = [
  {
    provider: "anthropic",
    slug: "claude-opus-5",
    id: "claude-opus-5",
    name: "Claude Opus 5",
    tier: "Opus",
    title: "Claude Opus 5 API — Price per Token & Access",
    description: "Claude Opus 5 API pricing: official $5/$25 per 1M tokens, $2.50/$12.50 with the flat 50% apiToken.sale discount. The newest Opus on the same Anthropic API.",
    keywords: ["claude opus 5 api", "claude opus 5 price", "claude opus 5 api cost", "opus 5 token pricing", "claude-opus-5", "buy claude opus api"],
    dek: "Claude Opus 5 is Anthropic's newest Opus — the July 2026 flagship for agentic coding, long-horizon tasks and hard reasoning, at the same token price as Opus 4.8.",
    inputPerM: 5,
    outputPerM: 25,
    cacheReadPerM: 0.5,
    cacheWrite5mPerM: 6.25,
    context: "1M tokens",
    maxOutput: "128K tokens",
    bestFor: [
      "Agentic coding in Claude Code, Cursor and Cline — the new default.",
      "Long-horizon autonomous tasks and complex refactors.",
      "The hardest reasoning, planning and review work.",
    ],
    notes: [
      "Same $5/$25 tariff as Opus 4.8 — a capability upgrade at no price change.",
      "Fast mode (research preview, ~2.5× speed) bills $10/$50 per 1M on requests that use it.",
      "Adaptive thinking is the recommended mode; thinking tokens bill as output.",
    ],
    faq: [
      { q: "How much does the Claude Opus 5 API cost?", a: "Officially $5 per 1M input tokens and $25 per 1M output tokens — unchanged from Opus 4.8. On apiToken.sale the same requests cost 50% less: $2.50/$12.50 at the flat discount applied to every call." },
      { q: "What is the model ID for Claude Opus 5?", a: "claude-opus-5. Use it unchanged with the Anthropic SDK, Claude Code, Cursor or any compatible tool pointed at https://router.apitoken.sale." },
      { q: "Opus 5 or Fable 5?", a: "Opus 5 is the default for almost everything — top-tier agentic quality at half the Fable token price. Route to Fable 5 only the longest-horizon runs where its edge is worth 2× the tokens." },
    ],
    related: ["claude-opus-api", "best-claude-model-for-coding", "claude-api-pricing-explained", "cheapest-claude-api"],
  },
  {
    provider: "anthropic",
    slug: "claude-fable-5",
    id: "claude-fable-5",
    name: "Claude Fable 5",
    tier: "Mythos",
    title: "Claude Fable 5 API — Price per Token & Access",
    description: "Claude Fable 5 API pricing: official $10/$50 per 1M tokens, $5/$25 with the flat 50% apiToken.sale discount. Mythos-class — the strongest Claude tier.",
    keywords: ["claude fable 5 api", "claude fable 5 price", "fable 5 api cost", "claude-fable-5", "mythos class model", "fable 5 token pricing"],
    dek: "Claude Fable 5 is Anthropic's Mythos-class model — a tier above Opus for the longest-horizon agentic work, at double the Opus token price.",
    inputPerM: 10,
    outputPerM: 50,
    cacheReadPerM: 1,
    cacheWrite5mPerM: 12.5,
    context: "1M tokens",
    maxOutput: "128K tokens",
    bestFor: [
      "The longest-horizon autonomous runs where failure costs more than tokens.",
      "The hardest software tasks — it leads SWE-bench Pro.",
      "An orchestrator or advisor role reviewing and steering cheaper models.",
    ],
    notes: [
      "Mythos class sits above Opus: $10/$50 vs the Opus $5/$25 tariff.",
      "Safety classifiers may reroute sensitive requests to Opus.",
      "Adaptive thinking is the recommended mode; thinking tokens bill as output.",
    ],
    faq: [
      { q: "How much does the Claude Fable 5 API cost?", a: "Officially $10 per 1M input tokens and $50 per 1M output tokens — double the Opus tariff. On apiToken.sale the flat 50% discount applies to every call: $5/$25." },
      { q: "What is the model ID for Claude Fable 5?", a: "claude-fable-5. Use it unchanged with the Anthropic SDK, Claude Code, Cursor or any compatible tool pointed at https://router.apitoken.sale." },
      { q: "Fable 5 or Mythos 5?", a: "They share weights and price; Mythos 5 stays restricted to Project Glasswing partners. Fable 5 is the publicly available model, and the one served here." },
    ],
    related: ["claude-opus-api", "best-claude-model-for-coding", "claude-api-pricing-explained", "cheapest-claude-api"],
  },
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
      { q: "What is the model ID for Claude Opus 4.8?", a: "claude-opus-4-8. Use it unchanged with the Anthropic SDK, Claude Code, Cursor or any compatible tool pointed at https://router.apitoken.sale." },
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
// request) above 272K input tokens. Terra and Luna carry the post-2026-07-30 official rates
// ($2/$12 and $0.20/$1.20); gpt-5.6-sol is unchanged. gpt-5.6 is a convenience alias of
// gpt-5.6-sol and is deliberately not listed as a separate page — one canonical pricing
// identity per model.
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
    context: "400K tokens",
    maxOutput: "128K tokens",
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
      { q: "What is the model ID for GPT-5.6 Sol?", a: "gpt-5.6-sol (gpt-5.6 is an alias of the same model). Use it with the OpenAI SDK, Codex CLI, opencode or any OpenAI-compatible tool pointed at https://router.apitoken.sale/v1." },
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
    description: "GPT-5.6 Terra API pricing: official $2/$12 per 1M tokens, $1/$6 with the flat 50% apiToken.sale discount. Balanced GPT-5.6 tier on one prepaid balance.",
    keywords: ["gpt-5.6 terra api", "gpt-5.6 terra price", "gpt-5.6-terra", "gpt-5.6 token pricing", "openai compatible api"],
    dek: "GPT-5.6 Terra is the balanced tier of the GPT-5.6 line — 40% of the flagship token price, with the same reasoning-effort controls and the full 400K context.",
    inputPerM: 2,
    cachedInputPerM: 0.2,
    cacheWritePerM: 2.5,
    outputPerM: 12,
    efforts: ["none", "low", "medium", "high", "xhigh", "max"],
    context: "400K tokens",
    maxOutput: "128K tokens",
    bestFor: [
      "Day-to-day coding and chat at 40% of the flagship price.",
      "Agentic workflows where flagship cost is not justified.",
      "High-volume production traffic on the OpenAI-compatible API.",
    ],
    notes: [
      "Same reasoning-effort range as the flagship, including max.",
      "Text and image input, text output. SSE streaming on both Responses and Chat Completions.",
      "Requests above 272K input tokens bill at OpenAI long-context rates: 2× input and 1.5× output on the whole request.",
    ],
    faq: [
      { q: "How much does the GPT-5.6 Terra API cost?", a: "Officially $2 per 1M input tokens and $12 per 1M output tokens, with cached input at $0.20. With the flat 50% apiToken.sale discount that is $1/$6." },
      { q: "Terra or Sol?", a: "Terra is the balanced default for most workloads at 40% of the price; route the hardest reasoning to gpt-5.6-sol. Both run on the same key, balance and endpoint." },
      { q: "What is the model ID?", a: "gpt-5.6-terra. Point any OpenAI-compatible client at https://router.apitoken.sale/v1 and send it as the model parameter." },
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
    description: "GPT-5.6 Luna API pricing: official $0.20/$1.20 per 1M tokens, $0.10/$0.60 with the flat 50% apiToken.sale discount. The fastest, cheapest GPT-5.6 tier.",
    keywords: ["gpt-5.6 luna api", "gpt-5.6 luna price", "gpt-5.6-luna", "cheapest gpt model", "openai compatible api"],
    dek: "GPT-5.6 Luna is the fast, economical tier of the GPT-5.6 line — built for high-volume, latency-sensitive work at a fraction of the flagship price.",
    inputPerM: 0.2,
    cachedInputPerM: 0.02,
    cacheWritePerM: 0.25,
    outputPerM: 1.2,
    efforts: ["none", "low", "medium", "high", "xhigh", "max"],
    context: "400K tokens",
    maxOutput: "128K tokens",
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
      { q: "How much does the GPT-5.6 Luna API cost?", a: "Officially $0.20 per 1M input tokens and $1.20 per 1M output tokens, with cached input at $0.02. With the flat 50% apiToken.sale discount that is $0.10/$0.60 — the cheapest way to run GPT-5.6." },
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
    context: "400K tokens",
    maxOutput: "128K tokens",
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
      { q: "What is the model ID?", a: "gpt-5.5 — use it as-is on the OpenAI-compatible endpoint at https://router.apitoken.sale/v1." },
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
    dek: "GPT-5.4 is the proven balanced tier of the previous generation — a workhorse for coding and production pipelines, priced just above the newer GPT-5.6 Terra.",
    inputPerM: 2.5,
    cachedInputPerM: 0.25,
    cacheWritePerM: 2.5,
    outputPerM: 15,
    efforts: ["none", "low", "medium", "high", "xhigh"],
    context: "400K tokens",
    maxOutput: "128K tokens",
    bestFor: [
      "Pipelines tuned and evaluated against GPT-5.4.",
      "Balanced coding and content workloads.",
      "Teams migrating gradually to GPT-5.6 Terra.",
    ],
    notes: [
      "Reasoning efforts none through xhigh; the max level is exclusive to the GPT-5.6 line.",
      "gpt-5.6-terra is now the cheaper balanced tier ($2/$12 vs $2.50/$15) — new projects should usually start there.",
      "Requests above 272K input tokens bill at OpenAI long-context rates: 2× input and 1.5× output on the whole request.",
    ],
    faq: [
      { q: "How much does the GPT-5.4 API cost?", a: "Officially $2.50 per 1M input tokens and $15 per 1M output tokens. With the flat 50% apiToken.sale discount that is $1.25/$7.50." },
      { q: "GPT-5.4 or GPT-5.6 Terra?", a: "Terra is newer and cheaper ($2/$12 vs $2.50/$15) — prefer it for new projects. Stay on 5.4 when your prompts and evals are pinned to it." },
      { q: "What is the model ID?", a: "gpt-5.4. One apiToken.sale key and balance covers it alongside every other Claude and GPT model." },
    ],
    related: ["openai-api-quickstart", "codex-cli-setup", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "openai",
    slug: "gpt-image-2",
    id: "gpt-image-2",
    name: "GPT Image 2",
    tier: "Image",
    title: "GPT Image 2 API — Price per Token & Access",
    description: "GPT Image 2 API pricing: official $5 per 1M text input tokens and $30 per 1M image output tokens, $2.50/$15 with the flat 50% apiToken.sale discount. Generation and edits on the same key and prepaid balance.",
    keywords: ["gpt image 2 api", "gpt-image-2 price", "openai image generation api cost", "gpt-image-2-2026-04-21", "gpt image edit api", "openai images api"],
    dek: "GPT Image 2 is OpenAI's image generation and editing model — text and reference images in, rendered images out, metered per token on the same prepaid balance as every GPT and Claude model here.",
    inputPerM: 5,
    cachedInputPerM: 1.25,
    cacheWritePerM: 0,
    outputPerM: 30,
    imageInputPerM: 8,
    imageOutputPerM: 30,
    context: "per request",
    maxOutput: "1 image",
    bestFor: [
      "Product image generation from a prompt on a bounded low-cost profile.",
      "Image editing against one reference PNG.",
      "Pipelines that already run GPT or Claude here — images debit the same balance.",
    ],
    notes: [
      "gpt-image-2 is the alias of the immutable snapshot gpt-image-2-2026-04-21 — the same model at the same price.",
      "Generation is POST /v1/images/generations, editing is POST /v1/images/edits with up to five reference PNGs — on the unified router (router.apitoken.sale/v1) or the legacy OpenAI host; one non-streaming PNG per call.",
      "The shipped contract is deliberately narrow: omit background/quality/size, or send only background=opaque, quality=low, size=auto — an explicit \"auto\" for background or quality is rejected with 400. The upstream subscription wire normalizes explicit sizes, so exact dimensions are not advertised.",
      "Image output bills per image-output token; cached text/image input bills at 25% of the fresh rate.",
    ],
    faq: [
      { q: "How much does the GPT Image 2 API cost?", a: "Officially $5 per 1M text input tokens, $8 per 1M image input tokens and $30 per 1M image output tokens (cached input at 25%). On apiToken.sale the same calls cost 50% less — $2.50/$4/$15 — at the flat discount applied to every call." },
      { q: "What is the model ID for GPT Image 2?", a: "gpt-image-2, an alias of the immutable snapshot gpt-image-2-2026-04-21. Send it as the model field of POST /v1/images/generations or /v1/images/edits on https://router.apitoken.sale/v1 (the legacy https://openai.api.apitoken.sale/v1 serves the same routes) with your Bearer key." },
      { q: "Does the same balance really cover image generation?", a: "Yes. GPT Image 2 debits the same prepaid balance as every Claude, GPT and Gemini model on the account — no separate image plan or key." },
    ],
    related: ["gpt-image-2-api-guide", "gpt-image-2-api-cost", "nano-banana-2-vs-gpt-image-2", "image-editing-api-guide"],
  },
];

// Gemini rates mirror the pinned engine catalog (crates/metering/src/gemini.rs): official
// Google standard paid-tier token pricing, cached input at 10% of input (exception:
// gemini-3.1-flash-image has no cache discount — cached input bills at the full input rate),
// no cache-write billing, and long-context pricing (2× input, 1.5× output on the whole
// request) above 200K input tokens on gemini-3.1-pro-preview. gemini-3.1-flash-image (Nano
// Banana 2) bills image output separately per image-output token. The native /v1beta
// generateContent surface is served as-is, authenticated with x-goog-api-key.

export const kimiModels: KimiModel[] = [
  {
    provider: "kimi",
    slug: "kimi-k3",
    id: "k3",
    name: "Kimi K3",
    tier: "K3",
    title: "Kimi K3 API — Price per Token & Access",
    description: "Kimi K3 API pricing: official $3.00/$15.00 per 1M tokens, $1.50/$7.50 with the flat 50% apiToken.sale discount. Moonshot's frontier model with a 1M-token window.",
    keywords: ["kimi k3 api", "kimi k3 price", "kimi k3 api cost", "k3 model id", "moonshot kimi pricing", "kimi api"],
    dek: "Kimi K3 is Moonshot's frontier model — a 1M-token window for agentic coding and long-context work, addressed as k3 on the Anthropic Messages lane.",
    inputPerM: 3,
    cachedInputPerM: 0.3,
    outputPerM: 15,
    cacheWritePerM: 3,
    context: "1M tokens",
    maxOutput: "not published",
    bestFor: [
      "Agentic coding sessions that outgrow a 256K window.",
      "Long-context analysis across a whole repository or corpus.",
      "Work already configured for Claude Code, which selects this window as k3[1m].",
    ],
    notes: [
      "Cached input bills at 10% of input — caching is automatic on repeated prefixes.",
      "KIMI publishes no separate cache-write rate: a write is a cache miss and bills at the input rate.",
    ],
    faq: [
      { q: "How much does the Kimi K3 API cost?", a: "Officially $3.00 per 1M input tokens and $15.00 per 1M output tokens, with cached input at $0.30. On apiToken.sale the same requests cost 50% less — $1.50/$7.50 at the flat discount applied to every call." },
      { q: "What is the model ID for Kimi K3?", a: "k3, or kimi/k3 on the unified router. Claude Code users can also send k3[1m], which is that client's spelling for the same 1M window — both settle identically." },
      { q: "Is there a 256K variant?", a: "Yes: k3-256k is the same model and the same rate card with a 256K accepted window. Choose it when a smaller window is what your harness expects." },
    ],
    related: ["openai-api-quickstart", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "kimi",
    slug: "kimi-k3-256k",
    id: "k3-256k",
    name: "Kimi K3 (256K)",
    tier: "K3",
    title: "Kimi K3 256K API — Price per Token & Access",
    description: "Kimi K3 with a 256K window: official $3.00/$15.00 per 1M tokens, $1.50/$7.50 with the flat 50% apiToken.sale discount. Same model and same rates as k3, smaller accepted context.",
    keywords: ["kimi k3 256k", "k3-256k model id", "kimi k3 context window", "kimi api price", "moonshot kimi 256k"],
    dek: "The same Kimi K3 at the same rates, with a 256K accepted window — for harnesses that expect a smaller context than the 1M variant.",
    inputPerM: 3,
    cachedInputPerM: 0.3,
    outputPerM: 15,
    cacheWritePerM: 3,
    context: "256K tokens",
    maxOutput: "not published",
    bestFor: [
      "Tools that cap or mis-handle a 1M-token window.",
      "Sessions where a smaller window keeps compaction predictable.",
    ],
    notes: [
      "Cached input bills at 10% of input — caching is automatic on repeated prefixes.",
      "KIMI publishes no separate cache-write rate: a write is a cache miss and bills at the input rate.",
    ],
    faq: [
      { q: "Does k3-256k cost less than k3?", a: "No. It is the same model on the same rate card — $3.00/$15.00 officially, $1.50/$7.50 here. Only the accepted context differs." },
      { q: "When should I pick it over k3?", a: "When your client compacts against the window it is told about: a harness configured for 256K will behave more predictably on this id than on the 1M one." },
    ],
    related: ["how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "kimi",
    slug: "kimi-for-coding",
    id: "kimi-for-coding",
    name: "Kimi for Coding",
    tier: "Coding",
    title: "Kimi for Coding API — Price per Token & Access",
    description: "Kimi for Coding API pricing: official $0.95/$4.00 per 1M tokens, $0.48/$2.00 with the flat 50% apiToken.sale discount. Moonshot's coding SKU with a 256K window.",
    keywords: ["kimi for coding api", "kimi for coding price", "kimi coding model", "kimi k2.7 code", "moonshot coding api"],
    dek: "Kimi for Coding is Moonshot's coding SKU — a stable alias that tracks the current coding model, at roughly a third of the K3 token price.",
    inputPerM: 0.95,
    cachedInputPerM: 0.19,
    outputPerM: 4,
    cacheWritePerM: 0.95,
    context: "256K tokens",
    maxOutput: "not published",
    bestFor: [
      "High-volume coding work where K3 token prices dominate the bill.",
      "Setups that want a stable id rather than a pinned model version.",
    ],
    notes: [
      "Cached input bills at 10% of input — caching is automatic on repeated prefixes.",
      "KIMI publishes no separate cache-write rate: a write is a cache miss and bills at the input rate.",
    ],
    faq: [
      { q: "How much does Kimi for Coding cost?", a: "Officially $0.95 per 1M input tokens and $4.00 per 1M output tokens, with cached input at $0.19. On apiToken.sale that is $0.48/$2.00 at the flat 50% discount." },
      { q: "Which model does it actually run?", a: "It is a stable alias maintained by Moonshot that resolves to the current coding model. Pinning a dated id instead is what causes silent retry loops in third-party tools." },
    ],
    related: ["how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "kimi",
    slug: "kimi-for-coding-highspeed",
    id: "kimi-for-coding-highspeed",
    name: "Kimi for Coding HighSpeed",
    tier: "Coding",
    title: "Kimi for Coding HighSpeed API — Price per Token & Access",
    description: "Kimi for Coding HighSpeed API pricing: official $1.90/$8.00 per 1M tokens, $0.95/$4.00 with the flat 50% apiToken.sale discount. Exactly double the base coding SKU on every leg.",
    keywords: ["kimi for coding highspeed", "kimi highspeed price", "kimi coding fast api", "moonshot highspeed"],
    dek: "The faster tier of the coding SKU, priced at exactly double the base model on every leg — input, cached input and output alike.",
    inputPerM: 1.9,
    cachedInputPerM: 0.38,
    outputPerM: 8,
    cacheWritePerM: 1.9,
    context: "256K tokens",
    maxOutput: "not published",
    bestFor: [
      "Interactive sessions where latency matters more than token price.",
      "Short agent turns that are dominated by time to first token.",
    ],
    notes: [
      "Cached input bills at 10% of input — caching is automatic on repeated prefixes.",
      "KIMI publishes no separate cache-write rate: a write is a cache miss and bills at the input rate.",
    ],
    faq: [
      { q: "How much more does HighSpeed cost?", a: "Exactly 2× the base coding SKU on every leg: $1.90/$8.00 officially against $0.95/$4.00, and the same doubling on cached input." },
      { q: "Is it a different model?", a: "It is the faster tier of the same coding SKU. The rate card is the only thing that differs by a fixed factor." },
    ],
    related: ["how-billing-works", "why-choose-apitoken"],
  },
];

export const geminiModels: GeminiModel[] = [
  {
    provider: "gemini",
    slug: "gemini-3-7-flash",
    id: "gemini-3.7-flash",
    name: "Gemini 3.7 Flash",
    tier: "Flash",
    title: "Gemini 3.7 Flash API — Price per Token & Access",
    description: "Gemini 3.7 Flash API pricing: promotional official rates of $0.75/$3.75 per 1M tokens through 2026, or $0.375/$1.875 with the flat 50% apiToken.sale discount.",
    keywords: ["gemini 3.7 flash api", "gemini 3.7 flash price", "gemini 3.7 api cost", "gemini-3.7-flash", "google gemini api"],
    dek: "Gemini 3.7 Flash is Google's newest GA Flash model, available through the native Gemini API with streaming and authoritative token usage.",
    inputPerM: 0.75,
    cachedInputPerM: 0.075,
    outputPerM: 3.75,
    context: "1M tokens",
    maxOutput: "64K tokens",
    bestFor: [
      "Text generation and long-context analysis on the newest GA Flash generation.",
      "Incremental SSE responses with terminal authoritative usage.",
      "Cost-sensitive production traffic during the promotional rate period.",
    ],
    notes: [
      "Thinking levels low, medium and high are live-proven on the subscription wire and selectable via reasoning_effort; minimal is not supported by this model.",
      "Function calling, JSON structured output, image/audio/video/PDF input, implicit prompt caching and Google Search grounding are live-proven on the same wire.",
      "Promotional official rates run through 2026-12-31; the engine automatically switches to $1.50 input, $0.15 cached input and $7.50 output on 2027-01-01.",
    ],
    faq: [
      { q: "What model ID should clients use?", a: "Always use gemini-3.7-flash. apiToken.sale keeps any private upstream routing name internal and returns only this public ID." },
      { q: "How much does Gemini 3.7 Flash cost?", a: "Through 2026 the official promotional rate is $0.75 per 1M input tokens, $0.075 cached input and $3.75 output. The flat 50% apiToken.sale B2C discount makes that $0.375, $0.0375 and $1.875." },
      { q: "Which capabilities are currently published?", a: "Text, image, audio (inline WAV), video (inline MP4) and PDF input, function calling, JSON structured output, implicit prompt caching, Google Search grounding, countTokens and incremental SSE streaming, plus explicit thinking levels low, medium and high through reasoning_effort." },
    ],
    related: ["openai-api-quickstart", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "gemini",
    slug: "gemini-3-6-flash",
    id: "gemini-3.6-flash",
    name: "Gemini 3.6 Flash",
    tier: "Flash",
    title: "Gemini 3.6 Flash API — Price per Token & Access",
    description: "Gemini 3.6 Flash API pricing: official $1.50/$7.50 per 1M tokens, $0.75/$3.75 with the flat 50% apiToken.sale discount. The newest Gemini on the native Google API.",
    keywords: ["gemini 3.6 flash api", "gemini 3.6 flash price", "gemini 3.6 api cost", "gemini-3.6-flash", "gemini flash token pricing", "google gemini api"],
    dek: "Gemini 3.6 Flash is the newest Gemini model — Google's frontier-class Flash for agentic coding, multimodal work and long-context tasks, at Flash-tier pricing.",
    inputPerM: 1.5,
    cachedInputPerM: 0.15,
    outputPerM: 7.5,
    context: "1M tokens",
    maxOutput: "64K tokens",
    bestFor: [
      "Agentic coding and tool use at Flash speed.",
      "Multimodal workloads across text, image, audio, video and PDF input.",
      "Long-context analysis in the full 1M-token window.",
    ],
    notes: [
      "Cached input bills at 10% of input — caching is automatic on repeated prefixes.",
      "1M-token context window with 64K max output at standard pricing — no long-context premium.",
    ],
    faq: [
      { q: "How much does the Gemini 3.6 Flash API cost?", a: "Officially $1.50 per 1M input tokens and $7.50 per 1M output tokens, with cached input at $0.15. On apiToken.sale the same requests cost 50% less — $0.75/$3.75 at the flat discount applied to every call." },
      { q: "What is the model ID for Gemini 3.6 Flash?", a: "gemini-3.6-flash. Use it unchanged with the Google GenAI SDK or any Gemini-compatible tool pointed at https://router.apitoken.sale, with the key sent as x-goog-api-key." },
      { q: "Gemini 3.6 Flash or 3.5 Flash?", a: "3.6 Flash is newer and cheaper on output — $7.50 vs $9.00 per 1M at the same input price — so new projects should default to it. Keep 3.5 Flash only where prompts and evals are pinned to it." },
    ],
    related: ["openai-api-quickstart", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "gemini",
    slug: "gemini-3-5-flash",
    id: "gemini-3.5-flash",
    name: "Gemini 3.5 Flash",
    tier: "Flash",
    title: "Gemini 3.5 Flash API — Price per Token & Access",
    description: "Gemini 3.5 Flash API pricing: official $1.50/$9.00 per 1M tokens, $0.75/$4.50 with the flat 50% apiToken.sale discount. Proven Flash tier on the native Google API.",
    keywords: ["gemini 3.5 flash api", "gemini 3.5 flash price", "gemini 3.5 api cost", "gemini-3.5-flash", "gemini flash token pricing"],
    dek: "Gemini 3.5 Flash is the previous-generation Flash — a proven high-throughput model for coding and multimodal workloads, at the same input rate as 3.6 Flash.",
    inputPerM: 1.5,
    cachedInputPerM: 0.15,
    outputPerM: 9,
    context: "1M tokens",
    maxOutput: "64K tokens",
    bestFor: [
      "Workloads pinned to Gemini 3.5 Flash for reproducibility.",
      "High-volume production API traffic.",
      "Multimodal pipelines migrating gradually to 3.6 Flash.",
    ],
    notes: [
      "Same input price as Gemini 3.6 Flash, but output is $9.00 vs $7.50 — new work should default to gemini-3.6-flash.",
      "Cached input bills at 10% of input; caching is automatic on repeated prefixes.",
    ],
    faq: [
      { q: "How much does the Gemini 3.5 Flash API cost?", a: "Officially $1.50 per 1M input tokens and $9.00 per 1M output tokens, with cached input at $0.15. With the flat 50% apiToken.sale discount that is $0.75/$4.50." },
      { q: "Gemini 3.5 Flash or 3.6 Flash?", a: "They share an input price, and 3.6 Flash is newer with cheaper output ($7.50 vs $9.00) — prefer it for new projects. Stay on 3.5 Flash when your prompts and evals are pinned to it." },
      { q: "What is the model ID?", a: "gemini-3.5-flash. It works on the same apiToken.sale key and balance as every other Claude, GPT and Gemini model — send it as the model on the native Gemini endpoint with x-goog-api-key." },
    ],
    related: ["openai-api-quickstart", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "gemini",
    slug: "gemini-3-flash-preview",
    id: "gemini-3-flash-preview",
    name: "Gemini 3 Flash Preview",
    tier: "Flash",
    title: "Gemini 3 Flash Preview API — Price per Token & Access",
    description: "Gemini 3 Flash Preview API pricing: official $0.50/$3.00 per 1M tokens, $0.25/$1.50 with the flat 50% apiToken.sale discount. Native Gemini access with a 1M-token context window.",
    keywords: ["gemini 3 flash preview api", "gemini 3 flash price", "gemini-3-flash-preview", "gemini flash preview token pricing", "google gemini api"],
    dek: "Gemini 3 Flash Preview is Google's cost-efficient multimodal Gemini 3 model for agentic coding, tool use and long-context work.",
    inputPerM: 0.5,
    cachedInputPerM: 0.05,
    outputPerM: 3,
    context: "1M tokens",
    maxOutput: "64K tokens",
    bestFor: [
      "Agentic coding and function calling at Flash latency.",
      "Multimodal text, image, audio, video and document input.",
      "Cost-efficient analysis across the full 1M-token window.",
    ],
    notes: [
      "Supports minimal, low, medium and high thinking levels on the native Gemini API.",
      "Text/image/video input is $0.50 per 1M; audio input is $1.00 per 1M. Cached text and audio are $0.05/$0.10 per 1M.",
    ],
    faq: [
      { q: "How much does the Gemini 3 Flash Preview API cost?", a: "Officially $0.50 per 1M text, image or video input tokens and $3 per 1M output tokens, including thinking. Cached text input is $0.05. With the flat 50% apiToken.sale discount that is $0.25/$1.50." },
      { q: "What is the model ID?", a: "gemini-3-flash-preview. Use it unchanged with the Google GenAI SDK or another Gemini-compatible client pointed at https://router.apitoken.sale." },
      { q: "Does it support thinking levels?", a: "Yes. The native Gemini API accepts minimal, low, medium and high thinking levels for this model." },
    ],
    related: ["openai-api-quickstart", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "gemini",
    slug: "gemini-3-1-pro-preview",
    id: "gemini-3.1-pro-preview",
    name: "Gemini 3.1 Pro Preview",
    tier: "Pro",
    title: "Gemini 3.1 Pro Preview API — Price per Token & Access",
    description: "Gemini 3.1 Pro Preview API pricing: official $2/$12 per 1M tokens, $1/$6 with the flat 50% apiToken.sale discount. Pro-tier reasoning on the native Google API.",
    keywords: ["gemini 3.1 pro api", "gemini 3.1 pro preview price", "gemini 3.1 pro api cost", "gemini-3.1-pro-preview", "gemini pro token pricing"],
    dek: "Gemini 3.1 Pro Preview is Google's Pro-tier reasoning model — the strongest Gemini for hard reasoning and long-horizon agentic work, with long-context rates above 200K input tokens.",
    inputPerM: 2,
    cachedInputPerM: 0.2,
    outputPerM: 12,
    longContext: { threshold: "200K", inputPerM: 4, cachedInputPerM: 0.4, outputPerM: 18 },
    context: "1M tokens",
    maxOutput: "64K tokens",
    bestFor: [
      "The hardest reasoning, planning and review work.",
      "Long-horizon agentic tasks with tool use.",
      "Deep document and codebase analysis in the 1M-token window.",
    ],
    notes: [
      "Requests above 200K input tokens bill at long-context rates — $4/$18 per 1M (2× input, 1.5× output) on the whole request.",
      "Cached input bills at 10% of input; caching is automatic on repeated prefixes.",
    ],
    faq: [
      { q: "How much does the Gemini 3.1 Pro Preview API cost?", a: "Officially $2 per 1M input tokens and $12 per 1M output tokens, with cached input at $0.20; above 200K input tokens the whole request bills at $4/$18. On apiToken.sale the flat 50% discount applies to every call — $1/$6, or $2/$9 at long-context rates." },
      { q: "What is the model ID for Gemini 3.1 Pro Preview?", a: "gemini-3.1-pro-preview. Use it unchanged with the Google GenAI SDK or any Gemini-compatible tool pointed at https://router.apitoken.sale, with the key sent as x-goog-api-key." },
      { q: "Gemini 3.1 Pro Preview or 3.6 Flash?", a: "3.6 Flash covers most workloads at a lower token price; route the hardest reasoning and longest-horizon runs to 3.1 Pro Preview. Both run on the same key, balance and endpoint." },
    ],
    related: ["openai-api-quickstart", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "gemini",
    slug: "gemini-3-1-flash-lite",
    id: "gemini-3.1-flash-lite",
    name: "Gemini 3.1 Flash-Lite",
    tier: "Flash-Lite",
    title: "Gemini 3.1 Flash-Lite API — Price per Token & Access",
    description: "Gemini 3.1 Flash-Lite API pricing: official $0.25/$1.50 per 1M tokens, $0.125/$0.75 with the flat 50% apiToken.sale discount. The economical Gemini 3 tier.",
    keywords: ["gemini 3.1 flash-lite api", "gemini 3.1 flash lite price", "gemini-3.1-flash-lite", "cheap gemini api", "gemini flash lite token pricing"],
    dek: "Gemini 3.1 Flash-Lite is the economical tier of the Gemini 3 line — built for high-volume, latency-sensitive work at a fraction of Flash pricing.",
    inputPerM: 0.25,
    cachedInputPerM: 0.025,
    outputPerM: 1.5,
    context: "1M tokens",
    maxOutput: "64K tokens",
    bestFor: [
      "Classification, extraction and summarization at scale.",
      "Latency-sensitive chat and routing layers.",
      "Cheap pre-processing before a Flash or Pro call.",
    ],
    notes: [
      "Cached input bills at 10% of input ($0.025 per 1M); caching is automatic on repeated prefixes.",
      "Full 1M-token context and 64K max output — the same window as the Flash line.",
    ],
    faq: [
      { q: "How much does the Gemini 3.1 Flash-Lite API cost?", a: "Officially $0.25 per 1M input tokens and $1.50 per 1M output tokens, with cached input at $0.025. With the flat 50% apiToken.sale discount that is $0.125/$0.75." },
      { q: "What is the model ID?", a: "gemini-3.1-flash-lite. Point any Gemini-compatible client at https://router.apitoken.sale and send it as the model, with the key in x-goog-api-key." },
      { q: "Flash-Lite or Flash?", a: "Flash-Lite handles bulk, latency-sensitive work at a fraction of the price; step up to gemini-3.6-flash for agentic coding and harder reasoning. Many teams route by task on the same key." },
    ],
    related: ["openai-api-quickstart", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "gemini",
    slug: "gemini-2-5-flash",
    id: "gemini-2.5-flash",
    name: "Gemini 2.5 Flash",
    tier: "Flash",
    title: "Gemini 2.5 Flash API — Price per Token & Access",
    description: "Gemini 2.5 Flash API pricing: official $0.30/$2.50 per 1M tokens, $0.15/$1.25 with the flat 50% apiToken.sale discount. Proven previous-generation Flash.",
    keywords: ["gemini 2.5 flash api", "gemini 2.5 flash price", "gemini 2.5 api cost", "gemini-2.5-flash", "gemini flash token pricing"],
    dek: "Gemini 2.5 Flash is the proven previous-generation Flash — a stable workhorse for production pipelines evaluated against the 2.5 line.",
    inputPerM: 0.3,
    cachedInputPerM: 0.03,
    outputPerM: 2.5,
    context: "1M tokens",
    maxOutput: "64K tokens",
    bestFor: [
      "Pipelines tuned and evaluated against Gemini 2.5 Flash.",
      "Balanced coding and content workloads.",
      "Teams migrating gradually to the Gemini 3 line.",
    ],
    notes: [
      "Cached input bills at 10% of input; caching is automatic on repeated prefixes.",
      "Same 1M-token context and 64K max output as the newer Flash models.",
    ],
    faq: [
      { q: "How much does the Gemini 2.5 Flash API cost?", a: "Officially $0.30 per 1M input tokens and $2.50 per 1M output tokens, with cached input at $0.03. With the flat 50% apiToken.sale discount that is $0.15/$1.25." },
      { q: "What is the model ID for Gemini 2.5 Flash?", a: "gemini-2.5-flash. Use it unchanged with the Google GenAI SDK or any Gemini-compatible tool pointed at https://router.apitoken.sale, with the key sent as x-goog-api-key." },
      { q: "Gemini 2.5 Flash or 3.5 Flash?", a: "2.5 Flash is far cheaper per token and proven in production; 3.5 Flash is the stronger current model. Keep 2.5 Flash where prompts and evals are pinned to it, default to the 3.x line for new work." },
    ],
    related: ["openai-api-quickstart", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "gemini",
    slug: "gemini-2-5-flash-lite",
    id: "gemini-2.5-flash-lite",
    name: "Gemini 2.5 Flash-Lite",
    tier: "Flash-Lite",
    title: "Gemini 2.5 Flash-Lite API — Price per Token & Access",
    description: "Gemini 2.5 Flash-Lite API pricing: official $0.10/$0.40 per 1M tokens, $0.05/$0.20 with the flat 50% apiToken.sale discount. The cheapest Gemini model.",
    keywords: ["gemini 2.5 flash-lite api", "gemini 2.5 flash lite price", "gemini-2.5-flash-lite", "cheapest gemini model", "gemini flash lite token pricing"],
    dek: "Gemini 2.5 Flash-Lite is the cheapest Gemini model — built for massive-volume, latency-sensitive work like classification, extraction and routing.",
    inputPerM: 0.1,
    cachedInputPerM: 0.01,
    outputPerM: 0.4,
    context: "1M tokens",
    maxOutput: "64K tokens",
    bestFor: [
      "Classification, extraction and summarization at massive scale.",
      "Latency-sensitive chat and routing layers.",
      "Cheap pre-processing before a Flash or Pro call.",
    ],
    notes: [
      "Cached input bills at 10% of input ($0.01 per 1M); caching is automatic on repeated prefixes.",
      "Pairs well with model routing: send bulk work to Flash-Lite, hard reasoning to 3.1 Pro Preview.",
    ],
    faq: [
      { q: "How much does the Gemini 2.5 Flash-Lite API cost?", a: "Officially $0.10 per 1M input tokens and $0.40 per 1M output tokens, with cached input at $0.01. With the flat 50% apiToken.sale discount that is $0.05/$0.20 — the cheapest way to run Gemini." },
      { q: "What is Flash-Lite good for?", a: "High-volume, low-latency work: classification, extraction, summarization, routing and simple chat. For complex reasoning, step up to 2.5 Flash or the Gemini 3 line." },
      { q: "What is the model ID?", a: "gemini-2.5-flash-lite. It works on the same apiToken.sale key and balance as every other supported Claude, GPT and Gemini model." },
    ],
    related: ["openai-api-quickstart", "how-billing-works", "why-choose-apitoken"],
  },
  {
    provider: "gemini",
    slug: "gemini-3-1-flash-image",
    id: "gemini-3.1-flash-image",
    name: "Gemini 3.1 Flash Image (Nano Banana 2)",
    tier: "Image",
    title: "Gemini 3.1 Flash Image (Nano Banana 2) API — Price per Token & Access",
    description: "Gemini 3.1 Flash Image (Nano Banana 2) API pricing: official $0.50/$3.00 per 1M text tokens, $0.25/$1.50 with the flat 50% apiToken.sale discount. Image output at $60 per 1M image tokens.",
    keywords: ["gemini 3.1 flash image api", "nano banana 2 api", "gemini image model price", "gemini-3.1-flash-image", "gemini image generation cost", "google gemini image api"],
    dek: "Gemini 3.1 Flash Image — Nano Banana 2 — is Google's image-generation Flash model: text and image in, rendered images out, billed per image-output token.",
    inputPerM: 0.5,
    cachedInputPerM: 0.5,
    outputPerM: 3,
    imageOutputPerM: 60,
    context: "128K tokens",
    maxOutput: "32K tokens",
    bestFor: [
      "Image generation and editing inside production apps.",
      "Multimodal pipelines that mix text and rendered output.",
      "High-volume creative and asset workflows.",
    ],
    notes: [
      "Image output bills separately at $60 per 1M image-output tokens ($30 here); text output bills at the standard $3.00 rate.",
      "Cached input bills at the full $0.50 rate — unlike text models, this image model has no cache discount.",
      "128K context window and 32K max output — smaller than the text Flash line.",
    ],
    faq: [
      { q: "How much does the Gemini 3.1 Flash Image API cost?", a: "Officially $0.50 per 1M input tokens and $3.00 per 1M text output tokens, plus $60 per 1M image-output tokens. On apiToken.sale the flat 50% discount applies to every call — $0.25/$1.50, and $30 per 1M image-output tokens." },
      { q: "What is the model ID for Nano Banana 2?", a: "gemini-3.1-flash-image. Use it unchanged with the Google GenAI SDK or any Gemini-compatible tool pointed at https://router.apitoken.sale, with the key sent as x-goog-api-key." },
      { q: "Flash Image or a text Flash model?", a: "Flash Image is the image-generation model — use it when the response must include rendered images. For text-only work, gemini-3.6-flash gives you a larger context window and lower output cost." },
    ],
    related: ["nano-banana-2-api-guide", "nano-banana-2-api-cost", "nano-banana-2-vs-gpt-image-2", "image-generation-api-pricing"],
  },
];

export const claudeModelBySlug: Record<string, ClaudeModel> = Object.fromEntries(
  claudeModels.map((model) => [model.slug, model]),
);

export const openaiModelBySlug: Record<string, OpenAiModel> = Object.fromEntries(
  openaiModels.map((model) => [model.slug, model]),
);

export const geminiModelBySlug: Record<string, GeminiModel> = Object.fromEntries(
  geminiModels.map((model) => [model.slug, model]),
);

export const kimiModelBySlug: Record<string, KimiModel> = Object.fromEntries(
  kimiModels.map((model) => [model.slug, model]),
);

export const catalogModelBySlug: Record<string, CatalogModel> = { ...claudeModelBySlug, ...openaiModelBySlug, ...geminiModelBySlug, ...kimiModelBySlug };

export function modelPath(slug: string): string {
  return `/models/${slug}`;
}

export const MODELS_HUB_PATH = "/models";
