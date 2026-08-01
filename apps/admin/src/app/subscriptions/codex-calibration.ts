import type { CodexConversionModel } from "./types";

export type CodexServiceTier = "standard" | "fast";
export type CodexContextTier = "short" | "long";
export type CodexTokenKind = "fresh" | "cached" | "write" | "output";

export const CODEX_TOKEN_KINDS: CodexTokenKind[] = ["fresh", "cached", "write", "output"];

export interface CodexTokenEconomics {
  apiNanousdPerToken: bigint;
  creditNanocreditsPerToken: bigint;
}

export interface CodexWorkloadInput {
  freshInputTokens: string;
  cachedInputTokens: string;
  cacheWriteInputTokens: string;
  outputTokens: string;
  reasoningOutputTokens: string;
}

export interface CodexWorkloadPrice {
  totalInputTokens: bigint;
  longContext: boolean;
  api: {
    freshInputNanousd: bigint;
    cachedInputNanousd: bigint;
    cacheWriteNanousd: bigint;
    outputNanousd: bigint;
    totalNanousd: bigint;
  };
  credits: {
    freshAndWriteNanocredits: bigint;
    cachedInputNanocredits: bigint;
    outputNanocredits: bigint;
    totalNanocredits: bigint;
  };
}

export type CodexWorkloadPriceResult =
  | { ok: true; value: CodexWorkloadPrice }
  | { ok: false; error: string };

export const CODEX_WORKLOAD_PRESETS: Record<"review" | "agent" | "long", CodexWorkloadInput> = {
  review: {
    freshInputTokens: "12000",
    cachedInputTokens: "8000",
    cacheWriteInputTokens: "4000",
    outputTokens: "2500",
    reasoningOutputTokens: "1800",
  },
  agent: {
    freshInputTokens: "50000",
    cachedInputTokens: "20000",
    cacheWriteInputTokens: "10000",
    outputTokens: "10000",
    reasoningOutputTokens: "8000",
  },
  long: {
    freshInputTokens: "240000",
    cachedInputTokens: "60000",
    cacheWriteInputTokens: "10000",
    outputTokens: "8000",
    reasoningOutputTokens: "6000",
  },
};

function parseUnsigned(value: string, label: string): bigint | string {
  if (!/^(0|[1-9][0-9]*)$/.test(value)) return `${label}: укажите целое число ≥ 0`;
  try {
    return BigInt(value);
  } catch {
    return `${label}: число слишком велико`;
  }
}

function parseRate(value: string): bigint | null {
  if (!/^(0|[1-9][0-9]*)$/.test(value)) return null;
  try {
    return BigInt(value);
  } catch {
    return null;
  }
}

// Rust metering::apply_multiplier parity: integer half-up at every modifier boundary.
export function applyCodexBasisPoints(amount: bigint, basisPoints: number): bigint | null {
  if (amount < 0n || !Number.isSafeInteger(basisPoints) || basisPoints < 0) return null;
  return (amount * BigInt(basisPoints) + 5_000n) / 10_000n;
}

function multiplyLeg(amount: bigint, contextBasisPoints: number, tierBasisPoints: number): bigint | null {
  const context = applyCodexBasisPoints(amount, contextBasisPoints);
  return context == null ? null : applyCodexBasisPoints(context, tierBasisPoints);
}

/**
 * Exact browser mirror of `forward::codex::billing::price_usage` and
 * `metering::codex_credit_cost_nano`.
 *
 * The five editable counters are disjoint except reasoning, which is a diagnostic subset of
 * output. Cache writes receive their own API rate but consume fresh-input ChatGPT credits.
 */
export function priceCodexWorkload(
  model: CodexConversionModel,
  workload: CodexWorkloadInput,
  tier: CodexServiceTier,
): CodexWorkloadPriceResult {
  const entries = [
    [workload.freshInputTokens, "Fresh input"],
    [workload.cachedInputTokens, "Cached input"],
    [workload.cacheWriteInputTokens, "Cache write"],
    [workload.outputTokens, "Output"],
    [workload.reasoningOutputTokens, "Reasoning"],
  ] as const;
  const parsed = entries.map(([value, label]) => parseUnsigned(value.trim(), label));
  const issue = parsed.find((value): value is string => typeof value === "string");
  if (issue) return { ok: false, error: issue };
  const [fresh, cached, cacheWrite, output, reasoning] = parsed as bigint[];
  if (reasoning > output) {
    return { ok: false, error: "Reasoning входит в output и не может быть больше output" };
  }
  if (fresh + cached + cacheWrite + output === 0n) {
    return { ok: false, error: "Нагрузка должна содержать хотя бы один тарифицируемый токен" };
  }

  const apiInput = parseRate(model.api.input_nanousd_per_token);
  const apiCached = parseRate(model.api.cached_input_nanousd_per_token);
  const apiWrite = parseRate(model.api.cache_write_nanousd_per_token);
  const apiOutput = parseRate(model.api.output_nanousd_per_token);
  const creditInput = parseRate(model.chatgpt_credits.input_nanocredits_per_token);
  const creditCached = parseRate(model.chatgpt_credits.cached_input_nanocredits_per_token);
  const creditOutput = parseRate(model.chatgpt_credits.output_nanocredits_per_token);
  const threshold = parseRate(model.api.long_context_threshold ?? "0");
  if (
    apiInput == null ||
    apiCached == null ||
    apiWrite == null ||
    apiOutput == null ||
    creditInput == null ||
    creditCached == null ||
    creditOutput == null ||
    threshold == null
  ) {
    return { ok: false, error: "Каталог тарифов модели повреждён" };
  }

  const apiTierBp = tier === "fast" ? model.api.fast_multiplier_basis_points : 10_000;
  const creditTierBp = tier === "fast" ? model.chatgpt_credits.fast_multiplier_basis_points : 10_000;
  if (apiTierBp == null || creditTierBp == null) {
    return { ok: false, error: "Fast недоступен для выбранной модели" };
  }
  const totalInput = fresh + cached + cacheWrite;
  const longContext = totalInput > threshold;
  const inputContextBp = longContext ? (model.api.long_input_multiplier_basis_points ?? 10_000) : 10_000;
  const outputContextBp = longContext ? (model.api.long_output_multiplier_basis_points ?? 10_000) : 10_000;

  const apiFresh = multiplyLeg(fresh * apiInput, inputContextBp, apiTierBp);
  const apiCachedLeg = multiplyLeg(cached * apiCached, inputContextBp, apiTierBp);
  const apiWriteLeg = multiplyLeg(cacheWrite * apiWrite, inputContextBp, apiTierBp);
  const apiOutputLeg = multiplyLeg(output * apiOutput, outputContextBp, apiTierBp);
  const creditFresh = applyCodexBasisPoints((fresh + cacheWrite) * creditInput, creditTierBp);
  const creditCachedLeg = applyCodexBasisPoints(cached * creditCached, creditTierBp);
  const creditOutputLeg = applyCodexBasisPoints(output * creditOutput, creditTierBp);
  if (
    apiFresh == null ||
    apiCachedLeg == null ||
    apiWriteLeg == null ||
    apiOutputLeg == null ||
    creditFresh == null ||
    creditCachedLeg == null ||
    creditOutputLeg == null
  ) {
    return { ok: false, error: "Некорректный множитель тарифного каталога" };
  }

  return {
    ok: true,
    value: {
      totalInputTokens: totalInput,
      longContext,
      api: {
        freshInputNanousd: apiFresh,
        cachedInputNanousd: apiCachedLeg,
        cacheWriteNanousd: apiWriteLeg,
        outputNanousd: apiOutputLeg,
        totalNanousd: apiFresh + apiCachedLeg + apiWriteLeg + apiOutputLeg,
      },
      credits: {
        freshAndWriteNanocredits: creditFresh,
        cachedInputNanocredits: creditCachedLeg,
        outputNanocredits: creditOutputLeg,
        totalNanocredits: creditFresh + creditCachedLeg + creditOutputLeg,
      },
    },
  };
}

// Convert a measured native capacity into public API replacement cost for the selected workload.
export function creditsToApiNanousd(
  capacityNanocredits: string | bigint | null | undefined,
  workloadPrice: CodexWorkloadPrice | null,
): bigint | null {
  if (capacityNanocredits == null || workloadPrice == null || workloadPrice.credits.totalNanocredits <= 0n) {
    return null;
  }
  try {
    const capacity = BigInt(capacityNanocredits);
    if (capacity < 0n) return null;
    const numerator = capacity * workloadPrice.api.totalNanousd;
    return (numerator + workloadPrice.credits.totalNanocredits / 2n) / workloadPrice.credits.totalNanocredits;
  } catch {
    return null;
  }
}

export function sumCodexIntegers(values: Array<string | null | undefined>): bigint | null {
  try {
    return values.reduce<bigint>((sum, value) => sum + BigInt(value ?? "0"), 0n);
  } catch {
    return null;
  }
}

export function formatCodexTokenCount(value: string | bigint | null | undefined): string {
  try {
    return BigInt(value ?? "0").toLocaleString("en-US");
  } catch {
    return "—";
  }
}

function modelTokenRates(
  model: CodexConversionModel,
  tokenKind: CodexTokenKind,
): { api: bigint; credits: bigint } | null {
  const api = parseRate(
    tokenKind === "fresh"
      ? model.api.input_nanousd_per_token
      : tokenKind === "cached"
        ? model.api.cached_input_nanousd_per_token
        : tokenKind === "write"
          ? model.api.cache_write_nanousd_per_token
          : model.api.output_nanousd_per_token,
  );
  // The native card has no cache-write bucket: writes consume fresh-input credits.
  const credits = parseRate(
    tokenKind === "cached"
      ? model.chatgpt_credits.cached_input_nanocredits_per_token
      : tokenKind === "output"
        ? model.chatgpt_credits.output_nanocredits_per_token
        : model.chatgpt_credits.input_nanocredits_per_token,
  );
  return api == null || credits == null ? null : { api, credits };
}

/** Exact one-token economics used by the compact capacity and profitability tables. */
export function codexTokenEconomics(
  model: CodexConversionModel,
  tier: CodexServiceTier,
  context: CodexContextTier,
  tokenKind: CodexTokenKind,
): CodexTokenEconomics | null {
  const rates = modelTokenRates(model, tokenKind);
  if (!rates) return null;
  const apiTierBp = tier === "fast" ? model.api.fast_multiplier_basis_points : 10_000;
  const creditTierBp = tier === "fast" ? model.chatgpt_credits.fast_multiplier_basis_points : 10_000;
  if (apiTierBp == null || creditTierBp == null) return null;
  const contextBp =
    context === "long"
      ? tokenKind === "output"
        ? (model.api.long_output_multiplier_basis_points ?? 10_000)
        : (model.api.long_input_multiplier_basis_points ?? 10_000)
      : 10_000;
  const api = multiplyLeg(rates.api, contextBp, apiTierBp);
  const credits = applyCodexBasisPoints(rates.credits, creditTierBp);
  if (api == null || credits == null || credits <= 0n) return null;
  return { apiNanousdPerToken: api, creditNanocreditsPerToken: credits };
}

export function codexTokensForCapacity(
  capacityNanocredits: string | bigint | null | undefined,
  model: CodexConversionModel,
  tier: CodexServiceTier,
  tokenKind: CodexTokenKind,
): bigint | null {
  if (capacityNanocredits == null) return null;
  try {
    const capacity = BigInt(capacityNanocredits);
    const economics = codexTokenEconomics(model, tier, "short", tokenKind);
    if (capacity < 0n || !economics) return null;
    return capacity / economics.creditNanocreditsPerToken;
  } catch {
    return null;
  }
}

export function compareCodexEfficiency(a: CodexTokenEconomics, b: CodexTokenEconomics): number {
  const left = a.apiNanousdPerToken * b.creditNanocreditsPerToken;
  const right = b.apiNanousdPerToken * a.creditNanocreditsPerToken;
  return left === right ? 0 : left > right ? 1 : -1;
}

export function codexApiValueForCredits(
  capacityNanocredits: string | bigint | null | undefined,
  economics: CodexTokenEconomics | null,
): bigint | null {
  if (capacityNanocredits == null || !economics) return null;
  try {
    const capacity = BigInt(capacityNanocredits);
    if (capacity < 0n) return null;
    return (
      capacity * economics.apiNanousdPerToken + economics.creditNanocreditsPerToken / 2n
    ) / economics.creditNanocreditsPerToken;
  } catch {
    return null;
  }
}

export function formatCodexUsdPerCredit(economics: CodexTokenEconomics | null): string {
  if (!economics) return "—";
  const scale = 1_000n;
  const scaled =
    (economics.apiNanousdPerToken * scale + economics.creditNanocreditsPerToken / 2n) /
    economics.creditNanocreditsPerToken;
  const whole = scaled / scale;
  const fraction = (scaled % scale).toString().padStart(3, "0");
  return `$${whole}.${fraction}`;
}
