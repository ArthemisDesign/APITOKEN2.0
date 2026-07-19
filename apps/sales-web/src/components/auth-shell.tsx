"use client";

import Link from "next/link";
import type { ReactNode } from "react";
import { Brand } from "@/components/ui";
import { LanguageToggle } from "@/components/i18n";

export function AuthShell({ children }: { children: ReactNode }) {
  return (
    <div className="auth-shell">
      <div className="auth-header">
        <Link href="/" className="brand" aria-label="APIToken Partners home">
          <Brand />
        </Link>
        <LanguageToggle />
      </div>
      <div className="auth-card">{children}</div>
    </div>
  );
}
