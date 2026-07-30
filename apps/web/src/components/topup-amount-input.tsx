"use client";

import { useState } from "react";
import { FLAT_DISCOUNT_PERCENT, FLAT_PRICE_MULTIPLIER } from "@/lib/pricing-tiers";
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
  // Плоская модель: любая сумма конвертируется по одной ставке −50% (×2 официальной ценности).
  const receive = amt / FLAT_PRICE_MULTIPLIER;
  const value = `$${receive.toLocaleString("en-US", { maximumFractionDigits: 0 })}`;
  const sub = amt <= 0
    ? (language === "ru" ? "Введите сумму" : "Enter an amount")
    : (language === "ru" ? "официального использования API · плоские −50%" : "of official API usage · flat −50%");
  return <div className="topup-live">
    {field}
    <div className="topup-live-out">
      <div className="tlo-row">
        <b>{amt > 0 ? `≈ ${value}` : "—"}</b>
        {amt > 0 && <span className="tlo-badge">−{FLAT_DISCOUNT_PERCENT}%</span>}
      </div>
      <span className="tlo-sub">{sub}</span>
    </div>
  </div>;
}
