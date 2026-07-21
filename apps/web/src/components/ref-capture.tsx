"use client";

import { usePathname } from "next/navigation";
import { useEffect } from "react";
import { captureReferralCode } from "@/lib/referral";
import { trackFirstProductEvent } from "@/lib/product-analytics";

// Глобальный перехват партнёрского ?ref=CODE: реф-ссылки ведут на главную (и любые
// страницы), код запоминается и уходит при регистрации. window.location вместо
// useSearchParams — чтобы не оборачивать корневой layout в Suspense.
export function RefCapture() {
  const pathname = usePathname();
  useEffect(() => {
    const ref = new URLSearchParams(window.location.search).get("ref");
    captureReferralCode(ref);
    if (ref && /^[A-Za-z0-9_-]{3,32}$/.test(ref)) {
      trackFirstProductEvent("referral", "Referral Captured", { landing_path: window.location.pathname });
    }
  }, [pathname]);
  return null;
}
