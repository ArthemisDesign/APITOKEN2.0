"use client";

// «Расход движка» — операторский ответ на два вопроса, которых нет в «Платящих»:
// сколько ушло по каждой МОДЕЛИ/провайдеру за 24ч/7д/30д и сколько тратят аккаунты
// БЕЗ commerce-юзера (OpenKeys, внутренние) — их расхода нет ни в одной таблице
// коммерции. Источник — /admin/finance/engine-spend (движковый /spend-stats,
// склеенный со справочником владельцев engine-аккаунтов).

import { startTransition, useEffect, useState, type ReactElement } from "react";
import { api } from "@/lib/api";
import { csvDate, downloadCsv } from "@/lib/csv";
import { ago, count, money } from "@/lib/format";
import { usePoll } from "@/lib/usePoll";
import { CardGrid, EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, StatCard, TableCard } from "@/components/ui";
import { OkInfo, okDirectory, type OkDirectoryRow } from "@/components/spend-stats-modal";
import {
  accountClassLabel,
  accountTitle,
  buildEngineSpendAccountsCsvRows,
  buildEngineSpendCsvRows,
  discountLabel,
  ENGINE_SPEND_ACCOUNTS_CSV_HEADER,
  ENGINE_SPEND_CSV_HEADER,
  ENGINE_SPEND_FILTERS,
  ENGINE_SPEND_WINDOWS,
  filterEngineSpendAccounts,
  providerLabel,
  type EngineSpendAccountRow,
  type EngineSpendDays,
  type EngineSpendFilter,
  type EngineSpendResponse,
} from "./engine-spend-lib";

function AccountsTable({
  rows,
  empty,
  okDir,
}: {
  rows: EngineSpendAccountRow[];
  empty: string;
  okDir: Map<string, OkDirectoryRow> | null;
}): ReactElement {
  return (
    <TableCard>
      <table>
        <thead>
          <tr>
            <th className="left">аккаунт</th>
            <th>запросы</th>
            <th>списано</th>
            <th>real-API</th>
            <th>скидка</th>
            <th>активность</th>
          </tr>
        </thead>
        <tbody>
          {rows.length ? (
            rows.map((row, index) => (
              <tr key={row.account ?? index}>
                <td className="left">
                  <b>{accountTitle(row)}</b>
                  {row.account_class === "openkeys" ? (
                    <span className="okb" title="Выпущен через OpenKeys">OpenKeys</span>
                  ) : null}
                  <div className="sub mono">
                    {accountClassLabel(row.account_class)}
                    {row.owner?.customer_type ? ` · ${row.owner.customer_type.toUpperCase()}` : ""}
                    {" · "}
                    {row.handle || row.account || "—"}
                  </div>
                  {/* Метка партии, номинал, продавец и профиль ключа — иначе openkeys-аккаунт
                      виден только как безымянный handle. */}
                  <OkInfo meta={okDir?.get(String(row.account ?? ""))} />
                </td>
                <td>{row.requests ?? 0}</td>
                <td><b>{money(row.charge_usd)}</b></td>
                <td>{money(row.real_usd)}</td>
                <td>{discountLabel(row.charge_usd, row.real_usd)}</td>
                <td>{ago((row.last_ts ?? 0) * 1000)}</td>
              </tr>
            ))
          ) : (
            <EmptyRow columns={6} text={empty} />
          )}
        </tbody>
      </table>
    </TableCard>
  );
}

export default function EngineSpendPage(): ReactElement {
  const [days, setDays] = useState<EngineSpendDays>(1);
  const [filter, setFilter] = useState<EngineSpendFilter>("");
  const [okDir, setOkDir] = useState<Map<string, OkDirectoryRow> | null>(null);
  const path = `/admin/finance/engine-spend?days=${days}`;
  const { data } = usePoll(path, () => api<EngineSpendResponse>(path), { interval: 60_000 });

  // Справочник ключей OpenKeys грузится один раз за вкладку; если портал недоступен —
  // строки просто остаются без метки партии, страница продолжает работать.
  useEffect(() => {
    let cancelled = false;
    okDirectory().then((directory) => {
      if (!cancelled) setOkDir(directory);
    }).catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  if (!data) {
    return (
      <>
        <PageHead title="Расход движка" sub="весь флот: модели, провайдеры и аккаунты" />
        <LoadingGrid count={4} />
      </>
    );
  }

  const models = data.models ?? [];
  const providers = data.providers ?? [];
  const accounts = filterEngineSpendAccounts(data.accounts ?? [], filter);
  const clients = accounts.filter((row) => row.account_class === "client");
  const others = accounts.filter((row) => row.account_class !== "client");
  const byClass = data.by_class;
  const windowLabel = ENGINE_SPEND_WINDOWS.find((item) => item.days === days)?.label ?? "";

  return (
    <div>
      <PageHead
        title="Расход движка"
        sub="весь флот, включая аккаунты без клиента коммерции · «списано» — по множителю аккаунта, «real-API» — прайс провайдера"
        badge={<Pill kind="info">обновлено {ago(data.generated_at)}</Pill>}
      />

      <div className="paying-window-switch" role="group" aria-label="Окно расхода">
        <span>Окно расхода</span>
        {ENGINE_SPEND_WINDOWS.map((window) => (
          <button
            type="button"
            key={window.days}
            className={days === window.days ? "on" : ""}
            aria-pressed={days === window.days}
            onClick={() => startTransition(() => setDays(window.days))}
          >
            {window.label}
          </button>
        ))}
      </div>

      <CardGrid>
        <StatCard label={`списано · ${windowLabel}`} value={money(data.charge_usd)} hint={`${data.requests ?? 0} запросов`} />
        <StatCard
          label="real-API эквивалент"
          value={money(data.real_usd)}
          hint={`средняя скидка ${discountLabel(data.charge_usd, data.real_usd)}`}
        />
        <StatCard
          label="клиенты коммерции"
          value={money(byClass?.client?.charge_usd)}
          hint={count(byClass?.client?.accounts ?? 0, "аккаунт", "аккаунта", "аккаунтов")}
        />
        <StatCard
          label="прочие аккаунты движка"
          value={money((byClass?.openkeys?.charge_usd ?? 0) + (byClass?.internal?.charge_usd ?? 0))}
          hint={`OpenKeys ${money(byClass?.openkeys?.charge_usd)} · внутренние ${money(byClass?.internal?.charge_usd)}`}
        />
      </CardGrid>

      <SectionHeader title="По моделям" sub={`top-20 по списанию за ${windowLabel.toLowerCase()}`} />
      <div className="toolbar" style={{ margin: "0 0 12px" }}>
        <button
          className="btn ghost"
          type="button"
          title="Выгрузить разбивку по моделям в CSV"
          onClick={() => downloadCsv(`engine-models-${days}d-${csvDate()}.csv`, ENGINE_SPEND_CSV_HEADER, buildEngineSpendCsvRows(models))}
        >
          CSV
        </button>
      </div>
      <TableCard>
        <table>
          <thead>
            <tr>
              <th className="left">модель</th>
              <th className="left">провайдер</th>
              <th>запросы</th>
              <th>списано</th>
              <th>real-API</th>
              <th>скидка</th>
            </tr>
          </thead>
          <tbody>
            {models.length ? (
              models.map((row, index) => (
                <tr key={`${row.provider ?? ""}/${row.model ?? index}`}>
                  <td className="left"><b>{row.model ?? "—"}</b></td>
                  <td className="left">{providerLabel(row.provider)}</td>
                  <td>{row.requests ?? 0}</td>
                  <td><b>{money(row.charge_usd)}</b></td>
                  <td>{money(row.real_usd)}</td>
                  <td>{discountLabel(row.charge_usd, row.real_usd)}</td>
                </tr>
              ))
            ) : (
              <EmptyRow columns={6} text="за это окно расхода не было" />
            )}
          </tbody>
        </table>
      </TableCard>

      <SectionHeader title="По провайдерам" />
      <TableCard>
        <table>
          <thead>
            <tr>
              <th className="left">провайдер</th>
              <th>запросы</th>
              <th>списано</th>
              <th>real-API</th>
              <th>скидка</th>
            </tr>
          </thead>
          <tbody>
            {providers.length ? (
              providers.map((row, index) => (
                <tr key={row.provider ?? index}>
                  <td className="left"><b>{providerLabel(row.provider)}</b></td>
                  <td>{row.requests ?? 0}</td>
                  <td><b>{money(row.charge_usd)}</b></td>
                  <td>{money(row.real_usd)}</td>
                  <td>{discountLabel(row.charge_usd, row.real_usd)}</td>
                </tr>
              ))
            ) : (
              <EmptyRow columns={5} text="за это окно расхода не было" />
            )}
          </tbody>
        </table>
      </TableCard>

      <SectionHeader title="Аккаунты" sub="фильтр применяется к обеим таблицам ниже" />
      <div className="toolbar" style={{ margin: "0 0 12px" }}>
        <label className="sr-only" htmlFor="engine-spend-filter">Класс аккаунта</label>
        <select
          id="engine-spend-filter"
          value={filter}
          title="Показать только ключи OpenKeys — или, наоборот, убрать их из выборки"
          onChange={(event) => startTransition(() => setFilter(event.target.value as EngineSpendFilter))}
        >
          {ENGINE_SPEND_FILTERS.map(([value, label]) => (
            <option key={value || "any"} value={value}>{label}</option>
          ))}
        </select>
        <button
          className="btn ghost"
          type="button"
          title="Выгрузить аккаунты текущего фильтра в CSV"
          onClick={() => downloadCsv(
            `engine-accounts-${days}d-${csvDate()}.csv`,
            ENGINE_SPEND_ACCOUNTS_CSV_HEADER,
            buildEngineSpendAccountsCsvRows(accounts),
          )}
        >
          CSV
        </button>
      </div>

      <SectionHeader title="Клиенты коммерции" sub="engine-аккаунты, у которых есть пользователь сайта" />
      <AccountsTable rows={clients} okDir={okDir} empty="клиентских списаний за это окно нет" />

      <SectionHeader
        title="Прочие аккаунты движка"
        sub="OpenKeys (с меткой партии, номиналом и продавцом) и внутренние — их расхода нет в коммерческих отчётах"
      />
      <AccountsTable rows={others} okDir={okDir} empty="прочих аккаунтов за это окно нет" />

      <footer>
        Источник — движковый /spend-stats (top-50 аккаунтов и top-20 моделей за окно).
        «Платящие» показывают только клиентов коммерции, здесь — весь флот.
      </footer>
    </div>
  );
}
