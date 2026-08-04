// Чистая часть страницы «Расход движка»: типы ответа /admin/finance/engine-spend и
// форматирование, вынесенные из page.tsx для юнит-тестов (как paying-users-lib.ts).

export type EngineSpendDays = 1 | 7 | 30;
export type EngineSpendClass = "client" | "openkeys" | "internal";

export interface EngineSpendProviderRow {
  provider?: string;
  requests?: number;
  charge_usd?: number;
  real_usd?: number;
}

export interface EngineSpendModelRow extends EngineSpendProviderRow {
  model?: string;
}

export interface EngineSpendAccountRow {
  account?: string;
  handle?: string | null;
  account_class?: EngineSpendClass;
  owner?: { user_id?: string; email?: string; customer_type?: "b2c" | "b2b" | null } | null;
  requests?: number;
  charge_usd?: number;
  real_usd?: number;
  last_ts?: number;
}

export interface EngineSpendClassTotals {
  accounts?: number;
  requests?: number;
  charge_usd?: number;
  real_usd?: number;
}

export interface EngineSpendResponse {
  generated_at?: string;
  days?: EngineSpendDays;
  requests?: number;
  charge_usd?: number;
  real_usd?: number;
  providers?: EngineSpendProviderRow[];
  models?: EngineSpendModelRow[];
  accounts?: EngineSpendAccountRow[];
  by_class?: Record<EngineSpendClass, EngineSpendClassTotals>;
}

export type EngineSpendFilter = "" | EngineSpendClass;

/** Фильтр по классу аккаунта: посмотреть только OpenKeys — или, наоборот, убрать их из выборки. */
export const ENGINE_SPEND_FILTERS: Array<[EngineSpendFilter, string]> = [
  ["", "все аккаунты"],
  ["client", "только клиенты"],
  ["openkeys", "только OpenKeys"],
  ["internal", "только внутренние"],
];

export function filterEngineSpendAccounts(
  rows: EngineSpendAccountRow[],
  filter: EngineSpendFilter,
): EngineSpendAccountRow[] {
  return filter === "" ? rows : rows.filter((row) => (row.account_class ?? "internal") === filter);
}

export const ENGINE_SPEND_WINDOWS: Array<{ days: EngineSpendDays; label: string }> = [
  { days: 1, label: "24 часа" },
  { days: 7, label: "7 дней" },
  { days: 30, label: "30 дней" },
];

/** Скидка = 1 − списано/real-API. Нулевой real-API (нет данных) → «—», а не Infinity. */
export function discountLabel(chargeUsd: number | undefined, realUsd: number | undefined): string {
  const real = realUsd ?? 0;
  const charge = chargeUsd ?? 0;
  if (real <= 0) return "—";
  return `${Math.round((1 - charge / real) * 100)}%`;
}

export function providerLabel(provider: string | undefined): string {
  if (provider === "openai") return "GPT (Codex)";
  if (provider === "anthropic") return "Claude (подписки)";
  if (provider === "google") return "Gemini";
  return provider || "—";
}

export function accountClassLabel(kind: EngineSpendClass | undefined): string {
  if (kind === "client") return "клиент";
  if (kind === "openkeys") return "OpenKeys";
  return "внутренний";
}

/** Подпись аккаунта: email клиента, иначе handle движка, иначе сам id. */
export function accountTitle(row: EngineSpendAccountRow): string {
  return row.owner?.email || row.handle || row.account || "—";
}

export function isClientAccount(row: EngineSpendAccountRow): boolean {
  return row.account_class === "client";
}

export const ENGINE_SPEND_ACCOUNTS_CSV_HEADER = [
  "аккаунт",
  "класс",
  "handle",
  "engine_account_id",
  "запросы",
  "списано_usd",
  "real_api_usd",
];

export function buildEngineSpendAccountsCsvRows(rows: EngineSpendAccountRow[]): unknown[][] {
  return rows.map((row) => [
    accountTitle(row),
    accountClassLabel(row.account_class),
    row.handle ?? "",
    row.account ?? "",
    row.requests ?? 0,
    row.charge_usd ?? 0,
    row.real_usd ?? 0,
  ]);
}

export const ENGINE_SPEND_CSV_HEADER = [
  "модель",
  "провайдер",
  "запросы",
  "списано_usd",
  "real_api_usd",
  "скидка",
];

export function buildEngineSpendCsvRows(models: EngineSpendModelRow[]): unknown[][] {
  return models.map((row) => [
    row.model ?? "",
    row.provider ?? "",
    row.requests ?? 0,
    row.charge_usd ?? 0,
    row.real_usd ?? 0,
    discountLabel(row.charge_usd, row.real_usd),
  ]);
}
