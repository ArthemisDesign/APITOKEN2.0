"use client";

import Link from "next/link";
import { useState } from "react";
import { BrandMark, LanguageToggle, ThemeToggle, useLanguage } from "@/components/chrome";

export type ShellSection = "profile" | "docs" | "claudeDocs" | "openaiDocs" | "support" | "stock" | "monitor";

interface NavItem {
  section: ShellSection;
  href: string;
  label: { en: string; ru: string };
  icon: string;
}

/**
 * Два разных контура, и смешивать их нельзя: покупателю нечего делать в админке,
 * а админу незачем уходить из неё в клиентские страницы посреди работы.
 */
const CLIENT_NAV: NavItem[] = [
  { section: "profile", href: "/profile", label: { en: "Key usage", ru: "Расход ключа" }, icon: "◧" },
  { section: "docs", href: "/docs", label: { en: "Connect", ru: "Как подключить" }, icon: "◎" },
  { section: "claudeDocs", href: "/docs/claude", label: { en: "Claude API", ru: "Claude API" }, icon: "❑" },
  { section: "openaiDocs", href: "/docs/openai", label: { en: "GPT / OpenAI", ru: "GPT / OpenAI" }, icon: "◇" },
  { section: "support", href: "/support", label: { en: "Support", ru: "Поддержка" }, icon: "◌" },
];

const ADMIN_NAV: Array<Omit<NavItem, "label"> & { label: string }> = [
  { section: "stock", href: "/admin", label: "Склад ключей", icon: "◧" },
  { section: "monitor", href: "/admin/monitor", label: "Наблюдение", icon: "◔" },
];

/** Тот же каркас, что у дашборда: разделы слева, содержимое справа. */
export function AppShell({
  section,
  title,
  actions,
  children,
}: {
  section: ShellSection;
  title: string;
  actions?: React.ReactNode;
  children: React.ReactNode;
}) {
  const [sideOpen, setSideOpen] = useState(false);
  const { language } = useLanguage();
  const isAdmin = section === "stock" || section === "monitor";
  const nav: Array<{ section: ShellSection; href: string; label: string; icon: string }> = isAdmin
    ? ADMIN_NAV
    : CLIENT_NAV.map((item) => ({ ...item, label: item.label[language] }));

  return (
    <div className="app">
      <aside className={`side ${sideOpen ? "open" : ""}`}>
        <Link className="brand side-brand" href={isAdmin ? "/admin" : "/profile"}>
          <BrandMark />
          apiToken
          <i className="openkeys-mark">{isAdmin ? "admin" : "openKeys"}</i>
        </Link>
        <nav className="side-nav">
          {nav.map((item) => (
            <div key={item.section} className="side-nav-item">
              <Link
                className={`side-link ${section === item.section ? "on" : ""}`}
                aria-current={section === item.section ? "page" : undefined}
                href={item.href}
              >
                <span className="si">{item.icon}</span>
                <span>{item.label}</span>
              </Link>
            </div>
          ))}
        </nav>
        <div className="side-foot">
          <div className="side-tools">
            <ThemeToggle />
          </div>
          {isAdmin ? null : (
            <nav className="side-legal" aria-label={language === "en" ? "External links" : "Внешние ссылки"}>
              <a href="https://apitoken.sale" target="_blank" rel="noreferrer">
                apiToken.sale
              </a>
              <a href="https://apitoken.sale/docs/learn" target="_blank" rel="noreferrer">
                {language === "en" ? "Claude API guides" : "Гайды по Claude API"}
              </a>
            </nav>
          )}
        </div>
      </aside>
      <button
        className={`side-scrim ${sideOpen ? "show" : ""}`}
        onClick={() => setSideOpen(false)}
        aria-label={language === "en" ? "Close menu" : "Закрыть меню"}
      />
      <main className="app-main" id="main-content">
        <header className="app-top">
          <div className="app-top-in">
            <button className="app-burger" onClick={() => setSideOpen(true)} aria-label={language === "en" ? "Menu" : "Меню"}>
              ☰
            </button>
            <div className="app-top-h">
              <div className="app-title">{title}</div>
            </div>
            {actions || !isAdmin ? (
              <div className="app-top-actions">
                {actions}
                {!isAdmin ? <LanguageToggle /> : null}
              </div>
            ) : null}
          </div>
        </header>
        {children}
      </main>
    </div>
  );
}
