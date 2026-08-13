"use client";

import { type ReactElement } from "react";
import { nanoMoney } from "@/lib/format";
import { providerInteger, usedPercentFromNano } from "./provider-calibration";
import {
  glmMeasuredCoverage,
  glmWindowDurations,
  glmWindowLabel,
  kimiFleetUsedPercent,
  kimiFleetWindowMoney,
  kimiMeasuredCoverage,
  kimiWindowDurations,
  kimiWindowLabel,
  sunoFleetUsedPercent,
  sunoFleetWindowMoney,
  sunoMeasuredCoverage,
  sunoWindowDurations,
  sunoWindowLabel,
  tripo3dFleetMoney,
  tripo3dMeasuredCoverage,
} from "./logic";
import type {
  CapacityResponse,
  ClaudeWindowTotal,
  CodexSubsResponse,
  CodexWindowTotal,
  GeminiSubsResponse,
  GeminiWindowTotal,
  GlmSubsResponse,
  KimiSubsResponse,
  SunoSubsResponse,
  Tripo3dSubsResponse,
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
  id: "claude" | "gpt" | "gemini" | "kimi" | "glm" | "tripo3d" | "suno";
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

function claudeCard(response: CapacityResponse | null | undefined): FleetCardValue {
  if (response === undefined) {
    return { id: "claude", label: "Claude", status: "warn", ready: 0, total: 0, coverage: "загрузка", rails: [] };
  }
  if (response === null) {
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

function gptCard(response: CodexSubsResponse | null | undefined): FleetCardValue {
  if (response === undefined) {
    return { id: "gpt", label: "GPT", status: "warn", ready: 0, total: 0, coverage: "загрузка", rails: [] };
  }
  if (response === null || response.enabled === false) {
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

function geminiCard(response: GeminiSubsResponse | null | undefined): FleetCardValue {
  if (response === undefined) {
    return { id: "gemini", label: "Gemini", status: "warn", ready: 0, total: 0, coverage: "загрузка", rails: [] };
  }
  if (response === null || response.enabled === false) {
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
function kimiCard(response: KimiSubsResponse | null | undefined, nowMs?: number): FleetCardValue {
  if (response === undefined) {
    return { id: "kimi", label: "KIMI", status: "warn", ready: 0, total: 0, coverage: "загрузка", rails: [] };
  }
  if (response === null || response.enabled === false) {
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
  const authorityAvailable = response.calibration_authority_available === true;
  const moneyReady = authorityAvailable && persistenceOk && dropped === 0 && pending === 0;
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
        : !authorityAvailable
          ? "калибровка недоступна"
          : primary == null
            ? "окон нет"
            : (() => {
                const { measured, observed } = kimiMeasuredCoverage(profiles, primary);
                return `${measured}/${observed} измерено`;
              })();
  return {
    id: "kimi",
    label: "KIMI",
    status: dropped > 0 || !persistenceOk || !authorityAvailable
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

// GLM публикует fleet window_totals для двух канонических окон (300/10080 минут —
// проекция exact duration_secs 18000/604800) fail-closed суммами: null у любого
// профиля делает всё окно неизвестным, поэтому rail показывает «ждём данные»,
// а не $0. Без window_totals карточка деградирует в coverage-only, как KIMI.
function glmCard(response: GlmSubsResponse | null | undefined): FleetCardValue {
  if (response === undefined) {
    return { id: "glm", label: "GLM", status: "warn", ready: 0, total: 0, coverage: "загрузка", rails: [] };
  }
  if (response === null || response.enabled === false) {
    return {
      id: "glm",
      label: "GLM",
      status: "bad",
      ready: 0,
      total: 0,
      coverage: response ? "выключен" : "нет связи",
      rails: [],
    };
  }
  const profiles = response.profiles ?? [];
  const totals = response.window_totals ?? [];
  const ready = Number(response.fleet?.available_profiles ?? 0);
  const total = Number(response.fleet?.profiles ?? profiles.length);
  const pending = Number(response.delivery?.pending_events ?? 0);
  const dropped = Number(response.delivery?.dropped_events ?? 0);
  const persistenceOk = response.delivery?.persistence_ok !== false;
  const moneyReady = persistenceOk && dropped === 0 && pending === 0;
  const rails: FleetRail[] = totals.map((window) => {
    const secs = Number(window.duration_secs ?? 0) || Number(window.window_minutes ?? 0) * 60;
    const label = glmWindowLabel(secs);
    if (!moneyReady) return { label };
    return { label, window, unknownMoney: "ждём данные" };
  });
  const primary = Number(totals[0]?.duration_secs ?? 0) || glmWindowDurations(profiles)[0];
  const coverage = dropped > 0
    ? `${dropped} потеряно`
    : pending > 0
      ? `${pending} сохраняется`
      : !persistenceOk
        ? "ошибка persistence"
        : primary == null
          ? "окон нет"
          : (() => {
              const { measured, observed } = glmMeasuredCoverage(profiles, primary);
              return `${measured}/${observed} измерено`;
            })();
  return {
    id: "glm",
    label: "GLM",
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

// Tripo3D публикует только per-profile balance/calibration без fleet window_totals, как
// KIMI. Окон нет: prepaid баланс не сбрасывается, поэтому единственный rail — «баланс»
// с fail-closed суммой remaining/full; used-доли у prepaid трека не существует («—»).
function tripo3dCard(response: Tripo3dSubsResponse | null | undefined, nowMs?: number): FleetCardValue {
  if (response === undefined) {
    return { id: "tripo3d", label: "Tripo3D", status: "warn", ready: 0, total: 0, coverage: "загрузка", rails: [] };
  }
  if (response === null || response.enabled === false) {
    return {
      id: "tripo3d",
      label: "Tripo3D",
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
  const authorityAvailable = response.calibration_authority_available === true;
  const moneyReady = authorityAvailable && persistenceOk && dropped === 0 && pending === 0;
  const rails: FleetRail[] = profiles.length === 0
    ? []
    : [(() => {
        if (!moneyReady) return { label: "баланс", used: { value: null, label: "—" } };
        const money = tripo3dFleetMoney(profiles, nowSec);
        return {
          label: "баланс",
          window: { capacity_nano: money.capacity, remaining_nano: money.remaining },
          used: { value: null, label: "—" },
          unknownMoney: "ждём данные",
        };
      })()];
  const coverage = dropped > 0
    ? `${dropped} потеряно`
    : pending > 0
      ? `${pending} сохраняется`
      : !persistenceOk
        ? "ошибка persistence"
        : !authorityAvailable
          ? "калибровка недоступна"
          : profiles.length === 0
            ? "профилей нет"
            : (() => {
                const { measured, observed } = tripo3dMeasuredCoverage(profiles);
                return `${measured}/${observed} измерено`;
              })();
  return {
    id: "tripo3d",
    label: "Tripo3D",
    status: dropped > 0 || !persistenceOk || !authorityAvailable
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

// Suno публикует только per-profile quota/calibration без fleet window_totals, как
// KIMI: rails агрегируются из реальных window_duration_secs (ежемесячный цикл плана),
// деньги суммируются fail-closed, а used-доля — Σusage/Σlimit по verbatim counters.
function sunoCard(response: SunoSubsResponse | null | undefined, nowMs?: number): FleetCardValue {
  if (response === undefined) {
    return { id: "suno", label: "Suno", status: "warn", ready: 0, total: 0, coverage: "загрузка", rails: [] };
  }
  if (response === null || response.enabled === false) {
    return {
      id: "suno",
      label: "Suno",
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
  const authorityAvailable = response.calibration_authority_available === true;
  const moneyReady = authorityAvailable && persistenceOk && dropped === 0 && pending === 0;
  const durations = sunoWindowDurations(profiles);
  const used = sunoFleetUsedPercent(profiles);
  const rails: FleetRail[] = durations.map((secs) => {
    if (!moneyReady) return { label: sunoWindowLabel(secs), used };
    const money = sunoFleetWindowMoney(profiles, secs, nowSec);
    return {
      label: sunoWindowLabel(secs),
      window: { capacity_nano: money.capacity, remaining_nano: money.remaining },
      used,
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
        : !authorityAvailable
          ? "калибровка недоступна"
          : primary == null
            ? "окон нет"
            : (() => {
                const { measured, observed } = sunoMeasuredCoverage(profiles, primary);
                return `${measured}/${observed} измерено`;
              })();
  return {
    id: "suno",
    label: "Suno",
    status: dropped > 0 || !persistenceOk || !authorityAvailable
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
  glm,
  tripo3d,
  suno,
  nowMs,
}: {
  claude: CapacityResponse | null | undefined;
  gpt: CodexSubsResponse | null | undefined;
  gemini: GeminiSubsResponse | null | undefined;
  kimi?: KimiSubsResponse | null;
  glm?: GlmSubsResponse | null;
  tripo3d?: Tripo3dSubsResponse | null;
  suno?: SunoSubsResponse | null;
  /** Момент снимка (мс); KIMI/Tripo3D/Suno считают cooling/staleness от response.now, это fallback. */
  nowMs?: number;
}): ReactElement {
  const cards = [
    claudeCard(claude),
    gptCard(gpt),
    geminiCard(gemini),
    kimiCard(kimi, nowMs),
    glmCard(glm),
    tripo3dCard(tripo3d, nowMs),
    sunoCard(suno, nowMs),
  ];
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
