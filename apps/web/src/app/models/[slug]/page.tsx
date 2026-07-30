import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { JsonLd } from "@/components/json-ld";
import { absoluteUrl, breadcrumbNode, createPageMetadata, SITE_ORIGIN } from "@/lib/seo";
import { learnPath, resolveArticle } from "@/lib/learn";
import {
  ANTHROPIC_BASE_URL,
  OPENAI_BASE_URL,
  catalogModelBySlug,
  claudeModels,
  formatUsd,
  modelPath,
  MODELS_HUB_PATH,
  openaiModels,
  priceFrom,
  type CatalogModel,
} from "@/lib/models";

type Params = { slug: string };

export function generateStaticParams(): Params[] {
  return [...claudeModels, ...openaiModels].map((model) => ({ slug: model.slug }));
}

export async function generateMetadata({ params }: { params: Promise<Params> }): Promise<Metadata> {
  const { slug } = await params;
  const model = catalogModelBySlug[slug];
  if (!model) return {};
  return {
    ...createPageMetadata({ path: modelPath(slug), title: model.title, description: model.description }),
    keywords: model.keywords,
  };
}

function priceRowsFor(model: CatalogModel) {
  if (model.provider === "anthropic") {
    return [
      { rate: "Input", official: formatUsd(model.inputPerM), yours: priceFrom(model.inputPerM) },
      { rate: "Output", official: formatUsd(model.outputPerM), yours: priceFrom(model.outputPerM) },
      { rate: "Cache read", official: formatUsd(model.cacheReadPerM), yours: priceFrom(model.cacheReadPerM) },
      { rate: "Cache write (5m)", official: formatUsd(model.cacheWrite5mPerM), yours: priceFrom(model.cacheWrite5mPerM) },
    ];
  }
  return [
    { rate: "Input", official: formatUsd(model.inputPerM), yours: priceFrom(model.inputPerM) },
    { rate: "Cached input", official: formatUsd(model.cachedInputPerM), yours: priceFrom(model.cachedInputPerM) },
    { rate: "Cache write", official: formatUsd(model.cacheWritePerM), yours: priceFrom(model.cacheWritePerM) },
    { rate: "Output", official: formatUsd(model.outputPerM), yours: priceFrom(model.outputPerM) },
  ];
}

const providerCopy = {
  anthropic: {
    officialCol: "Official Anthropic",
    othersHeading: "Other Claude models",
    howTo: (id: string) => <>Create a free account, generate one key, and point any Anthropic-compatible tool at {ANTHROPIC_BASE_URL} with model ID <code>{id}</code>. New accounts include $10 of API usage at official prices — enough to test the model before topping up.</>,
    cta: (name: string) => `Run ${name} on the same Anthropic API at a flat 50% off — instant key, prepaid balance, card or crypto.`,
  },
  openai: {
    officialCol: "Official OpenAI",
    othersHeading: "Other GPT models",
    howTo: (id: string) => <>Create a free account, generate one key, and point any OpenAI-compatible tool at {OPENAI_BASE_URL} with model ID <code>{id}</code> — Responses and Chat Completions both work, authenticated with <code>Authorization: Bearer</code>. New accounts include $10 of API usage at official prices — enough to test the model before topping up.</>,
    cta: (name: string) => `Run ${name} on the OpenAI-compatible API at a flat 50% off — instant key, prepaid balance, card or crypto.`,
  },
} as const;

export default async function ModelPage({ params }: { params: Promise<Params> }) {
  const { slug } = await params;
  const model = catalogModelBySlug[slug];
  if (!model) notFound();

  const copy = providerCopy[model.provider];
  const url = absoluteUrl(modelPath(slug));
  const related = model.related
    .map((relatedSlug) => resolveArticle(relatedSlug, "en"))
    .filter((entry): entry is NonNullable<typeof entry> => Boolean(entry));
  const siblings = model.provider === "anthropic" ? claudeModels : openaiModels;
  const others = siblings.filter((entry) => entry.slug !== slug);
  const priceRows = priceRowsFor(model);

  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([
        { name: "Home", path: "/" },
        { name: "Models", path: MODELS_HUB_PATH },
        { name: model.name, path: modelPath(slug) },
      ]),
      {
        "@type": "TechArticle",
        "@id": `${url}#article`,
        headline: model.title,
        description: model.description,
        url,
        mainEntityOfPage: url,
        inLanguage: "en",
        keywords: model.keywords.join(", "),
        author: { "@id": `${SITE_ORIGIN}/#organization` },
        publisher: { "@id": `${SITE_ORIGIN}/#organization` },
      },
      {
        "@type": "FAQPage",
        "@id": `${url}#faq`,
        mainEntity: model.faq.map((item) => ({
          "@type": "Question",
          name: item.q,
          acceptedAnswer: { "@type": "Answer", text: item.a },
        })),
      },
    ],
  };

  return (
    <><JsonLd data={structuredData} /><main className="learn-article">
      <div className="page-hero">
        <div className="wrap">
          <div className="learn-hero-top">
            <nav className="crumbs" aria-label="Breadcrumb">
              <Link href={MODELS_HUB_PATH}>Models</Link>
              <span aria-hidden="true">/</span>
              <span className="crumbs-current">{model.tier}</span>
            </nav>
          </div>
          <span className="eyebrow">{model.id}</span>
          <h1>{model.name} API — price per token</h1>
          <p>{model.dek}</p>
        </div>
      </div>

      <section className="borderless">
        <div className="wrap learn-body">
          <div className="learn-section">
            <h2 className="docs-h3">Pricing per 1M tokens</h2>
            <div className="tier-table-wrap model-pricing-table-wrap">
              <table className="tier-table">
                <thead>
                  <tr>
                    <th>Rate</th>
                    <th>{copy.officialCol}</th>
                    <th>Your price (−50%)</th>
                  </tr>
                </thead>
                <tbody>
                  {priceRows.map((row) => <tr key={row.rate}>
                    <td>{row.rate}</td>
                    <td>{row.official}</td>
                    <td>{row.yours}</td>
                  </tr>)}
                </tbody>
              </table>
            </div>
            <div className="model-pricing-mobile" aria-label="Pricing per 1M tokens">
              {priceRows.map((row) => <article className="model-pricing-card" key={row.rate}>
                <h3>{row.rate}</h3>
                <dl>
                  <div><dt>{copy.officialCol}</dt><dd>{row.official}</dd></div>
                  <div><dt>Your price (−50%)</dt><dd>{row.yours}</dd></div>
                </dl>
              </article>)}
            </div>
            <p className="docs-para">Every request is metered at the official rate first, then the flat 50% discount — the same for every account, with no conditions — is subtracted before it touches your prepaid balance. Context window: {model.context}. Max output: {model.maxOutput}.{model.provider === "openai" ? ` Reasoning efforts: ${model.efforts.join(", ")}.` : ""}</p>
          </div>

          <div className="learn-section">
            <h2 className="docs-h3">Best for</h2>
            <ul className="prod-list">
              {model.bestFor.map((item) => <li key={item}>{item}</li>)}
            </ul>
          </div>

          <div className="learn-section">
            <h2 className="docs-h3">Good to know</h2>
            <ul className="prod-list">
              {model.notes.map((item) => <li key={item}>{item}</li>)}
            </ul>
          </div>

          <div className="learn-section">
            <h2 className="docs-h3">How to use {model.name}</h2>
            <p className="docs-para">{copy.howTo(model.id)}</p>
          </div>

          {model.faq.length > 0 && (
            <div className="learn-section">
              <h2 className="docs-h3">Frequently asked questions</h2>
              <div className="faq">
                {model.faq.map((item) => (
                  <details key={item.q}>
                    <summary>{item.q}<span className="plus" aria-hidden="true">＋</span></summary>
                    <p className="ans">{item.a}</p>
                  </details>
                ))}
              </div>
            </div>
          )}

          <div className="learn-cta">
            <p>{copy.cta(model.name)}</p>
            <div className="hero-cta page-actions">
              <Link className="btn btn-primary" href="/register">Get API key</Link>
              <Link className="btn btn-ghost" href="/docs">Read documentation</Link>
            </div>
          </div>

          <div className="learn-section">
            <h2 className="docs-h3">{copy.othersHeading}</h2>
            <div className="learn-related">
              {others.map((entry) => (
                <Link className="learn-related-card" href={modelPath(entry.slug)} key={entry.slug}>
                  <span className="eyebrow">{entry.tier}</span>
                  <strong>{entry.name}</strong>
                </Link>
              ))}
            </div>
          </div>

          {related.length > 0 && (
            <div className="learn-section">
              <h2 className="docs-h3">Related guides</h2>
              <div className="learn-related">
                {related.map((entry) => (
                  <Link className="learn-related-card" href={learnPath(entry.slug, "en")} key={entry.slug}>
                    <span className="eyebrow">Guide</span>
                    <strong>{entry.content.h1}</strong>
                  </Link>
                ))}
              </div>
            </div>
          )}
        </div>
      </section>
    </main></>
  );
}
