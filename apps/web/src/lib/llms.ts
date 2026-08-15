import {
  articlesForLocale,
  clusterLabels,
  learnHubPath,
  learnMarkdownPath,
  learnPath,
  renderLearnMarkdown,
  resolveArticle,
  type Locale,
  type LearnCluster,
} from "./learn";
import { claudeModels, geminiModels, openaiModels } from "./models";
import { SITE_ORIGIN } from "./seo";

const clusterOrder: LearnCluster[] = ["buy", "free", "integrate", "compare", "explain"];
const kimiModelIds = ["kimi/k3", "kimi/k3-256k", "kimi/kimi-for-coding", "kimi/kimi-for-coding-highspeed"];

type LlmsCopy = {
  heading: string;
  summary: string;
  factsHeading: string;
  facts: string[];
  guidesHeading: string;
  allGuides: string;
};

const copy: Record<Locale, LlmsCopy> = {
  en: {
    heading: "# apiToken.sale — Claude, GPT, Gemini & Kimi API access",
    summary: "> apiToken.sale is an independent multi-provider API gateway built as a unified router: one endpoint serves Anthropic Messages for the supported Claude line and Kimi K3/Coding models, OpenAI Responses for GPT-5, and native Google generateContent for Gemini — plus one OpenAI-compatible route (Chat Completions) that reaches every catalog provider. One prepaid balance and one API key at a flat 50% discount off official spend on every request. No Anthropic, OpenAI, Google or Kimi account, waitlist, or billing-country requirement; pay by bank card or cryptocurrency.",
    factsHeading: "## Key facts",
    facts: [
      "Website: " + SITE_ORIGIN,
      "Unified router base URL: https://router.apitoken.sale — one endpoint for all providers, same sk-pool key on every lane",
      "Anthropic Messages lane: POST /v1/messages (x-api-key + anthropic-version headers) for Claude and namespaced Kimi models",
      "OpenAI lanes: POST /v1/responses and OpenAI-compatible POST /v1/chat/completions (Authorization: Bearer); unified catalog: GET /v1/models with namespaced IDs anthropic/*, openai/*, google/*, kimi/*",
      "Gemini lane (native): /v1beta/models/{model}:generateContent + streamGenerateContent (x-goog-api-key, GET /v1beta/models)",
      "Legacy per-provider hosts remain supported: https://api.apitoken.sale, https://openai.api.apitoken.sale/v1, https://gemini.api.apitoken.sale",
      "Claude models: Claude Opus 4.8, Opus 4.7, Sonnet 5, Sonnet 4.6, Haiku 4.5",
      "GPT models: gpt-5.6-sol (alias gpt-5.6), gpt-5.6-terra, gpt-5.6-luna, gpt-5.5, gpt-5.4 — text and image input, SSE streaming",
      "Gemini models: gemini-3.7-flash, gemini-3.6-flash, gemini-3.5-flash, gemini-3-flash-preview, gemini-3.1-pro-preview, gemini-3.1-flash-lite, gemini-2.5-flash, gemini-2.5-flash-lite, gemini-3.1-flash-image (Nano Banana 2)",
      "Kimi models: kimi/k3, kimi/k3-256k, kimi/kimi-for-coding, kimi/kimi-for-coding-highspeed",
      "Billing: prepaid, per-token at official rates minus a flat 50% B2C discount shared by all four providers; balance never expires",
      "Payment: bank card or cryptocurrency",
      "Free start: new Google/GitHub accounts get $5 of platform bonus credit; email/password accounts are ineligible",
      "Onboarding: instant, self-serve, no Anthropic, OpenAI, Google or Kimi account",
      "Support: Telegram and apitokensale@gmail.com (English, Russian)",
    ],
    guidesHeading: "## Guides",
    allGuides: "All API guides",
  },
  ru: {
    heading: "# apiToken.sale — доступ к Claude, GPT, Gemini и Kimi API",
    summary: "> apiToken.sale — независимый мультипровайдерный API-шлюз: один endpoint предоставляет Anthropic Messages для Claude и моделей Kimi K3/Coding, OpenAI Responses для GPT-5 и нативный Google generateContent для Gemini — плюс OpenAI-совместимый Chat Completions ко всем провайдерам каталога. Единый предоплаченный баланс и один API-ключ со скидкой 50% от официальной стоимости каждого запроса. Без аккаунтов Anthropic, OpenAI, Google или Kimi, без очереди и ограничений по billing country; оплата картой или криптовалютой.",
    factsHeading: "## Ключевые факты",
    facts: [
      "Сайт: " + SITE_ORIGIN,
      "Базовый URL единого router: https://router.apitoken.sale — один endpoint для всех провайдеров, тот же ключ sk-pool на всех маршрутах",
      "Маршрут Anthropic Messages: POST /v1/messages (x-api-key + anthropic-version) для Claude и namespaced-моделей Kimi",
      "Маршруты OpenAI: POST /v1/responses и OpenAI-совместимый POST /v1/chat/completions (Authorization: Bearer); единый каталог: GET /v1/models с namespaced ID anthropic/*, openai/*, google/*, kimi/*",
      "Маршрут Gemini (нативный): /v1beta/models/{model}:generateContent + streamGenerateContent (x-goog-api-key, GET /v1beta/models)",
      "Legacy-хосты по провайдерам поддерживаются: https://api.apitoken.sale, https://openai.api.apitoken.sale/v1, https://gemini.api.apitoken.sale",
      "Модели Claude: Claude Opus 4.8, Opus 4.7, Sonnet 5, Sonnet 4.6, Haiku 4.5",
      "Модели GPT: gpt-5.6-sol (псевдоним gpt-5.6), gpt-5.6-terra, gpt-5.6-luna, gpt-5.5, gpt-5.4 — текст и изображения на входе, SSE-стриминг",
      "Модели Gemini: gemini-3.7-flash, gemini-3.6-flash, gemini-3.5-flash, gemini-3-flash-preview, gemini-3.1-pro-preview, gemini-3.1-flash-lite, gemini-2.5-flash, gemini-2.5-flash-lite, gemini-3.1-flash-image (Nano Banana 2)",
      "Модели Kimi: kimi/k3, kimi/k3-256k, kimi/kimi-for-coding, kimi/kimi-for-coding-highspeed",
      "Оплата за токены по официальным ставкам минус единая скидка 50% для B2C, общая для всех четырёх провайдеров; баланс не сгорает",
      "Способы оплаты: банковская карта или криптовалюта",
      "Бесплатный старт: новые аккаунты через Google/GitHub получают бонус $5 на баланс платформы; аккаунты по email/паролю не участвуют",
      "Подключение: мгновенно, без аккаунтов Anthropic, OpenAI, Google или Kimi",
      "Поддержка: Telegram и apitokensale@gmail.com (русский, английский)",
    ],
    guidesHeading: "## Гайды",
    allGuides: "Все гайды по API",
  },
  zh: {
    heading: "# apiToken.sale — Claude、GPT、Gemini 与 Kimi API 接入",
    summary: "> apiToken.sale 是独立的多提供商 API 网关：单一端点以 Anthropic Messages 提供 Claude 与 Kimi K3/Coding 模型，以 OpenAI Responses 提供 GPT-5，并以原生 Google generateContent 提供 Gemini；另有 OpenAI-compatible Chat Completions 可访问目录中的全部提供商。共用一个预付余额和一个 API 密钥，每个请求按官方价格的 50% 折扣计费。无需 Anthropic、OpenAI、Google 或 Kimi 账户、无需排队，也无需对应厂商 billing country；支持银行卡或加密货币付款。",
    factsHeading: "## 关键信息",
    facts: [
      "网站: " + SITE_ORIGIN,
      "统一路由器基址: https://router.apitoken.sale —— 所有提供商共用一个端点，各通道使用同一把 sk-pool 密钥",
      "Anthropic Messages 通道: POST /v1/messages（x-api-key + anthropic-version），用于 Claude 与命名空间 Kimi 模型",
      "OpenAI 通道: POST /v1/responses 与 OpenAI 兼容 POST /v1/chat/completions（Authorization: Bearer）；统一目录: GET /v1/models，命名空间 ID 为 anthropic/*、openai/*、google/*、kimi/*",
      "Gemini 通道（原生）: /v1beta/models/{model}:generateContent + streamGenerateContent（x-goog-api-key，GET /v1beta/models）",
      "按提供商划分的旧版主机仍然可用: https://api.apitoken.sale、https://openai.api.apitoken.sale/v1、https://gemini.api.apitoken.sale",
      "Claude 模型: Claude Opus 4.8、Opus 4.7、Sonnet 5、Sonnet 4.6、Haiku 4.5",
      "GPT 模型: gpt-5.6-sol（别名 gpt-5.6）、gpt-5.6-terra、gpt-5.6-luna、gpt-5.5、gpt-5.4 —— 支持文本与图片输入、SSE 流式输出",
      "Gemini 模型: gemini-3.7-flash、gemini-3.6-flash、gemini-3.5-flash、gemini-3-flash-preview、gemini-3.1-pro-preview、gemini-3.1-flash-lite、gemini-2.5-flash、gemini-2.5-flash-lite、gemini-3.1-flash-image (Nano Banana 2)",
      "Kimi 模型: kimi/k3、kimi/k3-256k、kimi/kimi-for-coding、kimi/kimi-for-coding-highspeed",
      "计费: 预付、按 token 以官方价格计费再减去统一的 50% B2C 折扣（四家提供商共用）；余额永不过期",
      "支付: 银行卡或加密货币",
      "免费起步: 通过 Google/GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不参与",
      "开通: 即时、自助、无需 Anthropic、OpenAI、Google 或 Kimi 账户",
      "支持: Telegram 与 apitokensale@gmail.com（中文、英文、俄文）",
    ],
    guidesHeading: "## 指南",
    allGuides: "全部 API 指南",
  },
  ko: {
    heading: "# apiToken.sale — Claude, GPT, Gemini & Kimi API 액세스",
    summary: "> apiToken.sale은 독립 멀티 provider API gateway입니다. 하나의 endpoint에서 Anthropic Messages로 Claude와 Kimi K3/Coding 모델을, OpenAI Responses로 GPT-5를, native Google generateContent로 Gemini를 제공하며 OpenAI-compatible Chat Completions로 catalog의 모든 provider에 접근합니다. 하나의 선불 잔액과 API key로 모든 요청에 공식 요금 대비 50% 할인이 적용됩니다. Anthropic, OpenAI, Google 또는 Kimi 계정과 해당 vendor billing country 없이 카드나 암호화폐로 결제할 수 있습니다.",
    factsHeading: "## 핵심 정보",
    facts: [
      "웹사이트: " + SITE_ORIGIN,
      "통합 라우터 기본 URL: https://router.apitoken.sale — 모든 프로바이더를 위한 단일 엔드포인트, 모든 레인에서 동일한 sk-pool 키 사용",
      "Anthropic Messages lane: POST /v1/messages (x-api-key + anthropic-version)로 Claude와 namespaced Kimi 모델 제공",
      "OpenAI lane: POST /v1/responses 및 OpenAI 호환 POST /v1/chat/completions (Authorization: Bearer); 통합 catalog: GET /v1/models, namespace ID anthropic/*, openai/*, google/*, kimi/*",
      "Gemini 레인(네이티브): /v1beta/models/{model}:generateContent + streamGenerateContent (x-goog-api-key, GET /v1beta/models)",
      "레거시 프로바이더별 호스트는 계속 지원됩니다: https://api.apitoken.sale, https://openai.api.apitoken.sale/v1, https://gemini.api.apitoken.sale",
      "Claude 모델: Claude Opus 4.8, Opus 4.7, Sonnet 5, Sonnet 4.6, Haiku 4.5",
      "GPT 모델: gpt-5.6-sol(별칭 gpt-5.6), gpt-5.6-terra, gpt-5.6-luna, gpt-5.5, gpt-5.4 — 텍스트·이미지 입력, SSE 스트리밍",
      "Gemini 모델: gemini-3.7-flash, gemini-3.6-flash, gemini-3.5-flash, gemini-3-flash-preview, gemini-3.1-pro-preview, gemini-3.1-flash-lite, gemini-2.5-flash, gemini-2.5-flash-lite, gemini-3.1-flash-image (Nano Banana 2)",
      "Kimi 모델: kimi/k3, kimi/k3-256k, kimi/kimi-for-coding, kimi/kimi-for-coding-highspeed",
      "과금: 선불, 공식 요금 기준 token당 과금에서 B2C 50% 통일 할인(네 provider 공통); 잔액은 만료되지 않음",
      "결제: 신용카드 또는 암호화폐",
      "무료 시작: Google/GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧을 받으며, 이메일/비밀번호 계정은 제외",
      "온보딩: 즉시, self-service, Anthropic·OpenAI·Google·Kimi 계정 불필요",
      "지원: Telegram 및 apitokensale@gmail.com (한국어 요청 시 영어/러시아어 응대)",
    ],
    guidesHeading: "## 가이드",
    allGuides: "모든 API 가이드",
  },
};

export function buildLlms(locale: Locale): string {
  const c = copy[locale];
  const modelIds = [...claudeModels, ...openaiModels, ...geminiModels].map((model) => model.id).concat(kimiModelIds).join(", ");
  const lines: string[] = [
    c.heading,
    "",
    c.summary,
    "",
    "## For AI agents",
    "",
    "Every public section is available as clean Markdown — the private dashboard, auth and account pages are excluded. Core references:",
    "",
    `- Agent setup runbook (detect OS/tool, configure securely, verify and diagnose): ${SITE_ORIGIN}/md/connect`,
    `- API reference (base URLs, model IDs, streaming, tools, errors): ${SITE_ORIGIN}/md/docs`,
    `- Model catalog (exact IDs, context, pricing): ${SITE_ORIGIN}/md/models`,
    `- Error reference (exact response text, cause and fix for every error): ${SITE_ORIGIN}/md/docs/errors`,
    `- Pricing & flat 50% discount: ${SITE_ORIGIN}/md/plans`,
    `- Markdown index of everything: ${SITE_ORIGIN}/md`,
    `- Any guide as Markdown: append its slug to the gateway, e.g. ${SITE_ORIGIN}${learnMarkdownPath("cheapest-claude-api", locale)}`,
    `- Sitemap: ${SITE_ORIGIN}/sitemap.xml`,
    "",
    c.factsHeading,
    "",
    ...c.facts.map((fact) => "- " + fact),
    `- Exact API model IDs: ${modelIds}`,
    "- Capability evidence: the native Claude Messages lane preserves SSE, tool use, prompt caching (cache_control) and vision. GPT Responses and Chat Completions support incremental SSE and text+image input. Gemini preserves native generateContent and streamGenerateContent SSE. Kimi uses the Messages shape and accepts stream:true, but public chunk incrementality remains a preview capability under live validation",
    "",
    c.guidesHeading,
    "",
    `- [${c.allGuides}](${SITE_ORIGIN}${learnHubPath(locale)})`,
  ];
  for (const cluster of clusterOrder) {
    const slugs = articlesForLocale(locale).filter((slug) => resolveArticle(slug, locale)!.cluster === cluster);
    if (slugs.length === 0) continue;
    lines.push("", "### " + clusterLabels[locale][cluster].label);
    for (const slug of slugs) {
      const article = resolveArticle(slug, locale)!;
      lines.push(`- [${article.content.title}](${SITE_ORIGIN}${learnPath(slug, locale)}) — ${article.content.description}`);
    }
  }
  lines.push("");
  return lines.join("\n");
}

export function buildLlmsFull(locale: Locale): string {
  const c = copy[locale];
  const header = [c.heading.replace("# ", "# [full] "), "", c.summary, "", "---", ""].join("\n");
  const body = articlesForLocale(locale)
    .map((slug) => renderLearnMarkdown(resolveArticle(slug, locale)!, SITE_ORIGIN))
    .join("\n\n---\n\n");
  return header + body + "\n";
}
