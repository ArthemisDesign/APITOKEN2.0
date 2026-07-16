import Link from "next/link";
import { clusterLabels, learnArticlesBySlug, learnPath, LEARN_HUB_PATH, type LearnArticle, type LearnBlock } from "@/lib/learn";

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

export function LearnArticleView({ article }: { article: LearnArticle }) {
  const cluster = clusterLabels[article.cluster];
  const related = article.related
    .map((slug) => learnArticlesBySlug[slug])
    .filter((entry): entry is LearnArticle => Boolean(entry));

  return (
    <main className="learn-article">
      <div className="page-hero">
        <div className="wrap">
          <Link className="auth-back" href={LEARN_HUB_PATH}>← Claude API guides</Link>
          <span className="eyebrow">{cluster.label}</span>
          <h1>{article.h1}</h1>
          <p>{article.dek}</p>
        </div>
      </div>

      <section className="borderless">
        <div className="wrap learn-body">
          {article.sections.map((section) => (
            <div className="learn-section" key={section.h2}>
              <h2 className="docs-h3">{section.h2}</h2>
              {section.blocks.map((block, index) => <Block key={index} block={block} />)}
            </div>
          ))}

          {article.faq.length > 0 && (
            <div className="learn-section">
              <h2 className="docs-h3">Frequently asked questions</h2>
              <div className="faq">
                {article.faq.map((item) => (
                  <details key={item.q}>
                    <summary>{item.q}<span className="plus" aria-hidden="true">＋</span></summary>
                    <p className="ans">{item.a}</p>
                  </details>
                ))}
              </div>
            </div>
          )}

          <div className="hero-cta page-actions">
            <Link className="btn btn-primary" href="/register">Get API key</Link>
            <Link className="btn btn-ghost" href="/docs">Read documentation</Link>
          </div>

          {related.length > 0 && (
            <div className="learn-section">
              <h2 className="docs-h3">Related guides</h2>
              <div className="learn-related">
                {related.map((entry) => (
                  <Link className="learn-related-card" href={learnPath(entry.slug)} key={entry.slug}>
                    <span className="eyebrow">{clusterLabels[entry.cluster].label}</span>
                    <strong>{entry.h1}</strong>
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
