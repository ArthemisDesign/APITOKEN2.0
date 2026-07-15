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
]);

const authPaths = new Set([
  "/forgot-password",
  "/login",
  "/register",
  "/reset-password",
  "/verify-email",
]);

function usesPublicSiteShell(pathname: string): boolean {
  return publicSitePaths.has(pathname) || pathname.startsWith("/int-");
}

function usesAuthShell(pathname: string): boolean {
  return authPaths.has(pathname) || pathname.startsWith("/auth/");
}

export function PersistentRouteShell({ children }: Readonly<{ children: ReactNode }>) {
  const pathname = usePathname();

  if (usesPublicSiteShell(pathname)) {
    const home = pathname === "/";
    return <>
      <SiteHeader home={home} />
      {children}
      <SiteFooter full={home} />
      <MotionEffects />
    </>;
  }

  if (usesAuthShell(pathname)) return <AuthShell>{children}</AuthShell>;
  return children;
}
