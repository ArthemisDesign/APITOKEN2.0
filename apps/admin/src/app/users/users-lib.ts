// Типы и чистая логика страницы «Пользователи» — порт соответствующих кусков
// users()/renderUsers()/usersCsv() из crates/server/src/admin-panel.js (строки 637-730).
// Вынесено из page.tsx, чтобы юнит-тесты не тащили React-рендер.

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

// Тарифная метка: B2B по customer_type; B2C — единый flat-тариф (после релиз-катовера
// 2026-08-04 тир-лестницы нет, цена — глобальные −50% плюс overrides из активного release).
export function tierLabel(user: Pick<AdminUser, "customer_type">): string {
  return user.customer_type === "b2b" ? "B2B" : "B2C −50%";
}

// CSV текущей загруженной страницы: колонки повторяют таблицу, деньги — сырыми
// числами USD, даты — ISO, чтобы файл пригодился для сверки, а не только для просмотра.
export const USERS_CSV_HEADER = [
  "email",
  "имя",
  "статус",
  "тариф",
  "баланс_usd",
  "потрачено_usd",
  "расход_30д_usd",
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
