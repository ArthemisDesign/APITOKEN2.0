// Типизированный fetch-клиент для same-origin JSON API.
// Браузер ходит по относительным путям (/overview, /admin/*, /openkeys-admin/* и
// fenced /partner-admin/payouts/*); аутентификацию и серверные ключи внедряет Caddy — приложение
// секретов не имеет. Поведение повторяет rawApi/api/send из admin-panel.js.
import { publishInvalidation } from "@/lib/invalidation";

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

export type ApiOptions = {
  method?: "GET" | "POST" | "PATCH" | "PUT" | "DELETE";
  body?: unknown;
  headers?: Record<string, string>;
  signal?: AbortSignal;
};

// Вытаскивает человекочитаемое сообщение из тела ошибки (как в admin-panel.js):
// message может быть строкой или массивом строк, fallback — error, затем HTTP-код.
export function apiErrorMessage(payload: unknown, status: number): string {
  if (payload && typeof payload === "object") {
    const record = payload as { message?: unknown; error?: unknown };
    if (Array.isArray(record.message)) {
      const joined = record.message.filter((m): m is string => typeof m === "string").join(", ");
      if (joined) return joined;
    } else if (typeof record.message === "string" && record.message) {
      return record.message;
    }
    if (typeof record.error === "string" && record.error) return record.error;
  }
  return `HTTP ${status}`;
}

export async function api<T>(path: string, options: ApiOptions = {}): Promise<T> {
  const response = await fetch(path, {
    method: options.method ?? "GET",
    // Admin projections are operational state, not static assets. Revalidation must reach the
    // same-origin producer instead of reusing a browser HTTP-cache entry.
    cache: "no-store",
    headers: { "content-type": "application/json", ...(options.headers ?? {}) },
    body: options.body !== undefined ? JSON.stringify(options.body) : undefined,
    signal: options.signal,
  });
  const payload: unknown = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new ApiError(response.status, apiErrorMessage(payload, response.status), payload);
  }
  return payload as T;
}

// POST/PATCH с JSON-телом — аналог send() из admin-panel.js.
export async function send<T>(path: string, method: ApiOptions["method"], body: unknown): Promise<T> {
  const result = await api<T>(path, { method, body });
  publishInvalidation(mutationResources(path));
  return result;
}

export function mutationResources(path: string): string[] {
  const clean = path.split("?", 1)[0] ?? path;
  if (clean.endsWith("/referral-partner") && clean.startsWith("/admin/users/")) {
    return ["/admin/users", "/admin/referral/partners"];
  }
  if (clean.startsWith("/admin/users/")) return ["/admin/users"];
  if (clean.startsWith("/admin/admin-accounts/")) return ["/admin/admin-accounts"];
  if (clean.startsWith("/admin/business-invites/")) return ["/admin/business-invites"];
  if (clean.startsWith("/admin/business-users/")) return ["/admin/business-users", "/admin/users"];
  if (clean === "/openkeys-admin/keys" || clean.startsWith("/openkeys-admin/keys/")) {
    return ["/openkeys-admin/keys", "/openkeys-admin/sellers", "/openkeys-admin/paying-keys", "/openkeys-admin/lookup"];
  }
  if (clean === "/openkeys-admin/sellers" || clean.startsWith("/openkeys-admin/sellers/")) {
    return ["/openkeys-admin/sellers", "/openkeys-admin/keys", "/openkeys-admin/paying-keys", "/openkeys-admin/lookup"];
  }
  if (clean.startsWith("/proxy-admin/")) return ["/proxy-admin/inventory"];
  if (clean.startsWith("/gemini-subs/")) return ["/gemini-subs"];
  if (clean === "/admin/referral/partners") return ["/admin/referral/partners"];
  if (clean.startsWith("/admin/referral/requests/")) return ["/admin/referral/requests"];
  if (clean.startsWith("/admin/referral/payouts/")) return ["/admin/referral/payouts"];
  if (clean.startsWith("/partner-admin/payouts/")) return ["/partner-admin/payouts", "/partner-admin/payouts/batches", "/partner-admin/payouts/engine", "/partner-admin/payout-list"];
  return [clean];
}
