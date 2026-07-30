"use client";

// Раздел «Providers» (web/v2): карточки двух провайдеров пула — Anthropic (байт-в-байт
// Messages API) и OpenAI (совместимый API). Кнопка Quick Start открывает выезжающую справа
// панель с пошаговой инструкцией (в стиле OpenRouter): ключ → первый запрос на нескольких
// языках → стриминг → справочник эндпоинтов. Все команды и заголовки совпадают с /docs и
// lib/claude-connection.ts — единственный источник правды по подключению.
import { useEffect, useMemo, useState } from "react";
import { ANTHROPIC_BASE_URL, OPENAI_BASE_URL, claudeModels, formatUsd, openaiModels, priceFrom, type CatalogModel } from "@/lib/models";
import { CLAUDE_DEFAULT_MODEL, OPENAI_DEFAULT_MODEL } from "@/lib/claude-connection";
import { FLAT_DISCOUNT_PERCENT } from "@/lib/pricing-tiers";
import { useI18n } from "@/components/i18n-provider";
import { dashboardCopy, type DashboardCopy } from "@/lib/dashboard-copy";
import { CopyButton, PageHeading } from "./dashboard-sections";

type ProviderKey = "anthropic" | "openai";

// Реальные даты релизов моделей у провайдеров (официальные анонсы Anthropic/OpenAI).
const RELEASED: Record<string, string> = {
  "claude-opus-4-8": "2026-05-28",
  "claude-opus-4-7": "2026-04-16",
  "claude-sonnet-5": "2026-06-30",
  "claude-sonnet-4-6": "2026-02-17",
  "claude-haiku-4-5": "2025-10-15",
  "gpt-5.6-sol": "2026-07-09",
  "gpt-5.6-terra": "2026-07-09",
  "gpt-5.6-luna": "2026-07-09",
  "gpt-5.5": "2026-04-23",
  "gpt-5.4": "2026-03-05",
};

type ProviderCard = {
  key: ProviderKey;
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

// --- Сниппеты Quick Start (канон: /docs + lib/claude-connection.ts) ---

const ANTHROPIC_SNIPPETS = {
  env: `export APITOKEN_API_KEY=sk-pool-...`,
  curl: `curl ${ANTHROPIC_BASE_URL}/v1/messages \\
  -H "x-api-key: $APITOKEN_API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{
    "model": "${CLAUDE_DEFAULT_MODEL}",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'`,
  python: `import os
from anthropic import Anthropic

client = Anthropic(
    base_url="${ANTHROPIC_BASE_URL}",
    api_key=os.environ["APITOKEN_API_KEY"],
)

message = client.messages.create(
    model="${CLAUDE_DEFAULT_MODEL}",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello"}],
)
print(message.content[0].text)`,
  typescript: `import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic({
  baseURL: "${ANTHROPIC_BASE_URL}",
  apiKey: process.env.APITOKEN_API_KEY,
});

const message = await client.messages.create({
  model: "${CLAUDE_DEFAULT_MODEL}",
  max_tokens: 1024,
  messages: [{ role: "user", content: "Hello" }],
});
console.log(message.content[0]);`,
  stream: `curl -N ${ANTHROPIC_BASE_URL}/v1/messages \\
  -H "x-api-key: $APITOKEN_API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{
    "model": "${CLAUDE_DEFAULT_MODEL}",
    "max_tokens": 1024,
    "stream": true,
    "messages": [{"role": "user", "content": "Hello"}]
  }'`,
};

const OPENAI_SNIPPETS = {
  env: `export APITOKEN_API_KEY=sk-pool-...`,
  curl: `curl ${OPENAI_BASE_URL}/chat/completions \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "content-type: application/json" \\
  -d '{
    "model": "${OPENAI_DEFAULT_MODEL}",
    "messages": [{"role": "user", "content": "Hello"}]
  }'`,
  python: `import os
from openai import OpenAI

client = OpenAI(
    base_url="${OPENAI_BASE_URL}",
    api_key=os.environ["APITOKEN_API_KEY"],
)

response = client.responses.create(
    model="${OPENAI_DEFAULT_MODEL}",
    input="Hello",
)
print(response.output_text)`,
  typescript: `import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "${OPENAI_BASE_URL}",
  apiKey: process.env.APITOKEN_API_KEY,
});

const response = await client.responses.create({
  model: "${OPENAI_DEFAULT_MODEL}",
  input: "Hello",
});
console.log(response.output_text);`,
  stream: `curl -N ${OPENAI_BASE_URL}/chat/completions \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "content-type: application/json" \\
  -d '{
    "model": "${OPENAI_DEFAULT_MODEL}",
    "stream": true,
    "messages": [{"role": "user", "content": "Hello"}]
  }'`,
};

type EndpointRef = { method: string; url: string; rows: Array<[string, string]>; noteKey: keyof DashboardCopy };

function endpointRefs(provider: ProviderKey, copy: DashboardCopy): EndpointRef[] {
  if (provider === "anthropic") {
    return [{
      method: "POST",
      url: `${ANTHROPIC_BASE_URL}/v1/messages`,
      noteKey: "qsAnthropicEndpointNote",
      rows: [
        ["x-api-key", "sk-pool-•••"],
        ["anthropic-version", "2023-06-01"],
        ["content-type", "application/json"],
        [copy.qsModelRow, `${CLAUDE_DEFAULT_MODEL} · ${copy.qsAnyClaude}`],
      ],
    }];
  }
  return [
    {
      method: "POST",
      url: `${OPENAI_BASE_URL}/chat/completions`,
      noteKey: "qsOpenaiChatNote",
      rows: [
        ["Authorization", "Bearer sk-pool-•••"],
        ["content-type", "application/json"],
        [copy.qsModelRow, `${OPENAI_DEFAULT_MODEL} · ${copy.qsAnyGpt}`],
      ],
    },
    {
      method: "POST",
      url: `${OPENAI_BASE_URL}/responses`,
      noteKey: "qsOpenaiResponsesNote",
      rows: [
        ["Authorization", "Bearer sk-pool-•••"],
        ["content-type", "application/json"],
        [copy.qsModelRow, `${OPENAI_DEFAULT_MODEL} · ${copy.qsAnyGpt}`],
      ],
    },
  ];
}

// --- Лёгкая подсветка синтаксиса (без зависимостей) для статических сниппетов панели ---
type SnippetLang = "bash" | "python" | "typescript";

const LANG_KEYWORDS: Record<SnippetLang, string[]> = {
  bash: ["curl", "export"],
  python: ["import", "from", "print", "def", "return", "class", "True", "False", "None"],
  typescript: ["import", "from", "const", "let", "new", "await", "async", "export", "return", "true", "false"],
};

function highlight(code: string, lang: SnippetLang): React.ReactNode[] {
  const keywords = LANG_KEYWORDS[lang].join("|");
  const comment = lang === "typescript" ? "\\/\\/[^\\n]*" : "#[^\\n]*";
  // Порядок альтернатив = приоритет: комментарий → строка → env-переменная → ключевое слово →
  // ТипСБольшойБуквы → число. Остальное — обычный текст.
  const pattern = new RegExp(
    `(?<com>${comment})|(?<str>"(?:[^"\\\\\\n]|\\\\.)*"|'(?:[^'\\\\\\n]|\\\\.)*')|(?<envvar>\\$[A-Z_][A-Z0-9_]*)|(?<kw>\\b(?:${keywords})\\b)|(?<type>\\b[A-Z][A-Za-z0-9_]+\\b)|(?<num>\\b\\d+\\b)`,
    "g",
  );
  const nodes: React.ReactNode[] = [];
  let last = 0;
  for (const match of code.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > last) nodes.push(code.slice(last, index));
    const groups = match.groups ?? {};
    const cls = groups.com ? "tok-com" : groups.str ? "tok-str" : groups.envvar ? "tok-var" : groups.kw ? "tok-kw" : groups.type ? "tok-type" : "tok-num";
    nodes.push(<span key={`${index}-${cls}`} className={cls}>{match[0]}</span>);
    last = index + match[0].length;
  }
  if (last < code.length) nodes.push(code.slice(last));
  return nodes;
}

function CodeBlock({ code, lang, copyLabel, copiedLabel }: { code: string; lang: SnippetLang; copyLabel: string; copiedLabel: string }) {
  return <div className="qs-code">
    <CopyButton value={code} className="qs-code-copy" label={copyLabel} copiedLabel={copiedLabel} />
    <pre><code>{highlight(code, lang)}</code></pre>
  </div>;
}

const REQUEST_TABS = ["cURL", "Python", "TypeScript"] as const;
type RequestTab = typeof REQUEST_TABS[number];

function QuickStartDrawer({ provider, onClose, onCreateKey }: { provider: ProviderCard; onClose(): void; onCreateKey(): void }) {
  const { language } = useI18n();
  const copy = dashboardCopy[language];
  const [tab, setTab] = useState<RequestTab>("cURL");
  const snippets = provider.key === "anthropic" ? ANTHROPIC_SNIPPETS : OPENAI_SNIPPETS;
  const requestCode = tab === "cURL" ? snippets.curl : tab === "Python" ? snippets.python : snippets.typescript;
  const requestLang: SnippetLang = tab === "cURL" ? "bash" : tab === "Python" ? "python" : "typescript";

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    document.body.style.overflow = "hidden";
    return () => { window.removeEventListener("keydown", onKey); document.body.style.overflow = ""; };
  }, [onClose]);

  return <div className="qs-overlay" onClick={onClose}>
    <aside className="qs-drawer" role="dialog" aria-modal="true" aria-label={`${provider.name} — ${copy.qsTitle}`} onClick={(event) => event.stopPropagation()}>
      <header className="qs-head">
        <div>
          <h2>{copy.qsTitle}</h2>
          <p>{provider.key === "anthropic" ? copy.qsAnthropicSubtitle : copy.qsOpenaiSubtitle}</p>
        </div>
        <button className="qs-close" onClick={onClose} aria-label={copy.closeMenu}>×</button>
      </header>
      <div className="qs-body">
        <section className="qs-step">
          <div className="qs-step-head"><span className="qs-step-num">1</span><h3>{copy.qsStep1Title}</h3></div>
          <p>{copy.qsStep1Text}</p>
          <button className="btn btn-primary btn-sm qs-key-btn" onClick={onCreateKey}>+ {copy.getKey}</button>
          <CodeBlock code={snippets.env} lang="bash" copyLabel={copy.copy} copiedLabel={copy.copied} />
        </section>

        <section className="qs-step">
          <div className="qs-step-head"><span className="qs-step-num">2</span><h3>{copy.qsStep2Title}</h3></div>
          <p>{provider.key === "anthropic" ? copy.qsStep2AnthropicText : copy.qsStep2OpenaiText}</p>
          <div className="chart-toggle qs-tabs" role="tablist">
            {REQUEST_TABS.map((name) => <button key={name} type="button" role="tab" aria-selected={tab === name} className={tab === name ? "on" : ""} onClick={() => setTab(name)}>{name}</button>)}
          </div>
          <CodeBlock code={requestCode} lang={requestLang} copyLabel={copy.copy} copiedLabel={copy.copied} />
        </section>

        <section className="qs-step">
          <div className="qs-step-head"><span className="qs-step-num">3</span><h3>{copy.qsStep3Title}</h3></div>
          <p>{copy.qsStep3Text}</p>
          <CodeBlock code={snippets.stream} lang="bash" copyLabel={copy.copy} copiedLabel={copy.copied} />
        </section>

        <section className="qs-endpoints">
          <h3>{copy.qsEndpointsTitle}</h3>
          {endpointRefs(provider.key, copy).map((ref) => <div key={ref.url} className="qs-endpoint">
            <p className="qs-endpoint-note">{copy[ref.noteKey]}</p>
            <div className="qs-endpoint-url"><b>{ref.method}</b><code>{ref.url}</code><CopyButton value={ref.url} className="qs-code-copy-inline" label={copy.copy} copiedLabel={copy.copied} /></div>
            <dl className="qs-endpoint-rows">
              {ref.rows.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}
            </dl>
          </div>)}
        </section>
      </div>
    </aside>
  </div>;
}

// Кэш-ставки в одну колонку для обеих линеек: Claude — cacheRead/cacheWrite5m,
// GPT — cachedInput (аналог чтения) / cacheWrite.
function cacheRates(model: CatalogModel): { read: number; write: number } {
  return model.provider === "anthropic"
    ? { read: model.cacheReadPerM, write: model.cacheWrite5mPerM }
    : { read: model.cachedInputPerM, write: model.cacheWritePerM };
}

function PriceCell({ official }: { official: number }) {
  return <td className="tnum"><b>{priceFrom(official)}</b> <s>{formatUsd(official)}</s></td>;
}

function ProviderDetail({ provider, onBack, onQuickstart }: { provider: ProviderCard; onBack(): void; onQuickstart(): void }) {
  const { language } = useI18n();
  const copy = dashboardCopy[language];
  const models: CatalogModel[] = provider.key === "anthropic" ? claudeModels : openaiModels;
  const dateFormat = useMemo(
    () => new Intl.DateTimeFormat(language === "ru" ? "ru-RU" : "en-US", { month: "short", day: "numeric", year: "numeric" }),
    [language],
  );
  return <>
    <button className="link plain-button provider-back" onClick={onBack}>← {copy.providersTitle}</button>
    <div className="provider-detail-head">
      <span className={`model-provider-mark provider-mark-lg ${provider.key}`} aria-hidden="true">{provider.mark}</span>
      <div className="provider-detail-title">
        <h2>{provider.name}</h2>
        <div className="provider-endpoint">
          <code className="model-id">{provider.endpoint}</code>
          <CopyButton value={provider.endpoint} className="model-copy" label={copy.providerCopyEndpoint} copiedLabel={copy.copied} />
        </div>
      </div>
      <button className="btn btn-primary btn-sm qs-open-btn" onClick={onQuickstart}>{copy.qsTitle}</button>
    </div>
    <p className="model-desc provider-detail-desc">{copy[provider.descKey]}</p>
    <div className="provider-chips">
      <div className="provider-chip"><span>{copy.providerChipApi}</span><b>{copy[provider.apiKindKey]}</b></div>
      <div className="provider-chip"><span>{copy.providerChipModels}</span><b>{provider.models}</b></div>
      <div className="provider-chip"><span>{copy.providerChipDiscount}</span><b>−{FLAT_DISCOUNT_PERCENT}%</b></div>
      <div className="provider-chip"><span>{copy.providerChipLineup}</span><b>{provider.lineup}</b></div>
    </div>
    <h3 className="provider-models-title">{copy.providerModelsTitle}</h3>
    <p className="p-sub">{copy.providerModelsHint}</p>
    <div className="table-scroll"><table className="mtable provider-models-table">
      <thead><tr>
        <th>{copy.thModel}</th><th>{copy.thReleased}</th><th>{copy.thContext}</th><th>{copy.thMaxOutput}</th>
        <th className="tnum">{copy.thInput}</th><th className="tnum">{copy.thOutput}</th>
        <th className="tnum">{copy.thCacheRead}</th><th className="tnum">{copy.thCacheWrite}</th>
      </tr></thead>
      <tbody>{models.map((model) => {
        const cache = cacheRates(model);
        const released = RELEASED[model.id];
        return <tr key={model.id}>
          <td>
            <div className="provider-model-name"><b>{model.name}</b><span className="model-tier">{model.tier}</span></div>
            <div className="provider-model-id"><code className="model-id">{model.id}</code><CopyButton value={model.id} className="model-copy" label={copy.copy} copiedLabel={copy.copied} /></div>
          </td>
          <td>{released ? dateFormat.format(new Date(`${released}T00:00:00Z`)) : "—"}</td>
          <td>{model.context.replace(" tokens", "")}</td>
          <td>{model.maxOutput.replace(" tokens", "")}</td>
          <PriceCell official={model.inputPerM} />
          <PriceCell official={model.outputPerM} />
          <PriceCell official={cache.read} />
          <PriceCell official={cache.write} />
        </tr>;
      })}</tbody>
    </table></div>
    <p className="providers-note">{copy.providerPricesNote}</p>
  </>;
}

export function ProvidersCatalog({ onCreateKey }: { onCreateKey?: () => void }) {
  const { language } = useI18n();
  const copy = useMemo(() => dashboardCopy[language], [language]);
  const [quickstart, setQuickstart] = useState<ProviderKey | null>(null);
  const [selected, setSelected] = useState<ProviderKey | null>(null);
  const active = PROVIDERS.find((provider) => provider.key === quickstart) ?? null;
  const detail = PROVIDERS.find((provider) => provider.key === selected) ?? null;

  return <section className="panel">
    {!detail && <>
      <PageHeading eyebrow={copy.providersEyebrow} title={copy.providersTitle} subtitle={copy.providersSubtitle} />
      <div className="models-list">
        {PROVIDERS.map((provider) => <article
          key={provider.key}
          className="card model-card provider-card-clickable"
          role="button"
          tabIndex={0}
          onClick={() => setSelected(provider.key)}
          onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); setSelected(provider.key); } }}
        >
          <div className="model-card-head">
            <span className={`model-provider-mark ${provider.key}`} aria-hidden="true">{provider.mark}</span>
            <h2 className="model-name">{provider.name}</h2>
            <button className="btn btn-primary btn-sm qs-open-btn" onClick={(event) => { event.stopPropagation(); setQuickstart(provider.key); }}>{copy.qsTitle}</button>
            <span className="model-tier">{copy[provider.apiKindKey]}</span>
          </div>
          <p className="model-desc">{copy[provider.descKey]}</p>
          <div className="provider-endpoint" onClick={(event) => event.stopPropagation()}>
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
    </>}
    {detail && <ProviderDetail provider={detail} onBack={() => setSelected(null)} onQuickstart={() => setQuickstart(detail.key)} />}
    {active && <QuickStartDrawer
      provider={active}
      onClose={() => setQuickstart(null)}
      onCreateKey={() => { setQuickstart(null); onCreateKey?.(); }}
    />}
  </section>;
}
