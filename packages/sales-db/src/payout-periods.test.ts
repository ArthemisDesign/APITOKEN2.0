import { describe, expect, it, vi } from "vitest";
import type { SalesDatabase } from "./client.js";
import { getDuePayoutList } from "./payout-periods.js";

describe("payout-periods due-list projection", () => {
  it("projects email as display metadata without replacing immutable partner identity", async () => {
    const query = vi.fn().mockResolvedValue({
      rows: [{
        partner_id: "11111111-1111-4111-8111-111111111111",
        email: "partner@example.test",
        telegram_username: "partner_handle",
        display_name: "Partner Name",
        status: "active",
        gross: "100",
        adjustment: "0",
        net: "100",
        paid: "0",
        committed: "0",
        payout_method: "usdt-bep20",
        payout_details: { address: "0x1111111111111111111111111111111111111111" },
      }],
    });
    const database = { pool: { query } } as unknown as SalesDatabase;

    const result = await getDuePayoutList(database, new Date("2026-07-24T12:00:00.000Z"), 10n);

    expect(result.items).toEqual([expect.objectContaining({
      partnerId: "11111111-1111-4111-8111-111111111111",
      email: "partner@example.test",
      payableNano: "100",
      eligible: true,
    })]);
    expect(query.mock.calls[0]?.[0]).toContain("p.email");
  });
});
