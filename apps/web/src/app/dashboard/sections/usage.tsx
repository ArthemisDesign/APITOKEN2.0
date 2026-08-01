"use client";

import Link from "next/link";
import { useState, type CSSProperties } from "react";
import type {
  AccountView,
  ApiKeyView,
  CustomerPricingModelView,
  CustomerPricingRuleView,
  LedgerEntry,
  UsageView,
} from "@/lib/api";
import { useI18n } from "@/components/i18n-provider";
import type { DashboardCopy } from "@/lib/dashboard-copy";
import { buildUtcUsageSeries, usageWindowDays } from "@/lib/usage-series";
import { modelLabel } from "@/lib/model-label";
import { DASHBOARD_PROVIDERS, fallbackProvider } from "@/lib/providers";
import {
  NANO_PER_USD, PageHeading, Stat,
  compareBigInt, formatNanoUsd, interpolate, localDashboardCopy, roundDivide, useDashboardCopy,
} from "./shared";

const policyCopy = {
  en: {
    availableModels: "Available models",
    paidBalance: "Paid balance",
    fundingPending: "Funding split pending",
    unavailable: "Unavailable",
    access: "Access",
    pricingRule: "Pricing rule",
    progressive: "Progressive",
    legacy: "Legacy account rule",
    mixed: "Mixed rules",
    noRule: "No pricing rule",
    policyPending: "Desired policy is waiting for an engine ACK.",
    policyUnavailable: "No applied provider/model policy is available yet.",
    providerUnattributed: "Unattributed",
    actualProvider: "Actual provider",
    officialVsCharged: "Official → charged",
    paidFunding: "paid",
    bonusFunding: "bonus",
  },
  ru: {
    availableModels: "Доступные модели",
    paidBalance: "Оплаченный баланс",
    fundingPending: "Разбивка средств ожидает сверки",
    unavailable: "Недоступно",
    access: "Доступ",
    pricingRule: "Правило тарификации",
    progressive: "Прогрессивный тариф",
    legacy: "Legacy-правило аккаунта",
    mixed: "Разные правила",
    noRule: "Правило отсутствует",
    policyPending: "Desired policy ожидает подтверждения движка.",
    policyUnavailable: "Применённая provider/model policy пока недоступна.",
    providerUnattributed: "Без attribution",
    actualProvider: "Фактический провайдер",
    officialVsCharged: "Официальная стоимость → списано",
    paidFunding: "оплачено",
    bonusFunding: "бонус",
  },
} as const;

export function Usage({ account, keys, ledger, usage, ledgerAvailable }: { account: AccountView; keys: ApiKeyView[]; ledger: LedgerEntry[]; usage: UsageView; ledgerAvailable: boolean }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localPolicyCopy = policyCopy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const models = usage.models;
  const modelOfficialTotal = models.reduce((sum, model) => sum + BigInt(model.officialNano), 0n);
  const policy = account.pricingPolicies?.[0] ?? null;
  const appliedPolicy = policy?.applied ?? null;
  const policyModels = appliedPolicy?.providers.flatMap((provider) => provider.models.map((model) => ({
    ...model,
    providerId: provider.providerId,
  }))) ?? [];
  const availablePolicyModels = policyModels.filter((model) => model.available);

  // Stable model colours are shared by the distribution bar and model table.
  const modelColor = new Map<string, string>();
  const assignColor = (id: string) => { if (!modelColor.has(id)) modelColor.set(id, MODEL_COLORS[modelColor.size % MODEL_COLORS.length]!); };
  for (const model of models) assignColor(model.model);

  // Provider badges appear only when the window actually mixes providers — a single-provider
  // table would render the same tag on every row, which is noise.
  const providersPresent = new Set(models.map((model) => model.provider ?? "unattributed"));
  const showProviderBadge = providersPresent.size > 1;

  // Агрегаты по провайдерам для карточек «Connected providers»: реестр даёт метаданные
  // (логотип, цвет, имя), провайдеры вне реестра получают авто-карточку.
  const providerAgg = new Map<string, { requests: number; tokens: number; officialNano: bigint; chargedNano: bigint }>();
  for (const model of models) {
    const id = model.provider ?? "unattributed";
    const agg = providerAgg.get(id) ?? { requests: 0, tokens: 0, officialNano: 0n, chargedNano: 0n };
    agg.requests += model.requests;
    agg.tokens += model.inputTokens + model.outputTokens + model.cacheReadTokens + model.cacheWrite5mTokens + model.cacheWrite1hTokens;
    agg.officialNano += BigInt(model.officialNano);
    agg.chargedNano += BigInt(model.chargedNano);
    providerAgg.set(id, agg);
  }
  const providerIds = new Set([
    ...(appliedPolicy?.providers.map((provider) => provider.providerId) ?? []),
    ...providerAgg.keys(),
  ]);
  const providerCards = [...providerIds].sort().map((id) => {
    const registered = DASHBOARD_PROVIDERS.find((provider) => provider.id === id);
    const metadata = registered ?? fallbackProvider(
      id,
      id === "unattributed" ? localPolicyCopy.providerUnattributed : id,
    );
    return {
      ...metadata,
      agg: providerAgg.get(id),
      policy: appliedPolicy?.providers.find((provider) => provider.providerId === id) ?? null,
    };
  });
  const [copiedProvider, setCopiedProvider] = useState<string | null>(null);

  async function copyProviderEndpoint(id: string, endpoint: string) {
    await navigator.clipboard.writeText(endpoint);
    setCopiedProvider(id);
    window.setTimeout(() => setCopiedProvider((current) => (current === id ? null : current)), 1_200);
  }

  const series = buildUtcUsageSeries(usage.sinceTs, usage.untilTs, usage.daily).map((point) => ({
    day: point.dayTs * 1_000,
    requests: point.requests,
    value: BigInt(point.officialNano),
    charged: BigInt(point.chargedNano),
  }));
  const maxValue = series.reduce((max, point) => bigintMax(max, point.value), 0n);
  const scale = niceNanoScale(maxValue);
  const gridTicks = Array.from({ length: scale.divisions + 1 }, (_, index) => scale.max - BigInt(index) * scale.step);
  const summaryOfficialNano = BigInt(usage.totalOfficialNano);
  const summaryChargedNano = BigInt(usage.totalChargedNano);
  const summaryRequests = usage.requests;
  const peak = series.reduce((best, point) => (point.value > best.value ? point : best), {
    day: usage.sinceTs * 1_000,
    requests: 0,
    value: 0n,
    charged: 0n,
  });
  const averageDays = BigInt(usageWindowDays(usage.sinceTs, usage.untilTs));
  const LABEL_COUNT = 7;
  const axisMarkCount = Math.min(LABEL_COUNT, series.length);
  const axisMarks = series.length === 0 ? [] : [...new Set(Array.from(
    { length: axisMarkCount },
    (_, index) => Math.round(index * (series.length - 1) / Math.max(1, axisMarkCount - 1)),
  ))];

  // Разбивка модель-бара (mdist) с центрами сегментов — для наведения/подсказки.
  const modelShares = models.map((model) => modelOfficialTotal > 0n ? boundedRatio(BigInt(model.officialNano), modelOfficialTotal) : 1 / models.length);
  const mdistPlaced = models.map((model, index) => {
    const share = modelShares[index]!;
    const center = modelShares.slice(0, index).reduce((sum, value) => sum + value, 0) + share / 2;
    return { model, share, center };
  });
  const [hoverDay, setHoverDay] = useState<number | null>(null);
  const [mdistHover, setMdistHover] = useState<number | null>(null);

  const keyRows = [...usage.keys].sort((left, right) => compareBigInt(BigInt(right.officialNano), BigInt(left.officialNano)));
  const keyLabels = new Map(keys.flatMap((key) => key.label ? [[key.keyMasked, key.label] as const] : []));
  const ledgerMayBePartial = ledger.length >= 100;
  const legacyOfficialNano = BigInt(usage.buckets.unattributedLegacy.officialNano);

  return <section className="panel"><PageHeading eyebrow={copy.usageEyebrow} title={copy.usageTitle} subtitle={copy.usageSubtitle} />
    <div className="banner">💡 <b>{copy.sessionSavingTitle}</b><span> {copy.sessionSavingText}</span></div>

    <div className="ov-stats bill4">
      <div className="ovstat"><span className="dlabel">{copy.officialValue30d}</span><b className="num accent">{formatNanoUsd(summaryOfficialNano, locale)}</b><span className="dtrend">{copy.listPriceEquivalent}</span></div>
      <Stat label={copy.charged30d} value={formatNanoUsd(summaryChargedNano, locale)} detail={copy.settledCredits} />
      <div className="ovstat"><span className="dlabel">{localPolicyCopy.availableModels}</span><b className="num">{availablePolicyModels.length}</b><span className="dtrend">{policy?.inSync ? copy.ready : localPolicyCopy.policyPending}</span></div>
      <div className="ovstat"><span className="dlabel">{localPolicyCopy.paidBalance}</span><b className="num">{account.funding ? formatNanoUsd(account.funding.balances.paidNano, locale) : "—"}</b><span className="dtrend">{account.funding ? copy.available : localPolicyCopy.fundingPending}</span></div>
    </div>

    <section className="dsec uproviders">
      <div className="dsec-head analytics-heading"><div><h2>{copy.usageProviders}</h2><p>{copy.usageProvidersSub}</p></div></div>
      <div className="uprovider-grid">
        {providerCards.map((card) => {
          const isActive = (card.agg?.requests ?? 0) > 0;
          const ruleSummary = providerRuleSummary(card.policy?.models ?? [], localPolicyCopy);
          return <article
            className="uprovider-card"
            key={card.id}
            style={{ "--provider-color": card.color, ...(card.logo ? { "--provider-logo": `url("${card.logo}")` } : {}) } as CSSProperties}
          >
            <div className="uprovider-head">
              {card.logo
                ? <span className="uprovider-logo" aria-hidden="true" />
                : <span className="uprovider-logo uprovider-letter" aria-hidden="true">{card.name.slice(0, 1)}</span>}
              <div className="uprovider-name">
                <strong>{card.name}</strong>
                <span>{card.api}</span>
              </div>
              <span className={`uprovider-status${card.policy?.available ? " is-active" : ""}`}>{card.policy?.available ? copy.ready : localPolicyCopy.unavailable}</span>
              <span className="uprovider-discount" title={localPolicyCopy.pricingRule}>{ruleSummary}</span>
            </div>
            {card.endpoint && (
              <div className="uprovider-endpoint">
                <code>{card.endpoint}</code>
                {card.auth && <span>{card.auth}</span>}
                <button type="button" onClick={() => copyProviderEndpoint(card.id, card.endpoint!)}>
                  {copiedProvider === card.id ? copy.copiedEndpoint : copy.copyEndpoint}
                </button>
              </div>
            )}
            <div className="uprovider-stats">
              <strong>{formatNanoUsd(card.agg?.officialNano ?? 0n, locale)}</strong>
              <span>{isActive && card.agg
                ? interpolate(copy.usageProviderMeta, {
                    charged: formatNanoUsd(card.agg.chargedNano, locale),
                    requests: card.agg.requests.toLocaleString(locale),
                    tokens: fmtTokens(card.agg.tokens, locale),
                  })
                : copy.usageProviderEmpty}</span>
            </div>
            {card.docsPath && (
              <Link className="uprovider-guide" href={card.docsPath}>
                {copy.providerGuide} →
              </Link>
            )}
          </article>;
        })}
      </div>
      {!appliedPolicy && <div className="banner">{localPolicyCopy.policyUnavailable}</div>}
      {appliedPolicy && <>
        <p className="table-scroll-hint">{copy.tableScrollHint}</p>
        <div className="table-scroll" role="region" tabIndex={0} aria-label={localPolicyCopy.availableModels}>
          <table className="mtable"><thead><tr><th>{localPolicyCopy.actualProvider}</th><th>{copy.model}</th><th>{localPolicyCopy.access}</th><th>{localPolicyCopy.pricingRule}</th></tr></thead>
            <tbody>{policyModels.map((model) => <tr key={`${model.providerId}:${model.modelId}`}>
              <td>{providerDisplayName(model.providerId, localPolicyCopy.providerUnattributed)}</td>
              <td>{modelLabel(model.modelId)}</td>
              <td><span className={`pill ${model.available ? "pill-good" : "pill-soft"}`}>{model.available ? copy.ready : localPolicyCopy.unavailable}</span></td>
              <td>{pricingRuleLabel(model.rule, localPolicyCopy)}</td>
            </tr>)}</tbody>
          </table>
        </div>
      </>}
    </section>

    <div className="usage-graph">
      <div className="uchart">
        <div className="uchart-head"><b>{copy.usageOverTime}</b><span>{copy.chartWindowLabel}</span></div>
        {maxValue === 0n ? <div className="uchart-empty">{copy.noChargesPeriod}</div> : <>
          <div className="uchart-grid">
            <div className="uchart-yaxis">{gridTicks.map((tick, i) => <span key={i}>{formatAxisNanoUsd(tick, locale)}</span>)}</div>
            <div className="uchart-plotwrap">
              <div className="uchart-lines">{gridTicks.map((_, i) => <i key={i} />)}</div>
              <div className="uchart-plot" onMouseLeave={(event) => { if (!event.currentTarget.contains(document.activeElement)) setHoverDay(null); }}>
                {series.map((point, index) => <button type="button" key={point.day} className={`uchart-col${hoverDay === index ? " is-hover" : ""}`} aria-label={interpolate(copy.chartDayLabel, { date: fmtUtcDay(point.day, locale), value: formatNanoUsdSmart(point.value, locale) })} onMouseEnter={() => setHoverDay(index)} onFocus={() => setHoverDay(index)} onBlur={() => setHoverDay((current) => current === index ? null : current)} onClick={() => setHoverDay((current) => current === index ? null : index)} onKeyDown={(event) => { if (event.key === "Escape") { setHoverDay(null); event.currentTarget.blur(); } }}>
                  <div className="uchart-col-fill">
                    {point.value > 0n && <div className="uchart-seg" style={{ height: `${boundedPercent(point.value, scale.max)}%`, background: MODEL_COLORS[0] }} />}
                  </div>
                </button>)}
                {hoverDay !== null && series[hoverDay] && series[hoverDay]!.value > 0n && (() => {
                  const point = series[hoverDay]!;
                  const leftPct = Math.min(92, Math.max(8, (hoverDay + 0.5) / series.length * 100));
                  return <div className="chart-tip" role="tooltip" style={{ left: `${leftPct}%`, bottom: `${boundedPercent(point.value, scale.max)}%` }}>
                    <div className="chart-tip-h">{fmtUtcDay(point.day, locale)}</div>
                    <div className="chart-tip-row"><span className="chart-tip-dot" style={{ background: MODEL_COLORS[0] }} /><span className="chart-tip-nm">{copy.officialValueCol}</span><b>{formatNanoUsdSmart(point.value, locale)}</b></div>
                    <div className="chart-tip-total"><span>{copy.chargedCol}</span><b>{formatNanoUsdSmart(point.charged, locale)}</b></div>
                    <div className="chart-tip-total"><span>{copy.billedEvents}</span><b>{point.requests.toLocaleString(locale)}</b></div>
                  </div>;
                })()}
              </div>
              <div className="uchart-axis">{axisMarks.map((mark) => <span key={mark} style={{ left: `${(mark + 0.5) / series.length * 100}%` }}>{fmtUtcDay(series[mark]!.day, locale)}</span>)}</div>
            </div>
          </div>
        </>}
      </div>
      <div className="usum">
        <span className="usum-t">{copy.periodSummary}</span>
        <div className="usum-row"><span>{copy.officialSpend}</span><b className="accent">{formatNanoUsd(summaryOfficialNano, locale)}</b></div>
        <div className="usum-row"><span>{copy.chargedCol}</span><b>{formatNanoUsd(summaryChargedNano, locale)}</b></div>
        <div className="usum-row"><span>{copy.billedEvents}</span><b>{summaryRequests.toLocaleString(locale)}</b></div>
        <div className="usum-row"><span>{copy.peakDay}</span><b>{peak.value > 0n ? `${fmtUtcDay(peak.day, locale)} · ${formatNanoUsd(peak.value, locale)}` : "—"}</b></div>
        <div className="usum-row"><span>{copy.dailyAverage}</span><b>{summaryOfficialNano > 0n ? formatNanoUsd(roundDivide(summaryOfficialNano, averageDays), locale) : "—"}</b></div>
      </div>
    </div>

    <section className="dsec">
      <div className="dsec-head analytics-heading"><div><h2>{copy.tokensAndModels}</h2><p>{copy.tokensAndModelsSub}</p></div></div>
      <div className="tok-buckets">
        <div className="tokb"><span className="dlabel">{copy.inputTokens}</span><b>{fmtTokens(usage.buckets.input.tokens, locale)}</b><span className="tokb-usd">{fmtNanoUsd(usage.buckets.input.officialNano, locale)}</span></div>
        <div className="tokb"><span className="dlabel">{copy.outputTokens}</span><b>{fmtTokens(usage.buckets.output.tokens, locale)}</b><span className="tokb-usd">{fmtNanoUsd(usage.buckets.output.officialNano, locale)}</span></div>
        <div className="tokb"><span className="dlabel">{copy.cacheReadLabel}</span><b>{fmtTokens(usage.buckets.cacheRead.tokens, locale)}</b><span className="tokb-usd">{fmtNanoUsd(usage.buckets.cacheRead.officialNano, locale)}</span></div>
        <div className="tokb"><span className="dlabel">{copy.cacheWriteLabel}</span><b>{fmtTokens(usage.buckets.cacheWrite.tokens, locale)}</b><span className="tokb-usd">{fmtNanoUsd(usage.buckets.cacheWrite.officialNano, locale)}</span></div>
        {usage.buckets.webSearch.requests > 0 && <div className="tokb"><span className="dlabel">{copy.webSearchLabel}</span><b>{usage.buckets.webSearch.requests.toLocaleString(locale)}</b><span className="tokb-usd">{fmtNanoUsd(usage.buckets.webSearch.officialNano, locale)}</span></div>}
        {legacyOfficialNano > 0n && <div className="tokb tokb-legacy"><span className="dlabel">{copy.legacyUnattributed}</span><b>{copy.historicalUsage}</b><span className="tokb-usd">{fmtNanoUsd(usage.buckets.unattributedLegacy.officialNano, locale)}</span></div>}
      </div>
      {legacyOfficialNano > 0n && <p className="bucket-note">{copy.bucketAttributionNote}</p>}
      {models.length === 0 ? <div className="empty-box">{copy.tokensPending}</div> : <>
        <div className="mdist-wrap">
          <div className="mdist" role="group" aria-label={copy.tokensAndModels} onMouseLeave={(event) => { if (!event.currentTarget.contains(document.activeElement)) setMdistHover(null); }}>
            {mdistPlaced.map((seg, index) => <button type="button" aria-label={`${modelLabel(seg.model.model)} · ${fmtNanoUsd(seg.model.officialNano, locale)} · ${(seg.share * 100).toFixed(seg.share < 0.1 ? 1 : 0)}%`} key={seg.model.model} className={`mdist-seg${mdistHover === index ? " is-hover" : ""}`} style={{ width: `${seg.share * 100}%`, background: modelColor.get(seg.model.model) }} onMouseEnter={() => setMdistHover(index)} onFocus={() => setMdistHover(index)} onBlur={() => setMdistHover((current) => current === index ? null : current)} onClick={() => setMdistHover((current) => current === index ? null : index)} />)}
          </div>
          {mdistHover !== null && mdistPlaced[mdistHover] && (() => {
            const seg = mdistPlaced[mdistHover]!;
            const leftPct = Math.min(92, Math.max(8, seg.center * 100));
            return <div className="chart-tip mdist-tip" role="tooltip" style={{ left: `${leftPct}%` }}>
              <div className="chart-tip-row"><span className="chart-tip-dot" style={{ background: modelColor.get(seg.model.model) }} /><span className="chart-tip-nm">{modelLabel(seg.model.model)}</span><b>{fmtNanoUsd(seg.model.officialNano, locale)}</b></div>
              <div className="chart-tip-total"><span>{copy.shareOfUse}</span><b>{(seg.share * 100).toFixed(seg.share < 0.1 ? 1 : 0)}%</b></div>
            </div>;
          })()}
        </div>
        <div className="mdist-legend">{mdistPlaced.map((seg) => <span key={seg.model.model}><i style={{ background: modelColor.get(seg.model.model) }} />{modelLabel(seg.model.model)}<b>{(seg.share * 100).toFixed(seg.share < 0.1 ? 1 : 0)}%</b></span>)}</div>
        <p className="table-scroll-hint" id="models-table-scroll-hint">{copy.tableScrollHint}</p>
        <div className="table-scroll" role="region" tabIndex={0} aria-label={`${copy.tokensAndModels}. ${copy.tableScrollHint}`}><table className="mtable"><thead><tr><th>{copy.model}</th><th className="tnum">{copy.billedEvents}</th><th className="tnum">{copy.inputShort}</th><th className="tnum">{copy.outputShort}</th><th className="tnum">{copy.cacheRdShort}</th><th className="tnum">{copy.cacheWrShort}</th><th className="tnum">{copy.officialValueCol}</th><th className="tnum">{copy.chargedCol}</th></tr></thead>
          <tbody>{models.map((model, index) => <tr key={model.model}>
            <td><span className="tkmdl"><span className="tkmdl-dot" style={{ background: MODEL_COLORS[index % MODEL_COLORS.length] }} />{modelLabel(model.model)}{showProviderBadge && <span className="provider-tag">{providerDisplayName(model.provider, localPolicyCopy.providerUnattributed)}</span>}</span></td>
            <td className="tnum">{model.requests.toLocaleString(locale)}</td>
            <td className="tnum">{fmtTokens(model.inputTokens, locale)}</td>
            <td className="tnum">{fmtTokens(model.outputTokens, locale)}</td>
            <td className="tnum">{fmtTokens(model.cacheReadTokens, locale)}</td>
            <td className="tnum">{fmtTokens(model.cacheWrite5mTokens + model.cacheWrite1hTokens, locale)}</td>
            <td className="tnum">{fmtNanoUsd(model.officialNano, locale)}</td>
            <td className="tnum mprice">{fmtNanoUsd(model.chargedNano, locale)}</td>
          </tr>)}</tbody></table></div>
      </>}
    </section>

    <section className="dsec">
      <div className="dsec-head analytics-heading"><div><h2>{copy.usageByKey}</h2><p>{copy.usageByKeySub}</p></div></div>
      <div className="ubreak-sum">
        <div><span className="dlabel">{copy.keysCount}</span><b>{keyRows.length}</b></div>
        <div><span className="dlabel">{copy.billedEvents}</span><b>{summaryRequests.toLocaleString(locale)}</b></div>
        <div><span className="dlabel">{copy.officialValueCol}</span><b>{formatNanoUsd(summaryOfficialNano, locale)}</b></div>
        <div><span className="dlabel">{copy.chargedCol}</span><b>{formatNanoUsd(summaryChargedNano, locale)}</b></div>
      </div>
      <p className="table-scroll-hint">{copy.tableScrollHint}</p>
      <div className="table-scroll" role="region" tabIndex={0} aria-label={`${copy.usageByKey}. ${copy.tableScrollHint}`}><table className="mtable usage-key-table"><thead><tr><th>{copy.apiKey}</th><th className="tnum">{copy.billedEvents}</th><th className="tnum">{copy.officialValueCol}</th><th className="tnum">{copy.chargedCol}</th></tr></thead>
        <tbody>{keyRows.length === 0 ? <tr><td colSpan={4} className="empty-cell">{copy.noChargesPeriod}</td></tr> : keyRows.map((row) => <tr key={row.keyMasked ?? "__system__"}>
          <td>{row.keyMasked === null
            ? <span className="usage-key-label">{copy.systemCharge}</span>
            : keyLabels.has(row.keyMasked)
              ? <span className="usage-key-label">{keyLabels.get(row.keyMasked)}</span>
              : <code>{row.keyMasked}</code>}</td>
          <td className="tnum">{row.requests.toLocaleString(locale)}</td>
          <td className="tnum">{formatNanoUsd(row.officialNano, locale)}</td>
          <td className="tnum mprice">{formatNanoUsd(row.chargedNano, locale)}</td>
        </tr>)}</tbody></table></div>
    </section>

    {ledgerAvailable && <LedgerHistory ledger={ledger} mayBePartial={ledgerMayBePartial} />}
  </section>;
}

// История ledger сгруппирована по дням: компактные строки-дни (кол-во запросов + сумма), каждая
// раскрывается в отдельные списания. Топапы/коррекции — отдельными выделенными строками. Так вместо
// «вечного полотна» из сотен per-request строк видно читаемую сводку, а детали — по клику.
function LedgerHistory({ ledger, mayBePartial = false }: { ledger: LedgerEntry[]; mayBePartial?: boolean }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localCopy = localDashboardCopy[language];
  const attributionCopy = policyCopy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  if (ledger.length === 0) return <section className="dsec"><h2>{copy.transactions}</h2><div className="empty-box">{copy.noLedger}</div></section>;

  const groups = new Map<number, { day: number; charges: LedgerEntry[]; events: LedgerEntry[] }>();
  for (const entry of ledger) {
    const day = startOfDay(ledgerMs(entry.timestamp));
    const group = groups.get(day) ?? { day, charges: [], events: [] };
    if (entry.kind === "charge") group.charges.push(entry); else group.events.push(entry);
    groups.set(day, group);
  }
  const days = [...groups.values()].sort((a, b) => b.day - a.day);
  const CAP = 50;

  return <section className="dsec"><h2>{copy.transactions}</h2>
    {mayBePartial && <div className="banner">{localCopy.partialLedger}</div>}
    <div className="txh">
      {days.map((group) => {
        const chargeNano = group.charges.reduce((sum, entry) => sum + BigInt(entry.amountNano), 0n);
        return <div className="txh-day" key={group.day}>
          <div className="txh-date">{new Date(group.day).toLocaleDateString(locale, { weekday: "short", month: "short", day: "numeric", year: "numeric" })}</div>
          {group.events.map((entry) => <div className={`txh-ev ${entry.kind}`} key={entry.id}>
            <span className={`pill ${entry.kind === "topup" ? "pill-good" : "pill-soft"}`}>{entry.kind === "topup" ? copy.topupType : copy.adjustType}</span>
            <span className="txh-ev-ref">{entry.reference ?? "—"}</span>
            <span className="txh-ev-amt">{entry.kind === "topup" ? "+" : ""}{formatNanoUsdSmart(BigInt(entry.amountNano), locale)}</span>
          </div>)}
          {group.charges.length > 0 && <details className="txh-charges">
            <summary><span className="txh-sum-l"><span className="txh-ic" aria-hidden="true">▸</span>{formatBilledEventCount(group.charges.length, locale, copy)}</span><span className="txh-sum-amt">−{formatNanoUsdSmart(chargeNano, locale)}</span></summary>
            <div className="txh-list">
              {group.charges.slice(0, CAP).map((entry) => {
                const funding = fundingSummary(entry, attributionCopy);
                return <div className="txh-row" key={entry.id}>
                  <span className="txh-time">{new Date(ledgerMs(entry.timestamp)).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", second: "2-digit" })}</span>
                  <code className="txh-key">{entry.keyMasked ?? "—"}</code>
                  <span className="txh-ref">{entry.model ? modelLabel(entry.model) : entry.reference ?? "—"}{entry.provider ? ` · ${providerDisplayName(entry.provider, attributionCopy.providerUnattributed)}` : ""}{funding ? ` · ${funding}` : ""}</span>
                  <span className="txh-amt" title={attributionCopy.officialVsCharged}>{entry.officialNano != null ? `${formatNanoUsdSmart(BigInt(entry.officialNano), locale)} → ` : ""}{formatNanoUsdSmart(BigInt(entry.amountNano), locale)}</span>
                </div>;
              })}
              {group.charges.length > CAP && <div className="txh-more">{interpolate(copy.moreRows, { n: group.charges.length - CAP })}</div>}
            </div>
          </details>}
        </div>;
      })}
    </div>
  </section>;
}

// Мелкие суммы (суб-цент) не округляем в "$0" — показываем честно до значащих знаков.
function formatNanoUsdSmart(value: bigint, locale: string): string {
  if (value === 0n) return "$0.00";
  if (absoluteBigInt(value) >= 10_000_000n) return formatNanoUsd(value, locale, 2, 2);
  return formatNanoUsd(value, locale, 0, 9);
}

function startOfDay(ms: number): number { const date = new Date(ms); date.setHours(0, 0, 0, 0); return date.getTime(); }
function ledgerMs(timestamp: string): number { const numeric = Number(timestamp); return numeric < 10_000_000_000 ? numeric * 1_000 : numeric; }
function fmtUtcDay(ms: number, locale: string): string { return new Date(ms).toLocaleDateString(locale, { month: "numeric", day: "numeric", timeZone: "UTC" }); }
function formatBilledEventCount(count: number, locale: string, copy: DashboardCopy): string {
  const plural = new Intl.PluralRules(locale).select(count);
  const template = plural === "one" ? copy.billedEventOne : plural === "few" ? copy.billedEventsFew : copy.apiRequestsN;
  return interpolate(template, { n: count });
}
// «Красивая» шкала оси Y на целых нано-USD. В number переводятся только ограниченные отношения для CSS.
function niceNanoScale(max: bigint): { max: bigint; step: bigint; divisions: number } {
  const divisions = 4;
  if (max <= 0n) return { max: NANO_PER_USD, step: NANO_PER_USD / 4n, divisions };
  const rough = (max + BigInt(divisions) - 1n) / BigInt(divisions);
  const magnitude = 10n ** BigInt(Math.max(0, rough.toString().length - 1));
  const candidates = [magnitude, 2n * magnitude, 5n * magnitude, 10n * magnitude];
  const step = candidates.find((candidate) => candidate >= rough) ?? 10n * magnitude;
  return { max: step * BigInt(divisions), step, divisions };
}
function formatAxisNanoUsd(value: bigint, locale: string): string {
  if (value <= 0n) return "$0";
  if (value >= NANO_PER_USD) return formatNanoUsd(value, locale, 0, 1);
  if (value >= 10_000_000n) return formatNanoUsd(value, locale, 0, 2);
  if (value >= 100_000n) return formatNanoUsd(value, locale, 0, 4);
  return formatNanoUsd(value, locale, 0, 9);
}

// Палитра сегментов по моделям — средние тона, читаются и на светлой, и на тёмной теме.
const MODEL_COLORS = ["#3767f0", "#7c5cff", "#12a594", "#e0913a", "#d6455d", "#8b8f9a"];
function fmtTokens(n: number, locale: string): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toLocaleString(locale, { maximumFractionDigits: 2 })}M`;
  if (n >= 1_000) return `${(n / 1_000).toLocaleString(locale, { maximumFractionDigits: 1 })}K`;
  return n.toLocaleString(locale);
}
function fmtNanoUsd(nano: string, locale: string): string {
  const value = BigInt(nano);
  if (value > 0n && value < 10_000_000n) return "<$0.01";
  return formatNanoUsd(value, locale, 2, 2);
}

function pricingRuleLabel(
  rule: CustomerPricingRuleView | null,
  copy: typeof policyCopy.en | typeof policyCopy.ru,
): string {
  if (!rule) return copy.noRule;
  if (rule.pricingMode === "track") {
    return `${copy.progressive} · ${(100 - rule.payableMultiplierBp / 100).toLocaleString()}%`;
  }
  if (rule.discountBps !== null) return `−${rule.discountBps / 100}%`;
  return `${copy.legacy} · ${(100 - rule.payableMultiplierBp / 100).toLocaleString()}%`;
}

function providerRuleSummary(
  models: readonly CustomerPricingModelView[],
  copy: typeof policyCopy.en | typeof policyCopy.ru,
): string {
  const labels = new Set(models.filter((model) => model.available)
    .map((model) => pricingRuleLabel(model.rule, copy)));
  if (labels.size === 0) return copy.noRule;
  if (labels.size > 1) return copy.mixed;
  return [...labels][0]!;
}

function providerDisplayName(providerId: string | null | undefined, unattributed: string): string {
  if (!providerId || providerId === "unattributed") return unattributed;
  return DASHBOARD_PROVIDERS.find((provider) => provider.id === providerId)?.name ?? providerId;
}

function fundingSummary(
  entry: LedgerEntry,
  copy: typeof policyCopy.en | typeof policyCopy.ru,
): string {
  const sources = new Set((entry.fundingAllocations ?? []).map((allocation) => (
    allocation.sourceType === "paid"
      ? copy.paidFunding
      : allocation.sourceType === "welcome_track_bonus"
        ? copy.bonusFunding
        : allocation.sourceType
  )));
  return [...sources].join(" + ");
}
function boundedRatio(numerator: bigint, denominator: bigint): number {
  if (denominator <= 0n || numerator <= 0n) return 0;
  const scale = 1_000_000n;
  const bounded = bigintMax(0n, numerator > denominator ? denominator : numerator);
  return Number(bounded * scale / denominator) / Number(scale);
}
function boundedPercent(numerator: bigint, denominator: bigint): number {
  return boundedRatio(numerator, denominator) * 100;
}
function bigintMax(left: bigint, right: bigint): bigint { return left > right ? left : right; }
function absoluteBigInt(value: bigint): bigint { return value < 0n ? -value : value; }
