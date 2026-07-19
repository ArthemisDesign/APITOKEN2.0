"use client";

import { useEffect, useState } from "react";
import {
  api,
  ApiError,
  formatBps,
  formatDate,
  formatUsd,
  type EarningRow,
  type Overview,
  type ReferralRow,
} from "@/lib/api";
import {
  Card,
  CopyButton,
  EmptyState,
  Input,
  Loading,
  Notice,
} from "@/components/ui";
import { EarningsChart } from "@/components/earnings-chart";
import { useI18n } from "@/components/i18n";

export default function OverviewPage() {
  const { t } = useI18n();
  const [overview, setOverview] = useState<Overview | null>(null);
  const [earnings, setEarnings] = useState<EarningRow[] | null>(null);
  const [recent, setRecent] = useState<ReferralRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [ov, earn, refs] = await Promise.all([
          api<Overview>("/v1/partner/overview"),
          api<{ items: EarningRow[] }>("/v1/partner/earnings?days=30"),
          api<{ items: ReferralRow[] }>("/v1/partner/referrals"),
        ]);
        if (cancelled) return;
        setOverview(ov);
        setEarnings(earn.items);
        setRecent(
          [...refs.items]
            .sort((a, b) => (a.attributedAt < b.attributedAt ? 1 : -1))
            .slice(0, 6),
        );
      } catch (err) {
        if (!cancelled)
          setError(err instanceof ApiError ? err.message : t("Failed to load overview.", "Не удалось загрузить обзор."));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (error) return <Notice kind="error">{error}</Notice>;
  if (!overview || !earnings || !recent)
    return <Loading label={t("Loading overview…", "Загружаем обзор…")} />;

  return (
    <>
      <h1 className="page-title">{t("Overview", "Обзор")}</h1>
      <p className="page-sub">
        {t("You earn ", "Вы получаете ")}
        <strong>{formatBps(overview.commissionBps)}</strong>
        {t(
          " of what your referrals actually spend on apitoken.sale — their real paid API usage, after their own discount. Not their top-ups, not list price. Paid out in USDT (BEP-20).",
          " от того, что ваши рефералы реально тратят на apitoken.sale — их фактически оплаченного использования API, после их собственной скидки. Не от пополнений и не от прайс-листа. Выплаты в USDT (BEP-20).",
        )}
      </p>

      <div className="stat-grid">
        <div className="stat-card">
          <div className="stat-label">{t("Available", "Доступно")}</div>
          <div className="stat-value green">{formatUsd(overview.totals.availableNano)}</div>
          <div className="stat-foot">
            {formatUsd(overview.totals.pendingPayoutNano)} {t("pending payout", "ожидает выплаты")}
          </div>
        </div>
        <div className="stat-card">
          <div className="stat-label">{t("Total earned", "Всего заработано")}</div>
          <div className="stat-value">{formatUsd(overview.totals.earnedNano)}</div>
          <div className="stat-foot">{formatUsd(overview.totals.paidNano)} {t("paid out", "выплачено")}</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">{t("Referred users", "Приглашённые пользователи")}</div>
          <div className="stat-value">{overview.referredUsers}</div>
          <div className="stat-foot">
            {formatUsd(overview.last30d.spendNano)} {t("spend in 30d", "расход за 30 дн.")}
          </div>
        </div>
        <div className="stat-card">
          <div className="stat-label">{t("Earned in 30d", "Заработано за 30 дн.")}</div>
          <div className="stat-value">{formatUsd(overview.last30d.earnedNano)}</div>
          <div className="stat-foot">{t("rolling 30-day window", "скользящее окно 30 дней")}</div>
        </div>
      </div>

      <div className="stack">
        <Card
          title={t("Your referral link", "Ваша реферальная ссылка")}
          sub={t(
            "Anyone who registers on apitoken.sale through this link is attributed to you permanently.",
            "Каждый, кто зарегистрируется на apitoken.sale по этой ссылке, навсегда закрепляется за вами.",
          )}
        >
          <div className="reflink-row">
            <Input readOnly value={overview.referralUrl} onFocus={(e) => e.currentTarget.select()} />
            <CopyButton value={overview.referralUrl} label={t("Copy link", "Копировать ссылку")} />
          </div>
          <p className="field-hint" style={{ marginTop: 10 }}>
            {t("Referral code:", "Реферальный код:")} <span className="mono">{overview.referralCode}</span>
          </p>
        </Card>

        <Card title={t("How your commission works", "Как работает ваша комиссия")}>
          <ul className="how-list">
            <li>
              {t("You earn ", "Вы получаете ")}
              <strong>{formatBps(overview.commissionBps)}</strong>
              {t(
                " of every dollar your referrals ",
                " с каждого доллара, который ваши рефералы ",
              )}
              <strong>{t("actually pay", "реально платят")}</strong>
              {t(" for API usage on apitoken.sale.", " за использование API на apitoken.sale.")}
            </li>
            <li>
              {t("The base is their ", "Основа — их ")}
              <strong>{t("real spend", "фактический расход")}</strong>
              {t(
                " — the amount charged after their own tier discount. It is ",
                " — сумма, списанная после их собственной скидки за тир. Это ",
              )}
              <strong>{t("not", "не")}</strong>
              {t(" their top-up amount and ", " сумма их пополнения и ")}
              <strong>{t("not", "не")}</strong>{" "}
              {t("the full list price.", "полная цена по прайс-листу.")}
            </li>
            <li>
              {t("Example: a referral is charged ", "Пример: с реферала списали ")}
              <strong>$100</strong>
              {t(" for usage → you earn ", " за использование → вы получаете ")}
              <strong>{formatUsd((BigInt(overview.commissionBps) * 100n * 10n ** 9n / 10000n).toString())}</strong>.
            </li>
            <li>
              {t(
                "Commission accrues as they spend and is paid out in USDT (BEP-20).",
                "Комиссия начисляется по мере их расходов и выплачивается в USDT (BEP-20).",
              )}
            </li>
          </ul>
        </Card>

        <Card
          title={t("Last 30 days", "Последние 30 дней")}
          sub={t(
            `${formatUsd(overview.last30d.earnedNano)} earned on ${formatUsd(overview.last30d.spendNano)} of referral spend.`,
            `${formatUsd(overview.last30d.earnedNano)} заработано с ${formatUsd(overview.last30d.spendNano)} расходов рефералов.`,
          )}
        >
          <EarningsChart items={earnings} />
        </Card>

        <Card
          title={t("Recent activity", "Недавняя активность")}
          sub={t("Latest referrals attributed to you.", "Последние рефералы, закреплённые за вами.")}
        >
          {recent.length === 0 ? (
            <EmptyState title={t("No referrals yet", "Пока нет рефералов")}>
              {t(
                "Share your link to start building your list.",
                "Поделитесь своей ссылкой, чтобы начать собирать базу.",
              )}
            </EmptyState>
          ) : (
            <div className="activity-list">
              {recent.map((r) => (
                <div className="activity-item" key={`${r.userMask}-${r.attributedAt}`}>
                  <span>
                    <span className="mono">{r.userMask}</span> {t("joined ·", "присоединился ·")}{" "}
                    <span style={{ color: "var(--accent-strong)" }}>
                      {formatUsd(r.earnedNano)}
                    </span>{" "}
                    {t("earned for you", "заработано для вас")}
                  </span>
                  <span className="activity-date">{formatDate(r.attributedAt)}</span>
                </div>
              ))}
            </div>
          )}
        </Card>
      </div>
    </>
  );
}
