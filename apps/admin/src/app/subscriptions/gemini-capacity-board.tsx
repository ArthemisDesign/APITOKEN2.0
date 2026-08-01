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
  usedPercentFromNano,
} from "./provider-calibration";
import { ProviderCapacityStrip, ProviderQuotaMeter, ProviderSection } from "./provider-board-ui";
import { geminiProfileStatus } from "./logic";
import type {
  GeminiConversionModel,
  GeminiProfile,
  GeminiProfileWindow,
  GeminiQuota,
  GeminiSubsResponse,
} from "./types";

type GeminiTokenKind = "input" | "audio" | "cached" | "cached_audio" | "output" | "image";
type GeminiContext = "short" | "long";

const TOKEN_KINDS: GeminiTokenKind[] = ["input", "audio", "cached", "cached_audio", "output", "image"];
const TOKEN_LABELS: Record<GeminiTokenKind, string> = {
  input: "Input",
  audio: "Audio in",
  cached: "Cache",
  cached_audio: "Audio cache",
  output: "Output + thinking",
  image: "Image out",
};

interface GeminiProfitRow {
  model: GeminiConversionModel;
  context: GeminiContext;
  rates: Record<GeminiTokenKind, string>;
  bestKind: GeminiTokenKind;
  bestRate: string;
}

interface GeminiQuotaRow {
  model: GeminiConversionModel;
  quotaId: string;
  amount: bigint | null;
  tokenType: string;
  profileCount: number;
  usedPercent: number | null;
  usedLabel: string;
  remainingRange: string;
  resetAt: number | null;
}

function rateFor(model: GeminiConversionModel, context: GeminiContext, kind: GeminiTokenKind): string {
  const rates = model.rates;
  if (kind === "image") return rates.image_output_nanousd_per_token;
  if (context === "long") {
    if (kind === "input") return rates.long_input_nanousd_per_token;
    if (kind === "audio") return rates.long_audio_input_nanousd_per_token;
    if (kind === "cached") return rates.long_cached_input_nanousd_per_token;
    if (kind === "cached_audio") return rates.long_cached_audio_input_nanousd_per_token;
    return rates.long_output_nanousd_per_token;
  }
  if (kind === "input") return rates.input_nanousd_per_token;
  if (kind === "audio") return rates.audio_input_nanousd_per_token;
  if (kind === "cached") return rates.cached_input_nanousd_per_token;
  if (kind === "cached_audio") return rates.cached_audio_input_nanousd_per_token;
  return rates.output_nanousd_per_token;
}

function hasDistinctLongContext(model: GeminiConversionModel): boolean {
  return TOKEN_KINDS.some((kind) => compareProviderRates(rateFor(model, "short", kind), rateFor(model, "long", kind)) !== 0);
}

function profitabilityRows(models: GeminiConversionModel[]): GeminiProfitRow[] {
  return models
    .flatMap((model) => (["short", ...(hasDistinctLongContext(model) ? (["long"] as const) : [])] as GeminiContext[]).map((context) => {
      const rates = Object.fromEntries(TOKEN_KINDS.map((kind) => [kind, rateFor(model, context, kind)])) as Record<
        GeminiTokenKind,
        string
      >;
      const available = TOKEN_KINDS.filter((kind) => (providerInteger(rates[kind]) ?? 0n) > 0n);
      const bestKind = available.reduce((best, kind) =>
        compareProviderRates(rates[kind], rates[best]) > 0 ? kind : best,
      );
      return { model, context, rates, bestKind, bestRate: rates[bestKind] };
    }))
    .sort((a, b) => {
      const rate = compareProviderRates(b.bestRate, a.bestRate);
      return rate || a.model.id.localeCompare(b.model.id) || a.context.localeCompare(b.context);
    });
}

function moneyOrDash(value: string | null | undefined): string {
  return providerInteger(value) == null ? "—" : nanoMoney(value);
}

function quotaVariant(publicId: string, quotaId: string): string {
  if (publicId === quotaId) return "default";
  if (quotaId.endsWith("extra-low")) return "minimal";
  if (quotaId.endsWith("-low")) return "low";
  if (quotaId.endsWith("-medium")) return "medium";
  if (quotaId.endsWith("-high")) return "high";
  if (quotaId === "gemini-pro-agent") return "high / default";
  return quotaId;
}

function sumQuotaAmounts(items: GeminiQuota[]): bigint | null {
  if (!items.length || items.some((item) => item.remaining_amount == null)) return null;
  let total = 0n;
  for (const item of items) {
    const value = providerInteger(item.remaining_amount);
    if (value == null) return null;
    total += value;
  }
  return total;
}

function quotaRows(models: GeminiConversionModel[], profiles: GeminiProfile[]): GeminiQuotaRow[] {
  return models.flatMap((model) => {
    const ids = model.quota_model_ids?.length ? model.quota_model_ids : [model.id];
    return ids.map((quotaId) => {
      const matchedProfiles = profiles
        .map((profile) => ({ profile, quotas: (profile.quotas ?? []).filter((quota) => quota.model_id === quotaId) }))
        .filter((entry) => entry.quotas.length > 0);
      const matches = matchedProfiles.flatMap((entry) => entry.quotas);
      const fractions = matches
        .map((quota) => Number(quota.remaining_fraction))
        .filter((value) => Number.isFinite(value))
        .map((value) => Math.min(1, Math.max(0, value)));
      const averageRemaining = fractions.length
        ? fractions.reduce((sum, value) => sum + value, 0) / fractions.length
        : null;
      const usedPercent = averageRemaining == null ? null : Math.round((1 - averageRemaining) * 1_000) / 10;
      const min = fractions.length ? Math.min(...fractions) : null;
      const max = fractions.length ? Math.max(...fractions) : null;
      const resets = matches
        .map((quota) => Date.parse(quota.reset_time ?? "") / 1000)
        .filter((value) => Number.isFinite(value) && value > 0);
      const tokenTypes = [...new Set(matches.map((quota) => quota.token_type).filter(Boolean))];
      return {
        model,
        quotaId,
        amount: sumQuotaAmounts(matches),
        tokenType: tokenTypes.join(" / ") || "provider units",
        profileCount: matchedProfiles.length,
        usedPercent,
        usedLabel: usedPercent == null ? "—" : `${usedPercent.toLocaleString("ru-RU", { maximumFractionDigits: 1 })}%`,
        remainingRange:
          min == null || max == null
            ? "fraction не опубликован"
            : `остаток ${Math.round(min * 100)}–${Math.round(max * 100)}%`,
        resetAt: resets.length ? Math.min(...resets) : null,
      };
    });
  });
}

function windowFor(profile: GeminiProfile, kind: string): GeminiProfileWindow | undefined {
  return (profile.windows ?? []).find((window) => window.window_kind === kind);
}

function windowQuota(window: GeminiProfileWindow | undefined): { value: number | null; label: string } {
  if (!window) return { value: null, label: "—" };
  const units = Number(window.used_fraction_units);
  const value = Number.isFinite(units)
    ? Math.min(100, Math.max(0, units / 1_000_000))
    : window.remaining_fraction == null
      ? null
      : Math.min(100, Math.max(0, (1 - Number(window.remaining_fraction)) * 100));
  if (value == null || !Number.isFinite(value)) return { value: null, label: "—" };
  const rounded = Math.round(value * 10) / 10;
  return { value: rounded, label: `${rounded.toLocaleString("ru-RU", { maximumFractionDigits: 1 })}%` };
}

function GeminiOfficialQuota({ rows, nowSec }: { rows: GeminiQuotaRow[]; nowSec: number }) {
  return (
    <ProviderSection overline="Официальные buckets Google" title="Доступная квота по моделям" meta="— = amount не опубликован">
      <TableCard>
        <table className="provider-gemini-quota-table">
          <thead>
            <tr>
              <th className="left">Модель</th>
              <th className="left">Режим</th>
              <th>Доступно точно</th>
              <th>Quota / reset</th>
              <th>Профили</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={`${row.model.id}-${row.quotaId}`}>
                <td className="left"><b>{row.model.id}</b></td>
                <td className="left"><Pill>{quotaVariant(row.model.id, row.quotaId)}</Pill></td>
                <td title={exactTokenCount(row.amount)}>
                  <b>{compactTokenCount(row.amount)}</b>
                  <small>{row.amount == null ? "Google даёт только %" : row.tokenType}</small>
                </td>
                <td>
                  <ProviderQuotaMeter
                    usedPercent={row.usedPercent}
                    label={row.usedLabel}
                    reset={row.resetAt ? duration(Math.max(0, row.resetAt - nowSec)) : "—"}
                  />
                  <small>{row.remainingRange}</small>
                </td>
                <td><b>{row.profileCount}</b></td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableCard>
    </ProviderSection>
  );
}

function GeminiProfitability({ rows }: { rows: GeminiProfitRow[] }) {
  return (
    <ProviderSection overline="Paid-tier Developer API" title="Выгодность по убыванию" meta="$ / 1M токенов ↓">
      <TableCard>
        <table className="provider-profit-table provider-gemini-profit-table">
          <thead>
            <tr>
              <th>#</th>
              <th className="left">Модель</th>
              <th className="left">Контекст</th>
              <th className="left">Самый дорогой токен</th>
              <th>$/1M</th>
              {TOKEN_KINDS.map((kind) => <th key={kind}>{TOKEN_LABELS[kind]}</th>)}
              <th>Search</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row, index) => (
              <tr className={index === 0 ? "provider-top-efficiency" : ""} key={`${row.model.id}-${row.context}`}>
                <td className="provider-rank-cell">{index + 1}</td>
                <td className="left"><b>{row.model.id}</b></td>
                <td className="left"><Pill kind={row.context === "long" ? "warn" : "ok"}>{row.context}</Pill></td>
                <td className="left"><b>{TOKEN_LABELS[row.bestKind]}</b></td>
                <td className="provider-usd-ink"><b>{formatUsdPerMillion(row.bestRate)}</b></td>
                {TOKEN_KINDS.map((kind) => {
                  const rate = row.rates[kind];
                  return (
                    <td className={kind === row.bestKind ? "provider-best-rate" : ""} key={kind}>
                      {(providerInteger(rate) ?? 0n) > 0n ? formatUsdPerMillion(rate) : "—"}
                    </td>
                  );
                })}
                <td>
                  {formatUsdPerUnit(row.model.search?.nanousd_per_unit)}
                  <small>{row.model.search?.billing_unit === "query" ? "query" : "grounded prompt"}</small>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableCard>
    </ProviderSection>
  );
}

function GeminiSubscriptions({ profiles, modelCount, nowSec }: { profiles: GeminiProfile[]; modelCount: number; nowSec: number }) {
  return (
    <ProviderSection overline="Подписки" title="Окна по аккаунтам" meta={`${profiles.filter((profile) => profile.authenticated).length}/${profiles.length} auth`}>
      <TableCard>
        <table className="provider-home-capacity-table provider-gemini-home-table">
          <thead>
            <tr>
              <th className="left">Почта</th>
              <th className="left">Состояние</th>
              <th>Quota 5ч / reset</th>
              <th className="provider-five-hour-money">Доступно $ · 5ч</th>
              <th>Quota 7д / reset</th>
              <th>Доступно $ · 7д</th>
              <th>Модели</th>
            </tr>
          </thead>
          <tbody>
            {profiles.map((profile, index) => {
              const fiveWindow = windowFor(profile, "5h");
              const weeklyWindow = windowFor(profile, "weekly");
              const five = windowQuota(fiveWindow);
              const weekly = windowQuota(weeklyWindow);
              const health = geminiProfileStatus(profile, nowSec);
              const availableModels = profile.authenticated
                ? (profile.model_cooling ?? []).filter((model) => Number(model.cooling_until || 0) <= nowSec).length
                : 0;
              return (
                <tr key={profile.id ?? index}>
                  <td className="left"><b>{profile.email?.trim() || "—"}</b><small>{profile.plan ?? "—"}</small></td>
                  <td className="left"><Pill kind={health.kind}>{health.label}</Pill></td>
                  <td><ProviderQuotaMeter usedPercent={five.value} label={five.label} reset={fiveWindow?.resets_at ? duration(Math.max(0, fiveWindow.resets_at - nowSec)) : "—"} /></td>
                  <td className="provider-usd-ink provider-five-hour-money">
                    <b>{moneyOrDash(fiveWindow?.remaining_nano)}</b>
                    <small>{`из ${moneyOrDash(fiveWindow?.capacity_nano)}`}</small>
                  </td>
                  <td><ProviderQuotaMeter usedPercent={weekly.value} label={weekly.label} reset={weeklyWindow?.resets_at ? duration(Math.max(0, weeklyWindow.resets_at - nowSec)) : "—"} /></td>
                  <td className="provider-usd-ink">
                    <b>{moneyOrDash(weeklyWindow?.remaining_nano)}</b>
                    <small>{`из ${moneyOrDash(weeklyWindow?.capacity_nano)}`}</small>
                  </td>
                  <td><b>{availableModels}/{modelCount}</b></td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </TableCard>
    </ProviderSection>
  );
}

export function GeminiCapacityBoard({ response, nowMs }: { response: GeminiSubsResponse; nowMs: number }): ReactElement {
  const windows = response.window_totals ?? [];
  const weekly = windows.find((item) => Number(item.window_minutes) === 10_080) ?? windows.at(-1);
  const five = windows.find((item) => Number(item.window_minutes) === 300);
  const models = response.conversion_models ?? [];
  const profits = profitabilityRows(models);
  const quotas = quotaRows(models, response.profiles ?? []);
  const usedFive = usedPercentFromNano(five?.capacity_nano, five?.remaining_nano);
  const usedWeekly = usedPercentFromNano(weekly?.capacity_nano, weekly?.remaining_nano);
  const nowSec = Number(response.now || Math.floor(nowMs / 1000));
  const exactQuotaRows = quotas.filter((row) => row.amount != null).length;

  return (
    <div className="provider-capacity-board gemini-capacity-board">
      <ProviderCapacityStrip
        ariaLabel="Ёмкость Gemini-пула"
        items={[
          {
            label: "5ч · доступно",
            value: moneyOrDash(five?.remaining_nano),
            caption: `из ${moneyOrDash(five?.capacity_nano)} · текущая смесь`,
            usd: true,
          },
          {
            label: "7д · доступно",
            value: moneyOrDash(weekly?.remaining_nano),
            caption: `из ${moneyOrDash(weekly?.capacity_nano)} · текущая смесь`,
            usd: true,
          },
          {
            label: "5ч · использовано",
            value: usedFive.label,
            caption: "workload-equivalent",
          },
          {
            label: "7д · использовано",
            value: usedWeekly.label,
            caption: "workload-equivalent",
          },
          {
            label: "Профили в ротации",
            value: `${response.available ?? 0}/${response.profiles?.length ?? 0}`,
            caption: `${five?.measured_profiles ?? 0}/${five?.observed_profiles ?? response.profiles?.length ?? 0} измерено · exact ${exactQuotaRows}/${quotas.length}`,
          },
        ]}
      />
      {models.length ? (
        <>
          <GeminiOfficialQuota rows={quotas} nowSec={nowSec} />
          <GeminiProfitability rows={profits} />
        </>
      ) : (
        <div className="provider-no-catalog">Тарифный каталог Gemini недоступен.</div>
      )}
      <GeminiSubscriptions profiles={response.profiles ?? []} modelCount={response.models?.length ?? models.length} nowSec={nowSec} />
    </div>
  );
}
