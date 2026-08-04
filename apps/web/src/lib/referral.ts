// Партнёрская атрибуция (sales.apitoken.sale): код из ?ref=CODE запоминается на 30 дней,
// чтобы регистрация позже (или с другой страницы) всё ещё привязалась к партнёру.
//
// LAST-CLICK WINS: свежая ?ref= в URL — это ЯВНОЕ намерение пользователя воспользоваться именно
// ЭТОЙ ссылкой (особенно персональной скидочной). Раньше был first-wins — и застрявший в
// localStorage старый код 30 дней воровал атрибуцию у ссылки, по которой человек реально пришёл
// (типичный баг: кликнул новую 91%-ссылку, а привязался к старой уже-погашенной → обычный b2c).
// Поэтому всегда перезаписываем последним кликнутым кодом.

const STORAGE_KEY = "apitoken_ref";
const TTL_MS = 30 * 24 * 60 * 60 * 1000;
const CODE_PATTERN = /^[A-Za-z0-9_-]{3,32}$/;

// Runs in <head> before any visible link can be followed. RefCapture repeats the same operation
// after hydration for client-side navigations and analytics.
export const referralBootstrapScript = `(()=>{try{const code=new URLSearchParams(location.search).get("ref");if(!code||!${CODE_PATTERN}.test(code))return;const raw=localStorage.getItem("${STORAGE_KEY}");if(raw){try{const stored=JSON.parse(raw);if(stored&&stored.code===code&&typeof stored.at==="number"&&Date.now()-stored.at<=${TTL_MS})return}catch{}}localStorage.setItem("${STORAGE_KEY}",JSON.stringify({code,at:Date.now()}))}catch{}})()`;

export function isReferralCode(value: string | null): value is string {
  return Boolean(value && CODE_PATTERN.test(value));
}

export function captureReferralCode(codeFromUrl: string | null): void {
  if (typeof window === "undefined") return;
  if (!isReferralCode(codeFromUrl)) return;
  try {
    // Тот же код повторно — не трогаем (не сдвигаем окно без нужды); иначе перезаписываем.
    if (readStored() === codeFromUrl) return;
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
