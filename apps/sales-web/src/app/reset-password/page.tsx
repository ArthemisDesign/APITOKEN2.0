"use client";

import { Suspense, useState, type FormEvent } from "react";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { api, ApiError } from "@/lib/api";
import { AuthShell } from "@/components/auth-shell";
import { Button, Field, Input, Loading, Notice } from "@/components/ui";

function ResetForm() {
  const params = useSearchParams();
  const token = params.get("token") ?? "";
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    if (password !== confirm) {
      setError("Passwords do not match.");
      return;
    }
    setBusy(true);
    try {
      await api("/v1/auth/password/reset", {
        method: "POST",
        body: { token, password },
      });
      setDone(true);
    } catch (err) {
      setError(
        err instanceof ApiError ? err.message : "Reset failed. The link may have expired.",
      );
    } finally {
      setBusy(false);
    }
  }

  if (!token) {
    return (
      <>
        <h1>Invalid link</h1>
        <p className="auth-sub">
          This page needs a reset token. Open the link from the reset email, or
          request a new one.
        </p>
        <Link href="/forgot-password" className="btn btn-ghost" style={{ width: "100%" }}>
          Request a new link
        </Link>
      </>
    );
  }

  if (done) {
    return (
      <>
        <h1>Password updated</h1>
        <p className="auth-sub">You can sign in with your new password now.</p>
        <Link href="/login" className="btn btn-primary" style={{ width: "100%" }}>
          Sign in
        </Link>
      </>
    );
  }

  return (
    <>
      <h1>Choose a new password</h1>
      <p className="auth-sub">Set a new password for your partner account.</p>
      {error ? <Notice kind="error">{error}</Notice> : null}
      <form onSubmit={onSubmit}>
        <Field label="New password">
          <Input
            type="password"
            required
            minLength={8}
            autoComplete="new-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="At least 8 characters"
          />
        </Field>
        <Field label="Confirm new password">
          <Input
            type="password"
            required
            autoComplete="new-password"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            placeholder="Repeat the password"
          />
        </Field>
        <Button type="submit" loading={busy} style={{ width: "100%" }}>
          Update password
        </Button>
      </form>
    </>
  );
}

export default function ResetPasswordPage() {
  return (
    <AuthShell>
      <Suspense fallback={<Loading />}>
        <ResetForm />
      </Suspense>
    </AuthShell>
  );
}
