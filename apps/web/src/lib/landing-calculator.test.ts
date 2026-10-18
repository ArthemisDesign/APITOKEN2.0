import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it } from "vitest";

type CalculatorModel = {
  id: string;
  name: string;
  inputRate: bigint;
  outputRate: bigint;
  cacheRate: bigint;
};
type Estimate = { now: bigint; next: bigint; save: bigint; year: bigint };
type CalculatorInput = { inputTokens: string; outputTokens: string; cachePercent: string; model: CalculatorModel };
type Calculator = {
  modelsFromProviders(providers: unknown[]): CalculatorModel[];
  calculate(input: CalculatorInput): Estimate;
  formatMoney(nano: bigint, locale: string): string;
};

const calculatorSource = readFileSync(new URL("../../public/landing/calculator.js", import.meta.url), "utf8");
const commonJsModule = { exports: {} };
runInNewContext(calculatorSource, { module: commonJsModule, exports: commonJsModule.exports });
const calculator = commonJsModule.exports as Calculator;

function providersFromPage(file: "app.js" | "app-en.js"): unknown[] {
  const source = readFileSync(new URL(`../../public/landing/${file}`, import.meta.url), "utf8");
  const data = source.match(/const PROVIDERS\s*=\s*([\s\S]+?);\s*const INTEGRATIONS\s*=/);
  if (!data) throw new Error(`Missing landing provider catalog in ${file}`);
  return runInNewContext(`(${data[1]})`) as unknown[];
}

const models = calculator.modelsFromProviders(providersFromPage("app.js"));
const opus = models.find((model) => model.id === "claude-opus-5")!;
const defaults: CalculatorInput = { inputTokens: "12000000", outputTokens: "3000000", cachePercent: "40", model: opus };
const estimate = (overrides: Partial<CalculatorInput> = {}) => calculator.calculate({ ...defaults, ...overrides });

describe("static landing advanced calculator", () => {
  it("uses the displayed discounted rates and exposes only the eight supported text models", () => {
    const withKimi = [...providersFromPage("app.js"), {
      name: "Kimi", models: [{ id: "kimi-k3", name: "Kimi K3", in: "$0.50", out: "$2.50" }],
    }];
    const supported = calculator.modelsFromProviders(withKimi);
    expect(supported.map(({ id }) => id)).toEqual([
      "claude-opus-5", "claude-sonnet-5", "claude-haiku-4-5", "claude-fable-5",
      "gpt-5-6-sol", "gpt-5-6-terra", "gpt-5-6-luna", "gpt-5-5",
    ]);
    expect(supported.every(({ name }) => name.length > 0)).toBe(true);
    expect(supported.some(({ id }) => /gemini|kimi/i.test(id))).toBe(false);
    expect(opus).toMatchObject({ inputRate: 2_500_000_000n, outputRate: 12_500_000_000n, cacheRate: 250_000_000n });
    expect(supported.find(({ id }) => id === "gpt-5-6-luna")).toMatchObject({
      inputRate: 300_000_000n, outputRate: 1_500_000_000n, cacheRate: 30_000_000n,
    });
    expect(calculator.modelsFromProviders(providersFromPage("app-en.js"))).toEqual(models);
  });

  it("calculates the default monthly and annual comparison in exact nanoUSD", () => {
    expect(estimate()).toEqual({
      now: 113_400_000_000n,
      next: 56_700_000_000n,
      save: 56_700_000_000n,
      year: 680_400_000_000n,
    });
  });

  it("applies caching only to input and handles both cache endpoints", () => {
    expect(estimate({ cachePercent: "0" }).next).toBe(67_500_000_000n);
    expect(estimate({ cachePercent: "100" }).next).toBe(40_500_000_000n);
    expect(estimate({ inputTokens: "0", cachePercent: "0" }).next).toBe(37_500_000_000n);
    expect(estimate({ inputTokens: "0", cachePercent: "100" }).next).toBe(37_500_000_000n);
  });

  it("recalculates independently when either token count or the model changes", () => {
    expect(estimate({ inputTokens: "24000000" }).next).toBe(75_900_000_000n);
    expect(estimate({ outputTokens: "6000000" }).next).toBe(94_200_000_000n);
    const sonnet = models.find(({ id }) => id === "claude-sonnet-5")!;
    const sol = models.find(({ id }) => id === "gpt-5-6-sol")!;
    expect(estimate({ model: sonnet }).next).toBe(22_680_000_000n);
    expect(estimate({ model: sol }).next).toBe(40_440_000_000n);
  });

  it.each(["12 000 000", "12\u00a0000\u00a0000", "12\u202f000\u202f000", "12,000,000"])(
    "accepts grouped token counts such as %s", (inputTokens) => {
      expect(estimate({ inputTokens, outputTokens: "3,000,000" })).toEqual(estimate());
    },
  );

  it.each(["12.34", "12,34", "12.34%", "12,34%"])(
    "accepts a localized decimal cache percentage such as %s", (cachePercent) => {
      expect(estimate({ inputTokens: "1000000", outputTokens: "0", cachePercent }).next).toBe(2_222_350_000n);
    },
  );

  it("returns actual zero values for an empty workload", () => {
    expect(estimate({ inputTokens: "0", outputTokens: "0" })).toEqual({ now: 0n, next: 0n, save: 0n, year: 0n });
  });

  it("keeps exact arithmetic at the largest supported token count", () => {
    expect(estimate({ inputTokens: "9007199254740991", outputTokens: "0", cachePercent: "0" }).next)
      .toBe(22_517_998_136_852_477_500n);
  });

  for (const field of ["inputTokens", "outputTokens"] as const) {
    it.each(["", " ", "-1", "+1", "1.5", "1,5", "12,34", "1 23", "1e6", "Infinity", "NaN", "abc", "9007199254740992"])(
      `rejects invalid ${field} %j and identifies the field`, (value) => {
        expect(() => estimate({ [field]: value })).toThrow();
        try { estimate({ [field]: value }); }
        catch (error) { expect(error).toMatchObject({ field }); }
      },
    );
  }

  it.each(["", "-1", "100.01", "101%", "0.001", "40.000", "1,234", "forty", "Infinity", "1e2"])(
    "rejects an invalid cache percentage %j and identifies the field", (cachePercent) => {
      expect(() => estimate({ cachePercent })).toThrow();
      try { estimate({ cachePercent }); }
      catch (error) { expect(error).toMatchObject({ field: "cachePercent" }); }
    },
  );

  it.each([undefined, null, {}, { ...opus, inputRate: -1n }, { ...opus, outputRate: 12.5 }])(
    "rejects a missing or malformed model and identifies the field", (model) => {
      const input = { model: model as CalculatorModel };
      expect(() => estimate(input)).toThrow();
      try { estimate(input); }
      catch (error) { expect(error).toMatchObject({ field: "model" }); }
    },
  );

  it("formats USD with two decimal places without losing cents", () => {
    expect(calculator.formatMoney(56_700_000_000n, "en")).toBe("$56.70");
    expect(calculator.formatMoney(0n, "en")).toBe("$0.00");
    expect(calculator.formatMoney(1_235_000_000n, "en")).toBe("$1.24");
    const russian = calculator.formatMoney(56_700_000_000n, "ru");
    expect(russian).toContain("56,70");
    expect(russian).toMatch(/\$|USD|долл/);
  });
});
