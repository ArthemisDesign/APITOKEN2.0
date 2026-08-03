import type { CreateEngineAccount } from "@claude-api/contracts";
import type { EngineClient } from "@claude-api/engine-client";

export async function createFundedEngineAccount(
  engine: EngineClient,
  input: CreateEngineAccount & {
    userId: string;
    customerType: "b2c" | "b2b";
    welcomeBonusAmountNano: bigint | null;
  },
): Promise<{ account: string; multBp: number; handle: string | null }> {
  const account = await engine.createAccount({ handle: input.handle, multBp: input.multBp });
  if (input.welcomeBonusAmountNano !== null) {
    await engine.creditAccount(
      account.account,
      input.welcomeBonusAmountNano,
      `signup-bonus:${input.userId}`,
    );
  }
  return account;
}
