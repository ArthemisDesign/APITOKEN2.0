"use client";

// Раздел «Модели» (web/v2): каталог всех Claude/GPT-моделей пула в стиле списка OpenRouter.
// Источник данных — src/lib/models.ts (официальные ставки, синхронизированные с pinned-каталогом
// движка crates/metering); цены на карточках показываются С УЧЁТОМ плоской скидки 50%
// (единая для всех аккаунтов, тиров нет), официальная ставка — зачёркнутой рядом.
import { useMemo, useState } from "react";
import { claudeModels, openaiModels, formatUsd, type CatalogModel } from "@/lib/models";
import { FLAT_DISCOUNT_PERCENT, FLAT_PRICE_MULTIPLIER } from "@/lib/pricing-tiers";
import { useI18n } from "@/components/i18n-provider";
import { dashboardCopy } from "@/lib/dashboard-copy";
import { CopyButton, PageHeading } from "./dashboard-sections";

// Реальные даты релизов моделей у провайдеров (официальные анонсы Anthropic/OpenAI).
const RELEASED: Record<string, string> = {
  "claude-opus-4-8": "2026-05-28",
  "claude-opus-4-7": "2026-04-16",
  "claude-sonnet-5": "2026-06-30",
  "claude-sonnet-4-6": "2026-02-17",
  "claude-haiku-4-5": "2025-10-15",
  "gpt-5.6-sol": "2026-07-09",
  "gpt-5.6-terra": "2026-07-09",
  "gpt-5.6-luna": "2026-07-09",
  "gpt-5.5": "2026-04-23",
  "gpt-5.4": "2026-03-05",
};

const ALL_MODELS: CatalogModel[] = [...claudeModels, ...openaiModels].sort(
  (a, b) => (RELEASED[b.id] ?? "").localeCompare(RELEASED[a.id] ?? ""),
);

type ProviderFilter = "all" | "anthropic" | "openai";

function template(text: string, values: Record<string, string>): string {
  return text.replace(/\{(\w+)\}/g, (_, key: string) => values[key] ?? `{${key}}`);
}

export function ModelsCatalog() {
  const { language } = useI18n();
  const copy = dashboardCopy[language];
  const [query, setQuery] = useState("");
  const [provider, setProvider] = useState<ProviderFilter>("all");

  // Плоская скидка для всех: клиент платит половину официальной ставки провайдера.
  const discountPercent = FLAT_DISCOUNT_PERCENT;
  const yourPrice = (officialPerM: number) => formatUsd(officialPerM * FLAT_PRICE_MULTIPLIER);

  const dateFormat = useMemo(
    () => new Intl.DateTimeFormat(language === "ru" ? "ru-RU" : "en-US", { month: "short", day: "numeric", year: "numeric" }),
    [language],
  );

  const visible = ALL_MODELS.filter((model) => {
    if (provider !== "all" && model.provider !== provider) return false;
    const needle = query.trim().toLowerCase();
    if (!needle) return true;
    return `${model.name} ${model.id} ${model.tier}`.toLowerCase().includes(needle);
  });

  return <section className="panel">
    <PageHeading eyebrow={copy.modelsEyebrow} title={copy.modelsTitle} subtitle={copy.modelsSubtitle} />
    <div className="models-toolbar">
      <input
        className="set-in models-search"
        type="search"
        value={query}
        placeholder={copy.modelsSearch}
        aria-label={copy.modelsSearch}
        onChange={(event) => setQuery(event.target.value)}
      />
      <div className="chart-toggle" role="tablist">
        {([["all", copy.modelsAll], ["anthropic", "Claude"], ["openai", "GPT"]] as const).map(([value, label]) => (
          <button key={value} type="button" className={provider === value ? "on" : ""} onClick={() => setProvider(value)}>{label}</button>
        ))}
      </div>
      <span className="models-discount pill" title={template(copy.modelsDiscountNote, { discount: String(discountPercent) })}>
        −{discountPercent}%
      </span>
    </div>
    <div className="models-list">
      {visible.map((model) => {
        const released = RELEASED[model.id];
        return <article key={model.id} className="card model-card">
          <div className="model-card-head">
            <span className={`model-provider-mark ${model.provider}`} aria-hidden="true">{model.provider === "anthropic" ? "A" : "G"}</span>
            <h2 className="model-name">{model.name}</h2>
            <code className="model-id">{model.id}</code>
            <CopyButton value={model.id} className="model-copy" label={copy.modelsCopyId} copiedLabel={copy.copied} />
            <span className="model-tier">{model.tier}</span>
          </div>
          <p className="model-desc">{model.dek}</p>
          <div className="model-meta">
            <span>{copy.modelsBy} <strong>{model.provider === "anthropic" ? "anthropic" : "openai"}</strong></span>
            {released && <span>{dateFormat.format(new Date(`${released}T00:00:00Z`))}</span>}
            <span>{model.context.replace(" tokens", "")} {copy.modelsContext}</span>
            <span
              className="model-price"
              title={template(copy.modelsOfficialNote, { input: formatUsd(model.inputPerM), output: formatUsd(model.outputPerM) })}
            >
              {yourPrice(model.inputPerM)}<s>{formatUsd(model.inputPerM)}</s>{copy.modelsInputSuffix}
            </span>
            <span
              className="model-price"
              title={template(copy.modelsOfficialNote, { input: formatUsd(model.inputPerM), output: formatUsd(model.outputPerM) })}
            >
              {yourPrice(model.outputPerM)}<s>{formatUsd(model.outputPerM)}</s>{copy.modelsOutputSuffix}
            </span>
          </div>
        </article>;
      })}
      {visible.length === 0 && <p className="models-empty">{copy.modelsEmpty}</p>}
    </div>
  </section>;
}
