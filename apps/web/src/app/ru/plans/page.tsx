import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { PlansContent } from "@/components/marketing-pages";
import { breadcrumbJsonLd, seoPages } from "@/lib/seo";
import { coreMetadata, coreRu } from "@/lib/seo-core";

export const metadata: Metadata = coreMetadata("/plans", seoPages.plans, coreRu.plans, "ru");

export default function RuPlansPage() {
  return <><JsonLd data={breadcrumbJsonLd([{ name: "Главная", path: "/ru" }, { name: "Цены", path: "/ru/plans" }])} /><PlansContent /></>;
}
