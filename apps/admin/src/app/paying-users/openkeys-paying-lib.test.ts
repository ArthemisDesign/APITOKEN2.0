import { describe, expect, it } from "vitest";
import { buildCsv, spreadsheetExactInteger, spreadsheetSafeText } from "../../lib/csv";
import {
  addNano,
  buildOpenkeysPayingCsvRows,
  clampOpenkeysPayingOffset,
  INITIAL_OPENKEYS_PAYING_PAGE,
  OPENKEYS_PAYING_CSV_HEADER,
  OPENKEYS_PAYING_MAX_OFFSET,
  openkeysChargedNano,
  openkeysPayingQuery,
  providerLabel,
  type OpenkeysEngineUsage,
  type OpenkeysPayingRow,
} from "./openkeys-paying-lib";

const exact = "900719925474099312345678901234567890";

function engineUsage(overrides: Partial<OpenkeysEngineUsage> = {}): OpenkeysEngineUsage {
  return {
    account: "acct_openkeys_1",
    window: "7d",
    since_ts: 1,
    until_ts: 2,
    requests: 3,
    total_official_nano: exact,
    total_charged_nano: `${exact}1`,
    buckets: {
      input: { tokens: 11, official_nano: "1" },
      output: { tokens: 12, official_nano: "2" },
      cache_read: { tokens: 13, official_nano: "3" },
      cache_write: { tokens: 29, official_nano: "4" },
      web_search: { requests: 16, official_nano: "5" },
      unattributed_legacy: { official_nano: "0" },
    },
    models: [{
      model: "future-model",
      provider: "free-community-provider",
      requests: 3,
      input_tokens: 11,
      output_tokens: 12,
      cache_read_tokens: 13,
      cache_write_5m_tokens: 14,
      cache_write_1h_tokens: 15,
      web_search_requests: 16,
      official_nano: exact,
      charged_nano: `${exact}1`,
    }],
    daily: [{ day_ts: 1, requests: 3, official_nano: exact, charged_nano: `${exact}1` }],
    daily_providers: [{
      day_ts: 1,
      provider: "free-community-provider",
      requests: 3,
      official_nano: exact,
      charged_nano: `${exact}1`,
    }],
    keys: [{ key_masked: "sk-pool-…abcd", requests: 3, official_nano: exact, charged_nano: `${exact}1` }],
    ...overrides,
  };
}

function payingRow(overrides: Partial<OpenkeysPayingRow> = {}): OpenkeysPayingRow {
  return {
    id: "key-1",
    batchId: "batch-1",
    batchLabel: "August",
    createdBy: "seller@example.com",
    keyMasked: "sk-pool-…abcd",
    engineAccountId: "acct_openkeys_1",
    apiType: "openai",
    enabled: true,
    lifecycle: "delivered",
    faceValueNano: exact,
    lifetimeSpentNano: `${exact}2`,
    pricingContract: "official_1_to_1",
    createdAt: "2026-08-01T00:00:00.000Z",
    deliveredAt: "2026-08-02T00:00:00.000Z",
    usage: { status: "available", ...engineUsage() },
    ...overrides,
  };
}

describe("openkeysPayingQuery", () => {
  it("строит documented defaults и опускает пустой поиск", () => {
    expect(openkeysPayingQuery(INITIAL_OPENKEYS_PAYING_PAGE)).toBe(
      "days=30&limit=50&offset=0&status=all&sort=spent&dir=desc",
    );
  });

  it("сохраняет серверные фильтры и кодирует поиск", () => {
    expect(openkeysPayingQuery({
      days: 7,
      limit: 25,
      offset: 50,
      q: "batch seller@example.com",
      status: "disabled",
      sort: "nominal",
      dir: "asc",
    })).toBe("days=7&limit=25&offset=50&status=disabled&sort=nominal&dir=asc&q=batch+seller%40example.com");
  });

  it("ограничивает offset диапазоном producer", () => {
    expect(clampOpenkeysPayingOffset(Number.NaN)).toBe(0);
    expect(clampOpenkeysPayingOffset(-1)).toBe(0);
    expect(clampOpenkeysPayingOffset(12.9)).toBe(12);
    expect(clampOpenkeysPayingOffset(OPENKEYS_PAYING_MAX_OFFSET + 50)).toBe(OPENKEYS_PAYING_MAX_OFFSET);
    expect(openkeysPayingQuery({
      ...INITIAL_OPENKEYS_PAYING_PAGE,
      offset: OPENKEYS_PAYING_MAX_OFFSET + 50,
    })).toContain(`offset=${OPENKEYS_PAYING_MAX_OFFSET}`);
  });
});

describe("OpenKeys exact money and CSV", () => {
  it("суммирует nanoUSD через BigInt выше Number.MAX_SAFE_INTEGER", () => {
    expect(addNano([exact, exact, "1"])).toBe("1801439850948198624691357802469135781");
  });

  it("экспортирует одну строку key × provider × model с точными text money", () => {
    const [csv] = buildOpenkeysPayingCsvRows([payingRow()]);
    const record = Object.fromEntries(OPENKEYS_PAYING_CSV_HEADER.map((field, index) => [field, csv?.[index]]));
    expect(record).toMatchObject({
      key_id: "key-1",
      key_masked: "sk-pool-…abcd",
      engine_account_id: "acct_openkeys_1",
      lifecycle: "delivered",
      provider: "free-community-provider",
      model: "future-model",
      nominal_nanoUSD_text: `'${exact}`,
      lifetime_spent_nanoUSD_text: `'${exact}2`,
      official_nanoUSD_text: `'${exact}`,
      charged_nanoUSD_text: `'${exact}1`,
      usage_total_official_nanoUSD_text: `'${exact}`,
      usage_total_charged_nanoUSD_text: `'${exact}1`,
    });
  });

  it("оставляет отдельную однозначную строку складскому ключу без моделей", () => {
    const row = payingRow({ lifecycle: "stock", deliveredAt: null });
    if (row.usage.status === "available") row.usage.models = [];
    const [csv] = buildOpenkeysPayingCsvRows([row]);
    const record = Object.fromEntries(OPENKEYS_PAYING_CSV_HEADER.map((field, index) => [field, csv?.[index]]));
    expect(record).toMatchObject({
      key_id: "key-1",
      engine_account_id: "acct_openkeys_1",
      lifecycle: "stock",
      delivered_at: "",
      model: "",
      usage_total_charged_nanoUSD_text: `'${exact}1`,
    });
  });

  it("финальный CSV сохраняет huge integer и нейтрализует формулы во всех data cells", () => {
    const row = payingRow({
      id: "=key",
      keyMasked: "+masked",
      engineAccountId: "-account",
      batchId: "@batch",
      batchLabel: " =label",
      createdBy: "\t+seller",
      deliveredAt: "@date",
      usage: {
        status: "available",
        ...engineUsage({
          window: "-window",
          models: [{
            ...engineUsage().models[0]!,
            provider: "=provider",
            model: "+model",
          }],
        }),
      },
    });
    const rows = buildOpenkeysPayingCsvRows([row]);
    const serialized = buildCsv(OPENKEYS_PAYING_CSV_HEADER, rows);
    const dataCells = serialized.split("\r\n")[1]!.split(";");

    expect(serialized).toContain(`;'${exact};`);
    expect(dataCells.every((cell) => !/^[ \t]*[=+\-@]/.test(cell.replace(/^"|"$/g, "")))).toBe(true);
    expect(spreadsheetSafeText("ordinary")).toBe("ordinary");
    expect(spreadsheetExactInteger("not-decimal")).toBe("'not-decimal");
  });
});

describe("OpenKeys wire semantics", () => {
  it("не выводит provider из model или apiType", () => {
    expect(providerLabel("free-community-provider")).toBe("free-community-provider");
    expect(providerLabel(undefined)).toBe("не указан");
  });

  it("различает недоступный usage и доступный точный ноль", () => {
    expect(openkeysChargedNano({ status: "unavailable", window: "7d" })).toBeNull();
    expect(openkeysChargedNano({
      status: "available",
      ...engineUsage({
        requests: 0,
        total_official_nano: "0",
        total_charged_nano: "0",
        models: [],
      }),
    })).toBe("0");
  });
});
