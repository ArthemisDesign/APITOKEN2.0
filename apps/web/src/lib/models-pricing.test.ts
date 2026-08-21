import { describe, expect, it } from "vitest";
import {
  GEMINI_FLASH_PROMO_END_UNIX,
  geminiFlashPricingAt,
  geminiModels,
  priceHere,
} from "./models";

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
