import type { EngineAccount, EngineUsage } from "@claude-api/contracts";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  const getAccount = vi.fn();
  const getUsage = vi.fn();
  const limit = vi.fn();
  const query = {
    from: vi.fn(),
    innerJoin: vi.fn(),
    where: vi.fn(),
    limit,
  };
  query.from.mockReturnValue(query);
  query.innerJoin.mockReturnValue(query);
  query.where.mockReturnValue(query);
  const select = vi.fn(() => query);
  return { getAccount, getUsage, limit, select };
});

vi.mock("./db", () => ({
  getDatabase: vi.fn(() => ({ db: { select: mocks.select } })),
}));
vi.mock("./engine", () => ({
  getEngineClient: vi.fn(() => ({
    getAccount: mocks.getAccount,
    getUsage: mocks.getUsage,
  })),
}));

const VIEW_TOKEN = "abcdefghijklmnopqrstuv";
const ACCOUNT_ID = "acct_openkeys_cache_test";

function account(overrides: Partial<EngineAccount> = {}): EngineAccount {
  return {
    account: ACCOUNT_ID,
    balance_nano: "50000000000",
    reserved_nano: "0",
    spent_nano: "0",
    balance: "50.00",
    mult_bp: 10_000,
    status: "active",
    handle: null,
    ...overrides,
  };
}

function usage(window: string): EngineUsage {
  return {
    account: ACCOUNT_ID,
    window,
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
  };
}

function databaseResult(engineAccountId = ACCOUNT_ID) {
  return [{
    key: {
      viewToken: VIEW_TOKEN,
      keyMasked: "sk-pool-test…test",
      status: "active",
      createdAt: new Date("2026-01-01T00:00:00.000Z"),
      faceValueNano: 50_000_000_000n,
      multBp: 10_000,
      pricingContract: "official_1_to_1",
      engineAccountId,
    },
    apiType: "anthropic",
  }];
}

async function loadKeysModule() {
  return import("./keys");
}

describe("OpenKeys usage report cache", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00.000Z"));
    mocks.select.mockClear();
    mocks.limit.mockReset().mockResolvedValue(databaseResult());
    mocks.getAccount.mockReset().mockResolvedValue(account());
    mocks.getUsage.mockReset().mockImplementation((_accountId: string, window: string) => Promise.resolve(usage(window)));
  });

  it("reloads the database and account while reusing usage within the TTL", async () => {
    mocks.getAccount
      .mockResolvedValueOnce(account())
      .mockResolvedValueOnce(account({ balance_nano: "49000000000", spent_nano: "1000000000", status: "disabled" }));
    const { loadUsageByViewToken } = await loadKeysModule();

    const first = await loadUsageByViewToken(VIEW_TOKEN);
    const second = await loadUsageByViewToken(VIEW_TOKEN);

    expect(mocks.select).toHaveBeenCalledTimes(2);
    expect(mocks.getAccount).toHaveBeenCalledTimes(2);
    expect(mocks.getUsage).toHaveBeenCalledTimes(1);
    expect(first?.status).toBe("active");
    expect(second).toMatchObject({ status: "disabled", balanceNano: "49000000000", spentNano: "1000000000" });
    expect(second?.usage).toBe(first?.usage);
  });

  it("reloads usage after the 10 second TTL expires", async () => {
    const { loadUsageByViewToken } = await loadKeysModule();

    await loadUsageByViewToken(VIEW_TOKEN);
    vi.advanceTimersByTime(10_001);
    await loadUsageByViewToken(VIEW_TOKEN);

    expect(mocks.getUsage).toHaveBeenCalledTimes(2);
  });

  it("keeps an expired-by-clock in-flight request single-flight and starts TTL at settlement", async () => {
    let resolveUsage!: (value: EngineUsage) => void;
    mocks.getUsage
      .mockReturnValueOnce(new Promise<EngineUsage>((resolve) => {
        resolveUsage = resolve;
      }))
      .mockResolvedValueOnce(usage("30d"));
    const { loadUsageByViewToken } = await loadKeysModule();

    const first = loadUsageByViewToken(VIEW_TOKEN);
    await vi.waitFor(() => expect(mocks.getUsage).toHaveBeenCalledTimes(1));
    vi.advanceTimersByTime(10_001);
    const second = loadUsageByViewToken(VIEW_TOKEN);
    await vi.waitFor(() => expect(mocks.getAccount).toHaveBeenCalledTimes(2));
    expect(mocks.getUsage).toHaveBeenCalledTimes(1);

    resolveUsage(usage("30d"));
    const [firstView, secondView] = await Promise.all([first, second]);
    expect(firstView?.usage).toBe(secondView?.usage);

    vi.advanceTimersByTime(9_999);
    await loadUsageByViewToken(VIEW_TOKEN);
    expect(mocks.getUsage).toHaveBeenCalledTimes(1);

    vi.advanceTimersByTime(2);
    await loadUsageByViewToken(VIEW_TOKEN);
    expect(mocks.getUsage).toHaveBeenCalledTimes(2);
    expect(mocks.select).toHaveBeenCalledTimes(4);
    expect(mocks.getAccount).toHaveBeenCalledTimes(4);
  });

  it("keeps distinct usage entries for distinct windows", async () => {
    const { loadUsageByViewToken } = await loadKeysModule();

    await loadUsageByViewToken(VIEW_TOKEN, "30d");
    await loadUsageByViewToken(VIEW_TOKEN, "7d");
    await loadUsageByViewToken(VIEW_TOKEN, "30d");

    expect(mocks.getUsage).toHaveBeenCalledTimes(2);
    expect(mocks.getUsage).toHaveBeenNthCalledWith(1, ACCOUNT_ID, "30d");
    expect(mocks.getUsage).toHaveBeenNthCalledWith(2, ACCOUNT_ID, "7d");
  });

  it("evicts rejected usage requests so the next refresh retries", async () => {
    mocks.getUsage
      .mockRejectedValueOnce(new Error("usage unavailable"))
      .mockResolvedValueOnce(usage("30d"));
    const { loadUsageByViewToken } = await loadKeysModule();

    const failedView = await loadUsageByViewToken(VIEW_TOKEN);
    const retriedView = await loadUsageByViewToken(VIEW_TOKEN);

    expect(failedView?.usage).toBeNull();
    expect(retriedView?.usage).toEqual(usage("30d"));
    expect(mocks.getUsage).toHaveBeenCalledTimes(2);
  });

  it("bounds the usage cache at 1,000 entries", async () => {
    const { loadUsageByViewToken } = await loadKeysModule();

    for (let index = 0; index <= 1_000; index += 1) {
      await loadUsageByViewToken(VIEW_TOKEN, `${index}d`);
    }
    await loadUsageByViewToken(VIEW_TOKEN, "0d");

    expect(mocks.getUsage).toHaveBeenCalledTimes(1_002);
    expect(mocks.getUsage).toHaveBeenLastCalledWith(ACCOUNT_ID, "0d");
  });

  it("ignores stale settlement after a pending entry is evicted and replaced", async () => {
    let resolveStale!: (value: EngineUsage) => void;
    let resolveReplacement!: (value: EngineUsage) => void;
    const stale = new Promise<EngineUsage>((resolve) => {
      resolveStale = resolve;
    });
    const replacement = new Promise<EngineUsage>((resolve) => {
      resolveReplacement = resolve;
    });
    let targetLoads = 0;
    mocks.getUsage.mockImplementation((_accountId: string, window: string) => {
      if (window !== "target") return Promise.resolve(usage(window));
      targetLoads += 1;
      return targetLoads === 1 ? stale : replacement;
    });
    const { loadUsageByViewToken } = await loadKeysModule();

    const staleView = loadUsageByViewToken(VIEW_TOKEN, "target");
    await vi.waitFor(() => expect(targetLoads).toBe(1));
    for (let index = 0; index < 1_000; index += 1) {
      await loadUsageByViewToken(VIEW_TOKEN, `filler-${index}`);
    }
    const replacementView = loadUsageByViewToken(VIEW_TOKEN, "target");
    await vi.waitFor(() => expect(targetLoads).toBe(2));

    resolveStale(usage("target"));
    await staleView;
    vi.advanceTimersByTime(10_001);
    const sharedReplacementView = loadUsageByViewToken(VIEW_TOKEN, "target");
    await vi.waitFor(() => expect(mocks.getAccount).toHaveBeenCalledTimes(1_003));
    expect(targetLoads).toBe(2);

    resolveReplacement(usage("target"));
    const [replaced, shared] = await Promise.all([replacementView, sharedReplacementView]);
    expect(replaced?.usage).toBe(shared?.usage);
  });

  it("ignores stale rejection after a pending entry is evicted and replaced", async () => {
    let rejectStale!: (reason: Error) => void;
    let resolveReplacement!: (value: EngineUsage) => void;
    const stale = new Promise<EngineUsage>((_resolve, reject) => {
      rejectStale = reject;
    });
    const replacement = new Promise<EngineUsage>((resolve) => {
      resolveReplacement = resolve;
    });
    let targetLoads = 0;
    mocks.getUsage.mockImplementation((_accountId: string, window: string) => {
      if (window !== "target") return Promise.resolve(usage(window));
      targetLoads += 1;
      return targetLoads === 1 ? stale : replacement;
    });
    const { loadUsageByViewToken } = await loadKeysModule();

    const staleView = loadUsageByViewToken(VIEW_TOKEN, "target");
    await vi.waitFor(() => expect(targetLoads).toBe(1));
    for (let index = 0; index < 1_000; index += 1) {
      await loadUsageByViewToken(VIEW_TOKEN, `filler-${index}`);
    }
    const replacementView = loadUsageByViewToken(VIEW_TOKEN, "target");
    await vi.waitFor(() => expect(targetLoads).toBe(2));

    rejectStale(new Error("stale usage failure"));
    expect((await staleView)?.usage).toBeNull();
    const sharedReplacementView = loadUsageByViewToken(VIEW_TOKEN, "target");
    await vi.waitFor(() => expect(mocks.getAccount).toHaveBeenCalledTimes(1_003));
    expect(targetLoads).toBe(2);

    resolveReplacement(usage("target"));
    const [replaced, shared] = await Promise.all([replacementView, sharedReplacementView]);
    expect(replaced?.usage).toBe(shared?.usage);
  });
});
