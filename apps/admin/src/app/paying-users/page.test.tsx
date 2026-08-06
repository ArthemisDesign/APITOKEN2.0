import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import PayingUsersPage, { OpenkeysPayingTable, PayingLedger, PayingRow, PayingUsageDetails } from "./page";
import type { OpenkeysPayingResponse } from "./openkeys-paying-lib";
import {
  buildPayingUsersCsvRows,
  INITIAL_PAYING_USERS_PAGE,
  PAYING_USER_FUNDINGS,
  PAYING_USERS_CSV_HEADER,
  normalizePayingUsersSearch,
  payingCohortUsers,
  payingTierLabel,
  payingUsersQuery,
  providerShareBp,
  usageNanoMoney,
} from "./paying-users-lib";

describe("Платящие (paying users page)", () => {
  it("рендерит доступную навигацию и загрузочный каркас без запроса при SSR", () => {
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const html = renderToString(<PayingUsersPage />);
    expect(html).toContain("Расход клиентов");
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

  it("SSR bonus-only и spend_only строки не смешивают классификации", () => {
    const bonusHtml = renderToString(<table><tbody><PayingRow row={{
      email: "bonus@example.com",
      funding_kind: "bonus_only",
      bonus_funded_spent_nano: "9007199254740993",
      spent_nano: "9007199254740993",
      payments_count: 0,
      last_paid_at: null,
      usage: { status: "unavailable", window: "30d", account_count: 0, available_account_count: 0, unavailable_account_count: 0, requests: "0", total_official_nano: "0", total_charged_nano: "0", models: [] },
    }} rank={1} days={30} /></tbody></table>);
    expect(bonusHtml).toContain("строгий bonus-only");
    expect(bonusHtml).toContain('class="pill info"');
    expect(bonusHtml).not.toContain('class="pill ok"');
    expect(bonusHtml).toContain("$9,007,199.25");
    expect(bonusHtml).toContain("денежных пополнений нет");
    expect(bonusHtml).not.toContain("0 платежей");
    expect(bonusHtml).not.toContain("никогда");
    expect(bonusHtml).not.toContain("$0.00");

    const spendOnlyHtml = renderToString(<table><tbody><PayingRow row={{
      email: "legacy@example.com",
      funding_kind: "spend_only",
      spent_nano: "12",
      usage: { status: "unavailable", window: "30d", account_count: 0, available_account_count: 0, unavailable_account_count: 0, requests: "0", total_official_nano: "0", total_charged_nano: "0", models: [] },
    }} rank={2} days={30} /></tbody></table>);
    expect(spendOnlyHtml).toContain("расход без строгой классификации");
    expect(spendOnlyHtml).toContain('class="pill warn"');
    expect(spendOnlyHtml).not.toContain('class="pill ok"');
    expect(spendOnlyHtml).toContain("не bonus-only");
    expect(spendOnlyHtml).not.toContain("строгий bonus-only");
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
    }} funding="spenders" activeProvider="" onProviderSelect={() => undefined} />);
    expect(html).toContain("Получено денег");
    expect(html).toContain("$12.00");
    expect(html).toContain("Строгий bonus-only ·");
    expect(html).toContain("Все spenders");
    expect(html).toContain("7 дней");
    expect(html).toContain("$3.50");
    expect(html).toContain("2 bonus-only клиента");

    const filtered = renderToString(<PayingLedger data={{ days: 7, summary: { active_spenders: 1, spent_nano: "1" } }} funding="bonus" activeProvider="" onProviderSelect={() => undefined} />);
    expect(filtered).toContain("Выбранная когорта");
    expect(filtered).toContain("только бонусный расход");
    expect(filtered).not.toContain("включая mixed/legacy/unattributed");
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
      usage: { status: "unavailable", window: "30d", account_count: 0, available_account_count: 0, unavailable_account_count: 0, requests: "0", total_official_nano: "0", total_charged_nano: "0", models: [] },
    }} rank={1} days={30} /></tbody></table>);
    expect(html).toContain("ручное пополнение");
    expect(html).toContain('class="pill"');
    expect(html).not.toContain('class="pill ok"');
    expect(html).toContain("2 ручных пополнения");
    expect(html).not.toContain("0 платежей");
  });

  it("зелёным отмечает только подтверждённую provider-платежом часть", () => {
    const usage = { status: "unavailable" as const, window: "30d", account_count: 0, available_account_count: 0, unavailable_account_count: 0, requests: "0", total_official_nano: "0", total_charged_nano: "0", models: [] };
    const paymentHtml = renderToString(<table><tbody><PayingRow row={{
      email: "paid@example.com", funding_kind: "payments", paid_nano: "1000000000", payments_count: 1, usage,
    }} rank={1} days={30} /></tbody></table>);
    const mixedHtml = renderToString(<table><tbody><PayingRow row={{
      email: "mixed@example.com", funding_kind: "payments_and_manual", paid_nano: "2000000000", payments_count: 1, manual_topups_count: 1, manual_paid_nano: "1000000000", usage,
    }} rank={2} days={30} /></tbody></table>);

    expect(paymentHtml).toContain('<span class="pill ok">подтверждённый платёж</span>');
    expect(paymentHtml).not.toContain('class="dot ok"');
    expect(mixedHtml).toContain('<span class="pill ok">подтверждённый платёж</span>');
    expect(mixedHtml).toContain('<span class="pill">ручное пополнение</span>');
    expect(mixedHtml.match(/class="pill ok"/g)).toHaveLength(1);
  });

  it("commerce disclosure в collapsed состоянии не ссылается на отсутствующие детали", () => {
    const html = renderToString(<table><tbody><PayingRow row={{
      user_id: "user-1",
      email: "spender@example.com",
      funding_kind: "spend_only",
      spent_nano: "1",
      usage: {
        status: "complete", window: "7d", account_count: 1, available_account_count: 1,
        unavailable_account_count: 0, requests: "1", total_official_nano: "1",
        total_charged_nano: "1", models: [],
      },
    }} rank={1} days={7} /></tbody></table>);
    expect(html).toContain('aria-label="Показать usage клиента spender@example.com"');
    expect(html).toContain('aria-expanded="false"');
    expect(html).not.toContain("aria-controls=");
    expect(html).not.toContain('id="paying-user-details-1"');
  });

  it("рендерит complete, partial и unavailable usage без ложного нуля", () => {
    const complete = renderToString(<PayingUsageDetails row={{ usage: {
      status: "complete", window: "30d", account_count: 1, available_account_count: 1,
      unavailable_account_count: 0, requests: "9007199254740993", total_official_nano: "12",
      total_charged_nano: "10", models: [],
    } }} />);
    expect(complete).toContain("9007199254740993");
    expect(complete).toContain("запросов, моделей в окне нет");
    expect(complete).toContain("Official");
    expect(complete).toContain("&lt;$0.01");
    expect(complete).not.toContain("$0.00");

    const partial = renderToString(<PayingUsageDetails row={{ usage: {
      status: "partial", window: "7d", account_count: 2, available_account_count: 1,
      unavailable_account_count: 1, requests: "3", total_official_nano: "20",
      total_charged_nano: "15", models: [{
        provider: null, model: "=future-model", requests: "3", input_tokens: "9007199254740993",
        output_tokens: "2", cache_read_tokens: "3", cache_write_5m_tokens: "4",
        cache_write_1h_tokens: "5", web_search_requests: "6", official_nano: "20", charged_nano: "15",
      }],
    } }} />);
    expect(partial).toContain("Покрытие");
    expect(partial).toContain("1");
    expect(partial).toContain("/<!-- -->2");
    expect(partial).toContain("только к доступной части");
    expect(partial).toContain("не указан");
    expect(partial).toContain("вх 9007199254740993");
    expect(partial).toContain("=future-model");
    expect(partial.match(/&lt;\$0\.01/g)).toHaveLength(4);
    expect(partial).not.toContain("$0.00");

    const unavailable = renderToString(<PayingUsageDetails row={{ usage: {
      status: "unavailable", window: "1d", account_count: 1, available_account_count: 0,
      unavailable_account_count: 1, requests: "0", total_official_nano: "0",
      total_charged_nano: "0", models: [],
    } }} />);
    expect(unavailable).toContain("данные недоступны");
    expect(unavailable).toContain("это не нулевой расход");
    expect(unavailable).not.toContain("$0.00");
  });

  it("collapsed OpenKeys disclosure не ссылается на отсутствующую строку деталей", () => {
    const data: OpenkeysPayingResponse = {
      days: 7,
      total: 1,
      limit: 50,
      offset: 0,
      sort: "spent",
      dir: "desc",
      rows: [{
        id: "key-1",
        batchId: "batch-1",
        batchLabel: "Batch",
        createdBy: "seller",
        keyMasked: "sk-pool-…abcd",
        engineAccountId: "acct_openkeys_1",
        apiType: "anthropic",
        enabled: true,
        lifecycle: "stock",
        faceValueNano: "1000000000",
        lifetimeSpentNano: null,
        pricingContract: "official_1_to_1",
        createdAt: "2026-08-01T00:00:00Z",
        deliveredAt: null,
        usage: { status: "unavailable", window: "7d" },
      }],
    };
    const html = renderToString(<OpenkeysPayingTable data={data} />);
    expect(html).toContain('aria-expanded="false"');
    expect(html).toContain("на складе");
    expect(html).toContain("ещё не выдан");
    expect(html.match(/недоступен/g)).toHaveLength(2);
    expect(html.match(/не \$0/g)).toHaveLength(2);
    expect(html).not.toContain("aria-controls=");
    expect(html).not.toContain('id="openkeys-paying-details-key-1"');
  });

  it("OpenKeys показывает lifetime spend отдельно от расхода окна", () => {
    const data: OpenkeysPayingResponse = {
      days: 30,
      total: 1,
      limit: 50,
      offset: 0,
      sort: "spent",
      dir: "desc",
      rows: [{
        id: "key-2",
        batchId: "batch-1",
        batchLabel: "Batch",
        createdBy: "seller",
        keyMasked: "sk-pool-…efgh",
        engineAccountId: "acct_openkeys_2",
        apiType: "openai",
        enabled: true,
        lifecycle: "delivered",
        faceValueNano: "10000000000",
        lifetimeSpentNano: "7000000000",
        pricingContract: "official_1_to_1",
        createdAt: "2026-08-01T00:00:00Z",
        deliveredAt: "2026-08-02T00:00:00Z",
        usage: { status: "available", account: "acct_openkeys_2", window: "30d", since_ts: 1, until_ts: 2, requests: 1, total_official_nano: "3000000000", total_charged_nano: "3000000000", buckets: { input: { tokens: 1, official_nano: "1" }, output: { tokens: 1, official_nano: "1" }, cache_read: { tokens: 0, official_nano: "0" }, cache_write: { tokens: 0, official_nano: "0" }, web_search: { requests: 0, official_nano: "0" }, unattributed_legacy: { official_nano: "0" } }, models: [], daily: [], daily_providers: [], keys: [] },
      }],
    };
    const html = renderToString(<OpenkeysPayingTable data={data} />);
    expect(html).toContain("$7.00");
    expect(html).toContain("lifetime движка");
    expect(html).toContain("$3.00");
    expect(html).toContain("за выбранное окно");
  });
});

describe("payingUsersQuery", () => {
  it("строит exact default spender query с обязательным usage", () => {
    expect(payingUsersQuery(INITIAL_PAYING_USERS_PAGE)).toBe(
      "days=30&limit=50&offset=0&sort=spent&dir=desc&funding=spenders&include_usage=true",
    );
  });

  it("нормализует producer search до 200 символов", () => {
    expect(normalizePayingUsersSearch(`  ${"x".repeat(201)}  `)).toBe("x".repeat(200));
  });

  it("добавляет поиск, статус и провайдера, сохраняя funding и usage", () => {
    expect(payingUsersQuery({
      ...INITIAL_PAYING_USERS_PAGE,
      days: 7,
      q: "wwwvatroke@gmail.com",
      status: "active",
      provider: "openai",
      sort: "paid",
      dir: "asc",
    })).toBe(
      "days=7&limit=50&offset=0&sort=paid&dir=asc&funding=spenders&include_usage=true&q=wwwvatroke%40gmail.com&status=active&provider=openai",
    );
    expect(payingUsersQuery({ ...INITIAL_PAYING_USERS_PAGE, funding: "bonus" })).toContain("funding=bonus&include_usage=true");
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
  it("не округляет положительный usage меньше цента до нуля", () => {
    expect(usageNanoMoney("0")).toBe("$0.00");
    expect(usageNanoMoney("12")).toBe("<$0.01");
    expect(usageNanoMoney("9999999")).toBe("<$0.01");
    expect(usageNanoMoney("10000000")).toBe("$0.01");
    expect(usageNanoMoney("malformed")).toBe("$0.00");
  });

  it("публикует точные funding labels и предпочитает cohort_users", () => {
    expect(PAYING_USER_FUNDINGS).toEqual([
      ["spenders", "все с расходом"],
      ["all", "деньги + строгий бонус"],
      ["payments", "платёжный провайдер"],
      ["manual", "ручное денежное пополнение"],
      ["bonus", "только бонусный расход"],
    ]);
    expect(payingCohortUsers({ cohort_users: 7, paying_users: 5 })).toBe(7);
    expect(payingCohortUsers({ paying_users: 5 })).toBe(5);
    expect(payingCohortUsers(undefined)).toBe(0);
  });

  it("показывает B2B по классу и flat −50% для B2C", () => {
    expect(payingTierLabel({ customer_type: "b2b" })).toBe("B2B");
    expect(payingTierLabel({ customer_type: "b2c" })).toBe("B2C −50%");
  });

  it("экспортирует user × provider × model с exact strings и formula safety", () => {
    expect(PAYING_USERS_CSV_HEADER).toContain("usage_available_account_count");
    expect(PAYING_USERS_CSV_HEADER).toContain("model_charged_nanoUSD_text");
    const rows = buildPayingUsersCsvRows([{
      user_id: "=user-id",
      email: "=paid@example.com",
      display_name: "+Paid",
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
      provider_spend: { anthropic_nano: "20000000000000001" },
      active_api_keys: 1,
      usage: {
        status: "complete", window: "30d", account_count: 2, available_account_count: 2,
        unavailable_account_count: 0, requests: "90071992547409930",
        total_official_nano: "90071992547409931", total_charged_nano: "90071992547409932",
        models: [
          { provider: "=provider", model: "+model-a", requests: "90071992547409933", input_tokens: "1", output_tokens: "2", cache_read_tokens: "3", cache_write_5m_tokens: "4", cache_write_1h_tokens: "5", web_search_requests: "6", official_nano: "90071992547409934", charged_nano: "90071992547409935" },
          { provider: null, model: "model-b", requests: "7", input_tokens: "8", output_tokens: "9", cache_read_tokens: "10", cache_write_5m_tokens: "11", cache_write_1h_tokens: "12", web_search_requests: "13", official_nano: "14", charged_nano: "15" },
        ],
      },
    }]);
    expect(rows).toHaveLength(2);
    const first = Object.fromEntries(PAYING_USERS_CSV_HEADER.map((header, index) => [header, rows[0]![index]]));
    const second = Object.fromEntries(PAYING_USERS_CSV_HEADER.map((header, index) => [header, rows[1]![index]]));
    expect(first).toMatchObject({
      user_id: "'=user-id", email: "'=paid@example.com", имя: "'+Paid", provider: "'=provider",
      model: "'+model-a", оплачено_nanoUSD_text: "'25000000000000001",
      usage_requests_text: "'90071992547409930", model_requests_text: "'90071992547409933",
      model_official_nanoUSD_text: "'90071992547409934", usage_total_charged_nanoUSD_text: "'90071992547409932",
    });
    expect(second.provider).toBe("не указан");
    expect(second.model).toBe("model-b");

    const unavailable = buildPayingUsersCsvRows([{
      email: "down@example.com", funding_kind: "spend_only", spent_nano: "1",
      usage: { status: "unavailable", window: "7d", account_count: 1, available_account_count: 0, unavailable_account_count: 1, requests: "0", total_official_nano: "0", total_charged_nano: "0", models: [] },
    }])[0]!;
    const unavailableRecord = Object.fromEntries(PAYING_USERS_CSV_HEADER.map((header, index) => [header, unavailable[index]]));
    expect(unavailableRecord).toMatchObject({ usage_status: "unavailable", usage_requests_text: "", model: "", model_official_nanoUSD_text: "", usage_total_charged_nanoUSD_text: "" });
  });
});
