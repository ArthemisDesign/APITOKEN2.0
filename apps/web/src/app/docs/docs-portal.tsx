"use client";

import Image from "next/image";
import Link from "next/link";
import { useEffect, useState } from "react";
import { useI18n } from "@/components/i18n-provider";
import { ThemeToggle } from "@/components/site-chrome";
import { api } from "@/lib/api";
import { localeHref } from "@/lib/locale-routes";
import { IntegrationBuilder } from "./integration-builder";
import { ApiReference } from "./api-reference";

const AGENT_GUIDE_URL = "https://apitoken.sale/md/connect";
const SUPPORT_TELEGRAM_URL = "https://t.me/apitokensupportbot";
const SECTION_IDS = ["overview", "agent-setup", "setup-support", "quickstart", "api", "errors"] as const;

const copy = {
  en: {
    documentation: "Documentation",
    back: "Back to site",
    dashboard: "Dashboard",
    onThisPage: "On this page",
    overview: "Connect models",
    agentSetup: "AI agent setup",
    support: "Setup support",
    quickstart: "Quick start",
    api: "API",
    errors: "Errors",
    title: "Connect any model",
    lead: "One API key for every available model. Your AI agent configures and verifies the connection.",
    openKeys: "Get an API key",
    agentEyebrow: "For Your Agent",
    agentTitle: "Connect models in one instruction",
    agentLead: "Copy → paste into your agent → provide the API key when asked.",
    agentPrompt: `Connect this project or tool to apiToken.sale. First open ${AGENT_GUIDE_URL} and follow the current instructions. Detect my operating system, shell, runtime, and client or SDK; choose a compatible API protocol and an available model; ask for the sk-pool-… key only if it is not already stored in a secure environment variable; make the smallest required changes; and run a real verification request. Never print the key, put it in source control, or change unrelated files. If anything fails, diagnose it with the guide and explain the result in plain language.`,
    copyAgent: "Copy instruction",
    agentCopied: "Instruction copied",
    openAgentGuide: "Guide",
    agentSteps: [
      "Detects your setup",
      "Selects the right model",
      "Verifies the request",
    ],
    supportTitle: "Connection help",
    supportLead: "IDE, SDK, endpoint, models, and request errors.",
    openSupport: "Telegram",
    supportOnline: "AI 24/7 · human when needed",
    quickstartText: "Connect apiToken.sale to the coding agent that reads, edits, and runs your project. Choose the stack — the exact setup appears below.",
    apiTitle: "Use the API in your code",
    apiText: "One sk-pool key, two compatible surfaces. Pick a provider and a programming language — the exact request for your app, bot, or script appears below.",
    errorTitle: "Common response codes",
    errorText: "Error bodies on the Anthropic surface use Anthropic's JSON envelope; the OpenAI-compatible surface returns the OpenAI envelope — {\"error\":{\"message\",\"type\",\"param\",\"code\"}}. Treat 401 and 402 as account-state failures; retry only transient 429 and 5xx responses.",
    status: "Status",
    meaning: "Meaning",
    action: "What to do",
    e401: "API key is missing, invalid, or revoked",
    a401: "Send an active sk-pool key in x-api-key. If it was revoked, create a replacement; do not retry the same key.",
    e402: "Available prepaid balance is too low",
    a402: "Top up the account, confirm the balance is available, then retry. Backoff alone will not resolve a 402.",
    e429: "Rate limit or temporary upstream capacity limit",
    a429: "Honor Retry-After when present; retry with capped exponential backoff and jitter.",
    e5xx: "Temporary gateway or provider upstream failure",
    a5xx: "Retry with bounded exponential backoff. Keep the request ID and avoid unbounded duplicate attempts.",
    copy: "Copy",
    copied: "Copied",
    copyPage: "Copy page",
    footer: "apiToken.sale documentation · Claude and OpenAI-compatible text access",
  },
  ru: {
    documentation: "Документация",
    back: "На главный сайт",
    dashboard: "Кабинет",
    onThisPage: "На этой странице",
    overview: "Подключение моделей",
    agentSetup: "Настройка через AI‑агента",
    support: "Помощь с подключением",
    quickstart: "Быстрый старт",
    api: "API",
    errors: "Ошибки",
    title: "Подключение моделей",
    lead: "Один API‑ключ — все доступные модели. AI‑агент сам настроит и проверит подключение.",
    openKeys: "Получить API‑ключ",
    agentEyebrow: "Для вашего AI‑агента",
    agentTitle: "Подключите модели одной инструкцией",
    agentLead: "Скопируйте → вставьте агенту → передайте API‑ключ по запросу.",
    agentPrompt: `Подключи этот проект или инструмент к apiToken.sale. Сначала открой ${AGENT_GUIDE_URL} и следуй актуальной инструкции. Определи мою операционную систему, оболочку, среду и клиент или SDK; выбери совместимый API‑протокол и доступную модель; запроси ключ sk-pool-…, только если его нет в безопасной переменной окружения; внеси минимальные изменения и выполни реальный проверочный запрос. Не показывай ключ в логах, не коммить его и не меняй посторонние файлы. Если что‑то не сработает, диагностируй причину по guide и объясни результат простыми словами.`,
    copyAgent: "Скопировать",
    agentCopied: "Инструкция скопирована",
    openAgentGuide: "Инструкция",
    agentSteps: [
      "Определит вашу среду",
      "Выберет нужную модель",
      "Проверит подключение",
    ],
    supportTitle: "Помощь с подключением",
    supportLead: "IDE, SDK, endpoint, модели и ошибки запросов.",
    openSupport: "Telegram",
    supportOnline: "AI 24/7 · человек при необходимости",
    quickstartText: "Подключите apiToken.sale к coding agent, который читает, изменяет и запускает ваш проект. Выберите стек — точная инструкция появится ниже.",
    apiTitle: "Используйте API в своём коде",
    apiText: "Один ключ sk-pool, две совместимые поверхности. Выберите провайдера и язык программирования — готовый запрос для приложения, бота или скрипта появится ниже.",
    errorTitle: "Основные коды ответа",
    errorText: "Тело ошибки на Anthropic-поверхности использует JSON-формат Anthropic; OpenAI-совместимая поверхность возвращает конверт OpenAI — {\"error\":{\"message\",\"type\",\"param\",\"code\"}}. Коды 401 и 402 требуют исправить состояние аккаунта; автоматически повторяйте только временные ошибки 429 и 5xx.",
    status: "Статус",
    meaning: "Значение",
    action: "Что делать",
    e401: "API-ключ отсутствует, неверен или отозван",
    a401: "Передайте активный ключ sk-pool в x-api-key. Если ключ отозван, создайте новый; повторять запрос с тем же ключом не нужно.",
    e402: "Доступного предоплаченного баланса недостаточно",
    a402: "Пополните аккаунт, убедитесь, что баланс зачислен, и повторите запрос. Ожидание само по себе не устранит 402.",
    e429: "Лимит запросов или временный дефицит мощности провайдера",
    a429: "Учитывайте Retry-After, если он есть; используйте ограниченную экспоненциальную задержку со случайным смещением.",
    e5xx: "Временная ошибка шлюза или инфраструктуры провайдера",
    a5xx: "Повторите запрос с ограниченной экспоненциальной задержкой. Сохраните ID запроса и не допускайте бесконечных повторов.",
    copy: "Копировать",
    copied: "Скопировано",
    copyPage: "Копировать страницу",
    footer: "Документация apiToken.sale · Claude и OpenAI-совместимый текстовый API",
  },
} as const;

export function DocsPortal() {
  const { language, setLanguage } = useI18n();
  const t = copy[language];
  const [supportUrl, setSupportUrl] = useState(SUPPORT_TELEGRAM_URL);
  const [activeSection, setActiveSection] = useState<string>("overview");
  const sections: Array<{ id: string; label: string }> = [
    { id: "overview", label: t.overview },
    { id: "agent-setup", label: t.agentSetup },
    { id: "setup-support", label: t.support },
    { id: "quickstart", label: t.quickstart },
    { id: "api", label: t.api },
    { id: "errors", label: t.errors },
  ];

  useEffect(() => {
    let alive = true;
    api.me()
      .then(({ user }) => {
        if (alive && user?.id) setSupportUrl(`${SUPPORT_TELEGRAM_URL}?start=${encodeURIComponent(user.id)}`);
      })
      .catch(() => {/* Public docs keep the non-personalized support URL. */});
    return () => { alive = false; };
  }, []);

  useEffect(() => {
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) setActiveSection(entry.target.id);
      }
    }, { rootMargin: "-15% 0px -75% 0px" });
    for (const id of SECTION_IDS) {
      const element = document.getElementById(id);
      if (element) observer.observe(element);
    }
    return () => observer.disconnect();
  }, []);

  return <div className="docs-site">
    <header className="docs-header">
      <a className="skip-link" href="#main-content">{language === "ru" ? "К содержимому" : "Skip to content"}</a>
      <Link className="docs-brand" href={localeHref("/", language)}><BrandMark /><span>apiToken.sale</span><i>{t.documentation}</i></Link>
      <div className="docs-header-actions">
        <div className="lang" role="group" aria-label={language === "ru" ? "Язык" : "Language"}><button type="button" aria-pressed={language === "en"} className={language === "en" ? "active" : ""} onClick={() => setLanguage("en")}>EN</button><button type="button" aria-pressed={language === "ru"} className={language === "ru" ? "active" : ""} onClick={() => setLanguage("ru")}>RU</button></div>
        <ThemeToggle />
        <Link className="btn btn-ghost btn-sm docs-back" href={localeHref("/", language)}>{t.back}</Link>
        <Link className="btn btn-primary btn-sm" href={localeHref("/dashboard", language)}>{t.dashboard}</Link>
      </div>
    </header>
    <div className="docs-layout">
      <aside className="docs-sidebar"><span>{t.onThisPage}</span><nav>{sections.map(({ id, label }) => <a key={id} href={`#${id}`} className={activeSection === id ? "active" : ""}>{label}</a>)}</nav></aside>
      <main className="docs-main" id="main-content" tabIndex={-1}>
        <section className="docs-hero" id="overview"><div><h1>{t.title}</h1><p>{t.lead}</p></div><CopyPageButton label={t.copyPage} copiedLabel={t.copied} /></section>

        <section className="docs-section docs-connect-section" id="agent-setup">
          <article className="docs-agent-card ym-hide-content">
            <div className="docs-agent-top">
              <div className="docs-agent-heading">
                <span className="docs-agent-eyebrow">{t.agentEyebrow}</span>
                <h2>{t.agentTitle}</h2>
                <p>{t.agentLead}</p>
              </div>
              <div className="docs-agent-actions">
                <CopyControl className="docs-agent-copy" withIcon value={t.agentPrompt} label={t.copyAgent} copiedLabel={t.agentCopied} />
                <Link className="btn btn-ghost docs-guide-link" href="/md/connect" target="_blank"><GuideIcon />{t.openAgentGuide}</Link>
              </div>
            </div>
            <div className="docs-agent-bottom">
              <ul className="docs-agent-points">{t.agentSteps.map((step) => <li key={step}><CheckIcon />{step}</li>)}</ul>
              <Link href={localeHref("/dashboard?view=keys", language)}>{t.openKeys}<ArrowIcon /></Link>
            </div>
          </article>
        </section>

        <section className="docs-section docs-support-section" id="setup-support">
          <article className="docs-connect-support">
            <span className="docs-support-icon" aria-hidden="true"><TelegramIcon /></span>
            <div className="docs-support-copy">
              <h2>{t.supportTitle}</h2>
              <p>{t.supportLead}</p>
            </div>
            <div className="docs-support-actions">
              <a className="btn btn-primary" href={supportUrl} target="_blank" rel="noreferrer"><TelegramIcon />{t.openSupport}</a>
              <span><i aria-hidden="true" />{t.supportOnline}</span>
            </div>
          </article>
        </section>

        <section className="docs-section" id="quickstart">
          <div className="docs-section-heading"><span>02</span><div><h2>{t.quickstart}</h2><p>{t.quickstartText}</p></div></div>
          <IntegrationBuilder language={language} />
        </section>

        <section className="docs-section" id="api">
          <div className="docs-section-heading"><span>03</span><div><h2>{t.apiTitle}</h2><p>{t.apiText}</p></div></div>
          <ApiReference language={language} />
        </section>

        <section className="docs-section" id="errors">
          <div className="docs-section-heading"><span>04</span><div><h2>{t.errorTitle}</h2><p>{t.errorText}</p></div></div>
          <div className="table-scroll"><table className="mtable docs-errors"><thead><tr><th>{t.status}</th><th>{t.meaning}</th><th>{t.action}</th></tr></thead><tbody><ErrorRow code="401" meaning={t.e401} action={t.a401} labels={t} /><ErrorRow code="402" meaning={t.e402} action={t.a402} labels={t} /><ErrorRow code="429" meaning={t.e429} action={t.a429} labels={t} /><ErrorRow code="5xx" meaning={t.e5xx} action={t.a5xx} labels={t} /></tbody></table></div>
        </section>

        <footer className="docs-footer">{t.footer}</footer>
      </main>
    </div>
  </div>;
}

function BrandMark() {
  return <><Image className="brand-mark bm-light" src="/assets/logo-mark-light.png" width={24} height={24} alt="" /><Image className="brand-mark bm-dark" src="/assets/logo-mark-dark.png" width={24} height={24} alt="" /></>;
}

function CopyControl({ value, label, copiedLabel, className = "", withIcon = false }: { value: string; label: string; copiedLabel: string; className?: string; withIcon?: boolean }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1_200);
  }

  return <button className={`btn btn-ghost btn-sm docs-copy ${className}`.trim()} type="button" onClick={handleCopy}>{withIcon && <CopyIcon copied={copied} />}<span>{copied ? copiedLabel : label}</span></button>;
}

function ErrorRow({ code, meaning, action, labels }: { code: string; meaning: string; action: string; labels: { status: string; meaning: string; action: string } }) {
  return <tr><td data-label={labels.status}><code>{code}</code></td><td data-label={labels.meaning}><span>{meaning}</span></td><td data-label={labels.action}><span>{action}</span></td></tr>;
}

// Copies the whole page as markdown (served by /md/docs) — the reference
// "Copy page" affordance for pasting the docs into an LLM.
function CopyPageButton({ label, copiedLabel }: { label: string; copiedLabel: string }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    const response = await fetch("/md/docs");
    if (!response.ok) return;
    await navigator.clipboard.writeText(await response.text());
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1_600);
  }

  return <button className="btn btn-ghost btn-sm docs-copy-page" type="button" onClick={handleCopy}><CopyIcon copied={copied} /><span>{copied ? copiedLabel : label}</span></button>;
}

function CopyIcon({ copied }: { copied: boolean }) {
  return copied
    ? <svg viewBox="0 0 24 24" width="19" height="19" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m5 12 4 4L19 6" /></svg>
    : <svg viewBox="0 0 24 24" width="19" height="19" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><rect x="8" y="8" width="11" height="11" rx="2" /><path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3" /></svg>;
}

function GuideIcon() {
  return <svg viewBox="0 0 24 24" width="19" height="19" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M5 4.5A2.5 2.5 0 0 1 7.5 2H20v17H7.5A2.5 2.5 0 0 0 5 21.5v-17Z" /><path d="M5 4.5v17M9 6h7M9 10h7" /></svg>;
}

function ArrowIcon() {
  return <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M5 12h14M13 6l6 6-6 6" /></svg>;
}

function CheckIcon() {
  return <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m5 12 4 4L19 6" /></svg>;
}

function TelegramIcon() {
  return <svg viewBox="0 0 24 24" width="21" height="21" fill="currentColor" aria-hidden="true"><path d="M21.7 2.3 2.9 10.5c-1.3.5-1.3 1.3-.2 1.7l4.8 1.5 1.8 5.4c.2.7.1 1 .9 1 .6 0 .9-.3 1.2-.6l2.3-2.2 4.8 3.5c.9.5 1.5.2 1.8-.8l3.1-14.9c.4-1.3-.5-1.9-1.7-1.4ZM9.4 13.4l9.4-5.9c.5-.3.9-.1.5.2l-7.7 7-.3 3.1-1.9-4.4Z" /></svg>;
}
