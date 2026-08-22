"use client";

import { SupportContent } from "@/components/compliance-pages";
import type { AuthUser } from "@/lib/api";
import { PageHeading, useDashboardCopy } from "./shared";

export function SupportPanel({ user }: { user: AuthUser }) {
  const copy = useDashboardCopy();
  return <section className="panel"><PageHeading eyebrow={copy.supportEyebrow} title={copy.supportTitle} subtitle={copy.supportSubtitle} /><SupportContent accountId={user.id} /></section>;
}
