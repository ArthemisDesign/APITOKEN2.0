"use client";

import { useEffect } from "react";
import { useSearchParams } from "next/navigation";
import { ERROR_CODES } from "@/lib/api-errors";
import { trackProductEvent } from "@/lib/product-analytics";

/**
 * Handles arrivals from the /e/<code> short links.
 *
 * The short link is a wildcard redirect, so it can carry the code as a query
 * parameter but not as a per-code fragment. This scrolls to the matching entry and
 * records which error brought the visitor in — that tells us what people actually
 * trip over, which is a product signal as much as a marketing one.
 *
 * Only slugs from the catalog are honoured, so nothing user-controlled reaches the
 * DOM lookup or the analytics payload.
 */
export function ErrorAnchorBeacon() {
  const searchParams = useSearchParams();
  const code = searchParams.get("e");

  useEffect(() => {
    if (!code || !ERROR_CODES.includes(code)) return;

    trackProductEvent("Error Reference Opened", { code });

    const target = document.getElementById(`e-${code}`);
    if (target) target.scrollIntoView({ behavior: "smooth", block: "start" });
  }, [code]);

  return null;
}
