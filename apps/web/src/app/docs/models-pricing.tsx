"use client";

import Link from "next/link";
import { claudeModels, formatUsd, geminiModels, openaiModels, priceHere, type CatalogModel, type ClaudeModel, type GeminiModel, type OpenAiModel } from "@/lib/models";
import { Prose } from "./prose";

type Language = "en" | "ru";
const tr = (language: Language, en: string, ru: string) => (language === "ru" ? ru : en);

// Docs "Models & pricing" section: both providers, every model, official list
// rates vs. the flat 50% discount. All numbers come from the live catalog in
// @/lib/models — the same source as /models and the cost calculator.
export function ModelsPricing({ language }: { language: Language }) {
  return <div className="mp-root">
    <ProviderPanel
      language={language}
      provider="anthropic"
      title={tr(language, "Claude · Anthropic Messages API", "Claude · Anthropic Messages API")}
      host="api.apitoken.sale"
      auth="x-api-key"
      models={claudeModels}
      cacheNote={tr(language,
        "Cache reads bill at the cached-input rate (10% of input). Cache writes: 1.25× input for a 5-minute TTL, 2× input for a 1-hour TTL — send the `anthropic-beta: extended-cache-ttl-2025-04-11` header yourself.",
        "Чтение из кеша тарифицируется по ставке кешированного ввода (10% от ввода). Запись: 1,25× ввода при TTL 5 минут, 2× при TTL 1 час — заголовок `anthropic-beta: extended-cache-ttl-2025-04-11` передайте самостоятельно.")}
    />

    <ProviderPanel
      language={language}
      provider="openai"
      title={tr(language, "GPT · OpenAI-compatible API", "GPT · OpenAI-совместимый API")}
      host="openai.api.apitoken.sale/v1"
      auth="Authorization: Bearer"
      models={openaiModels}
      cacheNote={tr(language,
        "Caching is automatic — repeated prefixes bill at the cached-input rate with no opt-in. `gpt-5.6` is an alias of `gpt-5.6-sol`.",
        "Кеширование автоматическое — повторяющиеся префиксы тарифицируются по ставке кешированного ввода, ничего включать не нужно. `gpt-5.6` — алиас `gpt-5.6-sol`.")}
    />

    <ProviderPanel
      language={language}
      provider="gemini"
      title={tr(language, "Gemini · Google Gemini API", "Gemini · Google Gemini API")}
      host="gemini.api.apitoken.sale"
      auth="x-goog-api-key"
      models={geminiModels}
      cacheNote={tr(language,
        "Cached input bills at the cached-input rate (10% of input). `gemini-3.1-pro-preview` switches to long-context rates above 200K input tokens: 2× input and 1.5× output on the whole request. `gemini-3.1-flash-image` bills image output at $60 per 1M image tokens.",
        "Кешированный ввод тарифицируется по ставке кешированного ввода (10% от ввода). `gemini-3.1-pro-preview` переключается на ставки длинного контекста свыше 200K входных токенов: 2× ввод и 1,5× вывод на весь запрос. `gemini-3.1-flash-image` тарифицирует вывод изображений по $60 за 1M токенов изображения.")}
    />

    <MathCard language={language} />

    <ul className="mp-notes">
      <li>{tr(language,
        "Rates are per 1M tokens; billing is metered per token at official provider rates, then the flat 50% discount is subtracted.",
        "Ставки указаны за 1M токенов; списание идёт за фактические токены по официальным ставкам, затем вычитается единая скидка 50%.")}</li>
      <li>{tr(language,
        "Thinking and reasoning tokens bill as output on both providers.",
        "Токены размышлений (thinking/reasoning) тарифицируются как вывод у обоих провайдеров.")}</li>
      <li>{tr(language,
        "GPT requests above 272K input tokens bill at official long-context rates: 2× input and 1.5× output on the whole request.",
        "GPT-запросы свыше 272K входных токенов тарифицируются по официальным ставкам длинного контекста: 2× ввод и 1,5× вывод на весь запрос.")}</li>
      <li>{tr(language,
        "Gemini rates follow the official Google standard paid tier; image output on Nano Banana models bills per image-output token.",
        "Ставки Gemini соответствуют официальному стандартному платному тарифу Google; вывод изображений на моделях Nano Banana тарифицируется за токен вывода изображения.")}</li>
      <li><Prose text={tr(language,
        "The live enabled set is always available at `GET /v1/models` on either surface.",
        "Актуальный список доступных моделей — всегда в `GET /v1/models` на любой из поверхностей.")} /></li>
    </ul>
  </div>;
}

function ProviderPanel({ language, provider, title, host, auth, models, cacheNote }: {
  language: Language;
  provider: "anthropic" | "openai" | "gemini";
  title: string;
  host: string;
  auth: string;
  models: CatalogModel[];
  cacheNote: string;
}) {
  return <section className="mp-panel" aria-label={title}>
    <header className="mp-panel-head">
      <div className="mp-panel-title">
        <span className={`ib-icon p-${provider}`} aria-hidden="true" />
        <div><h3>{title}</h3><code>{host}</code></div>
      </div>
      <span className="mp-auth">{auth}</span>
    </header>
    <div className="mp-table-wrap">
      <table className="mp-table">
        <thead><tr>
          <th>{tr(language, "Model", "Модель")}</th>
          <th>{tr(language, "Context", "Контекст")}</th>
          <th>{tr(language, "Max output", "Макс. вывод")}</th>
          <th>{tr(language, "Input", "Ввод")}</th>
          <th>{tr(language, "Cached input", "Кеш. ввод")}</th>
          <th>{tr(language, "Cache write", "Запись в кеш")}</th>
          <th>{tr(language, "Output", "Вывод")}</th>
        </tr></thead>
        <tbody>{models.map((model, index) => <ModelRow key={model.id} language={language} model={model} latest={index === 0} />)}</tbody>
      </table>
    </div>
    <footer className="mp-panel-foot"><Prose text={cacheNote} /></footer>
  </section>;
}

function ModelRow({ language, model, latest }: { language: Language; model: CatalogModel; latest: boolean }) {
  const cached = model.provider === "anthropic" ? (model as ClaudeModel).cacheReadPerM : (model as OpenAiModel | GeminiModel).cachedInputPerM;
  const cacheWrite = model.provider === "anthropic" ? (model as ClaudeModel).cacheWrite5mPerM : model.provider === "openai" ? (model as OpenAiModel).cacheWritePerM : null;
  return <tr>
    <td className="mp-model">
      <Link href={`/models/${model.slug}`}>
        <strong>{model.name}{latest && <span className="mp-badge">{tr(language, "Latest", "Новая")}</span>}</strong>
        <code>{model.id}</code>
      </Link>
    </td>
    <td className="mp-num">{model.context.replace(" tokens", "")}</td>
    <td className="mp-num">{model.maxOutput.replace(" tokens", "")}</td>
    <Price official={model.inputPerM} />
    <Price official={cached} />
    <Price official={cacheWrite} />
    <Price official={model.outputPerM} />
  </tr>;
}

function Price({ official }: { official: number | null }) {
  if (official === null) return <td className="mp-price">—</td>;
  return <td className="mp-price"><b>{priceHere(official)}</b><s>{formatUsd(official)}</s></td>;
}

function MathCard({ language }: { language: Language }) {
  // Worked billing example on the flagship Claude model, computed from the catalog.
  const opus = claudeModels[0]!;
  const inputM = 2;
  const outputM = 0.4;
  const inputCost = inputM * opus.inputPerM;
  const outputCost = outputM * opus.outputPerM;
  const officialTotal = inputCost + outputCost;
  const hereTotal = officialTotal * 0.5;
  return <div className="mp-math">
    <div className="mp-math-example">
      <span>{tr(language, "Example · Claude Opus 4.8", "Пример · Claude Opus 4.8")}</span>
      <code>2M input × {formatUsd(opus.inputPerM)} = {formatUsd(inputCost)}</code>
      <code>400K output × {formatUsd(opus.outputPerM)} = {formatUsd(outputCost)}</code>
      <code className="mp-math-total">{tr(language, "Official total", "Официально итого")} = {formatUsd(officialTotal)}</code>
    </div>
    <div className="mp-math-arrow" aria-hidden="true">→</div>
    <div className="mp-math-result">
      <span>{tr(language, "You pay", "Вы платите")}</span>
      <b>{formatUsd(hereTotal)}</b>
    </div>
    <div className="mp-math-strip">{tr(language,
      "A $100 top-up buys $200 of official API usage on any model — every dollar converts at the same rate.",
      "Пополнение на $100 даёт $200 официального использования API на любой модели — каждый доллар конвертируется по той же ставке.")}</div>
  </div>;
}
