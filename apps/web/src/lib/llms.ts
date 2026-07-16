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
import { SITE_ORIGIN } from "./seo";

const clusterOrder: LearnCluster[] = ["buy", "free", "integrate", "compare", "explain"];

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
    heading: "# apiToken.sale — Claude API access",
    summary: "> apiToken.sale is an independent Claude API gateway. It serves the standard Anthropic Messages API and the full supported Claude line (Opus, Sonnet, Haiku) from prepaid balance at a progressive discount of 60% up to 80% off official spend. No Anthropic account, waitlist, or billing-country requirement; pay by bank card or cryptocurrency.",
    factsHeading: "## Key facts",
    facts: [
      "Website: " + SITE_ORIGIN,
      "API base URL: https://api.apitoken.sale",
      "API format: Anthropic Messages API (POST /v1/messages), x-api-key + anthropic-version headers",
      "Models: Claude Opus 4.8, Opus 4.7, Sonnet 5, Sonnet 4.6, Haiku 4.5 (one key and balance)",
      "Billing: prepaid, per-token at official rates minus a 60–80% B2C discount; balance never expires",
      "Payment: bank card or cryptocurrency",
      "Free start: $10 of Claude usage at official API prices, no card required",
      "Onboarding: instant, self-serve, no Anthropic account",
      "Support: Telegram and apitokensale@gmail.com (English, Russian)",
    ],
    guidesHeading: "## Guides",
    allGuides: "All Claude API guides",
  },
  ru: {
    heading: "# apiToken.sale — доступ к Claude API",
    summary: "> apiToken.sale — независимый шлюз к Claude API. Он предоставляет стандартный Anthropic Messages API и всю линейку Claude (Opus, Sonnet, Haiku) с предоплаченного баланса со скидкой 60–80% от официальной стоимости. Без аккаунта Anthropic, без очереди и ограничений по стране; оплата картой или криптовалютой.",
    factsHeading: "## Ключевые факты",
    facts: [
      "Сайт: " + SITE_ORIGIN,
      "Базовый URL API: https://api.apitoken.sale",
      "Формат API: Anthropic Messages API (POST /v1/messages), заголовки x-api-key + anthropic-version",
      "Модели: Claude Opus 4.8, Opus 4.7, Sonnet 5, Sonnet 4.6, Haiku 4.5 (один ключ и баланс)",
      "Оплата за токены по официальным ставкам минус скидка 60–80% для B2C; баланс не сгорает",
      "Способы оплаты: банковская карта или криптовалюта",
      "Бесплатный старт: $10 использования Claude по официальным ценам, без карты",
      "Подключение: мгновенно, без аккаунта Anthropic",
      "Поддержка: Telegram и apitokensale@gmail.com (русский, английский)",
    ],
    guidesHeading: "## Гайды",
    allGuides: "Все гайды по Claude API",
  },
  zh: {
    heading: "# apiToken.sale — Claude API 接入",
    summary: "> apiToken.sale 是独立的 Claude API 网关。它提供标准的 Anthropic Messages API 以及完整的 Claude 系列（Opus、Sonnet、Haiku），以预付余额按官方价格的 60%–80% 折扣计费。无需 Anthropic 账户、无需排队、不限国家；支持银行卡或加密货币付款。",
    factsHeading: "## 关键信息",
    facts: [
      "网站: " + SITE_ORIGIN,
      "API 基址: https://api.apitoken.sale",
      "API 格式: Anthropic Messages API (POST /v1/messages), x-api-key + anthropic-version 请求头",
      "模型: Claude Opus 4.8、Opus 4.7、Sonnet 5、Sonnet 4.6、Haiku 4.5（一个密钥和余额）",
      "计费: 预付、按 token 以官方价格计费再减去 60–80% 的 B2C 折扣；余额永不过期",
      "支付: 银行卡或加密货币",
      "免费起步: $10 的 Claude 官方价格额度，无需绑卡",
      "开通: 即时、自助、无需 Anthropic 账户",
      "支持: Telegram 与 apitokensale@gmail.com（中文、英文、俄文）",
    ],
    guidesHeading: "## 指南",
    allGuides: "全部 Claude API 指南",
  },
};

export function buildLlms(locale: Locale): string {
  const c = copy[locale];
  const lines: string[] = [
    c.heading,
    "",
    c.summary,
    "",
    "## For AI agents",
    "",
    `Every guide is available as clean Markdown — append the slug to the markdown gateway. Example: ${SITE_ORIGIN}${learnMarkdownPath("cheapest-claude-api", locale)}`,
    "",
    c.factsHeading,
    "",
    ...c.facts.map((fact) => "- " + fact),
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
