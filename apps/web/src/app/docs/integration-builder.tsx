"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { localeHref } from "@/lib/locale-routes";
import {
  INTEGRATION_MODELS,
  buildIntegrationGuide,
  isToolCompatible,
  type IntegrationLanguage,
  type IntegrationOs,
  type IntegrationProvider,
  type IntegrationTool,
} from "./integration-builder-data";
import { highlightCode } from "./integration-highlight";

const providers: Array<{ id: IntegrationProvider; name: string; en: string; ru: string }> = [
  { id: "anthropic", name: "Claude", en: "Anthropic Messages API", ru: "Anthropic Messages API" },
  { id: "openai", name: "GPT", en: "OpenAI-compatible API", ru: "OpenAI-совместимый API" },
];

const tools: Array<{ id: IntegrationTool; name: string; en: string; ru: string }> = [
  { id: "claude-code", name: "Claude Code", en: "Native Claude agent", ru: "Нативный агент Claude" },
  { id: "codex", name: "Codex", en: "Responses API agent", ru: "Агент Responses API" },
  { id: "opencode", name: "OpenCode", en: "Open-source terminal", ru: "Open-source терминал" },
  { id: "pi", name: "Pi", en: "Minimal coding harness", ru: "Минималистичный harness" },
  { id: "hermes", name: "Hermes", en: "General agent · advanced", ru: "Универсальный · advanced" },
];

const operatingSystems: Array<{ id: IntegrationOs; name: string; detail: string }> = [
  { id: "unix", name: "macOS / Linux", detail: "zsh · bash" },
  { id: "powershell", name: "Windows", detail: "PowerShell" },
  { id: "cmd", name: "Windows", detail: "CMD" },
];

const defaultTool: Record<IntegrationProvider, IntegrationTool> = {
  anthropic: "claude-code",
  openai: "codex",
};

function tr(language: IntegrationLanguage, en: string, ru: string): string {
  return language === "ru" ? ru : en;
}

export function IntegrationBuilder({ language }: { language: IntegrationLanguage }) {
  const [provider, setProviderState] = useState<IntegrationProvider>("anthropic");
  const [tool, setTool] = useState<IntegrationTool>("claude-code");
  const [os, setOs] = useState<IntegrationOs>("unix");
  const [modelId, setModelId] = useState(INTEGRATION_MODELS.anthropic[0].id);
  const [copiedStep, setCopiedStep] = useState<number | null>(null);
  const [copiedEndpoint, setCopiedEndpoint] = useState(false);

  const guide = useMemo(() => buildIntegrationGuide({ provider, tool, os, modelId, language }), [provider, tool, os, modelId, language]);
  const activeProvider = providers.find((candidate) => candidate.id === provider)!;
  const activeTool = tools.find((candidate) => candidate.id === tool)!;
  const activeOs = operatingSystems.find((candidate) => candidate.id === os)!;
  const activeModel = INTEGRATION_MODELS[provider].find((model) => model.id === modelId) ?? INTEGRATION_MODELS[provider][0];

  function setProvider(nextProvider: IntegrationProvider) {
    setProviderState(nextProvider);
    setModelId(INTEGRATION_MODELS[nextProvider][0].id);
    if (!isToolCompatible(tool, nextProvider)) setTool(defaultTool[nextProvider]);
  }

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

  return <article className="docs-integration-builder ym-hide-content">
    <header className="ib-head">
      <span className="ib-eyebrow">{tr(language, "Integrations", "Интеграции")}</span>
      <h3>{tr(language, "Connect your coding agent", "Подключите coding agent")}</h3>
      <p>{tr(language, "Choose a provider, a coding agent, and your operating system — the guide updates instantly.", "Выберите провайдера, coding agent и операционную систему — инструкция обновится сразу.")}</p>
    </header>

    <div className="ib-layout">
      <div className="ib-controls">
        <section className="ib-group" aria-label={tr(language, "Model provider", "Провайдер модели")}>
          <h6>{tr(language, "Provider", "Провайдер")}</h6>
          <div className="ib-rows">
            {providers.map((candidate) => {
              const active = provider === candidate.id;
              return <button type="button" key={candidate.id} className={active ? "ib-row active" : "ib-row"} aria-pressed={active} onClick={() => setProvider(candidate.id)}>
                <span className={`ib-icon p-${candidate.id}`} aria-hidden="true" />
                <span className="ib-row-text"><strong>{candidate.name}</strong><small>{tr(language, candidate.en, candidate.ru)}</small></span>
                {active && <CheckIcon />}
              </button>;
            })}
          </div>
        </section>

        <section className="ib-group" aria-label={tr(language, "Model", "Модель")}>
          <h6>{tr(language, "Model", "Модель")}</h6>
          <label className="ib-select">
            <select value={modelId} onChange={(event) => setModelId(event.target.value)} aria-label={tr(language, "Model", "Модель")}>
              {INTEGRATION_MODELS[provider].map((model) => <option value={model.id} key={model.id}>{model.name}</option>)}
            </select>
            <ChevronIcon />
          </label>
        </section>

        <section className="ib-group" aria-label={tr(language, "Operating system", "Операционная система")}>
          <h6>{tr(language, "Operating system", "Операционная система")}</h6>
          <div className="ib-rows">
            {operatingSystems.map((candidate) => {
              const active = os === candidate.id;
              return <button type="button" key={candidate.id} className={active ? "ib-row active" : "ib-row"} aria-pressed={active} onClick={() => setOs(candidate.id)}>
                <span className={`ib-icon o-${candidate.id === "powershell" || candidate.id === "cmd" ? "windows" : candidate.id}`} aria-hidden="true" />
                <span className="ib-row-text"><strong>{candidate.name}</strong><small>{candidate.detail}</small></span>
                {active && <CheckIcon />}
              </button>;
            })}
          </div>
        </section>

        <section className="ib-group" aria-label={tr(language, "Coding agent", "Coding agent")}>
          <h6>{tr(language, "Coding agent", "Coding agent")}</h6>
          <div className="ib-rows">
            {tools.map((candidate) => {
              const compatible = isToolCompatible(candidate.id, provider);
              const active = tool === candidate.id;
              return <button type="button" key={candidate.id} disabled={!compatible} className={active ? "ib-row active" : "ib-row"} aria-pressed={active} onClick={() => setTool(candidate.id)}>
                <span className={`ib-icon t-${candidate.id}`} aria-hidden="true" />
                <span className="ib-row-text"><strong>{candidate.name}</strong><small>{tr(language, candidate.en, candidate.ru)}</small></span>
                {active
                  ? <CheckIcon />
                  : !compatible && <em className="ib-row-tag">{tr(language, provider === "anthropic" ? "GPT only" : "Claude only", provider === "anthropic" ? "Только GPT" : "Только Claude")}</em>}
              </button>;
            })}
          </div>
        </section>
      </div>

      <section className="ib-guide" aria-live="polite">
        <header className="ib-guide-head">
          <div>
            <h4>{guide.title}</h4>
            <p>{guide.summary}</p>
            <ul className="ib-chips" aria-label={tr(language, "Current configuration", "Текущая конфигурация")}>
              <li><span className={`ib-icon p-${provider}`} aria-hidden="true" />{activeProvider.name}</li>
              <li><span className={`ib-icon t-${tool}`} aria-hidden="true" />{activeTool.name}</li>
              <li><span className={`ib-icon o-${os === "powershell" || os === "cmd" ? "windows" : os}`} aria-hidden="true" />{activeOs.name} · {activeOs.detail}</li>
              <li className="ib-chip-model">{activeModel.name}</li>
            </ul>
          </div>
        </header>

        <div className="ib-endpoint">
          <span>Endpoint</span><code>{guide.endpoint}</code>
          <button type="button" onClick={copyEndpoint}><CopyIcon copied={copiedEndpoint} />{copiedEndpoint ? tr(language, "Copied", "Скопировано") : tr(language, "Copy", "Копировать")}</button>
        </div>
        {guide.requirement && <p className="ib-callout"><InfoIcon />{guide.requirement}</p>}

        <ol className="ib-steps">
          {guide.steps.map((step, index) => <li key={`${tool}-${provider}-${os}-${index}`}>
            <div className="ib-step-head">
              <span aria-hidden="true">{index + 1}</span>
              <h5>{step.title}</h5>
            </div>
            <p>{step.text}</p>
            <div className="ib-code">
              <div className="ib-code-bar"><i className="ib-dots" aria-hidden="true" /><span>{step.codeLabel}</span><button type="button" onClick={() => copyStep(index, step.code)}><CopyIcon copied={copiedStep === index} />{copiedStep === index ? tr(language, "Copied", "Скопировано") : tr(language, "Copy", "Копировать")}</button></div>
              <pre><code><HighlightedCode code={step.code} /></code></pre>
            </div>
          </li>)}
        </ol>

        <footer className="ib-guide-foot">
          <span><ShieldIcon />{guide.securityNote ?? tr(language, "The full key stays in your terminal, never in project files.", "Полный ключ остаётся в терминале и не попадает в файлы проекта.")}</span>
          <Link href={localeHref("/dashboard?view=keys", language)}>{tr(language, "Get API key", "Получить API‑ключ")}<ArrowIcon /></Link>
        </footer>
      </section>
    </div>
  </article>;
}

function HighlightedCode({ code }: { code: string }) {
  const tokens = useMemo(() => highlightCode(code), [code]);
  return <>{tokens.map((part, index) => part.cls
    ? <span key={index} className={`tk-${part.cls}`}>{part.text}</span>
    : <span key={index}>{part.text}</span>)}</>;
}

function CheckIcon() {
  return <svg className="ib-check" viewBox="0 0 20 20" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m4 10 3.5 3.5L16 5.5" /></svg>;
}

function ChevronIcon() {
  return <svg viewBox="0 0 20 20" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m5.5 7.5 4.5 4.5 4.5-4.5" /></svg>;
}

function CopyIcon({ copied }: { copied: boolean }) {
  return copied
    ? <svg viewBox="0 0 20 20" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m4 10 3.5 3.5L16 5.5" /></svg>
    : <svg viewBox="0 0 20 20" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.7" aria-hidden="true"><rect x="7" y="7" width="9" height="9" rx="2" /><path d="M13 7V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2h2" /></svg>;
}

function InfoIcon() {
  return <svg viewBox="0 0 20 20" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true"><circle cx="10" cy="10" r="7.5" /><path d="M10 9v5M10 6.2h.01" /></svg>;
}

function ShieldIcon() {
  return <svg viewBox="0 0 20 20" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m10 2.5 6 2.6v4.2c0 3.8-2.5 6.4-6 7.7-3.5-1.3-6-3.9-6-7.7V5.1l6-2.6Z" /><path d="m7.5 9.8 1.6 1.6 3.5-3.6" /></svg>;
}

function ArrowIcon() {
  return <svg viewBox="0 0 20 20" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M3.5 10h13M11.5 5l5 5-5 5" /></svg>;
}
