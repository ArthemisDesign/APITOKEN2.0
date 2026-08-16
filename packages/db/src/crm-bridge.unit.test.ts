import { describe, expect, it, vi } from "vitest";
import { listCrmReferralRegistrations } from "./crm-bridge.js";

describe("CRM referral registration projection", () => {
  it("scopes the query by alias and preserves exact money and pricing overrides", async () => {
    const query = vi.fn().mockResolvedValue({ rows: [{
      candidate_id: "10000000-0000-4000-8000-000000000001",
      email: "candidate@example.test",
      email_verified: true,
      registered_at: new Date("2026-08-16T10:00:00.000Z"),
      customer_status: "active",
      customer_type: "b2b",
      default_multiplier_bp: 4_500,
      provider_overrides: [{ providerId: "openai", multiplierBp: 3_500 }],
      paid_topup_nano: "9007199254740993",
      refunded_nano: "1000000000",
      usage_spent_nano: "8000000000",
      customer_funded_spent_nano: "7000000000",
      engine_account_id: "acct_candidate",
      projected_engine_status: "active",
    }] });

    const rows = await listCrmReferralRegistrations({ pool: { query } } as never, "r_opaquealias000000000000");

    expect(query).toHaveBeenCalledOnce();
    expect(query.mock.calls[0]![1]).toEqual(["r_opaquealias000000000000"]);
    expect(query.mock.calls[0]![0]).toContain("WHERE attribution.code = $1");
    expect(query.mock.calls[0]![0]).toContain("payment.status IN ('paid', 'refunded', 'disputed')");
    expect(query.mock.calls[0]![0]).toContain("COALESCE(evidence.paid_funded_nano, usage.real_funded_nano)");
    expect(rows[0]).toMatchObject({
      candidateId: "10000000-0000-4000-8000-000000000001",
      paidTopupNano: 9_007_199_254_740_993n,
      refundedNano: 1_000_000_000n,
      providerOverrides: [{ providerId: "openai", multiplierBp: 3_500 }],
    });
  });

  it("fails closed on a malformed pricing projection", async () => {
    const row = {
      candidate_id: "10000000-0000-4000-8000-000000000001",
      email: "candidate@example.test",
      email_verified: false,
      registered_at: new Date(),
      customer_status: "active",
      customer_type: "b2c",
      default_multiplier_bp: 4_000,
      provider_overrides: [{ providerId: "openai", multiplierBp: 10_001 }],
      paid_topup_nano: "0",
      refunded_nano: "0",
      usage_spent_nano: "0",
      customer_funded_spent_nano: "0",
      engine_account_id: null,
      projected_engine_status: null,
    };
    const database = { pool: { query: vi.fn().mockResolvedValue({ rows: [row] }) } } as never;

    await expect(listCrmReferralRegistrations(database, "r_opaquealias000000000000"))
      .rejects.toThrow("invalid CRM referral provider override row");
  });
});
