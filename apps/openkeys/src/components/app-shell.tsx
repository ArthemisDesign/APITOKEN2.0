"use client";

import Link from "next/link";
import { useState } from "react";
import { BrandMark, ThemeToggle } from "@/components/chrome";

export type ShellSection = "profile" | "docs";

const NAV: { section: ShellSection; href: string; label: string; icon: string }[] = [
  { section: "profile", href: "/profile", label: "Расход ключа", icon: "◧" },
  { section: "docs", href: "/docs", label: "Документация", icon: "❑" },
];

/**
 * Тот же каркас, что у дашборда: разделы слева, содержимое справа. Пункты —
 * обычные ссылки, потому что страницы здесь серверные и живут на своих адресах.
 */
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

  return (
    <div className="app">
      <aside className={`side ${sideOpen ? "open" : ""}`}>
        <Link className="brand side-brand" href="/profile">
          <BrandMark />
          apiToken
          <i className="openkeys-mark">openKeys</i>
        </Link>
        <nav className="side-nav">
          {NAV.map((item) => (
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
          <nav className="side-legal" aria-label="Ссылки">
            <a href="https://apitoken.sale" target="_blank" rel="noreferrer">
              apiToken.sale
            </a>
            <a href="https://apitoken.sale/docs/learn" target="_blank" rel="noreferrer">
              Гайды по Claude API
            </a>
          </nav>
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
