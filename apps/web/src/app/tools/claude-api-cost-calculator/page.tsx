import type { Metadata } from "next";
import Link from "next/link";
import { CostCalculator } from "@/components/cost-calculator";
import { JsonLd } from "@/components/json-ld";
import { openaiModelsAt } from "@/lib/models";

export const dynamic = "force-dynamic";

const TITLE = "Claude API Cost Calculator — Free Price Estimator for Every Model";
const DESC =
  "Free Claude API cost calculator with a GPT model switch. Estimate every Claude or GPT model at official provider rates and compare your discounted price side by side.";
const URL = "https://apitoken.sale/tools/claude-api-cost-calculator";

export const metadata: Metadata = {
  title: TITLE,
  description: DESC,
  alternates: { canonical: "/tools/claude-api-cost-calculator" },
  openGraph: {
    title: TITLE,
    description: DESC,
    url: "/tools/claude-api-cost-calculator",
    type: "website",
    images: [{ url: "/og.png", width: 1200, height: 630 }],
  },
  twitter: { card: "summary_large_image", title: TITLE, description: DESC, images: ["/og.png"] },
};

const FAQ = [
  {
    q: "How much does the Claude API cost?",
    a: "Anthropic bills per token. As of 2026, list rates per 1M tokens are: Claude Opus 4.8 and 4.7 — $5 input / $25 output; Claude Sonnet 5 — $2 / $10 (introductory, through 2026-08-31); Claude Sonnet 4.6 — $3 / $15; Claude Haiku 4.5 — $1 / $5. Enter your token counts above to get an exact estimate for each model.",
  },
  {
    q: "How is API pricing calculated?",
    a: "Cost = (input tokens ÷ 1,000,000 × input rate) + (output tokens ÷ 1,000,000 × output rate), plus provider-specific cache charges. Use the switch to apply the correct Anthropic or OpenAI rates across every supported model.",
  },
  {
    q: "How can I make the Claude API cheaper?",
    a: "apiToken.sale serves the exact same Anthropic Messages API on one key and one balance, billed at a flat 50% below official prices on every request. Point any Anthropic-compatible tool (Claude Code, Cursor, Cline, the SDK) at our endpoint — same models, same responses, half the price per call.",
  },
  {
    q: "What is a token?",
    a: "A token is roughly ¾ of a word in English — about 4 characters. 1,000 tokens is ~750 words. Both the text you send (input) and the text the model generates (output) are counted, and output tokens are billed at a higher rate than input.",
  },
  {
    q: "Are these prices exact?",
    a: "The rates come from the pinned official Anthropic and OpenAI price catalogs, and the discounted column is your real apiToken.sale price. Your actual bill still depends on the exact token buckets and context size of each request, so treat whole-task totals as close estimates.",
  },
];

const jsonLd = {
  "@context": "https://schema.org",
  "@graph": [
    {
      "@type": "WebApplication",
      name: "Claude API Cost Calculator",
      url: URL,
      applicationCategory: "FinanceApplication",
      operatingSystem: "Any",
      description: DESC,
      offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
      provider: { "@type": "Organization", name: "apiToken.sale", url: "https://apitoken.sale" },
    },
    {
      "@type": "FAQPage",
      mainEntity: FAQ.map((f) => ({
        "@type": "Question",
        name: f.q,
        acceptedAnswer: { "@type": "Answer", text: f.a },
      })),
    },
    {
      "@type": "BreadcrumbList",
      itemListElement: [
        { "@type": "ListItem", position: 1, name: "apiToken.sale", item: "https://apitoken.sale" },
        { "@type": "ListItem", position: 2, name: "Tools", item: "https://apitoken.sale/tools/claude-api-cost-calculator" },
        { "@type": "ListItem", position: 3, name: "Claude API Cost Calculator" },
      ],
    },
  ],
};

export default function CostCalculatorPage() {
  return (
    <main>
      <JsonLd data={jsonLd} />

      <div className="page-hero">
        <div className="wrap">
          <span className="eyebrow">Free tool</span>
          <h1>Claude API Cost Calculator</h1>
          <p>
            Pick a real task — write an article, build a game, a month of coding — then switch between Claude and GPT models
            to compare official provider rates with your price at a flat 50% off. No sign-up, no card.
          </p>
        </div>
      </div>

      <section className="borderless">
        <div className="wrap">
          <CostCalculator openaiCatalog={openaiModelsAt()} />
        </div>
      </section>

      <section className="borderless calc-how">
        <div className="wrap">
          <div className="sec-head">
            <span className="eyebrow">How it works</span>
            <h2>Price a whole task, not a token</h2>
            <p>The calculator uses the pinned official Anthropic and OpenAI rate catalogs. Nothing is sent anywhere — it all runs in your browser.</p>
          </div>
          <div className="steps" data-reveal-stagger>
            <div className="step">
              <div className="n">01</div>
              <h3>Pick a real task</h3>
              <p>Choose a whole job — write an article, build a game, analyze 500 memecoins, a month of coding — not a single request.</p>
            </div>
            <div className="step">
              <div className="n">02</div>
              <h3>See the token budget</h3>
              <p>Each task carries a realistic total of input and output tokens to finish it end to end. Adjust them yourself if you like.</p>
            </div>
            <div className="step">
              <div className="n">03</div>
              <h3>Compare the price</h3>
              <p>Switch between Claude and GPT to see the official cost next to your apiToken.sale cost, with the cheapest model highlighted.</p>
            </div>
          </div>
        </div>
      </section>

      <section className="borderless">
        <div className="wrap">
          <div className="sec-head">
            <span className="eyebrow">FAQ</span>
            <h2>Claude API pricing, explained</h2>
          </div>
          <div className="faq">
            {FAQ.map((f) => (
              <details key={f.q}>
                <summary>
                  {f.q}
                  <span className="plus">+</span>
                </summary>
                <div className="ans">{f.a}</div>
              </details>
            ))}
          </div>
          <div className="hero-cta page-actions">
            <Link className="btn btn-primary" href="/register">
              Get an API key
            </Link>
            <Link className="btn btn-ghost" href="/models">
              See all model rates
            </Link>
          </div>
        </div>
      </section>
    </main>
  );
}
