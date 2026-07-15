import type { Metadata } from "next";
import { Suspense } from "react";
import { Dashboard } from "./dashboard";

export const metadata: Metadata = { title: "Dashboard", robots: { index: false, follow: false } };
export default function DashboardPage() {
  return <Suspense fallback={<div className="dashboard-loading"><span className="brand">apiToken.sale</span><p>Loading your account…</p></div>}><Dashboard /></Suspense>;
}
