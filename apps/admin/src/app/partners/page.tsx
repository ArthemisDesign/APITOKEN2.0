"use client";

// Партнёры — порт 1:1 функции partners() из crates/server/src/admin-panel.js.
// Шесть источников одним Promise.all (каждый деградирует молча в null), автоопроса
// нет — ручное обновление по кнопке ↻ и ревалидация на фокусе (как в легаси).
// Страница read-only: все мутации partner-контура живут на admin.partners.apitoken.sale.
import { api } from "@/lib/api";
import { usePoll } from "@/lib/usePoll";
import { ago, count, formatDate, nanoMoney } from "@/lib/format";
import { Banner, CardGrid, EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, StatCard, TableCard } from "@/components/ui";
import { memo, startTransition, useMemo, useState } from "react";
import {
  BATCH_STATUS_KIND,
  PARTNER_PHASE_LABEL,
  PARTNER_SORTS,
  PAYOUT_STATUS_KIND,
  clampOffset,
  eligibleSumNano,
  partnerName,
  payoutReasonText,
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

interface PartnersData {
  overview: PartnerOverview | null;
  engine: PayoutEngine | null;
  due: PayoutDue | null;
  analytics: PartnerAnalytics | null;
  payouts: PayoutHistory | null;
  batches: PayoutBatches | null;
  /** Фактический offset аналитики после клампа по total (см. loadPartners). */
  offset: number;
}

// Все шесть источников параллельно; падение любого → null, блок рендерит warn-баннер.
async function loadPartners(page: PartnersPage): Promise<PartnersData> {
  const analyticsQuery =
    "?sort=" + page.sort + "&dir=" + page.dir + "&limit=" + page.limit + "&offset=" + page.offset;
  const [overview, engine, due, firstAnalytics, payouts, batches] = await Promise.all([
    api<PartnerOverview>("/partner-admin/overview").catch(() => null),
    api<PayoutEngine>("/partner-admin/payouts/engine").catch(() => null),
    api<PayoutDue>("/partner-admin/payout-list").catch(() => null),
    api<PartnerAnalytics>("/partner-admin/partner-analytics" + analyticsQuery).catch(() => null),
    api<PayoutHistory>("/partner-admin/payouts").catch(() => null),
    api<PayoutBatches>("/partner-admin/payouts/batches").catch(() => null),
  ]);
  // Кламп offset, если total сократился под текущую страницу: легаси перезапрашивает
  // последнюю полную страницу и перерисовывает — здесь повторный fetch в том же проходе.
  const total = firstAnalytics?.totals?.total || 0;
  const offset = clampOffset(page.offset, page.limit, total);
  const analytics =
    firstAnalytics && offset !== page.offset
      ? await api<PartnerAnalytics>(
          "/partner-admin/partner-analytics?sort=" +
            page.sort +
            "&dir=" +
            page.dir +
            "&limit=" +
            page.limit +
            "&offset=" +
            offset,
        ).catch(() => null)
      : firstAnalytics;
  return { overview, engine, due, analytics, payouts, batches, offset };
}

const ANALYTICS_HEAD = (
  <thead>
    <tr>
      <th className="left">партнёр</th>
      <th>статус</th>
      <th>рефералы</th>
      <th>пополнения 30д</th>
      <th>расход 30д</th>
      <th>заработок 30д</th>
      <th>к выплате</th>
      <th>активность</th>
    </tr>
  </thead>
);

const DUE_HEAD = (
  <thead>
    <tr>
      <th className="left">партнёр</th>
      <th>к выплате</th>
      <th className="left">кошелёк BEP-20</th>
      <th>eligible</th>
    </tr>
  </thead>
);

const PAYOUTS_HEAD = (
  <thead>
    <tr>
      <th className="left">партнёр</th>
      <th>сумма</th>
      <th>статус</th>
      <th>метод</th>
      <th>запрошена</th>
      <th>решена</th>
      <th>выплачена</th>
    </tr>
  </thead>
);

const BATCHES_HEAD = (
  <thead>
    <tr>
      <th>статус</th>
      <th>сумма</th>
      <th>получатели</th>
      <th>gas, gwei</th>
      <th className="left">hot wallet</th>
      <th>создан</th>
      <th>отправлен</th>
      <th>завершён</th>
      <th className="left">ошибка</th>
    </tr>
  </thead>
);

const DueRow = memo(function DueRow({ item, minPayoutNano }: { item: PayoutDueItem; minPayoutNano?: string }) {
  const debtNano = /^\d+$/.test(item.debtNano || "") ? BigInt(item.debtNano!) : 0n;
  return (
    <tr>
      <td className="left">
        <b>{partnerName(item)}</b>
        <div className="sub mono">{item.partnerId}</div>
      </td>
      <td>
        <b>{nanoMoney(item.payableNano)}</b>
        {debtNano > 0n ? <div className="sub" style={{ color: "var(--bad)" }}>долг {nanoMoney(item.debtNano)}</div> : null}
      </td>
      <td className="left mono" title={item.walletAddress || "не привязан"}>
        {item.walletAddress ? shortWallet(item.walletAddress) : "—"}
      </td>
      <td>
        <Pill kind={item.eligible ? "ok" : ""}>{payoutReasonText(item)}</Pill>
        {item.reason === "below_minimum" ? <div className="sub">мин. {nanoMoney(minPayoutNano)}</div> : null}
      </td>
    </tr>
  );
});

const AnalyticsRow = memo(function AnalyticsRow({ partner }: { partner: PartnerAnalyticsItem }) {
  const debtNano = /^\d+$/.test(partner.debtNano || "") ? BigInt(partner.debtNano!) : 0n;
  return (
    <tr>
      <td className="left">
        <b>{partnerName(partner)}</b>
        <div className="sub mono">
          {partner.id} · {partner.referralCode}
        </div>
      </td>
      <td>
        <Pill kind={partner.status === "active" ? "ok" : partner.status === "suspended" ? "bad" : "warn"}>
          {partner.status}
        </Pill>
      </td>
      <td>
        {partner.referredUsers}
        <div className="sub">конверсия {partner.convertedUsers}</div>
      </td>
      <td>
        {nanoMoney(partner.deposits30dNano)}
        <div className="sub">всего {nanoMoney(partner.depositsTotalNano)}</div>
      </td>
      <td>{nanoMoney(partner.spend30dNano)}</td>
      <td>
        {nanoMoney(partner.net30dNano)}
        <div className="sub">net всего {nanoMoney(partner.netTotalNano)}</div>
      </td>
      <td>
        <b>{nanoMoney(partner.payableNano)}</b>
        {debtNano > 0n ? <div className="sub" style={{ color: "var(--bad)" }}>долг {nanoMoney(partner.debtNano)}</div> : null}
      </td>
      <td>{ago(partner.lastSeenAt)}</td>
    </tr>
  );
});

// История выплат: endpoint отдаёт только partnerId — имя подставляем из загруженной
// страницы аналитики (как в легаси); неизвестный id — укороченный mono-префикс.
const PayoutRow = memo(function PayoutRow({ item, name }: { item: PayoutItem; name?: string }) {
  return (
    <tr>
      <td className="left">
        {name ? (
          <>
            <b>{name}</b>
            <div className="sub mono">{item.partnerId}</div>
          </>
        ) : (
          <b className="mono">{(item.partnerId ?? "").slice(0, 8)}…</b>
        )}
      </td>
      <td>
        <b>{nanoMoney(item.amountNano)}</b>
      </td>
      <td>
        <Pill kind={(item.status && PAYOUT_STATUS_KIND[item.status]) || ""}>{item.status}</Pill>
      </td>
      <td>{item.method || "—"}</td>
      <td>
        {ago(item.requestedAt)}
        <div className="sub">{formatDate(item.requestedAt, true)}</div>
      </td>
      <td>{item.decidedAt ? ago(item.decidedAt) : "—"}</td>
      <td>{item.paidAt ? ago(item.paidAt) : "—"}</td>
    </tr>
  );
});

// On-chain батчи (stretch): список отдаёт строки без txHash — hash живёт в report отдельного батча.
const BatchRow = memo(function BatchRow({ item }: { item: PayoutBatch }) {
  return (
    <tr>
      <td>
        <Pill kind={(item.status && BATCH_STATUS_KIND[item.status]) || ""}>{item.status}</Pill>
      </td>
      <td>
        <b>{nanoMoney(item.totalNano)}</b>
      </td>
      <td>{item.recipientCount ?? "—"}</td>
      <td>{item.gasPriceGwei ?? "—"}</td>
      <td className="left mono" title={item.hotWalletAddress || ""}>
        {item.hotWalletAddress ? shortWallet(item.hotWalletAddress) : "—"}
      </td>
      <td>{formatDate(item.createdAt, true)}</td>
      <td>{item.sentAt ? ago(item.sentAt) : "—"}</td>
      <td>{item.completedAt ? ago(item.completedAt) : "—"}</td>
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
  const { offset, limit, total } = props;
  return (
    <div className="pager">
      <span>
        {total ? offset + 1 : 0}–{Math.min(offset + limit, total)} из {total}
      </span>
      <button
        type="button"
        className="btn ghost"
        disabled={offset <= 0}
        onClick={() => props.onOffset(Math.max(0, offset - limit))}
      >
        Назад
      </button>
      <button
        type="button"
        className="btn ghost"
        disabled={offset + limit >= total}
        onClick={() => props.onOffset(offset + limit)}
      >
        Дальше
      </button>
    </div>
  );
}

export default function PartnersPage() {
  const [pageParams, setPageParams] = useState<PartnersPage>({
    offset: 0,
    limit: PAGE_LIMIT,
    sort: "unpaid",
    dir: "desc",
  });

  const analyticsPath =
    "/partner-admin/partner-analytics?sort=" +
    pageParams.sort +
    "&dir=" +
    pageParams.dir +
    "&limit=" +
    pageParams.limit +
    "&offset=" +
    pageParams.offset;
  const { data: result } = usePoll(analyticsPath, () => loadPartners(pageParams));

  const analyticsTotal = result?.analytics?.totals?.total || 0;

  // Имена партнёров для истории выплат — из загруженной страницы аналитики.
  const names = useMemo(() => {
    const map: Record<string, string> = {};
    for (const partner of result?.analytics?.items ?? []) {
      if (partner.id) map[partner.id] = partnerName(partner);
    }
    return map;
  }, [result]);

  if (!result) {
    return (
      <>
        <PageHead title="Партнёры" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const { overview, engine, due, analytics, payouts, batches, offset } = result;
  const degraded = !overview || !engine || !due || !analytics || !payouts || !batches;

  const applyPage = (patch: Partial<PartnersPage>) => {
    startTransition(() => setPageParams((prev) => ({ ...prev, ...patch })));
  };

  // Окно выплат payout-движка: configured + window{open,opensAt,closesAt,enforced}.
  // Адрес hot wallet этот endpoint не отдаёт — он виден только в батчах.
  const win = engine?.window ?? {};
  const windowKind: "ok" | "warn" = engine?.configured && win.open ? "ok" : "warn";

  const period = due?.period ?? {};
  const dueItems = due?.items ?? [];
  const payoutItems = payouts?.items ?? [];
  const payoutsShown = payoutItems.slice(0, PAGE_LIMIT);
  const batchItems = batches?.items ?? [];
  const aTotals = analytics?.totals ?? {};

  return (
    <>
      <PageHead
        title="Партнёры"
        sub="сводка, окно выплат, к выплате и аналитика рефоводов"
        badge={
          <Pill kind={degraded ? "warn" : "ok"}>
            {analytics ? count(analyticsTotal, "партнёр", "партнёра", "партнёров") : "degraded"}
          </Pill>
        }
      />

      {overview ? (
        <CardGrid>
          <StatCard label="партнёры" value={overview.partners ?? "—"} hint={`активны ${overview.activePartners ?? "—"}`} />
          <StatCard label="рефералы" value={overview.referredUsers ?? "—"} hint="привлечённые клиенты" />
          <StatCard label="оборот рефералов" value={nanoMoney(overview.totalSpendNano)} hint="расход привлечённых клиентов" />
          <StatCard label="комиссии net" value={nanoMoney(overview.totalNetCommissionsNano)} hint={`${nanoMoney(overview.totalAdjustmentsNano)} возвраты`} />
          <StatCard label="доступно к выплате" value={nanoMoney(overview.totalPayableNano)} hint={`${nanoMoney(overview.totalDebtNano)} долг партнёров`} />
          <StatCard label="выплачено" value={nanoMoney(overview.paidPayoutsNano)} hint="статус paid" />
        </CardGrid>
      ) : (
        <Banner kind="warn" title="Партнёрская сводка недоступна">
          /partner-admin/overview не отвечает — остальные блоки ниже работают независимо
        </Banner>
      )}

      <SectionHeader title="Окно выплат" sub="payout-движок sales-api" />
      {!engine ? (
        <Banner kind="warn" title="Состояние окна выплат недоступно">
          /partner-admin/payouts/engine не отвечает
        </Banner>
      ) : (
        <Banner
          kind={windowKind}
          dot={windowKind === "ok" ? "" : "warn"}
          title={!engine.configured ? "Payout-движок не настроен" : win.open ? "Окно выплат открыто" : "Окно выплат закрыто"}
        >
          {!engine.configured
            ? "нет hot-wallet ключа или send RPC — on-chain отправки недоступны"
            : win.enforced === false
              ? "гейт окна выключен — отправка разрешена в любое время"
              : win.open
                ? "закроется " + formatDate(win.closesAt, true)
                : win.opensAt
                  ? "откроется " + formatDate(win.opensAt, true)
                  : "ближайшее окно не запланировано"}
        </Banner>
      )}

      {!due ? (
        <Banner kind="warn" title="Список «к выплате» недоступен">
          /partner-admin/payout-list не отвечает
        </Banner>
      ) : (
        <>
          <SectionHeader
            title="К выплате за период"
            sub={`${period.key || "—"} · ${formatDate(period.start)} – ${formatDate(period.end)} · фаза ${
              (period.phase && PARTNER_PHASE_LABEL[period.phase]) || period.phase || "—"
            } · окно ${formatDate(period.payoutWindowStart)} – ${formatDate(period.payoutWindowEnd)} · eligible на ${nanoMoney(
              eligibleSumNano(dueItems),
            )}`}
          />
          <TableCard>
            <table>
              {DUE_HEAD}
              <tbody>
                {dueItems.length ? (
                  dueItems.map((item) => <DueRow key={item.partnerId} item={item} minPayoutNano={due.minPayoutNano} />)
                ) : (
                  <EmptyRow columns={4} />
                )}
              </tbody>
            </table>
          </TableCard>
        </>
      )}

      {!analytics ? (
        <Banner kind="warn" title="Аналитика партнёров недоступна">
          /partner-admin/partner-analytics не отвечает
        </Banner>
      ) : (
        <>
          <SectionHeader
            title="Аналитика партнёров"
            sub={`${aTotals.total ?? "—"} · активны ${aTotals.active ?? "—"} · к выплате ${nanoMoney(aTotals.payableNano)} · долг ${nanoMoney(aTotals.debtNano)}`}
          />
          <form
            className="toolbar"
            onSubmit={(event) => {
              event.preventDefault();
              applyPage({ offset: 0 });
            }}
          >
            <label className="sr-only" htmlFor="pa-sort">
              Поле сортировки
            </label>
            <select
              id="pa-sort"
              value={pageParams.sort}
              onChange={(event) => applyPage({ offset: 0, sort: event.target.value })}
            >
              {PARTNER_SORTS.map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
            <label className="sr-only" htmlFor="pa-dir">
              Направление
            </label>
            <select
              id="pa-dir"
              value={pageParams.dir}
              onChange={(event) => applyPage({ offset: 0, dir: event.target.value })}
            >
              <option value="desc">по убыванию</option>
              <option value="asc">по возрастанию</option>
            </select>
            <button className="btn" type="submit">
              Применить
            </button>
          </form>
          <TableCard>
            <table>
              {ANALYTICS_HEAD}
              <tbody>
                {analytics.items?.length ? (
                  analytics.items.map((partner) => <AnalyticsRow key={partner.id} partner={partner} />)
                ) : (
                  <EmptyRow columns={8} />
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
        <Banner kind="warn" title="История выплат недоступна">
          /partner-admin/payouts не отвечает
        </Banner>
      ) : (
        <>
          <SectionHeader
            title="Выплаты"
            sub={`история · ${payoutItems.length}${
              payoutItems.length > payoutsShown.length ? " · показаны последние " + payoutsShown.length : ""
            }`}
          />
          <TableCard>
            <table>
              {PAYOUTS_HEAD}
              <tbody>
                {payoutsShown.length ? (
                  payoutsShown.map((item, index) => (
                    <PayoutRow key={item.partnerId ?? index} item={item} name={item.partnerId ? names[item.partnerId] : undefined} />
                  ))
                ) : (
                  <EmptyRow columns={7} />
                )}
              </tbody>
            </table>
          </TableCard>
        </>
      )}

      {!batches ? (
        <Banner kind="warn" title="On-chain батчи недоступны">
          /partner-admin/payouts/batches не отвечает
        </Banner>
      ) : (
        <details>
          <summary>On-chain батчи · {batchItems.length}</summary>
          <TableCard>
            <table>
              {BATCHES_HEAD}
              <tbody>
                {batchItems.length ? (
                  batchItems.map((item, index) => <BatchRow key={index} item={item} />)
                ) : (
                  <EmptyRow columns={9} />
                )}
              </tbody>
            </table>
          </TableCard>
        </details>
      )}

      <footer>
        Ручное обновление по кнопке ↻ — автообновления у вкладки нет. Суммы — nanoUSD-строки sales-api. Подготовка и
        отправка батчей, решения по заявкам и полный partner workflow остаются на{" "}
        <a className="link" href="https://admin.partners.apitoken.sale/admin" target="_blank" rel="noreferrer">
          admin.partners.apitoken.sale ↗
        </a>
        .
      </footer>
    </>
  );
}
