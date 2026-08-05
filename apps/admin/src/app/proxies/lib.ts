export type ProxyLiveness = "live" | "degraded" | "dead" | "unknown";
export type ProxyBinding = "bound" | "unbound" | "mismatch" | "unknown";
export type ProxyRenewItemStatus = "renewed" | "failed" | "uncertain";
export type ProxyRenewStatus = "succeeded" | "partial" | "failed" | "uncertain";

export interface ProxyProviderBalance {
  provider: string;
  balance_nano_usd: string | null;
  balance_observed_at: number | null;
  auto_extend_enabled: boolean;
}

export interface ProxyInventoryItem {
  inventory_id: string;
  proxy_hint: string;
  order_hint: string;
  provider: string;
  subscription_plan: string;
  liveness: ProxyLiveness;
  subscription_expires_at: number | null;
  proxy_expires_at: number | null;
  binding_status: ProxyBinding;
  renewable: boolean;
  renew_block_code: string | null;
}

export interface ProxyInventoryResponse {
  schema_version: number;
  observed_at: number | null;
  providers: ProxyProviderBalance[];
  items: ProxyInventoryItem[];
}

export interface ProxyRenewRequest {
  idempotency_key: string;
  inventory_ids: string[];
}

export interface ProxyRenewResult {
  inventory_id: string;
  status: ProxyRenewItemStatus;
  proxy_expires_at: number | null;
  result_code: string | null;
}

export interface ProxyRenewResponse {
  schema_version: number;
  idempotency_key: string;
  idempotent_replay: boolean;
  status: ProxyRenewStatus;
  observed_at: number | null;
  results: ProxyRenewResult[];
}

export interface ProxyFilters {
  query: string;
  provider: string;
  plan: string;
  liveness: string;
  binding: string;
}

const FORBIDDEN_KEYS = new Set([
  "credential",
  "credentials",
  "email",
  "full_identity",
  "ip",
  "password",
  "proxy_host",
  "proxy_url",
  "secret",
  "subject",
  "token",
  "username",
]);
const INTEGER = /^-?(0|[1-9]\d*)$/;
const LIVENESS = new Set<ProxyLiveness>(["live", "degraded", "dead", "unknown"]);
const BINDINGS = new Set<ProxyBinding>(["bound", "unbound", "mismatch", "unknown"]);
const RENEW_ITEM_STATUSES = new Set<ProxyRenewItemStatus>(["renewed", "failed", "uncertain"]);
const RENEW_STATUSES = new Set<ProxyRenewStatus>(["succeeded", "partial", "failed", "uncertain"]);

function record(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${label}: некорректный ответ`);
  return value as Record<string, unknown>;
}

function safeString(value: unknown, fallback = "—"): string {
  if (typeof value !== "string") return fallback;
  const text = value.trim();
  return text && text.length <= 160 ? text : fallback;
}

function nullableEpoch(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0 ? value : null;
}

function assertNoSecrets(value: unknown): void {
  if (!value || typeof value !== "object") return;
  if (Array.isArray(value)) {
    for (const item of value) assertNoSecrets(item);
    return;
  }
  for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
    if (FORBIDDEN_KEYS.has(key.toLowerCase())) throw new Error("Ответ реестра содержит запрещённые приватные поля");
    assertNoSecrets(child);
  }
}

export function projectProxyInventory(payload: unknown): ProxyInventoryResponse {
  assertNoSecrets(payload);
  const root = record(payload, "Реестр прокси");
  const providers = Array.isArray(root.providers) ? root.providers : [];
  const items = Array.isArray(root.items) ? root.items : [];
  return {
    schema_version: typeof root.schema_version === "number" ? root.schema_version : 1,
    observed_at: nullableEpoch(root.observed_at),
    providers: providers.map((raw) => {
      const item = record(raw, "Баланс провайдера");
      const balance = item.balance_nano_usd;
      return {
        provider: safeString(item.provider, "unknown"),
        balance_nano_usd: typeof balance === "string" && INTEGER.test(balance) ? balance : null,
        balance_observed_at: nullableEpoch(item.balance_observed_at),
        auto_extend_enabled: item.auto_extend_enabled === true,
      };
    }),
    items: items.map((raw) => {
      const item = record(raw, "Прокси");
      const inventoryId = safeString(item.inventory_id, "");
      if (!inventoryId) throw new Error("Прокси: отсутствует opaque inventory_id");
      const liveness = safeString(item.liveness, "unknown") as ProxyLiveness;
      const binding = safeString(item.binding_status, "unknown") as ProxyBinding;
      return {
        inventory_id: inventoryId,
        proxy_hint: safeString(item.proxy_hint),
        order_hint: safeString(item.order_hint),
        provider: safeString(item.provider, "unknown"),
        subscription_plan: safeString(item.subscription_plan),
        liveness: LIVENESS.has(liveness) ? liveness : "unknown",
        subscription_expires_at: nullableEpoch(item.subscription_expires_at),
        proxy_expires_at: nullableEpoch(item.proxy_expires_at),
        binding_status: BINDINGS.has(binding) ? binding : "unknown",
        renewable: item.renewable === true,
        renew_block_code: typeof item.renew_block_code === "string" ? safeString(item.renew_block_code, "") || null : null,
      };
    }),
  };
}

export function projectProxyRenew(payload: unknown): ProxyRenewResponse {
  assertNoSecrets(payload);
  const root = record(payload, "Продление прокси");
  const status = safeString(root.status, "uncertain") as ProxyRenewStatus;
  const results = Array.isArray(root.results) ? root.results : [];
  return {
    schema_version: typeof root.schema_version === "number" ? root.schema_version : 1,
    idempotency_key: safeString(root.idempotency_key, ""),
    idempotent_replay: root.idempotent_replay === true,
    status: RENEW_STATUSES.has(status) ? status : "uncertain",
    observed_at: nullableEpoch(root.observed_at),
    results: results.map((raw) => {
      const item = record(raw, "Результат продления");
      const itemStatus = safeString(item.status, "uncertain") as ProxyRenewItemStatus;
      return {
        inventory_id: safeString(item.inventory_id, ""),
        status: RENEW_ITEM_STATUSES.has(itemStatus) ? itemStatus : "uncertain",
        proxy_expires_at: nullableEpoch(item.proxy_expires_at),
        result_code: typeof item.result_code === "string" ? safeString(item.result_code, "") || null : null,
      };
    }),
  };
}

export function filterProxyInventory(items: ProxyInventoryItem[], filters: ProxyFilters): ProxyInventoryItem[] {
  const query = filters.query.trim().toLocaleLowerCase("ru-RU");
  return items.filter((item) => {
    if (filters.provider && item.provider !== filters.provider) return false;
    if (filters.plan && item.subscription_plan !== filters.plan) return false;
    if (filters.liveness && item.liveness !== filters.liveness) return false;
    if (filters.binding && item.binding_status !== filters.binding) return false;
    if (!query) return true;
    return [item.proxy_hint, item.order_hint, item.provider, item.subscription_plan]
      .some((value) => value.toLocaleLowerCase("ru-RU").includes(query));
  });
}

export function selectableProxyIds(items: ProxyInventoryItem[]): string[] {
  return items.filter((item) => item.renewable).map((item) => item.inventory_id).sort();
}

export function createProxyRenewRequest(ids: Iterable<string>, idempotencyKey: string): ProxyRenewRequest {
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(idempotencyKey)) {
    throw new Error("idempotency_key должен быть UUID");
  }
  const inventoryIds = [...new Set(ids)].sort();
  if (!inventoryIds.length || inventoryIds.length > 100 || inventoryIds.some((id) => !id || id.length > 160)) {
    throw new Error("Для продления нужно от 1 до 100 opaque inventory IDs");
  }
  return { idempotency_key: idempotencyKey, inventory_ids: inventoryIds };
}

export function proxyRenewSummary(response: ProxyRenewResponse): string {
  const renewed = response.results.filter((item) => item.status === "renewed").length;
  const failed = response.results.filter((item) => item.status === "failed").length;
  const uncertain = response.results.filter((item) => item.status === "uncertain").length;
  return `Продлено: ${renewed}. Ошибки: ${failed}. Неопределённо: ${uncertain}.`;
}
