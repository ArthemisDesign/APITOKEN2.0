// Typed client for the sales-api backend (apps/sales-api).
// All money fields are decimal STRINGS of nanoUSD (1 USD = 1e9 nano).
// Never do float math on them — formatting goes through formatUsd / usdToNano (BigInt only).

// В проде дефолт — same-origin ("" => относительные /v1/...): Caddy на partners.apitoken.sale
// проксирует /v1 на sales-api, куки остаются first-party. Прямой URL нужен только в dev.
export const API_URL =
  process.env.NEXT_PUBLIC_SALES_API_URL ||
  (process.env.NODE_ENV === "development" ? "http://127.0.0.1:3100" : "");

export class ApiError extends Error {
  status: number;
  data: unknown;
  constructor(status: number, message: string, data?: unknown) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.data = data;
  }
}

type ApiOptions = {
  method?: "GET" | "POST" | "PATCH" | "DELETE";
  body?: unknown;
  headers?: Record<string, string>;
};

export async function api<T>(path: string, opts: ApiOptions = {}): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`${API_URL}${path}`, {
      method: opts.method ?? "GET",
      credentials: "include",
      headers: {
        ...(opts.body !== undefined ? { "Content-Type": "application/json" } : {}),
        ...opts.headers,
      },
      body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
    });
  } catch {
    throw new ApiError(0, "Network error — could not reach the partner API.");
  }
  let data: unknown = null;
  try {
    data = await res.json();
  } catch {
    // non-JSON body (204 etc.) is fine
  }
  if (!res.ok) {
    const message =
      (data && typeof data === "object" && "error" in data && typeof (data as { error: unknown }).error === "string"
        ? (data as { error: string }).error
        : null) ?? `Request failed (${res.status})`;
    throw new ApiError(res.status, message, data);
  }
  return data as T;
}

// ---------------------------------------------------------------------------
// Money helpers (nanoUSD strings <-> display)
// ---------------------------------------------------------------------------

const NANO = 1_000_000_000n;

/** API money is an unsigned, canonical base-10 nanoUSD string. */
export function isCanonicalNanoUsd(value: unknown): value is string {
  return typeof value === "string" && /^(0|[1-9]\d*)$/.test(value);
}

/** Positive API money excludes zero while retaining the canonical string contract. */
export function isPositiveNanoUsd(value: unknown): value is string {
  return isCanonicalNanoUsd(value) && value !== "0";
}

/** Displayable balances may be signed, but still use one canonical base-10 representation. */
export function isCanonicalSignedNanoUsd(value: unknown): value is string {
  return typeof value === "string"
    && value !== "-0"
    && /^-?(0|[1-9]\d*)$/.test(value);
}

export function parseCanonicalNanoUsd(value: unknown): bigint | null {
  return isCanonicalNanoUsd(value) ? BigInt(value) : null;
}

export function parseCanonicalSignedNanoUsd(value: unknown): bigint | null {
  return isCanonicalSignedNanoUsd(value) ? BigInt(value) : null;
}

/** Sum API money without coercing malformed values to zero. */
export function sumCanonicalNanoUsd(values: readonly unknown[]): string | null {
  let total = 0n;
  for (const value of values) {
    const parsed = parseCanonicalNanoUsd(value);
    if (parsed === null) return null;
    total += parsed;
  }
  return total.toString();
}

/** Format a nanoUSD decimal string as "$1,234.56". Safe for arbitrary size. */
export function formatUsd(nano: string | null | undefined): string {
  const parsed = parseCanonicalSignedNanoUsd(nano);
  if (parsed === null) return "—";
  const negative = parsed < 0n;
  const n = negative ? -parsed : parsed;
  const dollars = n / NANO;
  const cents = (n % NANO) / 10_000_000n; // 2 decimal places, truncated
  return `${negative ? "−" : ""}$${dollars.toLocaleString("en-US")}.${cents
    .toString()
    .padStart(2, "0")}`;
}

/** Compact form for chart axes: "$12", "$1.2k". */
export function formatUsdCompact(nano: string | null | undefined): string {
  const parsed = parseCanonicalSignedNanoUsd(nano);
  if (parsed === null) return "—";
  const negative = parsed < 0n;
  const n = negative ? -parsed : parsed;
  const dollars = n / NANO;
  const prefix = negative ? "−$" : "$";
  if (dollars >= 1000n) {
    const whole = dollars / 1000n;
    const tenth = (dollars % 1000n) / 100n;
    return tenth === 0n ? `${prefix}${whole}k` : `${prefix}${whole}.${tenth}k`;
  }
  return `${prefix}${dollars}`;
}

/**
 * Parse a user-typed USD amount ("25", "25.5", "0.99") into a nanoUSD string.
 * Returns null on invalid input. Up to 9 decimal places, digits only, no signs.
 */
export function usdToNano(input: string): string | null {
  const s = input.trim();
  const m = /^(\d+)(?:\.(\d{1,9}))?$/.exec(s);
  if (!m) return null;
  const whole = BigInt(m[1]);
  const fracStr = (m[2] ?? "").padEnd(9, "0");
  const nano = whole * NANO + BigInt(fracStr);
  if (nano <= 0n) return null;
  return nano.toString();
}

/** "1250" bps -> "12.5%" */
export function formatBps(bps: number | null | undefined): string {
  if (bps === null || bps === undefined) return "—";
  const pct = bps / 100;
  return `${Number.isInteger(pct) ? pct : pct.toFixed(2).replace(/0+$/, "").replace(/\.$/, "")}%`;
}

export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleDateString("en-US", { year: "numeric", month: "short", day: "numeric" });
}

// ---------------------------------------------------------------------------
// API types
// ---------------------------------------------------------------------------

export type Partner = {
  id: string;
  email: string | null;
  displayName: string | null;
  telegramUsername: string | null;
  referralCode: string;
  commissionBps: number;
  subCommissionBps: number;
  status: string;
  payoutMethod?: string | null;
  payoutDetails?: { network?: string; asset?: string; address?: string } | string | null;
  promoEnabled?: boolean;
  promoMaxValueNano?: string;
  promoMaxCount?: number;
};

export type PromoCodeRow = {
  id: string;
  code: string;
  valueNano: string;
  status: "active" | "redeemed" | "disabled";
  redeemedAt?: string | null;
  createdAt?: string;
};

export type PromoListResponse = {
  enabled: boolean;
  maxValueNano: string;
  maxCount: number;
  redeemUrl: string;
  pricingAffected?: false;
  items: PromoCodeRow[];
};

export type Overview = {
  referralCode: string;
  referralUrl: string;
  commissionBps: number;
  subCommissionBps: number;
  referralDiscountEnabled?: boolean;
  referralDiscountBps?: number;
  referralPricingAffected?: false;
  referredUsers: number;
  teamSize: number;
  totals: {
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
  };
  last30d: { spendNano: string; earnedNano: string; adjustmentNano: string; netNano: string };
};

export type ReferralRow = {
  userMask: string;
  // Masked 8-hex machine reference retained by the expand-only API.
  userRef?: string;
  attributedAt: string;
  spendNano: string;
  earnedNano: string;
  adjustmentNano: string;
  netNano: string;
  topupNano: string;
  // Commerce/engine enrichment; discountPercent is the actual price. referralFloorBps is legacy
  // attribution metadata only and must never be rendered as an applied discount.
  customerType: "b2c" | "b2b" | null;
  discountPercent: number | null;
  referralFloorBps: number | null;
  balanceNano: string | null;
  status: string | null;
};

export type EarningRow = {
  date: string; spendNano: string; earnedNano: string; adjustmentNano: string; netNano: string;
};

export type TeamRow = {
  id: string;
  email: string | null;
  telegramUsername: string | null;
  displayName: string | null;
  commissionBps: number;
  referredUsers: number;
  earnedNano: string;
  adjustmentNano: string;
  netNano: string;
  myOverrideNano: string;
  myOverrideAdjustmentNano: string;
  myOverrideNetNano: string;
  status: string;
};

export type InviteRow = {
  code: string;
  inviteUrl: string;
  telegramUsername: string | null;
  commissionBps: number | null;
  subCommissionBps?: number | null;
  referralDiscountBps?: number;
  referralDiscountEnabled?: boolean;
  promoEnabled?: boolean;
  promoMaxCount?: number;
  promoMaxValueNano?: string;
  expiresAt: string | null;
  consumedAt: string | null;
};

export type PayoutRow = {
  id: string;
  amountNano: string;
  status: string;
  method: string;
  details?: unknown;
  requestedAt: string;
  paidAt: string | null;
  txHash?: string | null;
  chainStatus?: string | null;
  explorerUrl?: string | null;
};

export type PeriodHistoryRow = {
  key: string;
  index: 1 | 2;
  start: string;
  end: string;
  phase: "accruing" | "locked" | "payable" | "closed";
  payoutDate: string;
  earnedNano: string;
  adjustmentNano: string;
  netNano: string;
};

export type PeriodState = {
  now: string;
  current: { key: string; start: string; end: string; accruedNano: string; adjustmentNano: string; netNano: string };
  locked: { key: string; endedAt: string; unlocksAt: string; earnedNano: string; adjustmentNano: string; netNano: string }[];
  nextPayout: { date: string; estimatedNano: string };
  lifetimeEarnedNano: string;
  lifetimeAdjustmentNano: string;
  lifetimeNetNano: string;
  lifetimePaidNano: string;
  debtNano: string;
  payableNano: string;
  unpaidNano: string;
  wallet: string | null;
  minPayoutNano: string;
  lockDays: number;
  windowDays: number;
  history: PeriodHistoryRow[];
};

export type DuePayoutRow = {
  partnerId: string;
  telegramUsername: string | null;
  displayName: string | null;
  status: "active" | "suspended" | "pending";
  payableNano: string;
  debtNano: string;
  adjustmentNano: string;
  netNano: string;
  walletAddress: string | null;
  eligible: boolean;
  reason: "ok" | "below_minimum" | "no_wallet" | "zero" | "inactive";
};

export type PayoutListResponse = {
  period: {
    key: string;
    start: string;
    end: string;
    payoutWindowStart: string;
    payoutWindowEnd: string;
    phase: string;
  };
  items: DuePayoutRow[];
  minPayoutNano: string;
};

// Admin
export type AdminPartnerRow = {
  id: string;
  email: string | null;
  telegramUsername?: string | null;
  displayName?: string | null;
  referralCode?: string;
  parentEmail?: string | null;
  parentTelegramUsername?: string | null;
  parentId?: string | null;
  commissionBps: number;
  subCommissionBps: number;
  referralDiscountBps?: number;
  referralDiscountEnabled?: boolean;
  status: string;
  earnedNano?: string;
  referredUsers?: number;
  promoEnabled?: boolean;
  promoMaxValueNano?: string;
  promoMaxCount?: number;
  promoUsed?: number;
};

export type AdminPayoutRow = {
  id: string;
  partnerEmail?: string;
  partnerId?: string;
  amountNano: string;
  status: string;
  method: string;
  details?: string;
  requestedAt: string;
  paidAt?: string | null;
};

// ---------------------------------------------------------------------------
// Partner analytics (admin) — all *Nano are decimal strings, dates are ISO.
// ---------------------------------------------------------------------------

export type PartnerAnalyticsRow = {
  id: string;
  email: string | null;
  telegramUsername: string | null;
  displayName: string | null;
  status: "active" | "suspended" | "pending";
  referralCode: string;
  parentId: string | null;
  parentLabel: string | null;
  commissionBps: number;
  subCommissionBps: number;
  referralDiscountEnabled: boolean;
  referralDiscountBps: number;
  promoEnabled: boolean;
  depositsTotalNano: string;
  deposits30dNano: string;
  referredUsers: number;
  convertedUsers: number;
  spendTotalNano: string;
  spend30dNano: string;
  earnedTotalNano: string;
  earned30dNano: string;
  adjustmentTotalNano: string;
  adjustment30dNano: string;
  netTotalNano: string;
  net30dNano: string;
  paidNano: string;
  unpaidNano: string;
  debtNano: string;
  payableNano: string;
  teamSize: number;
  linksTotal: number;
  linksUsed: number;
  promosTotal: number;
  promosUsed: number;
  lastSeenAt: string | null;
  lastReferralAt: string | null;
  lastDepositAt: string | null;
  createdAt: string;
};

export type PartnerAnalyticsTotals = {
  total: number;
  active: number;
  depositsNano: string;
  referredUsers: number;
  convertedUsers: number;
  unpaidNano: string;
  adjustmentsNano: string;
  netCommissionsNano: string;
  debtNano: string;
  payableNano: string;
};

export type PartnerAnalyticsList = { items: PartnerAnalyticsRow[]; totals: PartnerAnalyticsTotals };

export type PartnerAnalyticsSortKey =
  | "deposits_total" | "deposits_30d" | "referred_users" | "converted_users"
  | "spend_total" | "spend_30d" | "earned_total" | "earned_30d" | "unpaid"
  | "team_size" | "last_seen_at" | "created_at";

export type PartnerActivityEvent = {
  type: string;
  at: string;
  amountNano: string | null;
  label: string;
  meta: Record<string, unknown>;
};

export type PartnerDetailBundle = {
  partner: PartnerAnalyticsRow;
  daily: { date: string; spendNano: string; earnedNano: string; adjustmentNano: string; netNano: string }[];
  team: {
    id: string; email: string | null; telegramUsername: string | null; displayName: string | null;
    status: string; commissionBps: number; referredUsers: number;
    theirEarnedNano: string; theirAdjustmentNano: string; theirNetNano: string;
    myOverrideNano: string; myOverrideAdjustmentNano: string; myOverrideNetNano: string;
  }[];
  discountLinks: { id: string; code: string; discountBps: number; note: string | null; consumedAt: string | null; createdAt: string }[];
  promos: { id: string; code: string; valueNano: string; status: string; discountBps: number; redeemedAt: string | null; createdAt: string }[];
  payouts: { id: string; amountNano: string; status: string; requestedAt: string; decidedAt: string | null; paidAt: string | null; adminNote: string | null }[];
  referrals: {
    userMask: string; userRef?: string; attributedAt: string; spendNano: string;
    earnedNano: string; adjustmentNano: string; netNano: string;
    customerType?: "b2c" | "b2b" | null; discountPercent?: number | null; referralFloorBps?: number | null;
  }[];
};

// ---------------------------------------------------------------------------
// On-chain payout batches (admin). *Nano are decimal strings; wei are strings.
// ---------------------------------------------------------------------------

export type PayoutWindow = { open: boolean; opensAt: string | null; closesAt: string | null; enforced?: boolean };

export type PayoutEngine = { configured: boolean; window: PayoutWindow };

export type PayoutBatchDto = {
  id: string;
  status: "preparing" | "prepared" | "sending" | "sent" | "failed" | "canceled";
  hotWalletAddress: string | null;
  totalNano: string;
  recipientCount: number;
  gasPriceGwei: string | null;
  minNano: string;
  note: string | null;
  createdBy: string | null;
  error: string | null;
  createdAt: string;
  preparedAt: string | null;
  sentAt: string | null;
  completedAt: string | null;
};

export type PayoutRowDto = {
  id: string;
  partnerId: string;
  partner: string;
  amountNano: string;
  status: string;
  walletAddress: string | null;
  txHash: string | null;
  chainStatus: string | null;
  chainError: string | null;
  paidAt: string | null;
};

export type PayoutReportDto = {
  batch: PayoutBatchDto;
  rows: PayoutRowDto[];
  window: PayoutWindow;
  chain: {
    configured: boolean;
    hotWalletAddress: string | null;
    currentHotWalletAddress: string | null;
    configurationMatchesBatch: boolean | null;
    usdtBalanceNano: string | null;
    bnbBalanceWei: string | null;
    requiredUsdtNano: string;
    requiredBnbWei: string | null;
    sufficientUsdt: boolean | null;
    sufficientBnb: boolean | null;
    gasPriceGwei: string;
  };
  invalidAddresses: { partnerId: string; walletAddress: string; reason: string }[];
  accounting: {
    ready: boolean;
    reasons: string[];
    usageCursor: string;
    usageSourceHead: string;
    fundingLotCursor: string;
    fundingLotSourceHead: string;
    paymentReversalCursor: string;
    paymentReversalSourceHead: string;
    incompleteUsageCount: string;
    missingCommissionSliceCount: string;
    incompleteReversalCount: string;
    reversalCount: string;
    adjustmentCount: string;
    adjustmentNano: string;
  } | null;
};
