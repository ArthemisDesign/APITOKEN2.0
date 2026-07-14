"use client";

import { useState } from "react";
import { useI18n } from "./i18n-provider";

export function TopUpAmountInput({ className, initialAmount }: { className: string; initialAmount: string }) {
  const [amount, setAmount] = useState(initialAmount);
  const { t } = useI18n();

  return <label className={className}>
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
}
