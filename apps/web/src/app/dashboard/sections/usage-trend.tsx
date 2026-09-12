"use client";

import { useId, useState, type KeyboardEvent, type PointerEvent } from "react";
import type { UsageView } from "@/lib/api";
import { useI18n } from "@/components/i18n-provider";
import { buildUtcProviderUsageSeries, usageWindowDays } from "@/lib/usage-series";
import { DASHBOARD_PROVIDERS } from "@/lib/providers";
import { formatNanoUsd, roundDivide, useDashboardCopy } from "./shared";
import { usageChartGeometry } from "./usage-chart-data";
import "./usage-charts.css";

const labels = {
  en: { title: "Daily spending", subtitle: "Actual charges compared with list-price value", detail: "Day details", hint: "Hover or tap to explore · ← → to change day", models: "Spending by model", unattributed: "Unattributed" },
  ru: { title: "Расходы по дням", subtitle: "Фактические списания и стоимость по официальному тарифу", detail: "Детали за день", hint: "Наведите или нажмите · ← → для выбора дня", models: "Расходы по моделям", unattributed: "Без провайдера" },
};

export function UsageTrend({ usage }: { usage: UsageView }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const text = labels[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const gradient = useId();
  const [selected, setSelected] = useState<number | null>(null);
  const points = buildUtcProviderUsageSeries(usage.sinceTs, usage.untilTs, usage.daily, usage.dailyProviders ?? []).map(point => ({
    ...point, official: BigInt(point.officialNano), charged: BigInt(point.chargedNano),
  }));
  const geometry = usageChartGeometry(points);
  const index = Math.min(selected ?? points.length - 1, points.length - 1);
  const point = points[index];
  const date = (ts: number, full = false) => new Date(ts * 1000).toLocaleDateString(locale, { day: "numeric", month: "short", ...(full ? { year: "numeric" } : {}), timeZone: "UTC" });
  const money = (amount: bigint | string) => {
    const value = BigInt(amount);
    return formatNanoUsd(value, locale, 0, value > 0n && value < 10_000_000n ? 9 : 2);
  };
  const axisMarks = Array.from({ length: Math.min(5, points.length) }, (_, i) => Math.round(i * (points.length - 1) / Math.max(1, Math.min(5, points.length) - 1)));
  const peak = points.length ? points.reduce((best, current) => current.official > best.official ? current : best) : undefined;
  const ariaValue = point ? `${date(point.dayTs, true)}. ${copy.chargedCol}: ${money(point.charged)}. ${copy.officialValueCol}: ${money(point.official)}. ${copy.billedEvents}: ${point.requests}` : copy.noChargesPeriod;
  function selectPointer(event: PointerEvent<HTMLDivElement>) {
    if (event.pointerType === "touch" && event.type === "pointermove" && !event.buttons) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    setSelected(Math.max(0, Math.min(points.length - 1, Math.round((event.clientX - bounds.left) / bounds.width * (points.length - 1)))));
  }
  function selectKey(event: KeyboardEvent<HTMLDivElement>) {
    const next = event.key === "ArrowLeft" || event.key === "ArrowDown" ? index - 1
      : event.key === "ArrowRight" || event.key === "ArrowUp" ? index + 1
      : event.key === "Home" ? 0 : event.key === "End" ? points.length - 1 : null;
    if (next !== null) { event.preventDefault(); setSelected(Math.max(0, Math.min(points.length - 1, next))); }
    if (event.key === "Escape") { setSelected(null); event.currentTarget.blur(); }
  }

  return <section className="usage-trend" aria-labelledby="usage-trend-title">
    <header className="usage-trend-head">
      <div><h2 id="usage-trend-title">{text.title}</h2><p>{text.subtitle}</p></div>
      <span className="usage-trend-window">{copy.chartWindowLabel}</span>
    </header>
    <div className="usage-trend-overview">
      <div><span>{copy.charged30d}</span><strong>{money(usage.totalChargedNano)}</strong></div>
      <div className="usage-trend-legend"><span><i className="charged" />{copy.chargedCol}</span><span><i className="official" />{copy.officialValueCol}</span></div>
    </div>
    {geometry.maximum === 0n ? <div className="usage-trend-empty">{copy.noChargesPeriod}</div> : <div className="usage-trend-body">
      <div className="usage-trend-canvas">
        <div className="usage-trend-y" aria-hidden="true">{geometry.ticks.map(tick => <span key={String(tick.value)} style={{ top: `${tick.y / 260 * 100}%` }}>{money(tick.value)}</span>)}</div>
        <div className="usage-trend-plot" role="slider" tabIndex={0} aria-label={text.title} aria-valuemin={0} aria-valuemax={points.length - 1} aria-valuenow={index} aria-valuetext={ariaValue} aria-orientation="horizontal" onPointerMove={selectPointer} onPointerDown={selectPointer} onKeyDown={selectKey}>
          <svg viewBox="0 0 1000 260" preserveAspectRatio="none" aria-hidden="true">
            <defs><linearGradient id={gradient} x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="var(--trend-color)" stopOpacity=".2" /><stop offset="100%" stopColor="var(--trend-color)" stopOpacity=".015" /></linearGradient></defs>
            {geometry.ticks.map(tick => <line className="trend-grid" key={String(tick.value)} x1="0" x2="1000" y1={tick.y} y2={tick.y} />)}
            <path d={geometry.chargedArea} fill={`url(#${gradient})`} />
            <path className="trend-line trend-official" d={geometry.officialLine} />
            <path className="trend-line trend-charged" d={geometry.chargedLine} />
            <line className="trend-crosshair" x1={geometry.x(index)} x2={geometry.x(index)} y1="12" y2="240" />
          </svg>
          {point && <><i className="trend-dot official" style={{ left: `${geometry.x(index) / 10}%`, top: `${geometry.y(point.official) / 260 * 100}%` }} /><i className="trend-dot charged" style={{ left: `${geometry.x(index) / 10}%`, top: `${geometry.y(point.charged) / 260 * 100}%` }} /></>}
        </div>
        <div className="usage-trend-x" aria-hidden="true">{axisMarks.map((mark, i) => <span key={mark} style={{ left: `${geometry.x(mark) / 10}%`, transform: i === 0 ? "none" : i === axisMarks.length - 1 ? "translateX(-100%)" : "translateX(-50%)" }}>{date(points[mark]!.dayTs)}</span>)}</div>
        <p className="usage-trend-hint">{text.hint}</p>
      </div>
      {point && <aside className="usage-trend-detail" aria-label={text.detail} data-day-index={index}>
        <span className="trend-detail-label">{text.detail}</span><h3>{date(point.dayTs, true)}</h3>
        <dl><div className="trend-detail-primary"><dt><i />{copy.chargedCol}</dt><dd>{money(point.charged)}</dd></div><div><dt>{copy.officialValueCol}</dt><dd>{money(point.official)}</dd></div><div><dt>{copy.billedEvents}</dt><dd>{point.requests.toLocaleString(locale)}</dd></div></dl>
        {point.providers.some(provider => BigInt(provider.officialNano) > 0n) && <div className="trend-detail-providers"><span>{copy.officialValueCol}</span>{point.providers.filter(provider => BigInt(provider.officialNano) > 0n).map(provider => <div key={provider.provider}><span>{DASHBOARD_PROVIDERS.find(item => item.id === provider.provider)?.name ?? provider.provider}</span><b>{money(provider.officialNano)}</b></div>)}</div>}
        {BigInt(point.unattributedOfficialNano) > 0n && <div className="trend-detail-providers"><div><span>{text.unattributed}</span><b>{money(point.unattributedOfficialNano)}</b></div></div>}
      </aside>}
    </div>}
    <footer className="usage-trend-summary" aria-label={copy.periodSummary}>
      <div><span>{copy.officialSpend}</span><b>{money(usage.totalOfficialNano)}</b></div>
      <div><span>{copy.dailyAverage}</span><b>{money(roundDivide(BigInt(usage.totalOfficialNano), BigInt(usageWindowDays(usage.sinceTs, usage.untilTs))))}</b></div>
      <div><span>{copy.peakDay}</span><b>{peak && peak.official > 0n ? `${date(peak.dayTs)} · ${money(peak.official)}` : "—"}</b></div>
      <div><span>{copy.billedEvents}</span><b>{usage.requests.toLocaleString(locale)}</b></div>
    </footer>
  </section>;
}

export { labels as usageChartLabels };
