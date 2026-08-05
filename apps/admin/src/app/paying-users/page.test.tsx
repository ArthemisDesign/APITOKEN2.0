import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import PayingUsersPage, { OpenkeysPayingTable, PayingLedger, PayingRow } from "./page";
import type { OpenkeysPayingResponse } from "./openkeys-paying-lib";
import {
  buildPayingUsersCsvRows,
  INITIAL_PAYING_USERS_PAGE,
  PAYING_USER_FUNDINGS,
  PAYING_USERS_CSV_HEADER,
  payingCohortUsers,
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
    expect(html).toContain("Клиенты");
    expect(html).toContain("OpenKeys");
    expect(html).toContain('id="paying-tab-customers"');
    expect(html).toContain('aria-controls="paying-panel-customers"');
    expect(html).toContain('id="paying-panel-customers"');
    expect(html).not.toContain('id="paying-panel-openkeys"');
    expect(html).not.toContain('aria-controls="paying-panel-openkeys"');
    expect(html).toContain("loading-grid");
    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });

  it("SSR bonus-only row показывает бонус без ложных payment/never значений", () => {
    const html = renderToString(<table><tbody><PayingRow row={{
      email: "bonus@example.com",
      funding_kind: "bonus_only",
      bonus_funded_spent_nano: "9007199254740993",
      spent_nano: "9007199254740993",
      payments_count: 0,
      last_paid_at: null,
    }} rank={1} days={30} /></tbody></table>);
    expect(html).toContain("только бонус");
    expect(html).toContain("$9,007,199.25");
    expect(html).toContain("денежных пополнений нет");
    expect(html).not.toContain("0 платежей");
    expect(html).not.toContain("никогда");
    expect(html).not.toContain("$0.00");
  });

  it("SSR ledger разделяет lifetime деньги и bonus-only окно", () => {
    const html = renderToString(<PayingLedger data={{
      days: 7,
      summary: {
        paying_users: 4,
        paid_nano: "12000000000",
        bonus_only_users: 2,
        bonus_only_spent_nano: "3500000000",
      },
    }} activeProvider="" onProviderSelect={() => undefined} />);
    expect(html).toContain("Получено денег");
    expect(html).toContain("$12.00");
    expect(html).toContain("Списано бонуса ·");
    expect(html).toContain("7 дней");
    expect(html).toContain("$3.50");
    expect(html).toContain("2 bonus-only клиента");
  });

  it("SSR manual-only row не выдаёт ручное пополнение за provider payment", () => {
    const html = renderToString(<table><tbody><PayingRow row={{
      email: "manual@example.com",
      funding_kind: "manual",
      paid_nano: "7000000000",
      manual_paid_nano: "7000000000",
      manual_topups_count: 2,
      payments_count: 0,
      last_paid_at: "2026-08-01T00:00:00Z",
    }} rank={1} days={30} /></tbody></table>);
    expect(html).toContain("ручное пополнение");
    expect(html).toContain("2 ручных пополнения");
    expect(html).not.toContain("0 платежей");
  });

  it("collapsed OpenKeys disclosure не ссылается на отсутствующую строку деталей", () => {
    const data: OpenkeysPayingResponse = {
      days: 7,
      total: 1,
      limit: 50,
      offset: 0,
      rows: [{
        id: "key-1",
        batchId: "batch-1",
        batchLabel: "Batch",
        createdBy: "seller",
        keyMasked: "sk-pool-…abcd",
        engineAccountId: "acct_openkeys_1",
        apiType: "anthropic",
        enabled: true,
        faceValueNano: "1000000000",
        pricingContract: "official_1_to_1",
        createdAt: "2026-08-01T00:00:00Z",
        deliveredAt: "2026-08-02T00:00:00Z",
        usage: { status: "unavailable", window: "7d" },
      }],
    };
    const html = renderToString(<OpenkeysPayingTable data={data} />);
    expect(html).toContain('aria-expanded="false"');
    expect(html).not.toContain("aria-controls=");
    expect(html).not.toContain('id="openkeys-paying-details-key-1"');
  });
});

describe("payingUsersQuery", () => {
  it("строит стабильный серверный запрос и пропускает пустые фильтры", () => {
    expect(payingUsersQuery(INITIAL_PAYING_USERS_PAGE)).toBe(
      "days=30&limit=50&offset=0&sort=spent&dir=desc&funding=all",
    );
  });

  it("добавляет поиск, статус и провайдера, сохраняя явный funding", () => {
    expect(payingUsersQuery({
      ...INITIAL_PAYING_USERS_PAGE,
      days: 7,
      q: "paid user@example.com",
      status: "active",
      provider: "openai",
      sort: "paid",
      dir: "asc",
    })).toBe(
      "days=7&limit=50&offset=0&sort=paid&dir=asc&funding=all&q=paid+user%40example.com&status=active&provider=openai",
    );
    expect(payingUsersQuery({ ...INITIAL_PAYING_USERS_PAGE, funding: "bonus" })).toContain("funding=bonus");
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
  it("публикует точные funding labels и предпочитает cohort_users", () => {
    expect(PAYING_USER_FUNDINGS).toEqual([
      ["all", "деньги + бонусный расход"],
      ["payments", "платёжный провайдер"],
      ["manual", "ручное денежное пополнение"],
      ["bonus", "только бонусный расход"],
    ]);
    expect(payingCohortUsers({ cohort_users: 7, paying_users: 5 })).toBe(7);
    expect(payingCohortUsers({ paying_users: 5 })).toBe(5);
    expect(payingCohortUsers(undefined)).toBe(0);
  });

  it("показывает B2B независимо от тира и имена B2C-тиpов", () => {
    expect(payingTierLabel({ customer_type: "b2b", tier: 0 })).toBe("B2B");
    expect(payingTierLabel({ customer_type: "b2c", tier: 2 })).toBe("Pro");
    expect(payingTierLabel({ customer_type: "b2c", tier: 8 })).toBe("—");
  });

  it("экспортирует exact funding header и nanoUSD-строки выше safe integer", () => {
    expect(PAYING_USERS_CSV_HEADER).toEqual([
      "email", "имя", "статус", "тариф", "funding_kind", "оплачено_nanoUSD", "платежей",
      "ручных_пополнений", "ручные_nanoUSD", "расход_окна_nanoUSD", "paid_funded_spent_nano",
      "bonus_funded_spent_nano", "other_funded_spent_nano", "unattributed_spent_nano", "claude_nanoUSD",
      "gpt_nanoUSD", "gemini_nanoUSD", "другое_nanoUSD", "последняя_оплата", "последняя_активность", "активные_ключи",
    ]);
    expect(buildPayingUsersCsvRows([{
      email: "paid@example.com",
      display_name: "Paid",
      status: "active",
      customer_type: "b2c",
      tier: 1,
      funding_kind: "payments_and_manual",
      paid_nano: "25000000000000001",
      payments_count: 2,
      manual_topups_count: 1,
      manual_paid_nano: "5000000000000001",
      spent_nano: "37000000000000004",
      paid_funded_spent_nano: "9007199254740993",
      bonus_funded_spent_nano: "9007199254740994",
      other_funded_spent_nano: "9007199254740995",
      unattributed_spent_nano: "9007199254740996",
      provider_spend: {
        anthropic_nano: "20000000000000001",
        openai_nano: "16000000000000002",
        google_nano: "1000000000000001",
      },
      last_paid_at: "2026-07-30T10:00:00Z",
      active_api_keys: 1,
    }])[0]).toEqual([
      "paid@example.com", "Paid", "active", "Builder", "payments_and_manual", "25000000000000001", 2, 1,
      "5000000000000001", "37000000000000004", "9007199254740993", "9007199254740994", "9007199254740995",
      "9007199254740996", "20000000000000001", "16000000000000002", "1000000000000001", "0",
      "2026-07-30T10:00:00Z", "", 1,
    ]);
  });
});
