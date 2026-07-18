"use client";

import { useState, type FormEvent } from "react";
import Link from "next/link";
import { api, ApiError } from "@/lib/api";
import { AuthShell } from "@/components/auth-shell";
import { Button, Field, Input, Notice } from "@/components/ui";

export default function ForgotPasswordPage() {
  const [email, setEmail] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sent, setSent] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await api("/v1/auth/password/forgot", {
        method: "POST",
        body: { email: email.trim() },
      });
      setSent(true);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Something went wrong. Try again.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <AuthShell>
      {sent ? (
        <>
          <h1>Check your inbox</h1>
          <p className="auth-sub">
            If an account exists for <strong>{email}</strong>, a password reset link
            is on its way.
          </p>
          <Link href="/login" className="btn btn-ghost" style={{ width: "100%" }}>
            Back to sign in
          </Link>
        </>
      ) : (
        <>
          <h1>Reset your password</h1>
          <p className="auth-sub">
            Enter your account email and we will send you a reset link.
          </p>
          {error ? <Notice kind="error">{error}</Notice> : null}
          <form onSubmit={onSubmit}>
            <Field label="Email">
              <Input
                type="email"
                required
                autoComplete="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder="you@example.com"
              />
            </Field>
            <Button type="submit" loading={busy} style={{ width: "100%" }}>
              Send reset link
            </Button>
          </form>
          <p className="auth-alt">
            Remembered it? <Link href="/login">Sign in</Link>
          </p>
        </>
      )}
    </AuthShell>
  );
}
