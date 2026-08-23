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
  balanceAfterNano: string | null;
  timestamp: string;
}

export interface B2CPricing {
  customerType: "b2c";
  // Flat B2C pricing: one global discount on the account, applied to every provider unless the
  // account carries a per-provider override.
  pricingMode: "flat";
  discountPercent: number;
  multiplierBp: number;
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
// официальным ставкам фактически обслужившего провайдера, chargedNano — списано с баланса после
// правила. Provider приходит из строки ledger движка и никогда не выводится из model ID.
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

export type ReferralRequestStatus = "pending" | "approved" | "rejected" | "applied" | "apply_failed";
export type ReferralRequestType = "commission_change" | "b2b_conversion" | "b2b_pricing";

export interface ReferralMembership {
  email: string;
  status: "active" | "suspended" | "pending";
  programEnabled: boolean;
  programStartedAt: string | null;
  referralCode: string;
  commissionBps: number;
  teamShareBps: number | null;
  teamOverrideMaxBps: number;
  teamInvitesEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  b2bCanDelegate: boolean;
  payoutMethod: string | null;
  payoutDetails: { network?: string; asset?: string; address?: string } | null;
  createdAt: string;
}

export interface ReferralTotals {
  earnedNano: string;
  directNano: string;
  overrideNano: string;
  adjustmentNano: string;
  directAdjustmentNano: string;
  overrideAdjustmentNano: string;
  netNano: string;
  directNetNano: string;
  overrideNetNano: string;
  paidNano: string;
  pendingPayoutNano: string;
  debtNano: string;
  availableNano: string;
  last30dSpendNano: string;
  last30dEarnedNano: string;
  last30dAdjustmentNano: string;
  last30dNetNano: string;
}

export interface ReferralCustomer {
  email: string | null;
  customerType: "b2c" | "b2b" | null;
  discountBps: number | null;
  providerDiscounts: Array<{ providerId: string; discountBps: number }>;
  attributedAt: string;
  spendNano: string;
  earnedNano: string;
  adjustmentNano: string;
  netNano: string;
  topupNano: string;
}

export interface ReferralTeamMember {
  email: string | null;
  programEnabled: boolean;
  programStartedAt: string | null;
  status: "active" | "suspended" | "pending";
  commissionBps: number;
  overrideBps: number;
  teamOverrideMaxBps: number;
  teamInvitesEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  b2bCanDelegate: boolean;
  referredUsers: number;
  theirEarnedNano: string;
  theirAdjustmentNano: string;
  theirNetNano: string;
  myOverrideNano: string;
  myOverrideAdjustmentNano: string;
  myOverrideNetNano: string;
}

export interface ReferralInvitation {
  id: string;
  email: string | null;
  overrideBps: number;
  teamOverrideMaxBps: number;
  teamInvitesEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  b2bCanDelegate: boolean;
  expiresAt: string;
  consumedAt: string | null;
  revokedAt: string | null;
  createdAt: string;
}

export interface ReferralRequest {
  id: string;
  requestType: ReferralRequestType;
  status: ReferralRequestStatus;
  requesterEmail: string | null;
  customerEmail: string | null;
  reason: string;
  stateSnapshot: Record<string, unknown>;
  requestedCommissionBps: number | null;
  requestedDiscountBps: number | null;
  approvedCommissionBps: number | null;
  approvedDiscountBps: number | null;
  reviewerActor: string | null;
  reviewerNote: string | null;
  reviewedAt: string | null;
  appliedAt: string | null;
  applyAttempts: number;
  lastApplyError: string | null;
  version: number;
  providerTerms: Array<{
    providerId: string;
    requestedDiscountBps: number | null;
    approvedDiscountBps: number | null;
    decided: boolean;
  }>;
  effect: {
    id: string;
    status: "pending" | "processing" | "applied" | "failed";
    attempts: number;
    nextAttemptAt: string | null;
    terminal: boolean;
    appliedAt: string | null;
    lastError: string | null;
  } | null;
  createdAt: string;
  updatedAt: string;
}

export interface ReferralPayout {
  id: string;
  amountNano: string;
  status: "requested" | "approved" | "paid" | "rejected";
  method: string;
  details: unknown;
  requestedAt: string;
  decidedAt: string | null;
  paidAt: string | null;
  adminNote: string | null;
  txHash: string | null;
  chainStatus: string | null;
}

export interface ReferralActiveSnapshot {
  state: "active";
  activated: boolean;
  membership: ReferralMembership;
  totals: ReferralTotals;
  referrals: ReferralCustomer[];
  team: ReferralTeamMember[];
  earnings: {
    days: number;
    daily: Array<{ date: string; spendNano: string; earnedNano: string; adjustmentNano: string; netNano: string }>;
    providers: Array<{ providerId: string | null; events: number; spendNano: string; earnedNano: string }>;
    providerDaily: Array<{
      date: string;
      providers: Array<{ providerId: string | null; events: number; spendNano: string; earnedNano: string }>;
    }>;
  };
  invitations: ReferralInvitation[];
  requests: ReferralRequest[];
  payouts: ReferralPayout[];
  period: {
    now: string;
    current: { key: string; start: string; end: string; accruedNano: string; adjustmentNano: string; netNano: string };
    locked: Array<{ key: string; endedAt: string; unlocksAt: string; earnedNano: string; adjustmentNano: string; netNano: string }>;
    nextPayout: { date: string; estimatedNano: string };
    lifetimeEarnedNano: string;
    lifetimeAdjustmentNano: string;
    lifetimeNetNano: string;
    lifetimePaidNano: string;
    debtNano: string;
    payableNano: string;
    unpaidNano: string;
  };
  periodHistory: Array<{
    key: string;
    index: 1 | 2;
    start: string;
    end: string;
    phase: "accruing" | "locked" | "payable" | "closed";
    payoutDate: string;
    earnedNano: string;
    adjustmentNano: string;
    netNano: string;
  }>;
  payoutPolicy: { minPayoutNano: string; lockDays: 7; windowDays: 3 };
}

export type ReferralSnapshot = ReferralActiveSnapshot
  | { state: "unavailable"; membership: null }
  | { state: "disabled"; membership: ReferralMembership };


export interface ReferralApplication {
  id: string;
  email: string;
  status: "pending" | "approved" | "rejected";
  message: string;
  reviewerNote: string | null;
  decidedAt: string | null;
  createdAt: string;
}

export interface PendingTeamInvitation {
  id: string;
  commissionBps: number;
  retainedShareBps: number;
  teamOverrideMaxBps: number;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  expiresAt: string | null;
  createdAt: string;
}

export interface ReferralAuthorityInput {
  teamOverrideMaxBps: number;
  teamInvitesEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  b2bCanDelegate: boolean;
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
      discountPercent?: number | null;
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
  referral: () => request<ReferralSnapshot>("/referral"),
  referralInvitation: () => request<{ invitation: PendingTeamInvitation | null }>("/referral/invitation"),
  acceptReferralInvitation: () => request<{ accepted: true }>("/referral/invitation/accept", { method: "POST" }),
  declineReferralInvitation: (inviteId: string) =>
    request<{ declined: boolean }>("/referral/invitation/decline", {
      method: "POST", body: JSON.stringify({ inviteId }),
    }),
  referralApplication: () => request<{ application: ReferralApplication | null }>("/referral/applications/me"),
  submitReferralApplication: (message: string) =>
    request<{ application: ReferralApplication }>("/referral/applications", {
      method: "POST", body: JSON.stringify({ message }),
    }),
  referralInviteTeam: (input: { email: string; overrideBps: number; authority: ReferralAuthorityInput }) =>
    request<{ invitation: ReferralInvitation }>("/referral/team-invitations", {
      method: "POST", body: JSON.stringify(input),
    }),
  referralRevokeInvitation: (inviteId: string) =>
    request<{ invitation: { id: string; revokedAt: string; revoked: boolean } }>(`/referral/team-invitations/${encodeURIComponent(inviteId)}`, {
      method: "DELETE",
    }),
  referralUpdateTeam: (input: { email: string; overrideBps?: number } & Partial<ReferralAuthorityInput>) =>
    request<{ authority: ReferralAuthorityInput & { email: string; overrideBps: number } }>("/referral/team", {
      method: "PATCH", body: JSON.stringify(input),
    }),
  referralRequestCommission: (input: { requestedCommissionBps: number; reason: string }, key: string) =>
    request<{ request: ReferralRequest }>("/referral/requests/commission", {
      method: "POST", headers: { "idempotency-key": key }, body: JSON.stringify(input),
    }),
  referralRequestB2B: (input: {
    customerEmail: string;
    requestType: "b2b_conversion" | "b2b_pricing";
    requestedDiscountBps: number;
    providers: Record<string, number | null>;
    reason: string;
  }, key: string) => request<{ request: ReferralRequest }>("/referral/requests/b2b", {
    method: "POST", headers: { "idempotency-key": key }, body: JSON.stringify(input),
  }),
  referralSetBusinessPricing: (input: {
    customerEmail: string;
    discountPercent?: number;
    providers?: Record<string, number | null>;
  }, key: string) => request<{ customerEmail: string; discountPercent: number; ceilingPercent: number }>("/referral/referrals/business-pricing", {
    method: "POST", headers: { "idempotency-key": key }, body: JSON.stringify(input),
  }),
  referralUpdateWallet: (address: string) => request<{ membership: ReferralMembership }>("/referral/wallet", {
    method: "PATCH", body: JSON.stringify({ address }),
  }),
};

export function oauthUrl(provider: "google" | "github", inviteToken?: string, referralCode?: string): string {
  const url = new URL(`${API_BASE_URL}/auth/${provider}`);
  if (inviteToken) url.searchParams.set("invite", inviteToken);
  // Carry partner ?ref= through OAuth so a newly created ordinary B2C account is attributed.
  if (referralCode) url.searchParams.set("ref", referralCode);
  return url.toString();
}
