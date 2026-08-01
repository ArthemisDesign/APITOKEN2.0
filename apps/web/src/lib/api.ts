export const API_BASE_URL = (process.env.NEXT_PUBLIC_BACKEND_URL ?? "https://backend.apitoken.sale/v1")
  .replace(/\/$/, "");

// web/v2 previews run as an already authenticated dashboard against browser-local
// stateful fixtures. This branch is statically disabled in production builds.
const PREVIEW_FIXTURES = process.env.NEXT_PUBLIC_PREVIEW_FIXTURES === "1";

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
  reservedNano?: string;
  spendLimitNano: string | null;
  expiresAt: string | null;
  lastUsedAt: string | null;
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
  // Модель за списанием (claude-* или gpt-*), когда движок помечает ею запрос (иначе выводим из reference).
  model?: string | null;
  requestId?: string | null;
  provider?: string | null;
  officialNano?: string | null;
  attribution?: LedgerAttribution | null;
  fundingAllocations?: LedgerFundingAllocation[];
  balanceAfterNano: string | null;
  timestamp: string;
}

export interface LedgerFundingAllocation {
  bucketId: string;
  sourceType: string;
  sourceReference: string;
  bucketVersion: string;
  direction: "debit" | "credit";
  amountNano: string;
  allocationOrder: string | null;
}

export interface LedgerAttribution {
  schemaVersion: string;
  snapshotKind: "policy_v1" | "legacy_scalar" | null;
  providerId: string | null;
  productId: string | null;
  accountClass: "b2c" | "b2b" | "openkeys" | "service" | null;
  requestedModelId: string | null;
  canonicalModelId: string | null;
  servedModelId: string | null;
  servedCanonicalModelId: string | null;
  ruleId: string | null;
  ruleScope: "provider" | "model" | null;
  pricingMode: "track" | "discount" | "legacy_scalar" | null;
  ruleOrigin: "managed" | "legacy" | null;
  discountBps: number | null;
  payableMultiplierBp: number | null;
  policyVersion: string | null;
  effectivePolicyVersion: string | null;
  catalogGeneration: string | null;
  switchGeneration: string | null;
  tariffScheduleId: string | null;
  tariffPricedTimestamp: string | null;
  officialNano: string | null;
  officialCost: Record<string, unknown> | null;
  paidFundedNano: string | null;
  bonusFundedNano: string | null;
  otherFundedNano: string | null;
  trackEligible: boolean | null;
  retentionEligible: boolean | null;
  commissionEligible: boolean | null;
}

export interface B2CPricing {
  customerType: "b2c";
  pricingMode: "progressive";
  discountPercent: number;
  multiplierBp: number;
  // Фиксированная партнёрская скидка (реф-ссылка сейлза). 0 = нет. Если > 0 — реальная ставка/скидка
  // берутся из effective* (поле переопределяет плоскую ставку), и дашборд показывает «партнёрскую ставку».
  referralFloorBps?: number;
  effectiveMultiplierBp?: number;
  effectiveDiscountPercent?: number;
  tier?: string;
  spentNano?: string;
  retentionSpendNano?: string;
  windowSpentNano?: string;
  windowStart?: string | null;
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
  funding?: AccountFundingView | null;
  pricing: B2CPricing | B2BPricing | null;
  pricingPolicies?: CustomerPricingPolicyView[];
}

export interface AccountFundingView {
  accountClass: "b2c" | "b2b" | "openkeys" | "service" | null;
  fundingEnforcement: "legacy_single" | "shadow" | "strict" | null;
  reconciliationState: "pending" | "verified" | "exception" | null;
  bucketCount: string;
  balances: FundingAmounts;
  reserved: FundingAmounts;
  spent: FundingAmounts;
}

export interface FundingAmounts {
  paidNano: string;
  bonusNano: string;
  otherNano: string;
  unattributedNano: string;
}

export interface CustomerPricingRuleView {
  ruleId: string;
  scope: "provider" | "model";
  pricingMode: "track" | "discount";
  ruleOrigin: "managed" | "legacy";
  discountBps: number | null;
  payableMultiplierBp: number;
  trackEligible: boolean;
  retentionEligible: boolean;
  commissionEligible: boolean;
}

export interface CustomerPricingModelView {
  modelId: string;
  available: boolean;
  unavailableReasons: string[];
  rule: CustomerPricingRuleView | null;
}

export interface CustomerPricingProviderView {
  providerId: string;
  available: boolean;
  models: CustomerPricingModelView[];
}

export interface CustomerPricingVersionView {
  effectiveVersion: string;
  policyVersion: string;
  catalogGeneration: string;
  switchGeneration: string;
  providers: CustomerPricingProviderView[];
}

export interface CustomerPricingPolicyView {
  accountClass: "b2c" | "b2b";
  productId: string;
  policyEnforcement: "legacy_scalar" | "shadow" | "strict";
  fundingEnforcement: "legacy_single" | "shadow" | "strict";
  reconciliationState: "pending" | "verified" | "exception";
  syncState: "legacy" | "pending" | "confirmed" | "failed";
  inSync: boolean;
  lastAcknowledgedAt: string | null;
  desired: CustomerPricingVersionView | null;
  applied: CustomerPricingVersionView | null;
}

// Полная разбивка расхода по токенам и моделям (движок считает всё это на каждом запросе).
// Токены — number (безопасно < 2^53); деньги — nano-строки (bigint-safe). officialNano — по
// официальным ставкам фактически обслужившего провайдера, chargedNano — списано с баланса после
// правила. Provider приходит из immutable engine attribution и никогда не выводится из model ID.
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
  provider?: string | null;
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
export interface UsageDailyRow {
  dayTs: number;
  requests: number;
  officialNano: string;
  chargedNano: string;
}
export interface UsageDailyProviderRow extends UsageDailyRow {
  provider: string;
}
export interface UsageKeyRow {
  keyMasked: string | null;
  requests: number;
  officialNano: string;
  chargedNano: string;
}
export interface UsageView {
  window: string;
  sinceTs: number;
  untilTs: number;
  requests: number;
  totalOfficialNano: string;
  totalChargedNano: string;
  buckets: {
    input: UsageBucket;
    output: UsageBucket;
    cacheRead: UsageBucket;
    cacheWrite: UsageBucket;
    webSearch: UsageWebSearchBucket;
    unattributedLegacy: { officialNano: string };
  };
  models: UsageModelRow[];
  daily: UsageDailyRow[];
  dailyProviders?: UsageDailyProviderRow[];
  keys: UsageKeyRow[];
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
  if (PREVIEW_FIXTURES) {
    const { previewRequest } = await import("./preview-fixtures");
    return previewRequest<T>(path, init);
  }
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
  businessInvitePreview: (token: string) =>
    request<{
      valid: boolean;
      emailBound?: boolean;
      maskedEmail?: string | null;
      email?: string | null;
      discountPercent?: number;
      expiresAt?: string;
    }>("/auth/business-invite/preview", {
      method: "POST", body: JSON.stringify({ token }),
    }),
  me: (signal?: AbortSignal) => request<{ user: AuthUser }>("/auth/me", { signal }),
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
  createApiKey: (input: { label?: string; spendLimitUsd?: string; expiresAt?: string; totpCode?: string }) => request<ApiKeyView>("/api-keys", {
    method: "POST", body: JSON.stringify(input),
  }),
  totpSetup: () => request<TotpSetup>("/security/totp/setup", { method: "POST" }),
  totpEnable: (code: string) => request<void>("/security/totp/enable", { method: "POST", body: JSON.stringify({ code }) }),
  totpDisable: (code: string) => request<void>("/security/totp/disable", { method: "POST", body: JSON.stringify({ code }) }),
  renameApiKey: (id: string, label: string) => request<ApiKeyView>(`/api-keys/${encodeURIComponent(id)}`, {
    method: "PATCH", body: JSON.stringify({ label }),
  }),
  updateApiKeyPolicy: (
    id: string,
    input: { spendLimitUsd: string | null; expiresAt: string | null; totpCode?: string },
  ) => request<ApiKeyView>(`/api-keys/${encodeURIComponent(id)}/policy`, {
    method: "PATCH", body: JSON.stringify(input),
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
