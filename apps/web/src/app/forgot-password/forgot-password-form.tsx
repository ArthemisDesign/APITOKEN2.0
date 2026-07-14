"use client";

import Link from "next/link";
import { useState, type FormEvent } from "react";
import { api, ApiError } from "@/lib/api";
import { AuthIntro, AuthShell, Feedback } from "@/components/auth-shell";

export function ForgotPasswordForm() {
  const [message, setMessage] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setMessage(null);
    const email = String(new FormData(event.currentTarget).get("email") ?? "").trim();
    try { await api.forgotPassword(email); setMessage("If that account exists, a password reset email has been queued."); setSuccess(true); }
    catch (error) { setMessage(error instanceof ApiError ? error.message : "Unable to request a reset right now"); setSuccess(false); }
    finally { setBusy(false); }
  }
  return <AuthShell>
    <AuthIntro title="Reset your password" subtitle="Enter your account email to receive a reset link." />
    <form onSubmit={submit}><div className="field"><label htmlFor="email">Email</label><input id="email" name="email" type="email" autoComplete="email" required /></div>
      <button className="btn btn-primary" disabled={busy}>{busy ? "…" : "Send reset link"}</button><Feedback message={message} success={success} /></form>
    <div className="auth-alt"><Link href="/login">Back to login</Link></div>
  </AuthShell>;
}
