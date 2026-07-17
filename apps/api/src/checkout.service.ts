import { Inject, Injectable } from "@nestjs/common";
import { timingSafeEqual } from "node:crypto";
import { ConfigService } from "@nestjs/config";
import type { CheckoutView } from "@claude-api/contracts";
import {
  activateCheckoutSession,
  applyVerifiedCheckoutPaymentEvent,
  createCheckoutSession,
  failCheckoutSession,
  getCheckoutByProviderPayment,
  getCheckoutSession,
  type AppliedPaymentEvent,
  type CheckoutSession,
  type Database,
} from "@claude-api/db";
import { PaymentProviderRegistry } from "@claude-api/payments";
import type { Environment } from "./config.js";
import { DATABASE } from "./infrastructure.module.js";

export type PaymentProviderCode = "cryptomus" | "platega";

@Injectable()
export class CheckoutService {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    private readonly providers: PaymentProviderRegistry,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async create(userId: string, amountInput: string, providerCode: PaymentProviderCode, paymentMethod?: number): Promise<CheckoutView> {
    const amountUsd = BigInt(amountInput);
    const minimum = this.config.get("MIN_TOPUP_USD", { infer: true });
    const maximum = this.config.get("MAX_TOPUP_USD", { infer: true });
    if (amountUsd < minimum || amountUsd > maximum) {
      throw new CheckoutAmountError(`amountUsd must be between ${minimum} and ${maximum} whole USD`);
    }
    const provider = this.providers.get(providerCode);
    const session = await createCheckoutSession(this.database, { userId, provider: providerCode, amountUsd });
    try {
      const appBaseUrl = this.config.get("PUBLIC_APP_BASE_URL", { infer: true });
      const returnUrl = new URL("/dashboard", appBaseUrl);
      returnUrl.searchParams.set("view", "credits");
      returnUrl.searchParams.set("checkoutId", session.id);
      returnUrl.searchParams.set("paymentReturn", "success");
      const cancelUrl = new URL(returnUrl);
      cancelUrl.searchParams.set("paymentReturn", "cancel");
      const creation = await provider.createCheckout({
        checkoutId: session.id,
        amount: amountInput,
        customerEmail: session.customerEmail,
        locale: "en-US",
        currency: "USD",
        returnUrl: returnUrl.toString(),
        cancelUrl: cancelUrl.toString(),
        ...(paymentMethod !== undefined ? { paymentMethod } : {}),
      });
      if (!creation.providerPaymentId) throw new Error("provider did not return a payment ID");
      if (creation.action.kind !== "redirect") throw new Error("provider did not return a redirect checkout");
      const activated = await activateCheckoutSession(this.database, {
        id: session.id,
        providerPaymentId: creation.providerPaymentId,
        checkoutUrl: creation.action.url,
        expiresAt: creation.expiresAt ? new Date(creation.expiresAt) : null,
        providerState: creation.raw,
      });
      return checkoutView(activated);
    } catch (error) {
      await failCheckoutSession(this.database, session.id, errorMessage(error));
      throw error;
    }
  }

  async get(userId: string, checkoutId: string): Promise<CheckoutView | null> {
    const session = await getCheckoutSession(this.database, { id: checkoutId, userId });
    return session ? checkoutView(session) : null;
  }

  async processCryptomusWebhook(rawBody: string | Uint8Array): Promise<AppliedPaymentEvent> {
    const provider = this.providers.getWebhook("cryptomus");
    const signal = provider.verifyWebhook(rawBody);
    const payment = await provider.verifyPayment(signal.providerPaymentId);
    if (!payment.checkoutId) throw new Error("verified Cryptomus payment has no checkout ID");
    if (payment.providerPaymentId !== signal.providerPaymentId) {
      throw new Error("Cryptomus webhook and payment lookup IDs differ");
    }
    if (payment.providerCurrency.toUpperCase() !== "USD") {
      throw new Error("Cryptomus checkout currency is not USD");
    }
    const amountUsd = parseProviderWholeUsd(payment.providerAmount);
    return applyVerifiedCheckoutPaymentEvent(this.database, {
      provider: payment.provider,
      providerEventId: payment.providerEventId,
      providerPaymentId: payment.providerPaymentId,
      checkoutId: payment.checkoutId,
      state: payment.state,
      amountUsd,
      currency: "USD",
      paidAt: payment.paidAt ? parseProviderDate(payment.paidAt) : null,
      payload: payment.raw,
    });
  }

  async processPlategaWebhook(
    rawBody: string | Uint8Array,
    headers: { secret: string | undefined; merchantId: string | undefined },
  ): Promise<AppliedPaymentEvent> {
    const secret = this.config.get("PLATEGA_SECRET", { infer: true });
    const merchantId = this.config.get("PLATEGA_MERCHANT_ID", { infer: true });
    if (!secret || !merchantId) throw new Error("unsupported payment provider: platega");
    // Platega authenticates callbacks only with these headers (no HMAC); reject before doing any work.
    if (!constantTimeEquals(headers.secret, secret) || !constantTimeEquals(headers.merchantId, merchantId)) {
      throw new PlategaWebhookAuthError("invalid Platega webhook credentials");
    }
    const provider = this.providers.getWebhook("platega");
    const signal = provider.verifyWebhook(rawBody);
    // The callback is only a wake-up; re-query Platega's authoritative status before crediting.
    const payment = await provider.verifyPayment(signal.providerPaymentId);
    if (!payment.checkoutId) throw new Error("verified Platega payment has no checkout ID");
    if (payment.providerPaymentId !== signal.providerPaymentId) {
      throw new Error("Platega webhook and payment lookup IDs differ");
    }
    // Local checkout is authoritative for the amount; credit the recorded USD, never the RUB collected.
    const checkout = await getCheckoutByProviderPayment(this.database, {
      provider: "platega",
      providerPaymentId: payment.providerPaymentId,
    });
    if (!checkout) throw new Error("verified Platega payment does not match a checkout session");
    if (checkout.id !== payment.checkoutId) throw new Error("Platega payload checkout ID does not match");
    return applyVerifiedCheckoutPaymentEvent(this.database, {
      provider: payment.provider,
      providerEventId: payment.providerEventId,
      providerPaymentId: payment.providerPaymentId,
      checkoutId: checkout.id,
      state: payment.state,
      amountUsd: checkout.amountUsd,
      currency: "USD",
      paidAt: payment.paidAt ? parseProviderDate(payment.paidAt) : null,
      payload: payment.raw,
    });
  }
}

export class CheckoutAmountError extends Error {}
export class PlategaWebhookAuthError extends Error {}

function constantTimeEquals(supplied: string | undefined, expected: string): boolean {
  if (typeof supplied !== "string") return false;
  const suppliedBytes = Buffer.from(supplied, "utf8");
  const expectedBytes = Buffer.from(expected, "utf8");
  return suppliedBytes.length === expectedBytes.length && timingSafeEqual(suppliedBytes, expectedBytes);
}

export function parseProviderWholeUsd(value: string): bigint {
  const match = /^([1-9]\d*)(?:\.0+)?$/.exec(value);
  if (!match?.[1]) throw new Error("provider amount is not positive whole USD");
  return BigInt(match[1]);
}

function parseProviderDate(value: string): Date {
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) throw new Error("provider paid timestamp is invalid");
  return parsed;
}

function checkoutView(session: CheckoutSession): CheckoutView {
  return {
    id: session.id,
    provider: session.provider,
    amountUsd: session.amountUsd.toString(),
    status: session.status,
    checkoutUrl: session.checkoutUrl,
    expiresAt: session.expiresAt?.toISOString() ?? null,
  };
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "unknown checkout creation error";
}
