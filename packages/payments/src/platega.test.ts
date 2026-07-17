import { describe, expect, it } from "vitest";
import { PlategaProvider } from "./platega.js";

const transactionId = "3fa85f64-5717-4562-b3fc-2c463f66afa6";
const checkoutId = "11111111-1111-4111-8111-111111111111";
const RATE_URL = "https://rates.test/rates";
const API_BASE = "https://platega.test";

function makeProvider(
  fetchImpl: typeof globalThis.fetch,
  overrides: Partial<ConstructorParameters<typeof PlategaProvider>[0]> = {},
): PlategaProvider {
  return new PlategaProvider({
    merchantId: "merchant-uuid",
    secret: "platega-secret",
    callbackUrl: "https://backend.apitoken.sale/v1/payments/platega/webhook",
    apiBaseUrl: API_BASE,
    rateUrl: RATE_URL,
    fetch: fetchImpl,
    ...overrides,
  });
}

function ratesResponse(askPrice: number): Response {
  return Response.json({ data: [{ symbol: "USDT/RUB", askPrice, close: askPrice - 1 }] });
}

describe("PlategaProvider", () => {
  it("charges RUB at the Rapira rate and returns the hosted redirect", async () => {
    let postBody = "";
    let postHeaders = new Headers();
    const provider = makeProvider(async (input, init) => {
      const url = String(input);
      if (url === RATE_URL) return ratesResponse(100);
      expect(url).toBe(`${API_BASE}/transaction/process`);
      postBody = String(init?.body);
      postHeaders = new Headers(init?.headers);
      // Real Platega shape: null (not absent) optionals, paymentDetails as a string.
      return Response.json({
        paymentMethod: "SBPQR",
        transactionId,
        redirect: "https://pay.platega.io?id=abc",
        return: null,
        paymentDetails: "108.5 RUB",
        status: "PENDING",
        expiresIn: null,
        merchantId: "merchant-uuid",
        usdtRate: 83.1,
      });
    });

    const checkout = await provider.createCheckout({
      checkoutId,
      amount: "25",
      customerEmail: "buyer@example.com",
      locale: "en-US",
      currency: "USD",
      returnUrl: "https://apitoken.sale/success",
      cancelUrl: "https://apitoken.sale/cancel",
    });

    expect(checkout).toMatchObject({
      action: { kind: "redirect", url: "https://pay.platega.io?id=abc" },
      providerPaymentId: transactionId,
    });
    expect(checkout.expiresAt).toBeNull(); // expiresIn was null
    expect(JSON.parse(postBody)).toMatchObject({
      paymentMethod: 2,
      paymentDetails: { amount: 2500, currency: "RUB" }, // 25 USD * 100 RUB/USD
      return: "https://apitoken.sale/success",
      failedUrl: "https://apitoken.sale/cancel",
      payload: checkoutId,
    });
    expect(postHeaders.get("X-MerchantId")).toBe("merchant-uuid");
    expect(postHeaders.get("X-Secret")).toBe("platega-secret");
  });

  it("applies the FX margin and the requested payment method, rounding RUB up", async () => {
    let postBody = "";
    const provider = makeProvider(async (input, init) => {
      const url = String(input);
      if (url === RATE_URL) return ratesResponse(100);
      postBody = String(init?.body);
      return Response.json({ transactionId, redirect: "https://pay.platega.io/abc", status: "PENDING" });
    }, { fxMarginBps: 300 });

    await provider.createCheckout({
      checkoutId,
      amount: "25",
      customerEmail: "b@e.com",
      locale: "en-US",
      currency: "USD",
      returnUrl: "https://apitoken.sale/success",
      cancelUrl: "https://apitoken.sale/cancel",
      paymentMethod: 11,
    });

    expect(JSON.parse(postBody)).toMatchObject({
      paymentMethod: 11,
      paymentDetails: { amount: 2575, currency: "RUB" }, // ceil(25 * 100 * 1.03)
    });
  });

  it("charges the crypto method directly in USD without touching Rapira", async () => {
    let postBody = "";
    let rateCalled = false;
    const provider = makeProvider(async (input, init) => {
      const url = String(input);
      if (url === RATE_URL) { rateCalled = true; return ratesResponse(100); }
      postBody = String(init?.body);
      return Response.json({ transactionId, redirect: "https://pay.platega.io?id=abc", status: "PENDING" });
    });

    await provider.createCheckout({
      checkoutId,
      amount: "100",
      customerEmail: "b@e.com",
      locale: "en-US",
      currency: "USD",
      returnUrl: "https://apitoken.sale/success",
      cancelUrl: "https://apitoken.sale/cancel",
      paymentMethod: 13,
    });

    expect(rateCalled).toBe(false);
    expect(JSON.parse(postBody)).toMatchObject({
      paymentMethod: 13,
      paymentDetails: { amount: 100, currency: "USD" },
    });
  });

  it("parses a webhook into a wake-up signal and rejects malformed bodies", () => {
    const provider = makeProvider(async () => new Response());
    expect(provider.verifyWebhook(JSON.stringify({ id: transactionId, status: "CONFIRMED", payload: checkoutId }))).toMatchObject({
      provider: "platega",
      providerPaymentId: transactionId,
      providerEventId: `${transactionId}:CONFIRMED`,
    });
    expect(() => provider.verifyWebhook("not json")).toThrow();
    expect(() => provider.verifyWebhook(JSON.stringify({ status: "CONFIRMED" }))).toThrow();
  });

  it("re-queries the authoritative status and maps Platega states", async () => {
    const provider = makeProvider(async (input) => {
      expect(String(input)).toBe(`${API_BASE}/transaction/${transactionId}`);
      return Response.json({
        id: transactionId,
        status: "CONFIRMED",
        paymentDetails: { amount: 2500, currency: "RUB" },
        payload: checkoutId,
      });
    });
    const payment = await provider.verifyPayment(transactionId);
    expect(payment).toMatchObject({
      provider: "platega",
      providerPaymentId: transactionId,
      providerEventId: `${transactionId}:CONFIRMED`,
      state: "paid",
      checkoutId,
      providerCurrency: "RUB",
      amountUsd: null,
    });
  });

  it("maps CANCELED and CHARGEBACKED to canceled and refunded", async () => {
    const canceled = makeProvider(async () => Response.json({ id: transactionId, status: "CANCELED", payload: checkoutId }));
    expect((await canceled.verifyPayment(transactionId)).state).toBe("canceled");
    const refunded = makeProvider(async () => Response.json({ id: transactionId, status: "CHARGEBACKED", payload: checkoutId }));
    expect((await refunded.verifyPayment(transactionId)).state).toBe("refunded");
  });

  it("rejects a non-positive Rapira rate", async () => {
    const provider = makeProvider(async (input) => {
      if (String(input) === RATE_URL) return Response.json({ data: [{ symbol: "USDT/RUB", askPrice: 0 }] });
      return new Response();
    });
    await expect(provider.createCheckout({
      checkoutId,
      amount: "25",
      customerEmail: "b@e.com",
      locale: "en-US",
      currency: "USD",
      returnUrl: "https://apitoken.sale/success",
      cancelUrl: "https://apitoken.sale/cancel",
    })).rejects.toThrow();
  });
});
