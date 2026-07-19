"use client";

import { usePathname } from "next/navigation";
import { useEffect } from "react";
import { captureReferralCode } from "@/lib/referral";

// Глобальный перехват партнёрского ?ref=CODE: реф-ссылки ведут на главную (и любые
// страницы), код запоминается и уходит при регистрации. window.location вместо
// useSearchParams — чтобы не оборачивать корневой layout в Suspense.
export function RefCapture() {
  const pathname = usePathname();
  useEffect(() => {
    const ref = new URLSearchParams(window.location.search).get("ref");
    captureReferralCode(ref);
  }, [pathname]);
  return null;
}
