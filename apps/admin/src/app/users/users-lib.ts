// Типы и чистая логика страницы «Пользователи» — порт соответствующих кусков
// users()/renderUsers()/usersCsv() из crates/server/src/admin-panel.js (строки 637-730).
// Вынесено из page.tsx, чтобы юнит-тесты не тащили React-рендер.
import { spreadsheetExactInteger } from "@/lib/csv";

// Серверные сортировки /admin/users — ровно sort-enum apps/api admin.controller;
// live-поля движка (balance/spent) сервер не сортирует, поэтому их в списке нет.
export const USER_SORTS = [
  ["created_at", "создан"],
  ["last_seen_at", "активность"],
  ["paid_total", "оплачено"],
  ["topup_total", "пополнено"],
  ["spent_30d", "расход 30д"],
] as const;
export type UserSortKey = (typeof USER_SORTS)[number][0];

// Backend требует reason в каждом действии (audit_log) — панель шлёт стандартную
// причину, чтобы не заставлять оператора печатать её на каждый клик.
export const PANEL_REASON = "ручное действие из админ-панели";

// Действия по пользователю: заголовки dialog() дословно из userAction() легаси.
export const USER_ACTION_LABELS = {
  disable: "Отключить пользователя и отозвать сессии",
  enable: "Включить пользователя",
  sessions: "Отозвать все активные сессии",
  totp: "Сбросить 2FA и отозвать сессии",
  business: "Перевести в B2B и установить договорную скидку",
  bonus: "Отозвать фактически выданный welcome-бонус. Идемпотентно: уже отозванный не спишется второй раз",
} as const;
export type UserAction = keyof typeof USER_ACTION_LABELS;

// GET /admin/users — строка страницы. Деньги — легаси-поля коммерции в долларах
// (number), только отображение через money(); все поля опциональны (деградация в "—").
export interface AdminUserPayments {
  paid_count?: number;
  paid_total_usd?: number;
  last_paid_at?: string | null;
  pending_checkouts?: number;
}

export interface AdminUserApiKeys {
  active?: number;
  total?: number;
}

export type UserProviderId = "anthropic" | "openai" | "google" | "kimi" | "other";

export interface AdminUserProviderSpend {
  anthropic_nano?: string;
  openai_nano?: string;
  google_nano?: string;
  kimi_nano?: string;
  other_nano?: string;
}

export const USER_PROVIDER_RAILS = [
  { id: "anthropic", label: "Claude", className: "claude" },
  { id: "openai", label: "GPT", className: "gpt" },
  { id: "google", label: "Gemini", className: "gemini" },
  { id: "kimi", label: "Kimi", className: "kimi" },
  { id: "other", label: "Другие", className: "other" },
] as const satisfies ReadonlyArray<{ id: UserProviderId; label: string; className: string }>;

export interface AdminUser {
  id?: string;
  email?: string;
  display_name?: string | null;
  status?: string;
  engine_live_status?: string | null;
  engine_account_id?: string | null;
  customer_type?: string;
  tier?: number;
  multiplier_bp?: number | null;
  balance_usd?: number | null;
  reserved_usd?: number | null;
  spent_usd?: number | null;
  spent_30d_usd?: number | null;
  provider_spend_30d?: AdminUserProviderSpend;
  cumulative_topup_usd?: number | null;
  payments?: AdminUserPayments;
  api_keys?: AdminUserApiKeys;
  auth_methods?: string[];
  email_verified?: boolean;
  totp_enabled?: boolean;
  last_seen_at?: string | null;
  created_at?: string;
}

export interface AdminUsersPage {
  users?: AdminUser[];
  total?: number;
  limit?: number;
  offset?: number;
}

// Платформенные итоги из /overview (engine demand): подсказка к «видимым» суммам страницы.
export interface EngineDemand {
  balance_usd?: number | null;
  spent_usd?: number | null;
}

// Ответы мутаций userAction(): поля используются в итоговом тосте легаси.
export interface UserActionResult {
  sessions_revoked?: number | null;
  customer_type?: string;
  discount_percent?: number;
  balance_usd?: number | null;
  idempotent_replay?: boolean;
  [key: string]: unknown;
}

export interface UserPageState {
  offset: number;
  limit: number;
  q: string;
  status: string;
  auth: string;
  sort: UserSortKey;
  dir: "asc" | "desc";
}

export const INITIAL_USER_PAGE: UserPageState = {
  offset: 0,
  limit: 50,
  q: "",
  status: "",
  auth: "",
  sort: "created_at",
  dir: "desc",
};

// Query /admin/users — порядок параметров как в users() легаси
// (limit/offset/sort/dir, затем опциональные q/status/auth).
export function usersQuery(page: UserPageState): string {
  const params = new URLSearchParams({
    limit: String(page.limit),
    offset: String(page.offset),
    sort: page.sort,
    dir: page.dir,
  });
  if (page.q) params.set("q", page.q);
  if (page.status) params.set("status", page.status);
  if (page.auth) params.set("auth", page.auth);
  return params.toString();
}

// Клиент, ушедший за последнюю страницу (удалили/отфильтровали): легаси откатывает
// offset на последнюю валидную и перезапрашивает. Возвращает null, если offset валиден.
export function clampedOffset(offset: number, limit: number, total: number): number | null {
  if (offset >= total && total > 0) {
    return Math.max(0, Math.floor((total - 1) / limit) * limit);
  }
  return null;
}

// Тарифная метка использует сохранённый scalar, а не сегодняшнюю типовую скидку. B2C остаётся
// flat-тарифом без тир-лестницы, но в базе есть легитимные dormant 4000-bp строки: оператор должен
// видеть их как −60%, а не получать ложное −50%. NULL не превращаем в придуманную цену.
export function tierLabel(user: Pick<AdminUser, "customer_type" | "multiplier_bp">): string {
  const segment = user.customer_type === "b2b" ? "B2B" : "B2C";
  return user.multiplier_bp == null ? segment : `${segment} −${100 - user.multiplier_bp / 100}%`;
}

export interface UserProviderRail {
  id: UserProviderId;
  label: string;
  className: string;
  amountNano: string | null;
  available: boolean;
  shareBp: number;
}

function providerNano(value: string | undefined): bigint | null {
  if (value === undefined || !/^\d+$/.test(value)) return null;
  try {
    return BigInt(value);
  } catch {
    return null;
  }
}

/**
 * Five stable scan-lines for one user. Width is relative to that user's largest provider, not a
 * share of total spend: a tiny secondary rail remains visible without turning money into floats.
 */
export function userProviderRails(spend: AdminUserProviderSpend | undefined): UserProviderRail[] {
  const parsed = USER_PROVIDER_RAILS.map((provider) => {
    const amount = providerNano(spend?.[`${provider.id}_nano`]);
    return { ...provider, amount };
  });
  const maximum = parsed.reduce(
    (max, provider) => provider.amount !== null && provider.amount > max ? provider.amount : max,
    0n,
  );
  return parsed.map((provider) => ({
    id: provider.id,
    label: provider.label,
    className: provider.className,
    amountNano: provider.amount?.toString() ?? null,
    available: provider.amount !== null,
    shareBp: provider.amount !== null && provider.amount > 0n && maximum > 0n
      ? Number((provider.amount * 10_000n) / maximum)
      : 0,
  }));
}

/** Screenshot-style money: whole/cent precision above $1, four decimals below $1. */
export function formatUserProviderNano(value: string | null): string {
  const amount = value === null ? null : providerNano(value);
  if (amount === null) return "—";
  const whole = amount / 1_000_000_000n;
  const remainder = amount % 1_000_000_000n;
  if (whole > 0n) {
    const cents = remainder / 10_000_000n;
    const fraction = remainder === 0n ? "" : `.${cents.toString().padStart(2, "0")}`;
    return `$${whole.toLocaleString("en-US")}${fraction}`;
  }
  if (amount > 0n && amount < 100_000n) return "<$0.0001";
  const tenThousandths = amount / 100_000n;
  return `$0.${tenThousandths.toString().padStart(4, "0")}`;
}

// CSV текущей загруженной страницы: легаси-деньги остаются сырыми USD, новые provider-поля —
// точными nanoUSD text, даты — ISO, чтобы файл пригодился для сверки, а не только просмотра.
export const USERS_CSV_HEADER = [
  "email",
  "имя",
  "статус",
  "тариф",
  "баланс_usd",
  "потрачено_usd",
  "расход_30д_usd",
  "claude_30д_nanoUSD_text",
  "gpt_30д_nanoUSD_text",
  "gemini_30д_nanoUSD_text",
  "kimi_30д_nanoUSD_text",
  "другие_30д_nanoUSD_text",
  "пополнено_всего_usd",
  "оплачено_usd",
  "платежей",
  "ключи_активные",
  "ключи_всего",
  "последняя_активность",
  "регистрация",
];

export function buildUsersCsvRows(users: AdminUser[]): unknown[][] {
  return users.map((user) => {
    const pay = user.payments ?? {};
    const keys = user.api_keys ?? {};
    return [
      user.email,
      user.display_name || "",
      user.status,
      tierLabel(user),
      user.balance_usd ?? "",
      user.spent_usd ?? "",
      user.spent_30d_usd ?? "",
      ...USER_PROVIDER_RAILS.map((provider) => {
        const value = user.provider_spend_30d?.[`${provider.id}_nano`];
        return value === undefined ? "" : spreadsheetExactInteger(value);
      }),
      user.cumulative_topup_usd ?? "",
      pay.paid_total_usd ?? "",
      pay.paid_count ?? "",
      Number(keys.active || 0),
      Number(keys.total || 0),
      user.last_seen_at || "",
      user.created_at || "",
    ];
  });
}
