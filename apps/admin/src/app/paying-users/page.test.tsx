import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import PayingUsersPage from "./page";
import {
  buildPayingUsersCsvRows,
  INITIAL_PAYING_USERS_PAGE,
  payingTierLabel,
  payingUsersQuery,
  providerShareBp,
} from "./paying-users-lib";

describe("Платящие (paying users page)", () => {
  it("рендерит доступную навигацию и загрузочный каркас без запроса при SSR", () => {
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const html = renderToString(<PayingUsersPage />);
    expect(html).toContain("Платящие");
    expect(html).toContain("денежный радар");
    expect(html).toContain("loading-grid");
    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});

describe("payingUsersQuery", () => {
  it("строит стабильный серверный запрос и пропускает пустые фильтры", () => {
    expect(payingUsersQuery(INITIAL_PAYING_USERS_PAGE)).toBe(
      "days=30&limit=50&offset=0&sort=spent&dir=desc",
    );
  });

  it("добавляет поиск, статус и провайдера", () => {
    expect(payingUsersQuery({
      ...INITIAL_PAYING_USERS_PAGE,
      days: 7,
      q: "paid user@example.com",
      status: "active",
      provider: "openai",
      sort: "paid",
      dir: "asc",
    })).toBe(
      "days=7&limit=50&offset=0&sort=paid&dir=asc&q=paid+user%40example.com&status=active&provider=openai",
    );
  });
});

describe("providerShareBp", () => {
  it("считает долю через BigInt даже для сумм выше safe integer", () => {
    expect(providerShareBp("9007199254740993000", "18014398509481986000")).toBe(5000);
    expect(providerShareBp("1", "3")).toBe(3333);
  });

  it("безопасно обрабатывает пустые, ошибочные и превышающие total значения", () => {
    expect(providerShareBp("1", "0")).toBe(0);
    expect(providerShareBp("not-a-number", "10")).toBe(0);
    expect(providerShareBp("20", "10")).toBe(10_000);
  });
});

describe("paying customer helpers", () => {
  it("показывает B2B независимо от тира и имена B2C-тиpов", () => {
    expect(payingTierLabel({ customer_type: "b2b", tier: 0 })).toBe("B2B");
    expect(payingTierLabel({ customer_type: "b2c", tier: 2 })).toBe("Pro");
    expect(payingTierLabel({ customer_type: "b2c", tier: 8 })).toBe("—");
  });

  it("экспортирует точные nanoUSD-строки без float-конвертации", () => {
    expect(buildPayingUsersCsvRows([{
      email: "paid@example.com",
      display_name: "Paid",
      status: "active",
      customer_type: "b2c",
      tier: 1,
      paid_nano: "25000000001",
      payments_count: 2,
      spent_nano: "7000000001",
      provider_spend: {
        anthropic_nano: "2000000001",
        openai_nano: "4000000000",
        google_nano: "1000000000",
      },
      last_paid_at: "2026-07-30T10:00:00Z",
      active_api_keys: 1,
    }])[0]).toEqual([
      "paid@example.com", "Paid", "active", "Builder", "25000000001", 2, "7000000001",
      "2000000001", "4000000000", "1000000000", "0", "2026-07-30T10:00:00Z", "", 1,
    ]);
  });
});
