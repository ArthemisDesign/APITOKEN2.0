"use client";

import Link from "next/link";
import { useEffect, useState, type ReactNode } from "react";
import { useRouter } from "next/navigation";
import { Brand, Loading } from "@/components/ui";
import { LanguageToggle, useI18n } from "@/components/i18n";
import { ThemeToggle } from "@/components/theme-toggle";
import { api, ApiError } from "@/lib/api";

type AuthEntryPhase = "checking" | "anonymous" | "error";

export function AuthShell({ children }: { children: ReactNode }) {
  const router = useRouter();
  const { t } = useI18n();
  const [phase, setPhase] = useState<AuthEntryPhase>("checking");
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;

    api("/v1/auth/me")
      .then(() => {
        if (!cancelled) router.replace("/dashboard");
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setPhase(error instanceof ApiError && error.status === 401 ? "anonymous" : "error");
      });

    return () => {
      cancelled = true;
    };
  }, [attempt, router]);

  return (
    <main className="auth-shell">
      <div className="auth-header">
        <Link href="/" className="brand" aria-label={t("APIToken Partners home", "Главная APIToken Partners")}>
          <Brand />
        </Link>
        <div className="gate-tools">
          <LanguageToggle />
          <ThemeToggle />
        </div>
      </div>
      <div className="auth-card">
        {phase === "anonymous" ? children : phase === "error" ? (
          <div role="alert">
            <h1>{t("We couldn’t check your session", "Не удалось проверить сессию")}</h1>
            <p className="auth-sub">
              {t("Check your connection and try again.", "Проверьте подключение и повторите попытку.")}
            </p>
            <button
              className="btn btn-primary"
              type="button"
              onClick={() => {
                setPhase("checking");
                setAttempt((current) => current + 1);
              }}
            >
              {t("Try again", "Повторить")}
            </button>
          </div>
        ) : (
          <div role="status" aria-live="polite">
            <Loading label={t("Checking your session…", "Проверяем сессию…")} />
          </div>
        )}
      </div>
    </main>
  );
}
