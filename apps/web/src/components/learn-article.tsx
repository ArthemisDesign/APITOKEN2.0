import Link from "next/link";
import {
  articlesForLocale,
  clusterLabels,
  learnHubPath,
  learnPath,
  learnUi,
  resolveArticle,
  type LearnBlock,
  type LearnCluster,
  type Locale,
  type ResolvedArticle,
} from "@/lib/learn";

const CLUSTER_ORDER: LearnCluster[] = ["buy", "free", "integrate", "compare", "explain"];

export function LearnHubView({ locale }: { locale: Locale }) {
  const ui = learnUi[locale];
  const labels = clusterLabels[locale];
  const articles = articlesForLocale(locale)
    .map((slug) => resolveArticle(slug, locale))
    .filter((entry): entry is ResolvedArticle => Boolean(entry));

  return (
    <main className="learn-hub">
      <div className="page-hero">
        <div className="wrap">
          <Link className="auth-back" href="/docs">{ui.docsBack}</Link>
          <span className="eyebrow">{ui.guidesEyebrow}</span>
          <h1>{ui.hubTitle}</h1>
          <p>{ui.hubDescription}</p>
        </div>
      </div>

      <section className="borderless">
        <div className="wrap">
          {CLUSTER_ORDER.map((cluster) => {
            const items = articles.filter((article) => article.cluster === cluster);
            if (items.length === 0) return null;
            return (
              <div className="learn-cluster" key={cluster}>
                <div className="learn-cluster-head">
                  <h2 className="docs-h3">{labels[cluster].label}</h2>
                  <p className="docs-para">{labels[cluster].blurb}</p>
                </div>
                <div className="learn-grid">
                  {items.map((article) => (
                    <Link className="learn-card" href={learnPath(article.slug, locale)} key={article.slug}>
                      <strong>{article.content.h1}</strong>
                      <span>{article.content.description}</span>
                    </Link>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      </section>
    </main>
  );
}

function Block({ block }: { block: LearnBlock }) {
  switch (block.type) {
    case "p":
      return <p className="docs-para">{block.text}</p>;
    case "list":
      return <ul className="prod-list">{block.items.map((item) => <li key={item}>{item}</li>)}</ul>;
    case "steps":
      return (
        <ol className="learn-steps">
          {block.items.map((item, index) => (
            <li key={item}><span className="learn-step-n">{String(index + 1).padStart(2, "0")}</span><span>{item}</span></li>
          ))}
        </ol>
      );
    case "code":
      return <pre className="codebox learn-code">{block.code}</pre>;
    case "note":
      return <p className="docs-notice">{block.text}</p>;
    default:
      return null;
  }
}

export function LearnArticleView({ article }: { article: ResolvedArticle }) {
  const { locale, content } = article;
  const ui = learnUi[locale];
  const cluster = clusterLabels[locale][article.cluster];
  const related = article.related
    .map((slug) => resolveArticle(slug, locale))
    .filter((entry): entry is ResolvedArticle => Boolean(entry));

  return (
    <main className="learn-article">
      <div className="page-hero">
        <div className="wrap">
          <Link className="auth-back" href={learnHubPath(locale)}>{ui.backToHub}</Link>
          <span className="eyebrow">{cluster.label}</span>
          <h1>{content.h1}</h1>
          <p>{content.dek}</p>
        </div>
      </div>

      <section className="borderless">
        <div className="wrap learn-body">
          {content.sections.map((section) => (
            <div className="learn-section" key={section.h2}>
              <h2 className="docs-h3">{section.h2}</h2>
              {section.blocks.map((block, index) => <Block key={index} block={block} />)}
            </div>
          ))}

          {content.faq.length > 0 && (
            <div className="learn-section">
              <h2 className="docs-h3">{ui.faqHeading}</h2>
              <div className="faq">
                {content.faq.map((item) => (
                  <details key={item.q}>
                    <summary>{item.q}<span className="plus" aria-hidden="true">＋</span></summary>
                    <p className="ans">{item.a}</p>
                  </details>
                ))}
              </div>
            </div>
          )}

          <div className="hero-cta page-actions">
            <Link className="btn btn-primary" href="/register">{ui.getKey}</Link>
            <Link className="btn btn-ghost" href="/docs">{ui.readDocs}</Link>
          </div>

          {related.length > 0 && (
            <div className="learn-section">
              <h2 className="docs-h3">{ui.relatedHeading}</h2>
              <div className="learn-related">
                {related.map((entry) => (
                  <Link className="learn-related-card" href={learnPath(entry.slug, locale)} key={entry.slug}>
                    <span className="eyebrow">{clusterLabels[locale][entry.cluster].label}</span>
                    <strong>{entry.content.h1}</strong>
                  </Link>
                ))}
              </div>
            </div>
          )}
        </div>
      </section>
    </main>
  );
}
