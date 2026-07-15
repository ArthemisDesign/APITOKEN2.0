"use client";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useState, type FormEvent } from "react";
import { api, ApiError } from "@/lib/api";
import { AuthIntro, Feedback } from "@/components/auth-shell";
import { SocialAuth } from "@/components/social-auth";
import { useI18n } from "@/components/i18n-provider";

export function LoginForm() {
  const { t } = useI18n();
  const router = useRouter();
  const search = useSearchParams();
  const [message, setMessage] = useState<string | null>(search.get("verified") ? "Email verified. You can now log in." : null);
  const [success, setSuccess] = useState(Boolean(search.get("verified")));
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setMessage(null); setSuccess(false);
    const data = new FormData(event.currentTarget);
    try {
      await api.login({ email: String(data.get("email") ?? "").trim(), password: String(data.get("password") ?? "") });
      router.replace("/dashboard");
    } catch (error) {
      setMessage(error instanceof ApiError ? error.message : "Unable to log in right now");
      setBusy(false);
    }
  }

  return <>
    <AuthIntro title={t("login_h")} subtitle={t("login_sub")} />
    <form onSubmit={submit} noValidate>
      <div className="field"><label htmlFor="email">{t("f_email")}</label><input id="email" name="email" type="email" autoComplete="email" placeholder="you@company.com" required /></div>
      <div className="field"><label htmlFor="password">{t("f_password")}</label><input id="password" name="password" type="password" autoComplete="current-password" placeholder="••••••••••••" minLength={12} required /></div>
      <div className="auth-helper"><Link href="/forgot-password">Forgot password?</Link></div>
      <button className="btn btn-primary" type="submit" disabled={busy}>{busy ? "…" : t("login_btn")}</button>
      <Feedback message={message} success={success} />
    </form>
    <SocialAuth />
    <div className="auth-alt"><span>{t("no_acc")}</span> <Link href="/register">{t("to_reg")}</Link></div>
  </>;
}
