"use client";

import { type ReactElement } from "react";
import { nanoMoney } from "@/lib/format";
import { providerInteger, usedPercentFromNano } from "./provider-calibration";
import type {
  CapacityResponse,
  ClaudeWindowTotal,
  CodexSubsResponse,
  CodexWindowTotal,
  GeminiSubsResponse,
  GeminiWindowTotal,
} from "./types";

interface ComparableWindow {
  window_minutes?: number;
  capacity_nano?: string | null;
  remaining_nano?: string | null;
}

interface FleetCardValue {
  id: "claude" | "gpt" | "gemini";
  label: string;
  status: "ok" | "warn" | "bad";
  ready: number;
  total: number;
  coverage: string;
  five?: ComparableWindow;
  weekly?: ComparableWindow;
}

function exactWindow<T extends ComparableWindow>(windows: T[] | undefined, minutes: number): T | undefined {
  return windows?.find((window) => Number(window.window_minutes) === minutes);
}

function moneyOrDash(value: string | null | undefined): string {
  return providerInteger(value) == null ? "—" : nanoMoney(value);
}

function claudeCard(response: CapacityResponse | null): FleetCardValue {
  if (!response) {
    return { id: "claude", label: "Claude", status: "bad", ready: 0, total: 0, coverage: "нет связи" };
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
    five,
    weekly,
  };
}

function gptCard(response: CodexSubsResponse | null): FleetCardValue {
  if (!response || response.enabled === false) {
    return { id: "gpt", label: "GPT", status: "bad", ready: 0, total: 0, coverage: response ? "выключен" : "нет связи" };
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
    five,
    weekly,
  };
}

function geminiCard(response: GeminiSubsResponse | null): FleetCardValue {
  if (!response || response.enabled === false) {
    return { id: "gemini", label: "Gemini", status: "bad", ready: 0, total: 0, coverage: response ? "выключен" : "нет связи" };
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
    five: persistenceOk && pending === 0 ? five : undefined,
    weekly: persistenceOk && pending === 0 ? weekly : undefined,
  };
}

function FleetWindowRail({ label, window }: { label: string; window?: ComparableWindow }): ReactElement {
  const used = usedPercentFromNano(window?.capacity_nano, window?.remaining_nano);
  const percent = used.value ?? 0;
  const tone = percent >= 95 ? "bad" : percent >= 70 ? "warn" : "";
  return (
    <div className="fleet-window">
      <div className="fleet-window-value">
        <span>{label}</span>
        <strong>{moneyOrDash(window?.remaining_nano)}</strong>
        <small>/ {moneyOrDash(window?.capacity_nano)}</small>
        <b>{used.label}</b>
      </div>
      <span className="fleet-window-rail" aria-label={`${label}: использовано ${used.label}`}>
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
      <FleetWindowRail label="5ч" window={value.five} />
      <FleetWindowRail label="7д" window={value.weekly} />
      <footer>{value.coverage}</footer>
    </article>
  );
}

export function FleetCapacityOverview({
  claude,
  gpt,
  gemini,
}: {
  claude: CapacityResponse | null;
  gpt: CodexSubsResponse | null;
  gemini: GeminiSubsResponse | null;
}): ReactElement {
  const cards = [claudeCard(claude), gptCard(gpt), geminiCard(gemini)];
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
