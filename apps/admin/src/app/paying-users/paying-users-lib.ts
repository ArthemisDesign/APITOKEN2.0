import { spreadsheetExactInteger, spreadsheetSafeText } from "@/lib/csv";
import { nanoMoney } from "@/lib/format";

export const PAYING_USER_SORTS = [
  ["spent", "расход за окно"],
  ["paid", "оплачено всего"],
  ["last_paid", "последняя оплата"],
  ["last_seen", "активность"],
] as const;

export type PayingUserSort = (typeof PAYING_USER_SORTS)[number][0];
export type PayingUserProvider = "anthropic" | "openai" | "google" | "other";
export type PayingUserFunding = "spenders" | "all" | "payments" | "manual" | "bonus";
export type PayingUserFundingKind = "payments" | "payments_and_manual" | "manual" | "bonus_only" | "spend_only";

/** Источник финансирования клиента: подписи для селектора когорты. */
export const PAYING_USER_FUNDINGS: Array<[PayingUserFunding, string]> = [
  ["spenders", "все с расходом"],
  ["all", "деньги + строгий бонус"],
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

export interface PayingUserUsageModel {
  provider: string | null;
  model: string;
  requests: string;
  input_tokens: string;
  output_tokens: string;
  cache_read_tokens: string;
  cache_write_5m_tokens: string;
  cache_write_1h_tokens: string;
  web_search_requests: string;
  official_nano: string;
  charged_nano: string;
}

export interface PayingUserUsage {
  status: "complete" | "partial" | "unavailable";
  window: string;
  account_count: number;
  available_account_count: number;
  unavailable_account_count: number;
  requests: string;
  total_official_nano: string;
  total_charged_nano: string;
  models: PayingUserUsageModel[];
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
  usage: PayingUserUsage;
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
  funding: "spenders",
  sort: "spent",
  dir: "desc",
};

export function normalizePayingUsersSearch(value: string): string {
  return value.trim().slice(0, 200);
}

export function payingUsersQuery(state: PayingUsersPageState): string {
  const params = new URLSearchParams({
    days: String(state.days),
    limit: String(state.limit),
    offset: String(state.offset),
    sort: state.sort,
    dir: state.dir,
    funding: state.funding,
    include_usage: "true",
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

export function usageNanoMoney(value: string | null | undefined): string {
  try {
    const nano = BigInt(value ?? "0");
    return nano > 0n && nano < 10_000_000n ? "<$0.01" : nanoMoney(value);
  } catch {
    return nanoMoney(value);
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
  "user_id",
  "email",
  "имя",
  "статус",
  "тариф",
  "funding_kind",
  "оплачено_nanoUSD_text",
  "платежей",
  "ручных_пополнений",
  "ручные_nanoUSD_text",
  "расход_окна_nanoUSD_text",
  "paid_funded_spent_nanoUSD_text",
  "bonus_funded_spent_nanoUSD_text",
  "other_funded_spent_nanoUSD_text",
  "unattributed_spent_nanoUSD_text",
  "claude_nanoUSD_text",
  "gpt_nanoUSD_text",
  "gemini_nanoUSD_text",
  "другое_nanoUSD_text",
  "последняя_оплата",
  "последняя_активность",
  "активные_ключи",
  "usage_status",
  "usage_window",
  "usage_account_count",
  "usage_available_account_count",
  "usage_unavailable_account_count",
  "usage_requests_text",
  "provider",
  "model",
  "model_requests_text",
  "input_tokens_text",
  "output_tokens_text",
  "cache_read_tokens_text",
  "cache_write_5m_tokens_text",
  "cache_write_1h_tokens_text",
  "web_search_requests_text",
  "model_official_nanoUSD_text",
  "model_charged_nanoUSD_text",
  "usage_total_official_nanoUSD_text",
  "usage_total_charged_nanoUSD_text",
];

export function buildPayingUsersCsvRows(rows: PayingUserRow[]): unknown[][] {
  return rows.flatMap((row) => {
    const usage = row.usage;
    const models = usage && usage.status !== "unavailable" && usage.models.length ? usage.models : [null];
    return models.map((model) => [
      spreadsheetSafeText(row.user_id ?? ""),
      spreadsheetSafeText(row.email ?? ""),
      spreadsheetSafeText(row.display_name ?? ""),
      spreadsheetSafeText(row.status ?? ""),
      spreadsheetSafeText(payingTierLabel(row)),
      spreadsheetSafeText(row.funding_kind ?? ""),
      spreadsheetExactInteger(row.paid_nano ?? "0"),
      row.payments_count ?? 0,
      row.manual_topups_count ?? 0,
      spreadsheetExactInteger(row.manual_paid_nano ?? "0"),
      spreadsheetExactInteger(row.spent_nano ?? "0"),
      spreadsheetExactInteger(row.paid_funded_spent_nano ?? "0"),
      spreadsheetExactInteger(row.bonus_funded_spent_nano ?? "0"),
      spreadsheetExactInteger(row.other_funded_spent_nano ?? "0"),
      spreadsheetExactInteger(row.unattributed_spent_nano ?? "0"),
      spreadsheetExactInteger(providerNano(row.provider_spend, "anthropic")),
      spreadsheetExactInteger(providerNano(row.provider_spend, "openai")),
      spreadsheetExactInteger(providerNano(row.provider_spend, "google")),
      spreadsheetExactInteger(providerNano(row.provider_spend, "other")),
      spreadsheetSafeText(row.last_paid_at ?? ""),
      spreadsheetSafeText(row.last_seen_at ?? ""),
      row.active_api_keys ?? 0,
      usage?.status ?? "",
      spreadsheetSafeText(usage?.window ?? ""),
      usage?.account_count ?? "",
      usage?.available_account_count ?? "",
      usage?.unavailable_account_count ?? "",
      spreadsheetExactInteger(usage && usage.status !== "unavailable" ? usage.requests : ""),
      spreadsheetSafeText(model ? (model.provider ?? "не указан") : ""),
      spreadsheetSafeText(model?.model ?? ""),
      spreadsheetExactInteger(model?.requests ?? ""),
      spreadsheetExactInteger(model?.input_tokens ?? ""),
      spreadsheetExactInteger(model?.output_tokens ?? ""),
      spreadsheetExactInteger(model?.cache_read_tokens ?? ""),
      spreadsheetExactInteger(model?.cache_write_5m_tokens ?? ""),
      spreadsheetExactInteger(model?.cache_write_1h_tokens ?? ""),
      spreadsheetExactInteger(model?.web_search_requests ?? ""),
      spreadsheetExactInteger(model?.official_nano ?? ""),
      spreadsheetExactInteger(model?.charged_nano ?? ""),
      spreadsheetExactInteger(usage && usage.status !== "unavailable" ? usage.total_official_nano : ""),
      spreadsheetExactInteger(usage && usage.status !== "unavailable" ? usage.total_charged_nano : ""),
    ]);
  });
}
