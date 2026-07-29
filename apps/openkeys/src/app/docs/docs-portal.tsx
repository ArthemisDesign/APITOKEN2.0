"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import { AppShell } from "@/components/app-shell";
import { useLanguage } from "@/components/chrome";

const BASE_URL = "https://api.apitoken.sale";
const MESSAGES_URL = `${BASE_URL}/v1/messages`;
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
const CACHE_SNIPPETS = {
  en: `curl https://api.apitoken.sale/v1/messages \\
  -H "x-api-key: sk-pool-•••" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{
    "model": "claude-opus-4-8",
    "max_tokens": 1024,
    "system": [
      {
        "type": "text",
        "text": "<long stable instruction or document>",
        "cache_control": { "type": "ephemeral" }
      }
    ],
    "messages": [{ "role": "user", "content": "Question about the document" }]
  }'`,
  ru: `curl https://api.apitoken.sale/v1/messages \\
  -H "x-api-key: sk-pool-•••" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{
    "model": "claude-opus-4-8",
    "max_tokens": 1024,
    "system": [
      {
        "type": "text",
        "text": "<длинная неизменная инструкция или документ>",
        "cache_control": { "type": "ephemeral" }
      }
    ],
    "messages": [{ "role": "user", "content": "Вопрос по документу" }]
  }'`,
} as const;
const CURSOR = `Provider: Anthropic
Base URL: https://api.apitoken.sale
API key: sk-pool-•••
Model ID: claude-opus-4-8`;
const PYTHON = `# pip install anthropic
from anthropic import Anthropic

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
const TYPESCRIPT = `// npm install @anthropic-ai/sdk
import Anthropic from "@anthropic-ai/sdk";

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
function cleanApiKey(value: string) {
  const key = value.trim();
  return /^sk-pool-[A-Za-z0-9._-]{4,}$/.test(key) ? key : "";
}

const copy = {
  en: {
    documentation: "Documentation",
    back: "Back to site",
    dashboard: "Dashboard",
    onThisPage: "On this page",
    overview: "Overview",
    quickstart: "Quick start",
    authentication: "Authentication",
    tools: "Developer tools",
    openai: "OpenAI-compatible API & Codex",
    sdks: "Official SDKs",
    errors: "Errors",
    cache: "Prompt caching",
    cacheTitle: "Prompt caching",
    cacheLead: "The single biggest lever on how fast your key is spent. Repeated context served from cache is billed at roughly a tenth of normal input.",
    cacheWhyHead: "Why it matters here",
    cacheWhyText: "An agent resends the whole conversation on every turn: the system prompt, the files it read, the previous replies. Without caching you pay full input price for that context on every single turn. With caching the repeated prefix is charged at the cache-read rate instead.",
    cacheNotice: "Your key is charged exactly what the request costs: input, output, cache writes and cache reads are metered separately and each at its own rate. Caching is not a discount we grant — it is a cheaper way to send the same request, and the saving lands on your balance.",
    cacheHowHead: "How to turn it on",
    cacheHowText: "Mark the stable part of the prompt with cache_control. Everything before that marker becomes the cached prefix; put volatile content — the current question, timestamps, per-request ids — after it.",
    cacheSnippetLabel: "Caching a long system prompt",
    cacheSnippetText: "The first request writes the cache, every following request reads it.",
    cacheRulesHead: "Rules worth knowing",
    cacheRules: [
      "The cache is a prefix match: a single changed byte anywhere in the prefix invalidates everything after it.",
      "Render order is tools → system → messages, so keep stable content first and volatile content last.",
      "A cache write costs about 1.25× normal input, a read about 0.1×. Two requests already pay it back.",
      "The default entry lives 5 minutes; each read refreshes it. A one-hour TTL is available and costs 2× to write.",
      "Do not interpolate the current time, a request id or a user name into the system prompt — that defeats caching entirely.",
      "Claude Code, Cursor and Cline enable caching themselves; you mostly need to avoid breaking the prefix.",
    ],
    cacheCheckHead: "How to check it works",
    cacheCheckText: "Every response reports cache_creation_input_tokens and cache_read_input_tokens in its usage block. If the read counter stays at zero across identical requests, something in the prefix is changing. Your key page also shows cache reads and writes as separate buckets.",
    eyebrow: "ANTHROPIC MESSAGES API · CONNECTION GUIDE",
    title: "Connect to the Claude API in three steps",
    lead: "Your universal sk-pool key gives Claude access through the Anthropic-compatible Messages API. Point the client at https://api.apitoken.sale and send a standard Messages request.",
    openKeys: "Open key usage",
    allConnections: "All connection options",
    gettingStarted: "Getting started path",
    stepKey: "Create an sk-pool key",
    stepBase: "Set the base URL",
    stepRequest: "POST /v1/messages",
    baseUrl: "Base URL",
    messagesEndpoint: "Messages endpoint",
    authHeader: "Authentication header",
    apiKeyLabel: "Your API key",
    apiKeyHelp: "Paste your sk-pool-… key to personalize examples that display a key. It stays only in this browser tab's memory: this page never uploads or saves it. Clear the field or reload the tab to remove it.",
    apiKeyApply: "Apply to examples",
    apiKeyClear: "Remove",
    apiKeyActive: "Examples use the key ending",
    oneEndpoint: "Connection details",
    oneEndpointText: "Only the host and API key change. Standard Messages request shapes, response objects, errors, and SSE streaming remain Anthropic-compatible.",
    keyNotice: "A full API key is shown only once when issued. Save it in an environment variable or secret manager. If it is lost or exposed, revoke it and create a replacement; the original value cannot be recovered.",
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
    openaiFullGuide: "Open the full GPT / OpenAI guide",
    openaiNotice: "This is an independent OpenAI-compatible service, not the OpenAI Platform and not an OpenAI-operated endpoint. It supports model discovery, Responses, and Chat Completions for text, including SSE streaming. Images, audio, files, realtime, assistants, batches, fine-tuning, and other OpenAI Platform services are not available.",
    openaiRequest: "Responses API",
    openaiRequestText: "Send a standard Responses request to the dedicated host. gpt-5.6-sol is currently available; use GET /v1/models instead of assuming a model ID.",
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
    authText: "For direct HTTP, send the sk-pool key in x-api-key and include anthropic-version: 2023-06-01. Official Anthropic SDKs add the version header automatically.",
    authNotice: "For Claude clients, x-api-key is the canonical header. The Authorization header is not needed in these examples.",
    errorTitle: "Common response codes",
    errorText: "Error bodies use Anthropic's JSON envelope. Treat 401 and 402 as account-state failures; retry only transient 429 and 5xx responses.",
    status: "Status",
    meaning: "Meaning",
    action: "What to do",
    e400: "The request body or parameters are invalid",
    a400: "Check the JSON shape, model ID, required max_tokens, and supported parameters. Do not retry unchanged input.",
    e401: "API key is missing, invalid, or revoked",
    a401: "Send an active sk-pool key in x-api-key. If it was revoked, create a replacement; do not retry the same key.",
    e402: "Available prepaid balance is too low",
    a402: "Top up the account, confirm the balance is available, then retry. Backoff alone will not resolve a 402.",
    e429: "Rate limit or temporary upstream capacity limit",
    a429: "Honor Retry-After when present; retry with capped exponential backoff and jitter.",
    e5xx: "Temporary gateway or Anthropic upstream failure",
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
    billingText: "We start with the model's official Anthropic token price, apply your tier discount, and deduct the remainder from your apiToken.sale balance. There are no fixed token packs or recurring subscriptions.",
    billingExample: "Example — Starter gives a 60% discount, so you pay 40%: $100 of usage at official Anthropic prices produces a $40 balance charge. In other words, $40 of balance covers about $100 of official usage (×2.5).",
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
    overview: "Обзор",
    quickstart: "Быстрый старт",
    authentication: "Аутентификация",
    tools: "Инструменты разработчика",
    openai: "OpenAI-совместимый API и Codex",
    sdks: "Официальные SDK",
    errors: "Ошибки",
    cache: "Кэширование",
    cacheTitle: "Кэширование промптов",
    cacheLead: "Главный рычаг, определяющий, как быстро расходуется ключ. Повторяющийся контекст, отданный из кэша, стоит примерно в десять раз дешевле обычного ввода.",
    cacheWhyHead: "Почему это важно именно здесь",
    cacheWhyText: "Агент пересылает весь диалог на каждом ходу: системный промпт, прочитанные файлы, предыдущие ответы. Без кэша вы платите за этот контекст полную входную ставку каждый раз. С кэшем повторяющийся префикс тарифицируется по ставке чтения кэша.",
    cacheNotice: "С ключа списывается ровно то, что стоит запрос: вход, выход, запись и чтение кэша считаются отдельно и каждый по своей ставке. Кэш — это не скидка, которую мы даём, а более дешёвый способ отправить тот же запрос, и экономия остаётся на вашем балансе.",
    cacheHowHead: "Как включить",
    cacheHowText: "Отметьте неизменную часть промпта через cache_control. Всё, что стоит до этой отметки, становится кэшируемым префиксом; переменное — текущий вопрос, время, идентификаторы запроса — размещайте после неё.",
    cacheSnippetLabel: "Кэшируем длинный системный промпт",
    cacheSnippetText: "Первый запрос пишет кэш, все последующие его читают.",
    cacheRulesHead: "Что стоит знать",
    cacheRules: [
      "Кэш работает по совпадению префикса: один изменившийся байт в начале обесценивает всё, что идёт после.",
      "Порядок сборки — tools → system → messages, поэтому стабильное держите в начале, изменчивое в конце.",
      "Запись кэша стоит около 1,25× обычного ввода, чтение — около 0,1×. Двух запросов уже достаточно, чтобы это окупилось.",
      "Запись живёт 5 минут, каждое чтение продлевает её. Есть часовой TTL, его запись стоит 2×.",
      "Не подставляйте в системный промпт текущее время, идентификатор запроса или имя пользователя — это полностью ломает кэш.",
      "Claude Code, Cursor и Cline включают кэш сами; от вас требуется в основном не ломать префикс.",
    ],
    cacheCheckHead: "Как проверить, что работает",
    cacheCheckText: "В каждом ответе в блоке usage приходят cache_creation_input_tokens и cache_read_input_tokens. Если счётчик чтения остаётся нулевым на одинаковых запросах — что-то в префиксе меняется. На странице вашего ключа чтение и запись кэша тоже показаны отдельными корзинами.",
    eyebrow: "ANTHROPIC MESSAGES API · РУКОВОДСТВО ПО ПОДКЛЮЧЕНИЮ",
    title: "Подключитесь к Claude API за три шага",
    lead: "Универсальный ключ sk-pool даёт доступ к Claude через совместимый Anthropic Messages API. Укажите клиенту адрес https://api.apitoken.sale и отправьте стандартный Messages-запрос.",
    openKeys: "Открыть расход ключа",
    allConnections: "Все способы подключения",
    gettingStarted: "Порядок подключения",
    stepKey: "Создайте ключ sk-pool",
    stepBase: "Укажите базовый URL",
    stepRequest: "POST /v1/messages",
    baseUrl: "Базовый URL",
    messagesEndpoint: "Адрес Messages API",
    authHeader: "Заголовок аутентификации",
    apiKeyLabel: "Ваш API-ключ",
    apiKeyHelp: "Вставьте ключ sk-pool-…, чтобы подставить его в примеры, где ключ указан явно. Он остаётся только в памяти этой вкладки браузера: страница не загружает и не сохраняет его. Чтобы удалить ключ, очистите поле или перезагрузите вкладку.",
    apiKeyApply: "Подставить в примеры",
    apiKeyClear: "Удалить",
    apiKeyActive: "В примерах используется ключ с окончанием",
    oneEndpoint: "Параметры подключения",
    oneEndpointText: "Меняются только адрес сервера и API-ключ. Стандартные тела Messages-запросов, объекты ответов, ошибки и SSE-потоки остаются Anthropic-совместимыми.",
    keyNotice: "Полный API-ключ показывается только один раз при выпуске. Сохраните его в переменной окружения или менеджере секретов. Если ключ потерян или раскрыт, отзовите его и создайте новый: восстановить исходное значение нельзя.",
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
    openaiFullGuide: "Открыть полную инструкцию GPT / OpenAI",
    openaiNotice: "Это независимый OpenAI-совместимый сервис, а не OpenAI Platform и не endpoint под управлением OpenAI. Поддерживаются список моделей, Responses и Chat Completions для текста, включая SSE-потоки. Изображения, аудио, файлы, realtime, assistants, batches, fine-tuning и другие сервисы OpenAI Platform недоступны.",
    openaiRequest: "Responses API",
    openaiRequestText: "Отправьте стандартный запрос Responses на отдельный хост. Модель gpt-5.6-sol доступна сейчас; получайте список через GET /v1/models, а не полагайтесь на неизменность ID.",
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
    authText: "В прямых HTTP-запросах передавайте ключ sk-pool в x-api-key и добавляйте anthropic-version: 2023-06-01. Официальные SDK Anthropic добавляют заголовок версии автоматически.",
    authNotice: "Для клиентов Claude канонический заголовок — x-api-key. Заголовок Authorization в этих примерах не нужен.",
    errorTitle: "Основные коды ответа",
    errorText: "Тело ошибки использует JSON-формат Anthropic. Коды 401 и 402 требуют исправить состояние аккаунта; автоматически повторяйте только временные ошибки 429 и 5xx.",
    status: "Статус",
    meaning: "Значение",
    action: "Что делать",
    e400: "Неверное тело запроса или параметры",
    a400: "Проверьте JSON, ID модели, обязательный max_tokens и поддерживаемые параметры. Не повторяйте неизменённый запрос.",
    e401: "API-ключ отсутствует, неверен или отозван",
    a401: "Передайте активный ключ sk-pool в x-api-key. Если ключ отозван, создайте новый; повторять запрос с тем же ключом не нужно.",
    e402: "Доступного предоплаченного баланса недостаточно",
    a402: "Пополните аккаунт, убедитесь, что баланс зачислен, и повторите запрос. Ожидание само по себе не устранит 402.",
    e429: "Лимит запросов или временный дефицит мощности провайдера",
    a429: "Учитывайте Retry-After, если он есть; используйте ограниченную экспоненциальную задержку со случайным смещением.",
    e5xx: "Временная ошибка шлюза или инфраструктуры Anthropic",
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
    billingText: "Мы берём официальную стоимость токенов выбранной модели у Anthropic, применяем скидку вашего тарифа и списываем остаток с баланса apiToken.sale. Фиксированных пакетов токенов и регулярной подписки нет.",
    billingExample: "Пример — Starter даёт скидку 60%, поэтому вы платите 40%: использование на $100 по официальным ценам Anthropic приведёт к списанию $40 с баланса. Иными словами, $40 баланса покрывают около $100 официального использования (×2,5).",
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
    footer: "Документация apiToken.sale · Claude и OpenAI-совместимый текстовый доступ",
  },
} as const;

export function DocsPortal() {
  const { language } = useLanguage();
  const t = copy[language];
  const [activeKey, setActiveKey] = useState("");
  const [keyInput, setKeyInput] = useState("");

  useEffect(() => () => {
    setActiveKey("");
    setKeyInput("");
  }, []);

  function applyKey() {
    const key = cleanApiKey(keyInput);
    setActiveKey(key);
    setKeyInput(key);
  }

  function clearKey() {
    setActiveKey("");
    setKeyInput("");
  }

  const withKey = (code: string) => activeKey
    ? code.replaceAll("sk-pool-•••", activeKey).replaceAll("$APITOKEN_API_KEY", activeKey)
    : code;

  return <AppShell
    section="claudeDocs"
    title={t.documentation}
  >
    <div className="docs-site">
    <div className="docs-layout">
      <aside className="docs-sidebar"><span>{t.onThisPage}</span><nav><a href="#overview">{t.overview}</a><a href="#quickstart">{t.quickstart}</a><a href="#authentication">{t.authentication}</a><a href="#tools">{t.tools}</a><a href="#sdks">{t.sdks}</a><a href="#errors">{t.errors}</a><a href="#cache">{t.cache}</a></nav></aside>
      <main className="docs-main" id="main-content" tabIndex={-1}>
        <section className="docs-hero" id="overview"><span className="eyebrow">{t.eyebrow}</span><h1>{t.title}</h1><p>{t.lead}</p><div className="hero-cta"><Link className="btn btn-primary" href="/profile">{t.openKeys}</Link><Link className="btn btn-ghost" href="/docs">{t.allConnections}</Link></div></section>

        <section className="docs-section">
          <div className="docs-section-heading"><span>01</span><div><h2>{t.oneEndpoint}</h2><p>{t.oneEndpointText}</p></div></div>
          <div className="docs-auth-flow" aria-label={t.gettingStarted} style={{ marginBottom: 14 }}>
            <div><b>1</b><code>{t.stepKey}</code></div><span>→</span><div><b>2</b><code>{t.stepBase}</code></div><span>→</span><div><b>3</b><code>{t.stepRequest}</code></div>
          </div>
          <div className="docs-notice" style={{ marginTop: 0, marginBottom: 14, display: "grid", gap: 12 }}>
            <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 12, flexWrap: "wrap" }}>
              <div><label htmlFor="docs-api-key" style={{ display: "block", color: "var(--txt)", fontWeight: 700 }}>{t.apiKeyLabel}</label><span id="docs-api-key-help" style={{ display: "block", marginTop: 3, color: "var(--txt-3)" }}>{t.apiKeyHelp}</span></div>
              {activeKey && <span role="status" aria-live="polite" style={{ color: "var(--accent)", fontSize: 11 }}>{t.apiKeyActive} <b>••••{activeKey.slice(-4)}</b></span>}
            </div>
            <form onSubmit={(event) => { event.preventDefault(); applyKey(); }} style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
              <input id="docs-api-key" className="ym-disable-keys" type="password" value={keyInput} onChange={(event) => setKeyInput(event.target.value)} placeholder="sk-pool-•••" aria-describedby="docs-api-key-help" autoComplete="off" autoCapitalize="none" autoCorrect="off" spellCheck={false} style={{ flex: "1 1 280px", minWidth: 0, padding: "10px 12px", border: "1px solid var(--line-strong)", borderRadius: 6, background: "var(--bg-card)", color: "var(--txt)", fontFamily: "var(--font-mono)", fontSize: 12 }} />
              <button className="btn btn-primary btn-sm" type="submit">{t.apiKeyApply}</button>
              <button className="btn btn-ghost btn-sm" type="button" onClick={clearKey}>{t.apiKeyClear}</button>
            </form>
          </div>
          <div className="docs-essential-grid"><Endpoint label={t.baseUrl} value={BASE_URL} copyLabel={t.copy} copiedLabel={t.copied} /><Endpoint label={t.messagesEndpoint} value={MESSAGES_URL} copyLabel={t.copy} copiedLabel={t.copied} /><Endpoint label={t.authHeader} value={withKey("x-api-key: sk-pool-•••")} copyLabel={t.copy} copiedLabel={t.copied} /></div>
          <div className="docs-notice">{t.keyNotice}</div>
        </section>

        <section className="docs-section" id="quickstart">
          <div className="docs-section-heading"><span>02</span><div><h2>{t.quickstart}</h2><p>{t.quickstartText}</p></div></div>
          <CodeBlock title={t.envTitle} description={t.envText} code={withKey(ENVIRONMENT)} copyLabel={t.copy} copiedLabel={t.copied} />
          <CodeBlock title={t.requestTitle} description={t.requestText} code={withKey(CURL)} copyLabel={t.copy} copiedLabel={t.copied} />
        </section>

        <section className="docs-section" id="authentication">
          <div className="docs-section-heading"><span>03</span><div><h2>{t.authentication}</h2><p>{t.authText}</p></div></div>
          <div className="docs-auth-flow"><div><b>1</b><code>x-api-key</code></div><span>＋</span><div><b>2</b><code>anthropic-version</code></div><span>→</span><div><b>API</b><code>/v1/messages</code></div></div>
          <div className="docs-notice">{t.authNotice}</div>
        </section>

        <section className="docs-section" id="tools">
          <div className="docs-section-heading"><span>04</span><div><h2>{t.tools}</h2><p>{t.toolsText}</p></div></div>
          <div className="docs-two-col"><CodeBlock title={t.claudeCode} description={t.claudeCodeText} code={withKey(CLAUDE_CODE)} copyLabel={t.copy} copiedLabel={t.copied} /><CodeBlock title={t.editors} description={t.editorsText} code={withKey(CURSOR)} copyLabel={t.copy} copiedLabel={t.copied} /></div>
        </section>

        <section className="docs-section" id="sdks">
          <div className="docs-section-heading"><span>05</span><div><h2>{t.sdks}</h2><p>{t.sdksText}</p></div></div>
          <div className="docs-two-col"><CodeBlock title={t.python} description={t.pythonText} code={PYTHON} copyLabel={t.copy} copiedLabel={t.copied} /><CodeBlock title={t.typescript} description={t.typescriptText} code={TYPESCRIPT} copyLabel={t.copy} copiedLabel={t.copied} /></div>
        </section>

        <section className="docs-section" id="errors">
          <div className="docs-section-heading"><span>06</span><div><h2>{t.errorTitle}</h2><p>{t.errorText}</p></div></div>
          <div className="table-scroll"><table className="mtable docs-errors"><thead><tr><th>{t.status}</th><th>{t.meaning}</th><th>{t.action}</th></tr></thead><tbody><ErrorRow code="400" meaning={t.e400} action={t.a400} labels={t} /><ErrorRow code="401" meaning={t.e401} action={t.a401} labels={t} /><ErrorRow code="402" meaning={t.e402} action={t.a402} labels={t} /><ErrorRow code="429" meaning={t.e429} action={t.a429} labels={t} /><ErrorRow code="5xx" meaning={t.e5xx} action={t.a5xx} labels={t} /></tbody></table></div>
          <div className="docs-checklist"><h3>{t.production}</h3><ul>{t.checklist.map((item) => <li key={item}>{item}</li>)}</ul></div>
        </section>

        <section className="docs-section" id="cache">
          <div className="docs-section-heading"><span>07</span><div><h2>{t.cacheTitle}</h2><p>{t.cacheLead}</p></div></div>
          <h3 className="docs-h3">{t.cacheWhyHead}</h3><p className="docs-para">{t.cacheWhyText}</p>
          <div className="docs-notice">{t.cacheNotice}</div>
          <h3 className="docs-h3">{t.cacheHowHead}</h3><p className="docs-para">{t.cacheHowText}</p>
          <CodeBlock title={t.cacheSnippetLabel} description={t.cacheSnippetText} code={withKey(CACHE_SNIPPETS[language])} copyLabel={t.copy} copiedLabel={t.copied} />
          <h3 className="docs-h3">{t.cacheRulesHead}</h3>
          <div className="docs-checklist"><ul>{t.cacheRules.map((item) => <li key={item}>{item}</li>)}</ul></div>
          <h3 className="docs-h3">{t.cacheCheckHead}</h3><p className="docs-para">{t.cacheCheckText}</p>
        </section>

        <footer className="docs-footer">{t.footer}</footer>
      </main>
    </div>
    </div>
  </AppShell>;
}

function Endpoint({ label, value, copyLabel, copiedLabel }: { label: string; value: string; copyLabel: string; copiedLabel: string }) {
  return <div className="docs-endpoint ym-hide-content"><span>{label}</span><code>{value}</code><CopyControl value={value} label={copyLabel} copiedLabel={copiedLabel} /></div>;
}

function CodeBlock({ title, description, code, copyLabel, copiedLabel }: { title: string; description?: string; code: string; copyLabel: string; copiedLabel: string }) {
  return <article className="docs-code-card ym-hide-content"><header><div><h3>{title}</h3>{description && <p>{description}</p>}</div><CopyControl value={code} label={copyLabel} copiedLabel={copiedLabel} /></header><pre><code>{code}</code></pre></article>;
}

function CopyControl({ value, label, copiedLabel }: { value: string; label: string; copiedLabel: string }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1_200);
  }

  return <button className="btn btn-ghost btn-sm docs-copy" type="button" onClick={handleCopy}>{copied ? copiedLabel : label}</button>;
}

function ErrorRow({ code, meaning, action, labels }: { code: string; meaning: string; action: string; labels: { status: string; meaning: string; action: string } }) {
  return <tr><td data-label={labels.status}><code>{code}</code></td><td data-label={labels.meaning}><span>{meaning}</span></td><td data-label={labels.action}><span>{action}</span></td></tr>;
}
