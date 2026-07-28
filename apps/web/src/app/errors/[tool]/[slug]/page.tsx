import type { Metadata } from "next";
import { notFound } from "next/navigation";
import {
  resolveForPage,
  ToolErrorArticlePage,
  toolErrorMetadata,
  toolErrorStaticParams,
} from "@/lib/tool-errors-page";

export const dynamicParams = false;

export function generateStaticParams() {
  return toolErrorStaticParams();
}

export async function generateMetadata({ params }: { params: Promise<{ tool: string; slug: string }> }): Promise<Metadata> {
  const { tool, slug } = await params;
  return toolErrorMetadata("en", tool, slug) ?? {};
}

export default async function Page({ params }: { params: Promise<{ tool: string; slug: string }> }) {
  const { tool, slug } = await params;
  const resolved = resolveForPage("en", tool, slug);
  if (!resolved) notFound();
  return <ToolErrorArticlePage locale="en" tool={resolved.tool} entry={resolved.entry} />;
}
