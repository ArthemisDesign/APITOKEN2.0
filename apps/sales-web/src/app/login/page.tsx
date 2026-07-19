"use client";

import Link from "next/link";
import { AuthShell } from "@/components/auth-shell";
import { TelegramLogin } from "@/components/telegram-login";
import { useI18n } from "@/components/i18n";

export default function LoginPage() {
  const { t } = useI18n();
  return (
    <AuthShell>
      <h1>{t("Partner sign in", "Вход для партнёров")}</h1>
      <p className="auth-sub">
        {t(
          "Invite-only program. If your Telegram was invited or approved, you’ll get straight in; otherwise you can apply for review.",
          "Программа по приглашениям. Если ваш Telegram был приглашён или одобрен, вы войдёте сразу; иначе можно подать заявку на рассмотрение.",
        )}
      </p>
      <TelegramLogin />
      <p className="auth-alt">
        {t("New here?", "Впервые здесь?")} <Link href="/register">{t("Apply to join", "Подать заявку")}</Link>
      </p>
    </AuthShell>
  );
}
