"use client";

import { useState, type FormEvent } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { api, ApiError } from "@/lib/api";
import { AuthShell } from "@/components/auth-shell";
import { Button, Field, Input, Notice } from "@/components/ui";

export default function LoginPage() {
  const router = useRouter();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [needsVerification, setNeedsVerification] = useState(false);
  const [resent, setResent] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setNeedsVerification(false);
    setBusy(true);
    try {
      const res = await api<{ ok?: boolean; verificationRequired?: boolean }>(
        "/v1/auth/login",
        { method: "POST", body: { email: email.trim(), password } },
      );
      if (res.verificationRequired) {
        setNeedsVerification(true);
        return;
      }
      router.replace("/dashboard");
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) {
        setError("Invalid email or password.");
      } else {
        setError(err instanceof ApiError ? err.message : "Something went wrong. Try again.");
      }
    } finally {
      setBusy(false);
    }
  }

  async function resend() {
    try {
      await api("/v1/auth/email/resend", { method: "POST", body: { email: email.trim() } });
      setResent(true);
    } catch {
      setError("Could not resend the verification email.");
    }
  }

  return (
    <AuthShell>
      <h1>Partner sign in</h1>
      <p className="auth-sub">Welcome back to APIToken Partners.</p>
      {error ? <Notice kind="error">{error}</Notice> : null}
      {needsVerification ? (
        <Notice kind="info">
          Your email is not verified yet. Check your inbox
          {resent ? " — a new link was sent." : ""}{" "}
          {!resent ? (
            <button
              type="button"
              onClick={resend}
              style={{
                background: "none",
                border: "none",
                color: "var(--accent-strong)",
                cursor: "pointer",
                fontSize: "inherit",
                padding: 0,
                textDecoration: "underline",
              }}
            >
              Resend email
            </button>
          ) : null}
        </Notice>
      ) : null}
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
        <Field label="Password" hint={<Link href="/forgot-password">Forgot password?</Link>}>
          <Input
            type="password"
            required
            autoComplete="current-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="Your password"
          />
        </Field>
        <Button type="submit" loading={busy} style={{ width: "100%" }}>
          Sign in
        </Button>
      </form>
      <p className="auth-alt">
        New here? <Link href="/register">Become a partner</Link>
      </p>
    </AuthShell>
  );
}
