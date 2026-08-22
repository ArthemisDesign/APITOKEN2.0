import { describe, expect, it, vi } from "vitest";
import { PayoutController, payoutRowIdentity } from "./payout.controller.js";
import type { PayoutService } from "./payout.service.js";

describe("PayoutController.engine", () => {
  it("passes through the additive read-only chain readiness projection", async () => {
    const state = {
      configured: true,
      window: { open: true, opensAt: null, closesAt: null, enforced: false },
      chain: {
        ready: true,
        hotWalletAddress: "0x19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A",
        usdtBalanceNano: "17953884700",
        bnbBalanceWei: "2500000000000000",
        gasCostPerTransferWei: "5000000000000",
        issue: null,
      },
    } as const;
    const engineState = vi.fn().mockResolvedValue(state);
    const controller = new PayoutController({ engineState } as unknown as PayoutService);

    await expect(controller.engine()).resolves.toEqual(state);
    expect(engineState).toHaveBeenCalledOnce();
  });
});

describe("payoutRowIdentity", () => {
  const base = {
    partnerId: "12345678-aaaa-bbbb-cccc-123456789012",
    email: null,
    displayName: null,
    telegramUsername: null,
  };

  it("uses account email before every legacy display fallback", () => {
    expect(payoutRowIdentity({
      ...base,
      email: "partner@example.com",
      displayName: "Partner name",
      telegramUsername: "partner_handle",
    })).toBe("partner@example.com");
  });

  it("falls back through display name, Telegram and the bounded id prefix", () => {
    expect(payoutRowIdentity({ ...base, displayName: "Partner name", telegramUsername: "partner_handle" })).toBe("Partner name");
    expect(payoutRowIdentity({ ...base, telegramUsername: "partner_handle" })).toBe("@partner_handle");
    expect(payoutRowIdentity(base)).toBe("12345678");
  });
});
