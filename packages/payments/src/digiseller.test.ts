import { createHash, createHmac } from "node:crypto";
import { describe, expect, it } from "vitest";
import { DigiSellerProvider } from "./digiseller.js";

const fixedNow = new Date("2026-07-13T12:00:00.000Z");

describe("DigiSellerProvider", () => {
  it("creates a provider form without embedding internal secrets", async () => {
    const provider = makeProvider(async () => new Response());
    const checkout = await provider.createCheckout({
      checkoutId: "checkout-123",
      amount: "25",
      customerEmail: "buyer@example.com",
      locale: "en-US",
      currency: "USD",
      returnUrl: "https://example.com/payments/return",
      cancelUrl: "https://example.com/payments/cancel",
    });
    expect(checkout.action.kind).toBe("form_post");
    if (checkout.action.kind !== "form_post") return;
    expect(checkout.action.fields).toMatchObject({ id_d: "777" });
    const checkoutUrl = new URL(checkout.action.url);
    expect(checkoutUrl.searchParams.get("checkout_id")).toBe("checkout-123");
    expect(checkoutUrl.searchParams.get("checkout_sig")).toBe(createHmac("sha256", "tracking-secret-with-enough-entropy")
      .update("checkout-123").digest("hex"));
    expect(JSON.stringify(checkout.action.fields)).not.toContain("seller-api-key");
  });

  it("logs in, verifies the invoice, and authenticates checkout tracking", async () => {
    const calls: Array<{ url: string; body?: string }> = [];
    const checkoutId = "7e2a9a72-c5d6-4a98-a0fc-c5c3e9490d31";
    const checkoutSig = createHmac("sha256", "tracking-secret-with-enough-entropy").update(checkoutId).digest("hex");
    const queryString = Buffer.from(new URLSearchParams({ checkout_id: checkoutId, checkout_sig: checkoutSig }).toString()).toString("base64");
    const provider = makeProvider(async (input, init) => {
      const url = String(input);
      calls.push({ url, ...(init?.body ? { body: String(init.body) } : {}) });
      if (url.endsWith("/api/apilogin")) {
        return Response.json({ retval: 0, desc: "", token: "token-1", valid_thru: "2026-07-13T14:00:00.000Z" });
      }
      return Response.json({
        retval: 0,
        retdesc: null,
        content: {
          item_id: 777,
          amount: 25,
          amount_usd: 25,
          currency_type: "WMZ",
          invoice_state: 3,
          date_pay: "2026-07-13T12:01:00Z",
          query_string: queryString,
          buyer_info: { email: "buyer@example.com" },
        },
      });
    });

    const verified = await provider.verifyPayment("123456789");
    const timestamp = Math.floor(fixedNow.getTime() / 1000);
    expect(JSON.parse(calls[0]!.body!)).toEqual({
      seller_id: 42,
      timestamp,
      sign: createHash("sha256").update(`seller-api-key${timestamp}`).digest("hex"),
    });
    expect(calls[1]!.url).toContain("/api/purchase/info/123456789?token=token-1");
    expect(verified).toMatchObject({
      state: "paid",
      providerPaymentId: "123456789",
      providerEventId: "123456789:3",
      providerProductId: "777",
      checkoutId,
      amountUsd: "25",
    });
  });

  it("rejects tampered checkout tracking", async () => {
    const queryString = Buffer.from("checkout_id=checkout-123&checkout_sig=00").toString("base64");
    const provider = makeProvider(async (input) => String(input).endsWith("/api/apilogin")
      ? Response.json({ retval: 0, token: "token-1" })
      : Response.json({
        retval: 0,
        content: {
          item_id: 777, amount: "10.00", amount_usd: "10.00", currency_type: "WMZ",
          invoice_state: 3, date_pay: null, query_string: queryString, buyer_info: null,
        },
      }));
    await expect(provider.verifyPayment("1")).resolves.toMatchObject({ checkoutId: null });
  });

  it("verifies the automatic redirect unique code through the seller API", async () => {
    const provider = makeProvider(async (input) => {
      const url = String(input);
      if (url.endsWith("/api/apilogin")) return Response.json({ retval: 0, token: "token-1" });
      expect(url).toContain("/api/purchases/unique-code/1234567890123456?token=token-1");
      return Response.json({
        retval: 0,
        retdesc: "",
        inv: 987654321,
        id_goods: 777,
        amount: "25.00",
        amount_usd: "25.00",
        type_curr: "WMZ",
        date_pay: "2026-07-13T12:01:00Z",
        email: "buyer@example.com",
        query_string: null,
      });
    });
    await expect(provider.verifyUniqueCode("1234567890123456")).resolves.toMatchObject({
      providerPaymentId: "987654321",
      providerEventId: "987654321:3",
      state: "paid",
      providerProductId: "777",
      providerAmount: "25.00",
    });
  });
});

function makeProvider(fetch: typeof globalThis.fetch): DigiSellerProvider {
  return new DigiSellerProvider({
    sellerId: 42,
    apiKey: "seller-api-key",
    productId: 777,
    checkoutTrackingSecret: "tracking-secret-with-enough-entropy",
    apiBaseUrl: "https://api.test",
    checkoutUrl: "https://checkout.test/pay",
    fetch,
    now: () => fixedNow,
  });
}
