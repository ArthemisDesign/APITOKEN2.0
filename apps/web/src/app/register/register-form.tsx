"use client";

import { useRouter, useSearchParams } from "next/navigation";
import { useEffect, useState, type FormEvent } from "react";
import { api, ApiError } from "@/lib/api";
import { captureReferralCode, storedReferralCode } from "@/lib/referral";
import { AuthIntro, Feedback, LocalizedAuthLink, WelcomeBonusNotice } from "@/components/auth-shell";
import { SocialAuth } from "@/components/social-auth";
import { useI18n } from "@/components/i18n-provider";
import { trackProductEvent } from "@/lib/product-analytics";

export function RegisterForm() {
  const { language, t } = useI18n();
  const router = useRouter();
  const search = useSearchParams();
  const inviteToken = search.get("invite") ?? undefined;
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [emailValue, setEmailValue] = useState("");
  const [invite, setInvite] = useState<{
    state: "loading" | "valid" | "invalid";
    emailBound?: boolean;
    maskedEmail?: string | null;
    email?: string | null;
    discountPercent?: number;
    expiresAt?: string;
  } | null>(inviteToken ? { state: "loading" } : null);
  useEffect(() => {
    captureReferralCode(search.get("ref"));
    if (!inviteToken) return;
    let active = true;
    api.businessInvitePreview(inviteToken)
      .then((value) => {
        if (!active) return;
        if (value.valid && value.emailBound && value.email) setEmailValue(value.email);
        setInvite(value.valid ? { state: "valid", ...value } : { state: "invalid" });
      })
      .catch(() => {
        if (active) setInvite({ state: "invalid" });
      });
    return () => { active = false; };
  }, [inviteToken, search]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setMessage(null);
    const data = new FormData(event.currentTarget);
    const email = String(data.get("email") ?? "").trim();
    const password = String(data.get("password") ?? "");
    if (password.length < 8) { setMessage(language === "ru" ? "Пароль должен содержать не менее 8 символов" : "Password must be at least 8 characters"); setBusy(false); return; }
    trackProductEvent("Sign Up Submitted", { method: "password", invited: Boolean(inviteToken), referred: Boolean(storedReferralCode()) });
    try {
      const result = await api.register({ email, password, inviteToken, referralCode: storedReferralCode() });
      trackProductEvent("Sign Up Succeeded", { method: "password", verification_required: result.verificationRequired, invited: Boolean(inviteToken), referred: Boolean(storedReferralCode()) });
      if (result.verificationRequired) router.replace(`/verify-email?email=${encodeURIComponent(email)}`);
      else router.replace("/dashboard");
    } catch (error) {
      trackProductEvent("Sign Up Failed", { method: "password", status: error instanceof ApiError ? error.status : 0 });
      setMessage(error instanceof ApiError ? error.message : language === "ru" ? "Сейчас не удалось создать аккаунт" : "Unable to create the account right now");
      setBusy(false);
    }
  }

  return <>
    <AuthIntro title={t("reg_h")} subtitle={t("reg_sub")} />
    {invite?.state === "valid" ? (
      <aside className="auth-bonus" aria-label={language === "ru" ? "B2B-приглашение" : "B2B invitation"}>
        <span className="auth-bonus-mark" aria-hidden="true">✦</span>
        <span>
          <strong>{language === "ru" ? `B2B-скидка ${invite.discountPercent}%` : `${invite.discountPercent}% B2B discount`}</strong>
          <small>
            {invite.emailBound
              ? (language === "ru" ? `Используйте адрес ${invite.maskedEmail}. ` : `Use the invited address ${invite.maskedEmail}. `)
              : (language === "ru" ? "Ссылку можно принять с любым рабочим email. " : "This link can be accepted with any work email. ")}
            {language === "ru" ? "Действует до " : "Valid until "}
            {invite.expiresAt ? new Date(invite.expiresAt).toLocaleString(language === "ru" ? "ru-RU" : "en-US") : "—"}.
          </small>
        </span>
      </aside>
    ) : invite?.state === "invalid" ? (
      <Feedback message={language === "ru"
        ? "Эта B2B-ссылка недействительна, отозвана, уже использована или истекла."
        : "This B2B invitation is invalid, revoked, already used, or expired."} />
    ) : invite?.state === "loading" ? (
      <div className="auth-msg" role="status">{language === "ru" ? "Проверяем B2B-приглашение…" : "Checking B2B invitation…"}</div>
    ) : <WelcomeBonusNotice />}
    <form onSubmit={submit} noValidate>
      <div className="field"><label htmlFor="email">{t("f_email")}</label><input id="email" name="email" type="email" autoComplete="email" placeholder={invite?.maskedEmail ?? "you@company.com"} value={emailValue} onChange={(event) => setEmailValue(event.target.value)} readOnly={invite?.state === "valid" && invite.emailBound} required /></div>
      <div className="field"><label htmlFor="password">{t("f_password")}</label><input id="password" name="password" type="password" autoComplete="new-password" placeholder="minimum 8 characters" minLength={8} maxLength={128} required /></div>
      <button className="btn btn-primary" type="submit" disabled={busy || invite?.state === "loading" || invite?.state === "invalid"}>{busy ? "…" : t("reg_btn")}</button>
      <Feedback message={message} />
    </form>
    {invite?.state !== "invalid" && invite?.state !== "loading" && <SocialAuth inviteToken={inviteToken} />}
    <div className="auth-alt"><span>{t("have_acc")}</span> <LocalizedAuthLink href="/login">{t("to_login")}</LocalizedAuthLink></div>
  </>;
}
