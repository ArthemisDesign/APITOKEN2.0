import { describe, expect, it } from "vitest";
import { listAdminPayingUsers } from "./admin-finance.js";
import type { Database } from "./client.js";

type RecordedQuery = { text: string; params: unknown[] };

function fakeDatabase(): { database: Database; queries: RecordedQuery[] } {
  const queries: RecordedQuery[] = [];
  const pool = {
    query: (text: string, params?: unknown[]) => {
      queries.push({ text, params: params ?? [] });
      if (text.includes("admin-finance:paying-users-summary")) {
        return Promise.resolve({ rows: [{
          paying_users: "8", active_spenders: "5", paid_nano: "90000000000", spent_nano: "32000000000",
          anthropic_nano: "12000000000", openai_nano: "15000000000", google_nano: "4000000000",
          other_nano: "1000000000", anthropic_users: "3", openai_users: "4", google_users: "2", other_users: "1",
        }] });
      }
      if (text.includes("admin-finance:paying-users-count")) {
        return Promise.resolve({ rows: [{ total: "3" }] });
      }
      return Promise.resolve({ rows: [{
        user_id: "user-1", email: "paid@example.com", display_name: "Paid User", status: "active",
        customer_type: "b2c", current_tier: 2, multiplier_bp: 5000,
        paid_nano: "25000000000", payments_count: "2", last_paid_at: new Date("2026-07-30T10:00:00Z"),
        spent_nano: "7000000000", anthropic_nano: "2000000000", openai_nano: "4000000000",
        google_nano: "1000000000", other_nano: "0", active_api_keys: "1",
        last_seen_at: new Date("2026-08-01T10:00:00Z"), created_at: new Date("2026-06-01T10:00:00Z"),
      }] });
    },
  };
  return { database: { pool } as unknown as Database, queries };
}

describe("listAdminPayingUsers", () => {
  it("returns exact provider money, stable totals and filtered pagination", async () => {
    const { database, queries } = fakeDatabase();
    const page = await listAdminPayingUsers(database, {
      days: 7, limit: 25, offset: 50, q: "paid@", status: "active", provider: "openai",
      sort: "paid", dir: "asc",
    });

    expect(queries).toHaveLength(3);
    expect(queries[0]!.params).toEqual([7, "paid@", "active", "openai", "", 25, 50]);
    expect(queries[0]!.text).toContain("JOIN paid ON paid.user_id = u.id");
    expect(queries[0]!.text).toContain("pricing_usage_attributions");
    expect(queries[0]!.text).toContain("COALESCE(a.provider_id, e.provider_id) = 'anthropic'");
    expect(queries[0]!.text).toContain("COALESCE(a.provider_id, e.provider_id) = 'openai'");
    expect(queries[0]!.text).toContain("COALESCE(a.provider_id, e.provider_id) = 'google'");
    expect(queries[0]!.text).toContain("ORDER BY paid.paid_nano ASC NULLS LAST, u.id ASC");
    expect(queries[1]!.params).toEqual([7, "paid@", "active", "openai", ""]);
    expect(queries[2]!.params).toEqual([7, ""]);
    expect(page).toMatchObject({
      total: 3,
      days: 7,
      rows: [{
        userId: "user-1",
        paidNano: "25000000000",
        spentNano: "7000000000",
        providerSpendNano: { anthropic: "2000000000", openai: "4000000000", google: "1000000000", other: "0" },
      }],
      summary: {
        payingUsers: 8,
        activeSpenders: 5,
        providerSpendNano: {
          anthropic: "12000000000", openai: "15000000000", google: "4000000000", other: "1000000000",
        },
        providerUsers: { anthropic: 3, openai: 4, google: 2, other: 1 },
      },
    });
  });

  it("сужает и страницу, и сводку, когда задана когорта источника денег", async () => {
    const { database, queries } = fakeDatabase();
    await listAdminPayingUsers(database, { days: 30, funding: "payments" });
    // Когорта уходит параметром во ВСЕ три запроса: иначе сводка осталась бы по всем платящим,
    // и «убрать админские начисления» не меняло бы итоговых сумм на плитках.
    for (const query of queries) expect(query.params).toContain("payments");
    expect(queries[0]!.text).toContain("$5 = 'payments' AND sum(payments_count) > 0");
    expect(queries[2]!.text).toContain("$2 = 'manual' AND sum(manual_count) > 0 AND sum(payments_count) = 0");
  });

  it("rejects sort, direction, provider and window values outside closed enums", async () => {
    const { database, queries } = fakeDatabase();
    await expect(listAdminPayingUsers(database, { days: 30, sort: "DROP TABLE users" as never }))
      .rejects.toThrow(/unsupported paying users sort/);
    await expect(listAdminPayingUsers(database, { days: 30, dir: "sideways" as never }))
      .rejects.toThrow(/unsupported paying users sort dir/);
    await expect(listAdminPayingUsers(database, { days: 30, provider: "future" as never }))
      .rejects.toThrow(/unsupported paying users provider/);
    await expect(listAdminPayingUsers(database, { days: 365 as never }))
      .rejects.toThrow(/unsupported paying users window/);
    await expect(listAdminPayingUsers(database, { days: 30, funding: "gift" as never }))
      .rejects.toThrow(/unsupported paying users funding/);
    expect(queries).toHaveLength(0);
  });
});
