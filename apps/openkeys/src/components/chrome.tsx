"use client";

import Image from "next/image";
import Link from "next/link";
import { useCallback, useEffect, useState } from "react";

/** Тема по умолчанию тёмная; светлая — только если пользователь сохранил её явно. */
export function ThemeToggle() {
  const [theme, setTheme] = useState<"light" | "dark">("dark");
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
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

  return (
    <button
      className="theme-tgl"
      aria-label={theme === "dark" ? "Светлая тема" : "Тёмная тема"}
      onClick={() => setTheme((current) => (current === "dark" ? "light" : "dark"))}
    >
      {theme === "dark" ? "☀" : "☾"}
    </button>
  );
}

export type Language = "ru" | "en";

/**
 * Язык хранится локально: OpenKeys — одностраничный продукт без локализованных
 * маршрутов, поэтому префиксы вида /ru здесь не нужны.
 */
export function useLanguage(): { language: Language; setLanguage: (next: Language) => void } {
  const [language, setLanguageState] = useState<Language>("ru");

  useEffect(() => {
    const saved = window.localStorage.getItem("openkeys-lang");
    if (saved === "en" || saved === "ru") setLanguageState(saved);
  }, []);

  const setLanguage = useCallback((next: Language) => {
    setLanguageState(next);
    window.localStorage.setItem("openkeys-lang", next);
  }, []);

  return { language, setLanguage };
}

export function BrandMark() {
  return (
    <>
      <Image className="brand-mark bm-light" src="/assets/logo-mark-light.png" width={24} height={24} alt="" />
      <Image className="brand-mark bm-dark" src="/assets/logo-mark-dark.png" width={24} height={24} alt="" />
    </>
  );
}

export function SiteHeader() {
  return (
    <header className="nav">
      <a className="skip-link" href="#main-content">
        К содержимому
      </a>
      <div className="wrap nav-in openkeys-nav-in">
        <Link className="brand" href="/">
          <BrandMark />
          <span className="brand-name">OpenKeys</span>
        </Link>
        <nav className="nav-links" id="site-navigation">
          <Link href="/docs">Подключение</Link>
          <Link href="/usage">Мой расход</Link>
        </nav>
        <div className="nav-right">
          <ThemeToggle />
          <div className="nav-actions">
            <a className="btn btn-ghost btn-sm" href="https://apitoken.sale" target="_blank" rel="noreferrer">
              apiToken.sale
            </a>
          </div>
        </div>
      </div>
    </header>
  );
}
