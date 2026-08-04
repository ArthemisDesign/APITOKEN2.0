"use client";

import { Fragment, type ReactElement } from "react";
import { Pill, TableCard } from "@/components/ui";
import { duration, nanoMoney } from "@/lib/format";
import { providerInteger } from "./provider-calibration";
import {
  kimiActiveCoolingAxes,
  kimiEvidenceState,
  kimiFleetUsedPercent,
  kimiFleetWindowMoney,
  kimiMeasuredCoverage,
  kimiProfileStatus,
  kimiUsedPercent,
  kimiWindowDurations,
  kimiWindowLabel,
} from "./logic";
import { ProviderCapacityStrip, ProviderQuotaMeter, ProviderSection, type ProviderStripItem } from "./provider-board-ui";
import type { KimiCalibrationWindow, KimiProfile, KimiQuotaWindow, KimiSubsResponse } from "./types";

function moneyOrDash(value: string | null | undefined): string {
  return providerInteger(value) == null ? "—" : nanoMoney(value);
}

function quotaFor(profile: KimiProfile, durationSecs: number): KimiQuotaWindow | undefined {
  return (profile.quota ?? []).find((window) => Number(window.duration_secs) === durationSecs);
}

function calibrationFor(profile: KimiProfile, durationSecs: number): KimiCalibrationWindow | undefined {
  return (profile.calibration ?? []).find((row) => Number(row.duration_secs) === durationSecs);
}

// Состояние денежной ячейки: saleable API-$ показываются только при здоровой
// delivery FIFO и свежем evidence; quota/reset остаются видны всегда — это live
// provider facts, полезные, пока dollar-evidence восстанавливается.
type KimiMoneyState = "ready" | "pending" | "degraded" | "stale" | "inactive";

function KimiMoney({
  row,
  state,
  primary = false,
}: {
  row: KimiCalibrationWindow | undefined;
  state: KimiMoneyState;
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

function KimiSubscriptions({
  profiles,
  durations,
  ready,
  nowSec,
  delivery,
}: {
  profiles: KimiProfile[];
  durations: number[];
  ready: number;
  nowSec: number;
  delivery: KimiSubsResponse["delivery"];
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
        <table className="provider-home-capacity-table">
          <thead>
            <tr>
              <th className="left">Профиль</th>
              <th className="left">Состояние</th>
              {durations.map((secs, index) => (
                <Fragment key={secs}>
                  <th>{`Quota ${kimiWindowLabel(secs)} / reset`}</th>
                  <th className={index === 0 ? "provider-five-hour-money" : undefined}>
                    {`Доступно $ · ${kimiWindowLabel(secs)}`}
                  </th>
                </Fragment>
              ))}
            </tr>
          </thead>
          <tbody>
            {profiles.map((profile, index) => {
              const status = kimiProfileStatus(profile, nowSec);
              const inactive = profile.live !== true || kimiActiveCoolingAxes(profile, nowSec).length > 0;
              const evidence = kimiEvidenceState(profile, nowSec);
              const moneyState: KimiMoneyState = inactive
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
                    const used = kimiUsedPercent(quota?.used_fraction_units ?? null);
                    const reset = quota?.resets_at ? duration(Math.max(0, quota.resets_at - nowSec)) : "—";
                    return (
                      <Fragment key={secs}>
                        <td><ProviderQuotaMeter usedPercent={used.value} label={used.label} reset={reset} /></td>
                        <KimiMoney row={calibrationFor(profile, secs)} state={moneyState} primary={windowIndex === 0} />
                      </Fragment>
                    );
                  })}
                </tr>
              );
            })}
          </tbody>
        </table>
      </TableCard>
    </ProviderSection>
  );
}

export function KimiCapacityBoard({
  response,
  nowMs,
  showSummary = true,
}: {
  response: KimiSubsResponse;
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
  const durations = kimiWindowDurations(profiles);

  const stripItems: ProviderStripItem[] = [];
  for (const secs of durations.slice(0, 2)) {
    const money = kimiFleetWindowMoney(profiles, secs, nowSec);
    const used = kimiFleetUsedPercent(profiles, secs);
    const label = kimiWindowLabel(secs);
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
        const { measured, observed } = kimiMeasuredCoverage(profiles, primary);
        return `${measured}/${observed} измерено`;
      })();
  stripItems.push({
    label: "Профили в ротации",
    value: `${ready}/${total}`,
    caption: coverage,
  });

  return (
    <div className="provider-capacity-board kimi-capacity-board">
      {showSummary ? (
        <ProviderCapacityStrip ariaLabel="Ёмкость KIMI-пула" items={stripItems} />
      ) : null}
      <KimiSubscriptions
        profiles={profiles}
        durations={durations}
        ready={ready}
        nowSec={nowSec}
        delivery={response.delivery}
      />
    </div>
  );
}
