export const PAYING_USER_SORTS = [
  ["spent", "расход за окно"],
  ["paid", "оплачено всего"],
  ["last_paid", "последняя оплата"],
  ["last_seen", "активность"],
] as const;

export type PayingUserSort = (typeof PAYING_USER_SORTS)[number][0];
export type PayingUserProvider = "anthropic" | "openai" | "google" | "other";
export type PayingUserFunding = "all" | "payments" | "manual" | "bonus";
export type PayingUserFundingKind = "payments" | "payments_and_manual" | "manual" | "bonus_only";

/** Источник финансирования клиента: подписи для селектора когорты. */
export const PAYING_USER_FUNDINGS: Array<[PayingUserFunding, string]> = [
  ["all", "деньги + бонусный расход"],
  ["payments", "платёжный провайдер"],
  ["manual", "ручное денежное пополнение"],
  ["bonus", "только бонусный расход"],
];
export type PayingUserDays = 1 | 7 | 30;

export interface ProviderSpend {
  anthropic_nano?: string;
  openai_nano?: string;
  google_nano?: string;
  other_nano?: string;
}

export interface PayingUserRow {
  user_id?: string;
  email?: string;
  display_name?: string;
  status?: "active" | "disabled";
  customer_type?: "b2c" | "b2b" | null;
  tier?: number | null;
  multiplier_bp?: number | null;
  paid_nano?: string;
  payments_count?: number;
  manual_paid_nano?: string;
  manual_topups_count?: number;
  last_paid_at?: string | null;
  spent_nano?: string;
  funding_kind?: PayingUserFundingKind;
  paid_funded_spent_nano?: string;
  bonus_funded_spent_nano?: string;
  other_funded_spent_nano?: string;
  unattributed_spent_nano?: string;
  provider_spend?: ProviderSpend;
  active_api_keys?: number;
  last_seen_at?: string | null;
  created_at?: string;
}

export interface PayingUsersSummary {
  paying_users?: number;
  cohort_users?: number;
  bonus_only_users?: number;
  active_spenders?: number;
  paid_nano?: string;
  manual_paid_nano?: string;
  spent_nano?: string;
  bonus_only_spent_nano?: string;
  provider_spend?: ProviderSpend;
  provider_users?: Partial<Record<PayingUserProvider, number>>;
}

export interface PayingUsersResponse {
  generated_at?: string;
  days?: PayingUserDays;
  total?: number;
  limit?: number;
  offset?: number;
  summary?: PayingUsersSummary;
  rows?: PayingUserRow[];
}

export interface PayingUsersPageState {
  days: PayingUserDays;
  limit: number;
  offset: number;
  q: string;
  status: "" | "active" | "disabled";
  provider: "" | PayingUserProvider;
  funding: PayingUserFunding;
  sort: PayingUserSort;
  dir: "asc" | "desc";
}

export const INITIAL_PAYING_USERS_PAGE: PayingUsersPageState = {
  days: 30,
  limit: 50,
  offset: 0,
  q: "",
  status: "",
  provider: "",
  funding: "all",
  sort: "spent",
  dir: "desc",
};

export function payingUsersQuery(state: PayingUsersPageState): string {
  const params = new URLSearchParams({
    days: String(state.days),
    limit: String(state.limit),
    offset: String(state.offset),
    sort: state.sort,
    dir: state.dir,
    funding: state.funding,
  });
  if (state.q) params.set("q", state.q);
  if (state.status) params.set("status", state.status);
  if (state.provider) params.set("provider", state.provider);
  return params.toString();
}

export function providerNano(spend: ProviderSpend | undefined, provider: PayingUserProvider): string {
  return spend?.[`${provider}_nano`] ?? "0";
}

export function isPositiveNano(value: string | null | undefined): boolean {
  try {
    return BigInt(value ?? "0") > 0n;
  } catch {
    return false;
  }
}

/** Доля 0..10000 basis points, вычисленная без преобразования денежных сумм в Number. */
export function providerShareBp(amount: string | null | undefined, total: string | null | undefined): number {
  try {
    const amountNano = BigInt(amount ?? "0");
    const totalNano = BigInt(total ?? "0");
    if (amountNano <= 0n || totalNano <= 0n) return 0;
    const basisPoints = (amountNano * 10_000n) / totalNano;
    return Number(basisPoints > 10_000n ? 10_000n : basisPoints);
  } catch {
    return 0;
  }
}

export function payingTierLabel(row: Pick<PayingUserRow, "customer_type" | "tier">): string {
  if (row.customer_type === "b2b") return "B2B";
  return row.tier != null ? (["Starter", "Builder", "Pro", "Studio", "Scale"][row.tier] ?? "—") : "—";
}

export function spendWindowLabel(days: PayingUserDays): string {
  return days === 1 ? "24 часа" : `${days} дней`;
}

export function payingCohortUsers(summary: PayingUsersSummary | undefined): number {
  return summary?.cohort_users ?? summary?.paying_users ?? 0;
}

export const PAYING_USERS_CSV_HEADER = [
  "email",
  "имя",
  "статус",
  "тариф",
  "funding_kind",
  "оплачено_nanoUSD",
  "платежей",
  "ручных_пополнений",
  "ручные_nanoUSD",
  "расход_окна_nanoUSD",
  "paid_funded_spent_nano",
  "bonus_funded_spent_nano",
  "other_funded_spent_nano",
  "unattributed_spent_nano",
  "claude_nanoUSD",
  "gpt_nanoUSD",
  "gemini_nanoUSD",
  "другое_nanoUSD",
  "последняя_оплата",
  "последняя_активность",
  "активные_ключи",
];

export function buildPayingUsersCsvRows(rows: PayingUserRow[]): unknown[][] {
  return rows.map((row) => [
    row.email ?? "",
    row.display_name ?? "",
    row.status ?? "",
    payingTierLabel(row),
    row.funding_kind ?? "",
    row.paid_nano ?? "0",
    row.payments_count ?? 0,
    row.manual_topups_count ?? 0,
    row.manual_paid_nano ?? "0",
    row.spent_nano ?? "0",
    row.paid_funded_spent_nano ?? "0",
    row.bonus_funded_spent_nano ?? "0",
    row.other_funded_spent_nano ?? "0",
    row.unattributed_spent_nano ?? "0",
    providerNano(row.provider_spend, "anthropic"),
    providerNano(row.provider_spend, "openai"),
    providerNano(row.provider_spend, "google"),
    providerNano(row.provider_spend, "other"),
    row.last_paid_at ?? "",
    row.last_seen_at ?? "",
    row.active_api_keys ?? 0,
  ]);
}
