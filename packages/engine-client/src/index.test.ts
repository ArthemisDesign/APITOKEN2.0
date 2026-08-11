import { describe, expect, it, vi } from "vitest";
import { EngineClient, EngineClientError } from "./index.js";

function client(handler: (url: string, init: RequestInit) => Response): EngineClient {
  return new EngineClient({
    baseUrl: "https://engine.test",
    controlKey: "control-key",
    fetch: ((input: unknown, init?: RequestInit) =>
      Promise.resolve(handler(String(input), init ?? {}))) as unknown as typeof globalThis.fetch,
  });
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

describe("account discounts", () => {
  it("sets one provider override and clears it with a null multiplier", async () => {
    const calls: Array<{ url: string; body: unknown }> = [];
    const engine = client((url, init) => {
      const body = JSON.parse(String(init.body)) as { provider_id: string; mult_bp: number | null };
      calls.push({ url, body });
      return json({ account: "acct_1", provider_id: body.provider_id, mult_bp: body.mult_bp, changed: true });
    });

    await engine.setAccountProviderDiscount("acct_1", "openai", 2_000);
    await engine.setAccountProviderDiscount("acct_1", "openai", null);

    expect(calls).toEqual([
      {
        url: "https://engine.test/admin/account/acct_1/discounts",
        body: { provider_id: "openai", mult_bp: 2_000 },
      },
      {
        url: "https://engine.test/admin/account/acct_1/discounts",
        body: { provider_id: "openai", mult_bp: null },
      },
    ]);
  });

  it("rejects a multiplier outside the payable range before any request", async () => {
    const fetchImpl = vi.fn();
    const engine = new EngineClient({
      baseUrl: "https://engine.test",
      controlKey: "control-key",
      fetch: fetchImpl as unknown as typeof globalThis.fetch,
    });

    await expect(engine.setAccountProviderDiscount("acct_1", "openai", 10_001))
      .rejects.toBeInstanceOf(RangeError);
    await expect(engine.setAccountProviderDiscount("acct_1", "openai", -1))
      .rejects.toBeInstanceOf(RangeError);
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("reads the account default with its per-provider overrides", async () => {
    const engine = client(() => json({
      account: "acct_1",
      mult_bp: 5_000,
      providers: { openai: 2_000, google: 10_000 },
    }));

    await expect(engine.getAccountDiscounts("acct_1")).resolves.toEqual({
      multiplierBp: 5_000,
      providers: { openai: 2_000, google: 10_000 },
    });
  });

  it("fails closed when the engine answers about another account", async () => {
    const engine = client(() => json({ account: "acct_other", mult_bp: 5_000, providers: {} }));

    await expect(engine.getAccountDiscounts("acct_1")).rejects.toBeInstanceOf(EngineClientError);
  });
});

describe("account multiplier", () => {
  it("rejects an account-creation multiplier outside the engine range before any request", async () => {
    const fetchImpl = vi.fn();
    const engine = new EngineClient({
      baseUrl: "https://engine.test",
      controlKey: "control-key",
      fetch: fetchImpl as unknown as typeof globalThis.fetch,
    });

    await expect(engine.createAccount({ multBp: 10_001 })).rejects.toBeInstanceOf(RangeError);
    await expect(engine.createAccount({ multBp: -1 })).rejects.toBeInstanceOf(RangeError);
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("rejects an invalid account handle with an accurate error before any request", async () => {
    const fetchImpl = vi.fn();
    const engine = new EngineClient({
      baseUrl: "https://engine.test",
      controlKey: "control-key",
      fetch: fetchImpl as unknown as typeof globalThis.fetch,
    });

    await expect(engine.createAccount({ handle: "   " }))
      .rejects.toThrow("account handle must be 1 to 200 non-whitespace characters");
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("fails closed when account creation returns an out-of-range multiplier", async () => {
    const engine = client(() => json({ account: "acct_1", mult_bp: 10_001, handle: null }));

    await expect(engine.createAccount({ multBp: 10_000 }))
      .rejects.toBeInstanceOf(EngineClientError);
  });

  it("confirms the engine applied exactly the requested multiplier", async () => {
    const engine = client(() => json({ account: "acct_1", mult_bp: 5_000, updated: 1 }));
    await expect(engine.setAccountMultiplier("acct_1", 5_000)).resolves.toBeUndefined();

    const drifted = client(() => json({ account: "acct_1", mult_bp: 4_000, updated: 1 }));
    await expect(drifted.setAccountMultiplier("acct_1", 5_000)).rejects.toBeInstanceOf(EngineClientError);
  });
});

describe("account debit", () => {
  it("sends one signed adjustment while exposing a positive magnitude to callers", async () => {
    const calls: Array<{ url: string; body: unknown }> = [];
    const engine = client((url, init) => {
      calls.push({ url, body: JSON.parse(String(init.body)) });
      return json({ account: "acct_1", balance_nano: "-25000000000", balance: "-25.000000000" });
    });

    await expect(engine.debitAccount("acct_1", 25_000_000_000n, "refund:payment-1"))
      .resolves.toMatchObject({ account: "acct_1", balance_nano: "-25000000000" });
    expect(calls).toEqual([{
      url: "https://engine.test/admin/account/acct_1/credit",
      body: { amount_nano: "-25000000000", ref: "refund:payment-1" },
    }]);
  });

  it("rejects a non-positive debit before any request", async () => {
    const fetchImpl = vi.fn();
    const engine = new EngineClient({
      baseUrl: "https://engine.test",
      controlKey: "control-key",
      fetch: fetchImpl as unknown as typeof globalThis.fetch,
    });

    await expect(engine.debitAccount("acct_1", 0n, "refund:payment-1"))
      .rejects.toBeInstanceOf(RangeError);
    await expect(engine.debitAccount("acct_1", -1n, "refund:payment-1"))
      .rejects.toBeInstanceOf(RangeError);
    await expect(engine.debitAccount("acct_1", 9_223_372_036_854_775_808n, "refund:payment-1"))
      .rejects.toBeInstanceOf(RangeError);
    await expect(engine.debitAccount("acct_1", 1n, ""))
      .rejects.toBeInstanceOf(RangeError);
    await expect(engine.debitAccount("acct_1", 1n, "refund with space:payment-1"))
      .rejects.toBeInstanceOf(RangeError);
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("fails closed when a debit response names another account", async () => {
    const engine = client(() => json({
      account: "acct_other",
      balance_nano: "0",
      balance: "0.000000000",
    }));

    await expect(engine.debitAccount("acct_1", 1n, "refund:payment-1"))
      .rejects.toBeInstanceOf(EngineClientError);
  });
});

describe("settlement shortfall ledger evidence", () => {
  const charge = {
    id: "7",
    kind: "charge",
    request_id: "request-7",
    amount_nano: "100",
    amount: "0.000000100",
    key_masked: null,
    ref: null,
    balance_after_nano: "-1000000000",
    ts: "1700000000",
    model: "gpt-test",
    provider: "openai",
    official_nano: "200",
  } as const;

  it("defaults an older producer to zero and preserves additive evidence", async () => {
    const older = client(() => json({ account: "acct_1", entries: [charge] }));
    await expect(older.getLedgerAfter("acct_1", 0n)).resolves.toMatchObject([
      { amount_nano: "100", uncollected_nano: "0" },
    ]);

    const expanded = client(() => json({
      account: "acct_1",
      entries: [{ ...charge, uncollected_nano: "40" }],
    }));
    await expect(expanded.getLedgerAfter("acct_1", 0n)).resolves.toMatchObject([
      { amount_nano: "100", uncollected_nano: "40" },
    ]);
  });

  it("fails closed on impossible shortfall evidence", async () => {
    const excessive = client(() => json({
      account: "acct_1",
      entries: [{ ...charge, uncollected_nano: "101" }],
    }));
    await expect(excessive.getLedgerAfter("acct_1", 0n)).rejects.toThrow();

    const nonCharge = client(() => json({
      account: "acct_1",
      entries: [{ ...charge, kind: "topup", uncollected_nano: "1" }],
    }));
    await expect(nonCharge.getLedgerAfter("acct_1", 0n)).rejects.toThrow();
  });
});
