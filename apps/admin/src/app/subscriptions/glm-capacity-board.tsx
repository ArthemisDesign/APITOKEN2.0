"use client";

import { Fragment, type ReactElement } from "react";
import { Pill, TableCard } from "@/components/ui";
import { duration, nanoMoney } from "@/lib/format";
import { exactTokenCount, providerInteger } from "./provider-calibration";
import {
  glmActiveCoolingAxes,
  glmEvidenceState,
  glmFleetUsedPercent,
  glmFleetWindowMoney,
  glmMeasuredCoverage,
  glmProfileStatus,
  glmUsedPercent,
  glmWindowDurations,
  glmWindowLabel,
} from "./logic";
import { ProviderCapacityStrip, ProviderQuotaMeter, ProviderSection, type ProviderStripItem } from "./provider-board-ui";
import type { GlmCalibrationWindow, GlmProfile, GlmQuotaWindow, GlmSubsResponse } from "./types";

function moneyOrDash(value: string | null | undefined): string {
  return providerInteger(value) == null ? "—" : nanoMoney(value);
}

function quotaFor(profile: GlmProfile, durationSecs: number): GlmQuotaWindow | undefined {
  return (profile.quota ?? []).find((window) => Number(window.duration_secs) === durationSecs);
}

function calibrationFor(profile: GlmProfile, durationSecs: number): GlmCalibrationWindow | undefined {
  return (profile.calibration ?? []).find((row) => Number(row.duration_secs) === durationSecs);
}

// Состояние денежной ячейки: saleable API-$ показываются только при здоровой
// delivery FIFO и свежем evidence; quota/reset и native counters остаются видны
// всегда — это live provider facts, полезные, пока dollar-evidence восстанавливается.
type GlmMoneyState = "ready" | "pending" | "degraded" | "stale" | "inactive";

function GlmMoney({
  row,
  state,
  primary = false,
}: {
  row: GlmCalibrationWindow | undefined;
  state: GlmMoneyState;
  primary?: boolean;
}): ReactElement {
  const remaining = providerInteger(row?.remaining?.api_nano ?? null);
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

// Native остаток (microcredits, exact integer) одной компактной колонкой справа:
// по строке на каждое реальное окно, null остаётся «—» и никогда не превращается в 0.
function GlmNativeUnits({
  profile,
  durations,
}: {
  profile: GlmProfile;
  durations: number[];
}): ReactElement {
  return (
    <td>
      {durations.map((secs) => {
        const native = providerInteger(calibrationFor(profile, secs)?.remaining?.native_units ?? null);
        return (
          <Fragment key={secs}>
            <b>{exactTokenCount(native)}</b>
            <small>{`${glmWindowLabel(secs)} · микрокредиты`}</small>
          </Fragment>
        );
      })}
    </td>
  );
}

function GlmSubscriptions({
  profiles,
  durations,
  ready,
  nowSec,
  delivery,
}: {
  profiles: GlmProfile[];
  durations: number[];
  ready: number;
  nowSec: number;
  delivery: GlmSubsResponse["delivery"];
}) {
  const pending = Number(delivery?.pending_events ?? 0);
  const dropped = Number(delivery?.dropped_events ?? 0);
  const persistenceOk = delivery?.persistence_ok !== false;
  return (
    <ProviderSection
      overline="Подписки"
      title="Окна по аккаунтам"
      meta={`${ready}/${profiles.length} в ротации`}
    >
      <TableCard>
        <table className="provider-home-capacity-table provider-glm-home-table">
          <thead>
            <tr>
              <th className="left">Профиль</th>
              <th className="left">Состояние</th>
              {durations.map((secs, index) => (
                <Fragment key={secs}>
                  <th>{`Quota ${glmWindowLabel(secs)} / reset`}</th>
                  <th className={index === 0 ? "provider-five-hour-money" : undefined}>
                    {`Доступно $ · ${glmWindowLabel(secs)}`}
                  </th>
                </Fragment>
              ))}
              <th>Native · остаток</th>
            </tr>
          </thead>
          <tbody>
            {profiles.map((profile, index) => {
              const status = glmProfileStatus(profile, nowSec);
              // Оси допуска runtime: dead/suspect флаги и timed cooling-оси.
              const inactive = profile.account_dead === true
                || profile.account_suspect === true
                || glmActiveCoolingAxes(profile, nowSec).length > 0;
              const evidence = glmEvidenceState(profile, nowSec);
              const moneyState: GlmMoneyState = inactive
                ? "inactive"
                : dropped > 0 || !persistenceOk
                  ? "degraded"
                  : pending > 0
                    ? "pending"
                    : evidence === "stale"
                      ? "stale"
                      : "ready";
              return (
                // Одна identity — одна строка независимо от числа окон.
                <tr key={profile.id ?? index}>
                  <td className="left"><b>{profile.id?.trim() || "—"}</b><small>{profile.plan ?? "—"}</small></td>
                  <td className="left"><Pill kind={status.kind}>{status.label}</Pill></td>
                  {durations.map((secs, windowIndex) => {
                    const quota = quotaFor(profile, secs);
                    const used = glmUsedPercent(quota?.used_fraction_units ?? null);
                    const reset = quota?.resets_at ? duration(Math.max(0, quota.resets_at - nowSec)) : "—";
                    return (
                      <Fragment key={secs}>
                        <td><ProviderQuotaMeter usedPercent={used.value} label={used.label} reset={reset} /></td>
                        <GlmMoney row={calibrationFor(profile, secs)} state={moneyState} primary={windowIndex === 0} />
                      </Fragment>
                    );
                  })}
                  <GlmNativeUnits profile={profile} durations={durations} />
                </tr>
              );
            })}
          </tbody>
        </table>
      </TableCard>
    </ProviderSection>
  );
}

export function GlmCapacityBoard({
  response,
  nowMs,
  showSummary = true,
}: {
  response: GlmSubsResponse;
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
  const durations = glmWindowDurations(profiles);

  const stripItems: ProviderStripItem[] = [];
  for (const secs of durations.slice(0, 2)) {
    const money = glmFleetWindowMoney(profiles, secs, nowSec);
    const used = glmFleetUsedPercent(profiles, secs);
    const label = glmWindowLabel(secs);
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
      value: used.label,
      caption: "по quota провайдера",
    });
  }
  const primary = durations[0];
  const coverage = primary == null
    ? "—"
    : (() => {
        const { measured, observed } = glmMeasuredCoverage(profiles, primary);
        return `${measured}/${observed} измерено`;
      })();
  stripItems.push({
    label: "Профили в ротации",
    value: `${ready}/${total}`,
    caption: coverage,
  });

  return (
    <div className="provider-capacity-board glm-capacity-board">
      {showSummary ? (
        <ProviderCapacityStrip ariaLabel="Ёмкость GLM-пула" items={stripItems} />
      ) : null}
      <GlmSubscriptions
        profiles={profiles}
        durations={durations}
        ready={ready}
        nowSec={nowSec}
        delivery={response.delivery}
      />
    </div>
  );
}
