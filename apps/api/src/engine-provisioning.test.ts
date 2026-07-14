import { describe, expect, it, vi } from "vitest";
import { B2C_SIGNUP_BONUS_BALANCE_NANO } from "@claude-api/contracts";
import type { EngineClient } from "@claude-api/engine-client";
import { createFundedEngineAccount } from "./engine-provisioning.js";

describe("engine account signup funding", () => {
  it("credits a B2C account with the exact idempotent signup balance", async () => {
    const createAccount = vi.fn(async () => ({ account: "acct_b2c", multBp: 4000, handle: "user:user-1" }));
    const creditAccount = vi.fn(async () => ({ account: "acct_b2c", balance_nano: "4000000000", balance: "$4.000000000" }));
    const engine = { createAccount, creditAccount } as unknown as EngineClient;

    await createFundedEngineAccount(engine, {
      userId: "user-1", customerType: "b2c", handle: "user:user-1", multBp: 4000,
    });

    expect(creditAccount).toHaveBeenCalledOnce();
    expect(creditAccount).toHaveBeenCalledWith(
      "acct_b2c", B2C_SIGNUP_BONUS_BALANCE_NANO, "signup-bonus:user-1",
    );
    expect(B2C_SIGNUP_BONUS_BALANCE_NANO).toBe(4_000_000_000n);
  });

  it("does not apply the B2C signup balance to an invited B2B account", async () => {
    const engine = {
      createAccount: vi.fn(async () => ({ account: "acct_b2b", multBp: 2500, handle: "user:user-2" })),
      creditAccount: vi.fn(),
    } as unknown as EngineClient;

    await createFundedEngineAccount(engine, {
      userId: "user-2", customerType: "b2b", handle: "user:user-2", multBp: 2500,
    });

    expect(engine.creditAccount).not.toHaveBeenCalled();
  });
});
