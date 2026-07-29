import type { Metadata } from "next";
import { LanguageProvider } from "@/components/chrome";
import "./globals.css";
import "./anim.css";

export const metadata: Metadata = {
  title: "apiToken — one key for Claude and GPT",
  description:
    "A universal prepaid API key for Claude and GPT with one balance and a live usage dashboard.",
  robots: { index: false, follow: false },
  icons: { icon: "/assets/favicon-32.png", apple: "/assets/favicon-192.png" },
};

// Тему выставляем до первой отрисовки, иначе тёмная страница мигает белым.
const themeScript = `(()=>{try{const t=localStorage.getItem('theme')||'dark';if(t==='dark')document.documentElement.dataset.theme='dark'}catch{}})()`;

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
      </head>
      <body><LanguageProvider>{children}</LanguageProvider></body>
    </html>
  );
}
