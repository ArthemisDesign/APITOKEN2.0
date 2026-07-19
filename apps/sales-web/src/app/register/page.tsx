"use client";

import { Suspense, useEffect, useState } from "react";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { api } from "@/lib/api";
import { AuthShell } from "@/components/auth-shell";
import { Badge, Loading, Notice } from "@/components/ui";
import { TelegramLogin } from "@/components/telegram-login";
import { useI18n } from "@/components/i18n";

// Онбординг invite-only: инвайт выписан на конкретный telegram-юзернейм,
// человек просто подтверждает вход через Telegram — аккаунт создаётся сам.

function RegisterCard() {
  const { t } = useI18n();
  const params = useSearchParams();
  const inviteCode = params.get("invite") ?? "";
  const [invite, setInvite] = useState<{ telegramUsername: string | null } | null>(null);
  const [state, setState] = useState<"loading" | "ok" | "invalid" | "none">(inviteCode ? "loading" : "none");

  useEffect(() => {
    if (!inviteCode) return;
    let cancelled = false;
    api<{ invite: { telegramUsername: string | null } }>(`/v1/auth/invite/${encodeURIComponent(inviteCode)}`)
      .then((res) => {
        if (cancelled) return;
        setInvite(res.invite);
        setState("ok");
      })
      .catch(() => {
        if (!cancelled) setState("invalid");
      });
    return () => {
      cancelled = true;
    };
  }, [inviteCode]);

  if (state === "none") {
    return (
      <>
        <h1>{t("Join APIToken Partners", "Присоединиться к APIToken Partners")}</h1>
        <p className="auth-sub">
          {t(
            "The program is invite-only. If you were invited or approved, signing in with Telegram is enough — your account opens automatically. New here? Sign in and apply for review.",
            "Программа работает по приглашениям. Если вас пригласили или одобрили, достаточно войти через Telegram — аккаунт откроется автоматически. Впервые здесь? Войдите и подайте заявку на рассмотрение.",
          )}
        </p>
        <TelegramLogin />
        <p className="auth-alt">
          {t("Already a partner?", "Уже партнёр?")} <Link href="/login">{t("Sign in", "Войти")}</Link>
        </p>
      </>
    );
  }

  if (state === "loading") {
    return (
      <>
        <h1>{t("Checking your invite…", "Проверяем ваше приглашение…")}</h1>
        <Loading />
      </>
    );
  }

  if (state === "invalid") {
    return (
      <>
        <h1>{t("Invite not found", "Приглашение не найдено")}</h1>
        <Notice kind="error">
          {t(
            "This invite link is invalid, expired, or already used.",
            "Эта ссылка-приглашение недействительна, истекла или уже использована.",
          )}
        </Notice>
        <p className="auth-alt">
          {t("Already a partner?", "Уже партнёр?")} <Link href="/login">{t("Sign in", "Войти")}</Link>
        </p>
      </>
    );
  }

  return (
    <>
      <h1>{t("You’re invited", "Вас пригласили")}</h1>
      <p className="auth-sub">
        {t(
          "Confirm with Telegram to activate your partner account — that’s it.",
          "Подтвердите вход через Telegram, чтобы активировать партнёрский аккаунт — и всё.",
        )}
      </p>
      {invite?.telegramUsername ? (
        <p style={{ marginBottom: 16 }}>
          <Badge tone="green">{t("Invite for", "Приглашение для")} @{invite.telegramUsername}</Badge>
        </p>
      ) : null}
      <TelegramLogin inviteCode={inviteCode} />
      <p className="auth-alt">
        {t("Already a partner?", "Уже партнёр?")} <Link href="/login">{t("Sign in", "Войти")}</Link>
      </p>
    </>
  );
}

export default function RegisterPage() {
  return (
    <AuthShell>
      <Suspense>
        <RegisterCard />
      </Suspense>
    </AuthShell>
  );
}
