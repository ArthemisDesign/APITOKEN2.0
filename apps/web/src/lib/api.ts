export const API_BASE_URL = (process.env.NEXT_PUBLIC_BACKEND_URL ?? "https://backend.apitoken.sale/v1")
  .replace(/\/$/, "");

export interface AuthUser {
  id: string;
  email: string;
  displayName: string;
  emailVerified: boolean;
  passwordEnabled: boolean;
  engineAccountStatus: "pending" | "active" | "error" | "disabled";
  customerType: "b2c" | "b2b";
  totpEnabled: boolean;
}

export interface TotpSetup {
  otpauthUri: string;
  secret: string;
  qrDataUrl: string;
}

export interface ApiKeyView {
  id: string;
  label: string | null;
  keyMasked: string;
  status: "active" | "disabled";
  spentNano: string;
  spentUsd: string;
  createdAt: string;
  key?: string;
}

export interface LedgerEntry {
  id: string;
  kind: "topup" | "charge" | "adjust";
  amountNano: string;
  amountUsd: string;
  keyMasked: string | null;
  reference: string | null;
  // Claude-модель за списанием, когда движок помечает ею запрос (иначе выводим из reference).
  model?: string | null;
  balanceAfterNano: string | null;
  timestamp: string;
}

export interface B2CPricing {
  customerType: "b2c";
  pricingMode: "progressive";
  monthStart: string;
  tier: string;
  discountPercent: number;
  multiplierBp: number;
  // Фиксированная партнёрская скидка (реф-ссылка сейлза). 0 = нет. Если > 0 — реальная ставка/скидка
  // берутся из effective* (пол переопределяет тир), и дашборд показывает «партнёрскую ставку».
  referralFloorBps?: number;
  effectiveMultiplierBp?: number;
  effectiveDiscountPercent?: number;
  spentNano: string;
  retentionSpendNano: string;
  windowSpentNano?: string;
  windowStart?: string | null;
  nextTier: null | {
    tier: string;
    discountPercent: number;
    spendThresholdNano: string;
    remainingNano: string;
    visibleOfficialUsageUsd: string;
  };
}

export interface B2BPricing {
  customerType: "b2b";
  pricingMode: "manual";
  discountPercent: number;
  multiplierBp: number;
}

export interface AccountView {
  balanceNano: string;
  reservedNano: string;
  spentNano: string;
  balanceUsd: string;
  markupBasisPoints: number;
  status: "active" | "disabled";
  pricing: B2CPricing | B2BPricing | null;
}

// Полная разбивка расхода по токенам и моделям (движок считает всё это на каждом запросе).
// Токены — number (безопасно < 2^53); деньги — nano-строки (bigint-safe). officialNano — по
// официальным ценам Anthropic (×1.0), chargedNano — списано с баланса после скидки.
export interface UsageBucket {
  tokens: number;
  officialNano: string;
}
export interface UsageWebSearchBucket {
  requests: number;
  officialNano: string;
}
export interface UsageModelRow {
  model: string;
  requests: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWrite5mTokens: number;
  cacheWrite1hTokens: number;
  webSearchRequests: number;
  officialNano: string;
  chargedNano: string;
}
export interface UsageView {
  window: string;
  requests: number;
  totalOfficialNano: string;
  totalChargedNano: string;
  buckets: {
    input: UsageBucket;
    output: UsageBucket;
    cacheRead: UsageBucket;
    cacheWrite: UsageBucket;
    webSearch: UsageWebSearchBucket;
  };
  models: UsageModelRow[];
}

export interface CheckoutView {
  id: string;
  provider: string;
  amountUsd: string;
  status: "creating" | "pending" | "paid" | "canceled" | "refunded" | "failed";
  checkoutUrl: string | null;
  expiresAt: string | null;
}

export interface ProviderStatus {
  email: { password: boolean; verificationRequired: boolean };
  google: { configured: boolean; enabled: boolean };
  github: { configured: boolean; enabled: boolean; emailScope: string };
}

export class ApiError extends Error {
  constructor(message: string, readonly status: number, readonly details?: unknown) {
    super(message);
    this.name = "ApiError";
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  if (init.body !== undefined && !headers.has("content-type")) headers.set("content-type", "application/json");
  const response = await fetch(`${API_BASE_URL}${path}`, {
    ...init,
    headers,
    credentials: "include",
    cache: "no-store",
  });
  if (!response.ok) {
    const details = await response.json().catch(() => null) as { message?: string | string[] } | null;
    const supplied = Array.isArray(details?.message) ? details.message.join(". ") : details?.message;
    throw new ApiError(supplied || defaultError(response.status), response.status, details);
  }
  if (response.status === 204) return undefined as T;
  return await response.json() as T;
}

function defaultError(status: number): string {
  if (status === 401) return "Authentication required";
  if (status === 403) return "This request is not allowed";
  if (status === 429) return "Too many attempts. Please try again later";
  if (status >= 500) return "The service is temporarily unavailable";
  return "The request could not be completed";
}

export const api = {
  providers: () => request<ProviderStatus>("/auth/providers"),
  me: () => request<{ user: AuthUser }>("/auth/me"),
  updateProfile: (displayName: string) => request<{ user: AuthUser }>("/auth/me", {
    method: "PATCH", body: JSON.stringify({ displayName }),
  }),
  register: (input: { email: string; password: string; inviteToken?: string; referralCode?: string }) =>
    request<{ user: AuthUser; verificationRequired: boolean }>("/auth/register", {
      method: "POST", body: JSON.stringify(input),
    }),
  login: (input: { email: string; password: string }) =>
    request<{ user: AuthUser }>("/auth/login", { method: "POST", body: JSON.stringify(input) }),
  verifyEmail: (token: string) =>
    request<{ user: AuthUser }>("/auth/email/verify", { method: "POST", body: JSON.stringify({ token }) }),
  resendVerification: (email: string) =>
    request<{ accepted: boolean }>("/auth/email/resend", { method: "POST", body: JSON.stringify({ email }) }),
  forgotPassword: (email: string) =>
    request<{ accepted: boolean }>("/auth/password/forgot", { method: "POST", body: JSON.stringify({ email }) }),
  resetPassword: (token: string, password: string) =>
    request<void>("/auth/password/reset", { method: "POST", body: JSON.stringify({ token, password }) }),
  logout: () => request<void>("/auth/logout", { method: "POST" }),
  redeemPromo: (code: string) =>
    request<{ credited_usd: string; credited_nano: string; balance?: string; balance_nano?: string }>(
      "/promo/redeem", { method: "POST", body: JSON.stringify({ code }) },
    ),
  account: () => request<AccountView>("/account"),
  ledger: (limit = 50) => request<{ entries: LedgerEntry[] }>(`/account/ledger?limit=${limit}`),
  usage: (window = "30d") => request<UsageView>(`/account/usage?window=${encodeURIComponent(window)}`),
  apiKeys: () => request<{ keys: ApiKeyView[] }>("/api-keys"),
  createApiKey: (label?: string, totpCode?: string) => request<ApiKeyView>("/api-keys", {
    method: "POST", body: JSON.stringify({ ...(label ? { label } : {}), ...(totpCode ? { totpCode } : {}) }),
  }),
  totpSetup: () => request<TotpSetup>("/security/totp/setup", { method: "POST" }),
  totpEnable: (code: string) => request<void>("/security/totp/enable", { method: "POST", body: JSON.stringify({ code }) }),
  totpDisable: (code: string) => request<void>("/security/totp/disable", { method: "POST", body: JSON.stringify({ code }) }),
  renameApiKey: (id: string, label: string) => request<ApiKeyView>(`/api-keys/${encodeURIComponent(id)}`, {
    method: "PATCH", body: JSON.stringify({ label }),
  }),
  revokeApiKey: (id: string) => request<void>(`/api-keys/${encodeURIComponent(id)}`, { method: "DELETE" }),
  createCheckout: (amountUsd: string, paymentMethod?: number) => request<CheckoutView>("/checkouts", {
    method: "POST",
    body: JSON.stringify({ amountUsd, provider: "platega", ...(paymentMethod ? { paymentMethod } : {}) }),
  }),
  checkout: (id: string) => request<CheckoutView>(`/checkouts/${encodeURIComponent(id)}`),
};

export function oauthUrl(provider: "google" | "github", inviteToken?: string, referralCode?: string): string {
  const url = new URL(`${API_BASE_URL}/auth/${provider}`);
  if (inviteToken) url.searchParams.set("invite", inviteToken);
  // Партнёрский ?ref= пробрасываем в OAuth: реф партнёра станет B2B до welcome-бонуса.
  if (referralCode) url.searchParams.set("ref", referralCode);
  return url.toString();
}
