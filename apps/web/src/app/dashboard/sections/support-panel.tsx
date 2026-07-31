"use client";

import { SupportContent } from "@/components/compliance-pages";
import { PageHeading, useDashboardCopy } from "./shared";

export function SupportPanel() {
  const copy = useDashboardCopy();
  return <section className="panel"><PageHeading eyebrow={copy.supportEyebrow} title={copy.supportTitle} subtitle={copy.supportSubtitle} /><SupportContent /></section>;
}
