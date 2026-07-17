import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { LegalPage } from "@/components/compliance-pages";
import { breadcrumbJsonLd, seoPages } from "@/lib/seo";
import { coreMetadata, coreRu } from "@/lib/seo-core";

export const metadata: Metadata = coreMetadata("/privacy", seoPages.privacy, coreRu.privacy, "ru");

export default function RuPrivacyPage() {
  return <><JsonLd data={breadcrumbJsonLd([{ name: "Главная", path: "/ru" }, { name: "Конфиденциальность", path: "/ru/privacy" }])} /><LegalPage kind="privacy" /></>;
}
