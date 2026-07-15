import type { Metadata } from "next";
import { notFound } from "next/navigation";
import {
  IntegrationGuidePage, IntegrationsPage, ModelsPage,
} from "@/components/marketing-pages";

const pageTitles: Record<string, string> = {
  models: "Models", integrations: "Integrations",
  "int-claude-code": "Claude Code integration", "int-cursor": "Cursor integration",
  "int-cline": "Cline integration", "int-continue": "Continue integration",
  "int-zed": "Zed integration", "int-sdk": "SDK integration",
};

export function generateStaticParams() { return Object.keys(pageTitles).map((slug) => ({ slug })); }
export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> { const { slug } = await params; return pageTitles[slug] ? { title: pageTitles[slug] } : {}; }

export default async function StaticPage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  if (slug === "models") return <ModelsPage />;
  if (slug === "integrations") return <IntegrationsPage />;
  if (slug.startsWith("int-")) {
    const guide = slug.slice(4);
    if (["claude-code","cursor","cline","continue","zed","sdk"].includes(guide)) return <IntegrationGuidePage slug={guide} />;
  }
  notFound();
}
