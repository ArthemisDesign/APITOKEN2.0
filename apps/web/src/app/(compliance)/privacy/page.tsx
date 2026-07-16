import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { LegalPage } from "@/components/compliance-pages";
import { breadcrumbJsonLd, createPageMetadata, seoPages } from "@/lib/seo";

export const metadata: Metadata = createPageMetadata(seoPages.privacy);

export default function PrivacyPage() {
  return <><JsonLd data={breadcrumbJsonLd([{ name: "Home", path: "/" }, { name: "Privacy Policy", path: "/privacy" }])} /><LegalPage kind="privacy" /></>;
}
