import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { SupportPage } from "@/components/compliance-pages";
import { absoluteUrl, breadcrumbNode, createPageMetadata, SITE_ORIGIN, seoPages } from "@/lib/seo";
import { coreAlternates } from "@/lib/seo-core";

export const metadata: Metadata = { ...createPageMetadata(seoPages.support), alternates: coreAlternates("/support") };

export default function CustomerSupportPage() {
  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([{ name: "Home", path: "/" }, { name: "Support", path: "/support" }]),
      {
        "@type": "ContactPage",
        "@id": `${absoluteUrl("/support")}#contact`,
        url: absoluteUrl("/support"),
        name: seoPages.support.title,
        description: seoPages.support.description,
        inLanguage: "en",
        about: { "@id": `${SITE_ORIGIN}/#organization` },
      },
    ],
  };
  return <><JsonLd data={structuredData} /><SupportPage /></>;
}
