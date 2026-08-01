"use client";

import { type ReactElement } from "react";
import { Pill, TableCard } from "@/components/ui";
import { duration, nanoMoney } from "@/lib/format";
import {
  compactTokenCount,
  compareProviderRates,
  exactTokenCount,
  formatUsdPerMillion,
  formatUsdPerUnit,
  providerInteger,
  tokensForNanoCapacity,
  usedPercentFromNano,
} from "./provider-calibration";
import { ProviderCapacityStrip, ProviderQuotaMeter, ProviderSection } from "./provider-board-ui";
import type {
  CapacityResponse,
  CapacitySub,
  ClaudeConversionModel,
  ClaudeRateTier,
} from "./types";

type ClaudeTokenKind = "input" | "cache_read" | "cache_write_5m" | "cache_write_1h" | "output";

const TOKEN_KINDS: ClaudeTokenKind[] = ["input", "cache_read", "cache_write_5m", "cache_write_1h", "output"];
const TOKEN_LABELS: Record<ClaudeTokenKind, string> = {
  input: "Input",
  cache_read: "Cache read",
  cache_write_5m: "Write 5м",
  cache_write_1h: "Write 1ч",
  output: "Output",
};

interface ClaudeProfitRow {
  model: ClaudeConversionModel;
  tier: ClaudeRateTier;
  rates: Record<ClaudeTokenKind, string>;
  bestKind: ClaudeTokenKind;
  bestRate: string;
}

function rateFor(tier: ClaudeRateTier, kind: ClaudeTokenKind): string {
  switch (kind) {
    case "input":
      return tier.input_nanousd_per_token;
    case "cache_read":
      return tier.cache_read_nanousd_per_token;
    case "cache_write_5m":
      return tier.cache_write_5m_nanousd_per_token;
    case "cache_write_1h":
      return tier.cache_write_1h_nanousd_per_token;
    case "output":
      return tier.output_nanousd_per_token;
  }
}

function profitabilityRows(models: ClaudeConversionModel[]): ClaudeProfitRow[] {
  return models
    .flatMap((model) =>
      (model.tiers ?? []).map((tier) => {
        const rates = Object.fromEntries(TOKEN_KINDS.map((kind) => [kind, rateFor(tier, kind)])) as Record<
          ClaudeTokenKind,
          string
        >;
        const bestKind = TOKEN_KINDS.reduce((best, kind) =>
          compareProviderRates(rates[kind], rates[best]) > 0 ? kind : best,
        );
        return { model, tier, rates, bestKind, bestRate: rates[bestKind] };
      }),
    )
    .sort((a, b) => {
      const rate = compareProviderRates(b.bestRate, a.bestRate);
      return rate || a.model.id.localeCompare(b.model.id) || a.tier.id.localeCompare(b.tier.id);
    });
}

function moneyOrDash(value: string | null | undefined): string {
  return providerInteger(value) == null ? "—" : nanoMoney(value);
}

function quotaValue(util: number | null | undefined): { value: number | null; label: string } {
  const value = Number(util);
  if (!Number.isFinite(value)) return { value: null, label: "—" };
  const percent = Math.min(100, Math.max(0, value * 100));
  const rounded = Math.round(percent * 10) / 10;
  return { value: rounded, label: `${rounded.toLocaleString("ru-RU", { maximumFractionDigits: 1 })}%` };
}

function subscriptionStatus(item: CapacitySub): { label: string; kind: "ok" | "warn" | "bad" } {
  if (item.auth_state === "dead") {
    return {
      label: item.dead_reason === "permission_error" ? "токен мёртв · бан" : "токен мёртв",
      kind: "bad",
    };
  }
  if (item.auth_state === "suspect") return { label: "auth под наблюдением", kind: "warn" };
  if (item.cooling) return { label: "cooling", kind: "warn" };
  if (item.routable === false) return { label: "вне ротации", kind: "warn" };
  if (item.calibrated === false) return { label: "active · prior", kind: "warn" };
  return { label: "active", kind: "ok" };
}

function ClaudeTokenCapacity({ models, remaining }: { models: ClaudeConversionModel[]; remaining?: string | null }) {
  return (
    <ProviderSection overline="Текущий остаток · весь пул" title="Сколько токенов доступно" meta="только один вид токенов">
      <TableCard>
        <table className="provider-token-capacity-table">
          <thead>
            <tr>
              <th className="left">Модель</th>
              <th className="left">Режим</th>
              {TOKEN_KINDS.map((kind) => <th key={kind}>{TOKEN_LABELS[kind]}</th>)}
            </tr>
          </thead>
          <tbody>
            {models.flatMap((model) =>
              (model.tiers ?? []).map((tier) => (
                <tr key={`${model.id}-${tier.id}`}>
                  <td className="left"><b>{model.id}</b></td>
                  <td className="left"><Pill kind={tier.id === "fast" ? "warn" : "ok"}>{tier.id}</Pill></td>
                  {TOKEN_KINDS.map((kind) => {
                    const tokens = tokensForNanoCapacity(remaining, rateFor(tier, kind));
                    return (
                      <td className={kind === "cache_read" ? "provider-cache-cell" : ""} key={kind} title={exactTokenCount(tokens)}>
                        <b>{compactTokenCount(tokens)}</b>
                      </td>
                    );
                  })}
                </tr>
              )),
            )}
          </tbody>
        </table>
      </TableCard>
    </ProviderSection>
  );
}

function ClaudeProfitability({ rows }: { rows: ClaudeProfitRow[] }) {
  return (
    <ProviderSection overline="Продажная ставка" title="Выгодность по убыванию" meta="$ / 1M токенов ↓">
      <TableCard>
        <table className="provider-profit-table">
          <thead>
            <tr>
              <th>#</th>
              <th className="left">Модель</th>
              <th className="left">Режим</th>
              <th className="left">Самый дорогой токен</th>
              <th>$/1M</th>
              {TOKEN_KINDS.map((kind) => <th key={kind}>{TOKEN_LABELS[kind]}</th>)}
              <th>Web search</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row, index) => (
              <tr className={index === 0 ? "provider-top-efficiency" : ""} key={`${row.model.id}-${row.tier.id}`}>
                <td className="provider-rank-cell">{index + 1}</td>
                <td className="left"><b>{row.model.id}</b></td>
                <td className="left"><Pill kind={row.tier.id === "fast" ? "warn" : "ok"}>{row.tier.id}</Pill></td>
                <td className="left"><b>{TOKEN_LABELS[row.bestKind]}</b></td>
                <td className="provider-usd-ink"><b>{formatUsdPerMillion(row.bestRate)}</b></td>
                {TOKEN_KINDS.map((kind) => (
                  <td className={kind === row.bestKind ? "provider-best-rate" : ""} key={kind}>
                    {formatUsdPerMillion(row.rates[kind])}
                  </td>
                ))}
                <td>{formatUsdPerUnit(row.model.web_search_nanousd_per_request)} / запрос</td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableCard>
    </ProviderSection>
  );
}

function ClaudeSubscriptions({ items }: { items: CapacitySub[] }) {
  return (
    <ProviderSection overline="Подписки" title="Окна по аккаунтам" meta={`${items.filter((item) => item.routable).length}/${items.length} в ротации`}>
      <TableCard>
        <table className="provider-home-capacity-table">
          <thead>
            <tr>
              <th className="left">Почта</th>
              <th className="left">Состояние</th>
              <th>Quota 5ч / reset</th>
              <th>Quota 7д / reset</th>
              <th>Доступно 7д</th>
            </tr>
          </thead>
          <tbody>
            {items.map((item, index) => {
              const five = quotaValue(item.util5h);
              const weekly = quotaValue(item.util7d);
              const health = subscriptionStatus(item);
              return (
                <tr key={`${item.email ?? "claude"}-${index}`}>
                  <td className="left"><b>{item.email || "—"}</b><small>{item.plan ?? "—"}</small></td>
                  <td className="left"><Pill kind={health.kind}>{health.label}</Pill></td>
                  <td><ProviderQuotaMeter usedPercent={five.value} label={five.label} reset={duration(item.reset5h_in)} /></td>
                  <td><ProviderQuotaMeter usedPercent={weekly.value} label={weekly.label} reset={duration(item.reset7d_in)} /></td>
                  <td className="provider-usd-ink"><b>{moneyOrDash(item.rem7d_nano)}</b></td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </TableCard>
    </ProviderSection>
  );
}

export function ClaudeCapacityBoard({ response }: { response: CapacityResponse }): ReactElement {
  const windows = response.window_totals ?? [];
  const weekly = windows.find((item) => Number(item.window_minutes) === 10_080) ?? windows.at(-1);
  const five = windows.find((item) => Number(item.window_minutes) === 300);
  const models = response.conversion_models ?? [];
  const rows = profitabilityRows(models);
  const used = usedPercentFromNano(weekly?.capacity_nano, weekly?.remaining_nano);

  return (
    <div className="provider-capacity-board claude-capacity-board">
      <ProviderCapacityStrip
        ariaLabel="Ёмкость Claude-пула"
        items={[
          {
            label: "7д · доступно",
            value: moneyOrDash(weekly?.remaining_nano),
            caption: "API-$ эквивалент",
            usd: true,
          },
          {
            label: "Полная ёмкость 7д",
            value: moneyOrDash(weekly?.capacity_nano),
            caption: `${weekly?.calibrated_subs ?? 0}/${weekly?.routable_subs ?? 0} откалибровано`,
            usd: true,
          },
          { label: "Использовано", value: used.label, caption: "текущее окно" },
          {
            label: "5ч · доступно",
            value: moneyOrDash(five?.remaining_nano ?? response.available_nano?.next_5h),
            caption: "текущее окно",
            usd: true,
          },
          {
            label: "В ротации",
            value: `${(response.per_sub ?? []).filter((item) => item.routable).length}/${response.per_sub?.length ?? 0}`,
            caption: response.calibrated === false ? "есть prior" : "калибровка готова",
          },
        ]}
      />
      {models.length ? (
        <>
          <ClaudeTokenCapacity models={models} remaining={weekly?.remaining_nano} />
          <ClaudeProfitability rows={rows} />
        </>
      ) : (
        <div className="provider-no-catalog">Тарифный каталог Claude недоступен.</div>
      )}
      <ClaudeSubscriptions items={response.per_sub ?? []} />
    </div>
  );
}
