"use client";

import Image from "next/image";
import Link from "next/link";
import { usePathname } from "next/navigation";
import type { ReactNode } from "react";
import { landingHref, localeHref, supportsRussianRoute } from "@/lib/locale-routes";
import { BackendPreconnect } from "./backend-preconnect";
import { useI18n } from "./i18n-provider";
import { ThemeToggle } from "./site-chrome";

export function AuthShell({ children }: { children: ReactNode }) {
  const { language, setLanguage } = useI18n();
  const pathname = usePathname();
  const russianSupported = supportsRussianRoute(pathname);
  const russianUnavailable = language === "ru" ? "Русская версия недоступна" : "Russian version unavailable";
  return (
    <>
      <BackendPreconnect />
      <header className="nav">
        <a className="skip-link" href="#main-content">{language === "ru" ? "К содержимому" : "Skip to content"}</a>
        <div className="wrap nav-in">
          <Link className="brand" href={landingHref(language)}>
            <Image className="brand-mark bm-light" src="/assets/logo-mark-light.png" width={24} height={24} alt="" />
            <Image className="brand-mark bm-dark" src="/assets/logo-mark-dark.png" width={24} height={24} alt="" />
            apiToken.sale
          </Link>
          <div className="nav-right">
            <div className="lang" role="group" aria-label={language === "ru" ? "Язык" : "Language"}>
              <button type="button" aria-pressed={language === "en"} className={language === "en" ? "active" : ""} onClick={() => setLanguage("en")}>EN</button>
              <button type="button" aria-pressed={language === "ru"} className={language === "ru" ? "active" : ""} disabled={!russianSupported} aria-disabled={!russianSupported} title={russianSupported ? undefined : russianUnavailable} onClick={() => setLanguage("ru")}>RU</button>
            </div>
            <ThemeToggle />
          </div>
        </div>
      </header>
      <main className="auth-shell" id="main-content" tabIndex={-1}><div className="auth-card ym-hide-content">{children}</div></main>
    </>
  );
}

export function AuthIntro({ title, subtitle }: { title: string; subtitle: string }) {
  const { language, t } = useI18n();
  return (
    <>
      <Link className="auth-back" href={landingHref(language)}>{t("back_home")}</Link>
      <h1>{title}</h1>
      <p className="sub">{subtitle}</p>
    </>
  );
}

export function LocalizedAuthLink({ href, children, className }: { href: string; children: ReactNode; className?: string }) {
  const { language } = useI18n();
  return <Link className={className} href={localeHref(href, language)}>{children}</Link>;
}

export function WelcomeBonusNotice() {
  const { t } = useI18n();
  return (
    <aside className="auth-bonus" aria-label={t("auth_bonus_title")}>
      <span className="auth-bonus-mark" aria-hidden="true">✦</span>
      <span>
        <strong>{t("auth_bonus_title")}</strong>
        <small>{t("auth_bonus_note")}</small>
      </span>
    </aside>
  );
}

export function Feedback({ message, success = false }: { message: string | null; success?: boolean }) {
  if (!message) return null;
  return <div className={`auth-msg ${success ? "ok" : "err"}`} role={success ? "status" : "alert"}>{message}</div>;
}
