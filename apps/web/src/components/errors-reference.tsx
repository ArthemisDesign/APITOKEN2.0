import Link from "next/link";
import { JsonLd } from "@/components/json-ld";
import { ErrorAnchorBeacon } from "@/components/error-anchor-beacon";
import { apiErrorsRu } from "@/lib/api-errors-ru";
import { errorsUi, resolveApiErrors, type ErrorLocale } from "@/lib/api-errors";
import { absoluteUrl, breadcrumbNode, SITE_NAME, SITE_ORIGIN } from "@/lib/seo";

const EN_PATH = "/docs/errors";

/**
 * The error reference, rendered from the shared catalog in the requested locale.
 * Both /docs/errors and /ru/docs/errors use this, so the two versions cannot drift.
 */
export function ErrorsReference({ locale }: { locale: ErrorLocale }) {
  const ui = errorsUi[locale];
  const entries = resolveApiErrors(locale, apiErrorsRu);
  const path = locale === "ru" ? `/ru${EN_PATH}` : EN_PATH;
  const url = absoluteUrl(path);
  const home = locale === "ru" ? { name: "Главная", path: "/ru" } : { name: "Home", path: "/" };
  const docsCrumb = locale === "ru" ? { name: "Документация", path: "/ru/docs" } : { name: "Docs", path: "/docs" };

  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([home, docsCrumb, { name: ui.allCodes, path }]),
      {
        "@type": "TechArticle",
        "@id": `${url}#article`,
        url,
        headline: ui.title,
        description: ui.description,
        inLanguage: locale,
        isPartOf: { "@id": `${SITE_ORIGIN}/#website` },
        publisher: { "@id": `${SITE_ORIGIN}/#organization` },
        about: { "@id": `${SITE_ORIGIN}/#organization` },
      },
      {
        "@type": "FAQPage",
        "@id": `${url}#faq`,
        inLanguage: locale,
        mainEntity: entries.map((entry) => ({
          "@type": "Question",
          name:
            locale === "ru"
              ? `Что означает «${entry.message}» в Claude API?`
              : `What does "${entry.message}" mean in the Claude API?`,
          acceptedAnswer: {
            "@type": "Answer",
            text: `HTTP ${entry.status}, error type ${entry.type}. ${entry.causes[0]} ${entry.fixes[0]}`,
          },
        })),
      },
    ],
  };

  return (
    <>
      <JsonLd data={structuredData} />
      <ErrorAnchorBeacon />
      <main className="learn-article">
        <div className="page-hero">
          <div className="wrap">
            <span className="eyebrow">{ui.eyebrow}</span>
            <h1>{ui.title}</h1>
            <p>{ui.description}</p>
          </div>
        </div>

        <section className="borderless">
          <div className="wrap learn-body">
            <div className="learn-section">
              <p className="docs-para">{ui.envelopeIntro}</p>
              <pre className="codebox learn-code">
                <code>{`{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}`}</code>
              </pre>
              <p className="docs-para">{ui.envelopeNote}</p>
            </div>

            <div className="learn-section">
              <h2 className="docs-h3">{ui.allCodes}</h2>
              <div className="tier-table-wrap">
                <table className="tier-table">
                  <thead>
                    <tr>
                      <th>{ui.colStatus}</th>
                      <th>{ui.colType}</th>
                      <th>{ui.colMeaning}</th>
                      <th>{ui.colRetry}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {entries.map((entry) => (
                      <tr key={entry.code}>
                        <td>
                          <a href={`#e-${entry.code}`}>{entry.status === 0 ? "—" : entry.status}</a>
                        </td>
                        <td>
                          <code>{entry.type}</code>
                        </td>
                        <td>{entry.localeTitle.replace(/^\d+\s+—\s+/, "")}</td>
                        <td>{entry.retryable ? ui.retryYes : ui.retryNo}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>

            {entries.map((entry) => (
              <div className="learn-section" key={entry.code} id={`e-${entry.code}`}>
                <h2 className="docs-h3">{entry.localeTitle}</h2>
                <pre className="codebox learn-code">
                  <code>
                    {entry.status === 0
                      ? entry.message
                      : `HTTP ${entry.status}
{"type":"error","error":{"type":"${entry.type}","message":${JSON.stringify(entry.message)}}}`}
                  </code>
                </pre>

                <h3 className="docs-h3">{ui.why}</h3>
                <ul className="prod-list">
                  {entry.causes.map((cause) => (
                    <li key={cause}>{cause}</li>
                  ))}
                </ul>

                <h3 className="docs-h3">{ui.how}</h3>
                <ul className="prod-list">
                  {entry.fixes.map((fix) => (
                    <li key={fix}>{fix}</li>
                  ))}
                </ul>

                {entry.snippet ? (
                  <>
                    <h3 className="docs-h3">{entry.snippet.label}</h3>
                    <pre className="codebox learn-code">
                      <code>{entry.snippet.code}</code>
                    </pre>
                  </>
                ) : null}

                {entry.alsoSearchedAs?.length ? (
                  <>
                    <h3 className="docs-h3">{ui.variants}</h3>
                    <ul className="prod-list">
                      {entry.alsoSearchedAs.map((variant) => (
                        <li key={variant}>
                          <code>{variant}</code>
                        </li>
                      ))}
                    </ul>
                  </>
                ) : null}

                <p className="docs-para">
                  <small>
                    {ui.shortLink}: <code>https://apitoken.sale/e/{entry.code}</code> ·{" "}
                    {entry.surface === "apitoken"
                      ? ui.originGateway
                      : entry.status === 0
                        ? ui.originSubscription
                        : ui.originShared}
                  </small>
                </p>
              </div>
            ))}

            <div className="learn-section">
              <h2 className="docs-h3">{ui.stuckHeading}</h2>
              <p className="docs-para">{ui.stuckBody}</p>
              <div className="hero-cta page-actions">
                <Link className="btn btn-primary" href={locale === "ru" ? "/ru/docs" : "/docs"}>
                  {ui.ctaDocs}
                </Link>
                <Link className="btn btn-ghost" href={locale === "ru" ? "/ru/support" : "/contacts"}>
                  {ui.ctaSupport}
                </Link>
              </div>
              <p className="docs-para">
                {SITE_NAME}
                {locale === "ru"
                  ? " отдаёт стандартный Anthropic Messages API, поэтому каждая не-шлюзовая ошибка здесь ведёт себя ровно так же, как против api.anthropic.com. "
                  : " serves the standard Anthropic Messages API, so every non-gateway error on this page behaves exactly as it does against api.anthropic.com. "}
                <Link href={locale === "ru" ? "/ru/docs/learn/claude-api-rate-limits" : "/docs/learn/claude-api-rate-limits"}>
                  {ui.seeAlso}
                </Link>
              </p>
            </div>
          </div>
        </section>
      </main>
    </>
  );
}
