import { createHash, createHmac, timingSafeEqual } from "node:crypto";
import { z } from "zod";
import type {
  CheckoutCreation,
  CheckoutContext,
  PaymentProviderAdapter,
  ProviderPaymentState,
  VerifiedProviderPayment,
} from "./provider.js";

const loginResponseSchema = z.object({
  retval: z.number().int(),
  desc: z.string().nullish(),
  token: z.string().min(1).optional(),
  valid_thru: z.string().optional(),
});

const purchaseResponseSchema = z.object({
  retval: z.number().int(),
  retdesc: z.string().nullish(),
  content: z.object({
    item_id: z.union([z.number().int(), z.string()]),
    amount: z.union([z.number(), z.string()]),
    amount_usd: z.union([z.number(), z.string()]).nullish(),
    currency_type: z.string(),
    invoice_state: z.number().int(),
    date_pay: z.string().nullish(),
    query_string: z.string().nullish(),
    buyer_info: z.object({ email: z.string().nullish() }).nullish(),
  }).nullish(),
});

const uniqueCodeResponseSchema = z.object({
  retval: z.number().int(),
  retdesc: z.string().nullish(),
  inv: z.union([z.number().int(), z.string()]).optional(),
  id_goods: z.union([z.number().int(), z.string()]).optional(),
  amount: z.union([z.number(), z.string()]).optional(),
  amount_usd: z.union([z.number(), z.string()]).nullish(),
  type_curr: z.string().optional(),
  date_pay: z.string().nullish(),
  email: z.string().nullish(),
  query_string: z.string().nullish(),
});

export interface DigiSellerOptions {
  sellerId: number;
  apiKey: string;
  productId: number;
  checkoutTrackingSecret: string;
  apiBaseUrl?: string;
  checkoutUrl?: string;
  fetch?: typeof globalThis.fetch;
  now?: () => Date;
}

interface CachedToken {
  value: string;
  refreshAt: number;
}

export class DigiSellerError extends Error {
  constructor(message: string, readonly retryable: boolean) {
    super(message);
    this.name = "DigiSellerError";
  }
}

export class DigiSellerProvider implements PaymentProviderAdapter {
  readonly code = "digiseller";
  private readonly apiBaseUrl: string;
  private readonly checkoutUrl: string;
  private readonly fetchImpl: typeof globalThis.fetch;
  private readonly now: () => Date;
  private token: CachedToken | undefined;
  private tokenRefresh: Promise<string> | undefined;
  private lastTimestamp = 0;

  constructor(private readonly options: DigiSellerOptions) {
    this.apiBaseUrl = (options.apiBaseUrl ?? "https://api.digiseller.com").replace(/\/$/, "");
    this.checkoutUrl = options.checkoutUrl ?? "https://oplata.info/asp2/pay.asp";
    this.fetchImpl = options.fetch ?? globalThis.fetch;
    this.now = options.now ?? (() => new Date());
  }

  async createCheckout(context: CheckoutContext): Promise<CheckoutCreation> {
    const trackingSignature = this.signCheckout(context.checkoutId);
    const checkoutUrl = new URL(this.checkoutUrl);
    // DigiSeller's automatic unique-code redirect preserves GET parameters from the payment URL.
    checkoutUrl.searchParams.set("checkout_id", context.checkoutId);
    checkoutUrl.searchParams.set("checkout_sig", trackingSignature);
    return {
      action: {
        kind: "form_post",
        url: checkoutUrl.toString(),
        fields: {
          id_d: this.options.productId.toString(),
          typecurr: context.currency,
          email: context.customerEmail,
          lang: context.locale,
          failpage: context.cancelUrl,
        },
      },
      providerPaymentId: null,
      expiresAt: null,
      raw: null,
    };
  }

  /** Primary completion flow for DigiSeller's automatic unique-code redirect. */
  async verifyUniqueCode(uniqueCode: string): Promise<VerifiedProviderPayment> {
    if (!/^\d{16}$/.test(uniqueCode)) throw new DigiSellerError("DigiSeller unique code must contain 16 digits", false);
    const token = await this.getToken();
    const url = `${this.apiBaseUrl}/api/purchases/unique-code/${encodeURIComponent(uniqueCode)}?token=${encodeURIComponent(token)}`;
    const response = await this.fetchImpl(url, { headers: { accept: "application/json" } });
    if (!response.ok) {
      if (response.status === 401) this.token = undefined;
      throw new DigiSellerError(`DigiSeller unique-code lookup returned HTTP ${response.status}`, response.status >= 500 || response.status === 401 || response.status === 429);
    }
    const parsed = uniqueCodeResponseSchema.safeParse(await response.json());
    if (!parsed.success) throw new DigiSellerError("DigiSeller returned an invalid unique-code response", true);
    const result = parsed.data;
    if (result.retval !== 0 || result.inv === undefined || result.id_goods === undefined || result.amount === undefined || !result.type_curr) {
      throw new DigiSellerError(result.retdesc ?? `DigiSeller unique-code lookup failed (${result.retval})`, false);
    }
    const providerPaymentId = String(result.inv);
    return {
      provider: this.code,
      providerPaymentId,
      providerEventId: `${providerPaymentId}:3`,
      state: "paid",
      providerProductId: String(result.id_goods),
      checkoutId: this.decodeTracking(result.query_string),
      paidAt: result.date_pay ?? null,
      buyerEmail: result.email ?? null,
      providerAmount: decimalString(result.amount),
      providerCurrency: result.type_curr,
      amountUsd: result.amount_usd == null ? null : decimalString(result.amount_usd),
      raw: result,
    };
  }

  async verifyPayment(providerPaymentId: string): Promise<VerifiedProviderPayment> {
    if (!/^\d+$/.test(providerPaymentId)) throw new DigiSellerError("DigiSeller invoice ID must be numeric", false);
    const token = await this.getToken();
    const url = `${this.apiBaseUrl}/api/purchase/info/${encodeURIComponent(providerPaymentId)}?token=${encodeURIComponent(token)}`;
    const response = await this.fetchImpl(url, { headers: { accept: "application/json" } });
    if (!response.ok) {
      if (response.status === 401) this.token = undefined;
      throw new DigiSellerError(`DigiSeller purchase lookup returned HTTP ${response.status}`, response.status >= 500 || response.status === 401 || response.status === 429);
    }

    const parsed = purchaseResponseSchema.safeParse(await response.json());
    if (!parsed.success) throw new DigiSellerError("DigiSeller returned an invalid purchase response", true);
    if (parsed.data.retval !== 0 || !parsed.data.content) {
      throw new DigiSellerError(parsed.data.retdesc ?? `DigiSeller purchase lookup failed (${parsed.data.retval})`, false);
    }
    const purchase = parsed.data.content;
    const tracking = this.decodeTracking(purchase.query_string);

    return {
      provider: this.code,
      providerPaymentId,
      providerEventId: `${providerPaymentId}:${purchase.invoice_state}`,
      state: invoiceState(purchase.invoice_state),
      providerProductId: String(purchase.item_id),
      checkoutId: tracking,
      paidAt: purchase.date_pay ?? null,
      buyerEmail: purchase.buyer_info?.email ?? null,
      providerAmount: decimalString(purchase.amount),
      providerCurrency: purchase.currency_type,
      amountUsd: purchase.amount_usd == null ? null : decimalString(purchase.amount_usd),
      raw: parsed.data,
    };
  }

  private async getToken(): Promise<string> {
    const now = this.now().getTime();
    if (this.token && this.token.refreshAt > now) return this.token.value;
    if (this.tokenRefresh) return this.tokenRefresh;
    this.tokenRefresh = this.login().finally(() => { this.tokenRefresh = undefined; });
    return this.tokenRefresh;
  }

  private async login(): Promise<string> {
    const unixSeconds = Math.floor(this.now().getTime() / 1000);
    const timestamp = Math.max(unixSeconds, this.lastTimestamp + 1);
    this.lastTimestamp = timestamp;
    const sign = createHash("sha256").update(`${this.options.apiKey}${timestamp}`, "utf8").digest("hex");
    const response = await this.fetchImpl(`${this.apiBaseUrl}/api/apilogin`, {
      method: "POST",
      headers: { accept: "application/json", "content-type": "application/json" },
      body: JSON.stringify({ seller_id: this.options.sellerId, timestamp, sign }),
    });
    if (!response.ok) throw new DigiSellerError(`DigiSeller login returned HTTP ${response.status}`, true);
    const parsed = loginResponseSchema.safeParse(await response.json());
    if (!parsed.success || parsed.data.retval !== 0 || !parsed.data.token) {
      throw new DigiSellerError(parsed.success ? parsed.data.desc ?? "DigiSeller login failed" : "DigiSeller returned an invalid login response", true);
    }
    const expiry = parsed.data.valid_thru ? Date.parse(parsed.data.valid_thru) : this.now().getTime() + 2 * 60 * 60 * 1000;
    this.token = { value: parsed.data.token, refreshAt: Math.max(this.now().getTime(), expiry - 60_000) };
    return parsed.data.token;
  }

  private signCheckout(checkoutId: string): string {
    return createHmac("sha256", this.options.checkoutTrackingSecret).update(checkoutId, "utf8").digest("hex");
  }

  private decodeTracking(encoded: string | null | undefined): string | null {
    if (!encoded) return null;
    try {
      const parameters = new URLSearchParams(Buffer.from(encoded, "base64").toString("utf8"));
      const checkoutId = parameters.get("checkout_id");
      const supplied = parameters.get("checkout_sig");
      if (!checkoutId || !supplied) return null;
      const expected = this.signCheckout(checkoutId);
      const suppliedBytes = Buffer.from(supplied, "hex");
      const expectedBytes = Buffer.from(expected, "hex");
      if (suppliedBytes.length !== expectedBytes.length || !timingSafeEqual(suppliedBytes, expectedBytes)) return null;
      return checkoutId;
    } catch {
      return null;
    }
  }
}

function invoiceState(value: number): ProviderPaymentState {
  switch (value) {
    case 3: return "paid";
    case 2:
    case 4: return "canceled";
    case 5:
    case 35: return "refunded";
    default: return "pending";
  }
}

function decimalString(value: number | string): string {
  if (typeof value === "string") return value;
  if (!Number.isFinite(value)) throw new DigiSellerError("DigiSeller returned a non-finite amount", false);
  return value.toString();
}
