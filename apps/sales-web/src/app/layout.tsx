import type { Metadata } from "next";
import "./globals.css";
import { I18nProvider } from "@/components/i18n";

export const metadata: Metadata = {
  title: {
    default: "APIToken Partners — earn from every dollar your referrals spend",
    template: "%s · APIToken Partners",
  },
  description:
    "Partner program for apitoken.sale: share your link and earn a percentage of what your referrals actually spend on Claude API usage — their real, after-discount usage paid with real money (not top-ups, not free credit).",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>
        <I18nProvider>{children}</I18nProvider>
      </body>
    </html>
  );
}
