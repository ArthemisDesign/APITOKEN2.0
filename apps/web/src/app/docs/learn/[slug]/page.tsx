import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { JsonLd } from "@/components/json-ld";
import { LearnArticleView } from "@/components/learn-article";
import { clusterLabels, learnArticles, learnArticlesBySlug, learnPath, LEARN_HUB_PATH } from "@/lib/learn";
import { absoluteUrl, breadcrumbNode, createNoIndexMetadata, createPageMetadata, LAST_CONTENT_UPDATE, SITE_ORIGIN } from "@/lib/seo";

export function generateStaticParams() {
  return learnArticles.map((article) => ({ slug: article.slug }));
}

export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> {
  const { slug } = await params;
  const article = learnArticlesBySlug[slug];
  if (!article) return createNoIndexMetadata("Guide not found", "The requested guide does not exist.");
  return {
    ...createPageMetadata({ path: learnPath(slug), title: article.title, description: article.description }),
    keywords: article.keywords,
  };
}

export default async function LearnArticlePage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const article = learnArticlesBySlug[slug];
  if (!article) notFound();

  const url = absoluteUrl(learnPath(slug));
  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([
        { name: "Home", path: "/" },
        { name: "Docs", path: "/docs" },
        { name: "Guides", path: LEARN_HUB_PATH },
        { name: article.h1, path: learnPath(slug) },
      ]),
      {
        "@type": "Article",
        "@id": `${url}#article`,
        headline: article.title,
        description: article.description,
        url,
        mainEntityOfPage: url,
        image: absoluteUrl("/og.png"),
        dateModified: LAST_CONTENT_UPDATE.toISOString(),
        datePublished: LAST_CONTENT_UPDATE.toISOString(),
        inLanguage: "en",
        articleSection: clusterLabels[article.cluster].label,
        keywords: article.keywords.join(", "),
        author: { "@id": `${SITE_ORIGIN}/#organization` },
        publisher: { "@id": `${SITE_ORIGIN}/#organization` },
      },
      {
        "@type": "FAQPage",
        "@id": `${url}#faq`,
        mainEntity: article.faq.map((item) => ({
          "@type": "Question",
          name: item.q,
          acceptedAnswer: { "@type": "Answer", text: item.a },
        })),
      },
    ],
  };

  return <><JsonLd data={structuredData} /><LearnArticleView article={article} /></>;
}
