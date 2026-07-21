"use client";

import { useState, type FormEvent } from "react";
import { api, ApiError } from "@/lib/api";
import { AuthIntro, Feedback, LocalizedAuthLink } from "@/components/auth-shell";
import { useI18n } from "@/components/i18n-provider";

const copy = {
  en: { title: "Reset your password", subtitle: "Enter your account email to receive a reset link.", email: "Email", send: "Send reset link", back: "Back to login", queued: "If that account exists, a password reset email has been queued.", failed: "Unable to request a reset right now" },
  ru: { title: "Сбросить пароль", subtitle: "Введите адрес аккаунта, чтобы получить ссылку для сброса.", email: "Электронная почта", send: "Отправить ссылку", back: "Вернуться ко входу", queued: "Если аккаунт существует, письмо для сброса пароля поставлено в очередь.", failed: "Сейчас не удалось запросить сброс пароля" },
} as const;

export function ForgotPasswordForm() {
  const { language } = useI18n();
  const t = copy[language];
  const [message, setMessage] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setMessage(null);
    const email = String(new FormData(event.currentTarget).get("email") ?? "").trim();
    try { await api.forgotPassword(email); setMessage(t.queued); setSuccess(true); }
    catch (error) { setMessage(error instanceof ApiError ? error.message : t.failed); setSuccess(false); }
    finally { setBusy(false); }
  }
  return <>
    <AuthIntro title={t.title} subtitle={t.subtitle} />
    <form onSubmit={submit}><div className="field"><label htmlFor="email">{t.email}</label><input id="email" name="email" type="email" autoComplete="email" required /></div>
      <button className="btn btn-primary" disabled={busy}>{busy ? "…" : t.send}</button><Feedback message={message} success={success} /></form>
    <div className="auth-alt"><LocalizedAuthLink href="/login">{t.back}</LocalizedAuthLink></div>
  </>;
}
