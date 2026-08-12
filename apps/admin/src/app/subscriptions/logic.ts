// Чистая логика страницы «Подписки» — порт вычислений из subscriptions()
// (crates/server/src/admin-panel.js): баннер флота, статусы Claude/GPT/Gemini/KIMI/GLM/
// Tripo3D, пороговые бары. Вынесена из JSX ради юнит-тестов.
import { count, duration } from "@/lib/format";
import type { Tone } from "@/components/ui";
import { providerInteger } from "./provider-calibration";
import type { CodexHome, GeminiProfile, GlmProfile, KimiProfile, Tripo3dProfile } from "./types";

// deadLabel: причина смерти Claude-токена → русская подпись пилюли.
export function deadLabel(reason: string | null | undefined): string {
  return reason === "permission_error"
    ? "токен мёртв · бан"
    : reason === "authentication_error"
      ? "токен мёртв · нужен re-auth"
      : "токен мёртв";
}

export interface BarSpec {
  percent: number;
  kind: Tone;
}

const clampPercent = (value: number): number => Math.min(100, Math.max(0, Math.round(value)));

// capacityBar(util): доля использования окна 0..1 → процент; тёплые тона при высокой загрузке.
export function barFromUtil(util: number | null | undefined): BarSpec {
  const percent = clampPercent((Number(util) || 0) * 100);
  return { percent, kind: percent >= 95 ? "bad" : percent >= 70 ? "warn" : "" };
}

// percentBar(percent): уже готовый процент использования (GPT-окна).
export function barFromPercent(value: number | null | undefined): BarSpec {
  const percent = clampPercent(Number(value) || 0);
  return { percent, kind: percent >= 95 ? "bad" : percent >= 70 ? "warn" : "" };
}

// remainingBar(fraction): остаток 0..1 преобразуется в ИСПОЛЬЗОВАННУЮ долю.
// Во всех трёх флотах заполненная полоса означает расход, а не доступный остаток.
export function barFromRemaining(fraction: number | null | undefined): BarSpec {
  const remaining = fraction == null || !Number.isFinite(Number(fraction)) ? 1 : Number(fraction);
  const percent = clampPercent((1 - remaining) * 100);
  return { percent, kind: percent >= 95 ? "bad" : percent >= 70 ? "warn" : "" };
}

// Отображаемый host прокси: порт обрезается, пустое значение → тире.
export function stripProxyPort(host: string | null | undefined): string {
  return String(host || "—").replace(/:[0-9]+$/, "");
}

export interface StatusPill {
  label: string;
  kind: Tone;
}

// homeStatus: вердикт допуска берётся у самого gateway (admitted/reject_reason),
// а не выводится панелью — иначе панель рано или поздно разъедется с роутингом.
export function homeStatus(home: CodexHome, nowSec: number): StatusPill {
  if (!home.process_live) return { label: "процесс остановлен", kind: "bad" };
  if (home.admitted === false || home.reject_reason) {
    switch (home.reject_reason) {
      case "account_dead":
        return { label: "подписка мертва", kind: "bad" };
      case "transport_wedged":
        return { label: "не отвечает · транспорт", kind: "bad" };
      case "transport_degraded":
        return { label: "не отвечает · деградация", kind: "bad" };
      case "cooling":
        return { label: "cooling " + duration(Math.max(0, (home.cooling_until ?? 0) - nowSec)), kind: "warn" };
      case "provider_limit":
        return { label: "лимит достигнут", kind: "warn" };
      default:
        return { label: "вне ротации", kind: "warn" };
    }
  }
  if (home.account_state === "suspect") return { label: "active · auth под вопросом", kind: "warn" };
  if (home.snapshot_age_secs != null && home.snapshot_age_secs > 600)
    return { label: "active · данные устарели", kind: "warn" };
  if (home.calibration_persistence_ok === false) return { label: "active · calibration storage", kind: "warn" };
  return { label: "active", kind: "ok" };
}

// Статус Gemini-подписки целиком. Не превращаем отсутствие probe конкретной модели
// в статус всего профиля: routing допускает authenticated профиль, пока он не cooling.
export function geminiProfileStatus(profile: GeminiProfile, nowSec: number): StatusPill {
  const coolingUntil = Number(profile.cooling_until || 0);
  // Оператор вывел профиль вручную — это состояние важнее любого автоматического диагноза:
  // «ошибка auth» на отключённом профиле сбивала бы с толку (он и не должен аутентифицироваться).
  if (profile.disabled) return { label: "отключён оператором", kind: "warn" };
  if (!profile.authenticated) return { label: "ошибка auth", kind: "bad" };
  if (coolingUntil > nowSec) return { label: "cooling " + duration(coolingUntil - nowSec), kind: "warn" };

  const models = profile.model_cooling ?? [];
  const coolingModels = models.filter((model) => Number(model.cooling_until || 0) > nowSec);
  if (models.length > 0 && coolingModels.length === models.length) {
    const soonestReady = Math.min(...coolingModels.map((model) => Number(model.cooling_until)));
    return { label: "модели cooling " + duration(soonestReady - nowSec), kind: "warn" };
  }

  const degraded = models.filter((model) => Number(model.failure_streak || 0) > 0).length;
  if (degraded > 0)
    return {
      label: "active · " + count(degraded, "модель degraded", "модели degraded", "моделей degraded"),
      kind: "warn",
    };
  if (coolingModels.length > 0)
    return {
      label: "active · " + count(coolingModels.length, "модель cooling", "модели cooling", "моделей cooling"),
      kind: "warn",
    };
  if (profile.calibration_persistence_ok === false) return { label: "active · calibration storage", kind: "warn" };
  return { label: "active", kind: "ok" };
}

/* ── KIMI ─────────────────────────────────────────────── */

// used_fraction_units во всех флотах — доля в единицах 1e-8.
const FRACTION_SCALE = 100_000_000n;

// Возраст evidence, после которого snapshot считается протухшим (как snapshot_age_secs у GPT).
const KIMI_STALE_SECS = 600;

// kimiWindowLabel: exact duration_secs → короткая подпись окна. 18000 → "5ч",
// 604800 → "7д"; любая другая длительность подписывается своим реальным
// размером — фиктивных 5ч/7д эквивалентов не существует.
export function kimiWindowLabel(durationSecs: number | null | undefined): string {
  const secs = Number(durationSecs) || 0;
  if (secs <= 0) return "окно";
  if (secs % 86_400 === 0) return `${secs / 86_400}д`;
  if (secs % 3_600 === 0) return `${secs / 3_600}ч`;
  if (secs % 60 === 0) return `${secs / 60}м`;
  return `${secs}с`;
}

// kimiWindowDurations: отсортированный набор реальных окон флота — union
// duration_secs из quota и calibration записей всех профилей.
export function kimiWindowDurations(profiles: KimiProfile[]): number[] {
  const found = new Set<number>();
  for (const profile of profiles) {
    for (const window of profile.quota ?? []) {
      const secs = Number(window.duration_secs);
      if (secs > 0) found.add(secs);
    }
    for (const row of profile.calibration ?? []) {
      const secs = Number(row.duration_secs);
      if (secs > 0) found.add(secs);
    }
  }
  return [...found].sort((a, b) => a - b);
}

export interface KimiCoolingAxis {
  name: string;
  until: number;
}

// kimiActiveCoolingAxes: активные оси cooling (auth/transport/quota) с их until.
export function kimiActiveCoolingAxes(profile: KimiProfile, nowSec: number): KimiCoolingAxis[] {
  const cooling = profile.cooling;
  return [
    { name: "auth", until: Number(cooling?.auth_until ?? 0) },
    { name: "транспорт", until: Number(cooling?.transport_until ?? 0) },
    { name: "quota", until: Number(cooling?.quota_until ?? 0) },
  ].filter((axis) => axis.until > nowSec);
}

// kimiLastObservedAt: свежайшая метка evidence профиля (quota snapshot или замер
// калибровки); null — наблюдений ещё не было.
export function kimiLastObservedAt(profile: KimiProfile): number | null {
  const stamps = [
    Number(profile.quota_observed_at ?? 0),
    ...(profile.quota ?? []).map((window) => Number(window.observed_at ?? 0)),
    ...(profile.calibration ?? []).map((row) => Number(row.last_measured_at ?? 0)),
  ].filter((value) => value > 0);
  return stamps.length ? Math.max(...stamps) : null;
}

export type KimiEvidenceState = "fresh" | "stale" | "empty";

export function kimiEvidenceState(profile: KimiProfile, nowSec: number): KimiEvidenceState {
  const observed = kimiLastObservedAt(profile);
  if (observed == null) return "empty";
  return nowSec - observed > KIMI_STALE_SECS ? "stale" : "fresh";
}

// kimiProfileStatus: состояние профиля целиком. Dead → «вне ротации»; активные
// cooling-оси — с отсчётом до последнего until; протухшие данные → «обновляем»;
// полное отсутствие наблюдений → «ждём данные» (null не превращается в 0).
export function kimiProfileStatus(profile: KimiProfile, nowSec: number): StatusPill {
  if (profile.live !== true) return { label: "вне ротации", kind: "bad" };
  const axes = kimiActiveCoolingAxes(profile, nowSec);
  if (axes.length > 0) {
    const last = Math.max(...axes.map((axis) => axis.until));
    const names = axes.map((axis) => axis.name).join("+");
    return { label: `cooling ${names} ${duration(last - nowSec)}`, kind: "warn" };
  }
  const evidence = kimiEvidenceState(profile, nowSec);
  if (evidence === "empty") return { label: "ждём данные", kind: "warn" };
  if (evidence === "stale") return { label: "обновляем", kind: "warn" };
  return { label: "active", kind: "ok" };
}

// kimiUsedPercent: used_fraction_units → точный процент с шагом 0.1 (BigInt,
// округление half-up, как usedPercentFromNano у остальных флотов).
export function kimiUsedPercent(
  unitsValue: number | string | bigint | null | undefined,
): { value: number | null; label: string } {
  const units = providerInteger(unitsValue);
  if (units == null) return { value: null, label: "—" };
  const bounded = units < 0n ? 0n : units > FRACTION_SCALE ? FRACTION_SCALE : units;
  const tenths = (bounded * 1_000n + FRACTION_SCALE / 2n) / FRACTION_SCALE;
  return {
    value: Number(tenths) / 10,
    label: `${tenths / 10n}${tenths % 10n ? `.${tenths % 10n}` : ""}%`,
  };
}

// kimiFleetUsedPercent: использованная доля окна по флоту — used_fraction_units
// профилей, взвешенные по limit_units их окон (BigInt); без лимитов — среднее.
export function kimiFleetUsedPercent(
  profiles: KimiProfile[],
  durationSecs: number,
): { value: number | null; label: string } {
  let weighted = 0n;
  let limits = 0n;
  let sum = 0n;
  let seen = 0n;
  for (const profile of profiles) {
    const window = (profile.quota ?? []).find((item) => Number(item.duration_secs) === durationSecs);
    const units = providerInteger(window?.used_fraction_units ?? null);
    if (units == null) continue;
    const limit = providerInteger(window?.limit_units ?? null);
    if (limit != null && limit > 0n) {
      weighted += units * limit;
      limits += limit;
    }
    sum += units;
    seen += 1n;
  }
  const combined = limits > 0n ? (weighted + limits / 2n) / limits : seen > 0n ? sum / seen : null;
  return kimiUsedPercent(combined);
}

// kimiFleetWindowMoney: сумма calibrated remaining/capacity окна только по
// профилям, чьи деньги продаваемы прямо сейчас (live, без активной cooling-оси,
// без протухшего snapshot) — ровно тем, чья строка показывает реальные API-$.
// Fail-closed: пустой набор или null у любого такого профиля делает итог
// неизвестным — никогда не частичная сумма и никогда не $0 вместо неизвестного.
export function kimiFleetWindowMoney(
  profiles: KimiProfile[],
  durationSecs: number,
  nowSec: number,
): { capacity: string | null; remaining: string | null } {
  const contributing = profiles.filter(
    (profile) =>
      profile.live === true
      && kimiActiveCoolingAxes(profile, nowSec).length === 0
      && kimiEvidenceState(profile, nowSec) !== "stale",
  );
  if (!contributing.length) return { capacity: null, remaining: null };
  let capacity = 0n;
  let remaining = 0n;
  for (const profile of contributing) {
    const row = (profile.calibration ?? []).find((item) => Number(item.duration_secs) === durationSecs);
    const current = providerInteger(row?.capacity?.current_nano ?? null);
    const api = providerInteger(row?.remaining?.api_nano ?? null);
    if (current == null || api == null) return { capacity: null, remaining: null };
    capacity += current;
    remaining += api;
  }
  return { capacity: capacity.toString(), remaining: remaining.toString() };
}

// kimiMeasuredCoverage: доля профилей с реальными замерами (samples > 0) в окне.
export function kimiMeasuredCoverage(
  profiles: KimiProfile[],
  durationSecs: number,
): { measured: number; observed: number } {
  const measured = profiles.filter((profile) =>
    (profile.calibration ?? []).some(
      (row) => Number(row.duration_secs) === durationSecs && Number(row.samples ?? 0) > 0,
    ),
  ).length;
  return { measured, observed: profiles.length };
}

/* ── GLM ─────────────────────────────────────────────── */

// Возраст evidence, после которого snapshot считается протухшим (как snapshot_age_secs у GPT).
const GLM_STALE_SECS = 600;

// glmWindowLabel: exact duration_secs → короткая подпись окна. 18000 → "5ч",
// 604800 → "7д"; любая другая длительность подписывается своим реальным
// размером — фиктивных 5ч/7д эквивалентов не существует.
export function glmWindowLabel(durationSecs: number | null | undefined): string {
  const secs = Number(durationSecs) || 0;
  if (secs <= 0) return "окно";
  if (secs % 86_400 === 0) return `${secs / 86_400}д`;
  if (secs % 3_600 === 0) return `${secs / 3_600}ч`;
  if (secs % 60 === 0) return `${secs / 60}м`;
  return `${secs}с`;
}

// glmWindowDurations: отсортированный набор реальных окон флота — union
// duration_secs из quota и calibration записей всех профилей.
export function glmWindowDurations(profiles: GlmProfile[]): number[] {
  const found = new Set<number>();
  for (const profile of profiles) {
    for (const window of profile.quota ?? []) {
      const secs = Number(window.duration_secs);
      if (secs > 0) found.add(secs);
    }
    for (const row of profile.calibration ?? []) {
      const secs = Number(row.duration_secs);
      if (secs > 0) found.add(secs);
    }
  }
  return [...found].sort((a, b) => a - b);
}

export interface GlmCoolingAxis {
  name: string;
  until: number;
}

// glmActiveCoolingAxes: активные timed оси cooling (transport/quota) с их until.
// Auth-оси GLM — durable флаги account_dead/account_suspect, а не timed quarantine,
// поэтому здесь их нет: они обрабатываются в glmProfileStatus отдельно.
export function glmActiveCoolingAxes(profile: GlmProfile, nowSec: number): GlmCoolingAxis[] {
  const cooling = profile.cooling;
  return [
    { name: "транспорт", until: Number(cooling?.transport_until ?? 0) },
    { name: "quota", until: Number(cooling?.quota_until ?? 0) },
  ].filter((axis) => axis.until > nowSec);
}

// glmLastObservedAt: свежайшая метка evidence профиля (quota snapshot или замер
// калибровки); null — наблюдений ещё не было.
export function glmLastObservedAt(profile: GlmProfile): number | null {
  const stamps = [
    Number(profile.quota_observed_at ?? 0),
    ...(profile.quota ?? []).map((window) => Number(window.observed_at ?? 0)),
    ...(profile.calibration ?? []).map((row) => Number(row.last_measured_at ?? 0)),
  ].filter((value) => value > 0);
  return stamps.length ? Math.max(...stamps) : null;
}

export type GlmEvidenceState = "fresh" | "stale" | "empty";

export function glmEvidenceState(profile: GlmProfile, nowSec: number): GlmEvidenceState {
  const observed = glmLastObservedAt(profile);
  if (observed == null) return "empty";
  return nowSec - observed > GLM_STALE_SECS ? "stale" : "fresh";
}

// glmProfileStatus: состояние профиля целиком, оси — ровно допуск runtime
// (selection ineligibility): account_dead → «вне ротации» до замены ключа;
// account_suspect → «под наблюдением» до свежего probe; активные cooling-оси —
// с отсчётом до последнего until; ключ без прошедшего probe (live:false) и
// полное отсутствие наблюдений → «ждём данные»; протухшие данные → «обновляем»
// (null не превращается в 0).
export function glmProfileStatus(profile: GlmProfile, nowSec: number): StatusPill {
  if (profile.account_dead === true) return { label: "вне ротации", kind: "bad" };
  if (profile.account_suspect === true) return { label: "под наблюдением", kind: "warn" };
  const axes = glmActiveCoolingAxes(profile, nowSec);
  if (axes.length > 0) {
    const last = Math.max(...axes.map((axis) => axis.until));
    const names = axes.map((axis) => axis.name).join("+");
    return { label: `cooling ${names} ${duration(last - nowSec)}`, kind: "warn" };
  }
  if (profile.live !== true) return { label: "ждём данные", kind: "warn" };
  const evidence = glmEvidenceState(profile, nowSec);
  if (evidence === "empty") return { label: "ждём данные", kind: "warn" };
  if (evidence === "stale") return { label: "обновляем", kind: "warn" };
  return { label: "active", kind: "ok" };
}

// glmUsedPercent: used_fraction_units → точный процент с шагом 0.1 (BigInt,
// округление half-up, как usedPercentFromNano у остальных флотов).
export function glmUsedPercent(
  unitsValue: number | string | bigint | null | undefined,
): { value: number | null; label: string } {
  const units = providerInteger(unitsValue);
  if (units == null) return { value: null, label: "—" };
  const bounded = units < 0n ? 0n : units > FRACTION_SCALE ? FRACTION_SCALE : units;
  const tenths = (bounded * 1_000n + FRACTION_SCALE / 2n) / FRACTION_SCALE;
  return {
    value: Number(tenths) / 10,
    label: `${tenths / 10n}${tenths % 10n ? `.${tenths % 10n}` : ""}%`,
  };
}

// glmFleetUsedPercent: использованная доля окна по флоту — used_fraction_units
// профилей, взвешенные по limit_units их окон (BigInt); без лимитов — среднее.
export function glmFleetUsedPercent(
  profiles: GlmProfile[],
  durationSecs: number,
): { value: number | null; label: string } {
  let weighted = 0n;
  let limits = 0n;
  let sum = 0n;
  let seen = 0n;
  for (const profile of profiles) {
    const window = (profile.quota ?? []).find((item) => Number(item.duration_secs) === durationSecs);
    const units = providerInteger(window?.used_fraction_units ?? null);
    if (units == null) continue;
    const limit = providerInteger(window?.limit_units ?? null);
    if (limit != null && limit > 0n) {
      weighted += units * limit;
      limits += limit;
    }
    sum += units;
    seen += 1n;
  }
  const combined = limits > 0n ? (weighted + limits / 2n) / limits : seen > 0n ? sum / seen : null;
  return glmUsedPercent(combined);
}

// glmFleetWindowMoney: сумма calibrated remaining/capacity окна только по
// профилям, чьи деньги продаваемы прямо сейчас (ключ подтверждён, не dead и не
// suspect, без активной cooling-оси, без протухшего snapshot) — ровно тем, чья
// строка показывает реальные API-$. Fail-closed: пустой набор или null у любого
// такого профиля делает итог неизвестным — никогда не частичная сумма и никогда
// не $0 вместо неизвестного.
export function glmFleetWindowMoney(
  profiles: GlmProfile[],
  durationSecs: number,
  nowSec: number,
): { capacity: string | null; remaining: string | null } {
  const contributing = profiles.filter(
    (profile) =>
      profile.live === true
      && profile.account_dead !== true
      && profile.account_suspect !== true
      && glmActiveCoolingAxes(profile, nowSec).length === 0
      && glmEvidenceState(profile, nowSec) !== "stale",
  );
  if (!contributing.length) return { capacity: null, remaining: null };
  let capacity = 0n;
  let remaining = 0n;
  for (const profile of contributing) {
    const row = (profile.calibration ?? []).find((item) => Number(item.duration_secs) === durationSecs);
    const current = providerInteger(row?.capacity?.current_nano ?? null);
    const api = providerInteger(row?.remaining?.api_nano ?? null);
    if (current == null || api == null) return { capacity: null, remaining: null };
    capacity += current;
    remaining += api;
  }
  return { capacity: capacity.toString(), remaining: remaining.toString() };
}

// glmMeasuredCoverage: доля профилей с реальными замерами (samples > 0) в окне.
export function glmMeasuredCoverage(
  profiles: GlmProfile[],
  durationSecs: number,
): { measured: number; observed: number } {
  const measured = profiles.filter((profile) =>
    (profile.calibration ?? []).some(
      (row) => Number(row.duration_secs) === durationSecs && Number(row.samples ?? 0) > 0,
    ),
  ).length;
  return { measured, observed: profiles.length };
}

/* ── Tripo3D ───────────────────────────────────────────── */

// Возраст evidence, после которого snapshot считается протухшим (как snapshot_age_secs у GPT).
const TRIPO3D_STALE_SECS = 600;

export interface Tripo3dCoolingAxis {
  name: string;
  until: number;
}

// tripo3dActiveCoolingAxes: активные cooling-оси (rate-limit/auth/transport) с их until.
// Balance wall — отдельный HARD verdict, а не timed ось, поэтому здесь его нет:
// он обрабатывается в tripo3dProfileStatus отдельно.
export function tripo3dActiveCoolingAxes(profile: Tripo3dProfile, nowSec: number): Tripo3dCoolingAxis[] {
  const cooling = profile.cooling;
  return [
    { name: "rate-limit", until: Number(cooling?.rate_limit_until ?? 0) },
    { name: "auth", until: Number(cooling?.auth_until ?? 0) },
    { name: "транспорт", until: Number(cooling?.transport_until ?? 0) },
  ].filter((axis) => axis.until > nowSec);
}

// tripo3dLastObservedAt: свежайшая метка evidence профиля (balance-probe или замер
// калибровки); null — наблюдений ещё не было.
export function tripo3dLastObservedAt(profile: Tripo3dProfile): number | null {
  const stamps = [
    Number(profile.balance?.observed_at ?? 0),
    Number(profile.calibration?.last_measured_at ?? 0),
  ].filter((value) => value > 0);
  return stamps.length ? Math.max(...stamps) : null;
}

export type Tripo3dEvidenceState = "fresh" | "stale" | "empty";

export function tripo3dEvidenceState(profile: Tripo3dProfile, nowSec: number): Tripo3dEvidenceState {
  const observed = tripo3dLastObservedAt(profile);
  if (observed == null) return "empty";
  return nowSec - observed > TRIPO3D_STALE_SECS ? "stale" : "fresh";
}

// tripo3dProfileStatus: состояние профиля целиком, оси — ровно допуск runtime
// (selection hard/soft): balance_walled (HARD provider verdict) → «баланс исчерпан»;
// активные cooling-оси — с отсчётом до последнего until; ключ без прошедшего probe
// (live:false) и полное отсутствие наблюдений → «ждём данные»; протухшие данные →
// «обновляем» (null не превращается в 0).
export function tripo3dProfileStatus(profile: Tripo3dProfile, nowSec: number): StatusPill {
  if (profile.balance_walled === true) return { label: "баланс исчерпан", kind: "warn" };
  const axes = tripo3dActiveCoolingAxes(profile, nowSec);
  if (axes.length > 0) {
    const last = Math.max(...axes.map((axis) => axis.until));
    const names = axes.map((axis) => axis.name).join("+");
    return { label: `cooling ${names} ${duration(last - nowSec)}`, kind: "warn" };
  }
  if (profile.live !== true) return { label: "ждём данные", kind: "warn" };
  const evidence = tripo3dEvidenceState(profile, nowSec);
  if (evidence === "empty") return { label: "ждём данные", kind: "warn" };
  if (evidence === "stale") return { label: "обновляем", kind: "warn" };
  return { label: "active", kind: "ok" };
}

// tripo3dFleetMoney: сумма calibrated remaining/capacity баланс-трека только по
// профилям, чьи деньги продаваемы прямо сейчас (probe подтверждён, нет balance wall,
// без активной cooling-оси, без протухшего snapshot) — ровно тем, чья строка показывает
// реальные API-$. Fail-closed: пустой набор или null у любого такого профиля делает
// итог неизвестным — никогда не частичная сумма и никогда не $0 вместо неизвестного.
export function tripo3dFleetMoney(
  profiles: Tripo3dProfile[],
  nowSec: number,
): { capacity: string | null; remaining: string | null } {
  const contributing = profiles.filter(
    (profile) =>
      profile.live === true
      && profile.balance_walled !== true
      && tripo3dActiveCoolingAxes(profile, nowSec).length === 0
      && tripo3dEvidenceState(profile, nowSec) !== "stale",
  );
  if (!contributing.length) return { capacity: null, remaining: null };
  let capacity = 0n;
  let remaining = 0n;
  for (const profile of contributing) {
    const current = providerInteger(profile.calibration?.capacity?.current_nano ?? null);
    const api = providerInteger(profile.calibration?.remaining?.api_nano ?? null);
    if (current == null || api == null) return { capacity: null, remaining: null };
    capacity += current;
    remaining += api;
  }
  return { capacity: capacity.toString(), remaining: remaining.toString() };
}

// tripo3dMeasuredCoverage: доля профилей с реальными замерами (samples > 0) на треке.
export function tripo3dMeasuredCoverage(
  profiles: Tripo3dProfile[],
): { measured: number; observed: number } {
  const measured = profiles.filter((profile) => Number(profile.calibration?.samples ?? 0) > 0).length;
  return { measured, observed: profiles.length };
}

export interface FleetBanner {
  kind: "ok" | "warn" | "bad";
  title: string;
  sub: string;
}

export interface FleetBannerInput {
  dead: number;
  suspect: number;
  subsDown: boolean;
  gptDown: boolean;
  geminiDown: boolean;
  geminiEmpty: boolean;
  gptAuthBad: number;
  gptProcDown: number;
  geminiAuthBad: number;
  geminiUnavailable: boolean;
  geminiMissing: number;
  kimiDown: boolean;
  kimiEmpty: boolean;
  kimiUnavailable: boolean;
  glmDown: boolean;
  glmEmpty: boolean;
  glmUnavailable: boolean;
  tripo3dDown: boolean;
  tripo3dEmpty: boolean;
  tripo3dUnavailable: boolean;
  claudeCount: number;
  /** homes.length или «выкл.» при отключённом контуре. */
  gptSummary: number | string;
  /** profiles.length или «выкл.». */
  geminiSummary: number | string;
  /** profiles.length или «выкл.». */
  kimiSummary: number | string;
  /** profiles.length или «выкл.». */
  glmSummary: number | string;
  /** profiles.length или «выкл.». */
  tripo3dSummary: number | string;
  /** Уже отформатированная метка обновления (formatDate(Date.now(), true)). */
  updatedAt: string;
}

// Баннер флота: auth/fleet faults имеют приоритет над состоянием наблюдения
// (порядок проверок — точно как в subscriptions()).
export function resolveBanner(input: FleetBannerInput): FleetBanner {
  if (input.dead)
    return {
      kind: "bad",
      title: count(
        input.dead,
        "Claude-подписка с мёртвым токеном",
        "Claude-подписки с мёртвым токеном",
        "Claude-подписок с мёртвым токеном",
      ),
      sub:
        "вне ротации — нужен свежий OAuth-токен (setup-token) на этот аккаунт" +
        (input.suspect ? ` · ${input.suspect} под наблюдением` : ""),
    };
  if (input.subsDown)
    return {
      kind: "warn",
      title: "Claude lifecycle-источник недоступен",
      sub: "/subs не отвечает — GPT и Gemini ниже работают независимо",
    };
  if (input.gptDown)
    return {
      kind: "warn",
      title: "GPT-контур (OpenAI Codex) не отвечает",
      sub: "данные по GPT-подпискам недоступны — проверьте openai-runtime",
    };
  if (input.geminiDown)
    return {
      kind: "warn",
      title: "Gemini-контур не отвечает",
      sub: "/gemini-subs недоступен — проверьте Gemini runtime и stable origin :8794",
    };
  if (input.geminiEmpty)
    return {
      kind: "warn",
      title: "В Gemini-пуле нет профилей",
      sub: "runtime работает, но Auth Bot ещё не опубликовал ни одной paid Code Assist подписки",
    };
  if (input.gptAuthBad || input.gptProcDown)
    return {
      kind: "warn",
      title:
        (input.gptAuthBad
          ? count(input.gptAuthBad, "GPT-подписка", "GPT-подписки", "GPT-подписок") + " с ошибкой auth"
          : "") +
        (input.gptAuthBad && input.gptProcDown ? " · " : "") +
        (input.gptProcDown ? count(input.gptProcDown, "процесс", "процесса", "процессов") + " остановлен" : ""),
      sub: "OpenAI Codex: часть homes вне ротации",
    };
  if (input.geminiAuthBad || input.geminiUnavailable || input.geminiMissing)
    return {
      kind: "warn",
      title:
        (input.geminiAuthBad
          ? count(input.geminiAuthBad, "Gemini-профиль", "Gemini-профиля", "Gemini-профилей") + " с ошибкой auth"
          : "") +
        (input.geminiAuthBad && (input.geminiUnavailable || input.geminiMissing) ? " · " : "") +
        (input.geminiUnavailable
          ? "нет доступных профилей"
          : input.geminiMissing
            ? "нет usage metadata: " + input.geminiMissing
            : ""),
      sub: "Gemini: auth-профили исключаются из ротации; поток без финального usage списывает только консервативный hold",
    };
  if (input.kimiDown)
    return {
      kind: "warn",
      title: "KIMI-контур не отвечает",
      sub: "/kimi-subs недоступен — проверьте KIMI runtime и stable origin :8803",
    };
  if (input.kimiEmpty)
    return {
      kind: "warn",
      title: "В KIMI-пуле нет профилей",
      sub: "плоскость включена, но roster ещё пуст — ни одной подписки не опубликовано",
    };
  if (input.kimiUnavailable)
    return {
      kind: "warn",
      title: "KIMI: нет доступных профилей",
      sub: "все профили cooling по одной из осей или вне ротации — ёмкость временно не продаётся",
    };
  if (input.glmDown)
    return {
      kind: "warn",
      title: "GLM-контур не отвечает",
      sub: "/glm-subs недоступен — проверьте GLM backend внутри Anthropic runtime",
    };
  if (input.glmEmpty)
    return {
      kind: "warn",
      title: "В GLM-пуле нет профилей",
      sub: "плоскость включена, но roster ещё пуст — ни одной подписки не опубликовано",
    };
  if (input.glmUnavailable)
    return {
      kind: "warn",
      title: "GLM: нет доступных профилей",
      sub: "все профили dead/suspect, cooling по одной из осей или вне ротации — ёмкость временно не продаётся",
    };
  if (input.tripo3dDown)
    return {
      kind: "warn",
      title: "Tripo3D-контур не отвечает",
      sub: "/tripo3d-subs недоступен — плоскость пока dormant: production origin не настроен, данные появятся после активации",
    };
  if (input.tripo3dEmpty)
    return {
      kind: "warn",
      title: "В Tripo3D-пуле нет профилей",
      sub: "плоскость включена, но roster ещё пуст — ни одного API-ключа не опубликовано",
    };
  if (input.tripo3dUnavailable)
    return {
      kind: "warn",
      title: "Tripo3D: нет доступных профилей",
      sub: "все профили balance-walled, cooling по одной из осей или вне ротации — ёмкость временно не продаётся",
    };
  if (input.suspect)
    return {
      kind: "warn",
      title:
        count(input.suspect, "подписка под наблюдением", "подписки под наблюдением", "подписок под наблюдением") +
        " (auth падает)",
      sub: "движок корроборирует чистыми probe; при подтверждении — пометит DEAD",
    };
  return {
    kind: "ok",
    title: "Все шесть флотов подписок в ротации",
    sub: `Claude ${input.claudeCount} · GPT ${input.gptSummary} · Gemini ${input.geminiSummary} · KIMI ${input.kimiSummary} · GLM ${input.glmSummary} · Tripo3D ${input.tripo3dSummary} · обновлено ${input.updatedAt}`,
  };
}
