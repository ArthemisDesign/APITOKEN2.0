import type { Metadata } from "next";
import Link from "next/link";
import { JsonLd } from "@/components/json-ld";
import { absoluteUrl, breadcrumbNode, DEFAULT_OG_IMAGE, SITE_NAME, SITE_ORIGIN } from "@/lib/seo";
import {
  findTool,
  findToolError,
  resolveToolError,
  resolveToolInfo,
  TOOL_ERROR_TOOLS,
  toolErrorPath,
  toolErrors,
  toolErrorsIndexPath,
  toolErrorsUiEn,
  toolHubPath,
  TOOL_ERRORS_INDEX,
  type ResolvedToolError,
  type ToolErrorLocale,
  type ToolErrorsUi,
  type ToolErrorTranslations,
  type ToolInfo,
  type ToolSlug,
} from "@/lib/tool-errors";
import { toolErrorsRu } from "@/lib/tool-errors-ru";
import { toolErrorsZh } from "@/lib/tool-errors-zh";
import { toolErrorsKo } from "@/lib/tool-errors-ko";

// The whole cluster shipped together, so one launch date for every page.
// Bump per entry later if an entry is materially revised.
export const TOOL_ERRORS_LAUNCH = new Date("2026-07-28T00:00:00.000Z");

const TRANSLATIONS: Partial<Record<ToolErrorLocale, ToolErrorTranslations>> = {
  ru: toolErrorsRu,
  zh: toolErrorsZh,
  ko: toolErrorsKo,
};

function ui(locale: ToolErrorLocale): ToolErrorsUi {
  return TRANSLATIONS[locale]?.ui ?? toolErrorsUiEn;
}

function translations(locale: ToolErrorLocale): ToolErrorTranslations | undefined {
  return TRANSLATIONS[locale];
}

const OG_LOCALES: Record<ToolErrorLocale, string> = {
  en: "en_US",
  ru: "ru_RU",
  zh: "zh_CN",
  ko: "ko_KR",
};

const HOME: Record<ToolErrorLocale, { name: string; path: string }> = {
  en: { name: "Home", path: "/" },
  ru: { name: "Главная", path: "/ru" },
  // zh/ko have no localized marketing root — the crumb points at the English home,
  // same convention as the learn cluster.
  zh: { name: "首页", path: "/" },
  ko: { name: "홈", path: "/" },
};

const INDEX_CRUMB: Record<ToolErrorLocale, string> = {
  en: "Errors",
  ru: "Ошибки",
  zh: "错误",
  ko: "오류",
};

function languagesFor(pathFor: (locale: ToolErrorLocale) => string): Record<string, string> {
  return {
    en: absoluteUrl(pathFor("en")),
    ru: absoluteUrl(pathFor("ru")),
    zh: absoluteUrl(pathFor("zh")),
    ko: absoluteUrl(pathFor("ko")),
    "x-default": absoluteUrl(pathFor("en")),
  };
}

function buildMetadata(
  locale: ToolErrorLocale,
  title: string,
  description: string,
  pathFor: (locale: ToolErrorLocale) => string,
  kind: "website" | "article",
): Metadata {
  const path = pathFor(locale);
  const url = absoluteUrl(path);
  const socialTitle = `${title} | ${SITE_NAME}`;
  return {
    // The cluster's titles are self-contained query matches; the layout template
    // would push them past the SERP truncation point, so they ship absolute.
    title: { absolute: socialTitle },
    description,
    alternates: { canonical: url, languages: languagesFor(pathFor) },
    openGraph: {
      type: kind,
      locale: OG_LOCALES[locale],
      url,
      siteName: SITE_NAME,
      title: socialTitle,
      description,
      images: [{ url: DEFAULT_OG_IMAGE, width: 1200, height: 630, alt: `${SITE_NAME} — Claude API` }],
      ...(kind === "article"
        ? { publishedTime: TOOL_ERRORS_LAUNCH.toISOString(), modifiedTime: TOOL_ERRORS_LAUNCH.toISOString() }
        : {}),
    },
    twitter: { card: "summary_large_image", title: socialTitle, description, images: [DEFAULT_OG_IMAGE] },
  };
}

// --- Metadata builders ------------------------------------------------------

export function toolErrorsIndexMetadata(locale: ToolErrorLocale): Metadata {
  const t = translations(locale)?.index ?? TOOL_ERRORS_INDEX;
  return buildMetadata(locale, t.title, t.description, (l) => toolErrorsIndexPath(l), "website");
}

export function toolHubMetadata(locale: ToolErrorLocale, toolSlug: string): Metadata | undefined {
  const tool = findTool(toolSlug);
  if (!tool) return undefined;
  const t = resolveToolInfo(tool, locale, translations(locale));
  return buildMetadata(locale, t.title, t.description, (l) => toolHubPath(tool.slug, l), "website");
}

export function toolErrorMetadata(locale: ToolErrorLocale, toolSlug: string, slug: string): Metadata | undefined {
  const entry = findToolError(toolSlug, slug);
  if (!entry) return undefined;
  const resolved = resolveToolError(entry, locale, translations(locale));
  return buildMetadata(
    locale,
    resolved.localeTitle,
    resolved.localeDescription,
    (l) => toolErrorPath(entry.tool, entry.slug, l),
    "article",
  );
}

// --- Shared render helpers --------------------------------------------------

function heroBlock(entry: ResolvedToolError): string {
  const primary = entry.searchStrings[0];
  return entry.status > 0 && primary.startsWith("{") ? `HTTP ${entry.status}\n${primary}` : primary;
}

function CtaBand({ locale, strings }: { locale: ToolErrorLocale; strings: ToolErrorsUi }) {
  const docsPath = locale === "ru" ? "/ru/docs" : "/docs";
  return (
    <div className="learn-section">
      <h2 className="docs-h3">{strings.ctaHeading}</h2>
      <p className="docs-para">{strings.ctaBody}</p>
      <div className="hero-cta page-actions">
        <Link className="btn btn-primary" href={locale === "ru" ? "/ru/register" : "/register"}>
          {strings.ctaButton}
        </Link>
        <Link className="btn btn-ghost" href={docsPath}>
          {strings.ctaDocs}
        </Link>
      </div>
    </div>
  );
}

// --- Pages ------------------------------------------------------------------

export function ToolErrorsIndexPage({ locale }: { locale: ToolErrorLocale }) {
  const strings = ui(locale);
  const t = translations(locale)?.index ?? TOOL_ERRORS_INDEX;
  const path = toolErrorsIndexPath(locale);
  const url = absoluteUrl(path);
  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([HOME[locale], { name: INDEX_CRUMB[locale], path }]),
      {
        "@type": "CollectionPage",
        "@id": `${url}#collection`,
        url,
        name: t.title,
        description: t.description,
        inLanguage: locale,
        isPartOf: { "@id": `${SITE_ORIGIN}/#website` },
        hasPart: TOOL_ERROR_TOOLS.map((tool) => ({
          "@type": "WebPage",
          "@id": absoluteUrl(toolHubPath(tool.slug, locale)),
          name: resolveToolInfo(tool, locale, translations(locale)).title,
        })),
      },
    ],
  };

  return (
    <>
      <JsonLd data={structuredData} />
      <main className="learn-article">
        <div className="page-hero">
          <div className="wrap">
            <span className="eyebrow">{strings.eyebrow}</span>
            <h1>{t.title}</h1>
            <p>{t.intro}</p>
          </div>
        </div>
        <section className="borderless">
          <div className="wrap learn-body">
            {TOOL_ERROR_TOOLS.map((tool) => {
              const info = resolveToolInfo(tool, locale, translations(locale));
              return (
                <div className="learn-section" key={tool.slug}>
                  <h2 className="docs-h3">
                    <Link href={toolHubPath(tool.slug, locale)}>{strings.errorsIn.replace("{tool}", tool.name)}</Link>
                  </h2>
                  <p className="docs-para">{info.description}</p>
                  <ul className="prod-list">
                    {toolErrors(tool.slug).map((entry) => {
                      const resolved = resolveToolError(entry, locale, translations(locale));
                      return (
                        <li key={entry.slug}>
                          <Link href={toolErrorPath(tool.slug, entry.slug, locale)}>{resolved.localeTitle}</Link>
                        </li>
                      );
                    })}
                  </ul>
                </div>
              );
            })}
            <div className="learn-section">
              <h2 className="docs-h3">
                <Link href={locale === "ru" ? "/ru/docs/errors" : "/docs/errors"}>{strings.fullReference}</Link>
              </h2>
              <p className="docs-para">{strings.fullReferenceBlurb}</p>
            </div>
            <CtaBand locale={locale} strings={strings} />
          </div>
        </section>
      </main>
    </>
  );
}

export function ToolErrorsHubPage({ locale, tool }: { locale: ToolErrorLocale; tool: ToolInfo }) {
  const strings = ui(locale);
  const info = resolveToolInfo(tool, locale, translations(locale));
  const path = toolHubPath(tool.slug, locale);
  const url = absoluteUrl(path);
  const entries = toolErrors(tool.slug).map((entry) => resolveToolError(entry, locale, translations(locale)));

  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([
        HOME[locale],
        { name: INDEX_CRUMB[locale], path: toolErrorsIndexPath(locale) },
        { name: tool.name, path },
      ]),
      {
        "@type": "CollectionPage",
        "@id": `${url}#collection`,
        url,
        name: info.title,
        description: info.description,
        inLanguage: locale,
        isPartOf: { "@id": `${SITE_ORIGIN}/#website` },
        hasPart: entries.map((entry) => ({
          "@type": "TechArticle",
          "@id": absoluteUrl(toolErrorPath(tool.slug, entry.slug, locale)),
          headline: entry.localeTitle,
        })),
      },
    ],
  };

  return (
    <>
      <JsonLd data={structuredData} />
      <main className="learn-article">
        <div className="page-hero">
          <div className="wrap">
            <span className="eyebrow">{strings.eyebrow}</span>
            <h1>{info.title}</h1>
            <p>{info.intro}</p>
          </div>
        </div>
        <section className="borderless">
          <div className="wrap learn-body">
            <div className="learn-section">
              <div className="tier-table-wrap">
                <table className="tier-table">
                  <thead>
                    <tr>
                      <th>{strings.colError}</th>
                      <th>{strings.colMeaning}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {entries.map((entry) => (
                      <tr key={entry.slug}>
                        <td>
                          <Link href={toolErrorPath(tool.slug, entry.slug, locale)}>
                            <code>{entry.searchStrings[0].length > 64 ? `${entry.searchStrings[0].slice(0, 64)}…` : entry.searchStrings[0]}</code>
                          </Link>
                        </td>
                        <td>{entry.localeDescription}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
            {entries.map((entry) => (
              <div className="learn-section" key={entry.slug}>
                <h2 className="docs-h3">
                  <Link href={toolErrorPath(tool.slug, entry.slug, locale)}>{entry.localeTitle}</Link>
                </h2>
                <p className="docs-para">{entry.localeDescription}</p>
              </div>
            ))}
            <div className="learn-section">
              <p className="docs-para">
                <Link href={toolErrorsIndexPath(locale)}>{strings.allTools}</Link>
                {" · "}
                <Link href={locale === "ru" ? "/ru/docs/errors" : "/docs/errors"}>{strings.fullReference}</Link>
                {tool.guidePath ? (
                  <>
                    {" · "}
                    <Link href={locale === "ru" ? `/ru${tool.guidePath}` : tool.guidePath}>{strings.setupGuide}</Link>
                  </>
                ) : null}
              </p>
            </div>
            <CtaBand locale={locale} strings={strings} />
          </div>
        </section>
      </main>
    </>
  );
}

export function ToolErrorArticlePage({
  locale,
  tool,
  entry,
}: {
  locale: ToolErrorLocale;
  tool: ToolInfo;
  entry: ResolvedToolError;
}) {
  const strings = ui(locale);
  const path = toolErrorPath(tool.slug, entry.slug, locale);
  const url = absoluteUrl(path);

  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([
        HOME[locale],
        { name: INDEX_CRUMB[locale], path: toolErrorsIndexPath(locale) },
        { name: tool.name, path: toolHubPath(tool.slug, locale) },
        { name: entry.localeTitle, path },
      ]),
      {
        "@type": "TechArticle",
        "@id": `${url}#article`,
        url,
        headline: entry.localeTitle,
        description: entry.localeDescription,
        inLanguage: locale,
        datePublished: TOOL_ERRORS_LAUNCH.toISOString(),
        dateModified: TOOL_ERRORS_LAUNCH.toISOString(),
        isPartOf: { "@id": `${SITE_ORIGIN}/#website` },
        publisher: { "@id": `${SITE_ORIGIN}/#organization` },
      },
      {
        "@type": "FAQPage",
        "@id": `${url}#faq`,
        inLanguage: locale,
        mainEntity: entry.faq.map((item) => ({
          "@type": "Question",
          name: item.q,
          acceptedAnswer: { "@type": "Answer", text: item.a },
        })),
      },
    ],
  };

  return (
    <>
      <JsonLd data={structuredData} />
      <main className="learn-article">
        <div className="page-hero">
          <div className="wrap">
            <span className="eyebrow">
              {strings.eyebrow} · {tool.name}
            </span>
            <h1>{entry.localeTitle}</h1>
            <p>{entry.localeDescription}</p>
          </div>
        </div>
        <section className="borderless">
          <div className="wrap learn-body">
            <div className="learn-section">
              <h2 className="docs-h3">{strings.whatYouSee}</h2>
              <pre className="codebox learn-code">
                <code>{heroBlock(entry)}</code>
              </pre>
            </div>

            <div className="learn-section">
              <h2 className="docs-h3">{strings.why}</h2>
              <ul className="prod-list">
                {entry.causes.map((cause) => (
                  <li key={cause}>{cause}</li>
                ))}
              </ul>
            </div>

            <div className="learn-section">
              <h2 className="docs-h3">{strings.how}</h2>
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
            </div>

            {entry.searchStrings.length > 1 ? (
              <div className="learn-section">
                <h2 className="docs-h3">{strings.alsoSearched}</h2>
                <ul className="prod-list">
                  {entry.searchStrings.slice(1).map((variant) => (
                    <li key={variant}>
                      <code>{variant}</code>
                    </li>
                  ))}
                </ul>
              </div>
            ) : null}

            <div className="learn-section">
              <h2 className="docs-h3">{strings.faqHeading}</h2>
              <div className="faq">
                {entry.faq.map((item) => (
                  <details key={item.q}>
                    <summary>
                      {item.q}
                      <span className="plus">+</span>
                    </summary>
                    <div className="ans">{item.a}</div>
                  </details>
                ))}
              </div>
            </div>

            <div className="learn-section">
              <p className="docs-para">
                <Link href={toolHubPath(tool.slug, locale)}>{strings.backToTool.replace("{tool}", tool.name)}</Link>
                {" · "}
                <Link
                  href={
                    entry.refCode
                      ? `${locale === "ru" ? "/ru" : ""}/docs/errors#e-${entry.refCode}`
                      : locale === "ru"
                        ? "/ru/docs/errors"
                        : "/docs/errors"
                  }
                >
                  {strings.fullReference}
                </Link>
                {tool.guidePath ? (
                  <>
                    {" · "}
                    <Link href={locale === "ru" ? `/ru${tool.guidePath}` : tool.guidePath}>{strings.setupGuide}</Link>
                  </>
                ) : null}
              </p>
            </div>

            <CtaBand locale={locale} strings={strings} />
          </div>
        </section>
      </main>
    </>
  );
}

// --- Static params helpers ---------------------------------------------------

export function toolStaticParams(): { tool: string }[] {
  return TOOL_ERROR_TOOLS.map((tool) => ({ tool: tool.slug }));
}

export function toolErrorStaticParams(): { tool: string; slug: string }[] {
  return TOOL_ERROR_TOOLS.flatMap((tool) =>
    toolErrors(tool.slug).map((entry) => ({ tool: tool.slug, slug: entry.slug })),
  );
}

export function resolveForPage(
  locale: ToolErrorLocale,
  toolSlug: string,
  slug: string,
): { tool: ToolInfo; entry: ResolvedToolError } | undefined {
  const tool = findTool(toolSlug);
  const entry = findToolError(toolSlug, slug);
  if (!tool || !entry) return undefined;
  return { tool, entry: resolveToolError(entry, locale, translations(locale)) };
}

export type { ToolErrorLocale, ToolSlug };
