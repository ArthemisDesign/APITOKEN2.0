import type { EngineAccount, EngineUsage } from "@claude-api/contracts";
import { PgDialect } from "drizzle-orm/pg-core";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  rows: [] as Array<Record<string, unknown>>,
  total: 0,
  where: vi.fn(),
  orderBy: vi.fn(),
  limit: vi.fn(),
  offset: vi.fn(),
  countWhere: vi.fn(),
  select: vi.fn(),
  getAccounts: vi.fn(),
  getUsage: vi.fn(),
}));

vi.mock("./db", () => ({
  getDatabase: vi.fn(() => ({ db: { select: mocks.select } })),
}));
vi.mock("./engine", () => ({
  getEngineClient: vi.fn(() => ({ getAccounts: mocks.getAccounts, getUsage: mocks.getUsage })),
}));

function candidateBuilder() {
  const query = { from: vi.fn(), innerJoin: vi.fn(), where: mocks.where };
  query.from.mockReturnValue(query);
  query.innerJoin.mockReturnValue(query);
  mocks.where.mockResolvedValue(mocks.rows);
  return query;
}

function pageBuilder() {
  const query = {
    from: vi.fn(),
    innerJoin: vi.fn(),
    where: mocks.where,
    orderBy: mocks.orderBy,
    limit: mocks.limit,
    offset: mocks.offset,
  };
  query.from.mockReturnValue(query);
  query.innerJoin.mockReturnValue(query);
  mocks.where.mockReturnValue(query);
  mocks.orderBy.mockReturnValue(query);
  mocks.limit.mockReturnValue(query);
  mocks.offset.mockResolvedValue(mocks.rows);
  return query;
}

function countBuilder() {
  const query = { from: vi.fn(), innerJoin: vi.fn(), where: mocks.countWhere };
  query.from.mockReturnValue(query);
  query.innerJoin.mockReturnValue(query);
  mocks.countWhere.mockResolvedValue([{ value: mocks.total }]);
  return query;
}

function usage(account: string, overrides: Partial<EngineUsage> = {}): EngineUsage {
  return {
    account,
    window: "7d",
    since_ts: 1,
    until_ts: 2,
    requests: 0,
    total_official_nano: "0",
    total_charged_nano: "0",
    buckets: {
      input: { tokens: 0, official_nano: "0" },
      output: { tokens: 0, official_nano: "0" },
      cache_read: { tokens: 0, official_nano: "0" },
      cache_write: { tokens: 0, official_nano: "0" },
      web_search: { requests: 0, official_nano: "0" },
      unattributed_legacy: { official_nano: "0" },
    },
    models: [],
    daily: [],
    daily_providers: [],
    keys: [],
    ...overrides,
  };
}

function account(index: number, spentNano: string): EngineAccount {
  return {
    account: `acct_paying_${index}`,
    balance_nano: "0",
    spent_nano: spentNano,
    reserved_nano: "0",
    balance: "$0.00",
    mult_bp: 10_000,
    status: "active",
    handle: null,
  };
}

function row(index: number): Record<string, unknown> {
  return {
    id: `00000000-0000-4000-8000-${index.toString().padStart(12, "0")}`,
    batchId: "10000000-0000-4000-8000-000000000001",
    batchLabel: "Retail batch",
    createdBy: "seller",
    keyMasked: `sk-pool-${index}…test`,
    engineAccountId: `acct_paying_${index}`,
    apiType: index % 2 === 0 ? "openai" : null,
    enabled: index === 2 ? "disabled" : "active",
    faceValueNano: BigInt(index) * 10_000_000_000n,
    pricingContract: "official_1_to_1",
    createdAt: new Date(`2026-07-${String(index).padStart(2, "0")}T00:00:00.000Z`),
    deliveredAt: index === 3 ? null : new Date(`2026-07-${String(10 - index).padStart(2, "0")}T00:00:00.000Z`),
  };
}

const defaults = {
  days: 7 as const,
  limit: 50,
  offset: 0,
  q: "",
  status: "all" as const,
  sort: "spent" as const,
  dir: "desc" as const,
};

async function loadModule() {
  return import("./keys");
}

describe("OpenKeys paying keys projection", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.rows = [];
    mocks.total = 0;
    mocks.getAccounts.mockResolvedValue([]);
    mocks.getUsage.mockImplementation((accountId: string) => Promise.resolve(usage(accountId)));
  });

  it("globally sorts the filtered cohort by exact lifetime spend before pagination", async () => {
    mocks.rows = [row(1), row(2), row(3)];
    mocks.select.mockImplementationOnce(() => candidateBuilder());
    mocks.getAccounts.mockResolvedValue([
      account(1, "900719925474099312345678901234567890"),
      account(2, "2"),
      account(3, "1000000000000000000000000000000000000"),
    ]);
    const { loadPayingKeys } = await loadModule();

    const result = await loadPayingKeys({ ...defaults, limit: 1, offset: 1 });

    const dialect = new PgDialect();
    const whereSql = dialect.sqlToQuery(mocks.where.mock.calls[0]![0]).sql;
    expect(whereSql).toContain('"openkeys_keys"."removed_at" is null');
    expect(whereSql).not.toContain('"openkeys_keys"."delivered_at" is not null');
    expect(mocks.getAccounts).toHaveBeenCalledWith([
      "acct_paying_1", "acct_paying_2", "acct_paying_3",
    ]);
    expect(mocks.limit).not.toHaveBeenCalled();
    expect(mocks.offset).not.toHaveBeenCalled();
    expect(mocks.getUsage).toHaveBeenCalledTimes(1);
    expect(mocks.getUsage).toHaveBeenCalledWith("acct_paying_1", "7d");
    expect(result).toMatchObject({
      total: 3,
      limit: 1,
      offset: 1,
      sort: "spent",
      dir: "desc",
      rows: [{ engineAccountId: "acct_paying_1", lifetimeSpentNano: "900719925474099312345678901234567890" }],
    });
  });

  it("keeps unavailable lifetime spend last in either direction", async () => {
    mocks.rows = [row(1), row(2), row(3)];
    mocks.select.mockImplementationOnce(() => candidateBuilder());
    mocks.getAccounts.mockResolvedValue([account(1, "10"), account(2, "20")]);
    const { loadPayingKeys } = await loadModule();

    const result = await loadPayingKeys({ ...defaults, dir: "asc" });

    expect(result.rows.map((item) => [item.engineAccountId, item.lifetimeSpentNano])).toEqual([
      ["acct_paying_1", "10"],
      ["acct_paying_2", "20"],
      ["acct_paying_3", null],
    ]);
  });

  it("uses PostgreSQL sorting and pagination for nominal and loads accounts only for the page", async () => {
    mocks.rows = [row(2)];
    mocks.total = 17;
    mocks.select.mockImplementationOnce(() => pageBuilder()).mockImplementationOnce(() => countBuilder());
    mocks.getAccounts.mockResolvedValue([account(2, "123")]);
    const { loadPayingKeys } = await loadModule();

    const result = await loadPayingKeys({
      ...defaults,
      limit: 1,
      offset: 5,
      q: " seller ",
      status: "disabled",
      sort: "nominal",
      dir: "asc",
    });

    expect(mocks.orderBy).toHaveBeenCalledTimes(1);
    expect(mocks.limit).toHaveBeenCalledWith(1);
    expect(mocks.offset).toHaveBeenCalledWith(5);
    expect(mocks.getAccounts).toHaveBeenCalledWith(["acct_paying_2"]);
    expect(result).toMatchObject({
      total: 17,
      sort: "nominal",
      dir: "asc",
      rows: [{ lifecycle: "delivered", lifetimeSpentNano: "123" }],
    });
  });

  it("bounds page usage concurrency at four and keeps partial outages row-local", async () => {
    mocks.rows = [row(1), row(2), row(3), row(4), row(5)];
    mocks.select.mockImplementationOnce(() => candidateBuilder());
    mocks.getAccounts.mockResolvedValue([
      account(1, "5"), account(2, "4"), account(3, "3"), account(4, "2"), account(5, "1"),
    ]);
    const resolvers: Array<{ resolve: (value: EngineUsage) => void; reject: (reason: Error) => void }> = [];
    let active = 0;
    let maxActive = 0;
    mocks.getUsage.mockImplementation((accountId: string) => new Promise<EngineUsage>((resolve, reject) => {
      active += 1;
      maxActive = Math.max(maxActive, active);
      resolvers.push({
        resolve: (value) => { active -= 1; resolve(value); },
        reject: (reason) => { active -= 1; reject(reason); },
      });
    }));
    const { loadPayingKeys } = await loadModule();

    const pending = loadPayingKeys({ ...defaults, limit: 5 });
    await vi.waitFor(() => expect(resolvers).toHaveLength(4));
    resolvers[0]!.resolve(usage("acct_paying_1"));
    resolvers[1]!.reject(new Error("engine unavailable"));
    resolvers[2]!.resolve(usage("acct_paying_3"));
    resolvers[3]!.resolve(usage("acct_paying_4"));
    await vi.waitFor(() => expect(resolvers).toHaveLength(5));
    resolvers[4]!.resolve(usage("acct_paying_5"));

    const result = await pending;
    expect(maxActive).toBe(4);
    expect(result.rows[1]!.usage).toEqual({ status: "unavailable", window: "7d" });
    expect(result.rows[0]!.usage).toMatchObject({ status: "available", window: "7d", requests: 0 });
  });

  it("preserves exact model usage without exposing secret fields", async () => {
    mocks.rows = [row(1)];
    mocks.select.mockImplementationOnce(() => candidateBuilder());
    mocks.getAccounts.mockResolvedValue([account(1, "1")]);
    const exact = "900719925474099312345678901234567890";
    mocks.getUsage.mockResolvedValue(usage("acct_paying_1", {
      requests: 3,
      total_official_nano: exact,
      total_charged_nano: `${exact}1`,
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
    }));
    const { loadPayingKeys } = await loadModule();

    const output = JSON.parse(JSON.stringify(await loadPayingKeys(defaults)));

    expect(output.rows[0].usage).toMatchObject({
      status: "available",
      total_official_nano: exact,
      total_charged_nano: `${exact}1`,
      models: [{ provider: "free-community-provider", cache_write_1h_tokens: 15 }],
    });
    expect(Object.keys(output.rows[0])).not.toEqual(expect.arrayContaining([
      "secret", "viewToken", "secretCiphertext", "secretNonce", "engineKeyId", "keySha256",
    ]));
    expect(JSON.stringify(output)).not.toContain("ciphertext");
  });
});
