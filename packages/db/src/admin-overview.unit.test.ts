import { describe, expect, it } from "vitest";
import {
  listAdminAudit,
  listAdminAuditActions,
  listAdminTopups,
  listAdminUserOverview,
  type AdminSortDir,
  type AdminUserSort,
} from "./admin-overview.js";
import type { Database } from "./client.js";

// Unit-тесты SQL-контракта админских списков без живой БД: фейковый pool записывает текст и
// параметры каждого запроса. Проверяем белый список сортировок (в ORDER BY попадает только
// фрагмент из закрытой таблицы), маппинг фильтров, totals и обратную совместимость дефолта.

interface RecordedQuery {
  text: string;
  params: unknown[];
}

interface CannedResponse {
  when: (text: string) => boolean;
  rows: Array<Record<string, unknown>>;
}

function fakeDatabase(canned: CannedResponse[] = []): { database: Database; queries: RecordedQuery[] } {
  const queries: RecordedQuery[] = [];
  const pool = {
    query: (text: string, params?: unknown[]) => {
      queries.push({ text, params: params ?? [] });
      const match = canned.find((response) => response.when(text));
      return Promise.resolve({ rows: match?.rows ?? [] });
    },
  };
  return { database: { pool } as unknown as Database, queries };
}

describe("listAdminUserOverview sorting", () => {
  it("keeps the historical created_at DESC ordering by default", async () => {
    const { database, queries } = fakeDatabase();
    await listAdminUserOverview(database, { limit: 10 });
    expect(queries[0]!.text).toContain("ORDER BY u.created_at DESC");
    expect(queries[0]!.text).not.toContain("NULLS LAST");
    expect(queries[0]!.params).toEqual(["", "", "", "", 10, 0]);
  });

  it("collapses default and provider pricing jobs into one deterministic bundle status", async () => {
    const { database, queries } = fakeDatabase();
    await listAdminUserOverview(database, { customerType: "b2b" });
    expect(queries[0]!.text).not.toContain("LEFT JOIN engine_pricing_jobs pj");
    expect(queries[0]!.text).toContain("LEFT JOIN LATERAL");
    expect(queries[0]!.text).toContain("count(*) FILTER (WHERE job.status <> 'confirmed') OVER ()");
    expect(queries[0]!.text).toContain("WHEN 'retry' THEN 1");
  });

  it("maps every whitelisted sort to its aggregate expression, never the raw input", async () => {
    const cases: Array<[AdminUserSort, AdminSortDir, string]> = [
      ["paid_total", "desc", "ORDER BY COALESCE(p.paid_total, 0) DESC NULLS LAST, u.id ASC"],
      ["topup_total", "asc", "ORDER BY COALESCE(cp.cumulative_topup_nano, 0) ASC NULLS LAST, u.id ASC"],
      ["spent_30d", "desc", "ORDER BY COALESCE(ue.spent_30d, 0) DESC NULLS LAST, u.id ASC"],
      ["last_seen_at", "asc", "ORDER BY s.last_seen_at ASC NULLS LAST, u.id ASC"],
      ["created_at", "asc", "ORDER BY u.created_at ASC NULLS LAST, u.id ASC"],
    ];
    for (const [sort, dir, expected] of cases) {
      const { database, queries } = fakeDatabase();
      await listAdminUserOverview(database, { limit: 5, sort, dir });
      expect(queries[0]!.text).toContain(expected);
    }
  });

  it("rejects sort/dir values outside the whitelist instead of interpolating them", async () => {
    const { database, queries } = fakeDatabase();
    await expect(listAdminUserOverview(database, {
      limit: 5,
      sort: "created_at DESC; DROP TABLE users;--" as never,
    })).rejects.toThrow(/unsupported admin user sort/);
    await expect(listAdminUserOverview(database, {
      limit: 5,
      sort: "balance_usd" as never,
    })).rejects.toThrow(/unsupported admin user sort/);
    await expect(listAdminUserOverview(database, {
      limit: 5,
      dir: "DESC;--" as never,
    })).rejects.toThrow(/unsupported admin user sort dir/);
    expect(queries).toHaveLength(0);
  });
});

describe("listAdminTopups filters", () => {
  it("keeps the legacy default windows and adds per-list totals", async () => {
    const { database, queries } = fakeDatabase([
      {
        when: (text) => text.includes("count(*)::text AS total") && text.includes("FROM payments p"),
        rows: [{ total: "7" }],
      },
      {
        when: (text) => text.includes("count(*)::text AS total") && text.includes("FROM checkout_sessions cs"),
        rows: [{ total: "3" }],
      },
    ]);
    const page = await listAdminTopups(database, { limit: 20 });
    expect(queries).toHaveLength(4);
    // Дефолт без фильтров = историческое поведение: оплаченные платежи + неоплаченные чекауты.
    expect(queries[0]!.text).toContain("p.paid_at IS NOT NULL");
    expect(queries[0]!.text).toContain("ORDER BY COALESCE(p.paid_at, p.created_at) DESC, p.id");
    expect(queries[0]!.params).toEqual(["", "", "", 20, 0]);
    expect(queries[2]!.text).toContain("cs.status <> 'paid'");
    expect(queries[2]!.text).toContain("ORDER BY cs.created_at DESC, cs.id");
    expect(queries[2]!.params).toEqual(["", "", "", 20, 0]);
    expect(page.paymentsTotal).toBe(7);
    expect(page.checkoutsTotal).toBe(3);
    // COUNT-запросы получают те же фильтры, но без limit/offset.
    expect(queries[1]!.params).toEqual(["", "", ""]);
    expect(queries[3]!.params).toEqual(["", "", ""]);
  });

  it("applies q/provider/status to both lists and both totals", async () => {
    const { database, queries } = fakeDatabase();
    await listAdminTopups(database, {
      limit: 10,
      offset: 30,
      q: "Alice@Example.com ",
      provider: "cryptomus",
      status: "failed",
    });
    for (const query of queries) {
      expect(query.text).toContain("u.email ILIKE '%' || $1 || '%'");
      expect(query.params.slice(0, 3)).toEqual(["Alice@Example.com", "cryptomus", "failed"]);
    }
    // Заданный status заменяет окна по умолчанию точным совпадением в каждом списке.
    expect(queries[0]!.text).toContain("p.status::text = $3");
    expect(queries[2]!.text).toContain("cs.status::text = $3");
    expect(queries[0]!.params).toEqual(["Alice@Example.com", "cryptomus", "failed", 10, 30]);
    expect(queries[2]!.params).toEqual(["Alice@Example.com", "cryptomus", "failed", 10, 30]);
  });
});

describe("listAdminAudit filters", () => {
  it("keeps the unfiltered latest-first listing by default and adds a total", async () => {
    const { database, queries } = fakeDatabase([
      { when: (text) => text.includes("count(*)::text AS total"), rows: [{ total: "42" }] },
    ]);
    const page = await listAdminAudit(database, { limit: 100 });
    expect(queries).toHaveLength(2);
    expect(queries[0]!.text).toContain("FROM audit_log");
    expect(queries[0]!.text).toContain("ORDER BY created_at DESC, id DESC");
    expect(queries[0]!.params).toEqual(["", "", "", null, null, 100, 0]);
    expect(page.rows).toEqual([]);
    expect(page.total).toBe(42);
  });

  it("applies action/actor_type/q/date filters with offset pagination", async () => {
    const { database, queries } = fakeDatabase();
    const from = new Date("2026-07-01T00:00:00.000Z");
    const to = new Date("2026-07-31T23:59:59.000Z");
    await listAdminAudit(database, {
      limit: 50,
      offset: 25,
      action: "admin.credit",
      actorType: "commercial-admin",
      q: "ref-1",
      from,
      to,
    });
    const rows = queries[0]!;
    expect(rows.text).toContain("action = $1");
    expect(rows.text).toContain("actor_type = $2");
    expect(rows.text).toContain("target_id ILIKE '%' || $3 || '%' OR metadata::text ILIKE '%' || $3 || '%'");
    expect(rows.text).toContain("created_at >= $4");
    expect(rows.text).toContain("created_at <= $5");
    expect(rows.params).toEqual(["admin.credit", "commercial-admin", "ref-1", from, to, 50, 25]);
    expect(queries[1]!.params).toEqual(["admin.credit", "commercial-admin", "ref-1", from, to]);
  });
});

describe("listAdminAuditActions", () => {
  it("returns the distinct sorted action list for the filter dropdown", async () => {
    const { database, queries } = fakeDatabase([
      { when: () => true, rows: [{ action: "admin.credit" }, { action: "auth.login" }] },
    ]);
    await expect(listAdminAuditActions(database)).resolves.toEqual(["admin.credit", "auth.login"]);
    expect(queries[0]!.text).toContain("SELECT DISTINCT action FROM audit_log ORDER BY action");
  });
});
