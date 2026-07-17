import {
  B2C_SIGNUP_BONUS_BALANCE_NANO,
  type CreateEngineAccount,
} from "@claude-api/contracts";
import type { EngineClient } from "@claude-api/engine-client";

export async function createFundedEngineAccount(
  engine: EngineClient,
  input: CreateEngineAccount & {
    userId: string;
    customerType: "b2c" | "b2b";
    welcomeBonusEligible: boolean;
  },
): Promise<{ account: string; multBp: number; handle: string | null }> {
  const account = await engine.createAccount({ handle: input.handle, multBp: input.multBp });
  if (input.customerType === "b2c" && input.welcomeBonusEligible) {
    await engine.creditAccount(
      account.account,
      B2C_SIGNUP_BONUS_BALANCE_NANO,
      `signup-bonus:${input.userId}`,
    );
  }
  return account;
}
