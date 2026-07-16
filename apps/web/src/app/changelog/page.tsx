import type { Metadata } from "next";
import Link from "next/link";
import { JsonLd } from "@/components/json-ld";
import { absoluteUrl, breadcrumbNode, createPageMetadata, LAST_CONTENT_UPDATE, SITE_ORIGIN } from "@/lib/seo";

const TITLE = "Changelog";
const DESCRIPTION = "Recent updates to apiToken.sale — new Claude models, guides, localization, and platform improvements.";

export const metadata: Metadata = {
  ...createPageMetadata({ path: "/changelog", title: TITLE, description: DESCRIPTION }),
  keywords: ["apitoken changelog", "claude api updates", "apitoken.sale news", "claude api platform changes"],
};

const entries = [
  { date: "2026-07", title: "Guides in Russian and Chinese", body: "The full Claude API guide library is now available in English, Russian and Simplified Chinese with per-language URLs." },
  { date: "2026-07", title: "Guide library expanded", body: "Added new guides covering gateways, rate limits, streaming, prompt caching, key security, and building AI agents on Claude." },
  { date: "2026-06", title: "Claude Opus 4.8 available", body: "Opus 4.8 is available on the same API3 key and prepaid balance as Opus 4.7, Sonnet 5, Sonnet 4.6 and Haiku 4.5." },
  { date: "2026-06", title: "Per-key spend controls", body: "Set daily and monthly spend caps per key, scope keys to tools, and rotate keys without downtime from the dashboard." },
  { date: "2026-05", title: "Progressive discount tiers", body: "B2C accounts start 60% below official spend and progress up to 80% off as cumulative top-ups grow." },
];

export default function ChangelogPage() {
  const url = absoluteUrl("/changelog");
  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([{ name: "Home", path: "/" }, { name: "Changelog", path: "/changelog" }]),
      {
        "@type": "CollectionPage",
        "@id": `${url}#changelog`,
        url,
        name: TITLE,
        description: DESCRIPTION,
        inLanguage: "en",
        dateModified: LAST_CONTENT_UPDATE.toISOString(),
        about: { "@id": `${SITE_ORIGIN}/#organization` },
      },
    ],
  };

  return (
    <><JsonLd data={structuredData} /><main className="learn-article">
      <div className="page-hero">
        <div className="wrap">
          <span className="eyebrow">Product</span>
          <h1>{TITLE}</h1>
          <p>{DESCRIPTION}</p>
        </div>
      </div>
      <section className="borderless">
        <div className="wrap learn-body">
          {entries.map((entry) => (
            <div className="learn-section" key={entry.title}>
              <h2 className="docs-h3"><span style={{ fontFamily: "var(--font-mono)", color: "var(--txt-3)", fontSize: "0.7em", marginRight: 10 }}>{entry.date}</span>{entry.title}</h2>
              <p className="docs-para">{entry.body}</p>
            </div>
          ))}
          <div className="hero-cta page-actions">
            <Link className="btn btn-primary" href="/register">Get API key</Link>
            <Link className="btn btn-ghost" href="/docs/learn">Read the guides</Link>
          </div>
        </div>
      </section>
    </main></>
  );
}
