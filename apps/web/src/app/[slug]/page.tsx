import type { Metadata } from "next";
import { notFound } from "next/navigation";
import {
  IntegrationGuidePage, IntegrationsPage, ModelsPage, PlansPage,
} from "@/components/marketing-pages";
import { LegalPage, SupportPage } from "@/components/compliance-pages";

const pageTitles: Record<string, string> = {
  plans: "Pricing", models: "Models", integrations: "Integrations",
  "int-claude-code": "Claude Code integration", "int-cursor": "Cursor integration",
  "int-cline": "Cline integration", "int-continue": "Continue integration",
  "int-zed": "Zed integration", "int-sdk": "SDK integration",
  terms: "User Agreement", privacy: "Privacy Policy", support: "Customer Support",
};

export function generateStaticParams() { return Object.keys(pageTitles).map((slug) => ({ slug })); }
export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> { const { slug } = await params; return pageTitles[slug] ? { title: pageTitles[slug] } : {}; }

export default async function StaticPage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  if (slug === "plans") return <PlansPage />;
  if (slug === "models") return <ModelsPage />;
  if (slug === "integrations") return <IntegrationsPage />;
  if (slug === "terms" || slug === "privacy") return <LegalPage kind={slug} />;
  if (slug === "support") return <SupportPage />;
  if (slug.startsWith("int-")) {
    const guide = slug.slice(4);
    if (["claude-code","cursor","cline","continue","zed","sdk"].includes(guide)) return <IntegrationGuidePage slug={guide} />;
  }
  notFound();
}
