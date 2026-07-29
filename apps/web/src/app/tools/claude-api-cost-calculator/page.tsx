import type { Metadata } from "next";
import Link from "next/link";
import { CostCalculator } from "@/components/cost-calculator";
import { JsonLd } from "@/components/json-ld";

const TITLE = "Claude API Cost Calculator — Free Price Estimator for Every Model";
const DESC =
  "Free Claude API cost calculator. Enter input, output and cache tokens to estimate the price of every Claude model (Opus 4.8, Sonnet 5, Haiku 4.5) at official Anthropic rates — and see your discounted price side by side.";
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
    q: "How is Claude API pricing calculated?",
    a: "Cost = (input tokens ÷ 1,000,000 × input rate) + (output tokens ÷ 1,000,000 × output rate). Prompt caching adds a cache-read charge at 0.1× the input rate and a 5-minute cache-write charge at 1.25× the input rate. The calculator does all of this for you across every model.",
  },
  {
    q: "How can I make the Claude API cheaper?",
    a: "apiToken.sale serves the exact same Anthropic Messages API on one key and one balance, billed at up to 70% below official prices. Point any Anthropic-compatible tool (Claude Code, Cursor, Cline, the SDK) at our endpoint — same models, same responses, lower price per call. New accounts start 60% below official rates for free.",
  },
  {
    q: "What is a token?",
    a: "A token is roughly ¾ of a word in English — about 4 characters. 1,000 tokens is ~750 words. Both the text you send (input) and the text Claude generates (output) are counted, and output tokens are billed at a higher rate than input.",
  },
  {
    q: "Are these prices exact?",
    a: "The rates are Anthropic's official published list prices, and the discounted column is your real apiToken.sale price. Your actual bill still depends on the exact number of tokens each request uses, so treat the totals as close estimates.",
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
            Pick a real task — write an article, build a game, a month of coding — and see what it costs to finish on any Claude
            model at official Anthropic rates, next to your price at up to 70% off. No sign-up, no card.
          </p>
        </div>
      </div>

      <section className="borderless">
        <div className="wrap">
          <CostCalculator />
        </div>
      </section>

      <section className="borderless calc-how">
        <div className="wrap">
          <div className="sec-head">
            <span className="eyebrow">How it works</span>
            <h2>Price a whole task, not a token</h2>
            <p>The calculator uses Anthropic&rsquo;s official published list rates. Nothing is sent anywhere — it all runs in your browser.</p>
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
              <p>See the official cost to finish that task next to your apiToken.sale cost for all five Claude models, cheapest highlighted.</p>
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
