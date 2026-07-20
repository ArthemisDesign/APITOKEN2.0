"use client";

import { useEffect, useState } from "react";
import {
  api,
  ApiError,
  formatDate,
  formatUsd,
  type ReferralRow,
} from "@/lib/api";
import { EmptyState, Loading, Notice, Table } from "@/components/ui";
import { useI18n } from "@/components/i18n";

export default function ReferralsPage() {
  const { t } = useI18n();
  const [items, setItems] = useState<ReferralRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await api<{ items: ReferralRow[] }>("/v1/partner/referrals");
        if (!cancelled)
          setItems(
            [...res.items].sort((a, b) => (a.attributedAt < b.attributedAt ? 1 : -1)),
          );
      } catch (err) {
        if (!cancelled)
          setError(err instanceof ApiError ? err.message : t("Failed to load referrals.", "Не удалось загрузить рефералов."));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <>
      <h1 className="page-title">{t("Referrals", "Рефералы")}</h1>
      <p className="page-sub">
        {t(
          "Users attributed to you. Identities are masked for their privacy. “Spend” is what each user has actually paid for API usage (after their own discount) — the base your commission is calculated from.",
          "Пользователи, закреплённые за вами. Личности скрыты в целях конфиденциальности. «Расход» — это то, что каждый пользователь фактически заплатил за использование API (после своей скидки), и именно от этой суммы считается ваша комиссия.",
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
                <th>{t("Joined", "Присоединился")}</th>
                <th className="num">{t("Spend", "Траты")}</th>
                <th className="num">{t("You earned", "Вы заработали")}</th>
              </>
            }
          >
            {items.map((r) => (
              <tr key={`${r.userMask}-${r.attributedAt}`}>
                <td className="mono">{r.userMask}</td>
                <td>{formatDate(r.attributedAt)}</td>
                <td className="num">{formatUsd(r.spendNano)}</td>
                <td className="num" style={{ color: "var(--accent-strong)", fontWeight: 700 }}>
                  {formatUsd(r.earnedNano)}
                </td>
              </tr>
            ))}
          </Table>
        )
      ) : null}
    </>
  );
}
