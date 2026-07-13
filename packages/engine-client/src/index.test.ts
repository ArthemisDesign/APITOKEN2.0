import { describe, expect, it } from "vitest";
import { EngineClient } from "./index.js";

describe("EngineClient", () => {
  it("sends nanodollars without floating-point conversion", async () => {
    let requestBody = "";
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (_input, init) => {
        requestBody = String(init?.body);
        return new Response('{"account":"acct_test","balance_nano":9007199254740993123,"balance":"$9007199254.740993123"}');
      },
    });

    const result = await client.creditAccount("acct_test", 9_007_199_254_740_993_123n, "payment:test");
    expect(requestBody).toContain('"amount_nano":9007199254740993123');
    expect(result.balance_nano).toBe("9007199254740993123");
  });

  it("does not send the control key to the public health endpoint", async () => {
    let sentHeaders: RequestInit["headers"];
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "must-not-leak",
      fetch: async (_input, init) => {
        sentHeaders = init?.headers;
        return new Response('{"ok":true}');
      },
    });
    await expect(client.health()).resolves.toBe(true);
    expect(sentHeaders).not.toHaveProperty("x-api-key");
  });

  it("normalizes exact engine integer fields to decimal strings", async () => {
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => Response.json({
        account: "acct_test",
        balance_nano: 0,
        spent_nano: 12,
        reserved_nano: 0,
        balance: "$0.000000000",
        mult_bp: 2000,
        status: "active",
        handle: null,
      }),
    });
    const account = await client.getAccount("acct_test");
    expect(account).toMatchObject({ balance_nano: "0", spent_nano: "12", reserved_nano: "0" });
  });

  it("manages keys by a non-secret stable identifier", async () => {
    const requests: Array<{ url: string; body: string }> = [];
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        const url = String(input);
        requests.push({ url, body: String(init?.body ?? "") });
        if (url.endsWith("/admin/key")) {
          return Response.json({
            key: "sk-pool-secret", key_id: "key_public", account: "acct_test", label: "prod",
          });
        }
        if (url.includes("/key-id/")) {
          return Response.json({ key_id: "key_public", status: "disabled", updated: 1 });
        }
        return Response.json({
          account: "acct_test",
          keys: [{
            key_id: "key_public", key_masked: "sk-pool-sec…cret", label: "prod",
            status: "active", spent_nano: 0, spent: "$0.000000000",
          }],
        });
      },
    });

    const issued = await client.issueKey("acct_test", "prod");
    expect(issued.key_id).toBe("key_public");
    await expect(client.listKeys("acct_test")).resolves.toMatchObject([{ spent_nano: "0" }]);
    await expect(client.disableKey("key_public")).resolves.toBeUndefined();
    expect(requests.at(-1)?.url).toContain("/admin/key-id/key_public/status");
    expect(requests.at(-1)?.body).not.toContain("sk-pool-secret");
  });

  it("reads a bounded ledger without numeric money conversion", async () => {
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => Response.json({
        account: "acct_test",
        entries: [{
          id: 1, kind: "topup", amount_nano: 1_000_000_000, amount: "$1.000000000",
          key_masked: null, ref: "payment:1", balance_after_nano: 1_000_000_000, ts: 1_700_000_000,
        }],
      }),
    });
    const entries = await client.getLedger("acct_test", 10);
    expect(entries[0]).toMatchObject({ id: "1", amount_nano: "1000000000", ts: "1700000000" });
    await expect(client.getLedger("acct_test", 0)).rejects.toThrow("limit");
  });

  it("reads ledger pages after an exact cursor", async () => {
    let requestedUrl = "";
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input) => {
        requestedUrl = String(input);
        return Response.json({ account: "acct_test", entries: [] });
      },
    });
    await expect(client.getLedgerAfter("acct_test", 9_007_199_254_740_993_123n, 1000)).resolves.toEqual([]);
    expect(requestedUrl).toContain("after_id=9007199254740993123&limit=1000");
  });

  it("updates account pricing through the control API", async () => {
    let request: { url: string; body: string } | undefined;
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        request = { url: String(input), body: String(init?.body) };
        return Response.json({ account: "acct_test", mult_bp: 3500, updated: 1 });
      },
    });
    await expect(client.setAccountMultiplier("acct_test", 3500)).resolves.toBeUndefined();
    expect(request).toEqual({
      url: "http://engine.test/admin/account/acct_test/pricing",
      body: '{"mult_bp":3500}',
    });
    await expect(client.setAccountMultiplier("acct_test", 10_001)).rejects.toThrow("multiplierBp");
  });
});
