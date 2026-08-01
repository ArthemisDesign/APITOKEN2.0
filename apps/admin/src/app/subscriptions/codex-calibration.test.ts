import { describe, expect, it } from "vitest";
import {
  codexApiValueForCredits,
  codexTokenEconomics,
  codexTokensForCapacity,
  compareCodexEfficiency,
  creditsToApiNanousd,
  formatCodexUsdPerCredit,
  priceCodexWorkload,
  type CodexWorkloadInput,
} from "./codex-calibration";
import type { CodexConversionModel } from "./types";

const MODEL: CodexConversionModel = {
  id: "gpt-5.6-sol",
  api_tariff_schedule_id: "openai/gpt-5.6-sol/2026-07-30/v2",
  credit_schedule_id: "chatgpt/codex-credits/2026-07-30/v1",
  api: {
    input_nanousd_per_token: "5000",
    cached_input_nanousd_per_token: "500",
    cache_write_nanousd_per_token: "6250",
    output_nanousd_per_token: "30000",
    fast_multiplier_basis_points: 20_000,
    long_context_threshold: "272000",
    long_input_multiplier_basis_points: 20_000,
    long_output_multiplier_basis_points: 15_000,
  },
  chatgpt_credits: {
    input_nanocredits_per_token: "125000",
    cached_input_nanocredits_per_token: "12500",
    output_nanocredits_per_token: "750000",
    fast_multiplier_basis_points: 25_000,
  },
};

const WORKLOAD: CodexWorkloadInput = {
  freshInputTokens: "500",
  cachedInputTokens: "400",
  cacheWriteInputTokens: "100",
  outputTokens: "100",
  reasoningOutputTokens: "80",
};

describe("Codex workload conversion", () => {
  it("повторяет Rust: API Fast ×2, subscription Fast ×2.5 и cache-write = fresh credits", () => {
    const result = priceCodexWorkload(MODEL, WORKLOAD, "fast");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.value.api).toMatchObject({
      freshInputNanousd: 5_000_000n,
      cachedInputNanousd: 400_000n,
      cacheWriteNanousd: 1_250_000n,
      outputNanousd: 6_000_000n,
      totalNanousd: 12_650_000n,
    });
    expect(result.value.credits).toMatchObject({
      freshAndWriteNanocredits: 187_500_000n,
      cachedInputNanocredits: 12_500_000n,
      outputNanocredits: 187_500_000n,
      totalNanocredits: 387_500_000n,
    });
  });

  it("reasoning остаётся subset output, а не второй оплачиваемой корзиной", () => {
    const baseline = priceCodexWorkload(MODEL, WORKLOAD, "standard");
    const changed = priceCodexWorkload(MODEL, { ...WORKLOAD, reasoningOutputTokens: "1" }, "standard");
    expect(baseline.ok && changed.ok && baseline.value).toEqual(changed.ok && changed.value);
    expect(priceCodexWorkload(MODEL, { ...WORKLOAD, reasoningOutputTokens: "101" }, "standard")).toEqual({
      ok: false,
      error: "Reasoning входит в output и не может быть больше output",
    });
  });

  it("long-context включается строго выше 272k total input и применяется ко всему запросу", () => {
    const boundary = priceCodexWorkload(
      MODEL,
      { ...WORKLOAD, freshInputTokens: "271500", cachedInputTokens: "400", cacheWriteInputTokens: "100" },
      "standard",
    );
    const long = priceCodexWorkload(
      MODEL,
      { ...WORKLOAD, freshInputTokens: "271501", cachedInputTokens: "400", cacheWriteInputTokens: "100" },
      "standard",
    );
    expect(boundary.ok && boundary.value.longContext).toBe(false);
    expect(long.ok && long.value.longContext).toBe(true);
    if (!boundary.ok || !long.ok) return;
    expect(long.value.api.cachedInputNanousd).toBe(boundary.value.api.cachedInputNanousd * 2n);
    expect(long.value.api.outputNanousd * 2n).toBe(boundary.value.api.outputNanousd * 3n);
    expect(long.value.credits.cachedInputNanocredits).toBe(boundary.value.credits.cachedInputNanocredits);
  });

  it("конвертирует native capacity в API equivalent только целочисленной арифметикой", () => {
    const result = priceCodexWorkload(MODEL, WORKLOAD, "fast");
    if (!result.ok) throw new Error(result.error);
    expect(creditsToApiNanousd("387500000000", result.value)).toBe(12_650_000_000n);
    expect(creditsToApiNanousd(null, result.value)).toBeNull();
  });

  it("строит точную убывающую экономику token kinds без float", () => {
    const shortFresh = codexTokenEconomics(MODEL, "standard", "short", "fresh");
    const shortCached = codexTokenEconomics(MODEL, "standard", "short", "cached");
    const shortWrite = codexTokenEconomics(MODEL, "standard", "short", "write");
    const longWrite = codexTokenEconomics(MODEL, "standard", "long", "write");
    const fastFresh = codexTokenEconomics(MODEL, "fast", "short", "fresh");
    expect(formatCodexUsdPerCredit(shortFresh)).toBe("$0.040");
    expect(formatCodexUsdPerCredit(shortCached)).toBe("$0.040");
    expect(formatCodexUsdPerCredit(shortWrite)).toBe("$0.050");
    expect(formatCodexUsdPerCredit(longWrite)).toBe("$0.100");
    expect(formatCodexUsdPerCredit(fastFresh)).toBe("$0.032");
    expect(compareCodexEfficiency(longWrite!, shortWrite!)).toBe(1);
    expect(compareCodexEfficiency(shortFresh!, shortCached!)).toBe(0);
  });

  it("показывает токеновую вместимость всего окна для каждого native bucket", () => {
    const capacity = "62327321317308";
    expect(codexTokensForCapacity(capacity, MODEL, "standard", "fresh")).toBe(498_618_570n);
    expect(codexTokensForCapacity(capacity, MODEL, "standard", "cached")).toBe(4_986_185_705n);
    expect(codexTokensForCapacity(capacity, MODEL, "standard", "write")).toBe(498_618_570n);
    expect(codexTokensForCapacity(capacity, MODEL, "standard", "output")).toBe(83_103_095n);
    expect(codexTokensForCapacity(capacity, MODEL, "fast", "cached")).toBe(1_994_474_282n);
    expect(codexApiValueForCredits(capacity, codexTokenEconomics(MODEL, "standard", "long", "write"))).toBe(
      6_232_732_131_731n,
    );
  });
});
