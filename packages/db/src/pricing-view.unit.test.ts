import { describe, expect, it } from "vitest";
import type { Database } from "./client.js";
import { getPricingView } from "./pricing.js";

function databaseRow(row: { customer_type: "b2c" | "b2b"; multiplier_bp: number } | undefined): Database {
  return {
    pool: {
      query: async () => ({ rows: row === undefined ? [] : [row] }),
    },
  } as unknown as Database;
}

describe("pricing view", () => {
  it("shows the persisted B2C scalar instead of substituting today's common default", async () => {
    await expect(getPricingView(databaseRow({ customer_type: "b2c", multiplier_bp: 4_000 }), "user-1"))
      .resolves.toEqual({
        customerType: "b2c",
        pricingMode: "flat",
        discountPercent: 60,
        multiplierBp: 4_000,
      });
  });

  it("keeps the negotiated B2B scalar and returns null for an unknown profile", async () => {
    await expect(getPricingView(databaseRow({ customer_type: "b2b", multiplier_bp: 2_500 }), "user-2"))
      .resolves.toEqual({
        customerType: "b2b",
        pricingMode: "manual",
        discountPercent: 75,
        multiplierBp: 2_500,
      });
    await expect(getPricingView(databaseRow(undefined), "missing")).resolves.toBeNull();
  });
});
