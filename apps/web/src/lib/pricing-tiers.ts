// Prepay-модель тиров: Starter −60% даётся СТАРТОВО (базовый, порог $0, удержания нет).
// - Выше — за НАКОПЛЕННУЮ сумму пополнений (`platformSpendUsd` = порог; суммируются, пока не слетел).
// - Удержание: за каждые 30 дней потратить ≥ `holdUsd` (= 50% порога), иначе откат на −1 тир.
// (`visibleOfficialUsageUsd` = порог ÷ доля оплаты; `spendThresholdNano` = порог в нано для прогресса.)
export const B2C_PRICING_MILESTONES = [
  { code: "starter", label: "Starter", messageKey: "tier_starter", discountPercent: 60, platformSpendUsd: "0", holdUsd: "0", spendThresholdNano: "0", visibleOfficialUsageUsd: "0" },
  { code: "builder", label: "Builder", messageKey: "tier_builder", discountPercent: 65, platformSpendUsd: "100", holdUsd: "50", spendThresholdNano: "100000000000", visibleOfficialUsageUsd: "286" },
  { code: "pro", label: "Pro", messageKey: "tier_pro", discountPercent: 70, platformSpendUsd: "250", holdUsd: "125", spendThresholdNano: "250000000000", visibleOfficialUsageUsd: "833" },
  { code: "studio", label: "Studio", messageKey: "tier_studio", discountPercent: 75, platformSpendUsd: "500", holdUsd: "250", spendThresholdNano: "500000000000", visibleOfficialUsageUsd: "2000" },
  { code: "scale", label: "Scale", messageKey: "tier_scale", discountPercent: 80, platformSpendUsd: "1000", holdUsd: "500", spendThresholdNano: "1000000000000", visibleOfficialUsageUsd: "5000" },
] as const;

export type B2CPricingMilestone = typeof B2C_PRICING_MILESTONES[number];

export function formatWholeUsd(value: string): string {
  return `$${BigInt(value).toLocaleString("en-US")}`;
}

/** Индекс тира после пополнения: текущая накопленная сумма + новое пополнение, оба в нано-USD. */
export function tierIndexForTopups(currentTopupsNano: string, proposedTopupNano: string): number {
  const totalTopupsNano = BigInt(currentTopupsNano) + BigInt(proposedTopupNano);
  let index = -1;
  B2C_PRICING_MILESTONES.forEach((milestone, i) => {
    if (totalTopupsNano >= BigInt(milestone.spendThresholdNano)) index = i;
  });
  return index;
}

/**
 * Прогресс (0..100) по НАКОПЛЕННЫМ пополнениям через равные визуальные сегменты. Первый сегмент —
 * путь «нет тира → Starter»; дальше сегмент на каждый тир.
 */
export function pricingMilestoneProgress(currentTier: string, spentNano: string): number {
  const index = B2C_PRICING_MILESTONES.findIndex((tier) => tier.code === currentTier);
  if (index >= B2C_PRICING_MILESTONES.length - 1) return 100;
  const segments = B2C_PRICING_MILESTONES.length;
  const start = index < 0 ? 0n : BigInt(B2C_PRICING_MILESTONES[index]!.spendThresholdNano);
  const end = BigInt(B2C_PRICING_MILESTONES[index + 1]!.spendThresholdNano);
  const spent = BigInt(spentNano);
  const position = spent <= start ? 0n : spent >= end ? end - start : spent - start;
  const within = end > start ? Number(position * 10_000n / (end - start)) / 10_000 : 0;
  return ((index + within) / (segments - 1)) * 100;
}
