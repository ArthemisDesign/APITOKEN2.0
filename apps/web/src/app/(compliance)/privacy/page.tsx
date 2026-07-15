import type { Metadata } from "next";
import { LegalPage } from "@/components/compliance-pages";

export const metadata: Metadata = { title: "Privacy Policy" };

export default function PrivacyPage() {
  return <LegalPage kind="privacy" />;
}
