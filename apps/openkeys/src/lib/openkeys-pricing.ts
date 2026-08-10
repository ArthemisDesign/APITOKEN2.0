import { type IssuedEngineApiKey } from "@claude-api/contracts";
import { EngineClientError, type EngineClient } from "@claude-api/engine-client";

/**
 * The one contract OpenKeys sells: face value at list price. It is a number on the engine
 * account — an OpenKeys account is 1:1 because its multiplier says so.
 */
export const OFFICIAL_ONE_TO_ONE_MULT_BP = 10_000;
export const OFFICIAL_ONE_TO_ONE_CONTRACT = "official_one_to_one";

export class OpenKeysPricingError extends Error {
  constructor(readonly code: string, message: string) {
    super(message);
    this.name = "OpenKeysPricingError";
  }
}

const PRICING_OVERRIDE_FIELDS = new Set([
  "discount",
  "discount_bps",
  "discountBps",
  "mult_bp",
  "multBp",
  "multiplier",
  "multiplier_bp",
  "multiplierBp",
  "pricing_contract",
  "pricingContract",
]);

/** API and direct service callers cannot smuggle an alternative economic contract. */
export function assertNoOpenKeysPricingOverride(input: object): void {
  const override = Object.keys(input).find((field) => PRICING_OVERRIDE_FIELDS.has(field));
  if (override !== undefined) {
    throw new OpenKeysPricingError(
      "pricing_override_forbidden",
      `OpenKeys pricing is fixed at 1:1; field ${override} is not accepted`,
    );
  }
}

export function assertOfficialEngineAccount(account: { account: string; multBp: number }): void {
  if (account.multBp !== OFFICIAL_ONE_TO_ONE_MULT_BP) {
    throw new OpenKeysPricingError(
      "engine_multiplier_mismatch",
      "engine did not create the OpenKeys account with the fixed 1:1 multiplier",
    );
  }
}

type OpenKeysPricingEngine = Pick<EngineClient, "creditAccount" | "issueKey">;


export interface IssuanceBlockReason {
  code: string;
  message: string;
}

/**
 * Безопасная причина недоступности выпуска для админ-UI: машинный код и
 * человекочитаемое описание без стеков, адресов движка и других внутренностей.
 */
export function describeIssuanceBlock(error: unknown): IssuanceBlockReason {
  if (error instanceof OpenKeysPricingError) {
    return {
      code: error.code,
      message: "Тариф OpenKeys не подтверждён — проверьте pricing authority в коммерции.",
    };
  }
  if (error instanceof EngineClientError) {
    return {
      code: "engine_unavailable",
      message: "Движок недоступен или вернул ошибку — проверьте состояние engine и ENGINE_CONTROL_KEY.",
    };
  }
  return {
    code: "authority_check_failed",
    message: "Не удалось проверить pricing authority — смотрите серверные логи OpenKeys.",
  };
}




/**
 * A new OpenKeys account is created, credited with its exact face value, and only then given a
 * key: the usable secret is issued last, so a half-provisioned account is never servable. Both
 * steps are idempotent under the issuance job's retry/compensation — the credit replays as
 * unchanged, and re-issuing returns the same account.
 */
export async function provisionOfficialOpenKeysCredential(
  engine: OpenKeysPricingEngine,
  input: {
    accountId: string;
    faceValueNano: bigint;
    creditReference: string;
    keyLabel: string;
    onCredited?: () => Promise<void>;
  },
): Promise<IssuedEngineApiKey> {
  if (input.faceValueNano <= 0n) {
    throw new OpenKeysPricingError("invalid_face_value", "OpenKeys face value must be positive");
  }
  await engine.creditAccount(input.accountId, input.faceValueNano, input.creditReference);
  await input.onCredited?.();
  return engine.issueKey(input.accountId, { label: input.keyLabel });
}

