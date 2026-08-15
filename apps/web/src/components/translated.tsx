"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useCallback, useSyncExternalStore, type ElementType, type HTMLAttributes, type ReactNode } from "react";
import { localeHref, supportsRussianRoute } from "@/lib/locale-routes";
import { browserStorage, readSavedLanguage } from "@/lib/user-preferences";
import { useI18n } from "./i18n-provider";

export function T({ k, as: Tag = "span", children, ...props }: {
  k: string;
  as?: ElementType;
  children?: ReactNode;
} & HTMLAttributes<HTMLElement>) {
  const { t } = useI18n();
  return <Tag {...props} data-i18n-key={k}>{t(k) === k ? children : t(k)}</Tag>;
}

export function LocalizedLink({ href, className, children }: { href: string; className?: string; children: ReactNode }) {
  const pathname = usePathname();
  const savedLanguage = useSyncExternalStore(
    useCallback(() => () => {}, []),
    useCallback(() => readSavedLanguage(browserStorage()), []),
    useCallback(() => null, []),
  );
  const urlLanguage = pathname === "/ru" || pathname.startsWith("/ru/") ? "ru" : "en";
  const language = !supportsRussianRoute(pathname) && savedLanguage ? savedLanguage : urlLanguage;
  return <Link className={className} href={localeHref(href, language)}>{children}</Link>;
}
