"use client";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useState, type FormEvent } from "react";
import { api, ApiError } from "@/lib/api";
import { AuthIntro, Feedback } from "@/components/auth-shell";

export function ResetPasswordForm() {
  const token = useSearchParams().get("token") ?? "";
  const router = useRouter();
  const [message, setMessage] = useState<string | null>(token ? null : "This reset link is missing its token.");
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setMessage(null); setBusy(true);
    const password = String(new FormData(event.currentTarget).get("password") ?? "");
    if (password.length < 12) { setMessage("Password must be at least 12 characters"); setBusy(false); return; }
    try { await api.resetPassword(token, password); router.replace("/login?reset=1"); }
    catch (error) { setMessage(error instanceof ApiError ? error.message : "Unable to reset the password"); setBusy(false); }
  }
  return <>
    <AuthIntro title="Choose a new password" subtitle="Use at least 12 characters." />
    <form onSubmit={submit}><div className="field"><label htmlFor="password">New password</label><input id="password" name="password" type="password" autoComplete="new-password" minLength={12} maxLength={128} required /></div>
      <button className="btn btn-primary" disabled={busy || !token}>{busy ? "…" : "Update password"}</button><Feedback message={message} /></form>
    <div className="auth-alt"><Link href="/login">Back to login</Link></div>
  </>;
}
