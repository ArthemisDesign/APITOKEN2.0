import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { JsonLd } from "@/components/json-ld";
import { LearnArticleView } from "@/components/learn-article";
import { articlesForLocale, resolveArticle } from "@/lib/learn";
import { buildArticleJsonLd, buildArticleMetadata } from "@/lib/learn-page";
import { createNoIndexMetadata } from "@/lib/seo";

export function generateStaticParams() {
  return articlesForLocale("ru").map((slug) => ({ slug }));
}

export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> {
  const { slug } = await params;
  return buildArticleMetadata(slug, "ru") ?? createNoIndexMetadata("Guide not found", "The requested guide does not exist.");
}

export default async function LearnArticlePageRu({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const article = resolveArticle(slug, "ru");
  if (!article) notFound();
  return <><JsonLd data={buildArticleJsonLd(slug, "ru")!} /><LearnArticleView article={article} /></>;
}
