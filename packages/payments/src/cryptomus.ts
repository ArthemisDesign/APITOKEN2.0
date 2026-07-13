import { createHash, timingSafeEqual } from "node:crypto";
import { z } from "zod";
import type {
  CheckoutAction,
  CheckoutContext,
  ProviderPaymentState,
  VerifiedProviderPayment,
  VerifiedWebhookSignal,
  WebhookPaymentProviderAdapter,
} from "./provider.js";

const paymentSchema = z.object({
  uuid: z.string().uuid(),
  order_id: z.string().min(1),
  amount: z.string().min(1),
  currency: z.string().min(1),
  payment_status: z.string().optional(),
  status: z.string().min(1),
  url: z.string().url(),
  is_final: z.boolean(),
  additional_data: z.string().nullish(),
  updated_at: z.string().nullish(),
}).passthrough();

const apiResponseSchema = z.object({
  state: z.number().int(),
  result: paymentSchema.optional(),
  message: z.string().optional(),
  errors: z.record(z.unknown()).optional(),
});

const webhookSchema = z.object({
  type: z.literal("payment"),
  uuid: z.string().uuid(),
  order_id: z.string().min(1),
  status: z.string().min(1),
  sign: z.string().regex(/^[a-fA-F0-9]{32}$/),
}).passthrough();

const metadataSchema = z.object({
  checkoutId: z.string().min(1),
  productId: z.string().min(1),
});

export interface CryptomusOptions {
  merchantId: string;
  paymentApiKey: string;
  callbackUrl: string;
  apiBaseUrl?: string;
  invoiceLifetimeSeconds?: number;
  fetch?: typeof globalThis.fetch;
}

export class CryptomusError extends Error {
  constructor(message: string, readonly retryable: boolean) {
    super(message);
    this.name = "CryptomusError";
  }
}

export class CryptomusProvider implements WebhookPaymentProviderAdapter {
  readonly code = "cryptomus";
  private readonly apiBaseUrl: string;
  private readonly fetchImpl: typeof globalThis.fetch;
  private readonly invoiceLifetimeSeconds: number;

  constructor(private readonly options: CryptomusOptions) {
    this.apiBaseUrl = (options.apiBaseUrl ?? "https://api.cryptomus.com").replace(/\/$/, "");
    this.fetchImpl = options.fetch ?? globalThis.fetch;
    this.invoiceLifetimeSeconds = options.invoiceLifetimeSeconds ?? 3600;
    if (this.invoiceLifetimeSeconds < 300 || this.invoiceLifetimeSeconds > 43_200) {
      throw new CryptomusError("Cryptomus invoice lifetime must be between 300 and 43200 seconds", false);
    }
  }

  async createCheckout(context: CheckoutContext): Promise<CheckoutAction> {
    if (!/^[A-Za-z0-9_-]{1,128}$/.test(context.checkoutId)) {
      throw new CryptomusError("Cryptomus checkout ID must be 1-128 alpha-dash characters", false);
    }
    if (!/^(?:0|[1-9]\d*)(?:\.\d+)?$/.test(context.amount) || /^0(?:\.0+)?$/.test(context.amount)) {
      throw new CryptomusError("Cryptomus amount must be a positive decimal string", false);
    }
    const additionalData = JSON.stringify({ checkoutId: context.checkoutId, productId: context.productId });
    if (Buffer.byteLength(additionalData, "utf8") > 255) {
      throw new CryptomusError("Cryptomus checkout metadata exceeds 255 bytes", false);
    }

    const payment = await this.request("/v1/payment", {
      amount: context.amount,
      currency: context.currency,
      order_id: context.checkoutId,
      url_return: context.cancelUrl,
      url_success: context.returnUrl,
      url_callback: this.options.callbackUrl,
      is_payment_multiple: true,
      lifetime: this.invoiceLifetimeSeconds,
      additional_data: additionalData,
    });
    if (payment.order_id !== context.checkoutId) {
      throw new CryptomusError("Cryptomus returned a mismatched order ID", true);
    }
    return { kind: "redirect", url: payment.url };
  }

  verifyWebhook(rawBody: string | Uint8Array): VerifiedWebhookSignal {
    const body = typeof rawBody === "string" ? rawBody : Buffer.from(rawBody).toString("utf8");
    let raw: unknown;
    try {
      raw = JSON.parse(body);
    } catch {
      throw new CryptomusError("Cryptomus webhook is not valid JSON", false);
    }
    const parsed = webhookSchema.safeParse(raw);
    if (!parsed.success) throw new CryptomusError("Cryptomus webhook has an invalid shape", false);

    const unsigned = { ...(raw as Record<string, unknown>) };
    delete unsigned.sign;
    const expected = this.sign(phpCompatibleJson(unsigned));
    if (!safeHexEqual(expected, parsed.data.sign)) {
      throw new CryptomusError("Cryptomus webhook signature is invalid", false);
    }
    return {
      provider: this.code,
      providerPaymentId: parsed.data.uuid,
      providerEventId: `${parsed.data.uuid}:${parsed.data.status}`,
      raw,
    };
  }

  async verifyPayment(providerPaymentId: string): Promise<VerifiedProviderPayment> {
    if (!z.string().uuid().safeParse(providerPaymentId).success) {
      throw new CryptomusError("Cryptomus payment ID must be a UUID", false);
    }
    const payment = await this.request("/v1/payment/info", { uuid: providerPaymentId });
    if (payment.uuid !== providerPaymentId) {
      throw new CryptomusError("Cryptomus returned a mismatched payment ID", true);
    }
    const metadata = parseMetadata(payment.additional_data);
    const state = paymentState(payment.status);
    return {
      provider: this.code,
      providerPaymentId: payment.uuid,
      providerEventId: `${payment.uuid}:${payment.status}`,
      state,
      productId: metadata.productId,
      checkoutId: metadata.checkoutId,
      paidAt: state === "paid" ? payment.updated_at ?? null : null,
      buyerEmail: null,
      providerAmount: payment.amount,
      providerCurrency: payment.currency,
      amountUsd: payment.currency.toUpperCase() === "USD" ? payment.amount : null,
      raw: payment,
    };
  }

  private async request(path: string, data: Record<string, unknown>): Promise<z.infer<typeof paymentSchema>> {
    const body = JSON.stringify(data);
    const response = await this.fetchImpl(`${this.apiBaseUrl}${path}`, {
      method: "POST",
      headers: {
        accept: "application/json",
        "content-type": "application/json",
        merchant: this.options.merchantId,
        sign: this.sign(body),
      },
      body,
    });
    if (!response.ok) {
      throw new CryptomusError(`Cryptomus returned HTTP ${response.status}`, response.status >= 500 || response.status === 429);
    }
    let json: unknown;
    try {
      json = await response.json();
    } catch {
      throw new CryptomusError("Cryptomus returned invalid JSON", true);
    }
    const parsed = apiResponseSchema.safeParse(json);
    if (!parsed.success) throw new CryptomusError("Cryptomus returned an invalid payment response", true);
    if (parsed.data.state !== 0 || !parsed.data.result) {
      throw new CryptomusError(parsed.data.message ?? "Cryptomus rejected the request", false);
    }
    return parsed.data.result;
  }

  private sign(body: string): string {
    const encoded = Buffer.from(body, "utf8").toString("base64");
    return createHash("md5").update(`${encoded}${this.options.paymentApiKey}`, "utf8").digest("hex");
  }
}

function parseMetadata(value: string | null | undefined): z.infer<typeof metadataSchema> {
  if (!value) throw new CryptomusError("Cryptomus payment has no checkout metadata", false);
  try {
    return metadataSchema.parse(JSON.parse(value));
  } catch {
    throw new CryptomusError("Cryptomus payment has invalid checkout metadata", false);
  }
}

function paymentState(status: string): ProviderPaymentState {
  switch (status) {
    case "paid":
    case "paid_over":
      return "paid";
    case "fail":
    case "cancel":
    case "system_fail":
      return "canceled";
    case "refund_paid":
      return "refunded";
    default:
      return "pending";
  }
}

// Cryptomus signs webhook JSON using PHP's default escaped-slash representation.
function phpCompatibleJson(value: unknown): string {
  return JSON.stringify(value).replace(/\//g, "\\/");
}

function safeHexEqual(expected: string, supplied: string): boolean {
  const expectedBytes = Buffer.from(expected, "hex");
  const suppliedBytes = Buffer.from(supplied, "hex");
  return expectedBytes.length === suppliedBytes.length && timingSafeEqual(expectedBytes, suppliedBytes);
}
