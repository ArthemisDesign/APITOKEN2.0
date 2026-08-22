// Точные денежные и payout-хелперы защищённого on-chain модуля партнёров.

// Серверные сортировки partner-analytics — ровно PARTNER_ANALYTICS_SORTS из packages/sales-db/analytics.
export const PARTNER_SORTS: ReadonlyArray<readonly [string, string]> = [
  ["unpaid", "к выплате"],
  ["deposits_total", "пополнения всего"],
  ["deposits_30d", "пополнения 30д"],
  ["earned_total", "заработок всего"],
  ["earned_30d", "заработок 30д"],
  ["spend_total", "расход всего"],
  ["spend_30d", "расход 30д"],
  ["converted_users", "конверсия"],
  ["referred_users", "рефералы"],
  ["team_size", "команда"],
  ["last_seen_at", "активность"],
  ["created_at", "регистрация"],
];

export const PARTNER_PHASE_LABEL: Record<string, string> = {
  accruing: "начисление",
  locked: "лок 7 дней",
  payable: "окно выплат",
  closed: "закрыт",
};

export const PAYOUT_REASON_LABEL: Record<string, string> = {
  below_minimum: "ниже минимума",
  no_wallet: "нет кошелька",
  inactive: "неактивен",
  zero: "нет суммы",
};

export const PAYOUT_STATUS_KIND: Record<string, "warn" | "info" | "ok" | "bad"> = {
  requested: "warn",
  approved: "info",
  paid: "ok",
  rejected: "bad",
};

export const BATCH_STATUS_KIND: Record<string, "warn" | "info" | "ok" | "bad" | ""> = {
  preparing: "warn",
  prepared: "info",
  sending: "warn",
  sent: "ok",
  failed: "bad",
  canceled: "",
};

export interface PartnerOverview {
  partners?: number;
  activePartners?: number;
  referredUsers?: number;
  totalSpendNano?: string;
  totalCommissionsNano?: string;
  totalAdjustmentsNano?: string;
  totalNetCommissionsNano?: string;
  totalDebtNano?: string;
  totalPayableNano?: string;
  pendingPayoutsNano?: string;
  paidPayoutsNano?: string;
}

export interface PayoutEngine {
  configured?: boolean;
  window?: {
    open?: boolean;
    opensAt?: string;
    closesAt?: string;
    enforced?: boolean;
  };
  chain?: {
    ready?: boolean;
    hotWalletAddress?: string | null;
    usdtBalanceNano?: string | null;
    bnbBalanceWei?: string | null;
    gasCostPerTransferWei?: string | null;
    issue?: "not_configured" | "read_unavailable" | null;
  };
}

export interface PayoutDueItem {
  partnerId?: string;
  email?: string;
  payableNano?: string;
  debtNano?: string;
  adjustmentNano?: string;
  netNano?: string;
  walletAddress?: string;
  eligible?: boolean;
  reason?: string;
}

export interface PayoutDue {
  period?: {
    key?: string;
    start?: string;
    end?: string;
    phase?: string;
    payoutWindowStart?: string;
    payoutWindowEnd?: string;
  };
  items?: PayoutDueItem[];
  minPayoutNano?: string;
}

export interface PartnerAnalyticsItem {
  id?: string;
  referralCode?: string;
  email?: string;
  status?: string;
  referredUsers?: number;
  convertedUsers?: number;
  deposits30dNano?: string;
  depositsTotalNano?: string;
  spend30dNano?: string;
  earned30dNano?: string;
  earnedTotalNano?: string;
  unpaidNano?: string;
  adjustmentTotalNano?: string;
  netTotalNano?: string;
  net30dNano?: string;
  debtNano?: string;
  payableNano?: string;
  lastSeenAt?: string;
}

export interface PartnerAnalytics {
  items?: PartnerAnalyticsItem[];
  totals?: { total?: number; active?: number; unpaidNano?: string; debtNano?: string; payableNano?: string };
}

export interface PayoutItem {
  partnerId?: string;
  amountNano?: string;
  status?: string;
  method?: string;
  requestedAt?: string;
  decidedAt?: string;
  paidAt?: string;
}

export interface PayoutHistory {
  items?: PayoutItem[];
}

export interface PayoutBatch {
  status?: string;
  totalNano?: string;
  recipientCount?: number;
  gasPriceGwei?: string | number;
  hotWalletAddress?: string;
  createdAt?: string;
  sentAt?: string;
  completedAt?: string;
  error?: string;
}

export interface PayoutBatches {
  items?: PayoutBatch[];
}

// shortWallet: "0x1234…cdef" — первые 6 и последние 4 символа адреса.
export function shortWallet(value: string): string {
  return value.slice(0, 6) + "…" + value.slice(-4);
}

// Сумма payableNano по eligible-строкам — целочисленная BigInt-арифметика, как в легаси.
export function eligibleSumNano(items: PayoutDueItem[]): string {
  return items
    .filter((item) => item.eligible)
    .reduce((sum, item) => sum + BigInt(item.payableNano || "0"), 0n)
    .toString();
}

const WEI_PER_BNB = 1_000_000_000_000_000_000n;

function canonicalUnsigned(value: string | null | undefined): bigint | null {
  if (!value || !/^(0|[1-9]\d*)$/.test(value)) return null;
  try {
    return BigInt(value);
  } catch {
    return null;
  }
}

// BNB gas balance: integer wei only. Keep up to eight fractional digits so the current
// 0.00001-BNB per-transfer requirement remains visible without float rounding.
export function bnbMoney(value: string | null | undefined): string {
  const amount = canonicalUnsigned(value);
  if (amount == null) return "—";
  const whole = amount / WEI_PER_BNB;
  const fraction = ((amount % WEI_PER_BNB) / 10_000_000_000n)
    .toString()
    .padStart(8, "0")
    .replace(/0+$/, "");
  return `${whole.toLocaleString("en-US")}${fraction ? `.${fraction}` : ""} BNB`;
}

export interface PayoutWalletReadiness {
  kind: "ok" | "warn" | "bad";
  title: string;
  detail: string;
  eligibleCount: number;
  requiredUsdtNano: string;
  requiredBnbWei: string | null;
}

/**
 * One operator verdict from the additive read-only chain proof and the exact current payout list.
 * Absence/malformed money is unavailable, never a false zero. An empty wallet remains visible even
 * between payout windows, while an actual current shortfall is a hard blocker.
 */
export function payoutWalletReadiness(
  engine: PayoutEngine,
  items: PayoutDueItem[],
): PayoutWalletReadiness {
  const eligible = items.filter((item) => item.eligible && canonicalUnsigned(item.payableNano) != null);
  const requiredUsdt = eligible.reduce((sum, item) => sum + BigInt(item.payableNano!), 0n);
  const base = {
    eligibleCount: eligible.length,
    requiredUsdtNano: requiredUsdt.toString(),
  };

  if (!engine.configured) {
    return {
      ...base,
      kind: "bad",
      title: "Payout-движок не настроен",
      detail: "Нет hot-wallet ключа или send RPC — on-chain отправки недоступны.",
      requiredBnbWei: null,
    };
  }

  const chain = engine.chain;
  if (!chain || !chain.ready) {
    return {
      ...base,
      kind: "bad",
      title: chain?.issue === "read_unavailable" ? "Кошелёк не удалось проверить" : "Состояние кошелька не получено",
      detail: "Баланс не считается нулевым: проверьте BSC RPC и контракт USDT, затем обновите страницу.",
      requiredBnbWei: null,
    };
  }

  const usdt = canonicalUnsigned(chain.usdtBalanceNano);
  const bnb = canonicalUnsigned(chain.bnbBalanceWei);
  const gasPerTransfer = canonicalUnsigned(chain.gasCostPerTransferWei);
  if (usdt == null || bnb == null || gasPerTransfer == null || !chain.hotWalletAddress) {
    return {
      ...base,
      kind: "bad",
      title: "Ответ кошелька неполный",
      detail: "Адрес или целочисленные балансы отсутствуют; отправка должна оставаться заблокированной.",
      requiredBnbWei: null,
    };
  }

  const requiredBnb = gasPerTransfer * BigInt(eligible.length);
  const requirement = { ...base, requiredBnbWei: requiredBnb.toString() };
  if (requiredUsdt > usdt || requiredBnb > bnb) {
    const assets = [requiredUsdt > usdt ? "USDT" : null, requiredBnb > bnb ? "BNB" : null]
      .filter(Boolean)
      .join(" и ");
    return {
      ...requirement,
      kind: "bad",
      title: `Не хватает ${assets}`,
      detail: `Текущий список: ${eligible.length} переводов. Пополните hot wallet до подготовки батча.`,
    };
  }
  if (usdt === 0n || bnb === 0n) {
    return {
      ...requirement,
      kind: "warn",
      title: "Hot wallet пуст",
      detail: "Сейчас eligible-переводов нет, но до следующего окна нужны и USDT, и BNB для gas.",
    };
  }
  if (eligible.length === 0) {
    return {
      ...requirement,
      kind: "ok",
      title: "Кошелёк доступен",
      detail: "BSC и USDT проверены; в текущем периоде eligible-переводов нет.",
    };
  }
  return {
    ...requirement,
    kind: "ok",
    title: "Средств хватает на текущий список",
    detail: `${eligible.length} переводов обеспечены USDT и BNB gas; перед отправкой backend проверит балансы ещё раз.`,
  };
}

// Текст eligible-ячейки: eligible → «eligible», reason 'ok' → «ждёт окна»,
// известная причина → русский ярлык, иначе сама причина или «нельзя».
export function payoutReasonText(item: { eligible?: boolean; reason?: string }): string {
  if (item.eligible) return "eligible";
  if (item.reason === "ok") return "ждёт окна";
  return (item.reason && PAYOUT_REASON_LABEL[item.reason]) || item.reason || "нельзя";
}

// Commerce email is the only supported browser-visible partner identity.
export function partnerName(partner: {
  email?: string;
}): string {
  return partner.email || "Commerce email недоступен";
}

/**
 * Parse an exact percentage with at most two decimal places into integer basis points.
 * Building the integer from decimal digits avoids rejecting values such as 19.99 because of
 * binary floating-point rounding.
 */
export function parsePercentBps(value: string, maximumBps: number): number | null {
  const match = /^(0|[1-9]\d{0,2})(?:\.(\d{1,2}))?$/.exec(value.trim());
  if (!match) return null;
  const bps = Number(match[1]) * 100 + Number((match[2] ?? "").padEnd(2, "0"));
  return bps <= maximumBps ? bps : null;
}

// Кламп offset, когда текущая страница вышла за сократившийся total
// (легаси: offset >= total && total > 0 → последняя полная страница).
// При total <= 0 кламп не применяется — как в оригинале.
export function clampOffset(offset: number, limit: number, total: number): number {
  if (total <= 0 || offset < total) return offset;
  return Math.max(0, Math.floor((total - 1) / limit) * limit);
}
