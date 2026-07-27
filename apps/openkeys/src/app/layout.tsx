import type { Metadata } from "next";
import "./globals.css";
import "./anim.css";

export const metadata: Metadata = {
  title: "apiToken — Claude API без регистрации",
  description:
    "Готовые ключи к Claude API с номиналом в долларах официального прайса Anthropic. Без регистрации и привязки карты.",
  robots: { index: false, follow: false },
  icons: { icon: "/assets/favicon-32.png", apple: "/assets/favicon-192.png" },
};

// Тему выставляем до первой отрисовки, иначе тёмная страница мигает белым.
const themeScript = `(()=>{try{const t=localStorage.getItem('theme')||'dark';if(t==='dark')document.documentElement.dataset.theme='dark'}catch{}})()`;

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="ru">
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
      </head>
      <body>{children}</body>
    </html>
  );
}
