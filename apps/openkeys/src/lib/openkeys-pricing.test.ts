import { describe, expect, it } from "vitest";
import { EngineClientError } from "@claude-api/engine-client";
import {
  assertNoOpenKeysPricingOverride,
  assertOpenKeysDatabaseContract,
  assertOfficialEngineAccount,
  describeIssuanceBlock,
  type OpenKeysDatabaseContractRow,
  OFFICIAL_ONE_TO_ONE_MULT_BP,
  OpenKeysPricingError,
} from "./openkeys-pricing.js";

const validDatabaseContract: readonly OpenKeysDatabaseContractRow[] = [
  { kind: "column", name: "openkeys_batches.pricing_contract", definition: "text NOT NULL" },
  { kind: "column", name: "openkeys_keys.pricing_contract", definition: "text NOT NULL" },
  {
    kind: "constraint",
    name: "openkeys_batches_pricing_contract",
    definition: "c:CHECK (pricing_contract = ANY (ARRAY['legacy'::text, 'official_1_to_1'::text]))",
  },
  {
    kind: "constraint",
    name: "openkeys_batches_official_1_to_1",
    definition: "c:CHECK (pricing_contract <> 'official_1_to_1'::text OR mult_bp = 10000)",
  },
  {
    kind: "constraint",
    name: "openkeys_keys_pricing_contract",
    definition: "c:CHECK (pricing_contract = ANY (ARRAY['legacy'::text, 'official_1_to_1'::text]))",
  },
  {
    kind: "constraint",
    name: "openkeys_keys_official_1_to_1",
    definition: "c:CHECK (pricing_contract <> 'official_1_to_1'::text OR mult_bp = 10000)",
  },
  {
    kind: "constraint",
    name: "openkeys_keys_batch_contract_fk",
    definition: "f:FOREIGN KEY (batch_id, pricing_contract) REFERENCES openkeys_batches(id, pricing_contract) ON DELETE RESTRICT",
  },
];

describe("OpenKeys official 1:1 pricing", () => {

  describe("database issuance contract", () => {
    it("accepts the exact migration-0007 shape", () => {
      expect(() => assertOpenKeysDatabaseContract(validDatabaseContract)).not.toThrow();
    });

    it("fails closed on a missing constraint, nullable column, or literal drift", () => {
      expect(() => assertOpenKeysDatabaseContract(validDatabaseContract.slice(1)))
        .toThrow(/missing or differs/u);
      expect(() => assertOpenKeysDatabaseContract(validDatabaseContract.map((row) => (
        row.name === "openkeys_batches.pricing_contract"
          ? { ...row, definition: "text NULL" }
          : row
      )))).toThrow(/missing or differs/u);
      expect(() => assertOpenKeysDatabaseContract(validDatabaseContract.map((row) => (
        row.name === "openkeys_batches_pricing_contract"
          ? { ...row, definition: row.definition.replace("official_1_to_1", "official_one_to_one") }
          : row
      )))).toThrow(/missing or differs/u);
    });
  });






  it("rejects multiplier, discount, and pricing-contract overrides at every caller boundary", () => {
    for (const field of [
      "multBp",
      "mult_bp",
      "multiplierBp",
      "discountBps",
      "discount_bps",
      "pricingContract",
      "pricing_contract",
    ]) {
      expect(() => assertNoOpenKeysPricingOverride({ [field]: 9_999 }), field)
        .toThrow("fixed at 1:1");
    }
    expect(() => assertNoOpenKeysPricingOverride({ faceValueNano: 50_000_000_000n })).not.toThrow();
    expect(() => assertOfficialEngineAccount({ account: "acct_ok", multBp: 9_999 }))
      .toThrow("fixed 1:1 multiplier");
    expect(() => assertOfficialEngineAccount({
      account: "acct_ok",
      multBp: OFFICIAL_ONE_TO_ONE_MULT_BP,
    })).not.toThrow();
  });




  describe("describeIssuanceBlock", () => {
    it("передаёт код pricing-ошибки без утечки внутреннего сообщения", () => {
      const reason = describeIssuanceBlock(
        new OpenKeysPricingError("pricing_database_contract_mismatch", "internal constraint detail"),
      );
      expect(reason.code).toBe("pricing_database_contract_mismatch");
      expect(reason.message).toContain("Контракт выпуска");
      expect(reason.message).not.toContain("internal constraint detail");
    });

    it("сетевую/HTTP-ошибку движка отличает от неподтверждённого authority", () => {
      const reason = describeIssuanceBlock(
        new EngineClientError("engine request failed", undefined, true),
      );
      expect(reason.code).toBe("engine_unavailable");
      expect(reason.message).toContain("Движок недоступен");
    });

    it("прочие ошибки сворачивает в общий код без внутренностей", () => {
      const reason = describeIssuanceBlock(new Error("ENGINE_BASE_URL must be an absolute URL"));
      expect(reason.code).toBe("authority_check_failed");
      expect(reason.message).not.toContain("ENGINE_BASE_URL");
    });
  });



});
