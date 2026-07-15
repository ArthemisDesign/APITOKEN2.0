"use client";

import { useState } from "react";
import { B2C_PRICING_MILESTONES } from "@/lib/pricing-tiers";
import { useI18n } from "./i18n-provider";

// Prepay-модель: сумма разового пополнения разблокирует тир (те же пороги, что и месячный расход) —
// чем больше кладёшь сразу, тем выше скидка. Согласовано с логикой дашборда.
function tierForAmount(amtUsd: number): typeof B2C_PRICING_MILESTONES[number] {
  let tier: typeof B2C_PRICING_MILESTONES[number] = B2C_PRICING_MILESTONES[0];
  for (const milestone of B2C_PRICING_MILESTONES) if (amtUsd >= Number(milestone.platformSpendUsd)) tier = milestone;
  return tier;
}

export function TopUpAmountInput({ className, initialAmount, showReceive }: { className: string; initialAmount: string; showReceive?: boolean }) {
  const [amount, setAmount] = useState(initialAmount);
  const { t, language } = useI18n();

  const field = <label className={className}>
    <span aria-hidden="true">$</span>
    <input
      aria-label={t("topup_amount_label")}
      inputMode="numeric"
      pattern="[1-9][0-9]*"
      maxLength={8}
      value={amount}
      onChange={(event) => setAmount(event.target.value.replace(/\D/g, "").replace(/^0+/, ""))}
      placeholder="0"
    />
  </label>;

  if (!showReceive) return field;

  const amt = Number(amount) || 0;
  const tier = tierForAmount(amt);
  const receive = amt / ((100 - tier.discountPercent) / 100);
  const value = `$${receive.toLocaleString("en-US", { maximumFractionDigits: 0 })}`;
  const note = language === "ru"
    ? `Claude API · офиц. цены · тариф ${tier.label} −${tier.discountPercent}%`
    : `of Claude API · official prices · ${tier.label} tier −${tier.discountPercent}%`;
  return <div className="topup-live">
    {field}
    <div className="topup-live-out">
      <b>{amt > 0 ? `≈ ${value}` : "—"}</b>
      <span>{amt > 0 ? note : (language === "ru" ? "Введите сумму" : "Enter an amount")}</span>
    </div>
  </div>;
}
