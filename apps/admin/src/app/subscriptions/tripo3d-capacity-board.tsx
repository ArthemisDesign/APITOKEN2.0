"use client";

import { type ReactElement } from "react";
import { Pill, TableCard } from "@/components/ui";
import { nanoMoney } from "@/lib/format";
import { exactTokenCount, providerInteger } from "./provider-calibration";
import {
  tripo3dActiveCoolingAxes,
  tripo3dEvidenceState,
  tripo3dFleetMoney,
  tripo3dMeasuredCoverage,
  tripo3dProfileStatus,
} from "./logic";
import { ProviderCapacityStrip, ProviderSection, type ProviderStripItem } from "./provider-board-ui";
import type { Tripo3dCalibration, Tripo3dProfile, Tripo3dSubsResponse } from "./types";

function moneyOrDash(value: string | null | undefined): string {
  return providerInteger(value) == null ? "—" : nanoMoney(value);
}

// Состояние денежной ячейки: saleable API-$ показываются только при здоровой
// delivery FIFO и свежем evidence; сырые половины баланса остаются видны всегда —
// это live provider facts, полезные, пока dollar-evidence восстанавливается.
type Tripo3dMoneyState = "ready" | "pending" | "degraded" | "stale" | "inactive";

function Tripo3dMoney({
  row,
  state,
}: {
  row: Tripo3dCalibration | null | undefined;
  state: Tripo3dMoneyState;
}): ReactElement {
  const remaining = providerInteger(row?.remaining?.api_nano ?? null);
  const cellClass = [
    state === "ready" && remaining != null ? "provider-usd-ink" : "provider-capacity-state",
    "provider-five-hour-money",
  ].filter(Boolean).join(" ");
  if (state === "inactive") {
    return <td className={cellClass}><b>вне ротации</b><small>не входит в ёмкость</small></td>;
  }
  if (state === "pending") {
    return <td className={cellClass}><b>сохраняется</b><small>баланс уже доступен</small></td>;
  }
  if (state === "degraded") {
    return <td className={cellClass}><b>обновляем</b><small>баланс уже доступен</small></td>;
  }
  if (state === "stale") {
    return <td className={cellClass}><b>обновляем</b><small>ждём свежий баланс</small></td>;
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

// Сырые половины баланса — verbatim текст провайдера, как пришёл (§5.2: parsed halves
// остаются null, пока unit не доказан; ничего не пересчитываем и не zero-fill'им).
function Tripo3dBalance({ profile }: { profile: Tripo3dProfile }): ReactElement {
  const balance = profile.balance;
  return (
    <td>
      <b>{balance?.balance_raw ?? "—"}</b>
      <small>{`заморожено ${balance?.frozen_raw ?? "—"}`}</small>
    </td>
  );
}

function Tripo3dSubscriptions({
  profiles,
  ready,
  nowSec,
  delivery,
}: {
  profiles: Tripo3dProfile[];
  ready: number;
  nowSec: number;
  delivery: Tripo3dSubsResponse["delivery"];
}) {
  const pending = Number(delivery?.pending_events ?? 0);
  const dropped = Number(delivery?.dropped_events ?? 0);
  const persistenceOk = delivery?.persistence_ok !== false;
  return (
    <ProviderSection
      overline="Подписки"
      title="Баланс по аккаунтам"
      meta={`${ready}/${profiles.length} в ротации`}
    >
      <TableCard>
        <table className="provider-home-capacity-table provider-tripo3d-home-table">
          <thead>
            <tr>
              <th className="left">Профиль</th>
              <th className="left">Состояние</th>
              <th>Баланс провайдера</th>
              <th className="provider-five-hour-money">Доступно $ · баланс</th>
              <th>Native · остаток</th>
            </tr>
          </thead>
          <tbody>
            {profiles.map((profile, index) => {
              const status = tripo3dProfileStatus(profile, nowSec);
              // Оси допуска runtime: HARD balance wall и timed cooling-оси.
              const inactive = profile.balance_walled === true
                || tripo3dActiveCoolingAxes(profile, nowSec).length > 0;
              const evidence = tripo3dEvidenceState(profile, nowSec);
              const moneyState: Tripo3dMoneyState = inactive
                ? "inactive"
                : dropped > 0 || !persistenceOk
                  ? "degraded"
                  : pending > 0
                    ? "pending"
                    : evidence === "stale"
                      ? "stale"
                      : "ready";
              const native = providerInteger(profile.calibration?.remaining?.native_micro_units ?? null);
              return (
                // Одна identity — одна строка: трек баланса один, окон нет.
                <tr key={profile.id ?? index}>
                  <td className="left"><b>{profile.id?.trim() || "—"}</b><small>{profile.cohort ?? "—"}</small></td>
                  <td className="left"><Pill kind={status.kind}>{status.label}</Pill></td>
                  <Tripo3dBalance profile={profile} />
                  <Tripo3dMoney row={profile.calibration} state={moneyState} />
                  <td><b>{exactTokenCount(native)}</b><small>баланс · микроюниты</small></td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </TableCard>
    </ProviderSection>
  );
}

export function Tripo3dCapacityBoard({
  response,
  nowMs,
  showSummary = true,
}: {
  response: Tripo3dSubsResponse;
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
  const money = tripo3dFleetMoney(profiles, nowSec);
  const { measured, observed } = tripo3dMeasuredCoverage(profiles);

  const stripItems: ProviderStripItem[] = [
    {
      label: "Баланс · доступно",
      value: !moneyReady
        ? pending > 0
          ? "сохраняется"
          : "обновляем"
        : money.remaining == null
          ? "ждём данные"
          : nanoMoney(money.remaining),
      caption: moneyReady && money.remaining != null ? `из ${moneyOrDash(money.capacity)}` : "capacity ещё не измерена",
      usd: moneyReady && money.remaining != null,
    },
    {
      label: "Профили в ротации",
      value: `${ready}/${total}`,
      caption: `${measured}/${observed} измерено`,
    },
    {
      label: "Задачи в работе",
      value: `${Number(response.fleet?.inflight_requests ?? 0)}`,
      caption: "inflight · prepaid, без окон",
    },
  ];

  return (
    <div className="provider-capacity-board tripo3d-capacity-board">
      {showSummary ? (
        <ProviderCapacityStrip ariaLabel="Ёмкость Tripo3D-пула" items={stripItems} />
      ) : null}
      <Tripo3dSubscriptions
        profiles={profiles}
        ready={ready}
        nowSec={nowSec}
        delivery={response.delivery}
      />
    </div>
  );
}
