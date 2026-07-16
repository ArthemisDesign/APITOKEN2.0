import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { PlansContent } from "@/components/marketing-pages";
import { breadcrumbJsonLd, createPageMetadata, seoPages } from "@/lib/seo";

export const metadata: Metadata = createPageMetadata(seoPages.plans);

export default function PricingPage() {
  return <><JsonLd data={breadcrumbJsonLd([{ name: "Home", path: "/" }, { name: "Pricing", path: "/plans" }])} /><PlansContent /></>;
}
