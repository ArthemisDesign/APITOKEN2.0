"use client";

// Подписки — порт 1:1 функции subscriptions() из crates/server/src/admin-panel.js.
// Живые флоты: Claude OAuth (/subs + compact /capacity?recent_turns=0),
// GPT/Codex, Gemini, KIMI, GLM. Tripo3D и Suno stay local dormant envelopes
// (no Caddy origin yet) so the page never GETs /tripo3d-subs or /suno-subs.
import { useMemo } from "react";
import { compactCapacityUrl, compactCodexSubsUrl, compactGeminiSubsUrl, compactGlmSubsUrl, compactKimiSubsUrl } from "@/lib/engine-urls";
import { useResources } from "@/lib/resources";
import { count, formatDate } from "@/lib/format";
import { Banner, LoadingGrid, PageHead, Pill } from "@/components/ui";
import { ClaudeCapacityBoard } from "./claude-capacity-board";
import { CodexCapacityBoard } from "./codex-capacity-board";
import { FleetCapacityOverview } from "./fleet-capacity-overview";
import { GeminiCapacityBoard } from "./gemini-capacity-board";
import { GlmCapacityBoard } from "./glm-capacity-board";
import { KimiCapacityBoard } from "./kimi-capacity-board";
import { SunoCapacityBoard } from "./suno-capacity-board";
import { Tripo3dCapacityBoard } from "./tripo3d-capacity-board";
import { resolveBanner } from "./logic";
import type {
  CapacityResponse,
  CodexSubsResponse,
  GeminiSubsResponse,
  GlmSubsResponse,
  KimiSubsResponse,
  SubsResponse,
  SunoSubsResponse,
  Tripo3dSubsResponse,
} from "./types";

interface SubsData {
  subs: SubsResponse;
  capacity: CapacityResponse;
  codex: CodexSubsResponse;
  gemini: GeminiSubsResponse;
  kimi: KimiSubsResponse;
  glm: GlmSubsResponse;
  tripo3d: Tripo3dSubsResponse;
  suno: SunoSubsResponse;
}

interface SubsPageState {
  data: { [K in keyof SubsData]: SubsData[K] | undefined };
  availability: { [K in keyof SubsData]: "loading" | "ready" | "error" };
  isLoading: boolean;
  updatedAt: number;
}

export function SubscriptionsView({ state }: { state: SubsPageState }) {
  const { data: result, availability, isLoading, updatedAt: nowMs } = state;

  // Все производные флотов пересчитываются только при смене снимка данных.
  const derived = useMemo(() => {
    const liveReady = [result.subs, result.capacity, result.codex, result.gemini, result.kimi, result.glm];
    if (isLoading && liveReady.every((value) => value === undefined)) return null;
    const { subs, capacity, codex, gemini, kimi, glm, tripo3d, suno } = result;
    const list = subs?.subs ?? [];
    const suspect = list.filter((item) => item.auth_state === "suspect").length;
    const subsDown = availability.subs === "error";
    const claudeCapacityLoading = availability.capacity === "loading";
    const claudeCapacityDown = availability.capacity === "error";
    const claudeCalibrationPending = Number(capacity?.calibration_delivery?.pending_events ?? 0);
    const claudeCalibrationDropped = Number(capacity?.calibration_delivery?.dropped_events ?? 0);
    const claudeCalibrationStorageBad = capacity?.calibration_delivery?.persistence_ok === false
      || capacity?.calibration_authority_available === false;

    const gptLoading = availability.codex === "loading";
    const gptDown = availability.codex === "error";
    const gptOff = Boolean(codex && codex.enabled === false);
    const homes = codex?.homes ?? [];
    const gptAuthBad = homes.filter((h) => !h.auth_ok).length;
    const gptProcDown = homes.filter((h) => !h.process_live).length;

    const geminiLoading = availability.gemini === "loading";
    const geminiDown = availability.gemini === "error";
    const geminiOff = Boolean(gemini && gemini.enabled === false);
    const geminiProfiles = gemini?.profiles ?? [];
    const geminiEmpty = availability.gemini === "ready" && !geminiOff && !geminiProfiles.length;
    const geminiUnavailable =
      !geminiDown && !geminiOff && geminiProfiles.length > 0 && Number(gemini?.available || 0) === 0;
    const geminiAuthBad = geminiProfiles.filter((profile) => !profile.authenticated).length;
    const geminiMissing = Number(gemini?.usage_metadata_missing || 0);
    const geminiCalibrationPending = Number(gemini?.calibration_delivery?.pending_events ?? 0);
    const geminiCalibrationDropped = Number(gemini?.calibration_delivery?.dropped_events ?? 0);
    const geminiCalibrationStorageBad = gemini?.calibration_authority_available === false
      || gemini?.calibration_delivery?.persistence_ok === false;

    const kimiLoading = availability.kimi === "loading";
    const kimiDown = availability.kimi === "error";
    const kimiOff = Boolean(kimi && kimi.enabled === false);
    const kimiProfiles = kimi?.profiles ?? [];
    const kimiEmpty = availability.kimi === "ready" && !kimiOff && !kimiProfiles.length;
    const kimiUnavailable =
      !kimiDown && !kimiOff && kimiProfiles.length > 0 && Number(kimi?.fleet?.available_profiles ?? 0) === 0;
    const kimiDeliveryPending = Number(kimi?.delivery?.pending_events ?? 0);
    const kimiDeliveryDropped = Number(kimi?.delivery?.dropped_events ?? 0);
    const kimiDeliveryBad = kimi?.delivery?.persistence_ok === false;
    const kimiCalibrationStorageBad = kimi?.calibration_authority_available === false || kimiDeliveryBad;

    const glmLoading = availability.glm === "loading";
    const glmDown = availability.glm === "error";
    const glmOff = Boolean(glm && glm.enabled === false);
    const glmProfiles = glm?.profiles ?? [];
    const glmEmpty = availability.glm === "ready" && !glmOff && !glmProfiles.length;
    const glmUnavailable =
      !glmDown && !glmOff && glmProfiles.length > 0 && Number(glm?.fleet?.available_profiles ?? 0) === 0;
    const glmDeliveryPending = Number(glm?.delivery?.pending_events ?? 0);
    const glmDeliveryDropped = Number(glm?.delivery?.dropped_events ?? 0);
    const glmDeliveryBad = glm?.delivery?.persistence_ok === false;

    const tripo3dLoading = availability.tripo3d === "loading";
    const tripo3dDown = availability.tripo3d === "error";
    const tripo3dOff = Boolean(tripo3d && tripo3d.enabled === false);
    const tripo3dProfiles = tripo3d?.profiles ?? [];
    const tripo3dEmpty = availability.tripo3d === "ready" && !tripo3dOff && !tripo3dProfiles.length;
    const tripo3dUnavailable =
      !tripo3dDown && !tripo3dOff && tripo3dProfiles.length > 0 && Number(tripo3d?.fleet?.available_profiles ?? 0) === 0;
    const tripo3dDeliveryPending = Number(tripo3d?.delivery?.pending_events ?? 0);
    const tripo3dDeliveryDropped = Number(tripo3d?.delivery?.dropped_events ?? 0);
    const tripo3dDeliveryBad = tripo3d?.delivery?.persistence_ok === false;
    const tripo3dCalibrationStorageBad = tripo3d?.calibration_authority_available === false || tripo3dDeliveryBad;

    const sunoLoading = availability.suno === "loading";
    const sunoDown = availability.suno === "error";
    const sunoOff = Boolean(suno && suno.enabled === false);
    const sunoProfiles = suno?.profiles ?? [];
    const sunoEmpty = availability.suno === "ready" && !sunoOff && !sunoProfiles.length;
    const sunoUnavailable =
      !sunoDown && !sunoOff && sunoProfiles.length > 0 && Number(suno?.fleet?.available_profiles ?? 0) === 0;
    const sunoDeliveryPending = Number(suno?.delivery?.pending_events ?? 0);
    const sunoDeliveryDropped = Number(suno?.delivery?.dropped_events ?? 0);
    const sunoDeliveryBad = suno?.delivery?.persistence_ok === false;
    const sunoCalibrationStorageBad = suno?.calibration_authority_available === false || sunoDeliveryBad;

    const fleetTotal = list.length + (gptOff ? 0 : homes.length) + (geminiOff ? 0 : geminiProfiles.length)
      + (kimiOff ? 0 : kimiProfiles.length) + (glmOff ? 0 : glmProfiles.length)
      + (tripo3dOff ? 0 : tripo3dProfiles.length) + (sunoOff ? 0 : sunoProfiles.length);
    const fleetWarn = Boolean(
      claudeCapacityDown || claudeCalibrationPending || claudeCalibrationDropped
        || claudeCalibrationStorageBad || gptDown || geminiDown || geminiEmpty
        || geminiUnavailable || geminiAuthBad || geminiMissing || geminiCalibrationPending
        || geminiCalibrationDropped || geminiCalibrationStorageBad || kimiDown || kimiEmpty
        || kimiUnavailable || kimiDeliveryPending || kimiDeliveryDropped || kimiCalibrationStorageBad
        || glmDown || glmEmpty || glmUnavailable || glmDeliveryPending || glmDeliveryDropped
        || glmDeliveryBad
        || tripo3dDown || tripo3dEmpty || tripo3dUnavailable || tripo3dDeliveryPending
        || tripo3dDeliveryDropped || tripo3dCalibrationStorageBad
        || sunoDown || sunoEmpty || sunoUnavailable || sunoDeliveryPending
        || sunoDeliveryDropped || sunoCalibrationStorageBad,
    );

    return {
      subs,
      capacity,
      list,
      suspect,
      subsDown,
      claudeCapacityLoading,
      claudeCapacityDown,
      claudeCalibrationPending,
      claudeCalibrationDropped,
      claudeCalibrationStorageBad,
      codex,
      homes,
      gptLoading,
      gptDown,
      gptOff,
      gptAuthBad,
      gptProcDown,
      gemini,
      geminiLoading,
      geminiDown,
      geminiOff,
      geminiProfiles,
      geminiEmpty,
      geminiUnavailable,
      geminiAuthBad,
      geminiMissing,
      geminiCalibrationPending,
      geminiCalibrationDropped,
      geminiCalibrationStorageBad,
      kimi,
      kimiLoading,
      kimiDown,
      kimiOff,
      kimiProfiles,
      kimiEmpty,
      kimiUnavailable,
      kimiDeliveryPending,
      kimiDeliveryDropped,
      kimiDeliveryBad,
      glm,
      glmLoading,
      glmDown,
      glmOff,
      glmProfiles,
      glmEmpty,
      glmUnavailable,
      glmDeliveryPending,
      glmDeliveryDropped,
      glmDeliveryBad,
      tripo3d,
      tripo3dLoading,
      tripo3dDown,
      tripo3dOff,
      tripo3dProfiles,
      tripo3dEmpty,
      tripo3dUnavailable,
      tripo3dDeliveryPending,
      tripo3dDeliveryDropped,
      tripo3dDeliveryBad,
      tripo3dCalibrationStorageBad,
      suno,
      sunoLoading,
      sunoDown,
      sunoOff,
      sunoProfiles,
      sunoEmpty,
      sunoUnavailable,
      sunoDeliveryPending,
      sunoDeliveryDropped,
      sunoDeliveryBad,
      sunoCalibrationStorageBad,
      fleetTotal,
      fleetWarn,
    };
  }, [availability, result, isLoading]);

  if (!derived) {
    return (
      <>
        <PageHead title="Подписки" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const banner = resolveBanner({
    suspect: derived.suspect,
    subsDown: derived.subsDown,
    gptDown: derived.gptDown,
    geminiDown: derived.geminiDown,
    geminiEmpty: derived.geminiEmpty,
    gptAuthBad: derived.gptAuthBad,
    gptProcDown: derived.gptProcDown,
    geminiAuthBad: derived.geminiAuthBad,
    geminiUnavailable: derived.geminiUnavailable,
    geminiMissing: derived.geminiMissing,
    kimiDown: derived.kimiDown,
    kimiEmpty: derived.kimiEmpty,
    kimiUnavailable: derived.kimiUnavailable,
    glmDown: derived.glmDown,
    glmEmpty: derived.glmEmpty,
    glmUnavailable: derived.glmUnavailable,
    tripo3dDown: derived.tripo3dDown,
    tripo3dEmpty: derived.tripo3dEmpty,
    tripo3dUnavailable: derived.tripo3dUnavailable,
    sunoDown: derived.sunoDown,
    sunoEmpty: derived.sunoEmpty,
    sunoUnavailable: derived.sunoUnavailable,
    claudeCount: derived.list.length,
    gptSummary: derived.gptOff ? "выкл." : derived.homes.length,
    geminiSummary: derived.geminiOff ? "выкл." : derived.geminiProfiles.length,
    kimiSummary: derived.kimiOff ? "выкл." : derived.kimiProfiles.length,
    glmSummary: derived.glmOff ? "выкл." : derived.glmProfiles.length,
    tripo3dSummary: derived.tripo3dOff ? "выкл." : derived.tripo3dProfiles.length,
    sunoSummary: derived.sunoOff ? "выкл." : derived.sunoProfiles.length,
    updatedAt: formatDate(nowMs, true),
  });

  return (
    <>
      <PageHead
        title="Подписки"
        sub={`остаток API-$, окна и экономика семи пулов · обновлено ${formatDate(nowMs, true)}`}
        badge={
          <Pill kind={derived.fleetWarn ? "warn" : "ok"}>
            {count(derived.fleetTotal, "подписка", "подписки", "подписок")}
          </Pill>
        }
      />

      <FleetCapacityOverview
        claude={derived.claudeCapacityDown ? null : derived.capacity}
        gpt={derived.gptDown ? null : derived.codex}
        gemini={derived.geminiDown ? null : derived.gemini}
        kimi={derived.kimiDown ? null : derived.kimi}
        glm={derived.glmDown ? null : derived.glm}
        tripo3d={derived.tripo3dDown ? null : derived.tripo3d}
        suno={derived.sunoDown ? null : derived.suno}
        nowMs={nowMs}
      />

      {banner.kind === "ok" ? null : (
        <Banner kind={banner.kind} title={banner.title}>
          {banner.sub}
        </Banner>
      )}

      <div className="subscription-provider-stack">
        <section className="subscription-provider-group provider-group-claude">
          <header className="subscription-provider-head">
            <div><span>01 · Claude</span><h2>Аккаунты и окна</h2></div>
            <b>API-$ · 5ч / 7д</b>
          </header>
          {derived.claudeCapacityLoading ? (
            <div className="tcard"><div className="empty">/capacity загружается</div></div>
          ) : derived.claudeCapacityDown ? (
            <div className="tcard"><div className="empty">/capacity не отвечает</div></div>
          ) : (
            <ClaudeCapacityBoard response={derived.capacity!} />
          )}
        </section>

        <section className="subscription-provider-group provider-group-gpt">
          <header className="subscription-provider-head">
            <div><span>02 · GPT</span><h2>Аккаунты и окна</h2></div>
            <b>credits · API-$ · модели</b>
          </header>
          {derived.gptLoading ? (
            <div className="tcard"><div className="empty">Данные GPT загружаются</div></div>
          ) : derived.gptDown || derived.gptOff ? (
            <div className="tcard">
              <div className="empty">
                {derived.gptDown ? "OpenAI-runtime не отвечает" : "Codex-контур выключен"}
              </div>
            </div>
          ) : (
            <CodexCapacityBoard response={derived.codex!} nowMs={nowMs} showSummary={false} />
          )}
        </section>

        <section className="subscription-provider-group provider-group-gemini">
          <header className="subscription-provider-head">
            <div><span>03 · Gemini</span><h2>Аккаунты и окна</h2></div>
            <b>API-$ · quota · модели</b>
          </header>
          {derived.geminiLoading ? (
            <div className="tcard"><div className="empty">Данные Gemini загружаются</div></div>
          ) : derived.geminiDown || derived.geminiOff ? (
            <div className="tcard">
              <div className="empty">
                {derived.geminiDown ? "Gemini runtime не отвечает" : "Gemini-контур выключен"}
              </div>
            </div>
          ) : (
            <GeminiCapacityBoard response={derived.gemini!} nowMs={nowMs} showSummary={false} />
          )}
        </section>

        <section className="subscription-provider-group provider-group-kimi">
          <header className="subscription-provider-head">
            <div><span>04 · KIMI</span><h2>Аккаунты и окна</h2></div>
            <b>API-$ · quota · окна</b>
          </header>
          {derived.kimiLoading ? (
            <div className="tcard"><div className="empty">Данные KIMI загружаются</div></div>
          ) : derived.kimiDown || derived.kimiOff ? (
            <div className="tcard">
              <div className="empty">
                {derived.kimiDown ? "KIMI runtime не отвечает" : "KIMI-контур выключен"}
              </div>
            </div>
          ) : (
            <KimiCapacityBoard response={derived.kimi!} nowMs={nowMs} showSummary={false} />
          )}
        </section>

        <section className="subscription-provider-group provider-group-glm">
          <header className="subscription-provider-head">
            <div><span>05 · GLM</span><h2>Аккаунты и окна</h2></div>
            <b>API-$ · quota · окна</b>
          </header>
          {derived.glmLoading ? (
            <div className="tcard"><div className="empty">Данные GLM загружаются</div></div>
          ) : derived.glmDown || derived.glmOff ? (
            <div className="tcard">
              <div className="empty">
                {derived.glmDown ? "GLM runtime не отвечает" : "GLM-контур выключен"}
              </div>
            </div>
          ) : (
            <GlmCapacityBoard response={derived.glm!} nowMs={nowMs} showSummary={false} />
          )}
        </section>

        <section className="subscription-provider-group provider-group-tripo3d">
          <header className="subscription-provider-head">
            <div><span>06 · Tripo3D</span><h2>Аккаунты и баланс</h2></div>
            <b>API-$ · баланс · prepaid</b>
          </header>
          {derived.tripo3dLoading ? (
            <div className="tcard"><div className="empty">Данные Tripo3D загружаются</div></div>
          ) : derived.tripo3dDown || derived.tripo3dOff ? (
            <div className="tcard">
              <div className="empty">
                {derived.tripo3dDown ? "Tripo3D runtime не отвечает" : "Tripo3D-контур выключен"}
              </div>
            </div>
          ) : (
            <Tripo3dCapacityBoard response={derived.tripo3d!} nowMs={nowMs} showSummary={false} />
          )}
        </section>

        <section className="subscription-provider-group provider-group-suno">
          <header className="subscription-provider-head">
            <div><span>07 · Suno</span><h2>Аккаунты и окна</h2></div>
            <b>API-$ · кредиты · месяц</b>
          </header>
          {derived.sunoLoading ? (
            <div className="tcard"><div className="empty">Данные Suno загружаются</div></div>
          ) : derived.sunoDown || derived.sunoOff ? (
            <div className="tcard">
              <div className="empty">
                {derived.sunoDown ? "Suno runtime не отвечает" : "Suno-контур выключен"}
              </div>
            </div>
          ) : (
            <SunoCapacityBoard response={derived.suno!} nowMs={nowMs} showSummary={false} />
          )}
        </section>
      </div>

      <footer>
        Realtime по изменениям провайдеров + контроль свежести каждые 30 с · nanoUSD · почта маскирована
      </footer>
    </>
  );
}

const DORMANT_FLEET: { enabled: false; profiles: [] } = { enabled: false, profiles: [] };

export const SUBSCRIPTION_LIVE_PATHS = {
  subs: "/subs",
  capacity: compactCapacityUrl(),
  codex: compactCodexSubsUrl(),
  gemini: compactGeminiSubsUrl(),
  kimi: compactKimiSubsUrl(),
  glm: compactGlmSubsUrl(),
} as const;

export default function SubsPage() {
  const live = useResources<Omit<SubsData, "tripo3d" | "suno">>(SUBSCRIPTION_LIVE_PATHS);
  const state: SubsPageState = {
    data: {
      ...live.data,
      tripo3d: DORMANT_FLEET,
      suno: DORMANT_FLEET,
    },
    availability: {
      ...live.availability,
      tripo3d: "ready",
      suno: "ready",
    },
    isLoading: live.isLoading,
    updatedAt: live.updatedAt,
  };
  return <SubscriptionsView state={state} />;
}
