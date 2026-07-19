"use client";

// Красивая тайм-карта периодов выплат: текущий период (накопление) → лок 7д → окно выплат 3д,
// с маркером «сейчас» и датами. Пропорциональные ширины сегментов по длительности.

import { localeFor, useI18n, type Lang } from "@/components/i18n";
import type { PeriodState } from "@/lib/api";

const DAY = 86_400_000;
const LOCK_DAYS = 7;
const WINDOW_DAYS = 3;

function fmt(iso: string | number, lang: Lang): string {
  return new Date(iso).toLocaleDateString(localeFor(lang), { day: "numeric", month: "short", timeZone: "UTC" });
}

export function PayoutTimeline({ state }: { state: PeriodState }) {
  const { t, lang } = useI18n();
  const now = new Date(state.now).getTime();
  const start = new Date(state.current.start).getTime();
  const end = new Date(state.current.end).getTime();
  const lockEnd = end + LOCK_DAYS * DAY;
  const windowEnd = lockEnd + WINDOW_DAYS * DAY;

  const accrualDays = Math.round((end - start) / DAY); // 15 or 16
  const totalDays = accrualDays + LOCK_DAYS + WINDOW_DAYS;
  const pct = (days: number) => `${(days / totalDays) * 100}%`;

  // Позиция «сейчас» на всей полосе [start, windowEnd].
  const span = windowEnd - start;
  const nowPct = Math.min(100, Math.max(0, ((now - start) / span) * 100));

  const phase =
    now < end ? "accruing" : now < lockEnd ? "locked" : now < windowEnd ? "payable" : "closed";

  const nextPeriodStart = end; // следующий период начинается в конце текущего
  const nextStartLabel = fmt(nextPeriodStart, lang);

  const segments = [
    {
      key: "accruing",
      days: accrualDays,
      title: t("Accruing", "Начисление"),
      range: `${fmt(start, lang)} – ${fmt(end - DAY, lang)}`,
      cls: "seg-accruing",
    },
    {
      key: "locked",
      days: LOCK_DAYS,
      title: t("Lock 7 days", "Лок 7 дней"),
      range: `${fmt(end, lang)} – ${fmt(lockEnd - DAY, lang)}`,
      cls: "seg-locked",
    },
    {
      key: "payable",
      days: WINDOW_DAYS,
      title: t("Payout", "Выплата"),
      range: `${fmt(lockEnd, lang)} – ${fmt(windowEnd - DAY, lang)}`,
      cls: "seg-payable",
    },
  ];

  const phaseLabel =
    phase === "accruing"
      ? t("Now: earnings are accruing in this period", "Сейчас: заработок начисляется в этом периоде")
      : phase === "locked"
        ? t("Now: last period is in its 7-day lock", "Сейчас: прошлый период на 7-дневном локе")
        : phase === "payable"
          ? t("Now: payout window is open", "Сейчас: открыто окно выплат")
          : t("Now: between windows", "Сейчас: между окнами");

  return (
    <div className="timeline-card">
      <div className="timeline-head">
        <span className="timeline-title">{t("Payout timeline", "Карта выплат")}</span>
        <span className={`timeline-now-badge phase-${phase}`}>{phaseLabel}</span>
      </div>

      <div className="timeline-track">
        {segments.map((seg) => (
          <div key={seg.key} className={`timeline-seg ${seg.cls}`} style={{ width: pct(seg.days) }}>
            <span className="timeline-seg-title">{seg.title}</span>
            <span className="timeline-seg-range">{seg.range}</span>
          </div>
        ))}
        <div className="timeline-now" style={{ left: `${nowPct}%` }} aria-hidden>
          <span className="timeline-now-dot" />
          <span className="timeline-now-label">{t("now", "сейчас")}</span>
        </div>
      </div>

      <div className="timeline-foot">
        <div className="timeline-next">
          <span className="timeline-next-label">{t("Next period starts", "Следующий период с")}</span>
          <span className="timeline-next-val">{nextStartLabel}</span>
        </div>
        <div className="timeline-next">
          <span className="timeline-next-label">{t("Next payout", "Следующая выплата")}</span>
          <span className="timeline-next-val">{fmt(state.nextPayout.date, lang)}</span>
        </div>
      </div>
    </div>
  );
}
