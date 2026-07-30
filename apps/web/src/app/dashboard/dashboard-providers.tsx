"use client";

// Раздел «Providers» (web/v2): вместо каталога отдельных моделей — карточки двух провайдеров,
// которые обслуживает пул: Anthropic (байт-в-байт Messages API) и OpenAI (совместимый API).
// Данные — из src/lib/models.ts (эндпоинты и состав линеек синхронизированы с движком);
// скидка — плоские −50% для всех аккаунтов (lib/pricing-tiers.ts).
import { useMemo } from "react";
import { ANTHROPIC_BASE_URL, OPENAI_BASE_URL, claudeModels, openaiModels } from "@/lib/models";
import { FLAT_DISCOUNT_PERCENT } from "@/lib/pricing-tiers";
import { useI18n } from "@/components/i18n-provider";
import { dashboardCopy, type DashboardCopy } from "@/lib/dashboard-copy";
import { CopyButton, PageHeading } from "./dashboard-sections";

type ProviderCard = {
  key: "anthropic" | "openai";
  name: string;
  mark: string;
  endpoint: string;
  models: number;
  lineup: string;
  descKey: keyof DashboardCopy;
  apiKindKey: keyof DashboardCopy;
};

const PROVIDERS: ProviderCard[] = [
  {
    key: "anthropic",
    name: "Anthropic",
    mark: "A",
    endpoint: ANTHROPIC_BASE_URL,
    models: claudeModels.length,
    lineup: "Claude Opus · Sonnet · Haiku",
    descKey: "providerAnthropicDesc",
    apiKindKey: "providerAnthropicApi",
  },
  {
    key: "openai",
    name: "OpenAI",
    mark: "G",
    endpoint: OPENAI_BASE_URL,
    models: openaiModels.length,
    lineup: "GPT-5.6 Sol · Terra · Luna, GPT-5.5, GPT-5.4",
    descKey: "providerOpenaiDesc",
    apiKindKey: "providerOpenaiApi",
  },
];

export function ProvidersCatalog() {
  const { language } = useI18n();
  const copy = useMemo(() => dashboardCopy[language], [language]);

  return <section className="panel">
    <PageHeading eyebrow={copy.providersEyebrow} title={copy.providersTitle} subtitle={copy.providersSubtitle} />
    <div className="models-list">
      {PROVIDERS.map((provider) => <article key={provider.key} className="card model-card">
        <div className="model-card-head">
          <span className={`model-provider-mark ${provider.key}`} aria-hidden="true">{provider.mark}</span>
          <h2 className="model-name">{provider.name}</h2>
          <span className="model-tier">{copy[provider.apiKindKey]}</span>
        </div>
        <p className="model-desc">{copy[provider.descKey]}</p>
        <div className="provider-endpoint">
          <span className="provider-endpoint-label">{copy.providerEndpoint}</span>
          <code className="model-id">{provider.endpoint}</code>
          <CopyButton value={provider.endpoint} className="model-copy" label={copy.providerCopyEndpoint} copiedLabel={copy.copied} />
        </div>
        <div className="model-meta">
          <span>{copy.modelsBy} <strong>{provider.key}</strong></span>
          <span>{provider.models} {copy.providerModels}</span>
          <span>{provider.lineup}</span>
          <span className="model-price">−{FLAT_DISCOUNT_PERCENT}% {copy.providerOffOfficial}</span>
        </div>
      </article>)}
    </div>
    <p className="providers-note">{copy.providersNote}</p>
  </section>;
}
