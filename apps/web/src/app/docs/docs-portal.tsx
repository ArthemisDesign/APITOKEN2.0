"use client";

import Image from "next/image";
import Link from "next/link";
import { useEffect, useState } from "react";
import { useI18n } from "@/components/i18n-provider";
import { ThemeToggle } from "@/components/site-chrome";
import { B2C_PRICING_MILESTONES, formatWholeUsd } from "@/lib/pricing-tiers";

const BASE_URL = "https://api.apitoken.sale";
const MESSAGES_URL = `${BASE_URL}/v1/messages`;
const CURL = `curl https://api.apitoken.sale/v1/messages \\
  -H "x-api-key: $APITOKEN_API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{
    "model": "claude-opus-4-8",
    "max_tokens": 1024,
    "messages": [{ "role": "user", "content": "Hello" }]
  }'`;
const ENVIRONMENT = `export ANTHROPIC_BASE_URL=https://api.apitoken.sale
export ANTHROPIC_API_KEY=sk-pool-•••
export APITOKEN_API_KEY=sk-pool-•••`;
const CLAUDE_CODE = `# Add these to your shell profile
export ANTHROPIC_BASE_URL=https://api.apitoken.sale
export ANTHROPIC_API_KEY=sk-pool-•••

# Start Claude Code
claude`;
const CURSOR = `Provider: Anthropic
Base URL: https://api.apitoken.sale
API key: sk-pool-•••
Model: claude-opus-4-8`;
const PYTHON = `from anthropic import Anthropic

client = Anthropic(
    base_url="https://api.apitoken.sale",
    api_key="sk-pool-•••",
)

message = client.messages.create(
    model="claude-opus-4-8",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello"}],
)
print(message.content)`;
const TYPESCRIPT = `import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic({
  baseURL: "https://api.apitoken.sale",
  apiKey: "sk-pool-•••",
});

const message = await client.messages.create({
  model: "claude-opus-4-8",
  max_tokens: 1024,
  messages: [{ role: "user", content: "Hello" }],
});`;

function cleanApiKey(value: string) {
  const key = value.trim();
  return /^sk-[A-Za-z0-9._-]{4,}$/.test(key) ? key : "";
}

const copy = {
  en: {
    documentation: "Documentation", back: "Back to site", dashboard: "Dashboard", onThisPage: "On this page",
    overview: "Overview", quickstart: "Quick start", authentication: "Authentication", tools: "Developer tools",
    sdks: "SDKs", errors: "Errors", pricing: "Pricing & tiers", eyebrow: "CLAUDE API · CONNECTION GUIDE",
    pricingTitle: "Pricing & discount tiers", pricingLead: "One prepaid balance. You pay a share of official Anthropic prices set by your discount tier — and the tier grows as you top up.",
    billingHead: "How billing works", billingText: "Every request is priced at the upstream model's official Anthropic rate, then multiplied by the share you pay after your discount. No token packs, no subscriptions.",
    billingExample: "Example — at Starter (−60%): $100 of official Claude API usage × 40% paid = $40 charged from your balance. So $40 of balance buys ≈ $100 of official usage (×2.5).",
    tiersHead: "Discount tiers", tiersText: "Starter −60% is free — every account has it from the start. Higher tiers are unlocked by your CUMULATIVE top-ups (they add up over time). The more you prepay, the deeper the discount.",
    tierColTier: "Tier", tierColDiscount: "Discount", tierColValue: "$1 buys", tierColGet: "Top up to reach", tierColKeep: "Keep / 30 days", tierBaseCell: "Free · default",
    keepHead: "Keeping a tier", keepText: "To keep a tier you must spend at least 50% of its threshold within every rolling 30-day window. If you don't, you drop one tier and your accumulated top-ups reset to the new tier's threshold. Until you drop, top-ups keep adding up toward the next tier.",
    keepFree: "Starter −60% is the base tier — it never expires and needs no top-up.",
    exampleHead: "A full example", exampleText: "Top up $250 in total → you reach Pro (−70%). To keep Pro, spend ≥ $125 of platform charge every 30 days. Spend enough and the window renews; miss it and you drop to Builder (−65%). Top up $250 more (cumulative $500) → Studio (−75%).",
    title: "Build with one Claude endpoint", lead: "Everything required to connect Claude Code, editors, scripts, and production applications to apiToken.sale.",
    openKeys: "Open API keys", baseUrl: "Base URL", messagesEndpoint: "Messages endpoint", authHeader: "Authentication header",
    apiKeyLabel: "Your API key", apiKeyHelp: "Paste a valid sk-… key. It stays only in this tab's memory, is never uploaded, and is used only to fill the examples below.", apiKeyApply: "Use this key", apiKeyClear: "Clear", apiKeyActive: "Snippets below use your key",
    oneEndpoint: "Connection essentials", oneEndpointText: "All supported Claude models use the Anthropic Messages API through the same base URL.",
    keyNotice: "Your raw API key is shown only once when created. Store it in a secret manager or environment variable; never commit it to source control.",
    requestTitle: "Send your first request", requestText: "This request uses the Anthropic Messages API and works with streaming clients as well.",
    envTitle: "Environment variables", envText: "Use environment variables so tools can share the same endpoint without embedding secrets in configuration files.",
    claudeCode: "Claude Code", claudeCodeText: "Set the Anthropic-compatible endpoint, then start Claude Code normally.",
    editors: "Cursor, Cline, Continue, and Zed", editorsText: "Choose Anthropic as the provider. Use the same base URL, API key, and a supported model ID.",
    python: "Python SDK", pythonText: "The official Anthropic Python SDK accepts a custom base URL.",
    typescript: "TypeScript SDK", typescriptText: "The official Anthropic TypeScript SDK works through the same Messages API.",
    authTitle: "How authentication works", authText: "Send your key in the x-api-key header. Also include anthropic-version: 2023-06-01 on direct HTTP requests.",
    errorTitle: "Common response codes", errorText: "Errors use HTTP status codes and a JSON response body. Do not retry authentication or balance errors without changing the request or account state.",
    status: "Status", meaning: "Meaning", action: "What to do", e401: "Missing, invalid, or revoked API key", a401: "Check x-api-key or create a new key.",
    e402: "Insufficient available balance", a402: "Top up the account and retry.", e429: "Request or provider capacity limit", a429: "Back off with jitter and retry.",
    e5xx: "Temporary gateway or upstream error", a5xx: "Retry safely with exponential backoff.", copy: "Copy", copied: "Copied",
    production: "Production checklist", checklist: ["Keep API keys server-side", "Use request timeouts", "Retry 429 and 5xx responses with backoff", "Log request IDs, not raw secrets", "Monitor balance and the request ledger"],
    footer: "apiToken.sale documentation · Anthropic-compatible Claude access",
  },
  ru: {
    documentation: "Документация", back: "На главный сайт", dashboard: "Кабинет", onThisPage: "На этой странице",
    overview: "Обзор", quickstart: "Быстрый старт", authentication: "Аутентификация", tools: "Инструменты",
    sdks: "SDK", errors: "Ошибки", pricing: "Цены и тарифы", eyebrow: "CLAUDE API · РУКОВОДСТВО ПО ПОДКЛЮЧЕНИЮ",
    pricingTitle: "Цены и тарифные скидки", pricingLead: "Единый предоплаченный баланс. Ты платишь долю от официальных цен Anthropic, заданную твоим тарифом — а тариф растёт по мере пополнений.",
    billingHead: "Как считается оплата", billingText: "Каждый запрос считается по официальной цене модели у Anthropic и умножается на долю, которую ты платишь после скидки. Никаких пакетов токенов и подписок.",
    billingExample: "Пример — на Starter (−60%): $100 официального использования Claude API × 40% оплаты = $40 списывается с баланса. То есть $40 баланса ≈ $100 официального использования (×2.5).",
    tiersHead: "Тарифные скидки", tiersText: "Starter −60% бесплатно — есть у каждого аккаунта сразу. Выше тиры открываются по НАКОПЛЕННЫМ пополнениям (они суммируются). Чем больше предоплата, тем выше скидка.",
    tierColTier: "Тариф", tierColDiscount: "Скидка", tierColValue: "$1 даёт", tierColGet: "Пополнить, чтобы получить", tierColKeep: "Держать / 30 дней", tierBaseCell: "Бесплатно · по умолчанию",
    keepHead: "Как удержать тариф", keepText: "Чтобы удержать тариф, нужно потратить не менее 50% его порога за каждые скользящие 30 дней. Если не выполнил — откат на один тариф, а накопленные пополнения сбрасываются к порогу нового тарифа. Пока не слетел — пополнения суммируются к следующему тарифу.",
    keepFree: "Starter −60% — базовый тариф, не истекает и не требует пополнения.",
    exampleHead: "Полный пример", exampleText: "Пополнил суммарно $250 → получил Pro (−70%). Чтобы удержать Pro, трать ≥ $125 списаний каждые 30 дней. Хватило — окно продлевается; не хватило — откат на Builder (−65%). Пополнил ещё $250 (суммарно $500) → Studio (−75%).",
    title: "Один адрес для работы с Claude", lead: "Всё необходимое для подключения Claude Code, редакторов, скриптов и production-приложений к apiToken.sale.",
    openKeys: "Открыть API-ключи", baseUrl: "Базовый URL", messagesEndpoint: "Адрес Messages API", authHeader: "Заголовок авторизации",
    apiKeyLabel: "Ваш API-ключ", apiKeyHelp: "Вставьте действующий ключ sk-…. Он хранится только в памяти этой вкладки, никуда не загружается и используется лишь для заполнения примеров ниже.", apiKeyApply: "Использовать ключ", apiKeyClear: "Очистить", apiKeyActive: "В примерах ниже используется ваш ключ",
    oneEndpoint: "Основные параметры", oneEndpointText: "Все поддерживаемые модели Claude используют Anthropic Messages API через единый базовый URL.",
    keyNotice: "Исходный API-ключ показывается только один раз при создании. Храните его в менеджере секретов или переменной окружения и никогда не добавляйте в репозиторий.",
    requestTitle: "Отправьте первый запрос", requestText: "Запрос использует Anthropic Messages API и также подходит для потоковых клиентов.",
    envTitle: "Переменные окружения", envText: "Переменные окружения позволяют инструментам использовать единый адрес без хранения секретов в конфигурационных файлах.",
    claudeCode: "Claude Code", claudeCodeText: "Укажите Anthropic-совместимый адрес и запускайте Claude Code как обычно.",
    editors: "Cursor, Cline, Continue и Zed", editorsText: "Выберите Anthropic как провайдера. Используйте тот же базовый URL, API-ключ и ID поддерживаемой модели.",
    python: "Python SDK", pythonText: "Официальный Anthropic SDK для Python поддерживает собственный базовый URL.",
    typescript: "TypeScript SDK", typescriptText: "Официальный Anthropic SDK для TypeScript работает через тот же Messages API.",
    authTitle: "Как работает аутентификация", authText: "Передавайте ключ в заголовке x-api-key. Для прямых HTTP-запросов также передавайте anthropic-version: 2023-06-01.",
    errorTitle: "Основные коды ответа", errorText: "Ошибки возвращаются HTTP-кодом и JSON-телом. Не повторяйте запрос при проблеме с ключом или балансом, пока не исправите причину.",
    status: "Статус", meaning: "Значение", action: "Что делать", e401: "Ключ отсутствует, неверен или отозван", a401: "Проверьте x-api-key или создайте новый ключ.",
    e402: "Недостаточно доступного баланса", a402: "Пополните аккаунт и повторите запрос.", e429: "Лимит запросов или мощности провайдера", a429: "Повторите запрос с задержкой и случайным смещением.",
    e5xx: "Временная ошибка шлюза или провайдера", a5xx: "Безопасно повторите с экспоненциальной задержкой.", copy: "Копировать", copied: "Скопировано",
    production: "Чек-лист для production", checklist: ["Храните API-ключи только на сервере", "Используйте таймауты запросов", "Повторяйте 429 и 5xx с задержкой", "Логируйте ID запросов, но не секреты", "Следите за балансом и журналом операций"],
    footer: "Документация apiToken.sale · Anthropic-совместимый доступ к Claude",
  },
} as const;

export function DocsPortal() {
  const { language, setLanguage } = useI18n();
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
  const withKey = (code: string) => activeKey ? code.replaceAll("sk-pool-•••", activeKey).replaceAll("$APITOKEN_API_KEY", activeKey) : code;
  return <div className="docs-site">
    <header className="docs-header"><Link className="docs-brand" href="/"><BrandMark /><span>apiToken.sale</span><i>{t.documentation}</i></Link><div className="docs-header-actions"><div className="lang"><button className={language === "en" ? "active" : ""} onClick={() => setLanguage("en")}>EN</button><button className={language === "ru" ? "active" : ""} onClick={() => setLanguage("ru")}>RU</button></div><ThemeToggle /><Link className="btn btn-ghost btn-sm docs-back" href="/">{t.back}</Link><Link className="btn btn-primary btn-sm" href="/dashboard">{t.dashboard}</Link></div></header>
    <div className="docs-layout">
      <aside className="docs-sidebar"><span>{t.onThisPage}</span><nav><a href="#overview">{t.overview}</a><a href="#quickstart">{t.quickstart}</a><a href="#authentication">{t.authentication}</a><a href="#tools">{t.tools}</a><a href="#sdks">{t.sdks}</a><a href="#errors">{t.errors}</a><a href="#pricing">{t.pricing}</a></nav></aside>
      <main className="docs-main">
        <section className="docs-hero" id="overview"><span className="eyebrow">{t.eyebrow}</span><h1>{t.title}</h1><p>{t.lead}</p><Link className="btn btn-primary" href="/dashboard?view=keys">{t.openKeys}</Link></section>
        <section className="docs-section"><div className="docs-section-heading"><span>01</span><div><h2>{t.oneEndpoint}</h2><p>{t.oneEndpointText}</p></div></div>
          <div className="docs-notice" style={{ marginTop: 0, marginBottom: 14, display: "grid", gap: 12 }}>
            <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 12, flexWrap: "wrap" }}><div><label htmlFor="docs-api-key" style={{ display: "block", color: "var(--txt)", fontWeight: 700 }}>{t.apiKeyLabel}</label><span id="docs-api-key-help" style={{ display: "block", marginTop: 3, color: "var(--txt-3)" }}>{t.apiKeyHelp}</span></div>{activeKey && <span role="status" aria-live="polite" style={{ color: "var(--accent)", fontSize: 11 }}>{t.apiKeyActive} <b>••••{activeKey.slice(-4)}</b></span>}</div>
            <form onSubmit={(event) => { event.preventDefault(); applyKey(); }} style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}><input id="docs-api-key" type="text" value={keyInput} onChange={(event) => setKeyInput(event.target.value)} placeholder="sk-pool-•••" aria-describedby="docs-api-key-help" autoComplete="off" autoCapitalize="none" autoCorrect="off" spellCheck={false} style={{ flex: "1 1 280px", minWidth: 0, padding: "10px 12px", border: "1px solid var(--line-strong)", borderRadius: 6, background: "var(--bg-card)", color: "var(--txt)", fontFamily: "var(--font-mono)", fontSize: 12 }} /><button className="btn btn-primary btn-sm" type="submit">{t.apiKeyApply}</button><button className="btn btn-ghost btn-sm" type="button" onClick={clearKey}>{t.apiKeyClear}</button></form>
          </div>
          <div className="docs-essential-grid"><Endpoint label={t.baseUrl} value={BASE_URL} copyLabel={t.copy} copiedLabel={t.copied} /><Endpoint label={t.messagesEndpoint} value={MESSAGES_URL} copyLabel={t.copy} copiedLabel={t.copied} /><Endpoint label={t.authHeader} value={withKey("x-api-key: sk-pool-•••")} copyLabel={t.copy} copiedLabel={t.copied} /></div><div className="docs-notice">{t.keyNotice}</div></section>
        <section className="docs-section" id="quickstart"><div className="docs-section-heading"><span>02</span><div><h2>{t.quickstart}</h2><p>{t.requestText}</p></div></div><CodeBlock title={t.requestTitle} code={withKey(CURL)} copyLabel={t.copy} copiedLabel={t.copied} /><CodeBlock title={t.envTitle} description={t.envText} code={withKey(ENVIRONMENT)} copyLabel={t.copy} copiedLabel={t.copied} /></section>
        <section className="docs-section" id="authentication"><div className="docs-section-heading"><span>03</span><div><h2>{t.authentication}</h2><p>{t.authText}</p></div></div><div className="docs-auth-flow"><div><b>1</b><code>x-api-key</code></div><span>＋</span><div><b>2</b><code>anthropic-version</code></div><span>→</span><div><b>API</b><code>/v1/messages</code></div></div></section>
        <section className="docs-section" id="tools"><div className="docs-section-heading"><span>04</span><div><h2>{t.tools}</h2><p>{t.oneEndpointText}</p></div></div><div className="docs-two-col"><CodeBlock title={t.claudeCode} description={t.claudeCodeText} code={withKey(CLAUDE_CODE)} copyLabel={t.copy} copiedLabel={t.copied} /><CodeBlock title={t.editors} description={t.editorsText} code={withKey(CURSOR)} copyLabel={t.copy} copiedLabel={t.copied} /></div></section>
        <section className="docs-section" id="sdks"><div className="docs-section-heading"><span>05</span><div><h2>{t.sdks}</h2><p>{t.oneEndpointText}</p></div></div><div className="docs-two-col"><CodeBlock title={t.python} description={t.pythonText} code={withKey(PYTHON)} copyLabel={t.copy} copiedLabel={t.copied} /><CodeBlock title={t.typescript} description={t.typescriptText} code={withKey(TYPESCRIPT)} copyLabel={t.copy} copiedLabel={t.copied} /></div></section>
        <section className="docs-section" id="errors"><div className="docs-section-heading"><span>06</span><div><h2>{t.errorTitle}</h2><p>{t.errorText}</p></div></div><div className="table-scroll"><table className="mtable docs-errors"><thead><tr><th>{t.status}</th><th>{t.meaning}</th><th>{t.action}</th></tr></thead><tbody><ErrorRow code="401" meaning={t.e401} action={t.a401} labels={t} /><ErrorRow code="402" meaning={t.e402} action={t.a402} labels={t} /><ErrorRow code="429" meaning={t.e429} action={t.a429} labels={t} /><ErrorRow code="5xx" meaning={t.e5xx} action={t.a5xx} labels={t} /></tbody></table></div><div className="docs-checklist"><h3>{t.production}</h3><ul>{t.checklist.map((item) => <li key={item}>{item}</li>)}</ul></div></section>
        <section className="docs-section" id="pricing"><div className="docs-section-heading"><span>07</span><div><h2>{t.pricingTitle}</h2><p>{t.pricingLead}</p></div></div>
          <h3 className="docs-h3">{t.billingHead}</h3><p className="docs-para">{t.billingText}</p><div className="docs-notice">{t.billingExample}</div>
          <h3 className="docs-h3">{t.tiersHead}</h3><p className="docs-para">{t.tiersText}</p>
          <div className="table-scroll"><table className="mtable docs-errors"><thead><tr><th>{t.tierColTier}</th><th>{t.tierColDiscount}</th><th>{t.tierColValue}</th><th>{t.tierColGet}</th><th>{t.tierColKeep}</th></tr></thead>
            <tbody>{B2C_PRICING_MILESTONES.map((tier) => <tr key={tier.code}><td><b>{tier.label}</b></td><td>{tier.discountPercent}%</td><td>×{(100 / (100 - tier.discountPercent)).toLocaleString("en-US", { maximumFractionDigits: 2 })}</td><td>{Number(tier.platformSpendUsd) === 0 ? t.tierBaseCell : formatWholeUsd(tier.platformSpendUsd)}</td><td>{Number(tier.holdUsd) === 0 ? "—" : `${formatWholeUsd(tier.holdUsd)} / 30d`}</td></tr>)}</tbody>
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
  return <div className="docs-endpoint"><span>{label}</span><code>{value}</code><CopyControl value={value} label={copyLabel} copiedLabel={copiedLabel} /></div>;
}

function CodeBlock({ title, description, code, copyLabel, copiedLabel }: { title: string; description?: string; code: string; copyLabel: string; copiedLabel: string }) {
  return <article className="docs-code-card"><header><div><h3>{title}</h3>{description && <p>{description}</p>}</div><CopyControl value={code} label={copyLabel} copiedLabel={copiedLabel} /></header><pre><code>{code}</code></pre></article>;
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
