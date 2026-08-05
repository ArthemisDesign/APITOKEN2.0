"use client";

import { useCallback, useMemo, useRef, useState } from "react";
import { Banner, EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, StatCard, TableCard } from "@/components/ui";
import { api, send } from "@/lib/api";
import { dialog } from "@/lib/dialog";
import { nanoMoney } from "@/lib/format";
import { toast } from "@/lib/toast";
import { usePoll } from "@/lib/usePoll";
import {
  createProxyRenewRequest,
  filterProxyInventory,
  projectProxyInventory,
  projectProxyRenew,
  proxyRenewSummary,
  selectableProxyIds,
  type ProxyFilters,
  type ProxyInventoryItem,
  type ProxyRenewResponse,
} from "./lib";

const INVENTORY_PATH = "/proxy-admin/inventory";
const RENEW_PATH = "/proxy-admin/renew";
const EMPTY_FILTERS: ProxyFilters = { query: "", provider: "", plan: "", liveness: "", binding: "" };

const LIVENESS_LABELS: Record<string, string> = {
  live: "live",
  degraded: "degraded",
  dead: "dead",
  unknown: "неизвестно",
};
const BINDING_LABELS: Record<string, string> = {
  bound: "связано",
  unbound: "не связано",
  mismatch: "расхождение",
  unknown: "неизвестно",
};

function expiryCell(value: number | null): string {
  if (!value) return "—";
  return new Date(value * 1000).toLocaleDateString("ru-RU", {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
    timeZone: "UTC",
  });
}

function stateTone(value: string): "ok" | "warn" | "bad" {
  return value === "live" || value === "bound" ? "ok" : value === "dead" || value === "mismatch" ? "bad" : "warn";
}

function renewalResultTone(result: ProxyRenewResponse): "ok" | "warn" | "bad" {
  return result.status === "succeeded" ? "ok" : result.status === "failed" ? "bad" : "warn";
}

function renewResultIds(result: ProxyRenewResponse, status: "failed" | "uncertain"): Set<string> {
  return new Set(result.results.filter((item) => item.status === status).map((item) => item.inventory_id));
}

export default function ProxiesPage() {
  const [filters, setFilters] = useState<ProxyFilters>(EMPTY_FILTERS);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [lastResult, setLastResult] = useState<ProxyRenewResponse | null>(null);
  const [uncertainRequest, setUncertainRequest] = useState<ReturnType<typeof createProxyRenewRequest> | null>(null);
  const submitLock = useRef(false);

  const loadInventory = useCallback(async () => projectProxyInventory(await api<unknown>(INVENTORY_PATH)), []);
  const { data: inventory, refresh } = usePoll(INVENTORY_PATH, loadInventory);
  const items = useMemo(() => inventory?.items ?? [], [inventory]);
  const filtered = useMemo(() => filterProxyInventory(items, filters), [items, filters]);
  const visibleSelectable = useMemo(() => selectableProxyIds(filtered), [filtered]);
  const allVisibleSelected = visibleSelectable.length > 0 && visibleSelectable.every((id) => selected.has(id));
  const providers = useMemo(() => [...new Set(items.map((item) => item.provider))].sort(), [items]);
  const plans = useMemo(() => [...new Set(items.map((item) => item.subscription_plan))].sort(), [items]);

  const toggle = (id: string) => {
    if (busy) return;
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  const toggleVisible = () => {
    if (busy) return;
    setSelected((current) => {
      const next = new Set(current);
      if (allVisibleSelected) visibleSelectable.forEach((id) => next.delete(id));
      else visibleSelectable.forEach((id) => next.add(id));
      return next;
    });
  };

  const renew = useCallback(async (
    ids: string[],
    retryRequest?: ReturnType<typeof createProxyRenewRequest>,
  ) => {
    if (!ids.length || submitLock.current) return;
    submitLock.current = true;
    try {
      const request = retryRequest ?? createProxyRenewRequest(ids, crypto.randomUUID());
      const values = await dialog({
        title: retryRequest ? "Проверить неопределённое продление" : ids.length === 1 ? "Продлить прокси" : `Продлить прокси: ${ids.length}`,
        message: retryRequest
          ? "Будет повторён тот же запрос с тем же idempotency UUID. Новый расход создаваться не должен."
          : "Действие расходует баланс провайдера. Будет продлён тот же proxy order; отменить операцию после отправки нельзя.",
        confirmLabel: retryRequest ? "Повторить безопасно" : "Продлить",
        danger: true,
      });
      if (!values) return;

      setBusy(true);
      setLastResult(null);
      try {
        const response = projectProxyRenew(await send<unknown>(RENEW_PATH, "POST", request));
        setLastResult(response);
        const failed = renewResultIds(response, "failed");
        const uncertain = renewResultIds(response, "uncertain");
        setSelected(new Set([...failed, ...uncertain]));
        setUncertainRequest(uncertain.size || response.status === "uncertain" ? request : null);
        toast(proxyRenewSummary(response), response.status === "succeeded" ? "ok" : "bad");
      } catch (cause) {
        const message = cause instanceof Error ? cause.message : String(cause);
        setUncertainRequest(request);
        setLastResult({
          schema_version: 1,
          idempotency_key: request.idempotency_key,
          idempotent_replay: false,
          status: "uncertain",
          observed_at: null,
          results: request.inventory_ids.map((inventory_id) => ({
            inventory_id,
            status: "uncertain",
            proxy_expires_at: null,
            result_code: "transport_uncertain",
          })),
        });
        toast(`${message}. Результат неопределён; реестр обновляется. Повторяйте только с сохранённым UUID.`, "bad");
      } finally {
        refresh();
        setBusy(false);
      }
    } finally {
      submitLock.current = false;
    }
  }, [refresh]);

  if (inventory === undefined) {
    return (
      <>
        <PageHead title="Прокси" sub="реестр и сроки загружаются" />
        <LoadingGrid />
      </>
    );
  }

  const autoExtendProviders = inventory.providers.filter((provider) => provider.auto_extend_enabled);
  const renewableCount = items.filter((item) => item.renewable).length;

  return (
    <>
      <PageHead
        title="Прокси"
        sub="bounded реестр без credentials и полной identity"
        badge={<Pill kind={autoExtendProviders.length ? "warn" : "ok"}>{items.length} прокси</Pill>}
      />

      {autoExtendProviders.length ? (
        <Banner kind="warn" title="У провайдера включено auto-extend">
          {autoExtendProviders.map((provider) => provider.provider).join(", ")}. Перед ручным продлением исключите двойное списание.
        </Banner>
      ) : null}

      {lastResult ? (
        <Banner kind={renewalResultTone(lastResult)} title={lastResult.status === "succeeded" ? "Продление завершено" : "Результат требует проверки"}>
          {proxyRenewSummary(lastResult)} Реестр запрошен повторно.{lastResult.idempotent_replay ? " Ответ — idempotent replay." : ""}
        </Banner>
      ) : null}

      {uncertainRequest ? (
        <div className="toolbar">
          <button
            type="button"
            className="btn warn"
            disabled={busy}
            onClick={() => void renew(uncertainRequest.inventory_ids, uncertainRequest)}
          >
            Повторить с тем же UUID
          </button>
          <span className="note mono">{uncertainRequest.idempotency_key}</span>
        </div>
      ) : null}

      <div className="proxy-stats">
        {inventory.providers.map((provider) => (
          <StatCard
            key={provider.provider}
            label={`${provider.provider} · баланс`}
            value={provider.balance_nano_usd == null ? "—" : nanoMoney(provider.balance_nano_usd)}
            hint={provider.auto_extend_enabled ? "auto-extend включён" : "auto-extend выключен"}
          />
        ))}
        <StatCard label="доступно для продления" value={renewableCount} hint={`выбрано ${selected.size}`} />
      </div>

      <SectionHeader title="Реестр" sub={`${filtered.length} из ${items.length}`} />
      <div className="toolbar proxy-toolbar">
        <input
          aria-label="Поиск прокси"
          placeholder="proxy, order, provider, plan"
          value={filters.query}
          onChange={(event) => setFilters((current) => ({ ...current, query: event.target.value }))}
        />
        <select aria-label="Провайдер" value={filters.provider} onChange={(event) => setFilters((current) => ({ ...current, provider: event.target.value }))}>
          <option value="">все провайдеры</option>
          {providers.map((provider) => <option key={provider} value={provider}>{provider}</option>)}
        </select>
        <select aria-label="План" value={filters.plan} onChange={(event) => setFilters((current) => ({ ...current, plan: event.target.value }))}>
          <option value="">все планы</option>
          {plans.map((plan) => <option key={plan} value={plan}>{plan}</option>)}
        </select>
        <select aria-label="Liveness" value={filters.liveness} onChange={(event) => setFilters((current) => ({ ...current, liveness: event.target.value }))}>
          <option value="">любой liveness</option>
          <option value="live">live</option><option value="degraded">degraded</option><option value="dead">dead</option><option value="unknown">unknown</option>
        </select>
        <select aria-label="Binding" value={filters.binding} onChange={(event) => setFilters((current) => ({ ...current, binding: event.target.value }))}>
          <option value="">любой binding</option>
          <option value="bound">bound</option><option value="unbound">unbound</option><option value="mismatch">mismatch</option><option value="unknown">unknown</option>
        </select>
        <button type="button" className="btn ghost" onClick={() => setFilters(EMPTY_FILTERS)} disabled={busy}>Сбросить</button>
        <button type="button" className="btn warn" onClick={() => void renew([...selected])} disabled={busy || selected.size === 0}>
          {busy ? "Продлеваем…" : `Продлить выбранные (${selected.size})`}
        </button>
      </div>

      <TableCard>
        <table className="proxy-inventory-table">
          <thead><tr>
            <th aria-label="Выбрать видимые"><input className="proxy-select" type="checkbox" checked={allVisibleSelected} onChange={toggleVisible} disabled={busy || !visibleSelectable.length} /></th>
            <th className="left">Прокси</th><th className="left">Order</th><th className="left">Provider</th><th className="left">План подписки</th>
            <th>Liveness</th><th>Окончание подписки</th><th>Окончание прокси</th><th>Binding</th><th aria-label="Действия" />
          </tr></thead>
          <tbody>
            {filtered.length ? filtered.map((item: ProxyInventoryItem) => (
              <tr key={item.inventory_id} className={selected.has(item.inventory_id) ? "selected" : ""}>
                <td><input className="proxy-select" type="checkbox" checked={selected.has(item.inventory_id)} onChange={() => toggle(item.inventory_id)} disabled={busy || !item.renewable} aria-label={`Выбрать ${item.proxy_hint}`} /></td>
                <td className="left"><b>{item.proxy_hint}</b></td><td className="left mono">{item.order_hint}</td><td className="left">{item.provider}</td><td className="left">{item.subscription_plan}</td>
                <td><Pill kind={stateTone(item.liveness)}>{LIVENESS_LABELS[item.liveness]}</Pill></td>
                <td>{expiryCell(item.subscription_expires_at)}</td><td>{expiryCell(item.proxy_expires_at)}</td>
                <td><Pill kind={stateTone(item.binding_status)}>{BINDING_LABELS[item.binding_status]}</Pill></td>
                <td><button type="button" className="btn ghost" onClick={() => void renew([item.inventory_id])} disabled={busy || !item.renewable} title={item.renew_block_code ?? undefined}>Продлить</button></td>
              </tr>
            )) : <EmptyRow columns={10} text="прокси по фильтрам не найдены" />}
          </tbody>
        </table>
      </TableCard>
      <footer>В браузер поступают только bounded hints и opaque inventory IDs. Credentials, полный proxy URL, IP и полная identity не запрашиваются и не отображаются.</footer>
    </>
  );
}
