"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import { AppShell } from "@/components/app-shell";
import { useLanguage } from "@/components/chrome";

const BASE_URL = "https://openai.api.apitoken.sale/v1";
const RESPONSES_URL = `${BASE_URL}/responses`;
const MODELS_URL = `${BASE_URL}/models`;

const ENVIRONMENT = `export OPENAI_BASE_URL="https://openai.api.apitoken.sale/v1"
export OPENAI_API_KEY="sk-pool-•••"`;

const CURL = `curl https://openai.api.apitoken.sale/v1/responses \\
  -H "Authorization: Bearer $OPENAI_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.6-sol",
    "input": "Reply with exactly: connected"
  }'`;

const MODELS = `curl https://openai.api.apitoken.sale/v1/models \\
  -H "Authorization: Bearer $OPENAI_API_KEY"`;

const CHAT = `curl https://openai.api.apitoken.sale/v1/chat/completions \\
  -H "Authorization: Bearer $OPENAI_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.6-sol",
    "messages": [{ "role": "user", "content": "Hello" }]
  }'`;

const PYTHON = `# pip install openai
import os
from openai import OpenAI

client = OpenAI(
    api_key=os.environ["OPENAI_API_KEY"],
    base_url="https://openai.api.apitoken.sale/v1",
)

response = client.responses.create(
    model="gpt-5.6-sol",
    input="Reply with exactly: connected",
)
print(response.output_text)`;

const TYPESCRIPT = `// npm install openai
import OpenAI from "openai";

const client = new OpenAI({
  apiKey: process.env.OPENAI_API_KEY,
  baseURL: "https://openai.api.apitoken.sale/v1",
});

const response = await client.responses.create({
  model: "gpt-5.6-sol",
  input: "Reply with exactly: connected",
});
console.log(response.output_text);`;

const CODEX_PROFILE = `# ~/.codex/apitoken.config.toml
model = "gpt-5.6-sol"
model_provider = "apitoken"

[model_providers.apitoken]
name = "apiToken.sale"
base_url = "https://openai.api.apitoken.sale/v1"
wire_api = "responses"
env_key = "APITOKEN_API_KEY"`;

const CODEX_RUN = `export APITOKEN_API_KEY="sk-pool-•••"
codex --profile apitoken`;

const copy = {
  ru: {
    titleBar: "GPT / OpenAI API",
    onPage: "На этой странице",
    overview: "Обзор",
    quickstart: "Быстрый старт",
    auth: "Авторизация",
    endpoints: "Методы API",
    sdk: "OpenAI SDK",
    codex: "Codex CLI",
    errors: "Ошибки",
    usage: "Расход ключа",
    eyebrow: "OPENAI-СОВМЕСТИМЫЙ API · ИНСТРУКЦИЯ",
    title: "Подключите GPT API за три шага",
    lead: "Тот же универсальный ключ sk-pool, который работает с Claude, открывает GPT-модели через отдельный OpenAI-совместимый Base URL. Баланс и USAGE остаются общими.",
    profile: "Открыть расход ключа",
    claude: "Инструкция Claude",
    allConnections: "Все способы подключения",
    connection: "Параметры подключения",
    connectionText: "Меняются только Base URL и ключ. Используйте OpenAI-совместимый формат запроса и получайте актуальный список моделей через GET /v1/models.",
    keyLabel: "Ваш API-ключ",
    keyHelp: "Можно подставить ключ в примеры. Он остаётся только в памяти этой вкладки и никуда не отправляется; перезагрузка удалит его.",
    apply: "Подставить",
    clear: "Очистить",
    active: "Подставлен ключ с окончанием",
    baseUrl: "Base URL",
    responses: "Responses endpoint",
    authHeader: "Заголовок авторизации",
    notice: "Это независимый OpenAI-совместимый сервис, а не OpenAI Platform и не endpoint под управлением OpenAI. Ключ показывается полностью только при выпуске — храните его в переменной окружения или менеджере секретов.",
    quickText: "Сначала задайте переменные окружения, затем отправьте минимальный Responses-запрос. Ответ подтверждает ключ, баланс, модель и endpoint.",
    envTitle: "Переменные окружения",
    envText: "Официальные OpenAI SDK читают OPENAI_API_KEY. OPENAI_BASE_URL поддерживается многими совместимыми клиентами.",
    firstTitle: "Первый запрос",
    firstText: "Пример использует gpt-5.6-sol. Перед production-вызовом проверьте доступные модели: их набор может меняться.",
    authText: "В прямых HTTP-запросах используйте канонический OpenAI-совместимый заголовок Authorization: Bearer. Заголовок anthropic-version здесь не нужен.",
    authNotice: "Рекомендуется: Authorization: Bearer sk-pool-… · Шлюз также принимает x-api-key для совместимости, но OpenAI SDK и примеры используют Bearer.",
    endpointText: "Поддерживается текстовая OpenAI-совместимая поверхность: модели, Responses и Chat Completions, включая SSE streaming.",
    modelsTitle: "Получить модели",
    modelsText: "Не предполагайте, что доступен любой ID OpenAI. GET /v1/models возвращает текущий набор этого шлюза.",
    chatTitle: "Chat Completions",
    chatText: "Используйте этот метод для клиентов, которые ещё не работают с Responses API.",
    unsupported: "Изображения, аудио, файлы, realtime, assistants, batches, fine-tuning и другие сервисы OpenAI Platform не поддерживаются.",
    sdkText: "Официальные Python и TypeScript SDK работают с собственным base_url. Ключ храните только на сервере.",
    python: "Python SDK",
    typescript: "TypeScript SDK",
    codexText: "Именованный профиль подключает Codex к apiToken.sale и не заменяет обычный логин или конфигурацию по умолчанию.",
    profileTitle: "Профиль Codex",
    profileText: "Сохраните overlay в ~/.codex/apitoken.config.toml.",
    runTitle: "Запуск Codex",
    runText: "Ключ остаётся в окружении, а профиль выбирается явно.",
    errorText: "401 и 402 требуют исправить ключ или баланс. Автоматически повторяйте только временные 429 и 5xx.",
    status: "Статус",
    meaning: "Значение",
    action: "Что делать",
    rows: [
      ["400", "Неверное тело запроса или неподдерживаемый параметр", "Проверьте JSON, метод API и ID модели; не повторяйте неизменённый запрос."],
      ["401", "Ключ отсутствует, неверен или отключён", "Проверьте Bearer-заголовок; отозванный ключ замените."],
      ["402", "Недостаточно предоплаченного баланса", "Пополните или замените ключ; ожидание не исправит 402."],
      ["404", "Endpoint или модель недоступны", "Проверьте путь и получите модели через GET /v1/models."],
      ["429", "Временный лимит мощности", "Учитывайте Retry-After и используйте ограниченный backoff с jitter."],
      ["5xx", "Временная ошибка шлюза", "Повторите запрос с ограниченным экспоненциальным backoff."],
    ],
    usageText: "Одна страница USAGE объединяет Claude и GPT: показывает общий остаток, временный резерв, списания по дням и отдельную разбивку по API и моделям.",
    usageButton: "Перейти в USAGE",
    usageRules: [
      "Откройте персональную ссылку, которую получили вместе с ключом, или войдите на странице расхода по самому sk-pool.",
      "В профиле одновременно показаны оба Base URL и отдельные ссылки на инструкции.",
      "Не публикуйте персональную ссылку: она открывает статистику конкретного ключа без повторного ввода секрета.",
    ],
    copy: "Копировать",
    copied: "Скопировано",
    footer: "Документация apiToken.sale · GPT / OpenAI-совместимый API",
  },
  en: {
    titleBar: "GPT / OpenAI API",
    onPage: "On this page",
    overview: "Overview",
    quickstart: "Quick start",
    auth: "Authentication",
    endpoints: "API methods",
    sdk: "OpenAI SDK",
    codex: "Codex CLI",
    errors: "Errors",
    usage: "Key usage",
    eyebrow: "OPENAI-COMPATIBLE API · CONNECTION GUIDE",
    title: "Connect to the GPT API in three steps",
    lead: "The same universal sk-pool key used with Claude opens GPT models through a dedicated OpenAI-compatible Base URL. Balance and USAGE stay shared.",
    profile: "Open key usage",
    claude: "Claude guide",
    allConnections: "All connection options",
    connection: "Connection details",
    connectionText: "Only the Base URL and key change. Use OpenAI-compatible request shapes and discover the current models through GET /v1/models.",
    keyLabel: "Your API key",
    keyHelp: "You can insert the key into examples. It stays only in this tab's memory and is never uploaded; reload the page to remove it.",
    apply: "Apply",
    clear: "Clear",
    active: "Using the key ending",
    baseUrl: "Base URL",
    responses: "Responses endpoint",
    authHeader: "Authorization header",
    notice: "This is an independent OpenAI-compatible service, not the OpenAI Platform and not an OpenAI-operated endpoint. The full key is shown only when issued; keep it in an environment variable or secret manager.",
    quickText: "Set the environment variables, then send a minimal Responses request. A response confirms the key, balance, model, and endpoint.",
    envTitle: "Environment variables",
    envText: "Official OpenAI SDKs read OPENAI_API_KEY. Many compatible clients also honor OPENAI_BASE_URL.",
    firstTitle: "First request",
    firstText: "The example uses gpt-5.6-sol. Discover available models before production requests because the set can change.",
    authText: "Use the canonical OpenAI-compatible Authorization: Bearer header in direct HTTP requests. anthropic-version is not needed here.",
    authNotice: "Recommended: Authorization: Bearer sk-pool-… · The gateway also accepts x-api-key for compatibility, while OpenAI SDKs and these examples use Bearer.",
    endpointText: "The supported OpenAI-compatible text surface includes models, Responses, and Chat Completions with SSE streaming.",
    modelsTitle: "Discover models",
    modelsText: "Do not assume every OpenAI model ID is available. GET /v1/models returns the gateway's current set.",
    chatTitle: "Chat Completions",
    chatText: "Use this method for clients that do not yet support the Responses API.",
    unsupported: "Images, audio, files, realtime, assistants, batches, fine-tuning, and other OpenAI Platform services are not supported.",
    sdkText: "Official Python and TypeScript SDKs work with a custom base_url. Keep the key server-side.",
    python: "Python SDK",
    typescript: "TypeScript SDK",
    codexText: "A named profile connects Codex to apiToken.sale without replacing the normal login or default configuration.",
    profileTitle: "Codex profile",
    profileText: "Save the overlay as ~/.codex/apitoken.config.toml.",
    runTitle: "Run Codex",
    runText: "Keep the key in the environment and select the profile explicitly.",
    errorText: "401 and 402 require a key or balance change. Retry only transient 429 and 5xx responses automatically.",
    status: "Status",
    meaning: "Meaning",
    action: "What to do",
    rows: [
      ["400", "Invalid request body or unsupported parameter", "Check the JSON shape, API method, and model ID; do not retry unchanged input."],
      ["401", "Key is missing, invalid, or disabled", "Check the Bearer header and replace a revoked key."],
      ["402", "Prepaid balance is too low", "Top up or replace the key; waiting cannot resolve 402."],
      ["404", "Endpoint or model is unavailable", "Check the path and discover models with GET /v1/models."],
      ["429", "Temporary capacity limit", "Honor Retry-After and use capped backoff with jitter."],
      ["5xx", "Temporary gateway failure", "Retry with bounded exponential backoff."],
    ],
    usageText: "One USAGE page combines Claude and GPT: shared remaining balance, temporary holds, daily charges, and separate API and model breakdowns.",
    usageButton: "Open USAGE",
    usageRules: [
      "Open the personal link delivered with the key, or sign in on the usage page with the sk-pool key itself.",
      "The profile displays both Base URLs and separate links to both connection guides.",
      "Do not publish the personal link: it opens that key's statistics without asking for the secret again.",
    ],
    copy: "Copy",
    copied: "Copied",
    footer: "apiToken.sale documentation · GPT / OpenAI-compatible API",
  },
} as const;

function cleanApiKey(value: string): string {
  const key = value.trim();
  return /^sk-pool-[A-Za-z0-9._-]{4,}$/.test(key) ? key : "";
}

export function OpenAiDocsPortal() {
  const { language } = useLanguage();
  const t = copy[language];
  const [keyInput, setKeyInput] = useState("");
  const [activeKey, setActiveKey] = useState("");

  useEffect(() => () => {
    setKeyInput("");
    setActiveKey("");
  }, []);

  const withKey = (value: string) => activeKey
    ? value.replaceAll("sk-pool-•••", activeKey).replaceAll("$OPENAI_API_KEY", activeKey)
    : value;

  return (
    <AppShell section="openaiDocs" title={t.titleBar}>
      <div className="docs-site">
        <div className="docs-layout">
          <aside className="docs-sidebar">
            <span>{t.onPage}</span>
            <nav>
              <a href="#overview">{t.overview}</a><a href="#quickstart">{t.quickstart}</a>
              <a href="#authentication">{t.auth}</a><a href="#endpoints">{t.endpoints}</a>
              <a href="#sdk">{t.sdk}</a><a href="#codex">{t.codex}</a>
              <a href="#errors">{t.errors}</a><a href="#usage">{t.usage}</a>
            </nav>
          </aside>
          <main className="docs-main" id="main-content" tabIndex={-1}>
            <section className="docs-hero" id="overview">
              <span className="eyebrow">{t.eyebrow}</span><h1>{t.title}</h1><p>{t.lead}</p>
              <div className="hero-cta"><Link className="btn btn-primary" href="/profile">{t.profile}</Link><Link className="btn btn-ghost" href="/docs">{t.allConnections}</Link><Link className="btn btn-ghost" href="/docs/claude">{t.claude}</Link></div>
            </section>

            <section className="docs-section">
              <Heading number="01" title={t.connection} text={t.connectionText} />
              <div className="docs-auth-flow" style={{ marginBottom: 14 }}><div><b>1</b><code>sk-pool</code></div><span>→</span><div><b>2</b><code>Base URL</code></div><span>→</span><div><b>3</b><code>POST /responses</code></div></div>
              <div className="docs-notice" style={{ marginTop: 0, marginBottom: 14, display: "grid", gap: 12 }}>
                <div><label htmlFor="openai-docs-key" style={{ display: "block", color: "var(--txt)", fontWeight: 700 }}>{t.keyLabel}</label><span id="openai-docs-key-help" style={{ display: "block", marginTop: 3, color: "var(--txt-3)" }}>{t.keyHelp}</span></div>
                {activeKey ? <span role="status" style={{ color: "var(--accent)", fontSize: 11 }}>{t.active} <b>••••{activeKey.slice(-4)}</b></span> : null}
                <form onSubmit={(event) => { event.preventDefault(); const key = cleanApiKey(keyInput); setKeyInput(key); setActiveKey(key); }} style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                  <input id="openai-docs-key" className="ym-disable-keys" type="password" value={keyInput} onChange={(event) => setKeyInput(event.target.value)} placeholder="sk-pool-•••" aria-describedby="openai-docs-key-help" autoComplete="off" spellCheck={false} style={{ flex: "1 1 280px", minWidth: 0, padding: "10px 12px", border: "1px solid var(--line-strong)", borderRadius: 6, background: "var(--bg-card)", color: "var(--txt)", fontFamily: "var(--font-mono)" }} />
                  <button className="btn btn-primary btn-sm" type="submit">{t.apply}</button><button className="btn btn-ghost btn-sm" type="button" onClick={() => { setKeyInput(""); setActiveKey(""); }}>{t.clear}</button>
                </form>
              </div>
              <div className="docs-essential-grid"><Endpoint label={t.baseUrl} value={BASE_URL} labels={t} /><Endpoint label={t.responses} value={RESPONSES_URL} labels={t} /><Endpoint label={t.authHeader} value={withKey("Authorization: Bearer sk-pool-•••")} labels={t} /></div>
              <div className="docs-notice">{t.notice}</div>
            </section>

            <section className="docs-section" id="quickstart">
              <Heading number="02" title={t.quickstart} text={t.quickText} />
              <CodeBlock title={t.envTitle} text={t.envText} code={withKey(ENVIRONMENT)} labels={t} />
              <CodeBlock title={t.firstTitle} text={t.firstText} code={withKey(CURL)} labels={t} />
            </section>

            <section className="docs-section" id="authentication">
              <Heading number="03" title={t.auth} text={t.authText} />
              <div className="docs-auth-flow"><div><b>1</b><code>Authorization</code></div><span>＋</span><div><b>2</b><code>Bearer sk-pool</code></div><span>→</span><div><b>API</b><code>/v1/*</code></div></div>
              <div className="docs-notice">{t.authNotice}</div>
            </section>

            <section className="docs-section" id="endpoints">
              <Heading number="04" title={t.endpoints} text={t.endpointText} />
              <div className="docs-two-col"><CodeBlock title={t.modelsTitle} text={t.modelsText} code={withKey(MODELS)} labels={t} /><CodeBlock title={t.chatTitle} text={t.chatText} code={withKey(CHAT)} labels={t} /></div>
              <div className="docs-notice">{t.unsupported}</div>
              <div className="docs-essential-grid" style={{ marginTop: 14 }}><Endpoint label="GET /models" value={MODELS_URL} labels={t} /><Endpoint label="POST /responses" value={RESPONSES_URL} labels={t} /><Endpoint label="POST /chat/completions" value={`${BASE_URL}/chat/completions`} labels={t} /></div>
            </section>

            <section className="docs-section" id="sdk">
              <Heading number="05" title={t.sdk} text={t.sdkText} />
              <div className="docs-two-col"><CodeBlock title={t.python} code={withKey(PYTHON)} labels={t} /><CodeBlock title={t.typescript} code={withKey(TYPESCRIPT)} labels={t} /></div>
            </section>

            <section className="docs-section" id="codex">
              <Heading number="06" title={t.codex} text={t.codexText} />
              <div className="docs-two-col"><CodeBlock title={t.profileTitle} text={t.profileText} code={CODEX_PROFILE} labels={t} /><CodeBlock title={t.runTitle} text={t.runText} code={withKey(CODEX_RUN)} labels={t} /></div>
            </section>

            <section className="docs-section" id="errors">
              <Heading number="07" title={t.errors} text={t.errorText} />
              <div className="table-scroll"><table className="mtable docs-errors"><thead><tr><th>{t.status}</th><th>{t.meaning}</th><th>{t.action}</th></tr></thead><tbody>{t.rows.map((row) => <tr key={row[0]}><td data-label={t.status}><code>{row[0]}</code></td><td data-label={t.meaning}>{row[1]}</td><td data-label={t.action}>{row[2]}</td></tr>)}</tbody></table></div>
            </section>

            <section className="docs-section" id="usage">
              <Heading number="08" title={t.usage} text={t.usageText} />
              <div className="docs-checklist"><ul>{t.usageRules.map((rule) => <li key={rule}>{rule}</li>)}</ul></div>
              <div className="hero-cta"><Link className="btn btn-primary" href="/profile">{t.usageButton}</Link></div>
            </section>

            <footer className="docs-footer">{t.footer}</footer>
          </main>
        </div>
      </div>
    </AppShell>
  );
}

function Heading({ number, title, text }: { number: string; title: string; text: string }) {
  return <div className="docs-section-heading"><span>{number}</span><div><h2>{title}</h2><p>{text}</p></div></div>;
}

function Endpoint({ label, value, labels }: { label: string; value: string; labels: { copy: string; copied: string } }) {
  return <div className="docs-endpoint ym-hide-content"><span>{label}</span><code>{value}</code><CopyControl value={value} labels={labels} /></div>;
}

function CodeBlock({ title, text, code, labels }: { title: string; text?: string; code: string; labels: { copy: string; copied: string } }) {
  return <article className="docs-code-card ym-hide-content"><header><div><h3>{title}</h3>{text ? <p>{text}</p> : null}</div><CopyControl value={code} labels={labels} /></header><pre><code>{code}</code></pre></article>;
}

function CopyControl({ value, labels }: { value: string; labels: { copy: string; copied: string } }) {
  const [copied, setCopied] = useState(false);
  return <button className="btn btn-ghost btn-sm docs-copy" type="button" onClick={() => { void navigator.clipboard.writeText(value).then(() => { setCopied(true); window.setTimeout(() => setCopied(false), 1_200); }); }}>{copied ? labels.copied : labels.copy}</button>;
}
