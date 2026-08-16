import { describe, expect, it, vi } from "vitest";
import { CrmBridgeController, CrmBridgeGuard } from "./crm-bridge.controller.js";

function contextWithKey(key?: string) {
  return {
    switchToHttp: () => ({
      getRequest: () => ({ headers: key === undefined ? {} : { "x-api-key": key } }),
    }),
  } as never;
}

describe("CRM bridge HTTP boundary", () => {
  it("is hidden while disabled and rejects every key except the dedicated CRM key", () => {
    const disabled = new CrmBridgeGuard({ get: () => undefined } as never);
    expect(() => disabled.canActivate(contextWithKey("x".repeat(32)))).toThrow();

    const expected = "c".repeat(32);
    const enabled = new CrmBridgeGuard({ get: () => expected } as never);
    expect(() => enabled.canActivate(contextWithKey("x".repeat(32)))).toThrow(
      "CRM bridge authentication required",
    );
    expect(enabled.canActivate(contextWithKey(expected))).toBe(true);
  });

  it("accepts only a UUID externalRef and never accepts a partner/email selector", async () => {
    const bridge = {
      ensureReferralLink: vi.fn().mockResolvedValue({ ok: true }),
      referralProfile: vi.fn().mockResolvedValue({ ok: true }),
    };
    const controller = new CrmBridgeController(bridge as never);
    const externalRef = "10000000-0000-4000-8000-000000000001";

    await expect(controller.referralLink({ externalRef })).resolves.toEqual({ ok: true });
    await expect(controller.referralProfile(externalRef)).resolves.toEqual({ ok: true });
    await expect(controller.referralLink({
      externalRef,
      partnerCode: "attacker-selected",
    })).rejects.toThrow("invalid CRM referral link payload");
    await expect(controller.referralProfile("customer@example.test")).rejects.toThrow(
      "invalid CRM externalRef",
    );
  });
});
