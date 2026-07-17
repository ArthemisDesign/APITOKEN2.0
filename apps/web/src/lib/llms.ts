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
      "Free start: new Google/GitHub accounts get $10 of Claude usage at official API prices; email/password accounts are ineligible",
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
      "Бесплатный старт: новые аккаунты через Google/GitHub получают $10 использования Claude по официальным ценам; аккаунты по email/паролю не участвуют",
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
      "免费起步: 通过 Google/GitHub 创建的新账户可获 $10 的 Claude 官方价格额度；邮箱密码账户不参与",
      "开通: 即时、自助、无需 Anthropic 账户",
      "支持: Telegram 与 apitokensale@gmail.com（中文、英文、俄文）",
    ],
    guidesHeading: "## 指南",
    allGuides: "全部 Claude API 指南",
  },
  ko: {
    heading: "# apiToken.sale — Claude API 액세스",
    summary: "> apiToken.sale은 독립적인 Claude API 게이트웨이입니다. 표준 Anthropic Messages API와 전체 Claude 라인업(Opus, Sonnet, Haiku)을 선불 잔액으로 공식 요금 대비 60~80% 할인된 가격에 제공합니다. Anthropic 계정, 대기열, 국가 제한이 없으며 신용카드 또는 암호화폐로 결제할 수 있습니다.",
    factsHeading: "## 핵심 정보",
    facts: [
      "웹사이트: " + SITE_ORIGIN,
      "API 기본 URL: https://api.apitoken.sale",
      "API 형식: Anthropic Messages API (POST /v1/messages), x-api-key + anthropic-version 헤더",
      "모델: Claude Opus 4.8, Opus 4.7, Sonnet 5, Sonnet 4.6, Haiku 4.5 (하나의 키와 잔액)",
      "과금: 선불, 공식 요금 기준 토큰당 과금에서 B2C 60~80% 할인; 잔액은 만료되지 않음",
      "결제: 신용카드 또는 암호화폐",
      "무료 시작: Google/GitHub로 만든 신규 계정은 공식 API 가격 기준 $10 상당의 Claude 사용량을 받으며, 이메일/비밀번호 계정은 제외",
      "온보딩: 즉시, 셀프서비스, Anthropic 계정 불필요",
      "지원: Telegram 및 apitokensale@gmail.com (한국어 요청 시 영어/러시아어 응대)",
    ],
    guidesHeading: "## 가이드",
    allGuides: "모든 Claude API 가이드",
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
