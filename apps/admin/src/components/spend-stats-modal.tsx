"use client";

// Модалка «Кто тратит» — порт spendStats() из admin-panel.js (строки 395-475):
// разбивка расхода по окнам 24ч/7д/30д + произвольный диапазон дат. «списано» —
// по множителю аккаунта, «real-API» — полный эквивалент провайдера; сверху
// сводка со средней скидкой и отдельным блоком OpenKeys (у портала своя
// экономика, смешивать её с обычными клиентами бесполезно).
//
// Использование на странице:
//   const { openSpendStats, spendStatsModal } = useSpendStatsModal();
//   <StatCard ... onClick={openSpendStats} title="Разбивка: сутки / 7 дней / 30 дней" />
//   {spendStatsModal}
import { useCallback, useMemo, useState, type FormEvent, type ReactElement, type ReactNode } from "react";
import { api } from "@/lib/api";
import { useResources } from "@/lib/resources";
import { ago, formatDate, money, nanoMoney } from "@/lib/format";
import { CardGrid, EmptyRow, LoadingGrid, Modal, SectionHeader, StatCard, TableCard } from "@/components/ui";

export type SpendAccount = {
  handle?: string;
  account?: string;
  requests?: number;
  charge_usd?: number;
  real_usd?: number;
  /** epoch-секунды последней активности. */
  last_ts?: number;
};
export type SpendProviderRow = { provider?: string; requests?: number; charge_usd?: number; real_usd?: number };
export type SpendModelRow = { model?: string; provider?: string; requests?: number; charge_usd?: number; real_usd?: number };
export type SpendPeriod = {
  requests?: number;
  charge_usd?: number;
  real_usd?: number;
  accounts?: SpendAccount[];
  providers?: SpendProviderRow[];
  models?: SpendModelRow[];
};
export type SpendStatsResponse = {
  periods?: { d1?: SpendPeriod; d7?: SpendPeriod; d30?: SpendPeriod };
  custom?: SpendPeriod;
};

// Строка /openkeys-admin/lookup: метка, номинал, продавец и профиль ключа.
export type OkDirectoryRow = {
  engineAccountId?: string;
  batchLabel?: string;
  faceValueNano?: string;
  createdBy?: string;
  apiType?: string;
  viewUrl?: string;
};

// Аккаунты портала OpenKeys узнаются по handle: он задаётся при выпуске ключа
// и другого способа отличить их на стороне движка нет.
export const isOpenkeys = (handle: string | null | undefined): boolean => /^openkeys-/i.test(String(handle ?? ""));

const PERIODS = [
  ["d1", "Сутки (24ч)"],
  ["d7", "7 дней"],
  ["d30", "30 дней"],
] as const;
type PeriodKey = (typeof PERIODS)[number][0] | "custom";

const discount = (charge: number, real: number): string =>
  real > 0 ? Math.round((1 - charge / real) * 100) + "%" : "—";

const providerLabel = (name: string | undefined): string =>
  name === "openai" ? "OpenAI (Codex)" : name === "anthropic" ? "Claude (подписки)" : name || "—";

export const okTypeLabel = (type: string | undefined): string => (type === "openai" ? "OpenAI" : "Claude");

export function OkInfo({ meta }: { meta: OkDirectoryRow | undefined }): ReactElement | null {
  if (!meta) return null;
  return (
    <div className="sub">
      {meta.batchLabel || "Без метки"} · {nanoMoney(meta.faceValueNano)} · {meta.createdBy ?? "—"} ·{" "}
      {okTypeLabel(meta.apiType)}
      {meta.viewUrl ? (
        <>
          {" · "}
          <a className="link" href={meta.viewUrl} target="_blank" rel="noreferrer">
            профиль ↗
          </a>
        </>
      ) : null}
    </div>
  );
}

function PeriodBody({
  period,
  okDir,
  subtitle,
}: {
  period: SpendPeriod | undefined;
  okDir: Map<string, OkDirectoryRow> | null;
  subtitle?: string;
}): ReactElement {
  const accounts = period?.accounts ?? [];
  const providers = period?.providers ?? [];
  const models = period?.models ?? [];
  // Отдельная сводка по OpenKeys (см. комментарий в шапке файла).
  const ok = accounts.filter((item) => isOpenkeys(item.handle));
  const okCharge = ok.reduce((sum, item) => sum + (item.charge_usd || 0), 0);
  const okReal = ok.reduce((sum, item) => sum + (item.real_usd || 0), 0);
  const okRequests = ok.reduce((sum, item) => sum + (item.requests || 0), 0);
  return (
    <>
      {subtitle ? <p className="dlg-sub">{subtitle}</p> : null}
      <CardGrid>
        <StatCard label="списано клиентам" value={money(period?.charge_usd)} hint={`${period?.requests ?? 0} запросов`} />
        <StatCard
          label="real-API эквивалент"
          value={money(period?.real_usd)}
          hint={`средняя скидка ${discount(period?.charge_usd ?? 0, period?.real_usd ?? 0)}`}
        />
        <StatCard
          label="OpenKeys"
          value={money(okReal)}
          hint={`${ok.length} ключей · ${okRequests} запросов · списано ${money(okCharge)}`}
        />
      </CardGrid>

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
              providers.map((item, index) => (
                <tr key={item.provider ?? index}>
                  <td className="left">
                    <b>{providerLabel(item.provider)}</b>
                  </td>
                  <td>{item.requests ?? 0}</td>
                  <td>
                    <b>{money(item.charge_usd)}</b>
                  </td>
                  <td>{money(item.real_usd)}</td>
                  <td>{discount(item.charge_usd ?? 0, item.real_usd ?? 0)}</td>
                </tr>
              ))
            ) : (
              <EmptyRow columns={5} />
            )}
          </tbody>
        </table>
      </TableCard>

      <SectionHeader title="По моделям" sub="top-20 по списанию за активное окно" />
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
              models.map((item, index) => (
                <tr key={`${item.provider ?? ""}/${item.model ?? index}`}>
                  <td className="left">
                    <b>{item.model ?? "—"}</b>
                  </td>
                  <td className="left">{providerLabel(item.provider)}</td>
                  <td>{item.requests ?? 0}</td>
                  <td>
                    <b>{money(item.charge_usd)}</b>
                  </td>
                  <td>{money(item.real_usd)}</td>
                  <td>{discount(item.charge_usd ?? 0, item.real_usd ?? 0)}</td>
                </tr>
              ))
            ) : (
              <EmptyRow columns={6} />
            )}
          </tbody>
        </table>
      </TableCard>

      <SectionHeader title="По аккаунтам" />
      <TableCard>
        <table>
          <thead>
            <tr>
              <th className="left">account</th>
              <th>запросы</th>
              <th>списано</th>
              <th>real-API</th>
              <th>скидка</th>
              <th>активность</th>
            </tr>
          </thead>
          <tbody>
            {accounts.length ? (
              accounts.map((item, index) => {
                const displayName = item.handle || "—";
                return (
                  <tr key={item.account ?? item.handle ?? index}>
                    <td className="left">
                      <b>{displayName}</b>
                      {isOpenkeys(item.handle) ? (
                        <span className="okb" title="Выпущен через OpenKeys">
                          OpenKeys
                        </span>
                      ) : null}
                      <div className="sub mono">
                        {item.handle && displayName !== item.handle ? item.handle : item.account ?? "—"}
                      </div>
                      <OkInfo meta={okDir?.get(String(item.account ?? ""))} />
                    </td>
                    <td>{item.requests ?? 0}</td>
                    <td>
                      <b>{money(item.charge_usd)}</b>
                    </td>
                    <td>{money(item.real_usd)}</td>
                    <td>{discount(item.charge_usd ?? 0, item.real_usd ?? 0)}</td>
                    <td>{ago((item.last_ts ?? 0) * 1000)}</td>
                  </tr>
                );
              })
            ) : (
              <EmptyRow columns={6} />
            )}
          </tbody>
        </table>
      </TableCard>
    </>
  );
}

// Внутренний компонент перемонтируется при каждом открытии (обёртка ниже
// возвращает null при закрытии), поэтому сброс состояния не нужен — вкладка
// всегда стартует с d1, а данные грузятся заново (окна live).
function SpendStatsContent({ onClose }: { onClose: () => void }): ReactElement {
  const { data: resources } = useResources<{
    stats: SpendStatsResponse;
    directory: { rows?: OkDirectoryRow[] };
  }>({
    stats: "/spend-stats",
    directory: "/openkeys-admin/lookup",
  });
  const data = resources.stats;
  const okDir = useMemo(
    () => new Map((resources.directory?.rows ?? []).map((row) => [String(row.engineAccountId ?? ""), row])),
    [resources.directory],
  );
  const [periodKey, setPeriodKey] = useState<PeriodKey>("d1");
  const [custom, setCustom] = useState<SpendPeriod | null>(null);
  const [customSubtitle, setCustomSubtitle] = useState("");
  const [customError, setCustomError] = useState("");
  const [customBusy, setCustomBusy] = useState(false);
  const [fromDate, setFromDate] = useState("");
  const [toDate, setToDate] = useState("");

  // Произвольный диапазон: /spend-stats?from&to (epoch-секунды). «по»
  // включительно → полуоткрытая граница +1 сутки; лимит 92 дней и зажатие
  // будущего — на сервере, его 400 показываем текстом.
  const submitCustom = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setCustomError("");
    if (!fromDate || !toDate) {
      setCustomError("Выберите обе даты.");
      return;
    }
    const [fromYear, fromMonth, fromDay] = fromDate.split("-").map(Number);
    const [toYear, toMonth, toDay] = toDate.split("-").map(Number);
    const from = (new Date(fromYear, fromMonth - 1, fromDay).getTime() / 1000) | 0;
    const to = (new Date(toYear, toMonth - 1, toDay).getTime() / 1000) | 0;
    setCustomBusy(true);
    try {
      const response = await api<{ custom?: SpendPeriod }>(`/spend-stats?from=${from}&to=${to + 86400}`);
      if (!response.custom) throw new Error("Ответ без custom-блока — обновите страницу панели.");
      setCustom(response.custom);
      setCustomSubtitle(
        `Диапазон ${formatDate(from * 1000)} — ${formatDate(to * 1000)} · те же top-50, что у стандартных окон`,
      );
      setPeriodKey("custom");
    } catch (cause) {
      setCustomError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setCustomBusy(false);
    }
  };

  const activePeriod = periodKey === "custom" ? (custom ?? undefined) : data?.periods?.[periodKey];

  return (
    <Modal
      open
      onClose={onClose}
      wide
      title="Кто тратит"
      message="«списано» — по множителю аккаунта · «real-API» — полный эквивалент провайдера · топ-50 за окно"
    >
      <div className="spend-tabs">
        {PERIODS.map(([key, label]) => (
          <button
            key={key}
            type="button"
            className={"btn" + (periodKey === key ? " on" : "")}
            onClick={() => {
              setPeriodKey(key);
              setCustomError("");
            }}
          >
            {label}
          </button>
        ))}
      </div>
      <form className="toolbar" style={{ margin: "0 0 12px" }} onSubmit={submitCustom}>
        <label className="sr-only" htmlFor="spend-from">
          С даты
        </label>
        <input id="spend-from" name="from" type="date" autoComplete="off" value={fromDate} onChange={(event) => setFromDate(event.target.value)} />
        <label className="sr-only" htmlFor="spend-to">
          По дату
        </label>
        <input id="spend-to" name="to" type="date" autoComplete="off" value={toDate} onChange={(event) => setToDate(event.target.value)} />
        <button className="btn" type="submit" disabled={customBusy}>
          Показать
        </button>
        <span className="note" style={{ color: "var(--bad)" }}>
          {customError}
        </span>
      </form>
      {data === undefined ? (
        <LoadingGrid count={4} />
      ) : (
        <PeriodBody period={activePeriod} okDir={okDir} subtitle={periodKey === "custom" ? customSubtitle : undefined} />
      )}
      <div className="dlg-actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          Закрыть
        </button>
      </div>
    </Modal>
  );
}

// Обёртка: закрытая модалка ничего не рендерит; при открытии контент
// монтируется заново (чистое состояние вкладок и свежая загрузка).
export function SpendStatsModal({ open, onClose }: { open: boolean; onClose: () => void }): ReactElement | null {
  if (!open) return null;
  return <SpendStatsContent onClose={onClose} />;
}

// Готовая пара «триггер + модалка» для страниц: openSpendStats вешается на
// StatCard.onClick / <th>, spendStatsModal рендерится в конце страницы.
export function useSpendStatsModal(): { openSpendStats: () => void; spendStatsModal: ReactNode } {
  const [open, setOpen] = useState(false);
  const openSpendStats = useCallback(() => setOpen(true), []);
  const close = useCallback(() => setOpen(false), []);
  return { openSpendStats, spendStatsModal: <SpendStatsModal open={open} onClose={close} /> };
}
