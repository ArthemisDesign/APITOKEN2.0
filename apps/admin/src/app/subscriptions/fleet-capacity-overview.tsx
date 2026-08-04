"use client";

import { type ReactElement } from "react";
import { nanoMoney } from "@/lib/format";
import { providerInteger, usedPercentFromNano } from "./provider-calibration";
import {
  kimiFleetUsedPercent,
  kimiFleetWindowMoney,
  kimiMeasuredCoverage,
  kimiWindowDurations,
  kimiWindowLabel,
} from "./logic";
import type {
  CapacityResponse,
  ClaudeWindowTotal,
  CodexSubsResponse,
  CodexWindowTotal,
  GeminiSubsResponse,
  GeminiWindowTotal,
  KimiSubsResponse,
} from "./types";

interface ComparableWindow {
  window_minutes?: number;
  capacity_nano?: string | null;
  remaining_nano?: string | null;
}

interface FleetRail {
  label: string;
  window?: ComparableWindow;
  /** Переопределение used-доли (KIMI: exact provider used_fraction_units). */
  used?: { value: number | null; label: string };
  /** Подпись вместо «—», когда окно есть, но деньги ещё неизвестны (KIMI: «ждём данные»). */
  unknownMoney?: string;
}

interface FleetCardValue {
  id: "claude" | "gpt" | "gemini" | "kimi";
  label: string;
  status: "ok" | "warn" | "bad";
  ready: number;
  total: number;
  coverage: string;
  rails: FleetRail[];
}

function exactWindow<T extends ComparableWindow>(windows: T[] | undefined, minutes: number): T | undefined {
  return windows?.find((window) => Number(window.window_minutes) === minutes);
}

function claudeCard(response: CapacityResponse | null): FleetCardValue {
  if (!response) {
    return { id: "claude", label: "Claude", status: "bad", ready: 0, total: 0, coverage: "нет связи", rails: [] };
  }
  const items = response.per_sub ?? [];
  const five = exactWindow<ClaudeWindowTotal>(response.window_totals, 300);
  const weekly = exactWindow<ClaudeWindowTotal>(response.window_totals, 10_080);
  const ready = items.filter((item) => item.routable).length;
  const covered = Number(five?.calibrated_subs ?? 0);
  const required = Number(five?.routable_subs ?? ready);
  const pending = Number(response.calibration_delivery?.pending_events ?? 0);
  const dropped = Number(response.calibration_delivery?.dropped_events ?? 0);
  const persistenceOk = response.calibration_delivery?.persistence_ok !== false
    && response.calibration_authority_available !== false;
  const delivery = dropped > 0
    ? `${dropped} потеряно`
    : pending > 0
      ? `${pending} сохраняется`
      : persistenceOk
        ? `${covered}/${required} измерено`
        : "ошибка authority";
  return {
    id: "claude",
    label: "Claude",
    status: dropped > 0 || !persistenceOk ? "bad" : response.calibrated && ready > 0 && pending === 0 ? "ok" : "warn",
    ready,
    total: items.length,
    coverage: delivery,
    rails: [
      { label: "5ч", window: five },
      { label: "7д", window: weekly },
    ],
  };
}

function gptCard(response: CodexSubsResponse | null): FleetCardValue {
  if (!response || response.enabled === false) {
    return {
      id: "gpt",
      label: "GPT",
      status: "bad",
      ready: 0,
      total: 0,
      coverage: response ? "выключен" : "нет связи",
      rails: [],
    };
  }
  const homes = response.homes ?? [];
  const five = exactWindow<CodexWindowTotal>(response.window_totals, 300);
  const weekly = exactWindow<CodexWindowTotal>(response.window_totals, 10_080);
  const ready = Number(response.available ?? homes.filter((home) => home.admitted !== false && home.process_live).length);
  const measured = Number(five?.measured_homes ?? 0);
  const observed = Number(five?.observed_homes ?? homes.length);
  return {
    id: "gpt",
    label: "GPT",
    status: ready > 0 && five?.remaining_nano != null && weekly?.remaining_nano != null ? "ok" : "warn",
    ready,
    total: homes.length,
    coverage: `${measured}/${observed} измерено`,
    rails: [
      { label: "5ч", window: five },
      { label: "7д", window: weekly },
    ],
  };
}

function geminiCard(response: GeminiSubsResponse | null): FleetCardValue {
  if (!response || response.enabled === false) {
    return {
      id: "gemini",
      label: "Gemini",
      status: "bad",
      ready: 0,
      total: 0,
      coverage: response ? "выключен" : "нет связи",
      rails: [],
    };
  }
  const profiles = response.profiles ?? [];
  const five = exactWindow<GeminiWindowTotal>(response.window_totals, 300);
  const weekly = exactWindow<GeminiWindowTotal>(response.window_totals, 10_080);
  const ready = Number(response.available ?? 0);
  const pending = Number(response.calibration_delivery?.pending_events ?? 0);
  const dropped = Number(response.calibration_delivery?.dropped_events ?? 0);
  const profilePersistenceOk = profiles
    .every((profile) => profile.calibration_persistence_ok !== false);
  const persistenceOk = response.calibration_authority_available === true
    && response.calibration_delivery?.persistence_ok === true
    && profilePersistenceOk;
  const covered = Number(five?.measured_profiles ?? 0);
  const observed = Number(five?.observed_profiles ?? profiles.length);
  const coverage = dropped > 0
    ? `${dropped} потеряно`
    : pending > 0
      ? `${pending} сохраняется`
      : persistenceOk
        ? `${covered}/${observed} измерено`
        : "ошибка authority";
  return {
    id: "gemini",
    label: "Gemini",
    status: dropped > 0 || !persistenceOk
      ? "bad"
      : ready > 0 && pending === 0 && five?.remaining_nano != null && weekly?.remaining_nano != null
        ? "ok"
        : "warn",
    ready,
    total: profiles.length,
    coverage,
    rails: [
      { label: "5ч", window: persistenceOk && pending === 0 ? five : undefined },
      { label: "7д", window: persistenceOk && pending === 0 ? weekly : undefined },
    ],
  };
}

// KIMI публикует только per-profile quota/calibration без fleet window_totals:
// rails агрегируются из реальных duration_secs, а деньги суммируются fail-closed.
function kimiCard(response: KimiSubsResponse | null, nowMs?: number): FleetCardValue {
  if (!response || response.enabled === false) {
    return {
      id: "kimi",
      label: "KIMI",
      status: "bad",
      ready: 0,
      total: 0,
      coverage: response ? "выключен" : "нет связи",
      rails: [],
    };
  }
  const profiles = response.profiles ?? [];
  const nowSec = Number(response.now || Math.floor((nowMs ?? Date.now()) / 1000));
  const ready = Number(response.fleet?.available_profiles ?? 0);
  const total = Number(response.fleet?.profiles ?? profiles.length);
  const pending = Number(response.delivery?.pending_events ?? 0);
  const dropped = Number(response.delivery?.dropped_events ?? 0);
  const persistenceOk = response.delivery?.persistence_ok !== false;
  const moneyReady = persistenceOk && dropped === 0 && pending === 0;
  const durations = kimiWindowDurations(profiles);
  const rails: FleetRail[] = durations.map((secs) => {
    if (!moneyReady) return { label: kimiWindowLabel(secs) };
    const money = kimiFleetWindowMoney(profiles, secs, nowSec);
    return {
      label: kimiWindowLabel(secs),
      window: { capacity_nano: money.capacity, remaining_nano: money.remaining },
      used: kimiFleetUsedPercent(profiles, secs),
      unknownMoney: "ждём данные",
    };
  });
  const primary = durations[0];
  const coverage = dropped > 0
    ? `${dropped} потеряно`
    : pending > 0
      ? `${pending} сохраняется`
      : !persistenceOk
        ? "ошибка persistence"
        : primary == null
          ? "окон нет"
          : (() => {
              const { measured, observed } = kimiMeasuredCoverage(profiles, primary);
              return `${measured}/${observed} измерено`;
            })();
  return {
    id: "kimi",
    label: "KIMI",
    status: dropped > 0 || !persistenceOk
      ? "bad"
      : ready > 0 && pending === 0 && rails.length > 0
          && rails.every((rail) => providerInteger(rail.window?.remaining_nano) != null)
        ? "ok"
        : "warn",
    ready,
    total,
    coverage,
    rails,
  };
}

function FleetWindowRail({ rail }: { rail: FleetRail }): ReactElement {
  const used = rail.used ?? usedPercentFromNano(rail.window?.capacity_nano, rail.window?.remaining_nano);
  const percent = used.value ?? 0;
  const tone = percent >= 95 ? "bad" : percent >= 70 ? "warn" : "";
  // Окно без денег: «—» для скрытых/несуществующих окон, unknownMoney для
  // известного окна с ещё не измеренной ёмкостью (никогда не $0).
  const money = (value: string | null | undefined): string =>
    providerInteger(value) != null ? nanoMoney(value) : rail.window ? (rail.unknownMoney ?? "—") : "—";
  return (
    <div className="fleet-window">
      <div className="fleet-window-value">
        <span>{rail.label}</span>
        <strong>{money(rail.window?.remaining_nano)}</strong>
        <small>/ {money(rail.window?.capacity_nano)}</small>
        <b>{used.label}</b>
      </div>
      <span className="fleet-window-rail" aria-label={`${rail.label}: использовано ${used.label}`}>
        <i className={tone} style={{ width: `${percent}%` }} />
      </span>
    </div>
  );
}

function FleetCard({ value }: { value: FleetCardValue }): ReactElement {
  return (
    <article className={`fleet-capacity-card fleet-${value.id}`}>
      <header>
        <div><i aria-hidden="true" /><strong>{value.label}</strong></div>
        <span className={`fleet-state ${value.status}`}>{value.ready}/{value.total}</span>
      </header>
      {value.rails.map((rail) => <FleetWindowRail rail={rail} key={rail.label} />)}
      <footer>{value.coverage}</footer>
    </article>
  );
}

export function FleetCapacityOverview({
  claude,
  gpt,
  gemini,
  kimi,
  nowMs,
}: {
  claude: CapacityResponse | null;
  gpt: CodexSubsResponse | null;
  gemini: GeminiSubsResponse | null;
  kimi?: KimiSubsResponse | null;
  /** Момент снимка (мс); KIMI считает cooling/staleness от response.now, это fallback. */
  nowMs?: number;
}): ReactElement {
  const cards = [claudeCard(claude), gptCard(gpt), geminiCard(gemini), kimiCard(kimi ?? null, nowMs)];
  return (
    <section className="fleet-capacity-overview" aria-label="Доступная API-долларовая ёмкость пулов">
      <header>
        <div>
          <span>API-$ · сейчас</span>
          <h2>Ёмкость пулов</h2>
        </div>
        <b>остаток / полное окно</b>
      </header>
      <div className="fleet-capacity-grid">
        {cards.map((card) => <FleetCard value={card} key={card.id} />)}
      </div>
    </section>
  );
}
