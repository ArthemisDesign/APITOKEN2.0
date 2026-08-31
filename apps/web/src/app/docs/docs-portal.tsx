"use client";

import Image from "next/image";
import Link from "next/link";
import { useEffect, useState } from "react";
import { useI18n } from "@/components/i18n-provider";
import { ThemeToggle } from "@/components/site-chrome";
import { api } from "@/lib/api";
import { localeHref } from "@/lib/locale-routes";
import { IntegrationBuilder } from "./integration-builder";
import { ROUTER_BASE_URL } from "./integration-builder-data";
import { ApiReference } from "./api-reference";
import { ModelsPricing } from "./models-pricing";
import { GeminiBatchGuide } from "./gemini-batch-guide";
import { HighlightedCode } from "./highlighted-code";
import { Prose } from "./prose";
import type { OpenAiModel } from "@/lib/models";

const CLAUDE_CACHE_JSON = `{
  "model": "claude-opus-4-8",
  "max_tokens": 1024,
  "system": [
    {
      "type": "text",
      "text": "Long stable instructions…",
      "cache_control": { "type": "ephemeral" }
    }
  ],
  "messages": [{ "role": "user", "content": "Short varying question" }]
}`;
const GPT_CACHE_JSON = `{
  "usage": {
    "prompt_tokens": 5120,
    "prompt_tokens_details": { "cached_tokens": 4096 },
    "completion_tokens": 128
  }
}`;

const GEMINI_INLINE_JSON = `{
  "contents": [
    {
      "role": "user",
      "parts": [
        { "text": "Describe this file" },
        { "inline_data": { "mime_type": "image/png", "data": "<base64>" } }
      ]
    }
  ]
}`;

const AGENT_GUIDE_URL = "https://github.com/apitokensale-admin/apitoken.sale/blob/main/skills/use-apitoken/SKILL.md";
const SUPPORT_TELEGRAM_URL = "https://t.me/apitokensupportbot";
const SECTION_IDS = ["overview", "agent-setup", "setup-support", "quickstart", "api", "usage", "models", "gemini-batch", "gemini-files", "errors", "caching", "next"] as const;

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
    usageBalance: "Balance & usage",
    errors: "Errors",
    title: "Connect any model",
    lead: "One API key, one endpoint, every available model — native Anthropic, OpenAI, and Gemini protocols plus an OpenAI-compatible API for any client. Your AI agent configures and verifies the connection.",
    agentPrompt: `Read ${AGENT_GUIDE_URL} and follow the instructions to connect this project to apiToken.sale.`,
    agentLabel: "For your agent",
    copyAgent: "Copy instruction",
    agentCopied: "Instruction copied",
    openAgentGuide: "Open the guide",
    supportTitle: "Connection help",
    supportLead: "IDE, SDK, endpoint, models, and request errors.",
    openSupport: "Telegram",
    supportOnline: "AI 24/7 · human when needed",
    quickstartText: "Connect apiToken.sale to the coding agent that reads, edits, and runs your project. Choose the stack — the exact setup appears below.",
    apiTitle: "One endpoint, every protocol",
    apiText: "router.apitoken.sale is the single entry point for all providers. Coding agents and official SDKs get byte-faithful native APIs; any OpenAI-compatible client reaches every catalog model through one universal route. Same sk-pool key, same prepaid balance, everywhere.",
    baseUrl: "Base URL",
    nativeLaneTitle: "Native APIs",
    nativeLaneText: "Byte-faithful provider protocols — for coding agents and official SDKs that need full fidelity: thinking, tool use, prompt caching, provider betas.",
    compatibleLaneTitle: "OpenAI-compatible",
    compatibleLaneText: "One universal route for every catalog model — Claude, GPT, and Gemini — from any OpenAI-compatible client or SDK. Unsupported parameters fail closed with a clear 400.",
    anyModel: "Any catalog model",
    catalogLaneTitle: "Unified catalog",
    catalogLaneText: "Every enabled model under namespaced IDs — anthropic/*, openai/*, google/*, kimi/*. Bare native IDs keep working while they are unambiguous.",
    legacyNote: "Already integrated? The per-provider endpoints `api.apitoken.sale`, `openai.api.apitoken.sale/v1` and `gemini.api.apitoken.sale` remain fully supported with the same key and balance — the unified router is the recommended entry for new integrations.",
    models: "Models & pricing",
    modelsTitle: "Models & pricing",
    modelsText: "All providers, every available model, and exact per-1M-token rates — official list price vs. what you actually pay at the flat 50% discount.",
    geminiBatch: "Gemini Batch",
    geminiBatchTitle: "Gemini Batch API",
    geminiBatchText: "Run many independent Gemini requests asynchronously, poll one durable operation, and retrieve every generated response or per-item error. The same native API works through the unified router and direct Gemini endpoint.",
    caching: "Prompt caching",
    cachingTitle: "Prompt caching",
    cachingText: "Cache the large stable prefix once — every later read of it bills at 10% of the input price.",
    cacheClaude: "Claude — explicit cache_control",
    cacheClaudeText: "Breakpoints pass through to Anthropic unchanged. Cache writes bill at 1.25× input (5-minute TTL) or 2× (1-hour TTL — send the `anthropic-beta: extended-cache-ttl-2025-04-11` header yourself). Repeat calls of the same prefix are pinned to the warm upstream, so keep the prefix byte-identical.",
    cacheGpt: "GPT — automatic",
    cacheGptText: "No opt-in: repeated prefixes are cached server-side. `usage.prompt_tokens_details.cached_tokens` (Chat Completions) or `input_tokens_details.cached_tokens` (Responses) bills at 10% of input automatically.",
    geminiFiles: "Gemini files",
    geminiFilesTitle: "Sending files to Gemini",
    geminiFilesText: "Synchronous generateContent requires inline bytes: files/… from any Google project is invisible to this gateway. For large Gemini Batch input, upload an account-scoped JSONL file to this gateway and pass its returned name as inputConfig.fileName. Uploaded files expire after 48 hours.",
    gfKind: "Input",
    gfStatus: "Supported",
    gfHow: "How to send it",
    gfImages: "Images (PNG, JPEG, WebP)",
    gfImagesHow: "`inline_data` with `mime_type` and base64. Up to ~23 MB of source file.",
    gfPdf: "PDF documents",
    gfPdfHow: "`inline_data` with `application/pdf`. The model reads the text inside.",
    gfText: "Text files",
    gfTextHow: "`inline_data` with `text/plain`, or simply paste into the `text` part.",
    gfAudio: "Audio",
    gfAudioHow: "Only `gemini-3-flash-preview`, as inline `audio/wav`.",
    gfFilesApi: "Files API (`file_uri` / `fileData`)",
    gfFilesApiHow: "Batch JSONL input only: upload to this gateway and pass the returned files/{id} as inputConfig.fileName. fileData embedding is not supported. For synchronous generateContent, inline the bytes.",
    gfCached: "`cachedContent`",
    gfCachedHow: "Not available. Send the content inline.",
    gfYes: "Yes",
    gfNo: "No",
    gfPartial: "One model",
    geminiInlineTitle: "Inline request",
    geminiInlineText: "Replace the upload plus reference with a single request. Base64 adds about a third to the size, so the request body stays under our 32 MB limit for a source file up to roughly 23 MB.",
    geminiReasonTitle: "When we refuse",
    geminiReasonText: "An input this gateway cannot accept is rejected before your request reaches a subscription, as `400 INVALID_ARGUMENT` with a stable reason in `error.details` — `FILE_URI_UNSUPPORTED`, `CACHED_CONTENT_UNSUPPORTED`, `AUDIO_INPUT_UNSUPPORTED`. `FILE_URI_UNSUPPORTED` applies to synchronous generateContent and foreign Google files; Batch may resolve only files uploaded to this gateway under the same account. Those refusals are final: changing the request is the fix, retrying is not. Every error also carries an `x-request-id` header — quote it to support and we can find your exact request.",
    errorTitle: "Common response codes",
    errorText: "On the unified endpoint every protocol keeps its own error envelope: Anthropic lanes return Anthropic's JSON, OpenAI lanes return {\"error\":{\"message\",\"type\",\"param\",\"code\"}}, and the Gemini lane returns {\"error\":{\"code\",\"message\",\"status\"}}. Treat 401 and 402 as account-state failures; retry only transient 429 and 5xx responses.",
    status: "Status",
    meaning: "Meaning",
    action: "What to do",
    e401: "API key is missing, invalid, or revoked",
    a401: "Send an active sk-pool key in x-api-key. If it was revoked, create a replacement; do not retry the same key. On the native Gemini lane the same failure arrives as `400 INVALID_ARGUMENT` with `error.details` reason `API_KEY_INVALID` — mirroring the official Google API; treat it exactly like a 401.",
    e402: "Available prepaid balance is too low",
    a402: "Top up the account, confirm the balance is available, then retry. Backoff alone will not resolve a 402.",
    e429: "Rate limit or temporary upstream capacity limit",
    a429: "Honor Retry-After when present; retry with capped exponential backoff and jitter.",
    e5xx: "Temporary gateway or provider upstream failure",
    a5xx: "Retry with bounded exponential backoff. Keep the request ID and avoid unbounded duplicate attempts.",
    copy: "Copy",
    copied: "Copied",
    copyPage: "Copy page",
    nextSteps: "Next steps",
    nextStepsText: "Everything referenced from this page, in one place.",
    nextModels: "Models",
    nextModelsText: "Exact model IDs, context windows, and token prices.",
    nextPricing: "Pricing",
    nextPricingText: "Top-ups, the flat 50% discount, and how billing works.",
    nextGuides: "Guides",
    nextGuidesText: "Step-by-step integration walkthroughs.",
    footer: "apiToken.sale documentation · one router endpoint — native Claude, GPT & Gemini APIs and OpenAI-compatible access",
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
    usageBalance: "Баланс и расход",
    errors: "Ошибки",
    title: "Подключение моделей",
    lead: "Один API‑ключ, один endpoint, все доступные модели — нативные протоколы Anthropic, OpenAI и Gemini плюс OpenAI‑совместимый API для любого клиента. AI‑агент сам настроит и проверит подключение.",
    agentPrompt: `Прочитай ${AGENT_GUIDE_URL} и следуй инструкциям, чтобы подключить этот проект к apiToken.sale.`,
    agentLabel: "Для вашего AI‑агента",
    copyAgent: "Скопировать",
    agentCopied: "Инструкция скопирована",
    openAgentGuide: "Открыть инструкцию",
    supportTitle: "Помощь с подключением",
    supportLead: "IDE, SDK, endpoint, модели и ошибки запросов.",
    openSupport: "Telegram",
    supportOnline: "AI 24/7 · человек при необходимости",
    quickstartText: "Подключите apiToken.sale к coding agent, который читает, изменяет и запускает ваш проект. Выберите стек — точная инструкция появится ниже.",
    apiTitle: "Один endpoint, все протоколы",
    apiText: "router.apitoken.sale — единая точка входа для всех провайдеров. Coding‑агенты и официальные SDK получают нативные API байт‑в‑байт; любой OpenAI‑совместимый клиент работает со всеми моделями каталога через один универсальный маршрут. Тот же ключ sk‑pool и тот же предоплаченный баланс.",
    baseUrl: "Base URL",
    nativeLaneTitle: "Нативные API",
    nativeLaneText: "Протоколы провайдеров байт‑в‑байт — для coding‑агентов и официальных SDK, которым нужна полная точность: thinking, tool use, кеширование промптов, beta‑возможности.",
    compatibleLaneTitle: "OpenAI-совместимый",
    compatibleLaneText: "Один универсальный маршрут для всех моделей каталога — Claude, GPT и Gemini — из любого OpenAI‑совместимого клиента или SDK. Неподдерживаемые параметры fail-closed с понятным 400.",
    anyModel: "Любая модель каталога",
    catalogLaneTitle: "Единый каталог",
    catalogLaneText: "Все доступные модели с namespaced ID — anthropic/*, openai/*, google/*, kimi/*. Обычные нативные ID продолжают работать, пока они однозначны.",
    legacyNote: "Уже подключены? Per-provider endpoint'ы `api.apitoken.sale`, `openai.api.apitoken.sale/v1` и `gemini.api.apitoken.sale` полностью поддерживаются с тем же ключом и балансом — единый router рекомендован для новых интеграций.",
    models: "Модели и цены",
    modelsTitle: "Модели и цены",
    modelsText: "Все провайдеры, все доступные модели и точные ставки за 1M токенов — официальная цена против той, что платите вы с единой скидкой 50%.",
    geminiBatch: "Gemini Batch",
    geminiBatchTitle: "Gemini Batch API",
    geminiBatchText: "Запускайте множество независимых запросов Gemini асинхронно, опрашивайте одну durable operation и получайте каждый ответ модели или ошибку отдельного item. Один нативный API работает через unified router и прямой Gemini endpoint.",
    caching: "Кеширование промптов",
    cachingTitle: "Кеширование промптов",
    cachingText: "Закешируйте большой стабильный префикс один раз — каждое следующее чтение стоит 10% от цены входных токенов.",
    cacheClaude: "Claude — явный cache_control",
    cacheClaudeText: "Точки кеширования пробрасываются в Anthropic без изменений. Запись в кеш стоит 1,25× от цены ввода (TTL 5 минут) или 2× (TTL 1 час — заголовок `anthropic-beta: extended-cache-ttl-2025-04-11` передайте самостоятельно). Повторные вызовы с тем же префиксом закрепляются за прогретым апстримом, поэтому держите префикс неизменным байт в байт.",
    cacheGpt: "GPT — автоматически",
    cacheGptText: "Ничего включать не нужно: повторяющиеся префиксы кешируются на стороне сервера. `usage.prompt_tokens_details.cached_tokens` (Chat Completions) или `input_tokens_details.cached_tokens` (Responses) автоматически тарифицируются как 10% от цены ввода.",
    geminiFiles: "Файлы в Gemini",
    geminiFilesTitle: "Как отправлять файлы в Gemini",
    geminiFilesText: "Синхронный generateContent требует inline-байты: files/… из любого Google-проекта невидим этому шлюзу. Для большого Gemini Batch input загрузите account-scoped JSONL в этот шлюз и передайте возвращённое имя как inputConfig.fileName. Загруженные файлы живут 48 часов.",
    gfKind: "Тип данных",
    gfStatus: "Поддержка",
    gfHow: "Как отправлять",
    gfImages: "Изображения (PNG, JPEG, WebP)",
    gfImagesHow: "`inline_data` с `mime_type` и base64. Примерно до 23 МБ исходного файла.",
    gfPdf: "PDF-документы",
    gfPdfHow: "`inline_data` с `application/pdf`. Модель читает текст внутри документа.",
    gfText: "Текстовые файлы",
    gfTextHow: "`inline_data` с `text/plain` — или просто вставьте текст в `text`.",
    gfAudio: "Аудио",
    gfAudioHow: "Только `gemini-3-flash-preview`, inline `audio/wav`.",
    gfFilesApi: "Files API (`file_uri` / `fileData`)",
    gfFilesApiHow: "Только JSONL input для Batch: загрузите файл в этот шлюз и передайте files/{id} как inputConfig.fileName. Встраивание через fileData не поддерживается. В синхронном generateContent передавайте байты inline.",
    gfCached: "`cachedContent`",
    gfCachedHow: "Недоступно. Передавайте содержимое inline.",
    gfYes: "Да",
    gfNo: "Нет",
    gfPartial: "Одна модель",
    geminiInlineTitle: "Запрос с файлом",
    geminiInlineText: "Загрузка и ссылка на файл заменяются одним запросом. Base64 добавляет примерно треть объёма, поэтому в наш лимит тела 32 МБ укладывается исходный файл примерно до 23 МБ.",
    geminiReasonTitle: "Когда мы отказываем",
    geminiReasonText: "Данные, которые шлюз принять не может, отклоняются ещё до обращения к подписке — `400 INVALID_ARGUMENT` со стабильной причиной в `error.details`: `FILE_URI_UNSUPPORTED`, `CACHED_CONTENT_UNSUPPORTED`, `AUDIO_INPUT_UNSUPPORTED`. `FILE_URI_UNSUPPORTED` относится к синхронному generateContent и чужим Google-файлам; Batch умеет разрешать только файлы, загруженные в этот шлюз тем же аккаунтом. Такой отказ окончательный: помогает изменение запроса, а не повтор. В каждой ошибке есть заголовок `x-request-id` — назовите его поддержке, и мы найдём именно ваш запрос.",
    errorTitle: "Основные коды ответа",
    errorText: "На едином endpoint каждый протокол сохраняет свой формат ошибок: маршруты Anthropic возвращают JSON Anthropic, маршруты OpenAI — {\"error\":{\"message\",\"type\",\"param\",\"code\"}}, маршрут Gemini — {\"error\":{\"code\",\"message\",\"status\"}}. Коды 401 и 402 требуют исправить состояние аккаунта; автоматически повторяйте только временные ошибки 429 и 5xx.",
    status: "Статус",
    meaning: "Значение",
    action: "Что делать",
    e401: "API-ключ отсутствует, неверен или отозван",
    a401: "Передайте активный ключ sk-pool в x-api-key. Если ключ отозван, создайте новый; повторять запрос с тем же ключом не нужно. На нативной линии Gemini та же ошибка приходит как `400 INVALID_ARGUMENT` с причиной `API_KEY_INVALID` в `error.details` — как в официальном Google API; обрабатывайте её так же, как 401.",
    e402: "Доступного предоплаченного баланса недостаточно",
    a402: "Пополните аккаунт, убедитесь, что баланс зачислен, и повторите запрос. Ожидание само по себе не устранит 402.",
    e429: "Лимит запросов или временный дефицит мощности провайдера",
    a429: "Учитывайте Retry-After, если он есть; используйте ограниченную экспоненциальную задержку со случайным смещением.",
    e5xx: "Временная ошибка шлюза или инфраструктуры провайдера",
    a5xx: "Повторите запрос с ограниченной экспоненциальной задержкой. Сохраните ID запроса и не допускайте бесконечных повторов.",
    copy: "Копировать",
    copied: "Скопировано",
    copyPage: "Копировать страницу",
    nextSteps: "Дальше",
    nextStepsText: "Всё, на что ссылается эта страница, в одном месте.",
    nextModels: "Модели",
    nextModelsText: "Точные ID моделей, контекстные окна и цены за токены.",
    nextPricing: "Цены",
    nextPricingText: "Пополнения, единая скидка 50% и принцип расчёта.",
    nextGuides: "Гайды",
    nextGuidesText: "Пошаговые инструкции по интеграции.",
    footer: "Документация apiToken.sale · один router endpoint — нативные Claude, GPT и Gemini API и OpenAI‑совместимый доступ",
  },
} as const;

export function DocsPortal({ openaiCatalog }: { openaiCatalog?: OpenAiModel[] }) {
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
    { id: "usage", label: t.usageBalance },
    { id: "models", label: t.models },
    { id: "gemini-batch", label: t.geminiBatch },
    { id: "gemini-files", label: t.geminiFiles },
    { id: "errors", label: t.errors },
    { id: "caching", label: t.caching },
    { id: "next", label: t.nextSteps },
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
          <h2 className="docs-agent-label">{t.agentLabel}</h2>
          <article className="docs-agent-card ym-hide-content">
            <InfoCircleIcon />
            <div className="docs-agent-chip">
              <p className="docs-agent-prompt" title={t.agentPrompt}>{t.agentPrompt}</p>
              <AgentCopyButton prompt={t.agentPrompt} label={t.copyAgent} copiedLabel={t.agentCopied} />
              <a className="docs-agent-chip-btn" href={AGENT_GUIDE_URL} target="_blank" rel="noreferrer" aria-label={t.openAgentGuide} title={t.openAgentGuide}><GuideIcon /></a>
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
          <EndpointsOverview t={t} />
          <ApiReference language={language} />
        </section>

        <section className="docs-section" id="models">
          <div className="docs-section-heading"><span>04</span><div><h2>{t.modelsTitle}</h2><p>{t.modelsText}</p></div></div>
          <ModelsPricing language={language} openaiCatalog={openaiCatalog} />
        </section>

        <section className="docs-section" id="gemini-batch">
          <div className="docs-section-heading"><span>05</span><div><h2>{t.geminiBatchTitle}</h2><p>{t.geminiBatchText}</p></div></div>
          <GeminiBatchGuide language={language} />
        </section>

        <section className="docs-section" id="gemini-files">
          <div className="docs-section-heading"><span>06</span><div><h2>{t.geminiFilesTitle}</h2><p>{t.geminiFilesText}</p></div></div>
          <div className="table-scroll"><table className="mtable"><thead><tr><th>{t.gfKind}</th><th>{t.gfStatus}</th><th>{t.gfHow}</th></tr></thead><tbody>
            <FileRow kind={t.gfImages} status={t.gfYes} how={t.gfImagesHow} labels={t} />
            <FileRow kind={t.gfPdf} status={t.gfYes} how={t.gfPdfHow} labels={t} />
            <FileRow kind={t.gfText} status={t.gfYes} how={t.gfTextHow} labels={t} />
            <FileRow kind={t.gfAudio} status={t.gfPartial} how={t.gfAudioHow} labels={t} />
            <FileRow kind={t.gfFilesApi} status={t.gfPartial} how={t.gfFilesApiHow} labels={t} />
            <FileRow kind={t.gfCached} status={t.gfNo} how={t.gfCachedHow} labels={t} />
          </tbody></table></div>
          <div className="docs-cache-stack">
            <CacheCard title={t.geminiInlineTitle} text={t.geminiInlineText} code={GEMINI_INLINE_JSON} codeLabel="JSON · Request" copyLabel={t.copy} copiedLabel={t.copied} />
          </div>
          <p className="docs-note"><strong>{t.geminiReasonTitle}.</strong> <Prose text={t.geminiReasonText} /></p>
        </section>

        <section className="docs-section" id="errors">
          <div className="docs-section-heading"><span>07</span><div><h2>{t.errorTitle}</h2><p>{t.errorText}</p></div></div>
          <div className="table-scroll"><table className="mtable docs-errors"><thead><tr><th>{t.status}</th><th>{t.meaning}</th><th>{t.action}</th></tr></thead><tbody><ErrorRow code="401" meaning={t.e401} action={t.a401} labels={t} /><ErrorRow code="402" meaning={t.e402} action={t.a402} labels={t} /><ErrorRow code="429" meaning={t.e429} action={t.a429} labels={t} /><ErrorRow code="5xx" meaning={t.e5xx} action={t.a5xx} labels={t} /></tbody></table></div>
        </section>

        <section className="docs-section" id="caching">
          <div className="docs-section-heading"><span>08</span><div><h2>{t.cachingTitle}</h2><p>{t.cachingText}</p></div></div>
          <div className="docs-cache-stack">
            <CacheCard title={t.cacheClaude} text={t.cacheClaudeText} code={CLAUDE_CACHE_JSON} codeLabel="JSON · Request" copyLabel={t.copy} copiedLabel={t.copied} />
            <CacheCard title={t.cacheGpt} text={t.cacheGptText} code={GPT_CACHE_JSON} codeLabel="JSON · Response" copyLabel={t.copy} copiedLabel={t.copied} />
          </div>
        </section>

        <section className="docs-section docs-next" id="next">
          <div className="docs-section-heading"><span>09</span><div><h2>{t.nextSteps}</h2><p>{t.nextStepsText}</p></div></div>
          <div className="learn-related">
            <Link className="learn-related-card" href={localeHref("/models", language)}><strong>{t.nextModels}</strong><span>{t.nextModelsText}</span></Link>
            <Link className="learn-related-card" href={localeHref("/plans", language)}><strong>{t.nextPricing}</strong><span>{t.nextPricingText}</span></Link>
            <Link className="learn-related-card" href={localeHref("/docs/learn", language)}><strong>{t.nextGuides}</strong><span>{t.nextGuidesText}</span></Link>
          </div>
        </section>

        <footer className="docs-footer">{t.footer}</footer>
      </main>
    </div>
  </div>;
}

type DocsCopy = { [K in keyof (typeof copy)["en"]]: string };

// The unified router at a glance: one base URL, then the three lane families —
// native provider protocols, the OpenAI-compatible universal route, and the
// aggregated catalog. Legacy per-provider hosts are acknowledged below.
function EndpointsOverview({ t }: { t: DocsCopy }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    await navigator.clipboard.writeText(ROUTER_BASE_URL);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1_200);
  }

  return <div className="docs-endpoints ym-hide-content">
    <div className="docs-endpoints-base">
      <span>{t.baseUrl}</span>
      <code>{ROUTER_BASE_URL}</code>
      <button type="button" onClick={handleCopy}><CopyIcon copied={copied} />{copied ? t.copied : t.copy}</button>
    </div>
    <div className="docs-endpoints-grid">
      <article className="docs-endpoint-card">
        <h3>{t.nativeLaneTitle}</h3>
        <p>{t.nativeLaneText}</p>
        <ul>
          <li><b>POST</b><code>/v1/messages</code><span>Anthropic</span></li>
          <li><b>POST</b><code>/v1/responses</code><span>OpenAI</span></li>
          <li><b>POST</b><code>{"/v1beta/models/{model}:generateContent"}</code><span>Gemini</span></li>
        </ul>
      </article>
      <article className="docs-endpoint-card">
        <h3>{t.compatibleLaneTitle}</h3>
        <p>{t.compatibleLaneText}</p>
        <ul>
          <li><b>POST</b><code>/v1/chat/completions</code><span>{t.anyModel}</span></li>
        </ul>
      </article>
      <article className="docs-endpoint-card">
        <h3>{t.catalogLaneTitle}</h3>
        <p>{t.catalogLaneText}</p>
        <ul>
          <li><b>GET</b><code>/v1/models</code><span>{t.anyModel}</span></li>
        </ul>
      </article>
    </div>
    <p className="docs-endpoints-legacy"><Prose text={t.legacyNote} /></p>
  </div>;
}

function BrandMark() {  return <><Image className="brand-mark bm-light" src="/assets/logo-mark-light.png" width={24} height={24} alt="" /><Image className="brand-mark bm-dark" src="/assets/logo-mark-dark.png" width={24} height={24} alt="" /></>;
}

function AgentCopyButton({ prompt, label, copiedLabel }: { prompt: string; label: string; copiedLabel: string }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    await navigator.clipboard.writeText(prompt);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1_200);
  }

  return <button className="docs-agent-chip-btn" type="button" aria-label={copied ? copiedLabel : label} title={copied ? copiedLabel : label} onClick={handleCopy}><CopyIcon copied={copied} /></button>;
}

function ErrorRow({ code, meaning, action, labels }: { code: string; meaning: string; action: string; labels: { status: string; meaning: string; action: string } }) {
  return <tr><td data-label={labels.status}><code>{code}</code></td><td data-label={labels.meaning}><span>{meaning}</span></td><td data-label={labels.action}><span><Prose text={action} /></span></td></tr>;
}

function FileRow({ kind, status, how, labels }: { kind: string; status: string; how: string; labels: { gfKind: string; gfStatus: string; gfHow: string } }) {
  return <tr><td data-label={labels.gfKind}><span><Prose text={kind} /></span></td><td data-label={labels.gfStatus}><span>{status}</span></td><td data-label={labels.gfHow}><span><Prose text={how} /></span></td></tr>;
}

function CacheCard({ title, text, code, codeLabel, copyLabel, copiedLabel }: { title: string; text: string; code: string; codeLabel: string; copyLabel: string; copiedLabel: string }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    await navigator.clipboard.writeText(code);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1_200);
  }

  return <article className="cache-card ym-hide-content">
    <header><h3>{title}</h3><p><Prose text={text} /></p></header>
    <div className="ib-code">
      <div className="ib-code-bar"><i className="ib-dots" aria-hidden="true" /><span>{codeLabel}</span><button type="button" onClick={handleCopy}><CopyIcon copied={copied} />{copied ? copiedLabel : copyLabel}</button></div>
      <pre><code><HighlightedCode code={code} /></code></pre>
    </div>
  </article>;
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

function InfoCircleIcon() {
  return <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9.2" /><path d="M12 11v5" /><circle cx="12" cy="7.8" r=".4" fill="currentColor" /></svg>;
}

function TelegramIcon() {
  return <svg viewBox="0 0 24 24" width="21" height="21" fill="currentColor" aria-hidden="true"><path d="M21.7 2.3 2.9 10.5c-1.3.5-1.3 1.3-.2 1.7l4.8 1.5 1.8 5.4c.2.7.1 1 .9 1 .6 0 .9-.3 1.2-.6l2.3-2.2 4.8 3.5c.9.5 1.5.2 1.8-.8l3.1-14.9c.4-1.3-.5-1.9-1.7-1.4ZM9.4 13.4l9.4-5.9c.5-.3.9-.1.5.2l-7.7 7-.3 3.1-1.9-4.4Z" /></svg>;
}
