import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { PlansContent } from "@/components/marketing-pages";
import { FLAT_DISCOUNT_PERCENT } from "@/lib/pricing-tiers";
import { absoluteUrl, breadcrumbNode, createPageMetadata, seoPages, SITE_ORIGIN } from "@/lib/seo";
import { coreAlternates } from "@/lib/seo-core";

export const metadata: Metadata = { ...createPageMetadata(seoPages.plans), alternates: coreAlternates("/plans") };

const plansUrl = absoluteUrl("/plans");

const structuredData = {
  "@context": "https://schema.org",
  "@graph": [
    breadcrumbNode([{ name: "Home", path: "/" }, { name: "Pricing", path: "/plans" }]),
    {
      "@type": "Service",
      "@id": `${plansUrl}#prepaid-access`,
      name: "Claude & GPT API prepaid access",
      serviceType: "Anthropic-compatible and OpenAI-compatible API access",
      description: `Prepaid API balance billed per token at official Anthropic and OpenAI rates minus a flat ${FLAT_DISCOUNT_PERCENT}% discount for every account. Top up any whole USD amount; balance never expires.`,
      url: plansUrl,
      provider: { "@id": `${SITE_ORIGIN}/#organization` },
      areaServed: "Worldwide",
      offers: {
        "@type": "Offer",
        name: `Flat ${FLAT_DISCOUNT_PERCENT}% off official API spend`,
        description: "One flat discount applied to every account on every Claude and GPT model, with no volume conditions.",
        priceCurrency: "USD",
        category: "SaaS",
        availability: "https://schema.org/InStock",
      },
    },
  ],
};

export default function PricingPage() {
  return <><JsonLd data={structuredData} /><PlansContent /></>;
}
