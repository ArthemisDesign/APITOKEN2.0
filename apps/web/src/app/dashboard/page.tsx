import type { Metadata } from "next";
import { Dashboard } from "./dashboard";

export const metadata: Metadata = { title: "Dashboard", robots: { index: false, follow: false } };
export default function DashboardPage() { return <Dashboard />; }
