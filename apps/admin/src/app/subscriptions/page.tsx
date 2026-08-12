"use client";

// Подписки — порт 1:1 функции subscriptions() из crates/server/src/admin-panel.js.
// Пять флотов на одной странице: Claude OAuth (/subs + live-ёмкость /capacity),
// GPT/Codex homes (/codex-subs), Gemini-профили (/gemini-subs), KIMI-профили
// (/kimi-subs) и GLM-профили (/glm-subs). Каждый provider-feed инвалидирует только
// затронутые URL; интервальных запросов нет.
import { useMemo } from "react";
import { useResources } from "@/lib/resources";
import { count, formatDate } from "@/lib/format";
import { Banner, LoadingGrid, PageHead, Pill } from "@/components/ui";
import { ClaudeCapacityBoard } from "./claude-capacity-board";
import { CodexCapacityBoard } from "./codex-capacity-board";
import { FleetCapacityOverview } from "./fleet-capacity-overview";
import { GeminiCapacityBoard } from "./gemini-capacity-board";
import { GlmCapacityBoard } from "./glm-capacity-board";
import { KimiCapacityBoard } from "./kimi-capacity-board";
import { resolveBanner } from "./logic";
import type {
  CapacityResponse,
  CodexSubsResponse,
  GeminiSubsResponse,
  GlmSubsResponse,
  KimiSubsResponse,
  SubsResponse,
} from "./types";

interface SubsData {
  subs: SubsResponse;
  capacity: CapacityResponse;
  codex: CodexSubsResponse;
  gemini: GeminiSubsResponse;
  kimi: KimiSubsResponse;
  glm: GlmSubsResponse;
}

export default function SubsPage() {
  const { data: result, availability, isLoading, updatedAt: nowMs } = useResources<SubsData>({
    subs: "/subs",
    capacity: "/capacity",
    codex: "/codex-subs",
    gemini: "/gemini-subs",
    kimi: "/kimi-subs",
    glm: "/glm-subs",
  });

  // Все производные флотов пересчитываются только при смене снимка данных.
  const derived = useMemo(() => {
    if (isLoading && Object.values(result).every((value) => value === undefined)) return null;
    const { subs, capacity, codex, gemini, kimi, glm } = result;
    const list = subs?.subs ?? [];
    const dead = list.filter((item) => item.auth_state === "dead").length;
    const suspect = list.filter((item) => item.auth_state === "suspect").length;
    const subsDown = availability.subs === "error";
    const claudeCapacityDown = availability.capacity === "error";
    const claudeCalibrationPending = Number(capacity?.calibration_delivery?.pending_events ?? 0);
    const claudeCalibrationDropped = Number(capacity?.calibration_delivery?.dropped_events ?? 0);
    const claudeCalibrationStorageBad = capacity?.calibration_delivery?.persistence_ok === false
      || capacity?.calibration_authority_available === false;

    const gptDown = availability.codex === "error";
    const gptOff = Boolean(codex && codex.enabled === false);
    const homes = codex?.homes ?? [];
    const gptAuthBad = homes.filter((h) => !h.auth_ok).length;
    const gptProcDown = homes.filter((h) => !h.process_live).length;

    const geminiDown = availability.gemini === "error";
    const geminiOff = Boolean(gemini && gemini.enabled === false);
    const geminiProfiles = gemini?.profiles ?? [];
    const geminiEmpty = !geminiDown && !geminiOff && !geminiProfiles.length;
    const geminiUnavailable =
      !geminiDown && !geminiOff && geminiProfiles.length > 0 && Number(gemini?.available || 0) === 0;
    const geminiAuthBad = geminiProfiles.filter((profile) => !profile.authenticated).length;
    const geminiMissing = Number(gemini?.usage_metadata_missing || 0);
    const geminiCalibrationPending = Number(gemini?.calibration_delivery?.pending_events ?? 0);
    const geminiCalibrationDropped = Number(gemini?.calibration_delivery?.dropped_events ?? 0);
    const geminiCalibrationStorageBad = gemini?.calibration_authority_available === false
      || gemini?.calibration_delivery?.persistence_ok === false;

    const kimiDown = availability.kimi === "error";
    const kimiOff = Boolean(kimi && kimi.enabled === false);
    const kimiProfiles = kimi?.profiles ?? [];
    const kimiEmpty = !kimiDown && !kimiOff && !kimiProfiles.length;
    const kimiUnavailable =
      !kimiDown && !kimiOff && kimiProfiles.length > 0 && Number(kimi?.fleet?.available_profiles ?? 0) === 0;
    const kimiDeliveryPending = Number(kimi?.delivery?.pending_events ?? 0);
    const kimiDeliveryDropped = Number(kimi?.delivery?.dropped_events ?? 0);
    const kimiDeliveryBad = kimi?.delivery?.persistence_ok === false;

    const glmDown = availability.glm === "error";
    const glmOff = Boolean(glm && glm.enabled === false);
    const glmProfiles = glm?.profiles ?? [];
    const glmEmpty = !glmDown && !glmOff && !glmProfiles.length;
    const glmUnavailable =
      !glmDown && !glmOff && glmProfiles.length > 0 && Number(glm?.fleet?.available_profiles ?? 0) === 0;
    const glmDeliveryPending = Number(glm?.delivery?.pending_events ?? 0);
    const glmDeliveryDropped = Number(glm?.delivery?.dropped_events ?? 0);
    const glmDeliveryBad = glm?.delivery?.persistence_ok === false;

    const fleetTotal = list.length + (gptOff ? 0 : homes.length) + (geminiOff ? 0 : geminiProfiles.length)
      + (kimiOff ? 0 : kimiProfiles.length) + (glmOff ? 0 : glmProfiles.length);
    const fleetWarn = Boolean(
      dead || claudeCapacityDown || claudeCalibrationPending || claudeCalibrationDropped
        || claudeCalibrationStorageBad || gptDown || geminiDown || geminiEmpty
        || geminiUnavailable || geminiAuthBad || geminiMissing || geminiCalibrationPending
        || geminiCalibrationDropped || geminiCalibrationStorageBad || kimiDown || kimiEmpty
        || kimiUnavailable || kimiDeliveryPending || kimiDeliveryDropped || kimiDeliveryBad
        || glmDown || glmEmpty || glmUnavailable || glmDeliveryPending || glmDeliveryDropped
        || glmDeliveryBad,
    );

    return {
      subs,
      capacity,
      list,
      dead,
      suspect,
      subsDown,
      claudeCapacityDown,
      claudeCalibrationPending,
      claudeCalibrationDropped,
      claudeCalibrationStorageBad,
      codex,
      homes,
      gptDown,
      gptOff,
      gptAuthBad,
      gptProcDown,
      gemini,
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
      kimiDown,
      kimiOff,
      kimiProfiles,
      kimiEmpty,
      kimiUnavailable,
      kimiDeliveryPending,
      kimiDeliveryDropped,
      kimiDeliveryBad,
      glm,
      glmDown,
      glmOff,
      glmProfiles,
      glmEmpty,
      glmUnavailable,
      glmDeliveryPending,
      glmDeliveryDropped,
      glmDeliveryBad,
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
    dead: derived.dead,
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
    claudeCount: derived.list.length,
    gptSummary: derived.gptOff ? "выкл." : derived.homes.length,
    geminiSummary: derived.geminiOff ? "выкл." : derived.geminiProfiles.length,
    kimiSummary: derived.kimiOff ? "выкл." : derived.kimiProfiles.length,
    glmSummary: derived.glmOff ? "выкл." : derived.glmProfiles.length,
    updatedAt: formatDate(nowMs, true),
  });

  return (
    <>
      <PageHead
        title="Подписки"
        sub="остаток API-$, окна и экономика пяти пулов"
        badge={
          <Pill kind={derived.fleetWarn ? "warn" : "ok"}>
            {count(derived.fleetTotal, "подписка", "подписки", "подписок")}
          </Pill>
        }
      />

      <FleetCapacityOverview
        claude={derived.capacity ?? null}
        gpt={derived.codex ?? null}
        gemini={derived.gemini ?? null}
        kimi={derived.kimi ?? null}
        glm={derived.glm ?? null}
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
          {derived.claudeCapacityDown ? (
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
          {derived.gptDown || derived.gptOff ? (
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
          {derived.geminiDown || derived.geminiOff ? (
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
          {derived.kimiDown || derived.kimiOff ? (
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
          {derived.glmDown || derived.glmOff ? (
            <div className="tcard">
              <div className="empty">
                {derived.glmDown ? "GLM runtime не отвечает" : "GLM-контур выключен"}
              </div>
            </div>
          ) : (
            <GlmCapacityBoard response={derived.glm!} nowMs={nowMs} showSummary={false} />
          )}
        </section>
      </div>

      <footer>
        Realtime по изменениям провайдеров · nanoUSD · почта маскирована
      </footer>
    </>
  );
}
