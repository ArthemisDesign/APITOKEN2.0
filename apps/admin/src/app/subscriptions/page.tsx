"use client";

// Подписки — порт 1:1 функции subscriptions() из crates/server/src/admin-panel.js.
// Три флота на одной странице: Claude OAuth (/subs + live-ёмкость /capacity),
// GPT/Codex homes (/codex-subs) и Gemini-профили (/gemini-subs). Опрос 10 с,
// как в легаси (scheduleRefresh: tab==='subs' → 10000).
import { useMemo, type ReactElement } from "react";
import { api } from "@/lib/api";
import { usePoll } from "@/lib/usePoll";
import { count, formatDate, money } from "@/lib/format";
import { Banner, CardGrid, LoadingGrid, PageHead, Pill, SectionHeader, StatCard } from "@/components/ui";
import { useSpendStatsModal } from "@/components/spend-stats-modal";
import { ClaudeTable, GeminiModelDetails, GeminiTable, TransportDetails } from "./components";
import { CodexCapacityBoard } from "./codex-capacity-board";
import { resolveBanner } from "./logic";
import type {
  CapacityResponse,
  CapacitySub,
  CodexSubsResponse,
  GeminiSubsResponse,
  GeminiWindowTotal,
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

// Gemini workload-карточка (5ч/7д): measured → remaining из cap + envelope,
// иначе «ждём Δused» — прайор не подставляется, как в легаси.
function GeminiBudgetCard({
  label,
  item,
  down,
}: {
  label: string;
  item: GeminiWindowTotal | undefined;
  down: boolean;
}): ReactElement {
  const measured = item != null && item.cap_usd != null;
  const range =
    measured && item.low_usd != null
      ? money(item.low_usd) + "–" + (item.high_usd == null ? "∞" : money(item.high_usd))
      : "—";
  return (
    <StatCard
      label={label}
      value={down ? "—" : measured ? money(item.remaining_usd) : "ждём Δused"}
      hint={
        measured
          ? `realized blend из ${money(item.cap_usd)} · workload envelope ${range} · ${Number(item.measured_profiles || 0)} профилей`
          : "первый снимок и движение цензурируются, прайора нет"
      }
    />
  );
}

export default function SubsPage() {
  const { data: result } = usePoll("subs", loadSubs, { interval: POLL_INTERVAL_MS });
  const { openSpendStats, spendStatsModal } = useSpendStatsModal();

  // Все производные флотов пересчитываются только при смене снимка данных.
  const derived = useMemo(() => {
    if (!result) return null;
    const { subs, capacity, codex, gemini } = result;
    // Claude: lifecycle (/subs) + live ёмкость (/capacity) по маскированному email.
    const list = subs?.subs ?? [];
    const liveByEmail: Record<string, CapacitySub> = {};
    for (const item of capacity?.per_sub ?? []) liveByEmail[item.email ?? ""] = item;
    const dead = list.filter((item) => item.auth_state === "dead").length;
    const suspect = list.filter((item) => item.auth_state === "suspect").length;
    const cooling = (capacity?.per_sub ?? []).filter((item) => item.cooling).length;
    const subsDown = subs === null;

    const gptDown = codex === null;
    const gptOff = Boolean(codex && codex.enabled === false);
    const homes = codex?.homes ?? [];
    const gptAuthBad = homes.filter((h) => !h.auth_ok).length;
    const gptProcDown = homes.filter((h) => !h.process_live).length;

    const geminiDown = gemini === null;
    const geminiOff = Boolean(gemini && gemini.enabled === false);
    const geminiProfiles = gemini?.profiles ?? [];
    const geminiModels = gemini?.models ?? [];
    const geminiEmpty = !geminiDown && !geminiOff && !geminiProfiles.length;
    const geminiUnavailable =
      !geminiDown && !geminiOff && geminiProfiles.length > 0 && Number(gemini?.available || 0) === 0;
    const geminiAuthBad = geminiProfiles.filter((profile) => !profile.authenticated).length;
    const geminiMissing = Number(gemini?.usage_metadata_missing || 0);
    const geminiAffinity = gemini?.affinity ?? {};
    const geminiFailures = gemini?.failures ?? {};
    const geminiFailTotal =
      Number(geminiFailures.transport || 0) +
      Number(geminiFailures.backend || 0) +
      Number(geminiFailures.malformed || 0) +
      Number(geminiFailures.stream_start || 0);
    const geminiTotals = Array.isArray(gemini?.window_totals) ? gemini.window_totals : [];
    const geminiFive = geminiTotals.find((item) => Number(item.window_minutes) === 300);
    const geminiWeek = geminiTotals.find((item) => Number(item.window_minutes) === 10080);
    const geminiSpend = geminiProfiles.reduce((sum, profile) => sum + (Number(profile.spend_usd_total) || 0), 0);

    const avail7d = capacity?.available_usd?.next_7d ?? 0;
    const avail1h = capacity?.available_usd?.next_1h;
    const avail5h = capacity?.available_usd?.next_5h;
    const avail1d = capacity?.available_usd?.next_1d;
    const routableCaps = (capacity?.per_sub ?? []).filter((item) => item.routable);
    const avgUtil7d = routableCaps.length
      ? Math.round((routableCaps.reduce((sum, item) => sum + (Number(item.util7d) || 0), 0) / routableCaps.length) * 100) + "%"
      : "—";

    const fleetTotal = list.length + (gptOff ? 0 : homes.length) + (geminiOff ? 0 : geminiProfiles.length);
    const fleetWarn = Boolean(
      dead || gptDown || geminiDown || geminiEmpty || geminiUnavailable || geminiAuthBad || geminiMissing,
    );

    return {
      subs,
      list,
      liveByEmail,
      dead,
      suspect,
      cooling,
      subsDown,
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
      geminiModels,
      geminiEmpty,
      geminiUnavailable,
      geminiAuthBad,
      geminiMissing,
      geminiAffinity,
      geminiFailures,
      geminiFailTotal,
      geminiFive,
      geminiWeek,
      geminiSpend,
      avail7d,
      avail1h,
      avail5h,
      avail1d,
      avgUtil7d,
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
        sub="Claude, GPT и Gemini: здоровье, окна, quota и transport"
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
        title="Claude"
        sub={`Anthropic · OAuth-флот · замена через ${derived.subs?.lifetime_days ?? "—"}д от добавления`}
      />
      <CardGrid>
        <StatCard
          label="Claude подписки"
          value={derived.list.length}
          hint={`${derived.list.length - derived.dead - derived.suspect} здоровы · ${derived.cooling} cooling`}
        />
        <StatCard
          label="Claude · доступно 7д"
          value={money(derived.avail7d)}
          hint={`1ч ${derived.avail1h != null ? money(derived.avail1h) : "—"} · 5ч ${derived.avail5h != null ? money(derived.avail5h) : "—"} · 1д ${derived.avail1d != null ? money(derived.avail1d) : "—"}`}
        />
        <StatCard label="утилизация 7д средняя" value={derived.avgUtil7d} hint="по routable подпискам" />
        <StatCard
          label="dead / suspect"
          value={`${derived.dead} / ${derived.suspect}`}
          hint={derived.dead ? "нужна замена токена" : derived.suspect ? "корроборация probe идёт" : "флот чист"}
        />
      </CardGrid>
      <div style={{ marginTop: 12 }}>
        <ClaudeTable list={derived.list} liveByEmail={derived.liveByEmail} />
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
        title="Gemini"
        sub="Antigravity OAuth · API-$ — realized workload equivalent, не номинал подписки"
      />
      <CardGrid>
        {derived.geminiOff ? (
          <StatCard label="Gemini подписки" value="выкл." hint="Gemini runtime без профилей" />
        ) : (
          <>
            <StatCard
              label="Gemini профили"
              value={derived.geminiDown ? "—" : derived.geminiProfiles.length}
              hint={
                derived.geminiDown
                  ? "источник недоступен"
                  : `${derived.gemini?.authenticated ?? "—"} authenticated`
              }
            />
            <GeminiBudgetCard label="Gemini · workload 5ч" item={derived.geminiFive} down={derived.geminiDown} />
            <GeminiBudgetCard label="Gemini · workload 7д" item={derived.geminiWeek} down={derived.geminiDown} />
            <StatCard
              label="Gemini · потрачено"
              value={derived.geminiDown ? "—" : money(derived.geminiSpend)}
              hint="official-price, накопительно"
              onClick={openSpendStats}
              title="Разбивка: сутки / 7 дней / 30 дней"
            />
            <StatCard
              label="Gemini · в работе"
              value={derived.geminiDown ? "—" : (derived.gemini?.inflight ?? "—")}
              hint="inflight requests сейчас"
            />
            <StatCard
              label="Gemini · missing usage"
              value={derived.geminiDown ? "—" : derived.geminiMissing}
              hint={derived.geminiMissing ? "списан conservative hold" : "authoritative settlement чист"}
            />
            <StatCard
              label="Gemini · сбои контура"
              value={
                derived.geminiDown ? (
                  "—"
                ) : derived.geminiFailTotal > 0 ? (
                  <span style={{ color: "var(--warn)" }}>{derived.geminiFailTotal}</span>
                ) : (
                  derived.geminiFailTotal
                )
              }
              hint={`transport ${Number(derived.geminiFailures.transport || 0)} · backend ${Number(derived.geminiFailures.backend || 0)} · malformed ${Number(derived.geminiFailures.malformed || 0)} · stream_start ${Number(derived.geminiFailures.stream_start || 0)}`}
            />
          </>
        )}
      </CardGrid>
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
          <>
            <GeminiTable
              profiles={derived.geminiProfiles}
              models={derived.geminiModels}
              now={derived.gemini?.now}
              nowMs={result.nowMs}
            />
            <GeminiModelDetails
              profiles={derived.geminiProfiles}
              models={derived.geminiModels}
              now={derived.gemini?.now}
              nowMs={result.nowMs}
            />
          </>
        )}
      </div>

      {!derived.geminiDown && !derived.geminiOff ? (
        <TransportDetails transport={derived.gemini?.transport ?? {}} affinity={derived.geminiAffinity} />
      ) : null}

      <footer>
        Обновление каждые 10с, пока вкладка видима · GPT: shared plan capacity показывает единый номинал одинаковых
        подписок, токеновая вместимость считается из текущего остатка, а модели сортируются по API-$ на native credit ·
        почта выводится только маскированной. Gemini workload blend остаётся официальным API-$ эквивалентом
        фактически наблюдённой смеси задач; Google прямо указывает, что фиксированного USD-номинала подписки нет.
      </footer>

      {spendStatsModal}
    </>
  );
}
