import type { Metadata } from "next";

export const dynamic = "force-dynamic";
import { notFound } from "next/navigation";
import { JsonLd } from "@/components/json-ld";
import { LearnArticleView } from "@/components/learn-article";
import { articlesForLocale, resolveArticle } from "@/lib/learn";
import { buildArticleJsonLd, buildArticleMetadata } from "@/lib/learn-page";
import { createNoIndexMetadata } from "@/lib/seo";

export function generateStaticParams() {
  return articlesForLocale("zh").map((slug) => ({ slug }));
}

export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> {
  const { slug } = await params;
  return buildArticleMetadata(slug, "zh") ?? createNoIndexMetadata("Guide not found", "The requested guide does not exist.");
}

export default async function LearnArticlePageZh({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const article = resolveArticle(slug, "zh");
  if (!article) notFound();
  return <><JsonLd data={buildArticleJsonLd(slug, "zh")!} /><LearnArticleView article={article} /></>;
}
