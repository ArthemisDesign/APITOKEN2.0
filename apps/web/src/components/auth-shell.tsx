"use client";

import Image from "next/image";
import Link from "next/link";
import type { ReactNode } from "react";
import { useI18n } from "./i18n-provider";
import { ThemeToggle } from "./site-chrome";

export function AuthShell({ children }: { children: ReactNode }) {
  const { language, setLanguage } = useI18n();
  return (
    <>
      <header className="nav">
        <div className="wrap nav-in">
          <Link className="brand" href="/">
            <Image className="brand-mark bm-light" src="/assets/logo-mark-light.png" width={24} height={24} alt="" />
            <Image className="brand-mark bm-dark" src="/assets/logo-mark-dark.png" width={24} height={24} alt="" />
            apiToken.sale
          </Link>
          <div className="nav-right">
            <div className="lang">
              <button className={language === "en" ? "on" : ""} onClick={() => setLanguage("en")}>EN</button>
              <button className={language === "ru" ? "on" : ""} onClick={() => setLanguage("ru")}>RU</button>
            </div>
            <ThemeToggle />
          </div>
        </div>
      </header>
      <div className="auth-shell"><div className="auth-card">{children}</div></div>
    </>
  );
}

export function AuthIntro({ title, subtitle }: { title: string; subtitle: string }) {
  return (
    <>
      <Link className="auth-back" href="/">← Back to home</Link>
      <h1>{title}</h1>
      <p className="sub">{subtitle}</p>
    </>
  );
}

export function Feedback({ message, success = false }: { message: string | null; success?: boolean }) {
  if (!message) return null;
  return <div className={`auth-msg ${success ? "ok" : "err"}`} role="status">{message}</div>;
}
