"use client";

import Image from "next/image";
import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";

/** Тема по умолчанию тёмная; светлая — только если пользователь сохранил её явно. */
export function ThemeToggle() {
  const { language } = useLanguage();
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
      aria-label={theme === "dark"
        ? language === "en" ? "Use light theme" : "Включить светлую тему"
        : language === "en" ? "Use dark theme" : "Включить тёмную тему"}
      onClick={() => setTheme((current) => (current === "dark" ? "light" : "dark"))}
    >
      {theme === "dark" ? "☀" : "☾"}
    </button>
  );
}

export type Language = "ru" | "en";

interface LanguageContextValue {
  language: Language;
  setLanguage: (next: Language) => void;
}

const LanguageContext = createContext<LanguageContextValue | null>(null);

/**
 * Язык хранится локально: OpenKeys — одностраничный продукт без локализованных
 * маршрутов, поэтому префиксы вида /ru здесь не нужны.
 */
export function LanguageProvider({ children }: { children: React.ReactNode }) {
  const [language, setLanguageState] = useState<Language>("en");

  useEffect(() => {
    const saved = window.localStorage.getItem("openkeys-lang");
    if (saved === "en" || saved === "ru") setLanguageState(saved);
  }, []);

  useEffect(() => {
    document.documentElement.lang = language;
  }, [language]);

  const setLanguage = useCallback((next: Language) => {
    setLanguageState(next);
    window.localStorage.setItem("openkeys-lang", next);
  }, []);

  const value = useMemo(() => ({ language, setLanguage }), [language, setLanguage]);
  return <LanguageContext.Provider value={value}>{children}</LanguageContext.Provider>;
}

export function useLanguage(): LanguageContextValue {
  const value = useContext(LanguageContext);
  if (!value) throw new Error("useLanguage must be used inside LanguageProvider");
  return value;
}

export function LanguageToggle() {
  const { language, setLanguage } = useLanguage();
  return (
    <div className="lang" role="group" aria-label={language === "ru" ? "Язык" : "Language"}>
      <button type="button" className={language === "en" ? "active" : ""} aria-pressed={language === "en"} onClick={() => setLanguage("en")}>EN</button>
      <button type="button" className={language === "ru" ? "active" : ""} aria-pressed={language === "ru"} onClick={() => setLanguage("ru")}>RU</button>
    </div>
  );
}

export function BrandMark() {
  return (
    <>
      <Image className="brand-mark bm-light" src="/assets/logo-mark-light.png" width={24} height={24} alt="" />
      <Image className="brand-mark bm-dark" src="/assets/logo-mark-dark.png" width={24} height={24} alt="" />
    </>
  );
}
