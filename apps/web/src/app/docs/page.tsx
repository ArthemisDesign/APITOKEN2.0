import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { DocsPortal } from "./docs-portal";
import { openaiModelsAt } from "@/lib/models";
import {
  absoluteUrl,
  breadcrumbNode,
  createPageMetadata,
  LAST_CONTENT_UPDATE,
  SITE_ORIGIN,
  seoPages,
} from "@/lib/seo";

export const metadata: Metadata = createPageMetadata(seoPages.docs);

// Pricing is effective-dated, so the docs resolve the official card on each request.
export const dynamic = "force-dynamic";

const docsJsonLd = {
  "@context": "https://schema.org",
  "@graph": [
    breadcrumbNode([{ name: "Home", path: "/" }, { name: "Documentation", path: "/docs" }]),
    {
      "@type": "TechArticle",
      "@id": `${absoluteUrl("/docs")}#documentation`,
      headline: seoPages.docs.title,
      description: seoPages.docs.description,
      url: absoluteUrl("/docs"),
      mainEntityOfPage: absoluteUrl("/docs"),
      image: absoluteUrl("/og.png"),
      dateModified: LAST_CONTENT_UPDATE.toISOString(),
      inLanguage: "en",
      author: { "@id": `${SITE_ORIGIN}/#organization` },
      publisher: { "@id": `${SITE_ORIGIN}/#organization` },
    },
  ],
};

export default function DocsPage() {
  return <><JsonLd data={docsJsonLd} /><DocsPortal openaiCatalog={openaiModelsAt()} /></>;
}
