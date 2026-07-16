import type { Metadata } from "next";
import Link from "next/link";
import { JsonLd } from "@/components/json-ld";
import { clusterLabels, learnArticles, learnPath, LEARN_HUB_PATH, type LearnCluster } from "@/lib/learn";
import { absoluteUrl, breadcrumbNode, createPageMetadata } from "@/lib/seo";

const HUB_TITLE = "Claude API Guides & Tutorials";
const HUB_DESCRIPTION = "Practical guides for buying, setting up and getting the most from the Claude API with apiToken.sale — pricing, integrations, payment, and model choice.";

export const metadata: Metadata = {
  ...createPageMetadata({ path: LEARN_HUB_PATH, title: HUB_TITLE, description: HUB_DESCRIPTION }),
  keywords: ["claude api guide", "claude api tutorial", "how to use claude api", "claude api help", "claude api docs"],
};

const clusterOrder: LearnCluster[] = ["buy", "free", "integrate", "compare", "explain"];

export default function LearnHubPage() {
  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      breadcrumbNode([
        { name: "Home", path: "/" },
        { name: "Docs", path: "/docs" },
        { name: "Guides", path: LEARN_HUB_PATH },
      ]),
      {
        "@type": "CollectionPage",
        "@id": `${absoluteUrl(LEARN_HUB_PATH)}#collection`,
        name: HUB_TITLE,
        description: HUB_DESCRIPTION,
        url: absoluteUrl(LEARN_HUB_PATH),
        inLanguage: "en",
        hasPart: learnArticles.map((article) => ({
          "@type": "Article",
          headline: article.title,
          url: absoluteUrl(learnPath(article.slug)),
        })),
      },
    ],
  };

  return (
    <><JsonLd data={structuredData} /><main className="learn-hub">
      <div className="page-hero">
        <div className="wrap">
          <Link className="auth-back" href="/docs">← Documentation</Link>
          <span className="eyebrow">Claude API guides</span>
          <h1>{HUB_TITLE}</h1>
          <p>{HUB_DESCRIPTION}</p>
        </div>
      </div>

      <section className="borderless">
        <div className="wrap">
          {clusterOrder.map((cluster) => {
            const items = learnArticles.filter((article) => article.cluster === cluster);
            if (items.length === 0) return null;
            return (
              <div className="learn-cluster" key={cluster}>
                <div className="learn-cluster-head">
                  <h2 className="docs-h3">{clusterLabels[cluster].label}</h2>
                  <p className="docs-para">{clusterLabels[cluster].blurb}</p>
                </div>
                <div className="learn-grid">
                  {items.map((article) => (
                    <Link className="learn-card" href={learnPath(article.slug)} key={article.slug}>
                      <strong>{article.h1}</strong>
                      <span>{article.description}</span>
                    </Link>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      </section>
    </main></>
  );
}
