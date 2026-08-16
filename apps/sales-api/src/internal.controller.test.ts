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

describe("InternalPartnersController external referral aliases", () => {
  it("returns an idempotent opaque alias without exposing a price marker", async () => {
    const createdAt = new Date("2026-08-16T10:00:00.000Z");
    const query = vi.fn(async (statement: string) => {
      if (statement.includes("SELECT id FROM partners")) return { rows: [{ id: "partner-1" }], rowCount: 1 };
      if (statement.includes("FROM external_referral_aliases") && statement.includes("WHERE source")) {
        return { rows: [], rowCount: 0 };
      }
      if (statement.includes("INSERT INTO external_referral_aliases")) {
        return {
          rows: [{
            source: "crm",
            external_ref: "contact:11111111-1111-4111-8111-111111111111",
            alias_code: "r_abcdefghijklmnopqrstuvwx",
            partner_id: "partner-1",
            created_at: createdAt,
          }],
          rowCount: 1,
        };
      }
      return { rows: [], rowCount: 0 };
    });
    const client = { query, release: vi.fn() };
    const database = { pool: { connect: vi.fn().mockResolvedValue(client) } } as unknown as SalesDatabase;
    const controller = new InternalPartnersController(database);

    await expect(controller.externalReferralAlias({
      source: "crm",
      externalRef: "contact:11111111-1111-4111-8111-111111111111",
      partnerCode: "CRM-OWNER",
    })).resolves.toEqual({
      source: "crm",
      externalRef: "contact:11111111-1111-4111-8111-111111111111",
      code: "r_abcdefghijklmnopqrstuvwx",
      partnerId: "partner-1",
      createdAt: createdAt.toISOString(),
    });
    expect(query).toHaveBeenCalledWith(
      expect.stringContaining("SELECT id FROM partners"),
      ["crm-owner"],
    );
    expect(client.release).toHaveBeenCalledOnce();
  });
});
