export type UsageProvider = "claude" | "openai" | "gemini" | "kimi";

export interface UsageModelRow {
  model: string;
  /** Engine provider id: anthropic/openai/google (Gemini traffic), free-form. */
  provider?: string;
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

/**
 * Короткая метка провайдера для таблицы моделей.
 *
 * Записана здесь, рядом с типом, а не инлайном в JSX: инлайновая карта не заставляет
 * компилятор проверять полноту, поэтому новый провайдер молча получал бы `undefined` в колонке.
 */
export const USAGE_PROVIDER_LABELS: Record<UsageProvider, string> = {
  claude: "Claude",
  openai: "GPT",
  gemini: "Gemini",
  kimi: "Kimi",
};

export function usageProviderOf(model: string, provider?: string): UsageProvider {
  if (provider === "openai") return "openai";
  // The engine tags Gemini traffic with its registry id "google".
  if (provider === "google" || provider === "gemini") return "gemini";
  if (provider === "kimi") return "kimi";
  if (provider === "anthropic") return "claude";
  const bare = model.includes("/") ? model.slice(model.indexOf("/") + 1) : model;
  const name = bare.toLowerCase();
  if (name.startsWith("gemini")) return "gemini";
  if (name.startsWith("gpt-")) return "openai";
  // KIMI publishes two unrelated-looking alias shapes: `kimi-for-coding…` and the bare `k3`
  // family. Both had been landing in the catch-all below, which does not mean "unknown" — it
  // means Claude, so KIMI spend was being reported to the customer as Anthropic spend.
  if (name.startsWith("kimi") || name === "k3" || name.startsWith("k3-") || name.startsWith("k3[")) {
    return "kimi";
  }
  return "claude";
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
    kimi: {
      provider: "kimi",
      label: "Kimi / Moonshot",
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

  return [summaries.claude, summaries.openai, summaries.gemini, summaries.kimi];
}
