import { describe, expect, it, vi } from "vitest";
import {
  findActiveReferralCommerceAccountByEmail,
  listReferralCommerceAccountsByIds,
} from "./referral-accounts.js";
import type { Database } from "./client.js";

function database(rows: unknown[]) {
  const query = vi.fn().mockResolvedValue({ rows });
  return {
    value: { pool: { query } } as unknown as Database,
    query,
  };
}

describe("Commerce account projection for Referral", () => {
  it("normalizes an email lookup and returns current account pricing", async () => {
    const db = database([{
      id: "11111111-1111-4111-8111-111111111111",
      email: "Partner@Example.test",
      email_verified: true,
      status: "active",
      customer_type: "b2b",
      multiplier_bp: 4_000,
      provider_discounts: [{ providerId: "openai", discountBps: 6_500 }],
    }]);

    await expect(findActiveReferralCommerceAccountByEmail(db.value, "  PARTNER@example.test ")).resolves.toEqual({
      id: "11111111-1111-4111-8111-111111111111",
      email: "Partner@Example.test",
      emailVerified: true,
      status: "active",
      customerType: "b2b",
      discountBps: 6_000,
      providerDiscounts: [{ providerId: "openai", discountBps: 6_500 }],
    });
    expect(db.query).toHaveBeenCalledWith(expect.stringContaining("lower(u.email) = $1"), ["partner@example.test"]);
  });

  it("deduplicates UUIDs and keeps missing profiles explicit", async () => {
    const id = "22222222-2222-4222-8222-222222222222";
    const db = database([{
      id,
      email: "member@example.test",
      email_verified: false,
      status: "disabled",
      customer_type: null,
      multiplier_bp: null,
      provider_discounts: null,
    }]);

    await expect(listReferralCommerceAccountsByIds(db.value, [id, id])).resolves.toEqual([expect.objectContaining({
      id,
      email: "member@example.test",
      status: "disabled",
      customerType: null,
      discountBps: null,
      providerDiscounts: [],
    })]);
    expect(db.query).toHaveBeenCalledWith(expect.stringContaining("u.id = ANY($1::uuid[])"), [[id]]);
  });

  it("does not query for an empty enrichment batch", async () => {
    const db = database([]);
    await expect(listReferralCommerceAccountsByIds(db.value, [])).resolves.toEqual([]);
    expect(db.query).not.toHaveBeenCalled();
  });
});
