"use client";

import {
  Fragment,
  memo,
  startTransition,
  useCallback,
  useEffect,
  useState,
  type Dispatch,
  type FormEvent,
  type KeyboardEvent,
  type ReactElement,
  type SetStateAction,
} from "react";
import { api } from "@/lib/api";
import { csvDate, downloadCsv } from "@/lib/csv";
import { ago, count, formatDate, nanoMoney } from "@/lib/format";
import { usePoll } from "@/lib/usePoll";
import { Dot, EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, TableCard } from "@/components/ui";
import {
  buildOpenkeysPayingCsvRows,
  clampOpenkeysPayingOffset,
  INITIAL_OPENKEYS_PAYING_PAGE,
  OPENKEYS_PAYING_CSV_HEADER,
  OPENKEYS_PAYING_MAX_OFFSET,
  OPENKEYS_PAYING_SORTS,
  openkeysChargedNano,
  openkeysPayingQuery,
  providerLabel,
  type OpenkeysPayingDays,
  type OpenkeysPayingPageState,
  type OpenkeysPayingResponse,
  type OpenkeysPayingRow,
  type OpenkeysUsageModel,
} from "./openkeys-paying-lib";
import {
  buildPayingUsersCsvRows,
  INITIAL_PAYING_USERS_PAGE,
  isPositiveNano,
  PAYING_USER_FUNDINGS,
  PAYING_USER_SORTS,
  PAYING_USERS_CSV_HEADER,
  normalizePayingUsersSearch,
  payingCohortUsers,
  payingTierLabel,
  payingUsersQuery,
  providerNano,
  providerShareBp,
  spendWindowLabel,
  usageNanoMoney,
  type PayingUserDays,
  type PayingUserFunding,
  type PayingUserProvider,
  type PayingUserRow,
  type PayingUserUsageModel,
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

type PayingCohort = "customers" | "openkeys";

type CohortControlsProps = {
  cohort: PayingCohort;
  days: PayingUserDays;
  customerFunding: PayingUserFunding;
  customerTotal?: number;
  openkeysTotal?: number;
  onCohortChange: (cohort: PayingCohort) => void;
  onDaysChange: (days: PayingUserDays) => void;
};

function CohortControls({ cohort, days, customerFunding, customerTotal, openkeysTotal, onCohortChange, onDaysChange }: CohortControlsProps): ReactElement {
  const onTabsKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const next = cohort === "customers" ? "openkeys" : "customers";
    onCohortChange(next);
    requestAnimationFrame(() => document.getElementById(`paying-tab-${next}`)?.focus());
  };
  return (
    <>
      <PageHead
        title="Расход клиентов"
        sub={cohort === "customers"
          ? (customerFunding === "spenders"
              ? "все commerce-клиенты с расходом окна · exact usage по provider/model с явным покрытием"
              : "выбранная funding-когорта commerce · exact usage по provider/model с явным покрытием")
          : "выданные OpenKeys · номинал и live usage движка остаются отдельной денежной властью"}
        badge={cohort === "customers"
          ? (customerTotal == null ? undefined : <Pill kind="ok">{count(customerTotal, "клиент", "клиента", "клиентов")}</Pill>)
          : (openkeysTotal == null ? undefined : <Pill kind="info">{count(openkeysTotal, "ключ", "ключа", "ключей")}</Pill>)}
      />
      <div className="paying-controls">
        <div className="paying-cohort-tabs" role="tablist" aria-label="Денежная когорта" onKeyDown={onTabsKeyDown}>
          <button
            type="button"
            role="tab"
            id="paying-tab-customers"
            aria-controls={cohort === "customers" ? "paying-panel-customers" : undefined}
            aria-selected={cohort === "customers"}
            tabIndex={cohort === "customers" ? 0 : -1}
            className={cohort === "customers" ? "on" : ""}
            onClick={() => onCohortChange("customers")}
          >
            <span aria-hidden="true">C</span>
            <b>Клиенты</b>
            <small>commerce ledger</small>
          </button>
          <button
            type="button"
            role="tab"
            id="paying-tab-openkeys"
            aria-controls={cohort === "openkeys" ? "paying-panel-openkeys" : undefined}
            aria-selected={cohort === "openkeys"}
            tabIndex={cohort === "openkeys" ? 0 : -1}
            className={cohort === "openkeys" ? "on" : ""}
            onClick={() => onCohortChange("openkeys")}
          >
            <span aria-hidden="true">O</span>
            <b>OpenKeys</b>
            <small>prepaid authority</small>
          </button>
        </div>
        <div className="paying-window-switch" role="group" aria-label="Окно расхода">
          <span>Окно расхода</span>
          {WINDOWS.map((window) => (
            <button
              type="button"
              key={window.days}
              className={days === window.days ? "on" : ""}
              aria-pressed={days === window.days}
              onClick={() => onDaysChange(window.days)}
            >
              {window.label}
            </button>
          ))}
        </div>
      </div>
    </>
  );
}

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

function payingTokenSummary(model: PayingUserUsageModel): string {
  return `вх ${model.input_tokens} · вых ${model.output_tokens} · cache read ${model.cache_read_tokens} · write 5m ${model.cache_write_5m_tokens} · write 1h ${model.cache_write_1h_tokens} · web ${model.web_search_requests}`;
}

export const PayingUsageDetails = memo(function PayingUsageDetails({ row }: { row: PayingUserRow }): ReactElement {
  const usage = row.usage;
  if (!usage || usage.status === "unavailable") {
    return <p className="paying-usage-unavailable"><Pill kind="warn">данные недоступны</Pill> Usage за окно {usage?.window ?? "—"} не получен; это не нулевой расход.</p>;
  }
  const warning = usage.status === "partial" ? (
    <p className="paying-usage-partial"><Pill kind="warn">частичные данные</Pill> Покрытие {usage.available_account_count}/{usage.account_count}; таблица и итоги ниже относятся только к доступной части.</p>
  ) : null;
  if (!usage.models.length) {
    return (
      <div className="paying-usage-copy">
        {warning}
        <p>Доступный отчёт: {usage.requests} запросов, моделей в окне нет. Official {usageNanoMoney(usage.total_official_nano)} · charged {usageNanoMoney(usage.total_charged_nano)}.</p>
      </div>
    );
  }
  return (
    <div className="paying-usage-copy">
      {warning}
      <p className="paying-usage-totals">Доступная часть: {usage.requests} запросов · official {usageNanoMoney(usage.total_official_nano)} · charged {usageNanoMoney(usage.total_charged_nano)}</p>
      <div className="openkeys-model-scroll paying-model-scroll">
        <table className="openkeys-model-table paying-model-table">
          <thead><tr><th className="left">провайдер</th><th className="left">модель</th><th>запросы</th><th className="left">токены</th><th>official</th><th>charged</th></tr></thead>
          <tbody>{usage.models.map((model, index) => (
            <tr key={`${model.provider ?? ""}:${model.model}:${index}`}>
              <td className="left"><b>{providerLabel(model.provider)}</b></td>
              <td className="left mono">{model.model}</td>
              <td>{model.requests}</td>
              <td className="left openkeys-token-data">{payingTokenSummary(model)}</td>
              <td className="openkeys-official-money">{usageNanoMoney(model.official_nano)}</td>
              <td className="openkeys-charged-money">{usageNanoMoney(model.charged_nano)}</td>
            </tr>
          ))}</tbody>
        </table>
      </div>
    </div>
  );
});

export const PayingRow = memo(function PayingRow({ row, rank, days }: { row: PayingUserRow; rank: number; days: PayingUserDays }) {
  const [expanded, setExpanded] = useState(false);
  const other = providerNano(row.provider_spend, "other");
  const discount = row.multiplier_bp == null ? null : 100 - row.multiplier_bp / 100;
  const detailsId = `paying-user-details-${rank}`;
  const providerPaid = row.funding_kind === "payments" || row.funding_kind === "payments_and_manual";
  const manualFunded = row.funding_kind === "manual" || row.funding_kind === "payments_and_manual";
  const bonusOnly = row.funding_kind === "bonus_only";
  const spendOnly = row.funding_kind === "spend_only";
  return (
    <Fragment>
      <tr>
        <td className="left paying-customer-cell">
          <span className="paying-rank" aria-label={`Место ${rank}`}>{String(rank).padStart(2, "0")}</span>
          <button type="button" className="paying-row-toggle" aria-label={`${expanded ? "Скрыть" : "Показать"} usage клиента ${row.email ?? "—"}`} aria-expanded={expanded} aria-controls={expanded ? detailsId : undefined} onClick={() => setExpanded((current) => !current)}>
            <span aria-hidden="true">{expanded ? "−" : "+"}</span>
            <span className="paying-customer-copy">
              <b><Dot kind={row.status === "disabled" ? "bad" : "off"} /> {row.email ?? "—"}</b>
              <span className="sub">
                {row.display_name || "Без имени"} · {payingTierLabel(row)}
                {discount == null ? "" : ` · скидка ${discount}%`}
              </span>
            </span>
          </button>
        </td>
        <td className="paying-money-cell">
          {bonusOnly ? (
            <>
              <Pill kind="info">строгий bonus-only</Pill>
              <b>{nanoMoney(row.bonus_funded_spent_nano)}</b>
              <span className="sub">денежных пополнений нет · не выручка</span>
            </>
          ) : spendOnly ? (
            <>
              <Pill kind="warn">расход без строгой классификации</Pill>
              <span className="sub">не bonus-only · lifetime деньги не подтверждены</span>
            </>
          ) : (
            <>
              <span className="paying-funding-badges">
                {providerPaid ? <Pill kind="ok">подтверждённый платёж</Pill> : null}
                {manualFunded ? <Pill>ручное пополнение</Pill> : null}
              </span>
              <b>{nanoMoney(row.paid_nano)}</b>
              <span className="sub">
                {row.funding_kind === "manual"
                  ? `${count(row.manual_topups_count ?? 0, "ручное пополнение", "ручных пополнения", "ручных пополнений")} · ${ago(row.last_paid_at)}`
                  : `${row.payments_count ?? 0} платежей${(row.manual_topups_count ?? 0) > 0 ? ` · ${row.manual_topups_count} ручных` : ""} · ${ago(row.last_paid_at)}`}
              </span>
              {isPositiveNano(row.manual_paid_nano) ? <span className="sub">вручную {nanoMoney(row.manual_paid_nano)}</span> : null}
            </>
          )}
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
      {expanded ? <tr id={detailsId} className="paying-model-row"><td colSpan={7}><PayingUsageDetails row={row} /></td></tr> : null}
    </Fragment>
  );
});

export function PayingLedger({
  data,
  funding,
  activeProvider,
  onProviderSelect,
}: {
  data: PayingUsersResponse;
  funding: PayingUserFunding;
  activeProvider: "" | PayingUserProvider;
  onProviderSelect: (provider: "" | PayingUserProvider) => void;
}): ReactElement {
  const summary = data.summary ?? {};
  const spend = summary.provider_spend;
  const spentTotal = summary.spent_nano ?? "0";
  const other = providerNano(spend, "other");
  const days = data.days ?? 30;
  const allSpenders = funding === "spenders";
  const fundingLabel = PAYING_USER_FUNDINGS.find(([value]) => value === funding)?.[1] ?? funding;
  return (
    <section className="paying-ledger" aria-label="Сводка клиентской когорты">
      <div className="paying-ledger-lead">
        <span>Получено денег</span>
        <strong>{nanoMoney(summary.paid_nano)}</strong>
        <small>
          за всё время · платежи и ручные пополнения · {count(summary.paying_users ?? 0, "денежный клиент", "денежных клиента", "денежных клиентов")}
          {summary.manual_paid_nano == null ? "" : ` · вручную ${nanoMoney(summary.manual_paid_nano)}`}
        </small>
      </div>
      <div className="paying-ledger-bonus">
        <span>Строгий bonus-only · {spendWindowLabel(days)}</span>
        <strong>{nanoMoney(summary.bonus_only_spent_nano)}</strong>
        <small>{count(summary.bonus_only_users ?? 0, "bonus-only клиент", "bonus-only клиента", "bonus-only клиентов")} · отдельно от mixed/legacy · не выручка</small>
      </div>
      <div className="paying-ledger-window">
        <span>{allSpenders ? "Все spenders" : `Выбранная когорта · ${fundingLabel}`} · {spendWindowLabel(days)}</span>
        <strong>{nanoMoney(spentTotal)}</strong>
        <small>{summary.active_spenders ?? 0} клиентов с расходом{allSpenders ? ", включая mixed/legacy/unattributed" : " в выбранной funding-когорте"} · обновлено {ago(data.generated_at)}</small>
      </div>
      <div className="paying-ledger-provider-area">
        <div className="paying-ledger-rail" aria-label="Распределение расхода по провайдерам">
          {PROVIDERS.map((provider) => {
            const share = providerShareBp(providerNano(spend, provider.id), spentTotal);
            return <i key={provider.id} className={provider.className} style={{ width: `${share / 100}%` }} />;
          })}
          {isPositiveNano(other) ? <i className="other" style={{ width: `${providerShareBp(other, spentTotal) / 100}%` }} /> : null}
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

type CustomerCohortProps = {
  page: PayingUsersPageState;
  search: string;
  setPage: Dispatch<SetStateAction<PayingUsersPageState>>;
  setSearch: Dispatch<SetStateAction<string>>;
  onTotalChange: (total: number) => void;
};

function CustomerCohort({ page, search, setPage, setSearch, onTotalChange }: CustomerCohortProps): ReactElement {
  const query = payingUsersQuery(page);
  const { data } = usePoll(
    `/admin/finance/paying-users?${query}`,
    () => api<PayingUsersResponse>(`/admin/finance/paying-users?${query}`),
    { interval: 30_000 },
  );
  const patchPage = useCallback((patch: Partial<PayingUsersPageState>, resetOffset = true) => {
    startTransition(() => setPage((current) => ({ ...current, ...patch, ...(resetOffset ? { offset: 0 } : {}) })));
  }, [setPage]);

  useEffect(() => {
    if (!data) return;
    onTotalChange(payingCohortUsers(data.summary));
    const total = data.total ?? 0;
    if (total > 0 && page.offset >= total) {
      const offset = Math.max(0, Math.floor((total - 1) / page.limit) * page.limit);
      startTransition(() => setPage((current) => current.offset === offset ? current : { ...current, offset }));
    }
  }, [data, onTotalChange, page.limit, page.offset, setPage]);

  const submitSearch = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    patchPage({ q: normalizePayingUsersSearch(search) });
  };

  return (
    <div id="paying-panel-customers" role="tabpanel" aria-labelledby="paying-tab-customers">
      {!data ? <LoadingGrid count={6} /> : (
        <>
          <PayingLedger
            data={data}
            funding={page.funding}
            activeProvider={page.provider}
            onProviderSelect={(provider) => patchPage({ provider })}
          />
          <SectionHeader
            title={page.funding === "spenders" ? "Все клиенты с расходом" : "Клиенты выбранной funding-когорты"}
            sub={`${data.total ?? 0} по текущему фильтру${page.funding === "spenders" ? " · spenders включают mixed, legacy и unattributed" : ""}`}
          />
          <form className="paying-toolbar" onSubmit={submitSearch}>
            <label className="sr-only" htmlFor="paying-search">Поиск клиентов с расходом</label>
            <input id="paying-search" name="q" type="search" maxLength={200} autoComplete="off" value={search} onChange={(event) => setSearch(event.target.value)} placeholder="email, имя или UUID…" />
            <label className="sr-only" htmlFor="paying-status">Статус</label>
            <select id="paying-status" value={page.status} onChange={(event) => patchPage({ status: event.target.value as PayingUsersPageState["status"] })}>
              <option value="">все статусы</option><option value="active">активные</option><option value="disabled">отключённые</option>
            </select>
            <label className="sr-only" htmlFor="paying-provider">Провайдер</label>
            <select id="paying-provider" value={page.provider} onChange={(event) => patchPage({ provider: event.target.value as PayingUsersPageState["provider"] })}>
              <option value="">все провайдеры</option><option value="anthropic">Claude</option><option value="openai">GPT</option><option value="google">Gemini</option><option value="other">другое / legacy</option>
            </select>
            <label className="sr-only" htmlFor="paying-funding">Фильтр spender-когорты</label>
            <select id="paying-funding" value={page.funding} title="Выберите всех spenders, lifetime money funding или строгий bonus-only" onChange={(event) => patchPage({ funding: event.target.value as PayingUsersPageState["funding"] })}>
              {PAYING_USER_FUNDINGS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select>
            <label className="sr-only" htmlFor="paying-sort">Сортировка</label>
            <select id="paying-sort" value={page.sort} onChange={(event) => patchPage({ sort: event.target.value as PayingUsersPageState["sort"] })}>
              {PAYING_USER_SORTS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select>
            <button type="button" className="paying-dir" aria-label={page.dir === "desc" ? "Сейчас по убыванию; переключить на возрастание" : "Сейчас по возрастанию; переключить на убывание"} title={page.dir === "desc" ? "По убыванию" : "По возрастанию"} onClick={() => patchPage({ dir: page.dir === "desc" ? "asc" : "desc" })}>
              {page.dir === "desc" ? "↓" : "↑"}
            </button>
            <button className="btn" type="submit">Найти</button>
            <button className="btn ghost" type="button" title="Выгрузить текущую страницу: одна строка на user × provider × model" onClick={() => downloadCsv(`paying-users-${page.days}d-${csvDate()}.csv`, PAYING_USERS_CSV_HEADER, buildPayingUsersCsvRows(data.rows ?? []))}>CSV</button>
          </form>
          <TableCard>
            <table className="paying-table">
              <thead><tr><th className="left">клиент</th><th>финансирование</th><th>расход · {page.days === 1 ? "24ч" : `${page.days}д`}</th>{PROVIDERS.map((provider) => <th key={provider.id} className={`paying-provider-head ${provider.className}`}><i />{provider.label}</th>)}<th>активность</th></tr></thead>
              <tbody>
                {(data.rows ?? []).length ? (data.rows ?? []).map((row, index) => <PayingRow key={row.user_id ?? row.email ?? index} row={row} rank={(data.offset ?? page.offset) + index + 1} days={page.days} />) : <EmptyRow columns={7} text="клиентов выбранной когорты по этому фильтру нет" />}
              </tbody>
            </table>
          </TableCard>
          <div className="pager">
            <span>{data.total ? (data.offset ?? page.offset) + 1 : 0}–{Math.min((data.offset ?? page.offset) + (data.limit ?? page.limit), data.total ?? 0)} из {data.total ?? 0}</span>
            <button type="button" className="btn ghost" disabled={(data.offset ?? page.offset) <= 0} onClick={() => patchPage({ offset: Math.max(0, (data.offset ?? page.offset) - (data.limit ?? page.limit)) }, false)}>Назад</button>
            <button type="button" className="btn ghost" disabled={(data.offset ?? page.offset) + (data.limit ?? page.limit) >= (data.total ?? 0)} onClick={() => patchPage({ offset: (data.offset ?? page.offset) + (data.limit ?? page.limit) }, false)}>Дальше</button>
          </div>
          <footer>
            По умолчанию режим «все с расходом» включает каждого commerce spender окна: money-funded, строгий bonus-only, mixed, legacy и unattributed; узкие funding-фильтры сужают строки и сводку. `spend_only` означает расход без строгой классификации и никогда не означает bonus-only. Lifetime деньги и строгий bonus-only показаны отдельно; бонус не является выручкой. Partial usage отражает только доступные аккаунты, unavailable не подменяется нулём. Расход аккаунтов без клиента — на странице «Расход движка».
          </footer>
        </>
      )}
    </div>
  );
}

function tokenSummary(model: OpenkeysUsageModel): string {
  return `вх ${model.input_tokens} · вых ${model.output_tokens} · cache read ${model.cache_read_tokens} · write 5m ${model.cache_write_5m_tokens} · write 1h ${model.cache_write_1h_tokens}${model.web_search_requests ? ` · web ${model.web_search_requests}` : ""}`;
}

const OpenkeysModelTable = memo(function OpenkeysModelTable({ row }: { row: OpenkeysPayingRow }): ReactElement {
  if (row.usage.status === "unavailable") {
    return <p className="openkeys-usage-unavailable"><Pill kind="warn">usage недоступен</Pill> Движок не вернул данные за окно {row.usage.window}; это не нулевой расход.</p>;
  }
  if (!row.usage.models.length) {
    return <p className="openkeys-usage-empty">Доступный отчёт: {row.usage.requests} запросов, моделей в окне нет. Official {nanoMoney(row.usage.total_official_nano)} · charged {nanoMoney(row.usage.total_charged_nano)}.</p>;
  }
  return (
    <div className="openkeys-model-scroll">
      <table className="openkeys-model-table">
        <thead><tr><th className="left">провайдер</th><th className="left">модель</th><th>запросы</th><th className="left">токены</th><th>official</th><th>charged</th></tr></thead>
        <tbody>{row.usage.models.map((model, index) => (
          <tr key={`${model.provider ?? ""}:${model.model}:${index}`}>
            <td className="left"><b>{providerLabel(model.provider)}</b></td>
            <td className="left mono">{model.model}</td>
            <td>{model.requests}</td>
            <td className="left openkeys-token-data">{tokenSummary(model)}</td>
            <td className="openkeys-official-money">{nanoMoney(model.official_nano)}</td>
            <td className="openkeys-charged-money">{nanoMoney(model.charged_nano)}</td>
          </tr>
        ))}</tbody>
      </table>
    </div>
  );
});

export function OpenkeysPayingTable({ data }: { data: OpenkeysPayingResponse }): ReactElement {
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const toggle = (id: string) => setExpanded((current) => {
    const next = new Set(current);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });
  return (
    <TableCard>
      <table className="openkeys-paying-table">
        <thead><tr><th className="left">ключ / партия / продавец</th><th>состояние</th><th>номинал</th><th>расход всего</th><th>charged · {data.days === 1 ? "24ч" : `${data.days}д`}</th><th>выдача</th></tr></thead>
        <tbody>
          {data.rows.length ? data.rows.map((row) => {
            const detailsId = `openkeys-paying-details-${row.id}`;
            const isExpanded = expanded.has(row.id);
            const charged = openkeysChargedNano(row.usage);
            return (
              <Fragment key={row.id}>
                <tr>
                  <td className="left openkeys-key-register">
                    <button type="button" className="openkeys-row-toggle" aria-expanded={isExpanded} aria-controls={isExpanded ? detailsId : undefined} onClick={() => toggle(row.id)}>
                      <span aria-hidden="true">{isExpanded ? "−" : "+"}</span>
                      <b className="mono">{row.keyMasked}</b>
                    </button>
                    <small><b>{row.batchLabel || "Без метки"}</b> · <span className="mono">{row.batchId}</span> · {row.createdBy}</small>
                    <small className="mono">{row.engineAccountId} · {row.apiType}</small>
                  </td>
                  <td>
                    <Pill kind={row.enabled ? "ok" : "bad"}>{row.enabled ? "активен" : "отключён"}</Pill>
                    <span className="openkeys-lifecycle"><Pill kind={row.lifecycle === "delivered" ? "info" : "warn"}>{row.lifecycle === "delivered" ? "выдан" : "на складе"}</Pill></span>
                  </td>
                  <td className="openkeys-nominal-money"><b>{nanoMoney(row.faceValueNano)}</b><small>{row.pricingContract}</small></td>
                  <td className="openkeys-charged-total">{row.lifetimeSpentNano === null ? <><Pill kind="warn">недоступен</Pill><small>не $0</small></> : <><b>{nanoMoney(row.lifetimeSpentNano)}</b><small>lifetime движка</small></>}</td>
                  <td className="openkeys-charged-total">{charged === null ? <><Pill kind="warn">недоступен</Pill><small>не $0</small></> : <><b>{nanoMoney(charged)}</b><small>за выбранное окно</small></>}</td>
                  <td>{row.deliveredAt ? formatDate(row.deliveredAt, true) : <><b>ещё не выдан</b><span className="sub">создан {formatDate(row.createdAt, true)}</span></>}</td>
                </tr>
                {isExpanded ? <tr id={detailsId} className="openkeys-model-row"><td colSpan={6}><OpenkeysModelTable row={row} /></td></tr> : null}
              </Fragment>
            );
          }) : <EmptyRow columns={6} text="живых OpenKeys по этому фильтру нет" />}
        </tbody>
      </table>
    </TableCard>
  );
}

type OpenkeysCohortProps = {
  page: OpenkeysPayingPageState;
  search: string;
  setPage: Dispatch<SetStateAction<OpenkeysPayingPageState>>;
  setSearch: Dispatch<SetStateAction<string>>;
  onTotalChange: (total: number) => void;
};

function OpenkeysCohort({ page, search, setPage, setSearch, onTotalChange }: OpenkeysCohortProps): ReactElement {
  const query = openkeysPayingQuery(page);
  const { data } = usePoll(
    `/openkeys-admin/paying-keys?${query}`,
    () => api<OpenkeysPayingResponse>(`/openkeys-admin/paying-keys?${query}`),
    { interval: 30_000 },
  );
  const patchPage = useCallback((patch: Partial<OpenkeysPayingPageState>, resetOffset = true) => {
    startTransition(() => setPage((current) => ({ ...current, ...patch, ...(resetOffset ? { offset: 0 } : {}) })));
  }, [setPage]);

  useEffect(() => {
    if (!data) return;
    onTotalChange(data.total);
    if (page.offset > 0 && page.offset >= data.total) {
      const offset = clampOpenkeysPayingOffset(
        data.total > 0 ? Math.floor((data.total - 1) / page.limit) * page.limit : 0,
      );
      startTransition(() => setPage((current) => current.offset === offset ? current : { ...current, offset }));
    }
  }, [data, onTotalChange, page.limit, page.offset, setPage]);

  const submitSearch = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    patchPage({ q: search.trim().slice(0, 80) });
  };

  return (
    <div id="paying-panel-openkeys" role="tabpanel" aria-labelledby="paying-tab-openkeys">
      {!data ? <LoadingGrid count={5} /> : (
        <>
          <SectionHeader title="OpenKeys" sub={`${data.total} живых ключей по текущему фильтру · складские и выданные · usage не входит в commerce ledger`} />
          <form className="paying-toolbar openkeys-paying-toolbar" onSubmit={submitSearch}>
            <label className="sr-only" htmlFor="openkeys-paying-search">Поиск OpenKeys</label>
            <input id="openkeys-paying-search" type="search" maxLength={80} value={search} onChange={(event) => setSearch(event.target.value)} placeholder="маска, партия, продавец или account…" />
            <label className="sr-only" htmlFor="openkeys-paying-status">Статус OpenKeys</label>
            <select id="openkeys-paying-status" value={page.status} onChange={(event) => patchPage({ status: event.target.value as OpenkeysPayingPageState["status"] })}>
              <option value="all">все статусы</option><option value="active">активные</option><option value="disabled">отключённые</option>
            </select>
            <label className="sr-only" htmlFor="openkeys-paying-sort">Сортировка OpenKeys</label>
            <select id="openkeys-paying-sort" value={page.sort} onChange={(event) => patchPage({ sort: event.target.value as OpenkeysPayingPageState["sort"] })}>
              {OPENKEYS_PAYING_SORTS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select>
            <button type="button" className="paying-dir" aria-label={page.dir === "desc" ? "Сейчас по убыванию; переключить на возрастание" : "Сейчас по возрастанию; переключить на убывание"} title={page.dir === "desc" ? "По убыванию" : "По возрастанию"} onClick={() => patchPage({ dir: page.dir === "desc" ? "asc" : "desc" })}>
              {page.dir === "desc" ? "↓" : "↑"}
            </button>
            <button className="btn" type="submit">Найти</button>
            <button className="btn ghost" type="button" title="Выгрузить текущую страницу: одна строка на provider/model" onClick={() => downloadCsv(`openkeys-paying-${page.days}d-${csvDate()}.csv`, OPENKEYS_PAYING_CSV_HEADER, buildOpenkeysPayingCsvRows(data.rows))}>CSV</button>
          </form>
          <OpenkeysPayingTable data={data} />
          <div className="pager">
            <span>{data.total ? data.offset + 1 : 0}–{Math.min(data.offset + data.limit, data.total)} из {data.total}</span>
            <button type="button" className="btn ghost" disabled={data.offset <= 0} onClick={() => patchPage({ offset: Math.max(0, data.offset - data.limit) }, false)}>Назад</button>
            <button type="button" className="btn ghost" disabled={data.offset >= OPENKEYS_PAYING_MAX_OFFSET || data.offset + data.limit >= data.total} onClick={() => patchPage({ offset: clampOpenkeysPayingOffset(data.offset + data.limit) }, false)}>Дальше</button>
          </div>
          <footer>OpenKeys — отдельная prepaid-когорта. Номинал ключа и charged usage движка показаны отдельно; недоступный отчёт никогда не подменяется нулём и не добавляется в сводку commerce.</footer>
        </>
      )}
    </div>
  );
}

export default function PayingUsersPage(): ReactElement {
  const [cohort, setCohort] = useState<PayingCohort>("customers");
  const [days, setDays] = useState<PayingUserDays>(30);
  const [customerPage, setCustomerPage] = useState<PayingUsersPageState>(INITIAL_PAYING_USERS_PAGE);
  const [openkeysPage, setOpenkeysPage] = useState<OpenkeysPayingPageState>(INITIAL_OPENKEYS_PAYING_PAGE);
  const [customerSearch, setCustomerSearch] = useState("");
  const [openkeysSearch, setOpenkeysSearch] = useState("");
  const [customerTotal, setCustomerTotal] = useState<number>();
  const [openkeysTotal, setOpenkeysTotal] = useState<number>();

  const changeDays = useCallback((nextDays: PayingUserDays) => {
    setDays(nextDays);
    startTransition(() => {
      setCustomerPage((current) => ({ ...current, days: nextDays, offset: 0 }));
      setOpenkeysPage((current) => ({ ...current, days: nextDays as OpenkeysPayingDays, offset: 0 }));
    });
  }, []);
  return (
    <div className="paying-page">
      <CohortControls
        cohort={cohort}
        days={days}
        customerFunding={customerPage.funding}
        customerTotal={customerTotal}
        openkeysTotal={openkeysTotal}
        onCohortChange={setCohort}
        onDaysChange={changeDays}
      />
      {cohort === "customers" ? (
        <CustomerCohort page={customerPage} search={customerSearch} setPage={setCustomerPage} setSearch={setCustomerSearch} onTotalChange={setCustomerTotal} />
      ) : (
        <OpenkeysCohort page={openkeysPage} search={openkeysSearch} setPage={setOpenkeysPage} setSearch={setOpenkeysSearch} onTotalChange={setOpenkeysTotal} />
      )}
    </div>
  );
}
