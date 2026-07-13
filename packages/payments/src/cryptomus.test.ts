import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";
import { CryptomusProvider } from "./cryptomus.js";

const paymentId = "26109ba0-b05b-4ee0-93d1-fd62c822ce95";

describe("CryptomusProvider", () => {
  it("creates a signed hosted invoice with idempotent local identity", async () => {
    let requestBody = "";
    let requestHeaders = new Headers();
    const provider = makeProvider(async (input, init) => {
      expect(String(input)).toBe("https://api.test/v1/payment");
      requestBody = String(init?.body);
      requestHeaders = new Headers(init?.headers);
      return Response.json(successfulPayment({ status: "check", payment_status: "check", is_final: false }));
    });

    const checkout = await provider.createCheckout({
      checkoutId: "checkout-123",
      amount: "25",
      customerEmail: "buyer@example.com",
      locale: "en-US",
      currency: "USD",
      returnUrl: "https://apitoken.sale/payments/success",
      cancelUrl: "https://apitoken.sale/payments/cancel",
    });

    expect(checkout).toMatchObject({
      action: { kind: "redirect", url: `https://pay.test/pay/${paymentId}` },
      providerPaymentId: paymentId,
    });
    expect(JSON.parse(requestBody)).toMatchObject({
      amount: "25",
      currency: "USD",
      order_id: "checkout-123",
      url_callback: "https://api.apitoken.sale/v1/payments/cryptomus/webhook",
      is_payment_multiple: true,
      additional_data: JSON.stringify({ checkoutId: "checkout-123" }),
    });
    expect(requestHeaders.get("merchant")).toBe("merchant-uuid");
    expect(requestHeaders.get("sign")).toBe(signRequest(requestBody));
    expect(requestBody).not.toContain("payment-api-key");
  });

  it("verifies a signed webhook and rejects tampering", () => {
    const provider = makeProvider(async () => new Response());
    const unsigned = {
      type: "payment",
      uuid: paymentId,
      order_id: "checkout-123",
      status: "paid",
      is_final: true,
      callback: "https://apitoken.sale/payment/complete",
    };
    const body = JSON.stringify({ ...unsigned, sign: signWebhook(unsigned) });
    expect(provider.verifyWebhook(body)).toMatchObject({
      provider: "cryptomus",
      providerPaymentId: paymentId,
      providerEventId: `${paymentId}:paid`,
    });
    expect(() => provider.verifyWebhook(body.replace('"paid"', '"cancel"'))).toThrow("signature is invalid");
  });

  it("rechecks payment information and maps paid_over to paid", async () => {
    const provider = makeProvider(async (input, init) => {
      expect(String(input)).toBe("https://api.test/v1/payment/info");
      expect(JSON.parse(String(init?.body))).toEqual({ uuid: paymentId });
      expect(new Headers(init?.headers).get("sign")).toBe(signRequest(String(init?.body)));
      return Response.json(successfulPayment({
        status: "paid_over",
        payment_status: "paid_over",
        is_final: true,
        updated_at: "2026-07-13T15:01:00+03:00",
      }));
    });

    await expect(provider.verifyPayment(paymentId)).resolves.toMatchObject({
      providerPaymentId: paymentId,
      providerEventId: `${paymentId}:paid_over`,
      state: "paid",
      providerProductId: null,
      checkoutId: "checkout-123",
      providerAmount: "25.00",
      providerCurrency: "USD",
      amountUsd: "25.00",
      paidAt: "2026-07-13T15:01:00+03:00",
    });
  });

  it("does not treat underpayments or in-progress refunds as paid", async () => {
    for (const status of ["wrong_amount", "confirm_check", "refund_process", "refund_fail"]) {
      const provider = makeProvider(async () => Response.json(successfulPayment({ status, payment_status: status })));
      await expect(provider.verifyPayment(paymentId)).resolves.toMatchObject({ state: "pending" });
    }
  });
});

function makeProvider(fetch: typeof globalThis.fetch): CryptomusProvider {
  return new CryptomusProvider({
    merchantId: "merchant-uuid",
    paymentApiKey: "payment-api-key",
    callbackUrl: "https://api.apitoken.sale/v1/payments/cryptomus/webhook",
    apiBaseUrl: "https://api.test",
    fetch,
  });
}

function successfulPayment(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    state: 0,
    result: {
      uuid: paymentId,
      order_id: "checkout-123",
      amount: "25.00",
      currency: "USD",
      payment_status: "paid",
      status: "paid",
      url: `https://pay.test/pay/${paymentId}`,
      is_final: true,
      additional_data: JSON.stringify({ checkoutId: "checkout-123" }),
      updated_at: "2026-07-13T15:00:00+03:00",
      expired_at: 1_783_955_600,
      ...overrides,
    },
  };
}

function signRequest(body: string): string {
  return createHash("md5").update(`${Buffer.from(body).toString("base64")}payment-api-key`).digest("hex");
}

function signWebhook(body: unknown): string {
  const json = JSON.stringify(body).replace(/\//g, "\\/");
  return signRequest(json);
}
