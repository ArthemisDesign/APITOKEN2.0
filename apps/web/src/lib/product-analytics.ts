"use client";

import { track } from "@vercel/analytics";

export type ProductEventProperties = Record<string, string | number | boolean | null>;

const FIRST_EVENT_PREFIX = "apitoken.analytics.first.";

/**
 * Send a deliberately anonymous product event to Vercel Analytics.
 *
 * Never pass account ids, email addresses, API keys, checkout ids, promo/referral
 * codes, error messages, or URLs with query strings to this function.
 */
export function trackProductEvent(name: string, properties?: ProductEventProperties): void {
  try {
    track(name, properties);
  } catch {
    // Analytics must never interrupt a client workflow (or SSR/test rendering).
  }
}

/** Tracks a browser milestone once. This is intentionally not an account identity. */
export function trackFirstProductEvent(
  milestone: string,
  name: string,
  properties?: ProductEventProperties,
): boolean {
  try {
    const key = `${FIRST_EVENT_PREFIX}${milestone}`;
    if (window.localStorage.getItem(key)) return false;
    window.localStorage.setItem(key, "1");
    trackProductEvent(name, properties);
    return true;
  } catch {
    // Storage can be disabled. Keep the funnel observable for this page without
    // adding cookies or fingerprinting; the event may then repeat on another visit.
    trackProductEvent(name, properties);
    return true;
  }
}

export function trackSuccessfulLogin(method: "password" | "google" | "github"): void {
  trackProductEvent("Login Succeeded", { method });
  trackFirstProductEvent("login", "First Login", { method });
}

export function coarseAcquisition(): ProductEventProperties {
  const query = new URLSearchParams(window.location.search);
  const utmSource = safeCampaignValue(query.get("utm_source"));
  const utmMedium = safeCampaignValue(query.get("utm_medium"));
  let referrer = "direct";

  try {
    if (document.referrer) {
      const host = new URL(document.referrer).hostname.toLowerCase();
      if (host !== window.location.hostname.toLowerCase()) {
        referrer = host.endsWith("google.com") || host.endsWith("google.ru") ? "google"
          : host.endsWith("yandex.ru") || host.endsWith("yandex.com") ? "yandex"
          : host.endsWith("bing.com") ? "bing"
          : host.includes("chatgpt.com") || host.includes("openai.com") ? "chatgpt"
          : host.includes("claude.ai") || host.includes("anthropic.com") ? "claude"
          : host.includes("perplexity.ai") ? "perplexity"
          : "other_referral";
      }
    }
  } catch {
    referrer = "unknown";
  }

  return {
    source: utmSource || referrer,
    medium: utmMedium || (referrer === "direct" ? "none" : "referral"),
    landing_path: window.location.pathname,
    language: window.location.pathname.startsWith("/ru") ? "ru"
      : window.location.pathname.startsWith("/zh") ? "zh-CN"
      : window.location.pathname.startsWith("/ko") ? "ko"
      : "en",
    has_referral: query.has("ref"),
  };
}

function safeCampaignValue(value: string | null): string {
  if (!value) return "";
  const normalized = value.trim().toLowerCase();
  return /^[a-z0-9_-]{1,40}$/.test(normalized) ? normalized : "other";
}

export function checkoutAmountBucket(amountUsd: string): string {
  const amount = BigInt(amountUsd);
  if (amount < 50n) return "under_50";
  if (amount < 100n) return "50_99";
  if (amount < 250n) return "100_249";
  if (amount < 500n) return "250_499";
  if (amount < 1_000n) return "500_999";
  return "1000_plus";
}
