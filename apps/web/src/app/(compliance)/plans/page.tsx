import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { PlansContent } from "@/components/marketing-pages";
import { B2C_PRICING_MILESTONES } from "@/lib/pricing-tiers";
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
      name: "Claude API prepaid access",
      serviceType: "Anthropic-compatible API access",
      description: "Prepaid Claude API balance billed per token at official Anthropic rates minus a progressive B2C discount of 60% up to 80%. Top up any whole USD amount; balance never expires.",
      url: plansUrl,
      provider: { "@id": `${SITE_ORIGIN}/#organization` },
      areaServed: "Worldwide",
      offers: {
        "@type": "OfferCatalog",
        name: "B2C discount tiers",
        itemListElement: B2C_PRICING_MILESTONES.map((tier) => ({
          "@type": "Offer",
          name: `${tier.label} — ${tier.discountPercent}% off official Claude API spend`,
          description: Number(tier.platformSpendUsd) === 0
            ? "Starter discount applied to every new account with no minimum."
            : `Unlocked after $${Number(tier.platformSpendUsd).toLocaleString("en-US")} in cumulative top-ups.`,
          priceCurrency: "USD",
          category: "SaaS",
          availability: "https://schema.org/InStock",
        })),
      },
    },
  ],
};

export default function PricingPage() {
  return <><JsonLd data={structuredData} /><PlansContent /></>;
}
