// Чистая логика страницы «Подписки» — порт вычислений из subscriptions()
// (crates/server/src/admin-panel.js): баннер флота, статусы Claude/GPT/Gemini,
// пороговые бары. Вынесена из JSX ради юнит-тестов.
import { count, duration } from "@/lib/format";
import type { Tone } from "@/components/ui";
import type { CodexHome, GeminiProfile } from "./types";

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
  claudeCount: number;
  /** homes.length или «выкл.» при отключённом контуре. */
  gptSummary: number | string;
  /** profiles.length или «выкл.». */
  geminiSummary: number | string;
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
    title: "Все три флота подписок в ротации",
    sub: `Claude ${input.claudeCount} · GPT ${input.gptSummary} · Gemini ${input.geminiSummary} · обновлено ${input.updatedAt}`,
  };
}
