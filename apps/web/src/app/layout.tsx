import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import "./globals.css";
import "./anim.css";
import { I18nProvider } from "@/components/i18n-provider";
import { PersistentRouteShell } from "@/components/persistent-route-shell";
import { SiteAnalytics } from "@/components/site-analytics";
import { DEMO_FIXTURE_SCRIPT } from "@/lib/demo-fixture";

export const metadata: Metadata = {
  metadataBase: new URL("https://apitoken.sale"),
  title: { default: "apiToken.sale — Claude API", template: "%s — apiToken.sale" },
  description: "One API key and one balance for supported Claude models.",
  icons: { icon: "/assets/favicon-32.png", apple: "/assets/favicon-192.png" },
  openGraph: {
    type: "website", siteName: "apiToken.sale", url: "/", images: [{ url: "/og.png", width: 1200, height: 630 }],
  },
  twitter: { card: "summary_large_image", images: ["/og.png"] },
};

export const viewport: Viewport = { width: "device-width", initialScale: 1, themeColor: "#ffffff" };

const themeScript = `(()=>{try{const t=localStorage.getItem('theme')||'light';document.documentElement.dataset.theme=t}catch{}})()`;

export default function RootLayout({ children }: Readonly<{ children: ReactNode }>) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
        {/* self-gated: активен только на *.vercel.app (превью), на apitoken.sale — no-op */}
        <script dangerouslySetInnerHTML={{ __html: DEMO_FIXTURE_SCRIPT }} />
      </head>
      <body>
        <I18nProvider>
          <PersistentRouteShell>{children}</PersistentRouteShell>
        </I18nProvider>
        <SiteAnalytics />
      </body>
    </html>
  );
}
