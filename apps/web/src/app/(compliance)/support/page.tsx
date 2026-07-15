import type { Metadata } from "next";
import { SupportPage } from "@/components/compliance-pages";

export const metadata: Metadata = { title: "Customer Support" };

export default function CustomerSupportPage() {
  return <SupportPage />;
}
