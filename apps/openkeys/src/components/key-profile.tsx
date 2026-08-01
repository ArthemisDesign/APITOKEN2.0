"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useCallback, useEffect, useState, useTransition, type CSSProperties } from "react";
import { AppShell } from "@/components/app-shell";
import { useLanguage } from "@/components/chrome";
import type { KeyUsageView } from "@/lib/keys";
import {
  MODEL_COLORS,
  bigintMax,
  boundedPercent,
  boundedRatio,
  compareBigInt,
  fmtNanoUsd,
  fmtTokens,
  fmtUtcDay,
  formatAxisNanoUsd,
  formatNanoUsd,
  formatNanoUsdSmart,
  modelLabel,
  niceNanoScale,
  roundDivide,
  usageWindowDays,
} from "@/lib/format";
import { buildUtcUsageSeries } from "@/lib/usage-series";
import { aggregateUsageProviders, usageProviderOf } from "@/lib/usage-providers";
import { UNIVERSAL_CONNECTIONS } from "@/lib/universal-key";

const copy = {
  en: {
    titleBar: "Key usage", eyebrow: "CLAUDE + GPT · SHARED BALANCE", title: "Universal API key",
    lead: "One key and one balance work across Claude API and the GPT/OpenAI-compatible API. Usage is combined below, while every request keeps the official price of the model that served it.",
    offline: "Offline — waiting for connection", syncing: "Syncing", updated: "Updated", loaded: "Data loaded",
    automatic: "automatically every 6 seconds", refresh: "Refresh usage now", copy: "Copy", copied: "Copied",
    balance: "Key balance", faceValue: "face value", remainingCompleted: "Remaining after completed requests",
    spent: "Actually spent", reserved: "Temporarily reserved", available: "Available for new requests",
    reserveNote: "After the response, the hold is replaced by the exact charge — it is not spent in full. The unused amount returns to available balance automatically.",
    key: "Key", active: "active", disabled: "disabled", issued: "issued", connectClaude: "Connect Claude",
    connectGpt: "Connect GPT", signOut: "Sign out", connections: "Two connections", oneKey: "one key", yourKey: "Your key",
    providersTitle: "Connected providers", providersDesc: "Both APIs run on this one key — point each tool at its endpoint and every request shows up here.",
    statusReady: "ready", guide: "Setup guide",
    officialCost: "Official cost", officialPrices: "at official prices of the models used", chargedKey: "Charged to key",
    last30: "last 30 days", requests: "Requests", processing: "Processing now", processingNote: "temporary hold for active requests",
    apiSpend: "Usage by API", apiSpendDesc: "One balance, with Claude and GPT/OpenAI usage shown separately for the last 30 days.",
    requestWord: "requests", tokenWord: "tokens", chargedLower: "charged", spendByDay: "Daily usage",
    noSpend: "No charges in this period yet", periodSummary: "Period summary", charged: "Charged", peakDay: "Peak day",
    dailyAverage: "Daily average", tokensModels: "Tokens and models", tokensModelsDesc: "How the cost was formed: input, output, and cache are billed at their own rates.",
    inputTokens: "Input tokens", outputTokens: "Output tokens", cacheRead: "Cache read", cacheWrite: "Cache write",
    webSearch: "Web search", unattributed: "Unattributed", earlyRequests: "early requests", modelsEmpty: "The breakdown will appear after the first requests",
    modelDistribution: "Model distribution", spendShare: "Share of usage", scrollHint: "Scroll the table horizontally",
    modelSpend: "Usage by model", model: "Model", input: "Input", output: "Output", cacheReadShort: "Cache read",
    cacheWriteShort: "Cache write", official: "Official", keySummary: "Key summary", keySummaryDesc: "Totals for the observation window.",
    remaining: "Remaining",
  },
  ru: {
    titleBar: "Расход ключа", eyebrow: "CLAUDE + GPT · ОБЩИЙ БАЛАНС", title: "Универсальный API-ключ",
    lead: "Один ключ и один баланс работают на Claude API и GPT/OpenAI-совместимом API. Расход объединён, а каждый запрос сохраняет официальный прайс модели, которая его обработала.",
    offline: "Нет сети — ждём подключения", syncing: "Синхронизация", updated: "Обновлено", loaded: "Данные загружены",
    automatic: "автоматически каждые 6 секунд", refresh: "Обновить расход сейчас", copy: "Скопировать", copied: "Скопировано",
    balance: "Баланс ключа", faceValue: "номинал", remainingCompleted: "Остаток после завершённых запросов",
    spent: "Фактически потрачено", reserved: "Временно в обработке", available: "Доступно новым запросам",
    reserveNote: "После ответа резерв заменится точной стоимостью, а не спишется целиком. Неиспользованная часть автоматически вернётся в доступный баланс.",
    key: "Ключ", active: "активен", disabled: "отключён", issued: "выпущен", connectClaude: "Подключить Claude",
    connectGpt: "Подключить GPT", signOut: "Выйти", connections: "Два подключения", oneKey: "один ключ", yourKey: "Ваш ключ",
    providersTitle: "Подключённые провайдеры", providersDesc: "Оба API работают на этом ключе — направьте каждый инструмент на его адрес, и каждый запрос появится здесь.",
    statusReady: "готов", guide: "Инструкция",
    officialCost: "Официальная стоимость", officialPrices: "по официальным прайсам использованных моделей", chargedKey: "Списано с ключа",
    last30: "за 30 дней", requests: "Запросов", processing: "Сейчас в обработке", processingNote: "временный резерв активных запросов",
    apiSpend: "Расход по API", apiSpendDesc: "Один баланс, отдельно показано использование Claude и GPT/OpenAI за последние 30 дней.",
    requestWord: "запросов", tokenWord: "токенов", chargedLower: "списано", spendByDay: "Расход по дням",
    noSpend: "За этот период списаний ещё не было", periodSummary: "Сводка за период", charged: "Списано", peakDay: "Пиковый день",
    dailyAverage: "В среднем в день", tokensModels: "Токены и модели", tokensModelsDesc: "Из чего сложился расход: вход, выход и кэш считаются по своим ставкам.",
    inputTokens: "Входные токены", outputTokens: "Выходные токены", cacheRead: "Чтение кэша", cacheWrite: "Запись кэша",
    webSearch: "Веб-поиск", unattributed: "Без разбивки", earlyRequests: "ранние запросы", modelsEmpty: "Разбивка появится после первых запросов",
    modelDistribution: "Распределение по моделям", spendShare: "Доля расхода", scrollHint: "Таблицу можно прокручивать вбок",
    modelSpend: "Расход по моделям", model: "Модель", input: "Вход", output: "Выход", cacheReadShort: "Кэш чт.",
    cacheWriteShort: "Кэш зап.", official: "Официально", keySummary: "Сводка по ключу", keySummaryDesc: "Итоги за окно наблюдения.",
    remaining: "Остаток",
  },
} as const;

const PROVIDER_COLORS = {
  anthropic: "#d97757",
  openai: "#10a37f",
  unattributed: "#6f7a8a",
} as const;

function CopyMiniButton({ value, label, copiedLabel }: { value: string; label: string; copiedLabel: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className="key-provider-copy"
      onClick={() => {
        void navigator.clipboard.writeText(value).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1600);
        });
      }}
    >
      {copied ? copiedLabel : label}
    </button>
  );
}

export function KeyProfile({ view, showSignOut = false }: { view: KeyUsageView; showSignOut?: boolean }) {
  const { language } = useLanguage();
  const t = copy[language];
  const locale = language === "en" ? "en-US" : "ru-RU";
  const [hoverDay, setHoverDay] = useState<number | null>(null);
  const [mdistHover, setMdistHover] = useState<number | null>(null);
  const [lastUpdatedAt, setLastUpdatedAt] = useState<Date | null>(null);
  const [isOnline, setIsOnline] = useState(true);
  const [isRefreshing, startRefresh] = useTransition();
  const router = useRouter();

  const refreshUsage = useCallback(() => {
    if (document.visibilityState !== "visible" || !navigator.onLine || isRefreshing) return;
    startRefresh(() => router.refresh());
  }, [isRefreshing, router]);

  useEffect(() => {
    setLastUpdatedAt(new Date());
  }, [view]);

  useEffect(() => {
    const syncNetworkState = () => {
      setIsOnline(navigator.onLine);
      if (navigator.onLine) refreshUsage();
    };
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") refreshUsage();
    };
    const interval = window.setInterval(refreshUsage, 6_000);
    setIsOnline(navigator.onLine);
    window.addEventListener("online", syncNetworkState);
    window.addEventListener("offline", syncNetworkState);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      window.clearInterval(interval);
      window.removeEventListener("online", syncNetworkState);
      window.removeEventListener("offline", syncNetworkState);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  }, [refreshUsage]);

  const usage = view.usage;
  const faceValueNano = BigInt(view.faceValueNano);
  const officialAvailable = BigInt(view.officialAvailableNano);
  const officialReserved = BigInt(view.officialReservedNano);
  const officialRemaining = BigInt(view.officialRemainingNano);
  const officialSpent = BigInt(view.officialSpentNano);
  const spentPercent = faceValueNano > 0n ? boundedPercent(officialSpent, faceValueNano) : 0;
  const reservedPercent = faceValueNano > 0n
    ? Math.min(boundedPercent(officialReserved, faceValueNano), 100 - spentPercent)
    : 0;

  const models = usage?.models ?? [];
  const providerSummaries = aggregateUsageProviders(models);
  const providerCards = ([
    {
      id: "claude" as const,
      connection: UNIVERSAL_CONNECTIONS.claude,
      name: "Claude",
      api: "Anthropic Messages API",
      color: PROVIDER_COLORS.anthropic,
    },
    {
      id: "openai" as const,
      connection: UNIVERSAL_CONNECTIONS.openai,
      name: "GPT",
      api: "OpenAI-compatible API",
      color: PROVIDER_COLORS.openai,
    },
  ]).map((entry) => ({
    ...entry,
    summary: providerSummaries.find((summary) => summary.provider === entry.id) ?? {
      requests: 0,
      tokens: 0,
      officialNano: 0n,
      chargedNano: 0n,
    },
  }));
  const modelOfficialTotal = models.reduce((sum, model) => sum + BigInt(model.official_nano), 0n);
  const modelColor = new Map<string, string>();
  for (const model of models) {
    if (!modelColor.has(model.model)) modelColor.set(model.model, MODEL_COLORS[modelColor.size % MODEL_COLORS.length]!);
  }

  const dailyProviders = new Map<number, {
    anthropic: bigint;
    openai: bigint;
    anthropicRequests: number;
    openaiRequests: number;
  }>();
  for (const row of usage?.daily_providers ?? []) {
    const current = dailyProviders.get(row.day_ts) ?? {
      anthropic: 0n,
      openai: 0n,
      anthropicRequests: 0,
      openaiRequests: 0,
    };
    if (row.provider === "openai") {
      current.openai += BigInt(row.official_nano);
      current.openaiRequests += row.requests;
    } else {
      current.anthropic += BigInt(row.official_nano);
      current.anthropicRequests += row.requests;
    }
    dailyProviders.set(row.day_ts, current);
  }

  const series = usage
    ? buildUtcUsageSeries(
        usage.since_ts,
        usage.until_ts,
        usage.daily.map((row) => ({
          dayTs: row.day_ts,
          requests: row.requests,
          officialNano: row.official_nano,
          chargedNano: row.charged_nano,
        })),
      ).map((point) => {
        const providers = dailyProviders.get(point.dayTs) ?? {
          anthropic: 0n,
          openai: 0n,
          anthropicRequests: 0,
          openaiRequests: 0,
        };
        const value = BigInt(point.officialNano);
        const attributed = providers.anthropic + providers.openai;
        return {
          day: point.dayTs * 1_000,
          requests: point.requests,
          value,
          charged: BigInt(point.chargedNano),
          anthropic: providers.anthropic,
          openai: providers.openai,
          unattributed: value > attributed ? value - attributed : 0n,
          anthropicRequests: providers.anthropicRequests,
          openaiRequests: providers.openaiRequests,
        };
      })
    : [];

  const maxValue = series.reduce((max, point) => bigintMax(max, point.value), 0n);
  const scale = niceNanoScale(maxValue);
  const gridTicks = Array.from({ length: scale.divisions + 1 }, (_, index) => scale.max - BigInt(index) * scale.step);
  const summaryOfficialNano = BigInt(usage?.total_official_nano ?? "0");
  const summaryChargedNano = BigInt(usage?.total_charged_nano ?? "0");
  const summaryRequests = usage?.requests ?? 0;
  const peak = series.reduce((best, point) => (point.value > best.value ? point : best), {
    day: (usage?.since_ts ?? 0) * 1_000,
    requests: 0,
    value: 0n,
    charged: 0n,
    anthropic: 0n,
    openai: 0n,
    unattributed: 0n,
    anthropicRequests: 0,
    openaiRequests: 0,
  });
  const averageDays = BigInt(usage ? usageWindowDays(usage.since_ts, usage.until_ts) : 1);

  const LABEL_COUNT = 7;
  const axisMarkCount = Math.min(LABEL_COUNT, series.length);
  const axisMarks =
    series.length === 0
      ? []
      : [
          ...new Set(
            Array.from({ length: axisMarkCount }, (_, index) =>
              Math.round((index * (series.length - 1)) / Math.max(1, axisMarkCount - 1)),
            ),
          ),
        ];

  const modelShares = models.map((model) =>
    modelOfficialTotal > 0n ? boundedRatio(BigInt(model.official_nano), modelOfficialTotal) : 1 / models.length,
  );
  const mdistPlaced = models.map((model, index) => {
    const share = modelShares[index]!;
    const center = modelShares.slice(0, index).reduce((sum, value) => sum + value, 0) + share / 2;
    return { model, share, center };
  });

  const legacyOfficialNano = BigInt(usage?.buckets.unattributed_legacy.official_nano ?? "0");
  const keyRows = [...(usage?.keys ?? [])].sort((left, right) =>
    compareBigInt(BigInt(right.official_nano), BigInt(left.official_nano)),
  );

  return (
    <AppShell section="profile" title={t.titleBar}>

      <div className="app-body">
      <div className="app-body-in">
        <div className="page-heading">
          <span className="eyebrow">{t.eyebrow}</span>
          <h1 className="p-h1">{t.title}</h1>
          <p className="p-sub">{t.lead}</p>
          <div className="usage-live-row" aria-live="polite">
            <span className={`usage-live-status${isRefreshing ? " syncing" : ""}${!isOnline ? " offline" : ""}`}>
              <i aria-hidden="true" />
              {!isOnline
                ? t.offline
                : isRefreshing
                  ? t.syncing
                  : lastUpdatedAt
                    ? `${t.updated} ${lastUpdatedAt.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`
                    : t.loaded}
            </span>
            <span className="usage-live-note">{t.automatic}</span>
            <button
              type="button"
              className="usage-refresh-button"
              onClick={refreshUsage}
              disabled={isRefreshing || !isOnline}
              aria-label={t.refresh}
              title={t.refresh}
            >
              ↻
            </button>
          </div>
        </div>

        <div className="overview-primary-grid overview-primary-grid--solo">
          <article className="card overview-balance-card">
            <div className="overview-card-head">
              <span className="overview-card-label">{t.balance}</span>
              <span className="overview-rate-chip">{t.faceValue} {formatNanoUsd(faceValueNano, 0, 0)}</span>
            </div>
            <div className="overview-balance-main">
              <div className="overview-balance-figure">
                <span>{t.remainingCompleted}</span>
                <strong key={view.officialRemainingNano} className="overview-balance-number">
                  {formatNanoUsd(officialRemaining, 2, 2)}
                </strong>
              </div>
              <div className="overview-balance-detail">
                <div className="overview-balance-breakdown">
                  <div>
                    <span><i className="spent" aria-hidden="true" />{t.spent}</span>
                    <b>{formatNanoUsd(officialSpent, 2, 2)}</b>
                  </div>
                  <div>
                    <span><i className="reserved" aria-hidden="true" />{t.reserved}</span>
                    <b>{formatNanoUsd(officialReserved, 2, 2)}</b>
                  </div>
                  <div>
                    <span><i className="available" aria-hidden="true" />{t.available}</span>
                    <b>{formatNanoUsd(officialAvailable, 2, 2)}</b>
                  </div>
                </div>
                <div className="overview-usage-track" aria-hidden="true">
                  <i className="spent" style={{ width: `${spentPercent}%` }} />
                  <i className="reserved" style={{ width: `${reservedPercent}%` }} />
                </div>
                {officialReserved > 0n && (
                  <p className="overview-reserve-note">{t.reserveNote}</p>
                )}
                <p className="overview-balance-rate">
                  {t.key} {view.status === "active" ? t.active : t.disabled} · {t.issued}{" "}
                  {view.createdAt.slice(0, 10)}
                </p>
                <div className="overview-card-actions">
                  <Link className="btn btn-primary btn-sm" href={UNIVERSAL_CONNECTIONS.claude.docsPath}>
                    {t.connectClaude}
                  </Link>
                  <Link className="btn btn-ghost btn-sm" href={UNIVERSAL_CONNECTIONS.openai.docsPath}>
                    {t.connectGpt}
                  </Link>
                  {showSignOut ? (
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      onClick={() => {
                        void fetch("/api/usage/logout", { method: "POST" }).then(() => {
                          window.location.assign("/profile");
                        });
                      }}
                    >
                      {t.signOut}
                    </button>
                  ) : null}
                </div>
              </div>
            </div>
          </article>

        </div>

        <section className="dsec key-providers-section">
          <div className="dsec-head analytics-heading">
            <div>
              <h2>{t.providersTitle}</h2>
              <p>{t.providersDesc}</p>
            </div>
          </div>
          <div className="key-providers-grid">
            {providerCards.map(({ id, connection, name, api, color, summary }) => {
              const isActive = summary.requests > 0;
              return (
                <article
                  className="card key-provider-card"
                  key={id}
                  style={{ "--provider-color": color } as CSSProperties}
                >
                  <div className="key-provider-head">
                    <span className={`key-provider-logo key-provider-logo-${id}`} aria-hidden="true" />
                    <div className="key-provider-name">
                      <strong>{name}</strong>
                      <span>{api}</span>
                    </div>
                    <span className={`key-provider-status${isActive ? " is-active" : ""}`}>
                      {isActive ? t.active : t.statusReady}
                    </span>
                  </div>
                  <div className="key-provider-endpoint">
                    <code>{connection.baseUrl.replace("https://", "")}</code>
                    <span>{connection.authHeader}</span>
                    <CopyMiniButton value={connection.baseUrl} label={t.copy} copiedLabel={t.copied} />
                  </div>
                  <div className="key-provider-stats">
                    <strong>{formatNanoUsd(summary.chargedNano, 2, 2)}</strong>
                    <span>
                      {summary.requests.toLocaleString(locale)} {t.requestWord} · {fmtTokens(summary.tokens)}{" "}
                      {t.tokenWord} · {t.last30}
                    </span>
                  </div>
                  <Link className="key-provider-guide" href={connection.docsPath}>
                    {t.guide} →
                  </Link>
                </article>
              );
            })}
          </div>
        </section>

        <div className="ov-stats bill4">
          <div className="ovstat">
            <span className="dlabel">{t.officialCost}</span>
            <b className="num accent">{formatNanoUsd(summaryOfficialNano)}</b>
            <span className="dtrend">{t.officialPrices}</span>
          </div>
          <div className="ovstat">
            <span className="dlabel">{t.chargedKey}</span>
            <b className="num">{formatNanoUsd(summaryChargedNano)}</b>
            <span className="dtrend">{t.last30}</span>
          </div>
          <div className="ovstat">
            <span className="dlabel">{t.requests}</span>
            <b className="num">{summaryRequests.toLocaleString(locale)}</b>
            <span className="dtrend">{t.last30}</span>
          </div>
          <div className="ovstat">
            <span className="dlabel">{t.processing}</span>
            <b className={`num${officialReserved > 0n ? " pending" : ""}`}>
              {formatNanoUsd(officialReserved, 2, 2)}
            </b>
            <span className="dtrend">{t.processingNote}</span>
          </div>
        </div>

        <div className="usage-graph">
          <div className="uchart">
            <div className="uchart-head">
              <b>{t.spendByDay}</b>
              <div className="usage-chart-legend" aria-label={t.apiSpend}>
                <span><i style={{ background: PROVIDER_COLORS.anthropic }} />Claude</span>
                <span><i style={{ background: PROVIDER_COLORS.openai }} />GPT / OpenAI</span>
                {series.some((point) => point.unattributed > 0n) && (
                  <span><i style={{ background: PROVIDER_COLORS.unattributed }} />{t.unattributed}</span>
                )}
              </div>
            </div>
            {maxValue === 0n ? (
              <div className="uchart-empty">{t.noSpend}</div>
            ) : (
              <div className="uchart-grid">
                <div className="uchart-yaxis">
                  {gridTicks.map((tick, index) => (
                    <span key={index}>{formatAxisNanoUsd(tick)}</span>
                  ))}
                </div>
                <div className="uchart-plotwrap">
                  <div className="uchart-lines">
                    {gridTicks.map((_, index) => (
                      <i key={index} />
                    ))}
                  </div>
                  <div
                    className="uchart-plot"
                    onMouseLeave={(event) => {
                      if (!event.currentTarget.contains(document.activeElement)) setHoverDay(null);
                    }}
                  >
                    {series.map((point, index) => (
                      <button
                        type="button"
                        key={point.day}
                        className={`uchart-col${hoverDay === index ? " is-hover" : ""}`}
                        aria-label={`${fmtUtcDay(point.day, locale)}: ${formatNanoUsdSmart(point.value)}`}
                        onMouseEnter={() => setHoverDay(index)}
                        onFocus={() => setHoverDay(index)}
                        onBlur={() => setHoverDay((current) => (current === index ? null : current))}
                        onClick={() => setHoverDay((current) => (current === index ? null : index))}
                      >
                        <div className="uchart-col-fill">
                          {point.anthropic > 0n && (
                            <div
                              className="uchart-seg provider-anthropic"
                              style={{ height: `${boundedPercent(point.anthropic, scale.max)}%`, background: PROVIDER_COLORS.anthropic }}
                            />
                          )}
                          {point.openai > 0n && (
                            <div
                              className="uchart-seg provider-openai"
                              style={{ height: `${boundedPercent(point.openai, scale.max)}%`, background: PROVIDER_COLORS.openai }}
                            />
                          )}
                          {point.unattributed > 0n && (
                            <div
                              className="uchart-seg provider-unattributed"
                              style={{ height: `${boundedPercent(point.unattributed, scale.max)}%`, background: PROVIDER_COLORS.unattributed }}
                            />
                          )}
                        </div>
                      </button>
                    ))}
                    {hoverDay !== null && series[hoverDay] && series[hoverDay]!.value > 0n && (
                      <div
                        className="chart-tip"
                        role="tooltip"
                        style={{
                          left: `${Math.min(92, Math.max(8, ((hoverDay + 0.5) / series.length) * 100))}%`,
                          bottom: `${boundedPercent(series[hoverDay]!.value, scale.max)}%`,
                        }}
                      >
                        <div className="chart-tip-h">{fmtUtcDay(series[hoverDay]!.day, locale)}</div>
                        {series[hoverDay]!.anthropic > 0n && (
                          <div className="chart-tip-row">
                            <span className="chart-tip-dot" style={{ background: PROVIDER_COLORS.anthropic }} />
                            <span className="chart-tip-nm">Claude</span>
                            <b>{formatNanoUsdSmart(series[hoverDay]!.anthropic)}</b>
                          </div>
                        )}
                        {series[hoverDay]!.openai > 0n && (
                          <div className="chart-tip-row">
                            <span className="chart-tip-dot" style={{ background: PROVIDER_COLORS.openai }} />
                            <span className="chart-tip-nm">GPT / OpenAI</span>
                            <b>{formatNanoUsdSmart(series[hoverDay]!.openai)}</b>
                          </div>
                        )}
                        {series[hoverDay]!.unattributed > 0n && (
                          <div className="chart-tip-row">
                            <span className="chart-tip-dot" style={{ background: PROVIDER_COLORS.unattributed }} />
                            <span className="chart-tip-nm">{t.unattributed}</span>
                            <b>{formatNanoUsdSmart(series[hoverDay]!.unattributed)}</b>
                          </div>
                        )}
                        <div className="chart-tip-total">
                          <span>{t.officialCost}</span>
                          <b>{formatNanoUsdSmart(series[hoverDay]!.value)}</b>
                        </div>
                        <div className="chart-tip-total">
                          <span>{t.charged}</span>
                          <b>{formatNanoUsdSmart(series[hoverDay]!.charged)}</b>
                        </div>
                        <div className="chart-tip-total">
                          <span>{t.requests}</span>
                          <b>{series[hoverDay]!.requests.toLocaleString(locale)}</b>
                        </div>
                      </div>
                    )}
                  </div>
                  <div className="uchart-axis">
                    {axisMarks.map((mark) => (
                      <span key={mark} style={{ left: `${((mark + 0.5) / series.length) * 100}%` }}>
                        {fmtUtcDay(series[mark]!.day, locale)}
                      </span>
                    ))}
                  </div>
                </div>
              </div>
            )}
          </div>
          <div className="usum">
            <span className="usum-t">{t.periodSummary}</span>
            <div className="usum-row">
              <span>{t.officialCost}</span>
              <b className="accent">{formatNanoUsd(summaryOfficialNano)}</b>
            </div>
            <div className="usum-row">
              <span>{t.charged}</span>
              <b>{formatNanoUsd(summaryChargedNano)}</b>
            </div>
            <div className="usum-row">
              <span>{t.requests}</span>
              <b>{summaryRequests.toLocaleString(locale)}</b>
            </div>
            <div className="usum-row">
              <span>{t.peakDay}</span>
              <b>{peak.value > 0n ? `${fmtUtcDay(peak.day, locale)} · ${formatNanoUsd(peak.value)}` : "—"}</b>
            </div>
            <div className="usum-row">
              <span>{t.dailyAverage}</span>
              <b>{summaryOfficialNano > 0n ? formatNanoUsd(roundDivide(summaryOfficialNano, averageDays)) : "—"}</b>
            </div>
          </div>
        </div>

        <section className="dsec">
          <div className="dsec-head analytics-heading">
            <div>
              <h2>{t.tokensModels}</h2>
              <p>{t.tokensModelsDesc}</p>
            </div>
          </div>
          <div className="tok-buckets">
            <div className="tokb">
              <span className="dlabel">{t.inputTokens}</span>
              <b>{fmtTokens(usage?.buckets.input.tokens ?? 0)}</b>
              <span className="tokb-usd">{fmtNanoUsd(usage?.buckets.input.official_nano ?? "0")}</span>
            </div>
            <div className="tokb">
              <span className="dlabel">{t.outputTokens}</span>
              <b>{fmtTokens(usage?.buckets.output.tokens ?? 0)}</b>
              <span className="tokb-usd">{fmtNanoUsd(usage?.buckets.output.official_nano ?? "0")}</span>
            </div>
            <div className="tokb">
              <span className="dlabel">{t.cacheRead}</span>
              <b>{fmtTokens(usage?.buckets.cache_read.tokens ?? 0)}</b>
              <span className="tokb-usd">{fmtNanoUsd(usage?.buckets.cache_read.official_nano ?? "0")}</span>
            </div>
            <div className="tokb">
              <span className="dlabel">{t.cacheWrite}</span>
              <b>{fmtTokens(usage?.buckets.cache_write.tokens ?? 0)}</b>
              <span className="tokb-usd">{fmtNanoUsd(usage?.buckets.cache_write.official_nano ?? "0")}</span>
            </div>
            {(usage?.buckets.web_search.requests ?? 0) > 0 && (
              <div className="tokb">
                <span className="dlabel">{t.webSearch}</span>
                <b>{(usage?.buckets.web_search.requests ?? 0).toLocaleString(locale)}</b>
                <span className="tokb-usd">{fmtNanoUsd(usage?.buckets.web_search.official_nano ?? "0")}</span>
              </div>
            )}
            {legacyOfficialNano > 0n && (
              <div className="tokb tokb-legacy">
                <span className="dlabel">{t.unattributed}</span>
                <b>{t.earlyRequests}</b>
                <span className="tokb-usd">{fmtNanoUsd(usage!.buckets.unattributed_legacy.official_nano)}</span>
              </div>
            )}
          </div>

          {models.length === 0 ? (
            <div className="empty-box">{t.modelsEmpty}</div>
          ) : (
            <>
              <div className="mdist-wrap">
                <div
                  className="mdist"
                  role="group"
                  aria-label={t.modelDistribution}
                  onMouseLeave={(event) => {
                    if (!event.currentTarget.contains(document.activeElement)) setMdistHover(null);
                  }}
                >
                  {mdistPlaced.map((seg, index) => (
                    <button
                      type="button"
                      key={`${seg.model.provider ?? "legacy"}:${seg.model.model}:${index}`}
                      aria-label={`${modelLabel(seg.model.model)} · ${fmtNanoUsd(seg.model.official_nano)}`}
                      className={`mdist-seg${mdistHover === index ? " is-hover" : ""}`}
                      style={{ width: `${seg.share * 100}%`, background: modelColor.get(seg.model.model) }}
                      onMouseEnter={() => setMdistHover(index)}
                      onFocus={() => setMdistHover(index)}
                      onBlur={() => setMdistHover((current) => (current === index ? null : current))}
                      onClick={() => setMdistHover((current) => (current === index ? null : index))}
                    />
                  ))}
                </div>
                {mdistHover !== null && mdistPlaced[mdistHover] && (
                  <div
                    className="chart-tip mdist-tip"
                    role="tooltip"
                    style={{ left: `${Math.min(92, Math.max(8, mdistPlaced[mdistHover]!.center * 100))}%` }}
                  >
                    <div className="chart-tip-row">
                      <span
                        className="chart-tip-dot"
                        style={{ background: modelColor.get(mdistPlaced[mdistHover]!.model.model) }}
                      />
                      <span className="chart-tip-nm">{modelLabel(mdistPlaced[mdistHover]!.model.model)}</span>
                      <b>{fmtNanoUsd(mdistPlaced[mdistHover]!.model.official_nano)}</b>
                    </div>
                    <div className="chart-tip-total">
                      <span>{t.spendShare}</span>
                      <b>
                        {(mdistPlaced[mdistHover]!.share * 100).toFixed(mdistPlaced[mdistHover]!.share < 0.1 ? 1 : 0)}%
                      </b>
                    </div>
                  </div>
                )}
              </div>

              <p className="table-scroll-hint">{t.scrollHint}</p>
              <div className="table-scroll" role="region" tabIndex={0} aria-label={t.modelSpend}>
                <table className="mtable">
                  <thead>
                    <tr>
                      <th>{t.model}</th>
                      <th>API</th>
                      <th className="tnum">{t.requests}</th>
                      <th className="tnum">{t.input}</th>
                      <th className="tnum">{t.output}</th>
                      <th className="tnum">{t.cacheReadShort}</th>
                      <th className="tnum">{t.cacheWriteShort}</th>
                      <th className="tnum">{t.official}</th>
                      <th className="tnum">{t.charged}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {models.map((model, index) => (
                      <tr key={`${model.provider ?? "legacy"}:${model.model}:${index}`}>
                        <td>
                          <span className="tkmdl">
                            <span
                              className="tkmdl-dot"
                              style={{ background: MODEL_COLORS[index % MODEL_COLORS.length] }}
                            />
                            {modelLabel(model.model)}
                          </span>
                        </td>
                        <td><span className="chip">{usageProviderOf(model.model, model.provider) === "openai" ? "GPT" : "Claude"}</span></td>
                        <td className="tnum">{model.requests.toLocaleString(locale)}</td>
                        <td className="tnum">{fmtTokens(model.input_tokens)}</td>
                        <td className="tnum">{fmtTokens(model.output_tokens)}</td>
                        <td className="tnum">{fmtTokens(model.cache_read_tokens)}</td>
                        <td className="tnum">{fmtTokens(model.cache_write_5m_tokens + model.cache_write_1h_tokens)}</td>
                        <td className="tnum">{fmtNanoUsd(model.official_nano)}</td>
                        <td className="tnum mprice">{fmtNanoUsd(model.charged_nano)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </>
          )}
        </section>

        {keyRows.length > 0 && (
          <section className="dsec">
            <div className="dsec-head analytics-heading">
              <div>
                <h2>{t.keySummary}</h2>
                <p>{t.keySummaryDesc}</p>
              </div>
            </div>
            <div className="ubreak-sum">
              <div>
                <span className="dlabel">{t.requests}</span>
                <b>{summaryRequests.toLocaleString(locale)}</b>
              </div>
              <div>
                <span className="dlabel">{t.official}</span>
                <b>{formatNanoUsd(summaryOfficialNano)}</b>
              </div>
              <div>
                <span className="dlabel">{t.charged}</span>
                <b>{formatNanoUsd(summaryChargedNano)}</b>
              </div>
              <div>
                <span className="dlabel">{t.remaining}</span>
                <b>{formatNanoUsd(officialRemaining, 2, 2)}</b>
              </div>
            </div>
          </section>
        )}
      </div>
      </div>
    </AppShell>
  );
}
