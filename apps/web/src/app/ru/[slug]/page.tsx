import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { JsonLd } from "@/components/json-ld";
import { IntegrationGuidePage, IntegrationsPage, ModelsPage } from "@/components/marketing-pages";
import { absoluteUrl, breadcrumbJsonLd, breadcrumbNode, createNoIndexMetadata, integrationGuideSeo, LAST_CONTENT_UPDATE, SITE_ORIGIN, seoPages, type IntegrationGuideSlug } from "@/lib/seo";
import { coreIntRu, coreMetadata, coreRu } from "@/lib/seo-core";

const staticPages: Record<string, { enPath: string; en: { title: string; description: string }; ru: { title: string; description: string } }> = {
  models: { enPath: "/models", en: seoPages.models, ru: coreRu.models },
  integrations: { enPath: "/integrations", en: seoPages.integrations, ru: coreRu.integrations },
};

function isIntegrationGuideSlug(slug: string): slug is IntegrationGuideSlug {
  return slug in integrationGuideSeo;
}

export function generateStaticParams() {
  return [
    { slug: "models" },
    { slug: "integrations" },
    ...Object.values(integrationGuideSeo).map((page) => ({ slug: page.path.replace(/^\/int-/, "int-").replace(/^\//, "") })),
  ];
}

export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> {
  const { slug } = await params;
  if (slug in staticPages) {
    const page = staticPages[slug];
    return coreMetadata(page.enPath, page.en, page.ru, "ru");
  }
  if (slug.startsWith("int-")) {
    const guide = slug.slice(4);
    if (isIntegrationGuideSlug(guide)) {
      return coreMetadata(`/int-${guide}`, integrationGuideSeo[guide], coreIntRu[guide], "ru");
    }
  }
  return createNoIndexMetadata("Страница не найдена", "Запрошенная страница не существует.");
}

export default async function RuStaticPage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;

  if (slug === "models") {
    return <><JsonLd data={breadcrumbJsonLd([{ name: "Главная", path: "/ru" }, { name: "Модели Claude", path: "/ru/models" }])} /><ModelsPage /></>;
  }
  if (slug === "integrations") {
    return <><JsonLd data={breadcrumbJsonLd([{ name: "Главная", path: "/ru" }, { name: "Интеграции", path: "/ru/integrations" }])} /><IntegrationsPage /></>;
  }
  if (slug.startsWith("int-")) {
    const guide = slug.slice(4);
    if (isIntegrationGuideSlug(guide)) {
      const seo = integrationGuideSeo[guide];
      const url = absoluteUrl(`/ru${seo.path}`);
      const structuredData = {
        "@context": "https://schema.org",
        "@graph": [
          breadcrumbNode([
            { name: "Главная", path: "/ru" },
            { name: "Интеграции", path: "/ru/integrations" },
            { name: coreIntRu[guide].title, path: `/ru${seo.path}` },
          ]),
          {
            "@type": "TechArticle",
            "@id": `${url}#guide`,
            headline: coreIntRu[guide].title,
            description: coreIntRu[guide].description,
            url,
            mainEntityOfPage: url,
            image: absoluteUrl("/og.png"),
            dateModified: LAST_CONTENT_UPDATE.toISOString(),
            inLanguage: "ru",
            author: { "@id": `${SITE_ORIGIN}/#organization` },
            publisher: { "@id": `${SITE_ORIGIN}/#organization` },
          },
        ],
      };
      return <><JsonLd data={structuredData} /><IntegrationGuidePage slug={guide} /></>;
    }
  }

  notFound();
}
