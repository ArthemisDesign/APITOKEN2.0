"use client";

import Link from "next/link";
import type { ReactNode } from "react";
import { CommercialDisclosure } from "./commercial-disclosure";
import { useI18n, type Language } from "./i18n-provider";
import { PricingOverview } from "./pricing-overview";
import { LocalizedLink, T } from "./translated";
import { DOCS_URL } from "@/lib/site-links";
import { formatUsd, geminiFlashPricingAt } from "@/lib/models";

export function MarketingFrame({ children }: { children: ReactNode }) {
  return <main>{children}</main>;
}

function intBase(language: Language): string {
  return language === "ru" ? "/ru/integrations" : "/integrations";
}

export function PageHero({ eyebrow, title, subtitle, back }: { eyebrow: string; title: string; subtitle: string; back?: boolean }) {
  const { language } = useI18n();
  return <div className="page-hero"><div className="wrap">{back && <Link className="auth-back" href={intBase(language)}><T k="int_back">← All integrations</T></Link>}<T k={eyebrow} as="span" className="eyebrow">Section</T><T k={title} as="h1">Title</T><T k={subtitle} as="p">Description</T></div></div>;
}

export function PlansContent() {
  return <><PageHero eyebrow="pr_eyebrow" title="plans_h" subtitle="plans_sub" /><section className="borderless"><div className="wrap plans-content"><CommercialDisclosure /><PricingOverview /></div></section></>;
}

const claudeModelRows = [
  ["Claude Opus 5","claude-opus-5","1M","$5","$25","m_opus5"],
  ["Claude Fable 5","claude-fable-5","1M","$10","$50","m_fable5"],
  ["Claude Opus 4.8","claude-opus-4-8","1M","$5","$25","m_opus48"],
  ["Claude Opus 4.7","claude-opus-4-7","1M","$5","$25","m_opus47"],
  ["Claude Sonnet 5","claude-sonnet-5","1M","$2*","$10*","m_son46"],
  ["Claude Sonnet 4.6","claude-sonnet-4-6","1M","$3","$15","m_son46"],
  ["Claude Haiku 4.5","claude-haiku-4-5","200K","$1","$5","m_haiku"],
] as const;

const gptModelRows = [
  ["GPT-5.6 Sol","gpt-5.6-sol","400K","$5","$30","m_gpt56sol"],
  ["GPT-5.6 Terra","gpt-5.6-terra","400K","$2","$12","m_gpt56terra"],
  ["GPT-5.6 Luna","gpt-5.6-luna","400K","$0.20","$1.20","m_gpt56luna"],
  ["GPT-5.5","gpt-5.5","400K","$5","$30","m_gpt55"],
  ["GPT-5.4","gpt-5.4","400K","$2.50","$15","m_gpt54"],
] as const;

const geminiFlashRates = geminiFlashPricingAt();
const geminiFlashInput = `${formatUsd(geminiFlashRates.inputPerM)}*`;
const geminiFlashOutput = `${formatUsd(geminiFlashRates.outputPerM)}*`;

const geminiModelRows = [
  ["Gemini 3.7 Flash","gemini-3.7-flash","1M",geminiFlashInput,geminiFlashOutput,"m_gem37flash"],
  ["Gemini 3.6 Flash","gemini-3.6-flash","1M",geminiFlashInput,geminiFlashOutput,"m_gem36flash"],
  ["Gemini 3.5 Flash","gemini-3.5-flash","1M","$1.50","$9.00","m_gem35flash"],
  ["Gemini 3 Flash Preview","gemini-3-flash-preview","1M","$0.50","$3.00","m_gem3flashpreview"],
  ["Gemini 3.1 Pro Preview","gemini-3.1-pro-preview","1M","$2*","$12*","m_gem31pro"],
  ["Gemini 3.1 Flash-Lite","gemini-3.1-flash-lite","1M","$0.25","$1.50","m_gem31lite"],
  ["Gemini 2.5 Flash","gemini-2.5-flash","1M","$0.30","$2.50","m_gem25flash"],
  ["Gemini 2.5 Flash-Lite","gemini-2.5-flash-lite","1M","$0.10","$0.40","m_gem25lite"],
  ["Gemini 3.1 Flash Image (Nano Banana 2)","gemini-3.1-flash-image","128K","$0.50","$3.00","m_gemimage"],
] as const;

const modelPageCopy: Record<Language, { sonnet5Footnote: string; gptFootnote: string; geminiFootnote: string }> = {
  en: {
    sonnet5Footnote: "* Claude Sonnet 5 introductory official pricing is $2 / $10 per 1M through 2026-08-31 and returns to $3 / $15 on 2026-09-01. The engine already charges the current effective rate.",
    gptFootnote: "GPT rows are official OpenAI standard rates. gpt-5.6 is a convenience alias of gpt-5.6-sol. Requests above 272K input tokens bill at OpenAI long-context rates (2× input, 1.5× output on the whole request).",
    geminiFootnote: "* Gemini 3.6 Flash and Gemini 3.7 Flash promotional rates are $0.75 / $3.75 per 1M through 2026-12-31 and become $1.50 / $7.50 on 2027-01-01. The table resolves the effective rate at build time. Gemini 3.1 Pro Preview bills $4 / $18 per 1M above 200K input tokens. Gemini 3.1 Flash Image bills image output at $60 per 1M image-output tokens.",
  },
  ru: {
    sonnet5Footnote: "* Для Claude Sonnet 5 официальная вводная цена $2 / $10 за 1 млн действует до 2026-08-31 включительно; с 2026-09-01 возвращается ставка $3 / $15. Движок уже применяет актуальную ставку.",
    gptFootnote: "Строки GPT — официальные стандартные ставки OpenAI. gpt-5.6 — удобный псевдоним gpt-5.6-sol. Запросы свыше 272K входных токенов тарифицируются по ставкам OpenAI для длинного контекста (×2 вход, ×1,5 выход за весь запрос).",
    geminiFootnote: "* Для Gemini 3.6 Flash и Gemini 3.7 Flash промо-ставки $0.75 / $3.75 за 1 млн действуют до 2026-12-31; с 2027-01-01 — $1.50 / $7.50. Таблица выбирает актуальную ставку во время сборки. Gemini 3.1 Pro Preview тарифицируется по $4 / $18 за 1 млн свыше 200K входных токенов. Gemini 3.1 Flash Image тарифицирует вывод изображений по $60 за 1 млн токенов изображения.",
  },
};

function ModelTable({ rows, footnote }: { rows: readonly (readonly [string, string, string, string, string, string])[]; footnote?: string }) {
  return <><div className="table-scroll"><table className="mtable"><thead><tr><T k="th_model" as="th">Model</T><T k="th_ctx" as="th">Context</T><T k="th_in" as="th">Input / 1M</T><T k="th_out" as="th">Output / 1M</T><T k="th_best" as="th">Best for</T></tr></thead><tbody>{rows.map(([name,id,ctx,input,output,best], index) => <tr key={id}><td><Link className="mname" href={`/models/${id.replaceAll(".", "-")}`}>{name}</Link>{index === 0 && <T k="latest_badge" as="span" className="model-badge">Latest</T>}<br /><code>{id}</code></td><td><span className={`context-badge ${ctx === "1M" ? "context-full" : ""}`}>{ctx}</span></td><td className="mprice">{input}</td><td className="mprice">{output}</td><T k={best} as="td">Use case</T></tr>)}</tbody></table></div>{footnote && <p className="tier-footnote">{footnote}</p>}</>;
}

export function ModelsPage() {
  const { language } = useI18n();
  return <MarketingFrame><PageHero eyebrow="nav_models" title="models_h" subtitle="models_sub" /><section className="borderless"><div className="wrap"><div className="model-rate-note"><div><T k="model_rate_tag" as="span" className="tag">Official list rates</T><T k="model_rate_h" as="h3">Official rates behind every spend calculation</T></div><T k="model_rate_p" as="p">These official Anthropic, OpenAI and Google list rates calculate official API spend. B2C accounts pay 50% of that spend on every request; B2B rates are negotiated.</T></div><T k="m_provider_claude" as="h3" className="docs-h3">Claude · Anthropic Messages API</T><ModelTable rows={claudeModelRows} footnote={modelPageCopy[language].sonnet5Footnote} /><T k="m_provider_gpt" as="h3" className="docs-h3">GPT · OpenAI-compatible API</T><ModelTable rows={gptModelRows} footnote={modelPageCopy[language].gptFootnote} /><T k="m_provider_gemini" as="h3" className="docs-h3">Gemini · Google Gemini API</T><ModelTable rows={geminiModelRows} footnote={modelPageCopy[language].geminiFootnote} /><PageActions /></div></section></MarketingFrame>;
}

const integrations = [
  ["claude-code","Claude Code","int_cc_tag"], ["codex","Codex CLI","int_codex_tag"], ["cursor","Cursor","int_cur_tag"], ["cline","Cline","int_cli_tag"],
  ["opencode","opencode","int_oc_tag"], ["continue","Continue","int_con_tag"], ["zed","Zed","int_zed_tag"], ["sdk","SDK (Python / TS)","int_sdk_tag"],
] as const;

export function IntegrationsPage() {
  const { language } = useI18n();
  const prefix = language === "ru" ? "/ru" : "";
  return <MarketingFrame><PageHero eyebrow="nav_int" title="int_h" subtitle="int_sub" /><section className="borderless"><div className="wrap"><div className="steps" data-reveal-stagger>{integrations.map(([slug,name,tag], index) => <Link className="step" href={`${prefix}/int-${slug}`} key={slug}><div className="n">{String(index + 1).padStart(2,"0")}</div><h3>{name}</h3><T k={tag} as="p">Integration description</T><T k="int_open" as="span" className="step-cta">Open guide</T></Link>)}</div></div></section></MarketingFrame>;
}

const guideCode: Record<string, ReactNode> = {
  "claude-code": <><span className="c"># set once in your shell profile</span>{`\n`}<span className="k">export</span> ANTHROPIC_BASE_URL=https://router.apitoken.sale{`\n`}<span className="k">export</span> ANTHROPIC_API_KEY=sk-pool-•••{`\n\n`}<span className="c"># then just run</span>{`\n`}<span className="k">claude</span></>,
  codex: <><span className="c"># ~/.codex/apitoken.config.toml</span>{`\n`}model = <span className="g">&quot;gpt-5.6-sol&quot;</span>{`\n`}model_provider = <span className="g">&quot;apitoken&quot;</span>{`\n\n`}[model_providers.apitoken]{`\n`}name = <span className="g">&quot;apiToken.sale&quot;</span>{`\n`}base_url = <span className="g">&quot;https://router.apitoken.sale/v1&quot;</span>{`\n`}wire_api = <span className="g">&quot;responses&quot;</span>{`\n`}env_key = <span className="g">&quot;APITOKEN_API_KEY&quot;</span>{`\n\n`}<span className="c"># keep the key in your shell, then pick the profile</span>{`\n`}<span className="k">export</span> APITOKEN_API_KEY=sk-pool-•••{`\n`}<span className="k">codex</span> --profile apitoken</>,
  cursor: <><span className="c"># Cursor → Settings → Models → Anthropic API</span>{`\n`}Base URL : https://router.apitoken.sale{`\n`}API key  : sk-pool-•••{`\n`}Model    : claude-opus-4-8</>,
  cline: <><span className="c"># Cline → Settings</span>{`\n`}API Provider : Anthropic{`\n`}Base URL     : https://router.apitoken.sale{`\n`}API Key      : sk-pool-•••{`\n`}Model        : claude-opus-4-8</>,
  opencode: <><span className="c">{"// opencode.json — provider block"}</span>{`\n`}{`{\n  "provider": {\n    "apitoken": {\n      "npm": "@ai-sdk/openai-compatible",\n      "name": "apiToken.sale",\n      "options": {\n        "baseURL": "https://router.apitoken.sale/v1",\n        "apiKey": "{env:APITOKEN_API_KEY}"\n      },\n      "models": {\n        "gpt-5.6-sol": { "name": "GPT-5.6 Sol" }\n      }\n    }\n  }\n}`}</>,
  continue: <><span className="c">{"// ~/.continue/config.json"}</span>{`\n`}{`{\n  "models": [{\n    "title": "Claude via apiToken.sale",\n    "provider": "anthropic",\n    "apiBase": "https://router.apitoken.sale",\n    "apiKey": "sk-pool-•••",\n    "model": "claude-opus-4-8"\n  }]\n}`}</>,
  zed: <><span className="c">{"// Zed settings.json"}</span>{`\n`}{`{\n  "assistant": {\n    "default_model": {\n      "provider": "anthropic",\n      "model": "claude-opus-4-8"\n    }\n  },\n  "language_models": {\n    "anthropic": { "api_url": "https://router.apitoken.sale" }\n  }\n}`}</>,
  sdk: <><span className="c"># Python</span>{`\n`}<span className="k">from</span> anthropic <span className="k">import</span> Anthropic{`\n`}client = Anthropic({`\n`}    base_url=<span className="g">&quot;https://router.apitoken.sale&quot;</span>,{`\n`}    api_key=<span className="g">&quot;sk-pool-•••&quot;</span>,{`\n`}){`\n`}msg = client.messages.create({`\n`}    model=<span className="g">&quot;claude-opus-4-8&quot;</span>,{`\n`}    max_tokens=1024,{`\n`}    messages=[{`{"role":"user","content":"Hello"}`}],{`\n`})</>,
};

export function IntegrationGuidePage({ slug }: { slug: string }) {
  const { language } = useI18n();
  const found = integrations.find(([candidate]) => candidate === slug);
  if (!found) return null;
  const [,name,tag] = found;
  return <MarketingFrame><div className="page-hero"><div className="wrap"><Link className="auth-back" href={intBase(language)}><T k="int_back">← All integrations</T></Link><T k="nav_int" as="span" className="eyebrow">Integrations</T><h1>{name}</h1><T k={tag} as="p">Integration description</T></div></div><section className="borderless"><div className="wrap"><div className="steps guide-steps" data-reveal-stagger>{[["int_s1_h","int_s1_p"],["int_s2_h","int_s2_p"],["int_s3_h","int_s3_p"]].map(([title,text], index) => <div className="step" key={title}><div className="n">{String(index + 1).padStart(2,"0")}</div><T k={title} as="h3">Step</T><T k={text} as="p">Description</T></div>)}</div><div className="doc-block"><T k="int_cfg" as="h3">Configuration</T><pre className="codebox">{guideCode[slug]}</pre><PageActions /></div></div></section></MarketingFrame>;
}

function PageActions({ primaryOnly = false }: { primaryOnly?: boolean }) {
  return <div className="hero-cta page-actions"><LocalizedLink className="btn btn-primary" href="/register"><T k="hero_cta1">Get API key</T></LocalizedLink>{!primaryOnly && <Link className="btn btn-ghost" href={DOCS_URL} target="_blank" rel="noreferrer"><T k="hero_cta2">Read documentation</T></Link>}</div>;
}
