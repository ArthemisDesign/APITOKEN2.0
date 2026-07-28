import type { Metadata } from "next";
import Link from "next/link";
import { Suspense } from "react";
import { JsonLd } from "@/components/json-ld";
import { ErrorAnchorBeacon } from "@/components/error-anchor-beacon";
import { API_ERRORS } from "@/lib/api-errors";
import { absoluteUrl, breadcrumbNode, createPageMetadata, SITE_NAME, SITE_ORIGIN } from "@/lib/seo";

const TITLE = "Claude API Error Codes";
const DESCRIPTION =
  "Every Claude API error explained: 401 invalid x-api-key, 429 rate_limit_error, 529 Overloaded, 413 request_too_large, and the 400s newer models introduced. Exact response text, cause and fix for each.";

export const metadata: Metadata = {
  ...createPageMetadata({ path: "/docs/errors", title: TITLE, description: DESCRIPTION }),
  keywords: [
    "claude api error codes",
    "anthropic api errors",
    "invalid x-api-key",
    "rate_limit_error",
    "overloaded_error",
    "authentication_error",
    "claude api 429",
    "claude api 401",
  ],
};

export default function ErrorsPage() {
  const url = absoluteUrl("/docs/errors");

  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([
        { name: "Home", path: "/" },
        { name: "Docs", path: "/docs" },
        { name: "Error codes", path: "/docs/errors" },
      ]),
      {
        "@type": "TechArticle",
        "@id": `${url}#article`,
        url,
        headline: TITLE,
        description: DESCRIPTION,
        inLanguage: "en",
        isPartOf: { "@id": `${SITE_ORIGIN}/#website` },
        publisher: { "@id": `${SITE_ORIGIN}/#organization` },
        about: { "@id": `${SITE_ORIGIN}/#organization` },
      },
      {
        "@type": "FAQPage",
        "@id": `${url}#faq`,
        mainEntity: API_ERRORS.map((entry) => ({
          "@type": "Question",
          name: `What does "${entry.message}" mean in the Claude API?`,
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
      <Suspense fallback={null}>
        <ErrorAnchorBeacon />
      </Suspense>
      <main className="learn-article">
        <div className="page-hero">
          <div className="wrap">
            <span className="eyebrow">Reference</span>
            <h1>{TITLE}</h1>
            <p>{DESCRIPTION}</p>
          </div>
        </div>

        <section className="borderless">
          <div className="wrap learn-body">
            <div className="learn-section">
              <p className="docs-para">
                Every error is returned as JSON with the same envelope, so you can branch on{" "}
                <code>error.type</code> without parsing the message text:
              </p>
              <pre className="codebox learn-code">
                <code>{`{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}`}</code>
              </pre>
              <p className="docs-para">
                Match on the HTTP status and <code>error.type</code>, never on the message
                string — messages are prose and can be reworded, while the type is a contract.
                In the official SDKs this means catching the typed exception classes
                (<code>RateLimitError</code>, <code>AuthenticationError</code>, and so on) rather
                than inspecting text. This page is written the other way round only because
                the message is what you have in front of you when something breaks.
              </p>
            </div>

            <div className="learn-section">
              <h2 className="docs-h3">All codes</h2>
              <div className="tier-table-wrap">
                <table className="tier-table">
                  <thead>
                    <tr>
                      <th>Status</th>
                      <th>error.type</th>
                      <th>Meaning</th>
                      <th>Retry?</th>
                    </tr>
                  </thead>
                  <tbody>
                    {API_ERRORS.map((entry) => (
                      <tr key={entry.code}>
                        <td>
                          <a href={`#e-${entry.code}`}>{entry.status === 0 ? "—" : entry.status}</a>
                        </td>
                        <td>
                          <code>{entry.type}</code>
                        </td>
                        <td>{entry.title.replace(/^\d+\s+—\s+/, "")}</td>
                        <td>{entry.retryable ? "Yes, back off" : "No"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>

            {API_ERRORS.map((entry) => (
              <div className="learn-section" key={entry.code} id={`e-${entry.code}`}>
                <h2 className="docs-h3">{entry.title}</h2>
                <pre className="codebox learn-code">
                  <code>
                    {entry.status === 0
                      ? entry.message
                      : `HTTP ${entry.status}
{"type":"error","error":{"type":"${entry.type}","message":${JSON.stringify(entry.message)}}}`}
                  </code>
                </pre>

                <h3 className="docs-h3">Why it happens</h3>
                <ul className="prod-list">
                  {entry.causes.map((cause) => (
                    <li key={cause}>{cause}</li>
                  ))}
                </ul>

                <h3 className="docs-h3">How to fix it</h3>
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
                    <h3 className="docs-h3">Other forms of the same failure</h3>
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
                    Short link: <code>https://apitoken.sale/e/{entry.code}</code>
                    {entry.surface === "apitoken"
                      ? " · This response is specific to this gateway — the Anthropic API has no equivalent."
                      : entry.status === 0
                        ? " · Comes from Anthropic's own apps and subscription plans, not from this gateway."
                        : " · Identical on api.anthropic.com and on this gateway."}
                  </small>
                </p>
              </div>
            ))}

            <div className="learn-section">
              <h2 className="docs-h3">Still stuck?</h2>
              <p className="docs-para">
                If a request fails in a way this page does not cover, send us the endpoint, the
                masked key id, the HTTP status and the response body. Never send the full key.
              </p>
              <div className="hero-cta page-actions">
                <Link className="btn btn-primary" href="/docs">
                  API docs
                </Link>
                <Link className="btn btn-ghost" href="/contacts">
                  Contact support
                </Link>
              </div>
              <p className="docs-para">
                {SITE_NAME} serves the standard Anthropic Messages API, so every non-gateway
                error on this page behaves exactly as it does against api.anthropic.com. See
                also the <Link href="/docs/learn/claude-api-rate-limits">rate limits guide</Link>{" "}
                and <Link href="/docs/learn/anthropic-sdk-base-url">how to point an SDK at a custom base URL</Link>.
              </p>
            </div>
          </div>
        </section>
      </main>
    </>
  );
}
