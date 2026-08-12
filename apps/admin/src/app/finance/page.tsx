"use client";

// Финансы — порт 1:1 функции finance() из crates/server/src/admin-panel.js.
// Read-only агрегаты commerce БД (/admin/finance/*), здоровье денежных пайплайнов
// (/admin/pipeline-health) и settlement движка (/settlement-health). Девять
// источников грузятся и обновляются независимо: быстрый блок не ждёт медленный,
// а SSE-инвалидация перезапрашивает только затронутый URL.
import { memo, startTransition, useEffect, useMemo, useState } from "react";
import { useResources } from "@/lib/resources";
import { ageText, ago, duration, formatDate, money, nanoMoney } from "@/lib/format";
import {
  Banner,
  CardGrid,
  EmptyRow,
  LoadingGrid,
  PageHead,
  Pill,
  SectionHeader,
  StatCard,
  TableCard,
  type Tone,
} from "@/components/ui";
import { ChartCard, LineChart } from "./line-chart";
import {
  FINANCE_WINDOWS,
  REFUND_PAGE_LIMIT,
  buildRevenueSeries,
  clampPercent,
  clampRefundOffset,
  funnelShare,
  customerClassName,
  type AdminRefunds,
  type FinanceChurn,
  type FinanceCohorts,
  type FinanceFunnel,
  type FinanceOverview,
  type FinanceRevenue,
  type FinanceTopCustomers,
  type PipelineHealth,
  type RefundRow,
  type SettlementHealth,
  type TopCustomer,
} from "./finance-lib";

interface FinanceData {
  overview: FinanceOverview;
  revenue: FinanceRevenue;
  funnel: FinanceFunnel;
  top: FinanceTopCustomers;
  refunds: AdminRefunds;
  cohorts: FinanceCohorts;
  churn: FinanceChurn;
  pipes: PipelineHealth;
  settle: SettlementHealth;
}

// plainBar из легаси: акцентная полоса + подпись процента (без warn/bad-раскраски).
function PlainBar(props: { percent: number }) {
  const percent = clampPercent(props.percent);
  return (
    <>
      <span className="bar">
        <i style={{ width: `${percent}%` }} />
      </span>
      <span className="bar-label">{percent}%</span>
    </>
  );
}

// pager из легаси: «Назад/Дальше» с диапазоном «offset+1–min из total».
function Pager(props: { offset: number; limit: number; total: number; onPage: (offset: number) => void }) {
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
        onClick={() => props.onPage(Math.max(0, offset - limit))}
      >
        Назад
      </button>
      <button
        type="button"
        className="btn ghost"
        disabled={offset + limit >= total}
        onClick={() => props.onPage(offset + limit)}
      >
        Дальше
      </button>
    </div>
  );
}

// errCell из легаси: текст ошибки обрезан CSS (.json), полный — в title.
function ErrCell(props: { value?: string }) {
  return (
    <td className="left">
      <div className="json" title={props.value || ""}>
        {props.value || "—"}
      </div>
    </td>
  );
}

// Строка «Топ клиентов» (по пополнениям и по расходу — одинаковая разметка).
const TopCustomerRow = memo(function TopCustomerRow(props: {
  item: TopCustomer;
  index: number;
  moneyField: "total_usd" | "spent_usd";
  withCount: boolean;
}) {
  const { item } = props;
  return (
    <tr>
      <td>{props.index + 1}</td>
      <td className="left">
        <b>{item.email}</b>
        <div className="sub mono">{item.user_id}</div>
      </td>
      <td>
        <b>{money(item[props.moneyField])}</b>
        {props.withCount ? <div className="sub">{item.payments_count} шт.</div> : null}
      </td>
      <td>{item.share_pct == null ? "—" : <PlainBar percent={item.share_pct} />}</td>
    </tr>
  );
});

const RefundRowView = memo(function RefundRowView(props: { item: RefundRow }) {
  const { item } = props;
  return (
    <tr>
      <td className="left">
        <b>{item.email}</b>
        <div className="sub mono">{item.user_id}</div>
      </td>
      <td>
        <Pill>{item.provider}</Pill>
      </td>
      <td>
        <b>{money(item.amount_usd)}</b>
      </td>
      <td>
        <Pill kind={item.status === "refunded" ? "warn" : "bad"}>{item.status}</Pill>
      </td>
      <td title={item.adjustment_last_error || ""}>
        <Pill kind={item.adjustment_status === "confirmed" ? "ok" : item.adjustment_status ? "warn" : "bad"}>
          {item.adjustment_status || "нет компенсации"}
        </Pill>
        {item.adjustment_confirmed_at ? <div className="sub">{formatDate(item.adjustment_confirmed_at, true)}</div> : null}
      </td>
      <td>{formatDate(item.paid_at, true)}</td>
      <td>{formatDate(item.updated_at, true)}</td>
      <td className="left mono muted">{item.provider_payment_id}</td>
    </tr>
  );
});

function WarnBanner(props: { title: string; children: string }) {
  return (
    <Banner kind="warn" title={props.title}>
      {props.children}
    </Banner>
  );
}

export default function FinancePage() {
  const [windowDays, setWindowDays] = useState(30);
  const [refundOffset, setRefundOffset] = useState(0);
  const { data: result, availability, isLoading } = useResources<FinanceData>({
    overview: "/admin/finance/overview",
    revenue: `/admin/finance/revenue?days=${windowDays}`,
    funnel: "/admin/finance/funnel?days=30",
    top: "/admin/finance/top-customers?days=30&limit=20",
    refunds: `/admin/refunds?limit=${REFUND_PAGE_LIMIT}&offset=${refundOffset}`,
    cohorts: "/admin/finance/cohorts?weeks=8",
    churn: "/admin/finance/churn-signals?days=14",
    pipes: "/admin/pipeline-health",
    settle: "/settlement-health",
  });

  // Если список возвратов сократился, новая последняя страница получает собственный
  // URL. Остальные восемь источников при этом остаются в кэше и не запрашиваются.
  useEffect(() => {
    const clamped = clampRefundOffset(refundOffset, REFUND_PAGE_LIMIT, result.refunds?.total);
    if (clamped != null) startTransition(() => setRefundOffset(clamped));
  }, [refundOffset, result.refunds?.total]);

  const chartSeries = useMemo(() => (result?.revenue ? buildRevenueSeries(result.revenue) : []), [result]);

  if (isLoading && Object.values(result).every((value) => value === undefined)) {
    return (
      <>
        <PageHead title="Финансы" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const { overview, revenue, funnel, top, refunds, cohorts, churn, pipes, settle } = result;

  const windowTitle = (FINANCE_WINDOWS.find(([days]) => days === windowDays) ?? ([0, ""] as const))[1];

  // ── Сводка: выручка с дельтой к предыдущим 30д, ARPPU/ARPU, доля платящих, классы клиентов ──
  const ov = overview ?? {};
  const delta = ov.revenue_delta_pct;
  const deltaText = delta == null ? "—" : (delta > 0 ? "+" : "") + delta + "%";
  const classText = (ov.customer_classes ?? []).map((item) => `${customerClassName(item.customer_class ?? "")} ${item.users}`).join(" · ");
  const overviewBlock = overview ? (
    <CardGrid>
      <StatCard
        label="выручка 30 дней"
        value={money(ov.revenue_30d_usd)}
        hint={`пред. ${money(ov.revenue_prev_30d_usd)} · дельта ${deltaText}`}
      />
      <StatCard
        label="ARPPU 30д"
        value={ov.arppu_30d_usd == null ? "—" : money(ov.arppu_30d_usd)}
        hint={`ARPU ${ov.arpu_30d_usd == null ? "—" : money(ov.arpu_30d_usd)} · средний чек ${ov.avg_check_30d_usd == null ? "—" : money(ov.avg_check_30d_usd)}`}
      />
      <StatCard
        label="платящие 30д"
        value={ov.paying_users_30d ?? "—"}
        hint={`доля ${ov.paying_share_pct == null ? "—" : ov.paying_share_pct + "%"} от ${ov.active_users_30d ?? "—"} активных · платежей ${ov.payments_30d_count ?? "—"}`}
      />
      <StatCard
        label="классы клиентов"
        value={(ov.customer_classes ?? []).reduce((sum, item) => sum + Number(item.users || 0), 0) || "—"}
        hint={classText || "профилей нет"}
      />
    </CardGrid>
  ) : availability.overview === "error" ? (
    <WarnBanner title="Финансовая сводка недоступна">
      /admin/finance/overview не отвечает — остальные блоки ниже работают независимо
    </WarnBanner>
  ) : <LoadingGrid count={4} />;

  // ── Выручка: SVG-график по дням, итог + линия на провайдера ──
  const revenueTotals = revenue?.totals ?? {};
  const chartBlock = availability.revenue === "error" ? (
    <WarnBanner title="Ряд выручки недоступен">/admin/finance/revenue не отвечает</WarnBanner>
  ) : !revenue ? (
    <LoadingGrid count={1} />
  ) : (
    <ChartCard
      title="Выручка по дням"
      sub={`окно ${windowTitle.toLowerCase()} · итого ${money(revenueTotals.total_usd)} · ${revenueTotals.payments_count ?? "—"} платежей`}
    >
      <LineChart series={chartSeries} fmt={money} />
    </ChartCard>
  );

  // ── Воронка чекаутов за 30д: доли от созданных барами, провайдеры таблицей ──
  let funnelBlock = availability.funnel === "error" ? (
    <WarnBanner title="Воронка чекаутов недоступна">/admin/finance/funnel не отвечает</WarnBanner>
  ) : <LoadingGrid count={3} />;
  if (funnel) {
    const ft = funnel.totals ?? {};
    const created = Number(ft.created) || 0;
    const stageRows: [string, number | undefined][] = [
      ["создано чекаутов", ft.created],
      ["оплачено", ft.paid],
      ["отменено", ft.canceled],
      ["ошибка провайдера", ft.failed],
      ["истекло без оплаты", ft.expired],
      ["ждут оплаты", ft.pending],
    ];
    const providerRows = funnel.by_provider ?? [];
    funnelBlock = (
      <>
        <CardGrid>
          <StatCard
            label="конверсия в оплату"
            value={ft.conversion_pct == null ? "—" : ft.conversion_pct + "%"}
            hint={`время до оплаты ${ft.avg_seconds_to_pay == null ? "—" : duration(ft.avg_seconds_to_pay)} · средний чек ${ft.avg_check_usd == null ? "—" : money(ft.avg_check_usd)}`}
          />
          <StatCard
            label="создано чекаутов"
            value={created}
            hint={`оплачено ${ft.paid ?? "—"} · ждут ${ft.pending ?? "—"}`}
          />
          <StatCard
            label="потери воронки"
            value={Number(ft.canceled || 0) + Number(ft.failed || 0) + Number(ft.expired || 0)}
            hint={`отменено ${ft.canceled ?? "—"} · ошибки ${ft.failed ?? "—"} · истекло ${ft.expired ?? "—"}`}
          />
        </CardGrid>
        <TableCard>
          <table>
            <thead>
              <tr>
                <th className="left">стадия за 30 дней</th>
                <th>число</th>
                <th>доля от созданных</th>
              </tr>
            </thead>
            <tbody>
              {stageRows.map(([label, value]) => (
                <tr key={label}>
                  <td className="left">{label}</td>
                  <td>
                    <b>{Number(value || 0)}</b>
                  </td>
                  <td>
                    <PlainBar percent={funnelShare(value, created)} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </TableCard>
        {providerRows.length ? (
          <div style={{ marginTop: 12 }}>
            <TableCard>
              <table>
                <thead>
                  <tr>
                    <th className="left">провайдер</th>
                    <th>создано</th>
                    <th>оплачено</th>
                    <th>конверсия</th>
                    <th>время до оплаты</th>
                    <th>средний чек</th>
                  </tr>
                </thead>
                <tbody>
                  {providerRows.map((item) => (
                    <tr key={item.provider}>
                      <td className="left">
                        <b>{item.provider}</b>
                      </td>
                      <td>{item.created ?? "—"}</td>
                      <td>{item.paid ?? "—"}</td>
                      <td>{item.conversion_pct == null ? "—" : item.conversion_pct + "%"}</td>
                      <td>{item.avg_seconds_to_pay == null ? "—" : duration(item.avg_seconds_to_pay)}</td>
                      <td>{item.avg_check_usd == null ? "—" : money(item.avg_check_usd)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </TableCard>
          </div>
        ) : null}
      </>
    );
  }

  // ── Топ клиентов: два списка — по пополнениям и по расходу, доля от суммы окна ──
  let topBlock = availability.top === "error" ? (
    <WarnBanner title="Топ клиентов недоступен">/admin/finance/top-customers не отвечает</WarnBanner>
  ) : <LoadingGrid count={2} />;
  if (top) {
    const topTotals = top.totals ?? {};
    const topTable = (items: TopCustomer[] | undefined, moneyField: "total_usd" | "spent_usd", withCount: boolean) => (
      <TableCard>
        <table>
          <thead>
            <tr>
              <th>#</th>
              <th className="left">клиент</th>
              <th>{moneyField === "total_usd" ? "пополнено" : "списано"}</th>
              <th>доля окна</th>
            </tr>
          </thead>
          <tbody>
            {items?.length ? (
              items.map((item, index) => (
                <TopCustomerRow key={item.user_id ?? index} item={item} index={index} moneyField={moneyField} withCount={withCount} />
              ))
            ) : (
              <EmptyRow columns={4} />
            )}
          </tbody>
        </table>
      </TableCard>
    );
    topBlock = (
      <>
        <SectionHeader title="Топ по пополнениям" sub={`30 дней · окно ${money(topTotals.topups_usd)}`} />
        {topTable(top.topups, "total_usd", true)}
        <SectionHeader title="Топ по расходу" sub={`30 дней · окно ${money(topTotals.spend_usd)}`} />
        {topTable(top.spend, "spent_usd", false)}
      </>
    );
  }

  // ── Возвраты/диспуты: авторитет — payments.status ──
  let refundsBlock = availability.refunds === "error" ? (
    <WarnBanner title="Список возвратов недоступен">/admin/refunds не отвечает</WarnBanner>
  ) : <LoadingGrid count={2} />;
  if (refunds) {
    refundsBlock = (
      <>
        <SectionHeader
          title="Возвраты и диспуты"
          sub={`${refunds.total} · страница ${money(refunds.page_amount_usd)} · всего ${money(refunds.total_amount_usd)}`}
        />
        <TableCard>
          <table>
            <thead>
              <tr>
                <th className="left">клиент</th>
                <th>провайдер</th>
                <th>сумма</th>
                <th>статус</th>
                <th>компенсация</th>
                <th>оплачен</th>
                <th>возврат</th>
                <th className="left">provider id</th>
              </tr>
            </thead>
            <tbody>
              {refunds.rows?.length ? (
                refunds.rows.map((item, index) => (
                  <RefundRowView key={item.provider_payment_id ?? item.user_id ?? index} item={item} />
                ))
              ) : (
                <EmptyRow columns={8} />
              )}
            </tbody>
          </table>
        </TableCard>
        <Pager
          offset={refundOffset}
          limit={REFUND_PAGE_LIMIT}
          total={refunds.total ?? 0}
          onPage={(offset) => startTransition(() => setRefundOffset(offset))}
        />
      </>
    );
  }

  // ── Когорты регистраций и сигналы оттока (stretch-блоки; при недоступности — без баннера) ──
  const cohortRows = cohorts?.cohorts ?? [];
  const cohortsBlock = cohorts ? (
    <>
      <SectionHeader title="Когорты регистраций" sub="по неделям · 8 недель" />
      <TableCard>
        <table>
          <thead>
            <tr>
              <th>неделя</th>
              <th>регистраций</th>
              <th>оплатили</th>
              <th>медиана до оплаты</th>
              <th>выручка когорты</th>
            </tr>
          </thead>
          <tbody>
            {cohortRows.length ? (
              cohortRows.map((item) => (
                <tr key={item.week}>
                  <td>{formatDate(item.week)}</td>
                  <td>{item.registered ?? "—"}</td>
                  <td>
                    {item.paid_share_pct == null ? "—" : item.paid_share_pct + "%"}
                    <div className="sub">{item.paid_users} оплатили</div>
                  </td>
                  <td>{item.median_days_to_first_payment == null ? "—" : item.median_days_to_first_payment + " д"}</td>
                  <td>
                    <b>{money(item.revenue_usd)}</b>
                  </td>
                </tr>
              ))
            ) : (
              <EmptyRow columns={5} />
            )}
          </tbody>
        </table>
      </TableCard>
    </>
  ) : null;

  const churnRows = churn?.rows ?? [];
  const churnBlock = churn ? (
    <>
      <SectionHeader title="Сигналы оттока" sub="платившие клиенты без сессий и расхода 14 дней" />
      <TableCard>
        <table>
          <thead>
            <tr>
              <th className="left">клиент</th>
              <th>был(а)</th>
              <th>последняя оплата</th>
              <th>расход 30д</th>
            </tr>
          </thead>
          <tbody>
            {churnRows.length ? (
              churnRows.map((item) => (
                <tr key={item.user_id ?? item.email}>
                  <td className="left">
                    <b>{item.email}</b>
                    <div className="sub mono">{item.user_id}</div>
                  </td>
                  <td>{ago(item.last_seen_at)}</td>
                  <td>{formatDate(item.last_paid_at)}</td>
                  <td>{money(item.spent_30d_usd)}</td>
                </tr>
              ))
            ) : (
              <EmptyRow columns={4} />
            )}
          </tbody>
        </table>
      </TableCard>
    </>
  ) : null;

  // ── Здоровье денежных пайплайнов: вердикт баннером, карточки, таблицы последних сбоев ──
  let pipelineBlock = availability.pipes === "error" ? (
    <WarnBanner title="Здоровье пайплайнов недоступно">/admin/pipeline-health не отвечает</WarnBanner>
  ) : <LoadingGrid count={4} />;
  if (pipes) {
    const credits = pipes.engine_credits ?? {};
    const cc = credits.counts_by_status ?? {};
    const webhooks = pipes.webhook_events ?? {};
    const email = pipes.email_outbox ?? {};
    const jobs = pipes.engine_pricing_jobs ?? {};
    const jc = jobs.counts_by_status ?? {};
    const kind: Tone = pipes.verdict === "bad" ? "bad" : pipes.verdict === "warn" ? "warn" : "ok";
    const reasons = (pipes.verdict_reasons ?? []).join(" · ");
    const webhookFailures = webhooks.recent_failures ?? [];
    const emailFailures = email.recent_failures ?? [];
    const jobErrors = jobs.recent_errors ?? [];
    pipelineBlock = (
      <>
        <Banner
          kind={kind}
          dot={kind === "ok" ? "" : kind}
          title={
            kind === "ok"
              ? "Денежные пайплайны в порядке"
              : kind === "bad"
                ? "Денежные пайплайны: есть сбои"
                : "Денежные пайплайны: требуют внимания"
          }
        >
          {reasons || "dead-кредитов, свежих failed-вебхуков и retry-очередей нет"}
        </Banner>
        <CardGrid>
          <StatCard
            label="кредиты движка"
            value={credits.stuck_nano != null ? nanoMoney(credits.stuck_nano) : "—"}
            hint={`застряло в пути · pending ${cc.pending ?? 0} · retry ${cc.retry ?? 0} · dead ${credits.dead_count ?? 0} · старейший ${ageText(credits.oldest_unconfirmed_age_seconds)}`}
          />
          <StatCard
            label="вебхуки"
            value={webhooks.failed_24h ?? "—"}
            hint={`failed за 24ч · всего failed ${webhooks.failed_total ?? "—"}`}
          />
          <StatCard label="почта" value={email.failed_total ?? "—"} hint="терминально недоставленные письма" />
          <StatCard
            label="pricing-джобы"
            value={jobs.retry_count ?? "—"}
            hint={`retry · pending ${jc.pending ?? 0} · processing ${jc.processing ?? 0} · confirmed ${jc.confirmed ?? 0} · старейшая ${ageText(jobs.oldest_unconfirmed_age_seconds)}`}
          />
        </CardGrid>
        {webhookFailures.length ? (
          <>
            <SectionHeader title="Последние сбои вебхуков" sub={`failed всего ${webhooks.failed_total ?? "—"}`} />
            <TableCard>
              <table>
                <thead>
                  <tr>
                    <th className="left">провайдер</th>
                    <th>попытки</th>
                    <th>получен</th>
                    <th className="left">ошибка</th>
                  </tr>
                </thead>
                <tbody>
                  {webhookFailures.map((item, index) => (
                    <tr key={`${item.provider}-${item.event_type}-${index}`}>
                      <td className="left">
                        <b>{item.provider}</b>
                        <div className="sub mono">{item.event_type}</div>
                      </td>
                      <td>{item.attempts ?? "—"}</td>
                      <td>{formatDate(item.received_at, true)}</td>
                      <ErrCell value={item.last_error} />
                    </tr>
                  ))}
                </tbody>
              </table>
            </TableCard>
          </>
        ) : null}
        {emailFailures.length ? (
          <>
            <SectionHeader title="Недоставленная почта" sub={`последние ${emailFailures.length}`} />
            <TableCard>
              <table>
                <thead>
                  <tr>
                    <th className="left">шаблон</th>
                    <th>попытки</th>
                    <th className="left">ошибка</th>
                  </tr>
                </thead>
                <tbody>
                  {emailFailures.map((item, index) => (
                    <tr key={`${item.template}-${index}`}>
                      <td className="left">
                        <b>{item.template}</b>
                      </td>
                      <td>{item.attempts ?? "—"}</td>
                      <ErrCell value={item.last_error} />
                    </tr>
                  ))}
                </tbody>
              </table>
            </TableCard>
          </>
        ) : null}
        {jobErrors.length ? (
          <>
            <SectionHeader title="Ошибки pricing-джоб" sub={`последние ${jobErrors.length}`} />
            <TableCard>
              <table>
                <thead>
                  <tr>
                    <th className="left">причина</th>
                    <th>статус</th>
                    <th>попытки</th>
                    <th className="left">ошибка</th>
                  </tr>
                </thead>
                <tbody>
                  {jobErrors.map((item, index) => (
                    <tr key={`${item.user_id}-${item.engine_account_id}-${index}`}>
                      <td className="left">
                        <b>{item.reason}</b>
                        <div className="sub mono">
                          {item.user_id} · {item.engine_account_id}
                        </div>
                      </td>
                      <td>
                        <Pill kind={item.status === "retry" ? "warn" : "bad"}>{item.status || "—"}</Pill>
                      </td>
                      <td>{item.attempts ?? "—"}</td>
                      <ErrCell value={item.last_error} />
                    </tr>
                  ))}
                </tbody>
              </table>
            </TableCard>
          </>
        ) : null}
      </>
    );
  }

  // ── Settlement движка: outbox расчётов и лаг pricing-consumer ──
  let settlementBlock = availability.settle === "error" ? (
    <WarnBanner title="Settlement движка недоступен">/settlement-health не отвечает</WarnBanner>
  ) : <LoadingGrid count={4} />;
  if (settle) {
    const out = settle.outbox ?? {};
    const lag = settle.pricing_consumer ?? {};
    const failedRows = out.recent_failed ?? [];
    settlementBlock = (
      <>
        <CardGrid>
          <StatCard
            label="settlement outbox"
            value={out.pending ?? "—"}
            hint={`pending · backlog ${out.backlog ?? 0} · failed 24ч ${out.failed_24h ?? 0} · всего failed ${out.failed ?? 0} · с ошибкой ${out.pending_with_error ?? 0}`}
          />
          <StatCard
            label="backlog settlement"
            value={out.backlog ?? "—"}
            hint={`несеттленых старше ${duration(settle.backlog_threshold_secs)} · старейшая ждёт ${ageText(out.oldest_unsettled_age_secs)}`}
          />
          <StatCard
            label="лаг pricing-consumer"
            value={lag.unacked ?? "—"}
            hint={`отставание передачи расхода в коммерцию · старейшая ждёт ${ageText(lag.oldest_unacked_age_secs)}`}
          />
          <StatCard
            label="settlement failed"
            value={out.failed_24h ?? "—"}
            hint={`failed за 24ч · всего ${out.failed ?? 0} · done ${out.done ?? 0}`}
          />
        </CardGrid>
        {failedRows.length ? (
          <div style={{ marginTop: 12 }}>
            <TableCard>
              <table>
                <thead>
                  <tr>
                    <th className="left">request id</th>
                    <th>сумма</th>
                    <th>попытки</th>
                    <th>обновлено</th>
                    <th className="left">ошибка</th>
                  </tr>
                </thead>
                <tbody>
                  {failedRows.map((item) => (
                    <tr key={item.request_id}>
                      <td className="left mono">{item.request_id}</td>
                      <td>
                        <b>{money(item.actual_usd)}</b>
                      </td>
                      <td>{item.attempts ?? "—"}</td>
                      <td>{ago(item.updated_ts != null ? item.updated_ts * 1000 : undefined)}</td>
                      <td className="left">
                        <div className="json" title={item.last_error || ""}>
                          {item.last_error || "—"}
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </TableCard>
          </div>
        ) : null}
      </>
    );
  }

  return (
    <>
      <PageHead
        title="Финансы"
        sub="prepay-метрики: выручка, воронка, клиенты и возвраты"
        badge={<Pill kind={overview ? "ok" : "warn"}>{overview ? `${money(ov.revenue_30d_usd)} / 30д` : "degraded"}</Pill>}
      />

      {overviewBlock}

      <SectionHeader title="Выручка" sub="paid-платежи по дате оплаты" />
      <div className="spend-tabs">
        {FINANCE_WINDOWS.map(([days, label]) => (
          <button
            key={days}
            type="button"
            className={"btn" + (windowDays === days ? " on" : "")}
            onClick={() => startTransition(() => setWindowDays(days))}
          >
            {label}
          </button>
        ))}
      </div>
      {chartBlock}

      <SectionHeader title="Воронка чекаутов" sub="30 дней · от создания до оплаты" />
      {funnelBlock}

      {topBlock}
      {refundsBlock}
      {cohortsBlock}
      {churnBlock}

      <SectionHeader title="Здоровье пайплайнов" sub="commerce: кредиты движка · вебхуки · почта · pricing-джобы" />
      {pipelineBlock}

      <SectionHeader title="Settlement движка" sub="outbox расчётов и лаг передачи расхода в коммерцию" />
      {settlementBlock}

      <footer>
        Данные обновляются по событиям источников; кнопка ↻ остаётся для явной перепроверки. Выручка —
        только подтверждённые платежи (prepay, подписок-продуктов нет). Возвраты: авторитет статуса —
        payments; соседний статус показывает durable компенсацию баланса в engine_adjustments. ARPU —
        выручка на активного за 30д, ARPPU — на платящего.
      </footer>
    </>
  );
}
