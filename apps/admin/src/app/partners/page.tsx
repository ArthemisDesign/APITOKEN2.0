"use client";

// Партнёры — порт 1:1 функции partners() из crates/server/src/admin-panel.js.
// Шесть независимых URL-ресурсов деградируют и рендерятся отдельно; изменения
// приходят из sales SSE без interval/focus-запросов.
// Сводка остаётся независимо деградирующей; write-workflows живут в соседних
// маршрутах этого же /partners control room.
import { useResources } from "@/lib/resources";
import { ago, count, formatDate, nanoMoney } from "@/lib/format";
import { localeFor, useI18n } from "@/lib/i18n";
import { Banner, CardGrid, EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, StatCard, TableCard } from "@/components/ui";
import { useRouter, useSearchParams } from "next/navigation";
import { memo, startTransition, useEffect, useMemo } from "react";
import {
  BATCH_STATUS_KIND,
  PARTNER_PHASE_LABEL,
  PARTNER_SORTS,
  PAYOUT_STATUS_KIND,
  clampOffset,
  eligibleSumNano,
  bnbMoney,
  partnerName,
  payoutWalletReadiness,
  shortWallet,
  type PartnerAnalytics,
  type PartnerAnalyticsItem,
  type PartnerOverview,
  type PayoutBatch,
  type PayoutBatches,
  type PayoutDue,
  type PayoutDueItem,
  type PayoutEngine,
  type PayoutHistory,
  type PayoutItem,
} from "./helpers";

const PAGE_LIMIT = 50;

interface PartnersPage {
  offset: number;
  limit: number;
  sort: string;
  dir: string;
}

function partnersUrl(page: PartnersPage): string {
  const params = new URLSearchParams();
  if (page.sort !== "unpaid") params.set("sort", page.sort);
  if (page.dir !== "desc") params.set("dir", page.dir);
  if (page.offset > 0) params.set("offset", String(page.offset));
  return `/partners${params.size ? `?${params}` : ""}`;
}

interface PartnersData {
  overview: PartnerOverview;
  engine: PayoutEngine;
  due: PayoutDue;
  analytics: PartnerAnalytics;
  payouts: PayoutHistory;
  batches: PayoutBatches;
}

function AnalyticsHead() {
  const { t } = useI18n();
  return (
  <thead>
    <tr>
      <th className="left">{t("Partner", "Партнёр")}</th>
      <th>{t("Status", "Статус")}</th>
      <th>{t("Referrals", "Рефералы")}</th>
      <th>{t("Top-ups · 30d", "Пополнения · 30д")}</th>
      <th>{t("Spend · 30d", "Расход · 30д")}</th>
      <th>{t("Earnings · 30d", "Заработок · 30д")}</th>
      <th>{t("Payable", "К выплате")}</th>
      <th>{t("Activity", "Активность")}</th>
    </tr>
  </thead>
  );
}

function DueHead() {
  const { t } = useI18n();
  return (
  <thead>
    <tr>
      <th className="left">{t("Partner", "Партнёр")}</th>
      <th>{t("Payable", "К выплате")}</th>
      <th className="left">{t("BEP-20 wallet", "Кошелёк BEP-20")}</th>
      <th>{t("Readiness", "Готовность")}</th>
    </tr>
  </thead>
  );
}

function PayoutsHead() {
  const { t } = useI18n();
  return (
  <thead>
    <tr>
      <th className="left">{t("Partner", "Партнёр")}</th>
      <th>{t("Amount", "Сумма")}</th>
      <th>{t("Status", "Статус")}</th>
      <th>{t("Method", "Метод")}</th>
      <th>{t("Requested", "Запрошена")}</th>
      <th>{t("Decided", "Решена")}</th>
      <th>{t("Paid", "Выплачена")}</th>
    </tr>
  </thead>
  );
}

function BatchesHead() {
  const { t } = useI18n();
  return (
  <thead>
    <tr>
      <th>{t("Status", "Статус")}</th>
      <th>{t("Amount", "Сумма")}</th>
      <th>{t("Recipients", "Получатели")}</th>
      <th>gas, gwei</th>
      <th className="left">hot wallet</th>
      <th>{t("Created", "Создан")}</th>
      <th>{t("Sent", "Отправлен")}</th>
      <th>{t("Completed", "Завершён")}</th>
      <th className="left">{t("Error", "Ошибка")}</th>
    </tr>
  </thead>
  );
}

function statusLabel(status: string | undefined, t: (en: string, ru: string) => string): string {
  const labels: Record<string, [string, string]> = {
    active: ["Active", "Активен"], suspended: ["Suspended", "Приостановлен"], pending: ["Pending", "Ожидает"],
    requested: ["Requested", "Запрошена"], approved: ["Approved", "Одобрена"], paid: ["Paid", "Выплачена"], rejected: ["Rejected", "Отклонена"],
    preparing: ["Preparing", "Подготавливается"], prepared: ["Prepared", "Подготовлен"], sending: ["Sending", "Отправляется"], sent: ["Sent", "Отправлен"], failed: ["Failed", "Ошибка"], canceled: ["Canceled", "Отменён"],
  };
  const label = status ? labels[status] : undefined;
  return label ? t(label[0], label[1]) : status ?? "—";
}

function dueReason(item: PayoutDueItem, t: (en: string, ru: string) => string): string {
  if (item.eligible) return t("Eligible", "Готово");
  if (item.reason === "ok") return t("Waiting for window", "Ждёт окна");
  if (item.reason === "below_minimum") return t("Below minimum", "Ниже минимума");
  if (item.reason === "no_wallet") return t("No wallet", "Нет кошелька");
  if (item.reason === "inactive") return t("Inactive", "Неактивен");
  if (item.reason === "zero") return t("No amount", "Нет суммы");
  return item.reason ?? t("Unavailable", "Нельзя");
}

const DueRow = memo(function DueRow({ item, minPayoutNano }: { item: PayoutDueItem; minPayoutNano?: string }) {
  const { t } = useI18n();
  const debtNano = /^\d+$/.test(item.debtNano || "") ? BigInt(item.debtNano!) : 0n;
  return (
    <tr>
      <td className="left">
        <b translate="no">{partnerName(item)}</b>
        <div className="sub mono" translate="no">{item.partnerId}</div>
      </td>
      <td>
        <b>{nanoMoney(item.payableNano)}</b>
        {debtNano > 0n ? <div className="sub partner-bad">{t("Debt", "Долг")} {nanoMoney(item.debtNano)}</div> : null}
      </td>
      <td className="left mono" translate="no" title={item.walletAddress || t("Not linked", "Не привязан")}>
        {item.walletAddress ? shortWallet(item.walletAddress) : "—"}
      </td>
      <td>
        <Pill kind={item.eligible ? "ok" : ""}>{dueReason(item, t)}</Pill>
        {item.reason === "below_minimum" ? <div className="sub">{t("min.", "мин.")} {nanoMoney(minPayoutNano)}</div> : null}
      </td>
    </tr>
  );
});

const AnalyticsRow = memo(function AnalyticsRow({ partner }: { partner: PartnerAnalyticsItem }) {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const debtNano = /^\d+$/.test(partner.debtNano || "") ? BigInt(partner.debtNano!) : 0n;
  return (
    <tr>
      <td className="left">
        <b translate="no">{partnerName(partner)}</b>
        <div className="sub mono" translate="no">
          {partner.id} · {partner.referralCode}
        </div>
      </td>
      <td>
        <Pill kind={partner.status === "active" ? "ok" : partner.status === "suspended" ? "bad" : "warn"}>
          {statusLabel(partner.status, t)}
        </Pill>
      </td>
      <td>
        {partner.referredUsers}
        <div className="sub">{t("converted", "конверсия")} {partner.convertedUsers}</div>
      </td>
      <td>
        {nanoMoney(partner.deposits30dNano)}
        <div className="sub">{t("total", "всего")} {nanoMoney(partner.depositsTotalNano)}</div>
      </td>
      <td>{nanoMoney(partner.spend30dNano)}</td>
      <td>
        {nanoMoney(partner.net30dNano)}
        <div className="sub">{t("net total", "net всего")} {nanoMoney(partner.netTotalNano)}</div>
      </td>
      <td>
        <b>{nanoMoney(partner.payableNano)}</b>
        {debtNano > 0n ? <div className="sub partner-bad">{t("Debt", "Долг")} {nanoMoney(partner.debtNano)}</div> : null}
      </td>
      <td>{ago(partner.lastSeenAt, locale)}</td>
    </tr>
  );
});

// История выплат: endpoint отдаёт только partnerId — имя подставляем из загруженной
// страницы аналитики (как в легаси); неизвестный id — укороченный mono-префикс.
const PayoutRow = memo(function PayoutRow({ item, name }: { item: PayoutItem; name?: string }) {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  return (
    <tr>
      <td className="left">
        {name ? (
          <>
            <b translate="no">{name}</b>
            <div className="sub mono" translate="no">{item.partnerId}</div>
          </>
        ) : (
          <b className="mono" translate="no">{(item.partnerId ?? "").slice(0, 8)}…</b>
        )}
      </td>
      <td>
        <b>{nanoMoney(item.amountNano)}</b>
      </td>
      <td>
        <Pill kind={(item.status && PAYOUT_STATUS_KIND[item.status]) || ""}>{statusLabel(item.status, t)}</Pill>
      </td>
      <td>{item.method || "—"}</td>
      <td>
        {ago(item.requestedAt, locale)}
        <div className="sub">{formatDate(item.requestedAt, true, locale)}</div>
      </td>
      <td>{item.decidedAt ? ago(item.decidedAt, locale) : "—"}</td>
      <td>{item.paidAt ? ago(item.paidAt, locale) : "—"}</td>
    </tr>
  );
});

// On-chain батчи (stretch): список отдаёт строки без txHash — hash живёт в report отдельного батча.
const BatchRow = memo(function BatchRow({ item }: { item: PayoutBatch }) {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  return (
    <tr>
      <td>
        <Pill kind={(item.status && BATCH_STATUS_KIND[item.status]) || ""}>{statusLabel(item.status, t)}</Pill>
      </td>
      <td>
        <b>{nanoMoney(item.totalNano)}</b>
      </td>
      <td>{item.recipientCount ?? "—"}</td>
      <td>{item.gasPriceGwei ?? "—"}</td>
      <td className="left mono" title={item.hotWalletAddress || ""}>
        {item.hotWalletAddress ? shortWallet(item.hotWalletAddress) : "—"}
      </td>
      <td>{formatDate(item.createdAt, true, locale)}</td>
      <td>{item.sentAt ? ago(item.sentAt, locale) : "—"}</td>
      <td>{item.completedAt ? ago(item.completedAt, locale) : "—"}</td>
      <td className="left">
        <div className="json" title={item.error || ""}>
          {item.error || "—"}
        </div>
      </td>
    </tr>
  );
});

// Пейджер 1:1 из admin-panel.js: «N–M из T» + Назад/Дальше.
function Pager(props: { offset: number; limit: number; total: number; onOffset: (offset: number) => void }) {
  const { t } = useI18n();
  const { offset, limit, total } = props;
  return (
    <div className="pager">
      <span>
        {total ? offset + 1 : 0}–{Math.min(offset + limit, total)} {t("of", "из")} {total}
      </span>
      <button
        type="button"
        className="btn ghost"
        disabled={offset <= 0}
        onClick={() => props.onOffset(Math.max(0, offset - limit))}
      >
        {t("Back", "Назад")}
      </button>
      <button
        type="button"
        className="btn ghost"
        disabled={offset + limit >= total}
        onClick={() => props.onOffset(offset + limit)}
      >
        {t("Next", "Дальше")}
      </button>
    </div>
  );
}

export default function PartnersPage() {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const router = useRouter();
  const search = useSearchParams();
  const sortParam = search.get("sort");
  const dirParam = search.get("dir");
  const rawOffset = Number(search.get("offset") ?? "0");
  const pageParams: PartnersPage = {
    offset: Number.isSafeInteger(rawOffset) && rawOffset >= 0 ? rawOffset : 0,
    limit: PAGE_LIMIT,
    sort: sortParam && PARTNER_SORTS.some(([value]) => value === sortParam) ? sortParam : "unpaid",
    dir: dirParam === "asc" || dirParam === "desc" ? dirParam : "desc",
  };

  const analyticsPath =
    "/partner-admin/partner-analytics?sort=" +
    pageParams.sort +
    "&dir=" +
    pageParams.dir +
    "&limit=" +
    pageParams.limit +
    "&offset=" +
    pageParams.offset;
  const { data: result, isLoading } = useResources<PartnersData>({
    overview: "/partner-admin/overview",
    engine: "/partner-admin/payouts/engine",
    due: "/partner-admin/payout-list",
    analytics: analyticsPath,
    payouts: "/partner-admin/payouts",
    batches: "/partner-admin/payouts/batches",
  });

  const analyticsTotal = result?.analytics?.totals?.total || 0;
  const effectiveOffset = clampOffset(pageParams.offset, pageParams.limit, analyticsTotal);

  useEffect(() => {
    if (result.analytics && effectiveOffset !== pageParams.offset) {
      router.replace(partnersUrl({
        offset: effectiveOffset,
        limit: PAGE_LIMIT,
        sort: pageParams.sort,
        dir: pageParams.dir,
      }), { scroll: false });
    }
  }, [effectiveOffset, pageParams.dir, pageParams.offset, pageParams.sort, result.analytics, router]);

  // Имена партнёров для истории выплат — из загруженной страницы аналитики.
  const names = useMemo(() => {
    const map: Record<string, string> = {};
    for (const partner of result?.analytics?.items ?? []) {
      if (partner.id) map[partner.id] = partnerName(partner);
    }
    return map;
  }, [result]);

  if (isLoading && Object.values(result).every((value) => value === undefined)) {
    return (
      <>
        <PageHead title={t("Partners", "Партнёры")} sub={t("Loading data; navigation is already available", "Данные загружаются, навигация уже доступна")} />
        <LoadingGrid label={t("Loading partner data", "Загрузка партнёрских данных")} />
      </>
    );
  }

  const { overview, engine, due, analytics, payouts, batches } = result;
  const offset = effectiveOffset;
  const degraded = !overview || !engine || !due || !analytics || !payouts || !batches;

  const applyPage = (patch: Partial<PartnersPage>) => {
    startTransition(() => router.replace(partnersUrl({ ...pageParams, ...patch }), { scroll: false }));
  };

  const win = engine?.window ?? {};

  const period = due?.period ?? {};
  const dueItems = due?.items ?? [];
  const wallet = engine ? payoutWalletReadiness(engine, dueItems) : null;
  const walletEnglish = wallet ? {
    title: wallet.kind === "ok"
      ? (wallet.eligibleCount ? "Balances cover the current payout list" : "Hot wallet is reachable")
      : wallet.kind === "warn"
        ? "Hot wallet is empty"
        : wallet.title.includes("USDT") || wallet.title.includes("BNB")
          ? `Insufficient ${[wallet.title.includes("USDT") ? "USDT" : "", wallet.title.includes("BNB") ? "BNB" : ""].filter(Boolean).join(" & ")}`
          : "Payout wallet is not ready",
    detail: wallet.kind === "ok"
      ? (wallet.eligibleCount
        ? `${wallet.eligibleCount} transfers are covered by USDT and BNB gas; the backend verifies balances again before sending.`
        : "BSC and USDT access are verified; no transfer is eligible in the current period.")
      : wallet.kind === "warn"
        ? "No transfer is currently eligible, but the next window requires both USDT and BNB gas."
        : "Sending remains blocked. Verify configuration, BSC RPC, the USDT contract and exact wallet balances, then refresh.",
  } : null;
  const chain = engine?.chain;
  const payoutItems = payouts?.items ?? [];
  const payoutsShown = payoutItems.slice(0, PAGE_LIMIT);
  const batchItems = batches?.items ?? [];
  const aTotals = analytics?.totals ?? {};

  return (
    <>
      <PageHead
        title={t("Partners", "Партнёры")}
        sub={t("Overview, payout window, payable balances and partner analytics", "Сводка, окно выплат, к выплате и аналитика рефоводов")}
        badge={
          <Pill kind={degraded ? "warn" : "ok"}>
            {analytics ? t(`${analyticsTotal} partners`, count(analyticsTotal, "партнёр", "партнёра", "партнёров")) : t("Degraded", "Частично недоступно")}
          </Pill>
        }
      />

      {overview ? (
        <CardGrid>
          <StatCard label={t("Partners", "Партнёры")} value={overview.partners ?? "—"} hint={`${t("active", "активны")} ${overview.activePartners ?? "—"}`} />
          <StatCard label={t("Referrals", "Рефералы")} value={overview.referredUsers ?? "—"} hint={t("attributed customers", "привлечённые клиенты")} />
          <StatCard label={t("Referral spend", "Оборот рефералов")} value={nanoMoney(overview.totalSpendNano)} hint={t("attributed customer usage", "расход привлечённых клиентов")} />
          <StatCard label={t("Net commissions", "Комиссии net")} value={nanoMoney(overview.totalNetCommissionsNano)} hint={`${nanoMoney(overview.totalAdjustmentsNano)} ${t("adjustments", "возвраты")}`} />
          <StatCard label={t("Available to pay", "Доступно к выплате")} value={nanoMoney(overview.totalPayableNano)} hint={`${nanoMoney(overview.totalDebtNano)} ${t("partner debt", "долг партнёров")}`} />
          <StatCard label={t("Paid", "Выплачено")} value={nanoMoney(overview.paidPayoutsNano)} hint={t("confirmed payouts", "подтверждённые выплаты")} />
        </CardGrid>
      ) : (
        <Banner kind="warn" title={t("Partner overview unavailable", "Партнёрская сводка недоступна")}>
          {t("/partner-admin/overview is unavailable; the remaining sections continue independently", "/partner-admin/overview не отвечает — остальные блоки ниже работают независимо")}
        </Banner>
      )}

      <SectionHeader title={t("Payout readiness", "Готовность выплат")} sub={t("BSC mainnet · USDT BEP-20 · read-only hot-wallet verification", "BSC mainnet · USDT BEP-20 · read-only проверка hot wallet")} />
      {!engine ? (
        <Banner kind="warn" title={t("Payout engine unavailable", "Состояние payout-движка недоступно")}>
          {t("/partner-admin/payouts/engine is unavailable", "/partner-admin/payouts/engine не отвечает")}
        </Banner>
      ) : (
        <>
          <Banner kind={wallet?.kind ?? "warn"} title={wallet ? t(walletEnglish!.title, wallet.title) : t("Wallet state unknown", "Состояние кошелька неизвестно")}>
            {wallet ? t(walletEnglish!.detail, wallet.detail) : null}
          </Banner>
          <CardGrid>
            <StatCard
              label="hot wallet"
              value={chain?.hotWalletAddress ? shortWallet(chain.hotWalletAddress) : "—"}
              hint={chain?.hotWalletAddress || t("address unavailable", "адрес не получен")}
              title={chain?.hotWalletAddress || undefined}
            />
            <StatCard
              label="USDT BEP-20"
              value={chain?.usdtBalanceNano == null ? "—" : nanoMoney(chain.usdtBalanceNano)}
              hint={`${t("current list requires", "текущий список требует")} ${nanoMoney(wallet?.requiredUsdtNano)}`}
            />
            <StatCard
              label={t("BNB for gas", "BNB для gas")}
              value={bnbMoney(chain?.bnbBalanceWei)}
              hint={`${t("current list requires", "текущий список требует")} ${bnbMoney(wallet?.requiredBnbWei)}`}
            />
            <StatCard
              label={t("Sending window", "Окно отправки")}
              value={win.open ? t("Open", "Открыто") : t("Closed", "Закрыто")}
              hint={
                win.enforced === false
                  ? t("window gate disabled", "гейт окна выключен")
                  : win.open
                    ? `${t("until", "до")} ${formatDate(win.closesAt, true, locale)}`
                    : win.opensAt
                      ? `${t("from", "с")} ${formatDate(win.opensAt, true, locale)}`
                      : t("no upcoming window scheduled", "ближайшее не запланировано")
              }
            />
          </CardGrid>
        </>
      )}

      {!due ? (
        <Banner kind="warn" title={t("Payable list unavailable", "Список «к выплате» недоступен")}>
          {t("/partner-admin/payout-list is unavailable", "/partner-admin/payout-list не отвечает")}
        </Banner>
      ) : (
        <>
          <SectionHeader
            title={t("Payable for the period", "К выплате за период")}
            sub={`${period.key || "—"} · ${formatDate(period.start, false, locale)} – ${formatDate(period.end, false, locale)} · ${t("phase", "фаза")} ${
              period.phase ? t({ accruing: "Accruing", locked: "7-day lock", payable: "Payout window", closed: "Closed" }[period.phase] ?? period.phase, PARTNER_PHASE_LABEL[period.phase] ?? period.phase) : "—"
            } · ${t("window", "окно")} ${formatDate(period.payoutWindowStart, false, locale)} – ${formatDate(period.payoutWindowEnd, false, locale)} · ${t("eligible", "eligible")} ${nanoMoney(
              eligibleSumNano(dueItems),
            )}`}
          />
          <TableCard>
            <table>
              <DueHead />
              <tbody>
                {dueItems.length ? (
                  dueItems.map((item) => <DueRow key={item.partnerId} item={item} minPayoutNano={due.minPayoutNano} />)
                ) : (
                  <EmptyRow columns={4} text={t("No payable balances", "Нет сумм к выплате")} />
                )}
              </tbody>
            </table>
          </TableCard>
        </>
      )}

      {!analytics ? (
        <Banner kind="warn" title={t("Partner analytics unavailable", "Аналитика партнёров недоступна")}>
          {t("/partner-admin/partner-analytics is unavailable", "/partner-admin/partner-analytics не отвечает")}
        </Banner>
      ) : (
        <>
          <SectionHeader
            title={t("Partner analytics", "Аналитика партнёров")}
            sub={`${aTotals.total ?? "—"} · ${t("active", "активны")} ${aTotals.active ?? "—"} · ${t("payable", "к выплате")} ${nanoMoney(aTotals.payableNano)} · ${t("debt", "долг")} ${nanoMoney(aTotals.debtNano)}`}
          />
          <form
            className="toolbar"
            onSubmit={(event) => {
              event.preventDefault();
              applyPage({ offset: 0 });
            }}
          >
            <label className="sr-only" htmlFor="pa-sort">
              {t("Sort field", "Поле сортировки")}
            </label>
            <select
              id="pa-sort"
              name="partnerAnalyticsSort"
              value={pageParams.sort}
              onChange={(event) => applyPage({ offset: 0, sort: event.target.value })}
            >
              {PARTNER_SORTS.map(([value, label]) => (
                <option key={value} value={value}>
                  {t({ unpaid: "Payable", deposits_total: "Total top-ups", deposits_30d: "Top-ups · 30d", earned_total: "Total earnings", earned_30d: "Earnings · 30d", spend_total: "Total spend", spend_30d: "Spend · 30d", converted_users: "Conversions", referred_users: "Referrals", team_size: "Team", last_seen_at: "Activity", created_at: "Registration" }[value] ?? value, label)}
                </option>
              ))}
            </select>
            <label className="sr-only" htmlFor="pa-dir">
              {t("Direction", "Направление")}
            </label>
            <select
              id="pa-dir"
              name="partnerAnalyticsDirection"
              value={pageParams.dir}
              onChange={(event) => applyPage({ offset: 0, dir: event.target.value })}
            >
              <option value="desc">{t("Descending", "По убыванию")}</option>
              <option value="asc">{t("Ascending", "По возрастанию")}</option>
            </select>
            <button className="btn" type="submit">
              {t("Apply", "Применить")}
            </button>
          </form>
          <TableCard>
            <table>
              <AnalyticsHead />
              <tbody>
                {analytics.items?.length ? (
                  analytics.items.map((partner) => <AnalyticsRow key={partner.id} partner={partner} />)
                ) : (
                  <EmptyRow columns={8} text={t("No partner analytics", "Нет данных аналитики партнёров")} />
                )}
              </tbody>
            </table>
          </TableCard>
          <Pager
            offset={offset}
            limit={pageParams.limit}
            total={aTotals.total || 0}
            onOffset={(next) => applyPage({ offset: next })}
          />
        </>
      )}

      {!payouts ? (
        <Banner kind="warn" title={t("Payout history unavailable", "История выплат недоступна")}>
          {t("/partner-admin/payouts is unavailable", "/partner-admin/payouts не отвечает")}
        </Banner>
      ) : (
        <>
          <SectionHeader
            title={t("Payouts", "Выплаты")}
            sub={`${t("history", "история")} · ${payoutItems.length}${
              payoutItems.length > payoutsShown.length ? ` · ${t("latest shown", "показаны последние")} ${payoutsShown.length}` : ""
            }`}
          />
          <TableCard>
            <table>
              <PayoutsHead />
              <tbody>
                {payoutsShown.length ? (
                  payoutsShown.map((item, index) => (
                    <PayoutRow key={item.partnerId ?? index} item={item} name={item.partnerId ? names[item.partnerId] : undefined} />
                  ))
                ) : (
                  <EmptyRow columns={7} text={t("No payout history", "Истории выплат нет")} />
                )}
              </tbody>
            </table>
          </TableCard>
        </>
      )}

      {!batches ? (
        <Banner kind="warn" title={t("On-chain batches unavailable", "On-chain батчи недоступны")}>
          {t("/partner-admin/payouts/batches is unavailable", "/partner-admin/payouts/batches не отвечает")}
        </Banner>
      ) : (
        <details>
          <summary>{t("On-chain batches", "On-chain батчи")} · {batchItems.length}</summary>
          <TableCard>
            <table>
              <BatchesHead />
              <tbody>
                {batchItems.length ? (
                  batchItems.map((item, index) => <BatchRow key={index} item={item} />)
                ) : (
                  <EmptyRow columns={9} text={t("No on-chain batches", "On-chain пакетов нет")} />
                )}
              </tbody>
            </table>
          </TableCard>
        </details>
      )}

      <footer>{t(
        "Data updates from sales-api events; use ↻ for an explicit recheck. Every amount is received and processed as integer nanoUSD. Decisions, onboarding, authority and payouts now live in this section and record the authenticated admin actor.",
        "Данные обновляются по событиям sales-api; кнопка ↻ остаётся для явной перепроверки. Все суммы приходят и обрабатываются как целочисленные nanoUSD. Решения, онбординг, права и выплаты теперь выполняются в маршрутах этого раздела и записывают аккаунт вошедшего администратора.",
      )}</footer>
    </>
  );
}
