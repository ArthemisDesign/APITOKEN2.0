"use client";

import Link from "next/link";
import { AuthShell } from "@/components/auth-shell";
import { TelegramLogin } from "@/components/telegram-login";

export default function LoginPage() {
  return (
    <AuthShell>
      <h1>Partner sign in</h1>
      <p className="auth-sub">Sign in with the Telegram account your partner profile is linked to.</p>
      <TelegramLogin />
      <p className="auth-alt">
        Have an invite? <Link href="/register">Join with your invite link</Link>
      </p>
    </AuthShell>
  );
}
