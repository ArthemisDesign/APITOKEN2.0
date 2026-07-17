import Link from "next/link";
import {
  articleLocales,
  articlesForLocale,
  clusterLabels,
  learnHubPath,
  learnPath,
  learnUi,
  LOCALES,
  articleUpdatedDate,
  resolveArticle,
  type LearnBlock,
  type LearnCluster,
  type Locale,
  type ResolvedArticle,
} from "@/lib/learn";

// Deterministic pick so the CTA varies across the cluster without being random.
function pickIndex(seed: string, n: number): number {
  let h = 0;
  for (let i = 0; i < seed.length; i += 1) h = (h * 31 + seed.charCodeAt(i)) >>> 0;
  return n > 0 ? h % n : 0;
}

const CLUSTER_ORDER: LearnCluster[] = ["buy", "free", "integrate", "compare", "explain"];

const LANG_LABEL: Record<Locale, string> = { en: "EN", ru: "RU", zh: "中文", ko: "한국어" };

function LearnLangSwitch({ current, locales, hrefFor }: { current: Locale; locales: Locale[]; hrefFor: (locale: Locale) => string }) {
  if (locales.length < 2) return null;
  return (
    <div className="learn-lang" aria-label="Language">
      {locales.map((locale) =>
        locale === current
          ? <span className="learn-lang-on" key={locale} aria-current="true">{LANG_LABEL[locale]}</span>
          : <Link className="learn-lang-off" hrefLang={locale === "zh" ? "zh-CN" : locale} href={hrefFor(locale)} key={locale}>{LANG_LABEL[locale]}</Link>,
      )}
    </div>
  );
}

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
          <div className="learn-hero-top">
            <Link className="auth-back" href="/docs">{ui.docsBack}</Link>
            <LearnLangSwitch current={locale} locales={LOCALES} hrefFor={(target) => learnHubPath(target)} />
          </div>
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
  const ctaText = ui.ctaVariants[pickIndex(article.slug, ui.ctaVariants.length)];
  const seeAlso = related[0];
  const updatedDate = articleUpdatedDate(article.slug).toISOString().slice(0, 10);

  return (
    <main className="learn-article">
      <div className="page-hero">
        <div className="wrap">
          <div className="learn-hero-top">
            <nav className="crumbs" aria-label="Breadcrumb">
              <Link href="/docs">{ui.crumbDocs}</Link>
              <span aria-hidden="true">/</span>
              <Link href={learnHubPath(locale)}>{ui.crumbGuides}</Link>
              <span aria-hidden="true">/</span>
              <span className="crumbs-current">{cluster.label}</span>
            </nav>
            <LearnLangSwitch current={locale} locales={articleLocales(article.slug)} hrefFor={(target) => learnPath(article.slug, target)} />
          </div>
          <span className="eyebrow">{cluster.label}</span>
          <h1>{content.h1}</h1>
          <p>{content.dek}</p>
          <p className="learn-updated"><span className="learn-byline">{ui.byline}</span> · <time dateTime={updatedDate}>{ui.updated} {updatedDate}</time></p>
        </div>
      </div>

      <section className="borderless">
        <div className="wrap learn-body">
          {content.sections.map((section, sectionIndex) => (
            <div className="learn-section" key={section.h2}>
              <h2 className="docs-h3">{section.h2}</h2>
              {section.blocks.map((block, index) => <Block key={index} block={block} />)}
              {sectionIndex === 0 && seeAlso && (
                <p className="learn-seealso">{ui.seeAlso} <Link href={learnPath(seeAlso.slug, locale)}>{seeAlso.content.h1}</Link></p>
              )}
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

          <div className="learn-cta">
            <p>{ctaText}</p>
            <div className="hero-cta page-actions">
              <Link className="btn btn-primary" href="/register">{ui.getKey}</Link>
              <Link className="btn btn-ghost" href="/docs">{ui.readDocs}</Link>
            </div>
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
