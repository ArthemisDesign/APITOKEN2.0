"use client";

import { useState, type KeyboardEvent, type PointerEvent } from "react";
import type { UsageView } from "@/lib/api";
import { useI18n } from "@/components/i18n-provider";
import { buildUtcProviderUsageSeries, usageWindowDays } from "@/lib/usage-series";
import { DASHBOARD_PROVIDERS, fallbackProvider } from "@/lib/providers";
import { formatNanoUsd, roundDivide, useDashboardCopy } from "./shared";
import { usageChartGeometry } from "./usage-chart-data";
import "./usage-charts.css";

const labels = {
  en: { title: "Daily spending", subtitle: "List-price value by provider", detail: "Day details", hint: "Hover or tap to explore · ← → to change day", models: "Spending by model", unattributed: "Unattributed" },
  ru: { title: "Расходы по дням", subtitle: "Стоимость по официальному тарифу в разрезе провайдеров", detail: "Детали за день", hint: "Наведите или нажмите · ← → для выбора дня", models: "Расходы по моделям", unattributed: "Без провайдера" },
};
const USAGE_PROVIDER_COLORS = ["#ef4444", "#172554", "#dc2626", "#1e3a8a"] as const;

export function UsageTrend({ usage }: { usage: UsageView }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const text = labels[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const [selected, setSelected] = useState<number | null>(null);
  const points = buildUtcProviderUsageSeries(usage.sinceTs, usage.untilTs, usage.daily, usage.dailyProviders ?? []).map(point => ({
    ...point, official: BigInt(point.officialNano), charged: BigInt(point.chargedNano),
  }));
  const providerOrder = new Map(DASHBOARD_PROVIDERS.map((provider, index) => [provider.id, index]));
  const providers: { id: string | null; name: string; color: string }[] = [...new Set(points.flatMap(point => point.providers.filter(provider => BigInt(provider.officialNano) > 0n).map(provider => provider.provider)))]
    .sort((a, b) => (providerOrder.get(a) ?? Number.MAX_SAFE_INTEGER) - (providerOrder.get(b) ?? Number.MAX_SAFE_INTEGER) || a.localeCompare(b))
    .map((id, index) => ({ id, name: DASHBOARD_PROVIDERS.find(provider => provider.id === id)?.name ?? id, color: USAGE_PROVIDER_COLORS[index % USAGE_PROVIDER_COLORS.length]! }));
  if (points.some(point => BigInt(point.unattributedOfficialNano) > 0n)) providers.push({ id: null, name: text.unattributed, color: fallbackProvider("unattributed", text.unattributed).color });
  const geometry = usageChartGeometry(points, providers.map(provider => provider.id));
  const index = Math.min(selected ?? points.length - 1, points.length - 1);
  const point = points[index];
  const date = (ts: number, full = false) => new Date(ts * 1000).toLocaleDateString(locale, { day: "numeric", month: "short", ...(full ? { year: "numeric" } : {}), timeZone: "UTC" });
  const money = (amount: bigint | string) => {
    const value = BigInt(amount);
    return formatNanoUsd(value, locale, 0, value > 0n && value < 10_000_000n ? 9 : 2);
  };
  const axisMarks = Array.from({ length: Math.min(5, points.length) }, (_, i) => Math.round(i * (points.length - 1) / Math.max(1, Math.min(5, points.length) - 1)));
  const peak = points.length ? points.reduce((best, current) => current.official > best.official ? current : best) : undefined;
  const ariaValue = point ? [`${date(point.dayTs, true)}. ${copy.officialValueCol}: ${money(point.official)}. ${copy.chargedCol}: ${money(point.charged)}. ${copy.billedEvents}: ${point.requests}`, ...providers.flatMap((provider, providerIndex) => geometry.layers[providerIndex]!.values[index]! > 0n ? [`${provider.name}: ${money(geometry.layers[providerIndex]!.values[index]!)}`] : [])].join(". ") : copy.noChargesPeriod;
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
      <div><span>{copy.officialValue30d}</span><strong>{money(usage.totalOfficialNano)}</strong></div>
      <div className="usage-trend-legend">{providers.map(provider => <span key={provider.id ?? "legacy-unattributed"}><i style={{ borderColor: provider.color }} />{provider.name}</span>)}</div>
    </div>
    {geometry.maximum === 0n ? <div className="usage-trend-empty">{copy.noChargesPeriod}</div> : <div className="usage-trend-body">
      <div className="usage-trend-canvas">
        <div className="usage-trend-y" aria-hidden="true">{geometry.ticks.map(tick => <span key={String(tick.value)} style={{ top: `${tick.y / 260 * 100}%` }}>{money(tick.value)}</span>)}</div>
        <div className="usage-trend-plot" role="slider" tabIndex={0} aria-label={text.title} aria-valuemin={0} aria-valuemax={points.length - 1} aria-valuenow={index} aria-valuetext={ariaValue} aria-orientation="horizontal" onPointerMove={selectPointer} onPointerDown={selectPointer} onKeyDown={selectKey}>
          <svg viewBox="0 0 1000 260" preserveAspectRatio="none" aria-hidden="true">
            {geometry.ticks.map(tick => <line className="trend-grid" key={String(tick.value)} x1="0" x2="1000" y1={tick.y} y2={tick.y} />)}
            {geometry.layers.map((layer, providerIndex) => <g key={layer.id ?? "legacy-unattributed"} data-provider={layer.id ?? "legacy-unattributed"}>
              <path className="trend-provider-area" d={layer.area} fill={providers[providerIndex]!.color} />
              <path className="trend-line" d={layer.line} stroke={providers[providerIndex]!.color} />
            </g>)}
            <line className="trend-crosshair" x1={geometry.x(index)} x2={geometry.x(index)} y1="12" y2="240" />
          </svg>
          {point && geometry.layers.map((layer, providerIndex) => layer.values[index]! > 0n && <i key={layer.id ?? "legacy-unattributed"} className="trend-dot" style={{ left: `${geometry.x(index) / 10}%`, top: `${geometry.y(layer.upper[index]!) / 260 * 100}%`, background: providers[providerIndex]!.color, boxShadow: `0 0 0 1px ${providers[providerIndex]!.color}` }} />)}
        </div>
        <div className="usage-trend-x" aria-hidden="true">{axisMarks.map((mark, i) => <span key={mark} style={{ left: `${geometry.x(mark) / 10}%`, transform: i === 0 ? "none" : i === axisMarks.length - 1 ? "translateX(-100%)" : "translateX(-50%)" }}>{date(points[mark]!.dayTs)}</span>)}</div>
        <p className="usage-trend-hint">{text.hint}</p>
      </div>
      {point && <aside className="usage-trend-detail" aria-label={text.detail} data-day-index={index}>
        <span className="trend-detail-label">{text.detail}</span><h3>{date(point.dayTs, true)}</h3>
        <dl><div className="trend-detail-primary"><dt>{copy.officialValueCol}</dt><dd>{money(point.official)}</dd></div><div><dt>{copy.chargedCol}</dt><dd>{money(point.charged)}</dd></div><div><dt>{copy.billedEvents}</dt><dd>{point.requests.toLocaleString(locale)}</dd></div></dl>
        {point.official > 0n && <div className="trend-detail-providers"><span>{copy.officialValueCol}</span>{providers.map((provider, providerIndex) => geometry.layers[providerIndex]!.values[index]! > 0n && <div key={provider.id ?? "legacy-unattributed"}><span><i style={{ background: provider.color }} />{provider.name}</span><b>{money(geometry.layers[providerIndex]!.values[index]!)}</b></div>)}</div>}
      </aside>}
    </div>}
    <footer className="usage-trend-summary" aria-label={copy.periodSummary}>
      <div><span>{copy.chargedCol}</span><b>{money(usage.totalChargedNano)}</b></div>
      <div><span>{copy.dailyAverage}</span><b>{money(roundDivide(BigInt(usage.totalOfficialNano), BigInt(usageWindowDays(usage.sinceTs, usage.untilTs))))}</b></div>
      <div><span>{copy.peakDay}</span><b>{peak && peak.official > 0n ? `${date(peak.dayTs)} · ${money(peak.official)}` : "—"}</b></div>
      <div><span>{copy.billedEvents}</span><b>{usage.requests.toLocaleString(locale)}</b></div>
    </footer>
  </section>;
}

export { labels as usageChartLabels };
