export const NANO_PER_USD = 1_000_000_000n;

/** "50" → 50000000000n. Только целые доллары, без точек и знаков. */
export function usdStringToNano(raw: string): bigint {
  if (!/^[1-9]\d{0,6}$/.test(raw)) {
    throw new Error("Сумма должна быть целым числом долларов от 1 до 9999999");
  }
  return BigInt(raw) * NANO_PER_USD;
}

export function formatUsd(nano: bigint, fractionDigits = 2): string {
  const negative = nano < 0n;
  const absolute = negative ? -nano : nano;
  const whole = absolute / NANO_PER_USD;
  const remainder = absolute % NANO_PER_USD;
  const scale = 10n ** BigInt(fractionDigits);
  const fraction = (remainder * scale) / NANO_PER_USD;
  const body =
    fractionDigits > 0
      ? `${whole.toString()}.${fraction.toString().padStart(fractionDigits, "0")}`
      : whole.toString();
  return `${negative ? "-" : ""}$${body}`;
}

/** Баланс движка → эквивалент официального прайса Anthropic. */
export function balanceToOfficialNano(balanceNano: bigint, multBp: number): bigint {
  if (multBp <= 0) return 0n;
  return (balanceNano * 10_000n) / BigInt(multBp);
}

/**
 * Разделяет доступный баланс, незакрытые холды и фактический расход.
 *
 * Движок вычитает резерв из balance_nano на время запроса. Для клиентского
 * профиля остаток по завершённым запросам поэтому равен balance + reserved,
 * а не одному balance. Все операции остаются целочисленными в nanoUSD.
 */
export function officialBalanceBreakdown(
  availableNano: bigint,
  reservedNano: bigint,
  spentNano: bigint,
  multBp: number,
): { available: bigint; reserved: bigint; remaining: bigint; spent: bigint } {
  return {
    available: balanceToOfficialNano(availableNano, multBp),
    reserved: balanceToOfficialNano(reservedNano, multBp),
    remaining: balanceToOfficialNano(availableNano + reservedNano, multBp),
    spent: balanceToOfficialNano(spentNano, multBp),
  };
}

/** Номинал в официальном эквиваленте → сколько зачислить на баланс движка. */
export function officialNanoToBalance(officialNano: bigint, multBp: number): bigint {
  return (officialNano * BigInt(multBp)) / 10_000n;
}
