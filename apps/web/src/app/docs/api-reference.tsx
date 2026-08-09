"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { localeHref } from "@/lib/locale-routes";
import type { IntegrationLanguage, IntegrationProvider } from "./integration-builder-data";
import { buildApiGuide, type ApiLanguage, type ApiStyle } from "./api-reference-data";
import { HighlightedCode } from "./highlighted-code";
import { Prose } from "./prose";

export const API_REFERENCE_PROVIDER_TABS: Array<{ id: IntegrationProvider; name: string; en: string; ru: string }> = [
  { id: "anthropic", name: "Claude", en: "Anthropic Messages API", ru: "Anthropic Messages API" },
  { id: "openai", name: "GPT", en: "OpenAI Responses API", ru: "OpenAI Responses API" },
  { id: "gemini", name: "Gemini", en: "Google Gemini API", ru: "Google Gemini API" },
  // KIMI has no dialect of its own — it is served over Anthropic Messages under its own
  // catalogue namespace, so the label states the wire format the reader will actually use.
  { id: "kimi", name: "Kimi", en: "Anthropic Messages API", ru: "Anthropic Messages API" },
];
const providers = API_REFERENCE_PROVIDER_TABS;

const apiStyles: Array<{ id: ApiStyle; en: string; ru: string }> = [
  { id: "native", en: "Native", ru: "Нативный" },
  { id: "openai-compatible", en: "OpenAI-compatible", ru: "OpenAI-совместимый" },
];

const apiLanguages: Array<{ id: ApiLanguage; name: string }> = [
  { id: "curl", name: "cURL" },
  { id: "python", name: "Python" },
  { id: "typescript", name: "TypeScript" },
];

function tr(language: IntegrationLanguage, en: string, ru: string): string {
  return language === "ru" ? ru : en;
}

export function ApiReference({ language }: { language: IntegrationLanguage }) {
  const [provider, setProvider] = useState<IntegrationProvider>("anthropic");
  const [apiStyle, setApiStyle] = useState<ApiStyle>("native");
  const [apiLanguage, setApiLanguage] = useState<ApiLanguage>("curl");
  const [copiedStep, setCopiedStep] = useState<number | null>(null);
  const [copiedEndpoint, setCopiedEndpoint] = useState(false);

  const guide = useMemo(() => buildApiGuide({ provider, apiStyle, apiLanguage, language }), [provider, apiStyle, apiLanguage, language]);
  const activeProvider = providers.find((candidate) => candidate.id === provider)!;

  async function copyStep(index: number, value: string) {
    await navigator.clipboard.writeText(value);
    setCopiedStep(index);
    window.setTimeout(() => setCopiedStep((current) => current === index ? null : current), 1_200);
  }

  async function copyEndpoint() {
    await navigator.clipboard.writeText(guide.endpoint);
    setCopiedEndpoint(true);
    window.setTimeout(() => setCopiedEndpoint(false), 1_200);
  }

  return <article className="docs-integration-builder api-ref ym-hide-content">
    <div className="api-controls">
      <div className="api-providers" role="group" aria-label={tr(language, "API provider", "API‑провайдер")}>
        {providers.map((candidate) => {
          const active = provider === candidate.id;
          return <button type="button" key={candidate.id} className={active ? "api-provider active" : "api-provider"} aria-pressed={active} onClick={() => setProvider(candidate.id)}>
            <span className={`ib-icon p-${candidate.id}`} aria-hidden="true" />
            <span className="api-provider-text"><strong>{candidate.name}</strong><small>{tr(language, candidate.en, candidate.ru)}</small></span>
          </button>;
        })}
      </div>
      <div className="api-toggles">
        <div className="api-langs" role="group" aria-label={tr(language, "API style", "Стиль API")}>
          {apiStyles.map((candidate) => {
            const active = apiStyle === candidate.id;
            return <button type="button" key={candidate.id} className={active ? "api-lang active" : "api-lang"} aria-pressed={active} onClick={() => setApiStyle(candidate.id)}>{tr(language, candidate.en, candidate.ru)}</button>;
          })}
        </div>
        <div className="api-langs" role="group" aria-label={tr(language, "Programming language", "Язык программирования")}>
          {apiLanguages.map((candidate) => {
            const active = apiLanguage === candidate.id;
            return <button type="button" key={candidate.id} className={active ? "api-lang active" : "api-lang"} aria-pressed={active} onClick={() => setApiLanguage(candidate.id)}>{candidate.name}</button>;
          })}
        </div>
      </div>
    </div>

    <section className="ib-guide" aria-live="polite">
      <header className="ib-guide-head">
        <h4>{guide.title}</h4>
        <p><Prose text={guide.summary} /></p>
        <ul className="ib-chips" aria-label={tr(language, "Current configuration", "Текущая конфигурация")}>
          <li><span className={`ib-icon p-${provider}`} aria-hidden="true" />{activeProvider.name}</li>
          <li className="ib-chip-model">{guide.auth}</li>
        </ul>
      </header>

      <div className="ib-endpoint">
        <span>Endpoint</span><code>{guide.endpoint}</code>
        <button type="button" onClick={copyEndpoint}><CopyIcon copied={copiedEndpoint} />{copiedEndpoint ? tr(language, "Copied", "Скопировано") : tr(language, "Copy", "Копировать")}</button>
      </div>

      <ol className="ib-steps">
        {guide.steps.map((step, index) => <li key={`${provider}-${apiStyle}-${apiLanguage}-${index}`}>
          <div className="ib-step-head">
            <span aria-hidden="true">{index + 1}</span>
            <h5>{step.title}</h5>
          </div>
          <p><Prose text={step.text} /></p>
          <div className="ib-code">
            <div className="ib-code-bar"><i className="ib-dots" aria-hidden="true" /><span>{step.codeLabel}</span><button type="button" onClick={() => copyStep(index, step.code)}><CopyIcon copied={copiedStep === index} />{copiedStep === index ? tr(language, "Copied", "Скопировано") : tr(language, "Copy", "Копировать")}</button></div>
            <pre><code><HighlightedCode code={step.code} /></code></pre>
          </div>
        </li>)}
      </ol>

      <footer className="ib-guide-foot">
        <span><ShieldIcon />{tr(language, "One sk-pool key works on every lane of the unified endpoint. Never ship it in client-side code.", "Один ключ sk-pool работает на всех маршрутах единого endpoint. Не включайте его в клиентский код.")}</span>
        <Link href={localeHref("/dashboard?view=keys", language)}>{tr(language, "Get API key", "Получить API‑ключ")}<ArrowIcon /></Link>
      </footer>
    </section>
  </article>;
}

function CopyIcon({ copied }: { copied: boolean }) {
  return copied
    ? <svg viewBox="0 0 20 20" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m4 10 3.5 3.5L16 5.5" /></svg>
    : <svg viewBox="0 0 20 20" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.7" aria-hidden="true"><rect x="7" y="7" width="9" height="9" rx="2" /><path d="M13 7V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2h2" /></svg>;
}

function ShieldIcon() {
  return <svg viewBox="0 0 20 20" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m10 2.5 6 2.6v4.2c0 3.8-2.5 6.4-6 7.7-3.5-1.3-6-3.9-6-7.7V5.1l6-2.6Z" /><path d="m7.5 9.8 1.6 1.6 3.5-3.6" /></svg>;
}

function ArrowIcon() {
  return <svg viewBox="0 0 20 20" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M3.5 10h13M11.5 5l5 5-5 5" /></svg>;
}
