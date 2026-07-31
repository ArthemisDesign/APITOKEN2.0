// Чистая логика страницы «Аудит» — порт 1:1 секции audit() из
// crates/server/src/admin-panel.js (строки 1026-1058). Вынесена из page.tsx,
// чтобы покрываться юнит-тестами без React-окружения.

export interface AuditEntry {
  created_at?: string;
  action?: string;
  actor_type?: string;
  actor_id?: string | null;
  target_type?: string;
  target_id?: string;
  metadata?: Record<string, unknown> | null;
}

export interface AuditPagePayload {
  rows?: AuditEntry[];
  total?: number;
}

export interface AuditFilters {
  offset: number;
  limit: number;
  action: string;
  actorType: string;
  q: string;
}

export const INITIAL_AUDIT_FILTERS: AuditFilters = { offset: 0, limit: 50, action: "", actorType: "", q: "" };

// actor_type в audit_log по коду коммерции (packages/db): операторы пишут commercial-admin,
// operator — старое имя того же типа, оставлено для исторических строк.
export const AUDIT_ACTORS = ["commercial-admin", "user", "provider", "operator"] as const;

// Query-строка запроса /admin/audit: пустые фильтры не отправляются.
export function buildAuditQuery(filters: AuditFilters): string {
  const params = new URLSearchParams({ limit: String(filters.limit), offset: String(filters.offset) });
  if (filters.action) params.set("action", filters.action);
  if (filters.actorType) params.set("actor_type", filters.actorType);
  if (filters.q) params.set("q", filters.q);
  return params.toString();
}

// /admin/audit/actions: новый backend отдаёт {actions:[...]}, старый — голый массив;
// любой другой ответ → пустой список (выпадайка просто без опций).
export function normalizeAuditActions(payload: unknown): string[] {
  if (Array.isArray(payload)) return payload.filter((a): a is string => typeof a === "string");
  if (payload && typeof payload === "object" && Array.isArray((payload as { actions?: unknown }).actions)) {
    return ((payload as { actions: unknown[] }).actions).filter((a): a is string => typeof a === "string");
  }
  return [];
}

// Старый backend отдаёт {rows} без total — деградируем к размеру страницы
// (пагинация при этом скрывается: «Дальше» выключается на первой же странице).
export function auditPageTotal(payload: AuditPagePayload | null): number {
  const rows = payload?.rows ?? [];
  return payload?.total ?? rows.length;
}

// Если offset уехал за хвост лога (события удались/фильтр сузился под другим
// оператором) — прыгаем на последнюю полную страницу, как легаси (`return audit()`).
export function clampAuditOffset(offset: number, limit: number, total: number): number {
  if (offset < total || total <= 0) return offset;
  return Math.max(0, Math.floor((total - 1) / limit) * limit);
}

// Опции выпадайки действий: выбранный action мог прийти из прошлого фильтра
// до загрузки списка — показываем его опцией первой.
export function auditActionOptions(actions: readonly string[], selected: string): string[] {
  const options = actions.slice();
  if (selected && !options.includes(selected)) options.unshift(selected);
  return options;
}

// Строки CSV выгрузки текущей страницы (кнопка audit-csv в легаси).
export function auditCsvRows(rows: AuditEntry[]): unknown[][] {
  return rows.map((item) => [
    item.created_at || "",
    item.action,
    item.actor_type,
    item.actor_id || "",
    item.target_type,
    item.target_id,
    JSON.stringify(item.metadata || {}),
  ]);
}

export const AUDIT_CSV_HEADER = ["время", "действие", "актор_тип", "актор_id", "цель_тип", "цель_id", "metadata"];
