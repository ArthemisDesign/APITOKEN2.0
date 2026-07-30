"use client";

import { useEffect, useState, type ReactNode } from "react";
import { useRouter } from "next/navigation";
import { api } from "@/lib/api";

const SESSION_CHECK_TIMEOUT_MS = 5_000;

type AuthEntryPhase = "checking" | "anonymous";

export function AuthEntryGuard({
  children,
  dashboardHref,
  language,
}: {
  children: ReactNode;
  dashboardHref: string;
  language: "en" | "ru";
}) {
  const router = useRouter();
  const [phase, setPhase] = useState<AuthEntryPhase>("checking");

  useEffect(() => {
    let cancelled = false;
    let finished = false;
    const controller = new AbortController();
    const timeout = window.setTimeout(() => {
      finished = true;
      controller.abort();
      if (!cancelled) setPhase("anonymous");
    }, SESSION_CHECK_TIMEOUT_MS);

    api.me(controller.signal)
      .then(() => {
        if (cancelled || finished) return;
        finished = true;
        router.replace(dashboardHref);
      })
      .catch(() => {
        if (cancelled || finished) return;
        finished = true;
        setPhase("anonymous");
      })
      .finally(() => {
        window.clearTimeout(timeout);
      });

    return () => {
      cancelled = true;
      window.clearTimeout(timeout);
      controller.abort();
    };
  }, [dashboardHref, router]);

  if (phase === "anonymous") return children;

  return (
    <div className="auth-session-check auth-entry-skeleton" role="status" aria-live="polite">
      <p className="sr-only">{language === "ru" ? "Проверяем сессию…" : "Checking your session…"}</p>
      <span className="auth-skeleton-block auth-skeleton-title" aria-hidden="true" />
      <span className="auth-skeleton-block auth-skeleton-subtitle" aria-hidden="true" />
      <span className="auth-skeleton-block auth-skeleton-bonus" aria-hidden="true" />
      <span className="auth-skeleton-block auth-skeleton-field" aria-hidden="true" />
      <span className="auth-skeleton-block auth-skeleton-field" aria-hidden="true" />
      <span className="auth-skeleton-block auth-skeleton-button" aria-hidden="true" />
      <span className="auth-skeleton-block auth-skeleton-social" aria-hidden="true" />
    </div>
  );
}
