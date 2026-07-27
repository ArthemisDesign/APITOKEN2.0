import type { Metadata } from "next";
import Link from "next/link";
import "./globals.css";

export const metadata: Metadata = {
  title: "OpenKeys — Claude API без регистрации",
  description:
    "Готовые ключи к Claude API с номиналом в долларах официального прайса Anthropic. Без регистрации и привязки карты.",
  robots: { index: false, follow: false },
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="ru">
      <body>
        <div className="wrap">
          <header className="topbar">
            <Link className="brand" href="/">
              OpenKeys
            </Link>
            <nav className="nav">
              <Link href="/docs">Подключение</Link>
              <Link href="/usage">Мой расход</Link>
            </nav>
          </header>
          {children}
        </div>
      </body>
    </html>
  );
}
