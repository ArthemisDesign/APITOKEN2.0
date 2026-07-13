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
});
