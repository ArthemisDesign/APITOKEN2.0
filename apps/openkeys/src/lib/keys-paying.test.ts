import type { EngineUsage } from "@claude-api/contracts";
import { PgDialect } from "drizzle-orm/pg-core";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  pageRows: [] as Array<Record<string, unknown>>,
  total: 0,
  pageWhere: vi.fn(),
  pageOrderBy: vi.fn(),
  pageLimit: vi.fn(),
  pageOffset: vi.fn(),
  countWhere: vi.fn(),
  select: vi.fn(),
  getUsage: vi.fn(),
}));

vi.mock("./db", () => ({
  getDatabase: vi.fn(() => ({ db: { select: mocks.select } })),
}));
vi.mock("./engine", () => ({
  getEngineClient: vi.fn(() => ({ getUsage: mocks.getUsage })),
}));

function pageBuilder() {
  const query = {
    from: vi.fn(),
    innerJoin: vi.fn(),
    where: mocks.pageWhere,
    orderBy: mocks.pageOrderBy,
    limit: mocks.pageLimit,
    offset: mocks.pageOffset,
  };
  query.from.mockReturnValue(query);
  query.innerJoin.mockReturnValue(query);
  mocks.pageWhere.mockReturnValue(query);
  mocks.pageOrderBy.mockReturnValue(query);
  mocks.pageLimit.mockReturnValue(query);
  mocks.pageOffset.mockResolvedValue(mocks.pageRows);
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
    faceValueNano: 50_000_000_000n,
    pricingContract: "official_1_to_1",
    createdAt: new Date("2026-07-01T00:00:00.000Z"),
    deliveredAt: new Date(`2026-07-${String(10 - index).padStart(2, "0")}T00:00:00.000Z`),
  };
}

async function loadModule() {
  return import("./keys");
}

describe("OpenKeys paying keys projection", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.pageRows = [];
    mocks.total = 0;
    mocks.select.mockImplementationOnce(() => pageBuilder()).mockImplementationOnce(() => countBuilder());
  });

  it("selects every non-removed key and paginates before loading usage", async () => {
    mocks.pageRows = [row(2)];
    mocks.total = 17;
    mocks.getUsage.mockResolvedValue(usage("acct_paying_2"));
    const { loadPayingKeys } = await loadModule();

    const result = await loadPayingKeys({ days: 7, limit: 1, offset: 5, q: " seller ", status: "disabled" });

    const dialect = new PgDialect();
    const whereSql = dialect.sqlToQuery(mocks.pageWhere.mock.calls[0]![0]).sql;
    expect(whereSql).toContain('"openkeys_keys"."removed_at" is null');
    expect(whereSql).not.toContain('"openkeys_keys"."delivered_at" is not null');
    expect(whereSql).toContain('"openkeys_keys"."status" = $1');
    expect(mocks.pageLimit).toHaveBeenCalledWith(1);
    expect(mocks.pageOffset).toHaveBeenCalledWith(5);
    expect(mocks.pageOrderBy).toHaveBeenCalledTimes(1);
    expect(mocks.getUsage).toHaveBeenCalledTimes(1);
    expect(mocks.getUsage).toHaveBeenCalledWith("acct_paying_2", "7d");
    expect(result).toMatchObject({
      days: 7,
      total: 17,
      limit: 1,
      offset: 5,
      rows: [{ lifecycle: "delivered", deliveredAt: "2026-07-08T00:00:00.000Z" }],
    });
  });

  it("keeps an active warehouse key visible with explicit stock lifecycle", async () => {
    mocks.pageRows = [{ ...row(1), deliveredAt: null }];
    mocks.total = 1;
    mocks.getUsage.mockResolvedValue(usage("acct_paying_1"));
    const { loadPayingKeys } = await loadModule();

    const result = await loadPayingKeys({ days: 30, limit: 50, offset: 0, q: "", status: "all" });

    expect(result.rows[0]).toMatchObject({ lifecycle: "stock", deliveredAt: null, enabled: true });
    expect(mocks.getUsage).toHaveBeenCalledWith("acct_paying_1", "30d");
  });

  it("bounds usage concurrency at four and keeps partial outages row-local", async () => {
    mocks.pageRows = [row(1), row(2), row(3), row(4), row(5)];
    mocks.total = 5;
    const resolvers: Array<{ resolve: (value: EngineUsage) => void; reject: (reason: Error) => void }> = [];
    let active = 0;
    let maxActive = 0;
    mocks.getUsage.mockImplementation((account: string) => new Promise<EngineUsage>((resolve, reject) => {
      active += 1;
      maxActive = Math.max(maxActive, active);
      resolvers.push({
        resolve: (value) => { active -= 1; resolve(value); },
        reject: (reason) => { active -= 1; reject(reason); },
      });
    }));
    const { loadPayingKeys } = await loadModule();

    const pending = loadPayingKeys({ days: 7, limit: 5, offset: 0, q: "", status: "all" });
    await vi.waitFor(() => expect(resolvers).toHaveLength(4));
    expect(mocks.getUsage).toHaveBeenCalledTimes(4);
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

  it("preserves exact usage strings, free-form providers and every model counter without secret fields", async () => {
    mocks.pageRows = [row(1)];
    mocks.total = 1;
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

    const result = await loadPayingKeys({ days: 7, limit: 50, offset: 0, q: "", status: "all" });
    const output = JSON.parse(JSON.stringify(result));

    expect(output.rows[0].usage).toMatchObject({
      status: "available",
      total_official_nano: exact,
      total_charged_nano: `${exact}1`,
      models: [{
        provider: "free-community-provider",
        input_tokens: 11,
        output_tokens: 12,
        cache_read_tokens: 13,
        cache_write_5m_tokens: 14,
        cache_write_1h_tokens: 15,
        web_search_requests: 16,
        official_nano: exact,
      }],
    });
    expect(Object.keys(output.rows[0])).not.toEqual(expect.arrayContaining([
      "secret", "viewToken", "secretCiphertext", "secretNonce", "engineKeyId", "keySha256",
    ]));
    expect(JSON.stringify(output)).not.toContain("ciphertext");
  });
});
