import type { Metadata } from "next";
import "./globals.css";
import { THEME_STORAGE_KEY } from "@/lib/theme";
import { Sidebar } from "@/components/sidebar";
import { ErrorCenter } from "@/components/error-center";
import { DialogHost } from "@/lib/dialog";
import { Toaster } from "@/lib/toast";

export const metadata: Metadata = {
  title: "apiToken.sale · admin",
  description: "Операционная админ-панель apiToken.sale",
  robots: { index: false, follow: false },
};

// Inline-скрипт выставляет data-theme на <html> ДО первой отрисовки, чтобы не было
// вспышки не той темы. Логика должна совпадать с resolveInitialTheme() в lib/theme.ts;
// модуль здесь импортировать нельзя — скрипт исполняется как есть в <head>.
const themeScript = `(function(){try{var t=localStorage.getItem(${JSON.stringify(THEME_STORAGE_KEY)});if(t!=="dark"&&t!=="light")t=matchMedia("(prefers-color-scheme: dark)").matches?"dark":"light";document.documentElement.dataset.theme=t}catch(e){document.documentElement.dataset.theme="light"}})()`;

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    // data-theme выставляет inline-скрипт до гидрации — атрибут на клиенте
    // намеренно отличается от серверной разметки.
    <html lang="ru" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
      </head>
      <body>
        <div className="shell">
          <Sidebar />
          <main id="main-content">{children}</main>
        </div>
        {/* Глобальные слои легаси: центр ошибок (#error-center), промис-диалоги, тосты. */}
        <ErrorCenter />
        <DialogHost />
        <Toaster />
      </body>
    </html>
  );
}
