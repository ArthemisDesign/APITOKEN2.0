// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createContext, runInContext } from "node:vm";
import { describe, expect, it } from "vitest";
import { claudeModels, geminiModels, openaiModelsAt, GPT_56_SOL_PROMO_END_UNIX } from "./models";

const landingFile = (name: string) => readFileSync(resolve("public/landing", name), "utf8");
const helper = landingFile("calculator.js");
const CLOCK = Date.UTC(2026, 9, 20);
const values = { inputTokens: "12 000 000", outputTokens: "3 000 000", cachePercent: "40%", modelId: "claude-opus-5" };
type Values = typeof values;
type Result = { now: bigint; next: bigint; save: bigint; year: bigint };
type Calculator = {
  estimate: (input: Values, now?: number) => Result;
  formatMoney: (nano: bigint, locale: string) => string;
  rateCard: (id: string, now: number) => { id: string; input: string; output: string; cache: string };
};

function calculator() {
  const context = createContext({});
  runInContext(helper, context);
  return context.LandingCalculator as Calculator;
}

describe("landing advanced token estimate", () => {
  const expected = [
    ["claude-opus-5", 113_400_000_000n, 56_700_000_000n],
    ["claude-sonnet-5", 68_040_000_000n, 34_020_000_000n],
    ["claude-haiku-4-5", 22_680_000_000n, 11_340_000_000n],
    ["claude-fable-5", 226_800_000_000n, 113_400_000_000n],
    ["gpt-5.6-sol", 90_720_000_000n, 45_360_000_000n],
    ["gpt-5.6-terra", 51_360_000_000n, 25_680_000_000n],
    ["gpt-5.6-luna", 5_136_000_000n, 2_568_000_000n],
    ["gpt-5.5", 128_400_000_000n, 64_200_000_000n],
    ["gemini-3.1-pro-preview", 51_360_000_000n, 25_680_000_000n],
    ["gemini-3-flash-preview", 12_840_000_000n, 6_420_000_000n],
  ] as const;

  it.each(expected)("prices input, output and cache consistently for %s", (id, now, next) => {
    expect(calculator().estimate({ ...values, modelId: id }, CLOCK)).toEqual({ now, next, save: now - next, year: (now - next) * 12n });
  });

  it.each(expected)("keeps the %s snapshot aligned with the local authoritative catalog", (id) => {
    const catalog = [...claudeModels, ...openaiModelsAt(CLOCK / 1000), ...geminiModels];
    const model = catalog.find(item => item.id === id)!;
    const cache = "cacheReadPerM" in model ? model.cacheReadPerM : model.cachedInputPerM;
    const rate = calculator().rateCard(id, CLOCK);
    expect([rate.input, rate.output, rate.cache]).toEqual([String(model.inputPerM), String(model.outputPerM), String(cache)]);
  });

  it("replaces input tokens with cache reads without discounting output twice", () => {
    const calc = calculator();
    expect(calc.estimate({ ...values, cachePercent: "0" }, CLOCK).next).toBe(67_500_000_000n);
    expect(calc.estimate({ ...values, cachePercent: "100%" }, CLOCK).next).toBe(40_500_000_000n);
    expect(calc.estimate({ ...values, inputTokens: "0", cachePercent: "100%" }, CLOCK).next).toBe(37_500_000_000n);
    expect(calc.estimate({ ...values, inputTokens: "0", outputTokens: "0" }, CLOCK).next).toBe(0n);
    expect(calc.estimate({ ...values, inputTokens: "1,000,000", outputTokens: "0", cachePercent: "40,5%" }, CLOCK).next).toBe(1_588_750_000n);
    expect(calc.estimate({ ...values, inputTokens: "1\u202f000\u202f000", outputTokens: "0", cachePercent: "40.5" }, CLOCK).next).toBe(1_588_750_000n);
  });

  it("uses the explicit clock on both sides of the Sol promotion boundary", () => {
    const calc = calculator();
    const input = { ...values, modelId: "gpt-5-6-sol" };
    expect(calc.estimate(input, GPT_56_SOL_PROMO_END_UNIX * 1000 - 1).next).toBe(45_360_000_000n);
    expect(calc.estimate(input, GPT_56_SOL_PROMO_END_UNIX * 1000).next).toBe(64_200_000_000n);
    expect(calc.rateCard("gpt-5.6-sol", GPT_56_SOL_PROMO_END_UNIX * 1000)).toMatchObject({ input: "5", output: "30", cache: "0.5" });
  });

  it.each([
    { inputTokens: "-1" }, { inputTokens: "1.5" }, { inputTokens: "" },
    { inputTokens: "12,34" }, { inputTokens: "9007199254740992" },
    { outputTokens: "NaN" }, { cachePercent: "-1%" }, { cachePercent: "101" },
    { cachePercent: "" }, { cachePercent: "1e2" }, { modelId: "unknown" },
    { modelId: "gemini-image" }, { modelId: "gemini-3-pro" },
  ])("rejects invalid or unsupported values: %j", (invalid) => {
    expect(() => calculator().estimate({ ...values, ...invalid }, CLOCK)).toThrow();
  });

  it("keeps small cache changes visible and derives annual savings before display rounding", () => {
    const calc = calculator();
    expect(calc.formatMoney(250n, "en-US")).toBe("$0.00000025");
    const result = calc.estimate({ ...values, modelId: "gpt-5.6-luna" }, CLOCK);
    expect(calc.formatMoney(result.next, "en-US")).toBe("$2.57");
    expect(calc.formatMoney(result.year, "ru-RU")).toBe("$30,82");
  });
});

function mountLanding(language: "ru" | "en") {
  const htmlName = language === "ru" ? "index" : "en";
  const appName = language === "ru" ? "app" : "app-en";
  const html = landingFile(`${htmlName}.html`);
  const app = landingFile(`${appName}.js`);
  document.body.innerHTML = html;
  const frames = new Map<number, (timestamp: number) => void>();
  let frameId = 0;
  const context = createContext({
    document,
    Date: class extends Date { static now() { return CLOCK; } },
    performance: { now: () => 0 },
    requestAnimationFrame: (callback: (timestamp: number) => void) => { frames.set(++frameId, callback); return frameId; },
    cancelAnimationFrame: (id: number) => frames.delete(id),
    setTimeout: () => 0,
    addEventListener: () => {},
  });
  context.window = context;
  runInContext(helper, context);
  // Exercise the actual RU/EN calculator initializers and handlers, without
  // loading unrelated video, routing, animation or model-cloud behavior.
  const profiles = app.match(/const PROFILES = (\[[\s\S]*?\n\]);/)![1];
  const start = app.lastIndexOf("(() => {", app.indexOf("const profWrap = $('#calcProfiles')"));
  const end = app.indexOf("\n})();", start) + "\n})();".length;
  runInContext(`
    const $ = (selector, root = document) => root.querySelector(selector);
    const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
    const el = (tag, cls, html) => { const node = document.createElement(tag); if (cls) node.className = cls; if (html != null) node.innerHTML = html; return node; };
    const money = value => '$' + Math.round(value).toLocaleString('${language === "ru" ? "ru-RU" : "en-US"}').replace(/,/g, ' ');
    const PROFILES = ${profiles};
    ${app.slice(start, end)}
  `, context);
  const field = (name: string) => document.querySelector<HTMLInputElement>(`[data-calc-field="${name}"]`)!;
  const write = (element: HTMLInputElement, value: string) => {
    element.value = value;
    element.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const flushFrames = () => {
    const pending = [...frames.values()];
    frames.clear();
    pending.forEach(callback => callback(400));
  };
  const text = (id: string) => document.getElementById(id)!.textContent!.replace(/\u00a0/g, " ");
  return { html, field, write, text, flushFrames, toggle: document.getElementById("advToggle")!, panel: document.getElementById("adv")! };
}

describe.each(["ru", "en"] as const)("%s landing calculator controls", (language) => {
  it("recalculates every advanced field, stops pending basic animation and restores the prior spend", () => {
    const page = mountLanding(language);
    const decimal = language === "ru" ? "," : ".";
    expect(page.html.indexOf('/landing/calculator.js')).toBeLessThan(page.html.indexOf(language === "ru" ? '/landing/app.js' : '/landing/app-en.js'));
    expect(page.text("resNow")).toBe("$9 750");
    const spend = document.getElementById("spendInput") as HTMLInputElement;
    page.write(spend, "20 000");
    page.toggle.click();
    page.flushFrames();
    expect(page.text("resNow")).toBe(`$113${decimal}40`);
    expect(page.text("resMo")).toBe(`$56${decimal}70`);
    expect(page.text("resYr")).toContain(`$680${decimal}40`);
    expect(page.toggle.getAttribute("aria-expanded")).toBe("true");
    expect(document.querySelectorAll("#advModels option")).toHaveLength(10);

    page.write(page.field("cachePercent"), "100%");
    expect(page.text("resNew")).toBe(`$40${decimal}50`);
    page.write(page.field("inputTokens"), "1 000 000");
    page.write(page.field("outputTokens"), "0");
    page.write(page.field("cachePercent"), "0");
    page.write(page.field("modelId"), "gpt-5-6-terra");
    expect(page.text("resNew")).toBe(`$1${decimal}00`);
    page.write(page.field("modelId"), "gpt-5.6-luna");
    expect(page.text("resNew")).toBe(`$0${decimal}10`);
    page.write(page.field("cachePercent"), "100");
    expect(page.text("resNew")).toBe(`$0${decimal}01`);
    page.write(page.field("inputTokens"), "2 000 000");
    expect(page.text("resNew")).toBe(`$0${decimal}02`);
    page.write(page.field("outputTokens"), "1 000 000");
    expect(page.text("resNew")).toBe(`$0${decimal}62`);

    page.write(page.field("cachePercent"), "101%");
    expect(page.text("resNow")).toBe("—");
    expect(page.field("cachePercent").getAttribute("aria-invalid")).toBe("true");
    page.write(page.field("cachePercent"), "0");
    expect(page.field("cachePercent").hasAttribute("aria-invalid")).toBe(false);
    page.write(page.field("modelId"), "gemini-image");
    expect(page.text("resNew")).toBe("—");
    expect(page.field("modelId").getAttribute("aria-invalid")).toBe("true");
    page.write(page.field("modelId"), "claude-opus-5");
    expect(page.text("resNew")).toBe(`$17${decimal}50`);

    page.toggle.click();
    page.flushFrames();
    expect(page.panel.hidden).toBe(true);
    expect(spend.value).toBe("20 000");
    expect(page.text("resNow")).toBe("$20 000");
    expect(page.text("resNew")).toBe("$10 000");
    page.toggle.click();
    expect(page.text("resNew")).toBe(`$17${decimal}50`);
    const profiles = document.querySelectorAll<HTMLButtonElement>("#calcProfiles button");
    profiles[1].click();
    const slider = document.getElementById("devRange") as HTMLInputElement;
    page.write(slider, "10");
    page.flushFrames();
    expect(page.panel.hidden).toBe(true);
    expect(page.text("resNow")).toBe("$3 900");
    expect(page.text("resNew")).toBe("$1 950");
  });
});
