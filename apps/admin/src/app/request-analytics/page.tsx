"use client";

import { startTransition, useMemo, useState, type ReactElement } from "react";
import { ago } from "@/lib/format";
import { useResource, useResources } from "@/lib/resources";
import { Banner, CardGrid, EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, StatCard, TableCard } from "@/components/ui";
import {
  displayValue,
  durationLabel,
  requestAnalyticsUrls,
  routeLabel,
  type LogicalResponse,
  type PageResponse,
  type RequestFactRow,
  type SummaryResponse,
  type WindowHours,
} from "./request-analytics-lib";

const WINDOWS: Array<{ hours: WindowHours; label: string }> = [
  { hours: 24, label: "24 часа" },
  { hours: 168, label: "7 дней" },
  { hours: 720, label: "30 дней" },
];

export default function RequestAnalyticsPage(): ReactElement {
  const [hours, setHours] = useState<WindowHours>(24);
  const [cursor, setCursor] = useState<string | undefined>();
  const [logicalId, setLogicalId] = useState<string | undefined>();
  const urls = useMemo(() => requestAnalyticsUrls(hours, cursor), [hours, cursor]);
  const { data } = useResources<{ summary: SummaryResponse; page: PageResponse }>({
    summary: urls.summary,
    page: urls.page,
  });
  const { data: logical } = useResource<LogicalResponse>(
    logicalId
      ? `/admin/request-analytics/logical/${encodeURIComponent(logicalId)}`
      : "/admin/request-analytics/logical/00000000-0000-4000-8000-000000000000",
  );
  const summary = data.summary;
  const page = data.page;

  if (!summary || !page) {
    return <><PageHead title="Request Analytics" sub="операторская аналитика запросов без содержимого и секретов" /><LoadingGrid count={4} /></>;
  }

  const totals = summary.summary?.totals;
  const runtime = summary.runtime;
  return (
    <div>
      <PageHead
        title="Request Analytics"
        sub="отдельно от расходов: маршруты, lifecycle и качество покрытия"
        badge={<Pill kind={runtime?.persistence_health === "failed" ? "bad" : "ok"}>persistence {displayValue(runtime?.persistence_health)}</Pill>}
      />
      <div className="paying-window-switch" role="group" aria-label="Окно аналитики">
        <span>Окно</span>
        {WINDOWS.map((window) => <button key={window.hours} type="button" className={hours === window.hours ? "on" : ""} aria-pressed={hours === window.hours} onClick={() => startTransition(() => { setHours(window.hours); setCursor(undefined); setLogicalId(undefined); })}>{window.label}</button>)}
      </div>

      <CardGrid>
        <StatCard label="persisted facts" value={totals?.persisted ?? 0} hint={`${totals?.terminal ?? 0} terminal`} />
        <StatCard label="nonterminal" value={totals?.nonterminal ?? 0} hint={`unknown evidence ${totals?.required_evidence_unknown ?? 0}`} />
        <StatCard label="inbox" value={runtime?.queue_depth ?? 0} hint={runtime?.continuity === "process_local" ? `процесс с ${ago((runtime.process_started_at ?? 0) * 1000)}` : "continuity неизвестна"} />
        <StatCard label="stuck > 1ч" value={runtime?.stuck_nonterminal_count ?? "—"} hint="долговременная authority-проверка" />
      </CardGrid>

      <Banner kind="warn" title="Coverage не подтверждён">
        Persisted facts не являются независимым знаменателем admitted requests. Потери inbox не имеют durable window attribution и не показываются как ноль.
      </Banner>

      <SectionHeader title="Последние запросы" sub="новые первыми · до 100 строк на страницу" />
      <TableCard>
        <table>
          <thead><tr><th className="left">маршрут</th><th className="left">модель</th><th>stream</th><th>status</th><th>delivery</th><th>first byte</th><th>terminal</th><th>время</th></tr></thead>
          <tbody>
            {(page.rows ?? []).length ? (page.rows ?? []).map((row) => (
              <tr key={row.fact_id} onClick={() => row.logical_request_id && setLogicalId(row.logical_request_id)} style={{ cursor: row.logical_request_id ? "pointer" : "default" }}>
                <td className="left"><b>{routeLabel(row)}</b><div className="sub">client {displayValue(row.client_kind)}</div></td>
                <td className="left"><span className="mono">{displayValue(row.requested_model)}</span><div className="sub mono">→ {displayValue(row.executable_model)}</div></td>
                <td>{row.stream ? "да" : "нет"}</td><td>{row.http_status_code ?? "—"}</td>
                <td>{displayValue(row.delivery_state)}</td><td>{durationLabel(row.admission_to_first_public_byte_seconds)}</td>
                <td>{durationLabel(row.admission_to_terminal_seconds)}</td><td>{ago((row.admitted_at ?? 0) * 1000)}</td>
              </tr>
            )) : <EmptyRow columns={8} text="в этом окне фактов нет" />}
          </tbody>
        </table>
      </TableCard>
      {page.next_cursor ? <div className="toolbar"><button type="button" className="btn ghost" onClick={() => setCursor(page.next_cursor ?? undefined)}>Следующая страница</button></div> : null}

      <SectionHeader title="Попытки логического запроса" sub={logicalId ? `operator-only ${logicalId}` : "выберите строку с logical ID"} />
      {logicalId && logical ? <AttemptsTable rows={logical.rows ?? []} /> : <Banner title="Выберите запрос">Logical ID не раскрывается в общей таблице producer-а; detail откроется только для строк, где он присутствует.</Banner>}
    </div>
  );
}

function AttemptsTable({ rows }: { rows: RequestFactRow[] }): ReactElement {
  return <TableCard><table><thead><tr><th>attempt</th><th className="left">route</th><th>provider terminal</th><th>billing</th><th>internal attempts</th><th>terminal</th></tr></thead><tbody>{rows.length ? rows.map((row) => <tr key={row.fact_id}><td>{row.attempt ?? "—"}</td><td className="left">{routeLabel(row)}</td><td>{displayValue(row.provider_terminal_class)}</td><td>{displayValue(row.billing_outcome)}</td><td>{row.internal_attempt_count ?? "—"}</td><td>{durationLabel(row.admission_to_terminal_seconds)}</td></tr>) : <EmptyRow columns={6} text="попытки не найдены" />}</tbody></table></TableCard>;
}
