"use client";

import { type ReactElement } from "react";
import { Pill, TableCard } from "@/components/ui";
import { duration, nanoMoney } from "@/lib/format";
import { providerInteger, usedPercentFromNano } from "./provider-calibration";
import { ProviderCapacityStrip, ProviderQuotaMeter, ProviderSection } from "./provider-board-ui";
import { geminiProfileStatus } from "./logic";
import type {
  GeminiProfile,
  GeminiProfileWindow,
  GeminiSubsResponse,
} from "./types";

function moneyOrDash(value: string | null | undefined): string {
  return providerInteger(value) == null ? "—" : nanoMoney(value);
}

function windowFor(profile: GeminiProfile, kind: string): GeminiProfileWindow | undefined {
  return (profile.windows ?? []).find((window) => window.window_kind === kind);
}

function windowQuota(window: GeminiProfileWindow | undefined): { value: number | null; label: string } {
  if (!window) return { value: null, label: "—" };
  const units = Number(window.used_fraction_units);
  const value = Number.isFinite(units)
    ? Math.min(100, Math.max(0, units / 1_000_000))
    : window.remaining_fraction == null
      ? null
      : Math.min(100, Math.max(0, (1 - Number(window.remaining_fraction)) * 100));
  if (value == null || !Number.isFinite(value)) return { value: null, label: "—" };
  const rounded = Math.round(value * 10) / 10;
  return { value: rounded, label: `${rounded.toLocaleString("ru-RU", { maximumFractionDigits: 1 })}%` };
}

function GeminiMoney({
  window,
  authorityReady,
  inactive,
  fiveHour = false,
}: {
  window: GeminiProfileWindow | undefined;
  authorityReady: boolean;
  inactive: boolean;
  fiveHour?: boolean;
}): ReactElement {
  const cellClass = [
    authorityReady && !inactive ? "provider-usd-ink" : "provider-capacity-state",
    fiveHour ? "provider-five-hour-money" : "",
  ].filter(Boolean).join(" ");
  if (inactive) {
    return <td className={cellClass}><b>вне ротации</b><small>не входит в ёмкость</small></td>;
  }
  if (!authorityReady) {
    return <td className={cellClass}><b>обновляем</b><small>quota уже доступна</small></td>;
  }
  return (
    <td className={cellClass}>
      <b>{moneyOrDash(window?.remaining_nano)}</b>
      <small>{`из ${moneyOrDash(window?.capacity_nano)}`}</small>
    </td>
  );
}

function GeminiSubscriptions({ profiles, modelCount, nowSec, authorityReady }: { profiles: GeminiProfile[]; modelCount: number; nowSec: number; authorityReady: boolean }) {
  return (
    <ProviderSection overline="Подписки" title="Окна по аккаунтам" meta={`${profiles.filter((profile) => profile.authenticated).length}/${profiles.length} auth`}>
      <TableCard>
        <table className="provider-home-capacity-table provider-gemini-home-table">
          <thead>
            <tr>
              <th className="left">Почта</th>
              <th className="left">Состояние</th>
              <th>Quota 5ч / reset</th>
              <th className="provider-five-hour-money">Доступно $ · 5ч</th>
              <th>Quota 7д / reset</th>
              <th>Доступно $ · 7д</th>
              <th>Модели</th>
            </tr>
          </thead>
          <tbody>
            {profiles.map((profile, index) => {
              // Quota/reset are live provider facts and remain useful while the API-$ evidence
              // FIFO is recovering. Only the saleable money projection is hidden fail closed.
              const fiveWindow = windowFor(profile, "5h");
              const weeklyWindow = windowFor(profile, "weekly");
              const five = windowQuota(fiveWindow);
              const weekly = windowQuota(weeklyWindow);
              const health = geminiProfileStatus(profile, nowSec);
              const inactive = profile.authenticated !== true || Number(profile.cooling_until ?? 0) > nowSec;
              const availableModels = profile.authenticated
                ? (profile.model_cooling ?? []).filter((model) => Number(model.cooling_until || 0) <= nowSec).length
                : 0;
              return (
                <tr key={profile.id ?? index}>
                  <td className="left"><b>{profile.email?.trim() || "—"}</b><small>{profile.plan ?? "—"}</small></td>
                  <td className="left"><Pill kind={health.kind}>{health.label}</Pill></td>
                  <td><ProviderQuotaMeter usedPercent={five.value} label={five.label} reset={fiveWindow?.resets_at ? duration(Math.max(0, fiveWindow.resets_at - nowSec)) : "—"} /></td>
                  <GeminiMoney window={fiveWindow} authorityReady={authorityReady} inactive={inactive} fiveHour />
                  <td><ProviderQuotaMeter usedPercent={weekly.value} label={weekly.label} reset={weeklyWindow?.resets_at ? duration(Math.max(0, weeklyWindow.resets_at - nowSec)) : "—"} /></td>
                  <GeminiMoney window={weeklyWindow} authorityReady={authorityReady} inactive={inactive} />
                  <td><b>{availableModels}/{modelCount}</b></td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </TableCard>
    </ProviderSection>
  );
}

export function GeminiCapacityBoard({
  response,
  nowMs,
  showSummary = true,
}: {
  response: GeminiSubsResponse;
  nowMs: number;
  showSummary?: boolean;
}): ReactElement {
  const profilePersistenceOk = (response.profiles ?? [])
    .every((profile) => profile.calibration_persistence_ok !== false);
  const authorityReady = response.calibration_authority_available === true
    && response.calibration_delivery?.persistence_ok === true
    && Number(response.calibration_delivery?.pending_events ?? 0) === 0
    && Number(response.calibration_delivery?.dropped_events ?? 0) === 0
    && profilePersistenceOk;
  const windows = authorityReady ? response.window_totals ?? [] : [];
  const weekly = windows.find((item) => Number(item.window_minutes) === 10_080) ?? windows.at(-1);
  const five = windows.find((item) => Number(item.window_minutes) === 300);
  const usedFive = usedPercentFromNano(five?.capacity_nano, five?.remaining_nano);
  const usedWeekly = usedPercentFromNano(weekly?.capacity_nano, weekly?.remaining_nano);
  const nowSec = Number(response.now || Math.floor(nowMs / 1000));

  return (
    <div className="provider-capacity-board gemini-capacity-board">
      {showSummary ? (
        <ProviderCapacityStrip
          ariaLabel="Ёмкость Gemini-пула"
          items={[
            {
              label: "5ч · доступно",
              value: moneyOrDash(five?.remaining_nano),
              caption: `из ${moneyOrDash(five?.capacity_nano)} · текущая смесь`,
              usd: true,
            },
            {
              label: "7д · доступно",
              value: moneyOrDash(weekly?.remaining_nano),
              caption: `из ${moneyOrDash(weekly?.capacity_nano)} · текущая смесь`,
              usd: true,
            },
            {
              label: "5ч · использовано",
              value: usedFive.label,
              caption: "workload-equivalent",
            },
            {
              label: "7д · использовано",
              value: usedWeekly.label,
              caption: "workload-equivalent",
            },
            {
              label: "Профили в ротации",
              value: `${response.available ?? 0}/${response.profiles?.length ?? 0}`,
              caption: `${five?.measured_profiles ?? 0}/${five?.observed_profiles ?? response.profiles?.length ?? 0} измерено`,
            },
          ]}
        />
      ) : null}
      <GeminiSubscriptions
        profiles={response.profiles ?? []}
        modelCount={response.models?.length ?? response.conversion_models?.length ?? 0}
        nowSec={nowSec}
        authorityReady={authorityReady}
      />
    </div>
  );
}
