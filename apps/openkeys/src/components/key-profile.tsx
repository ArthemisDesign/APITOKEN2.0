"use client";

import Link from "next/link";
import { useState } from "react";
import { AppShell } from "@/components/app-shell";
import type { KeyUsageView } from "@/lib/keys";
import { API_PRODUCTS } from "@/lib/api-product";
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

const LOCALE = "ru-RU";
function CopyButton({ value, label = "Скопировать" }: { value: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className="btn btn-ghost btn-sm"
      onClick={() => {
        void navigator.clipboard.writeText(value).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1600);
        });
      }}
    >
      {copied ? "Скопировано" : label}
    </button>
  );
}

export function KeyProfile({ view, showSignOut = false }: { view: KeyUsageView; showSignOut?: boolean }) {
  const [hoverDay, setHoverDay] = useState<number | null>(null);
  const [mdistHover, setMdistHover] = useState<number | null>(null);

  const product = API_PRODUCTS[view.apiType];
  const usage = view.usage;
  const faceValueNano = BigInt(view.faceValueNano);
  const officialRemaining = BigInt(view.officialRemainingNano);
  const officialSpent = BigInt(view.officialSpentNano);
  const usedPercent = faceValueNano > 0n ? boundedPercent(faceValueNano - officialRemaining, faceValueNano) : 0;

  const models = usage?.models ?? [];
  const modelOfficialTotal = models.reduce((sum, model) => sum + BigInt(model.official_nano), 0n);
  const modelColor = new Map<string, string>();
  for (const model of models) {
    if (!modelColor.has(model.model)) modelColor.set(model.model, MODEL_COLORS[modelColor.size % MODEL_COLORS.length]!);
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
      ).map((point) => ({
        day: point.dayTs * 1_000,
        requests: point.requests,
        value: BigInt(point.officialNano),
        charged: BigInt(point.chargedNano),
      }))
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
    <AppShell section="profile" title="Расход ключа">

      <div className="app-body">
      <div className="app-body-in">
        <div className="page-heading">
          <span className="eyebrow">Баланс ключа</span>
          <h1 className="p-h1">Расход по вашему ключу</h1>
          <p className="p-sub">
            {view.apiType === "openai"
              ? "Все суммы показаны в долларах прайса GPT API: здесь видны остаток, запросы, токены и модели OpenAI-совместимого ключа."
              : "Все суммы — в долларах официального прайса Anthropic: столько же вы заплатили бы за эти запросы на api.anthropic.com."}
          </p>
        </div>

        <div className="overview-primary-grid">
          <article className="card overview-balance-card">
            <div className="overview-card-head">
              <span className="overview-card-label">Остаток ключа</span>
              <span className="overview-rate-chip">номинал {formatNanoUsd(faceValueNano, 0, 0)}</span>
            </div>
            <div className="overview-balance-main">
              <strong className="overview-balance-number">{formatNanoUsd(officialRemaining, 2, 2)}</strong>
              <div className="overview-balance-detail">
                <p className="overview-balance-value">
                  Потрачено <b>{formatNanoUsd(officialSpent, 2, 2)}</b> из {formatNanoUsd(faceValueNano, 0, 0)}
                </p>
                <div className="key-usage-track" aria-hidden="true">
                  <span style={{ width: `${Math.min(100, usedPercent)}%` }} />
                </div>
                <p className="overview-balance-rate">
                  Ключ {view.status === "active" ? "активен" : "отключён"} · выпущен{" "}
                  {view.createdAt.slice(0, 10)}
                </p>
                <div className="overview-card-actions">
                  <Link className="btn btn-primary btn-sm" href={product.docsPath}>
                    Как подключить
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
                      Выйти
                    </button>
                  ) : (
                    view.apiType === "openai" ? (
                      <Link className="btn btn-ghost btn-sm" href="/docs">
                        Инструкция Claude
                      </Link>
                    ) : (
                      <a
                        className="btn btn-ghost btn-sm"
                        href="https://apitoken.sale/docs"
                        target="_blank"
                        rel="noreferrer"
                      >
                        Полная документация
                      </a>
                    )
                  )}
                </div>
              </div>
            </div>
          </article>

          <article className="card overview-access-card">
            <div className="overview-card-head">
              <span className="overview-card-label">Подключение</span>
              <span className="chip">base url</span>
            </div>
            <div className="secret-key-field">
              <code>{product.baseUrl}</code>
              <CopyButton value={product.baseUrl} />
            </div>
            <p className="overview-balance-rate" style={{ marginTop: 10 }}>
              Ваш ключ: <code className="key-mask">{view.keyMasked}</code>
            </p>
          </article>
        </div>

        <div className="ov-stats bill4">
          <div className="ovstat">
            <span className="dlabel">Официальная стоимость</span>
            <b className="num accent">{formatNanoUsd(summaryOfficialNano)}</b>
            <span className="dtrend">эквивалент прайса {product.priceLabel}</span>
          </div>
          <div className="ovstat">
            <span className="dlabel">Списано с ключа</span>
            <b className="num">{formatNanoUsd(summaryChargedNano)}</b>
            <span className="dtrend">за 30 дней</span>
          </div>
          <div className="ovstat">
            <span className="dlabel">Запросов</span>
            <b className="num">{summaryRequests.toLocaleString(LOCALE)}</b>
            <span className="dtrend">за 30 дней</span>
          </div>
          <div className="ovstat">
            <span className="dlabel">Номинал ключа</span>
            <b className="num">{formatNanoUsd(faceValueNano, 0, 0)}</b>
            <span className="dtrend">баланс {product.balanceLabel}</span>
          </div>
        </div>

        <div className="usage-graph">
          <div className="uchart">
            <div className="uchart-head">
              <b>Расход по дням</b>
              <span>последние 30 дней</span>
            </div>
            {maxValue === 0n ? (
              <div className="uchart-empty">За этот период списаний ещё не было</div>
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
                        aria-label={`${fmtUtcDay(point.day, LOCALE)}: ${formatNanoUsdSmart(point.value)}`}
                        onMouseEnter={() => setHoverDay(index)}
                        onFocus={() => setHoverDay(index)}
                        onBlur={() => setHoverDay((current) => (current === index ? null : current))}
                        onClick={() => setHoverDay((current) => (current === index ? null : index))}
                      >
                        <div className="uchart-col-fill">
                          {point.value > 0n && (
                            <div
                              className="uchart-seg"
                              style={{ height: `${boundedPercent(point.value, scale.max)}%`, background: MODEL_COLORS[0] }}
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
                        <div className="chart-tip-h">{fmtUtcDay(series[hoverDay]!.day, LOCALE)}</div>
                        <div className="chart-tip-row">
                          <span className="chart-tip-dot" style={{ background: MODEL_COLORS[0] }} />
                          <span className="chart-tip-nm">Официальная стоимость</span>
                          <b>{formatNanoUsdSmart(series[hoverDay]!.value)}</b>
                        </div>
                        <div className="chart-tip-total">
                          <span>Списано</span>
                          <b>{formatNanoUsdSmart(series[hoverDay]!.charged)}</b>
                        </div>
                        <div className="chart-tip-total">
                          <span>Запросов</span>
                          <b>{series[hoverDay]!.requests.toLocaleString(LOCALE)}</b>
                        </div>
                      </div>
                    )}
                  </div>
                  <div className="uchart-axis">
                    {axisMarks.map((mark) => (
                      <span key={mark} style={{ left: `${((mark + 0.5) / series.length) * 100}%` }}>
                        {fmtUtcDay(series[mark]!.day, LOCALE)}
                      </span>
                    ))}
                  </div>
                </div>
              </div>
            )}
          </div>
          <div className="usum">
            <span className="usum-t">Сводка за период</span>
            <div className="usum-row">
              <span>Официальная стоимость</span>
              <b className="accent">{formatNanoUsd(summaryOfficialNano)}</b>
            </div>
            <div className="usum-row">
              <span>Списано</span>
              <b>{formatNanoUsd(summaryChargedNano)}</b>
            </div>
            <div className="usum-row">
              <span>Запросов</span>
              <b>{summaryRequests.toLocaleString(LOCALE)}</b>
            </div>
            <div className="usum-row">
              <span>Пиковый день</span>
              <b>{peak.value > 0n ? `${fmtUtcDay(peak.day, LOCALE)} · ${formatNanoUsd(peak.value)}` : "—"}</b>
            </div>
            <div className="usum-row">
              <span>В среднем в день</span>
              <b>{summaryOfficialNano > 0n ? formatNanoUsd(roundDivide(summaryOfficialNano, averageDays)) : "—"}</b>
            </div>
          </div>
        </div>

        <section className="dsec">
          <div className="dsec-head analytics-heading">
            <div>
              <h2>Токены и модели</h2>
              <p>Из чего сложился расход: вход, выход и кэш считаются по своим ставкам.</p>
            </div>
          </div>
          <div className="tok-buckets">
            <div className="tokb">
              <span className="dlabel">Входные токены</span>
              <b>{fmtTokens(usage?.buckets.input.tokens ?? 0)}</b>
              <span className="tokb-usd">{fmtNanoUsd(usage?.buckets.input.official_nano ?? "0")}</span>
            </div>
            <div className="tokb">
              <span className="dlabel">Выходные токены</span>
              <b>{fmtTokens(usage?.buckets.output.tokens ?? 0)}</b>
              <span className="tokb-usd">{fmtNanoUsd(usage?.buckets.output.official_nano ?? "0")}</span>
            </div>
            <div className="tokb">
              <span className="dlabel">Чтение кэша</span>
              <b>{fmtTokens(usage?.buckets.cache_read.tokens ?? 0)}</b>
              <span className="tokb-usd">{fmtNanoUsd(usage?.buckets.cache_read.official_nano ?? "0")}</span>
            </div>
            <div className="tokb">
              <span className="dlabel">Запись кэша</span>
              <b>{fmtTokens(usage?.buckets.cache_write.tokens ?? 0)}</b>
              <span className="tokb-usd">{fmtNanoUsd(usage?.buckets.cache_write.official_nano ?? "0")}</span>
            </div>
            {(usage?.buckets.web_search.requests ?? 0) > 0 && (
              <div className="tokb">
                <span className="dlabel">Веб-поиск</span>
                <b>{(usage?.buckets.web_search.requests ?? 0).toLocaleString(LOCALE)}</b>
                <span className="tokb-usd">{fmtNanoUsd(usage?.buckets.web_search.official_nano ?? "0")}</span>
              </div>
            )}
            {legacyOfficialNano > 0n && (
              <div className="tokb tokb-legacy">
                <span className="dlabel">Без разбивки</span>
                <b>ранние запросы</b>
                <span className="tokb-usd">{fmtNanoUsd(usage!.buckets.unattributed_legacy.official_nano)}</span>
              </div>
            )}
          </div>

          {models.length === 0 ? (
            <div className="empty-box">Разбивка появится после первых запросов</div>
          ) : (
            <>
              <div className="mdist-wrap">
                <div
                  className="mdist"
                  role="group"
                  aria-label="Распределение по моделям"
                  onMouseLeave={(event) => {
                    if (!event.currentTarget.contains(document.activeElement)) setMdistHover(null);
                  }}
                >
                  {mdistPlaced.map((seg, index) => (
                    <button
                      type="button"
                      key={seg.model.model}
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
                      <span>Доля расхода</span>
                      <b>
                        {(mdistPlaced[mdistHover]!.share * 100).toFixed(mdistPlaced[mdistHover]!.share < 0.1 ? 1 : 0)}%
                      </b>
                    </div>
                  </div>
                )}
              </div>

              <p className="table-scroll-hint">Таблицу можно прокручивать вбок</p>
              <div className="table-scroll" role="region" tabIndex={0} aria-label="Расход по моделям">
                <table className="mtable">
                  <thead>
                    <tr>
                      <th>Модель</th>
                      <th className="tnum">Запросов</th>
                      <th className="tnum">Вход</th>
                      <th className="tnum">Выход</th>
                      <th className="tnum">Кэш чт.</th>
                      <th className="tnum">Кэш зап.</th>
                      <th className="tnum">Официально</th>
                      <th className="tnum">Списано</th>
                    </tr>
                  </thead>
                  <tbody>
                    {models.map((model, index) => (
                      <tr key={model.model}>
                        <td>
                          <span className="tkmdl">
                            <span
                              className="tkmdl-dot"
                              style={{ background: MODEL_COLORS[index % MODEL_COLORS.length] }}
                            />
                            {modelLabel(model.model)}
                          </span>
                        </td>
                        <td className="tnum">{model.requests.toLocaleString(LOCALE)}</td>
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
                <h2>Сводка по ключу</h2>
                <p>Итоги за окно наблюдения.</p>
              </div>
            </div>
            <div className="ubreak-sum">
              <div>
                <span className="dlabel">Запросов</span>
                <b>{summaryRequests.toLocaleString(LOCALE)}</b>
              </div>
              <div>
                <span className="dlabel">Официально</span>
                <b>{formatNanoUsd(summaryOfficialNano)}</b>
              </div>
              <div>
                <span className="dlabel">Списано</span>
                <b>{formatNanoUsd(summaryChargedNano)}</b>
              </div>
              <div>
                <span className="dlabel">Остаток</span>
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
