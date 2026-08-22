import type { Metadata } from "next";

export const dynamic = "force-dynamic";
import { notFound } from "next/navigation";
import { JsonLd } from "@/components/json-ld";
import { LearnArticleView } from "@/components/learn-article";
import { articlesForLocale, resolveArticle } from "@/lib/learn";
import { buildArticleJsonLd, buildArticleMetadata } from "@/lib/learn-page";
import { createNoIndexMetadata } from "@/lib/seo";

export function generateStaticParams() {
  return articlesForLocale("ko").map((slug) => ({ slug }));
}

export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> {
  const { slug } = await params;
  return buildArticleMetadata(slug, "ko") ?? createNoIndexMetadata("Guide not found", "The requested guide does not exist.");
}

export default async function LearnArticlePageKo({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const article = resolveArticle(slug, "ko");
  if (!article) notFound();
  return <><JsonLd data={buildArticleJsonLd(slug, "ko")!} /><LearnArticleView article={article} /></>;
}
