// Форматирование денег и токенов повторяет дашборд один в один: те же округления,
// те же пороги «<$0.01» и та же шкала оси, иначе одни и те же данные выглядели бы
// в двух наших интерфейсах по-разному.

export const NANO_PER_USD = 1_000_000_000n;

export const MODEL_COLORS = ["#3767f0", "#7c5cff", "#12a594", "#e0913a", "#d6455d", "#8b8f9a"];

export function absoluteBigInt(value: bigint): bigint {
  return value < 0n ? -value : value;
}

export function bigintMax(left: bigint, right: bigint): bigint {
  return left > right ? left : right;
}

export function compareBigInt(left: bigint, right: bigint): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

export function roundDivide(numerator: bigint, denominator: bigint): bigint {
  if (denominator <= 0n) throw new Error("denominator must be positive");
  const negative = numerator < 0n;
  const absolute = negative ? -numerator : numerator;
  const rounded = (absolute + denominator / 2n) / denominator;
  return negative ? -rounded : rounded;
}

export function formatNanoUsd(
  value: string | bigint,
  minimumFractionDigits = 0,
  maximumFractionDigits = 2,
): string {
  const nano = typeof value === "bigint" ? value : BigInt(value);
  const negative = nano < 0n;
  const absolute = negative ? -nano : nano;
  const digits = Math.max(0, Math.min(9, maximumFractionDigits));
  const minimum = Math.max(0, Math.min(digits, minimumFractionDigits));
  const quantum = 10n ** BigInt(9 - digits);
  const scaled = (absolute + quantum / 2n) / quantum;
  const units = 10n ** BigInt(digits);
  const whole = scaled / units;
  let fraction = digits > 0 ? (scaled % units).toString().padStart(digits, "0") : "";
  while (fraction.length > minimum && fraction.endsWith("0")) fraction = fraction.slice(0, -1);
  return `${negative ? "-" : ""}$${whole.toLocaleString("en-US")}${fraction ? `.${fraction}` : ""}`;
}

export function formatNanoUsdSmart(value: bigint): string {
  if (value === 0n) return "$0.00";
  if (absoluteBigInt(value) >= 10_000_000n) return formatNanoUsd(value, 2, 2);
  return formatNanoUsd(value, 0, 9);
}

export function fmtNanoUsd(nano: string): string {
  const value = BigInt(nano);
  if (value > 0n && value < 10_000_000n) return "<$0.01";
  return formatNanoUsd(value, 2, 2);
}

export function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toLocaleString("en-US", { maximumFractionDigits: 2 })}M`;
  if (n >= 1_000) return `${(n / 1_000).toLocaleString("en-US", { maximumFractionDigits: 1 })}K`;
  return n.toLocaleString("en-US");
}

export function fmtUtcDay(ms: number, locale: string): string {
  return new Date(ms).toLocaleDateString(locale, { month: "numeric", day: "numeric", timeZone: "UTC" });
}

/** «Красивая» шкала оси Y на целых нано-USD; в number переводятся только ограниченные отношения. */
export function niceNanoScale(max: bigint): { max: bigint; step: bigint; divisions: number } {
  const divisions = 4;
  if (max <= 0n) return { max: NANO_PER_USD, step: NANO_PER_USD / 4n, divisions };
  const rough = (max + BigInt(divisions) - 1n) / BigInt(divisions);
  const magnitude = 10n ** BigInt(Math.max(0, rough.toString().length - 1));
  const candidates = [magnitude, 2n * magnitude, 5n * magnitude, 10n * magnitude];
  const step = candidates.find((candidate) => candidate >= rough) ?? 10n * magnitude;
  return { max: step * BigInt(divisions), step, divisions };
}

export function formatAxisNanoUsd(value: bigint): string {
  if (value <= 0n) return "$0";
  if (value >= NANO_PER_USD) return formatNanoUsd(value, 0, 1);
  if (value >= 10_000_000n) return formatNanoUsd(value, 0, 2);
  if (value >= 100_000n) return formatNanoUsd(value, 0, 4);
  return formatNanoUsd(value, 0, 9);
}

export function boundedRatio(numerator: bigint, denominator: bigint): number {
  if (denominator <= 0n || numerator <= 0n) return 0;
  const scale = 1_000_000n;
  const bounded = bigintMax(0n, numerator > denominator ? denominator : numerator);
  return Number((bounded * scale) / denominator) / Number(scale);
}

export function boundedPercent(numerator: bigint, denominator: bigint): number {
  return boundedRatio(numerator, denominator) * 100;
}

export function modelLabel(id: string, provider?: string): string {
  const gpt = id.match(/^gpt-(.+)$/i);
  if (gpt) {
    const parts = gpt[1]!.split("-");
    const version = parts.shift() ?? "";
    const suffix = parts.map((part) => part ? part[0]!.toUpperCase() + part.slice(1) : "").join(" ");
    return `GPT-${version}${suffix ? ` ${suffix}` : ""}`;
  }

  const isClaude = /^claude-/i.test(id) || (!/^(?:gemini)-/i.test(id) && provider === "anthropic");
  const base = id.replace(/^claude-/i, "").replace(/-\d{8}$/, "");
  if (!isClaude) {
    const parts: string[] = [];
    for (const part of base.split("-").filter(Boolean)) {
      if (/^\d+$/.test(part) && /^\d+(?:\.\d+)*$/.test(parts.at(-1) ?? "")) {
        parts[parts.length - 1] = `${parts.at(-1)}.${part}`;
      } else {
        parts.push(/^\d+(?:\.\d+)*$/.test(part) ? part : part[0]!.toUpperCase() + part.slice(1));
      }
    }
    return parts.join(" ");
  }

  const words: string[] = [];
  const nums: string[] = [];
  for (const part of base.split("-")) {
    if (/^\d+$/.test(part)) nums.push(part);
    else if (part) words.push(part[0]!.toUpperCase() + part.slice(1));
  }
  return `Claude ${words.join(" ")}${nums.length ? ` ${nums.join(".")}` : ""}`.trim();
}

export function formatEffectiveDiscount(officialNano: bigint, chargedNano: bigint): string {
  if (officialNano <= 0n) return "—";
  const discountNano = officialNano > chargedNano ? officialNano - chargedNano : 0n;
  const tenths = roundDivide(discountNano * 1_000n, officialNano);
  return `${tenths / 10n}.${tenths % 10n}%`;
}

export function usageWindowDays(sinceTs: number, untilTs: number): number {
  const days = Math.round((untilTs - sinceTs) / 86_400);
  return days > 0 ? days : 1;
}
