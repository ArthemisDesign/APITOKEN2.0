"use client";

import { Suspense, useState, type FormEvent } from "react";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { api, ApiError } from "@/lib/api";
import { AuthShell } from "@/components/auth-shell";
import { Badge, Button, Field, Input, Notice } from "@/components/ui";

function RegisterForm() {
  const params = useSearchParams();
  const inviteCode = params.get("invite") ?? "";

  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await api<{ verificationRequired: boolean }>("/v1/auth/register", {
        method: "POST",
        body: {
          email: email.trim(),
          password,
          ...(displayName.trim() ? { displayName: displayName.trim() } : {}),
          ...(inviteCode ? { inviteCode } : {}),
        },
      });
      setDone(true);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Something went wrong. Try again.");
    } finally {
      setBusy(false);
    }
  }

  if (done) {
    return (
      <>
        <h1>Check your inbox</h1>
        <p className="auth-sub">
          We sent a verification link to <strong>{email}</strong>. Click it to
          activate your partner account, then sign in.
        </p>
        <Link href="/login" className="btn btn-primary" style={{ width: "100%" }}>
          Go to sign in
        </Link>
      </>
    );
  }

  return (
    <>
      <h1>Become a partner</h1>
      <p className="auth-sub">Create your APIToken Partners account.</p>
      {inviteCode ? (
        <p style={{ marginBottom: 16 }}>
          <Badge tone="green">Invited by a partner</Badge>
        </p>
      ) : null}
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
        <Field label="Password">
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
        <Field label="Display name (optional)">
          <Input
            type="text"
            autoComplete="name"
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
            placeholder="How we should address you"
          />
        </Field>
        {inviteCode ? <input type="hidden" name="inviteCode" value={inviteCode} /> : null}
        <Button type="submit" loading={busy} style={{ width: "100%" }}>
          Create account
        </Button>
      </form>
      <p className="auth-alt">
        Already a partner? <Link href="/login">Sign in</Link>
      </p>
    </>
  );
}

export default function RegisterPage() {
  return (
    <AuthShell>
      <Suspense>
        <RegisterForm />
      </Suspense>
    </AuthShell>
  );
}
