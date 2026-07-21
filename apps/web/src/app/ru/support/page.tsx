import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { SupportPage } from "@/components/compliance-pages";
import { breadcrumbJsonLd, seoPages } from "@/lib/seo";
import { coreMetadata, coreRu } from "@/lib/seo-core";

export const metadata: Metadata = coreMetadata("/support", seoPages.support, coreRu.support, "ru");

export default function RuSupportPage() {
  return <><JsonLd data={breadcrumbJsonLd([{ name: "Главная", path: "/ru" }, { name: "Поддержка", path: "/ru/support" }])} /><main><SupportPage /></main></>;
}
