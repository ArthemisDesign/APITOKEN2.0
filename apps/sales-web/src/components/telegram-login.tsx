"use client";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { api, ApiError } from "@/lib/api";
import { Loading, Notice } from "@/components/ui";

// Официальный Telegram Login Widget. Бот приходит с бэка (/v1/auth/telegram/config),
// подписанный payload виджета отправляется на /v1/auth/telegram; для первой
// регистрации добавляется inviteCode.

interface TelegramUser {
  id: number;
  first_name?: string;
  last_name?: string;
  username?: string;
  photo_url?: string;
  auth_date: number;
  hash: string;
}

declare global {
  interface Window {
    __tgAuth?: (user: TelegramUser) => void;
  }
}

export function TelegramLogin({ inviteCode }: { inviteCode?: string | null }) {
  const router = useRouter();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [state, setState] = useState<"loading" | "ready" | "unavailable">("loading");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      let botUsername: string;
      try {
        const config = await api<{ botUsername: string }>("/v1/auth/telegram/config");
        botUsername = config.botUsername;
      } catch {
        if (!cancelled) setState("unavailable");
        return;
      }
      if (cancelled || !containerRef.current) return;

      window.__tgAuth = async (user: TelegramUser) => {
        setError(null);
        setBusy(true);
        try {
          await api("/v1/auth/telegram", {
            method: "POST",
            body: { ...user, ...(inviteCode ? { inviteCode } : {}) },
          });
          router.replace("/dashboard");
        } catch (err) {
          setBusy(false);
          if (err instanceof ApiError && err.status === 403) {
            setError(
              inviteCode
                ? "This invite was issued for a different Telegram account."
                : "No partner account for this Telegram. Open your personal invite link to join.",
            );
          } else if (err instanceof ApiError) {
            setError(err.message);
          } else {
            setError("Something went wrong. Try again.");
          }
        }
      };

      const script = document.createElement("script");
      script.src = "https://telegram.org/js/telegram-widget.js?22";
      script.async = true;
      script.setAttribute("data-telegram-login", botUsername);
      script.setAttribute("data-size", "large");
      script.setAttribute("data-radius", "4");
      script.setAttribute("data-onauth", "__tgAuth(user)");
      containerRef.current.replaceChildren(script);
      setState("ready");
    })();
    return () => {
      cancelled = true;
      delete window.__tgAuth;
    };
  }, [inviteCode, router]);

  return (
    <div className="tg-login">
      {error ? <Notice kind="error">{error}</Notice> : null}
      {state === "loading" ? <Loading /> : null}
      {state === "unavailable" ? (
        <Notice kind="info">Telegram sign-in is not available right now. Try again later.</Notice>
      ) : null}
      <div ref={containerRef} className="tg-login-widget" aria-busy={busy} />
      {busy ? <Loading /> : null}
    </div>
  );
}
