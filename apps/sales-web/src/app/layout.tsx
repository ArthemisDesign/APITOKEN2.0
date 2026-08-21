import type { Metadata } from "next";
import "./globals.css";
import { I18nProvider, languageBootstrapScript } from "@/components/i18n";
import { themeBootstrapScript } from "@/lib/theme";

export const metadata: Metadata = {
  title: {
    default: "APIToken Partners — earn from every dollar your referrals spend",
    template: "%s · APIToken Partners",
  },
  description:
    "Partner program for apitoken.sale: share your link and earn a percentage of what your referrals actually spend on the API — across every provider in the catalog — their real, after-discount usage paid with real money (not top-ups, not free credit).",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: languageBootstrapScript }} />
        <script dangerouslySetInnerHTML={{ __html: themeBootstrapScript }} />
      </head>
      <body>
        <I18nProvider>{children}</I18nProvider>
      </body>
    </html>
  );
}
