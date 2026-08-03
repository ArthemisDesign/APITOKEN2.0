import { describe, expect, it, vi } from "vitest";
import { B2C_SIGNUP_BONUS_BALANCE_NANO } from "@claude-api/contracts";
import type { EngineClient } from "@claude-api/engine-client";
import { createFundedEngineAccount } from "./engine-provisioning.js";

describe("engine account welcome bonus funding", () => {
  it("credits an eligible OAuth B2C account with the exact idempotent welcome balance", async () => {
    const createAccount = vi.fn(async () => ({ account: "acct_b2c", multBp: 4000, handle: "user:user-1" }));
    const creditAccount = vi.fn(async () => ({ account: "acct_b2c", balance_nano: "5000000000", balance: "$5.000000000" }));
    const engine = { createAccount, creditAccount } as unknown as EngineClient;

    await createFundedEngineAccount(engine, {
      userId: "user-1", customerType: "b2c", handle: "user:user-1", multBp: 4000,
      welcomeBonusAmountNano: B2C_SIGNUP_BONUS_BALANCE_NANO,
    });

    expect(creditAccount).toHaveBeenCalledOnce();
    expect(creditAccount).toHaveBeenCalledWith(
      "acct_b2c", B2C_SIGNUP_BONUS_BALANCE_NANO, "signup-bonus:user-1",
    );
    expect(B2C_SIGNUP_BONUS_BALANCE_NANO).toBe(5_000_000_000n);
  });

  it("does not credit a password B2C account", async () => {
    const engine = {
      createAccount: vi.fn(async () => ({ account: "acct_password", multBp: 4000, handle: "user:user-password" })),
      creditAccount: vi.fn(),
    } as unknown as EngineClient;

    await createFundedEngineAccount(engine, {
      userId: "user-password", customerType: "b2c", handle: "user:user-password", multBp: 4000,
      welcomeBonusAmountNano: null,
    });

    expect(engine.creditAccount).not.toHaveBeenCalled();
  });

  it("does not apply the welcome balance to an invited B2B account", async () => {
    const engine = {
      createAccount: vi.fn(async () => ({ account: "acct_b2b", multBp: 2500, handle: "user:user-2" })),
      creditAccount: vi.fn(),
    } as unknown as EngineClient;

    await createFundedEngineAccount(engine, {
      userId: "user-2", customerType: "b2b", handle: "user:user-2", multBp: 2500,
      welcomeBonusAmountNano: null,
    });

    expect(engine.creditAccount).not.toHaveBeenCalled();
  });
});
