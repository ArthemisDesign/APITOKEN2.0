import type { SalesDatabase } from "@claude-api/sales-db";
import { describe, expect, it, vi } from "vitest";
import { InternalPartnersController } from "./internal.controller.js";

describe("InternalPartnersController referral marker contract", () => {
  it("states explicitly that a claimed marker does not affect pricing", async () => {
    const query = vi.fn().mockResolvedValue({ rows: [{ discount_bps: 7_500 }], rowCount: 1 });
    const database = { pool: { query } } as unknown as SalesDatabase;
    const controller = new InternalPartnersController(database);

    await expect(controller.referralDiscount({
      code: "legacy-link",
      commerceUserId: "11111111-1111-4111-8111-111111111111",
    })).resolves.toEqual({ discountBps: 7_500, pricingAffected: false });
    expect(query).toHaveBeenCalledOnce();
  });

  it("keeps the non-pricing flag on invalid compatibility requests", async () => {
    const query = vi.fn();
    const database = { pool: { query } } as unknown as SalesDatabase;
    const controller = new InternalPartnersController(database);

    await expect(controller.referralDiscount({ code: "?", commerceUserId: "invalid" }))
      .resolves.toEqual({ discountBps: 0, pricingAffected: false });
    expect(query).not.toHaveBeenCalled();
  });
});
