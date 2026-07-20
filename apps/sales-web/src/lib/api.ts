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

/** Format a nanoUSD decimal string as "$1,234.56". Safe for arbitrary size. */
export function formatUsd(nano: string | null | undefined): string {
  if (nano === null || nano === undefined || nano === "") return "$0.00";
  let s = String(nano).trim();
  let neg = false;
  if (s.startsWith("-")) {
    neg = true;
    s = s.slice(1);
  }
  if (!/^\d+$/.test(s)) return "$0.00";
  const n = BigInt(s);
  const dollars = n / NANO;
  const cents = (n % NANO) / 10_000_000n; // 2 decimal places, truncated
  return `${neg ? "−" : ""}$${dollars.toLocaleString("en-US")}.${cents
    .toString()
    .padStart(2, "0")}`;
}

/** Compact form for chart axes: "$12", "$1.2k". */
export function formatUsdCompact(nano: string | null | undefined): string {
  if (!nano || !/^\d+$/.test(String(nano).trim())) return "$0";
  const dollars = BigInt(String(nano).trim()) / NANO;
  if (dollars >= 1000n) {
    const whole = dollars / 1000n;
    const tenth = (dollars % 1000n) / 100n;
    return tenth === 0n ? `$${whole}k` : `$${whole}.${tenth}k`;
  }
  return `$${dollars}`;
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
  items: PromoCodeRow[];
};

export type Overview = {
  referralCode: string;
  referralUrl: string;
  commissionBps: number;
  subCommissionBps: number;
  referralDiscountEnabled?: boolean;
  referralDiscountBps?: number;
  referredUsers: number;
  teamSize: number;
  totals: {
    earnedNano: string;
    directNano: string;
    overrideNano: string;
    paidNano: string;
    pendingPayoutNano: string;
    availableNano: string;
  };
  last30d: { spendNano: string; earnedNano: string };
};

export type ReferralRow = {
  userMask: string;
  attributedAt: string;
  spendNano: string;
  earnedNano: string;
};

export type EarningRow = { date: string; spendNano: string; earnedNano: string };

export type TeamRow = {
  id: string;
  email: string | null;
  telegramUsername: string | null;
  displayName: string | null;
  commissionBps: number;
  referredUsers: number;
  earnedNano: string;
  myOverrideNano: string;
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
};

export type PeriodHistoryRow = {
  key: string;
  index: 1 | 2;
  start: string;
  end: string;
  phase: "accruing" | "locked" | "payable" | "closed";
  payoutDate: string;
  earnedNano: string;
};

export type PeriodState = {
  now: string;
  current: { key: string; start: string; end: string; accruedNano: string };
  locked: { key: string; endedAt: string; unlocksAt: string; earnedNano: string }[];
  nextPayout: { date: string; estimatedNano: string };
  lifetimeEarnedNano: string;
  lifetimePaidNano: string;
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
  payableNano: string;
  walletAddress: string | null;
  eligible: boolean;
  reason: "ok" | "below_minimum" | "no_wallet" | "zero";
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
