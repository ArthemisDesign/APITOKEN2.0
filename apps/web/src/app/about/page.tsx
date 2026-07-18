import type { Metadata } from "next";
import Link from "next/link";
import { JsonLd } from "@/components/json-ld";
import { absoluteUrl, breadcrumbNode, createPageMetadata, SITE_NAME, SITE_ORIGIN } from "@/lib/seo";

const ABOUT_TITLE = "About apiToken.sale";
const ABOUT_DESCRIPTION = "apiToken.sale is an independent Claude API gateway: the same Anthropic Messages API and models, resold from prepaid balance at a discount, with transparent per-token billing.";

export const metadata: Metadata = {
  ...createPageMetadata({ path: "/about", title: ABOUT_TITLE, description: ABOUT_DESCRIPTION }),
  keywords: ["about apitoken.sale", "claude api gateway", "anthropic api reseller", "independent claude api provider"],
};

const aboutUrl = absoluteUrl("/about");

const principles = [
  { h: "The same API, transparently", p: "We serve the standard Anthropic Messages API — same endpoints, same model IDs, same responses. Nothing about the protocol is altered; only the price and the way you pay change." },
  { h: "Money-authoritative engine", p: "Every request is metered against official token rates and recorded in a durable charge ledger. Your discount is applied on top, and each call is visible in your dashboard down to input, output, cache and thinking tokens." },
  { h: "Prepaid, never expiring", p: "You top up any whole-dollar amount. Balance is prepaid, never expires, and is consumed only when API requests run — there is no subscription and no idle cost." },
  { h: "Accessible payment", p: "Pay by bank card or with cryptocurrency. There is no Anthropic account, waitlist, or supported-billing-country requirement to get started." },
];

export default function AboutPage() {
  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([{ name: "Home", path: "/" }, { name: "About", path: "/about" }]),
      {
        "@type": "AboutPage",
        "@id": `${aboutUrl}#about`,
        url: aboutUrl,
        name: ABOUT_TITLE,
        description: ABOUT_DESCRIPTION,
        inLanguage: "en",
        about: { "@id": `${SITE_ORIGIN}/#organization` },
        mainEntity: { "@id": `${SITE_ORIGIN}/#organization` },
      },
    ],
  };

  return (
    <><JsonLd data={structuredData} /><main className="about-page">
      <div className="page-hero">
        <div className="wrap">
          <span className="eyebrow">About</span>
          <h1>{ABOUT_TITLE}</h1>
          <p>{ABOUT_DESCRIPTION}</p>
        </div>
      </div>

      <section className="borderless">
        <div className="wrap learn-body">
          <div className="learn-section">
            <h2 className="docs-h3">What {SITE_NAME} is</h2>
            <p className="docs-para">{SITE_NAME} (also written API Token Sale) is an independent gateway to the Claude API. Developers, agencies and startups point their existing Anthropic-compatible tools — Claude Code, Cursor, Cline, Continue, Zed and the official SDKs — at our endpoint and pay for the same Claude models at up to 80% below official spend.</p>
            <p className="docs-para">We exist for the developers Anthropic does not serve conveniently: those without a supported billing country, those who want to pay by crypto, and those who simply want the same models cheaper and without a monthly plan.</p>
          </div>

          <div className="learn-section">
            <h2 className="docs-h3">How we operate</h2>
            <div className="learn-grid">
              {principles.map((item) => (
                <div className="learn-card" key={item.h} style={{ cursor: "default" }}>
                  <strong>{item.h}</strong>
                  <span>{item.p}</span>
                </div>
              ))}
            </div>
          </div>

          <div className="learn-section">
            <h2 className="docs-h3">Contact and support</h2>
            <p className="docs-para">Support is available in English and Russian through Telegram, and by email at apitokensale@gmail.com. We answer most integration and billing questions quickly.</p>
            <div className="hero-cta page-actions">
              <Link className="btn btn-primary" href="/register">Get API key</Link>
              <Link className="btn btn-ghost" href="/docs/learn">Read the guides</Link>
            </div>
          </div>
        </div>
      </section>
    </main></>
  );
}
