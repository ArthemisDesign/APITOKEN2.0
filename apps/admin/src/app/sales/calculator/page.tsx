"use client";

import { useMemo, useState, type ReactNode } from "react";
import { api } from "@/lib/api";
import { nanoMoney } from "@/lib/format";
import { usePoll } from "@/lib/usePoll";
import { LoadingGrid, Pill } from "@/components/ui";
import type {
  CapacityResponse,
  CodexSubsResponse,
  GeminiSubsResponse,
} from "../../subscriptions/types";
import {
  PRODUCT_CATALOG,
  buildProductMetrics,
  calculateScenario,
  decimalUsdToNano,
  nanoToEditableUsd,
  type CalibrationPayload,
  type ProductMetric,
  type Provider,
  type WindowMetric,
} from "./calculation";

const POLL_INTERVAL_MS = 5_000;

interface CalibrationSnapshot extends CalibrationPayload {
  fetchedAt: number;
}

async function loadCalibration(): Promise<CalibrationSnapshot> {
  const [capacity, codex, gemini] = await Promise.all([
    api<CapacityResponse>("/capacity").catch(() => null),
    api<CodexSubsResponse>("/codex-subs").catch(() => null),
    api<GeminiSubsResponse>("/gemini-subs").catch(() => null),
  ]);
  return { capacity, codex, gemini, fetchedAt: Date.now() };
}

const PROVIDER_LABEL: Record<Provider, string> = {
  claude: "Claude",
  openai: "GPT",
  gemini: "Gemini",
};

function WindowValue({ metric }: { metric: WindowMetric }) {
  if (metric.capacityNano == null) {
    return (
      <div className="calc-window empty-value">
        <strong>ждём данные</strong>
        <span>нужен полный Δusage</span>
      </div>
    );
  }
  const hasEnvelope = metric.lowNano != null && metric.highNano != null;
  return (
    <div className="calc-window">
      <strong>{nanoMoney(metric.capacityNano)}</strong>
      <span>
        {hasEnvelope
          ? `${nanoMoney(metric.lowNano)}—${nanoMoney(metric.highNano)}`
          : `${metric.measuredProfiles} измер.`}
      </span>
    </div>
  );
}

function EvidenceStatus({ metric }: { metric: ProductMetric }) {
  if (!metric.sourceOnline) return <span className="calc-evidence is-down">источник недоступен</span>;
  if (!metric.profiles) return <span className="calc-evidence">нет в текущем пуле</span>;
  if (!metric.measuredProfiles) return <span className="calc-evidence is-waiting">ждём Δusage</span>;
  return (
    <span className="calc-evidence is-live">
      {metric.measuredProfiles}/{metric.profiles} измерено
      {metric.confidenceBp == null ? "" : ` · ${(metric.confidenceBp / 100).toFixed(1)}% confidence`}
    </span>
  );
}

function MetricCard({ label, value, hint, tone = "" }: { label: string; value: ReactNode; hint: string; tone?: string }) {
  return (
    <div className={`calc-result ${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{hint}</small>
    </div>
  );
}

function RangeControl({
  label,
  value,
  onChange,
  hint,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
  hint: string;
}) {
  return (
    <label className="calc-range-field">
      <span>
        {label} <output>{value}%</output>
      </span>
      <input
        type="range"
        min="0"
        max="100"
        step="1"
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
      />
      <small>{hint}</small>
    </label>
  );
}

export default function SalesCalculatorPage() {
  const { data, isLoading } = usePoll("sales-calculator", loadCalibration, { interval: POLL_INTERVAL_MS });
  const metrics = useMemo(() => (data ? buildProductMetrics(data) : []), [data]);
  const [selectedId, setSelectedId] = useState("claude-max20");
  const [quantity, setQuantity] = useState(1);
  const [utilizationPercent, setUtilizationPercent] = useState(65);
  const [discountPercent, setDiscountPercent] = useState(20);
  const initialProduct = PRODUCT_CATALOG.find((product) => product.id === selectedId) ?? PRODUCT_CATALOG[0];
  const [subscriptionCost, setSubscriptionCost] = useState(
    nanoToEditableUsd(initialProduct.defaultMonthlyCostNano),
  );

  const selected = metrics.find((metric) => metric.product.id === selectedId) ?? null;
  const subscriptionCostNano = decimalUsdToNano(subscriptionCost);
  const scenario =
    selected?.month.capacityNano == null
      ? null
      : calculateScenario({
          monthlyCapacityNano: selected.month.capacityNano,
          quantity,
          utilizationBp: utilizationPercent * 100,
          discountBp: discountPercent * 100,
          subscriptionCostNano,
        });

  const measuredProducts = metrics.filter((metric) => metric.month.capacityNano != null).length;
  const sourceCount = data
    ? Number(data.capacity !== null) + Number(data.codex !== null) + Number(data.gemini !== null)
    : 0;

  const chooseProduct = (id: string) => {
    setSelectedId(id);
    const product = PRODUCT_CATALOG.find((item) => item.id === id);
    setSubscriptionCost(nanoToEditableUsd(product?.defaultMonthlyCostNano ?? null));
  };

  if (isLoading && !data) return <LoadingGrid count={8} />;

  return (
    <div className="sales-calculator">
      <section className="calc-hero">
        <div className="calc-hero-copy">
          <div className="calc-kicker">
            <span>Sales intelligence</span>
            <Pill kind={sourceCount === 3 ? "ok" : "warn"}>live · 5 сек</Pill>
          </div>
          <h1>Цена подписки — факт. Её API-ёмкость — измерение.</h1>
          <p>
            Калькулятор переводит реальные движения quota в API-доллары, показывает запас на 5 часов,
            7 дней и устойчивый 30-дневный эквивалент — затем считает скидку, недоиспользование и
            упущенную выручку.
          </p>
        </div>
        <div className="calc-live-stamp" aria-label="Состояние источников">
          <span className={sourceCount === 3 ? "pulse" : "pulse warn"} />
          <div>
            <strong>{sourceCount}/3 источника</strong>
            <small>
              {data ? new Date(data.fetchedAt).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit", second: "2-digit" }) : "—"}
            </small>
          </div>
          <div>
            <strong>{measuredProducts}</strong>
            <small>тарифов с 30д оценкой</small>
          </div>
        </div>
      </section>

      <section className="calc-section" aria-labelledby="capacity-title">
        <div className="calc-section-head">
          <div>
            <span className="calc-overline">01 · Калибровочная лента</span>
            <h2 id="capacity-title">API-$ на одну подписку</h2>
          </div>
          <p>Среднее только по измеренным профилям. Прайоры и холодные якоря не участвуют.</p>
        </div>

        <div className="calc-matrix-scroll">
          <div className="calc-matrix" aria-label="Калибровка подписок">
            <div className="calc-matrix-head">
              <span>Тариф</span>
              <span>5 часов</span>
              <span>7 дней</span>
              <span>30 дней</span>
              <span>Доказательства</span>
            </div>
            {metrics.map((metric) => {
              const selectedRow = metric.product.id === selectedId;
              return (
                <button
                  className={`calc-matrix-row${selectedRow ? " is-selected" : ""}`}
                  data-provider={metric.product.provider}
                  type="button"
                  key={metric.product.id}
                  onClick={() => chooseProduct(metric.product.id)}
                  aria-pressed={selectedRow}
                >
                  <span className="calc-plan">
                    <i />
                    <span>
                      <strong>{metric.product.label}</strong>
                      <small>{PROVIDER_LABEL[metric.product.provider]} · {metric.profiles} в пуле</small>
                    </span>
                  </span>
                  <span><WindowValue metric={metric.fiveHour} /></span>
                  <span><WindowValue metric={metric.sevenDay} /></span>
                  <span><WindowValue metric={metric.month} /></span>
                  <span><EvidenceStatus metric={metric} /></span>
                </button>
              );
            })}
          </div>
        </div>
      </section>

      <section className="calc-section" aria-labelledby="scenario-title">
        <div className="calc-section-head">
          <div>
            <span className="calc-overline">02 · Сценарий сделки</span>
            <h2 id="scenario-title">Что получает клиент и что теряем мы</h2>
          </div>
          <p>30 дней = минимум из пересчитанных 5-часового и недельного лимитов.</p>
        </div>

        <div className="calc-workbench">
          <div className="calc-controls">
            <label className="calc-field">
              <span>Тариф</span>
              <select value={selectedId} onChange={(event) => chooseProduct(event.target.value)}>
                {PRODUCT_CATALOG.map((product) => <option key={product.id} value={product.id}>{product.label}</option>)}
              </select>
              <small>калибровка обновляется автоматически</small>
            </label>
            <label className="calc-field">
              <span>Подписок</span>
              <input
                type="number"
                min="1"
                max="10000"
                step="1"
                value={quantity}
                onChange={(event) => setQuantity(Math.min(10_000, Math.max(1, Number(event.target.value) || 1)))}
              />
              <small>одинакового тарифа</small>
            </label>
            <label className="calc-field">
              <span>Цена подписки, $ / мес</span>
              <input
                type="text"
                inputMode="decimal"
                placeholder="контрактная цена"
                value={subscriptionCost}
                onChange={(event) => setSubscriptionCost(event.target.value)}
                aria-invalid={subscriptionCost !== "" && subscriptionCostNano == null}
              />
              <small>{subscriptionCost === "" ? "введите вашу закупку" : subscriptionCostNano == null ? "до 9 знаков после точки" : "можно заменить фактической закупкой"}</small>
            </label>
            <RangeControl
              label="Использование лимита"
              value={utilizationPercent}
              onChange={setUtilizationPercent}
              hint="сколько квоты реально выбирает клиент"
            />
            <RangeControl
              label="Скидка от API-прайса"
              value={discountPercent}
              onChange={setDiscountPercent}
              hint="снижение цены относительно официального API"
            />
          </div>

          <div className="calc-output" aria-live="polite">
            {scenario ? (
              <>
                <div className="calc-equation" aria-label="Формула сценария">
                  <div><span>полная ёмкость</span><strong>{nanoMoney(scenario.fullCapacityNano)}</strong></div>
                  <b>×</b>
                  <div><span>использовано</span><strong>{utilizationPercent}%</strong></div>
                  <b>×</b>
                  <div><span>после скидки</span><strong>{100 - discountPercent}%</strong></div>
                  <b>=</b>
                  <div className="accent"><span>цена оффера</span><strong>{nanoMoney(scenario.offerNano)}</strong></div>
                </div>
                <div className="calc-results-grid">
                  <MetricCard label="Цена оффера" value={nanoMoney(scenario.offerNano)} hint="за фактически используемый API-эквивалент" tone="primary" />
                  <MetricCard label="Экономия клиента" value={nanoMoney(scenario.customerApiSavingsNano)} hint="против официального API при том же объёме" tone="positive" />
                  <MetricCard label="Упущенная выгода" value={nanoMoney(scenario.missedRevenueNano)} hint={`${nanoMoney(scenario.unusedCapacityNano)} API-$ останется неиспользовано`} tone="warning" />
                  <MetricCard
                    label="Валовая разница"
                    value={scenario.grossMarginNano == null ? "—" : nanoMoney(scenario.grossMarginNano)}
                    hint={scenario.subscriptionSpendNano == null ? "задайте цену подписки" : `после ${nanoMoney(scenario.subscriptionSpendNano)} стоимости подписок`}
                    tone={scenario.grossMarginNano != null && scenario.grossMarginNano < 0n ? "negative" : ""}
                  />
                  <MetricCard label="Реально использует" value={nanoMoney(scenario.usedCapacityNano)} hint="API-$ по наблюдаемой смеси моделей" />
                  <MetricCard
                    label="Оплата простоя"
                    value={scenario.idleSubscriptionSpendNano == null ? "—" : nanoMoney(scenario.idleSubscriptionSpendNano)}
                    hint="часть фиксированной подписки, которой клиент не пользуется"
                  />
                </div>
              </>
            ) : (
              <div className="calc-no-evidence">
                <span>Δ</span>
                <div>
                  <strong>Для {selected?.product.label ?? "этого тарифа"} ещё нет полного измерения 5ч + 7д.</strong>
                  <p>Калькулятор не подставляет рекламный номинал. Он включится сам после реального движения quota и settlement.</p>
                </div>
              </div>
            )}
          </div>
        </div>
      </section>

      <footer className="calc-footnote">
        API-$ — официальный API-эквивалент реально обслуженной смеси моделей, контекста, reasoning и tools.
        Это не обещанный номинал тарифа. GPT и Gemini показывают evidence envelope; Claude — calibrated EMA.
        Значения обновляются каждые 5 секунд, пока вкладка видима. Полные email, OAuth, project и proxy в браузер не передаются.
      </footer>
    </div>
  );
}
