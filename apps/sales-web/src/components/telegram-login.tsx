"use client";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { api, ApiError } from "@/lib/api";
import { Button, Loading, Notice, Textarea } from "@/components/ui";

// Официальный Telegram Login Widget. Бот приходит с бэка (/v1/auth/telegram/config),
// подписанный payload виджета отправляется на /v1/auth/telegram.
// Если аккаунта нет и инвайт на @username не найден, бэк отвечает 403 {code:"no_account"} —
// показываем форму заявки (тот же подписанный payload уходит на /v1/auth/apply).

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

type Mode = "widget" | "apply" | "applied" | "pending";

export function TelegramLogin({ inviteCode }: { inviteCode?: string | null }) {
  const router = useRouter();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [state, setState] = useState<"loading" | "ready" | "unavailable">("loading");
  const [mode, setMode] = useState<Mode>("widget");
  const [payload, setPayload] = useState<TelegramUser | null>(null);
  const [note, setNote] = useState("");
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
          return;
        } catch (err) {
          setBusy(false);
          const code =
            err instanceof ApiError && err.data && typeof err.data === "object" && "code" in err.data
              ? String((err.data as { code: unknown }).code)
              : null;
          if (code === "no_account") {
            setPayload(user);
            setMode("apply");
          } else if (code === "application_pending") {
            setMode("pending");
          } else if (err instanceof ApiError && err.status === 403) {
            setError(
              inviteCode
                ? "This invite was issued for a different Telegram account."
                : "This Telegram account cannot sign in right now.",
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

  async function submitApplication() {
    if (!payload) return;
    setBusy(true);
    setError(null);
    try {
      const res = await api<{ status: "applied" | "already_partner" }>("/v1/auth/apply", {
        method: "POST",
        body: { ...payload, ...(note.trim() ? { note: note.trim() } : {}) },
      });
      if (res.status === "already_partner") {
        router.replace("/dashboard");
        return;
      }
      setMode("applied");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not submit the application. Try again.");
    } finally {
      setBusy(false);
    }
  }

  if (mode === "pending" || mode === "applied") {
    return (
      <div className="tg-login">
        <Notice kind="info">
          {mode === "applied"
            ? "Application submitted. Once it is approved, just sign in with Telegram again — your account will be ready."
            : "Your application is being reviewed. Once it is approved, sign in with Telegram again."}
        </Notice>
      </div>
    );
  }

  if (mode === "apply" && payload) {
    return (
      <div className="tg-login">
        <Notice kind="info">
          No partner account for @{payload.username ?? `id${payload.id}`} yet. The program is
          invite-only — apply for review and we will get back to you.
        </Notice>
        {error ? <Notice kind="error">{error}</Notice> : null}
        <Textarea
          value={note}
          onChange={(e) => setNote(e.target.value)}
          placeholder="Tell us where you would promote apitoken.sale (channels, audience, experience)…"
          style={{ minHeight: 96, marginBottom: 12 }}
        />
        <Button onClick={submitApplication} loading={busy} style={{ width: "100%" }}>
          Apply for review
        </Button>
      </div>
    );
  }

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
