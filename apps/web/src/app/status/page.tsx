import type { Metadata } from "next";
import Link from "next/link";
import { JsonLd } from "@/components/json-ld";
import { absoluteUrl, breadcrumbNode, createPageMetadata, SITE_ORIGIN } from "@/lib/seo";
import { loadServiceStatus, type ServiceLevel } from "@/lib/service-status";

const TITLE = "Service Status";
const DESCRIPTION = "Live health of the apiToken.sale Claude and GPT API gateways and dashboard, plus current monitoring coverage for payments. Report an issue via Telegram support.";

export const metadata: Metadata = {
  ...createPageMetadata({ path: "/status", title: TITLE, description: DESCRIPTION }),
  keywords: ["apitoken status", "api status", "apitoken.sale uptime", "claude api status", "gpt api status"],
};

export const revalidate = 30;

const levelColor: Record<ServiceLevel, string> = {
  operational: "var(--ok, #12925a)",
  degraded: "var(--warn, #b7791f)",
  unavailable: "var(--danger, #c0392b)",
  unknown: "var(--txt-4, #777)",
};

export default async function StatusPage() {
  const url = absoluteUrl("/status");
  const status = await loadServiceStatus();
  const summary = status.overall === "operational"
    ? "Core API systems operational."
    : status.overall === "degraded"
      ? "A core API dependency is degraded."
      : "Live core status is temporarily unavailable.";
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
              <span style={{ width: 10, height: 10, borderRadius: "50%", background: levelColor[status.overall], display: "inline-block", flex: "0 0 auto" }} aria-hidden="true" />
              <div>
                <strong>{summary}</strong>
                <div style={{ color: "var(--txt-3)", fontSize: 12, marginTop: 3 }}>
                  Core check updated <time dateTime={status.checkedAt}>{new Date(status.checkedAt).toLocaleString("en-US", { timeZone: "UTC", dateStyle: "medium", timeStyle: "short" })} UTC</time>
                </div>
              </div>
            </div>
          </div>
          <div className="learn-section">
            <h2 className="docs-h3">Components</h2>
            <div className="learn-grid">
              {status.components.map((component) => (
                <div className="learn-card" key={component.name} style={{ cursor: "default", display: "grid", gap: 8 }}>
                  <strong style={{ display: "flex", alignItems: "center", gap: 9 }}>
                    <span style={{ width: 8, height: 8, borderRadius: "50%", background: levelColor[component.level], display: "inline-block", flex: "0 0 auto" }} aria-hidden="true" />
                    {component.name}
                  </strong>
                  <span>{component.note}</span>
                  <span style={{ color: levelColor[component.level], fontWeight: 700 }}>{component.label}</span>
                </div>
              ))}
            </div>
            <p className="docs-para" style={{ marginTop: 18 }}>The live check covers the commercial database and the gateway engine that serves both the Anthropic and OpenAI-compatible APIs. It does not currently measure third-party checkout providers, the payment worker, historical uptime, or incident history; those components are labelled accordingly.</p>
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
