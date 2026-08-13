"use client";

// Пополнения — порт 1:1 функции topups() из crates/server/src/admin-panel.js
// (строки 732-766): платежи + checkout-воронка одним экраном, общий offset
// листает оба списка, один status-фильтр применяется к платежам и чекаутам
// одновременно. Commerce SSE — быстрый путь, общий freshness-bridge — страховочный.
import { memo, startTransition, useCallback, useEffect, useState, type FormEvent } from "react";
import { useResource } from "@/lib/resources";
import { count, formatDate, money } from "@/lib/format";
import { csvDate, downloadCsv } from "@/lib/csv";
import { EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, TableCard } from "@/components/ui";

export const TOPUP_PROVIDERS = ["cryptomus", "platega"] as const;
export const TOPUP_STATUSES = ["paid", "refunded", "disputed", "failed", "pending", "canceled", "creating"] as const;
export const TOPUP_LIMIT = 50;

export interface TopupPayment {
  email?: string;
  user_id?: string;
  provider?: string;
  amount_usd?: number | string;
  status?: string;
  credit_status?: string;
  paid_at?: string;
  provider_payment_id?: string;
}

export interface TopupCheckout {
  email?: string;
  user_id?: string;
  provider?: string;
  amount_usd?: number | string;
  status?: string;
  created_at?: string;
  expires_at?: string;
  provider_payment_id?: string;
}

export interface TopupsResponse {
  payments?: TopupPayment[];
  checkouts?: TopupCheckout[];
  payments_total?: number;
  checkouts_total?: number;
}

export interface TopupsQuery {
  offset: number;
  limit: number;
  q: string;
  provider: string;
  status: string;
}

// Путь запроса ровно как в легаси: limit/offset всегда, фильтры — только непустые.
export function topupsPath(query: TopupsQuery): string {
  const params = new URLSearchParams({ limit: String(query.limit), offset: String(query.offset) });
  if (query.q) params.set("q", query.q);
  if (query.provider) params.set("provider", query.provider);
  if (query.status) params.set("status", query.status);
  return "/admin/topups?" + params.toString();
}

// Старый backend totals не отдаёт — деградируем к размеру текущей страницы
// (пагинация при этом скрывается), как в легаси.
export function computeTotals(data: TopupsResponse | null | undefined): {
  paymentsTotal: number;
  checkoutsTotal: number;
  total: number;
} {
  const payments = data?.payments ?? [];
  const checkouts = data?.checkouts ?? [];
  const paymentsTotal = data?.payments_total ?? payments.length;
  const checkoutsTotal = data?.checkouts_total ?? checkouts.length;
  return { paymentsTotal, checkoutsTotal, total: Math.max(paymentsTotal, checkoutsTotal) };
}

// Легаси-кламп: offset уехал за пределы total (и total > 0) → последняя страница.
export function clampOffset(offset: number, limit: number, total: number): number {
  if (offset < total || total <= 0) return offset;
  return Math.max(0, Math.floor((total - 1) / limit) * limit);
}

export const TOPUP_CSV_HEADER = [
  "kind",
  "email",
  "user_id",
  "провайдер",
  "сумма_usd",
  "статус",
  "зачисление",
  "оплачен",
  "создан",
  "истекает",
  "provider_id",
];

// Один CSV на оба списка: строки различаются колонкой kind=payment|checkout.
export function buildTopupCsvRows(payments: TopupPayment[], checkouts: TopupCheckout[]): unknown[][] {
  return payments
    .map((item) => [
      "payment",
      item.email,
      item.user_id,
      item.provider,
      item.amount_usd,
      item.status,
      item.credit_status || "",
      item.paid_at || "",
      "",
      "",
      item.provider_payment_id || "",
    ])
    .concat(
      checkouts.map((item) => [
        "checkout",
        item.email,
        item.user_id || "",
        item.provider,
        item.amount_usd,
        item.status,
        "",
        "",
        item.created_at || "",
        item.expires_at || "",
        item.provider_payment_id || "",
      ]),
    );
}

// Состояние фильтров переживает навигацию между страницами — как module-level
// topupsPage в admin-panel.js.
let savedQuery: TopupsQuery = { offset: 0, limit: TOPUP_LIMIT, q: "", provider: "", status: "" };

const PAYMENT_HEAD = (
  <thead>
    <tr>
      <th className="left">клиент</th>
      <th>провайдер</th>
      <th>сумма</th>
      <th>платёж</th>
      <th>зачисление</th>
      <th>оплачен</th>
      <th className="left">provider id</th>
    </tr>
  </thead>
);

const CHECKOUT_HEAD = (
  <thead>
    <tr>
      <th className="left">клиент</th>
      <th>провайдер</th>
      <th>сумма</th>
      <th>статус</th>
      <th>создан</th>
      <th>истекает</th>
      <th className="left">provider id</th>
    </tr>
  </thead>
);

const PaymentRow = memo(function PaymentRow({ item }: { item: TopupPayment }) {
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
        <Pill kind={item.status === "paid" ? "ok" : "warn"}>{item.status}</Pill>
      </td>
      <td>
        <Pill kind={item.credit_status === "confirmed" ? "ok" : "warn"}>{item.credit_status || "—"}</Pill>
      </td>
      <td>{formatDate(item.paid_at, true)}</td>
      <td className="left mono muted">{item.provider_payment_id}</td>
    </tr>
  );
});

const CheckoutRow = memo(function CheckoutRow({ item }: { item: TopupCheckout }) {
  return (
    <tr>
      <td className="left">
        <b>{item.email}</b>
      </td>
      <td>
        <Pill>{item.provider}</Pill>
      </td>
      <td>
        <b>{money(item.amount_usd)}</b>
      </td>
      <td>
        <Pill kind={item.status === "pending" ? "warn" : "bad"}>{item.status}</Pill>
      </td>
      <td>{formatDate(item.created_at, true)}</td>
      <td>{formatDate(item.expires_at, true)}</td>
      <td className="left mono muted">{item.provider_payment_id || "—"}</td>
    </tr>
  );
});

function Pager(props: { offset: number; limit: number; total: number; onOffset: (offset: number) => void }) {
  const { offset, limit, total, onOffset } = props;
  return (
    <div className="pager">
      <span>
        {total ? offset + 1 : 0}–{Math.min(offset + limit, total)} из {total}
      </span>
      <button type="button" className="btn ghost" disabled={offset <= 0} onClick={() => onOffset(Math.max(0, offset - limit))}>
        Назад
      </button>
      <button
        type="button"
        className="btn ghost"
        disabled={offset + limit >= total}
        onClick={() => onOffset(offset + limit)}
      >
        Дальше
      </button>
    </div>
  );
}

export default function TopupsPage() {
  const [query, setQuery] = useState<TopupsQuery>(savedQuery);
  // Черновики фильтров применяются только сабмитом формы («Найти»), как в легаси.
  const [draftQ, setDraftQ] = useState(savedQuery.q);
  const [draftProvider, setDraftProvider] = useState(savedQuery.provider);
  const [draftStatus, setDraftStatus] = useState(savedQuery.status);

  const updateQuery = useCallback((updater: (current: TopupsQuery) => TopupsQuery) => {
    startTransition(() => {
      setQuery((current) => {
        const next = updater(current);
        savedQuery = next;
        return next;
      });
    });
  }, []);

  const path = topupsPath(query);
  // Сам URL — ключ ресурса: смена фильтров/страницы получает отдельный снапшот,
  // данные предыдущего переживают навигацию (stale-while-revalidate).
  const { data } = useResource<TopupsResponse>(path);

  const payments = data?.payments ?? [];
  const checkouts = data?.checkouts ?? [];
  const { paymentsTotal, checkoutsTotal, total } = computeTotals(data);

  // Если текущий offset оказался за пределами total (сократились данные) —
  // уходим на последнюю страницу, легаси при этом перезапрашивал список.
  useEffect(() => {
    if (data === undefined) return;
    updateQuery((current) => {
      const clamped = clampOffset(current.offset, current.limit, total);
      return clamped === current.offset ? current : { ...current, offset: clamped };
    });
  }, [data, total, updateQuery]);

  if (data === undefined) {
    return (
      <>
        <PageHead title="Пополнения" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    updateQuery((current) => ({
      ...current,
      offset: 0,
      q: draftQ.trim(),
      provider: draftProvider,
      status: draftStatus,
    }));
  };

  const onCsv = () => {
    downloadCsv(`topups-${csvDate()}.csv`, TOPUP_CSV_HEADER, buildTopupCsvRows(payments, checkouts));
  };

  return (
    <>
      <PageHead
        title="Пополнения"
        sub="платежи и checkout-воронка"
        badge={<Pill kind="ok">{count(total, "запись", "записи", "записей")}</Pill>}
      />

      <form className="toolbar" onSubmit={onSubmit}>
        <label className="sr-only" htmlFor="topup-q">
          Поиск по email
        </label>
        <input
          id="topup-q"
          type="search"
          value={draftQ}
          placeholder="email клиента…"
          onChange={(event) => setDraftQ(event.target.value)}
        />
        <label className="sr-only" htmlFor="topup-provider">
          Провайдер
        </label>
        <select id="topup-provider" value={draftProvider} onChange={(event) => setDraftProvider(event.target.value)}>
          <option value="">все провайдеры</option>
          {TOPUP_PROVIDERS.map((provider) => (
            <option key={provider} value={provider}>
              {provider}
            </option>
          ))}
        </select>
        <label className="sr-only" htmlFor="topup-status">
          Статус
        </label>
        <select id="topup-status" value={draftStatus} onChange={(event) => setDraftStatus(event.target.value)}>
          <option value="">все статусы</option>
          {TOPUP_STATUSES.map((status) => (
            <option key={status} value={status}>
              {status}
            </option>
          ))}
        </select>
        <button className="btn" type="submit">
          Найти
        </button>
        <button className="btn ghost" type="button" onClick={onCsv} title="Выгрузить текущую страницу обоих списков в CSV">
          CSV
        </button>
      </form>

      <SectionHeader title="Подтверждённые платежи" sub={String(paymentsTotal)} />
      <TableCard>
        <table>
          {PAYMENT_HEAD}
          <tbody>
            {payments.length ? payments.map((item) => <PaymentRow key={item.provider_payment_id ?? item.user_id} item={item} />) : <EmptyRow columns={7} />}
          </tbody>
        </table>
      </TableCard>

      <SectionHeader title="Незавершённые и проблемные checkout" sub={String(checkoutsTotal)} />
      <TableCard>
        <table>
          {CHECKOUT_HEAD}
          <tbody>
            {checkouts.length ? checkouts.map((item, index) => <CheckoutRow key={item.provider_payment_id ?? index} item={item} />) : <EmptyRow columns={7} />}
          </tbody>
        </table>
      </TableCard>

      <Pager offset={query.offset} limit={query.limit} total={total} onOffset={(offset) => updateQuery((current) => ({ ...current, offset }))} />

      <footer>
        Один «Назад/Дальше» листает оба списка сразу (общий offset); status-фильтр применяется к платежам и чекаутам
        одновременно. CSV выгружает текущую страницу обоих списков с колонкой kind. Worker зачисляет баланс только после
        верифицированного платежа.
      </footer>
    </>
  );
}
