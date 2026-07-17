import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { JsonLd } from "@/components/json-ld";
import { absoluteUrl, breadcrumbNode, createPageMetadata, SITE_ORIGIN } from "@/lib/seo";
import { learnPath, resolveArticle } from "@/lib/learn";
import { claudeModelBySlug, claudeModels, formatUsd, modelPath, MODELS_HUB_PATH, priceBest, priceFrom } from "@/lib/models";

type Params = { slug: string };

export function generateStaticParams(): Params[] {
  return claudeModels.map((model) => ({ slug: model.slug }));
}

export async function generateMetadata({ params }: { params: Promise<Params> }): Promise<Metadata> {
  const { slug } = await params;
  const model = claudeModelBySlug[slug];
  if (!model) return {};
  return {
    ...createPageMetadata({ path: modelPath(slug), title: model.title, description: model.description }),
    keywords: model.keywords,
  };
}

export default async function ModelPage({ params }: { params: Promise<Params> }) {
  const { slug } = await params;
  const model = claudeModelBySlug[slug];
  if (!model) notFound();

  const url = absoluteUrl(modelPath(slug));
  const related = model.related
    .map((relatedSlug) => resolveArticle(relatedSlug, "en"))
    .filter((entry): entry is NonNullable<typeof entry> => Boolean(entry));
  const others = claudeModels.filter((entry) => entry.slug !== slug);

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
            <div className="tier-table-wrap">
              <table className="tier-table">
                <thead>
                  <tr>
                    <th>Rate</th>
                    <th>Official Anthropic</th>
                    <th>Here, from (−60%)</th>
                    <th>Here, best (−80%)</th>
                  </tr>
                </thead>
                <tbody>
                  <tr>
                    <td>Input</td>
                    <td>{formatUsd(model.inputPerM)}</td>
                    <td>{priceFrom(model.inputPerM)}</td>
                    <td>{priceBest(model.inputPerM)}</td>
                  </tr>
                  <tr>
                    <td>Output</td>
                    <td>{formatUsd(model.outputPerM)}</td>
                    <td>{priceFrom(model.outputPerM)}</td>
                    <td>{priceBest(model.outputPerM)}</td>
                  </tr>
                  <tr>
                    <td>Cache read</td>
                    <td>{formatUsd(model.cacheReadPerM)}</td>
                    <td>{priceFrom(model.cacheReadPerM)}</td>
                    <td>{priceBest(model.cacheReadPerM)}</td>
                  </tr>
                  <tr>
                    <td>Cache write (5m)</td>
                    <td>{formatUsd(model.cacheWrite5mPerM)}</td>
                    <td>{priceFrom(model.cacheWrite5mPerM)}</td>
                    <td>{priceBest(model.cacheWrite5mPerM)}</td>
                  </tr>
                </tbody>
              </table>
            </div>
            <p className="docs-para">Every request is metered at the official rate first, then your progressive B2C discount (60% at the start, up to 80% as cumulative top-ups grow) is subtracted before it touches your prepaid balance. Context window: {model.context}. Max output: {model.maxOutput}.</p>
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
            <p className="docs-para">Create a free account, generate one key, and point any Anthropic-compatible tool at https://api.apitoken.sale with model ID <code>{model.id}</code>. New accounts include $10 of Claude usage at official API prices — enough to test the model before topping up.</p>
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
            <p>Run {model.name} on the same Anthropic API at up to 80% off — instant key, prepaid balance, card or crypto.</p>
            <div className="hero-cta page-actions">
              <Link className="btn btn-primary" href="/register">Get API key</Link>
              <Link className="btn btn-ghost" href="/docs">Read documentation</Link>
            </div>
          </div>

          <div className="learn-section">
            <h2 className="docs-h3">Other Claude models</h2>
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
