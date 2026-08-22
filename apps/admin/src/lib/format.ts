// Форматтеры, портированные 1:1 из crates/server/src/admin-panel.js.
// Деньги: nanoUSD — целочисленные строки (BigInt), float-математика над суммами запрещена.

const NANO = 1_000_000_000n;

// nanoMoney: целочисленная nanoUSD-строка → "$1,234.56" (обрезка до центов, минус — "−").
// Невалидный ввод → "$0.00", как в оригинале.
export function nanoMoney(value: string | number | bigint | null | undefined): string {
  try {
    let n = BigInt(String(value ?? "0"));
    const neg = n < 0n;
    if (neg) n = -n;
    const whole = n / NANO;
    const cents = (n % NANO) / 10_000_000n;
    return `${neg ? "−" : ""}$${whole.toLocaleString("en-US")}.${cents.toString().padStart(2, "0")}`;
  } catch {
    return "$0.00";
  }
}

// nanoCredits: native ChatGPT quota in 10^-9 credits. Keep up to six fractional digits so the
// first successful turn remains visible instead of rounding to zero. This is deliberately not a
// dollar formatter: credits are the stable unit for comparing equal subscriptions.
export function nanoCredits(value: string | bigint | null | undefined): string {
  try {
    let n = BigInt(String(value ?? "0"));
    const neg = n < 0n;
    if (neg) n = -n;
    if (n > 0n && n < 1_000n) return `${neg ? "−" : ""}<0.000001 credits`;
    const whole = n / NANO;
    const fraction = ((n % NANO) / 1_000n).toString().padStart(6, "0").replace(/0+$/, "");
    return `${neg ? "−" : ""}${whole.toLocaleString("en-US")}${fraction ? `.${fraction}` : ""} credits`;
  } catch {
    return "0 credits";
  }
}

// money: форматирование ЛЕГАСИ-полей коммерции, которые API отдаёт уже в долларах
// (paid_usd, balance_usd и т.п.). Только для отображения; никакой арифметики над ними.
export function money(value: number | string | null | undefined): string {
  return (
    "$" +
    Number(value || 0).toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })
  );
}

// date: "01.02.2026" или с временем "01.02.2026, 15:04"; пустое значение → тире.
export function formatDate(
  value: string | number | Date | null | undefined,
  withTime = false,
  locale = "ru-RU",
): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return new Intl.DateTimeFormat(
    locale,
    withTime ? { dateStyle: "short", timeStyle: "short" } : { dateStyle: "short" },
  ).format(date);
}

// ago: "сейчас" | "5м" | "3ч" | "2д" — возраст метки времени.
export function ago(value: string | number | Date | null | undefined, locale = "ru-RU"): string {
  if (!value) return "—";
  const timestamp = new Date(value).getTime();
  if (Number.isNaN(timestamp)) return "—";
  const seconds = Math.max(0, ((Date.now() - timestamp) / 1000) | 0);
  const ru = locale.toLowerCase().startsWith("ru");
  if (seconds < 60) return ru ? "сейчас" : "now";
  if (seconds < 3600) return `${(seconds / 60) | 0}${ru ? "м" : "m"}`;
  if (seconds < 86400) return `${(seconds / 3600) | 0}${ru ? "ч" : "h"}`;
  return `${(seconds / 86400) | 0}${ru ? "д" : "d"}`;
}

// duration: секунды → "2д 3ч" | "5ч 12м" | "7м".
export function duration(seconds: number | null | undefined): string {
  const s = Math.max(0, Number(seconds) || 0);
  const days = (s / 86400) | 0;
  const hours = ((s % 86400) / 3600) | 0;
  const minutes = ((s % 3600) / 60) | 0;
  return days ? `${days}д ${hours}ч` : hours ? `${hours}ч ${minutes}м` : `${minutes}м`;
}

// ageText: возраст по готовым секундам из *_age_seconds ответов бэкендов; null → тире.
export function ageText(seconds: number | null | undefined): string {
  return seconds == null ? "—" : duration(seconds);
}

// ratio: "×2.5" (<10 — один знак после запятой, иначе целое); null/undefined → "∞".
export function ratio(value: number | null | undefined): string {
  if (value == null) return "∞";
  const n = Number(value);
  return "×" + n.toLocaleString("en-US", { maximumFractionDigits: n < 10 ? 1 : 0 });
}

// Русская плюрализация: plural(1, "подписка", "подписки", "подписок").
export function plural(n: number, one: string, few: string, many: string): string {
  const a = Math.abs(n) % 100;
  const b = a % 10;
  if (a > 10 && a < 20) return many;
  if (b > 1 && b < 5) return few;
  if (b === 1) return one;
  return many;
}

// count: "3 подписки".
export function count(n: number, one: string, few: string, many: string): string {
  return `${n} ${plural(n, one, few, many)}`;
}

// windowLabel: минуты → "15 мин" | "2 ч" | "3 д"; 0 → "окно".
export function windowLabel(minutes: number | null | undefined): string {
  const m = Number(minutes) || 0;
  if (!m) return "окно";
  if (m < 60) return `${m} мин`;
  if (m < 1440) return `${Math.round(m / 60)} ч`;
  return `${Math.round(m / 1440)} д`;
}
