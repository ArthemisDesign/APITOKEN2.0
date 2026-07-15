"use client";

import { useState } from "react";
import { useI18n } from "./i18n-provider";

// Стартовый тариф для незалогиненных: скидка 60% → клиент платит 40% → ×2.5 ценности.
const STARTER_PAY_FRACTION = 0.4;

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
  const value = `$${(amt / STARTER_PAY_FRACTION).toLocaleString("en-US", { maximumFractionDigits: 0 })}`;
  const note = language === "ru" ? "Claude API · офиц. цены · −60%" : "of Claude API · official prices · −60%";
  return <div className="topup-live">
    {field}
    <div className="topup-live-out">
      <b>{amt > 0 ? `≈ ${value}` : "—"}</b>
      <span>{amt > 0 ? note : (language === "ru" ? "Введите сумму" : "Enter an amount")}</span>
    </div>
  </div>;
}
