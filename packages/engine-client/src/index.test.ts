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
  it("confirms the engine applied exactly the requested multiplier", async () => {
    const engine = client(() => json({ account: "acct_1", mult_bp: 5_000, updated: 1 }));
    await expect(engine.setAccountMultiplier("acct_1", 5_000)).resolves.toBeUndefined();

    const drifted = client(() => json({ account: "acct_1", mult_bp: 4_000, updated: 1 }));
    await expect(drifted.setAccountMultiplier("acct_1", 5_000)).rejects.toBeInstanceOf(EngineClientError);
  });
});
