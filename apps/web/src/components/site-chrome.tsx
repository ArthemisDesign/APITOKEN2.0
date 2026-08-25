"use client";

import Image from "next/image";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { api } from "@/lib/api";
import { localeHref, supportsRussianRoute } from "@/lib/locale-routes";
import { DOCS_URL, GITHUB_URL } from "@/lib/site-links";
import { browserStorage, readSavedTheme, saveTheme, type SavedTheme } from "@/lib/user-preferences";
import { BackendPreconnect } from "./backend-preconnect";
import { useI18n } from "./i18n-provider";
import { T } from "./translated";

function localizeHref(language: string, href: string): string {
  return localeHref(href, language === "ru" ? "ru" : "en");
}

export function Brand() {
  const { language } = useI18n();
  return <Link className="brand" href={localeHref("/", language)} aria-label="apiToken.sale home">
    <Image className="brand-mark bm-light" src="/assets/logo-mark-light.png" width={24} height={24} alt="" />
    <Image className="brand-mark bm-dark" src="/assets/logo-mark-dark.png" width={24} height={24} alt="" />
    <span className="brand-name">apiToken.sale</span>
  </Link>;
}

export function SiteHeader({ home = false, compact = false }: { home?: boolean; compact?: boolean }) {
  const { language, setLanguage, t } = useI18n();
  const pathname = usePathname();
  const [menuOpen, setMenuOpen] = useState(false);
  const [authenticated, setAuthenticated] = useState(false);
  const burgerRef = useRef<HTMLButtonElement>(null);

  // Проверка идентичности стартует сразу при гидрации: сессионная кука HttpOnly и host-only,
  // поэтому ни JS, ни SSR не знают о логине без запроса — а залогиненный пользователь не должен
  // секундами видеть Login/Register. BackendPreconnect ниже греет TLS к API, чтобы ответ
  // пришёл за один RTT.
  useEffect(() => {
    let cancelled = false;
    api.me()
      .then(() => { if (!cancelled) setAuthenticated(true); })
      .catch(() => { if (!cancelled) setAuthenticated(false); });
    return () => { cancelled = true; };
  }, []);
  useEffect(() => {
    const frame = window.requestAnimationFrame(() => setMenuOpen(false));
    return () => window.cancelAnimationFrame(frame);
  }, [pathname]);
  useEffect(() => {
    if (!menuOpen) return;
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      setMenuOpen(false);
      window.requestAnimationFrame(() => burgerRef.current?.focus());
    }
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [menuOpen]);

  const ru = language === "ru";
  const loc = (path: string) => localeHref(path, language);
  const russianSupported = supportsRussianRoute(pathname);
  const languageLabel = ru ? "Язык" : "Language";
  const russianUnavailable = ru ? "Русская версия недоступна" : "Russian version unavailable";
  const links = <>
    <Link href={home ? "#how" : `${loc("/")}#how`}><T k="nav_how">How it works</T></Link>
    <Link href={loc("/integrations")}><T k="nav_int">Integrations</T></Link>
    <Link href={loc("/models")}><T k="nav_models">Models</T></Link>
    <Link href={loc("/docs/learn")}><T k="nav_guides">Guides</T></Link>
    <Link href={home ? "#pricing" : loc("/plans")}><T k="nav_pricing">Pricing</T></Link>
    <Link href={DOCS_URL} target="_blank" rel="noreferrer"><T k="nav_docs">Docs</T></Link>
  </>;

  const renderActions = () => authenticated ? <Link className="btn btn-primary" href={loc("/dashboard")}>{t("dash")}</Link> : <>
    <Link className="btn btn-ghost" href={loc("/login")}>{t("login")}</Link>
    <Link className="btn btn-primary" href={loc("/register")}>{t("signup")}</Link>
  </>;

  return <>
    <BackendPreconnect />
    <header className="nav">
    <a className="skip-link" href="#main-content">{ru ? "К содержимому" : "Skip to content"}</a>
    <div className="wrap nav-in">
      <Brand />
      {!compact && <nav id="site-navigation" className={`nav-links ${menuOpen ? "open" : ""}`} aria-label={ru ? "Основная навигация" : "Primary navigation"} onClick={(event) => {
        if ((event.target as HTMLElement).closest("a")) setMenuOpen(false);
      }}>
        {links}
        <div className="nav-auth-mobile">{renderActions()}</div>
      </nav>}
      <div className="nav-right">
        <div className="lang" role="group" aria-label={languageLabel}>
          <button type="button" className={language === "en" ? "active" : ""} aria-pressed={language === "en"} onClick={() => setLanguage("en")}>EN</button>
          <button type="button" className={language === "ru" ? "active" : ""} aria-pressed={language === "ru"} disabled={!russianSupported} aria-disabled={!russianSupported} title={russianSupported ? undefined : russianUnavailable} onClick={() => setLanguage("ru")}>RU</button>
        </div>
        <ThemeToggle />
        {!compact && <div className={`nav-actions ${authenticated ? "authenticated" : ""}`}>{renderActions()}</div>}
      </div>
      {!compact && <button ref={burgerRef} type="button" className="nav-burger" aria-label={menuOpen ? (ru ? "Закрыть меню" : "Close menu") : (ru ? "Открыть меню" : "Open menu")} aria-controls="site-navigation" aria-expanded={menuOpen} onClick={() => setMenuOpen((open) => !open)}>{menuOpen ? "×" : "☰"}</button>}
    </div>
  </header>
  </>;
}

export function ThemeToggle() {
  const [theme, setTheme] = useState<SavedTheme>("dark");
  const [mounted, setMounted] = useState(false);
  useEffect(() => {
    // Тема по умолчанию — тёмная; светлая только если пользователь её явно сохранил.
    const saved = readSavedTheme(browserStorage());
    const timer = window.setTimeout(() => {
      setTheme(saved);
      setMounted(true);
    }, 0);
    return () => window.clearTimeout(timer);
  }, []);
  useEffect(() => {
    if (!mounted) return;
    if (theme === "dark") document.documentElement.dataset.theme = "dark";
    else delete document.documentElement.dataset.theme;
    saveTheme(browserStorage(), theme);
  }, [mounted, theme]);
  return <button className="theme-tgl" aria-label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"} onClick={() => setTheme((current) => current === "dark" ? "light" : "dark")}>{theme === "dark" ? "☀" : "☾"}</button>;
}

export function SiteFooter({ full = false }: { full?: boolean }) {
  const { language } = useI18n();
  const l = (href: string) => localizeHref(language, href);
  if (!full) return <footer><div className="wrap"><div className="foot-bottom"><Brand /><T k="copyright" as="small">© 2026 apiToken.sale. All rights reserved.</T><FooterComplianceLinks /></div><T k="disclaimer" as="p" className="disclaimer">apiToken.sale is an independent platform and is not affiliated with or endorsed by Anthropic, PBC or OpenAI.</T></div></footer>;
  return <footer className="site-foot-full">
    <div className="wrap foot-inner">
      <div className="foot-grid">
        <div className="foot-brand"><Brand /><T k="foot_about" as="p">Claude and GPT API access platform for developers.</T></div>
        <FooterColumn title="foot_product" links={[["/plans","fp1"],["/models","fp2"],["/#pricing","fp3"],[DOCS_URL,"fp4"]]} />
        <FooterColumn title="foot_dev" links={[[DOCS_URL,"fd1"],[DOCS_URL,"fd2"],[DOCS_URL,"fd3"],[GITHUB_URL,"fd4"]]} />
        <div className="foot-col"><T k="foot_support" as="h4">Support</T><Link href={l("/support")}><T k="foot_support">Customer support</T></Link><Link href={l("/docs/learn")}>Guides</Link><Link href={l("/docs/errors")}><T k="foot_errors">Error codes</T></Link><Link href="/about">About</Link><Link href="/contacts">Contacts</Link><Link href="/changelog">Changelog</Link><Link href="/status">Status</Link></div>
        <div className="foot-col"><T k="foot_legal_h" as="h4">Legal</T><Link href={l("/terms")}><T k="legal_terms_h">User Agreement</T></Link><Link href={l("/privacy")}><T k="legal_privacy_h">Privacy Policy</T></Link><Link href={l("/plans")}><T k="nav_pricing">Prices &amp; tariffs</T></Link></div>
      </div>
      <div className="foot-bottom"><T k="copyright" as="small">© 2026 apiToken.sale. All rights reserved.</T><FooterComplianceLinks /></div>
      <T k="disclaimer" as="p" className="disclaimer">apiToken.sale is an independent platform and is not affiliated with or endorsed by Anthropic, PBC or OpenAI.</T>
    </div>
    <div className="foot-wordmark" aria-hidden="true">apiToken<span>.sale</span></div>
  </footer>;
}

function FooterComplianceLinks() {
  const { language } = useI18n();
  const l = (href: string) => localizeHref(language, href);
  return <span className="foot-legal"><Link href={l("/docs/learn")}><T k="nav_guides">Guides</T></Link><Link href={l("/privacy")}><T k="legal_privacy_h">Privacy Policy</T></Link><Link href={l("/terms")}><T k="legal_terms_h">User Agreement</T></Link><Link href={l("/support")}><T k="foot_support">Support</T></Link><Link href={l("/plans")}><T k="nav_pricing">Prices</T></Link></span>;
}

function FooterColumn({ title, links }: { title: string; links: Array<[string, string]> }) {
  const { language } = useI18n();
  return <div className="foot-col"><T k={title} as="h4">Section</T>{links.map(([href, key]) => {
    const external = href === DOCS_URL || /^https?:\/\//i.test(href);
    return <Link href={localizeHref(language, href)} key={key} target={external ? "_blank" : undefined} rel={external ? "noreferrer" : undefined}><T k={key}>{key}</T></Link>;
  })}</div>;
}
