"use client";

// Подписки — порт 1:1 функции subscriptions() из crates/server/src/admin-panel.js.
// Три флота на одной странице: Claude OAuth (/subs + live-ёмкость /capacity),
// GPT/Codex homes (/codex-subs) и Gemini-профили (/gemini-subs). Опрос 10 с,
// как в легаси (scheduleRefresh: tab==='subs' → 10000).
import { useMemo } from "react";
import { api } from "@/lib/api";
import { usePoll } from "@/lib/usePoll";
import { count, formatDate } from "@/lib/format";
import { Banner, LoadingGrid, PageHead, Pill, SectionHeader } from "@/components/ui";
import { ClaudeCapacityBoard } from "./claude-capacity-board";
import { CodexCapacityBoard } from "./codex-capacity-board";
import { GeminiCapacityBoard } from "./gemini-capacity-board";
import { resolveBanner } from "./logic";
import type {
  CapacityResponse,
  CodexSubsResponse,
  GeminiSubsResponse,
  SubsResponse,
} from "./types";

const POLL_INTERVAL_MS = 10_000;

interface SubsData {
  subs: SubsResponse | null;
  capacity: CapacityResponse | null;
  codex: CodexSubsResponse | null;
  gemini: GeminiSubsResponse | null;
  /** Момент снимка (мс): все отсчёты «до сброса» считаются от него — чистый рендер. */
  nowMs: number;
}

// Все четыре источника параллельно; падение любого → null, остальные флоты
// продолжают рендериться независимо (конвенция деградации admin-panel.js).
async function loadSubs(): Promise<SubsData> {
  const [subs, capacity, codex, gemini] = await Promise.all([
    api<SubsResponse>("/subs").catch(() => null),
    api<CapacityResponse>("/capacity").catch(() => null),
    api<CodexSubsResponse>("/codex-subs").catch(() => null),
    api<GeminiSubsResponse>("/gemini-subs").catch(() => null),
  ]);
  return { subs, capacity, codex, gemini, nowMs: Date.now() };
}

export default function SubsPage() {
  const { data: result } = usePoll("subs", loadSubs, { interval: POLL_INTERVAL_MS });

  // Все производные флотов пересчитываются только при смене снимка данных.
  const derived = useMemo(() => {
    if (!result) return null;
    const { subs, capacity, codex, gemini } = result;
    const list = subs?.subs ?? [];
    const dead = list.filter((item) => item.auth_state === "dead").length;
    const suspect = list.filter((item) => item.auth_state === "suspect").length;
    const subsDown = subs === null;
    const claudeCapacityDown = capacity === null;

    const gptDown = codex === null;
    const gptOff = Boolean(codex && codex.enabled === false);
    const homes = codex?.homes ?? [];
    const gptAuthBad = homes.filter((h) => !h.auth_ok).length;
    const gptProcDown = homes.filter((h) => !h.process_live).length;

    const geminiDown = gemini === null;
    const geminiOff = Boolean(gemini && gemini.enabled === false);
    const geminiProfiles = gemini?.profiles ?? [];
    const geminiEmpty = !geminiDown && !geminiOff && !geminiProfiles.length;
    const geminiUnavailable =
      !geminiDown && !geminiOff && geminiProfiles.length > 0 && Number(gemini?.available || 0) === 0;
    const geminiAuthBad = geminiProfiles.filter((profile) => !profile.authenticated).length;
    const geminiMissing = Number(gemini?.usage_metadata_missing || 0);

    const fleetTotal = list.length + (gptOff ? 0 : homes.length) + (geminiOff ? 0 : geminiProfiles.length);
    const fleetWarn = Boolean(
      dead || claudeCapacityDown || gptDown || geminiDown || geminiEmpty || geminiUnavailable || geminiAuthBad || geminiMissing,
    );

    return {
      subs,
      capacity,
      list,
      dead,
      suspect,
      subsDown,
      claudeCapacityDown,
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
      fleetTotal,
      fleetWarn,
    };
  }, [result]);

  if (!result || !derived) {
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
    claudeCount: derived.list.length,
    gptSummary: derived.gptOff ? "выкл." : derived.homes.length,
    geminiSummary: derived.geminiOff ? "выкл." : derived.geminiProfiles.length,
    updatedAt: formatDate(result.nowMs, true),
  });

  return (
    <>
      <PageHead
        title="Подписки"
        sub="Claude, GPT и Gemini: ёмкость, окна, quota и тарифы"
        badge={
          <Pill kind={derived.fleetWarn ? "warn" : "ok"}>
            {count(derived.fleetTotal, "подписка", "подписки", "подписок")}
          </Pill>
        }
      />

      <Banner kind={banner.kind} title={banner.title}>
        {banner.sub}
      </Banner>

      <SectionHeader
        title="Claude · ёмкость"
        sub="API-$, доступные токены и тариф по моделям"
      />
      <div style={{ marginTop: 12 }}>
        {derived.claudeCapacityDown ? (
          <div className="tcard"><div className="empty" style={{ padding: 26 }}>/capacity не отвечает</div></div>
        ) : (
          <ClaudeCapacityBoard response={derived.capacity!} />
        )}
      </div>

      <SectionHeader
        title="GPT · ёмкость"
        sub="Credits, доступные токены и выгодность моделей"
      />
      <div style={{ marginTop: 12 }}>
        {derived.gptDown || derived.gptOff ? (
          <div className="tcard">
            <div className="empty" style={{ padding: 26 }}>
              {derived.gptDown
                ? "OpenAI-runtime недоступен — /codex-subs не отвечает"
                : "Codex-контур выключен на этом runtime"}
            </div>
          </div>
        ) : (
          <CodexCapacityBoard response={derived.codex!} nowMs={result.nowMs} />
        )}
      </div>

      <SectionHeader
        title="Gemini · ёмкость"
        sub="Официальная quota, workload-$ и тариф по моделям"
      />
      <div style={{ marginTop: 12 }}>
        {derived.geminiDown || derived.geminiOff ? (
          <div className="tcard">
            <div className="empty" style={{ padding: 26 }}>
              {derived.geminiDown
                ? "Gemini runtime недоступен — /gemini-subs не отвечает"
                : "Gemini-контур выключен на этом runtime"}
            </div>
          </div>
        ) : (
          <GeminiCapacityBoard response={derived.gemini!} nowMs={result.nowMs} />
        )}
      </div>

      <footer>
        Обновление 10с · деньги считаются в nanoUSD · почта маскирована · Gemini «—» означает, что Google не публикует amount.
      </footer>
    </>
  );
}
