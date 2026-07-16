"use client";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useLayoutEffect, useRef, useState, type FormEvent } from "react";
import { api, ApiError } from "@/lib/api";
import { AuthIntro, Feedback } from "@/components/auth-shell";

export function ResetPasswordForm() {
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
    const capturedToken = fragmentToken || initialQueryToken.current;
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
    setMessage(capturedToken ? null : "This reset link is missing its token.");
  }, []);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setMessage(null); setBusy(true);
    const password = String(new FormData(event.currentTarget).get("password") ?? "");
    if (password.length < 12) { setMessage("Password must be at least 12 characters"); setBusy(false); return; }
    try { await api.resetPassword(token, password); router.replace("/login?reset=1"); }
    catch (error) { setMessage(error instanceof ApiError ? error.message : "Unable to reset the password"); setBusy(false); }
  }
  return <>
    <meta name="referrer" content="no-referrer" />
    <AuthIntro title="Choose a new password" subtitle="Use at least 12 characters." />
    <form onSubmit={submit}><div className="field"><label htmlFor="password">New password</label><input id="password" name="password" type="password" autoComplete="new-password" minLength={12} maxLength={128} required /></div>
      <button className="btn btn-primary" disabled={busy || !token}>{busy ? "…" : "Update password"}</button><Feedback message={message} /></form>
    <div className="auth-alt"><Link href="/login">Back to login</Link></div>
  </>;
}
