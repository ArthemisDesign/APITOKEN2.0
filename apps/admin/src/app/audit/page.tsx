"use client";

// Аудит — порт 1:1 секции audit()/bindAuditPage() из crates/server/src/admin-panel.js
// (строки 1026-1058): фильтры action/actor_type/q, пагинация offset/limit 50,
// CSV-выгрузка текущей страницы. Автообновления у вкладки нет — только фокус/кнопка ↻.
import { memo, useCallback, useRef, useState, type FormEvent } from "react";
import { api } from "@/lib/api";
import { usePoll } from "@/lib/usePoll";
import { count, formatDate } from "@/lib/format";
import { csvDate, downloadCsv } from "@/lib/csv";
import { EmptyRow, LoadingGrid, PageHead, Pill, TableCard } from "@/components/ui";
import {
  AUDIT_ACTORS,
  AUDIT_CSV_HEADER,
  INITIAL_AUDIT_FILTERS,
  auditActionOptions,
  auditCsvRows,
  auditPageTotal,
  buildAuditQuery,
  clampAuditOffset,
  normalizeAuditActions,
  type AuditEntry,
  type AuditFilters,
  type AuditPagePayload,
} from "./lib";

interface AuditData {
  rows: AuditEntry[];
  total: number;
}

// Ключ — путь API без query (sourceName маппит его в «Аудит» для ErrorCenter);
// конкретные фильтры fetcher читает из ref, поэтому poller один и данные
// переживают смену фильтров/страницы (stale-while-revalidate, как shell легаси).
const POLL_KEY = "/admin/audit";

// Статичный футер страницы — дословно из легаси.
const FOOTER = (
  <footer>Поиск ищет подстроку в id цели и в metadata. Секреты и полные API-ключи не записываются.</footer>
);

const AuditRow = memo(function AuditRow({ item }: { item: AuditEntry }) {
  const action = item.action ?? "";
  const meta = JSON.stringify(item.metadata || {});
  return (
    <tr>
      <td>{formatDate(item.created_at, true)}</td>
      <td className="left">
        <Pill kind={action.startsWith("admin.") ? "warn" : ""}>{action}</Pill>
      </td>
      <td className="left">
        {item.actor_type}
        <div className="sub mono">{item.actor_id || "system"}</div>
      </td>
      <td className="left">
        {item.target_type} · {item.target_id}
      </td>
      <td className="left">
        <div className="json" title={meta}>
          {meta}
        </div>
      </td>
    </tr>
  );
});

export default function AuditPage() {
  // Применённые фильтры: state — для рендера, ref — для fetcher'а (читается
  // синхронно в момент запроса, до коммита React).
  const [filters, setFilters] = useState<AuditFilters>(INITIAL_AUDIT_FILTERS);
  const filtersRef = useRef(filters);
  // Distinct-список action для выпадайки тянем лениво один раз; ошибка/404
  // старого backend — просто нет опций, список событий работает независимо.
  const [actions, setActions] = useState<string[]>([]);
  const actionsRef = useRef<string[] | null>(null);

  const loadAudit = useCallback(async (): Promise<AuditData> => {
    for (;;) {
      const current = filtersRef.current;
      const [page, actionsPayload] = await Promise.all([
        api<AuditPagePayload>(`${POLL_KEY}?${buildAuditQuery(current)}`).catch(() => null),
        actionsRef.current === null ? api<unknown>("/admin/audit/actions").catch(() => null) : Promise.resolve(null),
      ]);
      if (actionsPayload !== null) {
        actionsRef.current = normalizeAuditActions(actionsPayload);
        setActions(actionsRef.current);
      }
      const rows = page?.rows ?? [];
      const total = auditPageTotal(page);
      // Offset уехал за хвост лога → прыжок на последнюю страницу и повторный запрос,
      // как `return audit()` в легаси.
      const clamped = clampAuditOffset(current.offset, current.limit, total);
      if (clamped === current.offset) return { rows, total };
      const next = { ...current, offset: clamped };
      filtersRef.current = next;
      setFilters(next);
    }
  }, []);

  const { data, refresh } = usePoll(POLL_KEY, loadAudit);

  // Применить фильтры/страницу: ref обновляется синхронно (fetcher увидит его
  // сразу при refresh), state — через обычный апдейт.
  const applyFilters = useCallback(
    (next: AuditFilters) => {
      filtersRef.current = next;
      setFilters(next);
      refresh();
    },
    [refresh],
  );

  const onSubmit = useCallback(
    (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const form = new FormData(event.currentTarget);
      applyFilters({
        ...filtersRef.current,
        offset: 0,
        action: String(form.get("action") ?? ""),
        actorType: String(form.get("actorType") ?? ""),
        q: String(form.get("q") ?? "").trim(),
      });
    },
    [applyFilters],
  );

  const exportCsv = useCallback(() => {
    if (!data) return;
    downloadCsv(`audit-${csvDate()}.csv`, AUDIT_CSV_HEADER, auditCsvRows(data.rows));
  }, [data]);

  if (!data) {
    return (
      <>
        <PageHead title="Аудит" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const { rows, total } = data;
  const { offset, limit } = filters;
  const actionOptions = auditActionOptions(actions, filters.action);

  return (
    <>
      <PageHead
        title="Аудит"
        sub="operator/user/provider события и причины действий"
        badge={<Pill kind="ok">{count(total, "событие", "события", "событий")}</Pill>}
      />

      {/* Форма — неконтролируемая и перемонтируется по применённым фильтрам
          (key): как в легаси, незасабмиченный ввод сбрасывается при пагинации. */}
      <form
        key={`${filters.action}|${filters.actorType}|${filters.q}`}
        className="toolbar"
        onSubmit={onSubmit}
      >
        <label className="sr-only" htmlFor="audit-action">
          Действие
        </label>
        <select id="audit-action" name="action" defaultValue={filters.action}>
          <option value="">все действия</option>
          {actionOptions.map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
        <label className="sr-only" htmlFor="audit-actor">
          Тип актора
        </label>
        <select id="audit-actor" name="actorType" defaultValue={filters.actorType}>
          <option value="">все акторы</option>
          {AUDIT_ACTORS.map((actor) => (
            <option key={actor} value={actor}>
              {actor}
            </option>
          ))}
        </select>
        <label className="sr-only" htmlFor="audit-q">
          Поиск по аудиту
        </label>
        <input id="audit-q" name="q" type="search" defaultValue={filters.q} placeholder="id цели или текст в metadata…" />
        <button className="btn" type="submit">
          Найти
        </button>
        <button className="btn ghost" type="button" title="Выгрузить текущую страницу в CSV" onClick={exportCsv}>
          CSV
        </button>
      </form>

      <TableCard>
        <table>
          <thead>
            <tr>
              <th>время</th>
              <th className="left">действие</th>
              <th className="left">актор</th>
              <th className="left">цель</th>
              <th className="left">метаданные</th>
            </tr>
          </thead>
          <tbody>
            {rows.length ? rows.map((item, index) => <AuditRow key={index} item={item} />) : <EmptyRow columns={5} />}
          </tbody>
        </table>
      </TableCard>

      <div className="pager">
        <span>
          {total ? offset + 1 : 0}–{Math.min(offset + limit, total)} из {total}
        </span>
        <button
          type="button"
          className="btn ghost"
          disabled={offset <= 0}
          onClick={() => applyFilters({ ...filtersRef.current, offset: Math.max(0, offset - limit) })}
        >
          Назад
        </button>
        <button
          type="button"
          className="btn ghost"
          disabled={offset + limit >= total}
          onClick={() => applyFilters({ ...filtersRef.current, offset: offset + limit })}
        >
          Дальше
        </button>
      </div>

      {FOOTER}
    </>
  );
}
