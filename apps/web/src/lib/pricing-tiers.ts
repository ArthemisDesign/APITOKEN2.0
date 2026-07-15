// Prepay-модель тиров: скидку даёт ПОПОЛНЕНИЕ.
// - Тир получаешь, когда НАКОПЛЕННАЯ сумма пополнений достигает `platformSpendUsd` (пополнения
//   суммируются, пока не слетел). Ниже первого порога ($100) скидки нет.
// - Удержание: за каждые 30 дней надо потратить ≥ `holdUsd` (= 50% порога), иначе откат на −1 тир.
// - Выше — доложить (накопительно) до порога следующего тира.
// (`platformSpendUsd` = порог пополнения; `visibleOfficialUsageUsd` = порог ÷ доля оплаты = сколько
//  реального Claude API это даёт; `spendThresholdNano` = порог в нано — для визуального прогресса.)
export const B2C_PRICING_MILESTONES = [
  { code: "starter", label: "Starter", messageKey: "tier_starter", discountPercent: 60, platformSpendUsd: "100", holdUsd: "50", spendThresholdNano: "100000000000", visibleOfficialUsageUsd: "250" },
  { code: "builder", label: "Builder", messageKey: "tier_builder", discountPercent: 65, platformSpendUsd: "250", holdUsd: "125", spendThresholdNano: "250000000000", visibleOfficialUsageUsd: "714" },
  { code: "pro", label: "Pro", messageKey: "tier_pro", discountPercent: 70, platformSpendUsd: "500", holdUsd: "250", spendThresholdNano: "500000000000", visibleOfficialUsageUsd: "1667" },
  { code: "studio", label: "Studio", messageKey: "tier_studio", discountPercent: 75, platformSpendUsd: "1000", holdUsd: "500", spendThresholdNano: "1000000000000", visibleOfficialUsageUsd: "4000" },
  { code: "scale", label: "Scale", messageKey: "tier_scale", discountPercent: 80, platformSpendUsd: "2000", holdUsd: "1000", spendThresholdNano: "2000000000000", visibleOfficialUsageUsd: "10000" },
] as const;

export type B2CPricingMilestone = typeof B2C_PRICING_MILESTONES[number];

export function formatWholeUsd(value: string): string {
  return `$${BigInt(value).toLocaleString("en-US")}`;
}

/** Индекс тира по НАКОПЛЕННОЙ сумме пополнений (USD). −1 = тира ещё нет (ниже первого порога $100). */
export function tierIndexForTopups(topupUsd: number): number {
  let index = -1;
  B2C_PRICING_MILESTONES.forEach((milestone, i) => { if (topupUsd >= Number(milestone.platformSpendUsd)) index = i; });
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
  return ((index + 1 + within) / segments) * 100;
}
