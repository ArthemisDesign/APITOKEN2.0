"use client";

import { useRouter, useSearchParams } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { api, ApiError } from "@/lib/api";
import { AuthIntro, Feedback, LocalizedAuthLink } from "@/components/auth-shell";
import { useI18n } from "@/components/i18n-provider";
import { localeHref } from "@/lib/locale-routes";
import { trackProductEvent } from "@/lib/product-analytics";

const copy = {
  en: {
    title: "Verify your email",
    sentTo: "We sent a verification link to {email}.",
    subtitle: "Complete email verification to activate your account.",
    verifying: "Verifying your email…",
    inbox: "Check your inbox and open the verification link.",
    verified: "Email verified. Opening your dashboard…",
    invalid: "The verification link is invalid or expired",
    resent: "If the account is eligible, a new verification email has been queued.",
    resendFailed: "Unable to resend right now",
    resend: "Resend verification email",
    back: "Back to login",
  },
  ru: {
    title: "Подтвердите адрес электронной почты",
    sentTo: "Мы отправили ссылку для подтверждения на адрес {email}.",
    subtitle: "Подтвердите адрес электронной почты, чтобы активировать аккаунт.",
    verifying: "Подтверждаем адрес электронной почты…",
    inbox: "Проверьте почту и откройте ссылку для подтверждения.",
    verified: "Адрес подтверждён. Открываем личный кабинет…",
    invalid: "Ссылка для подтверждения недействительна или устарела",
    resent: "Если для аккаунта доступна повторная отправка, новое письмо поставлено в очередь.",
    resendFailed: "Сейчас не удалось отправить письмо повторно",
    resend: "Отправить письмо повторно",
    back: "Вернуться ко входу",
  },
} as const;

type MessageKey = "verifying" | "inbox" | "verified" | "invalid" | "resent" | "resendFailed";

function captureAndScrubVerificationToken(): string | null {
  const url = new URL(window.location.href);
  const hashParams = new URLSearchParams(url.hash.slice(1));
  const hashToken = hashParams.get("token");
  const queryToken = url.searchParams.get("token");

  if (hashToken === null && queryToken === null) return null;

  url.searchParams.delete("token");
  hashParams.delete("token");
  url.hash = hashParams.toString();
  window.history.replaceState(window.history.state, "", `${url.pathname}${url.search}${url.hash}`);

  return hashToken || queryToken || null;
}

export function VerifyEmail() {
  const { language } = useI18n();
  const t = copy[language];
  const search = useSearchParams();
  const router = useRouter();
  const email = search.get("email");
  const hasQueryToken = search.has("token");
  const processing = useRef(false);
  const [messageKey, setMessageKey] = useState<MessageKey | null>(hasQueryToken ? "verifying" : null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [hasToken, setHasToken] = useState(hasQueryToken);
  const [ready, setReady] = useState(hasQueryToken);
  const [busy, setBusy] = useState(false);

  /* eslint-disable react-hooks/set-state-in-effect */
  useEffect(() => {
    if (processing.current) return;
    const token = captureAndScrubVerificationToken();
    setReady(true);

    if (!token) {
      setHasToken(false);
      setMessageKey("inbox");
      setSuccess(true);
      return;
    }

    processing.current = true;
    setHasToken(true);
    setErrorMessage(null);
    setMessageKey("verifying");
    setSuccess(false);
    api.verifyEmail(token).then(() => {
      trackProductEvent("Email Verified");
      setMessageKey("verified");
      setSuccess(true);
      window.setTimeout(() => { router.replace(localeHref("/dashboard", language)); }, 500);
    }).catch((error) => {
      if (error instanceof ApiError) {
        setErrorMessage(error.message);
        setMessageKey(null);
      } else {
        setMessageKey("invalid");
      }
      setSuccess(false);
    });
  }, [language, router, search]);
  /* eslint-enable react-hooks/set-state-in-effect */

  async function resend() {
    if (!email) return;
    setBusy(true);
    setErrorMessage(null);
    try {
      await api.resendVerification(email);
      setMessageKey("resent");
      setSuccess(true);
    } catch (error) {
      if (error instanceof ApiError) {
        setErrorMessage(error.message);
        setMessageKey(null);
      } else {
        setMessageKey("resendFailed");
      }
      setSuccess(false);
    } finally {
      setBusy(false);
    }
  }

  const message = errorMessage ?? (messageKey ? t[messageKey] : null);

  return <>
    <meta name="referrer" content="no-referrer" />
    <AuthIntro title={t.title} subtitle={email ? t.sentTo.replace("{email}", email) : t.subtitle} />
    <Feedback message={message} success={success} />
    {ready && !hasToken && email && <button className="btn btn-primary" disabled={busy} onClick={resend}>{busy ? "…" : t.resend}</button>}
    <div className="auth-alt"><LocalizedAuthLink href="/login">{t.back}</LocalizedAuthLink></div>
  </>;
}
