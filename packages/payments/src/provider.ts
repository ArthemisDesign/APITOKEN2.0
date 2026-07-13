export type ProviderPaymentState = "pending" | "paid" | "canceled" | "refunded";

export interface CheckoutContext {
  checkoutId: string;
  productId: string;
  amount: string;
  customerEmail: string;
  locale: "en-US" | "ru-RU";
  currency: "USD" | "RUB" | "EUR";
  returnUrl: string;
  cancelUrl: string;
}

export interface VerifiedWebhookSignal {
  provider: string;
  providerPaymentId: string;
  providerEventId: string;
  raw: unknown;
}

export interface FormPostCheckout {
  kind: "form_post";
  url: string;
  fields: Readonly<Record<string, string>>;
}

export interface RedirectCheckout {
  kind: "redirect";
  url: string;
}

export type CheckoutAction = FormPostCheckout | RedirectCheckout;

export interface VerifiedProviderPayment {
  provider: string;
  providerPaymentId: string;
  providerEventId: string;
  state: ProviderPaymentState;
  productId: string;
  checkoutId: string | null;
  paidAt: string | null;
  buyerEmail: string | null;
  providerAmount: string;
  providerCurrency: string;
  amountUsd: string | null;
  raw: unknown;
}

/** Provider-specific transport and verification. It never decides how much engine credit to issue. */
export interface PaymentProviderAdapter {
  readonly code: string;
  createCheckout(context: CheckoutContext): Promise<CheckoutAction>;
  verifyPayment(providerPaymentId: string): Promise<VerifiedProviderPayment>;
}

/** A signed callback is only a wake-up signal; callers must still invoke verifyPayment(). */
export interface WebhookPaymentProviderAdapter extends PaymentProviderAdapter {
  verifyWebhook(rawBody: string | Uint8Array): VerifiedWebhookSignal;
}

export class PaymentProviderRegistry {
  private readonly providers = new Map<string, PaymentProviderAdapter>();

  constructor(adapters: readonly PaymentProviderAdapter[]) {
    for (const adapter of adapters) {
      if (this.providers.has(adapter.code)) throw new Error(`duplicate payment provider: ${adapter.code}`);
      this.providers.set(adapter.code, adapter);
    }
  }

  get(code: string): PaymentProviderAdapter {
    const provider = this.providers.get(code);
    if (!provider) throw new Error(`unsupported payment provider: ${code}`);
    return provider;
  }

  codes(): string[] {
    return [...this.providers.keys()].sort();
  }
}
