import type { Metadata } from "next";
import Link from "next/link";
import { JsonLd } from "@/components/json-ld";
import { absoluteUrl, breadcrumbNode, createPageMetadata, SITE_ORIGIN } from "@/lib/seo";

const TITLE = "Service Status";
const DESCRIPTION = "Current operational status of the apiToken.sale Claude API gateway, dashboard, and payments. Report an issue via Telegram support.";

export const metadata: Metadata = {
  ...createPageMetadata({ path: "/status", title: TITLE, description: DESCRIPTION }),
  keywords: ["apitoken status", "claude api status", "apitoken.sale uptime", "claude api gateway status"],
};

const components = [
  { name: "API gateway (api.apitoken.sale)", note: "Anthropic-compatible /v1/messages endpoint" },
  { name: "Dashboard & key management", note: "Account, keys, usage and top-ups" },
  { name: "Payments (card & crypto)", note: "Prepaid balance top-ups" },
  { name: "Guides & documentation", note: "Public docs and guide library" },
];

export default function StatusPage() {
  const url = absoluteUrl("/status");
  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([{ name: "Home", path: "/" }, { name: "Status", path: "/status" }]),
      {
        "@type": "WebPage",
        "@id": `${url}#status`,
        url,
        name: TITLE,
        description: DESCRIPTION,
        inLanguage: "en",
        about: { "@id": `${SITE_ORIGIN}/#organization` },
      },
    ],
  };

  return (
    <><JsonLd data={structuredData} /><main className="learn-article">
      <div className="page-hero">
        <div className="wrap">
          <span className="eyebrow">Status</span>
          <h1>{TITLE}</h1>
          <p>{DESCRIPTION}</p>
        </div>
      </div>
      <section className="borderless">
        <div className="wrap learn-body">
          <div className="learn-section">
            <div className="docs-notice" style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <span style={{ width: 10, height: 10, borderRadius: "50%", background: "var(--good, #12925a)", display: "inline-block" }} aria-hidden="true" />
              <strong>All systems operational.</strong>
            </div>
          </div>
          <div className="learn-section">
            <h2 className="docs-h3">Components</h2>
            <div className="learn-grid">
              {components.map((component) => (
                <div className="learn-card" key={component.name} style={{ cursor: "default" }}>
                  <strong>{component.name}</strong>
                  <span>{component.note} — operational</span>
                </div>
              ))}
            </div>
          </div>
          <div className="learn-section">
            <h2 className="docs-h3">Report an issue</h2>
            <p className="docs-para">Seeing a problem? Contact support in English or Russian on Telegram, or email apitokensale@gmail.com, and include your account email and the affected endpoint or key.</p>
            <div className="hero-cta page-actions">
              <Link className="btn btn-primary" href="/support">Contact support</Link>
              <Link className="btn btn-ghost" href="/changelog">Recent changes</Link>
            </div>
          </div>
        </div>
      </section>
    </main></>
  );
}
