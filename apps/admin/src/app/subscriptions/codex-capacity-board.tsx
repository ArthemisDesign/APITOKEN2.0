"use client";

import { type ReactElement } from "react";
import { Pill, TableCard } from "@/components/ui";
import { duration, nanoMoney, windowLabel } from "@/lib/format";
import {
  CODEX_TOKEN_KINDS,
  codexApiValueForCredits,
  codexTokenEconomics,
  compareCodexEfficiency,
  type CodexContextTier,
  type CodexServiceTier,
  type CodexTokenEconomics,
  type CodexTokenKind,
} from "./codex-calibration";
import { barFromPercent, homeStatus } from "./logic";
import type {
  CodexConversionModel,
  CodexHome,
  CodexHomeWindow,
  CodexPlanCohort,
  CodexSubsResponse,
} from "./types";

const FRACTION_SCALE = 100_000_000n;

interface CapacityWindow {
  plan: string;
  windowMinutes: number;
  homesTotal: number;
  measuredHomes: number;
  capacityPerHome: string | null;
  fleetCapacity: string | null;
  fleetRemaining: string | null;
}

interface BestCodexEconomics {
  model: CodexConversionModel;
  tier: CodexServiceTier;
  context: CodexContextTier;
  kind: CodexTokenKind;
  economics: CodexTokenEconomics;
}

const asBigInt = (value: string | number | bigint | null | undefined): bigint | null => {
  if (value == null) return null;
  try {
    return BigInt(value);
  } catch {
    return null;
  }
};

function compactCredits(value: string | bigint | null | undefined): string {
  const nano = asBigInt(value);
  if (nano == null) return "—";
  const tenths = (nano * 10n + 500_000_000n) / 1_000_000_000n;
  const whole = tenths / 10n;
  const fraction = tenths % 10n;
  return `${whole.toLocaleString("en-US")}${fraction ? `.${fraction}` : ""} credits`;
}

function percentOfCapacity(capacity: string | null, remaining: string | null): string {
  const cap = asBigInt(capacity);
  const rem = asBigInt(remaining);
  if (cap == null || rem == null || cap <= 0n) return "—";
  const used = cap > rem ? cap - rem : 0n;
  const tenths = (used * 1_000n + cap / 2n) / cap;
  return `${tenths / 10n}.${tenths % 10n}%`;
}

function quotaPercent(window: CodexHomeWindow | undefined): string {
  const units = asBigInt(window?.used_fraction_units ?? null);
  if (units == null) return window?.used_percent == null ? "—" : `${window.used_percent}%`;
  const tenths = (units + 50_000n) / 100_000n;
  return `${tenths / 10n}${tenths % 10n ? `.${tenths % 10n}` : ""}%`;
}

function quotaUsedPercent(window: CodexHomeWindow | undefined): number | null {
  const units = asBigInt(window?.used_fraction_units ?? null);
  if (units != null) {
    const bounded = units < 0n ? 0n : units > FRACTION_SCALE ? FRACTION_SCALE : units;
    return Number(bounded) / 1_000_000;
  }
  const fallback = Number(window?.used_percent);
  return Number.isFinite(fallback) ? Math.min(100, Math.max(0, fallback)) : null;
}

function QuotaBar({ window, reset }: { window: CodexHomeWindow | undefined; reset: string }): ReactElement {
  const exact = quotaUsedPercent(window);
  const bar = barFromPercent(exact);
  return (
    <div className="codex-quota-meter" title={exact == null ? undefined : `Использовано ${quotaPercent(window)}`}>
      <div>
        <span className="bar" aria-hidden="true">
          <i className={bar.kind} style={{ width: `${bar.percent}%` }} />
        </span>
        <b>{quotaPercent(window)}</b>
      </div>
      <small>сброс {reset}</small>
    </div>
  );
}

function sharedRemaining(capacityPerHome: string | null, window: CodexHomeWindow | undefined): bigint | null {
  const capacity = asBigInt(capacityPerHome);
  const used = asBigInt(window?.used_fraction_units ?? null);
  if (capacity == null) return asBigInt(window?.remaining_nanocredits);
  if (used == null) return asBigInt(window?.remaining_nanocredits) ?? capacity;
  const boundedUsed = used < 0n ? 0n : used > FRACTION_SCALE ? FRACTION_SCALE : used;
  return (capacity * (FRACTION_SCALE - boundedUsed) + FRACTION_SCALE / 2n) / FRACTION_SCALE;
}

function capacityWindows(response: CodexSubsResponse): CapacityWindow[] {
  const cohorts = (response.plan_cohorts ?? [])
    .filter((item) => Number(item.window_minutes ?? 0) > 0)
    .map((item: CodexPlanCohort): CapacityWindow => ({
      plan: item.plan ?? "unknown",
      windowMinutes: Number(item.window_minutes),
      homesTotal: Number(item.homes_total ?? 0),
      measuredHomes: Number(item.measured_homes ?? 0),
      capacityPerHome: item.capacity_per_home_nanocredits ?? null,
      fleetCapacity: item.fleet_capacity_nanocredits ?? null,
      fleetRemaining: item.fleet_remaining_nanocredits ?? null,
    }));
  if (cohorts.length) {
    return cohorts.sort((a, b) => b.windowMinutes - a.windowMinutes || b.homesTotal - a.homesTotal);
  }
  return (response.window_totals ?? [])
    .filter((item) => Number(item.window_minutes ?? 0) > 0)
    .map((item): CapacityWindow => ({
      plan: "весь пул",
      windowMinutes: Number(item.window_minutes),
      homesTotal: Number(item.credit_observed_homes ?? response.homes?.length ?? 0),
      measuredHomes: Number(item.credit_measured_homes ?? item.measured_homes ?? 0),
      capacityPerHome: null,
      fleetCapacity: item.capacity_nanocredits ?? null,
      fleetRemaining: item.remaining_nanocredits ?? null,
    }))
    .sort((a, b) => b.windowMinutes - a.windowMinutes);
}

function sumCapacity(values: Array<string | null>): string | null {
  let total = 0n;
  for (const value of values) {
    const parsed = asBigInt(value);
    if (parsed == null) return null;
    total += parsed;
  }
  return total.toString();
}

function aggregateCapacityWindows(windows: CapacityWindow[]): CapacityWindow | undefined {
  if (!windows.length) return undefined;
  return {
    plan: windows.length === 1 ? windows[0].plan : "весь пул",
    windowMinutes: windows[0].windowMinutes,
    homesTotal: windows.reduce((sum, item) => sum + item.homesTotal, 0),
    measuredHomes: windows.reduce((sum, item) => sum + item.measuredHomes, 0),
    capacityPerHome: windows.length === 1 ? windows[0].capacityPerHome : null,
    fleetCapacity: sumCapacity(windows.map((item) => item.fleetCapacity)),
    fleetRemaining: sumCapacity(windows.map((item) => item.fleetRemaining)),
  };
}

function bestCodexEconomics(models: CodexConversionModel[]): BestCodexEconomics | null {
  const candidates: BestCodexEconomics[] = [];
  for (const model of models) {
    for (const tier of ["standard", "fast"] as const) {
      for (const context of ["long", "short"] as const) {
        for (const kind of CODEX_TOKEN_KINDS) {
          const economics = codexTokenEconomics(model, tier, context, kind);
          if (economics) candidates.push({ model, tier, context, kind, economics });
        }
      }
    }
  }
  candidates.sort((a, b) => {
    const efficiency = compareCodexEfficiency(b.economics, a.economics);
    if (efficiency) return efficiency;
    const model = a.model.id.localeCompare(b.model.id);
    if (model) return model;
    if (a.tier !== b.tier) return a.tier === "standard" ? -1 : 1;
    if (a.context !== b.context) return a.context === "long" ? -1 : 1;
    return CODEX_TOKEN_KINDS.indexOf(a.kind) - CODEX_TOKEN_KINDS.indexOf(b.kind);
  });
  return candidates[0] ?? null;
}

function calibrationHealth(home: CodexHome, nowSec: number): { label: string; kind: "ok" | "warn" | "bad" } {
  if (Number(home.calibration_dropped_events ?? 0) > 0 || home.calibration_persistence_ok === false) {
    return { label: "ошибка данных", kind: "bad" };
  }
  if (Number(home.calibration_pending_events ?? 0) > 0) return { label: "сохраняется", kind: "warn" };
  const runtime = homeStatus(home, nowSec);
  if (runtime.kind !== "ok" && runtime.kind !== "") return { label: runtime.label, kind: runtime.kind };
  return { label: "active", kind: "ok" };
}

function CapacityStrip({
  window,
  standardValue,
  standardScenario,
  maxValue,
  maxScenario,
}: {
  window: CapacityWindow | undefined;
  standardValue: bigint | null;
  standardScenario: string;
  maxValue: bigint | null;
  maxScenario: string;
}): ReactElement {
  return (
    <section className="codex-capacity-strip" aria-label="Ёмкость GPT-пула">
      <div className="codex-capacity-primary">
        <span>{window ? windowLabel(window.windowMinutes) : "окно"} · доступно</span>
        <strong>{window?.fleetRemaining == null ? "ждём Δquota" : compactCredits(window.fleetRemaining)}</strong>
        <small>{window?.plan ?? "capacity ещё не измерена"}</small>
      </div>
      <div>
        <span>Полная ёмкость</span>
        <strong>{window?.fleetCapacity == null ? "—" : compactCredits(window.fleetCapacity)}</strong>
        <small>{window ? `${window.measuredHomes}/${window.homesTotal} измерено` : "—"}</small>
      </div>
      <div>
        <span>Использовано</span>
        <strong>{window ? percentOfCapacity(window.fleetCapacity, window.fleetRemaining) : "—"}</strong>
        <small>по shared plan capacity</small>
      </div>
      <div>
        <span>API · Standard fresh</span>
        <strong className="usd-ink">{standardValue == null ? "—" : nanoMoney(standardValue)}</strong>
        <small>{standardScenario}</small>
      </div>
      <div>
        <span>API · максимум тарифа</span>
        <strong className="usd-ink">{maxValue == null ? "—" : nanoMoney(maxValue)}</strong>
        <small>{maxScenario}</small>
      </div>
    </section>
  );
}

function HomeCapacityTable({
  homes,
  window,
  cohortWindows,
  nowMs,
  standard,
  maximum,
}: {
  homes: CodexHome[];
  window: CapacityWindow | undefined;
  cohortWindows: CapacityWindow[];
  nowMs: number;
  standard: CodexTokenEconomics | null;
  maximum: CodexTokenEconomics | null;
}): ReactElement {
  const nowSec = Math.floor(nowMs / 1000);
  return (
    <section className="codex-compact-section">
      <header>
        <div>
          <span className="codex-overline">Подписки</span>
          <h3>Доступная ёмкость по home</h3>
        </div>
        <b>{homes.length} homes</b>
      </header>
      <TableCard>
        <table className="codex-home-capacity-table">
          <thead>
            <tr>
              <th className="left">Почта</th>
              <th className="left">Состояние</th>
              <th>Quota / reset</th>
              <th>Доступно credits</th>
              <th>API Standard</th>
              <th>API максимум</th>
            </tr>
          </thead>
          <tbody>
            {homes.map((home, index) => {
              const matching = (home.windows ?? []).find((item) =>
                Number(item.window_minutes ?? 0) === Number(window?.windowMinutes ?? 0) && Number(item.window_minutes ?? 0) > 0,
              );
              const cohort =
                cohortWindows.find((item) => item.plan === home.plan) ??
                (cohortWindows.length === 1 ? cohortWindows[0] : undefined);
              const remaining = sharedRemaining(cohort?.capacityPerHome ?? null, matching);
              const health = calibrationHealth(home, nowSec);
              const reset = matching?.resets_at ? duration(Math.max(0, matching.resets_at - nowSec)) : "—";
              return (
                <tr key={home.id ?? index}>
                  <td className="left"><b>{home.email?.trim() || "—"}</b><small>{home.plan ?? "—"}</small></td>
                  <td className="left"><Pill kind={health.kind}>{health.label}</Pill></td>
                  <td><QuotaBar window={matching} reset={reset} /></td>
                  <td className="credit-ink"><b>{remaining == null ? "—" : compactCredits(remaining)}</b></td>
                  <td className="usd-ink"><b>{nanoMoney(codexApiValueForCredits(remaining, standard))}</b></td>
                  <td className="usd-ink"><b>{nanoMoney(codexApiValueForCredits(remaining, maximum))}</b></td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </TableCard>
    </section>
  );
}

export function CodexCapacityBoard({
  response,
  nowMs,
  showSummary = true,
}: {
  response: CodexSubsResponse;
  nowMs: number;
  showSummary?: boolean;
}): ReactElement {
  const models = response.conversion_models ?? [];
  const windows = capacityWindows(response);
  const targetWindowMinutes = windows.some((item) => item.windowMinutes === 10_080)
    ? 10_080
    : windows[0]?.windowMinutes;
  const cohortWindows = windows.filter((item) => item.windowMinutes === targetWindowMinutes);
  const weekly = aggregateCapacityWindows(cohortWindows);
  const standardModel = models.find((model) => model.id === "gpt-5.6-sol") ?? models[0];
  const standard = standardModel ? codexTokenEconomics(standardModel, "standard", "short", "fresh") : null;
  const maximumResult = bestCodexEconomics(models);
  const maximum = maximumResult?.economics ?? null;
  const standardValue = codexApiValueForCredits(weekly?.fleetRemaining, standard);
  const maxValue = codexApiValueForCredits(weekly?.fleetRemaining, maximum);
  const standardScenario = standardModel ? `${standardModel.id} · short` : "нет тарифа";
  const maxScenario = maximumResult
    ? `${maximumResult.model.id} · ${maximumResult.tier}/${maximumResult.context}/${maximumResult.kind}`
    : "нет тарифа";

  return (
    <div className="codex-capacity-board">
      {showSummary ? (
        <CapacityStrip
          window={weekly}
          standardValue={standardValue}
          standardScenario={standardScenario}
          maxValue={maxValue}
          maxScenario={maxScenario}
        />
      ) : null}
      <HomeCapacityTable
        homes={response.homes ?? []}
        window={weekly}
        cohortWindows={cohortWindows}
        nowMs={nowMs}
        standard={standard}
        maximum={maximum}
      />
    </div>
  );
}
