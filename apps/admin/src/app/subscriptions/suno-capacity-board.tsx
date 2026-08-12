"use client";

import { type ReactElement } from "react";
import { Pill, TableCard } from "@/components/ui";
import { nanoMoney } from "@/lib/format";
import { exactTokenCount, providerInteger } from "./provider-calibration";
import {
  sunoActiveCoolingAxes,
  sunoEvidenceState,
  sunoFleetUsedPercent,
  sunoFleetWindowMoney,
  sunoMeasuredCoverage,
  sunoProfileStatus,
  sunoUsedPercent,
  sunoWindowDurations,
  sunoWindowLabel,
} from "./logic";
import {
  ProviderCapacityStrip,
  ProviderQuotaMeter,
  ProviderSection,
  type ProviderStripItem,
} from "./provider-board-ui";
import type { SunoCalibrationWindow, SunoProfile, SunoSubsResponse } from "./types";

function moneyOrDash(value: string | null | undefined): string {
  return providerInteger(value) == null ? "—" : nanoMoney(value);
}

function calibrationFor(profile: SunoProfile, durationSecs: number): SunoCalibrationWindow | undefined {
  return (profile.calibration ?? []).find((row) => Number(row.window_duration_secs) === durationSecs);
}

// Текущее месячное окно флота — duration самой свежей calibration записи (identity
// окна выводится из строгого YYYY-MM периода, поэтому константы не существует).
function currentWindowDuration(profiles: SunoProfile[]): number | null {
  let best: { secs: number; measured: number } | null = null;
  for (const profile of profiles) {
    for (const row of profile.calibration ?? []) {
      const secs = Number(row.window_duration_secs);
      if (secs <= 0) continue;
      const measured = Number(row.last_measured_at ?? 0);
      if (!best || measured > best.measured) best = { secs, measured };
    }
  }
  return best?.secs ?? null;
}

// Состояние денежной ячейки: saleable API-$ показываются только при здоровой
// delivery FIFO и свежем evidence; quota/counters остаются видны всегда — это live
// provider facts, полезные, пока dollar-evidence восстанавливается.
type SunoMoneyState = "ready" | "pending" | "degraded" | "stale" | "inactive";

function SunoMoney({
  row,
  state,
  primary = false,
}: {
  row: SunoCalibrationWindow | undefined;
  state: SunoMoneyState;
  primary?: boolean;
}): ReactElement {
  const remaining = providerInteger(row?.remaining ?? null);
  const cellClass = [
    state === "ready" && remaining != null ? "provider-usd-ink" : "provider-capacity-state",
    primary ? "provider-five-hour-money" : "",
  ].filter(Boolean).join(" ");
  if (state === "inactive") {
    return <td className={cellClass}><b>вне ротации</b><small>не входит в ёмкость</small></td>;
  }
  if (state === "pending") {
    return <td className={cellClass}><b>сохраняется</b><small>quota уже доступна</small></td>;
  }
  if (state === "degraded") {
    return <td className={cellClass}><b>обновляем</b><small>quota уже доступна</small></td>;
  }
  if (state === "stale") {
    return <td className={cellClass}><b>обновляем</b><small>ждём свежую квоту</small></td>;
  }
  if (remaining == null) {
    return <td className={cellClass}><b>ждём данные</b><small>ещё не измерено</small></td>;
  }
  return (
    <td className={cellClass}>
      <b>{nanoMoney(remaining)}</b>
      <small>{`из ${moneyOrDash(row?.capacity?.current_nano)}`}</small>
    </td>
  );
}

// Native остаток месяца (verbatim credits, exact integer) одной компактной колонкой
// справа: null остаётся «—» и никогда не превращается в 0.
function SunoNativeCredits({ profile }: { profile: SunoProfile }): ReactElement {
  const left = providerInteger(profile.quota?.total_credits_left ?? null);
  const limit = providerInteger(profile.quota?.monthly_limit ?? null);
  return (
    <td>
      <b>{exactTokenCount(left)}</b>
      <small>{`из ${limit == null ? "—" : exactTokenCount(limit)} · кредиты`}</small>
    </td>
  );
}

function SunoSubscriptions({
  profiles,
  durations,
  currentDuration,
  ready,
  nowSec,
  delivery,
}: {
  profiles: SunoProfile[];
  durations: number[];
  currentDuration: number | null;
  ready: number;
  nowSec: number;
  delivery: SunoSubsResponse["delivery"];
}) {
  const pending = Number(delivery?.pending_events ?? 0);
  const dropped = Number(delivery?.dropped_events ?? 0);
  const persistenceOk = delivery?.persistence_ok !== false;
  const quotaLabel = currentDuration == null ? "месяц" : sunoWindowLabel(currentDuration);
  return (
    <ProviderSection
      overline="Подписки"
      title="Окна по аккаунтам"
      meta={`${ready}/${profiles.length} в ротации`}
    >
      <TableCard>
        <table className="provider-home-capacity-table provider-suno-home-table">
          <thead>
            <tr>
              <th className="left">Профиль</th>
              <th className="left">Состояние</th>
              <th>{`Quota ${quotaLabel} / reset`}</th>
              {durations.map((secs, index) => (
                <th key={secs} className={index === 0 ? "provider-five-hour-money" : undefined}>
                  {`Доступно $ · ${sunoWindowLabel(secs)}`}
                </th>
              ))}
              <th>Native · остаток</th>
            </tr>
          </thead>
          <tbody>
            {profiles.map((profile, index) => {
              const status = sunoProfileStatus(profile, nowSec);
              // Оси допуска runtime: снятие с ротации (Clerk verdict), HARD quota wall
              // и timed cooling-оси.
              const inactive = profile.routable === false
                || profile.quota_walled === true
                || sunoActiveCoolingAxes(profile, nowSec).length > 0;
              const evidence = sunoEvidenceState(profile, nowSec);
              const moneyState: SunoMoneyState = inactive
                ? "inactive"
                : dropped > 0 || !persistenceOk
                  ? "degraded"
                  : pending > 0
                    ? "pending"
                    : evidence === "stale"
                      ? "stale"
                      : "ready";
              const used = sunoUsedPercent(profile.quota?.monthly_usage ?? null, profile.quota?.monthly_limit ?? null);
              return (
                // Одна identity — одна строка независимо от числа окон.
                <tr key={profile.id ?? index}>
                  <td className="left"><b>{profile.id?.trim() || "—"}</b><small>{profile.plan ?? "—"}</small></td>
                  <td className="left"><Pill kind={status.kind}>{status.label}</Pill></td>
                  <td><ProviderQuotaMeter usedPercent={used.value} label={used.label} reset="—" /></td>
                  {durations.map((secs, windowIndex) => (
                    <SunoMoney
                      key={secs}
                      row={calibrationFor(profile, secs)}
                      state={moneyState}
                      primary={windowIndex === 0}
                    />
                  ))}
                  <SunoNativeCredits profile={profile} />
                </tr>
              );
            })}
          </tbody>
        </table>
      </TableCard>
    </ProviderSection>
  );
}

export function SunoCapacityBoard({
  response,
  nowMs,
  showSummary = true,
}: {
  response: SunoSubsResponse;
  nowMs: number;
  showSummary?: boolean;
}): ReactElement {
  const profiles = response.profiles ?? [];
  const nowSec = Number(response.now || Math.floor(nowMs / 1000));
  const ready = Number(response.fleet?.available_profiles ?? 0);
  const total = Number(response.fleet?.profiles ?? profiles.length);
  const pending = Number(response.delivery?.pending_events ?? 0);
  const dropped = Number(response.delivery?.dropped_events ?? 0);
  const persistenceOk = response.delivery?.persistence_ok !== false;
  const moneyReady = persistenceOk && dropped === 0 && pending === 0;
  const durations = sunoWindowDurations(profiles);
  const currentDuration = currentWindowDuration(profiles);

  const stripItems: ProviderStripItem[] = [];
  const fleetUsed = sunoFleetUsedPercent(profiles);
  for (const secs of durations.slice(0, 2)) {
    const money = sunoFleetWindowMoney(profiles, secs, nowSec);
    const label = sunoWindowLabel(secs);
    stripItems.push({
      label: `${label} · доступно`,
      value: !moneyReady
        ? pending > 0
          ? "сохраняется"
          : "обновляем"
        : money.remaining == null
          ? "ждём данные"
          : nanoMoney(money.remaining),
      caption: moneyReady && money.remaining != null ? `из ${moneyOrDash(money.capacity)}` : "capacity ещё не измерена",
      usd: moneyReady && money.remaining != null,
    });
    stripItems.push({
      label: `${label} · использовано`,
      value: fleetUsed.label,
      caption: "по quota провайдера",
    });
  }
  const primary = durations[0];
  const coverage = primary == null
    ? "—"
    : (() => {
        const { measured, observed } = sunoMeasuredCoverage(profiles, primary);
        return `${measured}/${observed} измерено`;
      })();
  stripItems.push({
    label: "Профили в ротации",
    value: `${ready}/${total}`,
    caption: coverage,
  });

  return (
    <div className="provider-capacity-board suno-capacity-board">
      {showSummary ? (
        <ProviderCapacityStrip ariaLabel="Ёмкость Suno-пула" items={stripItems} />
      ) : null}
      <SunoSubscriptions
        profiles={profiles}
        durations={durations}
        currentDuration={currentDuration}
        ready={ready}
        nowSec={nowSec}
        delivery={response.delivery}
      />
    </div>
  );
}
