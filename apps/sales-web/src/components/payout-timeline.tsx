"use client";

// Многодорожечная карта периодов выплат. Полумесячные периоды перекрываются: пока прошлый период
// стоит в 7-дневном локе (и затем в окне выплат), текущий уже копится. Каждый период — своя
// дорожка на ОБЩЕЙ шкале дат (накопление → лок 7д → выплата 3д), с единой линией «сейчас», так что
// вся ситуация видна целиком.

import { localeFor, useI18n, type Lang } from "@/components/i18n";
import { formatUsd, type PeriodState } from "@/lib/api";

const DAY = 86_400_000;

function fmt(ms: number, lang: Lang): string {
  return new Date(ms).toLocaleDateString(localeFor(lang), { day: "numeric", month: "short", timeZone: "UTC" });
}

// Предыдущий полумесячный период по началу текущего (UTC). Периоды: P1=[1,16), P2=[16, 1-е след. мес.).
function previousPeriod(currentStartMs: number): { start: number; end: number } {
  const d = new Date(currentStartMs);
  const y = d.getUTCFullYear();
  const m = d.getUTCMonth();
  if (d.getUTCDate() >= 16) {
    return { start: Date.UTC(y, m, 1), end: Date.UTC(y, m, 16) }; // текущий P2 → прошлый P1
  }
  const py = m === 0 ? y - 1 : y;
  const pm = m === 0 ? 11 : m - 1;
  return { start: Date.UTC(py, pm, 16), end: Date.UTC(y, m, 1) }; // текущий P1 → прошлый P2 прошлого мес.
}

type Lane = { key: string; start: number; end: number; amount: string; kind: "current" | "past" };

const COLORS = {
  accrue: { bg: "rgba(59,91,219,0.85)", soft: "rgba(59,91,219,0.16)" },
  lock: { bg: "rgba(214,158,46,0.9)", soft: "rgba(214,158,46,0.16)" },
  payout: { bg: "rgba(38,161,94,0.9)", soft: "rgba(38,161,94,0.16)" },
};

export function PayoutTimeline({ state }: { state: PeriodState }) {
  const { t, lang } = useI18n();
  const now = new Date(state.now).getTime();
  const lockDays = state.lockDays || 7;
  const windowDays = state.windowDays || 3;

  const curStart = new Date(state.current.start).getTime();
  const curEnd = new Date(state.current.end).getTime();

  // Дорожки: текущий (накопление) + прошлые периоды, ещё «в полёте» (лок/окно выплат).
  const lanes: Lane[] = [{
    key: state.current.key,
    start: curStart,
    end: curEnd,
    amount: state.current.accruedNano,
    kind: "current",
  }];
  const seen = new Set<string>([state.current.key]);

  // Предыдущий полумесячный период — показываем всегда, пока он «в полёте» (даже при $0), чтобы
  // было видно перекрытие: он в локе/окне выплат, а текущий уже копится.
  const prev = previousPeriod(curStart);
  const prevWinEnd = prev.end + (lockDays + windowDays) * DAY;
  if (now < prevWinEnd) {
    const match = state.history.find((h) => new Date(h.start).getTime() === prev.start);
    lanes.push({ key: match?.key ?? `prev-${prev.start}`, start: prev.start, end: prev.end, amount: match?.earnedNano ?? "0", kind: "past" });
    if (match) seen.add(match.key);
  }

  // Другие «в полёте» периоды из истории (на случай особых окон).
  for (const h of state.history) {
    if (seen.has(h.key)) continue;
    if (h.phase === "locked" || h.phase === "payable") {
      seen.add(h.key);
      lanes.push({ key: h.key, start: new Date(h.start).getTime(), end: new Date(h.end).getTime(), amount: h.earnedNano, kind: "past" });
    }
  }
  lanes.sort((a, b) => a.start - b.start);

  const laneWinEnd = (l: Lane) => l.end + (lockDays + windowDays) * DAY;
  const axisStart = Math.min(now, ...lanes.map((l) => l.start));
  const axisEnd = Math.max(now, ...lanes.map(laneWinEnd));
  const axisSpan = axisEnd - axisStart || 1;
  const leftPct = (ms: number) => `${Math.max(0, Math.min(100, ((ms - axisStart) / axisSpan) * 100))}%`;
  const widthPct = (a: number, b: number) => `${Math.max(0, ((b - a) / axisSpan) * 100)}%`;
  const nowPct = `${Math.max(0, Math.min(100, ((now - axisStart) / axisSpan) * 100))}%`;

  function phaseOf(l: Lane): "accrue" | "lock" | "payout" | "closed" {
    const lockEnd = l.end + lockDays * DAY;
    const winEnd = lockEnd + windowDays * DAY;
    if (now < l.end) return "accrue";
    if (now < lockEnd) return "lock";
    if (now < winEnd) return "payout";
    return "closed";
  }
  const phaseText: Record<string, [string, string]> = {
    accrue: ["Accruing now", "Копится сейчас"],
    lock: ["In 7-day lock", "В 7-дневном локе"],
    payout: ["Payout window open", "Окно выплат открыто"],
    closed: ["Paid / closed", "Выплачен / закрыт"],
  };
  const phaseColor: Record<string, string> = {
    accrue: COLORS.accrue.bg, lock: COLORS.lock.bg, payout: COLORS.payout.bg, closed: "var(--text-faint)",
  };

  function segs(l: Lane) {
    const lockEnd = l.end + lockDays * DAY;
    const winEnd = lockEnd + windowDays * DAY;
    return [
      { c: COLORS.accrue, a: l.start, b: l.end, label: t("accrue", "накопл.") },
      { c: COLORS.lock, a: l.end, b: lockEnd, label: t("lock", "лок") },
      { c: COLORS.payout, a: lockEnd, b: winEnd, label: t("pay", "выпл.") },
    ];
  }

  return (
    <div style={{ border: "1px solid var(--border)", borderRadius: 12, padding: "16px 16px 12px", position: "relative" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 14 }}>
        <span style={{ fontWeight: 700 }}>{t("Payout roadmap", "Карта периодов выплат")}</span>
        <span style={{ display: "flex", gap: 12, fontSize: 11, color: "var(--text-faint)", flexWrap: "wrap" }}>
          <Legend color={COLORS.accrue.bg} label={t("accruing", "накопление")} />
          <Legend color={COLORS.lock.bg} label={t("lock 7d", "лок 7д")} />
          <Legend color={COLORS.payout.bg} label={t("payout 3d", "выплата 3д")} />
        </span>
      </div>

      <div style={{ position: "relative" }}>
        {/* линия «сейчас» через все дорожки — яркая, контрастная */}
        <div style={{ position: "absolute", left: nowPct, top: 18, bottom: 22, width: 2.5, background: "#e8590c", zIndex: 4, boxShadow: "0 0 0 1px rgba(255,255,255,0.5)" }} aria-hidden>
          <span style={{ position: "absolute", top: -18, left: "50%", transform: "translateX(-50%)", fontSize: 10, fontWeight: 800, whiteSpace: "nowrap", background: "#e8590c", color: "#fff", padding: "2px 6px", borderRadius: 5 }}>
            {t("now", "сейчас")}
          </span>
          <span style={{ position: "absolute", top: -4, left: "50%", transform: "translateX(-50%)", width: 8, height: 8, borderRadius: "50%", background: "#e8590c", border: "2px solid #fff", boxSizing: "border-box" }} />
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 10, paddingTop: 6 }}>
          {lanes.map((l) => {
            const ph = phaseOf(l);
            return (
              <div key={l.key}>
                <div style={{ display: "flex", justifyContent: "space-between", fontSize: 12, marginBottom: 3 }}>
                  <span>
                    <strong>{fmt(l.start, lang)} – {fmt(l.end - DAY, lang)}</strong>
                    <span style={{ color: "var(--text-faint)" }}> · {formatUsd(l.amount)}</span>
                    {l.kind === "current" ? <span style={{ color: "var(--text-faint)" }}> ({t("this period", "текущий")})</span> : null}
                  </span>
                  <span style={{ color: phaseColor[ph], fontWeight: 600 }}>{t(phaseText[ph]![0], phaseText[ph]![1])}</span>
                </div>
                <div style={{ position: "relative", height: 24 }}>
                  {segs(l).map((s, i) => (
                    <div
                      key={i}
                      title={`${s.label}: ${fmt(s.a, lang)} – ${fmt(s.b - DAY, lang)}`}
                      style={{
                        position: "absolute", left: leftPct(s.a), width: widthPct(s.a, s.b), top: 0, bottom: 0,
                        background: s.c.bg, borderRadius: 4,
                        display: "flex", alignItems: "center", justifyContent: "center",
                        color: "#fff", fontSize: 10, fontWeight: 600, overflow: "hidden", whiteSpace: "nowrap",
                      }}
                    >
                      {s.label}
                    </div>
                  ))}
                </div>
              </div>
            );
          })}
        </div>

        {/* ось дат */}
        <div style={{ display: "flex", justifyContent: "space-between", marginTop: 6, fontSize: 10, color: "var(--text-faint)" }}>
          <span>{fmt(axisStart, lang)}</span>
          <span>{fmt(axisStart + axisSpan / 2, lang)}</span>
          <span>{fmt(axisEnd, lang)}</span>
        </div>
      </div>

      <div style={{ display: "flex", gap: 20, marginTop: 12, fontSize: 12, flexWrap: "wrap" }}>
        <span><span style={{ color: "var(--text-faint)" }}>{t("Next period starts", "Следующий период с")}:</span> <strong>{fmt(new Date(state.current.end).getTime(), lang)}</strong></span>
        <span><span style={{ color: "var(--text-faint)" }}>{t("Next payout", "Следующая выплата")}:</span> <strong>{fmt(new Date(state.nextPayout.date).getTime(), lang)}</strong></span>
      </div>
    </div>
  );
}

function Legend({ color, label }: { color: string; label: string }) {
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
      <span style={{ width: 10, height: 10, borderRadius: 3, background: color, display: "inline-block" }} />
      {label}
    </span>
  );
}
