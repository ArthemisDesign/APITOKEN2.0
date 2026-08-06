"use client";

import { type ReactElement } from "react";
import { Pill, TableCard } from "@/components/ui";
import { duration, nanoMoney } from "@/lib/format";
import { providerInteger } from "./provider-calibration";
import { ProviderQuotaMeter, ProviderSection } from "./provider-board-ui";
import { SubscriptionExpiry } from "./subscription-lifecycle";
import type { CapacityResponse, CapacitySub, ClaudeSubWindow } from "./types";

function moneyOrDash(value: string | null | undefined): string {
  return providerInteger(value) == null ? "—" : nanoMoney(value);
}

function quotaValue(util: number | null | undefined): { value: number | null; label: string } {
  if (util == null) return { value: null, label: "—" };
  const value = Number(util);
  if (!Number.isFinite(value)) return { value: null, label: "—" };
  const percent = Math.min(100, Math.max(0, value * 100));
  const rounded = Math.round(percent * 10) / 10;
  return { value: rounded, label: `${rounded.toLocaleString("ru-RU", { maximumFractionDigits: 1 })}%` };
}

type WindowState = "ready" | "last_known" | "updating" | "inactive";

interface ClaudeWindowView {
  state: WindowState;
  quota: { value: number | null; label: string };
  reset: string;
  remaining: string | null | undefined;
  capacity: string | null | undefined;
  /** Почему денег нет — конкретной причиной, а не одним словом на четыре разных случая. */
  moneyHint: string;
}

function windowFor(item: CapacitySub, kind: "5h" | "7d"): ClaudeSubWindow | undefined {
  const minutes = kind === "5h" ? 300 : 10_080;
  return item.windows?.find((window) => window.window_kind === kind || window.window_minutes === minutes);
}

// Денежная ячейка обязана называть ПРИЧИНУ отсутствия долларов: сбой доставки точных улик и
// отсутствие свежего probe — разные аварии с разными действиями оператора, и склеивать их в одно
// «ждём свежую квоту» значит прятать диагностику, которую бэкенд уже опубликовал в `missing_reason`.
function moneyHintFor(reason: string | null | undefined): string {
  switch (reason) {
    case "calibration_delivery_pending":
    case "calibration_delivery_degraded":
    case "calibration_delivery_unavailable":
      return "ждём доставку";
    case "calibration_authority_unavailable":
      return "нет авторитета";
    case "awaiting_plan_evidence":
      return "ждём данные плана";
    case "stale_current_quota_snapshot":
    case "missing_current_quota_snapshot":
      return "ждём probe";
    default:
      return "ждём свежую квоту";
  }
}

function claudeWindowView(item: CapacitySub, kind: "5h" | "7d", now: number): ClaudeWindowView {
  const window = windowFor(item, kind);
  const remaining = window?.remaining_nano ?? (kind === "5h" ? item.rem5h_nano : item.rem7d_nano);
  const capacity = window?.capacity_nano ?? (kind === "5h" ? item.cap5h_nano : item.cap7d_nano);
  const moneyHint = moneyHintFor(window?.missing_reason);
  const cooling = item.cooling === true;
  const unavailableOutsideQuota = item.auth_state === "dead" || (item.routable === false && !cooling);
  if (unavailableOutsideQuota) {
    return {
      state: "inactive",
      quota: { value: null, label: "—" },
      reset: "—",
      remaining,
      capacity,
      moneyHint,
    };
  }

  // Провайдерская квота и продаваемые доллары имеют РАЗНЫЕ авторитеты: сбой денежных улик не
  // должен ослеплять оператора относительно реальной стены лимита. Поэтому процент/reset берём из
  // точного снапшота всегда, когда бэкенд его опубликовал, а деньги гасим отдельно.
  // Payload без `windows` сохраняет прежний live-quota контракт.
  const usedFraction = window
    ? window.used_fraction_units == null
      ? null
      : window.used_fraction_units / 100_000_000
    : kind === "5h"
      ? item.util5h
      : item.util7d;
  const quota = quotaValue(usedFraction);
  const resetIn =
    kind === "5h"
      ? item.reset5h_in
      : item.reset7d_in;
  const resetFromWindow =
    window?.resets_at == null ? null : Math.max(0, window.resets_at - now);
  const resetSeconds = resetIn ?? resetFromWindow;
  const resetLabel = resetSeconds == null ? "уточняется" : duration(resetSeconds);
  const snapshotFresh = window ? window.snapshot_fresh === true : true;

  if (!snapshotFresh) {
    // Окно за своим reset пусто по построению: провайдер уже налил его заново. Бэкенд публикует
    // точный 0%, и прятать измеренный ноль за «обновляем» — ровно та ошибка, из-за которой
    // простаивающая исправная подписка выглядела как подписка без данных.
    if (window?.quota_state === "window_rolled_over") {
      return {
        state: item.routable === false ? "inactive" : "updating",
        quota,
        reset: resetLabel,
        remaining: null,
        capacity,
        moneyHint,
      };
    }
    const retainedBeforeReset = quota.value != null && resetSeconds != null && resetSeconds > 0;
    if (retainedBeforeReset) {
      const lastKnownMoney = window?.last_known_remaining_nano;
      return {
        state:
          item.routable === false
            ? "inactive"
            : providerInteger(lastKnownMoney) == null
              ? "updating"
              : "last_known",
        quota,
        reset: duration(resetSeconds),
        remaining: lastKnownMoney,
        capacity,
        moneyHint,
      };
    }
    return {
      state: item.routable === false ? "inactive" : "updating",
      quota: quota.value == null ? { value: null, label: "обновляем" } : quota,
      reset: resetLabel,
      remaining,
      capacity,
      moneyHint,
    };
  }

  const moneyReady = providerInteger(remaining) != null && providerInteger(capacity) != null;
  return {
    state: item.routable === false ? "inactive" : moneyReady ? "ready" : "updating",
    quota,
    reset: resetLabel,
    remaining,
    capacity,
    moneyHint,
  };
}

function ClaudeMoney({ view, fiveHour = false }: { view: ClaudeWindowView; fiveHour?: boolean }) {
  const cellClass = [
    view.state === "ready" ? "provider-usd-ink" : "provider-capacity-state",
    fiveHour ? "provider-five-hour-money" : "",
  ].filter(Boolean).join(" ");
  if (view.state === "inactive") {
    return <td className={cellClass}><b>вне ротации</b><small>не входит в ёмкость</small></td>;
  }
  if (view.state === "last_known") {
    return (
      <td className={cellClass}>
        <b>{moneyOrDash(view.remaining)}</b>
        <small>{`последнее · из ${moneyOrDash(view.capacity)}`}</small>
      </td>
    );
  }
  if (view.state === "updating") {
    return <td className={cellClass}><b>обновляем</b><small>{view.moneyHint}</small></td>;
  }
  return (
    <td className={cellClass}>
      <b>{moneyOrDash(view.remaining)}</b>
      <small>{`из ${moneyOrDash(view.capacity)}`}</small>
    </td>
  );
}

function subscriptionStatus(
  item: CapacitySub,
  five: ClaudeWindowView,
  weekly: ClaudeWindowView,
): { label: string; kind: "ok" | "warn" | "bad" } {
  if (item.auth_state === "dead") {
    return {
      label: item.dead_reason === "permission_error" ? "токен мёртв · бан" : "токен мёртв",
      kind: "bad",
    };
  }
  if (item.auth_state === "suspect") return { label: "auth под наблюдением", kind: "warn" };
  if (item.cooling) {
    const exhausted = [
      five.quota.value === 100 ? "5ч" : null,
      weekly.quota.value === 100 ? "7д" : null,
    ].filter((window): window is string => window != null);
    if (exhausted.length === 2) return { label: "лимиты 5ч и 7д исчерпаны", kind: "warn" };
    if (exhausted.length === 1) return { label: `лимит ${exhausted[0]} исчерпан`, kind: "warn" };
    return { label: "временно вне ротации", kind: "warn" };
  }
  if (item.routable === false) return { label: "вне ротации", kind: "warn" };
  if (item.calibrated === false) return { label: "ждём данные", kind: "warn" };
  return { label: "active", kind: "ok" };
}

function ClaudeSubscriptions({ items, now }: { items: CapacitySub[]; now: number }) {
  return (
    <ProviderSection overline="Подписки" title="Окна по аккаунтам" meta={`${items.filter((item) => item.routable).length}/${items.length} в ротации`}>
      <TableCard>
        <table className="provider-home-capacity-table">
          <thead>
            <tr>
              <th className="left">Почта</th>
              <th className="left">Состояние</th>
              <th>Окончание</th>
              <th>Quota 5ч / reset</th>
              <th className="provider-five-hour-money">Доступно $ · 5ч</th>
              <th>Quota 7д / reset</th>
              <th>Доступно $ · 7д</th>
            </tr>
          </thead>
          <tbody>
            {items.map((item, index) => {
              const five = claudeWindowView(item, "5h", now);
              const weekly = claudeWindowView(item, "7d", now);
              const health = subscriptionStatus(item, five, weekly);
              return (
                <tr key={`${item.email ?? "claude"}-${index}`}>
                  <td className="left"><b>{item.email || "—"}</b><small>{item.plan ?? "—"}</small></td>
                  <td className="left"><Pill kind={health.kind}>{health.label}</Pill></td>
                  <SubscriptionExpiry lifecycle={item} nowSeconds={now} />
                  <td><ProviderQuotaMeter usedPercent={five.quota.value} label={five.quota.label} reset={five.reset} /></td>
                  <ClaudeMoney view={five} fiveHour />
                  <td><ProviderQuotaMeter usedPercent={weekly.quota.value} label={weekly.quota.label} reset={weekly.reset} /></td>
                  <ClaudeMoney view={weekly} />
                </tr>
              );
            })}
          </tbody>
        </table>
      </TableCard>
    </ProviderSection>
  );
}

export function ClaudeCapacityBoard({ response }: { response: CapacityResponse }): ReactElement {
  return (
    <div className="provider-capacity-board claude-capacity-board">
      <ClaudeSubscriptions items={response.per_sub ?? []} now={response.now ?? 0} />
    </div>
  );
}
