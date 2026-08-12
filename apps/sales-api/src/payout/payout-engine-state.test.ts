import type { ConfigService } from "@nestjs/config";
import { describe, expect, it, vi } from "vitest";
import type { Environment } from "../config.js";
import type { PayoutChain } from "./chain.js";
import { PayoutService } from "./payout.service.js";

const HOT_WALLET = "0x19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A";

function config(overrides: Record<string, unknown> = {}): ConfigService<Environment, true> {
  const values: Record<string, unknown> = {
    PAYOUT_HOT_WALLET_KEY: `0x${"11".repeat(32)}`,
    PAYOUT_SEND_RPC_URL: "https://bsc.invalid",
    PAYOUT_ENFORCE_WINDOW: false,
    ...overrides,
  };
  return { get: (key: string) => values[key] } as unknown as ConfigService<Environment, true>;
}

function chain() {
  return {
    hotAddress: HOT_WALLET,
    assertReady: vi.fn().mockResolvedValue(undefined),
    balances: vi.fn().mockResolvedValue({
      usdtWei: 17_953_884_700_000_000_000n,
      bnbWei: 2_500_000_000_000_000n,
    }),
    gasCostPerTransferWei: vi.fn().mockReturnValue(5_000_000_000_000n),
  };
}

class TestPayoutService extends PayoutService {
  constructor(cfg: ConfigService<Environment, true>, private readonly testChain: ReturnType<typeof chain>) {
    super({} as never, cfg, {} as never);
  }

  protected override createChain(): PayoutChain {
    return this.testChain as unknown as PayoutChain;
  }
}

describe("PayoutService.engineState", () => {
  it("reports the public wallet and canonical read-only balances", async () => {
    const fake = chain();
    const state = await new TestPayoutService(config(), fake).engineState();

    expect(state).toEqual({
      configured: true,
      window: expect.objectContaining({ open: true, enforced: false }),
      chain: {
        ready: true,
        hotWalletAddress: HOT_WALLET,
        usdtBalanceNano: "17953884700",
        bnbBalanceWei: "2500000000000000",
        gasCostPerTransferWei: "5000000000000",
        issue: null,
      },
    });
    expect(fake.assertReady).toHaveBeenCalledOnce();
    expect(fake.balances).toHaveBeenCalledOnce();
  });

  it("marks an unconfigured engine without attempting a chain read", async () => {
    const fake = chain();
    const state = await new TestPayoutService(config({
      PAYOUT_HOT_WALLET_KEY: undefined,
      PAYOUT_SEND_RPC_URL: undefined,
    }), fake).engineState();

    expect(state.configured).toBe(false);
    expect(state.chain).toEqual({
      ready: false,
      hotWalletAddress: null,
      usdtBalanceNano: null,
      bnbBalanceWei: null,
      gasCostPerTransferWei: null,
      issue: "not_configured",
    });
    expect(fake.assertReady).not.toHaveBeenCalled();
  });

  it("fails closed without returning a provider diagnostic as money", async () => {
    const fake = chain();
    fake.assertReady.mockRejectedValueOnce(new Error("RPC https://secret.invalid rejected token"));
    const state = await new TestPayoutService(config(), fake).engineState();

    expect(state.configured).toBe(true);
    expect(state.chain).toEqual({
      ready: false,
      hotWalletAddress: null,
      usdtBalanceNano: null,
      bnbBalanceWei: null,
      gasCostPerTransferWei: null,
      issue: "read_unavailable",
    });
  });
});
