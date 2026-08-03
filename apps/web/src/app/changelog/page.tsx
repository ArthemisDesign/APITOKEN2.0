import type { Metadata } from "next";
import Link from "next/link";
import { JsonLd } from "@/components/json-ld";
import { absoluteUrl, breadcrumbNode, createPageMetadata, LAST_CONTENT_UPDATE, SITE_ORIGIN } from "@/lib/seo";

const TITLE = "Changelog";
const DESCRIPTION = "Recent updates to apiToken.sale — new Claude and GPT models, guides, localization, and platform improvements.";

export const metadata: Metadata = {
  ...createPageMetadata({ path: "/changelog", title: TITLE, description: DESCRIPTION }),
  keywords: ["apitoken changelog", "api updates", "apitoken.sale news", "claude api platform changes", "gpt api updates"],
};

const entries = [
  { date: "2026-08", title: "One unified router endpoint for every provider", body: "https://router.apitoken.sale now serves all three providers on a single endpoint: the native Anthropic lane (POST /v1/messages), the OpenAI lanes (POST /v1/responses and the OpenAI-compatible universal lane POST /v1/chat/completions for any catalog model) and the native Gemini lane (/v1beta/models/{model}:generateContent) — all with the same sk-pool key. A unified namespaced catalog (anthropic/*, openai/*, google/*) is available at GET /v1/models. The legacy per-provider hosts (api.apitoken.sale, openai.api.apitoken.sale/v1, gemini.api.apitoken.sale) remain fully supported for existing integrations." },
  { date: "2026-07", title: "One flat 50% discount — tiers retired", body: "B2C pricing is now a flat 50% off official provider spend on every request, for every account and any top-up amount. The Starter/Builder/Pro/Studio/Scale ladder, cumulative top-up thresholds and 30-day tier retention are gone — there is nothing to unlock and nothing to keep." },
  { date: "2026-07", title: "GPT-5.6 on the same key — apiToken.sale is now multi-provider", body: "The OpenAI-compatible API at openai.api.apitoken.sale/v1 serves the GPT-5.6 line (sol, terra, luna), gpt-5.5 and gpt-5.4 through Responses and Chat Completions with SSE streaming. One sk-pool key and one prepaid balance cover both Claude and GPT." },
  { date: "2026-07", title: "Tier ladder rebalanced", body: "B2C tier discounts now progress from 60% to 70% off official spend (Starter 60%, Builder 62.5%, Pro 65%, Studio 67.5%, Scale 70%). Thresholds and 30-day holds are unchanged; Starter stays free at 60%." },
  { date: "2026-07", title: "Guides in Russian and Chinese", body: "The full Claude API guide library is now available in English, Russian and Simplified Chinese with per-language URLs." },
  { date: "2026-07", title: "Guide library expanded", body: "Added new guides covering gateways, rate limits, streaming, prompt caching, key security, and building AI agents on Claude." },
  { date: "2026-06", title: "Claude Opus 4.8 available", body: "Opus 4.8 is available on the same API key and prepaid balance as Opus 4.7, Sonnet 5, Sonnet 4.6 and Haiku 4.5." },
  { date: "2026-07", title: "Per-key lifetime guardrails", body: "Set an optional lifetime spending limit and expiration date when you create a key, then update or remove either guardrail from the dashboard." },
  { date: "2026-05", title: "Progressive discount tiers", body: "B2C accounts start 60% below official spend and progress up to 70% off as cumulative top-ups grow." },
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
