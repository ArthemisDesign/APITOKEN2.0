const NANO_PER_USD = 1_000_000_000n;

export function providerInteger(value: string | number | bigint | null | undefined): bigint | null {
  if (value == null || !/^(0|[1-9][0-9]*)$/.test(String(value))) return null;
  try {
    return BigInt(value);
  } catch {
    return null;
  }
}

export function tokensForNanoCapacity(
  capacityNano: string | bigint | null | undefined,
  nanousdPerToken: string | bigint | null | undefined,
): bigint | null {
  const capacity = providerInteger(capacityNano);
  const rate = providerInteger(nanousdPerToken);
  if (capacity == null || rate == null || rate <= 0n) return null;
  return capacity / rate;
}

export function compactTokenCount(value: bigint | null): string {
  if (value == null) return "—";
  const units: Array<[bigint, string]> = [
    [1_000_000_000_000n, "T"],
    [1_000_000_000n, "B"],
    [1_000_000n, "M"],
    [1_000n, "K"],
  ];
  const unit = units.find(([size]) => value >= size);
  if (!unit) return value.toString();
  const [size, suffix] = unit;
  const tenths = (value * 10n + size / 2n) / size;
  const whole = tenths / 10n;
  const fraction = tenths % 10n;
  return `${whole}${fraction ? `.${fraction}` : ""}${suffix}`;
}

export function exactTokenCount(value: bigint | null): string {
  return value == null ? "—" : value.toLocaleString("en-US");
}

/** A catalogue rate is nanodollars/token, numerically equal to $/M tokens divided by 1,000. */
export function formatUsdPerMillion(rateValue: string | bigint | null | undefined): string {
  const rate = providerInteger(rateValue);
  if (rate == null) return "—";
  const whole = rate / 1_000n;
  let fraction = (rate % 1_000n).toString().padStart(3, "0");
  while (fraction.length > 2 && fraction.endsWith("0")) fraction = fraction.slice(0, -1);
  return `$${whole}.${fraction}`;
}

export function formatUsdPerUnit(nanoValue: string | bigint | null | undefined): string {
  const nano = providerInteger(nanoValue);
  if (nano == null) return "—";
  const whole = nano / NANO_PER_USD;
  const fraction = (nano % NANO_PER_USD)
    .toString()
    .padStart(9, "0")
    .replace(/0+$/, "")
    .padEnd(2, "0");
  return `$${whole}.${fraction}`;
}

export function compareProviderRates(
  left: string | bigint | null | undefined,
  right: string | bigint | null | undefined,
): number {
  const a = providerInteger(left) ?? -1n;
  const b = providerInteger(right) ?? -1n;
  return a === b ? 0 : a > b ? 1 : -1;
}

export function usedPercentFromNano(
  capacityValue: string | bigint | null | undefined,
  remainingValue: string | bigint | null | undefined,
): { value: number | null; label: string } {
  const capacity = providerInteger(capacityValue);
  const remaining = providerInteger(remainingValue);
  if (capacity == null || remaining == null || capacity <= 0n) return { value: null, label: "—" };
  const used = capacity > remaining ? capacity - remaining : 0n;
  const tenths = (used * 1_000n + capacity / 2n) / capacity;
  return {
    value: Number(tenths) / 10,
    label: `${tenths / 10n}${tenths % 10n ? `.${tenths % 10n}` : ""}%`,
  };
}
