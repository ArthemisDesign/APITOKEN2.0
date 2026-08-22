"use client";

import { createContext, useCallback, useContext, useSyncExternalStore, type ReactNode } from "react";

export type Lang = "en" | "ru";
type I18nValue = { lang: Lang; setLang: (lang: Lang) => void; t: (en: string, ru: string) => string };

const I18nContext = createContext<I18nValue | null>(null);
export const LANGUAGE_STORAGE_KEY = "lang:v1";
export const languageScript = `(()=>{try{const l=localStorage.getItem('${LANGUAGE_STORAGE_KEY}');const n=(navigator.languages&&navigator.languages[0])||navigator.language;document.documentElement.lang=l==='en'||l==='ru'?l:(n.toLowerCase().startsWith('ru')?'ru':'en')}catch{}})()`;
const LANGUAGE_EVENT = "apitoken:language";

function readLanguage(): Lang {
  try {
    const stored = localStorage.getItem(LANGUAGE_STORAGE_KEY);
    if (stored === "en" || stored === "ru") return stored;
  } catch {}
  const browserLanguage = navigator.languages?.[0] ?? navigator.language;
  return browserLanguage.toLowerCase().startsWith("ru") ? "ru" : "en";
}

function subscribeLanguage(onChange: () => void): () => void {
  const refresh = () => {
    document.documentElement.lang = readLanguage();
    onChange();
  };
  const handleStorage = (event: StorageEvent) => {
    if (event.key === LANGUAGE_STORAGE_KEY) refresh();
  };
  window.addEventListener("storage", handleStorage);
  window.addEventListener(LANGUAGE_EVENT, refresh);
  return () => {
    window.removeEventListener("storage", handleStorage);
    window.removeEventListener(LANGUAGE_EVENT, refresh);
  };
}

function getServerLanguage(): Lang {
  return "ru";
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const lang = useSyncExternalStore(subscribeLanguage, readLanguage, getServerLanguage);
  const setLang = useCallback((next: Lang) => {
    document.documentElement.lang = next;
    try { localStorage.setItem(LANGUAGE_STORAGE_KEY, next); } catch {}
    window.dispatchEvent(new Event(LANGUAGE_EVENT));
  }, []);
  const t = useCallback((en: string, ru: string) => lang === "ru" ? ru : en, [lang]);
  return <I18nContext.Provider value={{ lang, setLang, t }}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  const value = useContext(I18nContext);
  if (!value) throw new Error("useI18n must be used inside I18nProvider");
  return value;
}

export function localeFor(lang: Lang): string {
  return lang === "ru" ? "ru-RU" : "en-US";
}

export function LanguageToggle() {
  const { lang, setLang, t } = useI18n();
  return <div className="admin-lang" role="group" aria-label={t("Language", "Язык")}>
    <button type="button" className={lang === "en" ? "on" : ""} aria-pressed={lang === "en"} onClick={() => setLang("en")}>EN</button>
    <button type="button" className={lang === "ru" ? "on" : ""} aria-pressed={lang === "ru"} onClick={() => setLang("ru")}>RU</button>
  </div>;
}
