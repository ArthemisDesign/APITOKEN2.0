"use client";

import Link from "next/link";
import { AppShell } from "@/components/app-shell";
import { useLanguage } from "@/components/chrome";
import { UNIVERSAL_CONNECTIONS } from "@/lib/universal-key";

const copy = {
  ru: {
    titleBar: "Как подключить",
    eyebrow: "УНИВЕРСАЛЬНЫЙ SK-POOL КЛЮЧ",
    title: "Один ключ — два API",
    lead: "Выберите формат клиента. Один и тот же ключ работает с Claude и GPT, расходует общий баланс и отображается на одной странице USAGE.",
    heroKey: "ОДИН УНИВЕРСАЛЬНЫЙ КЛЮЧ",
    heroBalance: "общий баланс",
    heroUsage: "живой USAGE",
    choose: "Выберите протокол клиента",
    chooseText: "Ключ менять не нужно — отличаются только Base URL, заголовок авторизации и формат запроса.",
    claudeTitle: "Claude / Anthropic API",
    openaiTitle: "GPT / OpenAI-совместимый API",
    claudeText: "Claude Code, Anthropic SDK, Cursor и прямые запросы Messages API.",
    openaiText: "Codex CLI, OpenAI SDK, Responses API и Chat Completions.",
    claudeFits: ["Claude Code", "Anthropic SDK", "Messages API"],
    openaiFits: ["Codex CLI", "OpenAI SDK", "Responses API"],
    openGuide: "Открыть инструкцию",
    baseUrl: "Base URL",
    auth: "Авторизация",
    sharedTitle: "Что остаётся общим",
    shared: [
      "тот же ключ sk-pool — второй выпуск не нужен",
      "единый предоплаченный баланс для всех моделей",
      "одна персональная ссылка на остаток и расход",
      "автоматическое обновление USAGE каждые 6 секунд",
    ],
    usage: "Открыть USAGE",
    note: "Используйте Base URL и канонический заголовок именно из выбранной инструкции. Не добавляйте /v1 к Claude URL и не убирайте /v1 из GPT URL.",
  },
  en: {
    titleBar: "Connection guides",
    eyebrow: "UNIVERSAL SK-POOL KEY",
    title: "One key, two APIs",
    lead: "Choose your client's API format. The same key works with Claude and GPT, spends one shared balance, and appears on one USAGE page.",
    heroKey: "ONE UNIVERSAL KEY",
    heroBalance: "shared balance",
    heroUsage: "live USAGE",
    choose: "Choose your client protocol",
    chooseText: "The key stays the same — only the Base URL, authentication header, and request format change.",
    claudeTitle: "Claude / Anthropic API",
    openaiTitle: "GPT / OpenAI-compatible API",
    claudeText: "Claude Code, Anthropic SDKs, Cursor, and direct Messages API requests.",
    openaiText: "Codex CLI, OpenAI SDKs, Responses API, and Chat Completions.",
    claudeFits: ["Claude Code", "Anthropic SDK", "Messages API"],
    openaiFits: ["Codex CLI", "OpenAI SDK", "Responses API"],
    openGuide: "Open guide",
    baseUrl: "Base URL",
    auth: "Authentication",
    sharedTitle: "What stays shared",
    shared: [
      "the same sk-pool key — no second issuance",
      "one prepaid balance across all models",
      "one personal link for balance and usage",
      "automatic USAGE refresh every 6 seconds",
    ],
    usage: "Open USAGE",
    note: "Use the Base URL and canonical header from the selected guide. Do not append /v1 to the Claude URL or remove /v1 from the GPT URL.",
  },
} as const;

export function ConnectionDocsHub() {
  const { language } = useLanguage();
  const t = copy[language];
  const cards = [
    { connection: UNIVERSAL_CONNECTIONS.claude, title: t.claudeTitle, text: t.claudeText, fits: t.claudeFits, accent: "Claude", number: "01", kind: "claude" },
    { connection: UNIVERSAL_CONNECTIONS.openai, title: t.openaiTitle, text: t.openaiText, fits: t.openaiFits, accent: "GPT", number: "02", kind: "openai" },
  ];

  return (
    <AppShell section="docs" title={t.titleBar}>
      <div className="app-body">
        <div className="app-body-in docs-hub">
          <section className="docs-hub-hero">
            <div className="docs-hub-hero-copy">
              <span className="eyebrow">{t.eyebrow}</span>
              <h1>{t.title}</h1>
              <p>{t.lead}</p>
              <div className="docs-hub-hero-actions">
                <Link className="btn btn-primary" href="/profile">{t.usage}</Link>
                <span><i />{t.heroBalance}</span><span><i />{t.heroUsage}</span>
              </div>
            </div>
            <div className="docs-hub-router" aria-label={t.title}>
              <div className="docs-hub-key-node">
                <span>{t.heroKey}</span>
                <code>sk-pool-••••••••</code>
              </div>
              <div className="docs-hub-route-lines" aria-hidden="true"><i /><i /></div>
              <div className="docs-hub-route-targets">
                <div className="claude"><b>Claude</b><code>Messages API</code></div>
                <div className="openai"><b>GPT</b><code>Responses API</code></div>
              </div>
              <div className="docs-hub-shared-balance"><span>{t.sharedTitle}</span><b>$</b></div>
            </div>
          </section>

          <div className="docs-hub-section-head">
            <div><span>01 / 02</span><h2>{t.choose}</h2></div>
            <p>{t.chooseText}</p>
          </div>

          <div className="docs-hub-grid">
            {cards.map(({ connection, title, text, fits, accent, number, kind }) => (
              <article className={`card docs-hub-card docs-hub-card-${kind}`} key={connection.docsPath}>
                <div className="docs-hub-card-head">
                  <div className="docs-hub-card-id"><span>{number}</span><b>{accent}</b></div>
                  <span className="docs-hub-arrow" aria-hidden="true">↗</span>
                </div>
                <h2>{title}</h2>
                <p>{text}</p>
                <div className="docs-hub-fit-list">{fits.map((fit) => <span key={fit}>{fit}</span>)}</div>
                <div className="docs-hub-connection">
                  <div><span>{t.baseUrl}</span><code>{connection.baseUrl}</code></div>
                  <div><span>{t.auth}</span><code>{connection.authHeader}</code></div>
                </div>
                <Link className="docs-hub-card-link" href={connection.docsPath}><span>{t.openGuide}</span><b aria-hidden="true">→</b></Link>
              </article>
            ))}
          </div>

          <section className="docs-hub-shared">
            <div className="docs-hub-shared-copy"><span className="eyebrow">USAGE</span><h2>{t.sharedTitle}</h2><p>{t.note}</p></div>
            <div className="docs-hub-shared-items">{t.shared.map((item, index) => <div key={item}><b>0{index + 1}</b><span>{item}</span></div>)}</div>
            <Link className="btn btn-primary" href="/profile">{t.usage}<span aria-hidden="true">→</span></Link>
          </section>
        </div>
      </div>
    </AppShell>
  );
}
