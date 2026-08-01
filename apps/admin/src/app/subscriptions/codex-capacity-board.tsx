"use client";

import { type ReactElement } from "react";
import { Pill, TableCard } from "@/components/ui";
import { duration, nanoMoney, windowLabel } from "@/lib/format";
import {
  CODEX_TOKEN_KINDS,
  codexApiValueForCredits,
  codexTokenEconomics,
  codexTokensForCapacity,
  compareCodexEfficiency,
  formatCodexTokenCount,
  formatCodexUsdPerCredit,
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
const TOKEN_LABELS: Record<CodexTokenKind, string> = {
  fresh: "Fresh input",
  cached: "Cache read",
  write: "Cache write",
  output: "Output / reasoning",
};

interface CapacityWindow {
  plan: string;
  windowMinutes: number;
  homesTotal: number;
  measuredHomes: number;
  capacityPerHome: string | null;
  fleetCapacity: string | null;
  fleetRemaining: string | null;
}

interface ProfitabilityRow {
  model: CodexConversionModel;
  tier: CodexServiceTier;
  context: CodexContextTier;
  rates: Record<CodexTokenKind, CodexTokenEconomics | null>;
  bestKind: CodexTokenKind;
  best: CodexTokenEconomics;
}

const asBigInt = (value: string | number | bigint | null | undefined): bigint | null => {
  if (value == null) return null;
  try {
    return BigInt(value);
  } catch {
    return null;
  }
};

function compactInteger(value: bigint | null): string {
  if (value == null) return "—";
  const units: Array<[bigint, string]> = [
    [1_000_000_000_000n, "T"],
    [1_000_000_000n, "B"],
    [1_000_000n, "M"],
    [1_000n, "K"],
  ];
  const unit = units.find(([size]) => value >= size);
  if (!unit) return value.toString();
  const [size, suffix] = unit;
  const tenths = (value * 10n + size / 2n) / size;
  const whole = tenths / 10n;
  const fraction = tenths % 10n;
  return `${whole}${fraction ? `.${fraction}` : ""}${suffix}`;
}

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

function profitabilityRows(models: CodexConversionModel[]): ProfitabilityRow[] {
  const rows: ProfitabilityRow[] = [];
  for (const model of models) {
    for (const tier of ["standard", "fast"] as const) {
      const candidates: ProfitabilityRow[] = [];
      for (const context of ["long", "short"] as const) {
        const rates = Object.fromEntries(
          CODEX_TOKEN_KINDS.map((kind) => [kind, codexTokenEconomics(model, tier, context, kind)]),
        ) as Record<CodexTokenKind, CodexTokenEconomics | null>;
        const available = CODEX_TOKEN_KINDS.filter((kind) => rates[kind] != null);
        if (!available.length) continue;
        const bestKind = available.reduce((best, kind) =>
          compareCodexEfficiency(rates[kind]!, rates[best]!) > 0 ? kind : best,
        );
        candidates.push({ model, tier, context, rates, bestKind, best: rates[bestKind]! });
      }
      const best = candidates.sort((a, b) => compareCodexEfficiency(b.best, a.best))[0];
      if (best) rows.push(best);
    }
  }
  return rows.sort((a, b) => {
    const efficiency = compareCodexEfficiency(b.best, a.best);
    if (efficiency) return efficiency;
    const model = a.model.id.localeCompare(b.model.id);
    if (model) return model;
    if (a.tier !== b.tier) return a.tier === "standard" ? -1 : 1;
    return a.context === "long" ? -1 : 1;
  });
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

function TokenCapacityTable({
  models,
  remaining,
}: {
  models: CodexConversionModel[];
  remaining: string | null | undefined;
}): ReactElement {
  return (
    <section className="codex-compact-section">
      <header>
        <div>
          <span className="codex-overline">Текущий остаток · весь пул</span>
          <h3>Сколько токенов доступно</h3>
        </div>
        <b className="credit-ink">если тратить только один вид</b>
      </header>
      <TableCard>
        <table className="codex-token-capacity-table">
          <thead>
            <tr>
              <th className="left">Модель</th>
              <th className="left">Режим</th>
              {CODEX_TOKEN_KINDS.map((kind) => <th key={kind}>{TOKEN_LABELS[kind]}</th>)}
            </tr>
          </thead>
          <tbody>
            {models.flatMap((model) =>
              (["standard", "fast"] as const).map((tier) => (
                <tr key={`${model.id}-${tier}`}>
                  <td className="left"><b>{model.id}</b></td>
                  <td className="left"><Pill kind={tier === "fast" ? "info" : "ok"}>{tier}</Pill></td>
                  {CODEX_TOKEN_KINDS.map((kind) => {
                    const tokens = codexTokensForCapacity(remaining, model, tier, kind);
                    return (
                      <td className={kind === "cached" ? "cache-cell" : ""} key={kind} title={formatCodexTokenCount(tokens)}>
                        <b>{compactInteger(tokens)}</b>
                      </td>
                    );
                  })}
                </tr>
              )),
            )}
          </tbody>
        </table>
      </TableCard>
    </section>
  );
}

function ProfitabilityTable({ rows, remaining }: { rows: ProfitabilityRow[]; remaining: string | null | undefined }): ReactElement {
  const globalBest = rows[0]?.best ?? null;
  return (
    <section className="codex-compact-section">
      <header>
        <div>
          <span className="codex-overline">Продажная эффективность</span>
          <h3>Выгодность по убыванию</h3>
        </div>
        <b className="usd-ink">$ API-equivalent / native credit ↓</b>
      </header>
      <TableCard>
        <table className="codex-profit-table">
          <thead>
            <tr>
              <th>#</th>
              <th className="left">Модель</th>
              <th className="left">Режим / контекст</th>
              <th className="left">Лучший токен</th>
              <th>$/credit</th>
              <th>$ остатка</th>
              {CODEX_TOKEN_KINDS.map((kind) => <th key={kind}>{kind === "write" ? "write" : kind}</th>)}
            </tr>
          </thead>
          <tbody>
            {rows.map((row, index) => {
              const isGlobalBest = globalBest != null && compareCodexEfficiency(row.best, globalBest) === 0;
              const windowValue = codexApiValueForCredits(remaining, row.best);
              return (
                <tr className={isGlobalBest ? "top-efficiency" : ""} key={`${row.model.id}-${row.tier}-${row.context}`}>
                  <td className="rank-cell">{index + 1}</td>
                  <td className="left"><b>{row.model.id}</b></td>
                  <td className="left"><span>{row.tier}</span><small>{row.context === "long" ? "long" : "short"}</small></td>
                  <td className="left"><b>{TOKEN_LABELS[row.bestKind]}</b></td>
                  <td className="usd-ink"><b>{formatCodexUsdPerCredit(row.best)}</b></td>
                  <td className="usd-ink"><b>{windowValue == null ? "—" : nanoMoney(windowValue)}</b></td>
                  {CODEX_TOKEN_KINDS.map((kind) => (
                    <td className={kind === row.bestKind ? "best-rate" : ""} key={kind}>
                      {formatCodexUsdPerCredit(row.rates[kind])}
                    </td>
                  ))}
                </tr>
              );
            })}
          </tbody>
        </table>
      </TableCard>
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

export function CodexCapacityBoard({ response, nowMs }: { response: CodexSubsResponse; nowMs: number }): ReactElement {
  const models = response.conversion_models ?? [];
  const windows = capacityWindows(response);
  const targetWindowMinutes = windows.some((item) => item.windowMinutes === 10_080)
    ? 10_080
    : windows[0]?.windowMinutes;
  const cohortWindows = windows.filter((item) => item.windowMinutes === targetWindowMinutes);
  const weekly = aggregateCapacityWindows(cohortWindows);
  const profitRows = profitabilityRows(models);
  const standardModel = models.find((model) => model.id === "gpt-5.6-sol") ?? models[0];
  const standard = standardModel ? codexTokenEconomics(standardModel, "standard", "short", "fresh") : null;
  const maximumRow = profitRows[0];
  const maximum = maximumRow?.best ?? null;
  const standardValue = codexApiValueForCredits(weekly?.fleetRemaining, standard);
  const maxValue = codexApiValueForCredits(weekly?.fleetRemaining, maximum);
  const standardScenario = standardModel ? `${standardModel.id} · short` : "нет тарифа";
  const maxScenario = maximumRow
    ? `${maximumRow.model.id} · ${maximumRow.tier}/${maximumRow.context}/${maximumRow.bestKind}`
    : "нет тарифа";

  return (
    <div className="codex-capacity-board">
      <CapacityStrip
        window={weekly}
        standardValue={standardValue}
        standardScenario={standardScenario}
        maxValue={maxValue}
        maxScenario={maxScenario}
      />
      {models.length ? (
        <>
          <TokenCapacityTable models={models} remaining={weekly?.fleetRemaining} />
          <ProfitabilityTable rows={profitRows} remaining={weekly?.fleetRemaining} />
        </>
      ) : (
        <div className="codex-no-catalog">Тарифный каталог недоступен.</div>
      )}
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
