import { z } from "zod";
import type {
  CheckoutCreation,
  CheckoutContext,
  ProviderPaymentState,
  VerifiedProviderPayment,
  VerifiedWebhookSignal,
  WebhookPaymentProviderAdapter,
} from "./provider.js";

// Platega charges in RUB (SBP / ERIP / card / crypto). Our balance is priced in whole USD, so we
// convert USD -> RUB at the Rapira USDT/RUB rate ONLY to decide how much to collect. The engine
// credit is always the recorded USD (enforced by the DB), never the RUB the customer paid.
const DEFAULT_API_BASE_URL = "https://app.platega.io";
const DEFAULT_RATE_URL = "https://api.rapira.net/open/market/rates";

// POST /transaction/process response.
const createResponseSchema = z.object({
  transactionId: z.string().uuid(),
  redirect: z.string().url(),
  status: z.string().min(1),
  expiresIn: z.string().optional(),
  paymentMethod: z.string().optional(),
  merchantId: z.string().optional(),
}).passthrough();

// GET /transaction/{id} response.
const statusResponseSchema = z.object({
  id: z.string().uuid(),
  status: z.string().min(1),
  paymentDetails: z.object({ amount: z.number(), currency: z.string() }).partial().passthrough().optional(),
  amount: z.number().optional(),
  currency: z.string().optional(),
  paymentMethod: z.union([z.string(), z.number()]).optional(),
  payload: z.string().optional(),
}).passthrough();

// Callback body (POST to our webhook). Auth is via X-MerchantId / X-Secret headers, checked by the caller.
const webhookSchema = z.object({
  id: z.string().uuid(),
  amount: z.number().optional(),
  currency: z.string().optional(),
  status: z.string().min(1),
  paymentMethod: z.union([z.number(), z.string()]).optional(),
  payload: z.string().optional(),
}).passthrough();

// Rapira public rates: data[] with baseCurrency RUB / quoteCurrency USDT, price = RUB per 1 USDT.
const rapiraRatesSchema = z.object({
  data: z.array(z.object({
    symbol: z.string(),
    close: z.number().optional(),
    askPrice: z.number().optional(),
  }).passthrough()),
});

export interface PlategaOptions {
  merchantId: string;
  secret: string;
  callbackUrl: string;
  apiBaseUrl?: string;
  rateUrl?: string;
  /** Added on top of the Rapira rate, in basis points, to cover Platega's fee and FX drift. */
  fxMarginBps?: number;
  /** Platega payment-method id used when the checkout does not request a specific one. */
  defaultPaymentMethod?: number;
  fetch?: typeof globalThis.fetch;
}

export class PlategaError extends Error {
  constructor(message: string, readonly retryable: boolean) {
    super(message);
    this.name = "PlategaError";
  }
}

export class PlategaProvider implements WebhookPaymentProviderAdapter {
  readonly code = "platega";
  private readonly apiBaseUrl: string;
  private readonly rateUrl: string;
  private readonly fetchImpl: typeof globalThis.fetch;
  private readonly fxMarginBps: number;
  private readonly defaultPaymentMethod: number;

  constructor(private readonly options: PlategaOptions) {
    this.apiBaseUrl = (options.apiBaseUrl ?? DEFAULT_API_BASE_URL).replace(/\/$/, "");
    this.rateUrl = options.rateUrl ?? DEFAULT_RATE_URL;
    this.fetchImpl = options.fetch ?? globalThis.fetch;
    this.fxMarginBps = options.fxMarginBps ?? 0;
    this.defaultPaymentMethod = options.defaultPaymentMethod ?? 2;
    if (!options.merchantId) throw new PlategaError("Platega merchant ID is required", false);
    if (!options.secret) throw new PlategaError("Platega secret is required", false);
    if (this.fxMarginBps < 0 || this.fxMarginBps > 5000) {
      throw new PlategaError("Platega FX margin must be between 0 and 5000 bps", false);
    }
    if (!Number.isInteger(this.defaultPaymentMethod) || this.defaultPaymentMethod <= 0) {
      throw new PlategaError("Platega default payment method must be a positive integer", false);
    }
  }

  async createCheckout(context: CheckoutContext): Promise<CheckoutCreation> {
    if (!z.string().uuid().safeParse(context.checkoutId).success) {
      throw new PlategaError("Platega checkout ID must be a UUID", false);
    }
    if (!/^[1-9]\d*$/.test(context.amount)) {
      throw new PlategaError("Platega amount must be a positive whole USD string", false);
    }
    const paymentMethod = context.paymentMethod ?? this.defaultPaymentMethod;
    if (!Number.isInteger(paymentMethod) || paymentMethod <= 0) {
      throw new PlategaError("Platega payment method must be a positive integer", false);
    }

    const amountRub = await this.usdToRub(context.amount);
    const created = await this.request("POST", "/transaction/process", {
      paymentMethod,
      paymentDetails: { amount: amountRub, currency: "RUB" },
      description: `Top up ${context.amount} USD`,
      return: context.returnUrl,
      failedUrl: context.cancelUrl,
      payload: context.checkoutId,
    }, createResponseSchema);

    return {
      action: { kind: "redirect", url: created.redirect },
      providerPaymentId: created.transactionId,
      expiresAt: expiresAtFrom(created.expiresIn),
      raw: created,
    };
  }

  verifyWebhook(rawBody: string | Uint8Array): VerifiedWebhookSignal {
    const body = typeof rawBody === "string" ? rawBody : Buffer.from(rawBody).toString("utf8");
    let raw: unknown;
    try {
      raw = JSON.parse(body);
    } catch {
      throw new PlategaError("Platega webhook is not valid JSON", false);
    }
    const parsed = webhookSchema.safeParse(raw);
    if (!parsed.success) throw new PlategaError("Platega webhook has an invalid shape", false);
    return {
      provider: this.code,
      providerPaymentId: parsed.data.id,
      providerEventId: `${parsed.data.id}:${parsed.data.status.toUpperCase()}`,
      raw,
    };
  }

  async verifyPayment(providerPaymentId: string): Promise<VerifiedProviderPayment> {
    if (!z.string().uuid().safeParse(providerPaymentId).success) {
      throw new PlategaError("Platega transaction ID must be a UUID", false);
    }
    const payment = await this.request("GET", `/transaction/${providerPaymentId}`, undefined, statusResponseSchema);
    if (payment.id !== providerPaymentId) {
      throw new PlategaError("Platega returned a mismatched transaction ID", true);
    }
    const state = paymentState(payment.status);
    const providerAmount = payment.paymentDetails?.amount ?? payment.amount;
    const providerCurrency = payment.paymentDetails?.currency ?? payment.currency ?? "RUB";
    return {
      provider: this.code,
      providerPaymentId: payment.id,
      providerEventId: `${payment.id}:${payment.status.toUpperCase()}`,
      state,
      providerProductId: null,
      checkoutId: payment.payload ?? null,
      paidAt: null,
      buyerEmail: null,
      providerAmount: providerAmount !== undefined ? String(providerAmount) : "",
      providerCurrency,
      amountUsd: null,
      raw: payment,
    };
  }

  /** Whole USD -> integer RUB using the Rapira USDT/RUB rate plus the configured margin, rounded up. */
  private async usdToRub(amountUsd: string): Promise<number> {
    const rate = await this.rapiraUsdRubRate();
    const usd = Number(amountUsd);
    const rub = Math.ceil(usd * rate * (1 + this.fxMarginBps / 10_000));
    if (!Number.isFinite(rub) || rub <= 0) {
      throw new PlategaError("Platega computed a non-positive RUB amount", false);
    }
    return rub;
  }

  private async rapiraUsdRubRate(): Promise<number> {
    let response: Awaited<ReturnType<typeof globalThis.fetch>>;
    try {
      response = await this.fetchImpl(this.rateUrl, { headers: { accept: "application/json" } });
    } catch {
      throw new PlategaError("Rapira rate request failed", true);
    }
    if (!response.ok) {
      throw new PlategaError(`Rapira returned HTTP ${response.status}`, response.status >= 500 || response.status === 429);
    }
    let json: unknown;
    try {
      json = await response.json();
    } catch {
      throw new PlategaError("Rapira returned invalid JSON", true);
    }
    const parsed = rapiraRatesSchema.safeParse(json);
    if (!parsed.success) throw new PlategaError("Rapira returned an invalid rates payload", true);
    const row = parsed.data.data.find((entry) => entry.symbol === "USDT/RUB");
    const rate = row?.askPrice ?? row?.close;
    if (rate === undefined || rate <= 0) throw new PlategaError("Rapira USDT/RUB rate is unavailable", true);
    return rate;
  }

  private async request<T extends z.ZodTypeAny>(
    method: "GET" | "POST",
    path: string,
    body: Record<string, unknown> | undefined,
    schema: T,
  ): Promise<z.infer<T>> {
    const init: RequestInit = {
      method,
      headers: {
        accept: "application/json",
        ...(body ? { "content-type": "application/json" } : {}),
        "X-MerchantId": this.options.merchantId,
        "X-Secret": this.options.secret,
      },
    };
    if (body) init.body = JSON.stringify(body);
    let response: Awaited<ReturnType<typeof globalThis.fetch>>;
    try {
      response = await this.fetchImpl(`${this.apiBaseUrl}${path}`, init);
    } catch {
      throw new PlategaError("Platega request failed", true);
    }
    if (!response.ok) {
      throw new PlategaError(`Platega returned HTTP ${response.status}`, response.status >= 500 || response.status === 429);
    }
    let json: unknown;
    try {
      json = await response.json();
    } catch {
      throw new PlategaError("Platega returned invalid JSON", true);
    }
    const parsed = schema.safeParse(json);
    if (!parsed.success) throw new PlategaError("Platega returned an invalid response", true);
    return parsed.data;
  }
}

// Platega statuses: PENDING, CONFIRMED (paid), CANCELED (failed), CHARGEBACKED (refunded).
function paymentState(status: string): ProviderPaymentState {
  switch (status.toUpperCase()) {
    case "CONFIRMED":
      return "paid";
    case "CANCELED":
    case "CANCELLED":
      return "canceled";
    case "CHARGEBACKED":
      return "refunded";
    case "PENDING":
    default:
      return "pending";
  }
}

// expiresIn is an "HH:MM:SS" duration from the moment of creation.
function expiresAtFrom(expiresIn: string | undefined): string | null {
  if (!expiresIn) return null;
  const match = /^(\d{1,2}):(\d{2}):(\d{2})$/.exec(expiresIn);
  if (!match) return null;
  const seconds = Number(match[1]) * 3600 + Number(match[2]) * 60 + Number(match[3]);
  if (seconds <= 0) return null;
  return new Date(Date.now() + seconds * 1000).toISOString();
}
