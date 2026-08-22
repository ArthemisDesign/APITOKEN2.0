"use client";

import { useCallback, useEffect, useState } from "react";
import {
  api,
  ApiError,
  formatDate,
  formatUsd,
  type ReferralRow,
} from "@/lib/api";
import { Badge, Button, EmptyState, Loading, Notice, StatusBadge, Table } from "@/components/ui";
import { localeFor, useI18n } from "@/components/i18n";
import { usePartner } from "@/components/partner-context";
import { BusinessPricingDialog } from "@/components/business-pricing-dialog";

export default function ReferralsPage() {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const partner = usePartner();
  const [items, setItems] = useState<ReferralRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pricing, setPricing] = useState<ReferralRow | null>(null);
  // A direct grant enables self-service. Everyone else can send an explicit reviewed request.
  const b2bAllowed = partner.b2bEnabled === true && (partner.b2bMaxDiscountBps ?? 0) > 0;
  const ceilingPercent = Math.floor((partner.b2bMaxDiscountBps ?? 0) / 100);

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
          "Users attributed to you are identified by their account email. If Commerce enrichment is temporarily unavailable, a privacy-safe mask is shown instead. “Spend” is their real-money API usage after their discount, excluding free platform credit. That is the base your commission is calculated from.",
          "Закреплённые за вами пользователи отображаются по email аккаунта. Если данные Commerce временно недоступны, вместо него показывается безопасная маска. «Расход» — реальные траты на API после скидки, без бесплатных средств платформы. Именно от него считается ваша комиссия.",
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
            label={t("Referrals", "Рефералы")}
            head={
              <>
                <th>{t("User", "Пользователь")}</th>
                <th>{t("Plan", "Тип")}</th>
                <th className="num">{t("Discount", "Скидка")}</th>
                <th className="num">{t("Balance", "Баланс")}</th>
                <th className="num">{t("Top-ups", "Пополнения")}</th>
                <th className="num">{t("Spend", "Траты")}</th>
                <th className="num">{t("You earned", "Вы заработали")}</th>
                <th>{t("Business terms", "B2B-условия")}</th>
              </>
            }
          >
            {items.map((r) => {
              return (
                <tr key={`${r.userRef ?? r.userMask}-${r.attributedAt}`}>
                  <td>
                    <span className="identity-email" title={r.email ?? r.userMask} translate="no">{r.email ?? r.userMask}</span>
                    <div className="referral-joined">
                      {t("joined", "с")} {formatDate(r.attributedAt, locale)}
                    </div>
                  </td>
                  <td>
                    {r.customerType === "b2b" ? (
                      <Badge tone="green">B2B</Badge>
                    ) : r.customerType === "b2c" ? (
                      <Badge tone="neutral">B2C</Badge>
                    ) : (
                      <span className="muted-dash">—</span>
                    )}
                    {r.status && r.status !== "active" ? (
                      <div className="referral-status">
                        <StatusBadge status={r.status} />
                      </div>
                    ) : null}
                  </td>
                  <td className="num">
                    {r.discountPercent != null ? `${r.discountPercent}%` : <span className="muted-dash">—</span>}
                  </td>
                  <td className="num">
                    {r.balanceNano != null ? formatUsd(r.balanceNano) : <span className="muted-dash">—</span>}
                  </td>
                  <td className="num">{formatUsd(r.topupNano)}</td>
                  <td className="num">{formatUsd(r.spendNano)}</td>
                  <td className="num referral-earned">
                    {formatUsd(r.netNano)}
                    {BigInt(r.adjustmentNano) !== 0n ? <div className="field-hint">{formatUsd(r.adjustmentNano)} {t("returns", "возвраты")}</div> : null}
                  </td>
                  <td>
                    {r.userRef ? (
                      <Button size="sm" variant="ghost" onClick={() => setPricing(r)}>
                        {b2bAllowed
                          ? (r.customerType === "b2b" ? t("Edit rates", "Ставки") : t("Make B2B", "Сделать B2B"))
                          : (r.customerType === "b2b" ? t("Request rates", "Запросить ставки") : t("Request B2B", "Запросить B2B"))}
                      </Button>
                    ) : null}
                  </td>
                </tr>
              );
            })}
          </Table>
        )
      ) : null}
      {pricing ? (
        <BusinessPricingDialog
          row={pricing}
          ceilingPercent={ceilingPercent}
          mode={b2bAllowed ? "direct" : "request"}
          onClose={() => setPricing(null)}
          onSaved={() => { void load(); }}
        />
      ) : null}
    </>
  );
}
