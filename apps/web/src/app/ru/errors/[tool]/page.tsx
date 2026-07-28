import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { findTool } from "@/lib/tool-errors";
import { ToolErrorsHubPage, toolHubMetadata, toolStaticParams } from "@/lib/tool-errors-page";

export const dynamicParams = false;

export function generateStaticParams() {
  return toolStaticParams();
}

export async function generateMetadata({ params }: { params: Promise<{ tool: string }> }): Promise<Metadata> {
  const { tool } = await params;
  return toolHubMetadata("ru", tool) ?? {};
}

export default async function Page({ params }: { params: Promise<{ tool: string }> }) {
  const { tool: toolSlug } = await params;
  const tool = findTool(toolSlug);
  if (!tool) notFound();
  return <ToolErrorsHubPage locale="ru" tool={tool} />;
}
