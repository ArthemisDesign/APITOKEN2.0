"use client";

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import messages from "@/lib/messages.json";

export type Language = "en" | "ru";
type Dictionary = Record<string, string>;

interface I18nContextValue {
  language: Language;
  setLanguage(language: Language): void;
  t(key: string): string;
}

const I18nContext = createContext<I18nContextValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [language, setLanguageState] = useState<Language>("en");

  useEffect(() => {
    const saved = window.localStorage.getItem("lang");
    window.localStorage.removeItem("onekey_db_v1");
    window.localStorage.removeItem("onekey_session");
    window.localStorage.removeItem("onekey_promos");
    const timer = window.setTimeout(() => {
      if (saved === "ru" || saved === "en") setLanguageState(saved);
    }, 0);
    return () => window.clearTimeout(timer);
  }, []);

  const setLanguage = useCallback((next: Language) => {
    window.localStorage.setItem("lang", next);
    setLanguageState(next);
  }, []);

  const t = useCallback((key: string) => {
    const dictionary = messages[language] as Dictionary;
    const fallback = messages.en as Dictionary;
    return dictionary[key] ?? fallback[key] ?? key;
  }, [language]);

  useEffect(() => {
    document.documentElement.lang = language;
    document.documentElement.dataset.language = language;
  }, [language]);

  const value = useMemo(() => ({ language, setLanguage, t }), [language, setLanguage, t]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nContextValue {
  const value = useContext(I18nContext);
  if (!value) throw new Error("useI18n must be used within I18nProvider");
  return value;
}
