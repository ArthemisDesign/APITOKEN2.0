export function nanoToUsd(nano: string, maximumFractionDigits = 2): string {
  const value = BigInt(nano);
  const negative = value < 0n;
  const absolute = negative ? -value : value;
  const whole = absolute / 1_000_000_000n;
  const fraction = (absolute % 1_000_000_000n).toString().padStart(9, "0")
    .slice(0, maximumFractionDigits).replace(/0+$/, "");
  return `${negative ? "-" : ""}$${whole.toLocaleString("en-US")}${fraction ? `.${fraction}` : ""}`;
}

export function normalizeUsd(value: string): string {
  const plain = value.trim().replace(/^\$/, "");
  if (!/^\d+(?:\.\d+)?$/.test(plain)) return value;
  const [whole, fraction = ""] = plain.split(".");
  const shown = fraction.slice(0, 2).replace(/0+$/, "");
  return `$${BigInt(whole!).toLocaleString("en-US")}${shown ? `.${shown}` : ""}`;
}

export function wholeUsdError(value: string): string | null {
  if (!/^[1-9]\d*$/.test(value)) return "Enter a positive whole USD amount without decimals";
  return null;
}
