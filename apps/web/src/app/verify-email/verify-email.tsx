"use client";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { api, ApiError } from "@/lib/api";
import { AuthIntro, Feedback } from "@/components/auth-shell";

export function VerifyEmail() {
  const search = useSearchParams();
  const router = useRouter();
  const token = search.get("token");
  const email = search.get("email");
  const started = useRef(false);
  const [message, setMessage] = useState(token ? "Verifying your email…" : "Check your inbox and open the verification link.");
  const [success, setSuccess] = useState(!token);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!token || started.current) return;
    started.current = true;
    api.verifyEmail(token).then(() => {
      setMessage("Email verified. Opening your dashboard…"); setSuccess(true);
      window.setTimeout(() => { router.replace("/dashboard"); }, 500);
    }).catch((error) => { setMessage(error instanceof ApiError ? error.message : "The verification link is invalid or expired"); setSuccess(false); });
  }, [router, token]);

  async function resend() {
    if (!email) return;
    setBusy(true);
    try { await api.resendVerification(email); setMessage("If the account is eligible, a new verification email has been queued."); setSuccess(true); }
    catch (error) { setMessage(error instanceof ApiError ? error.message : "Unable to resend right now"); setSuccess(false); }
    finally { setBusy(false); }
  }

  return <>
    <AuthIntro title="Verify your email" subtitle={email ? `We sent a verification link to ${email}.` : "Complete email verification to activate your account."} />
    <Feedback message={message} success={success} />
    {!token && email && <button className="btn btn-primary" disabled={busy} onClick={resend}>{busy ? "…" : "Resend verification email"}</button>}
    <div className="auth-alt"><Link href="/login">Back to login</Link></div>
  </>;
}
