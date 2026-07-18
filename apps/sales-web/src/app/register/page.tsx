"use client";

import { Suspense, useEffect, useState } from "react";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { api } from "@/lib/api";
import { AuthShell } from "@/components/auth-shell";
import { Badge, Loading, Notice } from "@/components/ui";
import { TelegramLogin } from "@/components/telegram-login";

// Онбординг invite-only: инвайт выписан на конкретный telegram-юзернейм,
// человек просто подтверждает вход через Telegram — аккаунт создаётся сам.

function RegisterCard() {
  const params = useSearchParams();
  const inviteCode = params.get("invite") ?? "";
  const [invite, setInvite] = useState<{ telegramUsername: string | null } | null>(null);
  const [state, setState] = useState<"loading" | "ok" | "invalid" | "none">(inviteCode ? "loading" : "none");

  useEffect(() => {
    if (!inviteCode) return;
    let cancelled = false;
    api<{ invite: { telegramUsername: string | null } }>(`/v1/auth/invite/${encodeURIComponent(inviteCode)}`)
      .then((res) => {
        if (cancelled) return;
        setInvite(res.invite);
        setState("ok");
      })
      .catch(() => {
        if (!cancelled) setState("invalid");
      });
    return () => {
      cancelled = true;
    };
  }, [inviteCode]);

  if (state === "none") {
    return (
      <>
        <h1>Join APIToken Partners</h1>
        <p className="auth-sub">
          The partner program is invite-only. Open the personal invite link you received to join
          — or sign in if you already have an account.
        </p>
        <Link href="/login" className="btn btn-primary" style={{ width: "100%" }}>
          Sign in with Telegram
        </Link>
      </>
    );
  }

  if (state === "loading") {
    return (
      <>
        <h1>Checking your invite…</h1>
        <Loading />
      </>
    );
  }

  if (state === "invalid") {
    return (
      <>
        <h1>Invite not found</h1>
        <Notice kind="error">This invite link is invalid, expired, or already used.</Notice>
        <p className="auth-alt">
          Already a partner? <Link href="/login">Sign in</Link>
        </p>
      </>
    );
  }

  return (
    <>
      <h1>You&rsquo;re invited</h1>
      <p className="auth-sub">
        Confirm with Telegram to activate your partner account — that&rsquo;s it.
      </p>
      {invite?.telegramUsername ? (
        <p style={{ marginBottom: 16 }}>
          <Badge tone="green">Invite for @{invite.telegramUsername}</Badge>
        </p>
      ) : null}
      <TelegramLogin inviteCode={inviteCode} />
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
        <RegisterCard />
      </Suspense>
    </AuthShell>
  );
}
