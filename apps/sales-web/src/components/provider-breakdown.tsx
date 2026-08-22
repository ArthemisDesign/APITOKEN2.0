"use client";

import { useMemo, useState, type CSSProperties } from "react";
import { type DailyProviderEarningsPoint, type ProviderEarningsRow } from "@/lib/api";
import { EmptyState } from "@/components/ui";
import { localeFor, useI18n } from "@/components/i18n";

type ProviderMeta = { name: string; api: string; color: string };

const PROVIDERS: Record<string, ProviderMeta> = {
  anthropic: { name: "Claude", api: "Anthropic Messages API", color: "#d97757" },
  openai: { name: "GPT", api: "OpenAI-compatible API", color: "#10a37f" },
  google: { name: "Gemini", api: "Google Gemini API", color: "#4b8bf5" },
  kimi: { name: "Kimi", api: "Anthropic Messages API", color: "#b8348c" },
};
const UNKNOWN_COLORS = ["#6f7a8a", "#8b5cf6", "#0ea5a4", "#d97706"] as const;
const NANO = 1_000_000_000n;

export function providerLabel(providerId: string | null, unattributed: string): string {
  if (providerId === null) return unattributed;
  return PROVIDERS[providerId]?.name ?? providerId;
}

function providerMeta(providerId: string | null, unattributed: string): ProviderMeta {
  if (providerId === null) return { name: unattributed, api: "Historical usage", color: "#6f7a8a" };
  const known = PROVIDERS[providerId];
  if (known) return known;
  let hash = 0;
  for (const char of providerId) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return { name: providerId, api: "API provider", color: UNKNOWN_COLORS[hash % UNKNOWN_COLORS.length]! };
}

/** Share in tenths of a percent; money never passes through floating-point conversion. */
export function earningsShareTenths(earnedNano: string, totalNano: bigint): number {
  if (totalNano === 0n) return 0;
  return Number((BigInt(earnedNano) * 1000n) / totalNano);
}

export function dailyProviderTotal(point: DailyProviderEarningsPoint): bigint {
  return point.providers.reduce((sum, provider) => sum + BigInt(provider.earnedNano), 0n);
}

function absolute(value: bigint): bigint {
  return value < 0n ? -value : value;
}

function formatNanoUsd(value: bigint, locale: string): string {
  const amount = absolute(value);
  const whole = amount / NANO;
  const fraction = (amount % NANO).toString().padStart(9, "0");
  const maxDigits = amount >= 10_000_000n ? 2 : amount >= 1_000_000n ? 3 : amount >= 100_000n ? 4 : 6;
  let decimals = fraction.slice(0, maxDigits);
  while (decimals.length > 2 && decimals.endsWith("0")) decimals = decimals.slice(0, -1);
  return `${value < 0n ? "−" : ""}$${whole.toLocaleString(locale)}.${decimals}`;
}

function formatAxisNanoUsd(value: bigint, locale: string): string {
  const amount = absolute(value);
  if (amount >= 1_000n * NANO) {
    const thousands = amount / (1_000n * NANO);
    const tenth = (amount % (1_000n * NANO)) / (100n * NANO);
    return `${value < 0n ? "−" : ""}$${thousands.toLocaleString(locale)}${tenth === 0n ? "" : `.${tenth}`}k`;
  }
  if (amount >= NANO) return `${value < 0n ? "−" : ""}$${(amount / NANO).toLocaleString(locale)}`;
  return formatNanoUsd(value, locale);
}

function ratioPercent(value: bigint, maximum: bigint): number {
  if (value <= 0n || maximum <= 0n) return 0;
  return Number((value * 10_000n) / maximum) / 100;
}

function dateValue(date: string): number {
  return Date.parse(`${date}T00:00:00Z`);
}

export function ProviderBreakdown({ items, daily }: { items: ProviderEarningsRow[]; daily: DailyProviderEarningsPoint[] }) {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const unattributed = t("Before provider tracking", "До учёта провайдера");
  const [activeDay, setActiveDay] = useState<number | null>(null);
  const rows = useMemo(
    () => items.filter((row) => row.spendNano !== "0" || row.earnedNano !== "0" || row.events !== 0),
    [items],
  );
  const totalEarned = rows.reduce((sum, row) => sum + BigInt(row.earnedNano), 0n);
  const totalSpend = rows.reduce((sum, row) => sum + BigInt(row.spendNano), 0n);
  const totalEvents = rows.reduce((sum, row) => sum + row.events, 0);
  const dailyTotals = daily.map(dailyProviderTotal);
  const maximum = dailyTotals.reduce((max, value) => value > max ? value : max, 0n);
  const peakIndex = dailyTotals.reduce((best, value, index) => value > (dailyTotals[best] ?? 0n) ? index : best, 0);
  const providerIds = [...new Set(daily.flatMap((point) => point.providers
    .filter((provider) => BigInt(provider.earnedNano) > 0n)
    .map((provider) => provider.providerId)))]
    .sort((left, right) => {
      const order = ["anthropic", "openai", "google", "kimi"];
      const rank = (value: string | null) => {
        if (value === null) return Number.MAX_SAFE_INTEGER;
        const index = order.indexOf(value);
        return index < 0 ? Number.MAX_SAFE_INTEGER - 1 : index;
      };
      return rank(left) - rank(right) || String(left).localeCompare(String(right));
    });
  const dateFormatter = new Intl.DateTimeFormat(locale, { month: "short", day: "numeric", timeZone: "UTC" });
  const formatDay = (date: string) => {
    const timestamp = dateValue(date);
    return Number.isNaN(timestamp) ? date : dateFormatter.format(timestamp);
  };
  const axisCount = Math.min(7, daily.length);
  const axisMarks = daily.length === 0 ? [] : [...new Set(Array.from(
    { length: axisCount },
    (_, index) => Math.round(index * (daily.length - 1) / Math.max(1, axisCount - 1)),
  ))];

  if (rows.length === 0 && maximum === 0n) {
    return <EmptyState title={t("No usage yet", "Пока нет расхода")}>
      {t(
        "Once your referrals start using the API, this view will split your earnings by provider.",
        "Как только ваши рефералы начнут пользоваться API, здесь появится разбивка заработка по провайдерам.",
      )}
    </EmptyState>;
  }

  return <div className="provider-analytics">
    <div className="provider-card-grid">
      {rows.map((row) => {
        const meta = providerMeta(row.providerId, unattributed);
        const share = earningsShareTenths(row.earnedNano, totalEarned);
        return <article className="provider-card" key={row.providerId ?? "(unattributed)"} style={{ "--provider-color": meta.color } as CSSProperties}>
          <div className="provider-card-head">
            <span className="provider-card-mark" aria-hidden>{meta.name.slice(0, 1)}</span>
            <div className="provider-card-name"><strong>{meta.name}</strong><span>{meta.api}</span></div>
            <span className="provider-share">{(share / 10).toFixed(1)}%</span>
          </div>
          <div className="provider-card-value">{formatNanoUsd(BigInt(row.earnedNano), locale)}</div>
          <div className="provider-card-meta">
            {t("Your earnings", "Ваш заработок")} · {formatNanoUsd(BigInt(row.spendNano), locale)} {t("referral spend", "трат рефералов")} · {row.events.toLocaleString(locale)} {t("events", "событий")}
          </div>
        </article>;
      })}
    </div>

    <div className="provider-usage-graph">
      <section className="provider-chart" aria-labelledby="provider-chart-title">
        <div className="provider-chart-head">
          <div><h3 id="provider-chart-title">{t("Earnings over time", "Заработок по дням")}</h3><span>{t("UTC · rolling 30 days", "UTC · последние 30 дней")}</span></div>
          <div className="provider-chart-legend" aria-label={t("Providers", "Провайдеры")}>
            {providerIds.map((providerId) => {
              const meta = providerMeta(providerId, unattributed);
              return <span key={providerId ?? "(unattributed)"}><i aria-hidden style={{ background: meta.color }} />{meta.name}</span>;
            })}
          </div>
        </div>
        {maximum === 0n ? <div className="provider-chart-empty">{t("No earnings in this period", "За этот период заработка нет")}</div> : <div className="provider-chart-grid">
          <div className="provider-chart-yaxis">
            {[maximum, maximum * 3n / 4n, maximum / 2n, maximum / 4n, 0n].map((tick, index) => <span key={index}>{formatAxisNanoUsd(tick, locale)}</span>)}
          </div>
          <div className="provider-chart-plotwrap">
            <div className="provider-chart-lines" aria-hidden>{[0, 1, 2, 3, 4].map((line) => <i key={line} />)}</div>
            <div className="provider-chart-plot" onMouseLeave={(event) => { if (!event.currentTarget.contains(document.activeElement)) setActiveDay(null); }}>
              {daily.map((point, index) => {
                const total = dailyTotals[index] ?? 0n;
                const aria = [
                  `${formatDay(point.date)}: ${formatNanoUsd(total, locale)}`,
                  ...providerIds.flatMap((providerId) => {
                    const segment = point.providers.find((candidate) => candidate.providerId === providerId);
                    return segment && BigInt(segment.earnedNano) > 0n
                      ? [`${providerMeta(providerId, unattributed).name}: ${formatNanoUsd(BigInt(segment.earnedNano), locale)}`]
                      : [];
                  }),
                ].join(". ");
                return <button type="button" key={point.date} className={`provider-chart-col${activeDay === index ? " is-active" : ""}`} aria-label={aria}
                  onMouseEnter={() => setActiveDay(index)} onFocus={() => setActiveDay(index)} onBlur={() => setActiveDay((current) => current === index ? null : current)}
                  onClick={() => setActiveDay((current) => current === index ? null : index)} onKeyDown={(event) => { if (event.key === "Escape") { setActiveDay(null); event.currentTarget.blur(); } }}>
                  <span className="provider-chart-col-fill">
                    {providerIds.map((providerId) => {
                      const segment = point.providers.find((candidate) => candidate.providerId === providerId);
                      const value = BigInt(segment?.earnedNano ?? "0");
                      return value > 0n ? <i aria-hidden className="provider-chart-segment" key={providerId ?? "(unattributed)"} style={{ height: `${ratioPercent(value, maximum)}%`, background: providerMeta(providerId, unattributed).color }} /> : null;
                    })}
                  </span>
                </button>;
              })}
              {activeDay !== null && daily[activeDay] && dailyTotals[activeDay]! > 0n ? (() => {
                const point = daily[activeDay]!;
                const total = dailyTotals[activeDay]!;
                const left = Math.min(92, Math.max(8, (activeDay + 0.5) / daily.length * 100));
                return <div className="provider-chart-tip" role="tooltip" style={{ left: `${left}%`, bottom: `${ratioPercent(total, maximum)}%` }}>
                  <div className="provider-chart-tip-title">{formatDay(point.date)}</div>
                  {providerIds.map((providerId) => {
                    const segment = point.providers.find((candidate) => candidate.providerId === providerId);
                    if (!segment || BigInt(segment.earnedNano) === 0n) return null;
                    const meta = providerMeta(providerId, unattributed);
                    return <div className="provider-chart-tip-row" key={providerId ?? "(unattributed)"}><i aria-hidden style={{ background: meta.color }} /><span>{meta.name}</span><b>{formatNanoUsd(BigInt(segment.earnedNano), locale)}</b></div>;
                  })}
                  <div className="provider-chart-tip-total"><span>{t("Total", "Всего")}</span><b>{formatNanoUsd(total, locale)}</b></div>
                </div>;
              })() : null}
            </div>
            <div className="provider-chart-axis">
              {axisMarks.map((mark) => <span key={mark} style={{ left: `${(mark + 0.5) / daily.length * 100}%` }}>{formatDay(daily[mark]!.date)}</span>)}
            </div>
          </div>
        </div>}
      </section>

      <aside className="provider-summary" aria-label={t("Period summary", "Итоги периода")}>
        <span className="provider-summary-title">{t("Period summary", "Итоги периода")}</span>
        <div><span>{t("Your earnings", "Ваш заработок")}</span><b className="accent">{formatNanoUsd(totalEarned, locale)}</b></div>
        <div><span>{t("Referral spend", "Траты рефералов")}</span><b>{formatNanoUsd(totalSpend, locale)}</b></div>
        <div><span>{t("Usage events", "События расхода")}</span><b>{totalEvents.toLocaleString(locale)}</b></div>
        <div><span>{t("Peak day", "Пиковый день")}</span><b>{maximum > 0n && daily[peakIndex] ? `${formatDay(daily[peakIndex]!.date)} · ${formatNanoUsd(maximum, locale)}` : "—"}</b></div>
        <div><span>{t("Daily average", "Среднее за день")}</span><b>{daily.length > 0 ? formatNanoUsd(totalEarned / BigInt(daily.length), locale) : "—"}</b></div>
      </aside>
    </div>
  </div>;
}
