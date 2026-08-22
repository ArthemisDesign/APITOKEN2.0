"use client";

// Простой inline-билингвальный слой (RU/EN) для партнёрского кабинета.
// Строки не выносим в ключи — переводим прямо на месте: t("English", "Русский").
// Язык хранится в общем localStorage("lang:v1"), как в основном dashboard. Старый
// ключ sales_lang читается один раз для мягкой миграции существующих кабинетов.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";

export type Lang = "en" | "ru";

interface I18nValue {
  lang: Lang;
  setLang: (lang: Lang) => void;
  /** Возвращает строку для текущего языка. */
  t: (en: string, ru: string) => string;
}

const I18nContext = createContext<I18nValue | null>(null);

const STORAGE_KEY = "lang:v1";
const LEGACY_STORAGE_KEY = "sales_lang";

/** Set the document language before the first paint, using the same shared key as the main site. */
export const languageBootstrapScript = `(()=>{try{const s=localStorage.getItem('${STORAGE_KEY}')??localStorage.getItem('${LEGACY_STORAGE_KEY}');if(s==='ru')document.documentElement.lang='ru'}catch{}})()`;

function detectDefault(): Lang {
  if (typeof navigator === "undefined") return "en";
  const nav = (navigator.language || "").toLowerCase();
  return nav.startsWith("ru") ? "ru" : "en";
}

export function I18nProvider({ children }: { children: ReactNode }) {
  // Стабильный первый рендер ("en") — чтобы не было hydration mismatch;
  // реальный язык применяем после монтирования.
  const [lang, setLangState] = useState<Lang>("en");

  useEffect(() => {
    let resolved: Lang | null = null;
    try {
      const stored = localStorage.getItem(STORAGE_KEY) ?? localStorage.getItem(LEGACY_STORAGE_KEY);
      if (stored === "en" || stored === "ru") resolved = stored;
    } catch {
      // localStorage недоступен — игнорируем
    }
    if (!resolved) resolved = detectDefault();
    if (resolved !== lang) setLangState(resolved);
    try {
      document.documentElement.lang = resolved;
    } catch {
      // document is unavailable during non-browser rendering.
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const setLang = useCallback((next: Lang) => {
    setLangState(next);
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // игнорируем
    }
    try {
      document.documentElement.lang = next;
    } catch {
      // document is unavailable during non-browser rendering.
    }
  }, []);

  const t = useCallback((en: string, ru: string) => (lang === "ru" ? ru : en), [lang]);

  return <I18nContext.Provider value={{ lang, setLang, t }}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error("useI18n must be used inside <I18nProvider>");
  return ctx;
}

/** Локаль для toLocaleDateString и т.п. */
export function localeFor(lang: Lang): string {
  return lang === "ru" ? "ru-RU" : "en-US";
}

/** Небольшой переключатель EN / RU для шапки. */
export function LanguageToggle({ className = "" }: { className?: string }) {
  const { lang, setLang, t } = useI18n();
  return (
    <div className={`lang-toggle${className ? ` ${className}` : ""}`} role="group" aria-label={t("Language", "Язык")}>
      <button
        type="button"
        className={`lang-opt${lang === "en" ? " active" : ""}`}
        aria-pressed={lang === "en"}
        onClick={() => setLang("en")}
      >
        EN
      </button>
      <button
        type="button"
        className={`lang-opt${lang === "ru" ? " active" : ""}`}
        aria-pressed={lang === "ru"}
        onClick={() => setLang("ru")}
      >
        RU
      </button>
    </div>
  );
}
