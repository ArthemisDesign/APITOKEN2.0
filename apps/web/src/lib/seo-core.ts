import type { Metadata } from "next";
import { absoluteUrl, DEFAULT_OG_IMAGE, markdownAlternate, SITE_NAME } from "./seo";

// Russian copy for the public marketing pages that carry the EN/RU language
// switcher. English copy stays in seoPages (lib/seo.ts). Each core page is
// mirrored at /ru/<path> so the Russian version is a separately indexable URL.

export type CoreLocale = "en" | "ru";

type Localized = { title: string; description: string };

export const coreRu: Record<string, Localized> = {
  home: {
    title: "Купить доступ к Claude и GPT API со скидкой — один ключ",
    description: "Доступ к API Claude и GPT с фиксированной скидкой 50% — один ключ и предоплаченный баланс для Claude Opus, Sonnet, Haiku и линейки GPT-5. Готово для Claude Code, Codex и Cursor.",
  },
  models: {
    title: "Модели Claude и GPT, контекст и цены API",
    description: "Сравните поддерживаемые модели Claude Opus, Sonnet, Haiku и GPT-5, размеры контекста, точные ID моделей, официальные ставки и рекомендации по применению.",
  },
  integrations: {
    title: "Интеграции Claude и GPT API для инструментов кодинга",
    description: "Гайды по настройке Claude Code, Codex CLI, Cursor, Cline, opencode, Continue, Zed, Python и TypeScript через Anthropic-совместимый и OpenAI-совместимый эндпоинты.",
  },
  plans: {
    title: "Цены API, скидки и предоплата — Claude и GPT",
    description: "Гибкие пополнения в USD, фиксированная скидка B2C 50%, оценка официальной ценности API и договорные B2B-цены для моделей Claude и GPT.",
  },
  privacy: {
    title: "Политика конфиденциальности",
    description: "Как apiToken.sale собирает, использует, хранит и защищает данные на сайте, в панели, платежах, поддержке и API-шлюзе.",
  },
  support: {
    title: "Поддержка API",
    description: "Свяжитесь с поддержкой apiToken.sale по доступу к аккаунту, ключам, использованию, платежам, возвратам и интеграции — через Telegram или email.",
  },
  terms: {
    title: "Пользовательское соглашение",
    description: "Условия apiToken.sale: аккаунты, доступ к API, предоплаченный баланс, цены, платежи, возвраты, допустимое использование и поддержка.",
  },
};

export const coreIntRu: Record<string, Localized> = {
  "claude-code": {
    title: "Подключить Claude Code к apiToken.sale",
    description: "Настройте Claude Code с apiToken.sale через ANTHROPIC_BASE_URL и ANTHROPIC_API_KEY и используйте модели Claude на одном предоплаченном балансе.",
  },
  codex: {
    title: "Подключить Codex CLI к apiToken.sale",
    description: "Запустите Codex CLI на моделях GPT-5.6 через apiToken.sale: именованный профиль model_providers, OpenAI-совместимый base URL и ваш ключ sk-pool.",
  },
  cursor: {
    title: "Подключить Cursor к Claude API",
    description: "Укажите провайдеру Anthropic в Cursor base URL apiToken.sale, ключ и модель Claude — за три шага.",
  },
  cline: {
    title: "Подключить Cline к Claude API",
    description: "Настройте Cline в VS Code с Anthropic-совместимым эндпоинтом apiToken.sale, одним ключом и выбранной моделью Claude.",
  },
  opencode: {
    title: "Подключить opencode к apiToken.sale",
    description: "Подключите opencode к OpenAI-совместимому эндпоинту apiToken.sale одним блоком провайдера и запускайте модели GPT-5.6 на предоплаченном балансе.",
  },
  continue: {
    title: "Подключить Continue к Claude API",
    description: "Добавьте apiToken.sale как провайдера Anthropic в Continue и запускайте модели Claude из IDE на одном балансе.",
  },
  zed: {
    title: "Подключить Zed к Claude API",
    description: "Направьте настройки Anthropic в Zed на apiToken.sale и используйте модели Claude с одним ключом и предоплаченным балансом.",
  },
  sdk: {
    title: "Официальные SDK Anthropic с apiToken.sale",
    description: "Используйте официальные SDK Anthropic для Python и TypeScript с apiToken.sale, поменяв base URL и указав ваш ключ.",
  },
};

/** canonical + en/ru hreflang for the English version of a core page. */
export function coreAlternates(enPath: string): Metadata["alternates"] {
  const ruPath = enPath === "/" ? "/ru" : `/ru${enPath}`;
  return {
    canonical: absoluteUrl(enPath),
    languages: { en: absoluteUrl(enPath), ru: absoluteUrl(ruPath), "x-default": absoluteUrl(enPath) },
    ...markdownAlternate(enPath),
  };
}

/** Build metadata for a core page in the given locale, with canonical + hreflang. */
export function coreMetadata(enPath: string, en: Localized, ru: Localized, locale: CoreLocale): Metadata {
  const ruPath = enPath === "/" ? "/ru" : `/ru${enPath}`;
  const cur = locale === "ru" ? ru : en;
  const curPath = locale === "ru" ? ruPath : enPath;
  const socialTitle = `${cur.title} | ${SITE_NAME}`;
  return {
    title: locale === "ru" ? { absolute: socialTitle } : cur.title,
    description: cur.description,
    alternates: {
      canonical: absoluteUrl(curPath),
      languages: {
        en: absoluteUrl(enPath),
        ru: absoluteUrl(ruPath),
        "x-default": absoluteUrl(enPath),
      },
      ...markdownAlternate(enPath),
    },
    openGraph: {
      type: "website",
      locale: locale === "ru" ? "ru_RU" : "en_US",
      url: absoluteUrl(curPath),
      siteName: SITE_NAME,
      title: socialTitle,
      description: cur.description,
      images: [{ url: DEFAULT_OG_IMAGE, width: 1200, height: 630, alt: `${SITE_NAME} — Claude & GPT API` }],
    },
    twitter: {
      card: "summary_large_image",
      title: socialTitle,
      description: cur.description,
      images: [DEFAULT_OG_IMAGE],
    },
  };
}
