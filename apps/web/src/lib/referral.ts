// Партнёрская атрибуция (sales.apitoken.sale): код из ?ref=CODE запоминается на 30 дней,
// чтобы регистрация позже (или с другой страницы) всё ещё привязалась к партнёру.
// Первый увиденный код побеждает — не перезаписываем существующий.

const STORAGE_KEY = "apitoken_ref";
const TTL_MS = 30 * 24 * 60 * 60 * 1000;
const CODE_PATTERN = /^[A-Za-z0-9_-]{3,32}$/;

export function captureReferralCode(codeFromUrl: string | null): void {
  if (typeof window === "undefined") return;
  if (!codeFromUrl || !CODE_PATTERN.test(codeFromUrl)) return;
  try {
    if (readStored()) return;
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify({ code: codeFromUrl, at: Date.now() }));
  } catch {
    // localStorage may be unavailable (private mode) — attribution is best-effort
  }
}

export function storedReferralCode(): string | undefined {
  if (typeof window === "undefined") return undefined;
  return readStored();
}

function readStored(): string | undefined {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return undefined;
    const parsed = JSON.parse(raw) as { code?: unknown; at?: unknown };
    if (typeof parsed.code !== "string" || typeof parsed.at !== "number") return undefined;
    if (!CODE_PATTERN.test(parsed.code) || Date.now() - parsed.at > TTL_MS) {
      window.localStorage.removeItem(STORAGE_KEY);
      return undefined;
    }
    return parsed.code;
  } catch {
    return undefined;
  }
}
