import type { Metadata } from "next";
import Link from "next/link";
import { JsonLd } from "@/components/json-ld";
import { absoluteUrl, breadcrumbNode, createPageMetadata, SITE_NAME, SITE_ORIGIN } from "@/lib/seo";

const TITLE = "Contact apiToken.sale";
const DESCRIPTION = "Get in touch with apiToken.sale for Claude and GPT API access, billing, refunds, or integration help — support in English and Russian via Telegram and email.";

export const metadata: Metadata = {
  ...createPageMetadata({ path: "/contacts", title: TITLE, description: DESCRIPTION }),
  keywords: ["apitoken contact", "api support contact", "apitoken.sale telegram", "claude api help", "gpt api help"],
};

export default function ContactsPage() {
  const url = absoluteUrl("/contacts");
  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([{ name: "Home", path: "/" }, { name: "Contacts", path: "/contacts" }]),
      {
        "@type": "ContactPage",
        "@id": `${url}#contact`,
        url,
        name: TITLE,
        description: DESCRIPTION,
        inLanguage: "en",
        about: { "@id": `${SITE_ORIGIN}/#organization` },
        mainEntity: { "@id": `${SITE_ORIGIN}/#organization` },
      },
    ],
  };

  return (
    <><JsonLd data={structuredData} /><main className="learn-article">
      <div className="page-hero">
        <div className="wrap">
          <span className="eyebrow">Contact</span>
          <h1>{TITLE}</h1>
          <p>{DESCRIPTION}</p>
        </div>
      </div>
      <section className="borderless">
        <div className="wrap learn-body">
          <div className="learn-section">
            <h2 className="docs-h3">Support channels</h2>
            <div className="learn-grid">
              <a className="learn-card" href="https://t.me/apitokensupportbot" target="_blank" rel="noopener">
                <strong>Telegram</strong>
                <span>Fastest response, in English and Russian — @apitokensupportbot</span>
              </a>
              <a className="learn-card" href="mailto:apitokensale@gmail.com">
                <strong>Email</strong>
                <span>apitokensale@gmail.com — account, billing and integration questions</span>
              </a>
            </div>
          </div>
          <div className="learn-section">
            <h2 className="docs-h3">What to include</h2>
            <p className="docs-para">To help us resolve things quickly, include your account email and, where relevant, the affected endpoint, the masked key id, and a short description of what you expected versus what happened. Never share your full API key or password.</p>
          </div>
          <div className="learn-section">
            <h2 className="docs-h3">About {SITE_NAME}</h2>
            <p className="docs-para">{SITE_NAME} is an independent multi-provider API gateway. It serves the standard Anthropic Messages API and an OpenAI-compatible API with every supported Claude and GPT model from one prepaid balance at a discount, with no Anthropic or OpenAI account required.</p>
            <div className="hero-cta page-actions">
              <Link className="btn btn-primary" href="/register">Get API key</Link>
              <Link className="btn btn-ghost" href="/about">About us</Link>
            </div>
          </div>
        </div>
      </section>
    </main></>
  );
}
