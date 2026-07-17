import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { JsonLd } from "@/components/json-ld";
import {
  IntegrationGuidePage, IntegrationsPage, ModelsPage,
} from "@/components/marketing-pages";
import {
  absoluteUrl,
  breadcrumbJsonLd,
  breadcrumbNode,
  createNoIndexMetadata,
  createPageMetadata,
  integrationGuideSeo,
  LAST_CONTENT_UPDATE,
  SITE_ORIGIN,
  seoPages,
  type IntegrationGuideSlug,
} from "@/lib/seo";
import { coreAlternates } from "@/lib/seo-core";

const staticPageSeo = {
  "models": seoPages.models,
  "integrations": seoPages.integrations,
  "int-claude-code": integrationGuideSeo["claude-code"],
  "int-cursor": integrationGuideSeo.cursor,
  "int-cline": integrationGuideSeo.cline,
  "int-continue": integrationGuideSeo.continue,
  "int-zed": integrationGuideSeo.zed,
  "int-sdk": integrationGuideSeo.sdk,
} as const;

type StaticPageSlug = keyof typeof staticPageSeo;

function isStaticPageSlug(slug: string): slug is StaticPageSlug {
  return slug in staticPageSeo;
}

function isIntegrationGuideSlug(slug: string): slug is IntegrationGuideSlug {
  return slug in integrationGuideSeo;
}

export function generateStaticParams() {
  return Object.keys(staticPageSeo).map((slug) => ({ slug }));
}

export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> {
  const { slug } = await params;
  if (isStaticPageSlug(slug)) {
    const page = staticPageSeo[slug];
    return { ...createPageMetadata(page), alternates: coreAlternates(page.path) };
  }
  return createNoIndexMetadata("Page not found", "The requested page does not exist.");
}

export default async function StaticPage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;

  if (slug === "models") {
    return <><JsonLd data={breadcrumbJsonLd([{ name: "Home", path: "/" }, { name: "Claude models", path: "/models" }])} /><ModelsPage /></>;
  }

  if (slug === "integrations") {
    return <><JsonLd data={breadcrumbJsonLd([{ name: "Home", path: "/" }, { name: "Integrations", path: "/integrations" }])} /><IntegrationsPage /></>;
  }

  if (slug.startsWith("int-")) {
    const guideSlug = slug.slice(4);
    if (isIntegrationGuideSlug(guideSlug)) {
      const seo = integrationGuideSeo[guideSlug];
      const name = seo.title.replace(/^Connect /, "").replace(/ to (apiToken\.sale|the Claude API)$/, "");
      const structuredData = {
        "@context": "https://schema.org",
        "@graph": [
          breadcrumbNode([
            { name: "Home", path: "/" },
            { name: "Integrations", path: "/integrations" },
            { name, path: seo.path },
          ]),
          {
            "@type": "TechArticle",
            "@id": `${absoluteUrl(seo.path)}#guide`,
            headline: seo.title,
            description: seo.description,
            url: absoluteUrl(seo.path),
            mainEntityOfPage: absoluteUrl(seo.path),
            image: absoluteUrl("/og.png"),
            dateModified: LAST_CONTENT_UPDATE.toISOString(),
            inLanguage: "en",
            author: { "@id": `${SITE_ORIGIN}/#organization` },
            publisher: { "@id": `${SITE_ORIGIN}/#organization` },
          },
        ],
      };
      return <><JsonLd data={structuredData} /><IntegrationGuidePage slug={guideSlug} /></>;
    }
  }

  notFound();
}
