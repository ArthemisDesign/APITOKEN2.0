import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { DocsPortal } from "./docs-portal";
import {
  absoluteUrl,
  breadcrumbNode,
  createPageMetadata,
  LAST_CONTENT_UPDATE,
  SITE_ORIGIN,
  seoPages,
} from "@/lib/seo";

export const metadata: Metadata = createPageMetadata(seoPages.docs);

// English docs are content-only and can be generated once. Localized variants
// retain request-aware language handling in the shared root layout.
export const dynamic = "force-static";

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
  return <><JsonLd data={docsJsonLd} /><DocsPortal /></>;
}
