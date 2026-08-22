import { describe, expect, it } from "vitest";
import { CommercePartnerPricingError } from "./commerce.service.js";
import { isTerminalCommerceError } from "./partner-request-effect.service.js";

describe("partner request Commerce failure classification", () => {
  it("stops on durable conflicts and authorization/request defects", () => {
    expect(isTerminalCommerceError(new CommercePartnerPricingError(409, "payload drift"))).toBe(true);
    expect(isTerminalCommerceError(new CommercePartnerPricingError(403, "ownership"))).toBe(true);
    expect(isTerminalCommerceError(new CommercePartnerPricingError(400, "invalid request"))).toBe(true);
  });

  it("retries timeouts, throttling, server failures and transport errors", () => {
    expect(isTerminalCommerceError(new CommercePartnerPricingError(401, "control key rotation"))).toBe(false);
    expect(isTerminalCommerceError(new CommercePartnerPricingError(408, "timeout"))).toBe(false);
    expect(isTerminalCommerceError(new CommercePartnerPricingError(422, "unexpected proxy response"))).toBe(false);
    expect(isTerminalCommerceError(new CommercePartnerPricingError(429, "throttled"))).toBe(false);
    expect(isTerminalCommerceError(new CommercePartnerPricingError(503, "unavailable"))).toBe(false);
    expect(isTerminalCommerceError(new Error("network timeout"))).toBe(false);
  });
});
