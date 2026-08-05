"use client";

import type { BeforeSendEvent } from "@vercel/analytics/next";
import dynamic from "next/dynamic";
import { usePathname } from "next/navigation";
import { useEffect, useRef } from "react";
import { detectAiSource } from "@/lib/ai-source";
import { documentLanguageForPathname } from "@/lib/locale-routes";
import { coarseAcquisition, trackFirstProductEvent } from "@/lib/product-analytics";
import { YANDEX_METRIKA_ID } from "@/lib/yandex-metrika";

// Vercel Analytics и Speed Insights не нужны для первого рендера — грузим их
// отдельным чанком после гидратации, а не в основном бандле каждой страницы.
const Analytics = dynamic(
  () => import("@vercel/analytics/next").then((mod) => mod.Analytics),
  { ssr: false },
);
const SpeedInsights = dynamic(
  () => import("@vercel/speed-insights/next").then((mod) => mod.SpeedInsights),
  { ssr: false },
);

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

  // Keep the document language correct during client-side locale navigation too.
  // The first-paint value comes from the inline script in the root layout.
  useEffect(() => {
    const lang = documentLanguageForPathname(pathname);
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

export function SiteSpeedInsights() {
  return <SpeedInsights />;
}
