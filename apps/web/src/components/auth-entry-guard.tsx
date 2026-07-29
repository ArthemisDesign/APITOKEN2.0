"use client";

import { useEffect, useState, type ReactNode } from "react";
import { useRouter } from "next/navigation";
import { api, ApiError } from "@/lib/api";

type AuthEntryPhase = "checking" | "anonymous" | "error";

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
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;

    api.me()
      .then(() => {
        if (!cancelled) router.replace(dashboardHref);
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setPhase(error instanceof ApiError && error.status === 401 ? "anonymous" : "error");
      });

    return () => {
      cancelled = true;
    };
  }, [attempt, dashboardHref, router]);

  if (phase === "anonymous") return children;

  if (phase === "error") {
    return (
      <div className="auth-session-check" role="alert">
        <h1>{language === "ru" ? "Не удалось проверить сессию" : "We couldn’t check your session"}</h1>
        <p className="sub">
          {language === "ru"
            ? "Проверьте подключение и повторите попытку."
            : "Check your connection and try again."}
        </p>
        <button
          className="btn btn-primary"
          type="button"
          onClick={() => {
            setPhase("checking");
            setAttempt((current) => current + 1);
          }}
        >
          {language === "ru" ? "Повторить" : "Try again"}
        </button>
      </div>
    );
  }

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
