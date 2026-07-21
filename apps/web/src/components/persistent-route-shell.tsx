"use client";

import { usePathname } from "next/navigation";
import { useEffect, type ReactNode } from "react";
import { withoutRussianPrefix } from "@/lib/locale-routes";
import { AuthShell } from "./auth-shell";
import { MotionEffects } from "./motion-effects";
import { SiteFooter, SiteHeader } from "./site-chrome";

const publicSitePaths = new Set([
  "/",
  "/about",
  "/changelog",
  "/contacts",
  "/integrations",
  "/models",
  "/plans",
  "/privacy",
  "/status",
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

export function usesPublicSiteShell(pathname: string): boolean {
  const path = withoutRussianPrefix(pathname);
  return publicSitePaths.has(path) || path.startsWith("/int-") || path.startsWith("/models/") || path === "/blog" || path.startsWith("/blog/");
}

export function usesAuthShell(pathname: string): boolean {
  return authPaths.has(withoutRussianPrefix(pathname)) || pathname.startsWith("/auth/");
}

export function PersistentRouteShell({ children }: Readonly<{ children: ReactNode }>) {
  const pathname = usePathname();
  useEffect(() => {
    const main = document.querySelector("main");
    if (!main) return;
    if (!main.id) main.id = "main-content";
    if (!main.hasAttribute("tabindex")) main.setAttribute("tabindex", "-1");
  }, [pathname]);

  if (usesPublicSiteShell(pathname)) {
    const home = withoutRussianPrefix(pathname) === "/";
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
