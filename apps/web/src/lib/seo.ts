import type { Metadata } from "next";

export const SITE_NAME = "apiToken.sale";
export const SITE_ORIGIN = "https://apitoken.sale";
export const DEFAULT_OG_IMAGE = "/og.png";
export const LAST_CONTENT_UPDATE = new Date("2026-07-16T00:00:00.000Z");
export const SITE_ICONS = {
  // 48px multiples (48/96/192) are what Google Search requires for a rich-result
  // favicon; 32 stays for the small browser-tab slot.
  icon: [
    { url: "/assets/favicon-32.png", sizes: "32x32", type: "image/png" },
    { url: "/assets/favicon-48.png", sizes: "48x48", type: "image/png" },
    { url: "/assets/favicon-96.png", sizes: "96x96", type: "image/png" },
    { url: "/assets/favicon-192.png", sizes: "192x192", type: "image/png" },
  ],
  shortcut: [{ url: "/assets/favicon-48.png", type: "image/png" }],
  apple: [{ url: "/assets/favicon-192.png", sizes: "192x192", type: "image/png" }],
} satisfies NonNullable<Metadata["icons"]>;

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
    title: "Buy Claude & GPT API Access — Discounted, One Key",
    description: "Buy discounted Claude and GPT API access — one key and prepaid balance for Claude Opus, Sonnet, Haiku and the GPT-5 line. Official Anthropic and OpenAI-compatible APIs at a flat 50% off, ready for Claude Code, Codex and Cursor.",
    priority: 1,
    changeFrequency: "weekly",
  },
  models: {
    path: "/models",
    title: "Claude & GPT Models, Context Windows & API Pricing",
    description: "Compare supported Claude Opus, Sonnet, Haiku and GPT-5 models, context windows, exact model IDs, official reference rates, and recommended use cases.",
    priority: 0.9,
    changeFrequency: "weekly",
  },
  integrations: {
    path: "/integrations",
    title: "Claude & GPT API Integrations for Coding Tools",
    description: "Follow setup guides for Claude Code, Codex CLI, Cursor, Cline, opencode, Continue, Zed, Python, and TypeScript using Anthropic-compatible and OpenAI-compatible API endpoints.",
    priority: 0.9,
    changeFrequency: "monthly",
  },
  plans: {
    path: "/plans",
    title: "API Pricing, Discounts & Prepaid Plans — Claude & GPT",
    description: "Review flexible USD top-ups, the flat 50% discount for every account, estimated official API value, and negotiated B2B pricing across Claude and GPT models.",
    priority: 0.9,
    changeFrequency: "weekly",
  },
  docs: {
    path: "/docs",
    title: "API Documentation & Quickstart — Claude & GPT",
    description: "Connect to the Anthropic-compatible Messages API and the OpenAI-compatible API with curl, Python, TypeScript, Claude Code, Codex, and coding editors. Includes authentication, errors, and pricing.",
    priority: 0.9,
    changeFrequency: "weekly",
  },
  support: {
    path: "/support",
    title: "API Customer Support",
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
  codex: {
    path: "/int-codex",
    title: "Connect Codex CLI to apiToken.sale",
    description: "Run Codex CLI on GPT-5.6 models through apiToken.sale with a named model_providers profile, the OpenAI-compatible base URL, and your sk-pool key.",
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
  opencode: {
    path: "/int-opencode",
    title: "Connect opencode to apiToken.sale",
    description: "Wire opencode to the apiToken.sale OpenAI-compatible endpoint with one provider block and run GPT-5.6 models on your prepaid balance.",
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

// HTML path -> its clean Markdown twin under /md, so pages can self-advertise a machine-readable
// version to crawlers and AI agents via <link rel="alternate" type="text/markdown">.
export function markdownTwinPath(path: string): string | undefined {
  const exact: Record<string, string> = {
    "/": "/md",
    "/docs": "/md/docs",
    "/docs/errors": "/md/docs/errors",
    "/models": "/md/models",
    "/plans": "/md/plans",
    "/integrations": "/md/int",
    "/docs/learn": "/md",
  };
  if (exact[path]) return exact[path];
  if (path.startsWith("/int-")) return `/md/int/${path.slice("/int-".length)}`;
  if (path.startsWith("/models/")) return `/md/models/${path.slice("/models/".length)}`;
  if (path.startsWith("/docs/learn/")) return `/md${path}`;
  return undefined;
}

export function markdownAlternate(path: string): { types: Record<string, string> } | undefined {
  const twin = markdownTwinPath(path);
  return twin ? { types: { "text/markdown": absoluteUrl(twin) } } : undefined;
}

export function createPageMetadata(page: Pick<SeoPage, "path" | "title" | "description">, options?: { absoluteTitle?: string }): Metadata {
  const canonical = absoluteUrl(page.path);
  const socialTitle = options?.absoluteTitle ?? `${page.title} — ${SITE_NAME}`;

  return {
    title: options?.absoluteTitle ? { absolute: options.absoluteTitle } : page.title,
    description: page.description,
    alternates: { canonical, ...markdownAlternate(page.path) },
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
        alt: `${SITE_NAME} — Claude & GPT API access for developers`,
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
