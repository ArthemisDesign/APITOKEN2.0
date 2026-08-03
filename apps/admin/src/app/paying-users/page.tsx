"use client";

import {
  memo,
  startTransition,
  useCallback,
  useEffect,
  useState,
  type FormEvent,
  type ReactElement,
} from "react";
import { api } from "@/lib/api";
import { csvDate, downloadCsv } from "@/lib/csv";
import { ago, count, nanoMoney } from "@/lib/format";
import { usePoll } from "@/lib/usePoll";
import { Dot, EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, TableCard } from "@/components/ui";
import {
  buildPayingUsersCsvRows,
  INITIAL_PAYING_USERS_PAGE,
  isPositiveNano,
  PAYING_USER_SORTS,
  PAYING_USERS_CSV_HEADER,
  payingTierLabel,
  payingUsersQuery,
  providerNano,
  providerShareBp,
  spendWindowLabel,
  type PayingUserDays,
  type PayingUserProvider,
  type PayingUserRow,
  type PayingUsersPageState,
  type PayingUsersResponse,
} from "./paying-users-lib";

const PROVIDERS = [
  { id: "anthropic", label: "Claude", className: "claude" },
  { id: "openai", label: "GPT", className: "gpt" },
  { id: "google", label: "Gemini", className: "gemini" },
] as const;

const WINDOWS: Array<{ days: PayingUserDays; label: string }> = [
  { days: 1, label: "24 часа" },
  { days: 7, label: "7 дней" },
  { days: 30, label: "30 дней" },
];

const shareText = (basisPoints: number): string =>
  basisPoints <= 0 ? "0%" : `${(basisPoints / 100).toLocaleString("ru-RU", { maximumFractionDigits: 1 })}%`;

function ProviderCell({ row, provider }: { row: PayingUserRow; provider: (typeof PROVIDERS)[number] }): ReactElement {
  const amount = providerNano(row.provider_spend, provider.id);
  const share = providerShareBp(amount, row.spent_nano);
  return (
    <td className={`paying-provider-cell ${provider.className}`}>
      {isPositiveNano(amount) ? (
        <>
          <b>{nanoMoney(amount)}</b>
          <span className="paying-mini-meter" aria-label={`${shareText(share)} расхода клиента`}>
            <i style={{ width: `${share / 100}%` }} />
          </span>
        </>
      ) : (
        <span className="paying-zero">—</span>
      )}
    </td>
  );
}

const PayingRow = memo(function PayingRow({ row, rank, days }: { row: PayingUserRow; rank: number; days: PayingUserDays }) {
  const other = providerNano(row.provider_spend, "other");
  const discount = row.multiplier_bp == null ? null : 100 - row.multiplier_bp / 100;
  return (
    <tr>
      <td className="left paying-customer-cell">
        <span className="paying-rank" aria-label={`Место ${rank}`}>
          {String(rank).padStart(2, "0")}
        </span>
        <span className="paying-customer-copy">
          <b><Dot kind={row.status === "disabled" ? "bad" : "ok"} /> {row.email ?? "—"}</b>
          <span className="sub">
            {row.display_name || "Без имени"} · {payingTierLabel(row)}
            {discount == null ? "" : ` · скидка ${discount}%`}
          </span>
        </span>
      </td>
      <td className="paying-money-cell">
        <b>{nanoMoney(row.paid_nano)}</b>
        <span className="sub">
          {row.payments_count ?? 0} платежей · {ago(row.last_paid_at)}
        </span>
      </td>
      <td className="paying-money-cell paying-window-total">
        <b>{nanoMoney(row.spent_nano)}</b>
        <span className="sub">за {spendWindowLabel(days).toLowerCase()}</span>
        {isPositiveNano(other) ? <span className="sub">другое {nanoMoney(other)}</span> : null}
      </td>
      {PROVIDERS.map((provider) => <ProviderCell key={provider.id} row={row} provider={provider} />)}
      <td className="paying-activity-cell">
        <b>{ago(row.last_seen_at)}</b>
        <span className="sub">ключей активно: {row.active_api_keys ?? 0}</span>
      </td>
    </tr>
  );
});

function PayingLedger({
  data,
  activeProvider,
  onProviderSelect,
}: {
  data: PayingUsersResponse;
  activeProvider: "" | PayingUserProvider;
  onProviderSelect: (provider: "" | PayingUserProvider) => void;
}): ReactElement {
  const summary = data.summary ?? {};
  const spend = summary.provider_spend;
  const spentTotal = summary.spent_nano ?? "0";
  const other = providerNano(spend, "other");
  return (
    <section className="paying-ledger" aria-label="Сводка платящих клиентов">
      <div className="paying-ledger-lead">
        <span>Оплачено клиентами</span>
        <strong>{nanoMoney(summary.paid_nano)}</strong>
        <small>за всё время · {count(summary.paying_users ?? 0, "клиент", "клиента", "клиентов")}</small>
      </div>
      <div className="paying-ledger-window">
        <span>Расход · {spendWindowLabel(data.days ?? 30)}</span>
        <strong>{nanoMoney(spentTotal)}</strong>
        <small>{summary.active_spenders ?? 0} клиентов тратили · обновлено {ago(data.generated_at)}</small>
      </div>
      <div className="paying-ledger-provider-area">
        <div className="paying-ledger-rail" aria-label="Распределение расхода по провайдерам">
          {PROVIDERS.map((provider) => {
            const share = providerShareBp(providerNano(spend, provider.id), spentTotal);
            return <i key={provider.id} className={provider.className} style={{ width: `${share / 100}%` }} />;
          })}
          {isPositiveNano(other) ? (
            <i className="other" style={{ width: `${providerShareBp(other, spentTotal) / 100}%` }} />
          ) : null}
        </div>
        <div className="paying-provider-summaries">
          {PROVIDERS.map((provider) => {
            const amount = providerNano(spend, provider.id);
            const share = providerShareBp(amount, spentTotal);
            const selected = activeProvider === provider.id;
            return (
              <button
                type="button"
                key={provider.id}
                className={`paying-provider-summary ${provider.className}${selected ? " selected" : ""}`}
                aria-pressed={selected}
                title={selected ? "Снять фильтр" : `Показать клиентов, использовавших ${provider.label}`}
                onClick={() => onProviderSelect(selected ? "" : provider.id)}
              >
                <span><i /> {provider.label}<em>{summary.provider_users?.[provider.id] ?? 0}</em></span>
                <strong>{nanoMoney(amount)}</strong>
                <small>{shareText(share)} расхода</small>
              </button>
            );
          })}
        </div>
        {isPositiveNano(other) ? (
          <button type="button" className="paying-other" onClick={() => onProviderSelect(activeProvider === "other" ? "" : "other")}>
            Другое / legacy: {nanoMoney(other)} · {summary.provider_users?.other ?? 0} клиентов
          </button>
        ) : null}
      </div>
    </section>
  );
}

export default function PayingUsersPage() {
  const [page, setPage] = useState<PayingUsersPageState>(INITIAL_PAYING_USERS_PAGE);
  const [search, setSearch] = useState("");
  const query = payingUsersQuery(page);
  const { data } = usePoll(
    `/admin/finance/paying-users?${query}`,
    () => api<PayingUsersResponse>(`/admin/finance/paying-users?${query}`),
    { interval: 30_000 },
  );

  const patchPage = useCallback((patch: Partial<PayingUsersPageState>, resetOffset = true) => {
    startTransition(() => setPage((current) => ({ ...current, ...patch, ...(resetOffset ? { offset: 0 } : {}) })));
  }, []);

  useEffect(() => {
    const total = data?.total ?? 0;
    if (total > 0 && page.offset >= total) {
      const offset = Math.max(0, Math.floor((total - 1) / page.limit) * page.limit);
      startTransition(() => setPage((current) => current.offset === offset ? current : { ...current, offset }));
    }
  }, [data, page.limit, page.offset]);

  const submitSearch = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    patchPage({ q: search.trim() });
  };

  if (!data) {
    return (
      <>
        <PageHead title="Платящие" sub="денежный радар по клиентам и провайдерам" />
        <LoadingGrid count={6} />
      </>
    );
  }

  const rows = data.rows ?? [];
  const total = data.total ?? 0;
  const effectiveOffset = data.offset ?? page.offset;
  const effectiveLimit = data.limit ?? page.limit;
  const payingTotal = data.summary?.paying_users ?? 0;

  return (
    <div className="paying-page">
      <PageHead
        title="Платящие"
        sub="только клиенты с подтверждённой оплатой · точный расход Claude, GPT и Gemini"
        badge={<Pill kind="ok">{count(payingTotal, "клиент", "клиента", "клиентов")}</Pill>}
      />

      <div className="paying-window-switch" role="group" aria-label="Окно расхода">
        <span>Окно расхода</span>
        {WINDOWS.map((window) => (
          <button
            type="button"
            key={window.days}
            className={page.days === window.days ? "on" : ""}
            aria-pressed={page.days === window.days}
            onClick={() => patchPage({ days: window.days })}
          >
            {window.label}
          </button>
        ))}
      </div>

      <PayingLedger
        data={data}
        activeProvider={page.provider}
        onProviderSelect={(provider) => patchPage({ provider })}
      />

      <SectionHeader title="Клиенты" sub={`${total} по текущему фильтру · суммы в выбранном окне`} />
      <form className="paying-toolbar" onSubmit={submitSearch}>
        <label className="sr-only" htmlFor="paying-search">Поиск платящих клиентов</label>
        <input
          id="paying-search"
          type="search"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          placeholder="email, имя или UUID…"
        />
        <label className="sr-only" htmlFor="paying-status">Статус</label>
        <select id="paying-status" value={page.status} onChange={(event) => patchPage({ status: event.target.value as PayingUsersPageState["status"] })}>
          <option value="">все статусы</option>
          <option value="active">активные</option>
          <option value="disabled">отключённые</option>
        </select>
        <label className="sr-only" htmlFor="paying-provider">Провайдер</label>
        <select id="paying-provider" value={page.provider} onChange={(event) => patchPage({ provider: event.target.value as PayingUsersPageState["provider"] })}>
          <option value="">все провайдеры</option>
          <option value="anthropic">Claude</option>
          <option value="openai">GPT</option>
          <option value="google">Gemini</option>
          <option value="other">другое / legacy</option>
        </select>
        <label className="sr-only" htmlFor="paying-sort">Сортировка</label>
        <select id="paying-sort" value={page.sort} onChange={(event) => patchPage({ sort: event.target.value as PayingUsersPageState["sort"] })}>
          {PAYING_USER_SORTS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
        </select>
        <button
          type="button"
          className="paying-dir"
          aria-label={page.dir === "desc" ? "Сейчас по убыванию; переключить на возрастание" : "Сейчас по возрастанию; переключить на убывание"}
          title={page.dir === "desc" ? "По убыванию" : "По возрастанию"}
          onClick={() => patchPage({ dir: page.dir === "desc" ? "asc" : "desc" })}
        >
          {page.dir === "desc" ? "↓" : "↑"}
        </button>
        <button className="btn" type="submit">Найти</button>
        <button
          className="btn ghost"
          type="button"
          title="Выгрузить текущую страницу в CSV"
          onClick={() => downloadCsv(`paying-users-${page.days}d-${csvDate()}.csv`, PAYING_USERS_CSV_HEADER, buildPayingUsersCsvRows(rows))}
        >
          CSV
        </button>
      </form>

      <TableCard>
        <table className="paying-table">
          <thead>
            <tr>
              <th className="left">клиент</th>
              <th>оплачено</th>
              <th>расход · {page.days === 1 ? "24ч" : `${page.days}д`}</th>
              {PROVIDERS.map((provider) => <th key={provider.id} className={`paying-provider-head ${provider.className}`}><i />{provider.label}</th>)}
              <th>активность</th>
            </tr>
          </thead>
          <tbody>
            {rows.length ? rows.map((row, index) => (
              <PayingRow key={row.user_id ?? row.email ?? index} row={row} rank={effectiveOffset + index + 1} days={page.days} />
            )) : <EmptyRow columns={7} text="платящих клиентов по этому фильтру нет" />}
          </tbody>
        </table>
      </TableCard>

      <div className="pager">
        <span>{total ? effectiveOffset + 1 : 0}–{Math.min(effectiveOffset + effectiveLimit, total)} из {total}</span>
        <button type="button" className="btn ghost" disabled={effectiveOffset <= 0} onClick={() => patchPage({ offset: Math.max(0, effectiveOffset - effectiveLimit) }, false)}>
          Назад
        </button>
        <button type="button" className="btn ghost" disabled={effectiveOffset + effectiveLimit >= total} onClick={() => patchPage({ offset: effectiveOffset + effectiveLimit }, false)}>
          Дальше
        </button>
      </div>
      <footer>Платящий клиент — пользователь с хотя бы одним подтверждённым платежом. Расход взят из immutable usage events коммерции.</footer>
    </div>
  );
}
