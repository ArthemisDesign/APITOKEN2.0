import type { Metadata } from "next";
import { LegalPage } from "@/components/compliance-pages";

export const metadata: Metadata = { title: "User Agreement" };

export default function TermsPage() {
  return <LegalPage kind="terms" />;
}
