"use client";

import Link from "next/link";
import { AuthShell } from "@/components/auth-shell";
import { TelegramLogin } from "@/components/telegram-login";

export default function LoginPage() {
  return (
    <AuthShell>
      <h1>Partner sign in</h1>
      <p className="auth-sub">
        Invite-only program. If your Telegram was invited or approved, you&rsquo;ll get straight
        in; otherwise you can apply for review.
      </p>
      <TelegramLogin />
      <p className="auth-alt">
        New here? <Link href="/register">Apply to join</Link>
      </p>
    </AuthShell>
  );
}
