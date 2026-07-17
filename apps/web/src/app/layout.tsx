import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import "./globals.css";
import "./anim.css";
import { I18nProvider } from "@/components/i18n-provider";
import { PersistentRouteShell } from "@/components/persistent-route-shell";
import { SiteAnalytics } from "@/components/site-analytics";
import { DEFAULT_OG_IMAGE, SITE_ICONS, SITE_NAME, SITE_ORIGIN, seoPages } from "@/lib/seo";
import { YANDEX_METRIKA_ID, yandexMetrikaBootstrap } from "@/lib/yandex-metrika";

const webmasterVerification = {
  ...(process.env.GOOGLE_SITE_VERIFICATION ? { google: process.env.GOOGLE_SITE_VERIFICATION } : {}),
  ...(process.env.YANDEX_SITE_VERIFICATION ? { yandex: process.env.YANDEX_SITE_VERIFICATION } : {}),
  ...(process.env.BING_SITE_VERIFICATION ? { other: { "msvalidate.01": process.env.BING_SITE_VERIFICATION } } : {}),
};

export const metadata: Metadata = {
  metadataBase: new URL(SITE_ORIGIN),
  title: { default: `${seoPages.home.title} | ${SITE_NAME}`, template: `%s — ${SITE_NAME}` },
  description: seoPages.home.description,
  applicationName: SITE_NAME,
  authors: [{ name: SITE_NAME, url: SITE_ORIGIN }],
  creator: SITE_NAME,
  publisher: SITE_NAME,
  category: "technology",
  manifest: "/manifest.webmanifest",
  alternates: { types: { "application/rss+xml": "/feed.xml" } },
  formatDetection: { email: false, address: false, telephone: false },
  icons: SITE_ICONS,
  appleWebApp: { capable: true, title: SITE_NAME, statusBarStyle: "black-translucent" },
  robots: {
    index: true,
    follow: true,
    googleBot: {
      index: true,
      follow: true,
      "max-image-preview": "large",
      "max-snippet": -1,
      "max-video-preview": -1,
    },
  },
  verification: webmasterVerification,
  openGraph: {
    type: "website",
    locale: "en_US",
    siteName: SITE_NAME,
    url: "/",
    title: `${seoPages.home.title} | ${SITE_NAME}`,
    description: seoPages.home.description,
    images: [{ url: DEFAULT_OG_IMAGE, width: 1200, height: 630, alt: `${SITE_NAME} — Claude API access for developers` }],
  },
  twitter: {
    card: "summary_large_image",
    title: `${seoPages.home.title} | ${SITE_NAME}`,
    description: seoPages.home.description,
    images: [DEFAULT_OG_IMAGE],
  },
};

export const viewport: Viewport = { width: "device-width", initialScale: 1, themeColor: "#0a0a0a" };

const themeScript = `(()=>{try{const t=localStorage.getItem('theme')||'dark';document.documentElement.dataset.theme=t}catch{}})()`;

export default function RootLayout({ children }: Readonly<{ children: ReactNode }>) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
        <script id="yandex-metrika" dangerouslySetInnerHTML={{ __html: yandexMetrikaBootstrap }} />
        <link rel="alternate" type="text/plain" href="/llms.txt" title="LLM-readable site summary" />
        <link rel="alternate" type="text/plain" href="/llms-full.txt" title="LLM-readable full reference" />
      </head>
      <body>
        <I18nProvider>
          <PersistentRouteShell>{children}</PersistentRouteShell>
        </I18nProvider>
        <SiteAnalytics />
        <noscript>
          <div>
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src={`https://mc.yandex.ru/watch/${YANDEX_METRIKA_ID}`}
              style={{ position: "absolute", left: "-9999px" }}
              alt=""
            />
          </div>
        </noscript>
      </body>
    </html>
  );
}
