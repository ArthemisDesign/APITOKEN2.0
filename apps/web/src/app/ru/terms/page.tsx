import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { LegalPage } from "@/components/compliance-pages";
import { breadcrumbJsonLd, seoPages } from "@/lib/seo";
import { coreMetadata, coreRu } from "@/lib/seo-core";

export const metadata: Metadata = coreMetadata("/terms", seoPages.terms, coreRu.terms, "ru");

export default function RuTermsPage() {
  return <><JsonLd data={breadcrumbJsonLd([{ name: "Главная", path: "/ru" }, { name: "Условия", path: "/ru/terms" }])} /><main><LegalPage kind="terms" /></main></>;
}
