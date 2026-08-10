import { describe, expect, it } from "vitest";
import { listAdminPayingUsers } from "./admin-finance.js";
import type { Database } from "./client.js";

type RecordedQuery = { text: string; params: unknown[] };

type FakeDatabase = {
  database: Database;
  queries: RecordedQuery[];
  transactionStatements: string[];
  state: { released: boolean };
};

function fakeDatabase(options: { selectError?: Error; rollbackError?: Error } = {}): FakeDatabase {
  const queries: RecordedQuery[] = [];
  const transactionStatements: string[] = [];
  const state = { released: false };
  const query = (text: string, params?: unknown[]) => {
    if (!text.includes("admin-finance:")) {
      transactionStatements.push(text);
      if (text === "ROLLBACK" && options.rollbackError !== undefined) {
        return Promise.reject(options.rollbackError);
      }
      return Promise.resolve({ rows: [] });
    }
    queries.push({ text, params: params ?? [] });
    if (options.selectError !== undefined) return Promise.reject(options.selectError);
    if (text.includes("admin-finance:paying-users-summary")) {
      return Promise.resolve({ rows: [{
        paying_users: "8", cohort_users: "10", bonus_only_users: "2", active_spenders: "7",
        paid_nano: "90000000000", manual_paid_nano: "10000000000", spent_nano: "34000000000",
        bonus_only_spent_nano: "2000000000", anthropic_nano: "14000000000",
        openai_nano: "15000000000", google_nano: "4000000000", kimi_nano: "3000000000",
        other_nano: "1000000000",
        anthropic_users: "5", openai_users: "4", google_users: "2", kimi_users: "3",
        other_users: "1",
      }] });
    }
    if (text.includes("admin-finance:paying-users-count")) {
      return Promise.resolve({ rows: [{ total: "3" }] });
    }
    return Promise.resolve({ rows: [{
      user_id: "user-1", email: "paid@example.com", display_name: "Paid User", status: "active",
      customer_type: "b2c", current_tier: 2, multiplier_bp: 5000, funding_kind: "payments_and_manual",
      paid_nano: "25000000000", payments_count: "2", manual_paid_nano: "5000000000",
      manual_topups_count: "1", last_paid_at: new Date("2026-07-30T10:00:00Z"),
      spent_nano: "7000000000", paid_funded_nano: "4000000000", bonus_funded_nano: "2000000000",
      other_funded_nano: "500000000", unattributed_nano: "500000000",
      anthropic_nano: "2000000000", openai_nano: "4000000000",
      google_nano: "1000000000", kimi_nano: "750000000", other_nano: "0",
      engine_account_id: "acct-user-1",
      usage_account_ids: ["acct-historical", "acct-user-1"], active_api_keys: "1",
      last_seen_at: new Date("2026-08-01T10:00:00Z"), created_at: new Date("2026-06-01T10:00:00Z"),
    }] });
  };
  const client = { query, release: () => { state.released = true; } };
  const pool = { connect: () => Promise.resolve(client) };
  return { database: { pool } as unknown as Database, queries, transactionStatements, state };
}

describe("listAdminPayingUsers", () => {
  it("returns exact provider money, stable totals and filtered pagination", async () => {
    const { database, queries, transactionStatements, state } = fakeDatabase();
    const page = await listAdminPayingUsers(database, {
      days: 7, limit: 25, offset: 50, q: "paid@", status: "active", provider: "openai",
      sort: "paid", dir: "asc",
    });

    expect(transactionStatements).toEqual([
      "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
      "COMMIT",
    ]);
    expect(state.released).toBe(true);
    expect(queries).toHaveLength(3);
    expect(queries[0]!.params).toEqual([7, "paid@", "active", "openai", "", 25, 50]);
    expect(queries[0]!.text).toContain("JOIN paid ON paid.user_id = u.id");
    // The paid/bonus split of spend comes from free-first accounting on the immutable usage
    // event; the retired per-request attribution table is not read at all.
    expect(queries[0]!.text).not.toContain("pricing_usage_attributions");
    expect(queries[0]!.text).toContain("e.real_funded_nano");
    expect(queries[0]!.text).toContain("AS exact_modern_funding");
    expect(queries[0]!.text).toContain("usage.event_count = usage.exact_modern_event_count");
    expect(queries[0]!.text).not.toContain("free_balance_nano");
    expect(queries[0]!.text).toContain("array_agg(DISTINCT engine_account_id ORDER BY engine_account_id)");
    expect(queries[0]!.text).toContain("e.occurred_at < now()");
    expect(queries[0]!.text).toContain("ORDER BY paid.paid_nano ASC NULLS LAST, u.id ASC");
    expect(queries[1]!.params).toEqual([7, "paid@", "active", "openai", ""]);
    expect(queries[1]!.text).toContain("e.occurred_at < now()");
    expect(queries[2]!.params).toEqual([7, ""]);
    expect(queries[2]!.text).toContain("e.occurred_at < now()");
    expect(page).toMatchObject({
      total: 3,
      days: 7,
      rows: [{
        userId: "user-1",
        fundingKind: "payments_and_manual",
        paidNano: "25000000000",
        spentNano: "7000000000",
        paidFundedSpentNano: "4000000000",
        bonusFundedSpentNano: "2000000000",
        otherFundedSpentNano: "500000000",
        unattributedSpentNano: "500000000",
        providerSpendNano: {
          anthropic: "2000000000", openai: "4000000000", google: "1000000000",
          kimi: "750000000", other: "0",
        },
        engineAccountId: "acct-user-1",
        usageAccountIds: ["acct-historical", "acct-user-1"],
      }],
      summary: {
        payingUsers: 8,
        cohortUsers: 10,
        bonusOnlyUsers: 2,
        activeSpenders: 7,
        bonusOnlySpentNano: "2000000000",
        providerSpendNano: {
          anthropic: "14000000000", openai: "15000000000", google: "4000000000",
          kimi: "3000000000", other: "1000000000",
        },
        providerUsers: { anthropic: 5, openai: 4, google: 2, kimi: 3, other: 1 },
      },
    });
  });

  it("считает Kimi отдельным провайдером и убирает его из «другого»", async () => {
    const { database, queries } = fakeDatabase();
    await listAdminPayingUsers(database, { days: 30, provider: "kimi" });

    const rowQuery = queries[0]!.text;
    expect(rowQuery).toContain("FILTER (WHERE provider_id = 'kimi'), 0) AS kimi_nano");
    // Named providers must all be excluded from the residual bucket: leaving `kimi` in it would
    // show the same money twice — once in its own column and once as «другое / legacy».
    expect(rowQuery).toContain("NOT IN ('anthropic', 'openai', 'google', 'kimi')");
    expect(rowQuery).toContain("$4 = 'kimi' AND COALESCE(usage.kimi_nano, 0) > 0");
    expect(queries[0]!.params).toContain("kimi");
  });

  it("сужает и страницу, и сводку, когда задана когорта источника денег", async () => {
    const { database, queries } = fakeDatabase();
    await listAdminPayingUsers(database, { days: 30, funding: "payments" });
    // Когорта уходит параметром во ВСЕ три запроса: иначе сводка осталась бы по всем платящим,
    // и «убрать админские начисления» не меняло бы итоговых сумм на плитках.
    for (const query of queries) expect(query.params).toContain("payments");
    expect(queries[0]!.text).toContain("$5 = 'payments' AND COALESCE(money.payments_count, 0) > 0");
    expect(queries[2]!.text).toContain("$2 = 'manual'");
  });

  it("accepts bonus/all and applies the same strict cohort predicate to rows, count and summary", async () => {
    for (const funding of ["bonus", "all"] as const) {
      const { database, queries } = fakeDatabase();
      await listAdminPayingUsers(database, { days: 7, funding });
      expect(queries).toHaveLength(3);
      for (const query of queries) {
        expect(query.params).toContain(funding);
        expect(query.text).toContain("usage.bonus_funded_nano = usage.spent_nano");
        expect(query.text).toContain("usage.unattributed_nano = 0");
      }
      expect(queries[0]!.text).toContain("$5::text IN ('', 'all')");
      expect(queries[1]!.text).toContain("$5 IN ('bonus', 'all')");
      expect(queries[2]!.text).toContain("$2 IN ('bonus', 'all')");
      expect(queries[2]!.text).toContain("paid.funding_kind IN ('payments', 'payments_and_manual', 'manual')");
    }
  });

  it("accepts spenders as an additive all-positive-spend cohort", async () => {
    const { database, queries } = fakeDatabase();
    await listAdminPayingUsers(database, { days: 1, funding: "spenders" });
    for (const query of queries) expect(query.params).toContain("spenders");
    expect(queries[0]!.text).toContain("$5 = 'spenders' AND COALESCE(usage.spent_nano, 0) > 0");
    expect(queries[2]!.text).toContain("$2 = 'spenders' AND COALESCE(usage.spent_nano, 0) > 0");
    expect(queries[0]!.text).toContain("ELSE 'spend_only'");
  });

  it("preserves a SELECT failure when rollback also fails and always releases the client", async () => {
    const selectError = new Error("page select failed");
    const { database, transactionStatements, state } = fakeDatabase({
      selectError,
      rollbackError: new Error("rollback failed"),
    });

    await expect(listAdminPayingUsers(database, { days: 7 })).rejects.toBe(selectError);
    expect(transactionStatements).toEqual([
      "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
      "ROLLBACK",
    ]);
    expect(state.released).toBe(true);
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
