import type { Metadata } from "next";

export const SITE_NAME = "apiToken.sale";
export const SITE_ORIGIN = "https://apitoken.sale";
export const DEFAULT_OG_IMAGE = "/og.png";
export const LAST_CONTENT_UPDATE = new Date("2026-07-16T00:00:00.000Z");

export type SeoPage = {
  path: string;
  title: string;
  description: string;
  priority: number;
  changeFrequency: "weekly" | "monthly" | "yearly";
};

export const seoPages = {
  home: {
    path: "/",
    title: "Claude API Access for Developers",
    description: "Use one API key and prepaid balance for supported Claude models. Connect Claude Code, Cursor, Cline, Zed, or Anthropic SDKs through a compatible endpoint.",
    priority: 1,
    changeFrequency: "weekly",
  },
  models: {
    path: "/models",
    title: "Claude Models, Context Windows & API Pricing",
    description: "Compare supported Claude Opus, Sonnet, and Haiku models, context windows, model IDs, official reference rates, and recommended use cases.",
    priority: 0.9,
    changeFrequency: "weekly",
  },
  integrations: {
    path: "/integrations",
    title: "Claude API Integrations for Coding Tools",
    description: "Follow setup guides for Claude Code, Cursor, Cline, Continue, Zed, Python, and TypeScript using one Anthropic-compatible API endpoint.",
    priority: 0.9,
    changeFrequency: "monthly",
  },
  plans: {
    path: "/plans",
    title: "Claude API Pricing, Discounts & Prepaid Plans",
    description: "Review flexible USD top-ups, progressive B2C discounts, 30-day tier requirements, estimated Claude API value, and negotiated B2B pricing.",
    priority: 0.9,
    changeFrequency: "weekly",
  },
  docs: {
    path: "/docs",
    title: "Claude API Documentation & Quickstart",
    description: "Connect to the Anthropic-compatible Messages API with curl, Python, TypeScript, Claude Code, and coding editors. Includes authentication, errors, and pricing.",
    priority: 0.9,
    changeFrequency: "weekly",
  },
  support: {
    path: "/support",
    title: "Claude API Customer Support",
    description: "Contact apiToken.sale support for account access, API keys, usage, payments, refunds, and technical integration questions by Telegram or email.",
    priority: 0.6,
    changeFrequency: "monthly",
  },
  privacy: {
    path: "/privacy",
    title: "Privacy Policy",
    description: "Read how apiToken.sale collects, uses, shares, retains, and protects information across the website, dashboard, payments, support, and API gateway.",
    priority: 0.4,
    changeFrequency: "yearly",
  },
  terms: {
    path: "/terms",
    title: "User Agreement",
    description: "Review the apiToken.sale user agreement covering accounts, API access, prepaid balances, pricing, payments, refunds, acceptable use, and support.",
    priority: 0.4,
    changeFrequency: "yearly",
  },
} as const satisfies Record<string, SeoPage>;

export const integrationGuideSeo = {
  "claude-code": {
    path: "/int-claude-code",
    title: "Connect Claude Code to apiToken.sale",
    description: "Configure Claude Code with apiToken.sale using ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY, then use supported Claude models through one prepaid balance.",
  },
  cursor: {
    path: "/int-cursor",
    title: "Connect Cursor to the Claude API",
    description: "Configure Cursor's Anthropic provider with the apiToken.sale base URL, API key, and a supported Claude model in three steps.",
  },
  cline: {
    path: "/int-cline",
    title: "Connect Cline to the Claude API",
    description: "Set up Cline in VS Code with the apiToken.sale Anthropic-compatible endpoint, one API key, and your preferred supported Claude model.",
  },
  continue: {
    path: "/int-continue",
    title: "Connect Continue to the Claude API",
    description: "Add apiToken.sale as an Anthropic model provider in Continue and run supported Claude models from your IDE with one balance.",
  },
  zed: {
    path: "/int-zed",
    title: "Connect Zed to the Claude API",
    description: "Point Zed's Anthropic language-model settings to apiToken.sale and use supported Claude models with one API key and prepaid balance.",
  },
  sdk: {
    path: "/int-sdk",
    title: "Use Anthropic SDKs with apiToken.sale",
    description: "Use the official Anthropic Python and TypeScript SDKs with apiToken.sale by changing the base URL and providing your API key.",
  },
} as const;

export type IntegrationGuideSlug = keyof typeof integrationGuideSeo;

export const sitemapPages: SeoPage[] = [
  ...Object.values(seoPages),
  ...Object.values(integrationGuideSeo).map((page) => ({
    ...page,
    priority: 0.8,
    changeFrequency: "monthly" as const,
  })),
];

export function absoluteUrl(path: string): string {
  return new URL(path, SITE_ORIGIN).toString();
}

export function createPageMetadata(page: Pick<SeoPage, "path" | "title" | "description">, options?: { absoluteTitle?: string }): Metadata {
  const canonical = absoluteUrl(page.path);
  const socialTitle = options?.absoluteTitle ?? `${page.title} — ${SITE_NAME}`;

  return {
    title: options?.absoluteTitle ? { absolute: options.absoluteTitle } : page.title,
    description: page.description,
    alternates: { canonical },
    openGraph: {
      type: "website",
      locale: "en_US",
      url: canonical,
      siteName: SITE_NAME,
      title: socialTitle,
      description: page.description,
      images: [{
        url: DEFAULT_OG_IMAGE,
        width: 1200,
        height: 630,
        alt: `${SITE_NAME} — Claude API access for developers`,
      }],
    },
    twitter: {
      card: "summary_large_image",
      title: socialTitle,
      description: page.description,
      images: [DEFAULT_OG_IMAGE],
    },
  };
}

export function createNoIndexMetadata(title: string, description: string): Metadata {
  return {
    title,
    description,
    robots: {
      index: false,
      follow: false,
      nocache: true,
      googleBot: { index: false, follow: false, noimageindex: true },
    },
  };
}

export function breadcrumbNode(items: Array<{ name: string; path: string }>) {
  return {
    "@type": "BreadcrumbList",
    itemListElement: items.map((item, index) => ({
      "@type": "ListItem",
      position: index + 1,
      name: item.name,
      item: absoluteUrl(item.path),
    })),
  };
}

export function breadcrumbJsonLd(items: Array<{ name: string; path: string }>) {
  return { "@context": "https://schema.org", ...breadcrumbNode(items) };
}
