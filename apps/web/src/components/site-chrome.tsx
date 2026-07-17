"use client";

import Image from "next/image";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import { api } from "@/lib/api";
import { DOCS_URL } from "@/lib/site-links";
import { useI18n } from "./i18n-provider";
import { T } from "./translated";

// Internal paths that have a /ru mirror — so links inside the Russian site
// stay in Russian (no cross-locale dead-ends for crawlers).
const RU_MIRRORED = new Set(["/models", "/integrations", "/plans", "/privacy", "/terms", "/support"]);
function localizeHref(language: string, href: string): string {
  if (language !== "ru" || !href.startsWith("/")) return href;
  const base = href.split(/[#?]/)[0];
  const mirrored = base === "/" || RU_MIRRORED.has(base) || base.startsWith("/int-") || base === "/docs/learn" || base.startsWith("/docs/learn/");
  if (!mirrored) return href;
  return href === "/" ? "/ru" : `/ru${href}`;
}

export function Brand() {
  const { language } = useI18n();
  return <Link className="brand" href={language === "ru" ? "/ru" : "/"}>
    <Image className="brand-mark bm-light" src="/assets/logo-mark-light.png" width={24} height={24} alt="" />
    <Image className="brand-mark bm-dark" src="/assets/logo-mark-dark.png" width={24} height={24} alt="" />
    apiToken.sale
  </Link>;
}

export function SiteHeader({ home = false, compact = false }: { home?: boolean; compact?: boolean }) {
  const { language, t } = useI18n();
  const pathname = usePathname();
  const [menuOpen, setMenuOpen] = useState(false);
  const [authenticated, setAuthenticated] = useState(false);

  useEffect(() => { api.me().then(() => setAuthenticated(true)).catch(() => setAuthenticated(false)); }, []);
  useEffect(() => {
    const frame = window.requestAnimationFrame(() => setMenuOpen(false));
    return () => window.cancelAnimationFrame(frame);
  }, [pathname]);

  const ru = language === "ru";
  const loc = (path: string) => (ru ? (path === "/" ? "/ru" : `/ru${path}`) : path);
  const links = <>
    <Link href={home ? "#how" : `${loc("/")}#how`}><T k="nav_how">How it works</T></Link>
    <Link href={loc("/integrations")}><T k="nav_int">Integrations</T></Link>
    <Link href={loc("/models")}><T k="nav_models">Models</T></Link>
    <Link href={home ? "#pricing" : loc("/plans")}><T k="nav_pricing">Pricing</T></Link>
    <Link href={DOCS_URL} target="_blank" rel="noreferrer"><T k="nav_docs">Docs</T></Link>
  </>;

  const renderActions = () => authenticated ? <Link className="btn btn-primary" href="/dashboard">{t("dash")}</Link> : <>
    <Link className="btn btn-ghost" href="/login">{t("login")}</Link>
    <Link className="btn btn-primary" href="/register">{t("signup")}</Link>
  </>;

  return <header className="nav">
    <div className="wrap nav-in">
      <Brand />
      {!compact && <nav className={`nav-links ${menuOpen ? "open" : ""}`}>
        {links}
        <div className="nav-auth-mobile">{renderActions()}</div>
      </nav>}
      <div className="nav-right">
        <div className="lang"><Link className={language === "en" ? "active" : ""} href={ru ? (pathname.replace(/^\/ru/, "") || "/") : pathname} hrefLang="en">EN</Link><Link className={language === "ru" ? "active" : ""} href={ru ? pathname : (pathname === "/" ? "/ru" : `/ru${pathname}`)} hrefLang="ru">RU</Link></div>
        <ThemeToggle />
        {!compact && <div className={`nav-actions ${authenticated ? "authenticated" : ""}`}>{renderActions()}</div>}
      </div>
      {!compact && <button className="nav-burger" aria-label="Menu" aria-expanded={menuOpen} onClick={() => setMenuOpen((open) => !open)}>☰</button>}
    </div>
  </header>;
}

export function ThemeToggle() {
  const [theme, setTheme] = useState<"light" | "dark">("dark");
  const [mounted, setMounted] = useState(false);
  useEffect(() => {
    // Тема по умолчанию — тёмная; светлая только если пользователь её явно сохранил.
    const saved = window.localStorage.getItem("theme") === "light" ? "light" : "dark";
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
    window.localStorage.setItem("theme", theme);
  }, [mounted, theme]);
  return <button className="theme-tgl" aria-label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"} onClick={() => setTheme((current) => current === "dark" ? "light" : "dark")}>{theme === "dark" ? "☀" : "☾"}</button>;
}

export function SiteFooter({ full = false }: { full?: boolean }) {
  const { language } = useI18n();
  const l = (href: string) => localizeHref(language, href);
  if (!full) return <footer><div className="wrap"><div className="foot-bottom"><Brand /><T k="copyright" as="small">© 2026 apiToken.sale. All rights reserved.</T><FooterComplianceLinks /></div><T k="disclaimer" as="p" className="disclaimer">apiToken.sale is an independent platform and is not affiliated with or endorsed by Anthropic, PBC.</T></div></footer>;
  return <footer className="site-foot-full">
    <div className="wrap foot-inner">
      <div className="foot-grid">
        <div className="foot-brand"><Brand /><T k="foot_about" as="p">Claude API access platform for developers.</T></div>
        <FooterColumn title="foot_product" links={[["/plans","fp1"],["/models","fp2"],["/#pricing","fp3"],[DOCS_URL,"fp4"]]} />
        <FooterColumn title="foot_dev" links={[[DOCS_URL,"fd1"],[DOCS_URL,"fd2"],[DOCS_URL,"fd3"]]} />
        <div className="foot-col"><T k="foot_int" as="h4">Integrations</T><Link href={l("/int-claude-code")}>Claude Code</Link><Link href={l("/int-cursor")}>Cursor</Link><Link href={l("/int-zed")}>Zed</Link><Link href={l("/integrations")}><T k="foot_int_all">All integrations</T></Link></div>
        <div className="foot-col"><T k="foot_support" as="h4">Support</T><Link href={l("/support")}><T k="foot_support">Customer support</T></Link><Link href={l("/docs/learn")}>Guides</Link><Link href="/about">About</Link><Link href="/contacts">Contacts</Link><Link href="/changelog">Changelog</Link><Link href="/status">Status</Link><a href="mailto:apitokensale@gmail.com">apitokensale@gmail.com</a></div>
        <div className="foot-col"><T k="foot_legal_h" as="h4">Legal</T><Link href={l("/terms")}><T k="legal_terms_h">User Agreement</T></Link><Link href={l("/privacy")}><T k="legal_privacy_h">Privacy Policy</T></Link><Link href={l("/plans")}><T k="nav_pricing">Prices &amp; tariffs</T></Link></div>
      </div>
      <div className="foot-bottom"><T k="copyright" as="small">© 2026 apiToken.sale. All rights reserved.</T><FooterComplianceLinks /></div>
      <T k="disclaimer" as="p" className="disclaimer">apiToken.sale is an independent platform and is not affiliated with or endorsed by Anthropic, PBC.</T>
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
  return <div className="foot-col"><T k={title} as="h4">Section</T>{links.map(([href, key]) => <Link href={localizeHref(language, href)} key={key} target={href === DOCS_URL ? "_blank" : undefined} rel={href === DOCS_URL ? "noreferrer" : undefined}><T k={key}>{key}</T></Link>)}</div>;
}
