import type { Metadata } from "next";
import {
  articlesForLocale,
  clusterLabels,
  htmlLang,
  learnAlternates,
  learnHubPath,
  learnMarkdownPath,
  learnPath,
  learnUi,
  localePrefix,
  ogLocale,
  resolveArticle,
  type Locale,
} from "./learn";
import { absoluteUrl, breadcrumbNode, createPageMetadata, LAST_CONTENT_UPDATE, SITE_ORIGIN } from "./seo";

function hubAlternates(): Record<string, string> {
  const languages: Record<string, string> = {};
  for (const locale of ["en", "ru", "zh"] as Locale[]) {
    languages[htmlLang(locale)] = absoluteUrl(learnHubPath(locale));
  }
  languages["x-default"] = absoluteUrl(learnHubPath("en"));
  return languages;
}

export function buildArticleMetadata(slug: string, locale: Locale): Metadata | null {
  const article = resolveArticle(slug, locale);
  if (!article) return null;
  const path = learnPath(slug, locale);
  const base = createPageMetadata({ path, title: article.content.title, description: article.content.description });
  // en/ru get a generated per-article OG image via opengraph-image.tsx; omit the
  // static image entirely so the file convention supplies it. zh keeps og.png.
  const usesGeneratedOg = locale !== "zh";
  const { images: _ogImages, ...ogRest } = base.openGraph ?? {};
  const { images: _twImages, ...twRest } = base.twitter ?? {};
  const openGraph = {
    ...(usesGeneratedOg ? ogRest : base.openGraph),
    type: "article" as const,
    locale: ogLocale(locale),
    publishedTime: LAST_CONTENT_UPDATE.toISOString(),
    modifiedTime: LAST_CONTENT_UPDATE.toISOString(),
  };
  return {
    ...base,
    keywords: article.content.keywords,
    openGraph,
    twitter: usesGeneratedOg ? twRest : base.twitter,
    alternates: {
      canonical: absoluteUrl(path),
      languages: learnAlternates(slug, absoluteUrl),
      types: { "text/markdown": absoluteUrl(learnMarkdownPath(slug, locale)) },
    },
  };
}

export function buildArticleJsonLd(slug: string, locale: Locale) {
  const article = resolveArticle(slug, locale);
  if (!article) return null;
  const ui = learnUi[locale];
  const url = absoluteUrl(learnPath(slug, locale));
  const homePath = locale === "en" ? "/" : `${localePrefix(locale)}/`;
  return {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([
        { name: ui.crumbHome, path: homePath },
        { name: ui.crumbDocs, path: "/docs" },
        { name: ui.crumbGuides, path: learnHubPath(locale) },
        { name: article.content.h1, path: learnPath(slug, locale) },
      ]),
      {
        "@type": "Article",
        "@id": `${url}#article`,
        headline: article.content.title,
        description: article.content.description,
        url,
        mainEntityOfPage: url,
        image: absoluteUrl("/og.png"),
        dateModified: LAST_CONTENT_UPDATE.toISOString(),
        datePublished: LAST_CONTENT_UPDATE.toISOString(),
        inLanguage: htmlLang(locale),
        articleSection: clusterLabels[locale][article.cluster].label,
        keywords: article.content.keywords.join(", "),
        author: { "@id": `${SITE_ORIGIN}/#organization` },
        publisher: { "@id": `${SITE_ORIGIN}/#organization` },
      },
      {
        "@type": "FAQPage",
        "@id": `${url}#faq`,
        mainEntity: article.content.faq.map((item) => ({
          "@type": "Question",
          name: item.q,
          acceptedAnswer: { "@type": "Answer", text: item.a },
        })),
      },
    ],
  };
}

export function buildHubMetadata(locale: Locale): Metadata {
  const ui = learnUi[locale];
  const path = learnHubPath(locale);
  const base = createPageMetadata({ path, title: ui.hubTitle, description: ui.hubDescription });
  return {
    ...base,
    keywords: ui.hubKeywords,
    openGraph: { ...base.openGraph, locale: ogLocale(locale) },
    alternates: { canonical: absoluteUrl(path), languages: hubAlternates() },
  };
}

export function buildHubJsonLd(locale: Locale) {
  const ui = learnUi[locale];
  const homePath = locale === "en" ? "/" : `${localePrefix(locale)}/`;
  return {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([
        { name: ui.crumbHome, path: homePath },
        { name: ui.crumbDocs, path: "/docs" },
        { name: ui.crumbGuides, path: learnHubPath(locale) },
      ]),
      {
        "@type": "CollectionPage",
        "@id": `${absoluteUrl(learnHubPath(locale))}#collection`,
        name: ui.hubTitle,
        description: ui.hubDescription,
        url: absoluteUrl(learnHubPath(locale)),
        inLanguage: htmlLang(locale),
        hasPart: articlesForLocale(locale).map((slug) => {
          const article = resolveArticle(slug, locale)!;
          return { "@type": "Article", headline: article.content.title, url: absoluteUrl(learnPath(slug, locale)) };
        }),
      },
    ],
  };
}
