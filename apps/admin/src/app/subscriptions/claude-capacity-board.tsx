"use client";

import { type ReactElement } from "react";
import { Pill, TableCard } from "@/components/ui";
import { duration, nanoMoney } from "@/lib/format";
import { providerInteger } from "./provider-calibration";
import { ProviderQuotaMeter, ProviderSection } from "./provider-board-ui";
import type { CapacityResponse, CapacitySub, ClaudeSubWindow } from "./types";

function moneyOrDash(value: string | null | undefined): string {
  return providerInteger(value) == null ? "—" : nanoMoney(value);
}

function quotaValue(util: number | null | undefined): { value: number | null; label: string } {
  const value = Number(util);
  if (!Number.isFinite(value)) return { value: null, label: "—" };
  const percent = Math.min(100, Math.max(0, value * 100));
  const rounded = Math.round(percent * 10) / 10;
  return { value: rounded, label: `${rounded.toLocaleString("ru-RU", { maximumFractionDigits: 1 })}%` };
}

type WindowState = "ready" | "updating" | "inactive";

interface ClaudeWindowView {
  state: WindowState;
  quota: { value: number | null; label: string };
  reset: string;
  remaining: string | null | undefined;
  capacity: string | null | undefined;
}

function windowFor(item: CapacitySub, kind: "5h" | "7d"): ClaudeSubWindow | undefined {
  const minutes = kind === "5h" ? 300 : 10_080;
  return item.windows?.find((window) => window.window_kind === kind || window.window_minutes === minutes);
}

function claudeWindowView(item: CapacitySub, kind: "5h" | "7d", now: number): ClaudeWindowView {
  const window = windowFor(item, kind);
  const remaining = window?.remaining_nano ?? (kind === "5h" ? item.rem5h_nano : item.rem7d_nano);
  const capacity = window?.capacity_nano ?? (kind === "5h" ? item.cap5h_nano : item.cap7d_nano);
  const inactive = item.routable === false || item.auth_state === "dead";
  if (inactive) {
    return {
      state: "inactive",
      quota: { value: null, label: "—" },
      reset: "—",
      remaining,
      capacity,
    };
  }

  // New payloads explicitly prove freshness. The remaining-money check keeps the compact board
  // honest when calibration delivery or plan evidence is unavailable. Payloads without `windows`
  // retain the old contract only when they already contain both exact money fields.
  const snapshotFresh = window ? window.snapshot_fresh === true : true;
  const moneyReady =
    snapshotFresh && providerInteger(remaining) != null && providerInteger(capacity) != null;
  if (!moneyReady) {
    return {
      state: "updating",
      quota: { value: null, label: "обновляем" },
      reset: "уточняется",
      remaining,
      capacity,
    };
  }

  const usedFraction =
    window?.used_fraction_units == null
      ? kind === "5h"
        ? item.util5h
        : item.util7d
      : window.used_fraction_units / 100_000_000;
  const resetIn =
    kind === "5h"
      ? item.reset5h_in
      : item.reset7d_in;
  const resetFromWindow =
    window?.resets_at == null ? null : Math.max(0, window.resets_at - now);
  const resetSeconds = resetIn ?? resetFromWindow;
  return {
    state: "ready",
    quota: quotaValue(usedFraction),
    reset: resetSeconds == null ? "уточняется" : duration(resetSeconds),
    remaining,
    capacity,
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
  if (view.state === "updating") {
    return <td className={cellClass}><b>обновляем</b><small>ждём свежую квоту</small></td>;
  }
  return (
    <td className={cellClass}>
      <b>{moneyOrDash(view.remaining)}</b>
      <small>{`из ${moneyOrDash(view.capacity)}`}</small>
    </td>
  );
}

function subscriptionStatus(item: CapacitySub): { label: string; kind: "ok" | "warn" | "bad" } {
  if (item.auth_state === "dead") {
    return {
      label: item.dead_reason === "permission_error" ? "токен мёртв · бан" : "токен мёртв",
      kind: "bad",
    };
  }
  if (item.auth_state === "suspect") return { label: "auth под наблюдением", kind: "warn" };
  if (item.cooling) return { label: "cooling", kind: "warn" };
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
              const health = subscriptionStatus(item);
              return (
                <tr key={`${item.email ?? "claude"}-${index}`}>
                  <td className="left"><b>{item.email || "—"}</b><small>{item.plan ?? "—"}</small></td>
                  <td className="left"><Pill kind={health.kind}>{health.label}</Pill></td>
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
