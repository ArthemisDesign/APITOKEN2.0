import type { Metadata } from "next";
import { DocsPortal } from "@/app/docs/docs-portal";
import { JsonLd } from "@/components/json-ld";
import { absoluteUrl, breadcrumbNode, createPageMetadata, LAST_CONTENT_UPDATE, SITE_ORIGIN } from "@/lib/seo";
import { openaiModelsAt } from "@/lib/models";

export const dynamic = "force-dynamic";

const title = "Документация API — Claude и GPT";
const description = "Подключите Anthropic-совместимый или OpenAI-совместимый клиент к API Claude и GPT через apiToken.sale.";

export const metadata: Metadata = createPageMetadata({ path: "/ru/docs", title, description });

const docsJsonLd = {
  "@context": "https://schema.org",
  "@graph": [
    breadcrumbNode([{ name: "Главная", path: "/ru" }, { name: "Документация", path: "/ru/docs" }]),
    {
      "@type": "TechArticle",
      "@id": `${absoluteUrl("/ru/docs")}#documentation`,
      headline: title,
      description,
      url: absoluteUrl("/ru/docs"),
      mainEntityOfPage: absoluteUrl("/ru/docs"),
      image: absoluteUrl("/og.png"),
      dateModified: LAST_CONTENT_UPDATE.toISOString(),
      inLanguage: "ru",
      author: { "@id": `${SITE_ORIGIN}/#organization` },
      publisher: { "@id": `${SITE_ORIGIN}/#organization` },
    },
  ],
};

export default function DocsPageRu() {
  return <><JsonLd data={docsJsonLd} /><DocsPortal openaiCatalog={openaiModelsAt()} /></>;
}
