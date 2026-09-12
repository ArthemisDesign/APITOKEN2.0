"use client";

import Link from "next/link";
import { UsageTrend, usageChartLabels } from "./usage-trend";
import { useState, type CSSProperties } from "react";
import type {
  AccountView,
  ApiKeyView,
  LedgerEntry,
  UsageView,
} from "@/lib/api";
import { useI18n } from "@/components/i18n-provider";
import type { DashboardCopy } from "@/lib/dashboard-copy";
import { modelLabel } from "@/lib/model-label";
import { DASHBOARD_PROVIDERS, fallbackProvider } from "@/lib/providers";
import {
  PageHeading, Stat,
  compareBigInt, formatNanoUsd, interpolate, localDashboardCopy, useDashboardCopy,
} from "./shared";

const policyCopy = {
  en: {
    yourDiscount: "Your discount",
    offListPrice: "off the official rate",
    unavailable: "Unavailable",
    pricingRule: "Pricing rule",
    noRule: "No discount",
    providerUnattributed: "Unattributed",
    officialVsCharged: "Official → charged",
  },
  ru: {
    yourDiscount: "Ваша скидка",
    offListPrice: "от официального тарифа",
    unavailable: "Недоступно",
    pricingRule: "Правило тарификации",
    noRule: "Без скидки",
    providerUnattributed: "Без провайдера",
    officialVsCharged: "Официальная стоимость → списано",
  },
} as const;

export function Usage({ account, keys, ledger, usage, ledgerAvailable }: { account: AccountView; keys: ApiKeyView[]; ledger: LedgerEntry[]; usage: UsageView; ledgerAvailable: boolean }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localPolicyCopy = policyCopy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const providerRegistry = new Map(DASHBOARD_PROVIDERS.map((provider) => [provider.id, provider]));
  const providerMetadata = (id: string) => providerRegistry.get(id) ?? fallbackProvider(
    id,
    id === "unattributed" ? localPolicyCopy.providerUnattributed : id,
  );
  const models = usage.models;
  const modelOfficialTotal = models.reduce((sum, model) => sum + BigInt(model.officialNano), 0n);

  // Stable model colours are shared by the ranked model bars and model table.
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
  // Every provider we sell is available to every account: one discount prices all of them, so a
  // provider can no longer be missing from a per-account catalog and invisible until first use.
  const providerIds = new Set([
    ...DASHBOARD_PROVIDERS.map((provider) => provider.id),
    ...providerAgg.keys(),
  ]);
  const providerCards = [...providerIds].sort().map((id) => ({
    ...providerMetadata(id),
    agg: providerAgg.get(id),
  }));
  const [copiedProvider, setCopiedProvider] = useState<string | null>(null);

  async function copyProviderEndpoint(id: string, endpoint: string) {
    await navigator.clipboard.writeText(endpoint);
    setCopiedProvider(id);
    window.setTimeout(() => setCopiedProvider((current) => (current === id ? null : current)), 1_200);
  }

  const summaryOfficialNano = BigInt(usage.totalOfficialNano);
  const summaryChargedNano = BigInt(usage.totalChargedNano);
  const summaryRequests = usage.requests;
  const rankedModels = [...models].sort((a, b) => compareBigInt(BigInt(b.officialNano), BigInt(a.officialNano)));

  const keyRows = [...usage.keys].sort((left, right) => compareBigInt(BigInt(right.officialNano), BigInt(left.officialNano)));
  const keyLabels = new Map(keys.flatMap((key) => key.label ? [[key.keyMasked, key.label] as const] : []));
  const ledgerMayBePartial = ledger.length >= 100;
  const legacyOfficialNano = BigInt(usage.buckets.unattributedLegacy.officialNano);

  return <section className="panel usage-dashboard"><PageHeading eyebrow={copy.usageEyebrow} title={copy.usageTitle} subtitle={copy.usageSubtitle} />
    <div className="banner">💡 <b>{copy.sessionSavingTitle}</b><span> {copy.sessionSavingText}</span></div>

    <div className="ov-stats bill4 usage-kpis">
      <div className="ovstat"><span className="dlabel">{copy.officialValue30d}</span><b className="num accent">{formatNanoUsd(summaryOfficialNano, locale)}</b><span className="dtrend">{copy.listPriceEquivalent}</span></div>
      <Stat label={copy.charged30d} value={formatNanoUsd(summaryChargedNano, locale)} detail={copy.settledCredits} />
      <div className="ovstat"><span className="dlabel">{localPolicyCopy.yourDiscount}</span><b className="num">{accountDiscountLabel(account.markupBasisPoints, localPolicyCopy)}</b><span className="dtrend">{localPolicyCopy.offListPrice}</span></div>
      <Stat label={copy.available} value={formatNanoUsd(account.balanceNano, locale)} detail={copy.available} />
    </div>

    <section className="dsec uproviders usage-providers-section">
      <div className="dsec-head analytics-heading"><div><h2>{copy.usageProviders}</h2><p>{copy.usageProvidersSub}</p></div></div>
      <div className="uprovider-grid">
        {providerCards.map((card) => {
          const isActive = (card.agg?.requests ?? 0) > 0;
          // The account multiplier is exactly what settlement applies to this provider, so the
          // card shows the number the invoice will show rather than a nearby approximation.
          const available = true;
          const ruleSummary = accountDiscountLabel(account.markupBasisPoints, localPolicyCopy);
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
              <span className={`uprovider-status${available ? " is-active" : ""}`}>{available ? copy.ready : localPolicyCopy.unavailable}</span>
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
    </section>

    <UsageTrend usage={usage} />

    <section className="dsec usage-models-section">
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
        <section className="usage-model-ranking" aria-label={copy.shareOfUse}>
          <h3>{usageChartLabels[language].models}</h3>
          <ol className="usage-model-bars">{rankedModels.map((model, index) => {
            const percent = boundedRatio(BigInt(model.officialNano), modelOfficialTotal) * 100;
            const shareLabel = `${percent.toLocaleString(locale, { maximumFractionDigits: 1 })}%`;
            return <li key={model.model}>
              <div className="usage-model-bar-head"><span>{String(index + 1).padStart(2, "0")}</span><strong>{modelLabel(model.model)}</strong><b>{formatNanoUsdSmart(BigInt(model.officialNano), locale)}</b></div>
              <div className="usage-model-bar-bottom"><div className="usage-model-bar-track" aria-hidden="true"><i style={{ width: `${percent}%`, background: modelColor.get(model.model) }} /></div><span>{shareLabel}</span></div>
            </li>;
          })}</ol>
        </section>
        <p className="table-scroll-hint" id="models-table-scroll-hint">{copy.tableScrollHint}</p>
        <div className="table-scroll" role="region" tabIndex={0} aria-label={`${copy.tokensAndModels}. ${copy.tableScrollHint}`}><table className="mtable"><thead><tr><th>{copy.model}</th><th className="tnum">{copy.billedEvents}</th><th className="tnum">{copy.inputShort}</th><th className="tnum">{copy.outputShort}</th><th className="tnum">{copy.cacheRdShort}</th><th className="tnum">{copy.cacheWrShort}</th><th className="tnum">{copy.officialValueCol}</th><th className="tnum">{copy.chargedCol}</th></tr></thead>
          <tbody>{models.map((model) => <tr key={model.model}>
            <td><span className="tkmdl"><span className="tkmdl-dot" style={{ background: modelColor.get(model.model) }} />{modelLabel(model.model)}{showProviderBadge && <span className="provider-tag">{providerDisplayName(model.provider, localPolicyCopy.providerUnattributed)}</span>}</span></td>
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

    <section className="dsec usage-keys-section">
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
                return <div className="txh-row" key={entry.id}>
                  <span className="txh-time">{new Date(ledgerMs(entry.timestamp)).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", second: "2-digit" })}</span>
                  <code className="txh-key">{entry.keyMasked ?? "—"}</code>
                  <span className="txh-ref">{entry.model ? modelLabel(entry.model) : entry.reference ?? "—"}{entry.provider ? ` · ${providerDisplayName(entry.provider, attributionCopy.providerUnattributed)}` : ""}</span>
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
function formatBilledEventCount(count: number, locale: string, copy: DashboardCopy): string {
  const plural = new Intl.PluralRules(locale).select(count);
  const template = plural === "one" ? copy.billedEventOne : plural === "few" ? copy.billedEventsFew : copy.apiRequestsN;
  return interpolate(template, { n: count });
}
const MODEL_COLORS = ["#168578", "#638aca", "#9b79ba", "#b79450", "#6698a2", "#929aab"] as const;
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


/**
 * Discount label for a provider the pinned policy does not describe.
 *
 * `markupBasisPoints` is the account multiplier the engine bills such a model with, so this is
 * the same number the invoice will show rather than a nearby approximation.
 */
function accountDiscountLabel(
  markupBasisPoints: number,
  copy: typeof policyCopy.en | typeof policyCopy.ru,
): string {
  if (!Number.isFinite(markupBasisPoints) || markupBasisPoints <= 0 || markupBasisPoints >= 10_000) {
    return copy.noRule;
  }
  const percent = Math.round((10_000 - markupBasisPoints) / 100);
  return percent > 0 ? `-${percent}%` : copy.noRule;
}


function providerDisplayName(providerId: string | null | undefined, unattributed: string): string {
  if (!providerId || providerId === "unattributed") return unattributed;
  return DASHBOARD_PROVIDERS.find((provider) => provider.id === providerId)?.name ?? providerId;
}

function boundedRatio(numerator: bigint, denominator: bigint): number {
  if (denominator <= 0n || numerator <= 0n) return 0;
  const scale = 1_000_000n;
  const bounded = bigintMax(0n, numerator > denominator ? denominator : numerator);
  return Number(bounded * scale / denominator) / Number(scale);
}
function bigintMax(left: bigint, right: bigint): bigint { return left > right ? left : right; }
function absoluteBigInt(value: bigint): bigint { return value < 0n ? -value : value; }
