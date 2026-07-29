import type { Metadata } from "next";
import { Suspense } from "react";
import { Dashboard } from "./dashboard";
import { DashboardLoading } from "./dashboard-loading";
import { BackendPreconnect } from "@/components/backend-preconnect";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Dashboard", "Manage your private apiToken.sale API keys, balance, usage, billing, and account settings.");
export default function DashboardPage() {
  return <>
    <BackendPreconnect />
    <Suspense fallback={<DashboardLoading />}><Dashboard /></Suspense>
  </>;
}
