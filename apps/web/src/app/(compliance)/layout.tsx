import type { ReactNode } from "react";
import { MotionEffects } from "@/components/motion-effects";
import { SiteFooter, SiteHeader } from "@/components/site-chrome";

export default function ComplianceLayout({ children }: Readonly<{ children: ReactNode }>) {
  return <><SiteHeader /><main>{children}</main><SiteFooter /><MotionEffects /></>;
}
