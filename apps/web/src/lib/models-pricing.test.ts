import { describe, expect, it } from "vitest";
import {
  GEMINI_FLASH_PROMO_END_UNIX,
  GPT_56_SOL_PROMO_END_UNIX,
  geminiFlashPricingAt,
  geminiModels,
  gpt56SolPricingAt,
  openaiModelsAt,
  priceHere,
} from "./models";

describe("effective-dated GPT-5.6 Sol pricing", () => {
  it("uses OpenAI's promotional card through 2026-11-21", () => {
    expect(gpt56SolPricingAt(GPT_56_SOL_PROMO_END_UNIX - 1)).toEqual({
      inputPerM: 4,
      cachedInputPerM: 0.4,
      cacheWritePerM: 5,
      outputPerM: 20,
      promotional: true,
    });
  });

  it("returns atomically to the standard card on 2026-11-22 UTC", () => {
    expect(gpt56SolPricingAt(GPT_56_SOL_PROMO_END_UNIX)).toEqual({
      inputPerM: 5,
      cachedInputPerM: 0.5,
      cacheWritePerM: 6.25,
      outputPerM: 30,
      promotional: false,
    });
  });

  it("changes only Sol and applies the B2C discount after effective-date resolution", () => {
    const promotional = openaiModelsAt(GPT_56_SOL_PROMO_END_UNIX - 1);
    const standard = openaiModelsAt(GPT_56_SOL_PROMO_END_UNIX);
    const promoSol = promotional.find((model) => model.id === "gpt-5.6-sol")!;
    const standardSol = standard.find((model) => model.id === "gpt-5.6-sol")!;

    expect([promoSol.inputPerM, promoSol.cachedInputPerM, promoSol.cacheWritePerM, promoSol.outputPerM]).toEqual([4, 0.4, 5, 20]);
    expect([standardSol.inputPerM, standardSol.cachedInputPerM, standardSol.cacheWritePerM, standardSol.outputPerM]).toEqual([5, 0.5, 6.25, 30]);
    expect(priceHere(promoSol.inputPerM)).toBe("$2");
    expect(priceHere(promoSol.cachedInputPerM)).toBe("$0.2");
    expect(priceHere(promoSol.cacheWritePerM)).toBe("$2.5");
    expect(priceHere(promoSol.outputPerM)).toBe("$10");
    expect(promotional.find((model) => model.id === "gpt-5.5")).toEqual(standard.find((model) => model.id === "gpt-5.5"));
    expect(promotional.find((model) => model.id === "gpt-image-2")).toEqual(standard.find((model) => model.id === "gpt-image-2"));
  });
});

describe("effective-dated Gemini Flash pricing", () => {
  it("uses Google's promotional rate through 2026-12-31", () => {
    expect(geminiFlashPricingAt(GEMINI_FLASH_PROMO_END_UNIX - 1)).toEqual({
      inputPerM: 0.75,
      cachedInputPerM: 0.075,
      outputPerM: 3.75,
    });
  });

  it("switches atomically to Google's standard rate on 2027-01-01 UTC", () => {
    expect(geminiFlashPricingAt(GEMINI_FLASH_PROMO_END_UNIX)).toEqual({
      inputPerM: 1.5,
      cachedInputPerM: 0.15,
      outputPerM: 7.5,
    });
  });

  it("publishes one effective card for Gemini 3.6 and Gemini 3.7", () => {
    const flash36 = geminiModels.find((model) => model.id === "gemini-3.6-flash");
    const flash37 = geminiModels.find((model) => model.id === "gemini-3.7-flash");

    expect(flash36).toBeDefined();
    expect(flash37).toBeDefined();
    expect({
      inputPerM: flash36?.inputPerM,
      cachedInputPerM: flash36?.cachedInputPerM,
      outputPerM: flash36?.outputPerM,
    }).toEqual({
      inputPerM: flash37?.inputPerM,
      cachedInputPerM: flash37?.cachedInputPerM,
      outputPerM: flash37?.outputPerM,
    });
  });

  it("applies the B2C multiplier after the official promotional rate", () => {
    const promotional = geminiFlashPricingAt(GEMINI_FLASH_PROMO_END_UNIX - 1);
    expect(priceHere(promotional.inputPerM)).toBe("$0.375");
    expect(priceHere(promotional.cachedInputPerM)).toBe("$0.0375");
    expect(priceHere(promotional.outputPerM)).toBe("$1.875");
  });
});
