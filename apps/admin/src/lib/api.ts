// Типизированный fetch-клиент для same-origin JSON API.
// Браузер ходит по относительным путям (/overview, /admin/*, /openkeys-admin/*,
// /partner-admin/*); аутентификацию и серверные ключи внедряет Caddy — приложение
// секретов не имеет. Поведение повторяет rawApi/api/send из admin-panel.js.

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
export function send<T>(path: string, method: ApiOptions["method"], body: unknown): Promise<T> {
  return api<T>(path, { method, body });
}
