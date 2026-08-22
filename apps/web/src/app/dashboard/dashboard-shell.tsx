"use client";

import Image from "next/image";
import Link from "next/link";
import { memo, useCallback, type MouseEvent as ReactMouseEvent } from "react";
import type { AccountView, AuthUser } from "@/lib/api";
import { ThemeToggle } from "@/components/site-chrome";
import type { DashboardCopy } from "@/lib/dashboard-copy";
import { DOCS_URL } from "@/lib/site-links";
import { dashboardHref, type DashboardLanguage, type DashboardSection } from "./dashboard-route";
import { formatNanoUsd } from "./sections/shared";
import { localeHref } from "@/lib/locale-routes";

const NAV_ICONS = {
  grid: <><rect x="3" y="3" width="7" height="7" rx="1.5" /><rect x="14" y="3" width="7" height="7" rx="1.5" /><rect x="3" y="14" width="7" height="7" rx="1.5" /><rect x="14" y="14" width="7" height="7" rx="1.5" /></>,
  key: <><circle cx="8" cy="15" r="4.5" /><path d="m11 12 9-9" /><path d="m16 7 3 3" /></>,
  external: <><path d="M14 4h6v6" /><path d="M20 4 11 13" /><path d="M19 14v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h5" /></>,
  wallet: <><rect x="3" y="5" width="18" height="14" rx="2" /><path d="M3 10h18" /><path d="M7 15h4" /></>,
  chart: <><path d="M4 20V11" /><path d="M10 20V4" /><path d="M16 20v-6" /><path d="M2 20h20" /></>,
  chat: <><path d="M21 12a8.5 8.5 0 0 1-8.5 8.5c-1.6 0-3.1-.4-4.4-1.2L3 21l1.7-5.1A8.5 8.5 0 1 1 21 12z" /></>,
  user: <><circle cx="12" cy="8" r="4" /><path d="M4 21c1.4-3.7 4.6-6 8-6s6.6 2.3 8 6" /></>,
} as const;

type NavIconId = keyof typeof NAV_ICONS;
type NavigationItem = { section?: DashboardSection; label: keyof DashboardCopy; icon: NavIconId; href?: string; group?: keyof DashboardCopy };

const navigation: readonly NavigationItem[] = [
  { group: "navStart", section: "overview", label: "navOverview", icon: "grid" },
  { group: "navDevelopers", section: "keys", label: "navKeys", icon: "key" },
  { href: DOCS_URL, label: "navDocs", icon: "external" },
  { group: "navBilling", section: "credits", label: "navTopUp", icon: "wallet" },
  { group: "navActivity", section: "usage", label: "navUsage", icon: "chart" },
  { group: "navSupportGroup", section: "support", label: "navSupport", icon: "chat" },
  { group: "navAccount", section: "profile", label: "navProfile", icon: "user" },
];

function NavIcon({ id }: { id: NavIconId }) {
  return <svg viewBox="0 0 24 24" aria-hidden="true">{NAV_ICONS[id]}</svg>;
}

function BrandImages() {
  return <><Image className="brand-mark bm-light" src="/assets/logo-mark-light.png" width={24} height={24} alt="" /><Image className="brand-mark bm-dark" src="/assets/logo-mark-dark.png" width={24} height={24} alt="" /></>;
}

type DashboardSidebarProps = {
  activeSection: DashboardSection;
  copy: DashboardCopy;
  language: DashboardLanguage;
  sideOpen: boolean;
  user: AuthUser;
  loggingOut: boolean;
  logoutLabel: string;
  onLanguageChange(language: DashboardLanguage): void;
  onNavigate(section: DashboardSection): void;
  onLogout(): void;
};

export const DashboardSidebar = memo(function DashboardSidebar({
  activeSection,
  copy,
  language,
  sideOpen,
  user,
  loggingOut,
  logoutLabel,
  onLanguageChange,
  onNavigate,
  onLogout,
}: DashboardSidebarProps) {
  const handleSectionNav = useCallback((event: ReactMouseEvent<HTMLAnchorElement>, next: DashboardSection) => {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    onNavigate(next);
  }, [onNavigate]);

  return <aside className={`side ${sideOpen ? "open" : ""}`} data-lang={language}>
    <Link className="brand side-brand" href={localeHref("/", language)}><BrandImages />apiToken.sale</Link>
    <nav className="side-nav">
      {navigation.map((item, index) => <div key={`${item.label}-${index}`} className="side-nav-item">
        {item.group && <span className="side-group">{copy[item.group]}</span>}
        {item.href ? <Link className="side-link" href={item.href} target="_blank" rel="noreferrer"><span className="si"><NavIcon id={item.icon} /></span><span>{copy[item.label]}</span></Link> :
          <Link data-dashboard-section={item.section} className={`side-link${activeSection === item.section ? " on" : ""}`} aria-current={activeSection === item.section ? "page" : undefined} href={dashboardHref(item.section!, language)} onClick={(event) => handleSectionNav(event, item.section!)}><span className="si"><NavIcon id={item.icon} /></span><span>{copy[item.label]}</span></Link>}
      </div>)}
    </nav>
    <div className="side-foot">
      <div className="side-tools"><div className="lang"><button className={language === "en" ? "active" : ""} aria-pressed={language === "en"} onClick={() => onLanguageChange("en")}>EN</button><button className={language === "ru" ? "active" : ""} aria-pressed={language === "ru"} onClick={() => onLanguageChange("ru")}>RU</button></div><ThemeToggle /></div>
      <nav className="side-legal" aria-label={language === "ru" ? "Правовая информация" : "Legal information"}>
        <Link href={localeHref("/privacy", language)} target="_blank">{language === "ru" ? "Конфиденциальность" : "Privacy"}</Link>
        <Link href={localeHref("/terms", language)} target="_blank">{language === "ru" ? "Соглашение" : "Agreement"}</Link>
        <Link href={localeHref("/support", language)} target="_blank">{language === "ru" ? "Поддержка" : "Support"}</Link>
        <Link href={localeHref("/plans", language)} target="_blank">{language === "ru" ? "Цены" : "Pricing"}</Link>
      </nav>
      <div className="side-user"><span className="side-av">{(user.displayName || user.email)[0]?.toUpperCase()}</span><div className="side-uinfo"><b>{user.displayName || user.email.split("@")[0]}</b><span>{user.email}</span></div></div>
      <button className="btn btn-ghost btn-sm side-logout" disabled={loggingOut} onClick={onLogout}>{loggingOut ? logoutLabel : copy.logout}</button>
    </div>
  </aside>;
});

export const DashboardScrim = memo(function DashboardScrim({ open, label, onClose }: { open: boolean; label: string; onClose(): void }) {
  return <button className={`side-scrim ${open ? "show" : ""}`} onClick={onClose} aria-label={label} />;
});

type DashboardTopBarProps = {
  activeSection: DashboardSection;
  account: AccountView;
  copy: DashboardCopy;
  locale: string;
  onMenu(): void;
  onOpenCredits(): void;
};

export const DashboardTopBar = memo(function DashboardTopBar({ activeSection, account, copy, locale, onMenu, onOpenCredits }: DashboardTopBarProps) {
  const titleKey = navigation.find((item) => item.section === activeSection)?.label ?? "navOverview";
  return <header className="app-top">
    <div className="app-top-in">
      <button className="app-burger" onClick={onMenu} aria-label={copy.menu}>☰</button>
      <div className="app-top-h"><div className="app-title">{copy[titleKey]}</div></div>
      <div className="app-top-actions">
        <button className="app-top-bal" onClick={onOpenCredits} title={copy.navTopUp}>
          <span className="atb-ic" aria-hidden="true" />
          <span className="atb-label">{copy.creditsLabel}</span>
          <span className={`atb-val${BigInt(account.balanceNano) < 0n ? " atb-neg" : ""}`}>{formatNanoUsd(account.balanceNano, locale)}</span>
        </button>
      </div>
    </div>
  </header>;
});

export { navigation };
