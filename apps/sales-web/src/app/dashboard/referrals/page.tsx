"use client";

import { useCallback, useEffect, useState } from "react";
import {
  api,
  ApiError,
  formatDate,
  formatUsd,
  type ReferralRow,
} from "@/lib/api";
import { Badge, EmptyState, Loading, Notice, StatusBadge, Table } from "@/components/ui";
import { useI18n } from "@/components/i18n";

export default function ReferralsPage() {
  const { t } = useI18n();
  const [items, setItems] = useState<ReferralRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    const res = await api<{ items: ReferralRow[] }>("/v1/partner/referrals");
    setItems([...res.items].sort((a, b) => (a.attributedAt < b.attributedAt ? 1 : -1)));
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        await load();
      } catch (err) {
        if (!cancelled)
          setError(err instanceof ApiError ? err.message : t("Failed to load referrals.", "Не удалось загрузить рефералов."));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [load]);

  return (
    <>
      <h1 className="page-title">{t("Referrals", "Рефералы")}</h1>
      <p className="page-sub">
        {t(
          "Users attributed to you. Identities are masked for their privacy. “Spend” is their real-money API usage — the amount charged after their own discount, excluding anything paid from free bonus/promo balance. That is the base your commission is calculated from.",
          "Пользователи, закреплённые за вами. Личности скрыты в целях конфиденциальности. «Расход» — это их реальные траты на API: сумма, списанная после их скидки, без учёта оплаченного из бесплатного бонуса/промо. Именно от неё считается ваша комиссия.",
        )}
      </p>
      {error ? <Notice kind="error">{error}</Notice> : null}
      {!items && !error ? <Loading label={t("Loading referrals…", "Загружаем рефералов…")} /> : null}
      {items ? (
        items.length === 0 ? (
          <div className="card">
            <EmptyState title={t("No referrals yet", "Пока нет рефералов")}>
              {t(
                "Share your referral link — every user who registers through it appears here.",
                "Поделитесь реферальной ссылкой — каждый пользователь, зарегистрировавшийся по ней, появится здесь.",
              )}
            </EmptyState>
          </div>
        ) : (
          <Table
            head={
              <>
                <th>{t("User", "Пользователь")}</th>
                <th>{t("Plan", "Тип")}</th>
                <th className="num">{t("Discount", "Скидка")}</th>
                <th className="num">{t("Balance", "Баланс")}</th>
                <th className="num">{t("Top-ups", "Пополнения")}</th>
                <th className="num">{t("Spend", "Траты")}</th>
                <th className="num">{t("You earned", "Вы заработали")}</th>
              </>
            }
          >
            {items.map((r) => {
              return (
                <tr key={`${r.userMask}-${r.attributedAt}`}>
                  <td className="mono">
                    {r.userMask}
                    <div style={{ color: "var(--text-dim)", fontSize: "12px", marginTop: "2px" }}>
                      {t("joined", "с")} {formatDate(r.attributedAt)}
                    </div>
                  </td>
                  <td>
                    {r.customerType === "b2b" ? (
                      <Badge tone="green">B2B</Badge>
                    ) : r.customerType === "b2c" ? (
                      <Badge tone="neutral">B2C</Badge>
                    ) : (
                      <span style={{ color: "var(--text-dim)" }}>—</span>
                    )}
                    {r.status && r.status !== "active" ? (
                      <div style={{ marginTop: "4px" }}>
                        <StatusBadge status={r.status} />
                      </div>
                    ) : null}
                  </td>
                  <td className="num">
                    {r.discountPercent != null ? `${r.discountPercent}%` : <span style={{ color: "var(--text-dim)" }}>—</span>}
                  </td>
                  <td className="num">
                    {r.balanceNano != null ? formatUsd(r.balanceNano) : <span style={{ color: "var(--text-dim)" }}>—</span>}
                  </td>
                  <td className="num">{formatUsd(r.topupNano)}</td>
                  <td className="num">{formatUsd(r.spendNano)}</td>
                  <td className="num" style={{ color: "var(--accent-strong)", fontWeight: 700 }}>
                    {formatUsd(r.netNano)}
                    {BigInt(r.adjustmentNano) !== 0n ? <div className="field-hint">{formatUsd(r.adjustmentNano)} {t("returns", "возвраты")}</div> : null}
                  </td>
                </tr>
              );
            })}
          </Table>
        )
      ) : null}
    </>
  );
}
