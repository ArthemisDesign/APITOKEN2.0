// Типы payload'ов partner-admin и чистые хелперы страницы «Партнёры».
// Порт 1:1 констант и функций из partners() crates/server/src/admin-panel.js.

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
}

export interface PayoutDueItem {
  partnerId?: string;
  telegramUsername?: string;
  displayName?: string;
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
  telegramUsername?: string;
  email?: string;
  displayName?: string;
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

// Текст eligible-ячейки: eligible → «eligible», reason 'ok' → «ждёт окна»,
// известная причина → русский ярлык, иначе сама причина или «нельзя».
export function payoutReasonText(item: { eligible?: boolean; reason?: string }): string {
  if (item.eligible) return "eligible";
  if (item.reason === "ok") return "ждёт окна";
  return (item.reason && PAYOUT_REASON_LABEL[item.reason]) || item.reason || "нельзя";
}

// Отображаемое имя партнёра: @telegram, иначе email/displayName (analytics)
// или displayName (payout-list), иначе «—».
export function partnerName(partner: {
  telegramUsername?: string;
  email?: string;
  displayName?: string;
}): string {
  if (partner.telegramUsername) return "@" + partner.telegramUsername;
  return partner.email || partner.displayName || "—";
}

// Кламп offset, когда текущая страница вышла за сократившийся total
// (легаси: offset >= total && total > 0 → последняя полная страница).
// При total <= 0 кламп не применяется — как в оригинале.
export function clampOffset(offset: number, limit: number, total: number): number {
  if (total <= 0 || offset < total) return offset;
  return Math.max(0, Math.floor((total - 1) / limit) * limit);
}
