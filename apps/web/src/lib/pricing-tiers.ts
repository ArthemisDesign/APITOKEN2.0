export const B2C_PRICING_MILESTONES = [
  { code: "starter", label: "Starter", messageKey: "tier_starter", discountPercent: 60, spendThresholdNano: "0", platformSpendUsd: "0", visibleOfficialUsageUsd: "0" },
  { code: "builder", label: "Builder", messageKey: "tier_builder", discountPercent: 65, spendThresholdNano: "25000000000", platformSpendUsd: "25", visibleOfficialUsageUsd: "70" },
  { code: "pro", label: "Pro", messageKey: "tier_pro", discountPercent: 70, spendThresholdNano: "75000000000", platformSpendUsd: "75", visibleOfficialUsageUsd: "250" },
  { code: "studio", label: "Studio", messageKey: "tier_studio", discountPercent: 75, spendThresholdNano: "200000000000", platformSpendUsd: "200", visibleOfficialUsageUsd: "800" },
  { code: "scale", label: "Scale", messageKey: "tier_scale", discountPercent: 80, spendThresholdNano: "500000000000", platformSpendUsd: "500", visibleOfficialUsageUsd: "2500" },
] as const;

export type B2CPricingMilestone = typeof B2C_PRICING_MILESTONES[number];

export function formatWholeUsd(value: string): string {
  return `$${BigInt(value).toLocaleString("en-US")}`;
}

/**
 * Returns progress across equally sized visual milestone segments. Real spend
 * thresholds remain authoritative; equal segment widths keep every tier legible.
 */
export function pricingMilestoneProgress(currentTier: string, spentNano: string): number {
  const currentIndex = B2C_PRICING_MILESTONES.findIndex((tier) => tier.code === currentTier);
  const safeIndex = currentIndex < 0 ? 0 : currentIndex;
  if (safeIndex >= B2C_PRICING_MILESTONES.length - 1) return 100;

  const start = BigInt(B2C_PRICING_MILESTONES[safeIndex]!.spendThresholdNano);
  const end = BigInt(B2C_PRICING_MILESTONES[safeIndex + 1]!.spendThresholdNano);
  const spent = BigInt(spentNano);
  const position = spent <= start ? 0n : spent >= end ? end - start : spent - start;
  const segmentBasisPoints = Number(position * 10_000n / (end - start));
  return ((safeIndex + segmentBasisPoints / 10_000) / (B2C_PRICING_MILESTONES.length - 1)) * 100;
}
