"use client";

import { Analytics, type BeforeSendEvent } from "@vercel/analytics/next";
import { usePathname } from "next/navigation";
import { useEffect, useRef } from "react";
import { detectAiSource } from "@/lib/ai-source";
import { coarseAcquisition, trackFirstProductEvent } from "@/lib/product-analytics";
import { YANDEX_METRIKA_ID } from "@/lib/yandex-metrika";

type YandexMetrika = (counterId: number, method: string, ...args: unknown[]) => void;

declare global {
  interface Window {
    ym?: YandexMetrika;
  }
}

const VERCEL_UTM_PARAMETERS = new Set([
  "utm_source",
  "utm_medium",
  "utm_campaign",
  "utm_content",
  "utm_term",
  "utm_referrer",
]);

export function withoutSensitiveUrlData(url: string): string {
  const withoutFragment = url.split("#", 1)[0]!;
  const queryStart = withoutFragment.indexOf("?");
  if (queryStart === -1) return withoutFragment;

  const path = withoutFragment.slice(0, queryStart);
  const query = new URLSearchParams(withoutFragment.slice(queryStart + 1));
  for (const parameter of Array.from(query.keys())) {
    if (!VERCEL_UTM_PARAMETERS.has(parameter)) query.delete(parameter);
  }

  const safeQuery = query.toString();
  return safeQuery ? `${path}?${safeQuery}` : path;
}

const AI_SOURCE_SESSION_KEY = "ai_source_reported";

export function SiteAnalytics() {
  const pathname = usePathname();
  const previousPathname = useRef(pathname);

  // Anonymous, browser-local acquisition milestone. No account identity, full
  // referrer, query string, or campaign value outside a conservative allowlist.
  useEffect(() => {
    trackFirstProductEvent("touch", "First Touch", coarseAcquisition());
  }, []);

  // Keep <html lang> in sync with the localized route subtree (root layout
  // renders lang="en"; hreflang tags carry the authoritative signal for Google).
  useEffect(() => {
    const lang = pathname.startsWith("/zh") ? "zh-CN" : pathname.startsWith("/ru") ? "ru" : pathname.startsWith("/ko") ? "ko" : "en";
    if (document.documentElement.lang !== lang) document.documentElement.lang = lang;
  }, [pathname]);

  // Report AI-assistant referrals once per session for GEO ROI measurement.
  useEffect(() => {
    try {
      if (sessionStorage.getItem(AI_SOURCE_SESSION_KEY)) return;
      const utmSource = new URLSearchParams(location.search).get("utm_source") ?? "";
      const source = detectAiSource(document.referrer, utmSource);
      if (!source) return;
      sessionStorage.setItem(AI_SOURCE_SESSION_KEY, source);
      window.ym?.(YANDEX_METRIKA_ID, "params", { ai_source: source });
      window.ym?.(YANDEX_METRIKA_ID, "reachGoal", "ai_referral", { ai_source: source });
    } catch {
      // sessionStorage or referrer unavailable — ignore.
    }
  }, []);

  // Шеринг-ссылки приходят с utm_*, но посетитель должен видеть чистый https://apitoken.sale.
  // Ждём, пока Vercel Analytics и Метрика зафиксируют pageview с метками, затем чистим адрес.
  useEffect(() => {
    const hasUtm = Array.from(new URLSearchParams(location.search).keys()).some((k) => k.startsWith("utm_"));
    if (!hasUtm) return;
    const timer = setTimeout(() => {
      const query = new URLSearchParams(location.search);
      for (const parameter of Array.from(query.keys())) {
        if (parameter.startsWith("utm_")) query.delete(parameter);
      }
      const rest = query.toString();
      history.replaceState(history.state, "", location.pathname + (rest ? `?${rest}` : "") + location.hash);
    }, 3000);
    return () => clearTimeout(timer);
  }, []);

  useEffect(() => {
    if (previousPathname.current === pathname) return;

    window.ym?.(YANDEX_METRIKA_ID, "hit", location.origin + pathname, {
      referer: location.origin + previousPathname.current,
      title: document.title,
    });
    previousPathname.current = pathname;
  }, [pathname]);

  return <>
    <Analytics beforeSend={(event: BeforeSendEvent) => ({
      ...event,
      url: withoutSensitiveUrlData(event.url),
    })} />
  </>;
}
