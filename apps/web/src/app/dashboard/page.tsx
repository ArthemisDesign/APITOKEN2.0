import type { Metadata } from "next";
import { Suspense } from "react";
import { Dashboard } from "./dashboard";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Dashboard", "Manage your private apiToken.sale API keys, balance, usage, billing, and account settings.");
export default function DashboardPage() {
  return <Suspense fallback={<div className="dashboard-loading"><span className="brand">apiToken.sale</span><p>Loading your account…</p></div>}><Dashboard /></Suspense>;
}
