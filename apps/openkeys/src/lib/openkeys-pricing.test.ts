import { describe, expect, it } from "vitest";
import { EngineClientError } from "@claude-api/engine-client";
import {
  assertNoOpenKeysPricingOverride,
  assertOfficialEngineAccount,
  describeIssuanceBlock,
  OFFICIAL_ONE_TO_ONE_MULT_BP,
  OpenKeysPricingError,
} from "./openkeys-pricing.js";

describe("OpenKeys official 1:1 pricing", () => {






  it("rejects multiplier, discount, and pricing-contract overrides at every caller boundary", () => {
    for (const field of [
      "multBp",
      "mult_bp",
      "multiplierBp",
      "discountBps",
      "discount_bps",
      "pricingContract",
      "pricing_contract",
    ]) {
      expect(() => assertNoOpenKeysPricingOverride({ [field]: 9_999 }), field)
        .toThrow("fixed at 1:1");
    }
    expect(() => assertNoOpenKeysPricingOverride({ faceValueNano: 50_000_000_000n })).not.toThrow();
    expect(() => assertOfficialEngineAccount({ account: "acct_ok", multBp: 9_999 }))
      .toThrow("fixed 1:1 multiplier");
    expect(() => assertOfficialEngineAccount({
      account: "acct_ok",
      multBp: OFFICIAL_ONE_TO_ONE_MULT_BP,
    })).not.toThrow();
  });




  describe("describeIssuanceBlock", () => {
    it("передаёт код pricing-ошибки без утечки внутреннего сообщения", () => {
      const reason = describeIssuanceBlock(
        new OpenKeysPricingError("pricing_authority_missing", "internal catalog detail"),
      );
      expect(reason.code).toBe("pricing_authority_missing");
      expect(reason.message).toContain("authority");
      expect(reason.message).not.toContain("internal catalog detail");
    });

    it("сетевую/HTTP-ошибку движка отличает от неподтверждённого authority", () => {
      const reason = describeIssuanceBlock(
        new EngineClientError("engine request failed", undefined, true),
      );
      expect(reason.code).toBe("engine_unavailable");
      expect(reason.message).toContain("Движок недоступен");
    });

    it("прочие ошибки сворачивает в общий код без внутренностей", () => {
      const reason = describeIssuanceBlock(new Error("ENGINE_BASE_URL must be an absolute URL"));
      expect(reason.code).toBe("authority_check_failed");
      expect(reason.message).not.toContain("ENGINE_BASE_URL");
    });
  });



});
