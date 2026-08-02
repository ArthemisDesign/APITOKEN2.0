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

function GeminiSubscriptions({ profiles, modelCount, nowSec }: { profiles: GeminiProfile[]; modelCount: number; nowSec: number }) {
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
              const fiveWindow = windowFor(profile, "5h");
              const weeklyWindow = windowFor(profile, "weekly");
              const five = windowQuota(fiveWindow);
              const weekly = windowQuota(weeklyWindow);
              const health = geminiProfileStatus(profile, nowSec);
              const availableModels = profile.authenticated
                ? (profile.model_cooling ?? []).filter((model) => Number(model.cooling_until || 0) <= nowSec).length
                : 0;
              return (
                <tr key={profile.id ?? index}>
                  <td className="left"><b>{profile.email?.trim() || "—"}</b><small>{profile.plan ?? "—"}</small></td>
                  <td className="left"><Pill kind={health.kind}>{health.label}</Pill></td>
                  <td><ProviderQuotaMeter usedPercent={five.value} label={five.label} reset={fiveWindow?.resets_at ? duration(Math.max(0, fiveWindow.resets_at - nowSec)) : "—"} /></td>
                  <td className="provider-usd-ink provider-five-hour-money">
                    <b>{moneyOrDash(fiveWindow?.remaining_nano)}</b>
                    <small>{`из ${moneyOrDash(fiveWindow?.capacity_nano)}`}</small>
                  </td>
                  <td><ProviderQuotaMeter usedPercent={weekly.value} label={weekly.label} reset={weeklyWindow?.resets_at ? duration(Math.max(0, weeklyWindow.resets_at - nowSec)) : "—"} /></td>
                  <td className="provider-usd-ink">
                    <b>{moneyOrDash(weeklyWindow?.remaining_nano)}</b>
                    <small>{`из ${moneyOrDash(weeklyWindow?.capacity_nano)}`}</small>
                  </td>
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
  const windows = response.window_totals ?? [];
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
      />
    </div>
  );
}
