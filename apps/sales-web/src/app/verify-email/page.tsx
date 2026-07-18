"use client";

import { Suspense, useEffect, useRef, useState } from "react";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { api, ApiError } from "@/lib/api";
import { AuthShell } from "@/components/auth-shell";
import { Loading, Notice } from "@/components/ui";

function VerifyInner() {
  const params = useSearchParams();
  const token = params.get("token") ?? "";
  const [state, setState] = useState<"idle" | "busy" | "ok" | "error">(
    token ? "busy" : "idle",
  );
  const [message, setMessage] = useState<string | null>(null);
  const started = useRef(false);

  useEffect(() => {
    if (!token || started.current) return;
    started.current = true;
    (async () => {
      try {
        await api("/v1/auth/email/verify", { method: "POST", body: { token } });
        setState("ok");
      } catch (err) {
        setMessage(
          err instanceof ApiError ? err.message : "Verification failed. The link may have expired.",
        );
        setState("error");
      }
    })();
  }, [token]);

  if (state === "idle") {
    return (
      <>
        <h1>Verify your email</h1>
        <p className="auth-sub">
          This page needs a verification token. Open the link from the email we sent
          you — it contains the token automatically.
        </p>
        <Link href="/login" className="btn btn-ghost" style={{ width: "100%" }}>
          Back to sign in
        </Link>
      </>
    );
  }

  if (state === "busy") {
    return (
      <>
        <h1>Verifying…</h1>
        <Loading label="Confirming your email" />
      </>
    );
  }

  if (state === "ok") {
    return (
      <>
        <h1>Email verified</h1>
        <p className="auth-sub">Your partner account is active. You can sign in now.</p>
        <Link href="/login" className="btn btn-primary" style={{ width: "100%" }}>
          Sign in
        </Link>
      </>
    );
  }

  return (
    <>
      <h1>Verification failed</h1>
      <Notice kind="error">{message}</Notice>
      <p className="auth-sub">
        Request a fresh link by signing in — we will offer to resend it.
      </p>
      <Link href="/login" className="btn btn-ghost" style={{ width: "100%" }}>
        Back to sign in
      </Link>
    </>
  );
}

export default function VerifyEmailPage() {
  return (
    <AuthShell>
      <Suspense fallback={<Loading />}>
        <VerifyInner />
      </Suspense>
    </AuthShell>
  );
}
