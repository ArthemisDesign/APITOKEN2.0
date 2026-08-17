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
import { coreLearnArticles } from "./learn-core";

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
