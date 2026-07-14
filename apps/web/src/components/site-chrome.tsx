"use client";

import Image from "next/image";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { api } from "@/lib/api";
import { useI18n } from "./i18n-provider";
import { T } from "./translated";

export function Brand() {
  return <Link className="brand" href="/">
    <Image className="brand-mark bm-light" src="/assets/logo-mark-light.png" width={24} height={24} alt="" />
    <Image className="brand-mark bm-dark" src="/assets/logo-mark-dark.png" width={24} height={24} alt="" />
    apiToken.sale
  </Link>;
}

export function SiteHeader({ home = false, compact = false }: { home?: boolean; compact?: boolean }) {
  const router = useRouter();
  const { language, setLanguage, t } = useI18n();
  const [menuOpen, setMenuOpen] = useState(false);
  const [authenticated, setAuthenticated] = useState(false);

  useEffect(() => { api.me().then(() => setAuthenticated(true)).catch(() => setAuthenticated(false)); }, []);
  async function logout() { await api.logout().catch(() => undefined); setAuthenticated(false); router.push("/"); router.refresh(); }

  const links = <>
    <Link href={home ? "#how" : "/#how"}><T k="nav_how">How it works</T></Link>
    <Link href="/integrations"><T k="nav_int">Integrations</T></Link>
    <Link href="/models"><T k="nav_models">Models</T></Link>
    <Link href={home ? "#pricing" : "/plans"}><T k="nav_pricing">Pricing</T></Link>
    <Link href="/docs"><T k="nav_docs">Docs</T></Link>
  </>;

  const renderActions = () => authenticated ? <>
    <Link className="btn btn-ghost" href="/dashboard">{t("dash")}</Link>
    <button className="btn btn-primary" onClick={logout}>{t("logout")}</button>
  </> : <>
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
        <div className="lang"><button className={language === "en" ? "active" : ""} onClick={() => setLanguage("en")}>EN</button><button className={language === "ru" ? "active" : ""} onClick={() => setLanguage("ru")}>RU</button></div>
        <ThemeToggle />
        {!compact && <div className="nav-actions">{renderActions()}</div>}
      </div>
      {!compact && <button className="nav-burger" aria-label="Menu" aria-expanded={menuOpen} onClick={() => setMenuOpen((open) => !open)}>☰</button>}
    </div>
  </header>;
}

export function ThemeToggle() {
  const [theme, setTheme] = useState<"light" | "dark">("light");
  const [mounted, setMounted] = useState(false);
  useEffect(() => {
    const saved = window.localStorage.getItem("theme") === "dark" ? "dark" : "light";
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
  if (!full) return <footer><div className="wrap"><div className="foot-bottom"><Brand /><T k="copyright" as="small">© 2026 apiToken.sale. All rights reserved.</T></div><T k="disclaimer" as="p" className="disclaimer">apiToken.sale is an independent platform and is not affiliated with or endorsed by Anthropic, PBC.</T></div></footer>;
  return <footer><div className="wrap">
    <div className="foot-grid">
      <div className="foot-brand"><Brand /><T k="foot_about" as="p">Claude API access platform for developers.</T></div>
      <FooterColumn title="foot_product" links={[["/plans","fp1"],["/models","fp2"],["/#pricing","fp3"],["/docs","fp4"]]} />
      <FooterColumn title="foot_dev" links={[["/docs","fd1"],["/docs","fd2"],["/docs","fd3"]]} />
      <div className="foot-col"><T k="foot_int" as="h4">Integrations</T><Link href="/int-claude-code">Claude Code</Link><Link href="/int-cursor">Cursor</Link><Link href="/int-zed">Zed</Link><Link href="/integrations"><T k="foot_int_all">All integrations</T></Link></div>
      <div className="foot-col"><T k="foot_support" as="h4">Support</T><span><T k="fs1">Community</T></span><span><T k="fs2">Telegram support</T></span><span><T k="fs3">Contacts</T></span><span><T k="fs4">About</T></span></div>
      <div className="foot-col"><T k="foot_legal_h" as="h4">Legal</T><Link href="/terms"><T k="legal_terms_h">Terms</T></Link><Link href="/privacy"><T k="legal_privacy_h">Privacy</T></Link></div>
    </div>
    <div className="foot-bottom"><T k="copyright" as="small">© 2026 apiToken.sale. All rights reserved.</T><span className="foot-legal"><Link href="/terms"><T k="legal_terms_h">Terms</T></Link> · <Link href="/privacy"><T k="legal_privacy_h">Privacy</T></Link></span></div>
    <T k="disclaimer" as="p" className="disclaimer">apiToken.sale is an independent platform and is not affiliated with or endorsed by Anthropic, PBC.</T>
  </div></footer>;
}

function FooterColumn({ title, links }: { title: string; links: Array<[string, string]> }) {
  return <div className="foot-col"><T k={title} as="h4">Section</T>{links.map(([href, key]) => <Link href={href} key={key}><T k={key}>{key}</T></Link>)}</div>;
}
