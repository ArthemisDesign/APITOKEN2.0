"use client";

import { useState } from "react";
import { B2C_PRICING_MILESTONES, tierIndexForTopups } from "@/lib/pricing-tiers";
import { useI18n } from "./i18n-provider";

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
  const idx = tierIndexForTopups(amt);
  const tier = idx >= 0 ? B2C_PRICING_MILESTONES[idx] : null;
  const receive = amt / (tier ? (100 - tier.discountPercent) / 100 : 1);
  const value = `$${receive.toLocaleString("en-US", { maximumFractionDigits: 0 })}`;
  const sub = amt <= 0
    ? (language === "ru" ? "Введите сумму" : "Enter an amount")
    : tier
      ? (language === "ru" ? `официального Claude API · тариф ${tier.label}` : `of official Claude API · ${tier.label} tier`)
      : (language === "ru" ? "официального Claude API · пополни от $100 для скидки" : "of official Claude API · top up from $100 for a discount");
  return <div className="topup-live">
    {field}
    <div className="topup-live-out">
      <div className="tlo-row">
        <b>{amt > 0 ? `≈ ${value}` : "—"}</b>
        {amt > 0 && tier && <span className="tlo-badge">−{tier.discountPercent}%</span>}
      </div>
      <span className="tlo-sub">{sub}</span>
    </div>
  </div>;
}
