"use client";

import { useRouter, useSearchParams } from "next/navigation";
import { useState, type FormEvent } from "react";
import { api, ApiError } from "@/lib/api";
import { AuthIntro, Feedback, LocalizedAuthLink, WelcomeBonusNotice } from "@/components/auth-shell";
import { SocialAuth } from "@/components/social-auth";
import { useI18n } from "@/components/i18n-provider";
import { trackProductEvent, trackSuccessfulLogin } from "@/lib/product-analytics";

export function LoginForm() {
  const { language, t } = useI18n();
  const router = useRouter();
  const search = useSearchParams();
  const [message, setMessage] = useState<string | null>(search.get("verified") ? (language === "ru" ? "Адрес подтверждён. Теперь можно войти." : "Email verified. You can now log in.") : null);
  const [success, setSuccess] = useState(Boolean(search.get("verified")));
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setMessage(null); setSuccess(false);
    trackProductEvent("Login Submitted", { method: "password" });
    const data = new FormData(event.currentTarget);
    try {
      await api.login({ email: String(data.get("email") ?? "").trim(), password: String(data.get("password") ?? "") });
      trackSuccessfulLogin("password");
      router.replace("/dashboard");
    } catch (error) {
      trackProductEvent("Login Failed", { method: "password", status: error instanceof ApiError ? error.status : 0 });
      setMessage(error instanceof ApiError ? error.message : language === "ru" ? "Сейчас не удалось войти" : "Unable to log in right now");
      setBusy(false);
    }
  }

  return <>
    <AuthIntro title={t("login_h")} subtitle={t("login_sub")} />
    <WelcomeBonusNotice />
    <form onSubmit={submit} noValidate>
      <div className="field"><label htmlFor="email">{t("f_email")}</label><input id="email" name="email" type="email" autoComplete="email" placeholder="you@company.com" required /></div>
      <div className="field"><label htmlFor="password">{t("f_password")}</label><input id="password" name="password" type="password" autoComplete="current-password" placeholder="••••••••" minLength={8} required /></div>
      <div className="auth-helper"><LocalizedAuthLink href="/forgot-password">{language === "ru" ? "Забыли пароль?" : "Forgot password?"}</LocalizedAuthLink></div>
      <button className="btn btn-primary" type="submit" disabled={busy}>{busy ? "…" : t("login_btn")}</button>
      <Feedback message={message} success={success} />
    </form>
    <SocialAuth />
    <div className="auth-alt"><span>{t("no_acc")}</span> <LocalizedAuthLink href="/register">{t("to_reg")}</LocalizedAuthLink></div>
  </>;
}
