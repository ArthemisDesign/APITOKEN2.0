import { type IssuedEngineApiKey } from "@claude-api/contracts";
import { EngineClientError, type EngineClient } from "@claude-api/engine-client";

/**
 * The one contract OpenKeys sells: face value at list price. It is a number on the engine
 * account — an OpenKeys account is 1:1 because its multiplier says so.
 */
export const OFFICIAL_ONE_TO_ONE_MULT_BP = 10_000;
// Литерал обязан совпадать с CHECK-констрейнтом в packages/openkeys-db/migrations/
// 0007_openkeys_pricing_contract_expand.sql. Разошёлся 2026-08-09 (`official_one_to_one` в коде
// против `official_1_to_1` в БД) — следующий выпуск партии упал бы на констрейнте. Тип сужен до
// того же union, что и у строк, чтобы расхождение ловилось компилятором, а не продом.
export const OFFICIAL_ONE_TO_ONE_CONTRACT: "official_1_to_1" = "official_1_to_1";

export interface OpenKeysDatabaseContractRow {
  kind: "column" | "constraint";
  name: string;
  definition: string;
}

/**
 * Read-only PostgreSQL catalog proof for the exact columns and constraints the issuance writer
 * relies on. `NOT VALID` is stripped because migration 0007 deliberately left historical rows
 * unvalidated while still enforcing every new INSERT/UPDATE; a later validation is a compatible
 * strengthening and must not make readiness fail.
 */
export const OPENKEYS_DATABASE_CONTRACT_QUERY = `
  SELECT 'column'::text AS kind,
         relation.relname || '.' || attribute.attname AS name,
         format_type(attribute.atttypid, attribute.atttypmod)
           || CASE WHEN attribute.attnotnull THEN ' NOT NULL' ELSE ' NULL' END AS definition
  FROM pg_attribute attribute
  JOIN pg_class relation ON relation.oid = attribute.attrelid
  JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
  WHERE namespace.nspname = 'public'
    AND relation.relname IN ('openkeys_batches', 'openkeys_keys')
    AND attribute.attname = 'pricing_contract'
    AND NOT attribute.attisdropped
  UNION ALL
  SELECT 'constraint'::text AS kind,
         constraint_row.conname AS name,
         constraint_row.contype::text || ':'
           || regexp_replace(pg_get_constraintdef(constraint_row.oid, true), ' NOT VALID$', '')
           AS definition
  FROM pg_constraint constraint_row
  JOIN pg_class relation ON relation.oid = constraint_row.conrelid
  JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
  WHERE namespace.nspname = 'public'
    AND relation.relname IN ('openkeys_batches', 'openkeys_keys')
    AND constraint_row.conname IN (
      'openkeys_batches_pricing_contract',
      'openkeys_batches_official_1_to_1',
      'openkeys_keys_pricing_contract',
      'openkeys_keys_official_1_to_1',
      'openkeys_keys_batch_contract_fk'
    )
  ORDER BY name
`;

const EXPECTED_OPENKEYS_DATABASE_CONTRACT = new Map<string, string>([
  ["openkeys_batches.pricing_contract", "column:text NOT NULL"],
  ["openkeys_keys.pricing_contract", "column:text NOT NULL"],
  [
    "openkeys_batches_pricing_contract",
    "constraint:c:CHECK (pricing_contract = ANY (ARRAY['legacy'::text, 'official_1_to_1'::text]))",
  ],
  [
    "openkeys_batches_official_1_to_1",
    "constraint:c:CHECK (pricing_contract <> 'official_1_to_1'::text OR mult_bp = 10000)",
  ],
  [
    "openkeys_keys_pricing_contract",
    "constraint:c:CHECK (pricing_contract = ANY (ARRAY['legacy'::text, 'official_1_to_1'::text]))",
  ],
  [
    "openkeys_keys_official_1_to_1",
    "constraint:c:CHECK (pricing_contract <> 'official_1_to_1'::text OR mult_bp = 10000)",
  ],
  [
    "openkeys_keys_batch_contract_fk",
    "constraint:f:FOREIGN KEY (batch_id, pricing_contract) REFERENCES openkeys_batches(id, pricing_contract) ON DELETE RESTRICT",
  ],
]);

/** Fail closed unless PostgreSQL exposes the exact contract used by the service writer. */
export function assertOpenKeysDatabaseContract(rows: readonly OpenKeysDatabaseContractRow[]): void {
  const observed = new Map<string, string>();
  for (const row of rows) {
    if (observed.has(row.name)) {
      throw new OpenKeysPricingError(
        "pricing_database_contract_mismatch",
        `duplicate PostgreSQL contract row ${row.name}`,
      );
    }
    observed.set(row.name, `${row.kind}:${row.definition}`);
  }
  for (const [name, expected] of EXPECTED_OPENKEYS_DATABASE_CONTRACT) {
    if (observed.get(name) !== expected) {
      throw new OpenKeysPricingError(
        "pricing_database_contract_mismatch",
        `PostgreSQL contract row ${name} is missing or differs from the issuance contract`,
      );
    }
  }
  if (observed.size !== EXPECTED_OPENKEYS_DATABASE_CONTRACT.size) {
    throw new OpenKeysPricingError(
      "pricing_database_contract_mismatch",
      "PostgreSQL returned an unexpected issuance contract row",
    );
  }
}

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
      message: "Контракт выпуска OpenKeys не подтверждён — проверьте схему базы OpenKeys.",
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
    message: "Не удалось проверить контракт выпуска — смотрите серверные логи OpenKeys.",
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
