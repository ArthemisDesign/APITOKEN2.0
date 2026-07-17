"use client";

import { usePathname } from "next/navigation";
import type { ReactNode } from "react";
import { AuthShell } from "./auth-shell";
import { MotionEffects } from "./motion-effects";
import { SiteFooter, SiteHeader } from "./site-chrome";

const publicSitePaths = new Set([
  "/",
  "/integrations",
  "/models",
  "/plans",
  "/privacy",
  "/support",
  "/terms",
  "/tools/claude-api-cost-calculator",
]);

const authPaths = new Set([
  "/forgot-password",
  "/login",
  "/register",
  "/reset-password",
  "/verify-email",
]);

// Treat /ru mirrors of the marketing pages the same as their English paths.
function withoutRuPrefix(pathname: string): string {
  if (pathname === "/ru") return "/";
  if (pathname.startsWith("/ru/")) return pathname.slice(3);
  return pathname;
}

function usesPublicSiteShell(pathname: string): boolean {
  const path = withoutRuPrefix(pathname);
  return publicSitePaths.has(path) || path.startsWith("/int-");
}

function usesAuthShell(pathname: string): boolean {
  return authPaths.has(pathname) || pathname.startsWith("/auth/");
}

export function PersistentRouteShell({ children }: Readonly<{ children: ReactNode }>) {
  const pathname = usePathname();

  if (usesPublicSiteShell(pathname)) {
    const home = withoutRuPrefix(pathname) === "/";
    return <>
      <SiteHeader home={home} />
      {children}
      <SiteFooter full />
      <MotionEffects />
    </>;
  }

  if (usesAuthShell(pathname)) return <AuthShell>{children}</AuthShell>;
  return children;
}
