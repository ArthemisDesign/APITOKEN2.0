import type { Metadata, Viewport } from "next";
import "./globals.css";
import { THEME_STORAGE_KEY } from "@/lib/theme";
import { Sidebar } from "@/components/sidebar";
import { ErrorCenter } from "@/components/error-center";
import { DialogHost } from "@/lib/dialog";
import { Toaster } from "@/lib/toast";
import { RealtimeBridge, ResourceFreshnessBridge } from "@/lib/realtime";
import { I18nProvider, languageScript } from "@/lib/i18n";

export const metadata: Metadata = {
  title: "apiToken.sale · admin",
  description: "apiToken.sale operations admin",
  robots: { index: false, follow: false },
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  colorScheme: "light dark",
  themeColor: "#0a0a0a",
};

// Inline-скрипт выставляет data-theme на <html> ДО первой отрисовки, чтобы не было
// вспышки не той темы. Логика должна совпадать с resolveInitialTheme() в lib/theme.ts;
// модуль здесь импортировать нельзя — скрипт исполняется как есть в <head>.
const themeScript = `(function(){try{var s=localStorage.getItem(${JSON.stringify(THEME_STORAGE_KEY)});document.documentElement.dataset.theme=s==="light"?"light":"dark"}catch(e){document.documentElement.dataset.theme="dark"}})()`;

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    // data-theme выставляет inline-скрипт до гидрации — атрибут на клиенте
    // намеренно отличается от серверной разметки.
    <html lang="ru" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
        <script dangerouslySetInnerHTML={{ __html: languageScript }} />
      </head>
      <body>
        <I18nProvider>
          <RealtimeBridge />
          <ResourceFreshnessBridge />
          <div className="shell">
            <Sidebar />
            <main id="main-content">{children}</main>
          </div>
          {/* Глобальные слои легаси: центр ошибок (#error-center), промис-диалоги, тосты. */}
          <ErrorCenter />
          <DialogHost />
          <Toaster />
        </I18nProvider>
      </body>
    </html>
  );
}
