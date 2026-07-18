"use client";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useEffect, useState, type FormEvent } from "react";
import { api, ApiError } from "@/lib/api";
import { captureReferralCode, storedReferralCode } from "@/lib/referral";
import { AuthIntro, Feedback, WelcomeBonusNotice } from "@/components/auth-shell";
import { SocialAuth } from "@/components/social-auth";
import { useI18n } from "@/components/i18n-provider";

export function RegisterForm() {
  const { t } = useI18n();
  const router = useRouter();
  const search = useSearchParams();
  const inviteToken = search.get("invite") ?? undefined;
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => captureReferralCode(search.get("ref")), [search]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setMessage(null);
    const data = new FormData(event.currentTarget);
    const email = String(data.get("email") ?? "").trim();
    const password = String(data.get("password") ?? "");
    if (password.length < 12) { setMessage("Password must be at least 12 characters"); setBusy(false); return; }
    try {
      const result = await api.register({ email, password, inviteToken, referralCode: storedReferralCode() });
      if (result.verificationRequired) router.replace(`/verify-email?email=${encodeURIComponent(email)}`);
      else router.replace("/dashboard");
    } catch (error) {
      setMessage(error instanceof ApiError ? error.message : "Unable to create the account right now");
      setBusy(false);
    }
  }

  return <>
    <AuthIntro title={t("reg_h")} subtitle={t("reg_sub")} />
    <WelcomeBonusNotice />
    <form onSubmit={submit} noValidate>
      <div className="field"><label htmlFor="email">{t("f_email")}</label><input id="email" name="email" type="email" autoComplete="email" placeholder="you@company.com" required /></div>
      <div className="field"><label htmlFor="password">{t("f_password")}</label><input id="password" name="password" type="password" autoComplete="new-password" placeholder="minimum 12 characters" minLength={12} maxLength={128} required /></div>
      <button className="btn btn-primary" type="submit" disabled={busy}>{busy ? "…" : t("reg_btn")}</button>
      <Feedback message={message} />
    </form>
    <SocialAuth inviteToken={inviteToken} />
    <div className="auth-alt"><span>{t("have_acc")}</span> <Link href="/login">{t("to_login")}</Link></div>
  </>;
}
