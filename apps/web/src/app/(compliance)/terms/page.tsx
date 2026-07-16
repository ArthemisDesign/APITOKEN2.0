import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { LegalPage } from "@/components/compliance-pages";
import { breadcrumbJsonLd, createPageMetadata, seoPages } from "@/lib/seo";

export const metadata: Metadata = createPageMetadata(seoPages.terms);

export default function TermsPage() {
  return <><JsonLd data={breadcrumbJsonLd([{ name: "Home", path: "/" }, { name: "User Agreement", path: "/terms" }])} /><LegalPage kind="terms" /></>;
}
