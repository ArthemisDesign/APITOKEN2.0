// Learn cluster — programmatic SEO landing pages under /docs/learn.
// Each article targets one commercial search intent for apiToken.sale and is
// rendered server-side (fully crawlable) with Article + FAQPage + Breadcrumb
// structured data. Copy is grounded in real product facts only.

import { learnRu } from "./learn-ru";
import { learnZh } from "./learn-zh";
import { learnKo } from "./learn-ko";
import { learnProviderEn } from "./learn-provider-en";
import { learnProviderRu } from "./learn-provider-ru";
import { learnProviderZh } from "./learn-provider-zh";
import { learnProviderKo } from "./learn-provider-ko";
import {
  learnImageSeoEn,
  learnImageSeoKo,
  learnImageSeoRu,
  learnImageSeoZh,
} from "./learn-image-seo";

export type LearnCluster = "buy" | "free" | "integrate" | "compare" | "explain";

export type LearnBlock =
  | { type: "p"; text: string }
  | { type: "list"; items: string[] }
  | { type: "steps"; items: string[] }
  | { type: "code"; code: string }
  | { type: "note"; text: string }
  | { type: "table"; headers: string[]; rows: string[][] }
  | { type: "link"; text: string; href: string };

export type LearnSection = { h2: string; blocks: LearnBlock[] };

export type LearnFaq = { q: string; a: string };

export type LearnArticle = {
  slug: string;
  cluster: LearnCluster;
  /** <title> without the brand suffix (appended by metadata template). */
  title: string;
  h1: string;
  description: string;
  keywords: string[];
  dek: string;
  sections: LearnSection[];
  faq: LearnFaq[];
  related: string[];
  /** ISO date (YYYY-MM-DD) of the last substantive content change; falls back to the cluster launch date. */
  updated?: string;
  /** ISO date (YYYY-MM-DD) the article first shipped; falls back to the cluster launch date. */
  published?: string;
};

export type Locale = "en" | "ru" | "zh" | "ko";
// Guides are published in all four locales. (The core marketing site is
// English + Russian only.)
export const LOCALES: Locale[] = ["en", "ru", "zh", "ko"];

/** The translatable subset of an article (shared fields: slug, cluster, related). */
export type LocalizedContent = {
  title: string;
  h1: string;
  description: string;
  keywords: string[];
  dek: string;
  sections: LearnSection[];
  faq: LearnFaq[];
};

export const clusterLabels: Record<Locale, Record<LearnCluster, { label: string; blurb: string }>> = {
  en: {
    buy: { label: "Buying a key", blurb: "Get one API key for Claude, GPT, Gemini and Kimi, then start in minutes." },
    free: { label: "Free & low cost", blurb: "Try supported models across all four providers before you top up." },
    integrate: { label: "Tool setup", blurb: "Connect native APIs, SDKs, coding agents and image routes." },
    compare: { label: "Compare", blurb: "How apiToken.sale stacks up against the alternatives." },
    explain: { label: "How it works", blurb: "Pricing, billing, activation and support, explained." },
  },
  ru: {
    buy: { label: "Покупка ключа", blurb: "Один API-ключ для Claude, GPT, Gemini и Kimi — начните за несколько минут." },
    free: { label: "Бесплатно и дёшево", blurb: "Попробуйте поддерживаемые модели всех четырёх провайдеров до пополнения." },
    integrate: { label: "Настройка инструментов", blurb: "Подключите нативные API, SDK, coding agents и image routes." },
    compare: { label: "Сравнения", blurb: "Чем apiToken.sale отличается от альтернатив." },
    explain: { label: "Как это работает", blurb: "Цены, оплата, активация и поддержка — простыми словами." },
  },
  zh: {
    buy: { label: "购买密钥", blurb: "一个 API 密钥即可使用 Claude、GPT、Gemini 与 Kimi，几分钟内开始。" },
    free: { label: "免费与低成本", blurb: "充值前先试用四个提供商的支持模型。" },
    integrate: { label: "工具接入", blurb: "连接原生 API、SDK、编程代理与图像路由。" },
    compare: { label: "对比", blurb: "apiToken.sale 与其他方案的对比。" },
    explain: { label: "工作原理", blurb: "价格、计费、激活与支持，通俗讲解。" },
  },
  ko: {
    buy: { label: "키 구매", blurb: "하나의 API 키로 Claude, GPT, Gemini, Kimi를 몇 분 안에 시작하세요." },
    free: { label: "무료 및 저비용", blurb: "충전 전에 네 provider의 지원 모델을 사용해 보세요." },
    integrate: { label: "도구 설정", blurb: "native API, SDK, coding agent, image route를 연결하세요." },
    compare: { label: "비교", blurb: "apiToken.sale과 다른 대안들의 비교." },
    explain: { label: "작동 방식", blurb: "가격, 결제, 활성화, 지원을 쉽게 설명합니다." },
  },
};

export const learnUi: Record<Locale, {
  guidesEyebrow: string;
  backToHub: string;
  faqHeading: string;
  relatedHeading: string;
  getKey: string;
  readDocs: string;
  docsBack: string;
  hubTitle: string;
  hubDescription: string;
  hubKeywords: string[];
  crumbHome: string;
  crumbDocs: string;
  crumbGuides: string;
  updated: string;
  byline: string;
  seeAlso: string;
  ctaVariants: string[];
}> = {
  en: {
    guidesEyebrow: "Multi-provider API guides",
    backToHub: "← API guides",
    faqHeading: "Frequently asked questions",
    relatedHeading: "Related guides",
    getKey: "Get API key",
    readDocs: "Read documentation",
    docsBack: "← Documentation",
    hubTitle: "Claude, GPT, Gemini & Kimi API Guides",
    hubDescription: "Practical guides for buying, configuring and choosing Claude, GPT, Gemini and Kimi APIs with one apiToken.sale key — pricing, models, SDKs, coding agents and images.",
    hubKeywords: ["claude api guide", "gpt api guide", "gemini api guide", "kimi api guide", "openai compatible api", "multi provider api"],
    crumbHome: "Home",
    crumbDocs: "Docs",
    crumbGuides: "Guides",
    updated: "Updated",
    byline: "apiToken.sale Editorial",
    seeAlso: "See also:",
    ctaVariants: [
      "Start with Google or GitHub and get $5 of platform bonus credit — no card required.",
      "Try it before you pay: new Google/GitHub accounts include $5 of platform bonus credit.",
      "Create an account with Google or GitHub and test the gateway with $5 of platform bonus credit.",
      "Use Google or GitHub to create your key and get $5 of platform bonus credit before you top up.",
    ],
  },
  ru: {
    guidesEyebrow: "Гайды по API разных провайдеров",
    backToHub: "← Гайды по API",
    faqHeading: "Частые вопросы",
    relatedHeading: "Похожие гайды",
    getKey: "Получить API-ключ",
    readDocs: "Открыть документацию",
    docsBack: "← Документация",
    hubTitle: "Гайды по Claude, GPT, Gemini и Kimi API",
    hubDescription: "Практические гайды по покупке, настройке и выбору Claude, GPT, Gemini и Kimi API с одним ключом apiToken.sale — цены, модели, SDK, coding agents и изображения.",
    hubKeywords: ["claude api гайд", "gpt api гайд", "gemini api гайд", "kimi api гайд", "openai совместимый api", "api нескольких провайдеров"],
    crumbHome: "Главная",
    crumbDocs: "Документация",
    crumbGuides: "Гайды",
    updated: "Обновлено",
    byline: "Редакция apiToken.sale",
    seeAlso: "Читайте также:",
    ctaVariants: [
      "Войдите через Google или GitHub и получите приветственный бонус $5 на баланс платформы — без карты.",
      "Проверьте до оплаты: новые аккаунты через Google/GitHub получают бонус $5 на баланс платформы.",
      "Создайте аккаунт через Google или GitHub и протестируйте шлюз с бонусом $5 на балансе платформы.",
      "Используйте Google или GitHub, чтобы получить ключ и бонус $5 на баланс платформы до пополнения.",
    ],
  },
  zh: {
    guidesEyebrow: "多提供商 API 指南",
    backToHub: "← API 指南",
    faqHeading: "常见问题",
    relatedHeading: "相关指南",
    getKey: "获取 API 密钥",
    readDocs: "阅读文档",
    docsBack: "← 文档",
    hubTitle: "Claude、GPT、Gemini 与 Kimi API 指南",
    hubDescription: "使用一个 apiToken.sale 密钥购买、配置和选择 Claude、GPT、Gemini 与 Kimi API 的实用指南——价格、模型、SDK、编程代理与图像。",
    hubKeywords: ["claude api 指南", "gpt api 指南", "gemini api 指南", "kimi api 指南", "openai 兼容 api", "多提供商 api"],
    crumbHome: "首页",
    crumbDocs: "文档",
    crumbGuides: "指南",
    updated: "更新于",
    byline: "apiToken.sale 编辑部",
    seeAlso: "另见：",
    ctaVariants: [
      "使用 Google 或 GitHub 创建账户，可获 $5 平台欢迎奖励余额，无需绑卡。",
      "先试后付：通过 Google/GitHub 创建的新账户包含 $5 平台欢迎奖励余额。",
      "通过 Google 或 GitHub 创建密钥，用 $5 平台欢迎奖励余额测试网关。",
      "使用 Google 或 GitHub 创建账户并获得 $5 平台欢迎奖励余额，充值前先跑通配置。",
    ],
  },
  ko: {
    guidesEyebrow: "멀티 provider API 가이드",
    backToHub: "← API 가이드",
    faqHeading: "자주 묻는 질문",
    relatedHeading: "관련 가이드",
    getKey: "API 키 받기",
    readDocs: "문서 읽기",
    docsBack: "← 문서",
    hubTitle: "Claude, GPT, Gemini, Kimi API 가이드",
    hubDescription: "하나의 apiToken.sale 키로 Claude, GPT, Gemini, Kimi API를 구매·설정·선택하는 실용 가이드 — 가격, 모델, SDK, coding agent, 이미지.",
    hubKeywords: ["claude api 가이드", "gpt api 가이드", "gemini api 가이드", "kimi api 가이드", "openai 호환 api", "멀티 provider api"],
    crumbHome: "홈",
    crumbDocs: "문서",
    crumbGuides: "가이드",
    updated: "업데이트",
    byline: "apiToken.sale 편집팀",
    seeAlso: "함께 보기:",
    ctaVariants: [
      "Google 또는 GitHub로 가입하고 $5 플랫폼 웰컴 보너스 크레딧을 카드 없이 받으세요.",
      "결제 전에 사용해 보세요: Google/GitHub 신규 계정에는 $5 플랫폼 웰컴 보너스 크레딧이 포함됩니다.",
      "Google 또는 GitHub로 키를 만들고 $5 플랫폼 웰컴 보너스 크레딧으로 게이트웨이를 테스트하세요.",
      "Google 또는 GitHub로 계정을 만들고 $5 플랫폼 웰컴 보너스 크레딧으로 충전 전에 설정을 확인하세요.",
    ],
  },
};

const BASE = "https://router.apitoken.sale";
const OPENAI_BASE = "https://router.apitoken.sale/v1";
const KEY = "sk-pool-•••";

// Shared reusable blocks -----------------------------------------------------
const quickSetupSteps: LearnBlock = {
  type: "steps",
  items: [
    "Create a free account and open the dashboard — no approval or waitlist.",
    "Generate one API key (it looks like sk-pool-…). The same key works across supported Claude, GPT, Gemini and Kimi models.",
    `Choose the client protocol: Anthropic Messages at ${BASE} with x-api-key, OpenAI-compatible at ${OPENAI_BASE} with Authorization: Bearer, or native Gemini at ${BASE} with x-goog-api-key. Kimi works on Anthropic Messages and through the universal OpenAI-compatible lane.`,
  ],
};

const cta = (): LearnBlock => ({ type: "note", text: "New accounts created with Google or GitHub start with $5 of platform bonus credit — valid on supported Claude, GPT, Gemini and Kimi models; email/password accounts do not receive the bonus." });

const coreLearnArticles: LearnArticle[] = [
  // ─────────────────────────── BUY ───────────────────────────
  {
    slug: "how-to-buy-claude-api-key",
    cluster: "buy",
    title: "How to Buy a Claude API Key",
    h1: "How to buy a Claude API key",
    description: "Buy a Claude API key in minutes with apiToken.sale — one key for every Claude model, prepaid balance, card or crypto payment, no Anthropic account required.",
    keywords: ["buy claude api key", "how to buy claude api", "claude api key", "purchase claude api access", "anthropic api key", "buy claude api", "claude api access", "claude api tokens", "claude api top up", "claude api reseller", "claude api provider"],
    dek: "You do not need an Anthropic account, an invite, or a company card to buy a Claude API key. With apiToken.sale you top up prepaid balance, generate one key, and call the same Anthropic Messages API at a discount — instant Claude API access for Opus, Sonnet and Haiku.",
    sections: [
      { h2: "Get your key in three steps", blocks: [quickSetupSteps, cta()] },
      { h2: "How payment works", blocks: [
        { type: "p", text: "Top up any whole-dollar amount you like — there is no fixed product catalog. Your balance is prepaid, never expires, and is spent only when API requests run." },
        { type: "list", items: [
          "Pay by bank card or with cryptocurrency through a secure checkout provider.",
          "Every request is converted to official Anthropic API spend, then your active discount is applied.",
          "B2C accounts get a flat 50% off official spend on every request.",
        ] },
      ] },
      { h2: "What you can do with the key", blocks: [
        { type: "p", text: "One key unlocks the full supported Claude line — Opus, Sonnet and Haiku — across Claude Code, Cursor, Cline, Continue, Zed and the official Anthropic SDKs. Nothing about the protocol changes; only the price does." },
      ] },
      { h2: "Which Claude models and tools you get", blocks: [
        { type: "p", text: "One Claude API key unlocks the full supported line on a single balance, and works in every Anthropic-compatible tool." },
        { type: "list", items: [
          "Models: Claude Opus 4.8 and 4.7, Sonnet 5 and 4.6, Haiku 4.5.",
          "Tools: Claude Code, Cursor, Cline, Continue, Zed and the Anthropic SDKs.",
          "Formats: the Anthropic Messages API with streaming and tool use.",
        ] },
      ] },
    ],
    faq: [
      { q: "Do I need an Anthropic account to buy a Claude API key?", a: "No. apiToken.sale issues its own key and balance, so you can start without an Anthropic account, invite, or approval." },
      { q: "How fast is the key active?", a: "Instantly. You generate the key in the dashboard and it works on the next request — there is no waitlist or manual review." },
      { q: "How much does it cost to start?", a: "You can top up any whole-dollar amount. New accounts created with Google or GitHub also get $5 of platform bonus credit." },
      { q: "Is this the official Claude API?", a: "Yes — it serves the same Anthropic Messages API and the same Claude models. Only the price and the way you sign up and pay are different." },
    ],
    related: ["claude-api-quick-setup", "cheapest-claude-api", "claude-api-crypto-payment", "free-claude-api-key"],
    updated: "2026-07-17",
  },
  {
    slug: "cheapest-claude-api",
    cluster: "buy",
    title: "Cheapest Claude API — Flat 50% Discount",
    h1: "The cheapest way to use the Claude API",
    description: "The cheapest Claude API: buy discounted Claude API tokens at a flat 50% off. apiToken.sale sells the identical Anthropic API from prepaid balance with a flat Claude API discount.",
    keywords: ["cheapest claude api", "claude api discount", "claude api tokens", "discounted claude api", "cheap claude api", "claude api cheaper than anthropic", "buy claude api", "claude api access", "claude api top up", "claude api reseller", "claude api provider"],
    dek: "The Claude API is priced per token, and those tokens add up fast on long coding sessions. apiToken.sale gives you the identical API for 50% less by pooling prepaid balance and applying a flat discount.",
    sections: [
      { h2: "Why it is cheaper", blocks: [
        { type: "p", text: "You send the same request to the same Anthropic Messages API and get the same response. The only thing under the hood is billing: each call is metered at official rates, then your discount is subtracted before it touches your balance." },
        { type: "list", items: [
          "B2C accounts get a flat 50% off official spend.",
          "The same flat rate applies to every request — nothing to unlock.",
          "B2B volume pricing is negotiated separately.",
        ] },
      ] },
      { h2: "Where the savings show up most", blocks: [
        { type: "p", text: "Agentic coding, long multi-turn sessions and prompt-cache-heavy workflows burn the most tokens — so they see the biggest absolute savings. Choosing the right model for each task compounds it further." },
        { type: "note", text: "Tip: route quick, cheap work to Haiku and reserve Opus for hard reasoning to stretch your balance further." },
      ] },
      { h2: "No subscription, no lock-in", blocks: [
        { type: "p", text: "There is no monthly fee. You top up prepaid balance that never expires and spend it only when requests run, so idle days cost nothing." },
        cta(),
      ] },
      { h2: "How the Claude API discount is applied", blocks: [
        { type: "p", text: "There is no markup and no separate cheaper model — you get discounted access to the exact same Claude API." },
        { type: "list", items: [
          "Each request is metered at official Anthropic token rates.",
          "Your flat 50% discount is subtracted.",
          "The net amount is drawn from your prepaid balance.",
        ] },
        { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (\u221250%)"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "link", text: "Full per-model pricing, including cache rates", href: "/models" },
        { type: "link", text: "Estimate your monthly cost in the free calculator", href: "/tools/claude-api-cost-calculator" },
      ] },
    ],
    faq: [
      { q: "Is this really the same Claude API?", a: "Yes — the same Anthropic Messages API, same model IDs, same request and response format. Only the price per call is lower." },
      { q: "How much can I save?", a: "B2C pricing is a flat 50% below official API spend on every request." },
      { q: "Are there hidden fees or subscriptions?", a: "No. Balance is prepaid, never expires, and is consumed only by real API usage — there is no monthly charge." },
      { q: "Is there a cheaper Claude API than buying from Anthropic directly?", a: "Yes. apiToken.sale sells the identical Anthropic API at a flat 50% off official spend, with no subscription." },
    ],
    related: ["claude-api-pricing-explained", "save-tokens-on-claude-api", "apitoken-vs-anthropic-direct", "how-billing-works"],
    updated: "2026-07-17",
  },
  {
    slug: "claude-api-for-russia",
    cluster: "buy",
    title: "Claude API from Russia and Restricted Regions",
    h1: "Using the Claude API from Russia",
    description: "Access the Claude API from Russia and other restricted regions with apiToken.sale — no Anthropic account, pay by card or crypto, one key for every Claude model.",
    keywords: ["claude api russia", "claude api из россии", "anthropic api russia", "claude api restricted regions", "оплата claude api", "claude api без vpn", "buy claude api", "claude api access", "claude api tokens", "claude api top up", "claude api reseller"],
    dek: "Anthropic does not sell directly in every country, which leaves developers in Russia and other regions without an obvious way to pay. apiToken.sale removes that barrier: you buy prepaid balance and get a working Claude API key regardless of where Anthropic bills — no Anthropic account and no VPN required.",
    sections: [
      { h2: "Why direct access is hard", blocks: [
        { type: "p", text: "Signing up with Anthropic often requires a supported billing country and payment method. If you cannot complete that, you cannot get a key — even though the models themselves are reachable over the network." },
      ] },
      { h2: "How apiToken.sale solves it", blocks: [
        { type: "list", items: [
          "No Anthropic account needed — we issue the key and balance.",
          "Pay by bank card or cryptocurrency, whichever works for you.",
          "Instant activation, no waitlist, no company verification.",
        ] },
        cta(),
      ] },
      { h2: "Works with your existing tools", blocks: [
        { type: "p", text: `Point Claude Code, Cursor, Cline or the Anthropic SDK at ${BASE} and keep working exactly as before. Support is available in Russian and English over Telegram.` },
      ] },
      { h2: "Claude API from Russia without a VPN", blocks: [
        { type: "p", text: "There is no Anthropic billing-country gate on issuing your key and balance, so you do not need a foreign card or company to get started. Network reachability depends on your own connection, but nothing about buying balance or generating a key is region-locked." },
      ] },
    ],
    faq: [
      { q: "Can I pay from Russia?", a: "Yes. You can pay by bank card or with cryptocurrency through a checkout provider, so a supported Anthropic billing country is not required." },
      { q: "Do I need a VPN?", a: "You do not need an Anthropic account or billing country. Network reachability depends on your own connection, but there is no geographic gate on issuing your key and balance." },
      { q: "Is support available in Russian?", a: "Yes — support is available in Russian and English through Telegram." },
      { q: "Can I pay for the Claude API from Russia?", a: "Yes — pay by bank card or with cryptocurrency, so a supported Anthropic billing country is not required." },
    ],
    related: ["claude-api-crypto-payment", "claude-api-supported-countries", "how-to-buy-claude-api-key", "claude-api-without-waitlist"],
    updated: "2026-07-17",
  },
  {
    slug: "claude-api-crypto-payment",
    cluster: "buy",
    title: "Pay for the Claude API with Crypto",
    h1: "Pay for the Claude API with cryptocurrency",
    description: "Buy Claude API balance with cryptocurrency or bank card on apiToken.sale. No Anthropic account, instant activation, prepaid balance that never expires.",
    keywords: ["claude api crypto payment", "buy claude api with crypto", "claude api usdt", "pay anthropic api crypto", "claude api bitcoin", "buy claude api", "claude api access", "claude api tokens", "claude api top up", "claude api reseller", "claude api provider"],
    dek: "If a bank card is not an option — or you simply prefer crypto — you can fund your Claude API balance with cryptocurrency such as USDT or BTC and start immediately, with no Anthropic account.",
    sections: [
      { h2: "Card or crypto, your choice", blocks: [
        { type: "p", text: "At checkout you can pay by bank card or with cryptocurrency through a secure payment provider. Either way the balance lands prepaid in your account and is spent only when requests run." },
      ] },
      { h2: "Why crypto helps", blocks: [
        { type: "list", items: [
          "No supported Anthropic billing country required.",
          "Useful where cards are declined or unavailable.",
          "Balance never expires, so you fund once and draw down as you build.",
        ] },
        cta(),
      ] },
      { h2: "What to expect at checkout", blocks: [
        { type: "p", text: "Choose crypto at checkout, send the amount to the address shown, and your balance credits once the network confirms. Card remains available if you prefer it for a specific top-up." },
        { type: "list", items: [
          "Balance credits after on-chain confirmation.",
          "Any whole-dollar amount; balance never expires.",
          "Switch between card and crypto per top-up.",
        ] },
      ] },
      { h2: "Which cryptocurrencies you can pay with", blocks: [
        { type: "p", text: "Crypto top-ups go through a secure payment provider, so common coins are supported." },
        { type: "list", items: [
          "USDT and other stablecoins.",
          "BTC and major cryptocurrencies.",
          "Balance credits once the network confirms the transaction.",
        ] },
      ] },
    ],
    faq: [
      { q: "Which payment methods are supported?", a: "You can pay by bank card or with cryptocurrency through a checkout provider." },
      { q: "Does the balance expire?", a: "No. Prepaid balance never expires and is consumed only by real API usage." },
      { q: "Can I buy Claude API access with USDT?", a: "Yes — you can top up your Claude API balance with USDT or other supported cryptocurrencies at checkout." },
    ],
    related: ["claude-api-for-russia", "how-to-buy-claude-api-key", "how-billing-works", "claude-api-refund-policy"],
    updated: "2026-07-17",
  },
  {
    slug: "claude-api-without-waitlist",
    cluster: "buy",
    title: "Claude API With No Waitlist or Approval",
    h1: "Claude API access with no waitlist",
    description: "Skip the Anthropic waitlist and approval. Create an account on apiToken.sale, generate a Claude API key, and make your first call in minutes.",
    keywords: ["claude api no waitlist", "claude api instant access", "claude api without approval", "get claude api key fast", "claude api no anthropic account", "buy claude api", "claude api access", "claude api tokens", "claude api top up", "claude api reseller", "claude api provider"],
    dek: "Waiting for approval kills momentum. apiToken.sale gives you instant, self-serve access to every supported Claude model — no queue, no sales call, no company verification.",
    sections: [
      { h2: "Instant, self-serve access", blocks: [ quickSetupSteps, cta() ] },
      { h2: "What 'instant' actually means", blocks: [
        { type: "p", text: "The moment you generate a key it is live. There is no manual review step between signing up and your first successful request, so you can wire up a tool and ship in the same sitting." },
      ] },
      { h2: "From zero to first call", blocks: [
        { type: "list", items: [
          "Sign up and open the dashboard — no approval step.",
          "Generate a key and point your tool at router.apitoken.sale.",
          "Send a request and see it metered in your usage.",
        ] },
        { type: "p", text: "New accounts created with Google or GitHub also start with $5 of platform bonus credit, so you can validate the whole flow before topping up." },
      ] },
    ],
    faq: [
      { q: "Is there really no waitlist?", a: "Correct. Access is self-serve and instant — you generate a key and it works on the next request." },
      { q: "Do I need to talk to sales?", a: "No. B2C access is fully self-serve. Only negotiated B2B volume pricing involves a conversation." },
    ],
    related: ["how-to-buy-claude-api-key", "claude-api-quick-setup", "claude-api-activation-time", "free-claude-api-key"],
  },
  {
    slug: "claude-api-quick-setup",
    cluster: "integrate",
    title: "Claude API Setup in Two Minutes",
    h1: "Set up the Claude API in two minutes",
    description: "A two-minute Claude API quickstart: create a key, set your base URL to router.apitoken.sale, and send your first /v1/messages request with curl, Python or your IDE.",
    keywords: ["claude api quickstart", "claude api setup", "claude api first request", "anthropic messages api", "claude api base url", "buy claude api", "claude api access", "claude api tokens", "claude api top up", "claude api reseller", "claude api provider"],
    dek: "This is the fastest path from zero to a working Claude API call. Everything below uses the standard Anthropic Messages API, so it drops straight into your existing code.",
    sections: [
      { h2: "1. Create a key", blocks: [ { type: "p", text: "Sign up, open the dashboard, and generate a key. It looks like sk-pool-… and works across every supported model." } ] },
      { h2: "2. Set your endpoint", blocks: [
        { type: "p", text: "Point any Anthropic-compatible client at the gateway:" },
        { type: "code", code: `Base URL:  ${BASE}\nEndpoint:  POST /v1/messages\nHeaders:   x-api-key: ${KEY}\n           anthropic-version: 2023-06-01` },
      ] },
      { h2: "3. Send your first request", blocks: [
        { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        cta(),
      ] },
      { h2: "Common first-call errors", blocks: [
        { type: "list", items: [
          "401 Unauthorized — missing or wrong x-api-key, or wrong base URL.",
          "400 Bad Request — check the model ID and that max_tokens is set.",
          "429 Too Many Requests — respect Retry-After and lower concurrency.",
          "402 / insufficient balance — top up any whole-dollar amount.",
        ] },
      ] },
    ],
    faq: [
      { q: "What base URL do I use?", a: `Use ${BASE} with any Anthropic-compatible tool and send requests to /v1/messages. Existing integrations on the legacy https://api.apitoken.sale host keep working — the unified router is the recommended endpoint for new setups.` },
      { q: "Which auth header is required?", a: "Send x-api-key with your key and anthropic-version, exactly like the official Anthropic API." },
    ],
    related: ["claude-api-key-for-cursor", "anthropic-sdk-base-url", "how-to-buy-claude-api-key", "claude-api-for-vs-code"],
  },

  // ─────────────────────────── FREE ───────────────────────────
  {
    slug: "free-claude-api-key",
    cluster: "free",
    title: "Free Claude API Key to Get Started",
    h1: "Get a free Claude API key to start",
    description: "Create a Claude API key with Google or GitHub and get $5 of platform bonus credit — no card required, no Anthropic account, instant access.",
    keywords: ["free claude api key", "claude api free", "free claude api", "claude api free tier", "free anthropic api key", "claude api no card", "claude api no credit card", "claude api free credits", "try claude api free"],
    dek: "Create your account with Google or GitHub to receive $5 of platform bonus credit and make real calls before spending anything. Email and password accounts do not receive the bonus.",
    sections: [
      { h2: "What 'free' includes", blocks: [
        { type: "list", items: [
          "A working API key across all supported Claude models.",
          "A one-time $5 platform welcome bonus for new Google/GitHub accounts, no card required.",
          "Enough headroom to wire up your tools and run genuine requests.",
        ] },
        { type: "p", text: "When you are ready for more, top up any whole-dollar amount and your discount kicks in automatically." },
      ] },
      { h2: "How to claim it", blocks: [
        { type: "p", text: "Choose Google or GitHub when creating the account. Registering with email and password creates a usable account but does not grant the welcome bonus." },
        quickSetupSteps,
      ] },
      { h2: "Is the Claude API free forever?", blocks: [
        { type: "p", text: "The included $5 platform bonus is a free start, not an unlimited free tier. After it, you pay only for the tokens you use — there is no subscription and no monthly minimum, and your prepaid balance never expires." },
      ] },
    ],
    faq: [
      { q: "Is the free usage real API access?", a: "Yes. The $5 Google/GitHub platform bonus runs against the same supported models and endpoints as paid balance." },
      { q: "Do I need a card to start?", a: "No card is required. Create the account with Google or GitHub to receive the included $5 platform bonus." },
      { q: "Do I need a credit card for a free Claude API key?", a: "No. Create the account with Google or GitHub to receive the included $5 platform bonus without a card." },
    ],
    related: ["claude-api-free-trial", "how-to-buy-claude-api-key", "claude-code-without-subscription", "cheapest-claude-api"],
    updated: "2026-07-17",
  },
  {
    slug: "claude-api-free-trial",
    cluster: "free",
    title: "Claude API Free Trial — Start in Minutes",
    h1: "Try the Claude API free",
    description: "Start coding with Claude in minutes. New accounts created with Google or GitHub get $5 of platform bonus credit, with no card required.",
    keywords: ["claude api free trial", "try claude api", "claude api test", "claude api sandbox", "claude api demo", "free claude api", "claude api free", "claude api no credit card", "claude api free credits", "try claude api free", "claude api free tier"],
    dek: "There is no separate trial to apply for — create the account with Google or GitHub to get $5 of platform bonus credit and run real calls against every supported model.",
    sections: [
      { h2: "Prove it before you pay", blocks: [
        { type: "p", text: "The included usage is designed to check the gateway end to end: create a key, connect your editor, and confirm streaming, tool use and your favorite model all behave as expected." },
        cta(),
      ] },
      { h2: "Then scale on your terms", blocks: [
        { type: "p", text: "When the trial usage runs low, top up any amount. There is no subscription and balance never expires, so you only ever pay for what you actually call." },
      ] },
    ],
    faq: [
      { q: "How do I start the trial?", a: "Create a new account with Google or GitHub. The $5 platform bonus is added automatically; email and password accounts are not eligible." },
      { q: "What happens when the free usage runs out?", a: "Top up any whole-dollar amount to keep going; your flat discount applies immediately." },
    ],
    related: ["free-claude-api-key", "claude-api-without-waitlist", "claude-api-quick-setup", "claude-haiku-api"],
  },
  {
    slug: "claude-code-without-subscription",
    cluster: "free",
    title: "Use Claude Code Without a Subscription",
    h1: "Claude Code without the $200/month plan",
    description: "Run Claude Code on pay-as-you-go API balance instead of a monthly subscription. Set ANTHROPIC_BASE_URL to router.apitoken.sale and pay only for what you use.",
    keywords: ["claude code without subscription", "claude code api key", "claude code pay as you go", "claude code cheap", "claude code no subscription", "free claude api", "claude api free", "claude api no credit card", "claude api free credits", "try claude api free", "claude api free tier"],
    dek: "Claude Code does not have to mean a fixed monthly plan. Point it at an API key with prepaid balance and you pay per token — ideal if your usage is spiky or you just want to try it.",
    sections: [
      { h2: "Two environment variables", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# then just run\nclaude` },
        { type: "p", text: "That is the entire change. Claude Code keeps every feature — it simply bills against your prepaid balance at a discount instead of a subscription." },
      ] },
      { h2: "When pay-as-you-go wins", blocks: [
        { type: "list", items: [
          "Occasional or bursty usage where a flat monthly fee is wasteful.",
          "Trying Claude Code before committing to a plan.",
          "Keeping several tools on one balance and one key.",
        ] },
        cta(),
      ] },
    ],
    faq: [
      { q: "Does Claude Code work with a custom API key?", a: "Yes. Set ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY and Claude Code uses your key and balance directly." },
      { q: "Do I lose any features?", a: "No. Claude Code behaves identically; only billing changes from a subscription to prepaid per-token usage." },
    ],
    related: ["claude-api-key-for-cursor", "cheapest-claude-api", "claude-opus-api", "anthropic-sdk-base-url"],
  },
  {
    slug: "claude-opus-api",
    cluster: "free",
    title: "Claude Opus API Access",
    h1: "Claude Opus 4.8 through the API",
    description: "Access Claude Opus 4.8 and 4.7 through one apiToken.sale key at a flat 50% off official rates. Best for complex reasoning, refactors and long agent sessions.",
    keywords: ["claude opus api", "claude opus 4.8 api", "opus api key", "claude opus pricing", "claude opus discount", "free claude api", "claude api free", "claude api no credit card", "claude api free credits", "try claude api free", "claude api free tier"],
    dek: "Opus is Claude's most capable tier — the model to reach for on hard reasoning, architecture and long agentic runs. apiToken.sale gives you Opus 4.8 and 4.7 on the same key and balance as every other model.",
    sections: [
      { h2: "When to use Opus", blocks: [
        { type: "list", items: [
          "Complex refactors and multi-file changes.",
          "Architecture, planning and high-stakes reasoning.",
          "Long sessions where consistency and cache reuse matter.",
        ] },
      ] },
      { h2: "Opus on your balance", blocks: [
        { type: "p", text: "Opus 4.8 (model ID claude-opus-4-8) and Opus 4.7 are billed at official token rates minus your discount, so you get the top tier for a fraction of the list price." },
        { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (\u221250%)"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
        ] },
        { type: "link", text: "Claude Opus 4.8 pricing in detail (cache rates, context, FAQ)", href: "/models/claude-opus-4-8" },
        cta(),
      ] },
    ],
    faq: [
      { q: "Which Opus models are available?", a: "Claude Opus 4.8 (claude-opus-4-8) and Claude Opus 4.7, on the same key and prepaid balance as Sonnet and Haiku." },
      { q: "Is Opus worth the extra tokens?", a: "For complex reasoning, refactors and long agent runs, yes. For fast, cheap tasks, Haiku or Sonnet is usually the better value." },
    ],
    related: ["claude-opus-vs-sonnet", "claude-sonnet-api", "claude-haiku-api", "save-tokens-on-claude-api"],
    updated: "2026-07-17",
  },
  {
    slug: "claude-sonnet-api",
    cluster: "free",
    title: "Claude Sonnet API Access",
    h1: "Claude Sonnet through the API",
    description: "Use Claude Sonnet 5 and Sonnet 4.6 through apiToken.sale — the default model for daily coding and agents, at a flat 50% off official API pricing.",
    keywords: ["claude sonnet api", "claude sonnet 5 api", "sonnet api key", "claude sonnet pricing", "best claude model for coding", "free claude api", "claude api free", "claude api no credit card", "claude api free credits", "try claude api free", "claude api free tier"],
    dek: "Sonnet is the workhorse: fast enough for interactive coding, smart enough for real agent workflows. apiToken.sale serves Sonnet 5 and Sonnet 4.6 on one discounted balance.",
    sections: [
      { h2: "The daily-driver model", blocks: [
        { type: "p", text: "For most coding and agent tasks, Sonnet is the right default — a strong balance of quality, speed and cost. Reserve Opus for the genuinely hard problems." },
      ] },
      { h2: "Sonnet pricing note", blocks: [
        { type: "p", text: "Claude Sonnet 5 (claude-sonnet-5) ships with introductory official rates, and the engine always applies the current effective rate before your discount. Sonnet 4.6 remains available on the same key." },
        { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (\u221250%)"], rows: [
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
        ] },
        { type: "link", text: "Claude Sonnet 5 pricing in detail (cache rates, context, FAQ)", href: "/models/claude-sonnet-5" },
        cta(),
      ] },
    ],
    faq: [
      { q: "Which Sonnet models can I use?", a: "Claude Sonnet 5 (claude-sonnet-5) and Claude Sonnet 4.6, on the same balance as Opus and Haiku." },
      { q: "Is Sonnet good for coding?", a: "Yes — Sonnet is the recommended default for everyday coding and agent workflows." },
    ],
    related: ["claude-opus-vs-sonnet", "claude-opus-api", "claude-haiku-api", "claude-api-key-for-cursor"],
    updated: "2026-07-17",
  },
  {
    slug: "claude-haiku-api",
    cluster: "free",
    title: "Claude Haiku API Access",
    h1: "Claude Haiku 4.5 through the API",
    description: "Access Claude Haiku 4.5 through apiToken.sale — the fastest, most economical Claude model, ideal for high-volume and low-latency tasks, at a prepaid discount.",
    keywords: ["claude haiku api", "claude haiku 4.5 api", "fastest claude model", "cheap claude model", "haiku api key", "free claude api", "claude api free", "claude api no credit card", "claude api free credits", "try claude api free", "claude api free tier"],
    dek: "Haiku is built for speed and volume: classification, extraction, routing and any task where latency and cost matter more than deep reasoning.",
    sections: [
      { h2: "When Haiku is the right call", blocks: [
        { type: "list", items: [
          "High-volume, low-latency requests.",
          "Cheap background tasks and pre-processing.",
          "Stretching your balance on work that does not need Opus.",
        ] },
        cta(),
      ] },
      { h2: "Mix models on one key", blocks: [
        { type: "p", text: "Because every model shares one key and balance, you can route cheap work to Haiku (claude-haiku-4-5) and escalate only the hard requests to Sonnet or Opus." },
        { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (\u221250%)"], rows: [
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "link", text: "Claude Haiku 4.5 pricing in detail (cache rates, context, FAQ)", href: "/models/claude-haiku-4-5" },
      ] },
    ],
    faq: [
      { q: "How fast and cheap is Haiku?", a: "Haiku 4.5 is the fastest and lowest-cost Claude model, ideal for high-volume, latency-sensitive work." },
      { q: "Can I combine Haiku with other models?", a: "Yes. One key and balance covers Haiku, Sonnet and Opus, so you can route each task to the best-value model." },
    ],
    related: ["claude-sonnet-api", "claude-opus-api", "save-tokens-on-claude-api", "cheapest-claude-api"],
    updated: "2026-07-17",
  },

  // ─────────────────────────── INTEGRATE ───────────────────────────
  {
    slug: "claude-api-key-for-cursor",
    cluster: "integrate",
    title: "Claude API Key for Cursor",
    h1: "Use a Claude API key in Cursor",
    description: "Connect Cursor to Claude with an apiToken.sale key: set the Anthropic base URL to router.apitoken.sale, paste your key, pick a model, and code at a flat 50% off.",
    keywords: ["claude api key for cursor", "cursor claude api", "cursor anthropic key", "use claude in cursor", "cursor without cursor pro", "claude api key", "anthropic-compatible api", "claude api base url", "claude api setup", "claude api integration", "claude api cursor"],
    dek: "Cursor lets you bring your own Anthropic key, which means you can run Claude in Cursor on discounted prepaid balance instead of a bundled plan.",
    sections: [
      { h2: "Three-step setup", blocks: [
        { type: "steps", items: [
          "Open Cursor → Settings → Models → Anthropic API.",
          `Set the base URL to ${BASE} and paste your ${KEY} key.`,
          "Pick a model such as claude-opus-4-8 and start coding.",
        ] },
      ] },
      { h2: "Configuration", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : ${BASE}\nAPI key  : ${KEY}\nModel    : claude-opus-4-8` },
        cta(),
      ] },
      { h2: "Troubleshooting", blocks: [
        { type: "list", items: [
          "Cursor ignores the key: confirm you edited the Anthropic provider, not OpenAI.",
          "Model not found: set a current model ID like claude-opus-4-8.",
          "401: re-check the base URL and that the key was pasted in full.",
        ] },
        { type: "p", text: "Once connected, every supported Claude model is available on the same key and balance." },
      ] },
      { h2: "Your Claude API key in Cursor for any language", blocks: [
        { type: "p", text: "The key is language-agnostic — Cursor uses it for Python, JavaScript, TypeScript, Go, Rust or any project, on Windows, macOS and Linux. You are configuring the model provider, not the language." },
      ] },
    ],
    faq: [
      { q: "Can I use my own Claude key in Cursor?", a: "Yes. Cursor's Anthropic provider accepts a custom base URL and key, so you can point it at apiToken.sale." },
      { q: "Do I still need Cursor Pro?", a: "You can run Claude through your own API key and balance; features that require Cursor's own plan are separate from the model provider." },
      { q: "Does the Claude API key work in Cursor on Windows and Mac?", a: "Yes — the Anthropic provider setting is the same across Windows, macOS and Linux." },
    ],
    related: ["cursor-without-anthropic-account", "claude-api-for-vs-code", "claude-api-quick-setup", "claude-sonnet-api"],
    updated: "2026-07-17",
  },
  {
    slug: "claude-api-for-vs-code",
    cluster: "integrate",
    title: "Claude API in VS Code (Cline, Continue)",
    h1: "Use the Claude API in VS Code",
    description: "Run Claude in VS Code with Cline or Continue using an apiToken.sale key. Set the Anthropic base URL to router.apitoken.sale and pay per token at a discount.",
    keywords: ["claude api vs code", "cline claude api", "continue claude api", "claude in vscode", "vscode anthropic api key", "claude api key", "anthropic-compatible api", "claude api base url", "claude api setup", "claude api integration", "claude api vscode"],
    dek: "Free VS Code agents like Cline and Continue accept any Anthropic-compatible endpoint, so you can code with Claude inside VS Code on discounted balance.",
    sections: [
      { h2: "Cline", blocks: [
        { type: "code", code: `# Cline → Settings\nAPI Provider : Anthropic\nBase URL     : ${BASE}\nAPI Key      : ${KEY}\nModel        : claude-opus-4-8` },
      ] },
      { h2: "Continue", blocks: [
        { type: "code", code: `// ~/.continue/config.json\n{\n  "models": [{\n    "title": "Claude via apiToken.sale",\n    "provider": "anthropic",\n    "apiBase": "${BASE}",\n    "apiKey": "${KEY}",\n    "model": "claude-opus-4-8"\n  }]\n}` },
        cta(),
      ] },
      { h2: "Which extension and troubleshooting", blocks: [
        { type: "p", text: "Cline is a strong default for autonomous edits; Continue is lighter and good for inline chat and completions. Both are free and use your prepaid balance." },
        { type: "list", items: [
          "401 Unauthorized: the API key or base URL is wrong.",
          "Model not found: use a current ID such as claude-sonnet-5 or claude-opus-4-8.",
          "Slow or 429: reduce concurrency and respect Retry-After.",
        ] },
      ] },
    ],
    faq: [
      { q: "Which VS Code extensions work?", a: "Any extension that supports an Anthropic-compatible endpoint, including Cline and Continue, works with an apiToken.sale key." },
      { q: "Do I need a paid extension?", a: "No. Cline and Continue are free; you only pay for the Claude API usage against your prepaid balance." },
    ],
    related: ["claude-api-key-for-cursor", "anthropic-sdk-base-url", "claude-api-quick-setup", "claude-code-without-subscription"],
  },
  {
    slug: "cursor-without-anthropic-account",
    cluster: "integrate",
    title: "Claude in Cursor Without an Anthropic Account",
    h1: "Run Claude in Cursor without an Anthropic account",
    description: "No Anthropic account? Use Claude in Cursor with an apiToken.sale key instead. Instant access, card or crypto payment, and a flat 50% off official API rates.",
    keywords: ["cursor without anthropic account", "claude cursor no anthropic", "cursor claude api key", "use claude without anthropic account", "claude api key", "anthropic-compatible api", "claude api base url", "claude api setup", "claude api integration", "claude api python", "claude api typescript"],
    dek: "If you cannot or would rather not create an Anthropic account, apiToken.sale issues its own key that Cursor accepts as an Anthropic provider.",
    sections: [
      { h2: "Why this works", blocks: [
        { type: "p", text: "Cursor talks to the Anthropic Messages API. apiToken.sale exposes exactly that API, so Cursor cannot tell the difference — it just uses your key and base URL." },
      ] },
      { h2: "Set it up", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : ${BASE}\nAPI key  : ${KEY}\nModel    : claude-opus-4-8` },
        cta(),
      ] },
      { h2: "What you keep", blocks: [
        { type: "list", items: [
          "The full Claude line — Opus, Sonnet and Haiku — on one key.",
          "Standard Anthropic behaviour: streaming, tool use, system prompts.",
          "An optional lifetime spending limit and expiration date per key, plus token-level usage in the dashboard.",
        ] },
        { type: "p", text: "Nothing about how you use Cursor changes; you simply source the key from apiToken.sale instead of Anthropic." },
      ] },
    ],
    faq: [
      { q: "Do I need an Anthropic account for this?", a: "No. apiToken.sale provides the key and balance, so no Anthropic account is required." },
      { q: "Is the integration official Anthropic API?", a: "Cursor uses the standard Anthropic Messages API; apiToken.sale serves that same API at a discount." },
    ],
    related: ["claude-api-key-for-cursor", "claude-api-for-russia", "how-to-buy-claude-api-key", "apitoken-vs-anthropic-direct"],
  },
  {
    slug: "anthropic-sdk-base-url",
    cluster: "integrate",
    title: "Use Anthropic SDKs with a Custom Base URL",
    h1: "Point the Anthropic SDK at apiToken.sale",
    description: "Use the official Anthropic Python and TypeScript SDKs with apiToken.sale by setting base_url to router.apitoken.sale. Same SDK, same code, lower cost per token.",
    keywords: ["anthropic sdk base url", "anthropic python sdk custom endpoint", "claude sdk base url", "anthropic typescript sdk", "claude api sdk"],
    dek: "The official Anthropic SDKs let you override the base URL, so switching to apiToken.sale is a one-line change — your model IDs and message code stay exactly the same.",
    sections: [
      { h2: "Python", blocks: [
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="${BASE}",\n    api_key="${KEY}",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
      ] },
      { h2: "TypeScript", blocks: [
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "${BASE}",\n  apiKey: "${KEY}",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
        cta(),
      ] },
      { h2: "Verify the switch worked", blocks: [
        { type: "p", text: "After changing the base URL, make one request and confirm you get a normal Anthropic response. Streaming, tool use and system prompts all behave exactly as with api.anthropic.com — only the billing endpoint changed." },
        { type: "list", items: [
          "A 401 means the key or base URL is wrong — re-check both.",
          "Keep the same model IDs; no code around messages needs to change.",
          "Read usage per request in the dashboard to confirm spend and your discount.",
        ] },
      ] },
    ],
    faq: [
      { q: "Can I keep using the official Anthropic SDK?", a: "Yes. Set base_url (Python) or baseURL (TypeScript) to apiToken.sale and everything else stays the same." },
      { q: "Do model IDs change?", a: "No. Use the same model IDs such as claude-opus-4-8 and claude-sonnet-5." },
    ],
    related: ["claude-api-quick-setup", "claude-api-for-vs-code", "claude-code-without-subscription", "claude-api-key-for-cursor"],
  },
  {
    slug: "claude-api-langchain",
    cluster: "integrate",
    title: "Use the Claude API with LangChain",
    h1: "Use the Claude API with LangChain",
    description: "Connect LangChain to Claude through apiToken.sale: point ChatAnthropic at router.apitoken.sale, keep the same model IDs, and pay 50% less per token.",
    keywords: ["claude api langchain", "langchain anthropic", "langchain claude", "chatanthropic base url", "langchain claude api key", "langchain anthropic_api_url"],
    dek: "LangChain's Anthropic integration accepts a custom API URL, so your chains and agents can run on Claude through apiToken.sale with a two-line change — same models, lower token price.",
    published: "2026-07-17",
    updated: "2026-07-17",
    sections: [
      { h2: "Point ChatAnthropic at the gateway", blocks: [
        { type: "code", code: `from langchain_anthropic import ChatAnthropic\n\nllm = ChatAnthropic(\n    model="claude-opus-4-8",\n    anthropic_api_url="${BASE}",\n    anthropic_api_key="${KEY}",\n)\nprint(llm.invoke("Hello").content)` },
        { type: "p", text: "That is the whole integration: the same langchain-anthropic package, the same model IDs, the same streaming and tool-calling — only the endpoint and the price change." },
        cta(),
      ] },
      { h2: "Or configure via environment variables", blocks: [
        { type: "code", code: `export ANTHROPIC_API_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}` },
        { type: "p", text: "With the environment set, ChatAnthropic picks up both values automatically, so shared codebases need no code change at all." },
      ] },
      { h2: "What works", blocks: [
        { type: "list", items: [
          "Chains, agents and LangGraph workflows — the protocol is unchanged.",
          "Streaming, tool calling and structured output through the standard integration.",
          "Every supported Claude model (Opus, Sonnet, Haiku) on one key and balance.",
        ] },
      ] },
    ],
    faq: [
      { q: "Does LangChain work with a custom Claude API endpoint?", a: "Yes. ChatAnthropic accepts anthropic_api_url (or the ANTHROPIC_API_URL environment variable), so you can point it at https://router.apitoken.sale and keep everything else unchanged." },
      { q: "Do LangChain agents and tool calling still work?", a: "Yes — the gateway serves the standard Anthropic Messages API, so tool calling, streaming and LangGraph agents behave exactly as with the official endpoint." },
      { q: "Which models can I use from LangChain?", a: "All supported Claude models — claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5 and more — on the same key and prepaid balance." },
    ],
    related: ["anthropic-sdk-base-url", "claude-api-litellm", "claude-api-quick-setup", "cheapest-claude-api"],
  },
  {
    slug: "claude-api-litellm",
    cluster: "integrate",
    title: "Use the Claude API with LiteLLM",
    h1: "Use the Claude API with LiteLLM",
    description: "Route LiteLLM to Claude through apiToken.sale: set api_base to router.apitoken.sale in litellm_params or the proxy config and pay 50% less per token.",
    keywords: ["claude api litellm", "litellm anthropic", "litellm claude", "litellm api_base anthropic", "litellm proxy claude", "litellm claude api key"],
    dek: "LiteLLM speaks to Anthropic natively and lets you override the endpoint per model, so one config line sends all your Claude traffic through the discounted gateway.",
    published: "2026-07-17",
    updated: "2026-07-17",
    sections: [
      { h2: "Direct SDK call", blocks: [
        { type: "code", code: `import litellm\n\nresponse = litellm.completion(\n    model="anthropic/claude-opus-4-8",\n    api_base="${BASE}",\n    api_key="${KEY}",\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
        cta(),
      ] },
      { h2: "LiteLLM proxy config", blocks: [
        { type: "code", code: `# config.yaml\nmodel_list:\n  - model_name: claude-opus-4-8\n    litellm_params:\n      model: anthropic/claude-opus-4-8\n      api_base: ${BASE}\n      api_key: ${KEY}` },
        { type: "p", text: "Run the proxy with this config and every client of your LiteLLM gateway transparently uses the discounted Claude endpoint — useful when many services share one routing layer." },
      ] },
      { h2: "Why route Claude through LiteLLM here", blocks: [
        { type: "list", items: [
          "One place to switch all services to the cheaper endpoint.",
          "Same anthropic/ model prefix and parameters you already use.",
          "Spend tracked per key in the apiToken.sale dashboard with token detail.",
        ] },
      ] },
    ],
    faq: [
      { q: "Does LiteLLM support a custom Anthropic api_base?", a: "Yes — pass api_base in litellm.completion() or in litellm_params in the proxy config, and LiteLLM sends Anthropic-format requests to https://router.apitoken.sale." },
      { q: "Do I keep the anthropic/ model prefix?", a: "Yes. Use anthropic/claude-opus-4-8 (or any supported model) so LiteLLM applies the Anthropic protocol; only the endpoint and key change." },
      { q: "Does this work for tools built on LiteLLM?", a: "Yes — anything that routes through LiteLLM (including many coding agents) inherits the discounted endpoint from the same configuration." },
    ],
    related: ["claude-api-langchain", "claude-api-aider", "anthropic-sdk-base-url", "claude-api-gateway"],
  },
  {
    slug: "claude-api-aider",
    cluster: "integrate",
    title: "Use the Claude API with Aider",
    h1: "Use the Claude API with Aider",
    description: "Run Aider on Claude through apiToken.sale: export ANTHROPIC_API_BASE and your key, pick a Claude model, and pair-program in the terminal at a flat 50% off.",
    keywords: ["claude api aider", "aider anthropic", "aider claude", "aider anthropic api base", "aider claude api key", "aider cheap claude"],
    dek: "Aider is a terminal pair-programmer that burns tokens fast on long sessions. Point it at the discounted gateway with two environment variables and keep the exact same workflow.",
    published: "2026-07-17",
    updated: "2026-07-17",
    sections: [
      { h2: "Two environment variables", blocks: [
        { type: "code", code: `export ANTHROPIC_API_KEY=${KEY}\nexport ANTHROPIC_API_BASE=${BASE}\n\naider --model anthropic/claude-opus-4-8` },
        { type: "p", text: "Aider routes Anthropic traffic through LiteLLM under the hood, which honours ANTHROPIC_API_BASE — so no config file is required." },
        cta(),
      ] },
      { h2: "Picking a model for Aider", blocks: [
        { type: "list", items: [
          "anthropic/claude-opus-4-8 — hardest refactors and long agentic edits.",
          "anthropic/claude-sonnet-5 — the everyday default; near-Opus coding quality.",
          "anthropic/claude-haiku-4-5 — quick edits and cheap experimentation.",
        ] },
        { type: "p", text: "Long Aider sessions are exactly where the token discount compounds: repo maps, diffs and multi-file edits all bill as input and output tokens." },
      ] },
    ],
    faq: [
      { q: "Does Aider work with a custom Claude endpoint?", a: "Yes. Aider uses LiteLLM for Anthropic models, and LiteLLM honours the ANTHROPIC_API_BASE environment variable — set it to https://router.apitoken.sale and start Aider normally." },
      { q: "Which Claude model is best in Aider?", a: "claude-sonnet-5 is the best default for most coding; switch to claude-opus-4-8 for the hardest multi-file work. Both run on the same key." },
      { q: "How much cheaper is a long Aider session?", a: "Every request is billed at official token rates minus your flat 50% discount, so a session that would cost $10 direct costs $5 here." },
    ],
    related: ["claude-api-litellm", "claude-code-without-subscription", "best-claude-model-for-coding", "save-tokens-on-claude-api"],
  },
  {
    slug: "claude-api-roo-code",
    cluster: "integrate",
    title: "Use the Claude API with Roo Code",
    h1: "Use the Claude API with Roo Code",
    description: "Connect Roo Code in VS Code to Claude through apiToken.sale: choose the Anthropic provider, enable the custom base URL, paste your key and code at a flat 50% off.",
    keywords: ["claude api roo code", "roo code anthropic", "roo code claude", "roo code custom base url", "roo code api key", "roo code cheap claude"],
    dek: "Roo Code is an agentic VS Code extension with a native Anthropic provider and a custom base URL option — which makes it a two-minute setup on the discounted gateway.",
    published: "2026-07-17",
    updated: "2026-07-17",
    sections: [
      { h2: "Setup in three steps", blocks: [
        { type: "steps", items: [
          "Open Roo Code settings and choose Anthropic as the API provider.",
          `Enable the custom base URL option and set it to ${BASE}; paste your sk-pool-… key.`,
          "Pick a model such as claude-opus-4-8 or claude-sonnet-5 and start a task.",
        ] },
        cta(),
      ] },
      { h2: "Why Roo Code burns tokens — and how to pay less", blocks: [
        { type: "p", text: "Agentic extensions read files, plan, edit and re-check in loops, so a single task can run many model calls. That is precisely the workload where a per-token discount matters most: the same session, 50% cheaper, with token-level visibility in the dashboard." },
        { type: "list", items: [
          "Route everyday tasks to claude-sonnet-5 and hard ones to claude-opus-4-8.",
          "Prompt caching is billed at the cheaper official cache rates minus your discount.",
          "One key covers Roo Code, Cline, Cursor and the SDKs simultaneously.",
        ] },
      ] },
    ],
    faq: [
      { q: "Does Roo Code support a custom Anthropic base URL?", a: "Yes — the Anthropic provider settings include a custom base URL option; set it to https://router.apitoken.sale and use your apiToken.sale key." },
      { q: "Which models does Roo Code get on this key?", a: "Every supported Claude model — Opus 4.8 and 4.7, Sonnet 5 and 4.6, Haiku 4.5 — on one key and one prepaid balance." },
      { q: "Is this different from using Cline?", a: "The setup is nearly identical: both are VS Code agents with an Anthropic provider that accepts a custom base URL. Use whichever agent you prefer; the key works in both." },
    ],
    related: ["claude-api-for-vs-code", "claude-api-key-for-cursor", "claude-api-langchain", "cheapest-claude-api"],
  },

  // ─────────────────────────── COMPARE ───────────────────────────
  {
    slug: "apitoken-vs-anthropic-direct",
    cluster: "compare",
    title: "apiToken.sale vs Anthropic Direct",
    h1: "apiToken.sale vs buying from Anthropic directly",
    description: "Compare apiToken.sale and Anthropic direct: identical Messages API and models, but with a flat 50% off, no account requirement, and card or crypto payment.",
    keywords: ["claude api vs anthropic direct", "apitoken vs anthropic", "anthropic api alternative", "cheaper than anthropic api", "claude api reseller", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
    dek: "apiToken.sale is not a different API — it is the same Anthropic Messages API, resold from prepaid balance at a discount. Here is what actually changes and what does not.",
    sections: [
      { h2: "What stays the same", blocks: [
        { type: "list", items: [
          "The same Anthropic Messages API, endpoints and streaming.",
          "The same model IDs (claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5).",
          "The same request and response format your code already expects.",
        ] },
      ] },
      { h2: "What changes", blocks: [
        { type: "list", items: [
          "Price: a flat 50% below official spend for B2C.",
          "Onboarding: no Anthropic account, waitlist or billing-country requirement.",
          "Payment: bank card or cryptocurrency.",
        ] },
        cta(),
      ] },
      { h2: "Who each is for", blocks: [
        { type: "p", text: "If you already have frictionless Anthropic billing and enterprise agreements, direct may suit you. If you want the same models cheaper, faster to start, and payable by card or crypto, apiToken.sale is the pragmatic choice." },
      ] },
    ],
    faq: [
      { q: "Is apiToken.sale the real Claude API?", a: "Yes — it serves the same Anthropic Messages API and models. Only pricing and onboarding differ." },
      { q: "Why is it cheaper than Anthropic direct?", a: "Balance is prepaid and pooled, and a flat 50% discount is applied to official spend." },
    ],
    related: ["cheapest-claude-api", "apitoken-vs-openrouter", "claude-api-pricing-explained", "how-billing-works"],
  },
  {
    slug: "apitoken-vs-openrouter",
    cluster: "compare",
    title: "apiToken.sale vs OpenRouter for Claude",
    h1: "apiToken.sale vs OpenRouter for Claude",
    description: "Choosing a Claude gateway? Compare apiToken.sale and OpenRouter: a native Anthropic endpoint and prepaid discount vs a multi-provider router.",
    keywords: ["openrouter alternative", "apitoken vs openrouter", "claude api gateway", "openrouter claude", "best claude api gateway", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
    dek: "Both let you reach Claude without an Anthropic account, but they are built differently. If Claude is your main model, a native Anthropic endpoint keeps things simple.",
    sections: [
      { h2: "Native Anthropic endpoint", blocks: [
        { type: "p", text: `apiToken.sale exposes the standard Anthropic Messages API at ${BASE}, so Claude Code, Cursor and the Anthropic SDKs work with zero adapters. You are not routing through a generic multi-provider abstraction.` },
      ] },
      { h2: "Prepaid discount, not markup", blocks: [
        { type: "list", items: [
          "Flat 50% B2C discount off official Claude spend.",
          "One key and balance for Opus, Sonnet and Haiku.",
          "Card or crypto top-ups that never expire.",
        ] },
        cta(),
      ] },
      { h2: "When to choose each", blocks: [
        { type: "list", items: [
          "apiToken.sale — Claude is your main model and you want a native Anthropic endpoint with a discount.",
          "OpenRouter — you need to route across many providers behind one abstraction.",
          "Both let you start without an Anthropic account; only apiToken.sale discounts Claude spend directly.",
        ] },
      ] },
    ],
    faq: [
      { q: "Why pick a Claude-native gateway?", a: "If Claude is your primary model, a native Anthropic endpoint means your existing Anthropic tools and SDKs work unchanged." },
      { q: "Does apiToken.sale mark prices up?", a: "No — it applies a discount to official Claude spend rather than adding a markup." },
    ],
    related: ["apitoken-vs-anthropic-direct", "cheapest-claude-api", "claude-api-quick-setup", "anthropic-sdk-base-url"],
  },
  {
    slug: "claude-opus-vs-sonnet",
    cluster: "compare",
    title: "Claude Opus vs Sonnet — Which to Use",
    h1: "Claude Opus vs Sonnet: which model to use",
    description: "Opus or Sonnet? A practical guide to picking the right Claude model for coding and agents — and using both on one apiToken.sale key and balance.",
    keywords: ["claude opus vs sonnet", "which claude model", "opus or sonnet coding", "best claude model", "claude model comparison", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
    dek: "Opus and Sonnet solve different problems. Choosing well is the easiest way to get better results and spend fewer tokens — and you can keep both on one key.",
    sections: [
      { h2: "Use Sonnet by default", blocks: [
        { type: "p", text: "Sonnet 5 and Sonnet 4.6 handle the vast majority of coding and agent work quickly and cost-effectively. Start here." },
      ] },
      { h2: "Escalate to Opus for hard problems", blocks: [
        { type: "p", text: "Reach for Opus 4.8 on complex refactors, architecture, and long high-stakes sessions where extra reasoning pays for itself." },
        { type: "note", text: "Because one key covers both, you can route each task to the right tier without juggling providers." },
        { type: "table", headers: ["", "Claude Opus 4.8", "Claude Sonnet 5"], rows: [
          ["Official price (in / out per 1M)", "$5 / $25", "$3 / $15"],
          ["Here (\u221250%)", "$2.50 / $12.50", "$1.50 / $7.50"],
          ["Context window", "1M tokens", "1M tokens"],
          ["Best for", "Hard reasoning, long agent runs", "Everyday coding and agents"],
        ] },
        { type: "link", text: "Compare all Claude models and prices", href: "/models" },
        cta(),
      ] },
    ],
    faq: [
      { q: "Which is better for coding?", a: "Sonnet is the recommended default for daily coding; use Opus for complex reasoning and long refactors." },
      { q: "Can I use both on one account?", a: "Yes. Opus, Sonnet and Haiku all share the same key and prepaid balance." },
    ],
    related: ["claude-opus-api", "claude-sonnet-api", "claude-haiku-api", "save-tokens-on-claude-api"],
    updated: "2026-07-17",
  },

  // ─────────────────────────── EXPLAIN ───────────────────────────
  {
    slug: "claude-api-pricing-explained",
    cluster: "explain",
    title: "Claude API Pricing Explained",
    h1: "How Claude API pricing works",
    description: "Understand Claude API pricing: per-token input and output rates, prompt caching, and how apiToken.sale applies a flat 50% discount.",
    keywords: ["claude api pricing", "claude api tokens", "claude token pricing", "claude api cost", "how claude api pricing works", "anthropic api pricing explained", "how claude api works", "claude api explained", "anthropic api"],
    dek: "Claude is billed per token — separately for input and output — with discounts for cached content. apiToken.sale keeps those mechanics identical and layers a discount on top.",
    sections: [
      { h2: "Tokens, input and output", blocks: [
        { type: "p", text: "Every request is metered by tokens in (your prompt and context) and tokens out (the model's reply). Output tokens usually cost more than input, and larger models cost more per token." },
      ] },
      { h2: "Caching and thinking", blocks: [
        { type: "list", items: [
          "Cache writes and cache reads are metered separately, and cache reads are much cheaper.",
          "Thinking tokens count toward output on reasoning-heavy calls.",
          "Streaming and non-streaming requests are billed the same way.",
        ] },
      ] },
      { h2: "The apiToken.sale discount", blocks: [
        { type: "p", text: "Each call is converted to official Anthropic spend, then your discount is subtracted: B2C takes a flat 50% off every request. Every request is visible in your dashboard with token-level detail." },
        cta(),
      ] },
      { h2: "Claude API token pricing by model", blocks: [
        { type: "p", text: "Larger models cost more per token: Opus is the premium tier, Sonnet is the balanced default, and Haiku is the cheapest. Your discount applies to all of them, so the ranking stays the same but every price is lower." },
        { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (\u221250%)"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "link", text: "Per-model pages with cache rates and context windows", href: "/models" },
      ] },
    ],
    faq: [
      { q: "How is the Claude API priced?", a: "Per token, split into input and output, with separate cheaper rates for cache reads. Larger models cost more per token." },
      { q: "How does the discount apply?", a: "Official spend is calculated first, then your flat 50% B2C discount is subtracted before it touches your balance." },
      { q: "How are Claude API tokens priced?", a: "Per token, split into input and output, with cheaper cache reads. apiToken.sale applies your flat 50% discount on top of the official token rates." },
    ],
    related: ["cheapest-claude-api", "save-tokens-on-claude-api", "how-billing-works", "apitoken-vs-anthropic-direct"],
    updated: "2026-07-17",
  },
  {
    slug: "save-tokens-on-claude-api",
    cluster: "explain",
    title: "How to Save Tokens on the Claude API",
    h1: "How to save tokens on the Claude API",
    description: "Cut Claude API costs with prompt caching, the right model per task, and tighter context. Practical token-saving tactics that stack with the apiToken.sale discount.",
    keywords: ["save tokens claude api", "reduce claude api cost", "claude prompt caching", "claude api optimization", "lower claude api bill", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
    dek: "Your discount lowers the price per token; these tactics lower the number of tokens. Together they compound into a much smaller bill.",
    sections: [
      { h2: "Use prompt caching", blocks: [
        { type: "p", text: "Long, stable context — system prompts, large files, tool definitions — should be cached. Cache reads cost a fraction of fresh input tokens, so repeated context becomes cheap." },
      ] },
      { h2: "Pick the right model", blocks: [
        { type: "p", text: "Do not send every request to Opus. Route cheap or high-volume work to Haiku, keep everyday coding on Sonnet, and reserve Opus for genuinely hard reasoning." },
      ] },
      { h2: "Trim context", blocks: [
        { type: "list", items: [
          "Send only the files and history a task actually needs.",
          "Summarize long threads instead of resending them in full.",
          "Cap max_tokens to what the response really requires.",
        ] },
        cta(),
      ] },
    ],
    faq: [
      { q: "What is the single biggest token saver?", a: "Prompt caching for large, repeated context, combined with choosing the cheapest model that can do the job." },
      { q: "Do these tips stack with the discount?", a: "Yes. The discount lowers price per token; these tactics lower token count, so the savings multiply." },
    ],
    related: ["claude-api-pricing-explained", "cheapest-claude-api", "claude-haiku-api", "claude-opus-vs-sonnet"],
  },
  {
    slug: "how-billing-works",
    cluster: "explain",
    title: "How Billing Works on apiToken.sale",
    h1: "How billing works",
    description: "Understand one prepaid balance for Claude, GPT, Gemini and Kimi: exact provider-rate metering, a flat B2C discount and token-level usage in the dashboard.",
    keywords: ["multi provider api billing", "claude api billing", "gpt api billing", "gemini api billing", "kimi api billing", "prepaid api balance", "api usage tracking"],
    dek: "Billing is prepaid and transparent. Claude, GPT, Gemini and Kimi requests draw from one balance after exact provider-rate metering and your discount, with a breakdown you can audit.",
    sections: [
      { h2: "Prepaid balance", blocks: [
        { type: "p", text: "You top up any whole-dollar amount. Balance never expires and there is no customer subscription, so idle time costs nothing. The same balance covers supported Claude, GPT, Gemini and Kimi models." },
      ] },
      { h2: "Per-request metering", blocks: [
        { type: "list", items: [
          "Each call is converted to official provider spend by its exact usage legs: input, output, cache and any model-specific long-context or image buckets.",
          "Your flat 50% B2C discount is subtracted across every supported provider.",
          "The net amount is deducted from your prepaid balance.",
        ] },
      ] },
      { h2: "Full visibility", blocks: [
        { type: "p", text: "Every request appears in your dashboard with its model and provider and a token breakdown, so you always know where your balance goes." },
        cta(),
      ] },
    ],
    faq: [
      { q: "Is billing prepaid or postpaid?", a: "Prepaid. You fund a balance in advance and requests draw it down; there is no monthly invoice." },
      { q: "Does one balance cover Claude, GPT, Gemini and Kimi?", a: "Yes. Each provider is metered against its own official rate card, then the same B2C discount applies and the charge draws from one prepaid balance." },
      { q: "Can I see token-level usage?", a: "Yes. The dashboard breaks usage down by model, provider and token bucket." },
    ],
    related: ["claude-api-pricing-explained", "gpt-api-pricing", "gemini-api-pricing", "kimi-api-pricing"],
    updated: "2026-08-09",
  },
  {
    slug: "claude-api-activation-time",
    cluster: "explain",
    title: "How Fast Is Claude API Activation?",
    h1: "How fast your Claude API key activates",
    description: "apiToken.sale keys activate instantly. Generate a key, top up, and make a successful Claude API call within minutes — no manual review or waitlist.",
    keywords: ["claude api activation time", "how fast claude api key", "instant claude api key", "claude api ready time", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api", "claude api worldwide", "claude api china"],
    dek: "There is no waiting period between creating your key and using it. Activation is instant, so the only limit on speed is how fast you paste the key into your tool.",
    sections: [
      { h2: "Instant by design", blocks: [
        { type: "p", text: "Keys are live the moment you generate them. Top-ups credit your balance as soon as payment confirms, and card payments confirm in seconds." },
        cta(),
      ] },
      { h2: "What can add a short delay", blocks: [
        { type: "p", text: "The only wait is payment confirmation. Card top-ups clear in seconds; a crypto top-up credits once the network confirms the transaction, which depends on the coin and fee you choose." },
        { type: "list", items: [
          "Key generation: instant.",
          "Card top-up: seconds.",
          "Crypto top-up: after network confirmation.",
        ] },
      ] },
    ],
    faq: [
      { q: "How long until my key works?", a: "Immediately. There is no manual review — a freshly generated key works on the next request." },
      { q: "How long do top-ups take?", a: "Card payments credit in seconds; crypto credits after the network confirms the transaction." },
    ],
    related: ["claude-api-without-waitlist", "how-to-buy-claude-api-key", "how-billing-works", "claude-api-quick-setup"],
  },
  {
    slug: "claude-api-supported-countries",
    cluster: "explain",
    title: "Claude API Supported Countries",
    h1: "Where you can use apiToken.sale",
    description: "apiToken.sale works worldwide with no Anthropic billing-country requirement. Pay by card or crypto and use the Claude API from regions Anthropic does not serve directly.",
    keywords: ["claude api supported countries", "claude api worldwide", "anthropic api country restrictions", "claude api available regions", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
    dek: "Because we issue your key and balance, there is no Anthropic billing-country gate. That opens the Claude API to developers in regions where signing up directly is difficult.",
    sections: [
      { h2: "No billing-country gate", blocks: [
        { type: "list", items: [
          "No Anthropic account or supported billing country required.",
          "Card and cryptocurrency payment options.",
          "Support in English and Russian over Telegram.",
        ] },
        cta(),
      ] },
      { h2: "How payment works across regions", blocks: [
        { type: "p", text: "Because we issue the key and balance, you are not tied to an Anthropic-supported billing country. Pay with a bank card where available, or with cryptocurrency where cards are declined." },
        { type: "list", items: [
          "No Anthropic billing country required.",
          "Card or cryptocurrency at checkout.",
          "Support in English and Russian over Telegram.",
        ] },
      ] },
    ],
    faq: [
      { q: "Is the Claude API available in my country?", a: "apiToken.sale has no billing-country requirement, so you can buy balance and use a key from regions Anthropic does not bill directly." },
      { q: "What about payment restrictions?", a: "You can pay by card or with cryptocurrency, which helps where cards are unavailable." },
    ],
    related: ["claude-api-for-russia", "claude-api-crypto-payment", "how-to-buy-claude-api-key", "claude-api-without-waitlist"],
  },
  {
    slug: "claude-api-refund-policy",
    cluster: "explain",
    title: "Claude API Refund Policy",
    h1: "Refunds and support",
    description: "Learn how apiToken.sale handles balance, refunds and support. Prepaid balance never expires, and help is available in English and Russian through Telegram.",
    keywords: ["claude api refund", "apitoken refund policy", "claude api support", "claude api money back", "claude api help", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
    dek: "Prepaid balance is designed to be low-risk: it never expires, you spend only what you call, and support is one message away.",
    sections: [
      { h2: "Balance and refunds", blocks: [
        { type: "p", text: "Because balance is prepaid and never expires, unused funds stay available for future usage. Refund handling is processed through the original payment provider; reach out to support with your account details." },
      ] },
      { h2: "Getting help", blocks: [
        { type: "p", text: "Support is available in English and Russian via Telegram, and by email at apitokensale@gmail.com. Most integration questions are answered quickly." },
        cta(),
      ] },
      { h2: "How top-ups and balance work", blocks: [
        { type: "p", text: "You add balance in any whole-dollar amount, and it is drawn down only as requests run. Because it never expires, there is little reason to over-fund — top up as you go." },
        { type: "list", items: [
          "Prepaid, never-expiring balance.",
          "Refunds processed through the original payment provider.",
          "Contact support with your account email for help.",
        ] },
      ] },
    ],
    faq: [
      { q: "Does my balance expire?", a: "No. Prepaid balance never expires and is consumed only by real API usage." },
      { q: "How do I contact support?", a: "Reach support in English or Russian through Telegram, or by email at apitokensale@gmail.com." },
    ],
    related: ["how-billing-works", "claude-api-crypto-payment", "claude-api-supported-countries", "how-to-buy-claude-api-key"],
  },

  // ─────────────────────────── COMPARE (expansion) ───────────────────────────
  {
    slug: "apitoken-vs-proxyapi",
    cluster: "compare",
    title: "apiToken.sale vs ProxyAPI for Claude",
    h1: "apiToken.sale vs ProxyAPI",
    description: "Comparing Claude API resellers: apiToken.sale offers a native Anthropic endpoint with a flat 50% discount, card or crypto payment, and one key for every model.",
    keywords: ["proxyapi alternative", "apitoken vs proxyapi", "claude api reseller", "proxyapi claude", "claude api без proxyapi", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
    dek: "Both let you reach Claude without an Anthropic account. The differences are in how you pay, how much you save, and whether the endpoint is truly Anthropic-native.",
    sections: [
      { h2: "Native Anthropic endpoint", blocks: [
        { type: "p", text: `apiToken.sale exposes the standard Anthropic Messages API at ${BASE}, so Claude Code, Cursor and the Anthropic SDKs work unchanged — no adapter layer between you and Claude.` },
      ] },
      { h2: "Discount, not markup", blocks: [
        { type: "list", items: [
          "Flat 50% B2C discount off official Claude spend.",
          "One prepaid key and balance for Opus, Sonnet and Haiku.",
          "Card or cryptocurrency top-ups that never expire.",
        ] },
        cta(),
      ] },
      { h2: "When each fits", blocks: [
        { type: "list", items: [
          "apiToken.sale — a native Anthropic endpoint with a flat discount, lifetime key spending limits and optional expiration.",
          "A generic reseller — may suit you if you already use its other providers.",
          "Both remove the Anthropic-account barrier; the difference is price and how native the Claude access is.",
        ] },
      ] },
    ],
    faq: [
      { q: "Is apiToken.sale cheaper than a standard reseller?", a: "It applies a flat 50% discount to official Claude spend rather than adding a markup on top of list prices." },
      { q: "Do my Anthropic tools still work?", a: "Yes — it is the native Anthropic Messages API, so Claude Code, Cursor and the SDKs need only a base-URL change." },
    ],
    related: ["apitoken-vs-anthropic-direct", "apitoken-vs-openrouter", "cheapest-claude-api", "claude-api-for-russia"],
  },
  {
    slug: "apitoken-vs-portkey",
    cluster: "compare",
    title: "apiToken.sale vs Portkey for Claude",
    h1: "apiToken.sale vs Portkey",
    description: "Portkey is an AI gateway for routing and observability using your own provider keys. apiToken.sale provides the Claude key and balance itself, at a discount. Here is when to use each.",
    keywords: ["portkey alternative", "apitoken vs portkey", "ai gateway claude", "portkey claude api", "claude api gateway", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
    dek: "These tools solve different problems. Portkey sits in front of provider keys you already own; apiToken.sale is where the Claude key and discounted balance come from.",
    sections: [
      { h2: "Different jobs", blocks: [
        { type: "p", text: "Portkey adds routing, caching, and observability on top of API keys you bring. It does not sell you Claude access or a discount — you still need a funded Anthropic account behind it." },
        { type: "p", text: `apiToken.sale is the source of the key and balance: a native Anthropic endpoint at ${BASE} with a flat 50% off, no Anthropic account required.` },
      ] },
      { h2: "They can even combine", blocks: [
        { type: "p", text: "If you like Portkey's observability, you can point it at an apiToken.sale key as the Anthropic provider and get the discount underneath." },
        cta(),
      ] },
    ],
    faq: [
      { q: "Does Portkey give me a Claude discount?", a: "No — Portkey is a gateway over keys you already own. apiToken.sale is what provides the discounted Claude key and balance." },
      { q: "Can I use both together?", a: "Yes. Use an apiToken.sale key as Portkey's Anthropic provider to keep observability while paying less." },
    ],
    related: ["apitoken-vs-openrouter", "claude-api-gateway", "cheapest-claude-api", "anthropic-sdk-base-url"],
  },
  {
    slug: "apitoken-vs-litellm",
    cluster: "compare",
    title: "apiToken.sale vs LiteLLM for Claude",
    h1: "apiToken.sale vs LiteLLM",
    description: "LiteLLM is a self-hosted proxy that unifies model APIs but needs your own funded keys. apiToken.sale is a hosted, discounted Claude endpoint with nothing to run.",
    keywords: ["litellm alternative", "apitoken vs litellm", "litellm claude", "self-hosted claude proxy", "claude api hosted", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
    dek: "LiteLLM is great if you want to self-host a proxy across many providers. apiToken.sale is the opposite trade-off: nothing to run, and the Claude balance comes discounted.",
    sections: [
      { h2: "Self-hosted vs hosted", blocks: [
        { type: "list", items: [
          "LiteLLM: you run and maintain the proxy, and you still fund each provider yourself.",
          "apiToken.sale: fully hosted native Anthropic endpoint, no infrastructure to manage.",
          "apiToken.sale adds a flat 50% discount on Claude spend that a bare proxy cannot.",
        ] },
        cta(),
      ] },
      { h2: "When to choose each", blocks: [
        { type: "list", items: [
          "apiToken.sale — you want a hosted, discounted Claude endpoint with nothing to run.",
          "LiteLLM — you want to self-host a unified proxy across many providers you fund yourself.",
          "You can even put LiteLLM in front of an apiToken.sale key to keep the discount underneath.",
        ] },
      ] },
    ],
    faq: [
      { q: "Does LiteLLM discount Claude?", a: "No. LiteLLM routes to providers you fund yourself; the discount comes from apiToken.sale's pooled prepaid balance." },
      { q: "Do I need to host anything with apiToken.sale?", a: "No — it is a hosted endpoint. You only change your base URL and key." },
    ],
    related: ["apitoken-vs-portkey", "apitoken-vs-openrouter", "claude-api-gateway", "anthropic-sdk-base-url"],
  },
  {
    slug: "best-claude-model-for-coding",
    cluster: "compare",
    title: "Best Claude Model for Coding",
    h1: "The best Claude model for coding",
    description: "Which Claude model should you use for coding? A practical guide to picking Opus, Sonnet or Haiku per task — all available on one apiToken.sale key.",
    keywords: ["best claude model for coding", "claude model for programming", "opus vs sonnet coding", "claude coding model", "which claude for code", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api"],
    dek: "The best model depends on the task. Match the model to the job and you get better output for fewer tokens — and every tier is on one key.",
    sections: [
      { h2: "Sonnet for everyday coding", blocks: [
        { type: "p", text: "Claude Sonnet 5 and Sonnet 4.6 are the default for interactive coding and agent loops: fast, capable, and cost-effective. Start here for most work." },
      ] },
      { h2: "Opus for hard problems", blocks: [
        { type: "p", text: "Use Claude Opus 4.8 for complex refactors, architecture and long, high-stakes sessions where extra reasoning pays off." },
      ] },
      { h2: "Haiku for volume", blocks: [
        { type: "p", text: "Claude Haiku 4.5 handles fast, cheap, high-volume tasks — linting, extraction, quick edits — to stretch your balance." },
        cta(),
      ] },
    ],
    faq: [
      { q: "What is the best Claude model for coding?", a: "Sonnet for everyday coding, Opus for complex reasoning and refactors, Haiku for fast high-volume tasks — all on one apiToken.sale key." },
      { q: "Can I switch models per request?", a: "Yes. One key and balance cover every model, so you can route each request to the best-value tier." },
    ],
    related: ["claude-opus-vs-sonnet", "claude-sonnet-api", "claude-opus-api", "save-tokens-on-claude-api"],
  },
  {
    slug: "claude-max-plan-vs-api",
    cluster: "compare",
    title: "Claude Max Plan vs the Claude API",
    h1: "Claude Max subscription vs the API",
    description: "When to use a Claude subscription vs the Claude API. apiToken.sale gives pay-as-you-go API access to every model with no monthly fee and a flat 50% off.",
    keywords: ["claude max plan", "claude subscription vs api", "claude max vs api", "claude api pay as you go", "claude without subscription", "anthropic api alternative", "claude api discount", "cheap claude api", "claude api vs anthropic", "best claude api", "claude api tokens"],
    dek: "A flat Claude subscription and pay-as-you-go API billing suit different usage. For programmatic and bursty use, the API on prepaid balance is usually the better deal.",
    sections: [
      { h2: "Subscription vs per-token", blocks: [
        { type: "p", text: "A fixed monthly plan makes sense for steady, heavy interactive use in one app. But it is wasteful for spiky usage, and it does not give you a programmable API key for your own tools." },
      ] },
      { h2: "Why the API often wins", blocks: [
        { type: "list", items: [
          "Pay only for the tokens you actually use — no monthly floor.",
          "One key drives Claude Code, Cursor, agents and production calls.",
          "apiToken.sale takes a flat 50% off official token rates.",
        ] },
        cta(),
      ] },
    ],
    faq: [
      { q: "Is the API cheaper than a Claude subscription?", a: "For bursty or programmatic usage, pay-as-you-go API billing avoids paying a flat monthly fee for idle time, and apiToken.sale discounts it further." },
      { q: "Can I use the API in coding tools?", a: "Yes — the API key works in Claude Code, Cursor, VS Code agents and the SDKs, which a subscription does not provide." },
    ],
    related: ["claude-code-without-subscription", "claude-api-pricing-explained", "cheapest-claude-api", "how-billing-works"],
  },
  {
    slug: "claude-3-5-vs-claude-4",
    cluster: "compare",
    title: "Claude 3.5 vs Claude 4 — What Changed",
    h1: "Claude 3.5 vs Claude 4: what changed",
    description: "Moving from Claude 3.5 to the current Claude 4 line? What improved, updated model IDs, and how to switch on apiToken.sale with one base-URL change.",
    keywords: ["claude 3.5 vs 4", "claude 4 vs 3.5", "claude model migration", "upgrade claude model", "new claude models", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
    dek: "The current Claude line is a clear step up from 3.5 in reasoning and coding. Migrating is mostly a model-ID change — everything else stays the same.",
    sections: [
      { h2: "What improved", blocks: [
        { type: "p", text: "The Opus, Sonnet and Haiku 4-series models improve on 3.5 for agentic coding, long-context consistency and complex reasoning, while keeping the same Messages API." },
      ] },
      { h2: "How to migrate", blocks: [
        { type: "p", text: "Swap the model ID to a current one — for example claude-opus-4-8, claude-sonnet-5 or claude-haiku-4-5 — and keep your existing request code. On apiToken.sale it is the same key and endpoint." },
        cta(),
      ] },
    ],
    faq: [
      { q: "Is Claude 4 much better than 3.5?", a: "Yes, especially for coding, agents and long-context tasks, while using the same API format." },
      { q: "Is migrating hard?", a: "No — update the model ID (e.g. to claude-sonnet-5) and your existing Messages API code keeps working." },
    ],
    related: ["best-claude-model-for-coding", "claude-opus-vs-sonnet", "claude-sonnet-api", "claude-api-quick-setup"],
  },
  {
    slug: "why-choose-apitoken",
    cluster: "compare",
    title: "Why Choose apiToken.sale",
    h1: "Why choose apiToken.sale",
    description: "Why developers use one apiToken.sale key for Claude, GPT, Gemini and Kimi: native or compatible APIs, 50% off B2C pricing, and card or crypto payment.",
    keywords: ["why apitoken.sale", "multi provider api", "claude api discount", "gpt api discount", "gemini api discount", "kimi api key", "openai compatible api"],
    dek: "apiToken.sale puts four provider families behind one key and prepaid balance while preserving the protocol each client expects. Here is what that means in practice.",
    sections: [
      { h2: "The short version", blocks: [
        { type: "list", items: [
          "Anthropic Messages for Claude and Kimi, OpenAI-compatible routes for GPT and cross-provider clients (including Kimi), and native Gemini generateContent routes.",
          "A flat 50% off official spend on prepaid balance that never expires — one B2C rate covers supported models across all providers.",
          "Instant, self-serve access without separate Anthropic, OpenAI, Google Cloud or Kimi billing accounts.",
          "Pay by bank card or cryptocurrency.",
          "An optional lifetime spending limit and expiration date per key, plus token-level usage in the dashboard.",
        ] },
        cta(),
      ] },
      { h2: "Discounted API tokens on one balance", blocks: [
        { type: "p", text: "Prepay one balance, get a flat 50% off official B2C spend, and use it across supported Claude, GPT, Gemini and Kimi models. The balance never expires and there is no customer subscription." },
      ] },
    ],
    faq: [
      { q: "What makes apiToken.sale different?", a: "One key and balance cover four provider families at a flat 50% B2C discount, while each client keeps the appropriate native or compatible protocol." },
      { q: "Is every provider forced through one translated API?", a: "No. Claude and Kimi keep Anthropic Messages, GPT uses OpenAI-compatible routes, and Gemini keeps its native Google-shaped API. Kimi is additionally reachable through the universal OpenAI-compatible lane for clients that require it." },
      { q: "What is apiToken.sale?", a: "An independent multi-provider API gateway for discounted prepaid access to supported Claude, GPT, Gemini and Kimi models without separate provider billing accounts." },
    ],
    related: ["how-to-buy-claude-api-key", "how-to-buy-gpt-api-key", "how-to-buy-gemini-api-key", "how-to-buy-kimi-api-key"],
    updated: "2026-08-09",
  },

  // ─────────────────────────── EXPLAIN (expansion) ───────────────────────────
  {
    slug: "claude-api-gateway",
    cluster: "explain",
    title: "What Is a Claude API Gateway?",
    h1: "What a Claude API gateway is",
    description: "A Claude API gateway sits between your tools and Anthropic, adding access, billing and control. apiToken.sale is a native gateway with a flat 50% discount.",
    keywords: ["claude api gateway", "what is an api gateway", "anthropic gateway", "claude proxy", "claude api access layer", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
    dek: "A gateway is a thin layer between your code and the model provider. A good Claude gateway is invisible to your tools while improving access, price and control.",
    sections: [
      { h2: "What a gateway does", blocks: [
        { type: "list", items: [
          "Presents the standard Anthropic Messages API so tools work unchanged.",
          "Handles access and billing — here, prepaid balance at a discount.",
          "Adds per-key lifetime spending limits, optional expiration and usage visibility.",
        ] },
      ] },
      { h2: "Native, not a translation layer", blocks: [
        { type: "p", text: `apiToken.sale is Anthropic-native: point any client at ${BASE}/v1/messages and it behaves exactly like api.anthropic.com — plus your discount and dashboard controls.` },
        cta(),
      ] },
      { h2: "What to look for in a gateway", blocks: [
        { type: "list", items: [
          "Native Anthropic API, so tools and SDKs work unchanged.",
          "Transparent per-token billing you can audit in a dashboard.",
          "Per-key controls: an optional lifetime spending limit and expiration date.",
          "No lock-in — prepaid balance that never expires.",
        ] },
      ] },
    ],
    faq: [
      { q: "Does a gateway change the API?", a: "No. A native Claude gateway speaks the standard Anthropic Messages API, so your tools and SDKs are unchanged." },
      { q: "Why use a gateway instead of Anthropic directly?", a: "For a discount, instant access without an Anthropic account, and optional lifetime spending limits and expiration dates for individual keys." },
    ],
    related: ["apitoken-vs-anthropic-direct", "claude-api-key-security", "cheapest-claude-api", "anthropic-sdk-base-url"],
  },
  {
    slug: "claude-api-rate-limits",
    cluster: "explain",
    title: "Claude API Rate Limits",
    h1: "Understanding Claude API rate limits",
    description: "What a 429 means on apiToken.sale, how to handle it with Retry-After and backoff, and how key spending guardrails differ from throughput limits.",
    keywords: ["claude api rate limits", "claude api 429", "anthropic rate limit", "claude api throughput", "claude api retry", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
    dek: "Rate limits keep the gateway stable and your balance safe. Handling them well means smoother tools and no wasted spend.",
    sections: [
      { h2: "Traffic limits and spending guardrails", blocks: [
        { type: "p", text: "apiToken.sale does not publish a fixed RPM table. A 429 can represent a gateway or upstream capacity limit. The dashboard does not configure request throughput: its per-key guardrails are an optional lifetime spending limit and expiration date." },
      ] },
      { h2: "Handling a 429", blocks: [
        { type: "list", items: [
          "Respect the Retry-After header and back off exponentially.",
          "Reduce concurrency rather than hammering the endpoint.",
          "Contact support if you need sustained higher throughput.",
        ] },
        cta(),
      ] },
    ],
    faq: [
      { q: "What are the Claude API rate limits?", a: "apiToken.sale does not publish a fixed RPM number. If you receive a 429, honor Retry-After, back off and reduce concurrency; contact support when you need sustained higher throughput." },
      { q: "What should I do on a 429?", a: "Respect Retry-After, back off, and reduce concurrency; contact support for sustained higher limits." },
    ],
    related: ["claude-api-best-practices", "claude-api-streaming", "how-billing-works", "claude-api-key-security"],
  },
  {
    slug: "claude-api-streaming",
    cluster: "explain",
    title: "Streaming with the Claude API",
    h1: "Streaming responses from the Claude API",
    description: "How to stream Claude responses on apiToken.sale for responsive coding agents and UIs. Same Anthropic SSE format, billed the same as non-streaming.",
    keywords: ["claude api streaming", "claude sse", "stream claude responses", "anthropic streaming api", "claude api real-time", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
    dek: "Streaming sends tokens as they are generated, which makes agents and chat UIs feel instant. apiToken.sale supports the standard Anthropic streaming format.",
    sections: [
      { h2: "How to stream", blocks: [
        { type: "p", text: "Set \"stream\": true in your request (or use the SDK's streaming helper). The gateway returns standard Anthropic server-sent events." },
        { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "stream": true,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
      ] },
      { h2: "Billing is identical", blocks: [
        { type: "p", text: "Streaming and non-streaming requests are billed the same way — by input and output tokens — so you lose nothing by streaming." },
        cta(),
      ] },
      { h2: "When streaming is worth it", blocks: [
        { type: "list", items: [
          "Chat and coding UIs where users watch the answer appear.",
          "Long generations, so you can render or act on partial output early.",
          "Agents that stop as soon as a tool call is emitted.",
        ] },
        { type: "p", text: "For short batch jobs, non-streaming is simpler; the cost is identical either way." },
      ] },
    ],
    faq: [
      { q: "Does apiToken.sale support streaming?", a: "Yes — the standard Anthropic SSE streaming format works for coding agents, IDEs and production calls." },
      { q: "Does streaming cost more?", a: "No. Streaming and non-streaming requests are billed identically by tokens." },
    ],
    related: ["claude-api-quick-setup", "claude-api-rate-limits", "anthropic-sdk-base-url", "claude-api-for-ai-agents"],
  },
  {
    slug: "claude-api-prompt-caching",
    cluster: "explain",
    title: "Prompt Caching on the Claude API",
    h1: "Cutting costs with Claude prompt caching",
    description: "Prompt caching makes repeated context on the Claude API much cheaper. How it works on apiToken.sale, when to use it, and how it stacks with your discount.",
    keywords: ["claude prompt caching", "claude api cache", "anthropic prompt cache", "reduce claude cost caching", "claude cache read", "claude api key", "anthropic-compatible api", "claude api base url", "claude api setup", "claude api integration", "claude code api"],
    dek: "If you send the same large context repeatedly — system prompts, files, tool definitions — caching turns those tokens from expensive to nearly free.",
    sections: [
      { h2: "How caching saves money", blocks: [
        { type: "p", text: "Cache writes and cache reads are metered separately, and cache reads cost a fraction of fresh input tokens. Stable, reused context is the ideal candidate." },
      ] },
      { h2: "It stacks with your discount", blocks: [
        { type: "p", text: "Caching lowers the token count; your apiToken.sale discount lowers the price per token. Together they compound into a much smaller bill, and every cache line is visible in your usage breakdown." },
        cta(),
      ] },
    ],
    faq: [
      { q: "How much does prompt caching save?", a: "Cache reads cost a fraction of fresh input tokens, so repeated large context becomes far cheaper." },
      { q: "Does caching work with the discount?", a: "Yes — caching reduces token count and the discount reduces price per token, so the savings multiply." },
    ],
    related: ["save-tokens-on-claude-api", "claude-api-pricing-explained", "cheapest-claude-api", "how-billing-works"],
  },
  {
    slug: "claude-api-best-practices",
    cluster: "explain",
    title: "Claude API Best Practices",
    h1: "Claude API best practices",
    description: "Practical best practices for the Claude API on apiToken.sale: model choice, prompt caching, streaming, lifetime key spending limits, expiration, and secure key handling.",
    keywords: ["claude api best practices", "claude api tips", "claude api production", "claude api guidelines", "anthropic api best practices", "claude api key", "anthropic-compatible api", "claude api base url", "claude api setup", "claude api integration"],
    dek: "A short checklist to get reliable, economical results from the Claude API in production.",
    sections: [
      { h2: "The checklist", blocks: [
        { type: "list", items: [
          "Pick the cheapest model that can do each task; escalate only when needed.",
          "Cache large, stable context to slash input cost.",
          "Stream responses for responsive agents and UIs.",
          "Set an optional lifetime spending limit and expiration date on each key.",
          "Handle 429s with Retry-After and backoff.",
          "Watch the token-level usage breakdown to catch waste early.",
        ] },
        cta(),
      ] },
      { h2: "Keep costs and reliability in check", blocks: [
        { type: "list", items: [
          "Cap max_tokens to what each response actually needs.",
          "Retry 429/5xx with exponential backoff, not tight loops.",
          "Use separate, clearly named keys per environment so a leak can be revoked without replacing every client.",
          "Review token-level usage weekly to catch regressions early.",
        ] },
      ] },
    ],
    faq: [
      { q: "What is the most impactful best practice?", a: "Match the model to the task and cache repeated context — together they cut cost the most." },
      { q: "How do I keep keys safe?", a: "Store keys in a secret manager, set an appropriate lifetime spending limit and expiration date, and revoke a key immediately if it is exposed." },
    ],
    related: ["save-tokens-on-claude-api", "claude-api-rate-limits", "claude-api-key-security", "best-claude-model-for-coding"],
  },

  // ─────────────────────────── INTEGRATE (expansion) ───────────────────────────
  {
    slug: "claude-code-api-key",
    cluster: "integrate",
    title: "Set Up Claude Code with an API Key",
    h1: "Use Claude Code with an apiToken.sale key",
    description: "Configure Claude Code with an apiToken.sale key in two environment variables and run every Claude model on prepaid balance at a flat 50% off.",
    keywords: ["claude code api key", "claude code setup", "claude code anthropic base url", "claude code custom key", "run claude code cheap", "claude api key", "anthropic-compatible api", "claude api base url", "claude api setup", "claude api integration"],
    dek: "Claude Code reads two environment variables. Point them at apiToken.sale and you keep every feature while billing against discounted prepaid balance.",
    sections: [
      { h2: "Two variables", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# then just run\nclaude` },
        { type: "p", text: "That is the whole setup. Use claude-opus-4-8 for hard work and claude-sonnet-5 for everyday coding." },
        cta(),
      ] },
      { h2: "Verify and choose a model", blocks: [
        { type: "p", text: "Run a small prompt first to confirm the key works, then set your default model. If Claude Code reports an auth error, re-check both environment variables and restart your shell so they are exported." },
        { type: "list", items: [
          "Everyday coding: claude-sonnet-5.",
          "Hard refactors and long sessions: claude-opus-4-8.",
          "See per-request token usage in the dashboard to track spend.",
        ] },
      ] },
    ],
    faq: [
      { q: "How do I point Claude Code at apiToken.sale?", a: "Set ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY to your apiToken.sale endpoint and key, then run claude." },
      { q: "Do I keep all Claude Code features?", a: "Yes — only billing changes, from subscription to discounted prepaid usage." },
    ],
    related: ["claude-code-without-subscription", "claude-api-key-for-cursor", "anthropic-sdk-base-url", "best-claude-model-for-coding"],
  },
  {
    slug: "openai-api-quickstart",
    cluster: "integrate",
    title: "OpenAI-Compatible API Quickstart — GPT-5.6 on One Key",
    h1: "OpenAI-compatible API quickstart: Responses and Chat Completions",
    description: "Run GPT-5.6 models on apiToken.sale through the OpenAI-compatible API — Responses and Chat Completions with SSE streaming, one sk-pool key and balance shared with Claude, at a flat 50% off.",
    keywords: ["openai compatible api", "gpt-5.6 api", "responses api", "chat completions custom base url", "openai sdk base_url", "gpt api key", "gpt-5.6 price", "gpt-5.6-sol", "openai api alternative"],
    dek: "Your sk-pool key is not Claude-only. The same key and prepaid balance also serve the GPT-5 line through an OpenAI-compatible endpoint — standard Responses and Chat Completions calls, official OpenAI SDKs, SSE streaming, and the same flat 50% discount.",
    sections: [
      { h2: "Three steps to your first GPT call", blocks: [
        { type: "steps", items: [
          "Create a free account and generate one API key (it looks like sk-pool-…) — the same key already covers Claude models too.",
          `Point your client at ${OPENAI_BASE} and authenticate with Authorization: Bearer — not x-api-key; that header belongs to the Anthropic surface.`,
          "Confirm the enabled models with GET https://router.apitoken.sale/v1/models — the unified catalog namespaces IDs by provider (anthropic/*, openai/*, google/*) — then send a Responses request.",
        ] },
        { type: "code", code: `curl ${OPENAI_BASE}/responses \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{\n    "model": "gpt-5.6-sol",\n    "input": "Reply with exactly: connected"\n  }'` },
        cta(),
      ] },
      { h2: "Use the official OpenAI SDK", blocks: [
        { type: "p", text: "The official SDKs work unchanged — only base_url and the key change. Keep the key in a server-side environment variable in production." },
        { type: "code", code: `import os\nfrom openai import OpenAI\n\nclient = OpenAI(\n    api_key=os.environ["APITOKEN_API_KEY"],\n    base_url="${OPENAI_BASE}",\n)\n\nresponse = client.responses.create(\n    model="gpt-5.6-sol",\n    input="Reply with exactly: connected",\n)\nprint(response.output_text)` },
        { type: "p", text: "Chat Completions is served on the same host if your client expects it — the model ID and key stay the same." },
        { type: "code", code: `completion = client.chat.completions.create(\n    model="gpt-5.6-sol",\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(completion.choices[0].message.content)` },
      ] },
      { h2: "Which GPT models are available", blocks: [
        { type: "p", text: "The served set is pinned and priced in the engine; GET https://router.apitoken.sale/v1/models is always the live answer. Today the line covers three GPT-5.6 tiers and two previous-generation models:" },
        { type: "table", headers: ["Model ID", "Tier", "Official in / out ($ per 1M)", "Cached input"], rows: [
          ["gpt-5.6-sol (alias: gpt-5.6)", "Flagship", "$5 / $30", "$0.50"],
          ["gpt-5.6-terra", "Balanced", "$2 / $12", "$0.20"],
          ["gpt-5.6-luna", "Fast", "$0.20 / $1.20", "$0.02"],
          ["gpt-5.5", "Previous flagship", "$5 / $30", "$0.50"],
          ["gpt-5.4", "Previous balanced", "$2.50 / $15", "$0.25"],
        ] },
        { type: "list", items: [
          "Reasoning effort is adjustable per request — none through xhigh on every model, plus max on the GPT-5.6 line.",
          "Every model accepts text and image input and streams over SSE on both Responses and Chat Completions.",
          "Requests above 272K input tokens bill at OpenAI long-context rates: 2× input and 1.5× output on the whole request.",
          "Your B2C discount applies here exactly as it does to Claude usage — one balance, one rate, 50% off official spend.",
        ] },
        { type: "link", text: "Full per-model specs and discounted prices", href: "/models" },
      ] },
      { h2: "What the endpoint does and does not cover", blocks: [
        { type: "p", text: "This is an independent OpenAI-compatible service, not the OpenAI Platform. It serves model discovery, streaming Responses and Chat Completions, plus dedicated GPT Image 2 generation and edit routes. Audio, file, realtime, assistants, batch and fine-tuning endpoints are not available." },
        { type: "note", text: "Errors come in the OpenAI envelope — {\"error\":{\"message\",\"type\",\"param\",\"code\"}}. A 401 means the key or the auth header is wrong (use Bearer, not x-api-key), a 402 means the shared prepaid balance needs a top-up, and a 404 means the model ID is not enabled — check GET https://router.apitoken.sale/v1/models." },
      ] },
    ],
    faq: [
      { q: "Does the same key work beyond GPT?", a: "Yes. One sk-pool key and prepaid balance also cover supported Claude, Gemini and Kimi models; use the protocol and authentication header documented for each provider." },
      { q: "Which auth header does the OpenAI-compatible endpoint use?", a: "Authorization: Bearer sk-pool-…. The x-api-key header is only for the Anthropic surface — sending it to the OpenAI endpoint returns a 401." },
      { q: "Responses or Chat Completions?", a: "Both are served with SSE streaming. Use Responses for new code and the official SDKs; Chat Completions works for clients and frameworks that expect the classic shape." },
      { q: "How is GPT usage billed?", a: "Per token at official OpenAI rates — including cached-input and long-context pricing — then your flat 50% B2C discount is subtracted before the charge touches your prepaid balance, exactly like Claude usage." },
    ],
    related: ["how-to-buy-gpt-api-key", "gpt-api-pricing", "gpt-5-6-sol-vs-terra-vs-luna", "codex-cli-setup"],
    published: "2026-07-29",
    updated: "2026-07-29",
  },
  {
    slug: "codex-cli-setup",
    cluster: "integrate",
    title: "Set Up Codex CLI with apiToken.sale — GPT-5.6 Profile",
    h1: "Run Codex CLI on apiToken.sale",
    description: "Configure Codex CLI with a named model_providers profile pointing at the apiToken.sale OpenAI-compatible endpoint — GPT-5.6 models on prepaid balance at a flat 50% off, no ChatGPT account needed.",
    keywords: ["codex cli setup", "codex config.toml", "codex custom model provider", "codex api key", "codex cli gpt-5.6", "codex responses api", "codex cli without chatgpt", "openai codex cli"],
    dek: "Codex CLI runs entirely on API-key authentication when you give it a custom model provider. One TOML profile points it at apiToken.sale, and your prepaid balance covers every session — no ChatGPT login, at a flat 50% below official spend.",
    sections: [
      { h2: "Create the profile", blocks: [
        { type: "p", text: "Save this as ~/.codex/apitoken.config.toml. A named profile leaves your default Codex configuration and any ChatGPT login untouched — you opt in per run." },
        { type: "code", code: `# ~/.codex/apitoken.config.toml\nmodel = "gpt-5.6-sol"\nmodel_provider = "apitoken"\n\n[model_providers.apitoken]\nname = "apiToken.sale"\nbase_url = "${OPENAI_BASE}"\nwire_api = "responses"\nenv_key = "APITOKEN_API_KEY"` },
        { type: "p", text: "env_key names the environment variable Codex reads the key from — the secret stays in your shell, never in the TOML file." },
        cta(),
      ] },
      { h2: "Run and verify", blocks: [
        { type: "code", code: `export APITOKEN_API_KEY=${KEY}\ncodex --profile apitoken` },
        { type: "list", items: [
          "Always pass --profile apitoken explicitly so there is no ambiguity about which provider — and which env var — is active.",
          "Switch models per project by editing the model line: gpt-5.6-sol for the hardest work, gpt-5.6-terra for the daily driver, gpt-5.6-luna for fast cheap steps.",
          "GET " + OPENAI_BASE + "/models with the same Bearer key lists the currently enabled set — the unified catalog namespaces IDs by provider (anthropic/*, openai/*, google/*).",
        ] },
        { type: "note", text: "wire_api = \"responses\" is the right value for this gateway — it serves both Responses and Chat Completions, and Codex streams over Responses. Set it only to \"chat\" if a specific client requires the classic shape." },
      ] },
      { h2: "Errors you might hit", blocks: [
        { type: "list", items: [
          "Missing APITOKEN_API_KEY — the variable named by env_key is not exported in the shell that runs codex. Export it in that same shell, or in your shell profile.",
          "stream error: unexpected status 401 — the key is wrong, revoked, or the base_url lost its /v1 suffix. Reproduce with curl outside Codex to isolate which half is broken.",
          "stream error: unexpected status 404 — the model ID is not enabled; check GET https://router.apitoken.sale/v1/models instead of assuming.",
          "402 — the shared prepaid balance needs a top-up; backoff will not fix it.",
        ] },
        { type: "link", text: "The full Codex error playbook — config.toml, auth.json, stream errors", href: "/errors/codex" },
      ] },
    ],
    faq: [
      { q: "Do I need a ChatGPT account or subscription?", a: "No. With a custom model_providers profile and the provider's API key in the environment, Codex runs entirely on API-key authentication — the ChatGPT login in auth.json is irrelevant." },
      { q: "Does this change my default Codex setup?", a: "No. The profile lives in its own file and activates only when you pass --profile apitoken. Your default configuration and login stay as they were." },
      { q: "Is the discount the same as for Claude?", a: "Yes. GPT-5.6 usage is metered at official OpenAI token rates and your flat 50% B2C discount applies to the same prepaid balance." },
      { q: "Responses or Chat Completions for wire_api?", a: "Use wire_api = \"responses\" — the gateway serves both, and Codex is built around the Responses stream. The Chat Completions shape exists for clients that require it." },
    ],
    related: ["openai-api-quickstart", "gpt-5-6-sol-vs-terra-vs-luna", "gpt-api-pricing", "how-billing-works"],
    published: "2026-07-29",
    updated: "2026-07-29",
  },
  {
    slug: "vscode-ai-agents-one-prompt",
    cluster: "integrate",
    title: "Free VS Code AI Agents with Claude",
    h1: "Run free VS Code AI agents on Claude",
    description: "Set up free VS Code agents like Cline and Roo Code with an apiToken.sale Claude key — no Cursor Pro needed. One endpoint, every Claude model, at a discount.",
    keywords: ["free vscode ai agent", "cline roo code claude", "vscode claude agent", "cursor alternative free", "claude vscode without cursor", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api", "claude api agent"],
    dek: "You do not need Cursor Pro to get agentic coding. Free VS Code agents accept any Anthropic-compatible key, so Claude runs in VS Code on discounted balance.",
    sections: [
      { h2: "Point the agent at Claude", blocks: [
        { type: "steps", items: [
          "Install a free agent extension such as Cline or Roo Code.",
          "Choose Anthropic as the API provider.",
          `Set the base URL to ${BASE}, paste your ${KEY} key, and pick a model like claude-sonnet-5.`,
        ] },
        cta(),
      ] },
      { h2: "Pick the right model per task", blocks: [
        { type: "list", items: [
          "claude-sonnet-5 — the default for everyday coding and agent loops.",
          "claude-opus-4-8 — complex refactors, architecture and long sessions.",
          "claude-haiku-4-5 — fast, cheap edits and high-volume steps.",
        ] },
        { type: "p", text: "Because one key covers every model, you can switch per task in the extension without changing accounts or billing." },
      ] },
    ],
    faq: [
      { q: "Do I need Cursor Pro for AI coding?", a: "No. Free VS Code agents like Cline and Roo Code work with an apiToken.sale Claude key." },
      { q: "Which model should I pick?", a: "claude-sonnet-5 for everyday coding; claude-opus-4-8 for complex tasks." },
    ],
    related: ["claude-api-for-vs-code", "claude-api-key-for-cursor", "claude-code-api-key", "cursor-without-anthropic-account"],
  },
  {
    slug: "claude-api-key-security",
    cluster: "integrate",
    title: "Securing Your Claude API Key",
    h1: "Keep your Claude API key secure",
    description: "How to protect a Claude API key on apiToken.sale with a lifetime spending limit, optional expiration, separate named keys, prompt revocation, and safe secret storage.",
    keywords: ["claude api key security", "protect api key", "rotate claude api key", "claude api key management", "secure anthropic key"],
    dek: "Your key spends real balance, so treat it like a credential. apiToken.sale gives you controls to limit blast radius if a key ever leaks.",
    sections: [
      { h2: "Controls that limit risk", blocks: [
        { type: "list", items: [
          "Set a lifetime spending limit for the key.",
          "Choose an expiration date when temporary access should end automatically.",
          "Issue a separate, clearly named key per tool or environment.",
          "To replace a key, create the replacement, update the client, then revoke the old key.",
        ] },
      ] },
      { h2: "Basic hygiene", blocks: [
        { type: "list", items: [
          "Never commit keys to git or paste them into chats.",
          "Store keys in environment variables or a secret manager.",
          "Revoke and rotate immediately if a key is exposed.",
        ] },
        cta(),
      ] },
    ],
    faq: [
      { q: "How do I limit damage if a key leaks?", a: "Use a lifetime spending limit and expiration date, keep separate named keys per client, and revoke the exposed key immediately." },
      { q: "Where should I store my key?", a: "In environment variables or a secret manager — never committed to git or shared in chats." },
    ],
    related: ["claude-api-best-practices", "claude-api-rate-limits", "claude-code-api-key", "how-billing-works"],
  },
  {
    slug: "claude-api-for-ai-agents",
    cluster: "explain",
    title: "Claude API for AI Agents",
    h1: "Using the Claude API for AI agents",
    description: "Build AI agents on the Claude API with apiToken.sale: one key for every model, streaming, tool use, prompt caching and a lifetime key spending limit for long-run cost control.",
    keywords: ["claude api agents", "claude ai agent api", "claude tool use", "claude agent framework", "claude api for automation"],
    dek: "Agentic workloads are token-hungry and long-running, which makes model choice, caching and cost control matter most. Here is how apiToken.sale fits agents.",
    sections: [
      { h2: "What agents need", blocks: [
        { type: "list", items: [
          "Streaming and tool use — both standard on the Anthropic Messages API.",
          "Model routing: Haiku for cheap steps, Sonnet for reasoning, Opus for the hardest.",
          "Prompt caching for repeated system prompts and tool definitions.",
          "A per-key lifetime spending limit so a runaway loop cannot spend beyond that key's cap.",
        ] },
        cta(),
      ] },
      { h2: "A cost-aware agent loop", blocks: [
        { type: "p", text: "A practical pattern: route planning and reasoning to Sonnet, cheap sub-steps and parsing to Haiku, and escalate only the hardest calls to Opus. Cache the system prompt and tool definitions so repeated context is nearly free." },
        { type: "list", items: [
          "Set a per-key lifetime spending limit so a runaway loop cannot spend beyond the cap.",
          "Stream so the agent can act on partial output.",
          "Watch token usage to tune which steps use which model.",
        ] },
      ] },
    ],
    faq: [
      { q: "Is the Claude API good for agents?", a: "Yes — with streaming, tool use, model routing and prompt caching, all on one apiToken.sale key with spend controls." },
      { q: "How do I keep agent costs down?", a: "Route cheap steps to Haiku, cache repeated context, and set a lifetime spending limit on the agent's key." },
    ],
    related: ["claude-api-streaming", "claude-api-prompt-caching", "save-tokens-on-claude-api", "claude-api-key-security"],
  },
];

export const learnArticles: LearnArticle[] = [
  ...coreLearnArticles,
  ...learnProviderEn,
  ...learnImageSeoEn,
];

// Put the broadest provider entry points first inside each hub cluster. The
// remaining guides keep their source order, so new long-tail content does not
// unexpectedly reshuffle existing cards.
const LEARN_HUB_FEATURED_SLUGS: Partial<Record<LearnCluster, readonly string[]>> = {
  buy: [
    "how-to-buy-claude-api-key",
    "how-to-buy-gpt-api-key",
    "how-to-buy-gemini-api-key",
    "how-to-buy-kimi-api-key",
  ],
  integrate: [
    "claude-api-quick-setup",
    "openai-api-quickstart",
    "gemini-api-quickstart",
    "kimi-api-quickstart",
    "claude-code-api-key",
    "codex-cli-setup",
    "kimi-api-for-opencode",
    "kimi-api-for-claude-code",
    "kimi-api-for-kimi-code",
    "gpt-image-2-api-guide",
    "nano-banana-2-api-guide",
    "image-editing-api-guide",
    "batch-image-generation-api",
    "image-generation-api-for-ecommerce",
  ],
  compare: [
    "claude-opus-vs-sonnet",
    "gpt-5-6-sol-vs-terra-vs-luna",
    "gemini-pro-vs-flash-vs-flash-lite",
    "kimi-k3-vs-kimi-for-coding",
    "nano-banana-2-vs-gpt-image-2",
  ],
  explain: [
    "claude-api-pricing-explained",
    "gpt-api-pricing",
    "gemini-api-pricing",
    "kimi-api-pricing",
    "image-generation-api-pricing",
    "nano-banana-2-api-cost",
    "gpt-image-2-api-cost",
  ],
};
const LEARN_HUB_CLUSTER_ORDER: readonly LearnCluster[] = ["buy", "free", "integrate", "compare", "explain"];

/** Feature provider entry points on the hub while preserving long-tail order. */
export function orderLearnHubArticles<T extends Pick<LearnArticle, "slug" | "cluster">>(articles: readonly T[]): T[] {
  const rankByCluster = new Map<LearnCluster, Map<string, number>>();
  const clusterRank = new Map(LEARN_HUB_CLUSTER_ORDER.map((cluster, index) => [cluster, index]));
  for (const [cluster, slugs] of Object.entries(LEARN_HUB_FEATURED_SLUGS) as [LearnCluster, readonly string[]][]) {
    rankByCluster.set(cluster, new Map(slugs.map((slug, index) => [slug, index])));
  }

  return articles
    .map((article, index) => ({ article, index }))
    .sort((left, right) => {
      if (left.article.cluster !== right.article.cluster) {
        return clusterRank.get(left.article.cluster)! - clusterRank.get(right.article.cluster)!;
      }
      const ranks = rankByCluster.get(left.article.cluster);
      const leftRank = ranks?.get(left.article.slug) ?? Number.MAX_SAFE_INTEGER;
      const rightRank = ranks?.get(right.article.slug) ?? Number.MAX_SAFE_INTEGER;
      return leftRank - rightRank || left.index - right.index;
    })
    .map(({ article }) => article);
}

export const learnArticlesBySlug: Record<string, LearnArticle> = Object.fromEntries(
  learnArticles.map((article) => [article.slug, article]),
);

const translations: Record<Exclude<Locale, "en">, Record<string, LocalizedContent>> = {
  ru: { ...learnRu, ...learnProviderRu, ...learnImageSeoRu },
  zh: { ...learnZh, ...learnProviderZh, ...learnImageSeoZh },
  ko: { ...learnKo, ...learnProviderKo, ...learnImageSeoKo },
};

function enContent(article: LearnArticle): LocalizedContent {
  return {
    title: article.title,
    h1: article.h1,
    description: article.description,
    keywords: article.keywords,
    dek: article.dek,
    sections: article.sections,
    faq: article.faq,
  };
}

// The welcome-credit boilerplate is repeated on ~40 pages per locale.
// Strip it from the body so it is not duplicate content; the article view
// renders a single rotating CTA instead.
function stripBoilerplateCta(content: LocalizedContent): LocalizedContent {
  return {
    ...content,
    sections: content.sections.map((section) => ({
      ...section,
      blocks: section.blocks.filter((block) => !(block.type === "note" && block.text.includes("$5"))),
    })),
  };
}

export type ResolvedArticle = {
  slug: string;
  cluster: LearnCluster;
  related: string[];
  locale: Locale;
  content: LocalizedContent;
};

/** Resolve an article for a locale, or null if that translation is not published. */
export function resolveArticle(slug: string, locale: Locale): ResolvedArticle | null {
  const base = learnArticlesBySlug[slug];
  if (!base) return null;
  if (locale === "en") {
    return { slug, cluster: base.cluster, related: base.related, locale, content: stripBoilerplateCta(enContent(base)) };
  }
  const content = translations[locale][slug];
  if (!content) return null;
  return { slug, cluster: base.cluster, related: base.related, locale, content: stripBoilerplateCta(content) };
}

/** The day the learn cluster first shipped — the default published/modified date. */
export const LEARN_LAUNCH_DATE = new Date("2026-07-16T00:00:00.000Z");

/** Last substantive content change for an article (all locales share the date). */
export function articleUpdatedDate(slug: string): Date {
  const updated = learnArticlesBySlug[slug]?.updated;
  return updated ? new Date(`${updated}T00:00:00.000Z`) : articlePublishedDate(slug);
}

/** First publication date for an article. */
export function articlePublishedDate(slug: string): Date {
  const published = learnArticlesBySlug[slug]?.published;
  return published ? new Date(`${published}T00:00:00.000Z`) : LEARN_LAUNCH_DATE;
}

/** Locales that have a published version of this article (en is always present). */
export function articleLocales(slug: string): Locale[] {
  return LOCALES.filter((locale) => locale === "en" || Boolean(translations[locale as Exclude<Locale, "en">]?.[slug]));
}

/** Article slugs available in a locale (en = all; ru/zh = those translated). */
export function articlesForLocale(locale: Locale): string[] {
  const all = learnArticles.map((article) => article.slug);
  if (locale === "en") return all;
  return all.filter((slug) => Boolean(translations[locale as Exclude<Locale, "en">][slug]));
}

export function localePrefix(locale: Locale): string {
  return locale === "en" ? "" : `/${locale}`;
}

export function learnPath(slug: string, locale: Locale = "en"): string {
  return `${localePrefix(locale)}/docs/learn/${slug}`;
}

export function learnHubPath(locale: Locale = "en"): string {
  return `${localePrefix(locale)}/docs/learn`;
}

export const LEARN_HUB_PATH = "/docs/learn";

/** Path to the clean-markdown version of a learn article (AI-agent gateway). */
export function learnMarkdownPath(slug: string, locale: Locale = "en"): string {
  return `/md${localePrefix(locale)}/docs/learn/${slug}`;
}

export function ogLocale(locale: Locale): string {
  return locale === "ru" ? "ru_RU" : locale === "zh" ? "zh_CN" : locale === "ko" ? "ko_KR" : "en_US";
}

export function htmlLang(locale: Locale): string {
  return locale === "zh" ? "zh-CN" : locale;
}

/** hreflang alternates map for a learn article across every published locale. */
export function learnAlternates(slug: string, absolute: (path: string) => string): Record<string, string> {
  const languages: Record<string, string> = {};
  for (const locale of articleLocales(slug)) {
    languages[htmlLang(locale)] = absolute(learnPath(slug, locale));
  }
  languages["x-default"] = absolute(learnPath(slug, "en"));
  return languages;
}

function blockToMarkdown(block: LearnBlock): string {
  switch (block.type) {
    case "p":
      return block.text;
    case "note":
      return `> ${block.text}`;
    case "list":
      return block.items.map((item) => `- ${item}`).join("\n");
    case "steps":
      return block.items.map((item, index) => `${index + 1}. ${item}`).join("\n");
    case "code":
      return "```\n" + block.code + "\n```";
    case "table": {
      const header = `| ${block.headers.join(" | ")} |`;
      const divider = `| ${block.headers.map(() => "---").join(" | ")} |`;
      const rows = block.rows.map((row) => `| ${row.join(" | ")} |`);
      return [header, divider, ...rows].join("\n");
    }
    case "link":
      return `[${block.text}](${block.href})`;
    default:
      return "";
  }
}

/** Serialize a resolved article to clean Markdown for the AI-agent gateway. */
export function renderLearnMarkdown(article: ResolvedArticle, origin: string): string {
  const { content, slug, locale } = article;
  const ui = learnUi[locale];
  const lines: string[] = [
    "---",
    `title: ${content.title}`,
    `description: ${JSON.stringify(content.description)}`,
    `url: ${origin}${learnPath(slug, locale)}`,
    `language: ${htmlLang(locale)}`,
    "---",
    "",
    `# ${content.h1}`,
    "",
    content.dek,
    "",
  ];
  for (const section of content.sections) {
    lines.push(`## ${section.h2}`, "");
    for (const block of section.blocks) {
      lines.push(blockToMarkdown(block), "");
    }
  }
  if (content.faq.length > 0) {
    lines.push(`## ${ui.faqHeading}`, "");
    for (const item of content.faq) {
      lines.push(`### ${item.q}`, "", item.a, "");
    }
  }
  lines.push("---", `Get a key: ${origin}/register`, `More guides: ${origin}${learnHubPath(locale)}`, "");
  return lines.join("\n");
}
