"use client";

import { useState } from "react";
import { B2C_DISCOUNT_PERCENT, officialUsageForTopup } from "@/lib/pricing-tiers";
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
  // Плоская модель: любая сумма конвертируется по одной ставке — скидка 50% (×2 ценности).
  const receive = officialUsageForTopup(amt);
  const value = `$${receive.toLocaleString("en-US", { maximumFractionDigits: 0 })}`;
  const sub = amt <= 0
    ? (language === "ru" ? "Введите сумму" : "Enter an amount")
    : (language === "ru" ? "официального использования API" : "of official API usage");
  return <div className="topup-live">
    {field}
    <div className="topup-live-out">
      <div className="tlo-row">
        <b>{amt > 0 ? `≈ ${value}` : "—"}</b>
        {amt > 0 && <span className="tlo-badge">−{B2C_DISCOUNT_PERCENT}%</span>}
      </div>
      <span className="tlo-sub">{sub}</span>
    </div>
  </div>;
}
