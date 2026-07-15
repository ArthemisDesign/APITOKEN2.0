"use client";

import { Analytics, type BeforeSendEvent } from "@vercel/analytics/next";

export function withoutSensitiveUrlData(url: string): string {
  return url.split("#", 1)[0]!.split("?", 1)[0]!;
}

export function SiteAnalytics() {
  return <Analytics beforeSend={(event: BeforeSendEvent) => ({
    ...event,
    url: withoutSensitiveUrlData(event.url),
  })} />;
}
