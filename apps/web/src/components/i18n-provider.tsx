"use client";

import { usePathname, useRouter } from "next/navigation";
import { createContext, useCallback, useContext, useMemo, type ReactNode } from "react";
import messages from "@/lib/messages.json";
import { localeRoute } from "@/lib/locale-routes";

export type Language = "en" | "ru";
type Dictionary = Record<string, string>;

interface I18nContextValue {
  language: Language;
  setLanguage(language: Language): void;
  t(key: string): string;
}

const I18nContext = createContext<I18nContextValue | null>(null);

function isRuPath(pathname: string): boolean {
  return pathname === "/ru" || pathname.startsWith("/ru/");
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();
  // Language follows the URL: /ru and /ru/* render Russian, everything else English.
  const language: Language = isRuPath(pathname) ? "ru" : "en";

  const setLanguage = useCallback((next: Language) => {
    const destination = localeRoute(pathname, next);
    if (!destination) return;
    try { window.localStorage.setItem("lang:v1", next); } catch { /* ignore */ }
    const suffix = `${window.location.search}${window.location.hash}`;
    if (destination !== pathname) router.push(`${destination}${suffix}`);
  }, [pathname, router]);

  const t = useCallback((key: string) => {
    const dictionary = messages[language] as Dictionary;
    const fallback = messages.en as Dictionary;
    return dictionary[key] ?? fallback[key] ?? key;
  }, [language]);

  const value = useMemo(() => ({ language, setLanguage, t }), [language, setLanguage, t]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nContextValue {
  const value = useContext(I18nContext);
  if (!value) throw new Error("useI18n must be used within I18nProvider");
  return value;
}
