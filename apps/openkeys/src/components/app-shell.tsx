"use client";

import Link from "next/link";
import { useState } from "react";
import { BrandMark, ThemeToggle } from "@/components/chrome";

export type ShellSection = "profile" | "docs" | "stock" | "monitor";

interface NavItem {
  section: ShellSection;
  href: string;
  label: string;
  icon: string;
}

/**
 * Два разных контура, и смешивать их нельзя: покупателю нечего делать в админке,
 * а админу незачем уходить из неё в клиентские страницы посреди работы.
 */
const CLIENT_NAV: NavItem[] = [
  { section: "profile", href: "/profile", label: "Расход ключа", icon: "◧" },
  { section: "docs", href: "/docs", label: "Документация", icon: "❑" },
];

const ADMIN_NAV: NavItem[] = [
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
  const isAdmin = section === "stock" || section === "monitor";
  const nav = isAdmin ? ADMIN_NAV : CLIENT_NAV;

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
            <nav className="side-legal" aria-label="Ссылки">
              <a href="https://apitoken.sale" target="_blank" rel="noreferrer">
                apiToken.sale
              </a>
              <a href="https://apitoken.sale/docs/learn" target="_blank" rel="noreferrer">
                Гайды по Claude API
              </a>
            </nav>
          )}
        </div>
      </aside>
      <button
        className={`side-scrim ${sideOpen ? "show" : ""}`}
        onClick={() => setSideOpen(false)}
        aria-label="Закрыть меню"
      />
      <main className="app-main" id="main-content">
        <header className="app-top">
          <div className="app-top-in">
            <button className="app-burger" onClick={() => setSideOpen(true)} aria-label="Меню">
              ☰
            </button>
            <div className="app-top-h">
              <div className="app-title">{title}</div>
            </div>
            {actions ? <div className="app-top-actions">{actions}</div> : null}
          </div>
        </header>
        {children}
      </main>
    </div>
  );
}
