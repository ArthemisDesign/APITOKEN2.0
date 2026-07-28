"use client";

import { useEffect } from "react";
import { ERROR_CODES } from "@/lib/api-errors";
import { trackProductEvent } from "@/lib/product-analytics";

/**
 * Records which error brought a visitor in via an /e/<code> short link.
 *
 * The code arrives as the fragment (#e-<code>), so the browser does the scrolling
 * natively and no query parameter is created — a parameter would be a second
 * crawlable URL for the same page, which is duplicate surface we do not need.
 * Yandex Metrika strips the hash from the URL it reports, but reading
 * location.hash here is unaffected, so attribution still works.
 *
 * The legacy ?e=<code> form is still honoured for links shared before the change.
 * Only known slugs are accepted, so nothing user-controlled reaches the payload.
 */
export function ErrorAnchorBeacon() {
  useEffect(() => {
    const fromHash = window.location.hash.replace(/^#e-/, "");
    const fromQuery = new URLSearchParams(window.location.search).get("e") ?? "";
    const code = ERROR_CODES.includes(fromHash)
      ? fromHash
      : ERROR_CODES.includes(fromQuery)
        ? fromQuery
        : null;
    if (!code) return;

    trackProductEvent("Error Reference Opened", { code });

    // Only the legacy query form needs help scrolling; a fragment does it itself.
    if (code === fromQuery && code !== fromHash) {
      document.getElementById(`e-${code}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
    }
  }, []);

  return null;
}
