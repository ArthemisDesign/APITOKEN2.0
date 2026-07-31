"use client";

import Image from "next/image";
import Link from "next/link";
import { useEffect, useState } from "react";
import { useI18n } from "@/components/i18n-provider";
import { ThemeToggle } from "@/components/site-chrome";
import { api } from "@/lib/api";
import { localeHref } from "@/lib/locale-routes";
import { B2C_PRICING_MILESTONES, formatWholeUsd } from "@/lib/pricing-tiers";

const OPENAI_BASE_URL = "https://openai.api.apitoken.sale/v1";
const AGENT_GUIDE_URL = "https://apitoken.sale/md/connect";
const SUPPORT_TELEGRAM_URL = "https://t.me/apitokensupportbot";
const CURL = `curl https://api.apitoken.sale/v1/messages \
  -H "x-api-key: $APITOKEN_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "content-type: application/json" \
  -d '{
    "model": "claude-opus-4-8",
    "max_tokens": 1024,
    "messages": [{ "role": "user", "content": "Hello" }]
  }'`;
const ENVIRONMENT = `export ANTHROPIC_BASE_URL="https://api.apitoken.sale"
export ANTHROPIC_API_KEY="sk-pool-•••"
export APITOKEN_API_KEY="$ANTHROPIC_API_KEY"`;
const CLAUDE_CODE = `# Set for the current shell or add to your shell profile
export ANTHROPIC_BASE_URL="https://api.apitoken.sale"
export ANTHROPIC_API_KEY="sk-pool-•••"

# Start Claude Code normally
claude`;
const CURSOR = `Provider: Anthropic
Base URL: https://api.apitoken.sale
API key: sk-pool-•••
Model ID: claude-opus-4-8`;
const PYTHON = `from anthropic import Anthropic

# Reads ANTHROPIC_API_KEY from the environment
client = Anthropic(base_url="https://api.apitoken.sale")

message = client.messages.create(
    model="claude-opus-4-8",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello"}],
)

for block in message.content:
    if block.type == "text":
        print(block.text)`;
const TYPESCRIPT = `import Anthropic from "@anthropic-ai/sdk";

// Reads ANTHROPIC_API_KEY from the environment
const client = new Anthropic({
  baseURL: "https://api.apitoken.sale",
});

const message = await client.messages.create({
  model: "claude-opus-4-8",
  max_tokens: 1024,
  messages: [{ role: "user", content: "Hello" }],
});

for (const block of message.content) {
  if (block.type === "text") console.log(block.text);
}`;
const OPENAI_CURL = `curl https://openai.api.apitoken.sale/v1/responses \
  -H "Authorization: Bearer $APITOKEN_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-5.6-sol",
    "input": "Reply with exactly: connected"
  }'`;
const OPENAI_PYTHON = `import os
from openai import OpenAI

client = OpenAI(
    api_key=os.environ["APITOKEN_API_KEY"],
    base_url="https://openai.api.apitoken.sale/v1",
)

response = client.responses.create(
    model="gpt-5.6-sol",
    input="Reply with exactly: connected",
)
print(response.output_text)`;
const CODEX_PROFILE = `# ~/.codex/apitoken.config.toml
model = "gpt-5.6-sol"
model_provider = "apitoken"

[model_providers.apitoken]
name = "apiToken.sale"
base_url = "https://openai.api.apitoken.sale/v1"
wire_api = "responses"
env_key = "APITOKEN_API_KEY"`;
const CODEX_RUN = `# Keep the key in your shell, not in the TOML file
export APITOKEN_API_KEY="sk-pool-•••"
codex --profile apitoken`;

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
    authentication: "Authentication",
    tools: "Developer tools",
    openai: "OpenAI-compatible API & Codex",
    sdks: "Official SDKs",
    errors: "Errors",
    pricing: "Pricing & tiers",
    title: "Connect any model",
    lead: "One API key gives your tools access to every model available in your account. Let an AI agent select the right protocol, configure the client, and verify the connection.",
    openKeys: "Get an API key",
    agentTitle: "Give this setup to your AI agent",
    agentLead: "Copy one instruction into Codex, Claude Code, Cursor, or any agent that can configure your project. It will inspect the environment and complete the setup itself.",
    agentInstructionLabel: "Ready-to-use instruction",
    agentPrompt: `Connect this project or tool to apiToken.sale. First open ${AGENT_GUIDE_URL} and follow the current instructions. Detect my operating system, shell, runtime, and client or SDK; choose a compatible API protocol and an available model; ask for the sk-pool-… key only if it is not already stored in a secure environment variable; make the smallest required changes; and run a real verification request. Never print the key, put it in source control, or change unrelated files. If anything fails, diagnose it with the guide and explain the result in plain language.`,
    copyAgent: "Copy agent instruction",
    agentCopied: "Instruction copied",
    openAgentGuide: "Open technical guide",
    agentGuideNote: "Live guide · endpoints and model catalog stay current",
    agentSteps: [
      { title: "Understands your setup", text: "Detects Windows, macOS, or Linux, plus the shell, runtime, and existing client." },
      { title: "Chooses the right API", text: "Selects a compatible protocol and a currently available model instead of guessing." },
      { title: "Connects safely", text: "Keeps the key outside source code and changes only the settings the integration needs." },
      { title: "Proves it works", text: "Runs a minimal real request, checks the response, and explains any failure." },
    ],
    supportTitle: "Stuck? We’ll finish the setup with you.",
    supportLead: "Our AI support knows the supported clients, OS-specific setup, endpoints, headers, and current model catalog. It can guide you step by step, inspect an error, or hand the case to a person.",
    supportItems: [
      "Any compatible IDE, agent, CLI, or SDK",
      "Windows, macOS, Linux, shells, and environment variables",
      "Authentication, endpoint, model, balance, and rate-limit errors",
      "Streaming, tools, images, proxies, and client-specific settings",
    ],
    openSupport: "Open Telegram support",
    supportOnline: "AI replies 24/7 · a person joins when needed",
    diagnosticTitle: "Send the right context once",
    diagnosticText: "Copy this brief, fill in what you know, and paste it into the support chat. Never include your full API key.",
    diagnosticBrief: `Please help me connect apiToken.sale.

Operating system and version:
Client / IDE / CLI / SDK and version:
Runtime and shell:
What I am trying to connect:
API surface or endpoint used:
Model ID:
HTTP status and exact error text:
Approximate time and timezone:
Request ID, if present:
What I already tried:
API key: REDACTED (last 4 characters only):`,
    copyDiagnostic: "Copy diagnostic brief",
    diagnosticCopied: "Diagnostic brief copied",
    supportSecurity: "Support will never ask for your password, card details, or full API key.",
    baseUrl: "Base URL",
    authHeader: "Authentication header",
    quickstartText: "Export the connection settings, then run the minimal cURL request. A successful response confirms the key, balance, endpoint, and model are ready.",
    requestTitle: "Send your first request",
    requestText: "This sends one user message to claude-opus-4-8. To stream a response, add \"stream\": true; the endpoint returns standard Anthropic SSE events.",
    envTitle: "Set environment variables",
    envText: "Claude Code and the official SDKs read the ANTHROPIC variables. APITOKEN_API_KEY is a local alias used by the cURL example below.",
    toolsText: "Use each tool's Anthropic provider settings. Keep the model ID and API format unchanged; replace only the base URL and API key.",
    claudeCode: "Claude Code",
    claudeCodeText: "Set the endpoint and key in the shell, then run Claude Code normally. No project configuration is required.",
    editors: "Cursor, Cline, Continue, and Zed",
    editorsText: "Select Anthropic as the provider and enter these four values. Field names vary slightly by tool.",
    openaiText: "Use the same sk-pool key and prepaid balance on the dedicated OpenAI-compatible text endpoint. Authenticate with Authorization: Bearer and discover the currently available models with GET /v1/models.",
    openaiNotice: "This is an independent OpenAI-compatible service, not the OpenAI Platform and not an OpenAI-operated endpoint. It supports model discovery, Responses, and Chat Completions with SSE streaming and text+image input. Audio, files, realtime, assistants, batches, fine-tuning, and other OpenAI Platform services are not available.",
    openaiRequest: "Responses API",
    openaiRequestText: "Send a standard Responses request to the dedicated host. The GPT-5.6 line (sol, terra, luna), gpt-5.5 and gpt-5.4 are currently available; use GET /v1/models instead of assuming a model ID.",
    openaiPython: "OpenAI Python SDK",
    openaiPythonText: "Set base_url on the official SDK and pass the same sk-pool key. Keep the key in a server-side environment variable or secret manager in production.",
    codex: "Codex profile",
    codexText: "Save this as ~/.codex/apitoken.config.toml. The named profile leaves your normal Codex login and default configuration untouched.",
    codexRun: "Run Codex",
    codexRunText: "Export your sk-pool key in the shell, then select the named profile explicitly.",
    sdksText: "Use the official Anthropic SDKs with a custom base URL. Request parameters, response objects, and streaming behavior remain the same.",
    python: "Python SDK",
    pythonText: "The client reads the sk-pool key from ANTHROPIC_API_KEY; base_url points requests to apiToken.sale.",
    typescript: "TypeScript SDK",
    typescriptText: "The Node.js client reads the same environment key; baseURL changes only the API host.",
    authText: "For direct HTTP on the Anthropic surface, send the sk-pool key in x-api-key and include anthropic-version: 2023-06-01 — official Anthropic SDKs add the version header automatically. On the OpenAI-compatible surface (section 05), send the same key as Authorization: Bearer instead.",
    authNotice: "The right header depends on the surface: x-api-key for api.apitoken.sale, Authorization: Bearer for openai.api.apitoken.sale/v1. The same sk-pool key works on both.",
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
    production: "Production checklist",
    checklist: [
      "Load keys from a server-side secret manager or environment variable; never ship them in browser bundles",
      "Set connection and response timeouts; use streaming for long generations",
      "Retry only 429 and 5xx; honor Retry-After, add jitter, and cap attempts",
      "Treat other 4xx responses as non-retryable until the request or account state changes",
      "Log status, model, latency, and request ID; redact keys and sensitive prompts",
      "Alert on low balance and reconcile charges against the dashboard request ledger",
    ],
    pricingTitle: "Pricing and discount tiers",
    pricingLead: "Top up any whole-dollar amount into one prepaid balance. Each request receives your active discount, from Starter 60% through Scale 70%.",
    billingHead: "How billing works",
    billingText: "We start with the model's official provider token price (Anthropic or OpenAI), apply your tier discount, and deduct the remainder from your apiToken.sale balance. There are no fixed token packs or recurring subscriptions.",
    billingExample: "Example — Starter gives a 60% discount, so you pay 40%: $100 of usage at official list prices produces a $40 balance charge. In other words, $40 of balance covers about $100 of official usage (×2.5).",
    tiersHead: "Discount tiers",
    tiersText: "Starter is active by default. Cumulative top-ups unlock Builder at $100, Pro at $250, Studio at $500, and Scale at $1,000.",
    tierColTier: "Tier",
    tierColDiscount: "Discount",
    tierColValue: "$1 balance covers",
    tierColGet: "Cumulative top-ups",
    tierColKeep: "Spend / rolling 30d",
    tierBaseCell: "Default · $0",
    keepHead: "Keeping a tier",
    keepText: "To keep a paid tier, balance charges in every rolling 30-day window must total at least 50% of that tier's cumulative top-up threshold. If they do not, the account moves down one tier and cumulative progress resets to the new tier's threshold. While the tier is maintained, new top-ups continue toward the next threshold.",
    keepFree: "Starter 60% is the permanent base tier. It never expires and has no minimum top-up or spend requirement.",
    exampleHead: "Example from top-up to renewal",
    exampleText: "Reach $250 in cumulative top-ups to unlock Pro at 65%. Keep Pro by spending at least $125 from the balance in every rolling 30-day window. If spend falls below $125, the account moves to Builder at 62.5%. While Pro remains active, another $250 in top-ups brings the cumulative total to $500 and unlocks Studio at 67.5%.",
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
    authentication: "Аутентификация",
    tools: "Инструменты разработчика",
    openai: "OpenAI-совместимый API и Codex",
    sdks: "Официальные SDK",
    errors: "Ошибки",
    pricing: "Цены и тарифы",
    title: "Подключение моделей",
    lead: "Один API‑ключ открывает вашим инструментам все доступные модели. Поручите AI‑агенту выбрать нужный протокол, настроить клиент и проверить подключение.",
    openKeys: "Получить API‑ключ",
    agentTitle: "Передайте подключение своему AI‑агенту",
    agentLead: "Скопируйте одну инструкцию в Codex, Claude Code, Cursor или любой агент, который умеет настраивать проект. Остальное он сделает сам.",
    agentInstructionLabel: "Готовая инструкция",
    agentPrompt: `Подключи этот проект или инструмент к apiToken.sale. Сначала открой ${AGENT_GUIDE_URL} и следуй актуальной инструкции. Определи мою операционную систему, оболочку, среду и клиент или SDK; выбери совместимый API‑протокол и доступную модель; запроси ключ sk-pool-…, только если его нет в безопасной переменной окружения; внеси минимальные изменения и выполни реальный проверочный запрос. Не показывай ключ в логах, не коммить его и не меняй посторонние файлы. Если что‑то не сработает, диагностируй причину по guide и объясни результат простыми словами.`,
    copyAgent: "Скопировать инструкцию",
    agentCopied: "Инструкция скопирована",
    openAgentGuide: "Открыть технический guide",
    agentGuideNote: "Живая инструкция · endpoints и каталог моделей всегда актуальны",
    agentSteps: [
      { title: "Понимает вашу среду", text: "Определяет Windows, macOS или Linux, оболочку, runtime и текущий клиент." },
      { title: "Выбирает нужный API", text: "Находит совместимый протокол и актуальную модель, а не угадывает их." },
      { title: "Подключает безопасно", text: "Хранит ключ вне исходного кода и меняет только нужные настройки." },
      { title: "Доказывает, что всё работает", text: "Выполняет минимальный реальный запрос, проверяет ответ и объясняет ошибку." },
    ],
    supportTitle: "Не получилось? Доведём подключение до конца.",
    supportLead: "Наша AI‑поддержка знает совместимые клиенты, настройку для разных ОС, endpoints, headers и актуальные модели. Она проведёт по шагам, разберёт ошибку или подключит человека.",
    supportItems: [
      "Любая совместимая IDE, AI‑агент, CLI или SDK",
      "Windows, macOS, Linux, оболочки и переменные окружения",
      "Ошибки ключа, endpoint, модели, баланса и лимитов",
      "Streaming, tools, изображения, proxy и настройки клиента",
    ],
    openSupport: "Открыть поддержку в Telegram",
    supportOnline: "AI отвечает 24/7 · человек подключается при необходимости",
    diagnosticTitle: "Сразу передайте нужный контекст",
    diagnosticText: "Скопируйте шаблон, заполните то, что знаете, и отправьте в чат. Никогда не присылайте полный API‑ключ.",
    diagnosticBrief: `Помогите подключить apiToken.sale.

Операционная система и версия:
Клиент / IDE / CLI / SDK и версия:
Runtime и оболочка:
Что именно подключаю:
Используемый API или endpoint:
ID модели:
HTTP‑статус и точный текст ошибки:
Примерное время и часовой пояс:
Request ID, если есть:
Что уже пробовал:
API‑ключ: СКРЫТ (только последние 4 символа):`,
    copyDiagnostic: "Скопировать данные для диагностики",
    diagnosticCopied: "Шаблон скопирован",
    supportSecurity: "Поддержка не попросит пароль, данные карты или полный API‑ключ.",
    baseUrl: "Базовый URL",
    authHeader: "Заголовок аутентификации",
    quickstartText: "Сначала задайте параметры подключения, затем выполните минимальный cURL-запрос. Успешный ответ подтверждает, что ключ, баланс, адрес и модель готовы к работе.",
    requestTitle: "Отправьте первый запрос",
    requestText: "Запрос отправляет одно сообщение модели claude-opus-4-8. Для потокового ответа добавьте \"stream\": true; сервер вернёт стандартные SSE-события Anthropic.",
    envTitle: "Задайте переменные окружения",
    envText: "Claude Code и официальные SDK читают переменные ANTHROPIC. APITOKEN_API_KEY — локальный псевдоним, который используется в cURL-примере ниже.",
    toolsText: "Выберите в инструменте провайдера Anthropic. Не меняйте ID модели и формат API — замените только базовый URL и API-ключ.",
    claudeCode: "Claude Code",
    claudeCodeText: "Задайте адрес и ключ в оболочке, затем запускайте Claude Code как обычно. Настраивать проект не требуется.",
    editors: "Cursor, Cline, Continue и Zed",
    editorsText: "Выберите Anthropic как провайдера и укажите эти четыре значения. Названия полей могут немного отличаться.",
    openaiText: "Используйте тот же ключ sk-pool и единый предоплаченный баланс на отдельном OpenAI-совместимом текстовом API. Передавайте ключ в Authorization: Bearer, а актуальные модели получайте через GET /v1/models.",
    openaiNotice: "Это независимый OpenAI-совместимый сервис, а не OpenAI Platform и не endpoint под управлением OpenAI. Поддерживаются список моделей, Responses и Chat Completions с SSE-потоками и входом «текст + изображения». Аудио, файлы, realtime, assistants, batches, fine-tuning и другие сервисы OpenAI Platform недоступны.",
    openaiRequest: "Responses API",
    openaiRequestText: "Отправьте стандартный запрос Responses на отдельный хост. Сейчас доступны линейка GPT-5.6 (sol, terra, luna), gpt-5.5 и gpt-5.4; получайте список через GET /v1/models, а не полагайтесь на неизменность ID.",
    openaiPython: "OpenAI SDK для Python",
    openaiPythonText: "Укажите base_url в официальном SDK и передайте тот же ключ sk-pool. В production храните ключ в серверной переменной окружения или менеджере секретов.",
    codex: "Профиль Codex",
    codexText: "Сохраните этот файл как ~/.codex/apitoken.config.toml. Именованный профиль не меняет обычный вход и настройки Codex по умолчанию.",
    codexRun: "Запуск Codex",
    codexRunText: "Экспортируйте ключ sk-pool в оболочке, затем явно выберите именованный профиль.",
    sdksText: "Используйте официальные SDK Anthropic с собственным базовым URL. Параметры запросов, объекты ответов и потоковая передача не меняются.",
    python: "Python SDK",
    pythonText: "Клиент читает ключ sk-pool из ANTHROPIC_API_KEY, а base_url направляет запросы в apiToken.sale.",
    typescript: "TypeScript SDK",
    typescriptText: "Клиент Node.js читает ту же переменную с ключом; baseURL меняет только адрес API.",
    authText: "В прямых HTTP-запросах к Anthropic-поверхности передавайте ключ sk-pool в x-api-key и добавляйте anthropic-version: 2023-06-01 — официальные SDK Anthropic добавляют заголовок версии автоматически. На OpenAI-совместимой поверхности (раздел 05) тот же ключ передавайте как Authorization: Bearer.",
    authNotice: "Правильный заголовок зависит от поверхности: x-api-key для api.apitoken.sale, Authorization: Bearer для openai.api.apitoken.sale/v1. Один и тот же ключ sk-pool работает на обеих.",
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
    production: "Чек-лист для production",
    checklist: [
      "Загружайте ключи из серверного менеджера секретов или переменной окружения; не включайте их в браузерный bundle",
      "Задайте таймауты подключения и ответа; для долгой генерации используйте потоковый режим",
      "Повторяйте только 429 и 5xx; учитывайте Retry-After, добавляйте случайное смещение и ограничивайте число попыток",
      "Считайте остальные ответы 4xx неповторяемыми, пока не изменится запрос или состояние аккаунта",
      "Логируйте статус, модель, задержку и ID запроса; скрывайте ключи и конфиденциальные промпты",
      "Настройте уведомления о низком балансе и сверяйте списания с журналом запросов в кабинете",
    ],
    pricingTitle: "Цены и тарифные скидки",
    pricingLead: "Пополняйте единый предоплаченный баланс на любую целую сумму в долларах. К каждому запросу применяется активная скидка — от 60% на Starter до 70% на Scale.",
    billingHead: "Как рассчитывается стоимость",
    billingText: "Мы берём официальную стоимость токенов выбранной модели у провайдера (Anthropic или OpenAI), применяем скидку вашего тарифа и списываем остаток с баланса apiToken.sale. Фиксированных пакетов токенов и регулярной подписки нет.",
    billingExample: "Пример — Starter даёт скидку 60%, поэтому вы платите 40%: использование на $100 по официальным листовым ценам приведёт к списанию $40 с баланса. Иными словами, $40 баланса покрывают около $100 официального использования (×2,5).",
    tiersHead: "Тарифные скидки",
    tiersText: "Starter действует по умолчанию. Накопленные пополнения открывают Builder при $100, Pro при $250, Studio при $500 и Scale при $1 000.",
    tierColTier: "Тариф",
    tierColDiscount: "Скидка",
    tierColValue: "$1 баланса покрывает",
    tierColGet: "Накопленные пополнения",
    tierColKeep: "Расходы / 30 дней",
    tierBaseCell: "По умолчанию · $0",
    keepHead: "Как сохранить тариф",
    keepText: "Чтобы сохранить платный тариф, списания с баланса за каждые скользящие 30 дней должны составлять не менее 50% от порога накопленных пополнений этого тарифа. Если условие не выполнено, аккаунт опускается на один тариф, а накопленный прогресс сбрасывается до порога нового тарифа. Пока тариф сохраняется, новые пополнения продолжают приближать следующий порог.",
    keepFree: "Starter со скидкой 60% — постоянный базовый тариф. Он не истекает и не требует минимального пополнения или расхода.",
    exampleHead: "Пример от пополнения до продления",
    exampleText: "Накопите $250 пополнений, чтобы открыть Pro со скидкой 65%. Для сохранения Pro расходуйте с баланса не менее $125 за каждые скользящие 30 дней. Если расходы окажутся ниже $125, аккаунт перейдёт на Builder со скидкой 62,5%. Пока Pro остаётся активным, ещё $250 пополнений увеличат общую сумму до $500 и откроют Studio со скидкой 67,5%.",
    footer: "Документация apiToken.sale · Claude и OpenAI-совместимый текстовый API",
  },
} as const;

export function DocsPortal() {
  const { language, setLanguage } = useI18n();
  const t = copy[language];
  const [supportUrl, setSupportUrl] = useState(SUPPORT_TELEGRAM_URL);

  useEffect(() => {
    let alive = true;
    api.me()
      .then(({ user }) => {
        if (alive && user?.id) setSupportUrl(`${SUPPORT_TELEGRAM_URL}?start=${encodeURIComponent(user.id)}`);
      })
      .catch(() => {/* Public docs keep the non-personalized support URL. */});
    return () => { alive = false; };
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
      <aside className="docs-sidebar"><span>{t.onThisPage}</span><nav><a href="#overview">{t.overview}</a><a href="#agent-setup">{t.agentSetup}</a><a href="#setup-support">{t.support}</a><a href="#quickstart">{t.quickstart}</a><a href="#authentication">{t.authentication}</a><a href="#tools">{t.tools}</a><a href="#openai">{t.openai}</a><a href="#sdks">{t.sdks}</a><a href="#errors">{t.errors}</a><a href="#pricing">{t.pricing}</a></nav></aside>
      <main className="docs-main" id="main-content" tabIndex={-1}>
        <section className="docs-hero" id="overview"><h1>{t.title}</h1><p>{t.lead}</p></section>

        <section className="docs-section docs-connect-section" id="agent-setup">
          <article className="docs-agent-card">
            <div className="docs-agent-top">
              <div className="docs-agent-heading">
                <span className="docs-agent-number">01</span>
                <div><h2>{t.agentTitle}</h2><p>{t.agentLead}</p></div>
              </div>
              <div className="docs-agent-actions">
                <CopyControl className="docs-agent-copy" withIcon value={t.agentPrompt} label={t.copyAgent} copiedLabel={t.agentCopied} />
                <Link className="btn btn-ghost docs-guide-link" href="/md/connect" target="_blank"><GuideIcon />{t.openAgentGuide}</Link>
              </div>
            </div>
            <div className="docs-agent-command ym-hide-content">
              <span className="docs-agent-command-icon" aria-hidden="true"><AgentIcon /></span>
              <div><strong>{t.agentInstructionLabel}</strong><p>{t.agentPrompt}</p></div>
            </div>
            <div className="docs-agent-guide-note"><span aria-hidden="true" />{t.agentGuideNote}</div>
            <div className="docs-agent-plan">
              {t.agentSteps.map((step, index) => <article key={step.title}><span>{String(index + 1).padStart(2, "0")}</span><strong>{step.title}</strong><p>{step.text}</p></article>)}
            </div>
            <div className="docs-agent-keyline">
              <span><ShieldIcon />{language === "ru" ? "Ключ остаётся в вашей среде" : "The key stays in your environment"}</span>
              <Link href={localeHref("/dashboard?view=keys", language)}>{t.openKeys}<ArrowIcon /></Link>
            </div>
          </article>
        </section>

        <section className="docs-section docs-support-section" id="setup-support">
          <article className="docs-connect-support">
            <div className="docs-support-main">
              <span className="docs-support-icon" aria-hidden="true"><TelegramIcon /></span>
              <h2>{t.supportTitle}</h2>
              <p>{t.supportLead}</p>
              <ul>{t.supportItems.map((item) => <li key={item}><CheckIcon />{item}</li>)}</ul>
              <div className="docs-support-actions">
                <a className="btn btn-primary" href={supportUrl} target="_blank" rel="noreferrer"><TelegramIcon />{t.openSupport}</a>
                <span><i aria-hidden="true" />{t.supportOnline}</span>
              </div>
            </div>
            <aside className="docs-diagnostic">
              <span className="docs-diagnostic-icon" aria-hidden="true"><LifebuoyIcon /></span>
              <h3>{t.diagnosticTitle}</h3>
              <p>{t.diagnosticText}</p>
              <CopyControl className="docs-diagnostic-copy" withIcon value={t.diagnosticBrief} label={t.copyDiagnostic} copiedLabel={t.diagnosticCopied} />
              <div className="docs-support-security"><ShieldIcon />{t.supportSecurity}</div>
            </aside>
          </article>
        </section>

        <section className="docs-section" id="quickstart">
          <div className="docs-section-heading"><span>02</span><div><h2>{t.quickstart}</h2><p>{t.quickstartText}</p></div></div>
          <CodeBlock title={t.envTitle} description={t.envText} code={ENVIRONMENT} copyLabel={t.copy} copiedLabel={t.copied} />
          <CodeBlock title={t.requestTitle} description={t.requestText} code={CURL} copyLabel={t.copy} copiedLabel={t.copied} />
        </section>

        <section className="docs-section" id="authentication">
          <div className="docs-section-heading"><span>03</span><div><h2>{t.authentication}</h2><p>{t.authText}</p></div></div>
          <div className="docs-auth-flow"><div><b>1</b><code>x-api-key</code></div><span>＋</span><div><b>2</b><code>anthropic-version</code></div><span>→</span><div><b>API</b><code>/v1/messages</code></div></div>
          <div className="docs-notice">{t.authNotice}</div>
        </section>

        <section className="docs-section" id="tools">
          <div className="docs-section-heading"><span>04</span><div><h2>{t.tools}</h2><p>{t.toolsText}</p></div></div>
          <div className="docs-two-col"><CodeBlock title={t.claudeCode} description={t.claudeCodeText} code={CLAUDE_CODE} copyLabel={t.copy} copiedLabel={t.copied} /><CodeBlock title={t.editors} description={t.editorsText} code={CURSOR} copyLabel={t.copy} copiedLabel={t.copied} /></div>
        </section>

        <section className="docs-section" id="openai">
          <div className="docs-section-heading"><span>05</span><div><h2>{t.openai}</h2><p>{t.openaiText}</p></div></div>
          <div className="docs-essential-grid" style={{ marginBottom: 14 }}><Endpoint label={t.baseUrl} value={OPENAI_BASE_URL} copyLabel={t.copy} copiedLabel={t.copied} /><Endpoint label={t.authHeader} value="Authorization: Bearer sk-pool-•••" copyLabel={t.copy} copiedLabel={t.copied} /><Endpoint label="Models" value={`${OPENAI_BASE_URL}/models`} copyLabel={t.copy} copiedLabel={t.copied} /></div>
          <div className="docs-notice" style={{ marginBottom: 14 }}>{t.openaiNotice}</div>
          <div className="docs-two-col" style={{ marginBottom: 18 }}><CodeBlock title={t.openaiRequest} description={t.openaiRequestText} code={OPENAI_CURL} copyLabel={t.copy} copiedLabel={t.copied} /><CodeBlock title={t.openaiPython} description={t.openaiPythonText} code={OPENAI_PYTHON} copyLabel={t.copy} copiedLabel={t.copied} /></div>
          <div className="docs-two-col"><CodeBlock title={t.codex} description={t.codexText} code={CODEX_PROFILE} copyLabel={t.copy} copiedLabel={t.copied} /><CodeBlock title={t.codexRun} description={t.codexRunText} code={CODEX_RUN} copyLabel={t.copy} copiedLabel={t.copied} /></div>
        </section>

        <section className="docs-section" id="sdks">
          <div className="docs-section-heading"><span>06</span><div><h2>{t.sdks}</h2><p>{t.sdksText}</p></div></div>
          <div className="docs-two-col"><CodeBlock title={t.python} description={t.pythonText} code={PYTHON} copyLabel={t.copy} copiedLabel={t.copied} /><CodeBlock title={t.typescript} description={t.typescriptText} code={TYPESCRIPT} copyLabel={t.copy} copiedLabel={t.copied} /></div>
        </section>

        <section className="docs-section" id="errors">
          <div className="docs-section-heading"><span>07</span><div><h2>{t.errorTitle}</h2><p>{t.errorText}</p></div></div>
          <div className="table-scroll"><table className="mtable docs-errors"><thead><tr><th>{t.status}</th><th>{t.meaning}</th><th>{t.action}</th></tr></thead><tbody><ErrorRow code="401" meaning={t.e401} action={t.a401} labels={t} /><ErrorRow code="402" meaning={t.e402} action={t.a402} labels={t} /><ErrorRow code="429" meaning={t.e429} action={t.a429} labels={t} /><ErrorRow code="5xx" meaning={t.e5xx} action={t.a5xx} labels={t} /></tbody></table></div>
          <div className="docs-checklist"><h3>{t.production}</h3><ul>{t.checklist.map((item) => <li key={item}>{item}</li>)}</ul></div>
        </section>

        <section className="docs-section" id="pricing">
          <div className="docs-section-heading"><span>08</span><div><h2>{t.pricingTitle}</h2><p>{t.pricingLead}</p></div></div>
          <h3 className="docs-h3">{t.billingHead}</h3><p className="docs-para">{t.billingText}</p><div className="docs-notice">{t.billingExample}</div>
          <h3 className="docs-h3">{t.tiersHead}</h3><p className="docs-para">{t.tiersText}</p>
          <div className="table-scroll"><table className="mtable docs-errors"><thead><tr><th>{t.tierColTier}</th><th>{t.tierColDiscount}</th><th>{t.tierColValue}</th><th>{t.tierColGet}</th><th>{t.tierColKeep}</th></tr></thead>
            <tbody>{B2C_PRICING_MILESTONES.map((tier) => <tr key={tier.code}><td><b>{tier.label}</b></td><td>−{tier.discountPercent}%</td><td>×{(100 / (100 - tier.discountPercent)).toLocaleString(language === "ru" ? "ru-RU" : "en-US", { maximumFractionDigits: 2 })}</td><td>{Number(tier.platformSpendUsd) === 0 ? t.tierBaseCell : formatWholeUsd(tier.platformSpendUsd)}</td><td>{Number(tier.holdUsd) === 0 ? "—" : formatWholeUsd(tier.holdUsd)}</td></tr>)}</tbody>
          </table></div>
          <h3 className="docs-h3">{t.keepHead}</h3><p className="docs-para">{t.keepText}</p><div className="docs-notice">{t.keepFree}</div>
          <h3 className="docs-h3">{t.exampleHead}</h3><p className="docs-para">{t.exampleText}</p>
        </section>

        <footer className="docs-footer">{t.footer}</footer>
      </main>
    </div>
  </div>;
}

function BrandMark() {
  return <><Image className="brand-mark bm-light" src="/assets/logo-mark-light.png" width={24} height={24} alt="" /><Image className="brand-mark bm-dark" src="/assets/logo-mark-dark.png" width={24} height={24} alt="" /></>;
}

function Endpoint({ label, value, copyLabel, copiedLabel }: { label: string; value: string; copyLabel: string; copiedLabel: string }) {
  return <div className="docs-endpoint ym-hide-content"><span>{label}</span><code>{value}</code><CopyControl value={value} label={copyLabel} copiedLabel={copiedLabel} /></div>;
}

function CodeBlock({ title, description, code, copyLabel, copiedLabel }: { title: string; description?: string; code: string; copyLabel: string; copiedLabel: string }) {
  return <article className="docs-code-card ym-hide-content"><header><div><h3>{title}</h3>{description && <p>{description}</p>}</div><CopyControl value={code} label={copyLabel} copiedLabel={copiedLabel} /></header><pre><code>{code}</code></pre></article>;
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

function AgentIcon() {
  return <svg viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M12 3 9.9 8.9 4 11l5.9 2.1L12 19l2.1-5.9L20 11l-5.9-2.1L12 3Z" /><path d="m5 3 .6 1.7L7.3 5.3l-1.7.6L5 7.6l-.6-1.7-1.7-.6 1.7-.6L5 3Z" /></svg>;
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

function ShieldIcon() {
  return <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M12 3 5 6v5c0 4.5 3 7.6 7 9 4-1.4 7-4.5 7-9V6l-7-3Z" /><path d="m9.3 12 1.8 1.8 3.9-4" /></svg>;
}

function TelegramIcon() {
  return <svg viewBox="0 0 24 24" width="21" height="21" fill="currentColor" aria-hidden="true"><path d="M21.7 2.3 2.9 10.5c-1.3.5-1.3 1.3-.2 1.7l4.8 1.5 1.8 5.4c.2.7.1 1 .9 1 .6 0 .9-.3 1.2-.6l2.3-2.2 4.8 3.5c.9.5 1.5.2 1.8-.8l3.1-14.9c.4-1.3-.5-1.9-1.7-1.4ZM9.4 13.4l9.4-5.9c.5-.3.9-.1.5.2l-7.7 7-.3 3.1-1.9-4.4Z" /></svg>;
}

function LifebuoyIcon() {
  return <svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="3" /><path d="m5.6 5.6 4.3 4.3m4.2 4.2 4.3 4.3m0-12.8-4.3 4.3m-4.2 4.2-4.3 4.3" /></svg>;
}
