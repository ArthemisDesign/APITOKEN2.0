"use client";

import { AppShell } from "@/components/app-shell";
import { useLanguage } from "@/components/chrome";

const SUPPORT_TELEGRAM_HANDLE = "@apitokensupportbot";
const SUPPORT_TELEGRAM_URL = "https://t.me/apitokensupportbot";
const SUPPORT_EMAIL = "apitokensale@gmail.com";

const copy = {
  en: {
    titleBar: "Support",
    eyebrow: "CUSTOMER SUPPORT",
    title: "Help with your openKeys key",
    lead: "Message our AI assistant in Telegram. It answers instantly, and a real person joins the same chat when your case needs human attention.",
    poweredBy: "AI first line · Claude Sonnet 5",
    botName: "apiToken Support",
    botDescription: "Instant help with connection, usage, balance, and request errors.",
    cta: "Open in Telegram",
    handleLabel: "Support bot",
    availability: "AI 24/7 · humans 08:00–12:00 UTC",
    howTitle: "How it works",
    hours: "Humans · 08:00–12:00 UTC",
    steps: [
      { title: "Message the bot", text: "Describe the problem in plain words." },
      { title: "AI answers", text: "Get help in seconds, at any time." },
      { title: "Human if needed", text: "Money and account cases are escalated." },
    ],
    topicsTitle: "What it handles",
    topics: ["Connect Claude", "Connect GPT / OpenAI", "Request errors", "Key usage", "Balance", "Model IDs"],
    security: "Never send passwords, full API keys, or card details — support will not ask for them.",
    emailKicker: "Prefer email?",
    official: "The only official support bot is @apitokensupportbot.",
  },
  ru: {
    titleBar: "Поддержка",
    eyebrow: "ПОДДЕРЖКА КЛИЕНТОВ",
    title: "Помощь с ключом openKeys",
    lead: "Напишите нашему ИИ-ассистенту в Telegram. Он отвечает сразу, а если нужен ручной разбор, в тот же чат подключается человек.",
    poweredBy: "ИИ первой линии · Claude Sonnet 5",
    botName: "apiToken Support",
    botDescription: "Мгновенная помощь с подключением, расходом, балансом и ошибками запросов.",
    cta: "Открыть в Telegram",
    handleLabel: "Бот поддержки",
    availability: "ИИ 24/7 · люди 08:00–12:00 UTC",
    howTitle: "Как это работает",
    hours: "Люди · 08:00–12:00 UTC",
    steps: [
      { title: "Напишите боту", text: "Опишите проблему простыми словами." },
      { title: "ИИ отвечает", text: "Помощь за секунды в любое время." },
      { title: "Человек при необходимости", text: "Вопросы по деньгам и аккаунту передаются оператору." },
    ],
    topicsTitle: "С чем помогает",
    topics: ["Подключение Claude", "Подключение GPT / OpenAI", "Ошибки запросов", "Расход ключа", "Баланс", "ID моделей"],
    security: "Не отправляйте пароли, полные API-ключи и данные карт — поддержка их не спрашивает.",
    emailKicker: "Удобнее почтой?",
    official: "Единственный официальный бот — @apitokensupportbot.",
  },
} as const;

const STEP_ICONS = [
  <svg key="message" viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round"><path d="M21 11.5a8.4 8.4 0 0 1-9 8.4 9.4 9.4 0 0 1-3.4-.6L3 21l1.3-4a8.2 8.2 0 0 1-1-4A8.4 8.4 0 0 1 12 4a8.4 8.4 0 0 1 9 7.5Z" /></svg>,
  <svg key="ai" viewBox="0 0 24 24" width="20" height="20" fill="currentColor"><path d="M12 2.5l1.7 4.6 4.6 1.7-4.6 1.7L12 15.1l-1.7-4.6L5.7 8.8l4.6-1.7L12 2.5Z" /><path d="M18.7 14l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8.8-2.2Z" opacity=".55" /></svg>,
  <svg key="human" viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="8" r="3.4" /><path d="M5 20a7 7 0 0 1 14 0" /></svg>,
];

export function SupportPortal() {
  const { language } = useLanguage();
  const t = copy[language];

  return (
    <AppShell section="support" title={t.titleBar}>
      <div className="app-body">
        <div className="app-body-in support-portal">
          <div className="page-heading">
            <span className="eyebrow">{t.eyebrow}</span>
            <h1 className="p-h1">{t.title}</h1>
            <p className="p-sub">{t.lead}</p>
          </div>

          <div className="support-bot">
            <div className="support-bot-glow" aria-hidden="true" />
            <div className="support-bot-main">
              <div className="support-bot-head">
                <span className="support-bot-av" aria-hidden="true">
                  <svg viewBox="0 0 24 24" width="26" height="26" fill="currentColor"><path d="M22 2 2.5 10.6c-.9.4-.9 1.6.1 1.9l4.6 1.4 1.8 5.6c.3.9 1.4 1.1 2 .4l2.5-2.8 4.7 3.4c.8.6 2 .1 2.2-.9L23.9 3.3C24.1 2.3 23 1.5 22 2ZM9 13.6l8.3-5.7-6.4 6.9-.1 3.4L9 13.6Z" /></svg>
                </span>
                <div><span className="support-bot-tag">{t.poweredBy}</span><h2>{t.botName}</h2></div>
              </div>
              <p className="support-bot-desc">{t.botDescription}</p>
              <div className="support-bot-actions">
                <a className="btn btn-primary support-bot-cta" href={SUPPORT_TELEGRAM_URL} target="_blank" rel="noreferrer">{t.cta}</a>
                <a className="support-bot-handle" href={SUPPORT_TELEGRAM_URL} target="_blank" rel="noreferrer"><span>{t.handleLabel}</span>{SUPPORT_TELEGRAM_HANDLE}</a>
              </div>
              <span className="support-bot-avail"><i className="support-bot-dot" aria-hidden="true" />{t.availability}</span>
            </div>
          </div>

          <div className="support-how">
            <div className="support-how-head"><h2 className="support-how-title">{t.howTitle}</h2><span className="support-hours"><i className="support-bot-dot" aria-hidden="true" />{t.hours}</span></div>
            <div className="support-flow">
              {t.steps.map((step, index) => <article className="support-step" key={step.title}>
                <div className="support-step-top"><span className="support-step-ic" aria-hidden="true">{STEP_ICONS[index]}</span><span className="n">{String(index + 1).padStart(2, "0")}</span></div>
                <h3>{step.title}</h3><p>{step.text}</p>
              </article>)}
            </div>
          </div>

          <div className="support-topics"><h2 className="support-topics-title">{t.topicsTitle}</h2><ul className="support-chips">{t.topics.map((topic) => <li key={topic}>{topic}</li>)}</ul></div>
          <div className="support-notes support-notes-single"><div className="support-note"><span className="support-note-ic" aria-hidden="true">✓</span><p>{t.security}</p></div></div>
          <div className="support-footline">
            <a className="support-footmail" href={`mailto:${SUPPORT_EMAIL}`}>{t.emailKicker} <b>{SUPPORT_EMAIL}</b></a>
            <span className="support-footnote">{t.official}</span>
          </div>
        </div>
      </div>
    </AppShell>
  );
}
