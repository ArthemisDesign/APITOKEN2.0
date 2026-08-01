export type UsageProvider = "claude" | "openai" | "gemini";

export interface UsageModelRow {
  model: string;
  provider?: "anthropic" | "openai" | "gemini";
  requests: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_5m_tokens: number;
  cache_write_1h_tokens: number;
  official_nano: string;
  charged_nano: string;
}

export interface UsageProviderSummary {
  provider: UsageProvider;
  label: string;
  requests: number;
  tokens: number;
  officialNano: bigint;
  chargedNano: bigint;
}

export function usageProviderOf(model: string, provider?: "anthropic" | "openai" | "gemini"): UsageProvider {
  if (provider === "gemini") return "gemini";
  if (provider) return provider === "openai" ? "openai" : "claude";
  const name = model.toLowerCase();
  if (name.startsWith("gemini")) return "gemini";
  return name.startsWith("gpt-") ? "openai" : "claude";
}

/** Разбивка общего USAGE по API-планам, при этом деньги остаются bigint. */
export function aggregateUsageProviders(models: UsageModelRow[]): UsageProviderSummary[] {
  const summaries: Record<UsageProvider, UsageProviderSummary> = {
    claude: {
      provider: "claude",
      label: "Claude / Anthropic",
      requests: 0,
      tokens: 0,
      officialNano: 0n,
      chargedNano: 0n,
    },
    openai: {
      provider: "openai",
      label: "GPT / OpenAI",
      requests: 0,
      tokens: 0,
      officialNano: 0n,
      chargedNano: 0n,
    },
    gemini: {
      provider: "gemini",
      label: "Gemini / Google",
      requests: 0,
      tokens: 0,
      officialNano: 0n,
      chargedNano: 0n,
    },
  };

  for (const model of models) {
    const summary = summaries[usageProviderOf(model.model, model.provider)];
    summary.requests += model.requests;
    summary.tokens += model.input_tokens
      + model.output_tokens
      + model.cache_read_tokens
      + model.cache_write_5m_tokens
      + model.cache_write_1h_tokens;
    summary.officialNano += BigInt(model.official_nano);
    summary.chargedNano += BigInt(model.charged_nano);
  }

  return [summaries.claude, summaries.openai, summaries.gemini];
}
