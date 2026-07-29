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
    claudeTitle: "Claude / Anthropic API",
    openaiTitle: "GPT / OpenAI-совместимый API",
    claudeText: "Claude Code, Anthropic SDK, Cursor и прямые запросы Messages API.",
    openaiText: "Codex CLI, OpenAI SDK, Responses API и Chat Completions.",
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
    claudeTitle: "Claude / Anthropic API",
    openaiTitle: "GPT / OpenAI-compatible API",
    claudeText: "Claude Code, Anthropic SDKs, Cursor, and direct Messages API requests.",
    openaiText: "Codex CLI, OpenAI SDKs, Responses API, and Chat Completions.",
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
  const { language, setLanguage } = useLanguage();
  const t = copy[language];
  const cards = [
    { connection: UNIVERSAL_CONNECTIONS.claude, title: t.claudeTitle, text: t.claudeText, accent: "Claude" },
    { connection: UNIVERSAL_CONNECTIONS.openai, title: t.openaiTitle, text: t.openaiText, accent: "GPT" },
  ];

  return (
    <AppShell
      section="docs"
      title={t.titleBar}
      actions={
        <div className="lang" role="group" aria-label={language === "ru" ? "Язык" : "Language"}>
          <button type="button" className={language === "en" ? "active" : ""} aria-pressed={language === "en"} onClick={() => setLanguage("en")}>EN</button>
          <button type="button" className={language === "ru" ? "active" : ""} aria-pressed={language === "ru"} onClick={() => setLanguage("ru")}>RU</button>
        </div>
      }
    >
      <div className="app-body">
        <div className="app-body-in docs-hub">
          <div className="page-heading docs-hub-heading">
            <span className="eyebrow">{t.eyebrow}</span>
            <h1 className="p-h1">{t.title}</h1>
            <p className="p-sub">{t.lead}</p>
          </div>

          <div className="docs-hub-grid">
            {cards.map(({ connection, title, text, accent }) => (
              <article className="card docs-hub-card" key={connection.docsPath}>
                <div className="docs-hub-card-head">
                  <span className="docs-hub-monogram">{accent.slice(0, 1)}</span>
                  <div><span className="chip">{accent}</span><h2>{title}</h2></div>
                </div>
                <p>{text}</p>
                <dl>
                  <div><dt>{t.baseUrl}</dt><dd><code>{connection.baseUrl}</code></dd></div>
                  <div><dt>{t.auth}</dt><dd><code>{connection.authHeader}</code></dd></div>
                </dl>
                <Link className="btn btn-primary" href={connection.docsPath}>{t.openGuide}</Link>
              </article>
            ))}
          </div>

          <section className="card docs-hub-shared">
            <div><span className="eyebrow">USAGE</span><h2>{t.sharedTitle}</h2><p>{t.note}</p></div>
            <ul>{t.shared.map((item) => <li key={item}>{item}</li>)}</ul>
            <Link className="btn btn-ghost" href="/profile">{t.usage}</Link>
          </section>
        </div>
      </div>
    </AppShell>
  );
}
