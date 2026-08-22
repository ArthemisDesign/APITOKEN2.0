"use client";

import { startTransition, useMemo, useState, type FormEvent, type ReactElement } from "react";
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
  const [logicalDraft, setLogicalDraft] = useState("");
  const [logicalId, setLogicalId] = useState<string | undefined>();
  const urls = useMemo(() => requestAnalyticsUrls(hours, cursor), [hours, cursor]);
  const { data } = useResources<{ summary: SummaryResponse; page: PageResponse }>({
    summary: urls.summary,
    page: urls.page,
  });
  const summary = data.summary;
  const page = data.page;
  const totals = summary?.summary?.totals;
  const runtime = summary?.runtime;
  return (
    <div>
      <PageHead
        title="Request Analytics"
        sub="отдельно от расходов: маршруты, lifecycle и качество покрытия"
        badge={summary ? <Pill kind={runtime?.persistence_health === "failed" ? "bad" : "ok"}>persistence {displayValue(runtime?.persistence_health)}</Pill> : undefined}
      />
      <div className="paying-window-switch" role="group" aria-label="Окно аналитики">
        <span>Окно</span>
        {WINDOWS.map((window) => <button key={window.hours} type="button" className={hours === window.hours ? "on" : ""} aria-pressed={hours === window.hours} onClick={() => startTransition(() => { setHours(window.hours); setCursor(undefined); setLogicalId(undefined); })}>{window.label}</button>)}
      </div>

      {summary ? (
        <CardGrid>
          <StatCard label="persisted facts" value={totals?.persisted ?? 0} hint={`${totals?.terminal ?? 0} terminal`} />
          <StatCard label="nonterminal" value={totals?.nonterminal ?? 0} hint={`unknown evidence ${totals?.required_evidence_unknown ?? 0}`} />
          <StatCard label="inbox" value={runtime?.queue_depth ?? 0} hint={runtime?.continuity === "process_local" ? `процесс с ${ago((runtime.process_started_at ?? 0) * 1000)}` : "continuity неизвестна"} />
          <StatCard label="stuck > 1ч" value={runtime?.stuck_nonterminal_count ?? "—"} hint="долговременная authority-проверка" />
        </CardGrid>
      ) : <LoadingGrid count={4} />}

      <Banner kind="warn" title="Coverage не подтверждён">
        Persisted facts не являются независимым знаменателем admitted requests. Потери inbox не имеют durable window attribution и не показываются как ноль.
      </Banner>

      <SectionHeader title="Последние запросы" sub="новые первыми · до 100 строк на страницу" />
      {page ? (
        <>
          <TableCard>
            <table>
              <thead><tr><th className="left">маршрут</th><th className="left">модель</th><th>stream</th><th>status</th><th>delivery</th><th>first byte</th><th>terminal</th><th>время</th></tr></thead>
              <tbody>
                {(page.rows ?? []).length ? (page.rows ?? []).map((row) => (
                  <tr key={row.fact_id}>
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
        </>
      ) : <LoadingGrid count={2} />}

      <SectionHeader title="Попытки логического запроса" sub={logicalId ? `operator-only ${logicalId}` : "точный UUID из operator evidence"} />
      <form
        className="toolbar"
        style={{ margin: "0 0 12px" }}
        onSubmit={(event: FormEvent) => {
          event.preventDefault();
          const value = logicalDraft.trim();
          if (/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(value)) {
            setLogicalId(value);
          }
        }}
      >
        <label className="sr-only" htmlFor="logical-request-id">Logical request ID</label>
        <input id="logical-request-id" className="mono" value={logicalDraft} onChange={(event) => setLogicalDraft(event.target.value)} placeholder="xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx" maxLength={36} />
        <button type="submit" className="btn ghost" disabled={!/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(logicalDraft.trim())}>Найти</button>
      </form>
      {logicalId ? <LogicalAttempts logicalId={logicalId} /> : <Banner title="Введите operator-only ID">Общий drilldown намеренно не раскрывает logical ID. Вставьте точный UUID из incident evidence или журнала.</Banner>}
    </div>
  );
}

function LogicalAttempts({ logicalId }: { logicalId: string }): ReactElement {
  const { data } = useResource<LogicalResponse>(`/admin/request-analytics/logical/${encodeURIComponent(logicalId)}`);
  return data ? <AttemptsTable rows={data.rows ?? []} /> : <LoadingGrid count={1} />;
}

function AttemptsTable({ rows }: { rows: RequestFactRow[] }): ReactElement {
  return <TableCard><table><thead><tr><th>attempt</th><th className="left">route</th><th>provider terminal</th><th>billing</th><th>internal attempts</th><th>terminal</th></tr></thead><tbody>{rows.length ? rows.map((row) => <tr key={row.fact_id}><td>{row.attempt ?? "—"}</td><td className="left">{routeLabel(row)}</td><td>{displayValue(row.provider_terminal_class)}</td><td>{displayValue(row.billing_outcome)}</td><td>{row.internal_attempt_count ?? "—"}</td><td>{durationLabel(row.admission_to_terminal_seconds)}</td></tr>) : <EmptyRow columns={6} text="попытки не найдены" />}</tbody></table></TableCard>;
}
