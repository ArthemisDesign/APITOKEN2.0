import type { ReactNode } from "react";

export default function ComplianceLayout({ children }: Readonly<{ children: ReactNode }>) {
  return <main>{children}</main>;
}
