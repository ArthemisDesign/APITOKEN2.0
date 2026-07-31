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

const tools: Array<{ id: IntegrationTool; name: string; en: string; ru: string; mark: string }> = [
  { id: "claude-code", name: "Claude Code", en: "Native Claude agent", ru: "Нативный Claude agent", mark: "✦" },
  { id: "codex", name: "Codex", en: "Responses API agent", ru: "Responses API agent", mark: "⌁" },
  { id: "opencode", name: "OpenCode", en: "Open-source terminal", ru: "Open-source терминал", mark: "OC" },
  { id: "pi", name: "Pi", en: "Minimal coding harness", ru: "Минималистичный harness", mark: "π" },
  { id: "hermes", name: "Hermes", en: "General agent · advanced", ru: "Универсальный · advanced", mark: "☤" },
];

const operatingSystems: Array<{ id: IntegrationOs; name: string; detail: string; mark: string }> = [
  { id: "macos", name: "macOS", detail: "zsh", mark: "⌘" },
  { id: "linux", name: "Linux", detail: "bash", mark: ">_" },
  { id: "powershell", name: "Windows", detail: "PowerShell", mark: "PS" },
  { id: "cmd", name: "Windows", detail: "CMD", mark: "C:\\" },
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
  const [os, setOs] = useState<IntegrationOs>("macos");
  const [modelId, setModelId] = useState(INTEGRATION_MODELS.anthropic[0].id);
  const [copiedStep, setCopiedStep] = useState<number | null>(null);

  const guide = useMemo(() => buildIntegrationGuide({ provider, tool, os, modelId, language }), [provider, tool, os, modelId, language]);

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

  return <article className="docs-integration-builder ym-hide-content">
    <header className="docs-builder-head">
      <div>
        <span className="docs-builder-eyebrow">{tr(language, "Integrations", "Интеграции")}</span>
        <h3>{tr(language, "Connect your coding agent", "Подключите coding agent")}</h3>
        <p>{tr(language, "Choose a model provider, coding environment, and operating system. The guide updates instantly.", "Выберите провайдера модели, coding‑среду и операционную систему. Инструкция обновится сразу.")}</p>
      </div>
      <span className="docs-builder-live"><i aria-hidden="true" />{tr(language, "Live setup", "Живая инструкция")}</span>
    </header>

    <div className="docs-builder-layout">
      <div className="docs-builder-controls">
        <fieldset className="docs-builder-field">
          <legend><b>1</b><span>{tr(language, "Model provider", "Провайдер модели")}</span></legend>
          <div className="docs-provider-options">
            <button type="button" className={provider === "anthropic" ? "active" : ""} aria-pressed={provider === "anthropic"} onClick={() => setProvider("anthropic")}>
              <ProviderIcon provider="anthropic" /><span><strong>Claude</strong><small>Anthropic API</small></span><CheckMark />
            </button>
            <button type="button" className={provider === "openai" ? "active" : ""} aria-pressed={provider === "openai"} onClick={() => setProvider("openai")}>
              <ProviderIcon provider="openai" /><span><strong>GPT</strong><small>OpenAI-compatible</small></span><CheckMark />
            </button>
          </div>
          <label className="docs-model-select">
            <span>{tr(language, "Model", "Модель")}</span>
            <select value={modelId} onChange={(event) => setModelId(event.target.value)}>
              {INTEGRATION_MODELS[provider].map((model) => <option value={model.id} key={model.id}>{model.name}</option>)}
            </select>
          </label>
        </fieldset>

        <fieldset className="docs-builder-field">
          <legend><b>2</b><span>{tr(language, "Coding agent", "Coding agent")}</span></legend>
          <div className="docs-tool-options">
            {tools.map((candidate) => {
              const compatible = isToolCompatible(candidate.id, provider);
              return <button type="button" key={candidate.id} disabled={!compatible} className={tool === candidate.id ? "active" : ""} aria-pressed={tool === candidate.id} onClick={() => setTool(candidate.id)}>
                <ToolMark mark={candidate.mark} tool={candidate.id} />
                <span><strong>{candidate.name}</strong><small>{compatible ? (language === "ru" ? candidate.ru : candidate.en) : tr(language, provider === "anthropic" ? "GPT only" : "Claude only", provider === "anthropic" ? "Только GPT" : "Только Claude")}</small></span>
                {tool === candidate.id && <CheckMark />}
              </button>;
            })}
          </div>
        </fieldset>

        <fieldset className="docs-builder-field">
          <legend><b>3</b><span>{tr(language, "Operating system", "Операционная система")}</span></legend>
          <div className="docs-os-options">
            {operatingSystems.map((candidate) => <button type="button" key={candidate.id} className={os === candidate.id ? "active" : ""} aria-pressed={os === candidate.id} onClick={() => setOs(candidate.id)}>
              <span className="docs-os-mark" aria-hidden="true">{candidate.mark}</span><span><strong>{candidate.name}</strong><small>{candidate.detail}</small></span>
            </button>)}
          </div>
        </fieldset>
      </div>

      <section className="docs-builder-output" aria-live="polite">
        <header>
          <ToolMark mark={tools.find((candidate) => candidate.id === tool)?.mark ?? ""} tool={tool} />
          <div><span>{tr(language, "Your setup", "Ваша инструкция")}</span><h4>{guide.title}</h4><p>{guide.summary}</p></div>
          <span className="docs-builder-ready"><CheckMark />{tr(language, "Ready", "Готово")}</span>
        </header>

        <div className="docs-builder-endpoint"><span>Endpoint</span><code>{guide.endpoint}</code></div>
        {guide.requirement && <div className="docs-builder-requirement"><InfoIcon /><span>{guide.requirement}</span></div>}

        <div className="docs-builder-steps">
          {guide.steps.map((step, index) => <article key={`${tool}-${provider}-${os}-${index}`}>
            <span className="docs-builder-step-number">{String(index + 1).padStart(2, "0")}</span>
            <div className="docs-builder-step-body">
              <h5>{step.title}</h5>
              <p>{step.text}</p>
              <div className="docs-builder-code">
                <div><span>{step.codeLabel}</span><button type="button" onClick={() => copyStep(index, step.code)}><CopyIcon copied={copiedStep === index} />{copiedStep === index ? tr(language, "Copied", "Скопировано") : tr(language, "Copy", "Копировать")}</button></div>
                <pre><code>{step.code}</code></pre>
              </div>
            </div>
          </article>)}
        </div>

        <footer>
          <span><ShieldIcon />{guide.securityNote ?? tr(language, "The full key stays in your terminal, never in project files.", "Полный ключ остаётся в терминале и не попадает в файлы проекта.")}</span>
          <Link href={localeHref("/dashboard?view=keys", language)}>{tr(language, "Get API key", "Получить API‑ключ")}<ArrowIcon /></Link>
        </footer>
      </section>
    </div>
  </article>;
}

function ProviderIcon({ provider }: { provider: IntegrationProvider }) {
  return provider === "anthropic"
    ? <span className="docs-provider-mark anthropic" aria-hidden="true" />
    : <span className="docs-provider-mark openai" aria-hidden="true" />;
}

function ToolMark({ mark, tool }: { mark: string; tool: IntegrationTool }) {
  return <span className={`docs-tool-mark tool-${tool}`} aria-hidden="true">{mark}</span>;
}

function CheckMark() {
  return <svg className="docs-builder-check" viewBox="0 0 20 20" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m4 10 3.5 3.5L16 5.5" /></svg>;
}

function CopyIcon({ copied }: { copied: boolean }) {
  return copied
    ? <svg viewBox="0 0 20 20" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m4 10 3.5 3.5L16 5.5" /></svg>
    : <svg viewBox="0 0 20 20" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.7" aria-hidden="true"><rect x="7" y="7" width="9" height="9" rx="2" /><path d="M13 7V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2h2" /></svg>;
}

function InfoIcon() {
  return <svg viewBox="0 0 20 20" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true"><circle cx="10" cy="10" r="7.5" /><path d="M10 9v5M10 6.2h.01" /></svg>;
}

function ShieldIcon() {
  return <svg viewBox="0 0 20 20" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m10 2.5 6 2.6v4.2c0 3.8-2.5 6.4-6 7.7-3.5-1.3-6-3.9-6-7.7V5.1l6-2.6Z" /><path d="m7.5 9.8 1.6 1.6 3.5-3.6" /></svg>;
}

function ArrowIcon() {
  return <svg viewBox="0 0 20 20" width="17" height="17" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M3.5 10h13M11.5 5l5 5-5 5" /></svg>;
}
