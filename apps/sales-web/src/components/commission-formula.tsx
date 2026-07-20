"use client";

import { formatBps, formatUsd } from "@/lib/api";
import { useI18n } from "./i18n";

// Большая наглядная формула комиссии: (100% − скидка клиента) × ставка рефовода, применённая к тому,
// что реферал реально тратит по прайсу. Максимально просто, с числовым примером.
export function CommissionFormula({ commissionBps }: { commissionBps: number }) {
  const { t } = useI18n();
  const rate = formatBps(commissionBps);
  // Пример: прайс-использование $100, скидка клиента 50% → он платит $50, вы получаете rate от $50.
  const exampleDiscountPct = 50;
  const paidNano = (100n * 10n ** 9n * BigInt(100 - exampleDiscountPct)) / 100n; // $50
  const earnNano = (BigInt(commissionBps) * paidNano) / 10_000n;

  return (
    <div
      style={{
        border: "1px solid var(--border)",
        borderRadius: 12,
        padding: "22px 18px",
        textAlign: "center",
        background: "var(--surface-2, rgba(127,127,127,0.06))",
      }}
    >
      <div style={{ fontSize: 12, letterSpacing: "0.08em", textTransform: "uppercase", color: "var(--text-faint)", marginBottom: 12 }}>
        {t("Your reward per $1 of list-price usage", "Ваша награда с $1 использования по прайсу")}
      </div>
      <div style={{ fontSize: 30, fontWeight: 800, lineHeight: 1.25, letterSpacing: "-0.5px" }}>
        <span style={{ whiteSpace: "nowrap" }}>
          (100% − <span style={{ color: "var(--accent-strong, #3b5bdb)" }}>{t("discount", "скидка")}</span>)
        </span>
        <span style={{ opacity: 0.55, margin: "0 8px" }}>×</span>
        <span style={{ color: "var(--accent-strong, #3b5bdb)", whiteSpace: "nowrap" }}>{rate}</span>
      </div>
      <div style={{ fontSize: 13.5, color: "var(--text-faint)", marginTop: 14, maxWidth: 560, marginInline: "auto", lineHeight: 1.5 }}>
        {t(
          `The client pays (100 − their discount)% of the price. You get ${rate} of that — only the part paid with real money (free bonus/promo never counts).`,
          `Клиент платит (100 − его скидка)% от цены. Вы получаете ${rate} от этого — только с части, оплаченной реальными деньгами (бесплатный бонус/промо не считается).`,
        )}
      </div>
      <div style={{ fontSize: 13.5, marginTop: 10, fontWeight: 600 }}>
        {t(
          `Example: $100 of usage, ${exampleDiscountPct}% discount → they pay ${formatUsd(paidNano.toString())}, you earn ${formatUsd(earnNano.toString())}.`,
          `Пример: $100 использования, скидка ${exampleDiscountPct}% → он платит ${formatUsd(paidNano.toString())}, вы получаете ${formatUsd(earnNano.toString())}.`,
        )}
      </div>
    </div>
  );
}
