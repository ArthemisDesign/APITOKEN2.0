"use client";

import { useRouter, useSearchParams } from "next/navigation";
import { useEffect, useLayoutEffect, useRef, useState, type FormEvent } from "react";
import { api, ApiError } from "@/lib/api";
import { AuthIntro, Feedback, LocalizedAuthLink } from "@/components/auth-shell";
import { useI18n } from "@/components/i18n-provider";
import { localeHref } from "@/lib/locale-routes";
import { forgetAuthToken, rememberAuthToken, takeRememberedAuthToken } from "@/lib/auth-token-memory";

const copy = {
  en: { title: "Choose a new password", subtitle: "Use at least 8 characters.", label: "New password", update: "Update password", missing: "This reset link is missing its token.", short: "Password must be at least 8 characters", failed: "Unable to reset the password", back: "Back to login" },
  ru: { title: "Задайте новый пароль", subtitle: "Используйте не менее 8 символов.", label: "Новый пароль", update: "Обновить пароль", missing: "В ссылке для сброса отсутствует токен.", short: "Пароль должен содержать не менее 8 символов", failed: "Не удалось сбросить пароль", back: "Вернуться ко входу" },
} as const;

export function ResetPasswordForm() {
  const { language } = useI18n();
  const t = copy[language];
  const searchParams = useSearchParams();
  const initialQueryToken = useRef(searchParams.get("token") ?? "");
  const router = useRouter();
  const [token, setToken] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useLayoutEffect(() => {
    const url = new URL(window.location.href);
    const fragmentParams = new URLSearchParams(url.hash.slice(1));
    const fragmentToken = fragmentParams.get("token") ?? "";
    const capturedToken = fragmentToken || initialQueryToken.current || takeRememberedAuthToken("reset-password");
    const hadFragmentToken = fragmentParams.has("token");
    const hadQueryToken = url.searchParams.has("token");

    if (hadFragmentToken) {
      fragmentParams.delete("token");
      url.hash = fragmentParams.size ? `#${fragmentParams.toString()}` : "";
    }
    if (hadQueryToken) url.searchParams.delete("token");
    if (hadFragmentToken || hadQueryToken) {
      window.history.replaceState(window.history.state, "", `${url.pathname}${url.search}${url.hash}`);
    }

    setToken(capturedToken);
    setMessage(capturedToken ? null : t.missing);
  }, [t.missing]);
  useEffect(() => {
    if (!token) return;
    const carryTokenToLocalizedRoute = () => rememberAuthToken("reset-password", token);
    window.addEventListener("apitoken:locale-change", carryTokenToLocalizedRoute);
    return () => window.removeEventListener("apitoken:locale-change", carryTokenToLocalizedRoute);
  }, [token]);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setMessage(null); setBusy(true);
    const password = String(new FormData(event.currentTarget).get("password") ?? "");
    if (password.length < 8) { setMessage(t.short); setBusy(false); return; }
    try { await api.resetPassword(token, password); forgetAuthToken("reset-password"); router.replace(localeHref("/login?reset=1", language)); }
    catch (error) { setMessage(error instanceof ApiError ? error.message : t.failed); setBusy(false); }
  }
  return <>
    <meta name="referrer" content="no-referrer" />
    <AuthIntro title={t.title} subtitle={t.subtitle} />
    <form onSubmit={submit}><div className="field"><label htmlFor="password">{t.label}</label><input id="password" name="password" type="password" autoComplete="new-password" minLength={8} maxLength={128} required /></div>
      <button className="btn btn-primary" disabled={busy || !token}>{busy ? "…" : t.update}</button><Feedback message={message} /></form>
    <div className="auth-alt"><LocalizedAuthLink href="/login">{t.back}</LocalizedAuthLink></div>
  </>;
}
